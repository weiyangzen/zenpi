use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};
use zenpi::backend::{
    Backend, BackendError, Completion, CompletionRequest, OpenAiCompatibleBackend, ProviderEvent,
};

fn event(delta: Value, reason: Value) -> String {
    format!(
        "data: {}\n\n",
        json!({"id":"stream-1","model":"fixture","choices":[{"index":0,"delta":delta,"finish_reason":reason}]})
    )
}

fn request_body(stream: &mut TcpStream) -> Value {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut bytes = Vec::new();
    let end = loop {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            break bytes.len();
        }
        assert!(bytes.len() < 64 * 1024);
    };
    let headers = String::from_utf8_lossy(&bytes);
    let length = headers
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    bytes.resize(end + length, 0);
    stream.read_exact(&mut bytes[end..]).unwrap();
    let body: Value = serde_json::from_slice(&bytes[end..]).unwrap();
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    body
}

fn request(stream: &mut TcpStream) -> Value {
    let body = request_body(stream);
    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nConnection: close\r\n\r\n").unwrap();
    body
}

fn fixture(body: Vec<u8>) -> (OpenAiCompatibleBackend, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let backend = OpenAiCompatibleBackend::new(
        format!("http://{}/v1", listener.local_addr().unwrap()),
        None,
        "fixture",
    )
    .unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request(&mut stream);
        // Oversize/invalid fixtures may be rejected before the writer finishes.
        let _ = stream.write_all(&body);
    });
    (backend, handle)
}

fn run(body: String) -> (Result<Completion, BackendError>, Vec<ProviderEvent>) {
    let (backend, server) = fixture(body.into_bytes());
    let mut events = Vec::new();
    let result = backend.complete_with_control(
        CompletionRequest::new("turn-1", &[], None, &[]),
        &|| false,
        &mut |e| {
            events.push(e);
            Ok(())
        },
    );
    server.join().unwrap();
    (result, events)
}

#[test]
fn first_delta_reaches_sink_before_provider_sends_terminal_frame() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let backend = OpenAiCompatibleBackend::new(
        format!("http://{}", listener.local_addr().unwrap()),
        None,
        "fixture",
    )
    .unwrap();
    let (visible, observed) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request(&mut stream);
        stream
            .write_all(event(json!({"content":"first"}), Value::Null).as_bytes())
            .unwrap();
        stream.flush().unwrap();
        observed
            .recv_timeout(Duration::from_secs(2))
            .expect("host must receive first delta before terminal is sent");
        stream
            .write_all(event(json!({"content":" last"}), json!("stop")).as_bytes())
            .unwrap();
        stream.write_all(b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":4}}\n\ndata: [DONE]\n\n").unwrap();
    });
    let mut terminal = 0;
    let result = backend
        .complete_with_control(
            CompletionRequest::new("turn-1", &[], None, &[]),
            &|| false,
            &mut |e| {
                if matches!(e, ProviderEvent::TextDelta { ref delta } if delta == "first") {
                    visible.send(()).unwrap();
                }
                terminal += usize::from(matches!(e, ProviderEvent::Completed { .. }));
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(result.content, "first last");
    assert_eq!(result.usage.unwrap().total_tokens, 7);
    assert_eq!(result.annotations[0]["finish_reason"], "stop");
    assert_eq!(terminal, 1);
    server.join().unwrap();
}

#[test]
fn interleaved_calls_are_ordered_by_index_and_validated_as_a_whole() {
    let body = event(
        json!({"tool_calls":[{"index":1,"id":"b","function":{"name":"write_file","arguments":"{\"text\":"}}]}),
        Value::Null,
    ) + &event(
        json!({"tool_calls":[{"index":0,"id":"a","function":{"name":"read_file","arguments":"{\"path\":\"a\"}"}},{"index":1,"function":{"arguments":"\"内容\"}"}}]}),
        json!("tool_calls"),
    ) + "data: [DONE]\n\n";
    let (result, events) = run(body);
    let result = result.unwrap();
    assert_eq!(
        result
            .tool_calls
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(result.tool_calls[1].arguments["text"], "内容");
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, ProviderEvent::ToolCallDone { .. }))
            .count(),
        2
    );
}

#[test]
fn incomplete_or_malformed_stream_never_publishes_success_or_ready_tools() {
    let valid_tool = json!({"tool_calls":[{"index":0,"id":"a","function":{"name":"write_file","arguments":"{}"}}]});
    let partial_tool = json!({"tool_calls":[{"index":0,"id":"a","function":{"name":"write_file","arguments":"{"}}]});
    for body in [
        event(json!({"content":"partial"}), Value::Null),
        "data: [DONE]\n\n".into(),
        "data: {broken}\n\n".into(),
        event(partial_tool, json!("tool_calls")),
        event(valid_tool.clone(), json!("length")),
        event(valid_tool.clone(), json!("stop")),
        event(valid_tool.clone(), json!("tool_calls")) + "data: {broken}\n\n",
        event(json!({"content":"x"}), json!("stop")) + "data: partial",
        event(json!({"content":"x"}), json!("stop"))
            + &event(json!({"content":"late"}), Value::Null),
        event(
            json!({"tool_calls":[{"index":0,"id":"same","function":{"name":"write_file","arguments":"{}"}},{"index":1,"id":"same","function":{"name":"write_file","arguments":"{}"}}]}),
            json!("tool_calls"),
        ),
    ] {
        let (result, events) = run(body.clone());
        assert!(result.is_err(), "{body}");
        assert!(
            !events.iter().any(|e| matches!(
                e,
                ProviderEvent::Completed { .. } | ProviderEvent::ToolCallDone { .. }
            )),
            "{body}"
        );
    }
}

