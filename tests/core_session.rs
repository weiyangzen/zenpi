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

struct ScriptedToolBackend {
    completions: std::sync::Mutex<std::collections::VecDeque<Completion>>,
    requests: std::sync::Arc<AtomicUsize>,
}

impl Backend for ScriptedToolBackend {
    fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .completions
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected continuation"))
    }
}

fn tool_completion(ids: &[&str]) -> Completion {
    let mut completion = Completion::text("");
    completion.tool_calls = ids
        .iter()
        .map(|id| ToolCall {
            id: (*id).into(),
            name: "constant".into(),
            arguments: json!({}),
        })
        .collect();
    completion
}

fn scripted_agent(
    path: &std::path::Path,
    completions: Vec<Completion>,
) -> (Agent, std::sync::Arc<AtomicUsize>) {
    let requests = std::sync::Arc::new(AtomicUsize::new(0));
    let backend = ScriptedToolBackend {
        completions: std::sync::Mutex::new(completions.into()),
        requests: requests.clone(),
    };
    let mut agent = Agent::new(SessionStore::open(path).unwrap(), Box::new(backend));
    let mut registry = ToolRegistry::new();
    registry.register(ConstantTool).unwrap();
    agent.set_tools(
        registry,
        ToolContext::new(path.parent().unwrap()).unwrap(),
        SideEffectPolicy::read_only(),
    );
    (agent, requests)
}

fn worker_binding() -> zenpi::core::WorkerExecutionBinding {
    zenpi::core::WorkerExecutionBinding {
        blueprint_id: "blueprint".into(),
        blueprint_sha256: "b".repeat(64),
        goal_id: "goal".into(),
        item_id: "CF-402".into(),
        lease_id: "lease".into(),
        policy_digest: "a".repeat(64),
        expires_at_ms: u64::MAX,
    }
}

#[test]
fn every_call_and_result_preserves_worker_binding_without_claiming_gate_enforcement() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("worker.jsonl");
    let (mut agent, requests) = scripted_agent(
        &path,
        vec![tool_completion(&["a", "b"]), Completion::text("done")],
    );
    let binding = worker_binding();
    agent
        .set_worker_execution_binding(Some(binding.clone()))
        .unwrap();
    agent.process_sync("inspect").unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    let reopened = SessionStore::open(&path).unwrap();
    let calls = reopened
        .turns()
        .iter()
        .find_map(|turn| turn.metadata.as_ref()?.get("tool_calls"))
        .unwrap()
        .as_array()
        .unwrap();
    for call in calls {
        assert_eq!(call["policy_digest"], binding.policy_digest);
        assert_eq!(call["worker_binding"], json!(binding));
        assert_eq!(call["execution"]["origin"], "blueprint_worker");
        assert_eq!(call["execution"]["prohibition_gate_enforced"], false);
        let results: Vec<_> = reopened
            .turns()
            .iter()
            .filter(|turn| turn.role == TurnRole::Tool)
            .filter(|turn| turn.metadata.as_ref().unwrap()["tool_call_id"] == call["id"])
            .collect();
        assert_eq!(results.len(), 1);
        let metadata = results[0].metadata.as_ref().unwrap();
        assert_eq!(metadata["policy_digest"], call["policy_digest"]);
        assert_eq!(metadata["execution"], call["execution"]);
        assert_eq!(metadata["implementation_complete"], false);
        assert_eq!(metadata["outcome"], "succeeded");
        let result: Value = serde_json::from_str(&results[0].content).unwrap();
        assert_eq!(result["call_id"], call["id"]);
    }
    assert!(agent.unknown_tool_outcomes().is_empty());
}

