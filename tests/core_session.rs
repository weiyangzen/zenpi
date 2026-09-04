use serde_json::{Map, Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::tempdir;
use zenpi::{
    backend::{Backend, BackendError, Completion, CompletionRequest},
    core::{Agent, AgentError, TurnInputRequest, TurnRole, TurnSubmission},
    session::SessionStore,
    tools::{
        SideEffectPolicy, Tool, ToolCall, ToolContext, ToolDefinition, ToolError, ToolRegistry,
    },
};

struct FailingBackend;
impl Backend for FailingBackend {
    fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        Err(BackendError::Transport("offline".into()))
    }
}

struct SlowBackend;

impl Backend for SlowBackend {
    fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        std::thread::sleep(std::time::Duration::from_millis(25));
        Ok(Completion::text("must not persist"))
    }
}

struct ToolLoopBackend {
    calls: AtomicUsize,
}

impl Backend for ToolLoopBackend {
    fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            assert_eq!(request.tools.len(), 1);
            return Ok(Completion {
                content: String::new(),
                usage: None,
                model: None,
                tool_calls: vec![ToolCall {
                    id: "call-1".into(),
                    name: "constant".into(),
                    arguments: json!({}),
                }],
                response_id: None,
                refusal: None,
                annotations: Vec::new(),
            });
        }
        assert!(request.turns.iter().any(|turn| turn.role == TurnRole::Tool));
        Ok(Completion::text("tool result incorporated"))
    }
}

#[derive(Clone, Copy)]
struct ConstantTool;

impl Tool for ConstantTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "constant".into(),
            description: "Return a deterministic value for testing.".into(),
            input_schema: json!({"type": "object", "additionalProperties": false}),
            side_effect: zenpi::tools::ToolSideEffect::ReadOnly,
        }
    }

    fn invoke(&self, _: &ToolContext, _: &Map<String, Value>) -> Result<Value, ToolError> {
        Ok(json!({"value": 42}))
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
    assert!(
        BackendError::HttpStatus {
            status: 429,
            retry_after_ms: None,
        }
        .is_retryable()
    );
    assert!(
        BackendError::HttpStatus {
            status: 503,
            retry_after_ms: None,
        }
        .is_retryable()
    );
    assert!(
        !BackendError::HttpStatus {
            status: 400,
            retry_after_ms: None,
        }
        .is_retryable()
    );
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

#[test]
fn provider_tool_calls_execute_and_continue_until_final_text() {
    let workspace = tempdir().unwrap();
    let session = SessionStore::open(workspace.path().join("tool-loop.jsonl")).unwrap();
    let mut registry = ToolRegistry::new();
    registry.register(ConstantTool).unwrap();
    let context = ToolContext::new(workspace.path()).unwrap();
    let mut agent = Agent::new(
        session,
        Box::new(ToolLoopBackend {
            calls: AtomicUsize::new(0),
        }),
    );
    agent.set_tools(registry, context, SideEffectPolicy::read_only());

    let result = agent
        .process(TurnInputRequest::new("use the tool"))
        .unwrap();
    assert_eq!(
        result.assistant.unwrap().content,
        "tool result incorporated"
    );
    assert_eq!(
        agent
            .history()
            .iter()
            .filter(|turn| turn.role == TurnRole::Tool)
            .count(),
        1
    );
    assert!(agent.take_events().iter().any(|event| matches!(
        event,
        zenpi::core::AgentEvent::ToolResult { success: true, .. }
    )));
}

#[test]
fn cancellation_is_checked_before_assistant_persistence() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cancel.jsonl");
    let mut agent = Agent::new(SessionStore::open(&path).unwrap(), Box::new(SlowBackend));
    let error = agent
        .process_with_cancel(TurnInputRequest::new("cancel me"), || true)
        .expect_err("cancelled turn must not return an assistant");
    assert!(matches!(
        error,
        AgentError::Backend(BackendError::Cancelled)
    ));
    assert_eq!(
        agent
            .history()
            .iter()
            .filter(|turn| turn.role == TurnRole::Assistant)
            .count(),
        0
    );
    assert_eq!(agent.history().len(), 1);
}
