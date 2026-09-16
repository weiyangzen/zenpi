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
const MODEL: &str = "gemini-2.5-flash";
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
fn backend(url: String) -> OpenAiCompatibleBackend {
    OpenAiCompatibleBackend::new_with_wire_api(
        url,
        Some("native-fixture-key".into()),
        MODEL,
        OpenAiWireApi::GoogleGenerativeAi,
    )
    .unwrap()
    .with_model_registry("google".into(), ModelRegistry::default())
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
            "GEMINI_MODEL",
            "GEMINI_BASE_URL",
            "GEMINI_API_KEY",
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

fn answer(parts: Value, stop: &str) -> Value {
    json!({"responseId":"response_fixture","modelVersion":MODEL,"candidates":[{"index":0,"content":{"role":"model","parts":parts},"finishReason":stop}],"usageMetadata":{"promptTokenCount":12,"cachedContentTokenCount":3,"candidatesTokenCount":4,"thoughtsTokenCount":2,"totalTokenCount":18}})
}
fn signed() -> Value {
    json!([{"text":"summary","thought":true,"thoughtSignature":"c2lnbmF0dXJl"},{"text":"","thoughtSignature":"ZW1wdHk="},{"functionCall":{"name":"read_file","args":{"path":"hello.txt"}},"thoughtSignature":"dG9vbA=="}])
}
fn sse(events: &[Value]) -> String {
    events.iter().map(|v| format!("data: {v}\n\n")).collect()
}
fn user() -> [Turn; 1] {
    [Turn::new("u", TurnRole::User, "hello")]
}
#[test]
fn native_request_auth_system_schema_effort_usage_and_summary_contract() {
    let (url, requests, h) = serve(vec![(
        "application/json".into(),
        answer(json!([{"text":"answer"}]), "STOP").to_string(),
    )]);
    let mut b = backend(url);
    b.validate_reasoning_effort(None, Some("high")).unwrap();
    b.commit_reasoning_effort(Some("high".into()));
    let turns = [
        Turn::new("s", TurnRole::System, "system"),
        Turn::new("u", TurnRole::User, "hello"),
    ];
    let c = b
        .complete(
            CompletionRequest::new("u", &turns, None, &[tool()])
                .with_instructions(Some("instructions"))
                .with_metadata(Some(&json!({"purpose":"semantic_compaction"})))
                .with_max_output_tokens(1234),
        )
        .unwrap();
    h.join().unwrap();
    let req = requests.lock().unwrap();
    let (head, v) = &req[0];
    assert!(
        head.starts_with("POST /v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse ")
    );
    assert!(
        head.to_lowercase()
            .contains("x-goog-api-key: native-fixture-key")
    );
    assert!(!head.to_lowercase().contains("authorization:"));
    assert_eq!(
        v["systemInstruction"]["parts"],
        json!([{"text":"instructions"},{"text":"system"}])
    );
    assert_eq!(
        v["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"],
        tool().input_schema
    );
    assert_eq!(v["generationConfig"]["maxOutputTokens"], 1234);
    assert_eq!(
        v["generationConfig"]["thinkingConfig"],
        json!({"thinkingBudget":24576,"includeThoughts":true})
    );
    for key in ["stream", "model", "metadata", "response_format", "messages"] {
        assert!(v.get(key).is_none());
    }
    assert_eq!(c.usage.unwrap().input_tokens, 12);
    assert_eq!(c.usage.unwrap().output_tokens, 6);
    assert_eq!(c.content, "answer");
}
#[test]
fn streamed_parts_signatures_empty_text_and_parallel_calls_stay_ordered() {
    let parts = json!([{"text":"think","thought":true},{"text":"" ,"thoughtSignature":"c2ln"},{"text":"visible","thoughtSignature":"dGV4dA=="},{"functionCall":{"id":"a","name":"read_file","args":{"path":"a"}},"thoughtSignature":"dG9vbA=="},{"functionCall":{"id":"b","name":"read_file","args":{"path":"b"}}}]);
    let mut chunks = vec![];
    for p in parts.as_array().unwrap() {
        let mut v = answer(json!([p]), "STOP");
        v["candidates"][0]
            .as_object_mut()
            .unwrap()
            .remove("finishReason");
        v.as_object_mut().unwrap().remove("usageMetadata");
        chunks.push(v);
    }
    chunks.push(answer(json!([]), "STOP"));
    let (url, _, h) = serve(vec![("text/event-stream".into(), sse(&chunks))]);
    let mut seen = vec![];
    let c = backend(url)
        .complete_with_control(
            CompletionRequest::new("u", &user(), None, &[tool()]),
            &|| false,
            &mut |e| {
                seen.push(e);
                Ok(())
            },
        )
        .unwrap();
    h.join().unwrap();
    assert_eq!(c.content, "visible");
    assert_eq!(c.tool_calls.len(), 2);
    assert_eq!(c.annotations[0]["parts"], parts);
    assert!(
        seen.iter()
            .any(|e| matches!(e,ProviderEvent::ReasoningDelta{delta}if delta=="think"))
    );
    assert_eq!(
        seen.iter()
            .filter(|e| matches!(e, ProviderEvent::ToolCallDone { .. }))
            .count(),
        2
    );
}
#[test]
fn malformed_finish_signature_usage_and_candidate_never_publish_calls() {
    let base = answer(signed(), "STOP");
    let mut cases = vec![];
    for stop in ["MAX_TOKENS", "SAFETY", "MALFORMED_FUNCTION_CALL", "OTHER"] {
        let mut v = base.clone();
        v["candidates"][0]["finishReason"] = json!(stop);
        cases.push(v);
    }
    let mut v = base.clone();
    v["candidates"][0]
        .as_object_mut()
        .unwrap()
        .remove("finishReason");
    cases.push(v);
    let mut v = base.clone();
    v["candidates"][0]["content"]["parts"][2]["thoughtSignature"] = json!("");
    cases.push(v);
    let mut v = base.clone();
    v["candidates"][0]["content"]["parts"][2]["functionCall"]["args"] = json!("bad");
    cases.push(v);
    let mut v = base.clone();
    v["usageMetadata"]["totalTokenCount"] = json!(17);
    cases.push(v);
    let mut v = base.clone();
    v["candidates"][0]["content"]["parts"]
        .as_array_mut()
        .unwrap()
        .push(json!({"executableCode":{"code":"bad"}}));
    cases.push(v);
    let mut v = base.clone();
    v["candidates"][0]["index"] = json!(1);
    cases.push(v);
    let mut wrong_model = base.clone();
    wrong_model["modelVersion"] = json!("different-provider-model");
    cases.push(wrong_model);
    for v in cases {
        let (url, _, h) = serve(vec![("application/json".into(), v.to_string())]);
        let mut done = false;
        let r = backend(url).complete_with_control(
            CompletionRequest::new("u", &user(), None, &[tool()]),
            &|| false,
            &mut |e| {
                done |= matches!(e, ProviderEvent::ToolCallDone { .. });
                Ok(())
            },
        );
        assert!(r.is_err());
        assert!(!done);
        h.join().unwrap();
    }
}
#[test]
fn truncated_and_post_terminal_streams_fail_closed() {
    for body in [
        "data: {bad}\n\n".into(),
        format!("data: {}", answer(signed(), "STOP")),
        sse(&[
            answer(signed(), "STOP"),
            answer(json!([{"text":"late"}]), "STOP"),
        ]),
        format!("data: {}\n\ndata: [DONE]\n\n", answer(signed(), "STOP")),
    ] {
        let (url, _, h) = serve(vec![("text/event-stream".into(), body)]);
        let mut done = false;
        assert!(
            backend(url)
                .complete_with_control(
                    CompletionRequest::new("u", &user(), None, &[tool()]),
                    &|| false,
                    &mut |e| {
                        done |= matches!(e, ProviderEvent::ToolCallDone { .. });
                        Ok(())
                    }
                )
                .is_err()
        );
        assert!(!done);
        h.join().unwrap();
    }
}
#[test]
fn signed_tool_results_and_restart_use_exact_native_parts() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("user")).unwrap();
    std::fs::write(root.join("hello.txt"), "GEMINI_TOOL_BODY").unwrap();
    let parts = signed();
    let (url, requests, h) = serve(vec![
        (
            "text/event-stream".into(),
            sse(&[answer(parts.clone(), "STOP")]),
        ),
        (
            "application/json".into(),
            answer(json!([{"text":"done"}]), "STOP").to_string(),
        ),
        (
            "application/json".into(),
            answer(json!([{"text":"continued"}]), "STOP").to_string(),
        ),
    ]);
    std::fs::write(
        root.join("user/config.toml"),
        format!(
            "backend='google'\nbase_url='{url}'\nmodel='{MODEL}'\nmodel_reasoning_effort='low'\n"
        ),
    )
    .unwrap();
    {
        let mut cli = Cli::start(root);
        let r = cli.send(json!({"type":"prompt","id":"one","text":"read hello"}));
        assert_eq!(r["success"], true, "{r}");
    }
    {
        let mut cli = Cli::start(root);
        let r = cli.send(json!({"type":"prompt","id":"two","text":"continue"}));
        assert_eq!(r["success"], true, "{r}");
    }
    h.join().unwrap();
    let req = requests.lock().unwrap();
    assert_eq!(req.len(), 3);
    for i in [1, 2] {
        let contents = req[i].1["contents"].as_array().unwrap();
        let model = contents.iter().find(|c| c["role"] == "model").unwrap();
        assert_eq!(model["parts"], parts);
        let response = contents
            .iter()
            .flat_map(|c| c["parts"].as_array().unwrap())
            .find_map(|p| p.get("functionResponse"))
            .unwrap();
        assert_eq!(response["name"], "read_file");
        assert!(
            response["response"]["output"]
                .as_str()
                .unwrap()
                .contains("GEMINI_TOOL_BODY")
        );
        assert!(response.get("id").is_none());
    }
    let journal = std::fs::read_to_string(root.join("session.jsonl")).unwrap();
    assert!(journal.contains("dG9vbA=="));
    assert!(!journal.contains("native-fixture-key"));
}
#[test]
fn no_format_metadata_model_and_path_errors_fail_before_network() {
    let b = backend("http://127.0.0.1:1".into());
    let u = user();
    for metadata in [
        json!({"purpose":"unknown"}),
        json!({"user_id":"x"}),
        json!([]),
    ] {
        assert!(
            b.complete(CompletionRequest::new("u", &u, None, &[]).with_metadata(Some(&metadata)))
                .is_err()
        );
    }
    assert!(
        b.complete(
            CompletionRequest::new("u", &u, None, &[])
                .with_response_format(Some(&json!({"type":"json_object"})))
        )
        .is_err()
    );
    assert!(
        b.complete(CompletionRequest::new("u", &u, Some("bad/../model"), &[]))
            .is_err()
    );
    assert!(b.validate_reasoning_effort(None, Some("max")).is_err());
    assert!(
        OpenAiCompatibleBackend::new_with_wire_api(
            "http://127.0.0.1/models/x:generateContent",
            None,
            MODEL,
            OpenAiWireApi::GoogleGenerativeAi
        )
        .is_err()
    );
}
#[test]
fn native_history_tamper_cross_model_and_cross_wire_are_rejected() {
    let (url, _, h) = serve(vec![(
        "application/json".into(),
        answer(signed(), "STOP").to_string(),
    )]);
    let c = backend(url)
        .complete(CompletionRequest::new("u", &user(), None, &[tool()]))
        .unwrap();
    h.join().unwrap();
    let mut a = Turn::new("a", TurnRole::Assistant, "");
    a.metadata = Some(json!({"annotations":c.annotations,"tool_calls":c.tool_calls}));
    let mut t = Turn::new("t", TurnRole::Tool, "result");
    t.metadata = Some(json!({"tool_call_id":c.tool_calls[0].id}));
    let b = backend("http://127.0.0.1:1".into());
    let mut tampered = a.clone();
    tampered.content = "changed".into();
    assert!(
        b.complete(CompletionRequest::new(
            "u",
            &[user()[0].clone(), tampered, t.clone()],
            None,
            &[tool()]
        ))
        .is_err()
    );
    let mut missing = a.clone();
    missing.metadata.as_mut().unwrap()["annotations"][0]["parts"][2]["thoughtSignature"] =
        json!("");
    assert!(
        b.complete(CompletionRequest::new(
            "u",
            &[user()[0].clone(), missing, t.clone()],
            None,
            &[tool()]
        ))
        .is_err()
    );
    let openai = OpenAiCompatibleBackend::new_with_wire_api(
        "http://127.0.0.1:1",
        None,
        "gpt-5",
        OpenAiWireApi::Responses,
    )
    .unwrap();
    assert!(
        openai
            .complete(CompletionRequest::new(
                "u",
                &[user()[0].clone(), a.clone(), t.clone()],
                None,
                &[]
            ))
            .is_err()
    );
    let mut wrong = a;
    wrong.metadata.as_mut().unwrap()["annotations"][0]["model"] = json!("gemini-other");
    assert!(
        b.complete(CompletionRequest::new(
            "u",
            &[user()[0].clone(), wrong, t],
            None,
            &[tool()]
        ))
        .is_err()
    );
}
#[test]
fn native_image_bytes_and_effort_none_are_exact() {
    let (url, requests, h) = serve(vec![(
        "application/json".into(),
        answer(json!([{"text":"image"}]), "STOP").to_string(),
    )]);
    let mut b = backend(url);
    b.commit_reasoning_effort(Some("none".into()));
    let attachments = [RequestAttachment {
        turn_id: "u".into(),
        input: InputAttachment {
            kind: AttachmentKind::Image,
            mime_type: "image/png".into(),
            path: Some("image.png".into()),
            url: None,
            file_id: None,
        },
        data: Some(vec![1, 2, 3]),
        filename: None,
        size_bytes: Some(3),
        sha256: None,
    }];
    b.complete(CompletionRequest::new("u", &user(), None, &[]).with_attachments(&attachments))
        .unwrap();
    h.join().unwrap();
    let req = requests.lock().unwrap();
    assert_eq!(
        req[0].1["contents"][0]["parts"][1],
        json!({"inlineData":{"mimeType":"image/png","data":"AQID"}})
    );
    assert_eq!(
        req[0].1["generationConfig"]["thinkingConfig"],
        json!({"thinkingBudget":0,"includeThoughts":false})
    );
}
#[test]
fn native_config_does_not_borrow_openai_or_anthropic_identity() {
    use zenpi::config::{AuthFile, ConfigFile, ConfigOverrides};
    let env = BTreeMap::from([
        ("OPENAI_MODEL".into(), "wrong".into()),
        ("OPENAI_API_KEY".into(), "wrong-key".into()),
        ("ANTHROPIC_API_KEY".into(), "wrong-anthropic".into()),
        ("GEMINI_MODEL".into(), MODEL.into()),
        ("GEMINI_API_KEY".into(), "gemini-fixture".into()),
    ]);
    let cfg: ConfigFile = toml::from_str("backend='google'").unwrap();
    let resolved = zenpi::config::resolve(
        &ConfigOverrides::default(),
        &cfg,
        &AuthFile::default(),
        &env,
    )
    .unwrap();
    assert_eq!(resolved.provider.as_deref(), Some("google"));
    assert_eq!(resolved.model.as_deref(), Some(MODEL));
    assert_eq!(resolved.api_key.as_deref(), Some("gemini-fixture"));
    assert_eq!(
        resolved.base_url.as_deref(),
        Some("https://generativelanguage.googleapis.com/v1beta")
    );
}
#[test]
fn unknown_model_uses_bounded_native_json_and_cannot_execute_tool() {
    let mut response = answer(signed(), "STOP");
    response["modelVersion"] = json!("future-model");
    let (url, requests, h) = serve(vec![("application/json".into(), response.to_string())]);
    let b = backend(url);
    let mut done = false;
    assert!(
        b.complete_with_control(
            CompletionRequest::new("u", &user(), Some("future-model"), &[]),
            &|| false,
            &mut |e| {
                done |= matches!(e, ProviderEvent::ToolCallDone { .. });
                Ok(())
            }
        )
        .is_err()
    );
    assert!(!done);
    h.join().unwrap();
    let req = requests.lock().unwrap();
    assert!(
        req[0]
            .0
            .starts_with("POST /v1beta/models/future-model:generateContent ")
    );
    assert_eq!(req[0].1["generationConfig"]["maxOutputTokens"], 4096);
}
#[test]
fn first_text_precedes_finish_and_split_utf8_crlf_is_supported() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let mut first = answer(json!([{"text":"日本語"}]), "STOP");
    first["candidates"][0]
        .as_object_mut()
        .unwrap()
        .remove("finishReason");
    first.as_object_mut().unwrap().remove("usageMetadata");
    let first = sse(&[first]).replace('\n', "\r\n");
    let tail = sse(&[answer(json!([]), "STOP")]).replace('\n', "\r\n");
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
    let c = backend(url)
        .complete_with_control(
            CompletionRequest::new("u", &user(), None, &[]),
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
    assert!(observed);
    assert_eq!(c.content, "日本語");
    h.join().unwrap();
}

