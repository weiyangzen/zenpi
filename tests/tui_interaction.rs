use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use zenpi::{
    core::Agent,
    layout::{LayoutPreferences, PaneId, TabId},
    session::SessionStore,
    slash::{self, SlashCommand},
    tui::{
        BentoBoxLayoutAdapter, LeftPrompt, MessageRole, MessageTarget, TuiAction, TuiState,
        dispatch_slash_command,
    },
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
    for (command, option) in [("/goal ", "/goal run"), ("/learn ", "/learn evidence")] {
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
    let adapter = BentoBoxLayoutAdapter::new(state.workspace_layout(), Rect::new(0, 5, 140, 31));
    let conversation = adapter.pane(PaneId::ProjectConversation).unwrap().rect;
    // The Project tab keeps one conversation lane; the former Goal pane is
    // now carried by Gantt, so the lower-left pane is Arch.
    let arch = adapter.pane(PaneId::Arch).unwrap().rect;
    state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        arch.x + 2,
        arch.y + 1,
    ));
    assert_eq!(state.focused_workspace_pane(), Some(PaneId::Arch));
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
    // The goal lane is shown by the Goal tab; the Project tab is now a single
    // conversation lane with plan/goal carried by Gantt.
    state.set_workspace_tab(TabId::Goal);
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
            display_name: None,
            approval_mode: Default::default(),
            style: None,
        source: None,
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
            display_name: None,
            approval_mode: Default::default(),
            style: None,
        source: None,
            cwd: dir.path().display().to_string(),
            session_path: Some(default_path.display().to_string()),
        },
    );
    assert!(state.open_project_tab("api"));
    state.set_project_metadata(
        "api",
        zenpi::tui::ProjectTabMetadata {
            display_name: None,
            approval_mode: Default::default(),
            style: None,
        source: None,
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
            display_name: None,
        approval_mode: Default::default(),
        style: None,
        source: None,
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
            display_name: None,
            approval_mode: Default::default(),
            style: None,
        source: None,
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
fn project_checkpoint_restores_isolated_transcript_and_layout() {
    let mut state = TuiState::default();
    state.push_message(zenpi::tui::MessageRole::User, "default message");
    assert!(state.open_project_tab("api"));
    state.push_message(zenpi::tui::MessageRole::Assistant, "api message");
    state.set_active_project_metadata(zenpi::tui::ProjectTabMetadata {
            display_name: None,
        approval_mode: Default::default(),
        style: None,
        source: None,
        cwd: "/work/api".into(),
        session_path: Some("/work/api/session.jsonl".into()),
    });
    state.set_project_session_cursor("api", "session-api", 42);
    let checkpoint = state.project_checkpoint();

    let mut restored = TuiState::default();
    assert!(restored.restore_project_checkpoint(&checkpoint));
    assert_eq!(restored.active_project(), "api");
    assert_eq!(restored.message_count(), 1);
    assert_eq!(restored.messages().next().unwrap().text, "api message");
    assert_eq!(restored.project_metadata("api").unwrap().cwd, "/work/api");
    assert_eq!(
        restored.project_session_cursor("api"),
        Some(("session-api", 42))
    );
    assert!(
        checkpoint["project_state"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| { item["name"] == "api" && item["messages"][0]["text"] == "api message" })
    );
}

#[test]
fn project_view_reports_identity_and_current_feature_projection() {
    let mut state = TuiState::default();
    assert!(state.open_project_tab("api"));
    state.set_active_project_metadata(zenpi::tui::ProjectTabMetadata {
            display_name: None,
        approval_mode: Default::default(),
        style: None,
        source: None,
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
        assert!(text.contains("conversation lane"));
        if tab == TabId::Goal {
            assert!(text.contains("Goal"));
            assert!(text.contains("goal lane"));
        }
    }
}

#[test]
fn horizontal_split_drag_is_persisted_without_overlaps() {
    let mut state = TuiState::default();
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    let adapter = BentoBoxLayoutAdapter::new(state.workspace_layout(), Rect::new(0, 5, 140, 31));
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

#[test]
fn alt_m_focuses_the_arch_master_console_and_submits_bash() {
    use zenpi::tool_runtime::MasterSessionCommand;
    use zenpi::tui::LeftPrompt;

    let mut state = TuiState::default();
    assert_eq!(state.left_prompt(), LeftPrompt::Discussion);
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT)),
        TuiAction::Redraw
    );
    assert_eq!(state.left_prompt(), LeftPrompt::Arch);

    for character in "!echo hi".chars() {
        state.handle_key(key(KeyCode::Char(character)));
    }
    assert_eq!(state.arch_input(), "!echo hi");
    // The discussion draft is a separate buffer and is never touched.
    assert_eq!(state.input(), "");

    let action = state.handle_key(key(KeyCode::Enter));
    assert!(matches!(
        action,
        TuiAction::SubmitArch(MasterSessionCommand::Bash(command)) if command == "echo hi"
    ));
    assert_eq!(state.arch_input(), "");
    assert!(state.master_busy());
    assert_eq!(
        state.take_arch_intent(),
        Some(MasterSessionCommand::Bash("echo hi".into()))
    );

    // Esc returns focus to the discussion prompt without losing the console.
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::ALT)),
        TuiAction::Redraw
    );
    assert_eq!(state.left_prompt(), LeftPrompt::Discussion);
}

