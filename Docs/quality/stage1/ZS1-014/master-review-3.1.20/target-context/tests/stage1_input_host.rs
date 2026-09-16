use serde_json::{Map, Value, json};
use std::{
    io::Write,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use tempfile::tempdir;
use zenpi::{
    backend::{Backend, BackendError, Completion, CompletionRequest, ProviderEvent},
    core::{Agent, Turn, TurnInputRequest, TurnRole},
    input_queue::{
        InputKind, InputPort, InputQueue, InputQueueReply, InputStatus, InputTicket, QueueMode,
    },
    protocol::{InputQueueAction as A, InputQueueRequest},
    session::SessionStore,
    tools::{
        SideEffectPolicy, Tool, ToolCall, ToolContext, ToolDefinition, ToolError,
        ToolExecutionMode, ToolRegistry, ToolSideEffect,
    },
};

fn request(session: &str, id: &str, action: A) -> InputQueueRequest {
    InputQueueRequest {
        schema_version: 2,
        id: id.into(),
        kind: "input_queue".into(),
        session_id: session.into(),
        input_queue: action,
    }
}
fn enqueue(id: &str, kind: InputKind, text: &str) -> A {
    A::Enqueue {
        input_id: id.into(),
        kind,
        text: text.into(),
    }
}
fn ticket_reply(ticket: &InputTicket) -> Result<InputQueueReply, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match ticket.try_recv() {
            Ok(reply) => return reply,
            Err(mpsc::TryRecvError::Disconnected) => panic!("owner lost ticket"),
            Err(mpsc::TryRecvError::Empty) => {
                assert!(
                    Instant::now() < deadline,
                    "owner did not acknowledge during running tool"
                );
                thread::sleep(Duration::from_millis(1));
            }
        }
    }
}
fn send(port: &InputPort, id: &str, action: A) -> InputQueueReply {
    ticket_reply(
        &port
            .submit(request(&port.session_id(), id, action))
            .unwrap(),
    )
    .unwrap()
}
fn input(reply: InputQueueReply) -> zenpi::input_queue::QueuedInput {
    match reply {
        InputQueueReply::Input { input, .. } => input,
        _ => panic!("expected input"),
    }
}
#[derive(Default)]
struct Controls {
    release: [AtomicBool; 2],
    finished: AtomicUsize,
}
struct HoldingRead {
    controls: Arc<Controls>,
    started: mpsc::Sender<usize>,
    parallel: bool,
}
impl Tool for HoldingRead {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "holding_read".into(),
            description: "Read a real file after a test-controlled boundary".into(),
            input_schema: json!({"type":"object"}),
            side_effect: ToolSideEffect::ReadOnly,
        }
    }
    fn execution_mode(&self) -> ToolExecutionMode {
        if self.parallel {
            ToolExecutionMode::Parallel
        } else {
            ToolExecutionMode::Sequential
        }
    }
    fn invoke(&self, ctx: &ToolContext, args: &Map<String, Value>) -> Result<Value, ToolError> {
        self.invoke_cancellable(ctx, args, &|| false)
    }
    fn invoke_cancellable(
        &self,
        ctx: &ToolContext,
        args: &Map<String, Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        let slot = args["slot"].as_u64().unwrap() as usize;
        self.started.send(slot).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.controls.release[slot].load(Ordering::Acquire) {
            if cancelled() {
                self.controls.finished.fetch_add(1, Ordering::AcqRel);
                return Err(ToolError::Cancelled);
            }
            assert!(
                Instant::now() < deadline,
                "test did not release tool {slot}"
            );
            thread::sleep(Duration::from_millis(1));
        }
        let value = zenpi::tools::ReadFileTool.invoke(
            ctx,
            &json!({"path":"source.txt"}).as_object().unwrap().clone(),
        )?;
        self.controls.finished.fetch_add(1, Ordering::AcqRel);
        Ok(value)
    }
}
type Requests = Arc<Mutex<Vec<(String, Vec<Turn>)>>>;
struct RecordingBackend {
    seen: Requests,
    calls: AtomicUsize,
    controls: Arc<Controls>,
}
impl Backend for RecordingBackend {
    fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        let n = self.calls.fetch_add(1, Ordering::AcqRel);
        self.seen
            .lock()
            .unwrap()
            .push((request.turn_id.into(), request.turns.to_vec()));
        if n == 0 {
            let mut completion = Completion::text("");
            completion.tool_calls = (0..2)
                .map(|slot| ToolCall {
                    id: format!("read-{slot}"),
                    name: "holding_read".into(),
                    arguments: json!({"slot":slot}),
                })
                .collect();
            Ok(completion)
        } else {
            assert_eq!(
                self.controls.finished.load(Ordering::Acquire),
                2,
                "provider resumed before whole batch joined"
            );
            Ok(Completion::text(format!("answer-{n}")))
        }
    }
}
fn setup(
    path: &std::path::Path,
    parallel: bool,
) -> (Agent, Requests, Arc<Controls>, mpsc::Receiver<usize>) {
    std::fs::write(path.join("source.txt"), "actual file").unwrap();
    let controls = Arc::new(Controls::default());
    let seen = Requests::default();
    let (tx, rx) = mpsc::channel();
    let mut registry = ToolRegistry::new();
    registry
        .register(HoldingRead {
            controls: controls.clone(),
            started: tx,
            parallel,
        })
        .unwrap();
    let mut agent = Agent::new(
        SessionStore::open(path.join("session.jsonl")).unwrap(),
        Box::new(RecordingBackend {
            seen: seen.clone(),
            controls: controls.clone(),
            calls: AtomicUsize::new(0),
        }),
    );
    agent.set_tools(
        registry,
        ToolContext::new(path).unwrap(),
        SideEffectPolicy::read_only(),
    );
    (agent, seen, controls, rx)
}
fn user_texts(turns: &[Turn]) -> Vec<&str> {
    turns
        .iter()
        .filter(|t| t.role == TurnRole::User)
        .map(|t| t.content.as_str())
        .collect()
}
fn batch_boundary(parallel: bool) {
    let dir = tempdir().unwrap();
    let (mut agent, seen, controls, started) = setup(dir.path(), parallel);
    let turn_id = agent
        .submit(TurnInputRequest::new("original"))
        .unwrap()
        .turn_id()
        .unwrap()
        .to_owned();
    let port = agent.input_port();
    let owner = thread::spawn(move || {
        let answer = agent.run_active_turn().unwrap().unwrap();
        (agent, answer)
    });
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    if parallel {
        started.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    assert_eq!(
        input(send(
            &port,
            "receive-a",
            enqueue("a", InputKind::Steer, "old steer")
        ))
        .status,
        InputStatus::Received
    );
    send(
        &port,
        "receive-f",
        enqueue("f", InputKind::FollowUp, "follow later"),
    );
    send(
        &port,
        "receive-b",
        enqueue("b", InputKind::Steer, "cancel me"),
    );
    send(
        &port,
        "edit-a",
        A::Edit {
            input_id: "a".into(),
            expected_revision: 0,
            text: "edited steer".into(),
        },
    );
    assert_eq!(
        input(send(
            &port,
            "cancel-b",
            A::Cancel {
                input_id: "b".into()
            }
        ))
        .status,
        InputStatus::Cancelled
    );
    controls.release[0].store(true, Ordering::Release);
    if !parallel {
        assert_eq!(started.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
    }
    let InputQueueReply::Page { inputs, .. } = send(
        &port,
        "inspect",
        A::List {
            after_sequence: None,
            limit: 32,
        },
    ) else {
        panic!()
    };
    assert_eq!(
        inputs.iter().find(|i| i.id == "a").unwrap().status,
        InputStatus::Received
    );
    assert_eq!(seen.lock().unwrap().len(), 1, "half batch consumed input");
    controls.release[1].store(true, Ordering::Release);
    let (mut agent, answer) = owner.join().unwrap();
    assert_eq!(answer.content, "answer-2");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(
        seen.iter().all(|(id, _)| id == &turn_id),
        "queue input created another conversation"
    );
    assert_eq!(user_texts(&seen[1].1), ["original", "edited steer"]);
    assert_eq!(
        user_texts(&seen[2].1),
        ["original", "edited steer", "follow later"]
    );
    assert_eq!(
        seen[1]
            .1
            .iter()
            .filter(|t| t.role == TurnRole::Tool)
            .count(),
        2
    );
    let queue = InputQueue::recover(agent.session(), Default::default()).unwrap();
    assert_eq!(
        queue.get("a").unwrap().applied_parent_id.as_deref(),
        Some(turn_id.as_str())
    );
    assert_eq!(queue.get("f").unwrap().status, InputStatus::Applied);
    assert!(
        agent
            .input_queue_request(request(
                &port.session_id(),
                "late-edit",
                A::Edit {
                    input_id: "a".into(),
                    expected_revision: 1,
                    text: "too late".into()
                }
            ))
            .is_err()
    );
    assert!(
        port.submit(request(
            &port.session_id(),
            "stale",
            enqueue("stale", InputKind::Steer, "stale")
        ))
        .is_err()
    );
}
#[test]
fn serial_batch_receipts_edit_cancel_and_follow_up_use_real_owner() {
    batch_boundary(false);
}
#[test]
fn parallel_batch_waits_for_every_result_before_steer_context() {
    batch_boundary(true);
}

#[test]
fn all_mode_groups_each_lane_and_keeps_follow_ups_at_would_stop() {
    let dir = tempdir().unwrap();
    let (mut agent, seen, controls, started) = setup(dir.path(), true);
    let port = agent.input_port();
    for (id, kind) in [
        ("mode-s", InputKind::Steer),
        ("mode-f", InputKind::FollowUp),
    ] {
        agent
            .input_queue_request(request(
                &port.session_id(),
                id,
                A::Configure {
                    kind,
                    mode: QueueMode::All,
                },
            ))
            .unwrap();
    }
    agent.submit(TurnInputRequest::new("original")).unwrap();
    let owner = thread::spawn(move || {
        agent.run_active_turn().unwrap();
        agent
    });
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    for (id, kind) in [
        ("s1", InputKind::Steer),
        ("s2", InputKind::Steer),
        ("f1", InputKind::FollowUp),
        ("f2", InputKind::FollowUp),
    ] {
        send(&port, id, enqueue(id, kind, id));
    }
    for release in &controls.release {
        release.store(true, Ordering::Release);
    }
    let agent = owner.join().unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert_eq!(user_texts(&seen[1].1), ["original", "s1", "s2"]);
    assert_eq!(user_texts(&seen[2].1), ["original", "s1", "s2", "f1", "f2"]);
    assert_eq!(
        agent
            .session()
            .events()
            .iter()
            .filter(|e| e["type"] == "input_queue" && e["change"]["action"] == "applied")
            .count(),
        2
    );
}

#[test]
fn main_turn_cancel_preserves_received_follow_up_and_joins_parallel_tools() {
    let dir = tempdir().unwrap();
    let (mut agent, seen, controls, started) = setup(dir.path(), true);
    agent.submit(TurnInputRequest::new("original")).unwrap();
    let port = agent.input_port();
    let cancel = Arc::new(AtomicBool::new(false));
    let owner_cancel = cancel.clone();
    let owner = thread::spawn(move || {
        let result = agent.run_active_turn_cancelable(|| owner_cancel.load(Ordering::Acquire));
        (agent, result)
    });
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    send(&port, "f", enqueue("f", InputKind::FollowUp, "later"));
    cancel.store(true, Ordering::Release);
    let (mut agent, result) = owner.join().unwrap();
    assert!(result.is_err());
    assert_eq!(controls.finished.load(Ordering::Acquire), 2);
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(
        InputQueue::recover(agent.session(), Default::default())
            .unwrap()
            .get("f")
            .unwrap()
            .status,
        InputStatus::Received
    );
    let reply = agent
        .input_queue_request(request(
            &port.session_id(),
            "cancel-f",
            A::Cancel {
                input_id: "f".into(),
            },
        ))
        .unwrap();
    assert_eq!(input(reply).status, InputStatus::Cancelled);
}

struct JsonOutput {
    partial: Vec<u8>,
    tx: mpsc::Sender<Value>,
}
impl Write for JsonOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.partial.extend_from_slice(bytes);
        while let Some(end) = self.partial.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = self.partial.drain(..=end).collect();
            if line.len() > 1 {
                let _ = self.tx.send(serde_json::from_slice(&line).unwrap());
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn receive_until(
    rx: &mpsc::Receiver<Value>,
    records: &mut Vec<Value>,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    loop {
        let record = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("headless response/event timeout");
        let matches = predicate(&record);
        records.push(record.clone());
        if matches {
            return record;
        }
    }
}
#[cfg(unix)]
#[test]
fn headless_jsonl_and_slash_share_busy_queue_owner_without_cancelling_tools() {
    let dir = tempdir().unwrap();
    let (agent, seen, controls, started) = setup(dir.path(), true);
    let session_id = agent.session().session_id().to_owned();
    let (mut writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let (tx, rx) = mpsc::channel();
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(
            agent,
            reader,
            JsonOutput {
                partial: vec![],
                tx,
            },
        )
        .unwrap()
    });
    let mut records = vec![];
    writeln!(
        writer,
        "{}",
        json!({"schema_version":2,"id":"prompt","type":"prompt","text":"original"})
    )
    .unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    for (id, action) in [
        ("receive-s", enqueue("s", InputKind::Steer, "old")),
        ("receive-f", enqueue("f", InputKind::FollowUp, "follow")),
    ] {
        writeln!(writer, "{}", json!(request(&session_id, id, action))).unwrap();
        let response = receive_until(&rx, &mut records, |v| {
            v["type"] == "response" && v["id"] == id
        });
        assert_eq!(
            response["data"]["input"]["status"], "received",
            "{response}"
        );
    }
    writeln!(
        writer,
        "{}",
        json!({"schema_version":2,"id":"edit","type":"command","text":"/input edit s 0 edited"})
    )
    .unwrap();
    let edit = receive_until(&rx, &mut records, |v| {
        v["type"] == "response" && v["id"] == "edit"
    });
    assert_eq!(edit["data"]["input"]["text"], "edited", "{edit}");
    // Distinct transport IDs still reuse the durable input ID, with no second enqueue.
    writeln!(
        writer,
        "{}",
        json!(request(
            &session_id,
            "duplicate",
            enqueue("s", InputKind::Steer, "old")
        ))
    )
    .unwrap();
    let duplicate = receive_until(&rx, &mut records, |v| {
        v["type"] == "response" && v["id"] == "duplicate"
    });
    assert_eq!(duplicate["data"]["duplicate"], true);
    writeln!(
        writer,
        "{}",
        json!(request(
            &session_id,
            "conflict",
            enqueue("s", InputKind::Steer, "different")
        ))
    )
    .unwrap();
    let conflict = receive_until(&rx, &mut records, |v| {
        v["type"] == "response" && v["id"] == "conflict"
    });
    assert!(conflict["success"] == false);
    writeln!(
        writer,
        "{}",
        json!(request(
            "different-session",
            "wrong",
            enqueue("bad", InputKind::Steer, "wrong session")
        ))
    )
    .unwrap();
    let wrong = receive_until(&rx, &mut records, |v| {
        v["type"] == "response" && v["id"] == "wrong"
    });
    assert!(wrong["success"] == false);
    assert_eq!(
        controls.finished.load(Ordering::Acquire),
        0,
        "queue command cancelled a running tool"
    );
    for release in &controls.release {
        release.store(true, Ordering::Release);
    }
    let terminal = receive_until(&rx, &mut records, |v| {
        v["type"] == "response" && v["id"] == "prompt"
    });
    assert!(terminal["error"].is_null(), "{terminal}");
    drop(writer);
    host.join().unwrap();
    records.extend(rx.try_iter());
    let seen = seen.lock().unwrap();
    assert_eq!(user_texts(&seen[1].1), ["original", "edited"]);
    assert_eq!(user_texts(&seen[2].1), ["original", "edited", "follow"]);
    let accepted = records
        .iter()
        .position(|v| v["event"]["type"] == "turn_accepted")
        .unwrap();
    let delta = records
        .iter()
        .position(|v| v["event"]["type"] == "text_delta")
        .unwrap();
    assert!(accepted < delta);
    assert!(records[delta]["event"]["view"]["turn_id"].is_string());
    let session = SessionStore::open_existing(dir.path().join("session.jsonl")).unwrap();
    assert_eq!(
        session
            .turns()
            .iter()
            .filter(|t| t
                .metadata
                .as_ref()
                .is_some_and(|m| m["input_queue"]["input_id"] == "s"))
            .count(),
        1
    );
}

struct PausedBackend {
    first: AtomicBool,
    started: mpsc::Sender<()>,
    release: Arc<AtomicBool>,
    seen: Requests,
}
impl Backend for PausedBackend {
    fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        unreachable!()
    }
    fn complete_with_control(
        &self,
        request: CompletionRequest<'_>,
        cancelled: &dyn Fn() -> bool,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        self.seen
            .lock()
            .unwrap()
            .push((request.turn_id.into(), request.turns.to_vec()));
        if !self.first.swap(true, Ordering::AcqRel) {
            self.started.send(()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !self.release.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
        }
        if cancelled() {
            return Err(BackendError::Cancelled);
        }
        sink(ProviderEvent::TextDelta {
            delta: "done".into(),
        })?;
        Ok(Completion::text("done"))
    }
}
#[test]
fn cancelling_a_ticket_does_not_cancel_provider_or_another_ticket() {
    let dir = tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let release = Arc::new(AtomicBool::new(false));
    let seen = Requests::default();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("session.jsonl")).unwrap(),
        Box::new(PausedBackend {
            first: AtomicBool::new(false),
            started: tx,
            release: release.clone(),
            seen: seen.clone(),
        }),
    );
    agent.submit(TurnInputRequest::new("original")).unwrap();
    let port = agent.input_port();
    let owner = thread::spawn(move || {
        agent.run_active_turn().unwrap();
        agent
    });
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let cancel = port
        .submit(request(
            &port.session_id(),
            "cancel-ticket",
            enqueue("cancelled-ticket", InputKind::Steer, "never apply"),
        ))
        .unwrap();
    cancel.cancel();
    let live = port
        .submit(request(
            &port.session_id(),
            "live",
            enqueue("live", InputKind::Steer, "live input"),
        ))
        .unwrap();
    release.store(true, Ordering::Release);
    assert!(ticket_reply(&cancel).is_err());
    assert_eq!(
        input(ticket_reply(&live).unwrap()).status,
        InputStatus::Received
    );
    let agent = owner.join().unwrap();
    let queue = InputQueue::recover(agent.session(), Default::default()).unwrap();
    assert!(queue.get("cancelled-ticket").is_none());
    assert_eq!(queue.get("live").unwrap().status, InputStatus::Applied);
    assert_eq!(seen.lock().unwrap().len(), 2);
}

