use tempfile::tempdir;
use zenpi::{
    backend::{Backend, BackendError, Completion, CompletionRequest, Usage},
    core::{Agent, TurnInputRequest},
    governance::{
        BudgetCompletion, BudgetLedger, BudgetOrigin, BudgetReservation, BudgetTerminal,
        GovernanceError, ResourceKind, ResourceLimits, ResourceUsage, WorkerBudgetLedger,
        WorkerLease,
    },
    session::SessionStore,
};

struct UsageBackend;

impl Backend for UsageBackend {
    fn complete(&self, _request: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        Ok(Completion {
            content: "budgeted".into(),
            usage: Some(Usage {
                input_tokens: 2,
                output_tokens: 3,
                total_tokens: 5,
            }),
            ..Completion::default()
        })
    }
}

fn limits() -> ResourceLimits {
    ResourceLimits {
        max_input_tokens: 10,
        max_output_tokens: 20,
        max_wall_ms: 60_000,
        max_disk_bytes: 100,
        max_processes: 2,
        max_concurrency: 1,
        max_network_requests: 2,
    }
}

#[test]
fn every_resource_dimension_is_enforced_without_mutating_on_rejection() {
    for (kind, amount) in [
        (ResourceKind::InputTokens, 11),
        (ResourceKind::OutputTokens, 21),
        (ResourceKind::Disk, 101),
        (ResourceKind::Processes, 3),
        (ResourceKind::Concurrency, 2),
        (ResourceKind::NetworkRequests, 3),
    ] {
        let mut ledger = BudgetLedger::new(limits(), ResourceUsage::default()).unwrap();
        assert!(matches!(
            ledger.charge(kind, amount),
            Err(GovernanceError::BudgetExceeded { .. })
        ));
        assert_eq!(ledger.usage(), ResourceUsage::default());
    }
}

#[test]
fn retry_and_process_charges_cannot_bypass_an_exhausted_budget() {
    let mut ledger = BudgetLedger::new(limits(), ResourceUsage::default()).unwrap();
    ledger.charge(ResourceKind::NetworkRequests, 1).unwrap();
    ledger.charge(ResourceKind::NetworkRequests, 1).unwrap();
    assert!(ledger.charge(ResourceKind::NetworkRequests, 1).is_err());
    ledger.charge(ResourceKind::Concurrency, 1).unwrap();
    assert!(ledger.charge(ResourceKind::Concurrency, 1).is_err());
    ledger.release(ResourceKind::Concurrency, 1);
    ledger.charge(ResourceKind::Concurrency, 1).unwrap();
}

#[test]
fn accounting_persists_and_restores_from_a_session() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("budget.jsonl");
    let mut session = SessionStore::open(&path).unwrap();
    let mut ledger = BudgetLedger::new(limits(), ResourceUsage::default()).unwrap();
    ledger.charge(ResourceKind::InputTokens, 7).unwrap();
    ledger.charge(ResourceKind::NetworkRequests, 2).unwrap();
    ledger.persist(&mut session).unwrap();
    drop(session);

    let session = SessionStore::open(path).unwrap();
    let mut restored = BudgetLedger::restore(&session, limits()).unwrap();
    assert_eq!(restored.usage().input_tokens, 7);
    assert_eq!(restored.usage().network_requests, 2);
    assert!(restored.charge(ResourceKind::NetworkRequests, 1).is_err());
}

