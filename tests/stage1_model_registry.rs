use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};
use tempfile::tempdir;
use zenpi::{
    backend::{
        Backend, CompletionRequest, InputAttachment, OpenAiCompatibleBackend, OpenAiWireApi,
        ProviderCapabilities, RequestAttachment,
    },
    config::{AuthFile, ConfigFile, ConfigOverrides},
    context::ContextBudget,
    core::{Agent, Turn, TurnInputRequest, TurnRole},
    providers::registry::{FieldSource, ModelOverride, ModelRegistry, ReasoningLevel},
    session::SessionStore,
};

fn override_entry(value: Value) -> ModelOverride {
    serde_json::from_value(value).unwrap()
}
fn strict(
    url: &str,
    id: &str,
    wire: OpenAiWireApi,
    overrides: &[ModelOverride],
) -> OpenAiCompatibleBackend {
    OpenAiCompatibleBackend::new_with_wire_api(url, None, id, wire)
        .unwrap()
        .with_model_registry(
            "openai".into(),
            ModelRegistry::with_overrides(overrides).unwrap(),
        )
        .unwrap()
}

#[test]
fn exact_identity_and_partial_sources_never_infer_from_names_or_wire() {
    let registry = ModelRegistry::with_overrides(&[override_entry(json!({
        "provider":"openai","id":"gpt-4.1","version":"user-1","context_window":64000
    }))])
    .unwrap();
    let model = registry.resolve("openai", "gpt-4.1").unwrap();
    assert_eq!(model.context_window, 64000);
    assert!(matches!(
        model.sources["context_window"],
        FieldSource::UserOverride { .. }
    ));
    assert!(matches!(
        model.sources["images"],
        FieldSource::Builtin { .. }
    ));
    assert!(model.capabilities.files);
    assert!(
        !model
            .effective_capabilities(ProviderCapabilities::for_wire_api(
                OpenAiWireApi::ChatCompletions
            ))
            .files
    );
    for (provider, id) in [
        ("gateway", "gpt-4.1"),
        ("openai", "gpt-4.1-future"),
        ("openai", "GPT-4.1"),
    ] {
        let unknown = registry.resolve(provider, id).unwrap();
        assert_eq!(
            (unknown.provider.as_str(), unknown.id.as_str()),
            (provider, id)
        );
        assert_eq!(unknown.context_window, 32768);
        assert_eq!(unknown.max_output_tokens, 4096);
        // Provider openness: uncatalogued models default to the open wire
        // capability set rather than a closed text-only profile.
        assert!(
            unknown.capabilities.tools
                && unknown.capabilities.images
                && unknown.capabilities.streaming
        );
        assert!(unknown.price.is_none());
        assert!(
            unknown
                .sources
                .values()
                .all(|source| matches!(source, FieldSource::Conservative { .. }))
        );
    }
}

#[test]
fn strict_override_limits_duplicates_levels_and_unknown_keys_fail_closed() {
    for value in [
        json!({"context_window":0}),
        json!({"max_output_tokens":0}),
        json!({"context_window":300,"max_output_tokens":400}),
        json!({"context_window":16777217}),
    ] {
        let mut base = json!({"provider":"local","id":"custom","version":"v1"});
        base.as_object_mut()
            .unwrap()
            .extend(value.as_object().unwrap().clone());
        assert!(ModelRegistry::with_overrides(&[override_entry(base)]).is_err());
    }
    let entry = override_entry(json!({"provider":"local","id":"custom","version":"v1"}));
    assert!(ModelRegistry::with_overrides(&[entry.clone(), entry.clone()]).is_err());
    assert!(ModelRegistry::with_overrides(&vec![entry; 257]).is_err());
    assert!(
        serde_json::from_value::<ModelOverride>(
            json!({"provider":"local","id":"custom","version":"v1","surprise":true})
        )
        .is_err()
    );
    assert!(ReasoningLevel::parse("possibly").is_err());
}