#[test]
fn utf8_split_across_transport_reads_and_multiline_sse_are_preserved() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let backend = OpenAiCompatibleBackend::new(
        format!("http://{}", listener.local_addr().unwrap()),
        None,
        "fixture",
    )
    .unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request(&mut stream);
        let body = event(json!({"content":"日本語 🌊"}), json!("stop"));
        let split = body.find('日').unwrap() + 1;
        stream.write_all(&body.as_bytes()[..split]).unwrap();
        stream.flush().unwrap();
        thread::sleep(Duration::from_millis(20));
        stream.write_all(&body.as_bytes()[split..]).unwrap();
        stream.write_all(b": keepalive\r\ndata: {\r\ndata: \"choices\":[], \"usage\":{\"prompt_tokens\":1}}\r\n\r\ndata: [DONE]\r\n\r\n").unwrap();
    });
    let completion = backend
        .complete(CompletionRequest::new("turn-1", &[], None, &[]))
        .unwrap();
    assert_eq!(completion.content, "日本語 🌊");
    assert_eq!(completion.usage.unwrap().input_tokens, 1);
    server.join().unwrap();
}

#[test]
fn reasoning_is_separate_from_answer_and_projects_to_a_separate_view_block() {
    let (result, events) = run(event(json!({"reasoning_content":"checking"}), Value::Null)
        + &event(json!({"content":"answer"}), json!("stop")));
    let result = result.unwrap();
    assert_eq!(result.content, "answer");
    assert_eq!(result.annotations[1]["text"], "checking");
    let reasoning = events
        .iter()
        .find(|e| matches!(e, ProviderEvent::ReasoningDelta { .. }))
        .unwrap();
    let answer = events
        .iter()
        .find(|e| matches!(e, ProviderEvent::TextDelta { .. }))
        .unwrap();
    let a =
        zenpi::view_model::ViewEvent::from_provider_event(1, None, Some("turn-1"), None, reasoning)
            .unwrap();
    let b =
        zenpi::view_model::ViewEvent::from_provider_event(2, None, Some("turn-1"), None, answer)
            .unwrap();
    assert_ne!(a.block_id, b.block_id);
}

#[test]
fn oversized_frame_is_rejected_before_completion() {
    let (result, events) = run(format!("data: {}\n\n", "x".repeat(256 * 1024)));
    assert!(result.unwrap_err().to_string().contains("frame too large"));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ProviderEvent::Completed { .. }))
    );
}

#[test]
#[cfg(unix)]
fn actual_headless_jsonl_streams_before_terminal_and_persists_finish_reason() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixStream;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let backend = OpenAiCompatibleBackend::new(
        format!("http://{}", listener.local_addr().unwrap()),
        None,
        "fixture",
    )
    .unwrap();
    let (visible, observed) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request(&mut stream);
        stream
            .write_all(event(json!({"content":"live-headless"}), Value::Null).as_bytes())
            .unwrap();
        stream.flush().unwrap();
        observed
            .recv_timeout(Duration::from_secs(3))
            .expect("JSONL must expose delta before terminal");
        stream
            .write_all(event(json!({"content":" final"}), json!("stop")).as_bytes())
            .unwrap();
        stream.write_all(b"data: [DONE]\n\n").unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let agent = zenpi::core::Agent::new(
        zenpi::session::SessionStore::open(&path).unwrap(),
        Box::new(backend),
    );
    let (mut input, reader) = UnixStream::pair().unwrap();
    let (writer, output) = UnixStream::pair().unwrap();
    output
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let host =
        thread::spawn(move || zenpi::headless::run_async_streams(agent, reader, writer).unwrap());
    writeln!(
        input,
        "{}",
        json!({"schema_version":2,"type":"prompt","id":"stream-prompt","text":"hello"})
    )
    .unwrap();
    let mut output = BufReader::new(output);
    let mut saw_delta = false;
    loop {
        let mut line = String::new();
        assert!(output.read_line(&mut line).unwrap() > 0);
        let record: Value = serde_json::from_str(&line).unwrap();
        if record["type"] == "event" && record["event"]["type"] == "text_delta" && !saw_delta {
            assert_eq!(record["event"]["delta"], "live-headless");
            visible.send(()).unwrap();
            saw_delta = true;
        }
        if record["type"] == "response" && record["id"] == "stream-prompt" {
            assert!(saw_delta);
            assert_eq!(record["success"], true, "{record}");
            break;
        }
    }
    writeln!(
        input,
        "{}",
        json!({"schema_version":2,"type":"shutdown","id":"shutdown"})
    )
    .unwrap();
    drop(input);
    host.join().unwrap();
    server.join().unwrap();
    let journal = std::fs::read_to_string(&path).unwrap();
    assert!(journal.contains("live-headless final"));
    assert!(journal.contains("chat_finish_reason"));
    // Reopening the journal is a real recovery parse; it issues no provider request.
    zenpi::session::SessionStore::open(&path).unwrap();
}

#[test]
fn cancellation_after_delta_closes_connection_without_retrying() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let backend = OpenAiCompatibleBackend::new(
        format!("http://{}", listener.local_addr().unwrap()),
        None,
        "fixture",
    )
    .unwrap()
    .with_max_retries(3)
    .unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request(&mut stream);
        stream
            .write_all(event(json!({"content":"visible"}), Value::Null).as_bytes())
            .unwrap();
        stream.flush().unwrap();
        let mut byte = [0];
        assert_eq!(
            stream.read(&mut byte).unwrap(),
            0,
            "cancel must close body connection"
        );
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    });
    let sink_cancel = cancelled.clone();
    let result = backend.complete_with_control(
        CompletionRequest::new("turn-1", &[], None, &[]),
        &|| cancelled.load(Ordering::SeqCst),
        &mut |e| {
            if matches!(e, ProviderEvent::TextDelta { .. }) {
                sink_cancel.store(true, Ordering::SeqCst);
            }
            Ok(())
        },
    );
    assert!(matches!(result, Err(BackendError::Cancelled)));
    server.join().unwrap();
}

