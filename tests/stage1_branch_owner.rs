use serde_json::{Value, json};
use std::{
    cell::Cell,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use tempfile::tempdir;
use zenpi::{
    backend::{
        Backend, BackendError, Completion, CompletionRequest, OpenAiCompatibleBackend,
        OpenAiWireApi,
    },
    context::ContextBudget,
    core::{Agent, Turn, TurnInputRequest, TurnRole},
    protocol::{TreeAction as A, TreeRequest},
    session::SessionStore,
};
const BUDGET: ContextBudget = ContextBudget {
    max_tokens: 4000,
    reserved_output_tokens: 1000,
};
fn req(a: &Agent, action: A) -> TreeRequest {
    TreeRequest {
        schema_version: 2,
        id: "tree-test".into(),
        kind: "tree".into(),
        session_id: a.session().session_id().into(),
        tree: action,
    }
}
fn control(a: &mut Agent, action: A) -> Value {
    let r = req(a, action);
    a.tree_request(r, &|| false).unwrap()
}
fn leaf(a: &Agent) -> String {
    a.session()
        .tree_snapshot(&|| false)
        .unwrap()
        .active_leaf()
        .unwrap()
        .into()
}
fn add(a: &mut Agent, id: &str, text: &str) {
    a.session_mut()
        .append_turn(Turn::new(id, TurnRole::User, text))
        .unwrap();
}
fn seed(a: &mut Agent) -> (String, String, String) {
    add(a, "a", "COMMON_A");
    control(a, A::Enable {});
    let a_id = leaf(a);
    add(a, "b", "BRANCH_B_ONLY");
    let b = leaf(a);
    control(
        a,
        A::Select {
            leaf: Some(a_id.clone()),
        },
    );
    add(a, "c", "BRANCH_C_ONLY");
    let c = leaf(a);
    (a_id, b, c)
}
struct Peer {
    url: String,
    seen: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}
fn read_request(stream: &mut TcpStream) -> Value {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    let mut bytes = vec![];
    let end = loop {
        let mut part = [0; 4096];
        let n = stream.read(&mut part).unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&part[..n]);
        assert!(bytes.len() < 512 * 1024);
        if let Some(n) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break n + 4;
        }
    };
    let len: usize = String::from_utf8_lossy(&bytes[..end])
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|s| s.trim().parse().unwrap())
        })
        .unwrap();
    while bytes.len() < end + len {
        let mut part = [0; 4096];
        let n = stream.read(&mut part).unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&part[..n]);
    }
    serde_json::from_slice(&bytes[end..end + len]).unwrap()
}
impl Peer {
    fn new() -> Self {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", socket.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(vec![]));
        let out = seen.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let join = thread::spawn(move || {
            while !done.load(Ordering::Acquire) {
                match socket.accept() {
                    Ok((mut s, _)) => {
                        let request = read_request(&mut s);
                        let all = request.to_string();
                        let content = if request["metadata"]["purpose"] == "semantic_compaction" {
                            let data: Value = serde_json::from_str(
                                request["messages"].as_array().unwrap().last().unwrap()["content"]
                                    .as_str()
                                    .unwrap(),
                            )
                            .unwrap();
                            json!({"goals":["COMMON_A"],"constraints":[if all.contains("BRANCH_C_ONLY"){assert!(!all.contains("BRANCH_B_ONLY"));"BRANCH_C_ONLY"}else{"BRANCH_B_ONLY"}],"decisions":[],"progress":[],"pending_tasks":["verify selected ancestry"],"critical_facts":[],"read_files":data["read_files"],"modified_files":data["modified_files"],"unresolved_tools":data["unresolved_tools"]}).to_string()
                        } else {
                            "done".into()
                        };
                        out.lock().unwrap().push(request);
                        let body=json!({"model":"branch-test","choices":[{"message":{"role":"assistant","content":content},"finish_reason":"stop"}],"usage":{"prompt_tokens":200,"completion_tokens":150,"total_tokens":350}}).to_string();
                        write!(s,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        });
        Self {
            url,
            seen,
            stop,
            join: Some(join),
        }
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.join.take().unwrap().join().unwrap();
    }
}
fn agent(path: &Path, url: &str) -> Agent {
    let backend = OpenAiCompatibleBackend::new_with_wire_api(
        url,
        Some("test".into()),
        "branch-test",
        OpenAiWireApi::ChatCompletions,
    )
    .unwrap();
    let mut a = Agent::new(SessionStore::open(path).unwrap(), Box::new(backend));
    a.set_context_budget(BUDGET);
    a
}
#[test]
fn typed_jsonl_and_tui_projection_select_same_leaf_without_provider() {
    let dir = tempdir().unwrap();
    let peer = Peer::new();
    let path = dir.path().join("s.jsonl");
    let mut a = agent(&path, &peer.url);
    let (_, b, c) = seed(&mut a);
    let sid = a.session().session_id().to_owned();
    let input = format!(
        "{}\n{}\n",
        json!({"schema_version":2,"id":"select-b","type":"tree","session_id":sid,"tree":{"action":"select","leaf":b}}),
        json!({"schema_version":2,"id":"tree-page","type":"tree","session_id":sid,"tree":{"action":"list","cursor":0,"limit":2}})
    );
    let mut output = vec![];
    zenpi::headless::run_headless(&mut a, std::io::Cursor::new(input), &mut output).unwrap();
    let rows: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let page = rows.iter().find(|r| r["id"] == "tree-page").unwrap();
    assert_eq!(page["data"]["active_leaf"], b);
    assert_eq!(page["data"]["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(page["data"]["next_cursor"], 2);
    // EOF closes the previous host owner; reopen as the terminal would.
    let mut a = agent(&path, &peer.url);
    let mut state = zenpi::tui::TuiState::default();
    assert!(zenpi::tui::dispatch_tree_input(
        &mut state,
        Some(&mut a),
        &format!("/tree select {c}")
    ));
    assert_eq!(
        state.session_snapshot().unwrap().active_leaf(),
        Some(c.as_str())
    );
    assert_eq!(leaf(&a), c);
    assert!(peer.seen.lock().unwrap().is_empty());
    let reopened = agent(&path, &peer.url);
    assert_eq!(leaf(&reopened), c);
    assert_eq!(
        reopened
            .selected_history()
            .unwrap()
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "c"]
    );
}
#[test]
fn actual_http_after_selection_and_independent_restart_contains_only_selected_ancestry() {
    let dir = tempdir().unwrap();
    let peer = Peer::new();
    let path = dir.path().join("s.jsonl");
    let mut a = agent(&path, &peer.url);
    let (_, b, c) = seed(&mut a);
    control(&mut a, A::Select { leaf: Some(b) });
    control(&mut a, A::Select { leaf: Some(c) });
    drop(a);
    for n in 0..2 {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "branch_process_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("ZENPI_BRANCH_PATH", &path)
            .env("ZENPI_BRANCH_URL", &peer.url)
            .env("ZENPI_BRANCH_TEXT", format!("continue-{n}"))
            .status()
            .unwrap();
        assert!(status.success());
    }
    let seen = peer.seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    for request in seen.iter() {
        let wire = request.to_string();
        assert!(wire.contains("COMMON_A"));
        assert!(wire.contains("BRANCH_C_ONLY"));
        assert!(!wire.contains("BRANCH_B_ONLY"));
    }
    assert_eq!(SessionStore::open(&path).unwrap().turns().len(), 7);
}
#[test]
#[ignore = "independent process opened by product test"]
fn branch_process_helper() {
    let mut a = agent(
        Path::new(&std::env::var("ZENPI_BRANCH_PATH").unwrap()),
        &std::env::var("ZENPI_BRANCH_URL").unwrap(),
    );
    a.process(TurnInputRequest::new(
        std::env::var("ZENPI_BRANCH_TEXT").unwrap(),
    ))
    .unwrap();
}
#[test]
fn real_summary_is_scoped_to_branch_and_restored_after_restart() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let peer = Peer::new();
    let mut a = agent(&path, &peer.url);
    add(&mut a, "a", "COMMON_A");
    control(&mut a, A::Enable {});
    let root = leaf(&a);
    for branch in ["B", "C"] {
        if branch == "C" {
            control(
                &mut a,
                A::Select {
                    leaf: Some(root.clone()),
                },
            );
        }
        for n in 0..10 {
            add(
                &mut a,
                &format!("{branch}-{n}"),
                &format!(
                    "BRANCH_{branch}_ONLY {}",
                    "bounded historical text ".repeat(65)
                ),
            );
        }
        assert!(a.compact_context().unwrap().compacted);
        let cp = a
            .session()
            .latest_semantic_checkpoint(BUDGET)
            .unwrap()
            .unwrap();
        assert_eq!(cp.source.branch_id, a.session().selected_tree_branch());
        assert!(cp.summary.contains(&format!("BRANCH_{branch}_ONLY")));
    }
    let old_branch = a
        .session()
        .events()
        .iter()
        .find(|event| event["type"] == "semantic_checkpoint")
        .unwrap()["checkpoint"]
        .clone();
    let old_branch: zenpi::context::SemanticCheckpoint =
        serde_json::from_value(old_branch).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(
        a.session_mut()
            .append_semantic_checkpoint(&old_branch, BUDGET, &|| false)
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    drop(a);
    let mut a = agent(&path, &peer.url);
    a.process(TurnInputRequest::new("continue selected branch"))
        .unwrap();
    let seen = peer.seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(seen[2].to_string().contains("BRANCH_C_ONLY"));
    assert!(!seen[2].to_string().contains("BRANCH_B_ONLY"));
    assert!(seen[2].to_string().contains("COMMON_A"));
    assert!(a.session().turns().iter().any(|t| t.id == "B-9"));
}
#[test]
fn invalid_cross_session_cancelled_or_pending_selection_preserves_leaf_and_journal() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let peer = Peer::new();
    let mut a = agent(&path, &peer.url);
    let (_, b, c) = seed(&mut a);
    let original = std::fs::read(&path).unwrap();
    for invalid in [
        "bad\nleaf".to_owned(),
        "x".repeat(300),
        "entry-from-another-session".into(),
    ] {
        let r = req(
            &a,
            A::Select {
                leaf: Some(invalid),
            },
        );
        assert!(a.tree_request(r, &|| false).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    let mut r = req(
        &a,
        A::Select {
            leaf: Some(b.clone()),
        },
    );
    r.session_id = "foreign".into();
    assert!(a.tree_request(r, &|| false).is_err());
    let calls = Cell::new(0);
    let r = req(
        &a,
        A::Select {
            leaf: Some(b.clone()),
        },
    );
    assert!(
        a.tree_request(r, &|| {
            calls.set(calls.get() + 1);
            calls.get() >= 5
        })
        .is_err()
    );
    assert!(calls.get() >= 5);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(leaf(&a), c);
    a.input_queue_request(zenpi::protocol::InputQueueRequest {
        schema_version: 2,
        id: "enqueue".into(),
        kind: "input_queue".into(),
        session_id: a.session().session_id().into(),
        input_queue: zenpi::protocol::InputQueueAction::Enqueue {
            input_id: "later".into(),
            kind: zenpi::input_queue::InputKind::FollowUp,
            text: "pending on C".into(),
        },
    })
    .unwrap();
    let queued = std::fs::read(&path).unwrap();
    let r = req(&a, A::Select { leaf: Some(b) });
    assert!(a.tree_request(r, &|| false).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), queued);
    assert_eq!(leaf(&a), c);
    assert!(peer.seen.lock().unwrap().is_empty());
}
#[test]
fn strict_wire_rejects_unknown_fields_and_mutations_fail_before_context_switch() {
    for action in [
        json!({"action":"select","leaf":"a","extra":true}),
        json!({"action":"list","cursor":0,"limit":129}),
    ] {
        let line =
            json!({"schema_version":2,"id":"t","type":"tree","session_id":"s","tree":action})
                .to_string();
        assert!(zenpi::protocol::parse_line(&line).is_err());
    }
    let dir = tempdir().unwrap();
    let peer = Peer::new();
    let path = dir.path().join("s.jsonl");
    let mut a = agent(&path, &peer.url);
    let (_, b, c) = seed(&mut a);
    // A forged summary belonging to another source cannot silently become a view.
    a.session_mut().append_event(json!({"type":"semantic_checkpoint","checkpoint":{"schema_version":2,"source":{"session_id":"foreign","branch_id":"linear"}}})).unwrap();
    let before = std::fs::read(&path).unwrap();
    let r = req(&a, A::Select { leaf: Some(b) });
    assert!(a.tree_request(r, &|| false).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(leaf(&a), c);
}
struct SignatureBackend;
impl Backend for SignatureBackend {
    fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        panic!("navigation invoked provider")
    }
    fn name(&self) -> &str {
        "signature-test"
    }
    fn validate_history_model(&self, turns: &[Turn], _: Option<&str>) -> Result<(), BackendError> {
        if turns.iter().any(|t| t.content.contains("BRANCH_B_ONLY")) {
            Err(BackendError::InvalidResponse(
                "opaque signature model mismatch".into(),
            ))
        } else {
            Ok(())
        }
    }
}
#[test]
fn native_signature_validation_happens_before_durable_leaf_switch() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut a = Agent::new(
        SessionStore::open(&path).unwrap(),
        Box::new(SignatureBackend),
    );
    let (_, b, c) = seed(&mut a);
    let before = std::fs::read(&path).unwrap();
    let r = req(&a, A::Select { leaf: Some(b) });
    assert!(a.tree_request(r, &|| false).is_err());
    assert_eq!(leaf(&a), c);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn settled_historical_tools_are_only_context_and_fork_gets_no_live_owner_state() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let peer = Peer::new();
    let mut a = agent(&path, &peer.url);
    a.set_tools(
        zenpi::tools::ToolRegistry::with_all_builtins().unwrap(),
        zenpi::tools::ToolContext::new(dir.path()).unwrap(),
        zenpi::tools::SideEffectPolicy::all_builtins(),
    );
    add(&mut a, "a", "COMMON_A");
    control(&mut a, A::Enable {});
    let mut call = Turn::with_parent("historical-call", "a", TurnRole::Assistant, "");
    call.metadata = Some(
        json!({"tool_calls":[{"id":"historical-write","name":"write_file","arguments":{"path":"must-not-replay.txt","content":"bad"}}]}),
    );
    a.session_mut().append_turn(call).unwrap();
    let mut result = Turn::with_parent(
        "historical-result",
        "a",
        TurnRole::Tool,
        "historical success",
    );
    result.metadata = Some(
        json!({"tool_call_id":"historical-write","tool_name":"write_file","outcome":"succeeded"}),
    );
    a.session_mut().append_turn(result).unwrap();
    let completed = leaf(&a);
    add(&mut a, "b", "BRANCH_B_ONLY");
    control(
        &mut a,
        A::Select {
            leaf: Some(completed.clone()),
        },
    );
    let destination = dir.path().join("fork.jsonl");
    let reply = control(
        &mut a,
        A::Fork {
            leaf: Some(completed.clone()),
            destination: destination.display().to_string(),
        },
    );
    assert_eq!(leaf(&a), completed);
    let child = SessionStore::open(&destination).unwrap();
    assert_ne!(child.session_id(), a.session().session_id());
    assert_eq!(reply["fork"]["session_id"], child.session_id());
    assert_eq!(child.turns().len(), 3);
    assert!(child.operation_recovery().is_empty());
    a.process(TurnInputRequest::new("continue with context only"))
        .unwrap();
    assert!(!dir.path().join("must-not-replay.txt").exists());
    assert_eq!(peer.seen.lock().unwrap().len(), 1);
    assert!(
        !peer.seen.lock().unwrap()[0]
            .to_string()
            .contains("BRANCH_B_ONLY")
    );
    let forked_bytes = std::fs::read(&destination).unwrap();
    let r = req(
        &a,
        A::Fork {
            leaf: Some(completed),
            destination: destination.display().to_string(),
        },
    );
    assert!(a.tree_request(r, &|| false).is_err());
    assert_eq!(std::fs::read(&destination).unwrap(), forked_bytes);
}

#[cfg(unix)]
#[test]
fn production_async_fork_is_cancellable_and_status_stays_responsive() {
    struct Output(std::sync::mpsc::Sender<Value>, Vec<u8>);
    impl Write for Output {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.1.extend_from_slice(bytes);
            while let Some(end) = self.1.iter().position(|b| *b == b'\n') {
                let line: Vec<_> = self.1.drain(..=end).collect();
                if line.len() > 1 {
                    let _ = self.0.send(serde_json::from_slice(&line).unwrap());
                }
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let dir = tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let peer = Peer::new();
    let mut a = agent(&path, &peer.url);
    control(&mut a, A::Enable {});
    for n in 0..1500 {
        add(&mut a, &format!("history-{n}"), "history");
    }
    let tip = leaf(&a);
    let source = std::fs::read(&path).unwrap();
    let sid = a.session().session_id().to_owned();
    let destination = dir.path().join("cancelled-fork.jsonl");
    let (mut input, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(a, reader, Output(tx, vec![])).unwrap()
    });
    writeln!(input,"{}",json!({"schema_version":2,"type":"tree","id":"fork-job","session_id":sid,"tree":{"action":"fork","leaf":tip,"destination":destination}})).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let copying = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".tree-fork")
                    && std::fs::read_to_string(entry.path())
                        .is_ok_and(|s| s.contains("\"kind\":\"turn\""))
            });
        if copying {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fork never reached staged turn"
        );
        thread::sleep(Duration::from_millis(1));
    }
    writeln!(
        input,
        "{}",
        json!({"schema_version":2,"type":"status","id":"status"})
    )
    .unwrap();
    writeln!(
        input,
        "{}",
        json!({"schema_version":2,"type":"cancel","id":"cancel","target_id":"fork-job"})
    )
    .unwrap();
    let mut responses = vec![];
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !["status", "cancel", "fork-job"].iter().all(|id| {
        responses
            .iter()
            .any(|r: &Value| r["type"] == "response" && r["id"] == *id)
    }) {
        assert!(std::time::Instant::now() < deadline);
        responses.push(rx.recv_timeout(Duration::from_secs(5)).unwrap());
    }
    assert_eq!(
        responses
            .iter()
            .find(|r| r["id"] == "fork-job" && r["type"] == "response")
            .unwrap()["success"],
        false
    );
    assert_eq!(
        responses.iter().find(|r| r["id"] == "status").unwrap()["success"],
        true
    );
    drop(input);
    host.join().unwrap();
    assert!(!destination.exists());
    assert!(
        !std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with(".tree-fork"))
    );
    assert!(peer.seen.lock().unwrap().is_empty());
    // Transport responses may append their own replay events; source turns and leaf stay exact.
    let reopened = SessionStore::open(&path).unwrap();
    assert_eq!(reopened.active_tree_leaf(), Some(tip.as_str()));
    assert_eq!(reopened.turns().len(), 1500);
    assert!(std::fs::read(&path).unwrap().starts_with(&source));
}

#[test]
fn tree_enable_rejects_extra_fields_and_duplicate_identity_or_action_fields() {
    for line in [
        r#"{"schema_version":2,"id":"t","type":"tree","session_id":"s","tree":{"action":"enable","unexpected":true}}"#,
        r#"{"schema_version":2,"id":"t","id":"other","type":"tree","session_id":"s","tree":{"action":"enable"}}"#,
        r#"{"schema_version":2,"id":"t","type":"tree","session_id":"s","tree":{"action":"enable","action":"enable"}}"#,
        r#"{"schema_version":2,"id":"t","type":"tree","session_id":"s","tree":{"action":"select","leaf":"first","leaf":"second"}}"#,
    ] {
        assert!(
            zenpi::protocol::parse_line(line).is_err(),
            "accepted ambiguous tree control: {line}"
        );
    }
}