#[test]
fn clicking_the_arch_prompt_focuses_the_master_console() {
    use zenpi::tui::LeftPrompt;

    let mut state = TuiState::default();
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    let adapter = BentoBoxLayoutAdapter::new(state.workspace_layout(), Rect::new(0, 5, 140, 31));
    let arch = adapter.pane(PaneId::Arch).unwrap().rect;
    let (_, arch_prompt) = zenpi::layout::arch_prompt_group(
        zenpi::layout::PaneRect::new(arch.x, arch.y, arch.width, arch.height),
        zenpi::tui::ARCH_PROMPT_PANE_ROWS,
    );
    state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        arch_prompt.x + 2,
        arch_prompt.y + 1,
    ));
    assert_eq!(state.left_prompt(), LeftPrompt::Arch);
    state.handle_key(key(KeyCode::Char('x')));
    assert_eq!(state.arch_input(), "x");

    let conversation = adapter.pane(PaneId::ProjectConversation).unwrap().rect;
    let (_, discussion_prompt) = zenpi::layout::conversation_prompt_group(
        zenpi::layout::PaneRect::new(
            conversation.x,
            conversation.y,
            conversation.width,
            conversation.height,
        ),
        zenpi::tui::PROMPT_PANE_ROWS,
    );
    state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        discussion_prompt.x + 2,
        discussion_prompt.y + 1,
    ));
    assert_eq!(state.left_prompt(), LeftPrompt::Discussion);
}

#[test]
fn arch_console_transcript_is_isolated_from_the_discussion_lane() {
    let mut state = TuiState::default();
    state.push_message(MessageRole::User, "discussion-only");
    state.push_arch_message(MessageRole::User, "!arch-only");
    assert_eq!(state.message_count(), 1);
    assert_eq!(state.arch_message_count(), 1);
    assert_eq!(state.arch_messages().next().unwrap().text, "!arch-only");

    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("arch-only"));
    assert!(text.contains("discussion-only"));
}

#[test]
fn arch_steering_joins_the_active_master_turn_instead_of_forking() {
    use zenpi::tool_runtime::{MasterSessionCommand, MasterSessionInputError};

    let mut state = TuiState::default();
    state.set_arch_input("!echo one");
    assert!(matches!(
        state.submit_arch_prompt(),
        Ok(TuiAction::SubmitArch(MasterSessionCommand::Bash(_)))
    ));
    state.set_arch_input("!echo two");
    assert_eq!(
        state.submit_arch_prompt().unwrap_err(),
        MasterSessionInputError::Busy
    );
    state.set_arch_input("slow the workers down");
    assert_eq!(
        state.submit_arch_prompt().unwrap(),
        TuiAction::SubmitArch(MasterSessionCommand::Steer("slow the workers down".into()))
    );
    assert!(state.master_busy());
}

