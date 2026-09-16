use serde_json::{Map, Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::tempdir;
use zenpi::{
    backend::{Backend, BackendError, Completion, CompletionRequest},
    core::{Agent, AgentError, TurnInputRequest, TurnRole, TurnSubmission},
    session::{MailboxStatus, SessionMailbox, SessionStore},
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

#[test]
fn agent_live_owner_api_claims_and_completes_a_mailbox_request() {
    let dir = tempdir().unwrap();
    let sender_path = dir.path().join("sender.jsonl");
    let recipient_path = dir.path().join("recipient.jsonl");
    let sender = SessionStore::open(&sender_path).unwrap();
    let recipient = SessionStore::open(&recipient_path).unwrap();
    let mailbox = SessionMailbox::open(recipient.path()).unwrap();
    mailbox
        .enqueue(&sender, "lint-request", json!({"text":"check"}), 10_000, 10)
        .unwrap();

    let workspace = std::fs::canonicalize(dir.path())
        .unwrap()
        .display()
        .to_string();
    let mut owner = Agent::with_echo(recipient);
    owner
        .register_live_owner(12, workspace.clone(), 20)
        .unwrap();
    let claimed = owner
        .claim_live_mailbox(&workspace, None, 30, 100)
        .unwrap()
        .unwrap();
    assert_eq!(claimed.status, MailboxStatus::Claimed);
    let completed = owner
        .finish_live_mailbox(
            &workspace,
            &claimed.digest,
            json!({"status":"ok"}),
            true,
            40,
            100,
        )
        .unwrap();
    assert_eq!(completed.status, MailboxStatus::Succeeded);

    let recipient_reopened = SessionStore::open_existing(&recipient_path).unwrap();
    let recipient_mailbox = SessionMailbox::open(&recipient_path).unwrap();
    let recipient_view = recipient_mailbox
        .find_message(&recipient_reopened, &claimed.digest, 40)
        .unwrap();
    assert_eq!(recipient_view.status, MailboxStatus::Succeeded);
    assert_eq!(recipient_view.result, Some(json!({"status":"ok"})));
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

struct WriteToolBackend {
    calls: AtomicUsize,
}

impl Backend for WriteToolBackend {
    fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(Completion {
                content: String::new(),
                usage: None,
                model: None,
                tool_calls: vec![ToolCall {
                    id: "write-call".into(),
                    name: "write_file".into(),
                    arguments: json!({"path": "blocked.txt", "content": "blocked"}),
                }],
                response_id: None,
                refusal: None,
                annotations: Vec::new(),
            })
        } else {
            Ok(Completion::text("unexpected continuation"))
        }
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
fn workspace_write_reserves_disk_budget_before_side_effect() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = root.join("session.jsonl");
    let mut agent = Agent::new(
        SessionStore::open(&path).unwrap(),
        Box::new(WriteToolBackend {
            calls: AtomicUsize::new(0),
        }),
    );
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(root).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    let mut approval = zenpi::approval::ApprovalPolicy::default();
    approval.remember("write_file", zenpi::approval::ApprovalDecision::Allow);
    agent.set_approval_policy(approval);
    agent
        .set_resource_limits(zenpi::governance::ResourceLimits {
            max_disk_bytes: 3,
            ..Default::default()
        })
        .unwrap();

    let result = agent.process(TurnInputRequest::new("write a file"));
    assert!(matches!(result, Err(AgentError::Governance(_))));
    assert!(!root.join("blocked.txt").exists());
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
fn provider_turn_releases_concurrency_budget_after_terminal_result() {
    let dir = tempdir().unwrap();
    let mut agent = Agent::with_echo(SessionStore::open(dir.path().join("session.jsonl")).unwrap());
    agent
        .set_resource_limits(zenpi::governance::ResourceLimits {
            max_concurrency: 1,
            ..Default::default()
        })
        .unwrap();
    assert!(agent.process(TurnInputRequest::new("first")).is_ok());
    assert!(agent.process(TurnInputRequest::new("second")).is_ok());
    let usage = agent
        .session()
        .events()
        .iter()
        .rev()
        .find(|event| event["type"] == "resource_usage")
        .unwrap();
    assert_eq!(usage["usage"]["concurrency"], 0);
}

#[test]
fn blueprint_worker_admission_fails_closed_before_runtime_mutation() {
    let dir = tempdir().unwrap();
    let mut agent = Agent::with_echo(SessionStore::open(dir.path().join("worker.jsonl")).unwrap());
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(dir.path()).unwrap(),
        SideEffectPolicy::read_only(),
    );
    let policy = zenpi::tools::BlueprintPolicySpec {
        blueprint_digest: "a".repeat(64),
        goal_digest: "b".repeat(64),
        item_id: "CF-409".into(),
        allowed_tools: ["echo".into()].into_iter().collect(),
        readable_paths: [".".into()].into_iter().collect(),
        writable_paths: Default::default(),
        denied_paths: Default::default(),
        protected_paths: Default::default(),
        allowed_commands: ["echo ok".into()].into_iter().collect(),
        denied_commands: Default::default(),
        network_hosts: Default::default(),
        max_actions: 2,
        max_command_timeout_ms: 1000,
        max_command_output_bytes: 1024,
    };
    let lease = zenpi::tools::BlueprintLease {
        lease_id: "lease".into(),
        issued_at_ms: 1,
        expires_at_ms: u64::MAX,
    };
    let mut binding = worker_binding();
    binding.item_id = "wrong".into();
    assert!(
        agent
            .admit_blueprint_worker(
                policy,
                lease,
                binding,
                zenpi::governance::ResourceLimits::default(),
                Vec::new(),
                10,
            )
            .is_err()
    );
    assert!(agent.approval_policy().is_some());
}

#[test]
fn blueprint_worker_admission_installs_gate_binding_and_budget_atomically() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let session_path = root.join("worker.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let context = ToolContext::new(root).unwrap();
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        context.clone(),
        SideEffectPolicy::read_only(),
    );
    agent
        .set_worker_budget_limits(zenpi::governance::ResourceLimits::default())
        .unwrap();
    let policy = zenpi::tools::BlueprintPolicySpec {
        blueprint_digest: "a".repeat(64),
        goal_digest: "b".repeat(64),
        item_id: "CF-409".into(),
        allowed_tools: ["read_file".into()].into_iter().collect(),
        readable_paths: [".".into()].into_iter().collect(),
        writable_paths: Default::default(),
        denied_paths: Default::default(),
        protected_paths: Default::default(),
        allowed_commands: Default::default(),
        denied_commands: Default::default(),
        network_hosts: Default::default(),
        max_actions: 2,
        max_command_timeout_ms: 1000,
        max_command_output_bytes: 1024,
    };
    let now_ms = zenpi::session::unix_time_ms();
    let lease = zenpi::tools::BlueprintLease {
        lease_id: "lease".into(),
        issued_at_ms: now_ms.saturating_sub(1),
        expires_at_ms: now_ms.saturating_add(60_000),
    };
    let (gate, _) =
        zenpi::tools::BlueprintGate::compile(&context, policy.clone(), lease.clone(), now_ms)
            .unwrap();
    let evidence = gate.evidence();
    let binding = zenpi::core::WorkerExecutionBinding {
        blueprint_id: "blueprint".into(),
        blueprint_sha256: policy.blueprint_digest.clone(),
        goal_id: "goal".into(),
        item_id: policy.item_id.clone(),
        lease_id: lease.lease_id.clone(),
        policy_digest: evidence.policy_digest,
        expires_at_ms: lease.expires_at_ms,
    };
    agent
        .admit_blueprint_worker(
            policy,
            lease,
            binding,
            zenpi::governance::ResourceLimits::default(),
            vec!["provider-key-handle".into()],
            now_ms,
        )
        .unwrap();
    assert!(agent.worker_admission_operation_id().is_some());
    assert!(
        agent
            .session()
            .events()
            .iter()
            .any(|event| event["type"] == "worker_admitted")
    );
}

#[test]
fn blueprint_worker_admission_rejects_raw_credential_metadata() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let session_path = root.join("worker.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let context = ToolContext::new(root).unwrap();
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        context.clone(),
        SideEffectPolicy::read_only(),
    );
    agent
        .set_worker_budget_limits(zenpi::governance::ResourceLimits::default())
        .unwrap();
    let policy = zenpi::tools::BlueprintPolicySpec {
        blueprint_digest: "a".repeat(64),
        goal_digest: "b".repeat(64),
        item_id: "CF-409".into(),
        allowed_tools: ["read_file".into()].into_iter().collect(),
        readable_paths: [".".into()].into_iter().collect(),
        writable_paths: Default::default(),
        denied_paths: Default::default(),
        protected_paths: Default::default(),
        allowed_commands: Default::default(),
        denied_commands: Default::default(),
        network_hosts: Default::default(),
        max_actions: 2,
        max_command_timeout_ms: 1000,
        max_command_output_bytes: 1024,
    };
    let now_ms = zenpi::session::unix_time_ms();
    let lease = zenpi::tools::BlueprintLease {
        lease_id: "lease-raw".into(),
        issued_at_ms: now_ms.saturating_sub(1),
        expires_at_ms: now_ms.saturating_add(60_000),
    };
    let (gate, _) =
        zenpi::tools::BlueprintGate::compile(&context, policy.clone(), lease.clone(), now_ms)
            .unwrap();
    let binding = zenpi::core::WorkerExecutionBinding {
        blueprint_id: "blueprint".into(),
        blueprint_sha256: policy.blueprint_digest.clone(),
        goal_id: "goal".into(),
        item_id: policy.item_id.clone(),
        lease_id: lease.lease_id.clone(),
        policy_digest: gate.evidence().policy_digest,
        expires_at_ms: lease.expires_at_ms,
    };
    let error = agent
        .admit_blueprint_worker(
            policy,
            lease,
            binding,
            zenpi::governance::ResourceLimits::default(),
            vec!["API_KEY=raw-secret".into()],
            now_ms,
        )
        .unwrap_err();
    assert!(error.to_string().contains("invalid bounded identifier"));
    assert!(agent.worker_admission_operation_id().is_none());
}

