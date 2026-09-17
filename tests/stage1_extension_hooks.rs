#![cfg(unix)]
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use tempfile::tempdir;
use zenpi::{
    approval::{ApprovalDecision, ApprovalMode, ApprovalPolicy, ApprovalResponse},
    backend::{OpenAiCompatibleBackend, OpenAiWireApi},
    core::{Agent, TurnInputRequest},
    extension_runtime::ExtensionRuntime,
    extensions::ExtensionCatalog,
    session::SessionStore,
    tools::{SideEffectPolicy, ToolCall, ToolContext, ToolRegistry},
};

fn extension(root: &Path, name: &str, hooks: &[&str], mode: &str) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("mode"), mode).unwrap();
    fs::write(
        dir.join("extension.toml"),
        format!(
            r#"name = "{name}"
version = "1.0.0"
api_version = 2
executable = "plugin.sh"
hooks = {}
hook_timeout_ms = 1000
[permissions]
workspace_read = true
[[tools]]
name = "{name}_echo"
description = "Extension echo"
side_effect = "read_only"
input_schema = {{ type = "object" }}
"#,
            serde_json::to_string(hooks).unwrap()
        ),
    )
    .unwrap();
    // Spawn this already compiled test executable through a real shell/process
    // boundary. FD 3 is the extension protocol pipe; libtest stdout goes to
    // /dev/null so harness progress can never contaminate the JSON reply.
    let executable = std::env::current_exe().unwrap();
    let quoted = executable.to_str().unwrap().replace('\'', "'\"'\"'");
    fs::write(
        dir.join("plugin.sh"),
        format!(
            "#!/bin/sh\nexec '{quoted}' --ignored --exact extension_fixture_process --nocapture --test-threads=1 3>&1 1>/dev/null\n"
        ),
    )
    .unwrap();
    fs::set_permissions(dir.join("plugin.sh"), fs::Permissions::from_mode(0o700)).unwrap();
}