#[test]
fn normal_agent_has_a_policy_snapshot_and_never_inherits_worker_identity() {
    let dir = tempdir().unwrap();
    let (mut agent, _) = scripted_agent(
        &dir.path().join("normal.jsonl"),
        vec![tool_completion(&["a"]), Completion::text("done")],
    );
    agent.process_sync("inspect").unwrap();
    let metadata = agent
        .history()
        .iter()
        .find(|turn| turn.role == TurnRole::Tool)
        .unwrap()
        .metadata
        .as_ref()
        .unwrap();
    assert_eq!(metadata["execution"]["origin"], "agent_tool");
    assert!(metadata["worker_binding"].is_null());
    assert_eq!(metadata["policy_digest"].as_str().unwrap().len(), 64);
    assert_eq!(
        metadata["policy_digest"],
        metadata["execution"]["approval_policy_digest"]
    );
}

#[test]
fn worker_binding_rejects_malformed_expired_and_mid_turn_replacement() {
    let dir = tempdir().unwrap();
    let (mut agent, _) = scripted_agent(&dir.path().join("binding.jsonl"), vec![]);
    let mut invalid = worker_binding();
    invalid.policy_digest = "invalid".into();
    assert!(agent.set_worker_execution_binding(Some(invalid)).is_err());
    let mut expired = worker_binding();
    expired.expires_at_ms = 1;
    assert!(agent.set_worker_execution_binding(Some(expired)).is_err());
    agent.submit(TurnInputRequest::new("pending")).unwrap();
    assert!(matches!(
        agent.set_worker_execution_binding(Some(worker_binding())),
        Err(AgentError::NotIdle)
    ));
}

#[test]
fn duplicate_calls_are_rejected_before_any_duplicate_side_effect() {
    let dir = tempdir().unwrap();
    let (mut agent, requests) = scripted_agent(
        &dir.path().join("duplicates.jsonl"),
        vec![tool_completion(&["a", "a"])],
    );
    assert!(matches!(
        agent.process_sync("inspect"),
        Err(AgentError::InvalidTurn(_))
    ));
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert_eq!(agent.history().len(), 1);
    let (mut agent, requests) = scripted_agent(
        &dir.path().join("reuse.jsonl"),
        vec![tool_completion(&["a"]), tool_completion(&["a"])],
    );
    assert!(matches!(
        agent.process_sync("inspect"),
        Err(AgentError::InvalidTurn(_))
    ));
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert_eq!(
        agent
            .history()
            .iter()
            .filter(|turn| turn.role == TurnRole::Tool)
            .count(),
        1
    );
}

#[test]
fn provider_refusal_is_terminal_even_when_it_contains_tool_calls() {
    let dir = tempdir().unwrap();
    let mut refusal = tool_completion(&["never"]);
    refusal.refusal = Some("cannot proceed".into());
    let (mut agent, requests) = scripted_agent(&dir.path().join("refusal.jsonl"), vec![refusal]);
    let result = agent.process_sync("inspect").unwrap().assistant.unwrap();
    assert_eq!(result.content, "cannot proceed");
    assert_eq!(result.metadata.unwrap()["refusal"], "cannot proceed");
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert!(
        !agent
            .history()
            .iter()
            .any(|turn| turn.role == TurnRole::Tool)
    );
}

#[test]
fn unique_tool_calls_cannot_bypass_iteration_limit() {
    let dir = tempdir().unwrap();
    let completions = (0..9)
        .map(|i| tool_completion(&[&format!("call-{i}")]))
        .collect();
    let (mut agent, requests) = scripted_agent(&dir.path().join("limit.jsonl"), completions);
    assert!(matches!(
        agent.process_sync("inspect"),
        Err(AgentError::ToolLoopLimit(8))
    ));
    assert_eq!(requests.load(Ordering::SeqCst), 9);
    assert_eq!(
        agent
            .history()
            .iter()
            .filter(|turn| turn.role == TurnRole::Tool)
            .count(),
        8
    );
    assert!(agent.unknown_tool_outcomes().is_empty());
}

struct CancellingTool {
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    side_effect: zenpi::tools::ToolSideEffect,
    invocations: std::sync::Arc<AtomicUsize>,
}

