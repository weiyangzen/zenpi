use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use tempfile::tempdir;
use zenpi::{
    backend::{
        AttachmentKind, Backend, BackendError, CompletionRequest, InputAttachment,
        OpenAiCompatibleBackend, OpenAiWireApi, ProviderEvent, RequestAttachment,
    },
    core::{Turn, TurnRole},
    providers::registry::ModelRegistry,
    tools::{ToolDefinition, ToolSideEffect},
};
const MODEL: &str = "claude-sonnet-4-6";
type Requests = Arc<Mutex<Vec<(String, Value)>>>;
fn request(stream: &mut TcpStream) -> (String, Value) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    let mut bytes = vec![];
    let mut chunk = [0; 4096];
    let end = loop {
        let n = stream.read(&mut chunk).unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&chunk[..n]);
        assert!(bytes.len() < 2 * 1024 * 1024);
        if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
    let len = headers
        .lines()
        .find_map(|l| {
            l.split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .map(|(_, v)| v.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    while bytes.len() < end + len {
        let n = stream.read(&mut chunk).unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&chunk[..n]);
    }
    (
        headers,
        serde_json::from_slice(&bytes[end..end + len]).unwrap(),
    )
}
fn serve(replies: Vec<(String, String)>) -> (String, Requests, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(vec![]));
    let out = requests.clone();
    let h = thread::spawn(move || {
        for (mime, body) in replies {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "missing request");
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            out.lock().unwrap().push(request(&mut stream));
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    (url, requests, h)
}
fn message(blocks: Value, stop: &str) -> Value {
    json!({"type":"message","id":"msg_fixture","role":"assistant","model":MODEL,"content":blocks,"stop_reason":stop,"usage":{"input_tokens":7,"output_tokens":4,"cache_read_input_tokens":3,"cache_creation_input_tokens":2}})
}
fn events(blocks: Vec<Value>, stop: &str) -> Vec<Value> {
    let mut start = message(json!([]), stop);
    start["stop_reason"] = Value::Null;
    start["usage"]["output_tokens"] = json!(0);
    let mut ev = vec![json!({"type":"message_start","message":start})];
    for (i, block) in blocks.into_iter().enumerate() {
        ev.push(json!({"type":"content_block_start","index":i,"content_block":block}));
        ev.push(json!({"type":"content_block_stop","index":i}));
    }
    ev.push(
        json!({"type":"message_delta","delta":{"stop_reason":stop},"usage":{"output_tokens":4}}),
    );
    ev.push(json!({"type":"message_stop"}));
    ev
}
fn sse(events: &[Value]) -> String {
    events
        .iter()
        .map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap()))
        .collect()
}
fn backend(url: String) -> OpenAiCompatibleBackend {
    OpenAiCompatibleBackend::new_with_wire_api(
        url,
        Some("native-fixture-key".into()),
        MODEL,
        OpenAiWireApi::AnthropicMessages,
    )
    .unwrap()
    .with_model_registry("anthropic".into(), ModelRegistry::default())
    .unwrap()
}
fn tool() -> ToolDefinition {
    ToolDefinition {
        name: "read_file".into(),
        description: "Read".into(),
        input_schema: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
        side_effect: ToolSideEffect::ReadOnly,
    }
}
fn text() -> Value {
    json!({"type":"text","text":"native answer"})
}
fn thinking() -> Value {
    json!({"type":"thinking","thinking":"visible summary","signature":"opaque-signature-fixture"})
}
fn tool_block() -> Value {
    json!({"type":"tool_use","id":"toolu_fixture","name":"read_file","input":{"path":"hello.txt"}})
}

