use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;
use zenpi::{
    core::{Turn, TurnRole},
    session::{
        InterruptedOperation, MAX_SESSION_BYTES, OperationKind, OperationOutcome,
        OperationRecoveryState, SessionError, SessionStore,
    },
};

#[test]
fn typed_lifecycle_archive_resume_migrate_and_delete_are_restart_safe() {
    use zenpi::session::{
        SessionLifecycleReceipt as Receipt, SessionLifecycleRequest as Request,
        execute_session_lifecycle, session_catalog,
    };
    let dir = tempdir().unwrap();
    let original = dir.path().join("original.jsonl");
    let mut store = SessionStore::open(&original).unwrap();
    store
        .append_turn(Turn::new("u", TurnRole::User, "history"))
        .unwrap();
    let source_bytes = fs::read(&original).unwrap();
    let migrated = dir.path().join("migrated.jsonl");
    let receipt = execute_session_lifecycle(
        Request::Migrate {
            source: original.clone(),
            destination: migrated.clone(),
        },
        None,
        1,
    )
    .unwrap();
    assert!(matches!(receipt, Receipt::Session { .. }));
    assert_eq!(fs::read(&original).unwrap(), source_bytes);
    assert_eq!(fs::read(&migrated).unwrap(), source_bytes);
    execute_session_lifecycle(
        Request::Archive {
            path: migrated.clone(),
        },
        None,
        2,
    )
    .unwrap();
    assert!(
        SessionStore::open_existing(&migrated)
            .unwrap()
            .is_archived()
    );
    assert_eq!(session_catalog(dir.path(), false).unwrap().len(), 1);
    let resumed = execute_session_lifecycle(
        Request::ResumeLast {
            directory: dir.path().into(),
        },
        None,
        3,
    )
    .unwrap();
    assert!(
        matches!(resumed, Receipt::Session { session } if session.summary.path == original.display().to_string())
    );
    execute_session_lifecycle(
        Request::Unarchive {
            path: migrated.clone(),
        },
        None,
        4,
    )
    .unwrap();
    assert!(
        !SessionStore::open_existing(&migrated)
            .unwrap()
            .is_archived()
    );
    assert!(
        execute_session_lifecycle(
            Request::Delete {
                path: migrated.clone(),
                confirmed: false
            },
            None,
            5
        )
        .is_err()
    );
    assert!(
        execute_session_lifecycle(
            Request::Delete {
                path: original.clone(),
                confirmed: true
            },
            Some(&original),
            5
        )
        .is_err()
    );
    execute_session_lifecycle(
        Request::Delete {
            path: migrated.clone(),
            confirmed: true,
        },
        Some(&original),
        5,
    )
    .unwrap();
    assert!(!migrated.exists());
    assert_eq!(fs::read(original).unwrap(), source_bytes);
}