/// Real subprocess fixture selected only by the extension launcher above.
/// The host still owns actual pipes, process groups, deadlines and cancellation.
#[test]
#[ignore = "subprocess helper invoked by the 17 extension owner tests"]
fn extension_fixture_process() {
    use std::io::BufRead;
    use std::os::fd::FromRawFd;

    let name = std::env::var("ZENPI_EXTENSION_NAME").expect("extension child environment");
    // SAFETY: F_GETFD only inspects the inherited descriptor. The launcher
    // duplicates the extension stdout onto FD 3 before redirecting FD 1.
    assert_ne!(unsafe { libc::fcntl(3, libc::F_GETFD) }, -1);
    // SAFETY: FD 3 is the valid, uniquely owned protocol descriptor established
    // by the launcher. No other File/OwnedFd in this helper owns that descriptor.
    let mut output = unsafe { fs::File::from_raw_fd(3) };
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).unwrap();
    let request: Value = serde_json::from_str(&line).unwrap();
    let params = &request["params"];
    let method = request["method"].as_str().unwrap();
    let mode = fs::read_to_string("mode").unwrap();
    let hook = params["hook"].as_str().unwrap_or(method);
    let mut journal = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("events")
        .unwrap();
    writeln!(
        journal,
        "{}",
        json!({"hook":hook,"pid":std::process::id(),"capability":request["capability"],"event":params.get("event")})
    )
    .unwrap();
    journal.flush().unwrap();

    let result = match method {
        "initialize" => json!({
            "api_version":if mode == "bad_init" { 90 } else { 2 },
            "hooks":params["hooks"],
            "tools":params["tools"].as_array().unwrap().iter().map(|tool| tool["name"].clone()).collect::<Vec<_>>()
        }),
        "tools/call" => json!({"echo":params["arguments"]}),
        _ => {
            assert!(
                !(mode == "isolated_failures" && hook == "agent_start"),
                "isolated lifecycle error"
            );
            let mut result = match hook {
                "input" => {
                    json!({"action":"transform","text":format!("{}/{name}",params["event"]["text"].as_str().unwrap())})
                }
                "context" => {
                    json!({"action":"context","instructions":format!("{} CTX_{name}",params["event"]["instructions"].as_str().unwrap())})
                }
                "before_tool" => match mode.as_str() {
                    "rewrite" => json!({"action":"rewrite","arguments":{"path":"changed.txt"}}),
                    "write" => {
                        json!({"action":"rewrite","arguments":{"path":"approved.txt","content":"changed"}})
                    }
                    "bad_schema" => json!({"action":"rewrite","arguments":{"path":42}}),
                    "escape" => json!({"action":"rewrite","arguments":{"path":"../outside.txt"}}),
                    "deny" => json!({"action":"deny","reason":"extension veto"}),
                    "unknown" => json!({"action":"continue","allow":true}),
                    "throw" => panic!("fixture failure"),
                    "huge" => json!({"action":"rewrite","arguments":{"text":"x".repeat(70000)}}),
                    _ => json!({"action":"continue"}),
                },
                "after_tool" if mode == "isolated_failures" => {
                    json!({"action":"output","output":"x".repeat(70000)})
                }
                "after_tool" => {
                    json!({"action":"output","output":{"hook_output":name,"prior":params["event"]["output"]}})
                }
                _ => json!({"action":"continue"}),
            };
            if matches!(hook, "input" | "before_tool")
                && matches!(mode.as_str(), "hang" | "line_hang")
            {
                fs::write("running", std::process::id().to_string()).unwrap();
                if mode == "line_hang" {
                    writeln!(
                        output,
                        "{}",
                        json!({"jsonrpc":"2.0","id":request["id"],"capability":request["capability"],"result":result})
                    )
                    .unwrap();
                    output.flush().unwrap();
                }
                thread::sleep(Duration::from_secs(30));
            }
            if hook == "input" {
                match mode.as_str() {
                    "wrong_action" => result = json!({"action":"rewrite","arguments":{}}),
                    "huge_input" => result = json!({"action":"transform","text":"x".repeat(70000)}),
                    _ => {}
                }
            }
            result
        }
    };
    let mut reply = json!({"jsonrpc":"2.0","id":request["id"],"capability":request["capability"],"result":result});
    if method != "initialize" {
        if mode == "stale" {
            reply["capability"]["generation"] =
                json!(reply["capability"]["generation"].as_u64().unwrap() - 1);
        }
        if mode == "wrong_id" {
            reply["id"] = json!(reply["id"].as_u64().unwrap() - 1);
        }
    }
    let mut encoded = serde_json::to_string(&reply).unwrap();
    if method != "initialize" && mode == "duplicate" {
        assert!(encoded.contains("\"action\":\"continue\""));
        encoded = encoded.replace(
            "\"action\":\"continue\"",
            "\"action\":\"deny\",\"action\":\"continue\"",
        );
    }
    writeln!(output, "{encoded}").unwrap();
    output.flush().unwrap();
}
fn events(root: &Path, name: &str) -> Vec<Value> {
    fs::read_to_string(root.join(name).join("events"))
        .unwrap_or_default()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}