#[test]
fn native_headers_system_tool_schema_effort_and_usage_are_exact() {
    let (url, requests, h) = serve(vec![(
        "application/json".into(),
        message(json!([text()]), "end_turn").to_string(),
    )]);
    let mut b = backend(url);
    b.validate_reasoning_effort(None, Some("max")).unwrap();
    b.commit_reasoning_effort(Some("max".into()));
    let turns = [
        Turn::new("s", TurnRole::System, "system turn"),
        Turn::new("u", TurnRole::User, "hello"),
    ];
    let tools = [tool()];
    let c = b
        .complete(
            CompletionRequest::new("u", &turns, None, &tools)
                .with_instructions(Some("instructions"))
                .with_max_output_tokens(8192),
        )
        .unwrap();
    h.join().unwrap();
    assert_eq!(c.content, "native answer");
    assert_eq!(c.usage.unwrap().input_tokens, 12);
    assert_eq!(c.usage.unwrap().total_tokens, 16);
    let r = requests.lock().unwrap();
    assert!(r[0].0.starts_with("POST /v1/messages "));
    assert!(
        r[0].0
            .to_ascii_lowercase()
            .contains("anthropic-version: 2023-06-01")
    );
    assert!(
        r[0].0
            .to_ascii_lowercase()
            .contains("authorization: bearer native-fixture-key")
    );
    let p = &r[0].1;
    assert_eq!(
        p["system"],
        json!([{"type":"text","text":"instructions"},{"type":"text","text":"system turn"}])
    );
    assert_eq!(p["tools"][0]["input_schema"], tools[0].input_schema);
    assert_eq!(p["output_config"]["effort"], "max");
    assert_eq!(p["thinking"]["type"], "adaptive");
    assert_eq!(p["max_tokens"], 8192);
    assert!(p.get("stream_options").is_none());
    assert_eq!(b.name(), "anthropic-messages");
}

#[test]
fn streamed_text_tool_arguments_and_opaque_thinking_are_folded_separately() {
    let mut ev = events(vec![], "tool_use");
    ev.truncate(1);
    ev.extend([
 json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
 json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"summary"}}),
 json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"opaque"}}),
 json!({"type":"content_block_stop","index":0}),
 json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":"initial"}}),
 json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":" 日本語"}}),
 json!({"type":"content_block_stop","index":1}),
 json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_a","name":"read_file","input":{}}}),
 json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}),
 json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\"hello.txt\"}"}}),
 json!({"type":"content_block_stop","index":2}),
 json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":4}}),json!({"type":"message_stop"})]);
    let (url, _, h) = serve(vec![("text/event-stream".into(), sse(&ev))]);
    let b = backend(url);
    let turns = [Turn::new("u", TurnRole::User, "hello")];
    let mut seen = vec![];
    let c = b
        .complete_with_control(
            CompletionRequest::new("u", &turns, None, &[tool()]),
            &|| false,
            &mut |e| {
                seen.push(e);
                Ok(())
            },
        )
        .unwrap();
    h.join().unwrap();
    assert_eq!(c.content, "initial 日本語");
    assert_eq!(c.tool_calls[0].arguments, json!({"path":"hello.txt"}));
    assert_eq!(c.annotations[0]["blocks"][0]["signature"], "opaque");
    assert!(
        seen.iter()
            .any(|e| matches!(e,ProviderEvent::ReasoningDelta{delta}if delta=="summary"))
    );
    assert!(matches!(seen.last(), Some(ProviderEvent::Completed { .. })));
}

#[test]
fn malformed_terminal_and_signature_never_publish_executable_tools() {
    let mut cases = vec![];
    for reason in ["max_tokens", "pause_turn", "unknown", "end_turn"] {
        cases.push(events(vec![tool_block()], reason));
    }
    let mut missing = events(vec![tool_block()], "tool_use");
    missing.pop();
    cases.push(missing);
    cases.push(events(
        vec![
            json!({"type":"thinking","thinking":"summary","signature":""}),
            tool_block(),
        ],
        "tool_use",
    ));
    let mut wrong = events(vec![tool_block()], "tool_use");
    wrong[2]["index"] = json!(7);
    cases.push(wrong);
    let mut duplicate = events(vec![tool_block(), tool_block()], "tool_use");
    cases.push(duplicate.clone());
    duplicate.push(json!({"type":"message_stop"}));
    cases.push(duplicate);
    for ev in cases {
        let (url, _, h) = serve(vec![("text/event-stream".into(), sse(&ev))]);
        let b = backend(url);
        let turns = [Turn::new("u", TurnRole::User, "go")];
        let mut seen = vec![];
        assert!(
            b.complete_with_control(
                CompletionRequest::new("u", &turns, None, &[tool()]),
                &|| false,
                &mut |e| {
                    seen.push(e);
                    Ok(())
                }
            )
            .is_err()
        );
        h.join().unwrap();
        assert!(!seen.iter().any(|e| matches!(
            e,
            ProviderEvent::ToolCallDone { .. } | ProviderEvent::Completed { .. }
        )));
    }
}

