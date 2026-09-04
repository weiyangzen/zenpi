use std::io::Cursor;

use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use zenpi::{
    core::Agent,
    domain_store::{DomainStore, StoreChange, path_for_session},
    domains::Learn,
    headless::run_headless,
    session::SessionStore,
    slash::{SlashCommand, parse},
    tui::{MessageRole, SlashDispatchAction, TuiState, dispatch_slash_command},
};

fn records(bytes: &[u8]) -> Vec<Value> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn workspace_fixture(stem: &str, contents: &[u8]) -> (std::path::PathBuf, String) {
    let root = std::env::current_dir().unwrap();
    let directory = root
        .join("target")
        .join(format!("zenpi-learn-owner-{stem}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("evidence.md");
    std::fs::write(&path, contents).unwrap();
    let relative = path
        .strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    (directory, relative)
}

fn seed(session_path: &std::path::Path) {
    let mut store = DomainStore::open(path_for_session(session_path)).unwrap();
    store
        .put_learn(Learn::new("learn-1", "src", "Docs/learn", Vec::new()).unwrap())
        .unwrap();
}

#[test]
fn headless_learn_evidence_is_bounded_idempotent_and_restart_safe() {
    let directory = tempdir().unwrap();
    let session_path = directory.path().join("session.jsonl");
    seed(&session_path);
    let (evidence_dir, evidence_ref) = workspace_fixture("headless", b"# receipt\n");
    let expected_hash = format!("{:x}", Sha256::digest(b"# receipt\n"));

    let before_invalid = std::fs::read(path_for_session(&session_path)).unwrap();
    let input = [
        serde_json::json!({
            "type": "command",
            "id": "add-1",
            "text": format!("/learn evidence learn-1 {evidence_ref}"),
        }),
        serde_json::json!({
            "type": "command",
            "id": "add-2",
            "text": format!("/learn evidence learn-1 {evidence_ref}"),
        }),
        serde_json::json!({
            "type": "command",
            "id": "resume",
            "text": "/learn resume learn-1",
        }),
        serde_json::json!({
            "type": "command",
            "id": "missing",
            "text": format!("/learn evidence absent {evidence_ref}"),
        }),
        serde_json::json!({
            "type": "command",
            "id": "escape",
            "text": "/learn evidence learn-1 ../outside",
        }),
        serde_json::json!({"type": "shutdown", "id": "stop"}),
    ]
    .into_iter()
    .map(|value| serde_json::to_string(&value).unwrap())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.into_bytes()), &mut output).unwrap();
    let responses = records(&output);

    let first = responses
        .iter()
        .find(|record| record["id"] == "add-1")
        .unwrap();
    assert_eq!(first["success"], true, "{first}");
    assert_eq!(first["data"]["action"], "evidence");
    assert_eq!(first["data"]["change"], "updated");
    assert_eq!(first["data"]["evidence"]["reference"], evidence_ref);
    assert_eq!(first["data"]["evidence"]["sha256"], expected_hash);

    let retry = responses
        .iter()
        .find(|record| record["id"] == "add-2")
        .unwrap();
    assert_eq!(retry["success"], true, "{retry}");
    assert_eq!(retry["data"]["change"], "unchanged");
    assert_eq!(
        retry["data"]["learn"]["evidence"].as_array().unwrap().len(),
        1
    );

    let resumed = responses
        .iter()
        .find(|record| record["id"] == "resume")
        .unwrap();
    assert_eq!(resumed["success"], true, "{resumed}");
    assert_eq!(resumed["data"]["checkpoint"]["validated"], true);
    assert_eq!(
        resumed["data"]["checkpoint"]["execution_state"],
        "untracked"
    );
    assert_eq!(resumed["data"]["checkpoint"]["zenpi_started"], false);

    let missing = responses
        .iter()
        .find(|record| record["id"] == "missing")
        .unwrap();
    assert_eq!(missing["success"], false);
    assert_eq!(missing["code"], "learn_not_found");
    let escape = responses
        .iter()
        .find(|record| record["id"] == "escape")
        .unwrap();
    assert_eq!(escape["success"], false);
    assert_eq!(escape["code"], "learn_evidence_path_denied");

    let store_path = path_for_session(&session_path);
    let persisted = DomainStore::open(&store_path).unwrap();
    assert_eq!(
        persisted.learn("learn-1").unwrap().evidence,
        vec![evidence_ref]
    );
    assert_ne!(std::fs::read(&store_path).unwrap(), before_invalid);

    // A fresh process can inspect the same durable Learn checkpoint without a
    // provider call, and an invalid path/ID did not append a record.
    drop(agent);
    let mut reopened =
        Agent::with_echo(SessionStore::open_existing_writable(&session_path).unwrap());
    let mut restart_output = Vec::new();
    run_headless(
        &mut reopened,
        Cursor::new(
            b"{\"type\":\"command\",\"id\":\"restart\",\"text\":\"/learn resume learn-1\"}\n{\"type\":\"shutdown\",\"id\":\"stop-2\"}\n".as_slice(),
        ),
        &mut restart_output,
    )
    .unwrap();
    let restart = records(&restart_output)
        .into_iter()
        .find(|record| record["id"] == "restart")
        .unwrap();
    assert_eq!(restart["success"], true, "{restart}");
    assert_eq!(
        restart["data"]["learn"]["evidence"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let _ = std::fs::remove_dir_all(evidence_dir);
}

#[test]
fn tui_learn_evidence_and_resume_use_the_same_owner() {
    let directory = tempdir().unwrap();
    let session_path = directory.path().join("session.jsonl");
    seed(&session_path);
    let (evidence_dir, evidence_ref) = workspace_fixture("tui", b"evidence\n");
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let mut state = TuiState::default();

    let evidence = parse(&format!("/learn evidence learn-1 {evidence_ref}"))
        .unwrap()
        .unwrap();
    assert_eq!(
        dispatch_slash_command(evidence, &mut state, Some(&mut agent)),
        SlashDispatchAction::Continue
    );
    let resume = parse("/learn resume learn-1").unwrap().unwrap();
    assert_eq!(
        dispatch_slash_command(resume, &mut state, Some(&mut agent)),
        SlashDispatchAction::Continue
    );
    assert!(state.messages().any(|message| {
        message.role == MessageRole::System && message.text.contains("learn evidence added")
    }));
    assert!(state.messages().any(|message| {
        message.role == MessageRole::System && message.text.contains("learn resume checkpoint")
    }));
    assert_eq!(
        DomainStore::open(path_for_session(&session_path))
            .unwrap()
            .learn("learn-1")
            .unwrap()
            .evidence
            .len(),
        1
    );

    let _ = std::fs::remove_dir_all(evidence_dir);
}

#[test]
fn learn_parser_rejects_missing_or_extra_owner_arguments() {
    assert!(parse("/learn evidence learn-1").is_err());
    assert!(parse("/learn evidence learn-1 ref extra").is_err());
    assert!(parse("/learn resume").is_err());
    assert_eq!(
        parse("/learn resume learn-1").unwrap(),
        Some(SlashCommand::LearnResume {
            id: "learn-1".into()
        })
    );
    assert_eq!(
        parse("/learn evidence learn-1 docs/receipt.md").unwrap(),
        Some(SlashCommand::LearnEvidence {
            id: "learn-1".into(),
            reference: "docs/receipt.md".into(),
        })
    );
}

#[test]
fn domain_store_evidence_retry_at_limit_is_idempotent() {
    let directory = tempdir().unwrap();
    let session_path = directory.path().join("session.jsonl");
    let mut store = DomainStore::open(path_for_session(&session_path)).unwrap();
    let mut learn = Learn::new("learn-limit", "src", "dst", Vec::new()).unwrap();
    for index in 0..zenpi::domains::MAX_LEARN_EVIDENCE {
        learn.add_evidence(format!("ref-{index}")).unwrap();
    }
    store.put_learn(learn).unwrap();
    assert_eq!(
        store.add_learn_evidence("learn-limit", "ref-0").unwrap(),
        StoreChange::Unchanged
    );
    assert!(store.add_learn_evidence("learn-limit", "new-ref").is_err());
}