impl Tool for CancellingTool {
    fn definition(&self) -> ToolDefinition {
        let mut definition = ConstantTool.definition();
        definition.side_effect = self.side_effect;
        definition
    }
    fn invoke(&self, _: &ToolContext, _: &Map<String, Value>) -> Result<Value, ToolError> {
        self.invocations.fetch_add(1, Ordering::SeqCst);
        self.cancelled.store(true, Ordering::SeqCst);
        Err(ToolError::Cancelled)
    }
}

#[test]
fn cancellation_records_terminal_results_for_all_announced_calls() {
    let dir = tempdir().unwrap();
    let (mut agent, requests) = scripted_agent(
        &dir.path().join("batch-cancel.jsonl"),
        vec![tool_completion(&["a", "b", "c"])],
    );
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let invocations = std::sync::Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry
        .register(CancellingTool {
            cancelled: cancelled.clone(),
            side_effect: zenpi::tools::ToolSideEffect::ReadOnly,
            invocations: invocations.clone(),
        })
        .unwrap();
    agent.set_tools(
        registry,
        ToolContext::new(dir.path()).unwrap(),
        SideEffectPolicy::read_only(),
    );
    assert!(matches!(
        agent.process_with_cancel(TurnInputRequest::new("inspect"), || cancelled
            .load(Ordering::SeqCst)),
        Err(AgentError::Backend(BackendError::Cancelled))
    ));
    assert_eq!(invocations.load(Ordering::SeqCst), 1);
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    let results: Vec<_> = agent
        .history()
        .iter()
        .filter(|turn| turn.role == TurnRole::Tool)
        .collect();
    assert_eq!(results.len(), 3);
    for result in results {
        assert_eq!(result.metadata.as_ref().unwrap()["outcome"], "cancelled");
    }
    assert!(agent.unknown_tool_outcomes().is_empty());
}

#[test]
fn cancelled_side_effect_is_unknown_and_requires_explicit_host_decision_after_restart() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("unknown.jsonl");
    let (mut agent, requests) = scripted_agent(&path, vec![tool_completion(&["a"])]);
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let invocations = std::sync::Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry
        .register(CancellingTool {
            cancelled,
            side_effect: zenpi::tools::ToolSideEffect::WorkspaceWrite,
            invocations: invocations.clone(),
        })
        .unwrap();
    agent.set_tools(
        registry,
        ToolContext::new(dir.path()).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    agent.set_approval_policy(zenpi::approval::ApprovalPolicy {
        mode: zenpi::approval::ApprovalMode::TrustedWorkspace,
        trusted_tools: vec!["constant".into()],
        per_tool: Default::default(),
    });
    agent
        .set_worker_execution_binding(Some(worker_binding()))
        .unwrap();
    assert!(matches!(
        agent.process_sync("mutate"),
        Err(AgentError::Recovery(_))
    ));
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert_eq!(invocations.load(Ordering::SeqCst), 1);
    let pending = agent.unknown_tool_outcomes();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].worker_binding, Some(worker_binding()));
    drop(agent);
    let mut restarted = Agent::with_echo(SessionStore::open(&path).unwrap());
    restarted.acknowledge_recovery().unwrap();
    let before = restarted.session().next_sequence();
    assert!(matches!(
        restarted.process_sync("try again"),
        Err(AgentError::Recovery(_))
    ));
    assert_eq!(restarted.session().next_sequence(), before);
    restarted
        .resolve_tool_outcome(
            &pending[0].operation_id,
            zenpi::core::ToolRecoveryDecision::Retry,
        )
        .unwrap();
    assert_eq!(
        invocations.load(Ordering::SeqCst),
        1,
        "decision must not reexecute a side effect"
    );
    assert!(restarted.unknown_tool_outcomes().is_empty());
    assert!(restarted.process_sync("retry explicitly").is_ok());
    assert!(
        restarted
            .resolve_tool_outcome(
                &pending[0].operation_id,
                zenpi::core::ToolRecoveryDecision::Retry
            )
            .is_err()
    );
}

