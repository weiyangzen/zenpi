use std::io::Cursor;

use serde_json::Value;
use tempfile::tempdir;
use zenpi::{
    b3::RuntimeIntentKind,
    core::Agent,
    headless::run_headless,
    session::SessionStore,
    slash::SlashCommand,
    tui::{MessageRole, SlashDispatchAction, TuiState, dispatch_slash_command},
};

fn json_lines(bytes: &[u8]) -> Vec<Value> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn headless_compete_and_loop_persist_inert_typed_handoffs() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("runtime.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let input = concat!(
        "{\"type\":\"command\",\"id\":\"c\",\"text\":\"/compete submit --parent GOAL-1 --tokens 1200 audit owners\"}\n",
        "{\"type\":\"command\",\"id\":\"l\",\"text\":\"/loop start --parent GOAL-1 --parent-lease LEASE-1 --nested-run RUN-1 --attempts 2 repair ITEM-2\"}\n",
        "{\"type\":\"command\",\"id\":\"s\",\"text\":\"/loop status\"}\n",
        "{\"type\":\"shutdown\",\"id\":\"q\"}\n",
    );
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.as_bytes()), &mut output).unwrap();
    let records = json_lines(&output);

    for id in ["c", "l", "s"] {
        let response = records.iter().find(|record| record["id"] == id).unwrap();
        assert_eq!(response["success"], true, "{response}");
        assert_eq!(response["data"]["delivery"], "journal_only");
        assert_eq!(response["data"]["zenpi_started"], false);
        assert_eq!(response["data"]["execution_state"], "untracked");
    }
    assert_eq!(
        records.iter().find(|value| value["id"] == "s").unwrap()["data"]["stored_count"],
        1
    );
    assert!(
        agent.history().is_empty(),
        "runtime slash input reached provider history"
    );
    assert_eq!(agent.session().runtime_intents().len(), 2);
    assert_eq!(
        agent.session().runtime_intents()[0].kind,
        RuntimeIntentKind::Compete
    );
    assert_eq!(agent.session().runtime_intents()[0].parent_ref, "GOAL-1");
    assert_eq!(
        agent.session().runtime_intents()[0].envelope.limit.tokens,
        1200
    );
    assert_eq!(
        agent.session().runtime_intents()[1].kind,
        RuntimeIntentKind::Loop
    );
    assert_eq!(
        agent.session().runtime_intents()[1]
            .parent_lease
            .as_ref()
            .unwrap()
            .parent_lease_id,
        "LEASE-1"
    );

    let reopened = SessionStore::open_existing(&path).unwrap();
    assert_eq!(
        reopened.runtime_intents(),
        agent.session().runtime_intents()
    );
    let journal = std::fs::read_to_string(path).unwrap();
    assert!(!journal.contains("operation_started"));
    assert!(!journal.contains("turn\""));
}

#[test]
fn tui_runtime_owner_uses_the_same_durable_adapter() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("runtime.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let mut state = TuiState::default();

    assert_eq!(
        dispatch_slash_command(
            SlashCommand::Compete {
                args: vec!["coverage".into(), "owners".into()],
            },
            &mut state,
            Some(&mut agent),
        ),
        SlashDispatchAction::Continue
    );
    assert_eq!(agent.session().runtime_intents().len(), 1);
    assert!(state.messages().any(|message| {
        message.role == MessageRole::System
            && message.text.contains("compete runtime intent")
            && message.text.contains("delivery=journal_only")
            && message.text.contains("zenpi_started=false")
    }));

    dispatch_slash_command(
        SlashCommand::Compete {
            args: vec!["status".into()],
        },
        &mut state,
        Some(&mut agent),
    );
    assert_eq!(agent.session().runtime_intents().len(), 1);
    assert!(state.messages().any(|message| {
        message.role == MessageRole::System
            && message.text.contains("compete runtime intent")
            && message.text.contains("intent_id=compete-intent-")
    }));
}

#[test]
fn invalid_runtime_intent_is_rejected_without_journal_mutation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("runtime.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let before = std::fs::read_to_string(&path).unwrap();
    let input = concat!(
        "{\"type\":\"command\",\"id\":\"missing\",\"text\":\"/loop start --parent GOAL-1\"}\n",
        "{\"type\":\"command\",\"id\":\"secret\",\"text\":\"/compete audit api_key=hidden\"}\n",
        "{\"type\":\"command\",\"id\":\"lease\",\"text\":\"/loop repair --parent-lease LEASE-1\"}\n",
        "{\"type\":\"shutdown\",\"id\":\"q\"}\n",
    );
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.as_bytes()), &mut output).unwrap();
    let records = json_lines(&output);
    for id in ["missing", "secret", "lease"] {
        let response = records.iter().find(|record| record["id"] == id).unwrap();
        assert_eq!(response["success"], false);
        assert_eq!(response["code"], "runtime_intent_error");
    }
    assert!(agent.session().runtime_intents().is_empty());
    // Shutdown may only append skill close events when configured; the default
    // echo agent has none, so every rejected runtime command is write-free.
    assert_eq!(std::fs::read_to_string(path).unwrap(), before);
}

#[test]
fn session_fork_does_not_duplicate_pending_runtime_work() {
    let directory = tempdir().unwrap();
    let source_path = directory.path().join("source.jsonl");
    let destination = directory.path().join("fork.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&source_path).unwrap());
    zenpi::runtime_intent::runtime_intent_value(
        &mut agent,
        RuntimeIntentKind::Loop,
        &["repair".into(), "ITEM-1".into()],
    )
    .unwrap();

    let fork = agent.session().fork_to(&destination).unwrap();
    assert_eq!(fork.summary().runtime_intent_count, 0);
    assert!(fork.runtime_intents().is_empty());
    assert_ne!(fork.session_id(), agent.session().session_id());
}

#[test]
fn excessive_runtime_budget_is_rejected_before_persistence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("budget.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let before = std::fs::read_to_string(&path).unwrap();
    let error = zenpi::runtime_intent::runtime_intent_value(
        &mut agent,
        RuntimeIntentKind::Compete,
        &[
            "--tokens".into(),
            u64::MAX.to_string(),
            "risk-analysis".into(),
        ],
    )
    .unwrap_err();
    assert!(error.to_string().contains("invalid numeric value"));
    assert!(agent.session().runtime_intents().is_empty());
    assert_eq!(std::fs::read_to_string(path).unwrap(), before);
}

#[test]
fn runtime_task_can_preserve_double_dash_arguments_after_terminator() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("flags.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(path).unwrap());
    zenpi::runtime_intent::runtime_intent_value(
        &mut agent,
        RuntimeIntentKind::Compete,
        &[
            "submit".into(),
            "--parent".into(),
            "GOAL-1".into(),
            "--".into(),
            "audit".into(),
            "cargo".into(),
            "--all-targets".into(),
        ],
    )
    .unwrap();
    assert_eq!(
        agent.session().runtime_intents()[0].args,
        ["audit", "cargo", "--all-targets"]
    );
}