#[test]
fn resuming_a_session_reloads_governance_from_the_replacement_journal() {
    let dir = tempdir().unwrap();
    let original_path = dir.path().join("original-budget.jsonl");
    let replacement_path = dir.path().join("replacement-budget.jsonl");
    let mut agent = Agent::new(
        SessionStore::open(&original_path).unwrap(),
        Box::new(UsageBackend),
    );
    let mut session_limits = limits();
    session_limits.max_input_tokens = 10_000;
    session_limits.max_output_tokens = 10_000;
    agent.set_resource_limits(session_limits).unwrap();

    let mut replacement_session = SessionStore::open(&replacement_path).unwrap();
    let mut replacement_ledger =
        BudgetLedger::new(session_limits, ResourceUsage::default()).unwrap();
    replacement_ledger
        .charge(ResourceKind::NetworkRequests, 1)
        .unwrap();
    replacement_ledger
        .persist(&mut replacement_session)
        .unwrap();
    drop(replacement_session);

    agent
        .process(TurnInputRequest::new("first request"))
        .unwrap();
    agent
        .process(TurnInputRequest::new("second request"))
        .unwrap();
    let original = SessionStore::open(&original_path).unwrap();
    let original_usage = original
        .events()
        .iter()
        .rev()
        .find(|event| event["type"] == "resource_usage")
        .expect("original session should contain resource accounting");
    assert_eq!(original_usage["usage"]["network_requests"], 2);

    agent.resume_session(&replacement_path).unwrap();
    agent
        .process(TurnInputRequest::new("replacement request"))
        .expect("the replacement session must not inherit the exhausted network budget");

    let replacement = SessionStore::open(&replacement_path).unwrap();
    let replacement_usage = replacement
        .events()
        .iter()
        .rev()
        .find(|event| event["type"] == "resource_usage")
        .expect("replacement session should contain independent accounting");
    assert_eq!(replacement_usage["usage"]["network_requests"], 2);
}

#[test]
fn agent_emits_typed_budget_error_before_network_when_limit_is_exhausted() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("agent-budget.jsonl");
    let mut agent = Agent::new(SessionStore::open(&path).unwrap(), Box::new(UsageBackend));
    let mut strict = limits();
    strict.max_input_tokens = 0;
    agent.set_resource_limits(strict).unwrap();
    let error = agent
        .process(TurnInputRequest::new("this request needs tokens"))
        .unwrap_err();
    assert_eq!(error.code(), "resource_budget_exceeded");
    let journal = std::fs::read_to_string(path).unwrap();
    assert!(!journal.contains("budgeted"));
}

fn lease() -> WorkerLease {
    WorkerLease {
        lease_id: "worker-generation-1".into(),
        blueprint_item: "CF-703".into(),
        policy_digest: "a".repeat(64),
        expires_at_ms: 100_000,
        limits: limits(),
    }
}

fn reservation(id: &str, origin: BudgetOrigin) -> BudgetReservation {
    BudgetReservation {
        operation_id: id.into(),
        lease_id: lease().lease_id,
        policy_digest: lease().policy_digest,
        origin,
        resources: ResourceUsage {
            input_tokens: 2,
            output_tokens: 3,
            wall_ms: 1_000,
            disk_bytes: 25,
            processes: 1,
            concurrency: 1,
            network_requests: 1,
        },
        gate_decision_id: format!("gate-{id}"),
        network_host: Some("api.example.test".into()),
        credential_handles: vec!["provider-key-handle".into()],
    }
}

fn actual() -> ResourceUsage {
    ResourceUsage {
        input_tokens: 2,
        output_tokens: 3,
        wall_ms: 10,
        disk_bytes: 20,
        processes: 1,
        concurrency: 0,
        network_requests: 1,
    }
}

#[test]
fn legacy_accounting_rejects_overflow_without_erasing_prior_usage() {
    let mut strict = limits();
    strict.max_input_tokens = u64::MAX;
    let mut ledger = BudgetLedger::new(strict, ResourceUsage::default()).unwrap();
    ledger.charge(ResourceKind::InputTokens, 9).unwrap();
    assert!(matches!(
        ledger.charge(ResourceKind::InputTokens, u64::MAX),
        Err(GovernanceError::AccountingOverflow(
            ResourceKind::InputTokens
        ))
    ));
    assert_eq!(ledger.usage().input_tokens, 9);
}

