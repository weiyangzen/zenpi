use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};
use tempfile::tempdir;
use zenpi::{
    backend::{OpenAiCompatibleBackend, OpenAiWireApi},
    core::{Agent, TurnInputRequest},
    resource_loader::{ResourcePaths, TextResourcePath},
    session::SessionStore,
    tools::{SideEffectPolicy, ToolContext, ToolRegistry},
};

fn put(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn fixtures(root: &Path, version: &str) {
    put(
        root,
        ".zenpi/skills/research/SKILL.md",
        &format!(
            "---\nname: research\ndescription: research metadata\n---\nSECRET_SKILL_BODY_{version}\nUse ./references/source.md"
        ),
    );
    put(
        root,
        ".zenpi/skills/research/references/source.md",
        "relative resource fixture",
    );
    put(
        root,
        ".zenpi/skills/manual/SKILL.md",
        "---\nname: manual\ndescription: explicit only\ndisable-model-invocation: true\n---\nMANUAL_BODY",
    );
    put(
        root,
        ".zenpi/prompts/fixture-review.md",
        &format!(
            "---\ndescription: review template\n---\nTEMPLATE_{version}: $1 | $ARGUMENTS | ${{2:-default}}"
        ),
    );
    put(root, "SYSTEM.md", &format!("RESOURCE_TEXT_{version}"));
}

fn paths(root: &Path) -> ResourcePaths {
    ResourcePaths {
        user_skills: root.join("empty-user-skills"),
        project_skills: root.join(".zenpi/skills"),
        skill_paths: vec![],
        user_templates: root.join("empty-user-prompts"),
        project_templates: root.join(".zenpi/prompts"),
        template_paths: vec![],
        text_resources: vec![TextResourcePath {
            name: "system".into(),
            path: root.join("SYSTEM.md"),
        }],
    }
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
    let headers = String::from_utf8_lossy(&bytes[..end]);
    assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
    let len: usize = headers
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

// Supports both the baseline nonstreaming Chat adapter and main's SSE adapter.
fn reply(stream: &mut TcpStream, request: &Value, tool: bool) {
    let message = if tool {
        json!({"role":"assistant", "content":null, "tool_calls":[{"id":"read-fixture", "type":"function", "function":{"name":"read_file", "arguments":"{\"path\":\"SYSTEM.md\"}"}}]})
    } else {
        json!({"role":"assistant","content":"provider-ok"})
    };
    let (content_type, body) = if request["stream"] == true {
        // Streaming tool deltas carry an index, unlike a completed message.
        let mut delta = message.clone();
        if let Some(calls) = delta.get_mut("tool_calls").and_then(Value::as_array_mut) {
            for (index, call) in calls.iter_mut().enumerate() {
                call["index"] = json!(index);
            }
        }
        let chunks = [
            json!({"id":"fixture", "model":"fixture-model", "choices":[{"index":0, "delta":delta, "finish_reason":null}]}),
            json!({"id":"fixture", "model":"fixture-model", "choices":[{"index":0, "delta":{}, "finish_reason":if tool {"tool_calls"} else {"stop"}}]}),
        ];
        (
            "text/event-stream",
            format!(
                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                chunks[0], chunks[1]
            ),
        )
    } else {
        ("application/json", json!({"id":"fixture", "model":"fixture-model", "choices":[{"index":0,"message":message,"finish_reason":if tool {"tool_calls"} else {"stop"}}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}}).to_string())
    };
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    stream.flush().unwrap();
}

fn server(
    count: usize,
    first_tool: bool,
) -> (String, Arc<Mutex<Vec<Value>>>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let capture = requests.clone();
    let handle = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        for index in 0..count {
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
                    Err(error) => panic!("fixture accept: {error}"),
                }
            };
            let request = read_request(&mut stream);
            capture.lock().unwrap().push(request.clone());
            reply(&mut stream, &request, first_tool && index == 0);
        }
    });
    (url, requests, handle)
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
        SideEffectPolicy::read_only(),
    );
    agent
}