#[test]
fn fork_remaps_signed_handoffs_and_preserves_order_without_pending_ownership() {
    use zenpi::b3::HandoffRecord;
    let dir = tempdir().unwrap();
    let mut source = SessionStore::open(dir.path().join("source.jsonl")).unwrap();
    let source_id = source.session_id().to_owned();
    source.append_event(serde_json::json!({"type":"context", "session_id":source_id, "nested":{"owner_session_id":source_id}})).unwrap();
    source
        .append_turn(Turn::new("u", TurnRole::User, "after event"))
        .unwrap();
    source
        .append_handoff_record(
            HandoffRecord::new(
                "sender",
                "receiver",
                "claim",
                "handoff",
                vec![],
                &source_id,
                "2026-09-05T00:00:00Z",
            )
            .unwrap(),
        )
        .unwrap();
    source
        .begin_operation(&InterruptedOperation {
            operation_id: "pending".into(),
            kind: OperationKind::Tool,
            turn_id: "u".into(),
            retry_requires_confirmation: true,
        })
        .unwrap();
    let before = fs::read(source.path()).unwrap();
    let fork_path = dir.path().join("fork.jsonl");
    let fork = source.fork_to(&fork_path).unwrap();
    assert_ne!(fork.session_id(), source_id);
    assert_eq!(fork.records()[1].kind, "event");
    assert_eq!(fork.records()[2].kind, "turn");
    assert_eq!(fork.events()[0]["session_id"], fork.session_id());
    assert_eq!(
        fork.events()[0]["nested"]["owner_session_id"],
        fork.session_id()
    );
    assert_eq!(fork.handoff_records()[0].session_id, fork.session_id());
    fork.handoff_records()[0].validate().unwrap();
    assert!(fork.interrupted_operations().is_empty());
    let recovered = SessionStore::open_existing(&fork_path).unwrap();
    assert!(recovered.recovery_warnings().is_empty());
    assert_eq!(recovered.handoff_records().len(), 1);
    assert_eq!(fs::read(source.path()).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn addressed_mailbox_deduplicates_and_persists_ack_claim_result() {
    use zenpi::session::{MailboxAction, MailboxStatus, SessionMailbox};
    let dir = tempdir().unwrap();
    let sender = SessionStore::open(dir.path().join("sender.jsonl")).unwrap();
    let recipient = SessionStore::open(dir.path().join("recipient.jsonl")).unwrap();
    let mailbox = SessionMailbox::open(recipient.path()).unwrap();
    let payload = serde_json::json!({"instruction":"review patch"});
    let queued = mailbox
        .enqueue(&sender, "request-1", payload.clone(), 10_000, 1)
        .unwrap();
    let before = fs::read(mailbox.path()).unwrap();
    assert_eq!(
        mailbox
            .enqueue(&sender, "request-1", payload, 10_000, 2)
            .unwrap(),
        queued
    );
    assert_eq!(fs::read(mailbox.path()).unwrap(), before);
    assert!(
        mailbox
            .enqueue(
                &sender,
                "request-1",
                serde_json::json!("different"),
                10_000,
                2
            )
            .is_err()
    );
    assert!(mailbox.list(&sender, 0, 10, 2).is_err());
    let acked = mailbox
        .update(
            &recipient,
            sender.session_id(),
            "request-1",
            MailboxAction::Acknowledge,
            2,
        )
        .unwrap();
    assert_eq!(acked.status, MailboxStatus::Acknowledged);
    let claimed = mailbox
        .update(
            &recipient,
            sender.session_id(),
            "request-1",
            MailboxAction::Claim {
                token: "owner-1".into(),
            },
            3,
        )
        .unwrap();
    assert_eq!(claimed.status, MailboxStatus::Claimed);
    assert!(
        mailbox
            .update(
                &recipient,
                sender.session_id(),
                "request-1",
                MailboxAction::Claim {
                    token: "owner-2".into()
                },
                4
            )
            .is_err()
    );
    let action = MailboxAction::Complete {
        token: "owner-1".into(),
        result: serde_json::json!({"review":"done"}),
    };
    mailbox
        .update(
            &recipient,
            sender.session_id(),
            "request-1",
            action.clone(),
            5,
        )
        .unwrap();
    let before = fs::read(mailbox.path()).unwrap();
    let reopened = SessionMailbox::open(recipient.path()).unwrap();
    reopened
        .update(&recipient, sender.session_id(), "request-1", action, 6)
        .unwrap();
    assert_eq!(fs::read(mailbox.path()).unwrap(), before);
    let page = reopened.list(&recipient, 0, 10, 20_000).unwrap();
    assert_eq!(page.messages.len(), 1);
    assert_eq!(page.messages[0].status, MailboxStatus::Succeeded);
    assert_eq!(page.messages[0].sequence, queued.sequence);
    assert!(sender.turns().is_empty());
    assert!(recipient.turns().is_empty());
}

#[cfg(unix)]
#[test]
fn mailbox_ttl_pagination_secrets_and_archive_fail_closed() {
    use zenpi::session::{MailboxAction, MailboxStatus, SessionMailbox};
    let dir = tempdir().unwrap();
    let sender = SessionStore::open(dir.path().join("sender.jsonl")).unwrap();
    let mut recipient = SessionStore::open(dir.path().join("recipient.jsonl")).unwrap();
    let mailbox = SessionMailbox::open(recipient.path()).unwrap();
    assert!(
        mailbox
            .enqueue(
                &sender,
                "secret",
                serde_json::json!({"api_key":"private"}),
                100,
                1
            )
            .is_err()
    );
    assert!(
        mailbox
            .enqueue(&sender, "expired", serde_json::json!("x"), 0, 1)
            .is_err()
    );
    for i in 0..3 {
        mailbox
            .enqueue(&sender, &format!("r-{i}"), serde_json::json!(i), 100, 1)
            .unwrap();
    }
    let page = mailbox.list(&recipient, 0, 2, 2).unwrap();
    assert_eq!(page.messages.len(), 2);
    let next = mailbox
        .list(&recipient, page.next_sequence.unwrap(), 2, 2)
        .unwrap();
    assert_eq!(next.messages.len(), 1);
    assert!(next.next_sequence.is_none());
    assert!(
        mailbox
            .update(
                &recipient,
                sender.session_id(),
                "r-0",
                MailboxAction::Claim {
                    token: "owner".into()
                },
                101
            )
            .is_err()
    );
    assert!(
        mailbox
            .list(&recipient, 0, 3, 101)
            .unwrap()
            .messages
            .iter()
            .all(|message| message.status == MailboxStatus::Expired)
    );
    recipient.set_archived(true).unwrap();
    assert!(
        mailbox
            .enqueue(&sender, "r-4", serde_json::json!(4), 100, 2)
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn mailbox_lock_torn_tail_and_cross_workspace_access_are_rejected() {
    use std::os::fd::AsRawFd;
    use zenpi::session::SessionMailbox;
    let dir = tempdir().unwrap();
    let sender = SessionStore::open(dir.path().join("sender.jsonl")).unwrap();
    let recipient = SessionStore::open(dir.path().join("recipient.jsonl")).unwrap();
    let mailbox = SessionMailbox::open(recipient.path()).unwrap();
    mailbox
        .enqueue(&sender, "r", serde_json::json!("work"), 100, 1)
        .unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(mailbox.path())
        .unwrap();
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    assert!(
        mailbox
            .enqueue(&sender, "r-2", serde_json::json!("work"), 100, 1)
            .is_err()
    );
    assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) }, 0);
    let mut bytes = fs::read(mailbox.path()).unwrap();
    bytes.extend_from_slice(b"{\"sequence\":");
    fs::write(mailbox.path(), &bytes).unwrap();
    assert!(
        mailbox
            .enqueue(&sender, "r-2", serde_json::json!("work"), 100, 1)
            .is_err()
    );
    assert_eq!(fs::read(mailbox.path()).unwrap(), bytes);
    let other = tempdir().unwrap();
    let text = fs::read_to_string(sender.path()).unwrap();
    let mut header: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    header["cwd"] = serde_json::json!(other.path());
    fs::write(sender.path(), format!("{header}\n")).unwrap();
    assert!(
        mailbox
            .enqueue(&sender, "r-3", serde_json::json!("work"), 100, 1)
            .is_err()
    );
}

#[test]
fn malformed_prefix_is_warned_and_append_remains_recoverable() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("recovery.jsonl");
    fs::write(&path, b"{not-json}\n").unwrap();
    let mut store = SessionStore::open(&path).unwrap();
    assert_eq!(store.recovery_warnings().len(), 1);
    store
        .append_turn(Turn::new("u", TurnRole::User, "kept"))
        .unwrap();
    let recovered = SessionStore::open(&path).unwrap();
    assert_eq!(recovered.turns().len(), 1);
    assert!(recovered.summary().next_seq >= 2);
}

