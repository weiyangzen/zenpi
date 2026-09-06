use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use zenpi::{
    core::Agent,
    layout::{LayoutPreferences, PaneId, TabId},
    session::SessionStore,
    slash::{self, SlashCommand},
    tui::{BentoBoxLayoutAdapter, MessageRole, TuiAction, TuiState, dispatch_slash_command},
};

fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}
fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn palette_has_two_levels_and_never_submits_while_selecting() {
    let mut state = TuiState::default();
    state.set_input("/");
    assert!(state.slash_choices().contains(&"/persona".into()));
    state.set_input("/persona");
    assert_eq!(state.handle_key(key(KeyCode::Enter)), TuiAction::Redraw);
    assert_eq!(state.input(), "/persona ");
    assert_eq!(state.slash_choices().len(), 16);
    state.handle_key(key(KeyCode::Down));
    state.handle_key(key(KeyCode::Enter));
    assert_eq!(state.input(), "/persona INTP ");
    assert!(
        matches!(state.handle_key(key(KeyCode::Enter)), TuiAction::Submit(text) if text == "/persona INTP ")
    );
    for (command, option) in [("/goal ", "/goal status"), ("/learn ", "/learn evidence")] {
        state.set_input(command);
        assert!(state.slash_choices().contains(&option.into()));
    }
}

#[test]
fn slash_prompt_exposes_palette_state_in_the_prompt_border() {
    let mut state = TuiState::default();
    state.set_input("/");
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("command palette active"));
}

#[test]
fn persona_is_durable_and_conversation_has_colored_dots_not_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let mut state = TuiState::default();
    dispatch_slash_command(
        slash::parse("/personas ENFP").unwrap().unwrap(),
        &mut state,
        Some(&mut agent),
    );
    assert_eq!(agent.persona(), "ENFP");
    assert_eq!(
        Agent::with_echo(SessionStore::open(&path).unwrap()).persona(),
        "ENFP"
    );
    assert!(slash::parse("/persona nope").is_err());
    state.push_message(MessageRole::User, "hello");
    state.push_message(MessageRole::Assistant, "world");
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let text: String = buffer.content.iter().map(|c| c.symbol()).collect();
    assert!(!text.contains("you:"));
    assert!(!text.contains("zenpi:"));
    assert!(
        buffer
            .content
            .iter()
            .any(|c| c.symbol() == "●" && c.fg == zenpi::persona::color("ENFP"))
    );
}

#[test]
fn mouse_focus_tabs_and_split_drag_preserve_draft_and_layout() {
    let mut state = TuiState::default();
    state.set_input("draft");
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    let adapter = BentoBoxLayoutAdapter::new(state.workspace_layout(), Rect::new(0, 2, 140, 34));
    let conversation = adapter.pane(PaneId::ProjectConversation).unwrap().rect;
    let goal = adapter.pane(PaneId::GoalConversation).unwrap().rect;
    state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        goal.x + 2,
        goal.y + 1,
    ));
    assert_eq!(
        state.focused_workspace_pane(),
        Some(PaneId::GoalConversation)
    );
    state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        conversation.x + 2,
        conversation.y + 1,
    ));
    assert_eq!(
        state.focused_workspace_pane(),
        Some(PaneId::ProjectConversation)
    );
    let before = state.workspace_layout().ratios;
    state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        conversation.right() - 1,
        conversation.y + 2,
    ));
    state.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        85,
        conversation.y + 2,
    ));
    state.handle_mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        85,
        conversation.y + 2,
    ));
    assert_ne!(state.workspace_layout().ratios, before);
    assert_eq!(state.workspace_layout().ratios.right, before.right);
    assert_eq!(state.input(), "draft");
    let mut preferences = LayoutPreferences::default();
    preferences
        .set_model(None, state.workspace_layout())
        .unwrap();
    state.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 12, 1));
    assert_eq!(state.active_project(), "default");
    assert_eq!(state.input(), "draft");
}

#[test]
fn selecting_persona_does_not_start_a_provider_turn() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = Agent::with_echo(SessionStore::open(dir.path().join("s.jsonl")).unwrap());
    let mut state = TuiState::default();
    for name in zenpi::persona::TYPES {
        dispatch_slash_command(
            SlashCommand::Persona {
                name: Some(name.into()),
            },
            &mut state,
            Some(&mut agent),
        );
        assert_eq!(agent.persona(), name);
    }
    assert!(agent.history().is_empty());
}

#[test]
fn goal_hot_zone_keeps_an_independent_transcript_lane() {
    let mut state = TuiState::default();
    state.push_message(MessageRole::User, "conversation-only");
    state.push_goal_message(MessageRole::System, "goal-only");
    assert_eq!(state.message_count(), 1);
    assert_eq!(state.goal_message_count(), 1);
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("goal-only"));
    assert!(text.contains("conversation-only"));
    assert!(text.contains("Goal"));
}