fn read_request(stream: &mut TcpStream) -> Value {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut bytes = Vec::new();
    let end = loop {
        let mut buf = [0; 4096];
        let n = stream.read(&mut buf).unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buf[..n]);
        if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        assert!(bytes.len() < 65536);
    };
    let len = String::from_utf8_lossy(&bytes[..end])
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|s| s.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    while bytes.len() < end + len {
        let mut buf = [0; 4096];
        let n = stream.read(&mut buf).unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buf[..n]);
    }
    serde_json::from_slice(&bytes[end..end + len]).unwrap()
}
fn server(
    calls: Vec<Option<ToolCall>>,
) -> (String, Arc<Mutex<Vec<Value>>>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let handle = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        for call in calls {
            let start = Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && start.elapsed() < Duration::from_secs(12) =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("accept {e}"),
                }
            };
            let request = read_request(&mut stream);
            captured.lock().unwrap().push(request);
            let delta = match &call {
                Some(c) => {
                    json!({"tool_calls":[{"index":0,"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments.to_string()}}]})
                }
                None => json!({"content":"done"}),
            };
            let first = json!({"id":"reply","model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
            let last = json!({"id":"reply","model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":if call.is_some(){"tool_calls"}else{"stop"}}]});
            let body = format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n");
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    (url, requests, handle)
}
fn call(name: &str, args: Value) -> Option<ToolCall> {
    Some(ToolCall {
        id: "call-1".into(),
        name: name.into(),
        arguments: args,
    })
}
fn agent(root: &Path, url: String) -> Agent {
    let backend = OpenAiCompatibleBackend::from_values_with_wire_api(
        url,
        Some("fixture-key".into()),
        "fixture-model".into(),
        OpenAiWireApi::ChatCompletions,
    )
    .unwrap();
    let mut agent = Agent::new(
        SessionStore::open_in_workspace(root.join("session.jsonl"), root).unwrap(),
        Box::new(backend),
    );
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(root).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    agent.set_approval_policy(ApprovalPolicy {
        mode: ApprovalMode::Never,
        ..Default::default()
    });
    agent
}
fn echo_agent(root: &Path) -> Agent {
    let mut a =
        Agent::with_echo(SessionStore::open_in_workspace(root.join("echo.jsonl"), root).unwrap());
    a.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(root).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    a
}
/// Each owner below spawns real fixture subprocesses, CLI processes and HTTP
/// servers. libtest would otherwise start twenty of them at once and starve the
/// manifest's fixed 1000 ms hook deadline on a loaded host, producing spurious
/// `CommandTimeout` instead of exercising the behavior under test. The guard
/// admits one owner workload at a time; the cases themselves are unchanged.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn real_http_owner_chains_input_context_tools_and_exact_lifecycle() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("extensions");
    let hooks = [
        "input",
        "context",
        "before_tool",
        "after_tool",
        "session_start",
        "agent_start",
        "agent_end",
        "session_close",
    ];
    extension(&ext, "z", &hooks, "normal");
    extension(&ext, "a", &hooks, "rewrite");
    fs::write(root.join("original.txt"), "ORIGINAL").unwrap();
    fs::write(root.join("changed.txt"), "CHANGED").unwrap();
    let (url, requests, handle) = server(vec![
        call("read_file", json!({"path":"original.txt"})),
        None,
    ]);
    let mut a = agent(root, url);
    a.configure_extensions(&ext, || false).unwrap();
    let lease = a.extension_lease().unwrap();
    a.process_sync("hello").unwrap();
    a.try_close().unwrap();
    a.try_close().unwrap();
    handle.join().unwrap();
    assert!(!lease.is_active());
    let requests = requests.lock().unwrap();
    assert!(requests[0].to_string().contains("hello/a/z"));
    assert!(requests[0].to_string().contains("CTX_a CTX_z"));
    assert!(requests[1].to_string().contains("CHANGED"));
    assert!(requests[1].to_string().contains("hook_output"));
    assert_eq!(
        a.session()
            .turns()
            .iter()
            .find(|t| t.content == "hello")
            .unwrap()
            .content,
        "hello"
    );
    for name in ["a", "z"] {
        let ev = events(&ext, name);
        let kinds: Vec<_> = ev.iter().map(|e| e["hook"].as_str().unwrap()).collect();
        assert_eq!(
            kinds,
            vec![
                "initialize",
                "session_start",
                "agent_start",
                "input",
                "context",
                "before_tool",
                "after_tool",
                "context",
                "agent_end",
                "session_close"
            ]
        );
    }
    assert!(
        a.session()
            .events()
            .iter()
            .any(|e| e["type"] == "extension_tool_rewrite")
    );
}

#[test]
fn before_tool_failures_never_spawn_requested_command() {
    let _serial = serial();
    for mode in [
        "deny",
        "unknown",
        "duplicate",
        "throw",
        "huge",
        "hang",
        "line_hang",
        "stale",
        "wrong_id",
    ] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let ext = root.join("extensions");
        extension(&ext, "a", &["before_tool"], mode);
        let (url, requests, handle) = server(vec![
            call(
                "run_command",
                json!({"command":"printf bad > forbidden.txt"}),
            ),
            None,
        ]);
        let mut a = agent(root, url);
        a.configure_extensions(&ext, || false).unwrap();
        a.process_sync("run").unwrap();
        handle.join().unwrap();
        assert!(!root.join("forbidden.txt").exists(), "{mode}");
        assert!(
            requests.lock().unwrap()[1].to_string().contains("error"),
            "{mode}"
        );
        if let Ok(pid) = fs::read_to_string(ext.join("a/running")) {
            assert_eq!(
                unsafe { libc::kill(pid.parse().unwrap(), 0) },
                -1,
                "child must be reaped: {mode}"
            );
        }
    }
}

#[test]
fn rewritten_schema_path_and_original_deny_are_checked_before_dispatch() {
    let _serial = serial();
    for mode in ["bad_schema", "escape"] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("original.txt"), "safe").unwrap();
        let ext = root.join("extensions");
        extension(&ext, "a", &["before_tool"], mode);
        let (url, requests, h) = server(vec![
            call("read_file", json!({"path":"original.txt"})),
            None,
        ]);
        let mut a = agent(root, url);
        a.configure_extensions(&ext, || false).unwrap();
        a.process_sync("read").unwrap();
        h.join().unwrap();
        assert!(requests.lock().unwrap()[1].to_string().contains("error"));
    }
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("extensions");
    extension(&ext, "a", &["before_tool"], "normal");
    let (url, _, h) = server(vec![
        call("run_command", json!({"command":"touch forbidden.txt"})),
        None,
    ]);
    let mut a = agent(root, url);
    let mut policy = ApprovalPolicy {
        mode: ApprovalMode::Never,
        ..Default::default()
    };
    policy
        .per_tool
        .insert("run_command".into(), ApprovalDecision::Deny);
    a.set_approval_policy(policy);
    a.configure_extensions(&ext, || false).unwrap();
    a.process_sync("run").unwrap();
    h.join().unwrap();
    assert!(!root.join("forbidden.txt").exists());
    assert!(events(&ext, "a").iter().all(|e| e["hook"] != "before_tool"));
}

