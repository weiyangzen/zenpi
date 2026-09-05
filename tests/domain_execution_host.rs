use std::io::Cursor;

use serde_json::Value;
use tempfile::tempdir;
use zenpi::{
    b3::ResourceBudget,
    core::Agent,
    domain_execution::{ExecutionReceipt, ExecutionStatus, ExecutionStore, deterministic_cost},
    domain_store::{DomainStore, path_for_session},
    domains::{Blueprint, BlueprintItem, Goal},
    headless::run_headless,
    session::SessionStore,
    slash::SlashCommand,
    tui::{MessageRole, SlashDispatchAction, TuiState, dispatch_slash_command},
};

fn seed(session: &std::path::Path) {
    let blueprint = Blueprint::new(
        "host-plan",
        "1",
        vec![
            BlueprintItem::new("build", 10),
            BlueprintItem::new("verify", 20).with_dependencies(["build"]),
        ],
    )
    .unwrap();
    let goal = Goal::new(
        "host-goal",
        &blueprint,
        ResourceBudget {
            tokens: 100,
            wall_clock_ms: 10,
            attempts: 2,
            disk_bytes: 2_000,
        },
        None,
    )
    .unwrap();
    let mut store = DomainStore::open(path_for_session(session)).unwrap();
    store.put_blueprint(blueprint).unwrap();
    store.put_goal(goal).unwrap();
}

fn records(bytes: &[u8]) -> Vec<Value> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn headless_blueprint_run_executes_dependency_order_and_finishes_goal() {
    let dir = tempdir().unwrap();
    let session = dir.path().join("session.jsonl");
    seed(&session);
    let mut agent = Agent::with_echo(SessionStore::open(&session).unwrap());
    let input = concat!(
        "{\"type\":\"command\",\"id\":\"first\",\"text\":\"/blueprint run host-plan@1\"}\n",
        "{\"type\":\"command\",\"id\":\"second\",\"text\":\"/blueprint run host-plan@1\"}\n",
        "{\"type\":\"shutdown\",\"id\":\"shutdown\"}\n",
    );
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.as_bytes()), &mut output).unwrap();
    let values = records(&output);

    let first = values.iter().find(|value| value["id"] == "first").unwrap();
    assert_eq!(first["success"], true);
    assert_eq!(first["data"]["action"], "run");
    assert_eq!(first["data"]["status"], "succeeded");
    assert_eq!(first["data"]["receipt"]["item_id"], "build");
    assert_eq!(first["data"]["goal_status"], "running");

    let second = values.iter().find(|value| value["id"] == "second").unwrap();
    assert_eq!(second["success"], true);
    assert_eq!(second["data"]["receipt"]["item_id"], "verify");
    assert_eq!(second["data"]["goal_status"], "done");
    assert_eq!(second["data"]["receipt_count"], 2);

    let execution = zenpi::domain_execution::ExecutionStore::open(
        zenpi::domain_execution::path_for_session(&session),
    )
    .unwrap();
    assert_eq!(execution.receipts().len(), 2);
    assert!(
        execution
            .receipts()
            .iter()
            .all(|receipt| receipt.status == zenpi::domain_execution::ExecutionStatus::Succeeded)
    );
    let persisted = DomainStore::open(path_for_session(&session)).unwrap();
    assert_eq!(
        persisted.goal("host-goal").unwrap().status,
        zenpi::domains::GoalStatus::Done
    );
    assert!(
        agent.history().is_empty(),
        "slash owner must not call the provider"
    );
}

#[test]
fn blueprint_run_does_not_finish_goal_from_an_older_success_when_latest_attempt_failed() {
    let dir = tempdir().unwrap();
    let session = dir.path().join("session.jsonl");
    let blueprint = Blueprint::new(
        "latest-plan",
        "1",
        vec![
            BlueprintItem::new("first", 10).with_dependencies(["second"]),
            BlueprintItem::new("second", 20),
        ],
    )
    .unwrap();
    let goal = Goal::new(
        "latest-goal",
        &blueprint,
        ResourceBudget {
            tokens: 200,
            wall_clock_ms: 100,
            attempts: 4,
            disk_bytes: 10_000,
        },
        None,
    )
    .unwrap();
    let mut domains = DomainStore::open(path_for_session(&session)).unwrap();
    domains.put_blueprint(blueprint.clone()).unwrap();
    domains.put_goal(goal.clone()).unwrap();

    // Item `first` has a historical success, but its newer attempt failed.
    // The dependency keeps it out of the next selection so `second` can run;
    // completion must inspect `first`'s latest receipt, not any old success.
    let first = &blueprint.items[0];
    let mut execution =
        ExecutionStore::open(zenpi::domain_execution::path_for_session(&session)).unwrap();
    for (execution_id, attempt, status, error) in [
        ("first-success", 1, ExecutionStatus::Succeeded, None),
        (
            "first-failed",
            2,
            ExecutionStatus::Failed,
            Some("deterministic failure"),
        ),
    ] {
        execution
            .upsert_receipt(ExecutionReceipt {
                execution_id: execution_id.into(),
                goal_id: goal.id.clone(),
                blueprint_id: blueprint.id.clone(),
                blueprint_version: blueprint.version.clone(),
                blueprint_digest: blueprint.digest.clone(),
                item_id: first.id.clone(),
                attempt,
                status,
                cost: deterministic_cost(first),
                evidence: "deterministic_local_evidence seeded".into(),
                error: error.map(str::to_owned),
            })
            .unwrap();
    }

    let mut agent = Agent::with_echo(SessionStore::open(&session).unwrap());
    let input = concat!(
        "{\"type\":\"command\",\"id\":\"run\",\"text\":\"/blueprint run latest-plan@1\"}\n",
        "{\"type\":\"shutdown\",\"id\":\"shutdown\"}\n",
    );
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.as_bytes()), &mut output).unwrap();
    let values = records(&output);
    let run = values.iter().find(|value| value["id"] == "run").unwrap();
    assert_eq!(run["success"], true);
    assert_eq!(run["data"]["receipt"]["item_id"], "second");
    assert_eq!(run["data"]["goal_status"], "running");

    let persisted = DomainStore::open(path_for_session(&session)).unwrap();
    assert_eq!(
        persisted.goal("latest-goal").unwrap().status,
        zenpi::domains::GoalStatus::Running
    );
}

#[test]
fn tui_blueprint_run_uses_the_same_owner_and_reports_errors_without_model_turns() {
    let dir = tempdir().unwrap();
    let session = dir.path().join("session.jsonl");
    seed(&session);
    let mut agent = Agent::with_echo(SessionStore::open(&session).unwrap());
    let mut state = TuiState::default();

    assert_eq!(
        dispatch_slash_command(
            SlashCommand::Blueprint {
                action: zenpi::slash::BlueprintAction::Run {
                    target: "host-plan@1".into(),
                },
            },
            &mut state,
            Some(&mut agent),
        ),
        SlashDispatchAction::Continue
    );
    assert!(state.messages().any(|message| {
        message.role == MessageRole::System
            && message.text.contains("blueprint run:")
            && message.text.contains("build")
            && message.text.contains("running")
    }));
    assert!(agent.history().is_empty());

    dispatch_slash_command(
        SlashCommand::Blueprint {
            action: zenpi::slash::BlueprintAction::Run {
                target: "missing@1".into(),
            },
        },
        &mut state,
        Some(&mut agent),
    );
    assert!(state.messages().any(|message| {
        message.role == MessageRole::Error && message.text.contains("blueprint run failed")
    }));
}