#[test]
fn malformed_latest_usage_does_not_restore_an_empty_budget() {
    let dir = tempdir().unwrap();
    let mut session = SessionStore::open(dir.path().join("malformed.jsonl")).unwrap();
    session
        .append_event(serde_json::json!({ "type": "resource_usage", "usage": "broken" }))
        .unwrap();
    assert!(matches!(
        BudgetLedger::restore(&session, limits()),
        Err(GovernanceError::InvalidSnapshot(_))
    ));
}

#[test]
fn reservation_is_durable_and_recovery_blocks_duplicate_effects() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("worker.jsonl");
    let mut session = SessionStore::open(&path).unwrap();
    let mut ledger = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    ledger.open_lease(&mut session, lease(), 1_000).unwrap();
    ledger
        .reserve(
            &mut session,
            reservation("tool-1", BudgetOrigin::BlueprintWorker),
            1_010,
        )
        .unwrap();
    let bytes = std::fs::read_to_string(&path).unwrap();
    assert!(bytes.contains("gate-tool-1"));
    assert!(bytes.contains("api.example.test"));
    assert!(bytes.contains("provider-key-handle"));
    assert!(bytes.contains(&"a".repeat(64)));
    assert_eq!(ledger.committed_usage(None).unwrap().concurrency, 1);
    drop(ledger);
    drop(session);

    let mut session = SessionStore::open_existing_writable(&path).unwrap();
    let mut restored = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    assert!(restored.operations()["tool-1"].unknown_outcome);
    assert!(matches!(
        restored.reserve(
            &mut session,
            reservation("retry-1", BudgetOrigin::UserShell),
            1_015
        ),
        Err(GovernanceError::RecoveryRequired)
    ));
    restored
        .settle(
            &mut session,
            "tool-1",
            actual(),
            BudgetCompletion::Cancelled,
            1_020,
        )
        .unwrap();
    assert!(matches!(
        restored.reserve(
            &mut session,
            reservation("tool-1", BudgetOrigin::UserShell),
            1_021
        ),
        Err(GovernanceError::DuplicateOperation(_))
    ));
    restored
        .reserve(
            &mut session,
            reservation("retry-1", BudgetOrigin::UserShell),
            1_022,
        )
        .unwrap();
    assert_eq!(restored.committed_usage(None).unwrap().network_requests, 2);
}

#[test]
fn worker_tool_and_user_shell_share_durable_nonrefundable_spend() {
    let dir = tempdir().unwrap();
    let mut session = SessionStore::open(dir.path().join("origins.jsonl")).unwrap();
    let mut ledger = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    ledger.open_lease(&mut session, lease(), 100).unwrap();
    for (index, origin) in [BudgetOrigin::BlueprintWorker, BudgetOrigin::AgentTool]
        .into_iter()
        .enumerate()
    {
        let id = format!("request-{index}");
        let now = 110 + index as u64 * 20;
        ledger
            .reserve(&mut session, reservation(&id, origin), now)
            .unwrap();
        ledger
            .settle(
                &mut session,
                &id,
                actual(),
                BudgetCompletion::Failed,
                now + 10,
            )
            .unwrap();
    }
    assert_eq!(ledger.committed_usage(None).unwrap().concurrency, 0);
    assert_eq!(ledger.committed_usage(None).unwrap().network_requests, 2);
    assert!(matches!(
        ledger.reserve(
            &mut session,
            reservation("shell-retry", BudgetOrigin::UserShell),
            150
        ),
        Err(GovernanceError::WorkerStopped(
            BudgetTerminal::Exhausted { .. }
        ))
    ));
    assert!(ledger.terminal().is_some());
    assert!(!ledger.operations().contains_key("shell-retry"));
    let restored = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    assert!(restored.terminal().is_some());
    assert_eq!(restored.committed_usage(None).unwrap().network_requests, 2);
}