#[test]
fn rewritten_arguments_are_the_exact_human_approval_payload() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("extensions");
    extension(&ext, "a", &["before_tool"], "write");
    let (url, _, h) = server(vec![
        call(
            "write_file",
            json!({"path":"original.txt","content":"original"}),
        ),
        None,
    ]);
    let mut a = agent(root, url);
    let coordinator = a
        .set_approval_policy(ApprovalPolicy {
            mode: ApprovalMode::Always,
            ..Default::default()
        })
        .unwrap();
    a.configure_extensions(&ext, || false).unwrap();
    let worker = thread::spawn(move || {
        a.process_sync("write").unwrap();
        a
    });
    let start = Instant::now();
    let request = loop {
        if let Some(r) = coordinator.drain_pending().pop() {
            break r;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(
        request.arguments,
        json!({"path":"approved.txt","content":"changed"})
    );
    assert!(!root.join("approved.txt").exists());
    coordinator
        .respond(ApprovalResponse {
            request_id: request.request_id,
            decision: ApprovalDecision::Allow,
            remember: false,
        })
        .unwrap();
    let _a = worker.join().unwrap();
    h.join().unwrap();
    assert_eq!(
        fs::read_to_string(root.join("approved.txt")).unwrap(),
        "changed"
    );
    assert!(!root.join("original.txt").exists());
}

#[test]
fn failed_reload_keeps_tools_and_lease_successful_reload_revokes_old() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("extensions");
    extension(&ext, "a", &["input", "session_close"], "normal");
    let mut a = echo_agent(root);
    a.configure_extensions(&ext, || false).unwrap();
    let old = a.extension_lease().unwrap();
    let sequence = a.session().next_sequence();
    fs::write(ext.join("a/mode"), "bad_init").unwrap();
    assert!(a.configure_extensions(&ext, || false).is_err());
    assert!(old.is_active());
    assert_eq!(sequence, a.session().next_sequence());
    fs::write(ext.join("a/mode"), "normal").unwrap();
    a.process_sync("still active").unwrap();
    assert!(a.configure_extensions(&ext, || true).is_err());
    assert!(old.is_active());
    a.configure_extensions(&ext, || false).unwrap();
    assert!(!old.is_active());
    assert_ne!(a.extension_lease().unwrap().identity(), old.identity());
    let ev = events(&ext, "a");
    assert_eq!(
        ev.iter().filter(|e| e["hook"] == "session_close").count(),
        1
    );
}

