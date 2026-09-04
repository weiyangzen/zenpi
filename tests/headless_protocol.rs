use serde_json::Value;
use std::io::Cursor;
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile::tempdir;
use zenpi::{
    approval::ApprovalDecision,
    b3::ResourceBudget,
    backend::{Backend, BackendError, Completion, CompletionRequest, ProviderEvent},
    core::{Agent, Turn, TurnRole},
    domain_store::DomainStore,
    domains::{Blueprint, BlueprintItem, Goal, Learn},
    headless::run_headless,
    protocol::parse_line,
    session::{InterruptedOperation, OperationKind, SessionStore},
};

fn json_lines(bytes: &[u8]) -> Vec<Value> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// Create a domain input under the process workspace so the host's
/// workspace-relative path policy is exercised rather than bypassed with an
/// absolute temporary path.
fn workspace_domain_fixture(stem: &str, value: &str) -> (PathBuf, String) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = std::env::current_dir().unwrap();
    let directory = workspace.join("target").join(format!(
        "zenpi-domain-command-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{stem}.json"));
    fs::write(&path, value).unwrap();
    let relative = path
        .strip_prefix(workspace)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    (directory, relative)
}

/// Keep the first provider call open until the host sends a cancellation. The
/// deterministic boundary lets the burst shutdown regression prove that an
/// already-admitted steer is promoted before runtime shutdown is submitted.
struct BurstSteerBackend {
    calls: AtomicUsize,
    first_started: mpsc::SyncSender<()>,
    cancellation_seen: mpsc::SyncSender<()>,
}

impl Backend for BurstSteerBackend {
    fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        Ok(Completion::text("unused"))
    }

    fn complete_with_control(
        &self,
        _: CompletionRequest<'_>,
        cancelled: &dyn Fn() -> bool,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        let call = self.calls.fetch_add(1, Ordering::AcqRel);
        if call == 0 {
            let _ = self.first_started.send(());
            loop {
                if cancelled() {
                    let _ = self.cancellation_seen.send(());
                    return Err(BackendError::Cancelled);
                }
                thread::yield_now();
            }
        }
        if cancelled() {
            return Err(BackendError::Cancelled);
        }
        sink(ProviderEvent::TextDelta {
            delta: "steered answer".into(),
        })?;
        sink(ProviderEvent::Completed {
            response_id: None,
            model: None,
        })?;
        Ok(Completion::text("steered answer"))
    }

    fn name(&self) -> &str {
        "burst-steer-fixture"
    }
}