#[test]
fn config_roundtrip_keeps_exact_overrides_and_price_provenance() {
    let config: ConfigFile = toml::from_str(
        r#"
model = "gpt-4.1"
[[model_overrides]]
provider = "openai"
id = "gpt-4.1"
version = "fixture-contract-v1"
context_window = 48000
[model_overrides.price]
input_micro_usd_per_million = 1000000
output_micro_usd_per_million = 3000000
source = "fixture-pricing"
version = "fixture-price-v1"
"#,
    )
    .unwrap();
    config.validate().unwrap();
    let serialized = toml::to_string(&config).unwrap();
    let restored: ConfigFile = toml::from_str(&serialized).unwrap();
    let resolved = zenpi::config::resolve(
        &ConfigOverrides::default(),
        &restored,
        &AuthFile::default(),
        &Default::default(),
    )
    .unwrap();
    let first = ModelRegistry::with_overrides(&config.model_overrides)
        .unwrap()
        .resolve("openai", "gpt-4.1")
        .unwrap();
    let second = ModelRegistry::with_overrides(&resolved.model_overrides)
        .unwrap()
        .resolve("openai", "gpt-4.1")
        .unwrap();
    assert_eq!(first.digest(), second.digest());
    assert_eq!(second.price.unwrap().source, "fixture-pricing");
}

#[test]
fn failed_model_switch_preserves_journal_and_budget_and_restart_selection() {
    let dir = tempdir().unwrap();
    let session = dir.path().join("session.jsonl");
    let backend = OpenAiCompatibleBackend::new_with_settings(
        "http://127.0.0.1:9/v1",
        None,
        "gpt-5.2",
        OpenAiWireApi::Responses,
        Some("high".into()),
        None,
    )
    .unwrap()
    .with_model_registry("openai".into(), ModelRegistry::default())
    .unwrap();
    let mut agent = Agent::new(SessionStore::open(&session).unwrap(), Box::new(backend));
    agent.set_context_budget(ContextBudget {
        max_tokens: 500000,
        reserved_output_tokens: 8192,
    });
    let old = agent.model_status().unwrap();
    let bytes = fs::read(&session).unwrap();
    assert!(agent.set_model(Some("gpt-4.1".into())).is_err());
    assert_eq!(old, agent.model_status().unwrap());
    assert_eq!(bytes, fs::read(&session).unwrap());
    agent.set_model(Some("gpt-5.2-2025-12-11".into())).unwrap();
    let selected = agent.model_status().unwrap();
    drop(agent);
    let mut restored = Agent::new(
        SessionStore::open(&session).unwrap(),
        Box::new(strict(
            "http://127.0.0.1:9",
            "gpt-5.2",
            OpenAiWireApi::Responses,
            &[],
        )),
    );
    restored.set_context_budget(ContextBudget {
        max_tokens: 500000,
        reserved_output_tokens: 8192,
    });
    restored.restore_model_selection().unwrap();
    assert_eq!(selected, restored.model_status().unwrap());
    assert_eq!(restored.reasoning_effort(), Some("high"));
    restored.set_reasoning_effort(None).unwrap();
    restored.set_model(Some("unlisted".into())).unwrap();
    assert_eq!(
        restored.context_budget(),
        ContextBudget {
            max_tokens: 32768,
            reserved_output_tokens: 4096
        }
    );
}

#[test]
fn changed_saved_metadata_requires_explicit_selection_before_resume() {
    let dir = tempdir().unwrap();
    let session = dir.path().join("session.jsonl");
    let mut agent = Agent::new(
        SessionStore::open(&session).unwrap(),
        Box::new(strict(
            "http://127.0.0.1:9",
            "gpt-4.1",
            OpenAiWireApi::Responses,
            &[],
        )),
    );
    agent.set_model(Some("gpt-4.1".into())).unwrap();
    drop(agent);
    let entry = override_entry(
        json!({"provider":"openai","id":"gpt-4.1","version":"new","context_window":48000}),
    );
    let mut restored = Agent::new(
        SessionStore::open(&session).unwrap(),
        Box::new(strict(
            "http://127.0.0.1:9",
            "gpt-4.1",
            OpenAiWireApi::Responses,
            &[entry],
        )),
    );
    assert!(restored.restore_model_selection().is_err());
    restored.set_model(Some("gpt-4.1".into())).unwrap();
    restored.restore_model_selection().unwrap();
    assert_eq!(restored.context_budget().max_tokens, 48000);
}