#[test]
fn first_nonempty_reasoning_alias_streams_and_replays_under_its_original_field() {
    use zenpi::core::{Turn, TurnRole};
    for (delta, field, expected, provider) in [
        (
            json!({"reasoning_content":"", "reasoning":"fallback", "reasoning_text":"duplicate"}),
            "reasoning",
            "fallback",
            "openai",
        ),
        (
            json!({"reasoning_content":"", "reasoning":"", "reasoning_text":"third"}),
            "reasoning_text",
            "third",
            "openai",
        ),
        (
            json!({"reasoning_content":"first", "reasoning":"duplicate"}),
            "reasoning_content",
            "first",
            "openai",
        ),
        (
            json!({"reasoning_content":"", "reasoning":"mapped"}),
            "reasoning_content",
            "mapped",
            "opencode-go",
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let b = OpenAiCompatibleBackend::new(
            format!("http://{}", listener.local_addr().unwrap()),
            None,
            "fixture",
        )
        .unwrap()
        .with_model_registry(
            provider.into(),
            zenpi::providers::registry::ModelRegistry::with_overrides(&[serde_json::from_value(
                json!({
                    "provider": provider, "id": "fixture", "version": "loopback-v1",
                    "streaming": true
                }),
            )
            .unwrap()])
            .unwrap(),
        )
        .unwrap();
        let peer = thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            request(&mut first);
            write!(
                first,
                "{}{}",
                event(delta, Value::Null),
                event(json!({"content":"answer"}), json!("stop"))
            )
            .unwrap();
            drop(first);
            let (mut second, _) = listener.accept().unwrap();
            let payload = request(&mut second);
            assert_eq!(payload["messages"][0][field], expected);
            assert!(payload["messages"][0].get("reasoning_details").is_none());
            second
                .write_all(event(json!({"content":"next"}), json!("stop")).as_bytes())
                .unwrap();
        });
        let mut seen = Vec::new();
        let c = b
            .complete_with_control(
                CompletionRequest::new("one", &[], None, &[]),
                &|| false,
                &mut |e| {
                    seen.push(e);
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(
            seen.iter()
                .filter_map(|e| match e {
                    ProviderEvent::ReasoningDelta { delta } => Some(delta.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [expected]
        );
        let mut old = Turn::new("assistant", TurnRole::Assistant, c.content);
        old.metadata = Some(json!({"annotations":c.annotations}));
        assert!(zenpi::context::has_opaque_provider_state(&old));
        assert_eq!(
            b.complete(CompletionRequest::new("two", &[old], None, &[]))
                .unwrap()
                .content,
            "next"
        );
        peer.join().unwrap();
    }
}

#[test]
fn structured_details_merge_replay_after_tool_execution_and_survive_journal_restart() {
    use zenpi::{
        core::{Agent, TurnInputRequest},
        session::SessionStore,
        tools::{SideEffectPolicy, ToolContext, ToolRegistry},
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let expected = json!([
        {"type":"reasoning.text","text":"onetwo","signature":"sig-first","id":"text-id","index":0,"extra":{"retained":true}},
        {"type":"reasoning.summary","summary":"summary-ab","id":"summary-id"},
        {"type":"reasoning.encrypted","data":"opaque-one","id":"encrypted-1"},
        {"type":"reasoning.encrypted","data":"opaque-two","id":"encrypted-2"}
    ]);
    let expected_peer = expected.clone();
    let peer = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        request(&mut first);
        let a = event(
            json!({"reasoning_text":"visible summary","reasoning_details":[{"type":"reasoning.text","text":"one","signature":"sig-first","id":null,"extra":{"retained":true}}]}),
            Value::Null,
        );
        let b = event(
            json!({"reasoning_details":[{"type":"reasoning.text","text":"two","signature":"ignored-later","id":"text-id","index":0},{"type":"reasoning.summary","summary":"summary-a"},{"type":"reasoning.summary","summary":"b","id":"summary-id"},{"type":"reasoning.encrypted","data":"opaque-one","id":"encrypted-1"},{"type":"reasoning.encrypted","data":"opaque-two","id":"encrypted-2"}],"tool_calls":[{"index":0,"id":"call","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"input.txt\"}"}}]}),
            json!("tool_calls"),
        );
        write!(first, "{a}{b}").unwrap();
        drop(first);
        for i in 0..2 {
            let (mut next, _) = listener.accept().unwrap();
            let p = request(&mut next);
            let assistant = p["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m.get("tool_calls").is_some())
                .unwrap();
            assert_eq!(assistant["reasoning_details"], expected_peer);
            assert!(
                assistant.get("reasoning_text").is_none(),
                "structured details supersede raw replay"
            );
            assert!(p["messages"].as_array().unwrap().iter().any(|m| {
                m["role"] == "tool"
                    && m["content"]
                        .as_str()
                        .is_some_and(|s| s.contains("source-data"))
            }));
            let answer = if i == 0 {
                "first answer"
            } else {
                "after restart"
            };
            next.write_all(event(json!({"content":answer}), json!("stop")).as_bytes())
                .unwrap();
        }
    });
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("input.txt"), "source-data").unwrap();
    let path = dir.path().join("session.jsonl");
    let make = || {
        let b = OpenAiCompatibleBackend::new(&url, None, "fixture").unwrap();
        let mut a = Agent::new(
            SessionStore::open_in_workspace(&path, dir.path()).unwrap(),
            Box::new(b),
        );
        a.set_tools(
            ToolRegistry::with_all_builtins().unwrap(),
            ToolContext::new(dir.path()).unwrap(),
            SideEffectPolicy::all_builtins(),
        );
        a
    };
    let mut a = make();
    a.process(TurnInputRequest::new("read input")).unwrap();
    drop(a);
    let journal = std::fs::read_to_string(&path).unwrap();
    assert!(journal.contains("opaque-two"));
    let mut a = make();
    a.restore_model_selection().unwrap();
    a.process(TurnInputRequest::new("continue")).unwrap();
    drop(a);
    peer.join().unwrap();
    assert_eq!(expected.as_array().unwrap().len(), 4);
}

#[test]
fn malformed_or_oversized_reasoning_never_publishes_ready_tools() {
    let mut cases = vec![
        json!({"reasoning_content":false,"reasoning":"text"}),
        json!({"reasoning_details":{}}),
        json!({"reasoning_details":[{"type":"reasoning.encrypted","data":42}]}),
        json!({"reasoning_details":[{"type":"reasoning.text","text":"x","signature":false}]}),
        json!({"reasoning_details":[{"type":"unknown","data":"x"}]}),
        json!({"reasoning_details":[{"type":"reasoning.summary","summary":"x","index":null}]}),
        json!({"reasoning_text":"x".repeat(65537)}),
        json!({"reasoning_details":vec![json!({"type":"reasoning.encrypted","data":"x"});129]}),
    ];
    for delta in &mut cases {
        delta["tool_calls"] =
            json!([{"index":0,"id":"call","function":{"name":"read_file","arguments":"{}"}}]);
        let (r, events) = run(event(delta.clone(), json!("tool_calls")));
        assert!(r.is_err());
        assert!(!events.iter().any(|e| matches!(
            e,
            ProviderEvent::ToolCallDone { .. } | ProviderEvent::Completed { .. }
        )));
    }
    let (r, events) = run(event(
        json!({"reasoning_details":[{"type":"reasoning.text","text":"a".repeat(40000)}]}),
        Value::Null,
    ) + &event(
        json!({"reasoning_details":[{"type":"reasoning.text","text":"b".repeat(40000)}],"content":"answer"}),
        json!("stop"),
    ));
    assert!(r.is_err());
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ProviderEvent::Completed { .. }))
    );
}