#[test]
fn every_worker_reservation_dimension_is_checked_before_admission() {
    for kind in [
        ResourceKind::InputTokens,
        ResourceKind::OutputTokens,
        ResourceKind::WallTime,
        ResourceKind::Disk,
        ResourceKind::Processes,
        ResourceKind::Concurrency,
        ResourceKind::NetworkRequests,
    ] {
        let dir = tempdir().unwrap();
        let mut session = SessionStore::open(dir.path().join("dimension.jsonl")).unwrap();
        let mut ledger = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
        ledger.open_lease(&mut session, lease(), 100).unwrap();
        let mut request = reservation("over-budget", BudgetOrigin::UserShell);
        match kind {
            ResourceKind::InputTokens => request.resources.input_tokens = 11,
            ResourceKind::OutputTokens => request.resources.output_tokens = 21,
            ResourceKind::WallTime => request.resources.wall_ms = 60_001,
            ResourceKind::Disk => request.resources.disk_bytes = 101,
            ResourceKind::Processes => request.resources.processes = 3,
            ResourceKind::Concurrency => request.resources.concurrency = 2,
            ResourceKind::NetworkRequests => request.resources.network_requests = 3,
        }
        assert!(matches!(ledger.reserve(&mut session, request, 110),
            Err(GovernanceError::WorkerStopped(BudgetTerminal::Exhausted { kind: exceeded, .. })) if exceeded == kind));
        assert!(ledger.operations().is_empty());
        assert_eq!(
            ledger.committed_usage(None).unwrap(),
            ResourceUsage::default()
        );
        assert!(session.events().last().unwrap()["directive"]["terminal"]["status"] == "exhausted");
    }
}

#[test]
fn revoke_keeps_reservations_until_host_confirms_reaping() {
    let dir = tempdir().unwrap();
    let mut session = SessionStore::open(dir.path().join("cancel.jsonl")).unwrap();
    let mut ledger = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    ledger.open_lease(&mut session, lease(), 100).unwrap();
    ledger
        .reserve(
            &mut session,
            reservation("child", BudgetOrigin::BlueprintWorker),
            110,
        )
        .unwrap();
    let directive = ledger
        .revoke_lease(&mut session, &lease().lease_id, "host_cancel", 120)
        .unwrap();
    assert_eq!(directive.cancel_operations, ["child"]);
    assert_eq!(ledger.committed_usage(None).unwrap().concurrency, 1);
    assert!(ledger.operations()["child"].cancel_requested);
    assert!(matches!(
        ledger.reserve(
            &mut session,
            reservation("retry", BudgetOrigin::UserShell),
            125
        ),
        Err(GovernanceError::LeaseUnavailable(_))
    ));
    ledger
        .settle(
            &mut session,
            "child",
            actual(),
            BudgetCompletion::Cancelled,
            130,
        )
        .unwrap();
    assert_eq!(ledger.committed_usage(None).unwrap().concurrency, 0);
    assert_eq!(ledger.committed_usage(None).unwrap().wall_ms, 20);
    assert_eq!(ledger.committed_usage(None).unwrap().processes, 1);
}

#[test]
fn overshoot_records_actual_spend_and_cancels_instead_of_rolling_back() {
    let dir = tempdir().unwrap();
    let mut session = SessionStore::open(dir.path().join("overshoot.jsonl")).unwrap();
    let mut ledger = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    ledger.open_lease(&mut session, lease(), 100).unwrap();
    ledger
        .reserve(
            &mut session,
            reservation("write", BudgetOrigin::AgentTool),
            110,
        )
        .unwrap();
    let mut spent = actual();
    spent.disk_bytes = 101;
    let directive = ledger
        .settle(&mut session, "write", spent, BudgetCompletion::Failed, 120)
        .unwrap()
        .unwrap();
    assert!(matches!(
        directive.terminal,
        BudgetTerminal::Exhausted {
            kind: ResourceKind::Disk,
            used: 101,
            limit: 25
        }
    ));
    assert_eq!(ledger.committed_usage(None).unwrap().disk_bytes, 101);
    let mut restored = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    assert_eq!(restored.committed_usage(None).unwrap().disk_bytes, 101);
    assert!(matches!(
        restored.reserve(
            &mut session,
            reservation("retry", BudgetOrigin::UserShell),
            130
        ),
        Err(GovernanceError::WorkerStopped(_))
    ));
}