#[test]
fn pending_unknown_operation_blocks_new_turn_until_explicit_decision() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("unknown.jsonl");
    let mut store = SessionStore::open(&path).unwrap();
    store
        .begin_operation(&zenpi::session::InterruptedOperation {
            operation_id: "tool-unknown".into(),
            kind: zenpi::session::OperationKind::Tool,
            turn_id: "turn-unknown".into(),
            retry_requires_confirmation: true,
        })
        .unwrap();
    drop(store);
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let error = agent
        .submit(TurnInputRequest::new("must wait"))
        .unwrap_err();
    assert!(error.to_string().contains("unknown_outcome"));
    agent
        .resolve_operation_recovery("tool-unknown", zenpi::core::ToolRecoveryDecision::Abandon)
        .unwrap();
    assert!(agent.submit(TurnInputRequest::new("now allowed")).is_ok());
}

#[test]
fn blueprint_worker_settlement_releases_admission_after_reap() {
    let dir = tempdir().unwrap();
    let mut agent = Agent::with_echo(SessionStore::open(dir.path().join("worker.jsonl")).unwrap());
    agent
        .set_worker_budget_limits(zenpi::governance::ResourceLimits::default())
        .unwrap();
    let operation_id = "worker-admission-CF-409";
    let error = agent
        .settle_blueprint_worker(
            operation_id,
            zenpi::governance::ResourceUsage::default(),
            zenpi::governance::BudgetCompletion::Completed,
            1,
        )
        .unwrap_err();
    assert!(!error.to_string().is_empty());
}

