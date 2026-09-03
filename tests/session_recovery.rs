use std::fs;
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