#[test]
fn cancellation_kills_and_reaps_hook_and_next_turn_gets_fresh_process() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("extensions");
    extension(&ext, "a", &["input", "agent_start", "agent_end"], "hang");
    let mut a = echo_agent(root);
    a.configure_extensions(&ext, || false).unwrap();
    a.submit(TurnInputRequest::new("cancel me")).unwrap();
    let running = ext.join("a/running");
    let start = Instant::now();
    let error = a
        .run_active_turn_cancelable(|| {
            fs::read_to_string(&running).is_ok_and(|pid| pid.trim().parse::<i32>().is_ok())
        })
        .unwrap_err();
    assert_eq!(error.code(), "backend_cancelled");
    assert!(start.elapsed() < Duration::from_secs(2));
    let pid: i32 = fs::read_to_string(&running).unwrap().parse().unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    fs::write(ext.join("a/mode"), "normal").unwrap();
    a.process_sync("next").unwrap();
    let ev = events(&ext, "a");
    assert_eq!(ev.iter().filter(|e| e["hook"] == "agent_start").count(), 2);
    assert_eq!(ev.iter().filter(|e| e["hook"] == "agent_end").count(), 2);
}

#[test]
fn malformed_input_and_stale_capability_cannot_reach_provider() {
    let _serial = serial();
    for mode in ["wrong_action", "huge_input", "stale", "wrong_id"] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let ext = root.join("extensions");
        extension(&ext, "a", &["input"], mode);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut a = agent(
            root,
            format!("http://{}/v1", listener.local_addr().unwrap()),
        );
        a.configure_extensions(&ext, || false).unwrap();
        assert!(a.process_sync("fail").is_err(), "{mode}");
        assert!(listener.accept().is_err());
    }
}

#[test]
fn resume_and_process_restart_never_reactivate_old_capability() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("extensions");
    extension(
        &ext,
        "a",
        &["input", "session_start", "session_close"],
        "normal",
    );
    let mut a = echo_agent(root);
    a.configure_extensions(&ext, || false).unwrap();
    let first = a.extension_lease().unwrap();
    let target = root.join("replacement.jsonl");
    drop(SessionStore::open_in_workspace(&target, root).unwrap());
    a.resume_session(&target).unwrap();
    assert!(!first.is_active());
    let resumed = a.extension_lease().unwrap();
    assert_ne!(first.identity().session_id, resumed.identity().session_id);
    drop(a);
    assert!(!resumed.is_active());
    let mut b = echo_agent(root);
    b.configure_extensions(&ext, || false).unwrap();
    assert_ne!(
        first.identity().capability,
        b.extension_lease().unwrap().identity().capability
    );
    assert!(!first.is_active());
    assert_eq!(
        events(&ext, "a")
            .iter()
            .filter(|e| e["hook"] == "session_close")
            .count(),
        2
    );
}

