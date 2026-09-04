use tempfile::tempdir;
use zenpi::{
    backend::{Backend, BackendError, Completion, CompletionRequest, Usage},
    core::{Agent, TurnInputRequest},
    governance::{BudgetLedger, GovernanceError, ResourceKind, ResourceLimits, ResourceUsage},
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