#[test]
fn altered_duplicate_or_foreign_reasoning_history_fails_before_http() {
    use zenpi::core::{Turn, TurnRole};
    let (c, _) = run(event(
        json!({"reasoning_details":[{"type":"reasoning.encrypted","data":"opaque"}],"content":"answer"}),
        json!("stop"),
    ));
    let c = c.unwrap();
    let mut original = Turn::new("assistant", TurnRole::Assistant, c.content);
    original.metadata = Some(json!({"annotations":c.annotations}));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let b = OpenAiCompatibleBackend::new(
        format!("http://{}", listener.local_addr().unwrap()),
        None,
        "fixture",
    )
    .unwrap();
    let mut altered = original.clone();
    altered.metadata.as_mut().unwrap()["annotations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|a| a.get("chat_reasoning").is_some())
        .unwrap()["chat_reasoning"]["details"][0]["data"] = json!("altered");
    let mut duplicate = original.clone();
    let metadata = duplicate.metadata.as_mut().unwrap()["annotations"]
        .as_array_mut()
        .unwrap();
    let repeated = metadata
        .iter()
        .find(|a| a.get("chat_reasoning").is_some())
        .unwrap()
        .clone();
    metadata.push(repeated);
    let mut misplaced = original.clone();
    misplaced.role = TurnRole::User;
    for t in [altered, duplicate, misplaced] {
        assert!(
            b.complete(CompletionRequest::new("bad", &[t], None, &[]))
                .is_err()
        );
    }
    let other_provider = OpenAiCompatibleBackend::new(
        format!("http://{}", listener.local_addr().unwrap()),
        None,
        "fixture",
    )
    .unwrap()
    .with_model_registry(
        "other-provider".into(),
        zenpi::providers::registry::ModelRegistry::default(),
    )
    .unwrap();
    assert!(
        other_provider
            .complete(CompletionRequest::new(
                "provider",
                &[original.clone()],
                None,
                &[]
            ))
            .is_err()
    );
    let other_wire = OpenAiCompatibleBackend::new_with_wire_api(
        format!("http://{}", listener.local_addr().unwrap()),
        None,
        "fixture",
        zenpi::backend::OpenAiWireApi::Responses,
    )
    .unwrap();
    assert!(
        other_wire
            .complete(CompletionRequest::new(
                "wire",
                &[original.clone()],
                None,
                &[]
            ))
            .is_err()
    );
    assert!(
        b.validate_history_model(&[original.clone()], Some("another-model"))
            .is_err()
    );
    assert!(
        b.complete(CompletionRequest::new(
            "foreign",
            &[original],
            Some("another-model"),
            &[]
        ))
        .is_err()
    );
    assert!(matches!(listener.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock));
}

#[test]
fn json_fallback_records_and_replays_reasoning_with_created_before_delta() {
    use zenpi::core::{Turn, TurnRole};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let b = OpenAiCompatibleBackend::new(
        format!("http://{}", listener.local_addr().unwrap()),
        None,
        "fixture",
    )
    .unwrap();
    let peer = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        request_body(&mut first);
        let body=json!({"id":"json-id","model":"fixture","choices":[{"finish_reason":"stop","message":{"content":"answer","reasoning_content":"","reasoning_text":"json reasoning","reasoning_details":[{"type":"reasoning.encrypted","data":"opaque-json"}]}}]}).to_string();
        write!(first,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        drop(first);
        let (mut second, _) = listener.accept().unwrap();
        let p = request(&mut second);
        assert_eq!(
            p["messages"][0]["reasoning_details"][0]["data"],
            "opaque-json"
        );
        second
            .write_all(event(json!({"content":"next"}), json!("stop")).as_bytes())
            .unwrap();
    });
    let mut events = vec![];
    let c = b
        .complete_with_control(
            CompletionRequest::new("json", &[], None, &[]),
            &|| false,
            &mut |e| {
                events.push(e);
                Ok(())
            },
        )
        .unwrap();
    assert!(matches!(events[0], ProviderEvent::ResponseCreated { .. }));
    assert!(matches!(&events[1],ProviderEvent::ReasoningDelta{delta} if delta=="json reasoning"));
    let mut t = Turn::new("assistant", TurnRole::Assistant, c.content);
    t.metadata = Some(json!({"annotations":c.annotations}));
    b.complete(CompletionRequest::new("next", &[t], None, &[]))
        .unwrap();
    peer.join().unwrap();
}

