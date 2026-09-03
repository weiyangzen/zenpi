use tempfile::tempdir;
use zenpi::{
    backend::{Backend, BackendError, Completion, CompletionRequest},
    core::{Agent, AgentError, TurnInputRequest, TurnRole, TurnSubmission},
    session::SessionStore,
};

struct FailingBackend;
impl Backend for FailingBackend {
    fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        Err(BackendError::Transport("offline".into()))
    }
}

#[test]
fn prompt_is_durable_and_invalid_steer_is_refused() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let result = agent.process(TurnInputRequest::new("hello")).unwrap();
    assert!(matches!(result.submission, TurnSubmission::Started { .. }));
    assert_eq!(result.assistant.unwrap().role, TurnRole::Assistant);
    let rejected = agent.steer_turn(TurnInputRequest::new("late")).unwrap();
    assert_eq!(
        rejected,
        TurnSubmission::NotSubmitted {
            reason: zenpi::core::NotSubmittedReason::NoActiveTurn
        }
    );
    assert_eq!(agent.history().len(), 2);
    assert_eq!(SessionStore::open(path).unwrap().turns().len(), 2);
}

#[test]
fn closed_agent_rejects_without_a_write() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("closed.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    agent.close();
    assert!(matches!(
        agent.process(TurnInputRequest::new("x")),
        Err(AgentError::Closed)
    ));
    assert_eq!(agent.history().len(), 0);
}

#[test]
fn backend_failure_keeps_the_user_turn_and_returns_idle() {
    let dir = tempdir().unwrap();
    let store = SessionStore::open(dir.path().join("failed.jsonl")).unwrap();
    let mut agent = Agent::new(store, Box::new(FailingBackend));
    assert!(matches!(
        agent.process(TurnInputRequest::new("keep")),
        Err(AgentError::Backend(_))
    ));
    assert_eq!(agent.history().len(), 1);
    assert_eq!(agent.phase(), zenpi::core::AgentPhase::Idle);
}

#[test]
fn backend_retryability_is_typed() {
    assert!(BackendError::Transport("offline".into()).is_retryable());
    assert!(BackendError::HttpStatus { status: 429 }.is_retryable());
    assert!(BackendError::HttpStatus { status: 503 }.is_retryable());
    assert!(!BackendError::HttpStatus { status: 400 }.is_retryable());
    assert!(!BackendError::Configuration("bad key".into()).is_retryable());
    assert!(!BackendError::EmptyResponse.is_retryable());
}

#[test]
fn steer_requires_an_active_turn_and_expected_id_matches() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("steer.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    assert_eq!(
        agent
            .steer_turn(TurnInputRequest::new("before start"))
            .unwrap(),
        TurnSubmission::NotSubmitted {
            reason: zenpi::core::NotSubmittedReason::NoActiveTurn
        }
    );
    let started = agent
        .start_turn_if_idle(TurnInputRequest::new("start"))
        .unwrap();
    let turn_id = started.turn_id().unwrap().to_owned();
    assert_eq!(
        agent
            .steer_turn(TurnInputRequest::new("wrong").expecting("other"))
            .unwrap(),
        TurnSubmission::NotSubmitted {
            reason: zenpi::core::NotSubmittedReason::ExpectedTurnMismatch
        }
    );
    let steered = agent
        .steer_turn(TurnInputRequest::new("right").expecting(turn_id))
        .unwrap();
    assert!(matches!(steered, TurnSubmission::Steered { .. }));
}
