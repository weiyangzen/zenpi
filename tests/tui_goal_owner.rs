use tempfile::tempdir;
use zenpi::{
    b3::ResourceBudget,
    core::Agent,
    domain_store::{DomainStore, path_for_session},
    domains::{Blueprint, BlueprintItem, Goal, GoalStatus},
    session::SessionStore,
    slash::SlashCommand,
    tui::{MessageRole, SlashDispatchAction, TuiState, dispatch_slash_command},
};

fn seed_goal(session_path: &std::path::Path) {
    let blueprint = Blueprint::new(
        "tui-owner-plan",
        "1",
        vec![BlueprintItem::new("build", 100)],
    )
    .unwrap();
    let goal = Goal::new(
        "tui-owner-goal",
        &blueprint,
        ResourceBudget::default(),
        None,
    )
    .unwrap();
    let mut store = DomainStore::open(path_for_session(session_path)).unwrap();
    store.put_blueprint(blueprint).unwrap();
    store.put_goal(goal).unwrap();
}

fn last_system_text(state: &TuiState) -> String {
    state
        .messages()
        .filter(|message| message.role == MessageRole::System)
        .last()
        .map(|message| message.text.clone())
        .unwrap_or_default()
}

#[test]
fn tui_recovery_uses_the_shared_owner_without_retrying_work() {
    use zenpi::session::{InterruptedOperation, OperationKind};
    use zenpi::slash::RecoveryAction;
    let dir = tempdir().unwrap();
    let mut session = SessionStore::open(dir.path().join("recover.jsonl")).unwrap();
    session
        .begin_operation(&InterruptedOperation {
            operation_id: "uncertain".into(),
            kind: OperationKind::Provider,
            turn_id: "turn-1".into(),
            retry_requires_confirmation: true,
        })
        .unwrap();
    let mut agent = Agent::with_echo(session);
    let mut state = TuiState::default();
    dispatch_slash_command(
        SlashCommand::Recovery {
            action: RecoveryAction::Inspect,
        },
        &mut state,
        Some(&mut agent),
    );
    assert!(last_system_text(&state).contains("uncertain"));
    dispatch_slash_command(
        SlashCommand::Recovery {
            action: RecoveryAction::Abandon {
                operation_id: "uncertain".into(),
            },
        },
        &mut state,
        Some(&mut agent),
    );
    assert!(agent.operation_recovery().is_empty());
    assert!(agent.history().is_empty());
    let response = last_system_text(&state);
    assert!(response.contains("execution_started"));
    assert!(!response.contains("\"execution_started\":true"));
}

#[test]
fn tui_goal_owner_actions_match_headless_and_persist_status() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.jsonl");
    seed_goal(&session_path);
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let mut state = TuiState::default();

    assert_eq!(
        dispatch_slash_command(
            SlashCommand::Goal {
                instruction: "show tui-owner-goal".into(),
            },
            &mut state,
            Some(&mut agent),
        ),
        SlashDispatchAction::Continue
    );
    let shown = last_system_text(&state);
    assert!(shown.contains("tui-owner-goal"));
    assert!(shown.contains("queued"));

    dispatch_slash_command(
        SlashCommand::Goal {
            instruction: "status tui-owner-goal running".into(),
        },
        &mut state,
        Some(&mut agent),
    );
    let started = last_system_text(&state);
    assert!(started.contains("goal status:"));
    assert!(started.contains("running"));

    dispatch_slash_command(
        SlashCommand::Goal {
            instruction: "transition tui-owner-goal done".into(),
        },
        &mut state,
        Some(&mut agent),
    );
    let finished = last_system_text(&state);
    assert!(finished.contains("done"));

    let persisted = DomainStore::open(path_for_session(&session_path)).unwrap();
    assert_eq!(
        persisted.goal("tui-owner-goal").unwrap().status,
        GoalStatus::Done
    );
    assert!(
        agent.history().is_empty(),
        "owner actions must not become turns"
    );
}

#[test]
fn tui_goal_create_uses_shared_owner_and_persists_queued_goal() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.jsonl");
    let blueprint = Blueprint::new(
        "tui-create-plan",
        "1",
        vec![BlueprintItem::new("build", 100)],
    )
    .unwrap();
    let mut store = DomainStore::open(path_for_session(&session_path)).unwrap();
    store.put_blueprint(blueprint).unwrap();
    drop(store);
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let mut state = TuiState::default();
    dispatch_slash_command(
        SlashCommand::Goal {
            instruction: "create tui-created-goal tui-create-plan@1".into(),
        },
        &mut state,
        Some(&mut agent),
    );
    assert!(last_system_text(&state).contains("goal create:"));
    let persisted = DomainStore::open(path_for_session(&session_path)).unwrap();
    assert_eq!(
        persisted.goal("tui-created-goal").unwrap().status,
        GoalStatus::Queued
    );
}

#[test]
fn tui_plan_creates_sequential_blueprint_without_provider_turn() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("plan-session.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let mut state = TuiState::default();
    dispatch_slash_command(
        SlashCommand::Plan {
            instruction: Some("tui-plan :: inspect; test; publish".into()),
        },
        &mut state,
        Some(&mut agent),
    );
    assert!(last_system_text(&state).contains("plan created:"));
    assert!(agent.history().is_empty());
    let store = DomainStore::open(path_for_session(&session_path)).unwrap();
    let blueprint = store.blueprint("tui-plan", "1").unwrap();
    assert_eq!(blueprint.items.len(), 3);
    assert_eq!(blueprint.items[2].depends_on, vec!["step-2"]);
}

#[test]
fn tui_goal_owner_rejects_unknown_status_and_preserves_natural_language_refusal() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.jsonl");
    seed_goal(&session_path);
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let mut state = TuiState::default();

    dispatch_slash_command(
        SlashCommand::Goal {
            instruction: "status tui-owner-goal exploding".into(),
        },
        &mut state,
        Some(&mut agent),
    );
    assert!(state.messages().any(|message| {
        message.role == MessageRole::Error && message.text.contains("goal status must be")
    }));

    dispatch_slash_command(
        SlashCommand::Goal {
            instruction: "ship this feature".into(),
        },
        &mut state,
        Some(&mut agent),
    );
    assert!(state.messages().any(|message| {
        message.role == MessageRole::Error
            && message
                .text
                .contains("goal creation/execution requires an external b3ehive owner")
    }));
    assert!(agent.history().is_empty());
}

#[test]
fn tui_goal_run_resume_and_cancel_aliases_persist() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.jsonl");
    seed_goal(&session_path);
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let mut state = TuiState::default();
    for instruction in [
        "run tui-owner-goal",
        "status tui-owner-goal paused",
        "resume tui-owner-goal",
        "cancel tui-owner-goal",
    ] {
        dispatch_slash_command(
            SlashCommand::Goal {
                instruction: instruction.into(),
            },
            &mut state,
            Some(&mut agent),
        );
    }
    assert_eq!(
        DomainStore::open(path_for_session(&session_path))
            .unwrap()
            .goal("tui-owner-goal")
            .unwrap()
            .status,
        GoalStatus::Cancelled
    );
    assert!(agent.history().is_empty());
}