#[test]
fn malformed_frame_isolated_from_all_commands() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("wire.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let input = b"{bad}\n{\"type\":\"prompt\",\"id\":\"p\",\"text\":\"a\\u2028b\",\"mode\":\"start_if_idle\"}\n{\"type\":\"status\",\"id\":\"s\"}\n{\"type\":\"handoff\",\"id\":\"h\",\"to\":\"worker\",\"summary\":\"ready\",\"artifacts\":[\"Docs/plan.md\"]}\n{\"type\":\"shutdown\",\"id\":\"q\"}\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input), &mut output).unwrap();
    let records = json_lines(&output);
    assert!(records.iter().any(|v| v["success"] == false));
    for id in ["p", "s", "h", "q"] {
        assert!(
            records
                .iter()
                .any(|v| v["id"] == id && v["success"] == true)
        );
    }
    let journal = json_lines(&std::fs::read(&path).unwrap());
    assert!(journal.iter().any(|v| v["kind"] == "handoff_record"));
    let seq: Vec<u64> = journal.iter().map(|v| v["seq"].as_u64().unwrap()).collect();
    assert!(seq.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn status_snapshots_redact_session_paths_in_sync_and_async_hosts() {
    let dir = tempdir().unwrap();

    let sync_path = dir.path().join("sync-status.jsonl");
    let mut sync_agent = Agent::with_echo(SessionStore::open(&sync_path).unwrap());
    let mut sync_output = Vec::new();
    run_headless(
        &mut sync_agent,
        Cursor::new(
            b"{\"type\":\"status\",\"id\":\"sync-status\"}\n{\"type\":\"shutdown\",\"id\":\"sync-stop\"}\n",
        ),
        &mut sync_output,
    )
    .unwrap();
    let sync_records = json_lines(&sync_output);
    let sync_status = sync_records
        .iter()
        .find(|record| record["id"] == "sync-status")
        .unwrap();
    assert_eq!(
        sync_status["data"]["session"]["path"],
        "<session:sync-status.jsonl>"
    );
    assert!(
        !sync_status
            .to_string()
            .contains(dir.path().to_string_lossy().as_ref())
    );

    let async_path = dir.path().join("async-status.jsonl");
    let async_agent = Agent::with_echo(SessionStore::open(&async_path).unwrap());
    let async_input = Cursor::new(
        b"{\"type\":\"status\",\"id\":\"async-status\"}\n{\"type\":\"shutdown\",\"id\":\"async-stop\"}\n",
    );
    let async_output = SharedWriter::default();
    let captured = async_output.clone();
    zenpi::headless::run_async_streams(async_agent, async_input, async_output).unwrap();
    let async_records = json_lines(&captured.0.lock().unwrap());
    let async_status = async_records
        .iter()
        .find(|record| record["id"] == "async-status")
        .unwrap();
    assert_eq!(
        async_status["data"]["session"]["path"],
        "<session:async-status.jsonl>"
    );
    assert!(
        !async_status
            .to_string()
            .contains(dir.path().to_string_lossy().as_ref())
    );
}

#[test]
fn resume_path_rejects_missing_journals_without_creating_them() {
    let dir = tempdir().unwrap();
    let missing_sync = dir.path().join("missing-sync.jsonl");
    let active_sync = dir.path().join("active-sync.jsonl");
    let mut sync_agent = Agent::with_echo(SessionStore::open(&active_sync).unwrap());
    let sync_input = format!(
        "{}\n{}\n",
        serde_json::json!({
            "type": "resume",
            "id": "resume-sync",
            "path": missing_sync.display().to_string(),
        }),
        serde_json::json!({"type":"shutdown","id":"shutdown-sync"}),
    );
    let mut sync_output = Vec::new();
    run_headless(
        &mut sync_agent,
        Cursor::new(sync_input.into_bytes()),
        &mut sync_output,
    )
    .unwrap();
    let sync_records = json_lines(&sync_output);
    let sync_resume = sync_records
        .iter()
        .find(|record| record["id"] == "resume-sync")
        .unwrap();
    assert_eq!(sync_resume["success"], false);
    assert_eq!(sync_resume["code"], "session_path_denied");
    assert!(!missing_sync.exists());

    let missing_async = dir.path().join("missing-async.jsonl");
    let active_async = dir.path().join("active-async.jsonl");
    let async_agent = Agent::with_echo(SessionStore::open(&active_async).unwrap());
    let async_input = format!(
        "{}\n{}\n",
        serde_json::json!({
            "type": "resume",
            "id": "resume-async",
            "path": missing_async.display().to_string(),
        }),
        serde_json::json!({"type":"shutdown","id":"shutdown-async"}),
    );
    let async_output = SharedWriter::default();
    let captured = async_output.clone();
    zenpi::headless::run_async_streams(
        async_agent,
        Cursor::new(async_input.into_bytes()),
        async_output,
    )
    .unwrap();
    let async_records = json_lines(&captured.0.lock().unwrap());
    let async_resume = async_records
        .iter()
        .find(|record| record["id"] == "resume-async")
        .unwrap();
    assert_eq!(async_resume["success"], false);
    assert_eq!(async_resume["code"], "session_path_denied");
    assert!(!missing_async.exists());

    for (name, contents) in [("plain.txt", "not a zenpi journal\n"), ("empty.jsonl", "")] {
        let path = dir.path().join(name);
        fs::write(&path, contents).unwrap();
        let before = fs::read(&path).unwrap();
        let active = dir.path().join(format!("active-{name}.jsonl"));
        let mut agent = Agent::with_echo(SessionStore::open(&active).unwrap());
        let input = format!(
            "{}\n{}\n",
            serde_json::json!({"type":"resume","id":"bad-existing","path":path}),
            serde_json::json!({"type":"shutdown","id":"done"}),
        );
        let mut output = Vec::new();
        run_headless(&mut agent, Cursor::new(input.into_bytes()), &mut output).unwrap();
        let response = json_lines(&output)
            .into_iter()
            .find(|record| record["id"] == "bad-existing")
            .unwrap();
        assert_eq!(response["success"], false);
        assert!(matches!(
            response["code"].as_str(),
            Some("agent_error") | Some("session_error")
        ));
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}

#[cfg(unix)]
#[test]
fn shutdown_promotes_a_steer_admitted_before_the_shutdown_frame() {
    let (first_started_tx, first_started_rx) = mpsc::sync_channel(1);
    let (cancellation_seen_tx, cancellation_seen_rx) = mpsc::sync_channel(1);
    let backend = BurstSteerBackend {
        calls: AtomicUsize::new(0),
        first_started: first_started_tx,
        cancellation_seen: cancellation_seen_tx,
    };
    let dir = tempdir().unwrap();
    let path = dir.path().join("burst-steer.jsonl");
    let agent = Agent::new(SessionStore::open(&path).unwrap(), Box::new(backend));
    let (mut input_writer, input_reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let output = SharedWriter::default();
    let captured = output.clone();
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(agent, input_reader, output).unwrap();
    });

    writeln!(
        input_writer,
        "{}",
        serde_json::json!({"type":"prompt","id":"burst-prompt","text":"first"})
    )
    .unwrap();
    first_started_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("first provider call did not start");
    // These frames intentionally arrive back-to-back. The steer is accepted
    // into the host's deferred queue, then shutdown establishes its boundary
    // before the scheduler can finish the cancel/reissue pair.
    writeln!(
        input_writer,
        "{}",
        serde_json::json!({"type":"steer","id":"burst-steer","text":"second"})
    )
    .unwrap();
    writeln!(
        input_writer,
        "{}",
        serde_json::json!({"type":"shutdown","id":"burst-shutdown"})
    )
    .unwrap();
    cancellation_seen_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("accepted steer never cancelled the active request");
    drop(input_writer);
    host.join().unwrap();

    let records = json_lines(&captured.0.lock().unwrap());
    let prompt = records
        .iter()
        .find(|record| record["id"] == "burst-prompt" && record["type"] == "response")
        .unwrap();
    assert_eq!(prompt["success"], false);
    assert_eq!(prompt["code"], "backend_cancelled");
    let steer = records
        .iter()
        .find(|record| record["id"] == "burst-steer" && record["type"] == "response")
        .unwrap();
    assert_eq!(steer["success"], true);
    assert_eq!(steer["data"]["assistant"]["content"], "steered answer");
    assert!(
        !records
            .iter()
            .any(|record| { record["id"] == "burst-steer" && record["code"] == "runtime_closed" })
    );
    let shutdown = records
        .iter()
        .find(|record| record["id"] == "burst-shutdown" && record["type"] == "response")
        .unwrap();
    assert_eq!(shutdown["success"], true);
    assert_eq!(shutdown["data"]["drained"], true);

    let session = SessionStore::open(path).unwrap();
    assert_eq!(
        session
            .turns()
            .iter()
            .filter(|turn| turn.role == zenpi::core::TurnRole::User)
            .count(),
        2
    );
    assert!(session.turns().iter().any(|turn| {
        turn.role == zenpi::core::TurnRole::Assistant && turn.content == "steered answer"
    }));
}

#[test]
#[cfg(unix)]
fn async_reader_bounds_overlong_and_invalid_frames_without_stopping() {
    use zenpi::protocol::MAX_LINE_BYTES;

    let dir = tempdir().unwrap();
    let path = dir.path().join("async-bounds.jsonl");
    let agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let (mut writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let output = SharedWriter::default();
    let captured = output.clone();
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(agent, reader, output).unwrap();
    });

    let mut oversized = vec![b'x'; MAX_LINE_BYTES + 1];
    oversized.push(b'\n');
    writer.write_all(&oversized).unwrap();
    writer.write_all(b"\xff\n").unwrap();
    writeln!(
        writer,
        "{}",
        serde_json::json!({"type":"status","id":"status-after-invalid"})
    )
    .unwrap();
    writeln!(
        writer,
        "{}",
        serde_json::json!({"type":"shutdown","id":"shutdown-after-invalid"})
    )
    .unwrap();
    drop(writer);
    host.join().unwrap();

    let records = json_lines(&captured.0.lock().unwrap());
    assert!(
        records
            .iter()
            .any(|record| { record["code"] == "line_too_long" && record["success"] == false })
    );
    assert!(
        records
            .iter()
            .any(|record| { record["code"] == "invalid_utf8" && record["success"] == false })
    );
    assert!(
        records
            .iter()
            .any(|record| { record["id"] == "status-after-invalid" && record["success"] == true })
    );
    assert!(
        records.iter().any(|record| {
            record["id"] == "shutdown-after-invalid" && record["success"] == true
        })
    );
}

#[test]
#[cfg(unix)]
fn async_recovery_event_does_not_overtake_turn_admission() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("recovery-order.jsonl");
    let mut store = SessionStore::open(&path).unwrap();
    store
        .begin_operation(&InterruptedOperation {
            operation_id: "tool-recovery-order".into(),
            kind: OperationKind::Tool,
            turn_id: "turn-recovery-order".into(),
            retry_requires_confirmation: true,
        })
        .unwrap();
    drop(store);
    // Agent construction queues a recovery warning before the first request's
    // TurnAccepted marker. The host must still emit both before provider data.
    let agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let (mut writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let output = SharedWriter::default();
    let captured = output.clone();
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(agent, reader, output).unwrap();
    });
    writeln!(
        writer,
        "{}",
        serde_json::json!({"type":"prompt","id":"recovery-prompt","text":"hello"})
    )
    .unwrap();
    writeln!(
        writer,
        "{}",
        serde_json::json!({"type":"shutdown","id":"recovery-shutdown"})
    )
    .unwrap();
    drop(writer);
    host.join().unwrap();

    let records = json_lines(&captured.0.lock().unwrap());
    let accepted = records
        .iter()
        .position(|record| record["type"] == "event" && record["event"]["type"] == "turn_accepted")
        .expect("missing turn admission event");
    let provider = records
        .iter()
        .position(|record| record["type"] == "event" && record["event"]["type"] == "text_delta")
        .expect("missing provider event");
    assert!(
        accepted < provider,
        "provider event overtook admission: {records:?}"
    );
    assert!(
        records
            .iter()
            .any(|record| { record["type"] == "event" && record["event"]["type"] == "error" })
    );
}