#[test]
fn model_selection_rejects_opaque_history_before_journal_but_plain_text_can_switch() {
    use zenpi::{core::Agent, session::SessionStore};
    let (url, _, h) = serve(vec![(
        "application/json".into(),
        answer(json!([{"text":"done","thoughtSignature":"c2ln"}]), "STOP").to_string(),
    )]);
    let c = backend(url)
        .complete(CompletionRequest::new("u", &user(), None, &[]))
        .unwrap();
    h.join().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut session = SessionStore::open(&path).unwrap();
    session.append_turn(user()[0].clone()).unwrap();
    let mut a = Turn::new("a", TurnRole::Assistant, c.content);
    a.metadata = Some(json!({"annotations":c.annotations}));
    session.append_turn(a).unwrap();
    let mut agent = Agent::new(session, Box::new(backend("http://127.0.0.1:1".into())));
    let before = std::fs::read(&path).unwrap();
    assert!(agent.set_model(Some("compatible-future".into())).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    agent.set_model(Some(MODEL.into())).unwrap();
    let path = dir.path().join("plain.jsonl");
    let mut session = SessionStore::open(&path).unwrap();
    session.append_turn(user()[0].clone()).unwrap();
    session
        .append_turn(Turn::new("a", TurnRole::Assistant, "plain"))
        .unwrap();
    let mut agent = Agent::new(session, Box::new(backend("http://127.0.0.1:1".into())));
    agent.set_model(Some("compatible-future".into())).unwrap();
}

#[test]
fn production_incomplete_stream_never_executes_and_restart_stays_readable() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("user")).unwrap();
    let parts = json!([{"functionCall":{"name":"write_file","args":{"path":"NEVER.txt","content":"must not execute"}}}]);
    let (url, requests, h) = serve(vec![(
        "text/event-stream".into(),
        sse(&[answer(parts, "MAX_TOKENS")]),
    )]);
    std::fs::write(
        root.join("user/config.toml"),
        format!("backend='google'\nbase_url='{url}'\nmodel='{MODEL}'\n"),
    )
    .unwrap();
    {
        let mut cli = Cli::start(root);
        let r = cli.send(json!({"type":"prompt","id":"one","text":"write file"}));
        assert_eq!(r["success"], false);
    }
    h.join().unwrap();
    {
        let mut cli = Cli::start(root);
        let r = cli.send(json!({"type":"status","id":"status"}));
        assert_eq!(r["success"], true);
    }
    assert!(!root.join("NEVER.txt").exists());
    assert_eq!(requests.lock().unwrap().len(), 1);
}