#[test]
fn validated_records_retain_durable_sequences_for_replay_owners() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("records.jsonl");
    let mut store = SessionStore::open(&path).unwrap();
    store
        .append_turn(Turn::new("u", TurnRole::User, "hello"))
        .unwrap();
    store
        .append_event(serde_json::json!({"type":"progress"}))
        .unwrap();
    let records = store.records();
    assert_eq!(records.len(), 3, "header, turn, and event");
    assert_eq!(records[0].sequence, 0);
    assert_eq!(records[1].kind, "turn");
    assert_eq!(records[2].kind, "event");
    assert_eq!(store.next_sequence(), 3);
}

#[test]
fn invalid_high_sequence_does_not_poison_following_appends() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("high-sequence.jsonl");
    let store = SessionStore::open(&path).unwrap();
    let session_id = store.session_id().to_owned();
    let mut raw = fs::read_to_string(&path).unwrap();
    raw.push_str(&format!(
        "{{\"kind\":\"turn\",\"turn\":{{\"id\":\"bad\",\"role\":\"user\",\"content\":\"\"}},\"schema_version\":1,\"session_id\":\"{session_id}\",\"seq\":18446744073709551615}}\n"
    ));
    fs::write(&path, raw).unwrap();

    let mut recovered = SessionStore::open(&path).unwrap();
    assert_eq!(recovered.next_sequence(), 1);
    recovered
        .append_turn(Turn::new("u", TurnRole::User, "after invalid"))
        .unwrap();
    let reopened = SessionStore::open(&path).unwrap();
    assert_eq!(reopened.turns().len(), 1);
    assert_eq!(reopened.turns()[0].content, "after invalid");
    assert!(!reopened.recovery_warnings().is_empty());
}

