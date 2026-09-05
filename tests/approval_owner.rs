use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use tempfile::tempdir;
use zenpi::{
    approval::{ApprovalDecision, ApprovalMode, ApprovalPolicy, ApprovalResponse},
    backend::{Backend, BackendError, Completion, CompletionRequest},
    core::{Agent, TurnInputRequest},
    session::SessionStore,
    slash::{ApproveDecision, SlashCommand},
    tools::{SideEffectPolicy, ToolCall, ToolContext, ToolRegistry},
    tui::{MessageRole, SlashDispatchAction, TuiState, dispatch_slash_command},
};

#[derive(Clone)]
struct ApprovalBackend {
    calls: Arc<AtomicUsize>,
}

impl Backend for ApprovalBackend {
    fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        match self.calls.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(Completion {
                content: String::new(),
                usage: None,
                model: None,
                tool_calls: vec![ToolCall {
                    id: "owner".into(),
                    name: "write_file".into(),
                    arguments: serde_json::json!({
                        "path": "note.txt",
                        "content": "approved",
                    }),
                }],
                response_id: None,
                refusal: None,
                annotations: Vec::new(),
            }),
            _ => Ok(Completion::text("done")),
        }
    }

    fn name(&self) -> &str {
        "approval-owner-fixture"
    }
}

fn configured_agent(workspace: &std::path::Path) -> Agent {
    let session = SessionStore::open(workspace.join("approval.jsonl")).unwrap();
    let mut agent = Agent::new(
        session,
        Box::new(ApprovalBackend {
            calls: Arc::new(AtomicUsize::new(0)),
        }),
    );
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(workspace).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    agent.set_approval_policy(ApprovalPolicy {
        mode: ApprovalMode::Always,
        ..ApprovalPolicy::default()
    });
    agent
}

fn wait_for_pending(
    coordinator: &zenpi::approval::ApprovalCoordinator,
) -> zenpi::approval::ApprovalRequest {
    for _ in 0..500 {
        if let Some(request) = coordinator.drain_pending().pop() {
            return request;
        }
        thread::sleep(Duration::from_millis(2));
    }
    panic!("approval request did not become pending");
}

#[test]
fn tui_approval_owner_rejects_missing_coordinator() {
    let directory = tempdir().unwrap();
    let mut agent =
        Agent::with_echo(SessionStore::open(directory.path().join("no-tools.jsonl")).unwrap());
    let mut state = TuiState::default();

    assert_eq!(
        dispatch_slash_command(
            SlashCommand::Approve {
                id: "approval-missing".into(),
                decision: ApproveDecision::Once,
            },
            &mut state,
            Some(&mut agent),
        ),
        SlashDispatchAction::Continue
    );
    assert!(state.messages().any(|message| {
        message.role == MessageRole::Error
            && message
                .text
                .contains("no tool registry or approval coordinator")
    }));
}

#[test]
fn denied_pending_request_is_durable_and_has_no_side_effect() {
    let workspace = tempdir().unwrap();
    let agent = configured_agent(workspace.path());
    let coordinator = agent.approval_coordinator().unwrap();
    let (tx, rx) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let mut agent = agent;
        let result = agent.process(TurnInputRequest::new("write it"));
        tx.send((agent, result)).unwrap();
    });
    let request = wait_for_pending(&coordinator);
    let preview = request
        .preview
        .as_ref()
        .expect("write approval must include a preview before the decision");
    assert!(matches!(
        preview,
        zenpi::tools::ToolPreview::Diff {
            path,
            patch,
            changed: true,
            ..
        } if path == "note.txt" && patch.contains("+approved\n")
    ));

    coordinator
        .respond(ApprovalResponse {
            request_id: request.request_id.clone(),
            decision: ApprovalDecision::Deny,
            remember: false,
        })
        .unwrap();

    let (agent, result) = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    result.unwrap();
    worker.join().unwrap();
    assert!(!workspace.path().join("note.txt").exists());
    assert!(agent.session().events().iter().any(|event| {
        event.get("type").and_then(serde_json::Value::as_str) == Some("approval_resolved")
            && event.get("request_id").and_then(serde_json::Value::as_str)
                == Some(request.request_id.as_str())
            && event.get("decision").and_then(serde_json::Value::as_str) == Some("deny")
    }));
}