#[test]
fn resume_rejects_saved_model_incompatible_with_signed_history_without_changing_owner() {
    use zenpi::{core::Agent, session::SessionStore};
    let dir = tempdir().unwrap();
    let target = dir.path().join("target.jsonl");
    let mut session = SessionStore::open(&target).unwrap();
    session.append_turn(user()[0].clone()).unwrap();
    let mut a = Turn::new("a", TurnRole::Assistant, "signed");
    a.metadata = Some(
        json!({"annotations":[{"type":"native_history","wire":"google_generative_ai","provider":"google","model":MODEL,"parts":[{"text":"signed","thoughtSignature":"c2ln"}]}]}),
    );
    session.append_turn(a).unwrap();
    let b = backend("http://127.0.0.1:1".into());
    let descriptor = b.model_descriptor(Some("future-model")).unwrap().unwrap();
    session.append_event(json!({"type":"model_selected","model":"future-model","descriptor":descriptor,"digest":descriptor.digest(),"reasoning_effort":null})).unwrap();
    drop(session);
    let old = dir.path().join("old.jsonl");
    let mut agent = Agent::new(SessionStore::open(&old).unwrap(), Box::new(b));
    let before = agent.snapshot();
    let bytes = std::fs::read(&old).unwrap();
    assert!(agent.resume_session(&target).is_err());
    assert_eq!(
        agent.snapshot().session.session_id,
        before.session.session_id
    );
    assert_eq!(std::fs::read(&old).unwrap(), bytes);
}