struct CancelBeforeDelta {
    started: mpsc::Sender<()>,
}
impl Backend for CancelBeforeDelta {
    fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        unreachable!()
    }
    fn complete_with_control(
        &self,
        _: CompletionRequest<'_>,
        cancelled: &dyn Fn() -> bool,
        _: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        self.started.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !cancelled() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        Err(BackendError::Cancelled)
    }
}
#[cfg(unix)]
#[test]
fn explicit_shutdown_can_cancel_after_admission_before_any_provider_delta() {
    let dir = tempdir().unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let agent = Agent::new(
        SessionStore::open(dir.path().join("session.jsonl")).unwrap(),
        Box::new(CancelBeforeDelta {
            started: started_tx,
        }),
    );
    let (mut writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let (tx, rx) = mpsc::channel();
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(
            agent,
            reader,
            JsonOutput {
                partial: vec![],
                tx,
            },
        )
        .unwrap()
    });
    writeln!(
        writer,
        "{}",
        json!({"schema_version":2,"id":"prompt","type":"prompt","text":"hello"})
    )
    .unwrap();
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    writeln!(
        writer,
        "{}",
        json!({"schema_version":2,"id":"shutdown","type":"shutdown"})
    )
    .unwrap();
    drop(writer);
    host.join().unwrap();
    let records: Vec<_> = rx.try_iter().collect();
    assert!(
        records
            .iter()
            .any(|r| r["event"]["type"] == "turn_accepted")
    );
    assert!(!records.iter().any(|r| r["event"]["type"] == "text_delta"));
    let response = records
        .iter()
        .find(|r| r["type"] == "response" && r["id"] == "prompt")
        .unwrap();
    assert!(response["success"] == false, "{response}");
    assert!(
        records
            .iter()
            .any(|r| r["id"] == "shutdown" && r["data"]["drained"] == true)
    );
}