#[test]
fn remembered_allow_is_durable_and_executes_once() {
    let workspace = tempdir().unwrap();
    let agent = configured_agent(workspace.path());
    let coordinator = agent.approval_coordinator().unwrap();
    let (tx, rx) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let mut agent = agent;
        let result = agent.process(TurnInputRequest::new("write it"));
        tx.send((agent, result)).unwrap();
    });
    let request = wait_for_pending(&coordinator);

    coordinator
        .respond(ApprovalResponse {
            request_id: request.request_id.clone(),
            decision: ApprovalDecision::Allow,
            remember: true,
        })
        .unwrap();

    let (agent, result) = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    result.unwrap();
    worker.join().unwrap();
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("note.txt")).unwrap(),
        "approved"
    );
    let resolved = agent
        .session()
        .events()
        .iter()
        .find(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("approval_resolved")
        })
        .unwrap();
    assert_eq!(resolved["decision"], "allow");
    assert_eq!(resolved["remember"], true);
    assert_eq!(resolved["policy_digest"], request.policy_digest.unwrap());
    assert_eq!(resolved["preview"]["kind"], "diff");
    assert_eq!(resolved["preview"]["path"], "note.txt");
    assert_eq!(resolved["preview"]["after_bytes"], 8);
    assert!(resolved["preview"].get("patch").is_none());
}

fn gated_agent(root: &std::path::Path, denied: bool) -> (Agent, zenpi::tools::BlueprintRevocation) {
    use zenpi::{
        core::WorkerExecutionBinding,
        tools::{BlueprintGate, BlueprintLease, BlueprintPolicySpec},
    };
    let context = ToolContext::new(root).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let (gate, revoke) = BlueprintGate::compile(
        &context,
        BlueprintPolicySpec {
            blueprint_digest: "a".repeat(64),
            goal_digest: "b".repeat(64),
            item_id: "CF-405".into(),
            allowed_tools: ["write_file".into()].into(),
            readable_paths: [".".into()].into(),
            writable_paths: [".".into()].into(),
            denied_paths: if denied {
                ["note.txt".into()].into()
            } else {
                Default::default()
            },
            protected_paths: Default::default(),
            allowed_commands: Default::default(),
            denied_commands: Default::default(),
            network_hosts: Default::default(),
            max_actions: 10,
            max_command_timeout_ms: 1000,
            max_command_output_bytes: 1024,
        },
        BlueprintLease {
            lease_id: "lease-405".into(),
            issued_at_ms: now,
            expires_at_ms: now + 30_000,
        },
        now,
    )
    .unwrap();
    let evidence = gate.evidence();
    let mut agent = configured_agent(root);
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        context.with_blueprint_gate(gate).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    agent
        .set_worker_execution_binding(Some(WorkerExecutionBinding {
            blueprint_id: "blueprint".into(),
            blueprint_sha256: evidence.blueprint_digest,
            goal_id: "goal".into(),
            item_id: evidence.item_id,
            lease_id: evidence.lease_id,
            policy_digest: evidence.policy_digest,
            expires_at_ms: evidence.expires_at_ms,
        }))
        .unwrap();
    agent.set_approval_policy(ApprovalPolicy {
        mode: ApprovalMode::WorkerAllowAfterPreflight,
        ..Default::default()
    });
    (agent, revoke)
}

#[test]
fn worker_allow_requires_a_live_matched_gate_and_records_its_digest() {
    let root = tempdir().unwrap();
    let (mut agent, _) = gated_agent(root.path(), false);
    agent.process_sync("write").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.path().join("note.txt")).unwrap(),
        "approved"
    );
    assert!(
        agent
            .approval_coordinator()
            .unwrap()
            .drain_pending()
            .is_empty()
    );
    let event = agent
        .session()
        .events()
        .iter()
        .find(|event| event["type"] == "approval_resolved")
        .unwrap();
    assert_eq!(event["source"], "worker_allow_after_preflight");
    assert_eq!(event["execution"]["prohibition_gate_enforced"], true);
    assert_eq!(
        event["policy_digest"],
        event["execution"]["worker_binding"]["policy_digest"]
    );
    assert_eq!(event["remember"], false);
}