#[test]
fn goal_lane_can_be_cleared_at_session_boundary() {
    let mut state = TuiState::default();
    state.push_goal_message(MessageRole::System, "old execution");
    assert_eq!(state.goal_message_count(), 1);
    state.clear_goal_messages();
    assert_eq!(state.goal_message_count(), 0);
}

#[test]
fn project_commands_create_select_and_close_wave_tabs() {
    let mut state = TuiState::default();
    assert!(state.open_project_tab("api"));
    state.push_message(MessageRole::User, "api-only");
    assert!(state.select_project_tab(0));
    assert!(state.select_project_tab(1));
    assert_eq!(state.active_project(), "api");
    assert!(state.messages().any(|m| m.text == "api-only"));
    assert!(state.close_project_tab("api"));
    assert_eq!(state.project_tabs(), &["default"]);
}

#[test]
fn project_select_binds_real_agent_session_and_workspace() {
    use std::path::Path;
    use zenpi::core::Agent;
    use zenpi::session::SessionStore;

    let dir = tempfile::tempdir().unwrap();
    let default_path = dir.path().join("default.jsonl");
    let api_path = dir.path().join("api.jsonl");
    let default = SessionStore::open(&default_path).unwrap();
    SessionStore::open(&api_path).unwrap();
    let mut agent = Agent::with_echo(default);
    let mut state = TuiState::default();
    assert!(state.open_project_tab("api"));
    state.set_project_metadata(
        "api",
        zenpi::tui::ProjectTabMetadata {
            approval_mode: Default::default(),
            cwd: dir.path().display().to_string(),
            session_path: Some(api_path.display().to_string()),
        },
    );
    let action = dispatch_slash_command(
        zenpi::slash::SlashCommand::Project {
            action: zenpi::slash::ProjectAction::Select { name: "api".into() },
        },
        &mut state,
        Some(&mut agent),
    );
    assert!(matches!(action, zenpi::tui::SlashDispatchAction::Continue));
    assert_eq!(agent.session().path(), Path::new(&api_path));
    assert_eq!(
        agent.attachment_workspace_root(),
        Some(dir.path().canonicalize().unwrap().as_path())
    );
}

#[test]
fn closing_active_project_rebinds_agent_to_remaining_project() {
    use zenpi::core::Agent;
    use zenpi::session::SessionStore;

    let dir = tempfile::tempdir().unwrap();
    let default_path = dir.path().join("default.jsonl");
    let api_path = dir.path().join("api.jsonl");
    let default = SessionStore::open(&default_path).unwrap();
    SessionStore::open(&api_path).unwrap();
    let mut agent = Agent::with_echo(default);
    let mut state = TuiState::default();
    state.set_project_metadata(
        "default",
        zenpi::tui::ProjectTabMetadata {
            approval_mode: Default::default(),
            cwd: dir.path().display().to_string(),
            session_path: Some(default_path.display().to_string()),
        },
    );
    assert!(state.open_project_tab("api"));
    state.set_project_metadata(
        "api",
        zenpi::tui::ProjectTabMetadata {
            approval_mode: Default::default(),
            cwd: dir.path().display().to_string(),
            session_path: Some(api_path.display().to_string()),
        },
    );
    assert!(state.select_project_tab(1));
    let action = dispatch_slash_command(
        zenpi::slash::SlashCommand::Project {
            action: zenpi::slash::ProjectAction::Close { name: "api".into() },
        },
        &mut state,
        Some(&mut agent),
    );
    assert!(matches!(action, zenpi::tui::SlashDispatchAction::Continue));
    assert_eq!(state.active_project(), "default");
    assert_eq!(agent.session().path(), default_path.as_path());
}

#[test]
fn project_tab_state_isolated_from_feature_tab_selector() {
    let mut state = TuiState::default();
    assert!(state.open_project_tab("api"));
    state.set_workspace_tab(TabId::Goal);
    assert_eq!(state.active_project(), "api");
    assert_eq!(state.workspace_tab(), TabId::Goal);
    assert!(state.select_project_tab(0));
    assert_eq!(state.active_project(), "default");
    // Feature projection selection is retained by the shared workspace model;
    // it does not redefine the top-level project identity.
    assert_eq!(state.workspace_tab(), TabId::Project);
}

#[test]
fn closing_project_before_active_keeps_active_identity() {
    let mut state = TuiState::default();
    assert!(state.open_project_tab("api"));
    assert!(state.open_project_tab("web"));
    assert!(state.select_project_tab(2));
    assert!(state.close_project_tab("default"));
    assert_eq!(state.active_project(), "web");
    assert_eq!(state.project_tabs(), &["api", "web"]);
}

#[test]
fn closing_project_removes_its_runtime_metadata_projection() {
    let mut state = TuiState::default();
    assert!(state.open_project_tab("api"));
    state.set_active_project_metadata(zenpi::tui::ProjectTabMetadata {
        approval_mode: Default::default(),
        cwd: "/work/api".into(),
        session_path: Some("/sessions/api.jsonl".into()),
    });
    assert!(state.close_project_tab("api"));
    assert!(state.project_metadata("api").is_none());
    assert!(state.project_tab_snapshot("api").is_none());
}