#[test]
fn unsupported_attachment_and_reasoning_do_not_open_http_or_append_turn() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    // An explicit restrictive override still closes capabilities even though
    // the uncatalogued default is now open.
    let restricted = [override_entry(json!({
        "provider":"openai","id":"unknown","version":"fixture-v1",
        "images":false,"files":false,"tools":false,"structured_output":false
    }))];
    let backend = strict(&url, "unknown", OpenAiWireApi::Responses, &restricted);
    let input = serde_json::from_value::<InputAttachment>(
        json!({"kind":"image","mime_type":"image/png","url":"https://example.test/image.png"}),
    )
    .unwrap();
    let turns = [Turn::new("t", TurnRole::User, "question")];
    let attachment = RequestAttachment {
        turn_id: "t".into(),
        input: input.clone(),
        filename: None,
        data: None,
        size_bytes: None,
        sha256: None,
    };
    assert!(
        backend
            .complete(
                CompletionRequest::new("t", &turns, None, &[]).with_attachments(&[attachment])
            )
            .unwrap_err()
            .to_string()
            .contains("attachment")
    );
    let format = json!({"type":"json_object"});
    assert!(
        backend
            .complete(
                CompletionRequest::new("t", &turns, None, &[]).with_response_format(Some(&format))
            )
            .unwrap_err()
            .to_string()
            .contains("structured")
    );
    let reasoning = OpenAiCompatibleBackend::new_with_settings(
        &url,
        None,
        "unknown",
        OpenAiWireApi::Responses,
        Some("high".into()),
        None,
    )
    .unwrap();
    assert!(
        reasoning
            .with_model_registry("openai".into(), ModelRegistry::default())
            .is_err()
    );
    let dir = tempdir().unwrap();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("s")).unwrap(),
        Box::new(backend),
    );
    let before = agent.history().len();
    let mut request = TurnInputRequest::new("question");
    request.attachments = vec![input];
    assert!(agent.submit(request).is_err());
    assert_eq!(before, agent.history().len());
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

