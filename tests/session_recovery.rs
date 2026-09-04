use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;
use zenpi::{
    core::{Turn, TurnRole},
    session::{InterruptedOperation, OperationKind, OperationOutcome, SessionError, SessionStore},
};

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