#[test]
fn api_two_requires_session_negotiation_and_duplicate_reload_is_atomic() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("extensions");
    extension(&ext, "a", &["input"], "normal");
    let catalog = ExtensionCatalog::load(&ext).unwrap();
    assert!(catalog.register_tools(&mut ToolRegistry::new()).is_err());
    let mut a = echo_agent(root);
    a.configure_extensions(&ext, || false).unwrap();
    let old = a.extension_lease().unwrap();
    extension(&ext, "b", &["input"], "normal");
    let manifest = ext.join("b/extension.toml");
    let text = fs::read_to_string(&manifest)
        .unwrap()
        .replace("b_echo", "a_echo");
    fs::write(manifest, text).unwrap();
    assert!(a.configure_extensions(&ext, || false).is_err());
    assert!(old.is_active());
    let runtime = ExtensionRuntime::prepare(&ext, root, "other-session", &|| true);
    assert!(runtime.is_err());
}

struct Cli {
    child: std::process::Child,
    input: std::process::ChildStdin,
    lines: std::sync::mpsc::Receiver<Value>,
    reader: Option<thread::JoinHandle<()>>,
}
impl Cli {
    fn start(root: &Path, url: &str) -> Self {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        fs::create_dir_all(root.join("user")).unwrap();
        fs::write(root.join("user/config.toml"),format!("model='fixture-model'\nbase_url='{url}'\nwire_api='chat'\nrequires_openai_auth=false\n")).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_zenpi"))
            .args(["--headless", "--session", "cli.jsonl"])
            .current_dir(root)
            .env("ZENPI_HOME", root.join("user"))
            .env("ZENPI_BASE_URL", url)
            .env("ZENPI_WIRE_API", "chat")
            .env("ZENPI_API_KEY", "fixture-key")
            .env_remove("ZENPI_PROFILE")
            .env_remove("ZENPI_MODEL")
            .env_remove("OPENAI_MODEL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = std::sync::mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.unwrap();
                if tx.send(serde_json::from_str(&line).unwrap()).is_err() {
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
    fn wait_exit(&mut self) {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn send(&mut self, value: Value) {
        writeln!(self.input, "{value}").unwrap();
        self.input.flush().unwrap();
    }
    fn response(&self, id: &str) -> Value {
        loop {
            let value = self.lines.recv_timeout(Duration::from_secs(12)).unwrap();
            if value["id"] == id && value["type"] == "response" {
                return value;
            }
        }
    }
    fn request(&mut self, value: Value) -> Value {
        let id = value["id"].as_str().unwrap().to_owned();
        self.send(value);
        self.response(&id)
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
fn production_jsonl_reload_and_new_process_lease_affect_real_requests() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("user/extensions");
    extension(
        &ext,
        "a",
        &["input", "session_start", "session_close"],
        "normal",
    );
    let (url, requests, h) = server(vec![None, None, None]);
    let mut cli = Cli::start(root, &url);
    assert_eq!(
        cli.request(json!({"type":"prompt","id":"first","text":"production"}))["success"],
        true
    );
    let first = events(&ext, "a")[0]["capability"].clone();
    fs::write(ext.join("a/mode"), "bad_init").unwrap();
    assert_eq!(
        cli.request(json!({"type":"command","id":"badreload","text":"/reload"}))["success"],
        false
    );
    fs::write(ext.join("a/mode"), "normal").unwrap();
    assert_eq!(
        cli.request(json!({"type":"command","id":"reload","text":"/reload"}))["success"],
        true
    );
    assert_eq!(
        cli.request(json!({"type":"prompt","id":"second","text":"reloaded"}))["success"],
        true
    );
    assert_eq!(
        cli.request(json!({"type":"shutdown","id":"stop"}))["success"],
        true
    );
    cli.wait_exit();
    drop(cli);
    let mut restarted = Cli::start(root, &url);
    assert_eq!(
        restarted.request(json!({"type":"prompt","id":"third","text":"restarted"}))["success"],
        true
    );
    assert_eq!(
        restarted.request(json!({"type":"shutdown","id":"stop2"}))["success"],
        true
    );
    restarted.wait_exit();
    h.join().unwrap();
    let r = requests.lock().unwrap();
    assert!(r[0].to_string().contains("production/a"));
    assert!(r[1].to_string().contains("reloaded/a"));
    assert!(r[2].to_string().contains("restarted/a"));
    let ev = events(&ext, "a");
    let last = ev.last().unwrap();
    assert_ne!(first["capability"], last["capability"]["capability"]);
    assert_eq!(
        ev.iter().filter(|e| e["hook"] == "session_close").count(),
        3
    );
}

#[test]
fn revoked_inflight_lease_reaps_child_before_any_provider_request() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("extensions");
    extension(&ext, "a", &["input"], "hang");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut a = agent(
        root,
        format!("http://{}/v1", listener.local_addr().unwrap()),
    );
    a.configure_extensions(&ext, || false).unwrap();
    let old = a.extension_lease().unwrap();
    let running = ext.join("a/running");
    a.submit(TurnInputRequest::new("old scope")).unwrap();
    assert!(
        a.run_active_turn_cancelable(|| {
            if fs::read_to_string(&running).is_ok_and(|pid| pid.trim().parse::<i32>().is_ok()) {
                old.revoke();
            }
            false
        })
        .is_err()
    );
    assert!(!old.is_active());
    assert!(listener.accept().is_err());
    let pid: i32 = fs::read_to_string(running).unwrap().parse().unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    fs::write(ext.join("a/mode"), "normal").unwrap();
    a.configure_extensions(&ext, || false).unwrap();
    assert!(a.extension_lease().unwrap().is_active());
    assert!(!old.is_active());
}

#[test]
fn legacy_tool_cancellation_covers_blocked_stdin_and_reaps_process() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("extensions");
    extension(&ext, "a", &[], "normal");
    let manifest = ext.join("a/extension.toml");
    let s = fs::read_to_string(&manifest)
        .unwrap()
        .replace("api_version = 2", "api_version = 1");
    fs::write(manifest, s).unwrap();
    fs::write(
        ext.join("a/plugin.sh"),
        "#!/bin/sh\necho $$ > running\nsleep 30\n",
    )
    .unwrap();
    let mut registry = ToolRegistry::new();
    ExtensionCatalog::load(&ext)
        .unwrap()
        .register_tools(&mut registry)
        .unwrap();
    let running = ext.join("a/running");
    let start = Instant::now();
    let result = registry.execute_cancellable(
        &ToolContext::new(root).unwrap(),
        SideEffectPolicy::read_only(),
        ToolCall {
            id: "legacy".into(),
            name: "a_echo".into(),
            arguments: json!({"text":"x".repeat(60000)}),
        },
        &|| fs::read_to_string(&running).is_ok_and(|pid| pid.trim().parse::<i32>().is_ok()),
    );
    assert!(!result.is_success());
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(
        serde_json::to_string(&result)
            .unwrap()
            .contains("cancelled")
    );
    let pid: i32 = fs::read_to_string(running).unwrap().trim().parse().unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
}

#[test]
fn production_cancel_reaps_input_hook_while_jsonl_owner_remains_responsive() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("user/extensions");
    extension(&ext, "a", &["input", "agent_end"], "hang");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut cli = Cli::start(
        root,
        &format!("http://{}/v1", listener.local_addr().unwrap()),
    );
    cli.send(json!({"type":"prompt","id":"blocked","text":"cancel plugin"}));
    let running = ext.join("a/running");
    let start = Instant::now();
    let pid = loop {
        if let Ok(pid) = fs::read_to_string(&running)
            && let Ok(pid) = pid.parse::<i32>()
        {
            break pid;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(5));
    };
    cli.send(json!({"schema_version":2,"type":"cancel","id":"cancel","target_id":"blocked"}));
    let stopped = cli.response("blocked");
    assert_eq!(stopped["success"], false);
    assert_eq!(stopped["code"], "backend_cancelled");
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert!(listener.accept().is_err());
    assert_eq!(
        cli.request(json!({"type":"status","id":"alive"}))["success"],
        true
    );
    assert_eq!(
        cli.request(json!({"type":"shutdown","id":"shutdown"}))["success"],
        true
    );
    cli.wait_exit();
}

#[test]
fn successful_api_two_tool_negotiation_runs_through_real_owner() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("extensions");
    extension(&ext, "a", &[], "normal");
    let (url, requests, h) = server(vec![call("a_echo", json!({"message":"negotiated"})), None]);
    let mut a = agent(root, url);
    a.configure_extensions(&ext, || false).unwrap();
    a.process_sync("extension tool").unwrap();
    h.join().unwrap();
    assert!(
        requests.lock().unwrap()[1]
            .to_string()
            .contains("negotiated")
    );
    assert!(events(&ext, "a").iter().any(|e| e["hook"] == "tools/call"));
}

#[test]
fn first_extension_deny_cannot_be_overridden_by_later_hook() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("extensions");
    extension(&ext, "a", &["before_tool"], "deny");
    extension(&ext, "z", &["before_tool"], "normal");
    let (url, _, h) = server(vec![
        call("run_command", json!({"command":"touch forbidden.txt"})),
        None,
    ]);
    let mut a = agent(root, url);
    a.configure_extensions(&ext, || false).unwrap();
    a.process_sync("do not run").unwrap();
    h.join().unwrap();
    assert!(!root.join("forbidden.txt").exists());
    assert!(events(&ext, "z").iter().all(|e| e["hook"] != "before_tool"));
}

