#![cfg(unix)]

use serde_json::{Value, json};
use std::{
    io::{Cursor, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use tempfile::tempdir;
use zenpi::{
    backend::{Backend, BackendError, Completion, CompletionRequest, ProviderEvent},
    core::Agent,
    input_queue::{InputQueue, InputStatus},
    session::SessionStore,
};

struct FiniteRead {
    release: Arc<AtomicBool>,
    dropped: mpsc::Sender<()>,
}
impl Backend for FiniteRead {
    fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        unreachable!("controlled provider entry required")
    }
    fn complete_with_control(
        &self,
        _: CompletionRequest<'_>,
        cancelled: &dyn Fn() -> bool,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        sink(ProviderEvent::TextDelta {
            delta: "finite read started".into(),
        })?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.release.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        while Instant::now() < deadline {
            if cancelled() {
                return Err(BackendError::Cancelled);
            }
            thread::sleep(Duration::from_millis(1));
        }
        Ok(Completion::text("finite read completed"))
    }
    fn name(&self) -> &str {
        "finite-input-shutdown-test"
    }
}
impl Drop for FiniteRead {
    fn drop(&mut self) {
        let _ = self.dropped.send(());
    }
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
            self.tx
                .send(serde_json::from_slice(&line).unwrap())
                .unwrap();
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn receive(
    rx: &mpsc::Receiver<Value>,
    records: &mut Vec<Value>,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(7);
    loop {
        let value = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        let found = predicate(&value);
        records.push(value.clone());
        if found {
            return value;
        }
    }
}
#[derive(Clone, Copy)]
enum ReleaseAt {
    BeforeCancel,
    CancelRequested,
    ShutdownAck,
}

fn exercise(release_at: ReleaseAt, ticket_count: usize) {
    let dir = tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let path = dir.path().join("session.jsonl");
    let release = Arc::new(AtomicBool::new(false));
    let (dropped_tx, dropped_rx) = mpsc::channel();
    let session = SessionStore::open_in_workspace(&path, &workspace).unwrap();
    let session_id = session.session_id().to_owned();
    let agent = Agent::new(
        session,
        Box::new(FiniteRead {
            release: release.clone(),
            dropped: dropped_tx,
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
    let mut records = vec![];
    writeln!(
        writer,
        "{}",
        json!({"schema_version":2,"type":"project","id":"initial","project":{"action":"list"}})
    )
    .unwrap();
    let initial = receive(&rx, &mut records, |v| v["id"] == "initial");
    let project = initial["project"].clone();
    writeln!(
        writer,
        "{}",
        json!({"schema_version":2,"type":"prompt","id":"work","text":"finite original"})
    )
    .unwrap();
    receive(&rx, &mut records, |v| v["event"]["type"] == "text_delta");
    let requests: Vec<_> = (0..ticket_count).map(|i| json!({
        "schema_version":2,"type":"input_queue","id":format!("ticket-{i}"),"session_id":session_id,
        "input_queue":{"action":"enqueue","input_id":format!("input-{i}"),"kind":"follow_up","text":format!("queued-{i}")}
    })).collect();
    for request in &requests {
        writeln!(writer, "{request}").unwrap();
    }
    writeln!(
        writer,
        "{}",
        json!({"schema_version":2,"type":"status","id":"barrier"})
    )
    .unwrap();
    receive(&rx, &mut records, |v| v["id"] == "barrier");
    let pending_at_barrier = requests.iter().all(|r| {
        !records
            .iter()
            .any(|v| v["type"] == "response" && v["id"] == r["id"])
    });
    let started = Instant::now();
    writeln!(
        writer,
        "{}",
        json!({"schema_version":2,"type":"shutdown","id":"stop"})
    )
    .unwrap();
    match release_at {
        ReleaseAt::BeforeCancel => release.store(true, Ordering::Release),
        ReleaseAt::CancelRequested => {
            receive(&rx, &mut records, |v| {
                v["event"]["type"] == "cancel_requested"
            });
            release.store(true, Ordering::Release);
        }
        ReleaseAt::ShutdownAck => {}
    }
    let ack = receive(&rx, &mut records, |v| {
        v["type"] == "response" && v["id"] == "stop"
    });
    let elapsed = started.elapsed();
    // Always release/reap before assertions, including the failing baseline.
    release.store(true, Ordering::Release);
    drop(writer);
    host.join().unwrap();
    dropped_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    records.extend(rx.try_iter());
    assert!(pending_at_barrier, "ticket must be pending before shutdown");
    assert!(
        elapsed < Duration::from_secs(2),
        "shutdown grace changed: {elapsed:?}"
    );
    assert_eq!(ack["project"], project);
    assert_eq!(ack["data"]["drained"], true);
    assert_eq!(
        records.last(),
        Some(&ack),
        "late wire output after shutdown"
    );
    let wal_path = path.with_extension("jsonl.reconnect");
    let wal: Vec<Value> = std::fs::read_to_string(&wal_path)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let terminals: Vec<Value> = requests
        .iter()
        .map(|request| {
            let matches: Vec<_> = records
                .iter()
                .filter(|v| v["type"] == "response" && v["id"] == request["id"])
                .collect();
            assert_eq!(
                matches.len(),
                1,
                "missing/duplicate terminal for {}: {records:?}",
                request["id"]
            );
            let terminal = matches[0].clone();
            assert_eq!(terminal["project"], project);
            assert!(
                records.iter().position(|v| v == &terminal).unwrap()
                    < records.iter().position(|v| v == &ack).unwrap()
            );
            let cached: Vec<_> = wal
                .iter()
                .filter(|v| v["event"]["type"] == "terminal" && v["event"]["id"] == request["id"])
                .collect();
            assert_eq!(cached.len(), 1);
            assert_eq!(
                serde_json::from_str::<Value>(cached[0]["event"]["line"].as_str().unwrap())
                    .unwrap(),
                terminal
            );
            match release_at {
                ReleaseAt::BeforeCancel => assert_eq!(terminal["success"], true),
                ReleaseAt::CancelRequested => assert_eq!(terminal["code"], "input_queue_error"),
                ReleaseAt::ShutdownAck => {
                    assert_eq!(terminal["code"], "input_queue_result_unknown");
                    assert!(
                        terminal["error"]
                            .as_str()
                            .unwrap()
                            .contains("may have been applied")
                    );
                    assert!(
                        terminal["error"]
                            .as_str()
                            .unwrap()
                            .contains("inspect the input queue")
                    );
                }
            }
            terminal
        })
        .collect();
    {
        let session = SessionStore::open_existing(&path).unwrap();
        let queue = InputQueue::recover(&session, Default::default()).unwrap();
        for i in 0..ticket_count {
            if matches!(release_at, ReleaseAt::BeforeCancel) {
                assert_eq!(
                    queue.get(&format!("input-{i}")).unwrap().status,
                    InputStatus::Received
                );
            } else {
                // This fixture has not serviced the ticket. Unknown results in
                // general must not be interpreted as proof of no mutation.
                assert!(queue.get(&format!("input-{i}")).is_none());
            }
        }
    }
    let (dropped_tx, _) = mpsc::channel();
    let agent = Agent::new(
        SessionStore::open_in_workspace(&path, &workspace).unwrap(),
        Box::new(FiniteRead {
            release,
            dropped: dropped_tx,
        }),
    );
    let mut input = requests
        .iter()
        .map(|v| format!("{v}\n"))
        .collect::<String>();
    input.push_str(&format!(
        "{}\n",
        json!({"schema_version":2,"type":"shutdown","id":"stop-reconnect"})
    ));
    let mut output = Vec::new();
    zenpi::headless::run_async_streams(agent, Cursor::new(input.into_bytes()), &mut output)
        .unwrap();
    let replay: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    for terminal in terminals {
        let matches: Vec<_> = replay
            .iter()
            .filter(|v| v["type"] == "response" && v["id"] == terminal["id"])
            .collect();
        assert_eq!(
            matches,
            vec![&terminal],
            "reconnect must return exact cached terminal"
        );
    }
}

#[test]
fn ready_input_receipt_is_not_relabelled_when_shutdown_cancels_provider() {
    exercise(ReleaseAt::BeforeCancel, 1);
}
#[test]
fn cooperative_cancellation_closes_ticket_without_unknown_result() {
    exercise(ReleaseAt::CancelRequested, 1);
}
#[test]
fn finite_delayed_owner_gets_unknown_terminals_before_ack_and_exact_reconnect() {
    exercise(ReleaseAt::ShutdownAck, 2);
}