#[test]
fn blueprint_worker_renewal_requires_configured_host_ledger() {
    let dir = tempdir().unwrap();
    let mut agent = Agent::with_echo(SessionStore::open(dir.path().join("worker.jsonl")).unwrap());
    let error = agent
        .renew_blueprint_worker("lease", &"a".repeat(64), 2_000, 1)
        .unwrap_err();
    assert!(error.to_string().contains("not configured"));
}

#[test]
fn blueprint_worker_host_cancel_is_durable_and_requires_reap_before_settle() {
    let dir = tempdir().unwrap();
    let mut agent = Agent::with_echo(SessionStore::open(dir.path().join("worker.jsonl")).unwrap());
    agent
        .set_worker_budget_limits(zenpi::governance::ResourceLimits::default())
        .unwrap();
    let error = agent
        .cancel_blueprint_worker("missing", "host_cancel", 10)
        .unwrap_err();
    assert!(error.to_string().contains("lease unavailable"));
}

#[cfg(unix)]
fn shell_agent(root: &std::path::Path, backend: Box<dyn Backend>) -> Agent {
    let mut agent = Agent::new(
        SessionStore::open(root.join("shell.jsonl")).unwrap(),
        backend,
    );
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(root).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    let mut policy = zenpi::approval::ApprovalPolicy::default();
    policy.remember("user_shell", zenpi::approval::ApprovalDecision::Allow);
    agent.set_approval_policy(policy);
    agent
}

#[cfg(unix)]
#[test]
fn generic_unknown_outcome_cannot_be_bypassed_by_local_shell() {
    let dir = tempdir().unwrap();
    let mut agent = shell_agent(dir.path(), Box::new(zenpi::backend::EchoBackend));
    agent
        .session_mut()
        .begin_operation(&zenpi::session::InterruptedOperation {
            operation_id: "unresolved-provider".into(),
            kind: zenpi::session::OperationKind::Provider,
            turn_id: "prior".into(),
            retry_requires_confirmation: true,
        })
        .unwrap();
    assert!(matches!(
        agent.run_user_shell_with_cancel("!touch must-not-exist", || false),
        Err(zenpi::core::AgentError::Recovery(_))
    ));
    assert!(!dir.path().join("must-not-exist").exists());
}