#[test]
fn malformed_json_frames_and_bad_tool_json_fail_closed() {
    let good = events(vec![tool_block()], "tool_use");
    let mut bad = good.clone();
    bad.insert(2,json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{"}}));
    for body in [
        "event: message_start\ndata: {broken}\n\n".into(),
        sse(&good).replace("event: message_stop", "event: message_delta"),
        sse(&good).trim_end().into(),
        format!("data: {}\n\n", "x".repeat(256 * 1024 + 1)),
        sse(&bad),
    ] {
        let (url, _, h) = serve(vec![("text/event-stream".into(), body)]);
        let turns = [Turn::new("u", TurnRole::User, "go")];
        assert!(
            backend(url)
                .complete(CompletionRequest::new("u", &turns, None, &[tool()]))
                .is_err()
        );
        h.join().unwrap();
    }
}

#[test]
fn unsupported_request_fields_and_history_fail_before_network() {
    let b = backend("http://127.0.0.1:1".into());
    let turns = [Turn::new("u", TurnRole::User, "go")];
    assert!(matches!(
        b.complete(
            CompletionRequest::new("u", &turns, None, &[])
                .with_response_format(Some(&json!({"type":"json_object"})))
        ),
        Err(BackendError::Configuration(_))
    ));
    assert!(b.validate_reasoning_effort(None, Some("xhigh")).is_err());
    assert!(
        b.validate_reasoning_effort(Some("unknown"), Some("high"))
            .is_err()
    );
    let a = RequestAttachment {
        turn_id: "u".into(),
        input: InputAttachment {
            kind: AttachmentKind::File,
            mime_type: "application/pdf".into(),
            path: None,
            url: Some("https://example.com/a.pdf".into()),
            file_id: None,
        },
        filename: None,
        data: None,
        size_bytes: None,
        sha256: None,
    };
    assert!(matches!(
        b.complete(CompletionRequest::new("u", &turns, None, &[]).with_attachments(&[a])),
        Err(BackendError::Configuration(_))
    ));
    let mut orphan = Turn::new("t", TurnRole::Tool, "x");
    orphan.metadata = Some(json!({"tool_call_id":"missing"}));
    assert!(matches!(
        b.complete(CompletionRequest::new(
            "u",
            &[turns[0].clone(), orphan],
            None,
            &[]
        )),
        Err(BackendError::Configuration(_))
    ));
}

#[test]
fn native_history_integrity_and_cross_wire_are_authoritative() {
    let (url, _, h) = serve(vec![(
        "application/json".into(),
        message(json!([thinking(), text(), tool_block()]), "tool_use").to_string(),
    )]);
    let user = Turn::new("u", TurnRole::User, "go");
    let c = backend(url)
        .complete(CompletionRequest::new(
            "u",
            std::slice::from_ref(&user),
            None,
            &[tool()],
        ))
        .unwrap();
    h.join().unwrap();
    let mut a = Turn::new("a", TurnRole::Assistant, c.content);
    a.metadata = Some(json!({"annotations":c.annotations,"tool_calls":c.tool_calls}));
    let mut result = Turn::new("t", TurnRole::Tool, "result");
    result.metadata = Some(json!({"tool_call_id":"toolu_fixture"}));
    let b = backend("http://127.0.0.1:1".into());
    let mut changed = a.clone();
    changed.content.push('!');
    assert!(matches!(
        b.complete(CompletionRequest::new(
            "u",
            &[user.clone(), changed, result.clone()],
            None,
            &[tool()]
        )),
        Err(BackendError::Configuration(_))
    ));
    let chat = OpenAiCompatibleBackend::new("http://127.0.0.1:1", None, "gpt-4.1").unwrap();
    assert!(matches!(
        chat.complete(CompletionRequest::new(
            "u",
            &[user.clone(), a.clone(), result.clone()],
            None,
            &[tool()]
        )),
        Err(BackendError::Configuration(_))
    ));
    assert!(matches!(
        b.complete(CompletionRequest::new(
            "u",
            &[user, a, result],
            Some("other"),
            &[tool()]
        )),
        Err(BackendError::Configuration(_))
    ));
}

#[test]
fn image_input_uses_native_base64_part() {
    let (url, requests, h) = serve(vec![(
        "application/json".into(),
        message(json!([text()]), "end_turn").to_string(),
    )]);
    let a = RequestAttachment {
        turn_id: "u".into(),
        input: InputAttachment {
            kind: AttachmentKind::Image,
            mime_type: "image/png".into(),
            path: Some("a.png".into()),
            url: None,
            file_id: None,
        },
        filename: None,
        data: Some(vec![1, 2, 3]),
        size_bytes: Some(3),
        sha256: None,
    };
    backend(url)
        .complete(
            CompletionRequest::new("u", &[Turn::new("u", TurnRole::User, "image")], None, &[])
                .with_attachments(&[a]),
        )
        .unwrap();
    h.join().unwrap();
    assert_eq!(
        requests.lock().unwrap()[0].1["messages"][0]["content"][1],
        json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"AQID"}})
    );
}