#[test]
fn records_before_a_header_are_ignored_instead_of_binding_foreign_state() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("header-order.jsonl");
    fs::write(
        &path,
        b"{\"kind\":\"event\",\"event\":{\"type\":\"foreign\"},\"schema_version\":1,\"session_id\":\"foreign\",\"seq\":0}\n",
    )
    .unwrap();
    let store = SessionStore::open(&path).unwrap();
    assert_eq!(store.events().len(), 0);
    assert_ne!(store.session_id(), "foreign");
    assert!(
        store
            .recovery_warnings()
            .iter()
            .any(|warning| warning.reason.contains("before session header"))
    );
}

#[test]
fn startup_rejects_an_oversized_journal_without_modifying_it() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("oversized.jsonl");
    let file = fs::File::create(&path).unwrap();
    file.set_len(MAX_SESSION_BYTES as u64 + 1).unwrap();
    #[cfg(unix)]
    {
        let mut permissions = file.metadata().unwrap().permissions();
        permissions.set_mode(0o644);
        file.set_permissions(permissions).unwrap();
    }
    let before = file.metadata().unwrap();
    drop(file);

    let error = SessionStore::open(&path).unwrap_err();
    assert!(matches!(
        error,
        SessionError::LimitExceeded { max, actual }
            if max == MAX_SESSION_BYTES && actual == MAX_SESSION_BYTES as u64 + 1
    ));
    let after = fs::metadata(&path).unwrap();
    assert_eq!(after.len(), before.len());
    #[cfg(unix)]
    assert_eq!(
        after.permissions().mode() & 0o777,
        before.permissions().mode() & 0o777
    );
}

#[test]
fn startup_accepts_a_journal_within_the_byte_limit() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("within-limit.jsonl");
    let mut store = SessionStore::open(&path).unwrap();
    store
        .append_turn(Turn::new("u", TurnRole::User, "ordinary history"))
        .unwrap();
    drop(store);
    let before = fs::read(&path).unwrap();
    assert!(before.len() < MAX_SESSION_BYTES);

    let store = SessionStore::open_existing(&path).unwrap();
    assert_eq!(store.turns().len(), 1);
    assert_eq!(store.turns()[0].content, "ordinary history");
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn unfinished_operations_are_detected_and_never_retried_implicitly() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("interrupted.jsonl");
    let mut store = SessionStore::open(&path).unwrap();
    let provider = InterruptedOperation {
        operation_id: "provider-turn-1".into(),
        kind: OperationKind::Provider,
        turn_id: "turn-1".into(),
        retry_requires_confirmation: false,
    };
    let tool = InterruptedOperation {
        operation_id: "tool-call-1".into(),
        kind: OperationKind::Tool,
        turn_id: "turn-1".into(),
        retry_requires_confirmation: true,
    };
    store.begin_operation(&provider).unwrap();
    store.begin_operation(&tool).unwrap();
    store
        .finish_operation(&provider.operation_id, OperationOutcome::Succeeded)
        .unwrap();
    drop(store);

    let mut recovered = SessionStore::open(&path).unwrap();
    assert_eq!(recovered.interrupted_operations(), vec![tool.clone()]);
    let marked = recovered.mark_interrupted_operations().unwrap();
    assert_eq!(marked, vec![tool]);
    assert!(recovered.interrupted_operations().is_empty());
    let reopened = SessionStore::open(path).unwrap();
    assert!(reopened.interrupted_operations().is_empty());
    assert!(reopened.events().iter().any(|event| {
        event["operation_id"] == "tool-call-1" && event["outcome"] == "interrupted"
    }));
}