struct CaptureOnly(Requests);
impl Backend for CaptureOnly {
    fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        if request
            .metadata
            .is_some_and(|v| v["purpose"] == "semantic_compaction")
        {
            let data: Value = serde_json::from_str(&request.turns[0].content).unwrap();
            let mut completion = Completion::text(
                json!({
                    "goals":["Continue the original task"],"constraints":[],"decisions":[],
                    "progress":["Earlier history reviewed"],"pending_tasks":[],"critical_facts":[],
                    "read_files":data["read_files"],"modified_files":data["modified_files"],
                    "unresolved_tools":data["unresolved_tools"],
                })
                .to_string(),
            );
            completion.model = Some("fixture-summary".into());
            completion.usage = Some(zenpi::backend::Usage {
                input_tokens: 100,
                output_tokens: 100,
                total_tokens: 200,
            });
            return Ok(completion);
        }
        self.0
            .lock()
            .unwrap()
            .push((request.turn_id.into(), request.turns.to_vec()));
        Ok(Completion::text("complete"))
    }
}
fn prepare_arrivals(initial: bool, all: bool) {
    let dir = tempdir().unwrap();
    let seen = Requests::default();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("prepare.jsonl")).unwrap(),
        Box::new(CaptureOnly(seen.clone())),
    );
    for n in 0..10 {
        agent
            .session_mut()
            .append_turn(Turn::new(
                format!("old-{n}"),
                TurnRole::User,
                "old history ".repeat(110),
            ))
            .unwrap();
    }
    agent.set_context_budget(zenpi::context::ContextBudget {
        max_tokens: 4000,
        reserved_output_tokens: 1000,
    });
    let port = agent.input_port();
    if all {
        agent
            .input_queue_request(request(
                &port.session_id(),
                "mode",
                A::Configure {
                    kind: InputKind::Steer,
                    mode: QueueMode::All,
                },
            ))
            .unwrap();
    }
    if initial {
        agent
            .input_queue_request(request(
                &port.session_id(),
                "early",
                enqueue("early", InputKind::Steer, "early"),
            ))
            .unwrap();
    }
    agent.submit(TurnInputRequest::new("original")).unwrap();
    let injected = AtomicBool::new(false);
    let tickets = Mutex::new(Vec::new());
    agent
        .run_active_turn_cancelable(|| {
            if port.is_preparing() && !injected.swap(true, Ordering::AcqRel) {
                for id in ["late-a", "late-b"] {
                    tickets.lock().unwrap().push(
                        port.submit(request(
                            &port.session_id(),
                            id,
                            enqueue(id, InputKind::Steer, id),
                        ))
                        .unwrap(),
                    );
                }
            }
            false
        })
        .unwrap();
    assert!(
        injected.load(Ordering::Acquire),
        "fixture did not reach actual compaction preparation"
    );
    for ticket in tickets.lock().unwrap().iter() {
        assert!(ticket_reply(ticket).is_ok());
    }
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.len(),
        if all {
            if initial { 2 } else { 1 }
        } else {
            if initial { 3 } else { 2 }
        }
    );
    let first = user_texts(&seen[0].1);
    assert_eq!(first.contains(&"early"), initial);
    assert_eq!(first.contains(&"late-a"), !initial);
    assert_eq!(first.contains(&"late-b"), !initial && all);
    let queue = InputQueue::recover(agent.session(), Default::default()).unwrap();
    for id in ["late-a", "late-b"] {
        assert_eq!(queue.get(id).unwrap().status, InputStatus::Applied);
        assert_eq!(
            agent
                .session()
                .turns()
                .iter()
                .filter(|t| t
                    .metadata
                    .as_ref()
                    .is_some_and(|m| m["input_queue"]["input_id"] == id))
                .count(),
            1
        );
    }
    assert!(!port.is_preparing());
}
#[test]
fn empty_prepare_poll_consumes_late_input_once_in_one_mode() {
    prepare_arrivals(false, false);
}
#[test]
fn nonempty_prepare_poll_keeps_late_input_for_later_model_turns() {
    prepare_arrivals(true, false);
}
#[test]
fn empty_prepare_poll_respects_all_mode() {
    prepare_arrivals(false, true);
}
#[test]
fn nonempty_prepare_poll_freezes_even_all_mode() {
    prepare_arrivals(true, true);
}

