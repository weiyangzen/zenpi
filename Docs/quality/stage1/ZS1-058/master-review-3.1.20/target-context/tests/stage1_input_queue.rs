use std::{fs, process::Command, sync::mpsc, thread, time::Duration};
use tempfile::TempDir;
use zenpi::{
    input_queue::{
        InputKind, InputQueue, InputQueueError, InputQueueLimits, InputQueueReply, InputStatus,
        QueueMode,
    },
    protocol::{InputQueueAction as Action, parse_input_queue_line},
    runtime::InputBoundaryGate,
    session::SessionStore,
};

fn fixture() -> (TempDir, SessionStore, InputQueue) {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionStore::open(dir.path().join("session.jsonl")).unwrap();
    let queue = InputQueue::recover(&session, InputQueueLimits::default()).unwrap();
    (dir, session, queue)
}

fn enqueue(
    queue: &mut InputQueue,
    session: &mut SessionStore,
    id: &str,
    kind: InputKind,
    text: &str,
) {
    queue
        .execute(
            session,
            Action::Enqueue {
                input_id: id.into(),
                kind,
                text: text.into(),
            },
        )
        .unwrap();
}

fn boundary(id: &str, stop: bool) -> zenpi::runtime::InputBoundary {
    InputBoundaryGate::new(id).unwrap().boundary(stop).unwrap()
}

#[test]
fn steer_and_follow_up_have_independent_modes_and_safe_stop_priority() {
    let (_dir, mut session, mut queue) = fixture();
    enqueue(
        &mut queue,
        &mut session,
        "follow-1",
        InputKind::FollowUp,
        "later first",
    );
    enqueue(
        &mut queue,
        &mut session,
        "steer-1",
        InputKind::Steer,
        "now first",
    );
    enqueue(
        &mut queue,
        &mut session,
        "follow-2",
        InputKind::FollowUp,
        "later second",
    );
    enqueue(
        &mut queue,
        &mut session,
        "steer-2",
        InputKind::Steer,
        "now second",
    );
    queue
        .execute(
            &mut session,
            Action::Configure {
                kind: InputKind::FollowUp,
                mode: QueueMode::All,
            },
        )
        .unwrap();
    let first = queue
        .apply_boundary(&mut session, &boundary("model-2", true))
        .unwrap();
    assert_eq!(
        first.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
        ["steer-1"]
    );
    let next = queue
        .apply_boundary(&mut session, &boundary("model-3", false))
        .unwrap();
    assert_eq!(next[0].id, "steer-2");
    assert!(
        queue
            .apply_boundary(&mut session, &boundary("model-4", false))
            .unwrap()
            .is_empty()
    );
    let follow = queue
        .apply_boundary(&mut session, &boundary("model-4", true))
        .unwrap();
    assert_eq!(
        follow.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
        ["follow-1", "follow-2"]
    );
    assert!(follow[0].sequence < follow[1].sequence);
}