#[test]
fn operation_markers_are_idempotent_and_recovery_requires_a_new_decision() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("operation-marker.jsonl");
    let mut store = SessionStore::open(&path).unwrap();
    let operation = InterruptedOperation {
        operation_id: "tool-attempt-1".into(),
        kind: OperationKind::Tool,
        turn_id: "turn-1".into(),
        retry_requires_confirmation: true,
    };
    store.begin_operation_with_key(&operation, "idem-1").unwrap();
    let before = fs::read(&path).unwrap();
    store.begin_operation_with_key(&operation, "idem-1").unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(store
        .begin_operation_with_key(&operation, "idem-2")
        .is_err());
    assert_eq!(store.operation_recovery().len(), 1);
    assert_eq!(
        store.operation_recovery()[0].state,
        OperationRecoveryState::UnknownOutcome
    );
    store
        .finish_operation(&operation.operation_id, OperationOutcome::UnknownOutcome)
        .unwrap();
    let after = fs::read(&path).unwrap();
    store
        .finish_operation(&operation.operation_id, OperationOutcome::UnknownOutcome)
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), after);
    assert!(store
        .finish_operation(&operation.operation_id, OperationOutcome::Succeeded)
        .is_err());
    assert_eq!(store.operation_recovery().len(), 1);
    store
        .decide_operation_recovery(
            &operation.operation_id,
            zenpi::core::ToolRecoveryDecision::Retry,
        )
        .unwrap();
    assert!(store.operation_recovery().is_empty());
    let next = fs::read(&path).unwrap();
    store
        .decide_operation_recovery(
            &operation.operation_id,
            zenpi::core::ToolRecoveryDecision::Retry,
        )
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), next);
}

#[test]
fn unterminated_tail_gets_a_separator_before_append() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("tail.jsonl");
    fs::write(&path, b"{\"kind\":\"session\",\"version\":1,\"session_id\":\"s\",\"created_at_ms\":1,\"cwd\":\".\"}").unwrap();
    let mut store = SessionStore::open(&path).unwrap();
    store
        .append_turn(Turn::new("u", TurnRole::User, "ok"))
        .unwrap();
    assert!(fs::read_to_string(path).unwrap().contains("}\n{"));
}

#[test]
fn out_of_order_envelope_is_warned() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("order.jsonl");
    let mut store = SessionStore::open(&path).unwrap();
    store
        .append_turn(Turn::new("u", TurnRole::User, "ok"))
        .unwrap();
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("{\"kind\":\"event\",\"event\":{},\"schema_version\":1,\"session_id\":\"");
    text.push_str(store.session_id());
    text.push_str("\",\"seq\":0}\n");
    fs::write(&path, text).unwrap();
    assert!(
        !SessionStore::open(path)
            .unwrap()
            .recovery_warnings()
            .is_empty()
    );
}

#[test]
fn rejected_business_record_does_not_advance_recovery_sequence() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("rejected-sequence.jsonl");
    let high_sequence = u64::MAX - 1;
    let header = serde_json::json!({
        "kind": "session",
        "version": 1,
        "session_id": "session-a",
        "created_at_ms": 1,
        "cwd": ".",
        "schema_version": 1,
        "seq": 0
    });
    let invalid_turn = serde_json::json!({
        "kind": "turn",
        "turn": {
            "id": "",
            "role": "user",
            "content": "must be ignored",
            "created_at_ms": 1
        },
        "schema_version": 1,
        "session_id": "session-a",
        "seq": high_sequence
    });
    let following_event = serde_json::json!({
        "kind": "event",
        "event": {"type": "survived"},
        "schema_version": 1,
        "session_id": "session-a",
        "seq": 1
    });
    fs::write(
        &path,
        format!("{header}\n{invalid_turn}\n{following_event}\n"),
    )
    .unwrap();

    let store = SessionStore::open(&path).unwrap();
    assert!(
        store
            .recovery_warnings()
            .iter()
            .any(|warning| warning.reason.contains("invalid turn"))
    );
    assert_eq!(store.events(), &[serde_json::json!({"type": "survived"})]);
    assert_eq!(store.next_sequence(), 2);
    assert_eq!(
        store
            .records()
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        [0, 1]
    );
}