#[test]
fn native_config_does_not_borrow_openai_credentials_or_identity() {
    use zenpi::config::{AuthFile, ConfigFile, ConfigOverrides, resolve};
    let config: ConfigFile = toml::from_str(
        "backend='anthropic'\nmodel='claude-sonnet-4-6'\nbase_url='https://api.anthropic.com'\n",
    )
    .unwrap();
    let env = BTreeMap::from([
        ("OPENAI_API_KEY".into(), "wrong".into()),
        ("OPENAI_MODEL".into(), "wrong-model".into()),
        ("OPENAI_BASE_URL".into(), "https://wrong.example".into()),
        ("ANTHROPIC_API_KEY".into(), "right".into()),
    ]);
    let r = resolve(
        &ConfigOverrides::default(),
        &config,
        &AuthFile::default(),
        &env,
    )
    .unwrap();
    assert_eq!(r.api_key.as_deref(), Some("right"));
    assert_eq!(r.model.as_deref(), Some(MODEL));
    assert_eq!(r.base_url.as_deref(), Some("https://api.anthropic.com"));
}

#[test]
fn cancellation_during_slow_body_drops_one_connection_without_retry() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request(&mut stream);
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 99999\r\nConnection: close\r\n\r\n").unwrap();
        stream.flush().unwrap();
        tx.send(()).unwrap();
        let mut b = [0u8; 1];
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        assert_eq!(stream.read(&mut b).unwrap(), 0);
    });
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let worker = thread::spawn(move || {
        backend(url)
            .with_max_retries(2)
            .unwrap()
            .complete_with_control(
                CompletionRequest::new("u", &[Turn::new("u", TurnRole::User, "go")], None, &[]),
                &|| flag.load(Ordering::SeqCst),
                &mut |_| Ok(()),
            )
    });
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let start = Instant::now();
    cancelled.store(true, Ordering::SeqCst);
    assert!(matches!(
        worker.join().unwrap(),
        Err(BackendError::Cancelled)
    ));
    assert!(start.elapsed() < Duration::from_millis(700));
    h.join().unwrap();
}