#[test]
fn json_fallback_rejects_provider_supplied_internal_history_before_any_events() {
    for annotation in [
        json!({"type":"native_history", "wire":"chat_completions"}),
        json!({"type":"native_history", "wire":"anthropic_messages"}),
        json!({"type":"citation", "chat_reasoning":{}}),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let b = OpenAiCompatibleBackend::new(
            format!("http://{}", listener.local_addr().unwrap()),
            None,
            "fixture",
        )
        .unwrap();
        let peer = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            request_body(&mut s);
            let body = json!({"id":"json-id", "model":"fixture", "annotations":[annotation],
                "choices":[{"finish_reason":"stop", "message":{"content":"answer"}}]})
            .to_string();
            write!(s,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let mut seen = vec![];
        assert!(
            b.complete_with_control(
                CompletionRequest::new("json", &[], None, &[]),
                &|| false,
                &mut |e| {
                    seen.push(e);
                    Ok(())
                },
            )
            .is_err()
        );
        assert!(seen.is_empty());
        peer.join().unwrap();
    }
}

#[cfg(unix)]
#[test]
fn actual_headless_cancel_drops_partial_reasoning_history_and_next_request_is_clean() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixStream;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let backend = OpenAiCompatibleBackend::new(
        format!("http://{}", listener.local_addr().unwrap()),
        None,
        "fixture",
    )
    .unwrap()
    .with_max_retries(3)
    .unwrap();
    let peer = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        request(&mut first);
        first
            .write_all(
                event(
                    json!({"reasoning_content":"","reasoning_text":"live reasoning",
            "reasoning_details":[{"type":"reasoning.encrypted","data":"never-persist-partial"}]}),
                    Value::Null,
                )
                .as_bytes(),
            )
            .unwrap();
        first.flush().unwrap();
        first
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        assert_eq!(
            first.read(&mut [0u8]).unwrap(),
            0,
            "cancel must close original socket"
        );
        let (mut next, _) = listener.accept().unwrap();
        let payload = request(&mut next);
        assert!(!payload.to_string().contains("never-persist-partial"));
        assert!(!payload.to_string().contains("reasoning_details"));
        next.write_all(event(json!({"content":"fresh answer"}), json!("stop")).as_bytes())
            .unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let agent = zenpi::core::Agent::new(
        zenpi::session::SessionStore::open(&path).unwrap(),
        Box::new(backend),
    );
    let (mut input, reader) = UnixStream::pair().unwrap();
    let (writer, output) = UnixStream::pair().unwrap();
    output
        .set_read_timeout(Some(Duration::from_secs(6)))
        .unwrap();
    let host =
        thread::spawn(move || zenpi::headless::run_async_streams(agent, reader, writer).unwrap());
    writeln!(
        input,
        "{}",
        json!({"schema_version":2,"type":"prompt","id":"blocked","text":"first"})
    )
    .unwrap();
    let mut output = BufReader::new(output);
    let mut cancelled = false;
    let mut failed = false;
    while !failed {
        let mut line = String::new();
        assert!(output.read_line(&mut line).unwrap() > 0);
        let record: Value = serde_json::from_str(&line).unwrap();
        assert!(
            !line.contains("never-persist-partial"),
            "opaque details must not enter public deltas"
        );
        if record["type"] == "event" && record["event"]["type"] == "reasoning_delta" && !cancelled {
            assert_eq!(record["event"]["delta"], "live reasoning");
            writeln!(
                input,
                "{}",
                json!({"schema_version":2,"type":"cancel","id":"cancel","target_id":"blocked"})
            )
            .unwrap();
            cancelled = true;
        }
        if record["type"] == "response" && record["id"] == "blocked" {
            assert!(cancelled);
            assert_eq!(record["success"], false);
            assert_eq!(record["code"], "backend_cancelled");
            failed = true;
        }
    }
    writeln!(
        input,
        "{}",
        json!({"schema_version":2,"type":"prompt","id":"next","text":"next"})
    )
    .unwrap();
    loop {
        let mut line = String::new();
        assert!(output.read_line(&mut line).unwrap() > 0);
        let record: Value = serde_json::from_str(&line).unwrap();
        if record["type"] == "response" && record["id"] == "next" {
            assert_eq!(record["success"], true);
            break;
        }
    }
    writeln!(
        input,
        "{}",
        json!({"schema_version":2,"type":"shutdown","id":"shutdown"})
    )
    .unwrap();
    drop(input);
    host.join().unwrap();
    peer.join().unwrap();
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("never-persist-partial")
    );
    zenpi::session::SessionStore::open(&path).unwrap();
}

fn json_call(id: &str, arguments: &str) -> Value {
    json!({"id":id,"type":"function","function":{"name":"write_file","arguments":arguments}})
}