#[test]
fn maximum_sequence_is_ignored_without_poisoning_later_appends() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("maximum-sequence.jsonl");
    let header = serde_json::json!({
        "kind": "session",
        "version": 1,
        "session_id": "session-a",
        "created_at_ms": 1,
        "cwd": ".",
        "schema_version": 1,
        "seq": 0
    });
    let exhausted = serde_json::json!({
        "kind": "event",
        "event": {"type": "poison"},
        "schema_version": 1,
        "session_id": "session-a",
        "seq": u64::MAX
    });
    fs::write(&path, format!("{header}\n{exhausted}\n")).unwrap();

    let mut store = SessionStore::open(&path).unwrap();
    assert_eq!(store.next_sequence(), 1);
    assert!(store.events().is_empty());
    assert!(
        store
            .recovery_warnings()
            .iter()
            .any(|warning| warning.reason.contains("exhausted sequence"))
    );
    store
        .append_event(serde_json::json!({"type": "survived"}))
        .unwrap();
    drop(store);

    let reopened = SessionStore::open_existing(&path).unwrap();
    assert_eq!(
        reopened.events(),
        &[serde_json::json!({"type": "survived"})]
    );
    assert_eq!(reopened.next_sequence(), 2);
}

#[test]
fn append_rejects_sequence_exhaustion_before_mutating_the_journal() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("exhausted-append.jsonl");
    let header = serde_json::json!({
        "kind": "session",
        "version": 1,
        "session_id": "session-a",
        "created_at_ms": 1,
        "cwd": ".",
        "schema_version": 1,
        "seq": u64::MAX - 1
    });
    fs::write(&path, format!("{header}\n")).unwrap();
    let mut store = SessionStore::open(&path).unwrap();
    assert_eq!(store.next_sequence(), u64::MAX);
    let before = fs::read(&path).unwrap();

    let error = store
        .append_event(serde_json::json!({"type": "must-not-write"}))
        .unwrap_err();
    assert!(matches!(
        error,
        SessionError::InvalidRecord(reason) if reason.contains("sequence space is exhausted")
    ));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn foreign_runtime_intent_before_header_is_quarantined() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("pre-header-intent.jsonl");
    let intent = serde_json::json!({
        "schema_version": 1,
        "intent_id": "intent-1",
        "kind": "compete",
        "parent_ref": "goal-1",
        "args": ["audit"],
        "route": {
            "route_id": "route-1",
            "parent_ref": "goal-1",
            "route_class": "external_compete",
            "runner": "external_b3ehive",
            "validator_strength": "host_selected"
        },
        "envelope": {
            "envelope_id": "envelope-1",
            "owner": "compete",
            "limit": {
                "tokens": 1,
                "wall_clock_ms": 1,
                "attempts": 1,
                "disk_bytes": 1
            },
            "spent": {
                "tokens": 0,
                "wall_clock_ms": 0,
                "attempts": 0,
                "disk_bytes": 0
            },
            "status": "active"
        },
        "session_id": "foreign-session",
        "created_at_ms": 1
    });
    let pre_header = serde_json::json!({
        "kind": "runtime_intent",
        "intent": intent,
        "schema_version": 1,
        "session_id": "foreign-session",
        "seq": 0
    });
    fs::write(&path, format!("{pre_header}\n")).unwrap();

    let generated_session_id = {
        let store = SessionStore::open(&path).unwrap();
        assert!(store.runtime_intents().is_empty());
        assert_eq!(store.next_sequence(), 1);
        assert!(
            store
                .recovery_warnings()
                .iter()
                .any(|warning| warning.reason.contains("before session header"))
        );
        store.session_id().to_owned()
    };

    let reopened = SessionStore::open_existing(&path).unwrap();
    assert_eq!(reopened.session_id(), generated_session_id);
    assert!(reopened.runtime_intents().is_empty());
    assert_eq!(reopened.records().len(), 1);
    assert_eq!(reopened.records()[0].kind, "session");
    assert_eq!(reopened.records()[0].sequence, 0);
}

#[cfg(unix)]
#[test]
fn session_journal_is_private_to_the_current_user() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("private.jsonl");
    let _store = SessionStore::open(&path).unwrap();
    let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