#[test]
fn elapsed_deadline_and_lease_expiration_issue_durable_cancel_directives() {
    let dir = tempdir().unwrap();
    let mut session = SessionStore::open(dir.path().join("timeout.jsonl")).unwrap();
    let mut ledger = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    let mut short_lease = lease();
    short_lease.expires_at_ms = 1_200;
    ledger.open_lease(&mut session, short_lease, 100).unwrap();
    ledger
        .reserve(
            &mut session,
            reservation("child", BudgetOrigin::UserShell),
            110,
        )
        .unwrap();
    let directives = ledger.expire(&mut session, 1_201).unwrap();
    assert!(directives.iter().any(|directive| matches!(
        directive.terminal,
        BudgetTerminal::Exhausted {
            kind: ResourceKind::WallTime,
            ..
        }
    )));
    assert!(
        directives
            .iter()
            .any(|directive| matches!(directive.terminal, BudgetTerminal::LeaseExpired { .. }))
    );
    assert_eq!(ledger.committed_usage(None).unwrap().concurrency, 1);
    assert!(ledger.operations()["child"].cancel_requested);
    assert!(matches!(
        ledger.expire(&mut session, 1_200),
        Err(GovernanceError::ClockRegressed)
    ));
    let restored = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    assert!(restored.terminal().is_some());
}

#[test]
fn policy_lease_limits_and_snapshot_identity_are_immutable() {
    let dir = tempdir().unwrap();
    let mut session = SessionStore::open(dir.path().join("identity.jsonl")).unwrap();
    let mut ledger = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    ledger.open_lease(&mut session, lease(), 100).unwrap();
    assert!(matches!(
        ledger.open_lease(&mut session, lease(), 110),
        Err(GovernanceError::DuplicateLease(_))
    ));
    let mut changed = reservation("changed", BudgetOrigin::BlueprintWorker);
    changed.policy_digest = "b".repeat(64);
    assert!(matches!(
        ledger.reserve(&mut session, changed, 120),
        Err(GovernanceError::PolicyMismatch)
    ));
    let mut increased = limits();
    increased.max_network_requests += 1;
    assert!(matches!(
        WorkerBudgetLedger::restore(&mut session, increased),
        Err(GovernanceError::InvalidSnapshot(_))
    ));
    let mut other = SessionStore::open(dir.path().join("other.jsonl")).unwrap();
    assert!(matches!(
        ledger.reserve(
            &mut other,
            reservation("wrong-session", BudgetOrigin::BlueprintWorker),
            130
        ),
        Err(GovernanceError::InvalidSnapshot(_))
    ));
    assert!(ledger.operations().is_empty());
}

#[test]
fn invalid_latest_worker_snapshot_fails_closed() {
    let dir = tempdir().unwrap();
    let mut session = SessionStore::open(dir.path().join("corrupt.jsonl")).unwrap();
    let mut ledger = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    ledger.open_lease(&mut session, lease(), 100).unwrap();
    ledger
        .reserve(
            &mut session,
            reservation("child", BudgetOrigin::UserShell),
            110,
        )
        .unwrap();
    let mut corrupted = session.events().last().unwrap().clone();
    corrupted["snapshot"]["operations"]["child"]["reservation"]["policy_digest"] =
        "b".repeat(64).into();
    session.append_event(corrupted).unwrap();
    assert!(matches!(
        WorkerBudgetLedger::restore(&mut session, limits()),
        Err(GovernanceError::InvalidSnapshot(_))
    ));
}