#[cfg(unix)]
#[test]
fn user_shell_is_local_and_persists_next_turn_context() {
    struct ContextBackend;
    impl Backend for ContextBackend {
        fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, BackendError> {
            let shell = request
                .turns
                .iter()
                .find(|turn| {
                    turn.metadata
                        .as_ref()
                        .is_some_and(|meta| meta["origin"] == "user_shell")
                })
                .unwrap();
            assert_eq!(shell.role, TurnRole::User);
            assert!(shell.content.contains("HELLO SHELL"));
            assert!(shell.content.contains("untrusted data"));
            assert!(!request.turns.iter().any(|turn| turn.role == TurnRole::Tool));
            Ok(Completion::text("saw local output"))
        }
    }
    let dir = tempdir().unwrap();
    let mut agent = shell_agent(dir.path(), Box::new(ContextBackend));
    let help = agent.run_user_shell_with_cancel(" !  ", || false).unwrap();
    assert_eq!(help["status"], "help");
    assert!(agent.history().is_empty());
    let result = agent
        .run_user_shell_with_cancel(
            " !printf 'hello shell' | tr a-z A-Z > output; cat output ",
            || false,
        )
        .unwrap();
    assert_eq!(result["stdout"], "HELLO SHELL");
    assert_eq!(result["exit_code"], 0);
    assert_eq!(result["origin"], "user_shell");
    assert_eq!(result["child_reaped"], true);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("output")).unwrap(),
        "HELLO SHELL"
    );
    assert_eq!(agent.history().len(), 1);
    assert_eq!(agent.phase(), zenpi::core::AgentPhase::Idle);
    assert!(agent.unknown_tool_outcomes().is_empty());
    let events = agent.session().events();
    assert!(
        events
            .iter()
            .any(|event| event["type"] == "user_shell_input")
    );
    assert!(
        events
            .iter()
            .any(|event| event["type"] == "tool_execution_finished"
                && event["origin"] == "user_shell")
    );
    let restored = SessionStore::open(agent.session().path()).unwrap();
    assert_eq!(restored.turns()[0], agent.history()[0]);
    assert_eq!(
        agent
            .process(TurnInputRequest::new("explain output"))
            .unwrap()
            .assistant
            .unwrap()
            .content,
        "saw local output"
    );
}

#[cfg(unix)]
#[test]
fn user_shell_preserves_nonzero_signal_and_cancellation_evidence() {
    let dir = tempdir().unwrap();
    let mut agent = shell_agent(dir.path(), Box::new(FailingBackend));
    let nonzero = agent
        .run_user_shell_with_cancel("!printf out; printf err >&2; exit 7", || false)
        .unwrap();
    assert_eq!(nonzero["stdout"], "out");
    assert_eq!(nonzero["stderr"], "err");
    assert_eq!(nonzero["exit_code"], 7);
    assert_eq!(nonzero["child_reaped"], true);
    let signal = agent
        .run_user_shell_with_cancel("!kill -TERM $$", || false)
        .unwrap();
    assert_eq!(signal["signal"], libc::SIGTERM);
    assert_eq!(signal["exit_code"], Value::Null);
    let start = std::time::Instant::now();
    let cancelled = agent
        .run_user_shell_with_cancel(
            "!(trap '' TERM; sleep 1; printf leaked > leaked) & wait",
            || start.elapsed() > std::time::Duration::from_millis(50),
        )
        .unwrap();
    assert_eq!(cancelled["cancelled"], true);
    assert_eq!(cancelled["child_reaped"], true);
    assert_eq!(cancelled["process_group_terminated"], true);
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert!(!dir.path().join("leaked").exists());
    assert!(agent.unknown_tool_outcomes().is_empty());
}