#[test]
fn approval_and_yolo_completion_offer_only_valid_modes() {
    let state = TuiState::default();
    let mut state = state;
    state.set_input("/yolo ");
    assert_eq!(state.slash_choices(), vec!["/yolo on", "/yolo off"]);
    state.set_input("/approval ");
    assert_eq!(
        state.slash_choices(),
        vec!["/approval ask", "/approval always", "/approval never"]
    );
}

#[test]
fn project_tab_strip_round_trips_and_clamps_active_index() {
    let mut state = TuiState::default();
    assert!(state.open_project_tab("api"));
    assert!(state.open_project_tab("web"));
    let saved = state.project_tabs_json();
    let mut restored = TuiState::default();
    let mut saved = saved;
    saved["active"] = serde_json::json!(99);
    assert!(restored.restore_project_tabs(&saved));
    assert_eq!(restored.project_tabs(), &["default", "api", "web"]);
    assert_eq!(restored.active_project(), "web");
}

#[test]
fn project_metadata_round_trips_with_project_strip() {
    let mut state = TuiState::default();
    assert!(state.open_project_tab("api"));
    state.set_project_metadata(
        "api",
        zenpi::tui::ProjectTabMetadata {
            approval_mode: Default::default(),
            cwd: "/workspace/api".into(),
            session_path: Some("/sessions/api.jsonl".into()),
        },
    );
    let saved = state.project_tabs_json();
    let mut restored = TuiState::default();
    assert!(restored.restore_project_tabs(&saved));
    let metadata = restored.project_metadata("api").unwrap();
    assert_eq!(metadata.cwd, "/workspace/api");
    assert_eq!(
        metadata.session_path.as_deref(),
        Some("/sessions/api.jsonl")
    );
}

#[test]
fn project_checkpoint_is_independent_from_feature_layouts() {
    let mut state = TuiState::default();
    assert!(state.open_project_tab("api"));
    let checkpoint = state.project_checkpoint();
    let mut restored = TuiState::default();
    assert!(restored.restore_project_checkpoint(&checkpoint));
    assert_eq!(restored.project_tabs(), &["default", "api"]);
    assert_eq!(restored.active_project(), "api");
}

#[test]
fn project_view_reports_identity_and_current_feature_projection() {
    let mut state = TuiState::default();
    assert!(state.open_project_tab("api"));
    state.set_active_project_metadata(zenpi::tui::ProjectTabMetadata {
        approval_mode: Default::default(),
        cwd: "/work/api".into(),
        session_path: Some("/work/api/session.jsonl".into()),
    });
    let view = state.project_view();
    assert_eq!(view["name"], "api");
    assert_eq!(view["metadata"]["cwd"], "/work/api");
    assert_eq!(view["feature_projection"], "project");
}

#[test]
fn new_project_has_explicit_empty_host_context() {
    let mut state = TuiState::default();
    assert!(state.open_project_tab("docs"));
    let metadata = state.project_metadata("docs").unwrap();
    assert!(metadata.cwd.is_empty());
    assert!(metadata.session_path.is_none());
    assert_eq!(state.project_switch_context(0).unwrap().cwd, "");
}

#[test]
fn both_hot_zones_remain_visible_when_switching_workspace_tabs() {
    let mut state = TuiState::default();
    state.push_message(MessageRole::User, "conversation lane");
    state.push_goal_message(MessageRole::System, "goal lane");
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    for tab in [TabId::Project, TabId::Goal] {
        state.set_workspace_tab(tab);
        terminal
            .draw(|f| state.render_bentobox(f, "zenpi"))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("Conversation"));
        assert!(text.contains("Goal"));
        assert!(text.contains("conversation lane"));
        assert!(text.contains("goal lane"));
    }
}

#[test]
fn horizontal_split_drag_is_persisted_without_overlaps() {
    let mut state = TuiState::default();
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    let adapter = BentoBoxLayoutAdapter::new(state.workspace_layout(), Rect::new(0, 2, 140, 34));
    let upper = adapter.pane(PaneId::ProjectConversation).unwrap().rect;
    state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        upper.x + 3,
        upper.bottom(),
    ));
    state.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        upper.x + 3,
        upper.bottom() + 3,
    ));
    state.handle_mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        upper.x + 3,
        upper.bottom() + 3,
    ));
    assert!(!state.workspace_layout().row_weights.is_empty());
    let mut preferences = LayoutPreferences::default();
    preferences
        .set_model(None, state.workspace_layout())
        .unwrap();
    let value = serde_json::to_value(&preferences).unwrap();
    let decoded: LayoutPreferences = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), value);
    for (w, h) in [(140, 34), (80, 20), (1, 1), (0, 0)] {
        assert!(
            state
                .workspace_layout()
                .compute(w, h)
                .visible_rects_non_overlapping()
        );
    }
}