#[test]
fn symlinked_session_is_rejected_without_touching_target() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let target = dir.path().join("outside.jsonl");
    let link = dir.path().join("session.jsonl");
    fs::write(&target, b"keep this file\n").unwrap();
    let mut permissions = fs::metadata(&target).unwrap().permissions();
    permissions.set_mode(0o644);
    fs::set_permissions(&target, permissions).unwrap();
    symlink(&target, &link).unwrap();

    let error = SessionStore::open(&link).unwrap_err();
    assert!(matches!(error, SessionError::Symlink(path) if path == link));
    assert_eq!(fs::read(&target).unwrap(), b"keep this file\n");
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o644
    );
}

#[test]
fn fork_preserves_events_with_a_new_session_identity() {
    let dir = tempdir().unwrap();
    let source_path = dir.path().join("source.jsonl");
    let fork_path = dir.path().join("fork.jsonl");
    let mut source = SessionStore::open(&source_path).unwrap();
    source
        .append_event(serde_json::json!({"type":"checkpoint","value":1}))
        .unwrap();
    let fork = source.fork_to(&fork_path).unwrap();
    assert_ne!(source.session_id(), fork.session_id());
    assert_eq!(fork.events(), source.events());
    assert!(source_path.exists());
}

#[test]
fn session_list_import_and_explicit_gc_are_safe() {
    use zenpi::session::{
        GarbageCollectionPolicy, garbage_collect_sessions, import_session, list_sessions,
    };

    let dir = tempdir().unwrap();
    let sessions = dir.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let source = sessions.join("source.jsonl");
    let export = dir.path().join("imported.jsonl");
    let store = SessionStore::open(&source).unwrap();
    let imported = import_session(&source, &export).unwrap();
    assert_eq!(store.session_id(), imported.session_id());
    assert_eq!(list_sessions(&sessions).unwrap().len(), 1);
    assert!(
        garbage_collect_sessions(
            &sessions,
            GarbageCollectionPolicy {
                retain_newest: 0,
                older_than_ms: 0,
            },
            u64::MAX,
        )
        .is_err()
    );
    let removed = garbage_collect_sessions(
        &sessions,
        GarbageCollectionPolicy {
            retain_newest: 0,
            older_than_ms: 1,
        },
        u64::MAX,
    )
    .unwrap();
    assert_eq!(removed, vec![source]);
    assert!(export.exists());
}

#[test]
fn session_gc_skips_domain_store_and_non_owned_links() {
    use zenpi::session::{GarbageCollectionPolicy, garbage_collect_sessions};

    let dir = tempdir().unwrap();
    let sessions = dir.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let session_path = sessions.join("old.jsonl");
    SessionStore::open(&session_path).unwrap();
    let domain_path = sessions.join(zenpi::domain_store::DOMAIN_STORE_FILE_NAME);
    zenpi::domain_store::DomainStore::open(&domain_path).unwrap();
    let domain_before = fs::read(&domain_path).unwrap();

    #[cfg(unix)]
    let (link_path, external_path) = {
        let external_path = dir.path().join("external.jsonl");
        fs::write(&external_path, b"must remain untouched\n").unwrap();
        let link_path = sessions.join("alias.jsonl");
        std::os::unix::fs::symlink(&external_path, &link_path).unwrap();
        (Some(link_path), Some(external_path))
    };
    #[cfg(not(unix))]
    let external_path: Option<std::path::PathBuf> = None;

    #[cfg(unix)]
    {
        let result = garbage_collect_sessions(
            &sessions,
            GarbageCollectionPolicy {
                retain_newest: 0,
                older_than_ms: 1,
            },
            u64::MAX,
        );
        assert!(matches!(
            result,
            Err(zenpi::session::SessionError::Symlink(_))
        ));
        assert!(session_path.exists());
        assert_eq!(fs::read(&domain_path).unwrap(), domain_before);
    }

    #[cfg(unix)]
    fs::remove_file(link_path.as_ref().unwrap()).unwrap();

    let removed = garbage_collect_sessions(
        &sessions,
        GarbageCollectionPolicy {
            retain_newest: 0,
            older_than_ms: 1,
        },
        u64::MAX,
    )
    .unwrap();
    assert_eq!(removed, vec![session_path]);
    assert_eq!(fs::read(&domain_path).unwrap(), domain_before);
    assert!(zenpi::domain_store::DomainStore::open_existing(&domain_path).is_ok());
    if let Some(external_path) = external_path {
        assert_eq!(fs::read(external_path).unwrap(), b"must remain untouched\n");
    }
}

