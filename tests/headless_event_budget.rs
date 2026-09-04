#![cfg(unix)]

use std::{
    io::{self, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;
use tempfile::tempdir;
use zenpi::{
    backend::{Backend, BackendError, Completion, CompletionRequest, ProviderEvent},
    core::Agent,
    session::SessionStore,
};

const MEDIUM_DELTA_BYTES: usize = 600 * 1024;
const OVERSIZED_DELTA_BYTES: usize = 2 * 1024 * 1024;

#[derive(Default)]
struct SlowConsumerGate {
    gate_write_seen: AtomicBool,
    flood_complete: AtomicBool,
    release_writer: AtomicBool,
}

struct FloodBackend {
    gate: Arc<SlowConsumerGate>,
}

impl Backend for FloodBackend {
    fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        Ok(Completion::text("unused"))
    }

    fn complete_with_control(
        &self,
        _: CompletionRequest<'_>,
        cancelled: &dyn Fn() -> bool,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        sink(ProviderEvent::TextDelta {
            delta: "gate".into(),
        })?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.gate.gate_write_seen.load(Ordering::Acquire) {
            if cancelled() {
                return Err(BackendError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(BackendError::Transport(
                    "slow-consumer fixture never reached its output gate".into(),
                ));
            }
            thread::sleep(Duration::from_millis(1));
        }

        for _ in 0..4 {
            sink(ProviderEvent::TextDelta {
                delta: "m".repeat(MEDIUM_DELTA_BYTES),
            })?;
        }
        sink(ProviderEvent::TextDelta {
            delta: "x".repeat(OVERSIZED_DELTA_BYTES),
        })?;
        self.gate.flood_complete.store(true, Ordering::Release);
        Ok(Completion::text("bounded completion"))
    }

    fn name(&self) -> &str {
        "event-budget-fixture"
    }
}

#[derive(Clone)]
struct GateWriter {
    bytes: Arc<Mutex<Vec<u8>>>,
    gate: Arc<SlowConsumerGate>,
}

impl Write for GateWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes.lock().unwrap().extend_from_slice(bytes);
        let is_gate_delta = bytes
            .windows(b"\"delta\":\"gate\"".len())
            .any(|window| window == b"\"delta\":\"gate\"");
        if is_gate_delta && !self.gate.gate_write_seen.swap(true, Ordering::AcqRel) {
            while !self.gate.release_writer.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn wait_for_flag(flag: &AtomicBool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !flag.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    flag.load(Ordering::Acquire)
}

fn wait_for_text(bytes: &Arc<Mutex<Vec<u8>>>, needle: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if String::from_utf8_lossy(&bytes.lock().unwrap()).contains(needle) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn slow_consumer_gets_exact_byte_drop_marker_before_terminal_response() {
    let gate = Arc::new(SlowConsumerGate::default());
    let backend = FloodBackend {
        gate: Arc::clone(&gate),
    };
    let directory = tempdir().unwrap();
    let session_path = directory.path().join("byte-budget.jsonl");
    let agent = Agent::new(SessionStore::open(session_path).unwrap(), Box::new(backend));
    let (mut input, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let output = GateWriter {
        bytes: Arc::clone(&captured),
        gate: Arc::clone(&gate),
    };
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(agent, reader, output).unwrap();
    });

    writeln!(
        input,
        "{}",
        serde_json::json!({
            "schema_version": 2,
            "type": "prompt",
            "id": "byte-prompt",
            "text": "flood the bounded mailbox"
        })
    )
    .unwrap();
    let flood_completed = wait_for_flag(&gate.flood_complete);
    gate.release_writer.store(true, Ordering::Release);
    assert!(
        flood_completed,
        "provider flood did not complete while stdout was blocked"
    );
    assert!(
        wait_for_text(&captured, "event_dropped"),
        "drop marker was not emitted after releasing stdout"
    );

    writeln!(
        input,
        "{}",
        serde_json::json!({
            "schema_version": 2,
            "type": "shutdown",
            "id": "byte-shutdown"
        })
    )
    .unwrap();
    drop(input);
    host.join().unwrap();

    let output = captured.lock().unwrap().clone();
    let records = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let provider_drop = records
        .iter()
        .find(|record| {
            record["type"] == "event"
                && record["event"]["type"] == "event_dropped"
                && record["event"]["stream"] == "provider"
        })
        .expect("missing provider drop marker");

    let medium = ProviderEvent::TextDelta {
        delta: "m".repeat(MEDIUM_DELTA_BYTES),
    };
    let oversized = ProviderEvent::TextDelta {
        delta: "x".repeat(OVERSIZED_DELTA_BYTES),
    };
    let expected_dropped_bytes = 3 * serde_json::to_vec(&medium).unwrap().len()
        + serde_json::to_vec(&oversized).unwrap().len();
    assert_eq!(provider_drop["event"]["count"], 4);
    assert_eq!(
        provider_drop["event"]["bytes"],
        u64::try_from(expected_dropped_bytes).unwrap()
    );
    assert_eq!(provider_drop["event"]["truncated"], true);
    assert_eq!(provider_drop["event"]["limit_bytes"], 1024 * 1024);

    let emitted_deltas = records
        .iter()
        .filter(|record| record["type"] == "event" && record["event"]["type"] == "text_delta")
        .count();
    assert_eq!(emitted_deltas, 2, "gate plus one medium delta should fit");
    let drop_position = records
        .iter()
        .position(|record| record["event"]["type"] == "event_dropped")
        .unwrap();
    let terminal_position = records
        .iter()
        .position(|record| record["type"] == "response" && record["id"] == "byte-prompt")
        .unwrap();
    assert!(drop_position < terminal_position);
    assert!(records.iter().any(|record| {
        record["type"] == "response" && record["id"] == "byte-prompt" && record["success"] == true
    }));
}