#[test]
#[ignore = "separate-process crash-window helper"]
fn input_host_crash_probe() {
    let path = std::env::var("ZENPI_INPUT_CRASH_PATH").unwrap();
    let stage = std::env::var("ZENPI_INPUT_CRASH_STAGE").unwrap();
    let mut session = SessionStore::open(path).unwrap();
    session
        .append_turn(Turn::new("original-turn", TurnRole::User, "original"))
        .unwrap();
    let mut queue = InputQueue::recover(&session, Default::default()).unwrap();
    queue
        .execute(&mut session, enqueue("a", InputKind::Steer, "committed-a"))
        .unwrap();
    queue
        .execute(&mut session, enqueue("b", InputKind::Steer, "pending-b"))
        .unwrap();
    if stage != "received" {
        let gate = zenpi::runtime::InputBoundaryGate::new("next-model")
            .unwrap()
            .with_context_parent("original-turn")
            .unwrap();
        queue
            .apply_boundary(&mut session, &gate.boundary(false).unwrap())
            .unwrap();
    }
    if stage == "projection" {
        queue.repair_projections(&mut session).unwrap();
    }
    // No destructors/close markers: only the fsynced records survive.
    std::process::exit(91);
}
#[test]
fn separate_process_received_applied_and_projection_crash_windows_recover_before_provider() {
    for stage in ["received", "applied", "projection"] {
        let dir = tempdir().unwrap();
        let path = dir.path().join("crash.jsonl");
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "input_host_crash_probe", "--ignored"])
            .env("ZENPI_INPUT_CRASH_PATH", &path)
            .env("ZENPI_INPUT_CRASH_STAGE", stage)
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(91),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        let seen = Requests::default();
        let mut agent = Agent::new(
            SessionStore::open_existing(&path).unwrap(),
            Box::new(CaptureOnly(seen.clone())),
        );
        let session_id = agent.session().session_id().to_owned();
        let duplicate = agent
            .input_queue_request(request(
                &session_id,
                "replay",
                enqueue("a", InputKind::Steer, "committed-a"),
            ))
            .unwrap();
        assert!(matches!(
            duplicate,
            InputQueueReply::Input {
                duplicate: true,
                ..
            }
        ));
        agent
            .process(TurnInputRequest::new("resume explicitly"))
            .unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), if stage == "received" { 2 } else { 1 });
        assert_eq!(
            user_texts(&seen[0].1)
                .iter()
                .filter(|s| **s == "committed-a")
                .count(),
            1,
            "provider ran without repaired projection at {stage}"
        );
        assert!(
            seen.last()
                .unwrap()
                .1
                .iter()
                .any(|t| t.content == "pending-b")
        );
        for id in ["a", "b"] {
            assert_eq!(
                agent
                    .session()
                    .events()
                    .iter()
                    .filter(|e| e["type"] == "input_queue"
                        && e["change"]["action"] == "applied"
                        && e["change"]["ids"]
                            .as_array()
                            .is_some_and(|ids| ids.iter().any(|v| v == id)))
                    .count(),
                1,
                "reapplied {id} after {stage}"
            );
            assert_eq!(
                agent
                    .session()
                    .turns()
                    .iter()
                    .filter(|t| t
                        .metadata
                        .as_ref()
                        .is_some_and(|m| m["input_queue"]["input_id"] == id))
                    .count(),
                1
            );
        }
        drop(agent);
        // A second reopening has no missing projection and never replays applied IDs.
        let seen2 = Requests::default();
        let mut again = Agent::new(
            SessionStore::open_existing(&path).unwrap(),
            Box::new(CaptureOnly(seen2.clone())),
        );
        again
            .process(TurnInputRequest::new("next user turn"))
            .unwrap();
        assert_eq!(seen2.lock().unwrap().len(), 1);
        assert_eq!(
            again
                .session()
                .turns()
                .iter()
                .filter(|t| t.content == "committed-a")
                .count(),
            1
        );
    }
}