#[test]
fn worker_allow_cannot_override_prohibitions_revocation_or_missing_preflight() {
    for scenario in ["denied_path", "revoked", "missing_gate", "explicit_deny"] {
        let root = tempdir().unwrap();
        let mut agent = if scenario == "missing_gate" {
            configured_agent(root.path())
        } else {
            let (agent, revoke) = gated_agent(root.path(), scenario == "denied_path");
            if scenario == "revoked" {
                revoke.revoke();
            }
            agent
        };
        agent.set_approval_policy(ApprovalPolicy {
            mode: ApprovalMode::WorkerAllowAfterPreflight,
            per_tool: [(
                "write_file".into(),
                if scenario == "explicit_deny" {
                    ApprovalDecision::Deny
                } else {
                    ApprovalDecision::Allow
                },
            )]
            .into(),
            ..Default::default()
        });
        agent.process_sync("write").unwrap();
        assert!(!root.path().join("note.txt").exists(), "{scenario}");
        assert!(
            agent
                .approval_coordinator()
                .unwrap()
                .drain_pending()
                .is_empty()
        );
        assert!(
            !agent
                .session()
                .events()
                .iter()
                .any(|event| event["type"] == "tool_execution_started")
        );
    }
}

#[test]
fn request_identity_is_session_scoped_and_responses_carry_policy() {
    let mut prior: Option<String> = None;
    for _ in 0..2 {
        let root = tempdir().unwrap();
        let agent = configured_agent(root.path());
        let coordinator = agent.approval_coordinator().unwrap();
        let worker = thread::spawn(move || {
            let mut agent = agent;
            agent.process_sync("write").unwrap();
            agent
        });
        let request = wait_for_pending(&coordinator);
        assert_eq!(request.policy_digest.as_ref().unwrap().len(), 64);
        if let Some(prior) = &prior {
            assert_ne!(prior, &request.request_id);
            assert!(
                coordinator
                    .respond(ApprovalResponse {
                        request_id: prior.clone(),
                        decision: ApprovalDecision::Allow,
                        remember: false
                    })
                    .is_err()
            );
        }
        let accepted = coordinator
            .respond_with_request(ApprovalResponse {
                request_id: request.request_id.clone(),
                decision: ApprovalDecision::Deny,
                remember: false,
            })
            .unwrap();
        assert_eq!(accepted.policy_digest, request.policy_digest);
        prior = Some(request.request_id);
        worker.join().unwrap();
    }
}

#[test]
fn host_emergency_cancel_denies_pending_approval_without_a_write() {
    let root = tempdir().unwrap();
    let agent = configured_agent(root.path());
    let coordinator = agent.approval_coordinator().unwrap();
    let worker = thread::spawn(move || {
        let mut agent = agent;
        let result = agent.process_sync("write");
        (agent, result)
    });
    let request = wait_for_pending(&coordinator);
    coordinator.emergency_cancel();
    let (_, result) = worker.join().unwrap();
    assert!(result.is_err());
    assert!(!root.path().join("note.txt").exists());
    assert!(!coordinator.is_pending(&request.request_id));
}

#[cfg(unix)]
#[test]
fn host_emergency_cancel_stops_and_reaps_an_already_allowed_command() {
    struct CommandBackend;
    impl Backend for CommandBackend {
        fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
            let mut completion = Completion::text("");
            completion.tool_calls.push(ToolCall { id: "process".into(), name: "run_command".into(), arguments: serde_json::json!({
                "command": "printf started > started; (sleep 0.5; printf survived > survived) & wait", "timeout_ms": 5000
            }) });
            Ok(completion)
        }
        fn name(&self) -> &str {
            "command-cancel-fixture"
        }
    }
    let root = tempdir().unwrap();
    let mut agent = Agent::new(
        SessionStore::open(root.path().join("session.jsonl")).unwrap(),
        Box::new(CommandBackend),
    );
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(root.path()).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    agent.set_approval_policy(ApprovalPolicy {
        mode: ApprovalMode::TrustedWorkspace,
        trusted_tools: vec!["run_command".into()],
        ..Default::default()
    });
    let host = agent.approval_coordinator().unwrap();
    let worker = thread::spawn(move || agent.process_sync("run"));
    for _ in 0..500 {
        if root.path().join("started").exists() {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(root.path().join("started").exists());
    host.emergency_cancel();
    assert!(worker.join().unwrap().is_err());
    thread::sleep(Duration::from_millis(600));
    assert!(
        !root.path().join("survived").exists(),
        "descendant survived host cancellation"
    );
}