fn json_completion(calls: Vec<Value>, reason: Value) -> Value {
    json!({"id":"json-strict","model":"fixture","choices":[{"index":0,"finish_reason":reason,
        "message":{"content":"answer","tool_calls":calls}}]})
}

fn write_json_response(stream: &mut TcpStream, payload: &Value) {
    let body = payload.to_string();
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
}

fn invalid_json_completions() -> Vec<(&'static str, Value)> {
    let good = json_completion(vec![json_call("call", "{}")], json!("tool_calls"));
    let mut cases = vec![];
    let mut multiple = good.clone();
    let second = multiple["choices"][0].clone();
    multiple["choices"].as_array_mut().unwrap().push(second);
    cases.push(("multiple choices", multiple));
    cases.push((
        "duplicate call IDs",
        json_completion(
            vec![json_call("same", "{}"), json_call("same", "{}")],
            json!("tool_calls"),
        ),
    ));
    for (label, arguments) in [
        ("array arguments", "[]"),
        ("null arguments", "null"),
        ("scalar arguments", "1"),
        ("string arguments", "\"text\""),
        ("partial arguments", "{"),
    ] {
        cases.push((
            label,
            json_completion(vec![json_call("call", arguments)], json!("tool_calls")),
        ));
    }
    for (label, reason) in [
        ("length", json!("length")),
        ("filtered", json!("content_filter")),
        ("incomplete", json!("incomplete")),
        ("null terminal with tools", Value::Null),
        ("stop with tools", json!("stop")),
    ] {
        cases.push((
            label,
            json_completion(vec![json_call("call", "{}")], reason),
        ));
    }
    cases.push((
        "tools terminal without tools",
        json_completion(vec![], json!("tool_calls")),
    ));
    for (label, reason) in [
        ("null text terminal", Value::Null),
        ("length text terminal", json!("length")),
        ("incomplete text terminal", json!("incomplete")),
    ] {
        cases.push((label, json_completion(vec![], reason)));
    }
    let mut missing_text = json_completion(vec![], json!("stop"));
    missing_text["choices"][0]
        .as_object_mut()
        .unwrap()
        .remove("finish_reason");
    cases.push(("missing text terminal", missing_text));
    let mut missing = good.clone();
    missing["choices"][0]
        .as_object_mut()
        .unwrap()
        .remove("finish_reason");
    cases.push(("missing terminal with tools", missing));
    for (label, field, value) in [
        ("wrong choice index", "index", json!(1)),
        ("null choice index", "index", Value::Null),
    ] {
        let mut value_payload = good.clone();
        value_payload["choices"][0][field] = value;
        cases.push((label, value_payload));
    }
    for (label, value) in [
        ("object tool_calls", json!({})),
        (
            "too many calls",
            Value::Array(
                (0..129)
                    .map(|i| json_call(&format!("call-{i}"), "{}"))
                    .collect(),
            ),
        ),
    ] {
        let mut payload = good.clone();
        payload["choices"][0]["message"]["tool_calls"] = value;
        cases.push((label, payload));
    }
    for (label, field, value) in [
        ("empty ID", "id", json!("")),
        ("control ID", "id", json!("bad\n")),
        ("oversized ID", "id", json!("x".repeat(513))),
        ("nonfunction tool", "type", json!("custom")),
    ] {
        let mut payload = good.clone();
        payload["choices"][0]["message"]["tool_calls"][0][field] = value;
        cases.push((label, payload));
    }
    for (label, field, value) in [
        ("nonstring arguments", "arguments", json!({})),
        ("empty name", "name", json!("")),
    ] {
        let mut payload = good.clone();
        payload["choices"][0]["message"]["tool_calls"][0]["function"][field] = value;
        cases.push((label, payload));
    }
    let mut missing_args = good.clone();
    missing_args["choices"][0]["message"]["tool_calls"][0]["function"]
        .as_object_mut()
        .unwrap()
        .remove("arguments");
    cases.push(("missing arguments", missing_args));
    for (label, key, value) in [
        ("provider error", "error", json!({"message":"failed"})),
        ("incomplete status", "status", json!("incomplete")),
        (
            "incomplete details",
            "incomplete_details",
            json!({"reason":"max_output_tokens"}),
        ),
    ] {
        let mut payload = good.clone();
        payload[key] = value;
        cases.push((label, payload));
    }
    cases
}

#[test]
fn json_fallback_rejects_ambiguous_or_incomplete_tools_before_publication() {
    let mut failures = vec![];
    for (label, payload) in invalid_json_completions() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let backend = OpenAiCompatibleBackend::new(
            format!("http://{}", listener.local_addr().unwrap()),
            None,
            "fixture",
        )
        .unwrap();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            request_body(&mut stream);
            write_json_response(&mut stream, &payload);
        });
        let mut events = vec![];
        let result = backend.complete_with_control(
            CompletionRequest::new("json-strict", &[], None, &[]),
            &|| false,
            &mut |event| {
                events.push(event);
                Ok(())
            },
        );
        peer.join().unwrap();
        if !matches!(result, Err(BackendError::InvalidResponse(_))) || !events.is_empty() {
            failures.push(format!(
                "{label}: result={result:?}; published_events={}",
                events.len()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn json_fallback_cancel_between_created_and_ready_tool_publishes_no_success() {
    for cancel_on_reasoning in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let backend = OpenAiCompatibleBackend::new(
            format!("http://{}", listener.local_addr().unwrap()),
            None,
            "fixture",
        )
        .unwrap();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            request_body(&mut stream);
            let mut payload = json_completion(vec![json_call("call", "{}")], json!("tool_calls"));
            payload["choices"][0]["message"]["reasoning_text"] = json!("visible");
            write_json_response(&mut stream, &payload);
        });
        let cancelled = AtomicBool::new(false);
        let mut events = vec![];
        let result = backend.complete_with_control(
            CompletionRequest::new("cancel", &[], None, &[]),
            &|| cancelled.load(Ordering::SeqCst),
            &mut |event| {
                if (cancel_on_reasoning && matches!(event, ProviderEvent::ReasoningDelta { .. }))
                    || (!cancel_on_reasoning
                        && matches!(event, ProviderEvent::ResponseCreated { .. }))
                {
                    cancelled.store(true, Ordering::SeqCst);
                }
                events.push(event);
                Ok(())
            },
        );
        assert!(matches!(result, Err(BackendError::Cancelled)));
        assert!(!events.iter().any(|event| matches!(
            event,
            ProviderEvent::ToolCallDone { .. } | ProviderEvent::Completed { .. }
        )));
        peer.join().unwrap();
    }
}