#[test]
fn projection_collision_fails_closed_before_new_user_admission_or_provider() {
    use sha2::{Digest, Sha256};
    let dir = tempdir().unwrap();
    let mut session = SessionStore::open(dir.path().join("collision.jsonl")).unwrap();
    session
        .append_turn(Turn::new("original-turn", TurnRole::User, "original"))
        .unwrap();
    let mut queue = InputQueue::recover(&session, Default::default()).unwrap();
    queue
        .execute(
            &mut session,
            enqueue("same-input", InputKind::Steer, "intended"),
        )
        .unwrap();
    let gate = zenpi::runtime::InputBoundaryGate::new("model")
        .unwrap()
        .with_context_parent("original-turn")
        .unwrap();
    queue
        .apply_boundary(&mut session, &gate.boundary(false).unwrap())
        .unwrap();
    let key = serde_json::to_vec(&(session.session_id(), "same-input")).unwrap();
    session
        .append_turn(Turn::new(
            format!("queued-input-{:x}", Sha256::digest(key)),
            TurnRole::User,
            "user's conflicting turn",
        ))
        .unwrap();
    let count = session.turns().len();
    let seen = Requests::default();
    let mut agent = Agent::new(session, Box::new(CaptureOnly(seen.clone())));
    assert!(
        agent
            .process(TurnInputRequest::new("must not admit"))
            .is_err()
    );
    assert_eq!(agent.session().turns().len(), count);
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn ticket_and_durable_queue_capacity_are_visible_without_eviction() {
    let dir = tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let release = Arc::new(AtomicBool::new(false));
    let seen = Requests::default();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("capacity.jsonl")).unwrap(),
        Box::new(PausedBackend {
            first: AtomicBool::new(false),
            started: tx,
            release: release.clone(),
            seen: seen.clone(),
        }),
    );
    let port = agent.input_port();
    agent
        .input_queue_request(request(
            &port.session_id(),
            "all",
            A::Configure {
                kind: InputKind::Steer,
                mode: QueueMode::All,
            },
        ))
        .unwrap();
    agent.submit(TurnInputRequest::new("original")).unwrap();
    let owner = thread::spawn(move || {
        agent.run_active_turn().unwrap();
        agent
    });
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let tickets: Vec<_> = (0..32)
        .map(|i| {
            let id = format!("input-{i}");
            port.submit(request(
                &port.session_id(),
                &id,
                enqueue(&id, InputKind::Steer, &id),
            ))
            .unwrap()
        })
        .collect();
    assert!(
        port.submit(request(
            &port.session_id(),
            "overflow",
            enqueue("overflow", InputKind::Steer, "overflow")
        ))
        .is_err()
    );
    release.store(true, Ordering::Release);
    for ticket in &tickets {
        assert!(ticket_reply(ticket).is_ok());
    }
    let mut agent = owner.join().unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
    for i in 0..32 {
        let id = format!("pending-{i}");
        agent
            .input_queue_request(request(
                &port.session_id(),
                &id,
                enqueue(&id, InputKind::FollowUp, &id),
            ))
            .unwrap();
    }
    assert!(
        agent
            .input_queue_request(request(
                &port.session_id(),
                "pending-overflow",
                enqueue("pending-overflow", InputKind::FollowUp, "overflow")
            ))
            .is_err()
    );
    let queue = InputQueue::recover(agent.session(), Default::default()).unwrap();
    assert!(queue.get("pending-0").is_some());
    assert!(queue.get("pending-overflow").is_none());
}