struct Cli {
    child: Child,
    input: ChildStdin,
    lines: mpsc::Receiver<Value>,
}
impl Cli {
    fn start(root: &Path) -> Self {
        let mut c = Command::new(env!("CARGO_BIN_EXE_zenpi"));
        c.current_dir(root)
            .args(["--mode", "headless", "--session"])
            .arg(root.join("session.jsonl"))
            .env("ZENPI_HOME", root.join("user"))
            .env("ZENPI_API_KEY", "native-fixture-key")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        for key in [
            "ZENPI_PROFILE",
            "ZENPI_BACKEND",
            "ZENPI_PROVIDER",
            "ZENPI_BASE_URL",
            "ZENPI_MODEL",
            "ZENPI_WIRE_API",
            "ZENPI_MODEL_REASONING_EFFORT",
            "ZENPI_MODEL_VERBOSITY",
            "OPENAI_MODEL",
            "OPENAI_BASE_URL",
            "OPENAI_API_KEY",
            "ANTHROPIC_MODEL",
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_API_KEY",
        ] {
            c.env_remove(key);
        }
        let mut child = c.spawn().unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if let Ok(line) = line {
                    if let Ok(v) = serde_json::from_str(&line)
                        && tx.send(v).is_err()
                    {
                        break;
                    }
                } else {
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
    fn send(&mut self, v: Value) -> Value {
        writeln!(self.input, "{v}").unwrap();
        self.input.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let r = self
                .lines
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            if r.get("success").is_some() && r["id"] == v["id"] {
                return r;
            }
        }
    }
}
impl Drop for Cli {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[test]
fn production_tool_continuation_and_restart_keep_signed_ordered_blocks() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("user")).unwrap();
    std::fs::write(root.join("hello.txt"), "TOOL_FILE_BODY").unwrap();
    let blocks = json!([thinking(),{"type":"redacted_thinking","data":"opaque-redacted"},text(),tool_block()]);
    let (url, requests, h) = serve(vec![
        (
            "text/event-stream".into(),
            sse(&events(blocks.as_array().unwrap().clone(), "tool_use")),
        ),
        (
            "application/json".into(),
            message(json!([text()]), "end_turn").to_string(),
        ),
        (
            "application/json".into(),
            message(json!([text()]), "end_turn").to_string(),
        ),
    ]);
    std::fs::write(root.join("user/config.toml"),format!("backend='anthropic'\nbase_url='{url}'\nmodel='{MODEL}'\nmodel_reasoning_effort='high'\n")).unwrap();
    {
        let mut cli = Cli::start(root);
        let r = cli
            .send(json!({"type":"prompt","id":"one","text":"read hello","mode":"start_or_steer"}));
        assert_eq!(r["success"], true, "{r}");
    }
    {
        let mut cli = Cli::start(root);
        let r =
            cli.send(json!({"type":"prompt","id":"two","text":"continue","mode":"start_or_steer"}));
        assert_eq!(r["success"], true, "{r}");
    }
    h.join().unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    for index in [1, 2] {
        let m = requests[index].1["messages"].as_array().unwrap();
        let a = m.iter().find(|m| m["role"] == "assistant").unwrap();
        assert_eq!(a["content"], blocks);
        assert!(m.iter().any(
            |m| m["content"].as_array().is_some_and(|b| b.iter().any(|b| {
                b["type"] == "tool_result"
                    && b["content"]
                        .as_str()
                        .is_some_and(|s| s.contains("TOOL_FILE_BODY"))
            }))
        ));
    }
    let journal = std::fs::read_to_string(root.join("session.jsonl")).unwrap();
    assert!(journal.contains("opaque-signature-fixture"));
    assert!(journal.contains("opaque-redacted"));
    assert!(!journal.contains("native-fixture-key"));
}

#[test]
fn first_text_is_observable_before_terminal_and_split_utf8_crlf_is_valid() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let mut ev = events(vec![json!({"type":"text","text":"日本語"})], "end_turn");
    let tail = sse(&ev.split_off(2)).replace('\n', "\r\n");
    let first = sse(&ev).replace('\n', "\r\n");
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request(&mut stream);
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",first.len()+tail.len()).unwrap();
        for bytes in first.as_bytes().chunks(2) {
            stream.write_all(bytes).unwrap();
            stream.flush().unwrap();
        }
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        stream.write_all(tail.as_bytes()).unwrap();
    });
    let mut observed = false;
    let b = backend(url);
    let c = b
        .complete_with_control(
            CompletionRequest::new("u", &[Turn::new("u", TurnRole::User, "go")], None, &[]),
            &|| false,
            &mut |e| {
                if matches!(e, ProviderEvent::TextDelta { .. }) {
                    observed = true;
                    tx.send(()).unwrap();
                }
                Ok(())
            },
        )
        .unwrap();
    h.join().unwrap();
    assert!(observed);
    assert_eq!(c.content, "日本語");
    let (url, _, h) = serve(vec![(
        "text/event-stream".into(),
        sse(&events(vec![text()], "end_turn")).replace('\n', "\r"),
    )]);
    assert!(
        backend(url)
            .complete(CompletionRequest::new(
                "u",
                &[Turn::new("u", TurnRole::User, "go")],
                None,
                &[]
            ))
            .is_ok()
    );
    h.join().unwrap();
}