#[test]
fn unsupported_version_and_missing_id_are_rejected() {
    assert!(
        parse_line(r#"{"schema_version":3,"type":"status","id":"x"}"#)
            .unwrap()
            .into_command()
            .is_err()
    );
    assert!(
        parse_line(r#"{"type":"status"}"#)
            .unwrap()
            .into_command()
            .is_err()
    );
}

#[test]
fn approval_command_is_typed_and_bounded() {
    let command = parse_line(
        r#"{"type":"approve","id":"a","approval_id":"approval-call-1","decision":"allow","remember":true}"#,
    )
    .unwrap()
    .into_command()
    .unwrap();
    assert!(matches!(
        command,
        zenpi::protocol::Command::Approve {
            approval_id,
            decision: ApprovalDecision::Allow,
            remember: true,
        } if approval_id == "approval-call-1"
    ));
}

#[test]
fn sequential_steer_and_eof_are_safe_terminal_paths() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("steer-eof.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let input = b"{\"type\":\"prompt\",\"id\":\"p\",\"text\":\"hello\",\"mode\":\"start_if_idle\"}\n{\"type\":\"steer\",\"id\":\"s\",\"text\":\"follow-up\"}\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input), &mut output).unwrap();
    let records = json_lines(&output);
    assert!(
        records
            .iter()
            .any(|value| value["id"] == "p" && value["success"] == true)
    );
    assert!(
        records
            .iter()
            .any(|value| value["id"] == "s" && value["success"] == true)
    );
    assert_eq!(agent.phase(), zenpi::core::AgentPhase::Closed);
}

#[test]
fn protocol_v2_accepts_resume_sequence_and_v1_remains_supported() {
    let v2 = parse_line(r#"{"schema_version":2,"type":"resume","id":"r2","from_sequence":7}"#)
        .unwrap()
        .into_command()
        .unwrap();
    assert!(matches!(
        v2,
        zenpi::protocol::Command::Resume {
            path: None,
            from_sequence: Some(7)
        }
    ));
    let v1 = parse_line(r#"{"schema_version":1,"type":"status","id":"r1"}"#)
        .unwrap()
        .into_command();
    assert!(v1.is_ok());
    let response =
        zenpi::protocol::StdioResponse::success(Some("r2".into()), "status", None).for_version(2);
    assert_eq!(response.schema_version, 2);
    assert!(parse_line(
        r#"{"schema_version":2,"type":"resume","id":"both","path":"other.jsonl","from_sequence":7}"#
    )
    .unwrap()
    .into_command()
    .is_err());
}

#[test]
fn headless_slash_commands_are_control_plane_only() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("slash-command.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let input = concat!(
        "{\"type\":\"command\",\"id\":\"m\",\"text\":\"/model fixture\"}\n",
        "{\"type\":\"command\",\"id\":\"g\",\"text\":\"/goal keep bounded\"}\n",
        "{\"type\":\"command\",\"id\":\"s\",\"text\":\"/status\"}\n",
        "{\"type\":\"command\",\"id\":\"h\",\"text\":\"/history 3\"}\n",
        "{\"type\":\"command\",\"id\":\"c\",\"text\":\"/clear\"}\n",
        "{\"type\":\"command\",\"id\":\"r\",\"text\":\"/compete run-a\"}\n",
        "{\"type\":\"command\",\"id\":\"l\",\"text\":\"/loop repair item-a\"}\n",
        "{\"type\":\"command\",\"id\":\"e\",\"text\":\"/session export /private/secret.jsonl\"}\n",
        "{\"type\":\"command\",\"id\":\"bad\",\"text\":\"/does-not-exist\"}\n",
        "{\"type\":\"status\",\"id\":\"after\"}\n",
        "{\"type\":\"shutdown\",\"id\":\"q\"}\n",
    );
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.as_bytes()), &mut output).unwrap();
    let records = json_lines(&output);
    for id in ["m", "s", "h", "c", "r", "l", "after", "q"] {
        assert!(
            records.iter().any(|record| {
                record["id"] == id && record["type"] == "response" && record["success"] == true
            }),
            "missing successful response for {id}"
        );
    }
    for id in ["g", "e"] {
        let response = records.iter().find(|record| record["id"] == id).unwrap();
        assert_eq!(
            response["success"], false,
            "unsupported {id} was reported as success"
        );
        assert_eq!(response["code"], "unsupported_command");
    }
    assert!(records.iter().any(|record| {
        record["id"] == "bad"
            && record["type"] == "response"
            && record["success"] == false
            && record["code"] == "slash_invalid"
    }));
    let unsupported_session = records.iter().find(|record| record["id"] == "e").unwrap();
    assert!(
        unsupported_session["error"]
            .as_str()
            .unwrap()
            .contains("session export")
    );
    assert!(
        !unsupported_session
            .to_string()
            .contains("/private/secret.jsonl")
    );
    assert_eq!(agent.history().len(), 0, "slash input reached the model");
    assert_eq!(agent.session().runtime_intents().len(), 2);
    for id in ["r", "l"] {
        let response = records.iter().find(|record| record["id"] == id).unwrap();
        assert_eq!(response["data"]["durable"], true);
        assert_eq!(response["data"]["persisted"], true);
        assert_eq!(response["data"]["delivery"], "journal_only");
        assert_eq!(response["data"]["zenpi_started"], false);
        assert_eq!(response["data"]["execution_state"], "untracked");
    }
    let status = records.iter().find(|record| record["id"] == "s").unwrap();
    assert_eq!(status["data"]["model"], "fixture");
}

#[test]
fn session_slash_list_and_open_use_real_session_owner_paths() {
    let dir = tempdir().unwrap();
    let source_path = dir.path().join("source.jsonl");
    let mut source = SessionStore::open(&source_path).unwrap();
    source
        .append_turn(Turn::new("source-user", TurnRole::User, "persisted"))
        .unwrap();
    drop(source);

    let active_path = dir.path().join("active.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&active_path).unwrap());
    let input = [
        serde_json::json!({"type":"command","id":"list","text":"/session list"}),
        serde_json::json!({
            "type":"command",
            "id":"open",
            "text":format!("/session open {}", source_path.display()),
        }),
        serde_json::json!({"type":"status","id":"status"}),
        serde_json::json!({"type":"shutdown","id":"shutdown"}),
    ]
    .into_iter()
    .map(|value| serde_json::to_string(&value).unwrap())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.into_bytes()), &mut output).unwrap();
    let records = json_lines(&output);

    let listed = records
        .iter()
        .find(|record| record["id"] == "list")
        .unwrap();
    assert_eq!(listed["success"], true);
    assert_eq!(listed["data"]["action"], "list");
    assert!(listed["data"]["sessions"].is_array());

    let opened = records
        .iter()
        .find(|record| record["id"] == "open")
        .unwrap();
    assert_eq!(opened["success"], true);
    assert_eq!(opened["data"]["action"], "open");
    assert_eq!(opened["data"]["session"]["path"], "<session:source.jsonl>");
    assert!(
        !opened
            .to_string()
            .contains(dir.path().to_string_lossy().as_ref())
    );
    assert!(
        agent
            .history()
            .iter()
            .any(|turn| turn.content == "persisted")
    );
}

#[test]
fn inspection_responses_redact_paths_and_reject_domain_escape() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("inspection.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let domain_path = zenpi::domain_store::path_for_session(&session_path);
    assert!(
        !domain_path.exists(),
        "read-only domain inspection must start without a snapshot"
    );
    let input = concat!(
        "{\"type\":\"command\",\"id\":\"bp\",\"text\":\"/blueprint show\"}\n",
        "{\"type\":\"resources\",\"id\":\"res\",\"path\":\"src\"}\n",
        "{\"type\":\"command\",\"id\":\"escape\",\"text\":\"/blueprint put ../outside.json\"}\n",
        "{\"type\":\"shutdown\",\"id\":\"stop\"}\n",
    );
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.as_bytes()), &mut output).unwrap();
    let records = json_lines(&output);

    let blueprint = records.iter().find(|record| record["id"] == "bp").unwrap();
    assert_eq!(blueprint["success"], true);
    assert_eq!(blueprint["data"]["store"]["path"], "<domain-store>");

    let resources = records.iter().find(|record| record["id"] == "res").unwrap();
    assert_eq!(resources["success"], true);
    assert_eq!(resources["data"]["workspace"]["root"], ".");

    let escape = records
        .iter()
        .find(|record| record["id"] == "escape")
        .unwrap();
    assert_eq!(escape["success"], false);
    assert_eq!(escape["code"], "domain_input_path_denied");
    assert!(
        !domain_path.exists(),
        "show/validate commands must not create a missing domain snapshot"
    );
}