#[test]
fn actual_provider_keeps_admitted_skill_and_text_generation_through_tool_loop() {
    let root = tempdir().unwrap();
    fixtures(root.path(), "OLD");
    let (url, captured, handle) = server(3, true);
    let mut agent = agent(root.path(), url);
    agent
        .configure_resources(paths(root.path()), || false)
        .unwrap();
    agent
        .submit(TurnInputRequest::new(
            "/skill:research literal $1 $(touch never-created)",
        ))
        .unwrap();
    fixtures(root.path(), "NEW");
    let reloaded = agent.reload_resources(|| false).unwrap();
    assert_eq!(reloaded["generation"], 2);
    assert_eq!(reloaded["active_generation"], 1);
    agent.run_active_turn().unwrap();
    agent
        .process(TurnInputRequest::new("/skill:research next"))
        .unwrap();
    handle.join().unwrap();
    let captured = captured.lock().unwrap();
    assert_eq!(captured.len(), 3);
    for request in &captured[..2] {
        let text = request.to_string();
        assert!(text.contains("SECRET_SKILL_BODY_OLD"));
        assert!(!text.contains("SECRET_SKILL_BODY_NEW"));
        assert!(text.contains("RESOURCE_TEXT_OLD"));
        assert!(text.contains("literal $1 $(touch never-created)"));
        assert!(text.contains("base_dir"));
    }
    assert!(captured[2].to_string().contains("SECRET_SKILL_BODY_NEW"));
    let journal = fs::read_to_string(root.path().join("session.jsonl")).unwrap();
    assert!(!journal.contains("SECRET_SKILL_BODY"));
    assert!(journal.contains("source_hash"));
    assert!(!root.path().join("never-created").exists());
}

#[test]
fn cancelled_or_failed_admission_reload_and_restart_preserve_resource_boundaries() {
    let root = tempdir().unwrap();
    fixtures(root.path(), "OLD");
    let (url, captured, handle) = server(2, false);
    let mut first = agent(root.path(), url.clone());
    first
        .configure_resources(paths(root.path()), || false)
        .unwrap();
    first
        .process(TurnInputRequest::new("/skill:manual hello"))
        .unwrap();
    let old = first.resource_snapshot();
    let before = fs::read(root.path().join("session.jsonl")).unwrap();
    assert!(
        first
            .submit_with_cancel(TurnInputRequest::new("/skill:research x"), || true)
            .is_err()
    );
    assert!(first.reload_resources(|| true).is_err());
    assert!(
        first
            .start_steer_reissue_with_cancel(
                "/skill:research cancelled".into(),
                "superseded",
                || true
            )
            .is_err()
    );
    put(
        root.path(),
        ".zenpi/prompts/broken.md",
        "---\ndescription: [bad]\n---\nbroken",
    );
    assert!(first.reload_resources(|| false).is_err());
    assert!(Arc::ptr_eq(&old, &first.resource_snapshot()));
    assert_eq!(fs::read(root.path().join("session.jsonl")).unwrap(), before);
    fs::remove_file(root.path().join(".zenpi/prompts/broken.md")).unwrap();
    first
        .process(TurnInputRequest::new("/skill:research old"))
        .unwrap();
    drop(first);
    fixtures(root.path(), "NEW");
    let mut reopened = agent(root.path(), url);
    reopened
        .restore_resources(paths(root.path()), || false)
        .unwrap();
    assert!(
        reopened
            .submit(TurnInputRequest::new("/skill:research should-fail"))
            .unwrap_err()
            .to_string()
            .contains("/reload required")
    );
    reopened.reload_resources(|| false).unwrap();
    assert!(
        reopened
            .submit(TurnInputRequest::new("/skill:research fresh"))
            .unwrap()
            .accepted()
    );
    handle.join().unwrap();
    assert_eq!(captured.lock().unwrap().len(), 2);
}

