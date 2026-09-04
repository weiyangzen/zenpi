use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;
use zenpi::{
    core::{Turn, TurnRole},
    session::SessionStore,
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
