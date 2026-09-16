//! Product tests use the real HTTP adapter and journal. The peer is a bounded
//! local HTTP fixture; it validates the actual summary and continuation wires.
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tempfile::tempdir;
use zenpi::{
    backend::{OpenAiCompatibleBackend, OpenAiWireApi},
    context::{self, ContextBudget},
    core::{Agent, Turn, TurnInputRequest, TurnRole},
    governance::{BudgetLedger, ResourceLimits},
    providers::registry::{ModelOverride, ModelRegistry},
    session::SessionStore,
};
const BUDGET: ContextBudget = ContextBudget {
    max_tokens: 4000,
    reserved_output_tokens: 1000,
};
fn read_request(stream: &mut TcpStream) -> Value {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    let mut bytes = Vec::new();
    let end = loop {
        let mut chunk = [0; 4096];
        let n = stream.read(&mut chunk).unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&chunk[..n]);
        assert!(bytes.len() < 256 * 1024);
        if let Some(n) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
            break n + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..end]);
    assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
    let len: usize = headers
        .lines()
        .find_map(|v| {
            v.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse().unwrap())
        })
        .unwrap();
    while bytes.len() < end + len {
        let mut chunk = [0; 4096];
        let n = stream.read(&mut chunk).unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&chunk[..n]);
    }
    serde_json::from_slice(&bytes[end..end + len]).unwrap()
}
fn respond(stream: &mut TcpStream, body: Value) {
    let body = body.to_string();
    // Headers were sent before the handler. Delayed JSON bodies exercise the
    // actual transport receive polling, including cancelled socket closure.
    let _ = write!(stream, "{:x}\r\n{}\r\n0\r\n\r\n", body.len(), body);
}

fn summary_data(request: &Value) -> Value {
    assert_eq!(request["metadata"]["purpose"], "semantic_compaction");
    assert_eq!(request["max_completion_tokens"], 1000);
    assert!(
        request
            .get("tools")
            .is_none_or(|v| v.as_array().is_some_and(Vec::is_empty))
    );
    assert!(request.get("response_format").is_none());
    let messages = request["messages"].as_array().unwrap();
    assert!(
        messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("do not continue")
    );
    serde_json::from_str(messages.last().unwrap()["content"].as_str().unwrap()).unwrap()
}
fn good_summary(data: &Value) -> String {
    let all = data.to_string();
    assert!(
        all.contains("Keep BentoBox") || all.contains("keep BentoBox"),
        "lost old constraint before summary: {data}"
    );
    assert!(
        all.contains("verify restart"),
        "lost pending work before summary"
    );
    json!({"goals":["Improve project tabs"],"constraints":["Keep BentoBox and Unicode paths"],
        "decisions":["Use the existing owner"],"progress":["Read constraints and wrote output"],
        "pending_tasks":["verify restart"],"critical_facts":["No unknown tool outcome is completed"],
        "read_files":data["read_files"],"modified_files":data["modified_files"],"unresolved_tools":data["unresolved_tools"]}).to_string()
}
fn completion(content: String) -> Value {
    json!({"id":"http-summary","model":"summary-test","choices":[{"message":{"role":"assistant","content":content},"finish_reason":"stop"}],"usage":{"prompt_tokens":210,"completion_tokens":130,"total_tokens":340}})
}
struct Peer {
    url: String,
    seen: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}