#[test]
fn failed_journal_write_cannot_admit_work_in_memory() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("readonly.jsonl");
    {
        let mut session = SessionStore::open(&path).unwrap();
        let mut ledger = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
        ledger.open_lease(&mut session, lease(), 100).unwrap();
    }
    let mut session = SessionStore::open_existing(&path).unwrap();
    let mut ledger = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    let mut writer = SessionStore::open_existing_writable(&path).unwrap();
    writer
        .append_event(serde_json::json!({ "type": "other_writer" }))
        .unwrap();
    assert!(
        ledger
            .reserve(
                &mut session,
                reservation("child", BudgetOrigin::UserShell),
                110
            )
            .is_err()
    );
    assert!(ledger.operations().is_empty());
}

#[test]
fn metadata_rejects_raw_environment_values_and_unaccounted_network() {
    let dir = tempdir().unwrap();
    let mut session = SessionStore::open(dir.path().join("metadata.jsonl")).unwrap();
    let mut ledger = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    ledger.open_lease(&mut session, lease(), 100).unwrap();
    let mut request = reservation("bad-secret", BudgetOrigin::BlueprintWorker);
    request.credential_handles = vec!["API_KEY=do-not-store".into()];
    assert!(matches!(
        ledger.reserve(&mut session, request, 110),
        Err(GovernanceError::InvalidSnapshot(_))
    ));
    let mut request = reservation("missing-host", BudgetOrigin::BlueprintWorker);
    request.network_host = None;
    assert!(matches!(
        ledger.reserve(&mut session, request, 120),
        Err(GovernanceError::InvalidSnapshot(_))
    ));
    let mut request = reservation("host-without-budget", BudgetOrigin::BlueprintWorker);
    request.resources.network_requests = 0;
    assert!(matches!(
        ledger.reserve(&mut session, request, 130),
        Err(GovernanceError::InvalidSnapshot(_))
    ));
    assert!(ledger.operations().is_empty());
}

#[test]
fn new_worker_generation_cannot_reset_the_items_budget() {
    let dir = tempdir().unwrap();
    let mut session = SessionStore::open(dir.path().join("generation.jsonl")).unwrap();
    let mut ledger = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    let mut first = lease();
    first.limits.max_network_requests = 1;
    ledger.open_lease(&mut session, first.clone(), 100).unwrap();
    ledger
        .reserve(
            &mut session,
            reservation("first", BudgetOrigin::BlueprintWorker),
            110,
        )
        .unwrap();
    ledger
        .settle(
            &mut session,
            "first",
            actual(),
            BudgetCompletion::Failed,
            120,
        )
        .unwrap();
    let mut retry = first;
    retry.lease_id = "worker-generation-2".into();
    ledger.open_lease(&mut session, retry.clone(), 130).unwrap();
    let mut request = reservation("second", BudgetOrigin::UserShell);
    request.lease_id = retry.lease_id;
    assert!(matches!(
        ledger.reserve(&mut session, request, 140),
        Err(GovernanceError::WorkerStopped(BudgetTerminal::Exhausted {
            kind: ResourceKind::NetworkRequests,
            used: 2,
            limit: 1,
        }))
    ));
    assert_eq!(ledger.committed_usage(None).unwrap().network_requests, 1);
}

#[test]
fn replaying_an_older_valid_snapshot_cannot_erase_spend() {
    let dir = tempdir().unwrap();
    let mut session = SessionStore::open(dir.path().join("rollback.jsonl")).unwrap();
    let mut ledger = WorkerBudgetLedger::restore(&mut session, limits()).unwrap();
    ledger.open_lease(&mut session, lease(), 100).unwrap();
    let previous = session.events().last().unwrap().clone();
    ledger
        .reserve(
            &mut session,
            reservation("first", BudgetOrigin::BlueprintWorker),
            110,
        )
        .unwrap();
    ledger
        .settle(
            &mut session,
            "first",
            actual(),
            BudgetCompletion::Failed,
            120,
        )
        .unwrap();
    session.append_event(previous).unwrap();
    assert!(matches!(
        WorkerBudgetLedger::restore(&mut session, limits()),
        Err(GovernanceError::InvalidSnapshot(_))
    ));
}
