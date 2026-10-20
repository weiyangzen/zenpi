use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::tempdir;
use zenpi::approval::ApprovalRequest;
use zenpi::core::{Agent, TurnInputRequest};
use zenpi::dag::DagStore;
use zenpi::layout::PaneId;
use zenpi::session::SessionStore;
use zenpi::tools::{ToolOrigin, ToolSideEffect};
use zenpi::tui::{
    AgentState, ApprovalLane, ProjectTabMetadata, TrajectoryLevel, TuiAction, TuiState,
};

fn request(id: &str) -> ApprovalRequest {
    ApprovalRequest {
        request_id: id.into(),
        turn_id: "turn".into(),
        call_id: format!("call-{id}"),
        tool: "run_command".into(),
        side_effect: ToolSideEffect::CommandExecution,
        arguments: serde_json::json!({"command": "ls -la"}),
        preview: None,
        origin: ToolOrigin::AgentTool,
        policy_digest: None,
        lease_id: None,
    }
}

fn plain(state: &mut TuiState, code: KeyCode) -> TuiAction {
    state.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// Screen text with the padding cells of wide (CJK) glyphs removed.
fn render(state: &mut TuiState, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn compact(screen: &str) -> String {
    screen.replace(' ', "")
}

#[test]
fn bare_y_in_the_prompt_selects_allow_instead_of_steering() {
    let mut state = TuiState::default();
    state.set_busy(true);
    state.present_approval(request("a"));
    // Esc leaves the review; the turn is still blocked on it.
    plain(&mut state, KeyCode::Esc);
    let screen = compact(&render(&mut state, 160, 40));
    assert!(screen.contains("等待审批"), "{screen}");
    assert!(!screen.contains("思考中"), "{screen}");

    state.set_input("y");
    assert_eq!(plain(&mut state, KeyCode::Enter), TuiAction::Redraw);
    assert_eq!(state.input(), "");
    assert!(render(&mut state, 160, 40).contains("❯ 1. Yes"));
    assert!(matches!(
        plain(&mut state, KeyCode::Enter),
        TuiAction::RespondApproval { allow: true, .. }
    ));
}

#[test]
fn bare_n_selects_deny_and_plain_text_still_steers() {
    let mut state = TuiState::default();
    state.set_busy(true);
    state.present_approval(request("a"));
    plain(&mut state, KeyCode::Esc);
    state.set_input(" No ");
    assert_eq!(plain(&mut state, KeyCode::Enter), TuiAction::Redraw);
    assert!(matches!(
        plain(&mut state, KeyCode::Enter),
        TuiAction::RespondApproval { allow: false, .. }
    ));

    // Anything else is an ordinary message, and so is "y" with nothing pending.
    state.retire_approval("a");
    state.set_input("y");
    assert_eq!(
        plain(&mut state, KeyCode::Enter),
        TuiAction::Submit("y".into())
    );
}

fn project_state(cwd: &std::path::Path) -> TuiState {
    let mut state = TuiState::default();
    state.set_active_project_metadata(ProjectTabMetadata {
        cwd: cwd.display().to_string(),
        ..ProjectTabMetadata::default()
    });
    state
}

#[test]
fn gantt_lists_the_dag_tree_and_opens_a_worker_trajectory() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let store = DagStore::at(root.join(".zenpi").join("dag.json"));
    store.upsert("plan", None).unwrap();
    store.upsert("api", Some("plan")).unwrap();
    store.upsert("ui", Some("plan")).unwrap();
    store
        .set_status("api", "green", Some("routes done"))
        .unwrap();
    // A worker that was assigned but whose process is gone.
    store.assign_worker("ui", "pid:4000000").unwrap();
    store.send("plan", "ui", "please add the form").unwrap();
    // The api worker left a real journal in the default worker directory.
    let workers = root.join(".zenpi").join("workers");
    std::fs::create_dir_all(&workers).unwrap();
    let mut worker = Agent::with_echo(SessionStore::open(workers.join("api.jsonl")).unwrap());
    worker
        .process(TurnInputRequest::new("implement the api"))
        .unwrap();

    let mut state = project_state(root);
    state.refresh_agents();
    assert_eq!(
        state.gantt_agents(),
        [
            ("master".to_owned(), AgentState::Idle),
            ("plan".to_owned(), AgentState::Open),
            ("api".to_owned(), AgentState::Done),
            ("ui".to_owned(), AgentState::Stale),
        ]
    );

    assert!(state.focus_workspace_pane(PaneId::Gantt));
    let screen = render(&mut state, 180, 48);
    assert!(screen.contains("Agents"), "{screen}");
    assert!(screen.contains("├ ✓ api"), "{screen}");
    assert!(screen.contains("└ ⚠ ui"), "{screen}");
    assert!(screen.contains("worker exited"), "{screen}");
    assert!(screen.contains("routes done"), "{screen}");
    assert!(screen.contains("✉1"), "{screen}");
    assert!(screen.contains("stale"), "{screen}");

    // Select api and open its trajectory from its journal.
    plain(&mut state, KeyCode::Down);
    plain(&mut state, KeyCode::Down);
    assert_eq!(state.gantt_agent_cursor(), 2);
    plain(&mut state, KeyCode::Enter);
    assert_eq!(state.trajectory_level(), Some(TrajectoryLevel::Records));
    let zoom = render(&mut state, 180, 48);
    assert!(zoom.contains("Agents › api"), "{zoom}");
    assert!(zoom.contains("implement the api"), "{zoom}");

    // Esc returns to the zoomed agent overview, Esc again to the workspace.
    plain(&mut state, KeyCode::Esc);
    assert_eq!(state.trajectory_level(), Some(TrajectoryLevel::Agents));
    assert!(render(&mut state, 180, 48).contains("✓ done"));
    plain(&mut state, KeyCode::Esc);
    assert!(!state.trajectory_open());

    // `z` zooms straight into the overview.
    plain(&mut state, KeyCode::Char('z'));
    assert_eq!(state.trajectory_level(), Some(TrajectoryLevel::Agents));
}

#[test]
fn gantt_shows_the_master_alone_without_a_dag() {
    let dir = tempdir().unwrap();
    let mut state = project_state(dir.path());
    state.refresh_agents();
    assert_eq!(
        state.gantt_agents(),
        [("master".to_owned(), AgentState::Idle)]
    );
    state.set_busy(true);
    state.refresh_agents();
    assert_eq!(state.gantt_agents()[0].1, AgentState::Running);
    state.present_approval(request("a"));
    state.refresh_agents();
    assert_eq!(state.gantt_agents()[0].1, AgentState::Waiting);
    plain(&mut state, KeyCode::Esc);
    let screen = render(&mut state, 160, 40);
    assert!(screen.contains("approval"), "{screen}");
    assert!(screen.contains("no DAG workers"), "{screen}");
}

#[test]
fn approval_is_answered_inline_in_the_discussion_prompt() {
    let mut state = TuiState::default();
    state.set_busy(true);
    state.present_approval(request("inline"));
    assert_eq!(state.inline_approval_lane(), Some(ApprovalLane::Discussion));
    let screen = render(&mut state, 160, 40);
    let lines: Vec<&str> = screen.lines().collect();
    // The form sits at the bottom, in the prompt; the workspace stays visible.
    let title = lines
        .iter()
        .rposition(|line| line.contains("Approval required 1/1 · run_command"))
        .expect(&screen);
    assert!(title > lines.len() / 2, "{screen}");
    assert!(screen.contains("❯ 3. No"), "{screen}");
    assert!(screen.contains("Conversation"), "{screen}");
    assert!(screen.contains("Gantt"), "{screen}");
    assert!(!screen.contains("Policy:"), "{screen}");

    // `v` opens the full review, Esc returns to the prompt form, Esc again
    // leaves the request pending while the draft is edited.
    plain(&mut state, KeyCode::Char('v'));
    assert_eq!(state.inline_approval_lane(), None);
    assert!(render(&mut state, 160, 40).contains("Policy:"));
    plain(&mut state, KeyCode::Esc);
    assert_eq!(state.inline_approval_lane(), Some(ApprovalLane::Discussion));
    plain(&mut state, KeyCode::Esc);
    assert_eq!(state.inline_approval_lane(), None);
    assert_eq!(state.approval_count(), 1);

    plain(&mut state, KeyCode::Char('y'));
    assert_eq!(state.input(), "y");
    state.set_input("y");
    plain(&mut state, KeyCode::Enter);
    assert!(render(&mut state, 160, 40).contains("❯ 1. Yes"));
    assert!(matches!(
        plain(&mut state, KeyCode::Enter),
        TuiAction::RespondApproval { allow: true, .. }
    ));
}

#[test]
fn arch_owner_approval_is_answered_in_the_arch_prompt() {
    let mut state = TuiState::default();
    let project = state.active_project().to_owned();
    state.present_approval_in(project, request("arch"), ApprovalLane::Arch);
    assert_eq!(state.inline_approval_lane(), Some(ApprovalLane::Arch));
    let screen = render(&mut state, 160, 44);
    let lines: Vec<&str> = screen.lines().collect();
    let form = lines
        .iter()
        .position(|line| line.contains("Approval required 1/1 · run_command"))
        .expect(&screen);
    let arch = lines
        .iter()
        .position(|line| line.starts_with("┌ Arch ·"))
        .expect(&screen);
    let conversation = lines
        .iter()
        .position(|line| line.contains("Conversation"))
        .expect(&screen);
    // In the left column the form is under the Arch transcript and above the
    // conversation, and the discussion prompt keeps its normal title.
    assert!(arch < form && form < conversation, "{screen}");

    // While the arch request waits unfocused, only the Arch title says so.
    plain(&mut state, KeyCode::Esc);
    let waiting = render(&mut state, 160, 44);
    assert!(
        waiting.replace(' ', "").contains("Arch·⏸等待审批"),
        "{waiting}"
    );
    assert!(
        !waiting.replace(' ', "").contains("Prompt·⏸等待审批"),
        "{waiting}"
    );
    assert!(
        !lines[lines.len() - 3].contains("Approval required"),
        "{screen}"
    );

    // Typing y in the arch prompt selects ALLOW for that request.
    plain(&mut state, KeyCode::Esc);
    state.set_arch_input("y");
    assert_eq!(state.submit_arch_prompt().unwrap(), TuiAction::Redraw);
    assert!(matches!(
        plain(&mut state, KeyCode::Enter),
        TuiAction::RespondApproval { allow: true, .. }
    ));
}

#[test]
fn new_session_choice_discards_pending_inputs_or_keeps_them() {
    let mut state = TuiState::default();
    let project = state.active_project().to_owned();
    let pending = vec![("tui-17923-1".to_owned(), "y".to_owned())];
    state.offer_new_session_choice(project.clone(), pending.clone());
    assert!(state.new_session_choice_open());
    let screen = render(&mut state, 140, 40);
    assert!(screen.contains("New session · 1 pending input"), "{screen}");
    assert!(screen.contains("(tui-17923-1)"), "{screen}");
    assert!(
        screen.contains("❯ 1. Discard them and start a new session"),
        "{screen}"
    );
    assert!(
        screen.contains("  2. Keep them and cancel /new"),
        "{screen}"
    );

    // Enter on the default discards and retries /new.
    assert_eq!(
        plain(&mut state, KeyCode::Enter),
        TuiAction::Submit("/new".into())
    );
    assert!(!state.new_session_choice_open());
    assert_eq!(state.take_discard_before_new(&project), ["tui-17923-1"]);
    assert!(state.take_discard_before_new(&project).is_empty());

    // Down + Enter (or 2, or Esc) keeps them and starts nothing.
    for keys in [
        &[KeyCode::Down, KeyCode::Enter][..],
        &[KeyCode::Char('2')],
        &[KeyCode::Esc],
    ] {
        state.offer_new_session_choice(project.clone(), pending.clone());
        let mut last = TuiAction::None;
        for key in keys {
            last = plain(&mut state, *key);
        }
        assert_eq!(last, TuiAction::Redraw);
        assert!(!state.new_session_choice_open());
        assert!(state.take_discard_before_new(&project).is_empty());
    }
}