impl Peer {
    fn new(handler: impl Fn(usize, &Value) -> Value + Send + 'static) -> Self {
        Self::with_status(200, handler)
    }
    fn with_status(status: u16, handler: impl Fn(usize, &Value) -> Value + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let saved = seen.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let join = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !done.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline, "HTTP fixture deadline");
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let request = read_request(&mut stream);
                        let mut seen = saved.lock().unwrap();
                        let n = seen.len();
                        seen.push(request.clone());
                        drop(seen);
                        write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
                        stream.flush().unwrap();
                        respond(&mut stream, handler(n, &request));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1))
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
        if let Some(j) = self.join.take()
            && let Err(e) = j.join()
            && !thread::panicking()
        {
            std::panic::resume_unwind(e);
        }
    }
}
fn agent(path: &Path, url: &str, priced: bool) -> Agent {
    let price=priced.then(||json!({"input_micro_usd_per_million":1_000_000,"output_micro_usd_per_million":2_000_000,"cached_input_micro_usd_per_million":null,"source":"HTTP fixture","version":"1"}));
    let override_:ModelOverride=serde_json::from_value(json!({"provider":"fixture","id":"summary-test","version":"1","context_window":8000,"max_output_tokens":1000,"text":true,"tools":true,"price":price})).unwrap();
    let backend = OpenAiCompatibleBackend::from_values_with_wire_api(
        url.into(),
        None,
        "summary-test".into(),
        OpenAiWireApi::ChatCompletions,
    )
    .unwrap()
    .with_model_registry(
        "fixture".into(),
        ModelRegistry::with_overrides(&[override_]).unwrap(),
    )
    .unwrap()
    .with_max_retries(3)
    .unwrap();
    let mut agent = Agent::new(SessionStore::open(path).unwrap(), Box::new(backend));
    agent.set_context_budget(BUDGET);
    agent
}
fn seed(agent: &mut Agent) {
    for n in 0..10 {
        let content = if n == 0 {
            format!(
                "Goal: Improve project tabs. Keep BentoBox and Unicode paths. Pending: verify restart. {}",
                "old history ".repeat(110)
            )
        } else {
            "old history ".repeat(110)
        };
        agent
            .session_mut()
            .append_turn(Turn::new(format!("old-{n}"), TurnRole::User, content))
            .unwrap();
        if n == 0 {
            let mut calls = Turn::with_parent("calls", "old-0", TurnRole::Assistant, "");
            calls.metadata = Some(
                json!({"tool_calls":[{"id":"r","name":"read_file","arguments":{"path":"约束.txt"}},{"id":"w","name":"write_file","arguments":{"path":"output.txt"}}]}),
            );
            agent.session_mut().append_turn(calls).unwrap();
            for (id, name) in [("r", "read_file"), ("w", "write_file")] {
                let mut t =
                    Turn::with_parent(format!("result-{id}"), "old-0", TurnRole::Tool, "done");
                t.metadata =
                    Some(json!({"tool_call_id":id,"tool_name":name,"outcome":"succeeded"}));
                agent.session_mut().append_turn(t).unwrap();
            }
        }
    }
}
fn append_more(agent: &mut Agent) {
    for n in 10..15 {
        agent
            .session_mut()
            .append_turn(Turn::new(
                format!("old-{n}"),
                TurnRole::User,
                "new history ".repeat(110),
            ))
            .unwrap();
    }
}
fn checkpoint(agent: &Agent) -> Option<context::SemanticCheckpoint> {
    agent
        .session()
        .latest_semantic_checkpoint(ContextBudget {
            max_tokens: u64::MAX,
            reserved_output_tokens: 0,
        })
        .unwrap()
}
#[test]
fn real_http_summary_preserves_constraints_files_usage_and_iterative_restart() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");
    let peer = Peer::new(|n, request| {
        if n < 2 {
            let data = summary_data(request);
            if n == 0 {
                assert!(data["previous_summary"].is_null());
            } else {
                assert!(
                    data["previous_summary"]
                        .as_str()
                        .unwrap()
                        .contains("Keep BentoBox")
                );
                assert!(!data["new_records"].to_string().contains("old-0"));
            }
            completion(good_summary(&data))
        } else {
            let wire = request.to_string();
            assert!(wire.contains("Keep BentoBox"));
            assert!(wire.contains("verify restart"));
            assert!(wire.contains("约束.txt"));
            assert!(!wire.contains("old history old history old history"));
            completion("Continue verifying restart".into())
        }
    });
    let mut a = agent(&path, &peer.url, true);
    seed(&mut a);
    a.set_summary_cost_limit(Some(20_000)).unwrap();
    let report = a.compact_context().unwrap();
    assert!(report.compacted);
    assert!(report.checkpoint.is_none());
    let first = checkpoint(&a).unwrap();
    assert_eq!(first.summary_usage.output_tokens, 130);
    assert_eq!(first.summary_provider, "fixture");
    assert_eq!(first.read_files, ["约束.txt"]);
    assert_eq!(first.modified_files, ["output.txt"]);
    append_more(&mut a);
    a.compact_context().unwrap();
    let second = checkpoint(&a).unwrap();
    assert!(second.source_end > first.source_end);
    drop(a);
    let mut a = agent(&path, &peer.url, true);
    assert_eq!(checkpoint(&a).unwrap(), second);
    a.process(TurnInputRequest::new("verify the remaining work"))
        .unwrap();
    let budget = BudgetLedger::restore(a.session(), ResourceLimits::default()).unwrap();
    assert_eq!(budget.summary_cost_budget().reserved_micro_usd, 12_000);
    let usage = a
        .session()
        .events()
        .iter()
        .filter(|e| e["type"] == "semantic_summary_response")
        .collect::<Vec<_>>();
    assert_eq!(usage.len(), 2);
    assert_eq!(usage[0]["usage_cost_estimate_micro_usd"], 470);
    assert_eq!(peer.seen.lock().unwrap().len(), 3);
}
#[test]
fn actual_http_invalid_summary_never_replaces_previous_checkpoint() {
    for case in [
        "empty",
        "json",
        "missing_usage",
        "missing_input_usage",
        "length",
        "over_output",
        "file_loss",
        "refusal",
        "tool_call",
        "over_input",
        "over_cost",
    ] {
        let case = case.to_string();
        let chosen = case.clone();
        let peer = Peer::new(move |n, request| {
            let data = summary_data(request);
            let mut body = completion(good_summary(&data));
            if n == 1 {
                match chosen.as_str() {
                    "empty" => body["choices"][0]["message"]["content"] = json!(""),
                    "json" => body["choices"][0]["message"]["content"] = json!("partial {"),
                    "missing_usage" => body
                        .as_object_mut()
                        .unwrap()
                        .remove("usage")
                        .map(|_| ())
                        .unwrap(),
                    "missing_input_usage" => {
                        body["usage"]
                            .as_object_mut()
                            .unwrap()
                            .remove("prompt_tokens");
                    }
                    "length" => body["choices"][0]["finish_reason"] = json!("length"),
                    "over_input" | "over_cost" => {
                        body["usage"]["prompt_tokens"] = json!(10_000);
                        body["usage"]["total_tokens"] = json!(10_130);
                    }
                    "over_output" => {
                        body["usage"]["completion_tokens"] = json!(1001);
                        body["usage"]["total_tokens"] = json!(1211);
                    }
                    "file_loss" => {
                        let mut summary: Value = serde_json::from_str(
                            body["choices"][0]["message"]["content"].as_str().unwrap(),
                        )
                        .unwrap();
                        summary["read_files"] = json!([]);
                        body["choices"][0]["message"]["content"] = json!(summary.to_string());
                    }
                    "refusal" => body["choices"][0]["message"]["refusal"] = json!("no"),
                    "tool_call" => {
                        body["choices"][0]["finish_reason"] = json!("tool_calls");
                        body["choices"][0]["message"]["tool_calls"] = json!([{"id":"bad","type":"function","function":{"name":"write_file","arguments":"{}"}}]);
                    }
                    _ => unreachable!(),
                }
            }
            body
        });
        let dir = tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let mut a = agent(&path, &peer.url, case == "over_cost");
        seed(&mut a);
        if case == "over_cost" {
            a.set_summary_cost_limit(Some(12_000)).unwrap();
        }
        a.compact_context().unwrap();
        let before = checkpoint(&a).unwrap();
        append_more(&mut a);
        assert!(a.compact_context().is_err(), "{case}");
        assert_eq!(checkpoint(&a).unwrap(), before, "{case}");
        drop(a);
        let a = agent(&path, &peer.url, false);
        assert_eq!(checkpoint(&a).unwrap(), before);
        assert_eq!(peer.seen.lock().unwrap().len(), 2, "{case}");
    }
}
#[test]
fn quota_cost_unknown_price_and_oversized_source_fail_before_http() {
    for case in ["cost", "unknown", "network", "input", "output", "source"] {
        let peer = Peer::new(|_, _| panic!("rejected summary reached provider"));
        let dir = tempdir().unwrap();
        let mut a = agent(
            &dir.path().join("journal.jsonl"),
            &peer.url,
            case != "unknown",
        );
        seed(&mut a);
        match case {
            "cost" => a.set_summary_cost_limit(Some(5999)).unwrap(),
            "unknown" => a.set_summary_cost_limit(Some(10_000)).unwrap(),
            "network" | "input" | "output" => {
                let mut limits = ResourceLimits::default();
                match case {
                    "network" => limits.max_network_requests = 0,
                    "input" => limits.max_input_tokens = 1,
                    _ => limits.max_output_tokens = 999,
                };
                a.set_resource_limits(limits).unwrap();
            }
            "source" => append_more(&mut a),
            _ => unreachable!(),
        }
        assert!(a.compact_context().is_err(), "{case}");
        assert!(checkpoint(&a).is_none());
        assert!(peer.seen.lock().unwrap().is_empty());
    }
}
#[test]
fn http_cancellation_retains_valid_checkpoint_and_does_not_retry() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let peer = Peer::new(move |n, request| {
        let body = completion(good_summary(&summary_data(request)));
        if n == 1 {
            flag.store(true, Ordering::Release);
        }
        body
    });
    let dir = tempdir().unwrap();
    let mut a = agent(&dir.path().join("journal.jsonl"), &peer.url, false);
    seed(&mut a);
    a.compact_context().unwrap();
    let before = checkpoint(&a).unwrap();
    append_more(&mut a);
    assert!(
        a.compact_context_with_control(|| cancelled.load(Ordering::Acquire))
            .is_err()
    );
    assert_eq!(checkpoint(&a).unwrap(), before);
    assert_eq!(peer.seen.lock().unwrap().len(), 2);
}
#[test]
fn native_state_is_pinned_counted_and_branch_bound() {
    let peer = Peer::new(|_, request| completion(good_summary(&summary_data(request))));
    let dir = tempdir().unwrap();
    let mut a = agent(&dir.path().join("journal.jsonl"), &peer.url, false);
    seed(&mut a);
    let mut native = Turn::with_parent("native", "old-9", TurnRole::Assistant, "visible");
    native.metadata = Some(
        json!({"annotations":[{"type":"native_history","wire":"anthropic_messages","provider":"anthropic","model":"claude-sonnet-4-6","blocks":[{"type":"thinking","thinking":"opaque thinking","signature":"raw+signature/bytes=="},{"type":"text","text":"visible"}]}]}),
    );
    a.session_mut().append_turn(native.clone()).unwrap();
    a.session_mut()
        .append_turn(Turn::new("latest", TurnRole::User, "continue"))
        .unwrap();
    a.compact_context().unwrap();
    let cp = checkpoint(&a).unwrap();
    let restored = context::restore_semantic_checkpoint(
        a.history(),
        &cp,
        a.session().session_id(),
        "linear",
        BUDGET,
    )
    .unwrap();
    assert_eq!(restored.iter().find(|t| t.id == "native"), Some(&native));
    assert!(
        context::restore_semantic_checkpoint(
            a.history(),
            &cp,
            a.session().session_id(),
            "other-branch",
            BUDGET
        )
        .is_err()
    );
    let mut huge = native.clone();
    huge.metadata.as_mut().unwrap()["annotations"][0]["blocks"][0]["signature"] =
        json!("s".repeat(20_000));
    assert!(context::estimate_tokens(&[huge]).input_tokens > 5000);
}
#[test]
#[ignore = "launched as an independent process by the parent test"]
fn semantic_process_helper() {
    let path = std::env::var("ZENPI_SEMANTIC_TEST_PATH").unwrap();
    let url = std::env::var("ZENPI_SEMANTIC_TEST_URL").unwrap();
    let mut a = agent(Path::new(&path), &url, false);
    if std::env::var("ZENPI_SEMANTIC_TEST_MODE").unwrap() == "compact" {
        seed(&mut a);
        a.compact_context().unwrap();
        std::process::exit(0);
    } else if std::env::var("ZENPI_SEMANTIC_TEST_MODE").unwrap() == "interrupted" {
        append_more(&mut a);
        a.compact_context().unwrap();
        panic!("parent must terminate the helper before checkpoint commit");
    } else {
        assert!(checkpoint(&a).is_some());
        assert!(a.operation_recovery().is_empty());
        a.process(TurnInputRequest::new("verify restart")).unwrap();
    }
}
#[test]
fn separate_process_commit_and_reopen_use_semantic_context_on_real_wire() {
    let peer = Peer::new(|n, request| {
        if n == 0 {
            completion(good_summary(&summary_data(request)))
        } else {
            let wire = request.to_string();
            assert!(wire.contains("Keep BentoBox"));
            assert!(wire.contains("verify restart"));
            assert!(!wire.contains("semantic_checkpoint"));
            completion("resumed".into())
        }
    });
    let dir = tempdir().unwrap();
    for mode in ["compact", "resume"] {
        let path = if mode == "compact" {
            dir.path().join("journal.jsonl")
        } else {
            let bytes = std::fs::read(dir.path().join("journal.jsonl")).unwrap();
            let mut prefix = Vec::new();
            let mut found = false;
            for line in bytes.split_inclusive(|b| *b == b'\n') {
                prefix.extend_from_slice(line);
                let record: Value = serde_json::from_slice(line).unwrap();
                if record["event"]["type"] == "semantic_checkpoint" {
                    found = true;
                    break;
                }
            }
            assert!(found);
            let crash = dir.path().join("crash-after-commit.jsonl");
            std::fs::write(&crash, prefix).unwrap();
            crash
        };
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "semantic_process_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("ZENPI_SEMANTIC_TEST_PATH", &path)
            .env("ZENPI_SEMANTIC_TEST_URL", &peer.url)
            .env("ZENPI_SEMANTIC_TEST_MODE", mode)
            .status()
            .unwrap();
        assert!(status.success());
    }
    assert_eq!(peer.seen.lock().unwrap().len(), 2);
}