struct Cli {
    child: Child,
    input: ChildStdin,
    lines: mpsc::Receiver<Value>,
    reader: Option<thread::JoinHandle<()>>,
}
impl Cli {
    fn start(root: &Path, url: &str) -> Self {
        put(
            root,
            "user/config.toml",
            &format!(
                "model='fixture-model'\nbase_url='{url}'\nwire_api='chat'\nrequires_openai_auth=false\n"
            ),
        );
        put(
            root,
            "user/auth.json",
            "{\"OPENAI_API_KEY\":\"fixture-key\"}",
        );
        let mut child = Command::new(env!("CARGO_BIN_EXE_zenpi"))
            .args(["--headless", "--session", "cli.jsonl"])
            .current_dir(root)
            .env("ZENPI_HOME", root.join("user"))
            .env("ZENPI_BASE_URL", url)
            .env_remove("ZENPI_MODEL")
            .env_remove("OPENAI_MODEL")
            .env_remove("ZENPI_PROFILE")
            .env("ZENPI_API_KEY", "fixture-key")
            .env("ZENPI_WIRE_API", "chat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout: ChildStdout = child.stdout.take().unwrap();
        let (send, lines) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.unwrap();
                let value = serde_json::from_str(&line).unwrap();
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
        writeln!(
            self.input,
            "{}",
            json!({"type":"command","id":id,"text":text})
        )
        .unwrap();
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
fn production_jsonl_skill_template_reload_and_unknown_slash_reach_real_http_backend() {
    let root = tempdir().unwrap();
    fixtures(root.path(), "OLD");
    let (url, captured, handle) = server(4, false);
    let mut cli = Cli::start(root.path(), &url);
    assert_eq!(cli.command("catalog", "/skills")["success"], true);
    assert_eq!(cli.command("unknown", "/not-a-template")["success"], false);
    assert_eq!(
        cli.command("reserved", "/help extra arguments")["success"],
        false
    );
    assert_eq!(
        cli.command(
            "template",
            "/fixture-review \"literal $1\" $(touch never-created)"
        )["success"],
        true
    );
    assert_eq!(
        cli.command("skill", "/skill:research literal $1")["success"],
        true
    );
    fixtures(root.path(), "NEW");
    put(
        root.path(),
        ".zenpi/prompts/broken.md",
        "---\ndescription: [bad]\n---\nbroken",
    );
    assert_eq!(cli.command("badreload", "/reload")["success"], false);
    assert_eq!(
        cli.command("oldtemplate", "/fixture-review old")["success"],
        true
    );
    fs::remove_file(root.path().join(".zenpi/prompts/broken.md")).unwrap();
    assert_eq!(cli.command("reload", "/reload")["success"], true);
    assert_eq!(
        cli.command("newtemplate", "/fixture-review new")["success"],
        true
    );
    handle.join().unwrap();
    let captured = captured.lock().unwrap();
    assert_eq!(captured.len(), 4);
    assert!(!captured[0].to_string().contains("SECRET_SKILL_BODY"));
    assert!(!captured[0].to_string().contains("MANUAL_BODY"));
    assert!(captured[0].to_string().contains("TEMPLATE_OLD: literal $1"));
    assert!(captured[1].to_string().contains("SECRET_SKILL_BODY_OLD"));
    assert!(captured[2].to_string().contains("TEMPLATE_OLD: old"));
    assert!(captured[3].to_string().contains("TEMPLATE_NEW: new"));
    assert!(!root.path().join("never-created").exists());
    let journal = fs::read_to_string(root.path().join("cli.jsonl")).unwrap();
    assert!(!journal.contains("SECRET_SKILL_BODY"));
    assert!(!journal.contains("TEMPLATE_OLD:"));
    assert!(journal.contains("source_hash"));
}

#[test]
fn initial_cli_provider_uses_project_configuration_before_first_turn() {
    let root = tempdir().unwrap();
    fixtures(root.path(), "OLD");
    put(
        root.path(),
        ".zenpi/config.toml",
        "model='project-initial-model'",
    );
    let (url, captured, handle) = server(1, false);
    let mut cli = Cli::start(root.path(), &url);
    assert_eq!(
        cli.command("first", "/fixture-review initial")["success"],
        true
    );
    handle.join().unwrap();
    assert_eq!(
        captured.lock().unwrap()[0]["model"],
        "project-initial-model"
    );
}

#[test]
fn production_reload_selection_survives_restart_and_rejects_changed_skill_until_reload() {
    let root = tempdir().unwrap();
    fixtures(root.path(), "OLD");
    put(
        root.path(),
        "chosen/selected/SKILL.md",
        "---\nname: selected\ndescription: chosen\n---\nCHOSEN_BODY_OLD",
    );
    let mut selected = paths(root.path());
    selected.skill_paths = vec!["chosen".into()];
    put(
        root.path(),
        "selection.json",
        &serde_json::to_string(&selected).unwrap(),
    );
    let (url, captured, handle) = server(3, false);
    {
        let mut cli = Cli::start(root.path(), &url);
        assert_eq!(
            cli.command("choose", "/reload selection.json")["success"],
            true
        );
        assert_eq!(
            cli.command("selected", "/skill:selected first")["success"],
            true
        );
        assert_eq!(
            cli.command("badselection", "/reload ../outside.json")["success"],
            false
        );
    }
    {
        let mut cli = Cli::start(root.path(), &url);
        assert_eq!(
            cli.command("restored", "/skill:selected same")["success"],
            true
        );
    }
    put(
        root.path(),
        "chosen/selected/SKILL.md",
        "---\nname: selected\ndescription: chosen\n---\nCHOSEN_BODY_NEW",
    );
    {
        let mut cli = Cli::start(root.path(), &url);
        let failed = cli.command("stale", "/skill:selected must-fail");
        assert_eq!(failed["success"], false);
        assert!(failed.to_string().contains("/reload required"));
        assert_eq!(cli.command("reload", "/reload")["success"], true);
        assert_eq!(
            cli.command("fresh", "/skill:selected fresh")["success"],
            true
        );
    }
    handle.join().unwrap();
    let requests = captured.lock().unwrap();
    assert!(requests[0].to_string().contains("CHOSEN_BODY_OLD"));
    assert!(requests[1].to_string().contains("CHOSEN_BODY_OLD"));
    assert!(requests[2].to_string().contains("CHOSEN_BODY_NEW"));
}

#[test]
#[ignore = "requires explicit ZENPI_SKILL_FIXTURE_ROOT pointing to installed standard skills"]
fn actual_installed_learn_execution_skills_reach_production_provider_only_when_selected() {
    let installed = std::env::var_os("ZENPI_SKILL_FIXTURE_ROOT").expect("explicit skill root");
    let root = tempdir().unwrap();
    fixtures(root.path(), "OLD");
    let mut selected = paths(root.path());
    selected.skill_paths = ["learn-cron-builder", "execution-cron-builder"]
        .iter()
        .map(|name| Path::new(&installed).join(name).join("SKILL.md"))
        .collect();
    put(
        root.path(),
        "selection.json",
        &serde_json::to_string(&selected).unwrap(),
    );
    let loader = zenpi::skills::SkillSet::load_with_paths(
        &selected.user_skills,
        &selected.project_skills,
        &selected.skill_paths,
        || false,
    )
    .unwrap();
    let bodies = ["learn-cron-builder", "execution-cron-builder"].map(|name| {
        loader
            .load_body(
                name,
                zenpi::skills::SkillInvocation::Explicit,
                "literal $1",
                || false,
            )
            .unwrap()
            .body
    });
    let (url, captured, handle) = server(3, false);
    let mut cli = Cli::start(root.path(), &url);
    assert_eq!(
        cli.command("selection", "/reload selection.json")["success"],
        true
    );
    assert_eq!(
        cli.command("index", "/fixture-review just-metadata")["success"],
        true
    );
    assert_eq!(
        cli.command("learn", "/skill:learn-cron-builder literal $1")["success"],
        true
    );
    assert_eq!(
        cli.command("execution", "/skill:execution-cron-builder literal $1")["success"],
        true
    );
    handle.join().unwrap();
    let requests = captured.lock().unwrap();
    let instructions = |index: usize| {
        requests[index]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|message| message["content"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(!instructions(0).contains(&bodies[0]));
    assert!(!instructions(0).contains(&bodies[1]));
    assert!(instructions(1).contains(&bodies[0]));
    assert!(!instructions(1).contains(&bodies[1]));
    assert!(instructions(2).contains(&bodies[1]));
    assert!(!instructions(2).contains(&bodies[0]));
    let journal = fs::read_to_string(root.path().join("cli.jsonl")).unwrap();
    for body in bodies {
        assert!(!journal.contains(&serde_json::to_string(&body).unwrap()));
    }
}

#[test]
fn production_queued_reload_cancellation_keeps_generation_and_provider_responsive() {
    let root = tempdir().unwrap();
    fixtures(root.path(), "OLD");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (arrived_send, arrived) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        arrived_send.send(()).unwrap();
        released.recv_timeout(Duration::from_secs(15)).unwrap();
        reply(&mut stream, &request, false);
    });
    let mut cli = Cli::start(root.path(), &url);
    writeln!(
        cli.input,
        "{}",
        json!({"type":"command","id":"held","text":"/skill:research held"})
    )
    .unwrap();
    cli.input.flush().unwrap();
    arrived.recv_timeout(Duration::from_secs(15)).unwrap();
    fixtures(root.path(), "NEW");
    writeln!(
        cli.input,
        "{}",
        json!({"type":"command","id":"reload-queued","text":"/reload"})
    )
    .unwrap();
    writeln!(
        cli.input,
        "{}",
        json!({"type":"cancel","id":"cancel-reload","target_id":"reload-queued"})
    )
    .unwrap();
    cli.input.flush().unwrap();
    let mut cancelled = false;
    let mut reload_done = false;
    while !cancelled {
        let value = cli.lines.recv_timeout(Duration::from_secs(15)).unwrap();
        if value["id"] == "reload-queued" && value.get("success").is_some() {
            assert_eq!(value["success"], false);
            reload_done = true;
        }
        if value["id"] == "cancel-reload" && value.get("success").is_some() {
            assert_eq!(value["success"], true);
            cancelled = true;
        }
    }
    release.send(()).unwrap();
    let mut held_done = false;
    while !held_done || !reload_done {
        let value = cli.lines.recv_timeout(Duration::from_secs(15)).unwrap();
        if value["id"] == "held" && value.get("success").is_some() {
            assert_eq!(value["success"], true);
            held_done = true;
        }
        if value["id"] == "reload-queued" && value.get("success").is_some() {
            assert_eq!(value["success"], false);
            reload_done = true;
        }
    }
    let status = cli.command("status-resources", "/skills");
    assert_eq!(status["data"]["resources"]["generation"], 1);
    handle.join().unwrap();
    let journal = fs::read_to_string(root.path().join("cli.jsonl")).unwrap();
    assert!(!journal.contains("resources_selected"));
}