#[test]
fn no_argument_call_and_supplied_id_replay_without_rewriting_native_parts() {
    let parts =
        json!([{"functionCall":{"id":"call_fixed","name":"list_files"},"thoughtSignature":"c2ln"}]);
    let (url, requests, h) = serve(vec![
        (
            "application/json".into(),
            answer(parts.clone(), "STOP").to_string(),
        ),
        (
            "application/json".into(),
            answer(json!([{"text":"done"}]), "STOP").to_string(),
        ),
    ]);
    let b = backend(url);
    let c = b
        .complete(CompletionRequest::new("u", &user(), None, &[tool()]))
        .unwrap();
    assert_eq!(c.tool_calls[0].arguments, json!({}));
    let mut a = Turn::new("a", TurnRole::Assistant, "");
    a.metadata = Some(json!({"annotations":c.annotations,"tool_calls":c.tool_calls}));
    let mut t = Turn::new("t", TurnRole::Tool, "ok");
    t.metadata = Some(json!({"tool_call_id":"call_fixed"}));
    b.complete(CompletionRequest::new(
        "u",
        &[user()[0].clone(), a, t],
        None,
        &[tool()],
    ))
    .unwrap();
    h.join().unwrap();
    let req = requests.lock().unwrap();
    assert_eq!(req[1].1["contents"][1]["parts"], parts);
    assert_eq!(
        req[1].1["contents"][2]["parts"][0]["functionResponse"]["id"],
        "call_fixed"
    );
}

