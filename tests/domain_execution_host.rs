#[path = "support/sequential_host.rs"]
mod sequential_host;
use sequential_host::run_headless_sequential;

use serde_json::Value;
use tempfile::tempdir;
use zenpi::{
    b3::ResourceBudget,
    core::Agent,
    domain_execution::{ExecutionReceipt, ExecutionStatus, ExecutionStore, deterministic_cost},
    domain_store::{DomainStore, path_for_session},
    domains::{Blueprint, BlueprintItem, Goal},
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
fn headless_blueprint_run_records_dependency_order_without_faking_goal_completion() {
    let dir = tempdir().unwrap();
    let session = dir.path().join("session.jsonl");
    seed(&session);
    let agent = Agent::with_echo(SessionStore::open(&session).unwrap());
    let input = concat!(
        "{\"type\":\"command\",\"id\":\"first\",\"text\":\"/blueprint run host-plan@1\"}\n",
        "{\"type\":\"command\",\"id\":\"second\",\"text\":\"/blueprint run host-plan@1\"}\n",
        "{\"type\":\"shutdown\",\"id\":\"shutdown\"}\n",
    );
    let mut output = Vec::new();
    run_headless_sequential(agent, input.as_bytes(), &mut output).unwrap();
    let values = records(&output);

    let first = values.iter().find(|value| value["id"] == "first").unwrap();
    assert_eq!(first["success"], true);
    assert_eq!(first["data"]["action"], "run");
    assert_eq!(first["data"]["status"], "recorded");
    assert_eq!(first["data"]["execution_scope"], "control_plane_only");
    assert_eq!(first["data"]["external_work_executed"], false);
    assert_eq!(first["data"]["receipt"]["item_id"], "build");
    assert_eq!(first["data"]["goal_status"], "running");

    let second = values.iter().find(|value| value["id"] == "second").unwrap();
    assert_eq!(second["success"], true);
    assert_eq!(second["data"]["status"], "recorded");
    assert_eq!(second["data"]["receipt"]["item_id"], "verify");
    assert_eq!(second["data"]["goal_status"], "running");
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
        zenpi::domains::GoalStatus::Running
    );
    assert!(
        SessionStore::open_existing(&session)
            .unwrap()
            .turns()
            .is_empty(),
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
                external_work_executed: false,
                manifest_checksum: None,
                error: error.map(str::to_owned),
            })
            .unwrap();
    }

    let agent = Agent::with_echo(SessionStore::open(&session).unwrap());
    let input = concat!(
        "{\"type\":\"command\",\"id\":\"run\",\"text\":\"/blueprint run latest-plan@1\"}\n",
        "{\"type\":\"shutdown\",\"id\":\"shutdown\"}\n",
    );
    let mut output = Vec::new();
    run_headless_sequential(agent, input.as_bytes(), &mut output).unwrap();
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
        message.role == MessageRole::System && message.text.contains("blueprint run:")
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