#[test]
fn arch_console_supports_slash_commands_with_isolated_feedback() {
    let mut state = TuiState::default();
    // The discussion draft is independent of the arch draft.
    state.set_input("/help");
    state.set_arch_input("/help");
    let action = state.submit_arch_prompt().unwrap();
    assert!(
        matches!(
            action,
            TuiAction::SubmitArchSlash {
                command: SlashCommand::Help { .. },
                ..
            }
        ),
        "arch slash must route as an arch command, got {action:?}"
    );
    // Submitting the arch command clears only the arch draft.
    assert_eq!(state.arch_input(), "");
    assert_eq!(state.input(), "/help");

    // Feedback for an arch command lands in the arch transcript only (ZS1-165).
    state.set_message_target(MessageTarget::Arch);
    state.push_message(MessageRole::Error, "arch-only-error");
    state.set_message_target(MessageTarget::Discussion);
    assert_eq!(state.message_count(), 0);
    assert_eq!(state.arch_message_count(), 1);

    // Non-slash drafts keep the master-session bash/steer classification.
    state.set_arch_input("!echo hi");
    assert!(matches!(
        state.submit_arch_prompt(),
        Ok(TuiAction::SubmitArch(_))
    ));
}

#[cfg(unix)]
#[test]
fn shell_pane_forwards_keys_when_focused() {
    let mut state = TuiState::default();
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    assert!(state.has_shell(), "Shell pane must own a real PTY");
    assert!(state.focus_workspace_pane(PaneId::Execution));
    let action = state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert!(matches!(action, TuiAction::Redraw));
    assert_eq!(
        state.input(),
        "",
        "a Shell keystroke must drive the PTY, not the prompt"
    );
}

#[cfg(unix)]
#[test]
fn shell_pane_forwards_ordinary_chars_before_the_paste_buffer() {
    let mut state = TuiState::default();
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    assert!(state.has_shell());
    assert!(state.focus_workspace_pane(PaneId::Execution));
    for character in "echo ZS171".chars() {
        let _ = state.handle_event_at(
            Event::Key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)),
            std::time::Instant::now(),
        );
    }
    assert_eq!(
        state.input(),
        "",
        "ordinary chars must reach the PTY, not the prompt buffer"
    );
}

#[test]
fn only_the_focused_input_owns_the_ime_cursor() {
    let mut state = TuiState::default();
    assert!(
        state.discussion_prompt_focused(),
        "the discussion prompt owns the IME anchor by default"
    );
    state.set_left_prompt(LeftPrompt::Arch);
    assert_eq!(state.left_prompt(), LeftPrompt::Arch);
    assert!(
        !state.discussion_prompt_focused(),
        "focusing arch must release the discussion cursor"
    );
    state.set_left_prompt(LeftPrompt::Discussion);
    assert!(state.discussion_prompt_focused());

    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    assert!(state.focus_workspace_pane(PaneId::Execution));
    assert!(
        !state.discussion_prompt_focused(),
        "focusing the Shell pane must release the discussion cursor"
    );
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
}

#[test]
fn discussion_and_arch_select_independent_zone_models() {
    use zenpi::view_model::{ViewModelError, Zone};

    let mut state = TuiState::default();
    assert_eq!(state.zone_model(Zone::Discussion), None);
    assert_eq!(state.zone_model(Zone::Arch), None);

    state
        .set_zone_model(Zone::Discussion, Some("discussion-a".into()))
        .unwrap();
    state
        .set_zone_model(Zone::Arch, Some("arch-b".into()))
        .unwrap();
    assert_eq!(state.zone_model(Zone::Discussion), Some("discussion-a"));
    assert_eq!(state.zone_model(Zone::Arch), Some("arch-b"));
    assert_eq!(
        state.effective_zone_model(Zone::Discussion, Some("global-g")),
        Some("discussion-a")
    );
    assert_eq!(
        state.effective_zone_model(Zone::Worker, Some("global-g")),
        Some("global-g")
    );

    // Workers never own a model choice.
    assert!(matches!(
        state.set_zone_model(Zone::Worker, Some("worker-c".into())),
        Err(ViewModelError::Invalid { .. })
    ));
    assert_eq!(state.zone_model(Zone::Worker), None);

    // A blank or control-bearing identity fails closed and changes nothing.
    assert!(
        state
            .set_zone_model(Zone::Arch, Some("bad\nmodel".into()))
            .is_err()
    );
    assert_eq!(state.zone_model(Zone::Arch), Some("arch-b"));

    // Clearing a zone override restores the global fallback.
    state.set_zone_model(Zone::Discussion, None).unwrap();
    assert_eq!(state.zone_model(Zone::Discussion), None);
    assert_eq!(
        state.effective_zone_model(Zone::Discussion, Some("global-g")),
        Some("global-g")
    );
}