struct JsonCli {
    child: std::process::Child,
    input: std::process::ChildStdin,
    lines: mpsc::Receiver<Value>,
}

impl JsonCli {
    fn start(root: &std::path::Path, endpoint: &str) -> Self {
        use std::{
            io::{BufRead, BufReader},
            process::{Command, Stdio},
        };
        let mut command = Command::new(env!("CARGO_BIN_EXE_zenpi"));
        for (key, _) in std::env::vars() {
            if key.starts_with("ZENPI_") || key.starts_with("OPENAI_") {
                command.env_remove(key);
            }
        }
        // Preserve real HOME and CODEX_HOME in the process and every child.
        let mut child = command
            .current_dir(root)
            .args(["--mode", "headless", "--session"])
            .arg(root.join("session.jsonl"))
            .env("ZENPI_HOME", root.join("user"))
            .env("ZENPI_BACKEND", "openai")
            .env("ZENPI_PROVIDER", "openai")
            .env("ZENPI_MODEL", "gpt-4.1")
            .env("ZENPI_WIRE_API", "chat")
            .env("ZENPI_BASE_URL", endpoint)
            .env("ZENPI_API_KEY", "json-fixture-key")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.unwrap();
                let value: Value = serde_json::from_str(&line).unwrap();
                if send.send(value).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            lines,
        }
    }
    fn send(&mut self, value: Value) {
        writeln!(self.input, "{value}").unwrap();
        self.input.flush().unwrap();
    }
    fn response(&self, id: &str) -> (Value, Vec<Value>) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut events = vec![];
        loop {
            let value = self
                .lines
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .unwrap();
            if value.get("success").is_some() && value["id"] == id {
                return (value, events);
            }
            events.push(value);
        }
    }
    fn shutdown(&mut self) {
        let id = format!("shutdown-{}", self.child.id());
        self.send(json!({"type":"shutdown","id":id}));
        assert_eq!(self.response(&id).0["success"], true);
        assert!(self.child.wait().unwrap().success());
    }
}
impl Drop for JsonCli {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn production_json_rejects_invalid_tools_without_execution_or_retry() {
    for (label, mut payload) in invalid_json_completions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("input.txt"), "must-not-execute-json-tool").unwrap();
        payload["model"] = json!("gpt-4.1");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            request_body(&mut stream);
            write_json_response(&mut stream, &payload);
            drop(stream);
            listener
        });
        let mut cli = JsonCli::start(dir.path(), &endpoint);
        cli.send(
            json!({"schema_version":2,"type":"prompt","id":"invalid","text":"reject invalid tool"}),
        );
        let (response, events) = cli.response("invalid");
        assert_eq!(response["success"], false, "{label}: {response}");
        assert_eq!(
            response["code"], "backend_invalid_response",
            "{label}: {response}"
        );
        assert!(
            !events
                .iter()
                .any(|event| event.to_string().contains("tool_call_done")),
            "{label}"
        );
        cli.shutdown();
        let listener = peer.join().unwrap();
        listener.set_nonblocking(true).unwrap();
        assert!(
            matches!(listener.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock),
            "{label}: unexpected retry or tool continuation"
        );
        let journal = std::fs::read_to_string(dir.path().join("session.jsonl")).unwrap();
        assert!(
            !journal.contains("\"role\":\"tool\""),
            "{label}: tool persisted"
        );
        assert!(!journal.contains("must-not-execute-json-tool"), "{label}");
        zenpi::session::SessionStore::open(dir.path().join("session.jsonl")).unwrap();
    }
}

#[test]
fn production_json_tool_continuation_and_restart_preserve_reasoning() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("input.txt"), "json-tool-source-data").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let peer = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        request_body(&mut first);
        let mut call = json_call("json-read", "{\"path\":\"input.txt\"}");
        call["function"]["name"] = json!("read_file");
        let mut payload = json_completion(vec![call], json!("tool_calls"));
        payload["model"] = json!("gpt-4.1");
        payload["choices"][0]["message"]["content"] = Value::Null;
        payload["choices"][0]["message"]["reasoning_content"] = json!("");
        payload["choices"][0]["message"]["reasoning_text"] = json!("json reasoning");
        payload["choices"][0]["message"]["reasoning_details"] =
            json!([{"type":"reasoning.encrypted","data":"json-opaque-restart"}]);
        write_json_response(&mut first, &payload);
        drop(first);
        for _ in 0..2 {
            let (mut next, _) = listener.accept().unwrap();
            let request = request_body(&mut next);
            let messages = request["messages"].as_array().unwrap();
            assert!(messages.iter().any(|message| {
                message["role"] == "tool"
                    && message["tool_call_id"] == "json-read"
                    && message["content"]
                        .as_str()
                        .is_some_and(|s| s.contains("json-tool-source-data"))
            }));
            let assistant = messages
                .iter()
                .find(|message| message.get("tool_calls").is_some())
                .unwrap();
            assert_eq!(
                assistant["reasoning_details"][0]["data"],
                "json-opaque-restart"
            );
            let mut answer = json_completion(vec![], json!("stop"));
            answer["model"] = json!("gpt-4.1");
            // Ordinary compatible single-choice JSON may omit its index.
            answer["choices"][0]
                .as_object_mut()
                .unwrap()
                .remove("index");
            write_json_response(&mut next, &answer);
        }
    });
    for id in ["first", "restart"] {
        let mut cli = JsonCli::start(dir.path(), &endpoint);
        cli.send(json!({"schema_version":2,"type":"prompt","id":id,"text":"read and continue"}));
        assert_eq!(cli.response(id).0["success"], true);
        cli.shutdown();
    }
    peer.join().unwrap();
    let journal = std::fs::read_to_string(dir.path().join("session.jsonl")).unwrap();
    assert!(journal.contains("json-opaque-restart"));
    assert!(journal.contains("chat_finish_reason"));
    assert!(!journal.contains("json-fixture-key"));
}