#[test]
fn cancelled_turn_rejects_late_tickets_before_new_turn_or_project_owner() {
    let dir = tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let release = Arc::new(AtomicBool::new(false));
    let seen = Requests::default();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("old.jsonl")).unwrap(),
        Box::new(PausedBackend {
            first: AtomicBool::new(false),
            started: tx,
            release: release.clone(),
            seen: seen.clone(),
        }),
    );
    agent.submit(TurnInputRequest::new("original")).unwrap();
    let port = agent.input_port();
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    let owner = thread::spawn(move || {
        let result = agent.run_active_turn_cancelable(|| worker_cancelled.load(Ordering::Acquire));
        assert!(result.is_err());
        agent
    });
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let late = port
        .submit(request(
            &port.session_id(),
            "late",
            enqueue("late", InputKind::Steer, "old turn only"),
        ))
        .unwrap();
    cancelled.store(true, Ordering::Release);
    release.store(true, Ordering::Release);
    assert!(ticket_reply(&late).is_err());
    let mut agent = owner.join().unwrap();
    agent.process(TurnInputRequest::new("new turn")).unwrap();
    assert!(
        InputQueue::recover(agent.session(), Default::default())
            .unwrap()
            .get("late")
            .is_none()
    );
    assert!(
        !seen
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .1
            .iter()
            .any(|t| t.content == "old turn only")
    );
    agent
        .start_steer_reissue("reissued turn".into(), "legacy-parent")
        .unwrap();
    let reissued = port
        .submit(request(
            &port.session_id(),
            "reissued-input",
            enqueue("reissued-input", InputKind::Steer, "for reissued turn"),
        ))
        .unwrap();
    agent.run_active_turn().unwrap();
    assert!(ticket_reply(&reissued).is_ok());
    assert_eq!(
        InputQueue::recover(agent.session(), Default::default())
            .unwrap()
            .get("reissued-input")
            .unwrap()
            .status,
        InputStatus::Applied
    );
    let mut other = Agent::with_echo(SessionStore::open(dir.path().join("other.jsonl")).unwrap());
    assert!(
        other
            .input_queue_request(request(
                &port.session_id(),
                "wrong-project",
                enqueue("wrong-project", InputKind::Steer, "old project")
            ))
            .is_err()
    );
    assert!(other.session().events().is_empty());
}

#[test]
fn strict_queue_and_shared_slash_controls_are_local_and_validate_before_mutation() {
    use zenpi::protocol::{Command, parse_line};
    assert!(matches!(
        parse_line(
            &json!(request(
                "session",
                "r",
                enqueue("i", InputKind::Steer, "text")
            ))
            .to_string()
        )
        .unwrap()
        .into_command()
        .unwrap(),
        Command::InputQueue(_)
    ));
    let mut extra = json!(request(
        "session",
        "r",
        enqueue("i", InputKind::Steer, "text")
    ));
    extra["text"] = json!("ambiguous prompt");
    assert!(parse_line(&extra.to_string()).is_err());
    for text in [
        "/input mode steer all",
        "/input mode follow-up one-at-a-time",
        "/input list 3",
        "/input steer s text",
        "/input follow-up f next",
        "/input edit s 0 edited",
        "/input cancel s",
    ] {
        assert!(
            zenpi::slash_actions::input_queue_control_request("session", "r", text)
                .unwrap()
                .is_some(),
            "{text}"
        );
    }
    for text in [
        "/input steer s !rm file",
        "/input edit s bad text",
        "/input mode steer many",
        "/input cancel too many",
        "/input list -1",
    ] {
        assert!(
            zenpi::slash_actions::input_queue_control_request("session", "r", text).is_err(),
            "{text}"
        );
    }
    assert!(
        zenpi::slash_actions::input_queue_control_request("session", "r", "ordinary prompt")
            .unwrap()
            .is_none()
    );
}