#[test]
fn zone_concurrency_is_single_for_talk_zones_and_project_defined_for_workers() {
    use zenpi::view_model::Zone;

    let mut state = TuiState::default();
    assert_eq!(state.worker_concurrency(), 1);
    for zone in [Zone::Discussion, Zone::Arch, Zone::Worker] {
        assert_eq!(state.zone_concurrency(zone), 1);
    }

    // The active layer-2 workspace defines the worker pool size. Talk zones
    // stay single-concurrency regardless of the project's worker count.
    assert!(state.subtab_concurrency(0, 3));
    assert_eq!(state.worker_concurrency(), 4);
    assert_eq!(state.zone_concurrency(Zone::Worker), 4);
    assert_eq!(state.zone_concurrency(Zone::Discussion), 1);
    assert_eq!(state.zone_concurrency(Zone::Arch), 1);
}

#[test]
fn zone_models_round_trip_through_a_restart_snapshot() {
    use zenpi::view_model::{Zone, ZoneModels};

    let mut state = TuiState::default();
    state
        .set_zone_model(Zone::Discussion, Some("discussion-a".into()))
        .unwrap();
    state
        .set_zone_model(Zone::Arch, Some("arch-b".into()))
        .unwrap();
    let encoded = serde_json::to_value(state.zone_models()).unwrap();
    let decoded: ZoneModels = serde_json::from_value(encoded).unwrap();

    let mut restarted = TuiState::default();
    assert!(restarted.restore_zone_models(decoded).unwrap());
    assert_eq!(restarted.zone_model(Zone::Discussion), Some("discussion-a"));
    assert_eq!(restarted.zone_model(Zone::Arch), Some("arch-b"));
    // Restoring the same snapshot is idempotent.
    assert!(
        !restarted
            .restore_zone_models(restarted.zone_models().clone())
            .unwrap()
    );
}

#[test]
fn focused_arch_model_selection_does_not_disturb_the_discussion_agent() {
    use zenpi::view_model::Zone;
    use zenpi::tui::LeftPrompt;

    let dir = tempfile::tempdir().unwrap();
    let mut agent = Agent::with_echo(SessionStore::open(dir.path().join("s.jsonl")).unwrap());
    let mut state = TuiState::default();
    assert!(state.set_left_prompt(LeftPrompt::Arch));

    let action = dispatch_slash_command(
        SlashCommand::Model {
            name: Some("arch-model".into()),
        },
        &mut state,
        Some(&mut agent),
    );
    assert!(matches!(action, zenpi::tui::SlashDispatchAction::Continue));
    assert_eq!(state.zone_model(Zone::Arch), Some("arch-model"));
    assert_eq!(state.zone_model(Zone::Discussion), None);
    assert_eq!(agent.zone_model(Zone::Arch), Some("arch-model"));
    // The discussion agent's active model is untouched by an arch selection.
    assert_eq!(agent.snapshot().model, None);

    // Selecting on the discussion zone updates the shared global model.
    assert!(state.set_left_prompt(LeftPrompt::Discussion));
    dispatch_slash_command(
        SlashCommand::Model {
            name: Some("discussion-model".into()),
        },
        &mut state,
        Some(&mut agent),
    );
    assert_eq!(state.zone_model(Zone::Discussion), Some("discussion-model"));
    assert_eq!(agent.zone_model(Zone::Discussion), Some("discussion-model"));
    assert_eq!(agent.snapshot().model.as_deref(), Some("discussion-model"));
    // The arch override is retained and independent.
    assert_eq!(agent.zone_model(Zone::Arch), Some("arch-model"));
}