#[test]
fn legacy_json_null_result_remains_valid_but_missing_result_is_rejected() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("extensions");
    extension(&ext, "a", &[], "normal");
    let manifest = ext.join("a/extension.toml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest)
            .unwrap()
            .replace("api_version = 2", "api_version = 1"),
    )
    .unwrap();
    for (result, success) in [(",\"result\":null", true), ("", false)] {
        fs::write(
            ext.join("a/plugin.sh"),
            format!(
                "#!/bin/sh\nread line\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1{result}}}'\n"
            ),
        )
        .unwrap();
        let mut registry = ToolRegistry::new();
        ExtensionCatalog::load(&ext)
            .unwrap()
            .register_tools(&mut registry)
            .unwrap();
        let output = registry.execute(
            &ToolContext::new(root).unwrap(),
            SideEffectPolicy::read_only(),
            ToolCall {
                id: "null".into(),
                name: "a_echo".into(),
                arguments: json!({}),
            },
        );
        assert_eq!(output.is_success(), success, "{output:?}");
    }
}

#[test]
fn lifecycle_and_after_tool_failures_preserve_valid_owner_state() {
    let _serial = serial();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ext = root.join("extensions");
    extension(
        &ext,
        "a",
        &["agent_start", "after_tool"],
        "isolated_failures",
    );
    extension(&ext, "z", &["agent_start", "agent_end"], "normal");
    fs::write(root.join("result.txt"), "ORIGINAL_RESULT").unwrap();
    let (url, requests, h) = server(vec![call("read_file", json!({"path":"result.txt"})), None]);
    let mut a = agent(root, url);
    a.configure_extensions(&ext, || false).unwrap();
    a.process_sync("isolate errors").unwrap();
    h.join().unwrap();
    let r = requests.lock().unwrap();
    assert!(r[1].to_string().contains("ORIGINAL_RESULT"));
    assert!(!r[1].to_string().contains(&"x".repeat(100)));
    let events = events(&ext, "z");
    assert!(events.iter().any(|e| e["hook"] == "agent_start"));
    assert!(events.iter().any(|e| e["hook"] == "agent_end"));
    assert!(
        a.take_events()
            .iter()
            .filter(|e| matches!(e, zenpi::core::AgentEvent::Error { .. }))
            .count()
            >= 2
    );
}