#[test]
fn all_steering_preserves_receipt_order_not_lexicographic_ids() {
    let (_dir, mut session, mut queue) = fixture();
    enqueue(&mut queue, &mut session, "z", InputKind::Steer, "one");
    enqueue(&mut queue, &mut session, "a", InputKind::Steer, "two");
    queue
        .execute(
            &mut session,
            Action::Configure {
                kind: InputKind::Steer,
                mode: QueueMode::All,
            },
        )
        .unwrap();
    let inputs = queue
        .apply_boundary(&mut session, &boundary("round", false))
        .unwrap();
    assert_eq!(
        inputs.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
        ["z", "a"]
    );
    assert!(
        queue
            .apply_boundary(&mut session, &boundary("round", true))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn long_prepare_empty_poll_can_pick_up_late_input_once() {
    let (_dir, mut session, mut queue) = fixture();
    let mut gate = InputBoundaryGate::new("after-prepare").unwrap();
    assert!(
        queue
            .apply_boundary(&mut session, &gate.boundary(false).unwrap())
            .unwrap()
            .is_empty()
    );
    gate.begin_prepare().unwrap();
    enqueue(
        &mut queue,
        &mut session,
        "first",
        InputKind::Steer,
        "arrived while preparing",
    );
    enqueue(
        &mut queue,
        &mut session,
        "second",
        InputKind::Steer,
        "also arrived while preparing",
    );
    assert!(gate.boundary(false).is_err());
    gate.finish_prepare().unwrap();
    assert_eq!(
        queue
            .apply_boundary(&mut session, &gate.boundary(false).unwrap())
            .unwrap()[0]
            .id,
        "first"
    );
    assert!(
        queue
            .apply_boundary(&mut session, &gate.boundary(false).unwrap())
            .unwrap()
            .is_empty()
    );
    assert_eq!(queue.get("second").unwrap().status, InputStatus::Received);
}

#[test]
fn nonempty_pre_prepare_snapshot_cannot_consume_second_input() {
    let (_dir, mut session, mut queue) = fixture();
    let mut gate = InputBoundaryGate::new("after-tools").unwrap();
    enqueue(
        &mut queue,
        &mut session,
        "first",
        InputKind::Steer,
        "snapshot",
    );
    let first = queue
        .apply_boundary(&mut session, &gate.boundary(false).unwrap())
        .unwrap();
    gate.begin_prepare().unwrap();
    enqueue(
        &mut queue,
        &mut session,
        "second",
        InputKind::Steer,
        "late arrival",
    );
    gate.finish_prepare().unwrap();
    assert!(
        queue
            .apply_boundary(&mut session, &gate.boundary(false).unwrap())
            .unwrap()
            .is_empty()
    );
    assert_eq!(first[0].text, "snapshot");
    assert_eq!(queue.inputs_for_turn("after-tools"), first);
    assert_eq!(queue.get("second").unwrap().status, InputStatus::Received);
}

#[test]
fn real_file_operations_complete_before_input_boundary_is_available() {
    let (dir, mut session, mut queue) = fixture();
    let mut gate = InputBoundaryGate::new("after-batch").unwrap();
    let revoked = gate.boundary(false).unwrap();
    gate.begin_tool_batch(&["tool-1".into(), "tool-2".into()])
        .unwrap();
    let (release_tx, release_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let second_path = dir.path().join("second.txt");
    let second = thread::spawn(move || {
        release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        fs::write(&second_path, "second side effect").unwrap();
        done_tx.send("tool-2").unwrap();
    });
    fs::write(dir.path().join("first.txt"), "first side effect").unwrap();
    gate.tool_completed("tool-1").unwrap();
    enqueue(
        &mut queue,
        &mut session,
        "follow",
        InputKind::FollowUp,
        "run when stopping",
    );
    enqueue(
        &mut queue,
        &mut session,
        "steer",
        InputKind::Steer,
        "next complete batch",
    );
    assert!(gate.boundary(false).is_err());
    assert!(gate.begin_prepare().is_err());
    assert!(queue.apply_boundary(&mut session, &revoked).is_err());
    assert_eq!(queue.get("steer").unwrap().status, InputStatus::Received);
    release_tx.send(()).unwrap();
    gate.tool_completed(done_rx.recv_timeout(Duration::from_secs(3)).unwrap())
        .unwrap();
    second.join().unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("second.txt")).unwrap(),
        "second side effect"
    );
    assert_eq!(
        queue
            .apply_boundary(&mut session, &gate.boundary(false).unwrap())
            .unwrap()[0]
            .id,
        "steer"
    );
    assert_eq!(queue.get("follow").unwrap().status, InputStatus::Received);
}

#[test]
fn duplicate_ids_are_idempotent_and_different_payload_conflicts_after_edit() {
    let (_dir, mut session, mut queue) = fixture();
    enqueue(
        &mut queue,
        &mut session,
        "same",
        InputKind::Steer,
        "original",
    );
    let before = session.next_sequence();
    let result = queue
        .execute(
            &mut session,
            Action::Enqueue {
                input_id: "same".into(),
                kind: InputKind::Steer,
                text: "original".into(),
            },
        )
        .unwrap();
    assert!(matches!(
        result,
        InputQueueReply::Input {
            duplicate: true,
            ..
        }
    ));
    assert_eq!(before, session.next_sequence());
    queue
        .execute(
            &mut session,
            Action::Edit {
                input_id: "same".into(),
                expected_revision: 0,
                text: "edited".into(),
            },
        )
        .unwrap();
    let result = queue
        .execute(
            &mut session,
            Action::Enqueue {
                input_id: "same".into(),
                kind: InputKind::Steer,
                text: "original".into(),
            },
        )
        .unwrap();
    assert!(
        matches!(result, InputQueueReply::Input { input, duplicate: true } if input.text == "edited")
    );
    for (kind, text) in [
        (InputKind::Steer, "edited"),
        (InputKind::FollowUp, "original"),
    ] {
        assert!(matches!(
            queue.execute(
                &mut session,
                Action::Enqueue {
                    input_id: "same".into(),
                    kind,
                    text: text.into()
                }
            ),
            Err(InputQueueError::Conflict)
        ));
    }
}

#[test]
fn edit_uses_revision_and_applied_input_is_immutable() {
    let (_dir, mut session, mut queue) = fixture();
    enqueue(
        &mut queue,
        &mut session,
        "one",
        InputKind::Steer,
        "original",
    );
    queue
        .execute(
            &mut session,
            Action::Edit {
                input_id: "one".into(),
                expected_revision: 0,
                text: "改过的输入".into(),
            },
        )
        .unwrap();
    assert!(matches!(
        queue.execute(
            &mut session,
            Action::Edit {
                input_id: "one".into(),
                expected_revision: 0,
                text: "racing edit".into()
            }
        ),
        Err(InputQueueError::StaleRevision)
    ));
    let applied = queue
        .apply_boundary(&mut session, &boundary("model", false))
        .unwrap();
    assert_eq!(applied[0].text, "改过的输入");
    assert_eq!(applied[0].revision, 1);
    for action in [
        Action::Edit {
            input_id: "one".into(),
            expected_revision: 1,
            text: "too late".into(),
        },
        Action::Cancel {
            input_id: "one".into(),
        },
    ] {
        assert!(matches!(
            queue.execute(&mut session, action),
            Err(InputQueueError::Terminal)
        ));
    }
}

#[test]
fn queue_cancellation_and_execution_cancellation_have_separate_effects() {
    let (_dir, mut session, mut queue) = fixture();
    enqueue(
        &mut queue,
        &mut session,
        "removed",
        InputKind::FollowUp,
        "remove me",
    );
    queue
        .execute(
            &mut session,
            Action::Cancel {
                input_id: "removed".into(),
            },
        )
        .unwrap();
    assert_eq!(queue.get("removed").unwrap().status, InputStatus::Cancelled);
    let before = session.next_sequence();
    queue
        .execute(
            &mut session,
            Action::Cancel {
                input_id: "removed".into(),
            },
        )
        .unwrap();
    assert_eq!(before, session.next_sequence());
    enqueue(
        &mut queue,
        &mut session,
        "retained",
        InputKind::Steer,
        "not applied by cancelled execution",
    );
    let gate = InputBoundaryGate::new("cancelled-turn").unwrap();
    let permit = gate.boundary(false).unwrap();
    gate.cancel();
    assert!(queue.apply_boundary(&mut session, &permit).is_err());
    assert!(gate.boundary(true).is_err());
    assert_eq!(queue.get("retained").unwrap().status, InputStatus::Received);
}

#[test]
fn saturation_preserves_previous_queue_and_journal() {
    let (_dir, mut session, _) = fixture();
    let limits = InputQueueLimits {
        max_pending: 1,
        max_records: 2,
        max_retained_bytes: 20,
    };
    let mut queue = InputQueue::recover(&session, limits).unwrap();
    enqueue(&mut queue, &mut session, "a", InputKind::Steer, "first");
    let before = session.next_sequence();
    assert!(matches!(
        queue.execute(
            &mut session,
            Action::Enqueue {
                input_id: "b".into(),
                kind: InputKind::FollowUp,
                text: "second".into()
            }
        ),
        Err(InputQueueError::Capacity)
    ));
    assert!(matches!(
        queue.execute(
            &mut session,
            Action::Edit {
                input_id: "a".into(),
                expected_revision: 0,
                text: "x".repeat(30)
            }
        ),
        Err(InputQueueError::Capacity)
    ));
    assert_eq!(session.next_sequence(), before);
    assert_eq!(queue.get("a").unwrap().text, "first");
    queue
        .execute(
            &mut session,
            Action::Cancel {
                input_id: "a".into(),
            },
        )
        .unwrap();
    enqueue(&mut queue, &mut session, "b", InputKind::Steer, "second");
    queue
        .apply_boundary(&mut session, &boundary("round", false))
        .unwrap();
    assert!(matches!(
        queue.execute(
            &mut session,
            Action::Enqueue {
                input_id: "c".into(),
                kind: InputKind::Steer,
                text: "third".into()
            }
        ),
        Err(InputQueueError::Capacity)
    ));
    assert_eq!(queue.get("a").unwrap().status, InputStatus::Cancelled);
}

#[test]
fn stale_session_writer_does_not_publish_received_input() {
    let (dir, mut session, mut queue) = fixture();
    let mut stale_session = SessionStore::open_existing(dir.path().join("session.jsonl")).unwrap();
    let mut stale_queue = InputQueue::recover(&stale_session, InputQueueLimits::default()).unwrap();
    enqueue(&mut queue, &mut session, "a", InputKind::Steer, "first");
    assert!(
        stale_queue
            .execute(
                &mut stale_session,
                Action::Enqueue {
                    input_id: "b".into(),
                    kind: InputKind::Steer,
                    text: "not committed".into()
                }
            )
            .is_err()
    );
    assert!(stale_queue.get("b").is_none());
    assert!(matches!(
        stale_queue.execute(
            &mut session,
            Action::Configure {
                kind: InputKind::Steer,
                mode: QueueMode::All
            }
        ),
        Err(InputQueueError::StaleWriter)
    ));
}

#[test]
fn queue_cannot_write_into_another_session() {
    let (_dir, _session, mut queue) = fixture();
    let (_other, mut session, _) = fixture();
    assert!(matches!(
        queue.execute(
            &mut session,
            Action::List {
                after_sequence: None,
                limit: 10
            }
        ),
        Err(InputQueueError::WrongSession)
    ));
}

#[test]
fn transport_request_is_bound_to_the_actual_session_owner() {
    let (_dir, mut session, mut queue) = fixture();
    let value = serde_json::json!({"schema_version":2,"id":"request-1","type":"input_queue",
        "session_id":session.summary().session_id,
        "input_queue":{"action":"enqueue","input_id":"input-1","kind":"follow_up","text":"persist through owner"}});
    let reply = queue
        .execute_request(
            &mut session,
            parse_input_queue_line(&value.to_string()).unwrap(),
        )
        .unwrap();
    assert!(
        matches!(reply, InputQueueReply::Input { input, duplicate: false } if input.status == InputStatus::Received)
    );
    let mut wrong_session = value.clone();
    wrong_session["session_id"] = "another-session".into();
    assert!(matches!(
        queue.execute_request(
            &mut session,
            parse_input_queue_line(&wrong_session.to_string()).unwrap()
        ),
        Err(InputQueueError::WrongSession)
    ));
    assert_eq!(
        InputQueue::recover(&session, InputQueueLimits::default())
            .unwrap()
            .get("input-1")
            .unwrap()
            .text,
        "persist through owner"
    );
}

#[test]
fn list_pages_use_receipt_sequence_and_return_real_edited_status() {
    let (_dir, mut session, mut queue) = fixture();
    enqueue(&mut queue, &mut session, "z", InputKind::FollowUp, "first");
    enqueue(&mut queue, &mut session, "a", InputKind::FollowUp, "second");
    queue
        .execute(
            &mut session,
            Action::Edit {
                input_id: "z".into(),
                expected_revision: 0,
                text: "edited".into(),
            },
        )
        .unwrap();
    let InputQueueReply::Page {
        inputs,
        next_sequence,
    } = queue
        .execute(
            &mut session,
            Action::List {
                after_sequence: None,
                limit: 1,
            },
        )
        .unwrap()
    else {
        panic!("expected page")
    };
    assert_eq!(inputs[0].id, "z");
    assert_eq!(inputs[0].text, "edited");
    let InputQueueReply::Page {
        inputs,
        next_sequence,
    } = queue
        .execute(
            &mut session,
            Action::List {
                after_sequence: next_sequence,
                limit: 1,
            },
        )
        .unwrap()
    else {
        panic!("expected page")
    };
    assert_eq!(inputs[0].id, "a");
    assert_eq!(next_sequence, None);
}

#[test]
fn strict_protocol_preserves_ids_and_rejects_invalid_or_ambiguous_fields() {
    let value = serde_json::json!({"schema_version":2,"id":"request-1","type":"input_queue","session_id":"session-1",
        "input_queue":{"action":"enqueue","input_id":"message-1","kind":"follow_up","text":"你好"}});
    let request = parse_input_queue_line(&value.to_string()).unwrap();
    assert_eq!(request.id, "request-1");
    assert!(
        matches!(request.input_queue, Action::Enqueue { input_id, kind: InputKind::FollowUp, .. } if input_id == "message-1")
    );
    for (field, bad) in [
        ("schema_version", serde_json::json!(1)),
        ("unexpected", serde_json::json!(true)),
        ("session_id", serde_json::json!("\n")),
    ] {
        let mut bad_value = value.clone();
        bad_value[field] = bad;
        assert!(parse_input_queue_line(&bad_value.to_string()).is_err());
    }
    for text in ["", "\0", "!rm something"] {
        let mut bad = value.clone();
        bad["input_queue"]["text"] = text.into();
        assert!(parse_input_queue_line(&bad.to_string()).is_err());
    }
    let mut bad = value.clone();
    bad["input_queue"]["kind"] = "other".into();
    assert!(parse_input_queue_line(&bad.to_string()).is_err());
    let mut bad = value.clone();
    bad["input_queue"]["expected_revision"] = 1.into();
    assert!(parse_input_queue_line(&bad.to_string()).is_err());
}

#[test]
fn restart_in_separate_process_preserves_applied_and_pending_identities() {
    let (dir, mut session, mut queue) = fixture();
    enqueue(
        &mut queue,
        &mut session,
        "applied",
        InputKind::Steer,
        "must remain in reconstructed context",
    );
    queue
        .apply_boundary(&mut session, &boundary("committed-turn", false))
        .unwrap();
    enqueue(
        &mut queue,
        &mut session,
        "pending",
        InputKind::FollowUp,
        "must still be pending",
    );
    drop(session);
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "restart_probe", "--nocapture"])
        .env(
            "ZENPI_INPUT_QUEUE_RESTART_JOURNAL",
            dir.path().join("session.jsonl"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    let session = SessionStore::open_existing(dir.path().join("session.jsonl")).unwrap();
    let recovered = InputQueue::recover(&session, InputQueueLimits::default()).unwrap();
    assert_eq!(
        recovered.get("pending").unwrap().applied_turn_id.as_deref(),
        Some("resumed-turn")
    );
}

#[test]
#[ignore = "invoked only by the separate-process recovery test with a real journal"]
fn restart_probe() {
    let path =
        std::env::var_os("ZENPI_INPUT_QUEUE_RESTART_JOURNAL").expect("parent provides journal");
    let mut session = SessionStore::open_existing(path).unwrap();
    let mut queue = InputQueue::recover(&session, InputQueueLimits::default()).unwrap();
    assert_eq!(
        queue.inputs_for_turn("committed-turn")[0].text,
        "must remain in reconstructed context"
    );
    assert!(
        queue
            .apply_boundary(&mut session, &boundary("committed-turn", false))
            .unwrap()
            .is_empty()
    );
    assert_eq!(queue.get("pending").unwrap().status, InputStatus::Received);
    assert_eq!(
        queue
            .apply_boundary(&mut session, &boundary("resumed-turn", true))
            .unwrap()[0]
            .id,
        "pending"
    );
}

#[test]
fn malformed_queue_events_fail_recovery_without_rewriting_the_journal() {
    let (dir, mut session, mut queue) = fixture();
    enqueue(&mut queue, &mut session, "a", InputKind::Steer, "content");
    let mut forged = session
        .events()
        .iter()
        .find(|v| v["type"] == "input_queue")
        .unwrap()
        .clone();
    forged["sequence"] = 1.into();
    forged["change"]["input"]["status"] = "applied".into();
    session.append_event(forged).unwrap();
    let before = fs::read(dir.path().join("session.jsonl")).unwrap();
    assert!(InputQueue::recover(&session, InputQueueLimits::default()).is_err());
    assert_eq!(before, fs::read(dir.path().join("session.jsonl")).unwrap());
}

#[test]
fn interrupted_final_append_restores_last_complete_queue_event() {
    use std::io::Write;
    let (dir, mut session, mut queue) = fixture();
    enqueue(
        &mut queue,
        &mut session,
        "retained",
        InputKind::FollowUp,
        "durable",
    );
    drop(session);
    let path = dir.path().join("session.jsonl");
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{\"kind\":\"event\"")
        .unwrap();
    let session = SessionStore::open(&path).unwrap();
    let queue = InputQueue::recover(&session, InputQueueLimits::default()).unwrap();
    assert_eq!(queue.get("retained").unwrap().status, InputStatus::Received);
}