#[test]
fn unknown_tool_result_stays_in_retained_tail_and_summary_does_not_claim_success() {
    let peer = Peer::new(|_, request| {
        let data = summary_data(request);
        assert_eq!(data["unresolved_tools"][0]["call_id"], "unknown-call");
        completion(good_summary(&data))
    });
    let dir = tempdir().unwrap();
    let mut a = agent(&dir.path().join("journal.jsonl"), &peer.url, false);
    seed(&mut a);
    let mut call = Turn::with_parent("unknown-assistant", "old-9", TurnRole::Assistant, "");
    call.metadata = Some(
        json!({"tool_calls":[{"id":"unknown-call","name":"write_file","arguments":{"path":"unconfirmed.txt"}}]}),
    );
    a.session_mut().append_turn(call.clone()).unwrap();
    let mut result = Turn::with_parent(
        "unknown-result",
        "old-9",
        TurnRole::Tool,
        "Outcome unknown after restart",
    );
    result.metadata = Some(
        json!({"tool_call_id":"unknown-call","tool_name":"write_file","outcome":"unknown_outcome"}),
    );
    a.session_mut().append_turn(result.clone()).unwrap();
    a.compact_context().unwrap();
    let cp = checkpoint(&a).unwrap();
    assert_eq!(cp.unresolved_calls.len(), 1);
    assert!(!cp.modified_files.contains(&"unconfirmed.txt".into()));
    let restored = context::restore_semantic_checkpoint(
        a.history(),
        &cp,
        a.session().session_id(),
        "linear",
        BUDGET,
    )
    .unwrap();
    assert!(restored.contains(&call));
    assert!(restored.contains(&result));
}

