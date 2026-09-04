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
fn keyed_runtime_intent_replays_after_restart_without_a_second_append() {
    use zenpi::runtime_intent::{RuntimeIntentSource, runtime_intent_value_with_source};

    let directory = tempdir().unwrap();
    let path = directory.path().join("idempotent.jsonl");
    let args = ["audit".into(), "owners".into()];
    let fingerprint = "a".repeat(64);
    let source = RuntimeIntentSource {
        request_id: "wire-1",
        fingerprint: &fingerprint,
    };
    let mut first = Agent::with_echo(SessionStore::open(&path).unwrap());
    let created = runtime_intent_value_with_source(
        &mut first,
        RuntimeIntentKind::Compete,
        &args,
        Some(source),
    )
    .unwrap();
    assert_eq!(created["created"], true);
    let sequence_after_create = first.session().next_sequence();
    drop(first);

    let mut reopened = Agent::with_echo(SessionStore::open_existing_writable(&path).unwrap());
    assert_eq!(
        reopened.session().runtime_intents()[0]
            .request_fingerprint
            .as_deref(),
        Some(fingerprint.as_str())
    );
    let replayed = runtime_intent_value_with_source(
        &mut reopened,
        RuntimeIntentKind::Compete,
        &args,
        Some(source),
    )
    .unwrap();
    assert_eq!(replayed["idempotent_replay"], true);
    assert_eq!(replayed["created"], false);
    assert_eq!(reopened.session().runtime_intents().len(), 1);
    assert_eq!(reopened.session().next_sequence(), sequence_after_create);
}

#[test]
fn keyed_runtime_intent_conflict_does_not_mutate_journal() {
    use zenpi::runtime_intent::{
        RuntimeIntentError, RuntimeIntentSource, runtime_intent_value_with_source,
    };

    let directory = tempdir().unwrap();
    let path = directory.path().join("conflict.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let first_fingerprint = "b".repeat(64);
    let conflicting_fingerprint = "c".repeat(64);
    runtime_intent_value_with_source(
        &mut agent,
        RuntimeIntentKind::Loop,
        &["repair".into()],
        Some(RuntimeIntentSource {
            request_id: "wire-2",
            fingerprint: &first_fingerprint,
        }),
    )
    .unwrap();
    let before = std::fs::read(&path).unwrap();
    let error = runtime_intent_value_with_source(
        &mut agent,
        RuntimeIntentKind::Loop,
        &["different".into()],
        Some(RuntimeIntentSource {
            request_id: "wire-2",
            fingerprint: &conflicting_fingerprint,
        }),
    )
    .unwrap_err();
    assert!(matches!(error, RuntimeIntentError::RequestConflict(_)));
    assert_eq!(std::fs::read(path).unwrap(), before);
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

#[test]
fn headless_runtime_intent_retry_is_deduplicated_across_restart() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("durable-idempotency.jsonl");
    let request = concat!(
        "{\"schema_version\":2,\"type\":\"command\",\"id\":\"intent-request-1\",",
        "\"text\":\"/compete submit --parent GOAL-1 audit owners\"}\n",
        "{\"schema_version\":2,\"type\":\"shutdown\",\"id\":\"first-stop\"}\n",
    );

    let mut first = Agent::with_echo(SessionStore::open(&path).unwrap());
    let mut first_output = Vec::new();
    run_headless(
        &mut first,
        Cursor::new(request.as_bytes()),
        &mut first_output,
    )
    .unwrap();
    let first_response = json_lines(&first_output)
        .into_iter()
        .find(|record| record["id"] == "intent-request-1")
        .unwrap();
    assert_eq!(first_response["data"]["created"], true);
    assert_eq!(first_response["data"]["idempotent_replay"], false);
    let first_next_sequence = first.session().next_sequence();
    assert_eq!(first.session().runtime_intents().len(), 1);

    let retry = concat!(
        "{\"schema_version\":1,\"type\":\"slash\",\"id\":\"intent-request-1\",",
        "\"message\":\"/compete submit --parent GOAL-1 audit owners\"}\n",
        "{\"schema_version\":2,\"type\":\"shutdown\",\"id\":\"second-stop\"}\n",
    );
    let mut second = Agent::with_echo(SessionStore::open(&path).unwrap());
    let mut second_output = Vec::new();
    run_headless(
        &mut second,
        Cursor::new(retry.as_bytes()),
        &mut second_output,
    )
    .unwrap();
    let retry_response = json_lines(&second_output)
        .into_iter()
        .find(|record| record["id"] == "intent-request-1")
        .unwrap();
    assert_eq!(retry_response["success"], true);
    assert_eq!(retry_response["data"]["created"], false);
    assert_eq!(retry_response["data"]["idempotent_replay"], true);
    assert_eq!(second.session().runtime_intents().len(), 1);
    assert_eq!(second.session().next_sequence(), first_next_sequence);
}

#[test]
fn headless_runtime_intent_request_id_conflict_is_durable_and_write_free() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("durable-conflict.jsonl");
    let first_input = concat!(
        "{\"schema_version\":2,\"type\":\"command\",\"id\":\"shared-intent\",",
        "\"text\":\"/loop start repair ITEM-1\"}\n",
        "{\"schema_version\":2,\"type\":\"shutdown\",\"id\":\"first-stop\"}\n",
    );
    let mut first = Agent::with_echo(SessionStore::open(&path).unwrap());
    run_headless(&mut first, Cursor::new(first_input.as_bytes()), Vec::new()).unwrap();
    let before = std::fs::read(&path).unwrap();

    let conflict_input = concat!(
        "{\"schema_version\":2,\"type\":\"command\",\"id\":\"shared-intent\",",
        "\"text\":\"/loop start repair ITEM-2\"}\n",
        "{\"schema_version\":2,\"type\":\"shutdown\",\"id\":\"second-stop\"}\n",
    );
    let mut second = Agent::with_echo(SessionStore::open(&path).unwrap());
    let mut output = Vec::new();
    run_headless(
        &mut second,
        Cursor::new(conflict_input.as_bytes()),
        &mut output,
    )
    .unwrap();
    let response = json_lines(&output)
        .into_iter()
        .find(|record| record["id"] == "shared-intent")
        .unwrap();
    assert_eq!(response["success"], false);
    assert_eq!(response["code"], "runtime_intent_conflict");
    assert_eq!(second.session().runtime_intents().len(), 1);
    assert_eq!(std::fs::read(path).unwrap(), before);
}