fn read_request(stream: &mut TcpStream) -> Value {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut bytes = Vec::new();
    let end = loop {
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
        assert!(bytes.len() < 65536);
    };
    let len: usize = String::from_utf8_lossy(&bytes[..end])
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(str::to_owned)
        })
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(len < 2 * 1024 * 1024);
    while bytes.len() < end + len {
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&chunk[..count]);
    }
    serde_json::from_slice(&bytes[end..end + len]).unwrap()
}
fn server(
    count: usize,
    responses: bool,
) -> (String, mpsc::Receiver<Value>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (send, recv) = mpsc::channel();
    let handle = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        for _ in 0..count {
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            };
            let request = read_request(&mut stream);
            let response = if responses {
                json!({"id":"response","status":"completed","model":request["model"],"output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}]})
            } else {
                json!({"id":"response","model":request["model"],"choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]})
            };
            let (mime, body) = if responses && request["stream"] == true {
                (
                    "text/event-stream",
                    format!(
                        "data: {}\n\ndata: {}\n\n",
                        json!({"type":"response.output_text.delta","delta":"ok"}),
                        json!({"type":"response.completed","response":response})
                    ),
                )
            } else if !responses && request["stream"] == true {
                (
                    "text/event-stream",
                    format!(
                        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                        json!({"id":"chunk","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"ok"},"finish_reason":null}]}),
                        json!({"id":"chunk","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}})
                    ),
                )
            } else {
                ("application/json", response.to_string())
            };
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            stream.flush().unwrap();
            send.send(request).unwrap();
        }
    });
    (url, recv, handle)
}

#[test]
fn responses_retains_images_files_and_structured_output_and_honors_output_cap() {
    let (url, requests, server) = server(1, true);
    let backend = strict(&url, "gpt-4.1", OpenAiWireApi::Responses, &[]);
    let turns = [Turn::new("t", TurnRole::User, "question")];
    let attachments = [
        serde_json::from_value::<InputAttachment>(
            json!({"kind":"image","mime_type":"image/png","url":"https://example.test/image.png"}),
        )
        .unwrap(),
        serde_json::from_value::<InputAttachment>(
            json!({"kind":"file","mime_type":"application/pdf","file_id":"file-fixture"}),
        )
        .unwrap(),
    ]
    .into_iter()
    .map(|input| RequestAttachment {
        turn_id: "t".into(),
        input,
        filename: None,
        data: None,
        size_bytes: None,
        sha256: None,
    })
    .collect::<Vec<_>>();
    let format = json!({"type":"json_schema","json_schema":{"name":"answer","strict":true,"schema":{"type":"object","properties":{},"additionalProperties":false}}});
    backend
        .complete(
            CompletionRequest::new("t", &turns, None, &[])
                .with_attachments(&attachments)
                .with_response_format(Some(&format))
                .with_max_output_tokens(1024),
        )
        .unwrap();
    let sent = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(sent["max_output_tokens"], 1024);
    assert_eq!(sent["text"]["format"]["type"], "json_schema");
    let serialized = sent.to_string();
    assert!(serialized.contains("input_image") && serialized.contains("input_file"));
    server.join().unwrap();
}

struct Cli {
    child: Child,
    input: ChildStdin,
    lines: mpsc::Receiver<Value>,
    reader: Option<thread::JoinHandle<()>>,
}
impl Cli {
    fn start(root: &Path, url: &str, model: &str, extra: &str, wire: &str) -> Self {
        fs::create_dir_all(root.join("user")).unwrap();
        fs::write(root.join("user/config.toml"), format!("backend='openai'\nprovider='openai'\nmodel='{model}'\nbase_url='{url}'\nwire_api='{wire}'\nrequires_openai_auth=false\n{extra}")).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_zenpi"));
        command
            .args(["--headless", "--session", "cli.jsonl"])
            .current_dir(root)
            .env("ZENPI_HOME", root.join("user"));
        for key in [
            "ZENPI_MODEL",
            "OPENAI_MODEL",
            "ZENPI_PROFILE",
            "ZENPI_BASE_URL",
            "OPENAI_BASE_URL",
            "ZENPI_API_KEY",
            "OPENAI_API_KEY",
            "ZENPI_WIRE_API",
            "ZENPI_BACKEND",
            "ZENPI_PROVIDER",
            "ZENPI_MODEL_REASONING_EFFORT",
            "ZENPI_MODEL_VERBOSITY",
        ] {
            command.env_remove(key);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let value = serde_json::from_str(&line.unwrap()).unwrap();
                if send.send(value).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            lines,
            reader: Some(reader),
        }
    }
    fn command(&mut self, id: &str, text: &str) -> Value {
        self.request(json!({"type":if text.starts_with('/') {"command"} else {"prompt"},"id":id,"text":text}))
    }
    fn request(&mut self, payload: Value) -> Value {
        let id = payload["id"].as_str().unwrap();
        writeln!(self.input, "{payload}").unwrap();
        self.input.flush().unwrap();
        loop {
            let value = self.lines.recv_timeout(Duration::from_secs(15)).unwrap();
            if value["id"] == id && value.get("success").is_some() {
                return value;
            }
        }
    }
}
impl Drop for Cli {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[test]
fn actual_headless_unknown_is_open_and_models_query_is_local() {
    let dir = tempdir().unwrap();
    let (url, requests, server) = server(2, false);
    let mut cli = Cli::start(dir.path(), &url, "nonexistent-model", "", "chat");
    let query = cli.command("query", "/models");
    assert_eq!(query["success"], true, "{query}");
    let text = query.to_string();
    assert!(text.contains("32768"));
    assert!(requests.try_recv().is_err());
    // Provider openness: an uncatalogued model accepts images and opens HTTP
    // instead of being rejected as text-only.
    let accepted=cli.request(json!({"type":"prompt","id":"image","text":"image","attachments":[{"kind":"image","mime_type":"image/png","url":"https://example.test/image.png"}]}));
    assert_eq!(accepted["success"], true, "{accepted}");
    let _ = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    let response = cli.command("turn", "hello");
    assert_eq!(response["success"], true, "{response}");
    let sent = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(sent["model"], "nonexistent-model");
    assert_eq!(sent["stream"], true);
    assert!(sent["tools"].as_array().is_some_and(|tools| !tools.is_empty()));
    assert_eq!(sent["max_completion_tokens"], 4096);
    server.join().unwrap();
}

#[test]
fn actual_headless_override_restores_tools_and_survives_model_switch_restart() {
    let dir = tempdir().unwrap();
    let (url, requests, server) = server(2, false);
    let extra = "[[model_overrides]]\nprovider='openai'\nid='custom-model'\nversion='fixture-v1'\ncontext_window=16000\nmax_output_tokens=2000\ntools=true\nstreaming=true\nimages=true\n";
    let mut cli = Cli::start(dir.path(), &url, "custom-model", extra, "chat");
    let first = cli.command("turn", "hello");
    assert_eq!(first["success"], true, "{first}");
    let sent = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        sent["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty())
    );
    assert_eq!(sent["stream"], true);
    assert_eq!(sent["max_completion_tokens"], 2000);
    assert_eq!(cli.command("switch", "/model gpt-4.1")["success"], true);
    let selected = cli.command("query", "/models");
    drop(cli);
    let mut cli = Cli::start(dir.path(), &url, "custom-model", extra, "chat");
    let restored = cli.command("query", "/models");
    assert_eq!(
        selected["data"]["selection"]["active"]["id"], "gpt-4.1",
        "{selected}"
    );
    assert_eq!(selected["data"]["selection"], restored["data"]["selection"]);
    let response = cli.command("turn2", "next");
    assert_eq!(response["success"], true, "{response}");
    assert_eq!(
        requests.recv_timeout(Duration::from_secs(5)).unwrap()["model"],
        "gpt-4.1"
    );
    server.join().unwrap();
}

#[test]
fn actual_headless_failed_reasoning_switch_keeps_selection_and_responses_attachments_work() {
    let dir = tempdir().unwrap();
    let (url, requests, server) = server(1, true);
    let mut cli = Cli::start(
        dir.path(),
        &url,
        "gpt-5.2",
        "model_reasoning_effort='high'\n",
        "responses",
    );
    let before = cli.command("before", "/models");
    assert_eq!(before["success"], true, "{before}");
    let failed = cli.command("failed", "/model gpt-4.1");
    assert_eq!(failed["success"], false, "{failed}");
    let after = cli.command("after", "/models");
    assert_eq!(before["data"]["selection"], after["data"]["selection"]);
    assert!(requests.try_recv().is_err());
    let reply = cli.request(
        json!({"type":"prompt","id":"files","text":"describe","attachments":[
            {"kind":"image","mime_type":"image/png","url":"https://example.test/image.png"},
            {"kind":"file","mime_type":"application/pdf","file_id":"file-fixture"}
        ]}),
    );
    assert_eq!(reply["success"], true, "{reply}");
    let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(request["model"], "gpt-5.2");
    assert_eq!(request["reasoning"]["effort"], "high");
    assert!(
        request.to_string().contains("input_image") && request.to_string().contains("input_file")
    );
    server.join().unwrap();
}

#[test]
fn local_model_query_during_an_admitted_turn_does_not_change_cancel_or_session_state() {
    let dir = tempdir().unwrap();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("s")).unwrap(),
        Box::new(strict(
            "http://127.0.0.1:9",
            "gpt-4.1",
            OpenAiWireApi::Responses,
            &[],
        )),
    );
    agent.submit(TurnInputRequest::new("pending")).unwrap();
    let phase = agent.phase();
    let before = fs::read(dir.path().join("s")).unwrap();
    assert!(
        agent.model_status().unwrap()["registry_bound"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(phase, agent.phase());
    assert_eq!(before, fs::read(dir.path().join("s")).unwrap());
    assert!(agent.set_model(Some("other".into())).is_err());
}

#[test]
fn incomplete_json_or_stream_never_executes_a_complete_looking_tool_call() {
    for case in 0..4 {
        let dir = tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            let call = json!({"id":"write","type":"function","function":{"name":"write_file","arguments":"{\"path\":\"must-not-exist\",\"content\":\"bad\"}"}});
            let response_call = json!({"type":"function_call","call_id":"write","name":"write_file","arguments":"{\"path\":\"must-not-exist\",\"content\":\"bad\"}"});
            let (mime, body) = match case {
                0 => (
                    "application/json",
                    json!({"choices":[{"message":{"tool_calls":[call]},"finish_reason":"length"}]})
                        .to_string(),
                ),
                1 => (
                    "application/json",
                    json!({"status":"incomplete","output":[response_call]}).to_string(),
                ),
                2 => (
                    "text/event-stream",
                    format!(
                        "data: {}\n\ndata: {}\n\n",
                        json!({"type":"response.output_item.done","item":response_call}),
                        json!({"type":"response.completed","response":{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"}}})
                    ),
                ),
                _ => (
                    "application/json",
                    json!({"output":[response_call]}).to_string(),
                ),
            };
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            stream.flush().unwrap();
        });
        let wire = if case == 0 {
            OpenAiWireApi::ChatCompletions
        } else {
            OpenAiWireApi::Responses
        };
        let mut agent = Agent::new(
            SessionStore::open(dir.path().join("session.jsonl")).unwrap(),
            Box::new(strict(&url, "gpt-4.1", wire, &[])),
        );
        agent.set_tools(
            zenpi::tools::ToolRegistry::with_all_builtins().unwrap(),
            zenpi::tools::ToolContext::new(dir.path()).unwrap(),
            zenpi::tools::SideEffectPolicy::all_builtins(),
        );
        assert!(
            agent.process(TurnInputRequest::new("write")).is_err(),
            "case {case}"
        );
        assert!(!dir.path().join("must-not-exist").exists());
        assert!(
            !agent
                .history()
                .iter()
                .any(|turn| turn.role == TurnRole::Tool)
        );
        handle.join().unwrap();
    }
}

#[test]
fn text_only_model_cannot_execute_unsolicited_tools_from_either_wire() {
    for wire in [OpenAiWireApi::ChatCompletions, OpenAiWireApi::Responses] {
        let dir = tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            let request = read_request(&mut stream);
            assert!(request.get("tools").is_none());
            assert_eq!(request["stream"], false);
            let args = json!({"path":"must-not-exist","content":"unrequested"}).to_string();
            let response = match wire {
                OpenAiWireApi::GoogleGenerativeAi | OpenAiWireApi::AnthropicMessages => {
                    unreachable!("this fixture iterates only the two OpenAI wires")
                }
                OpenAiWireApi::ChatCompletions => {
                    json!({"choices":[{"message":{"tool_calls":[{"id":"unsolicited","type":"function","function":{"name":"write_file","arguments":args}}]},"finish_reason":"tool_calls"}]})
                }
                OpenAiWireApi::Responses => {
                    json!({"status":"completed","output":[{"type":"function_call","call_id":"unsolicited","name":"write_file","arguments":args}]})
                }
            };
            let body = response.to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            stream.flush().unwrap();
        });
        let text_only = [override_entry(json!({
            "provider":"openai","id":"unlisted","version":"fixture-v1",
            "tools":false,"streaming":false
        }))];
        let mut agent = Agent::new(
            SessionStore::open(dir.path().join("session.jsonl")).unwrap(),
            Box::new(strict(&url, "unlisted", wire, &text_only)),
        );
        agent.set_tools(
            zenpi::tools::ToolRegistry::with_all_builtins().unwrap(),
            zenpi::tools::ToolContext::new(dir.path()).unwrap(),
            zenpi::tools::SideEffectPolicy::all_builtins(),
        );
        let error = agent
            .process(TurnInputRequest::new("plain text only"))
            .unwrap_err();
        assert!(
            error.to_string().contains("unsupported tool call"),
            "{error}"
        );
        assert!(!dir.path().join("must-not-exist").exists());
        assert!(
            !agent
                .history()
                .iter()
                .any(|turn| turn.role == TurnRole::Tool)
        );
        handle.join().unwrap();
    }
}

#[test]
fn complete_environment_configuration_does_not_borrow_ambient_codex_effort() {
    let dir = tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_zenpi"));
    for (name, _) in std::env::vars() {
        if name.starts_with("ZENPI_") || name.starts_with("OPENAI_") {
            command.env_remove(name);
        }
    }
    let mut child = command
        .args(["--headless", "--session", "environment.jsonl"])
        .current_dir(dir.path())
        .env("ZENPI_HOME", dir.path().join("user"))
        .env("ZENPI_BACKEND", "openai")
        .env("ZENPI_PROVIDER", "openai")
        .env("ZENPI_MODEL", "environment-only-model")
        .env("ZENPI_BASE_URL", "http://127.0.0.1:9/v1")
        .env("ZENPI_WIRE_API", "chat")
        .env("ZENPI_API_KEY", "registry-local-environment-fixture")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "{}",
        json!({"type":"shutdown","id":"stop"})
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"success\":true"));
}