#[test]
fn production_json_cancel_closes_partial_body_and_next_request_is_clean() {
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (sent, ready) = mpsc::channel();
    let peer = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        request_body(&mut first);
        let mut payload =
            json_completion(vec![json_call("never-ready", "{}")], json!("tool_calls"));
        payload["choices"][0]["message"]["reasoning_details"] =
            json!([{"type":"reasoning.encrypted","data":"never-persist-json-partial"}]);
        let body = payload.to_string();
        write!(first,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),&body[..body.len()-1]).unwrap();
        first.flush().unwrap();
        sent.send(()).unwrap();
        first
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        assert_eq!(
            first.read(&mut [0u8]).unwrap(),
            0,
            "cancel must close JSON body socket"
        );
        let (mut next, _) = listener.accept().unwrap();
        let request = request_body(&mut next);
        assert!(!request.to_string().contains("never-persist-json-partial"));
        assert!(
            !request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["role"] == "tool")
        );
        let mut answer = json_completion(vec![], json!("stop"));
        answer["model"] = json!("gpt-4.1");
        write_json_response(&mut next, &answer);
    });
    let mut cli = JsonCli::start(dir.path(), &endpoint);
    cli.send(json!({"schema_version":2,"type":"prompt","id":"blocked-json","text":"first"}));
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    cli.send(
        json!({"schema_version":2,"type":"cancel","id":"cancel-json","target_id":"blocked-json"}),
    );
    let (response, events) = cli.response("blocked-json");
    assert_eq!(response["success"], false);
    assert_eq!(response["code"], "backend_cancelled");
    assert!(
        !events
            .iter()
            .any(|value| value.to_string().contains("tool_call_done"))
    );
    cli.send(json!({"schema_version":2,"type":"prompt","id":"next-json","text":"next"}));
    assert_eq!(cli.response("next-json").0["success"], true);
    cli.shutdown();
    peer.join().unwrap();
    let journal = std::fs::read_to_string(dir.path().join("session.jsonl")).unwrap();
    assert!(!journal.contains("never-persist-json-partial"));
    assert!(!journal.contains("never-ready"));
}

#[test]
fn sse_cancel_on_text_done_suppresses_ready_tools_and_completion() {
    let body = event(
        json!({"content":"visible","tool_calls":[{"index":0,"id":"one","function":{"name":"read_file","arguments":"{}"}}]}),
        json!("tool_calls"),
    ) + "data: [DONE]\n\n";
    let (backend, peer) = fixture(body.into_bytes());
    let cancelled = AtomicBool::new(false);
    let mut events = Vec::new();
    let result = backend.complete_with_control(
        CompletionRequest::new("cancel-terminal", &[], None, &[]),
        &|| cancelled.load(Ordering::SeqCst),
        &mut |event| {
            if matches!(event, ProviderEvent::TextDone { .. }) {
                cancelled.store(true, Ordering::SeqCst);
            }
            events.push(event);
            Ok(())
        },
    );
    peer.join().unwrap();
    eprintln!("cancel-on-text-done result={result:?} events={events:?}");
    assert!(matches!(result, Err(BackendError::Cancelled)));
    assert!(!events.iter().any(|e| matches!(
        e,
        ProviderEvent::ToolCallDone { .. } | ProviderEvent::Completed { .. }
    )));
}

#[test]
fn sse_cancel_on_first_ready_tool_suppresses_remaining_ready_tools_and_completion() {
    let body = event(
        json!({"tool_calls":[{"index":0,"id":"one","function":{"name":"read_file","arguments":"{}"}},{"index":1,"id":"two","function":{"name":"read_file","arguments":"{}"}}]}),
        json!("tool_calls"),
    ) + "data: [DONE]\n\n";
    let (backend, peer) = fixture(body.into_bytes());
    let cancelled = AtomicBool::new(false);
    let mut events = Vec::new();
    let result = backend.complete_with_control(
        CompletionRequest::new("cancel-terminal", &[], None, &[]),
        &|| cancelled.load(Ordering::SeqCst),
        &mut |event| {
            if matches!(event, ProviderEvent::ToolCallDone { .. }) {
                cancelled.store(true, Ordering::SeqCst);
            }
            events.push(event);
            Ok(())
        },
    );
    peer.join().unwrap();
    eprintln!("cancel-on-ready result={result:?} events={events:?}");
    assert!(matches!(result, Err(BackendError::Cancelled)));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, ProviderEvent::ToolCallDone { .. }))
            .count(),
        1
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ProviderEvent::Completed { .. }))
    );
}