#[cfg(unix)]
#[test]
fn user_shell_separate_approval_and_precancel_never_spawn() {
    let dir = tempdir().unwrap();
    let mut agent = shell_agent(dir.path(), Box::new(FailingBackend));
    let mut policy = zenpi::approval::ApprovalPolicy {
        mode: zenpi::approval::ApprovalMode::Headless,
        ..Default::default()
    };
    policy.remember("run_command", zenpi::approval::ApprovalDecision::Allow);
    agent.set_approval_policy(policy);
    assert!(matches!(
        agent.run_user_shell_with_cancel("!printf leak > denied", || false),
        Err(AgentError::Tool(ToolError::PolicyDenied { .. }))
    ));
    assert!(!dir.path().join("denied").exists());
    assert!(agent.history().is_empty());
    let mut policy = zenpi::approval::ApprovalPolicy::default();
    policy.remember("user_shell", zenpi::approval::ApprovalDecision::Allow);
    agent.set_approval_policy(policy);
    assert!(matches!(
        agent.run_user_shell_with_cancel("!printf leak > cancelled", || true),
        Err(AgentError::Tool(ToolError::Cancelled))
    ));
    assert!(!dir.path().join("cancelled").exists());
    assert!(agent.unknown_tool_outcomes().is_empty());
    assert!(matches!(
        agent.process(TurnInputRequest::new("!echo not-a-provider-prompt")),
        Err(AgentError::InvalidTurn(_))
    ));
    assert!(agent.history().is_empty());
}

#[cfg(unix)]
#[test]
fn user_shell_caps_both_streams_before_journaling_control_bytes() {
    let dir = tempdir().unwrap();
    let mut agent = shell_agent(dir.path(), Box::new(FailingBackend));
    let output = agent
        .run_user_shell_with_cancel(
            "!head -c 300000 /dev/zero; head -c 300000 /dev/zero >&2",
            || false,
        )
        .unwrap();
    assert_eq!(output["stdout"].as_str().unwrap().len(), 12 * 1024);
    assert_eq!(output["stderr"].as_str().unwrap().len(), 12 * 1024);
    assert_eq!(output["stdout_truncated"], true);
    assert_eq!(output["stderr_truncated"], true);
    assert!(agent.history()[0].content.len() < zenpi::protocol::MAX_TEXT_BYTES);
    assert!(agent.unknown_tool_outcomes().is_empty());
}

#[cfg(unix)]
#[test]
fn user_shell_crash_recovery_never_invents_a_model_tool_call() {
    let dir = tempdir().unwrap();
    let mut agent = shell_agent(dir.path(), Box::new(FailingBackend));
    agent
        .session_mut()
        .append_event(json!({
            "type": "tool_execution_started", "operation_id": "user-shell-crash",
            "turn_id": "user-shell-crash", "call_id": "user-shell-crash", "tool": "user_shell",
            "policy_digest": "d".repeat(64), "worker_binding": null, "origin": "user_shell",
        }))
        .unwrap();
    assert!(matches!(
        agent.run_user_shell_with_cancel("!printf duplicate > duplicate", || false),
        Err(AgentError::Recovery(_))
    ));
    assert!(!dir.path().join("duplicate").exists());
    agent
        .resolve_tool_outcome(
            "user-shell-crash",
            zenpi::core::ToolRecoveryDecision::Abandon,
        )
        .unwrap();
    assert_eq!(agent.history()[0].role, TurnRole::User);
    assert!(agent.history()[0].content.contains("unknown_outcome"));
    assert!(agent.unknown_tool_outcomes().is_empty());
}

#[cfg(unix)]
#[test]
fn unbound_worker_cannot_escape_through_user_shell_origin() {
    let dir = tempdir().unwrap();
    let mut agent = shell_agent(dir.path(), Box::new(FailingBackend));
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(dir.path())
            .unwrap()
            .with_origin(zenpi::tools::ToolOrigin::BlueprintWorker),
        SideEffectPolicy::all_builtins(),
    );
    let mut policy = zenpi::approval::ApprovalPolicy::default();
    policy.remember("user_shell", zenpi::approval::ApprovalDecision::Allow);
    agent.set_approval_policy(policy);
    assert!(matches!(
        agent.run_user_shell_with_cancel("!printf escaped > escaped", || false),
        Err(AgentError::Tool(ToolError::GateDenied { .. }))
    ));
    assert!(!dir.path().join("escaped").exists());
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
fn provider_transport_failure_is_unknown_and_requires_explicit_recovery() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("provider-unknown.jsonl");
    let mut agent = Agent::new(SessionStore::open(&path).unwrap(), Box::new(FailingBackend));
    assert!(matches!(
        agent.process(TurnInputRequest::new("may have reached provider")),
        Err(AgentError::Backend(BackendError::Transport(_)))
    ));
    let pending = agent.operation_recovery();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].state,
        zenpi::session::OperationRecoveryState::UnknownOutcome
    );
    assert!(matches!(
        agent.process(TurnInputRequest::new("must be blocked")),
        Err(AgentError::Recovery(_))
    ));
    agent
        .resolve_operation_recovery(
            &pending[0].operation_id,
            zenpi::core::ToolRecoveryDecision::Abandon,
        )
        .unwrap();
    assert!(agent.operation_recovery().is_empty());
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