#[test]
fn headless_domain_put_commands_persist_and_round_trip() {
    let blueprint = Blueprint::new(
        "headless-plan",
        "1",
        vec![
            BlueprintItem::new("build", 100),
            BlueprintItem::new("verify", 200).with_dependencies(["build"]),
        ],
    )
    .unwrap();
    let goal = Goal::new("headless-goal", &blueprint, ResourceBudget::default(), None).unwrap();
    let learn = Learn::new(
        "headless-learn",
        "src/core.rs",
        "Docs/learn/core.md",
        vec!["tests/headless_protocol.rs".into()],
    )
    .unwrap();
    let (blueprint_dir, blueprint_path) =
        workspace_domain_fixture("blueprint", &blueprint.encode_json().unwrap());
    let (goal_dir, goal_path) =
        workspace_domain_fixture("goal", &serde_json::to_string(&goal).unwrap());
    let (learn_dir, learn_path) =
        workspace_domain_fixture("learn", &serde_json::to_string(&learn).unwrap());

    let session_path = blueprint_dir.join("session.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let input = [
        serde_json::json!({
            "type": "command",
            "id": "bp-put",
            "text": format!("/blueprint put {blueprint_path}"),
        }),
        serde_json::json!({
            "type": "command",
            "id": "goal-put",
            "text": format!("/goal put {goal_path}"),
        }),
        serde_json::json!({
            "type": "command",
            "id": "learn-put",
            "text": format!("/learn put {learn_path}"),
        }),
        serde_json::json!({"type": "command", "id": "bp-show", "text": "/blueprint show"}),
        serde_json::json!({"type": "command", "id": "goal-show", "text": "/goal show"}),
        serde_json::json!({"type": "command", "id": "learn-show", "text": "/learn show"}),
        serde_json::json!({"type": "shutdown", "id": "shutdown"}),
    ]
    .into_iter()
    .map(|value| serde_json::to_string(&value).unwrap())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.into_bytes()), &mut output).unwrap();
    let records = json_lines(&output);

    for id in [
        "bp-put",
        "goal-put",
        "learn-put",
        "bp-show",
        "goal-show",
        "learn-show",
    ] {
        let response = records
            .iter()
            .find(|record| record["id"] == id)
            .unwrap_or_else(|| panic!("missing response for {id}"));
        assert_eq!(response["success"], true, "command {id} failed: {response}");
        assert_eq!(
            response["data"]["accepted"], true,
            "command {id}: {response}"
        );
    }
    assert_eq!(
        records.iter().find(|r| r["id"] == "bp-put").unwrap()["data"]["change"],
        "inserted"
    );
    assert_eq!(
        records.iter().find(|r| r["id"] == "goal-put").unwrap()["data"]["change"],
        "inserted"
    );
    assert_eq!(
        records.iter().find(|r| r["id"] == "learn-put").unwrap()["data"]["change"],
        "inserted"
    );

    let blueprint_view = records.iter().find(|r| r["id"] == "bp-show").unwrap();
    assert_eq!(
        blueprint_view["data"]["blueprints"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let goal_view = records.iter().find(|r| r["id"] == "goal-show").unwrap();
    assert_eq!(goal_view["data"]["goals"].as_array().unwrap().len(), 1);
    let learn_view = records.iter().find(|r| r["id"] == "learn-show").unwrap();
    assert_eq!(learn_view["data"]["learns"].as_array().unwrap().len(), 1);

    let store_path = zenpi::domain_store::path_for_session(&session_path);
    let store = DomainStore::open(store_path).unwrap();
    assert_eq!(store.blueprints().len(), 1);
    assert_eq!(store.goals().len(), 1);
    assert_eq!(store.learns().len(), 1);
    assert_eq!(
        agent.history().len(),
        0,
        "domain control commands must not become provider turns"
    );
    let _ = fs::remove_dir_all(blueprint_dir);
    let _ = fs::remove_dir_all(goal_dir);
    let _ = fs::remove_dir_all(learn_dir);
}

#[cfg(unix)]
#[test]
fn async_shutdown_acknowledges_only_after_prior_turn_terminal_response() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("shutdown-order.jsonl");
    let agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let (mut writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let output = SharedWriter::default();
    let captured = output.clone();
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(agent, reader, output).unwrap();
    });
    writeln!(
        writer,
        "{}",
        serde_json::json!({"type":"prompt","id":"prompt","text":"hello"})
    )
    .unwrap();
    writeln!(
        writer,
        "{}",
        serde_json::json!({"type":"shutdown","id":"shutdown"})
    )
    .unwrap();
    drop(writer);
    host.join().unwrap();
    let records = json_lines(&captured.0.lock().unwrap());
    let prompt_terminal = records
        .iter()
        .position(|record| record["id"] == "prompt" && record["type"] == "response")
        .unwrap();
    let shutdown_terminal = records
        .iter()
        .position(|record| record["id"] == "shutdown" && record["type"] == "response")
        .unwrap();
    assert!(prompt_terminal < shutdown_terminal);
    assert_eq!(records[shutdown_terminal]["data"]["drained"], true);
    let accepted = records
        .iter()
        .position(|record| record["event"]["type"] == "turn_accepted")
        .unwrap();
    assert!(accepted < prompt_terminal);
}

