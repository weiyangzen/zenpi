use std::fs;

use tempfile::tempdir;
use zenpi::{
    b3::ResourceBudget,
    domain_execution::{
        BlueprintExecutor, ExecutionError, ExecutionStatus, ExecutionStore, RunOutcome,
        deterministic_cost,
    },
    domains::{Blueprint, BlueprintItem, Goal},
};

fn fixture() -> (Blueprint, Goal) {
    let blueprint = Blueprint::new(
        "execution-plan",
        "1",
        vec![
            BlueprintItem::new("first", 10),
            BlueprintItem::new("second", 20).with_dependencies(["first"]),
        ],
    )
    .unwrap();
    let goal = Goal::new(
        "execution-goal",
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
    (blueprint, goal)
}

#[test]
fn owner_selects_dependencies_and_persists_running_then_terminal_receipts() {
    let dir = tempdir().unwrap();
    let store = ExecutionStore::open(dir.path().join("execution.json")).unwrap();
    let (blueprint, goal) = fixture();
    let mut owner = BlueprintExecutor::new(store);

    let first = owner.run_next(&goal, &blueprint, || false).unwrap();
    let RunOutcome::Executed { receipt, .. } = first else {
        panic!("expected first item to execute");
    };
    assert_eq!(receipt.item_id, "first");
    assert_eq!(receipt.status, ExecutionStatus::Succeeded);
    assert_eq!(owner.store().receipts().len(), 1);
    assert_eq!(
        owner.store().receipts()[0].status,
        ExecutionStatus::Succeeded
    );

    let second = owner.run_next(&goal, &blueprint, || false).unwrap();
    let RunOutcome::Executed { receipt, .. } = second else {
        panic!("expected second item to execute");
    };
    assert_eq!(receipt.item_id, "second");
    assert_eq!(
        owner.run_next(&goal, &blueprint, || false).unwrap(),
        RunOutcome::Complete
    );

    // Reopening the same path is enough to prove the receipts survive a
    // process boundary; no in-memory state is used for dependency admission.
    let reopened = ExecutionStore::open(dir.path().join("execution.json")).unwrap();
    let mut restarted = BlueprintExecutor::new(reopened);
    assert_eq!(
        restarted.run_next(&goal, &blueprint, || false).unwrap(),
        RunOutcome::Complete
    );
}

#[test]
fn cancellation_before_admission_is_idempotent_and_does_not_consume_budget() {
    let dir = tempdir().unwrap();
    let store = ExecutionStore::open(dir.path().join("execution.json")).unwrap();
    let (blueprint, goal) = fixture();
    let mut owner = BlueprintExecutor::new(store);

    assert_eq!(
        owner.run_next(&goal, &blueprint, || true).unwrap(),
        RunOutcome::Cancelled {
            item_id: Some("first".into()),
            attempt: Some(1),
        }
    );
    assert!(owner.store().receipts().is_empty());
    let RunOutcome::Executed { receipt, .. } = owner.run_next(&goal, &blueprint, || false).unwrap()
    else {
        panic!("uncancelled retry should execute");
    };
    assert_eq!(receipt.item_id, "first");
}

#[test]
fn cancellation_after_running_is_durable_and_restart_safe() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("execution.json");
    let store = ExecutionStore::open(&path).unwrap();
    let (blueprint, goal) = fixture();
    let mut owner = BlueprintExecutor::new(store);
    let calls = std::sync::atomic::AtomicUsize::new(0);

    let outcome = owner
        .run_next(&goal, &blueprint, || {
            // The first check admits the operation; the second check cancels it.
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) > 0
        })
        .unwrap();
    assert!(matches!(outcome, RunOutcome::Cancelled { .. }));
    assert_eq!(owner.store().receipts().len(), 1);
    assert_eq!(
        owner.store().receipts()[0].status,
        ExecutionStatus::Cancelled
    );

    let reopened = ExecutionStore::open(&path).unwrap();
    let mut restarted = BlueprintExecutor::new(reopened);
    let retry = restarted.run_next(&goal, &blueprint, || false).unwrap();
    let RunOutcome::Executed {
        receipt,
        resumed_running,
    } = retry
    else {
        panic!("cancelled item should be retryable");
    };
    assert_eq!(receipt.item_id, "first");
    assert_eq!(receipt.attempt, 2);
    assert!(!resumed_running);
}

#[test]
fn budget_and_tamper_fail_closed_without_mutating_receipts() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("execution.json");
    let store = ExecutionStore::open(&path).unwrap();
    let (blueprint, mut goal) = fixture();
    goal.budget = ResourceBudget {
        tokens: 1,
        wall_clock_ms: 1,
        attempts: 1,
        disk_bytes: 1,
    };
    let mut owner = BlueprintExecutor::new(store);
    let before = fs::read(&path).unwrap();
    assert!(matches!(
        owner.run_next(&goal, &blueprint, || false),
        Err(ExecutionError::BudgetExceeded { .. })
    ));
    assert!(owner.store().receipts().is_empty());
    assert_eq!(fs::read(&path).unwrap(), before);

    let mut bytes = before;
    bytes.extend_from_slice(b"x");
    fs::write(&path, bytes).unwrap();
    assert!(matches!(
        ExecutionStore::open(&path),
        Err(ExecutionError::Json(_)) | Err(ExecutionError::DigestMismatch)
    ));
}

#[test]
fn running_receipt_is_resumed_in_place_without_duplicate_attempt() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("execution.json");
    let store = ExecutionStore::open(&path).unwrap();
    let (blueprint, goal) = fixture();
    let mut owner = BlueprintExecutor::new(store);
    let item = &blueprint.items[0];
    owner
        .store_mut()
        .upsert_receipt(zenpi::domain_execution::ExecutionReceipt {
            execution_id: "running-receipt".into(),
            goal_id: goal.id.clone(),
            blueprint_id: blueprint.id.clone(),
            blueprint_version: blueprint.version.clone(),
            blueprint_digest: blueprint.digest.clone(),
            item_id: item.id.clone(),
            attempt: 1,
            status: ExecutionStatus::Running,
            cost: deterministic_cost(item),
            evidence: "deterministic_local_evidence pending".into(),
            error: None,
        })
        .unwrap();
    // A fresh owner sees the running receipt and completes attempt one rather
    // than allocating attempt two.
    let mut restarted = BlueprintExecutor::new(ExecutionStore::open(&path).unwrap());
    let RunOutcome::Executed {
        receipt,
        resumed_running,
    } = restarted.run_next(&goal, &blueprint, || false).unwrap()
    else {
        panic!("expected running receipt to resume");
    };
    assert!(resumed_running);
    assert_eq!(receipt.attempt, 1);
}

#[test]
fn deterministic_cost_is_bounded_by_item_estimate() {
    let item = BlueprintItem::new("item", 42);
    assert_eq!(deterministic_cost(&item).tokens, 42);
    assert_eq!(deterministic_cost(&item).attempts, 1);
}