#[test]
fn input_receipts_remain_responsive_during_tool_approval() {
    use zenpi::approval::{ApprovalDecision, ApprovalMode, ApprovalPolicy, ApprovalResponse};
    struct NeedsApproval {
        first: AtomicBool,
        seen: Requests,
    }
    impl Backend for NeedsApproval {
        fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, BackendError> {
            self.seen
                .lock()
                .unwrap()
                .push((request.turn_id.into(), request.turns.to_vec()));
            let mut completion = Completion::text("done");
            if !self.first.swap(true, Ordering::AcqRel) {
                completion.tool_calls = vec![ToolCall {
                    id: "write".into(),
                    name: "write_file".into(),
                    arguments: json!({"path":"denied.txt","content":"must not write"}),
                }];
            }
            Ok(completion)
        }
    }
    let dir = tempdir().unwrap();
    let seen = Requests::default();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("approval.jsonl")).unwrap(),
        Box::new(NeedsApproval {
            first: AtomicBool::new(false),
            seen: seen.clone(),
        }),
    );
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(dir.path()).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    agent.set_approval_policy(ApprovalPolicy {
        mode: ApprovalMode::Always,
        ..Default::default()
    });
    agent.submit(TurnInputRequest::new("original")).unwrap();
    let coordinator = agent.approval_coordinator().unwrap();
    let port = agent.input_port();
    let owner = thread::spawn(move || {
        agent.run_active_turn().unwrap();
        agent
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let approval = loop {
        if let Some(pending) = coordinator.drain_pending().pop() {
            break pending;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(
        input(send(
            &port,
            "follow",
            enqueue("follow", InputKind::FollowUp, "follow")
        ))
        .status,
        InputStatus::Received
    );
    assert!(!dir.path().join("denied.txt").exists());
    coordinator
        .respond(ApprovalResponse {
            request_id: approval.request_id,
            decision: ApprovalDecision::Deny,
            remember: false,
        })
        .unwrap();
    let agent = owner.join().unwrap();
    assert!(!dir.path().join("denied.txt").exists());
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(!user_texts(&seen[1].1).contains(&"follow"));
    assert!(user_texts(&seen[2].1).contains(&"follow"));
    assert_eq!(
        InputQueue::recover(agent.session(), Default::default())
            .unwrap()
            .get("follow")
            .unwrap()
            .status,
        InputStatus::Applied
    );
}

#[test]
fn one_at_a_time_drains_full_follow_up_capacity_without_spending_tool_iterations() {
    let dir = tempdir().unwrap();
    let seen = Requests::default();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("one-mode.jsonl")).unwrap(),
        Box::new(CaptureOnly(seen.clone())),
    );
    let session_id = agent.session().session_id().to_owned();
    for n in 0..32 {
        let id = format!("follow-{n}");
        agent
            .input_queue_request(request(
                &session_id,
                &id,
                enqueue(&id, InputKind::FollowUp, &id),
            ))
            .unwrap();
    }
    agent.process(TurnInputRequest::new("original")).unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 33);
    for (n, (_, turns)) in seen.iter().enumerate() {
        assert_eq!(user_texts(turns).len(), n + 1);
    }
    let queue = InputQueue::recover(agent.session(), Default::default()).unwrap();
    assert!(!queue.has_pending());
}

#[test]
fn shared_slash_queue_preserves_payload_bytes_for_multiline_edit_roundtrip() {
    let dir = tempdir().unwrap();
    let mut agent =
        Agent::with_echo(SessionStore::open(dir.path().join("whitespace.jsonl")).unwrap());
    let session_id = agent.session().session_id().to_owned();
    for (id, lane) in [("s", "steer"), ("f", "follow-up")] {
        let text = "  first line\n\tsecond line  \r\n";
        let request = zenpi::slash_actions::input_queue_control_request(
            &session_id,
            &format!("enqueue-{id}"),
            &format!("/input {lane} {id} {text}"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            input(agent.input_queue_request(request).unwrap()).text,
            text
        );
        let edited = "\n  changed\n\ttrailing blank follows\n\n";
        let request = zenpi::slash_actions::input_queue_control_request(
            &session_id,
            &format!("edit-{id}"),
            &format!("/input edit {id} 0 {edited}"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            input(agent.input_queue_request(request).unwrap()).text,
            edited
        );
    }
    let queue = InputQueue::recover(agent.session(), Default::default()).unwrap();
    assert_eq!(
        queue.get("s").unwrap().text,
        "\n  changed\n\ttrailing blank follows\n\n"
    );
}

#[test]
fn resume_replaces_input_port_identity_and_never_reactivates_old_session_handles() {
    let dir = tempdir().unwrap();
    let seen = Requests::default();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("old.jsonl")).unwrap(),
        Box::new(CaptureOnly(seen.clone())),
    );
    let old = agent.input_port();
    let next_path = dir.path().join("next.jsonl");
    let next = SessionStore::open(&next_path).unwrap();
    let next_id = next.session_id().to_owned();
    drop(next);
    assert_ne!(old.session_id(), next_id);
    agent.resume_session(&next_path).unwrap();
    let current = agent.input_port();
    assert_eq!(current.session_id(), next_id);
    agent
        .submit(TurnInputRequest::new("next session prompt"))
        .unwrap();
    for session in [old.session_id(), next_id.clone()] {
        assert!(
            old.submit(request(
                &session,
                "old-handle",
                enqueue("old-input", InputKind::Steer, "must stay out")
            ))
            .is_err()
        );
    }
    let ticket = current
        .submit(request(
            &next_id,
            "current-handle",
            enqueue("new-input", InputKind::Steer, "current session input"),
        ))
        .unwrap();
    agent.run_active_turn().unwrap();
    assert!(ticket_reply(&ticket).is_ok());
    let queue = InputQueue::recover(agent.session(), Default::default()).unwrap();
    assert!(queue.get("old-input").is_none());
    assert_eq!(queue.get("new-input").unwrap().status, InputStatus::Applied);
    assert_eq!(
        user_texts(&seen.lock().unwrap()[0].1),
        ["next session prompt", "current session input"]
    );
    let old_session = SessionStore::open_existing(dir.path().join("old.jsonl")).unwrap();
    assert!(old_session.turns().is_empty());
    assert!(old_session.events().is_empty());
}