#[test]
fn actual_http_prepare_services_late_steer_and_consumes_it_once() {
    use zenpi::{
        input_queue::{InputKind, InputQueue, InputStatus},
        protocol::{InputQueueAction, InputQueueRequest},
    };
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let release = Arc::new(AtomicBool::new(false));
    let released = release.clone();
    let peer = Peer::new(move |n, request| {
        if n == 0 {
            let body = completion(good_summary(&summary_data(request)));
            started_tx.send(()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !released.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
            body
        } else {
            assert_eq!(request.to_string().matches("late requirement").count(), 1);
            completion("final response".into())
        }
    });
    let dir = tempdir().unwrap();
    let mut a = agent(&dir.path().join("journal.jsonl"), &peer.url, false);
    seed(&mut a);
    let port = a.input_port();
    a.submit(TurnInputRequest::new("continue original goal"))
        .unwrap();
    let owner = thread::spawn(move || {
        a.run_active_turn().unwrap();
        a
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(port.is_preparing());
    let ticket = port
        .submit(InputQueueRequest {
            schema_version: 2,
            id: "enqueue-late".into(),
            kind: "input_queue".into(),
            session_id: port.session_id(),
            input_queue: InputQueueAction::Enqueue {
                input_id: "late".into(),
                kind: InputKind::Steer,
                text: "late requirement".into(),
            },
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match ticket.try_recv() {
            Ok(reply) => {
                assert!(reply.is_ok());
                break;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
            Err(e) => panic!("{e}"),
        }
    }
    release.store(true, Ordering::Release);
    let mut a = owner.join().unwrap();
    assert_eq!(peer.seen.lock().unwrap().len(), 2);
    assert_eq!(
        InputQueue::recover(a.session(), Default::default())
            .unwrap()
            .get("late")
            .unwrap()
            .status,
        InputStatus::Applied
    );
    assert_eq!(
        a.history()
            .iter()
            .filter(|t| t.content == "late requirement")
            .count(),
        1
    );
    assert!(
        !serde_json::to_string(&a.take_events())
            .unwrap()
            .contains("critical_facts")
    );
}

#[cfg(unix)]
#[test]
fn async_headless_manual_compact_remains_responsive_and_cancellable_during_http() {
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
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let release = Arc::new(AtomicBool::new(false));
    let released = release.clone();
    let peer = Peer::new(move |_, request| {
        let body = completion(good_summary(&summary_data(request)));
        started_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !released.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        body
    });
    let dir = tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");
    let mut a = agent(&path, &peer.url, false);
    seed(&mut a);
    let (mut input, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(a, reader, Output(tx, vec![])).unwrap()
    });
    writeln!(
        input,
        "{}",
        json!({"schema_version":2,"type":"command","id":"compact","text":"/compact"})
    )
    .unwrap();
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    writeln!(
        input,
        "{}",
        json!({"schema_version":2,"type":"status","id":"status"})
    )
    .unwrap();
    let mut records = Vec::new();
    loop {
        let v = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let done = v["id"] == "status" && v["type"] == "response";
        records.push(v);
        if done {
            break;
        }
    }
    writeln!(
        input,
        "{}",
        json!({"schema_version":2,"type":"cancel","id":"cancel","target_id":"compact"})
    )
    .unwrap();
    loop {
        let v = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let done = v["id"] == "compact" && v["type"] == "response";
        if done {
            assert_eq!(v["success"], false, "{v}");
        }
        records.push(v);
        if done {
            break;
        }
    }
    release.store(true, Ordering::Release);
    drop(input);
    host.join().unwrap();
    records.extend(rx.try_iter());
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("critical_facts")
    );
    assert_eq!(peer.seen.lock().unwrap().len(), 1);
    let reopened = agent(&path, &peer.url, false);
    assert!(checkpoint(&reopened).is_none());
}

#[test]
fn summary_http_failure_does_not_use_configured_automatic_retries() {
    let peer = Peer::with_status(503, |n, _| {
        assert_eq!(n, 0, "summary made an unreserved second request");
        json!({"error":"unavailable"})
    });
    let dir = tempdir().unwrap();
    let mut a = agent(&dir.path().join("journal.jsonl"), &peer.url, false);
    seed(&mut a);
    assert!(a.compact_context().is_err());
    assert_eq!(peer.seen.lock().unwrap().len(), 1);
    assert!(checkpoint(&a).is_none());
    assert_eq!(
        BudgetLedger::restore(a.session(), ResourceLimits::default())
            .unwrap()
            .usage()
            .network_requests,
        1
    );
}

#[test]
fn opaque_state_before_every_cut_fails_without_losing_history_or_starting_http() {
    let peer = Peer::new(|_, _| panic!("no safe native cut should reach provider"));
    let dir = tempdir().unwrap();
    let mut a = agent(&dir.path().join("journal.jsonl"), &peer.url, false);
    let mut first = Turn::new("pinned", TurnRole::User, "original");
    first.metadata = Some(
        json!({"annotations":[{"type":"native_history","wire":"google_generative_ai","parts":[{"thoughtSignature":"opaque"}]}]}),
    );
    a.session_mut().append_turn(first).unwrap();
    seed(&mut a);
    let original = a.history().to_vec();
    assert!(a.compact_context().is_err());
    assert_eq!(a.history(), original);
    assert!(checkpoint(&a).is_none());
    assert!(peer.seen.lock().unwrap().is_empty());
}

#[test]
fn killed_summary_process_preserves_previous_commit_and_requires_explicit_recovery() {
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let release = Arc::new(AtomicBool::new(false));
    let released = release.clone();
    let peer = Peer::new(move |n, request| {
        let body = completion(good_summary(&summary_data(request)));
        if n == 1 {
            started_tx.send(()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(8);
            while !released.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
        }
        body
    });
    let dir = tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");
    let mut a = agent(&path, &peer.url, false);
    seed(&mut a);
    a.compact_context().unwrap();
    let before = checkpoint(&a).unwrap();
    drop(a);
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "semantic_process_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("ZENPI_SEMANTIC_TEST_PATH", &path)
        .env("ZENPI_SEMANTIC_TEST_URL", &peer.url)
        .env("ZENPI_SEMANTIC_TEST_MODE", "interrupted")
        .spawn()
        .unwrap();
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    release.store(true, Ordering::Release);
    let mut a = agent(&path, &peer.url, false);
    assert_eq!(checkpoint(&a).unwrap(), before);
    assert!(!a.operation_recovery().is_empty());
    assert!(
        a.process(TurnInputRequest::new("do not silently retry"))
            .is_err()
    );
    assert_eq!(peer.seen.lock().unwrap().len(), 2);
}

#[cfg(unix)]
#[test]
fn manual_compaction_accepts_durable_queue_input_before_http_cancellation() {
    manual_compaction_queue_case(true);
    manual_compaction_queue_case(false);
}

#[cfg(unix)]
fn manual_compaction_queue_case(cancel: bool) {
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
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let release = Arc::new(AtomicBool::new(false));
    let released = release.clone();
    let peer = Peer::new(move |_, request| {
        let body = completion(good_summary(&summary_data(request)));
        started_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !released.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        body
    });
    let dir = tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");
    let mut a = agent(&path, &peer.url, false);
    seed(&mut a);
    let session_id = a.session().session_id().to_owned();
    let port = a.input_port();
    let (mut input, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(a, reader, Output(tx, vec![])).unwrap()
    });
    writeln!(
        input,
        "{}",
        json!({"schema_version":2,"type":"command","id":"compact","text":"/compact"})
    )
    .unwrap();
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    writeln!(
        input,
        "{}",
        json!({"schema_version":2,"type":"input_queue","id":"queue","session_id":session_id,"input_queue":{"action":"enqueue","input_id":"during-manual-summary","kind":"steer","text":"keep after cancellation"}})
    )
    .unwrap();
    let mut records = Vec::new();
    loop {
        let v = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let done = v["id"] == "queue" && v["type"] == "response";
        records.push(v);
        if done {
            break;
        }
    }
    if cancel {
        writeln!(
            input,
            "{}",
            json!({"schema_version":2,"type":"cancel","id":"cancel","target_id":"compact"})
        )
        .unwrap();
    } else {
        release.store(true, Ordering::Release);
    }
    loop {
        let v = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let done = v["id"] == "compact" && v["type"] == "response";
        if done {
            assert_eq!(v["success"], !cancel, "{v}");
        }
        records.push(v);
        if done {
            break;
        }
    }
    release.store(true, Ordering::Release);
    drop(input);
    host.join().unwrap();
    records.extend(rx.try_iter());
    if cancel {
        assert!(
            !serde_json::to_string(&records)
                .unwrap()
                .contains("critical_facts")
        );
    }
    assert_eq!(peer.seen.lock().unwrap().len(), 1);
    let reopened = agent(&path, &peer.url, false);
    assert_eq!(checkpoint(&reopened).is_none(), cancel);
    if let Some(evidence) = std::env::var_os("ZS1_MANUAL_QUEUE_EVIDENCE") {
        let evidence =
            std::path::PathBuf::from(evidence).join(if cancel { "cancel" } else { "success" });
        std::fs::create_dir_all(&evidence).unwrap();
        std::fs::copy(&path, evidence.join("journal.jsonl")).unwrap();
        std::fs::write(
            evidence.join("responses.json"),
            serde_json::to_vec_pretty(&records).unwrap(),
        )
        .unwrap();
        std::fs::write(
            evidence.join("requests.json"),
            serde_json::to_vec_pretty(&*peer.seen.lock().unwrap()).unwrap(),
        )
        .unwrap();
    }
    let receipt = records
        .iter()
        .find(|v| v["id"] == "queue" && v["type"] == "response")
        .unwrap();
    assert_eq!(receipt["success"], true, "queue receipt: {receipt}");
    let queue =
        zenpi::input_queue::InputQueue::recover(reopened.session(), Default::default()).unwrap();
    assert_eq!(
        queue.get("during-manual-summary").unwrap().status,
        zenpi::input_queue::InputStatus::Received
    );
    assert!(!port.is_preparing());
    assert!(
        port.submit(zenpi::protocol::InputQueueRequest {
            schema_version: 2,
            id: "after-idle".into(),
            kind: "input_queue".into(),
            session_id,
            input_queue: zenpi::protocol::InputQueueAction::List {
                after_sequence: None,
                limit: 32
            },
        })
        .unwrap_err()
        .contains("idle")
    );
    drop(reopened);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "manual_summary_queue_recovery_probe",
            "--ignored",
            "--nocapture",
        ])
        .env("ZS1_MANUAL_QUEUE_JOURNAL", &path)
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    println!(
        "manual summary cancel={cancel}, durable receipt: {receipt}; HTTP count 1; fresh-process queue recovery: {}",
        String::from_utf8_lossy(&child.stdout)
    );
}

#[test]
#[ignore = "new manual compaction test starts an independent recovery process"]
fn manual_summary_queue_recovery_probe() {
    let session =
        SessionStore::open_existing(std::env::var("ZS1_MANUAL_QUEUE_JOURNAL").unwrap()).unwrap();
    let queue = zenpi::input_queue::InputQueue::recover(&session, Default::default()).unwrap();
    let input = queue.get("during-manual-summary").unwrap();
    assert_eq!(input.status, zenpi::input_queue::InputStatus::Received);
    assert_eq!(input.text, "keep after cancellation");
    assert_eq!(
        session
            .events()
            .iter()
            .filter(|e| e["type"] == "input_queue" && e["change"]["action"] == "applied")
            .count(),
        0
    );
    println!(
        "independent process {} recovered received ID {}",
        std::process::id(),
        input.id
    );
}

#[test]
fn actual_http_deferred_summary_annotation_preserves_previous_checkpoint() {
    let mut failures = Vec::new();
    for field in ["finish_reason", "stop_reason", "stopReason", "status"] {
        let peer = Peer::new(move |n, request| {
            let mut body = completion(good_summary(&summary_data(request)));
            if n == 1 {
                // The transport completed, but the summary itself is deferred.
                let mut annotation = json!({"type":"summary_state"});
                annotation[field] = json!("deferred");
                body["annotations"] = json!([annotation]);
            }
            body
        });
        let dir = tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let mut a = agent(&path, &peer.url, false);
        seed(&mut a);
        a.compact_context().unwrap();
        let before = checkpoint(&a).unwrap();
        append_more(&mut a);
        let outcome = a.compact_context();
        let after = checkpoint(&a).unwrap();
        let rejected = outcome.is_err();
        let preserved = after == before;
        let requests = peer.seen.lock().unwrap().clone();
        if let Some(evidence) = std::env::var_os("ZS1_DEFERRED_SUMMARY_EVIDENCE") {
            let evidence = std::path::PathBuf::from(evidence).join(field);
            std::fs::create_dir_all(&evidence).unwrap();
            std::fs::copy(&path, evidence.join("journal.jsonl")).unwrap();
            std::fs::write(
                evidence.join("observation.json"),
                serde_json::to_vec_pretty(&json!({
                    "field":field,"rejected":rejected,"checkpoint_preserved":preserved,
                    "before":before,"after":after,"requests":requests,
                    "error":outcome.as_ref().err().map(ToString::to_string)
                }))
                .unwrap(),
            )
            .unwrap();
        }
        drop(a);
        let reopened = agent(&path, &peer.url, false);
        let reopened_preserved = checkpoint(&reopened).unwrap() == before;
        println!(
            "deferred {field}: rejected={rejected}, preserved={preserved}, reopened={reopened_preserved}, HTTP={}",
            requests.len()
        );
        assert_eq!(requests.len(), 2, "no retry after deferred {field}");
        if !rejected || !preserved || !reopened_preserved {
            failures.push(field);
        }
    }
    assert!(
        failures.is_empty(),
        "deferred summary replaced valid context for {failures:?}"
    );
}

#[test]
fn actual_http_valid_summary_annotation_allows_checkpoint_progress() {
    let peer = Peer::new(|_, request| {
        let mut body = completion(good_summary(&summary_data(request)));
        body["annotations"] = json!([{
            "type":"url_citation","url":"https://example.invalid/notes",
            "title":"deferred work remains pending"
        }]);
        body
    });
    let dir = tempdir().unwrap();
    let path = dir.path().join("journal.jsonl");
    let mut a = agent(&path, &peer.url, false);
    seed(&mut a);
    a.compact_context().unwrap();
    let first = checkpoint(&a).unwrap();
    append_more(&mut a);
    a.compact_context().unwrap();
    let second = checkpoint(&a).unwrap();
    assert!(second.source_end > first.source_end);
    drop(a);
    let reopened = agent(&path, &peer.url, false);
    assert_eq!(checkpoint(&reopened).unwrap(), second);
    assert_eq!(peer.seen.lock().unwrap().len(), 2);
}