#[test]
fn headless_handoff_queue_round_trips_immutable_claims_without_receipts() {
    let dir = tempdir().unwrap();
    let session = dir.path().join("handoff-session.jsonl");
    let blueprint = Blueprint::new(
        "handoff-plan",
        "1",
        vec![
            BlueprintItem::new("build", 10).with_task(zenpi::domains::BlueprintTask {
                instruction: "implement the bounded task".into(),
                acceptance_commands: vec!["self-test".into()],
            }),
        ],
    )
    .unwrap();
    let goal = Goal::new(
        "handoff-goal",
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
    let mut domains = DomainStore::open(path_for_session(&session)).unwrap();
    domains.put_blueprint(blueprint).unwrap();
    domains.put_goal(goal).unwrap();
    let agent = Agent::with_echo(SessionStore::open(&session).unwrap());
    let input = concat!(
        "{\"type\":\"command\",\"id\":\"queue\",\"text\":\"/blueprint handoff handoff-plan@1\"}\n",
        "{\"type\":\"command\",\"id\":\"list\",\"text\":\"/blueprint handoffs\"}\n",
        "{\"type\":\"shutdown\",\"id\":\"shutdown\"}\n",
    );
    let mut output = Vec::new();
    run_headless_sequential(agent, input.as_bytes(), &mut output).unwrap();
    let values = records(&output);
    let queued = values.iter().find(|value| value["id"] == "queue").unwrap();
    assert_eq!(queued["success"], true);
    let claim = queued["data"]["handoff"].clone();
    let listed = values.iter().find(|value| value["id"] == "list").unwrap();
    assert_eq!(listed["success"], true);
    assert_eq!(listed["data"]["handoffs"][0]["claim"], claim);
    assert_eq!(listed["data"]["handoffs"][0]["receipt_status"], "running");
    assert_eq!(listed["data"]["handoffs"][0]["manifest_imported"], false);
    assert_eq!(listed["data"]["delivery"], "read_only_claims");
    let execution =
        ExecutionStore::open(zenpi::domain_execution::path_for_session(&session)).unwrap();
    assert_eq!(execution.receipts().len(), 1);
    assert_eq!(execution.receipts()[0].status, ExecutionStatus::Running);
    assert!(!execution.receipts()[0].external_work_executed);
    let journal = SessionStore::open_existing(&session).unwrap();
    assert!(journal.events().iter().any(|event| {
        event["type"] == "blueprint_handoff_claimed"
            && event["claim_digest"] == claim["claim_digest"]
    }));
}

#[test]
fn goal_run_executes_task_and_acceptance_until_done() {
    let dir = tempdir().unwrap();
    let session = dir.path().join("goal-session.jsonl");
    let blueprint = Blueprint::new(
        "goal-plan",
        "1",
        vec![
            BlueprintItem::new("build", 10).with_task(zenpi::domains::BlueprintTask {
                instruction: "make the acceptance command pass".into(),
                acceptance_commands: vec!["true".into()],
            }),
        ],
    )
    .unwrap();
    let goal = Goal::new(
        "goal",
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
    let mut domains = DomainStore::open(path_for_session(&session)).unwrap();
    domains.put_blueprint(blueprint).unwrap();
    domains.put_goal(goal).unwrap();

    let mut agent = Agent::with_echo(SessionStore::open(&session).unwrap());
    let result = zenpi::headless::run_goal_steps_for_tui(&mut agent, "goal-plan@1", 1).unwrap();
    assert_eq!(result["status"], "complete");
    assert_eq!(result["goal_status"], "done");
    assert_eq!(result["completion_basis"], "local_acceptance_commands");
    assert_eq!(result["execution_scope"], "local_goal_runner");
    assert_eq!(result["task_executed"], true);
    assert!(
        result["receipt"]["evidence"]
            .as_str()
            .unwrap()
            .contains("local_task_executed=true")
    );
    assert_eq!(result["steps_recorded"], 1);
    assert_eq!(agent.history().len(), 2);

    let persisted = DomainStore::open(path_for_session(&session)).unwrap();
    assert_eq!(
        persisted.goal("goal").unwrap().status,
        zenpi::domains::GoalStatus::Done
    );
    let execution =
        ExecutionStore::open(zenpi::domain_execution::path_for_session(&session)).unwrap();
    assert_eq!(execution.receipts().len(), 1);
    assert_eq!(execution.receipts()[0].status, ExecutionStatus::Succeeded);
}

#[test]
fn goal_run_records_a_failed_acceptance_without_claiming_completion() {
    let dir = tempdir().unwrap();
    let session = dir.path().join("goal-failed-session.jsonl");
    let blueprint = Blueprint::new(
        "goal-failed-plan",
        "1",
        vec![
            BlueprintItem::new("build", 10).with_task(zenpi::domains::BlueprintTask {
                instruction: "this acceptance must fail".into(),
                acceptance_commands: vec!["false".into()],
            }),
        ],
    )
    .unwrap();
    let goal = Goal::new(
        "goal-failed",
        &blueprint,
        ResourceBudget {
            tokens: 100,
            wall_clock_ms: 10,
            attempts: 1,
            disk_bytes: 2_000,
        },
        None,
    )
    .unwrap();
    let mut domains = DomainStore::open(path_for_session(&session)).unwrap();
    domains.put_blueprint(blueprint).unwrap();
    domains.put_goal(goal).unwrap();

    let mut agent = Agent::with_echo(SessionStore::open(&session).unwrap());
    let result =
        zenpi::headless::run_goal_steps_for_tui(&mut agent, "goal-failed-plan@1", 1).unwrap();
    assert_eq!(result["status"], "blocked");
    assert_eq!(result["receipt"]["status"], "failed");
    assert!(
        result["receipt"]["error"]
            .as_str()
            .unwrap()
            .contains("false")
    );
    assert_eq!(result["goal_status"], "blocked");
    assert_eq!(result["completion_basis"], "attempts_exhausted");
    assert_eq!(result["goal_outcome"], "could_not_be_achieved");

    let persisted = DomainStore::open(path_for_session(&session)).unwrap();
    assert_eq!(
        persisted.goal("goal-failed").unwrap().status,
        zenpi::domains::GoalStatus::Blocked
    );
}

#[test]
fn goal_run_blocks_without_a_model_turn_when_attempts_are_exhausted() {
    let dir = tempdir().unwrap();
    let session = dir.path().join("goal-exhausted-session.jsonl");
    let blueprint = Blueprint::new(
        "goal-exhausted-plan",
        "1",
        vec![
            BlueprintItem::new("build", 10).with_task(zenpi::domains::BlueprintTask {
                instruction: "the prior attempt already failed".into(),
                acceptance_commands: vec!["true".into()],
            }),
        ],
    )
    .unwrap();
    let goal = Goal::new(
        "goal-exhausted",
        &blueprint,
        ResourceBudget {
            tokens: 100,
            wall_clock_ms: 10,
            attempts: 1,
            disk_bytes: 2_000,
        },
        None,
    )
    .unwrap();
    let mut domains = DomainStore::open(path_for_session(&session)).unwrap();
    domains.put_blueprint(blueprint.clone()).unwrap();
    domains.put_goal(goal.clone()).unwrap();
    let item = &blueprint.items[0];
    let mut execution =
        ExecutionStore::open(zenpi::domain_execution::path_for_session(&session)).unwrap();
    execution
        .upsert_receipt(ExecutionReceipt {
            execution_id: "exhausted-attempt".into(),
            goal_id: goal.id.clone(),
            blueprint_id: blueprint.id.clone(),
            blueprint_version: blueprint.version.clone(),
            blueprint_digest: blueprint.digest.clone(),
            item_id: item.id.clone(),
            attempt: 1,
            status: ExecutionStatus::Failed,
            cost: deterministic_cost(item),
            evidence: "local_goal_runner local_task_executed=true".into(),
            external_work_executed: false,
            manifest_checksum: None,
            error: Some("acceptance failed".into()),
        })
        .unwrap();

    let mut agent = Agent::with_echo(SessionStore::open(&session).unwrap());
    let result =
        zenpi::headless::run_goal_steps_for_tui(&mut agent, "goal-exhausted-plan@1", 1).unwrap();
    assert_eq!(result["status"], "blocked");
    assert_eq!(result["goal_status"], "blocked");
    assert_eq!(result["completion_basis"], "attempts_exhausted");
    assert_eq!(result["goal_outcome"], "could_not_be_achieved");
    assert_eq!(result["steps_recorded"], 0);
    assert!(agent.history().is_empty());

    let persisted = DomainStore::open(path_for_session(&session)).unwrap();
    assert_eq!(
        persisted.goal("goal-exhausted").unwrap().status,
        zenpi::domains::GoalStatus::Blocked
    );
}