#[test]
fn explicitly_enabled_gemini3_requires_first_call_signature_and_never_invents_it() {
    use zenpi::providers::registry::ModelOverride;
    let model = "gemini-3-explicit-fixture";
    let override_model:ModelOverride=serde_json::from_value(json!({"provider":"google","id":model,"version":"fixture-only","tools":true,"streaming":true,"reasoning_levels":["low"]})).unwrap();
    for sig in [None, Some("c2ln")] {
        let mut part = json!({"functionCall":{"name":"read_file","args":{"path":"hello.txt"}}});
        if let Some(sig) = sig {
            part["thoughtSignature"] = json!(sig);
        }
        let mut response = answer(json!([part]), "STOP");
        response["modelVersion"] = json!(model);
        let (url, _, h) = serve(vec![("application/json".into(), response.to_string())]);
        let b = OpenAiCompatibleBackend::new_with_wire_api(
            url,
            None,
            model,
            OpenAiWireApi::GoogleGenerativeAi,
        )
        .unwrap()
        .with_model_registry(
            "google".into(),
            ModelRegistry::with_overrides(std::slice::from_ref(&override_model)).unwrap(),
        )
        .unwrap();
        let mut done = false;
        let r = b.complete_with_control(
            CompletionRequest::new("u", &user(), None, &[tool()]),
            &|| false,
            &mut |e| {
                done |= matches!(e, ProviderEvent::ToolCallDone { .. });
                Ok(())
            },
        );
        assert_eq!(r.is_ok(), sig.is_some());
        assert_eq!(done, sig.is_some());
        h.join().unwrap();
    }
}