#[test]
fn predispatch_budget_failure_releases_scope_and_preserves_received_inputs() {
    use zenpi::core::{AgentError, AgentPhase};
    let dir = tempdir().unwrap();
    let path = dir.path().join("budget-admission.jsonl");
    let seen = Requests::default();
    let mut agent = Agent::new(
        SessionStore::open(&path).unwrap(),
        Box::new(CaptureOnly(seen.clone())),
    );
    agent
        .set_resource_limits(zenpi::governance::ResourceLimits {
            max_concurrency: 0,
            ..Default::default()
        })
        .unwrap();
    let original = agent
        .submit(TurnInputRequest::new("durable original prompt"))
        .unwrap()
        .turn_id()
        .unwrap()
        .to_owned();
    let port = agent.input_port();
    let session = agent.session().session_id().to_owned();
    agent
        .input_queue_request(request(
            &session,
            "received",
            enqueue("received-input", InputKind::Steer, "durable steer"),
        ))
        .unwrap();
    let waiting = port
        .submit(request(
            &session,
            "waiting",
            enqueue("waiting-input", InputKind::Steer, "unacknowledged steer"),
        ))
        .unwrap();
    assert!(matches!(
        agent.run_active_turn(),
        Err(AgentError::Governance(_))
    ));
    assert!(
        seen.lock().unwrap().is_empty(),
        "provider must not be called"
    );
    assert_eq!(agent.phase(), AgentPhase::Idle);
    assert_eq!(agent.active_turn_id(), None);
    assert!(ticket_reply(&waiting).is_err());
    assert!(
        port.submit(request(
            &session,
            "late",
            enqueue("late-input", InputKind::Steer, "late")
        ))
        .is_err()
    );
    let queue = InputQueue::recover(agent.session(), Default::default()).unwrap();
    assert_eq!(
        queue.get("received-input").unwrap().status,
        InputStatus::Received
    );
    assert!(queue.get("waiting-input").is_none());
    assert!(agent.operation_recovery().is_empty());
    assert_eq!(
        agent
            .history()
            .iter()
            .filter(|turn| turn.id == original)
            .count(),
        1
    );
    let reopened = SessionStore::open_existing(&path).unwrap();
    assert_eq!(
        reopened
            .turns()
            .iter()
            .filter(|turn| turn.id == original)
            .count(),
        1
    );
    assert_eq!(
        InputQueue::recover(&reopened, Default::default())
            .unwrap()
            .get("received-input")
            .unwrap()
            .status,
        InputStatus::Received
    );
    drop(reopened);
    agent
        .set_resource_limits(zenpi::governance::ResourceLimits {
            max_concurrency: 1,
            ..Default::default()
        })
        .unwrap();
    agent.process_sync("fresh attempt").unwrap();
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert!(user_texts(&seen.lock().unwrap()[0].1).contains(&"durable steer"));
    assert_eq!(
        InputQueue::recover(agent.session(), Default::default())
            .unwrap()
            .get("received-input")
            .unwrap()
            .status,
        InputStatus::Applied
    );
    assert_eq!(agent.phase(), AgentPhase::Idle);
}

#[test]
fn predispatch_begin_marker_failure_returns_idle_without_losing_user_turn() {
    use zenpi::core::{AgentError, AgentPhase};
    let dir = tempdir().unwrap();
    let path = dir.path().join("begin-admission.jsonl");
    let backup = dir.path().join("saved.jsonl");
    let seen = Requests::default();
    let mut agent = Agent::new(
        SessionStore::open(&path).unwrap(),
        Box::new(CaptureOnly(seen.clone())),
    );
    let original = agent
        .submit(TurnInputRequest::new("admitted before storage error"))
        .unwrap()
        .turn_id()
        .unwrap()
        .to_owned();
    let port = agent.input_port();
    let session = agent.session().session_id().to_owned();
    let waiting = port
        .submit(request(
            &session,
            "waiting",
            enqueue("waiting-input", InputKind::Steer, "not yet durable"),
        ))
        .unwrap();
    std::fs::rename(&path, &backup).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(matches!(
        agent.run_active_turn(),
        Err(AgentError::Session(_))
    ));
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(agent.phase(), AgentPhase::Idle);
    assert_eq!(agent.active_turn_id(), None);
    assert!(ticket_reply(&waiting).is_err());
    assert!(agent.operation_recovery().is_empty());
    assert_eq!(
        agent
            .history()
            .iter()
            .filter(|turn| turn.id == original)
            .count(),
        1
    );
    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(&backup, &path).unwrap();
    let reopened = SessionStore::open_existing(&path).unwrap();
    assert_eq!(
        reopened
            .turns()
            .iter()
            .filter(|turn| turn.id == original)
            .count(),
        1
    );
    assert!(reopened.operation_recovery().is_empty());
    drop(reopened);
    agent
        .process_sync("continue after repaired storage")
        .unwrap();
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(agent.phase(), AgentPhase::Idle);
}

#[test]
fn predispatch_reservation_write_failure_releases_only_its_new_concurrency_slot() {
    use zenpi::core::{AgentError, AgentPhase};
    let dir = tempdir().unwrap();
    let path = dir.path().join("reservation.jsonl");
    let backup = dir.path().join("saved.jsonl");
    let seen = Requests::default();
    let mut agent = Agent::new(
        SessionStore::open(&path).unwrap(),
        Box::new(CaptureOnly(seen.clone())),
    );
    agent
        .set_resource_limits(zenpi::governance::ResourceLimits {
            max_concurrency: 1,
            ..Default::default()
        })
        .unwrap();
    agent
        .submit(TurnInputRequest::new("reservation failure"))
        .unwrap();
    std::fs::rename(&path, &backup).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(matches!(
        agent.run_active_turn(),
        Err(AgentError::Governance(_))
    ));
    assert_eq!(agent.phase(), AgentPhase::Idle);
    assert!(seen.lock().unwrap().is_empty());
    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(&backup, &path).unwrap();
    // Do not reset the ledger: the same in-memory owner must have released its slot.
    agent.process_sync("next attempt").unwrap();
    assert_eq!(seen.lock().unwrap().len(), 1);
    let usage = agent
        .session()
        .events()
        .iter()
        .rev()
        .find(|event| event["type"] == "resource_usage")
        .unwrap();
    assert_eq!(usage["usage"]["concurrency"], 0);
}

#[test]
#[ignore = "PTY driver only; requires isolated root and fixture URL"]
fn acceptance101_sync_tui_probe() {
    let root = std::path::PathBuf::from(std::env::var("ZS1_SYNC_ROOT").unwrap());
    let backend = zenpi::backend::OpenAiCompatibleBackend::new(
        std::env::var("ZS1_SYNC_URL").unwrap(),
        None,
        "fixture",
    )
    .unwrap();
    let mut agent = Agent::new(
        SessionStore::open_in_workspace(root.join("session.jsonl"), &root).unwrap(),
        Box::new(backend),
    );
    zenpi::tui::run(&mut agent).unwrap();
}