#[test]
fn secret_handle_uses_native_headers_and_http_auth_failure_stays_typed() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let h = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let (headers, _) = request(&mut stream);
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: bearer opaque-native-key")
        );
        stream
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
    });
    let digest = "a".repeat(64);
    let (secret, _revoke) =
        zenpi::security::SecretHandle::new("opaque-native-key", digest.clone()).unwrap();
    let b = OpenAiCompatibleBackend::new_with_secret_handle_and_settings(
        url,
        secret,
        digest,
        MODEL,
        OpenAiWireApi::AnthropicMessages,
        None,
        None,
    )
    .unwrap();
    assert!(!format!("{b:?}").contains("opaque-native-key"));
    assert!(matches!(
        b.complete(CompletionRequest::new(
            "u",
            &[Turn::new("u", TurnRole::User, "go")],
            None,
            &[]
        )),
        Err(BackendError::HttpStatus { status: 401, .. })
    ));
    h.join().unwrap();
}

#[test]
fn unknown_model_cannot_execute_unadvertised_tool_and_refusal_has_no_calls() {
    let mut ev = events(vec![tool_block()], "tool_use");
    ev[0]["message"]["model"] = json!("unknown-native");
    let (url, _, h) = serve(vec![("text/event-stream".into(), sse(&ev))]);
    let b = OpenAiCompatibleBackend::new_with_wire_api(
        url,
        None,
        "unknown-native",
        OpenAiWireApi::AnthropicMessages,
    )
    .unwrap()
    .with_model_registry("anthropic".into(), ModelRegistry::default())
    .unwrap();
    let mut seen = vec![];
    assert!(
        b.complete_with_control(
            CompletionRequest::new("u", &[Turn::new("u", TurnRole::User, "go")], None, &[]),
            &|| false,
            &mut |e| {
                seen.push(e);
                Ok(())
            }
        )
        .is_err()
    );
    h.join().unwrap();
    assert!(
        !seen
            .iter()
            .any(|e| matches!(e, ProviderEvent::ToolCallDone { .. }))
    );
    let (url, _, h) = serve(vec![(
        "application/json".into(),
        message(json!([]), "refusal").to_string(),
    )]);
    let c = backend(url)
        .complete(CompletionRequest::new(
            "u",
            &[Turn::new("u", TurnRole::User, "go")],
            None,
            &[],
        ))
        .unwrap();
    h.join().unwrap();
    assert!(c.refusal.is_some());
    assert!(c.tool_calls.is_empty());
}

#[test]
fn production_incomplete_tool_stream_never_writes_and_restart_does_not_replay() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("user")).unwrap();
    let bad = json!({"type":"tool_use","id":"toolu_bad","name":"write_file","input":{"path":"NEVER.txt","content":"must not write"}});
    let (url, requests, h) = serve(vec![(
        "text/event-stream".into(),
        sse(&events(vec![bad], "max_tokens")),
    )]);
    std::fs::write(
        root.join("user/config.toml"),
        format!("backend='anthropic'\nmodel='{MODEL}'\nbase_url='{url}'\n"),
    )
    .unwrap();
    {
        let mut cli = Cli::start(root);
        let r = cli.send(json!({"type":"prompt","id":"bad","text":"write"}));
        assert_eq!(r["success"], false);
    }
    h.join().unwrap();
    {
        let mut cli = Cli::start(root);
        let r = cli.send(json!({"type":"status","id":"status"}));
        assert_eq!(r["success"], true, "{r}");
    }
    assert!(!root.join("NEVER.txt").exists());
    assert_eq!(requests.lock().unwrap().len(), 1);
    let journal = std::fs::read_to_string(root.join("session.jsonl")).unwrap();
    assert!(
        !journal
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .any(|v| v["payload"]["turn"]["role"] == "tool")
    );
}