#[test]
fn worker_correlation_does_not_turn_denied_tools_into_all_allow() {
    let dir = tempdir().unwrap();
    let (mut agent, _) = scripted_agent(
        &dir.path().join("no-implicit-allow.jsonl"),
        vec![tool_completion(&["a"]), Completion::text("denied")],
    );
    let invocations = std::sync::Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry
        .register(CancellingTool {
            cancelled: Default::default(),
            side_effect: zenpi::tools::ToolSideEffect::WorkspaceWrite,
            invocations: invocations.clone(),
        })
        .unwrap();
    agent.set_tools(
        registry,
        ToolContext::new(dir.path()).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    agent.set_approval_policy(zenpi::approval::ApprovalPolicy {
        mode: zenpi::approval::ApprovalMode::Headless,
        trusted_tools: vec![],
        per_tool: Default::default(),
    });
    agent
        .set_worker_execution_binding(Some(worker_binding()))
        .unwrap();
    agent.process_sync("mutate").unwrap();
    assert_eq!(invocations.load(Ordering::SeqCst), 0);
    let metadata = agent
        .history()
        .iter()
        .find(|turn| turn.role == TurnRole::Tool)
        .unwrap()
        .metadata
        .as_ref()
        .unwrap();
    assert_eq!(metadata["outcome"], "denied");
    assert_eq!(metadata["policy_digest"], worker_binding().policy_digest);
    assert!(agent.unknown_tool_outcomes().is_empty());
}

struct DisruptJournalTool {
    journal: std::path::PathBuf,
    backup: std::path::PathBuf,
}

impl Tool for DisruptJournalTool {
    fn definition(&self) -> ToolDefinition {
        ConstantTool.definition()
    }
    fn invoke(&self, _: &ToolContext, _: &Map<String, Value>) -> Result<Value, ToolError> {
        std::fs::rename(&self.journal, &self.backup)?;
        std::fs::create_dir(&self.journal)?;
        Ok(json!({"executed": true}))
    }
}

#[test]
fn dispatch_marker_recovers_unknown_outcome_when_result_and_error_persistence_fail() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("storage-failure.jsonl");
    let backup = dir.path().join("storage-backup.jsonl");
    let (mut agent, requests) = scripted_agent(&path, vec![tool_completion(&["a"])]);
    let mut registry = ToolRegistry::new();
    registry
        .register(DisruptJournalTool {
            journal: path.clone(),
            backup: backup.clone(),
        })
        .unwrap();
    agent.set_tools(
        registry,
        ToolContext::new(dir.path()).unwrap(),
        SideEffectPolicy::read_only(),
    );
    assert!(matches!(
        agent.process_sync("inspect"),
        Err(AgentError::Session(_))
    ));
    assert_eq!(agent.phase(), zenpi::core::AgentPhase::Idle);
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(backup, &path).unwrap();
    let mut restarted = Agent::with_echo(SessionStore::open(&path).unwrap());
    let pending = restarted.unknown_tool_outcomes();
    assert_eq!(pending.len(), 1);
    assert!(matches!(
        restarted.process_sync("continue"),
        Err(AgentError::Recovery(_))
    ));
    restarted
        .resolve_tool_outcome(
            &pending[0].operation_id,
            zenpi::core::ToolRecoveryDecision::Abandon,
        )
        .unwrap();
    let results: Vec<_> = restarted
        .history()
        .iter()
        .filter(|turn| turn.role == TurnRole::Tool)
        .collect();
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].metadata.as_ref().unwrap()["outcome"],
        "unknown_outcome"
    );
    assert!(results[0].content.contains("no automatic retry"));
    drop(restarted);
    let reopened = Agent::with_echo(SessionStore::open(&path).unwrap());
    assert!(reopened.unknown_tool_outcomes().is_empty());
}