#[test]
fn synchronous_replay_and_control_idempotency_are_explicit() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("replay.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let input = concat!(
        "{\"type\":\"prompt\",\"id\":\"p\",\"text\":\"hello\"}\n",
        "{\"type\":\"status\",\"id\":\"same-status\"}\n",
        "{\"type\":\"status\",\"id\":\"same-status\"}\n",
        "{\"schema_version\":2,\"type\":\"resume\",\"id\":\"replay\",\"from_sequence\":0}\n",
        "{\"type\":\"shutdown\",\"id\":\"stop\"}\n",
    );
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.as_bytes()), &mut output).unwrap();
    let records = json_lines(&output);

    let statuses: Vec<&Value> = records
        .iter()
        .filter(|record| record["id"] == "same-status")
        .collect();
    assert_eq!(statuses.len(), 2);
    assert_eq!(
        statuses[0], statuses[1],
        "same ID must replay the same response"
    );

    let replay = records
        .iter()
        .find(|record| record["id"] == "replay" && record["type"] == "response")
        .expect("missing replay response");
    assert_eq!(replay["schema_version"], 2);
    assert!(replay["data"]["replayed"].as_u64().unwrap_or(0) > 0);
    assert!(records.iter().any(|record| record["type"] == "event"));
}

#[test]
fn in_flight_duplicate_request_id_is_suppressed_before_side_effects() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || -> Result<(), String> {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let body = read_http_body(&mut stream)?;
        if body.to_string().contains("duplicate prompt") {
            return Err("duplicate prompt reached provider".into());
        }
        thread::sleep(Duration::from_millis(100));
        let payload = "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"one answer\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"once\"}}\n\n";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        )
        .map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        thread::sleep(Duration::from_millis(100));
        if listener.accept().is_ok() {
            return Err("duplicate request opened a second provider connection".into());
        }
        Ok(())
    });
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("duplicate.jsonl");
    let backend = zenpi::backend::OpenAiCompatibleBackend::new_with_wire_api(
        format!("http://127.0.0.1:{port}"),
        None,
        "mock-model",
        zenpi::backend::OpenAiWireApi::Responses,
    )
    .unwrap();
    let agent = Agent::new(
        SessionStore::open(&session_path).unwrap(),
        Box::new(backend),
    );
    let (mut writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let output = SharedWriter::default();
    let captured = output.clone();
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(agent, reader, output).unwrap();
    });
    writeln!(
        writer,
        "{}",
        serde_json::json!({"schema_version":2,"type":"prompt","id":"same","text":"original prompt"})
    )
    .unwrap();
    writeln!(writer, "{}", serde_json::json!({"schema_version":2,"type":"prompt","id":"same","text":"duplicate prompt"})).unwrap();
    wait_for_output(&captured, "one answer");
    writeln!(
        writer,
        "{}",
        serde_json::json!({"schema_version":2,"type":"shutdown","id":"stop"})
    )
    .unwrap();
    drop(writer);
    host.join().unwrap();
    server.join().unwrap().unwrap();
    let records = json_lines(&captured.0.lock().unwrap());
    assert!(records.iter().any(|record| {
        record["id"] == "same" && record["code"] == "duplicate_request_in_flight"
    }));
    let session = SessionStore::open(session_path).unwrap();
    assert_eq!(
        session
            .turns()
            .iter()
            .filter(|turn| turn.role == zenpi::core::TurnRole::User)
            .count(),
        1
    );
}