#[test]
fn session_gc_only_removes_clean_owned_journals_and_reports_skips() {
    use zenpi::session::GarbageCollectionPolicy;
    use zenpi::session::garbage_collect_sessions_with_active;

    let dir = tempdir().unwrap();
    let active_dir = tempdir().unwrap();
    let active_path = active_dir.path().join("active.jsonl");
    let _active = SessionStore::open(&active_path).unwrap();
    let owned_path = dir.path().join("owned.jsonl");
    SessionStore::open(&owned_path).unwrap();
    let foreign_path = dir.path().join("foreign.jsonl");
    fs::write(&foreign_path, b"{\"not\":\"a zenpi session\"}\n").unwrap();
    let domain_path = dir.path().join(zenpi::domain_store::DOMAIN_STORE_FILE_NAME);
    zenpi::domain_store::DomainStore::open(&domain_path).unwrap();

    let report = garbage_collect_sessions_with_active(
        dir.path(),
        GarbageCollectionPolicy {
            retain_newest: 0,
            older_than_ms: 1,
        },
        Some(&active_path),
        u64::MAX,
    )
    .unwrap();
    assert_eq!(report.removed, vec![owned_path.clone()]);
    assert_eq!(report.removed.len(), 1);
    assert!(report.inspected >= 1);
    assert!(report.skipped_unowned >= 1);
    assert!(!owned_path.exists());
    assert!(foreign_path.exists());
    assert!(domain_path.exists());
}

#[test]
fn session_gc_refuses_the_active_journal_before_any_removal() {
    use zenpi::session::{GarbageCollectionPolicy, garbage_collect_sessions_with_active};

    let dir = tempdir().unwrap();
    let active_path = dir.path().join("active.jsonl");
    SessionStore::open(&active_path).unwrap();
    let other_path = dir.path().join("other.jsonl");
    SessionStore::open(&other_path).unwrap();
    let error = garbage_collect_sessions_with_active(
        dir.path(),
        GarbageCollectionPolicy {
            retain_newest: 0,
            older_than_ms: 1,
        },
        Some(&active_path),
        u64::MAX,
    )
    .unwrap_err();
    assert!(error.to_string().contains("active session"));
    assert!(active_path.exists());
    assert!(other_path.exists());
}

#[test]
fn session_listing_skips_sibling_domain_store_without_mutating_it() {
    let dir = tempdir().unwrap();
    let sessions = dir.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let session_path = sessions.join("conversation.jsonl");
    let domain_path = sessions.join(zenpi::domain_store::DOMAIN_STORE_FILE_NAME);
    SessionStore::open(&session_path).unwrap();
    zenpi::domain_store::DomainStore::open(&domain_path).unwrap();
    let before = fs::read(&domain_path).unwrap();

    let listed = zenpi::session::list_sessions(&sessions).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].path, session_path.display().to_string());
    assert_eq!(fs::read(&domain_path).unwrap(), before);
    assert!(zenpi::domain_store::DomainStore::open_existing(&domain_path).is_ok());
}

#[test]
fn session_listing_includes_legacy_default_journal_without_moving_it() {
    use zenpi::session::list_sessions_with_fallback;

    let dir = tempdir().unwrap();
    let sessions = dir.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let fallback = dir.path().join("session.jsonl");
    let store = SessionStore::open(&fallback).unwrap();
    let before = fs::read(&fallback).unwrap();

    let listed = list_sessions_with_fallback(&sessions, &fallback).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session_id, store.session_id());
    assert_eq!(fs::read(&fallback).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn session_listing_rejects_a_symlinked_directory() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = dir.path().join("outside");
    let alias = dir.path().join("sessions");
    fs::create_dir_all(&outside).unwrap();
    let session_path = outside.join("external.jsonl");
    SessionStore::open(&session_path).unwrap();
    symlink(&outside, &alias).unwrap();

    let error = zenpi::session::list_sessions(&alias).unwrap_err();
    assert!(matches!(error, SessionError::Symlink(path) if path == alias));
    assert!(session_path.is_file());
}