#[test]
fn signed_no_tool_completion_survives_real_101_follow_up_boundary() {
    use zenpi::{
        core::{Agent, TurnInputRequest},
        input_queue::InputKind,
        protocol::{InputQueueAction, InputQueueRequest},
        session::SessionStore,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let requests = Arc::new(Mutex::new(vec![]));
    let seen = requests.clone();
    let blocks = json!([thinking(), text()]);
    let first = message(blocks.clone(), "end_turn").to_string();
    let h = thread::spawn(move || {
        for (i, body) in [first, message(json!([text()]), "end_turn").to_string()]
            .iter()
            .enumerate()
        {
            let (mut stream, _) = listener.accept().unwrap();
            seen.lock().unwrap().push(request(&mut stream).1);
            if i == 0 {
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            }
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    let dir = tempdir().unwrap();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("s.jsonl")).unwrap(),
        Box::new(backend(url)),
    );
    agent.submit(TurnInputRequest::new("original")).unwrap();
    let port = agent.input_port();
    let owner = thread::spawn(move || {
        agent.run_active_turn().unwrap();
        agent
    });
    started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let ticket = port
        .submit(InputQueueRequest {
            schema_version: 2,
            id: "enqueue-follow".into(),
            kind: "input_queue".into(),
            session_id: port.session_id(),
            input_queue: InputQueueAction::Enqueue {
                input_id: "follow-id".into(),
                kind: InputKind::FollowUp,
                text: "next input".into(),
            },
        })
        .unwrap();
    release_tx.send(()).unwrap();
    let agent = owner.join().unwrap();
    assert!(ticket.try_recv().unwrap().is_ok());
    h.join().unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "assistant" && m["content"] == blocks)
    );
    let saved = agent
        .session()
        .turns()
        .iter()
        .find(|t| {
            t.metadata
                .as_ref()
                .is_some_and(|m| m["input_boundary_completion"] == true)
        })
        .unwrap();
    assert_eq!(
        saved.metadata.as_ref().unwrap()["annotations"][0]["blocks"],
        blocks
    );
}

#[test]
fn empty_refusal_remains_replayable_after_next_user_turn() {
    let (url, requests, h) = serve(vec![
        (
            "application/json".into(),
            message(json!([]), "refusal").to_string(),
        ),
        (
            "application/json".into(),
            message(json!([text()]), "end_turn").to_string(),
        ),
    ]);
    let b = backend(url);
    let u = Turn::new("u", TurnRole::User, "go");
    let c = b
        .complete(CompletionRequest::new(
            "u",
            std::slice::from_ref(&u),
            None,
            &[],
        ))
        .unwrap();
    let mut a = Turn::new("a", TurnRole::Assistant, c.content);
    a.metadata = Some(json!({"annotations":c.annotations,"refusal":c.refusal}));
    b.complete(CompletionRequest::new(
        "v",
        &[u, a, Turn::new("v", TurnRole::User, "another question")],
        None,
        &[],
    ))
    .unwrap();
    h.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 2);
}

#[test]
fn independent_semantic_summary_marker_is_local_and_no_format_is_forced() {
    let (url, requests, h) = serve(vec![(
        "application/json".into(),
        message(json!([text()]), "end_turn").to_string(),
    )]);
    let b = backend(url);
    let summary = [Turn::new(
        "summary",
        TurnRole::User,
        "Summarize the supplied transcript",
    )];
    b.complete(
        CompletionRequest::new("summary", &summary, None, &[])
            .with_metadata(Some(&json!({"purpose":"semantic_compaction"})))
            .with_max_output_tokens(1024),
    )
    .unwrap();
    h.join().unwrap();
    let r = requests.lock().unwrap();
    assert!(r[0].1.get("metadata").is_none());
    assert!(r[0].1.get("response_format").is_none());
    assert!(r[0].1.get("output_config").is_none());
    assert!(r[0].1.get("tools").is_none());
    assert_eq!(r[0].1["max_tokens"], 1024);
}