#[derive(Clone, Default)]
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(unix)]
#[test]
fn async_runtime_lifecycle_events_are_correlated_ordered_and_replayable() {
    let (first_started_tx, first_started_rx) = mpsc::sync_channel(1);
    let (cancellation_seen_tx, cancellation_seen_rx) = mpsc::sync_channel(1);
    let backend = BurstSteerBackend {
        calls: AtomicUsize::new(0),
        first_started: first_started_tx,
        cancellation_seen: cancellation_seen_tx,
    };
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("runtime-lifecycle.jsonl");
    let agent = Agent::new(
        SessionStore::open(&session_path).unwrap(),
        Box::new(backend),
    );
    let (mut writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let output = SharedWriter::default();
    let captured = output.clone();
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(agent, reader, output).unwrap();
    });

    writeln!(
        writer,
        "{}",
        serde_json::json!({"schema_version":2,"type":"prompt","id":"life-p","text":"first"})
    )
    .unwrap();
    first_started_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("provider request did not start");
    wait_for_output(&captured, "request_started");

    writeln!(
        writer,
        "{}",
        serde_json::json!({"schema_version":2,"type":"cancel","id":"life-c","target_id":"life-p"})
    )
    .unwrap();
    cancellation_seen_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("provider did not observe cancellation");
    wait_for_output(&captured, "backend_cancelled");

    writeln!(
        writer,
        "{}",
        serde_json::json!({"schema_version":2,"type":"resume","id":"life-r","from_sequence":0})
    )
    .unwrap();
    wait_for_output(&captured, "\"id\":\"life-r\"");
    writeln!(
        writer,
        "{}",
        serde_json::json!({"schema_version":2,"type":"shutdown","id":"life-q"})
    )
    .unwrap();
    drop(writer);
    host.join().unwrap();

    let records = json_lines(&captured.0.lock().unwrap());
    let resume_response_index = records
        .iter()
        .position(|record| record["id"] == "life-r")
        .expect("missing resume response");
    let resume_response = &records[resume_response_index];
    let replayed_count = resume_response["data"]["replayed"].as_u64().unwrap() as usize;
    let original_end = resume_response_index
        .checked_sub(replayed_count)
        .expect("resume replay count exceeds prior output");
    let original_events = records[..original_end]
        .iter()
        .filter(|record| record["type"] == "event")
        .collect::<Vec<_>>();
    let lifecycle = original_events
        .iter()
        .filter(|record| record["request_id"] == "life-p")
        .filter_map(|record| record["event"]["type"].as_str())
        .collect::<Vec<_>>();
    let accepted = lifecycle
        .iter()
        .position(|kind| *kind == "request_accepted")
        .expect("missing runtime acceptance event");
    let started = lifecycle
        .iter()
        .position(|kind| *kind == "request_started")
        .expect("missing runtime start event");
    let cancelled = lifecycle
        .iter()
        .position(|kind| *kind == "cancel_requested")
        .expect("missing runtime cancellation event");
    assert!(accepted < started && started < cancelled);
    assert_eq!(
        original_events
            .iter()
            .map(|record| record["sequence"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        (0..original_events.len() as u64).collect::<Vec<_>>()
    );

    assert_eq!(resume_response["success"], true);
    assert_eq!(resume_response["data"]["replay_gap"], false);
    assert_eq!(replayed_count, original_events.len());
    for kind in ["request_accepted", "request_started", "cancel_requested"] {
        assert_eq!(
            records
                .iter()
                .filter(|record| record["event"]["type"] == kind)
                .count(),
            2,
            "{kind} was not replayed exactly once"
        );
    }
}

#[test]
fn live_steer_cancels_and_reissues_without_losing_or_duplicating_input() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    // Keep the first response open after its initial delta. Otherwise the
    // complete SSE payload can be consumed before the test writes the steer,
    // turning a valid live-steer scenario into a scheduler-dependent
    // `no_active_turn` race.
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (steer_tx, steer_rx) = mpsc::sync_channel(1);
    let server = thread::spawn(move || -> Result<(), String> {
        let (mut first, _) = listener.accept().map_err(|error| error.to_string())?;
        read_http_body(&mut first)?;
        let first_payload = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-1\"}}\n\n"
        );
        let partial = "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n";
        write!(
            first,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{partial}",
            first_payload.len()
        )
        .map_err(|error| error.to_string())?;
        first.flush().map_err(|error| error.to_string())?;
        ready_tx
            .send(())
            .map_err(|error| format!("ready signal failed: {error}"))?;
        steer_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| format!("steer signal failed: {error}"))?;
        // Complete the first stream only after the steer is on the wire. The
        // worker then observes cancellation at its completion boundary and
        // the scheduler can issue exactly one replacement request.
        let _ = first.write_all(&first_payload.as_bytes()[partial.len()..]);
        let _ = first.flush();
        drop(first);

        let (mut second, _) = listener.accept().map_err(|error| error.to_string())?;
        let body = read_http_body(&mut second)?;
        let occurrences = body
            .to_string()
            .matches("use the corrected request")
            .count();
        if occurrences != 1 {
            return Err(format!(
                "steer appeared {occurrences} times in reissue body: {body}"
            ));
        }
        let payload = "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"corrected answer\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-2\"}}\n\n";
        write!(
            second,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    });

    let dir = tempdir().unwrap();
    let path = dir.path().join("live-steer.jsonl");
    let backend = zenpi::backend::OpenAiCompatibleBackend::new_with_wire_api(
        format!("http://127.0.0.1:{port}"),
        None,
        "mock-model",
        zenpi::backend::OpenAiWireApi::Responses,
    )
    .unwrap();
    let agent = Agent::new(SessionStore::open(&path).unwrap(), Box::new(backend));
    let (mut input_writer, input_reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let output = SharedWriter::default();
    let captured = output.clone();
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(agent, input_reader, output).unwrap();
    });
    writeln!(
        input_writer,
        "{}",
        serde_json::json!({"schema_version":2,"type":"prompt","id":"p1","text":"original request"})
    )
    .unwrap();
    wait_for_output(&captured, "partial");
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("fixture did not hold the first stream open");
    writeln!(
        input_writer,
        "{}",
        serde_json::json!({"schema_version":2,"type":"steer","id":"s1","text":"use the corrected request"})
    )
    .unwrap();
    steer_tx.send(()).unwrap();
    wait_for_output(&captured, "corrected answer");
    writeln!(
        input_writer,
        "{}",
        serde_json::json!({"schema_version":2,"type":"shutdown","id":"q1"})
    )
    .unwrap();
    drop(input_writer);
    host.join().unwrap();
    server.join().unwrap().unwrap();

    let output = captured.0.lock().unwrap().clone();
    let records = json_lines(&output);
    assert_eq!(
        records
            .iter()
            .filter(|record| record["id"] == "p1" && record["type"] == "response")
            .count(),
        1
    );
    assert!(
        records
            .iter()
            .any(|record| { record["id"] == "s1" && record["success"] == true })
    );
    // ProviderEvent itself intentionally has no turn_id field.  The async
    // host must source correlation from the accepted AsyncWork metadata.
    let provider_events: Vec<&Value> = records
        .iter()
        .filter(|record| record["type"] == "event" && record["event"]["type"] == "text_delta")
        .collect();
    assert!(!provider_events.is_empty());
    assert!(provider_events.iter().all(|record| {
        record["turn_id"]
            .as_str()
            .is_some_and(|turn_id| turn_id.starts_with("turn-"))
    }));
    let session = SessionStore::open(path).unwrap();
    assert_eq!(
        session
            .turns()
            .iter()
            .filter(|turn| turn.content == "use the corrected request")
            .count(),
        1
    );
    assert_eq!(
        session
            .turns()
            .iter()
            .filter(|turn| {
                turn.role == zenpi::core::TurnRole::Assistant && turn.content == "corrected answer"
            })
            .count(),
        1
    );
}

fn wait_for_output(output: &SharedWriter, needle: &str) {
    for _ in 0..300 {
        if String::from_utf8_lossy(&output.0.lock().unwrap()).contains(needle) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for {needle}; output={}",
        String::from_utf8_lossy(&output.0.lock().unwrap())
    );
}

fn read_http_body(stream: &mut TcpStream) -> Result<Value, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("request ended before headers".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        })
        .ok_or_else(|| "missing content length".to_owned())?;
    while bytes.len() < header_end + length {
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("request ended before body".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    serde_json::from_slice(&bytes[header_end..header_end + length])
        .map_err(|error| error.to_string())
}
