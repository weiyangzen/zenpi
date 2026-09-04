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

fn wait_for_pending(coordinator: &zenpi::approval::ApprovalCoordinator) {
    for _ in 0..500 {
        if coordinator.is_pending("approval-owner") {
            return;
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
    wait_for_pending(&coordinator);

    coordinator
        .respond(ApprovalResponse {
            request_id: "approval-owner".into(),
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
            && event.get("request_id").and_then(serde_json::Value::as_str) == Some("approval-owner")
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
    wait_for_pending(&coordinator);

    coordinator
        .respond(ApprovalResponse {
            request_id: "approval-owner".into(),
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
}
