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
        BentoBoxLayoutAdapter, HotZone, LeftPrompt, MessageRole, MessageTarget, TuiAction,
        TuiState, dispatch_slash_command,
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
    use zenpi::tui::LeftPrompt;
    use zenpi::view_model::Zone;

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

#[test]
fn arch_typing_through_the_event_stream_never_touches_the_discussion_draft() {
    let mut state = TuiState::default();
    assert!(state.set_left_prompt(LeftPrompt::Arch));
    for character in "hi 世界".chars() {
        let _ = state.handle_event_at(
            Event::Key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)),
            std::time::Instant::now(),
        );
    }
    assert_eq!(state.arch_input(), "hi 世界");
    assert_eq!(
        state.input(),
        "",
        "the real event stream must route arch characters to the arch console"
    );
}

#[test]
fn paste_follows_the_unique_hot_zone() {
    let mut state = TuiState::default();
    assert!(state.set_left_prompt(LeftPrompt::Arch));
    let _ = state.handle_event(Event::Paste("hello 世界".into()));
    assert_eq!(state.arch_input(), "hello 世界");
    assert_eq!(state.input(), "");

    assert!(state.set_left_prompt(LeftPrompt::Discussion));
    let _ = state.handle_event(Event::Paste("disc".into()));
    assert_eq!(state.input(), "disc");
    assert_eq!(state.arch_input(), "hello 世界");
}

#[cfg(unix)]
#[test]
fn shell_hot_zone_owns_keys_even_with_a_discussion_draft() {
    let mut state = TuiState::default();
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    assert!(state.has_shell());
    state.set_input("draft stays");
    assert!(state.focus_workspace_pane(PaneId::Execution));
    assert_eq!(state.hot_zone(), HotZone::Shell);
    let action = state.handle_key(key(KeyCode::Char('x')));
    assert!(matches!(action, TuiAction::Redraw));
    assert_eq!(
        state.input(),
        "draft stays",
        "a Shell keystroke must not touch the discussion draft"
    );
}

#[test]
fn hot_zone_follows_workspace_pane_focus() {
    let mut state = TuiState::default();
    assert_eq!(state.hot_zone(), HotZone::Conversation);
    assert!(state.focus_workspace_pane(PaneId::Arch));
    assert_eq!(state.hot_zone(), HotZone::Arch);
    assert_eq!(state.left_prompt(), LeftPrompt::Arch);
    assert!(state.focus_workspace_pane(PaneId::Resources));
    assert_eq!(state.hot_zone(), HotZone::Resources);
    assert!(state.focus_workspace_pane(PaneId::Gantt));
    assert_eq!(state.hot_zone(), HotZone::Gantt);
    assert!(state.set_hot_zone(HotZone::None));
    assert_eq!(state.hot_zone(), HotZone::None);
    assert_eq!(state.focused_workspace_pane(), None);
    assert!(state.set_hot_zone(HotZone::Conversation));
    assert_eq!(state.left_prompt(), LeftPrompt::Discussion);
    assert!(state.focus_workspace_pane(PaneId::Execution));
    assert_eq!(state.hot_zone(), HotZone::Shell);
    assert!(state.set_hot_zone(HotZone::Arch));
    assert_eq!(state.hot_zone(), HotZone::Arch);
    assert_ne!(state.focused_workspace_pane(), Some(PaneId::Execution));
}

#[test]
fn arch_editing_is_grapheme_safe() {
    let mut state = TuiState::default();
    assert!(state.set_left_prompt(LeftPrompt::Arch));
    state.set_arch_input("a👨‍👩‍👧b");
    let _ = state.handle_key(key(KeyCode::End));
    let _ = state.handle_key(key(KeyCode::Left));
    let _ = state.handle_key(key(KeyCode::Backspace));
    assert_eq!(
        state.arch_input(),
        "ab",
        "backspace must delete the whole ZWJ family grapheme"
    );
    let _ = state.handle_key(key(KeyCode::Delete));
    assert_eq!(state.arch_input(), "a");
}

#[test]
fn switching_projects_isolates_the_arch_draft_and_transcript() {
    let mut state = TuiState::default();
    assert!(state.open_project_tab("second"));
    assert!(state.set_left_prompt(LeftPrompt::Arch));
    state.set_arch_input("first-draft");
    state.push_arch_message(MessageRole::User, "first-turn");
    assert!(state.select_project_tab(0));
    assert_eq!(state.hot_zone(), HotZone::Conversation);
    assert_eq!(state.arch_input(), "");
    assert_eq!(state.arch_message_count(), 0);
    assert!(state.select_project_tab(1));
    assert_eq!(state.arch_input(), "first-draft");
    assert_eq!(state.arch_message_count(), 1);
}

#[test]
fn resources_double_click_opens_the_enlarged_worker_stdio_view() {
    use zenpi::resources::{
        FootprintPhase, HeadlessFootprintBudget, HeadlessFootprintSummary,
        HeadlessFootprintVerdict, HeadlessProcessFootprint, ResourceCollector, SignalStatus,
    };

    let dir = tempfile::tempdir().unwrap();
    let journal = dir.path().join("worker.jsonl");
    std::fs::write(
        &journal,
        "{\"kind\":\"turn\",\"turn\":{\"id\":\"t1\",\"role\":\"user\",\"content\":\"build it\",\"created_at_ms\":1}}\n\
         {\"kind\":\"turn\",\"turn\":{\"id\":\"t2\",\"role\":\"assistant\",\"content\":\"done 世界\",\"created_at_ms\":2}}\n",
    )
    .unwrap();

    let mut state = TuiState::default();
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();

    let mut snapshot = ResourceCollector::new(dir.path())
        .unwrap()
        .collect()
        .unwrap();
    snapshot.headless = HeadlessFootprintSummary::from_processes(
        HeadlessFootprintBudget::default(),
        vec![HeadlessProcessFootprint {
            pid: 4321,
            phase: FootprintPhase::Busy,
            cpu_percent: 12.5,
            resident_bytes: 4096,
            status: SignalStatus::Available,
            verdict: HeadlessFootprintVerdict::Within,
            session: Some(journal.display().to_string()),
        }],
    );
    state.set_resource_snapshot(snapshot);
    state.set_project_metadata(
        "default",
        zenpi::tui::ProjectTabMetadata {
            cwd: dir.path().display().to_string(),
            session_path: Some(journal.display().to_string()),
            ..Default::default()
        },
    );

    let adapter = BentoBoxLayoutAdapter::new(state.workspace_layout(), state.workspace_area());
    let rect = adapter
        .pane(PaneId::Resources)
        .expect("Resources pane is part of the project preset")
        .rect;
    let point = (rect.x + 2, rect.y + 2);
    let _ = state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        point.0,
        point.1,
    ));
    assert!(
        !state.resources_zoom_open(),
        "one click keeps block selection"
    );
    let _ = state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        point.0,
        point.1,
    ));
    assert!(
        state.resources_zoom_open(),
        "two presses on the same cell open the enlarged view"
    );
    // ZS1-185: a live swarm counts as the matrix (no extra master squares).
    assert_eq!(state.resources_zoom_worker_count(), 1);
    assert_eq!(
        state.resources_zoom_worker_stdio(4321),
        Some((Some("build it"), Some("done 世界")))
    );

    let _ = state.handle_key(key(KeyCode::Esc));
    assert!(!state.resources_zoom_open());
}

#[test]
fn resources_zoom_skips_workers_from_other_projects() {
    use zenpi::resources::{
        FootprintPhase, HeadlessFootprintBudget, HeadlessFootprintSummary,
        HeadlessFootprintVerdict, HeadlessProcessFootprint, ResourceCollector, SignalStatus,
    };

    let mine = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let journal = mine.path().join("mine.jsonl");
    std::fs::write(&journal, "{\"kind\":\"turn\",\"turn\":{\"id\":\"t1\",\"role\":\"user\",\"content\":\"x\",\"created_at_ms\":1}}\n").unwrap();
    let foreign = other.path().join("foreign.jsonl");
    std::fs::write(&foreign, "{\"kind\":\"turn\",\"turn\":{\"id\":\"t1\",\"role\":\"user\",\"content\":\"y\",\"created_at_ms\":1}}\n").unwrap();

    let mut state = TuiState::default();
    let mut snapshot = ResourceCollector::new(mine.path())
        .unwrap()
        .collect()
        .unwrap();
    snapshot.headless = HeadlessFootprintSummary::from_processes(
        HeadlessFootprintBudget::default(),
        vec![
            HeadlessProcessFootprint {
                pid: 1,
                phase: FootprintPhase::Idle,
                cpu_percent: 0.0,
                resident_bytes: 1024,
                status: SignalStatus::Available,
                verdict: HeadlessFootprintVerdict::Within,
                session: Some(journal.display().to_string()),
            },
            HeadlessProcessFootprint {
                pid: 2,
                phase: FootprintPhase::Idle,
                cpu_percent: 0.0,
                resident_bytes: 1024,
                status: SignalStatus::Available,
                verdict: HeadlessFootprintVerdict::Within,
                session: Some(foreign.display().to_string()),
            },
        ],
    );
    state.set_resource_snapshot(snapshot);
    state.set_project_metadata(
        "default",
        zenpi::tui::ProjectTabMetadata {
            cwd: mine.path().display().to_string(),
            session_path: Some(journal.display().to_string()),
            ..Default::default()
        },
    );
    state.open_resources_zoom();
    assert_eq!(state.resources_zoom_worker_count(), 1);
    assert!(state.resources_zoom_worker_stdio(1).is_some());
    assert!(state.resources_zoom_worker_stdio(2).is_none());
}

fn screen_rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    let width = usize::from(buffer.area.width);
    let mut rows = vec![String::new()];
    for (index, cell) in buffer.content().iter().enumerate() {
        if index > 0 && index % width == 0 {
            rows.push(String::new());
        }
        rows.last_mut().unwrap().push_str(cell.symbol());
    }
    rows
}

#[test]
fn subtab_concurrency_number_is_centered_and_double_click_edits_it() {
    let mut state = TuiState::default();
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    let rows = screen_rows(&terminal);
    let row = rows
        .iter()
        .position(|line| line.contains("[↑]"))
        .expect("the layer-2 concurrency control is rendered");
    assert!(
        rows[row].contains("[↑] 1 [↓]"),
        "the concurrency number is centered between the arrows: {:?}",
        rows[row]
    );

    // Double-clicking the number cell opens the inline editor. Column math is
    // in buffer cells, not UTF-8 bytes.
    let cells: Vec<char> = rows[row].chars().collect();
    let arrow = cells
        .windows(3)
        .position(|window| window == ['[', '↑', ']'])
        .expect("the up arrow is rendered");
    let point = ((arrow + 4) as u16, row as u16);
    let _ = state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        point.0,
        point.1,
    ));
    assert!(!state.subtab_concurrency_edit_active());
    let _ = state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        point.0,
        point.1,
    ));
    assert!(
        state.subtab_concurrency_edit_active(),
        "the second press on the number cell opens the inline editor"
    );

    // Clear the prefilled value, type a new one and commit with Enter.
    let _ = state.handle_key(key(KeyCode::Backspace));
    let _ = state.handle_key(key(KeyCode::Char('1')));
    let _ = state.handle_key(key(KeyCode::Char('2')));
    let _ = state.handle_key(key(KeyCode::Enter));
    assert!(!state.subtab_concurrency_edit_active());
    assert_eq!(state.subtabs()[0].concurrency, 12);

    // Esc cancels without changing the value.
    assert!(state.begin_subtab_concurrency_edit(0));
    let _ = state.handle_key(key(KeyCode::Backspace));
    let _ = state.handle_key(key(KeyCode::Backspace));
    let _ = state.handle_key(key(KeyCode::Char('7')));
    let _ = state.handle_key(key(KeyCode::Esc));
    assert_eq!(state.subtabs()[0].concurrency, 12);

    // Committed values share the arrow clamp.
    assert!(state.set_subtab_concurrency(0, 999));
    assert_eq!(state.subtabs()[0].concurrency, 64);
    assert!(state.set_subtab_concurrency(0, 0));
    assert_eq!(state.subtabs()[0].concurrency, 1);
}

fn observation_state(dir: &tempfile::TempDir, count: usize) -> TuiState {
    use zenpi::resources::{
        FootprintPhase, HeadlessFootprintBudget, HeadlessFootprintSummary,
        HeadlessFootprintVerdict, HeadlessProcessFootprint, ResourceCollector, SignalStatus,
    };

    let journal = dir.path().join("worker.jsonl");
    std::fs::write(
        &journal,
        "{\"kind\":\"turn\",\"turn\":{\"id\":\"t1\",\"role\":\"user\",\"content\":\"build it\",\"created_at_ms\":1}}\n\
         {\"kind\":\"turn\",\"turn\":{\"id\":\"t2\",\"role\":\"assistant\",\"content\":\"done\",\"created_at_ms\":2}}\n",
    )
    .unwrap();
    let mut state = TuiState::default();
    let mut snapshot = ResourceCollector::new(dir.path())
        .unwrap()
        .collect()
        .unwrap();
    let rows = (0..count)
        .map(|index| HeadlessProcessFootprint {
            pid: 4000 + index as u32,
            phase: FootprintPhase::Busy,
            cpu_percent: 12.5,
            resident_bytes: 4096,
            status: SignalStatus::Available,
            verdict: HeadlessFootprintVerdict::Within,
            session: Some(journal.display().to_string()),
        })
        .collect::<Vec<_>>();
    snapshot.headless =
        HeadlessFootprintSummary::from_processes(HeadlessFootprintBudget::default(), rows);
    state.set_resource_snapshot(snapshot);
    state.set_project_metadata(
        "default",
        zenpi::tui::ProjectTabMetadata {
            cwd: dir.path().display().to_string(),
            session_path: Some(journal.display().to_string()),
            ..Default::default()
        },
    );
    state
}

#[test]
fn resources_worker_grid_uses_fixed_doubling_tiers() {
    // ZS1-185: 1x2, 2x4, 4x8, ... with rows and columns doubling per tier.
    assert_eq!(zenpi::tui::worker_grid_tier(1), (1, 2));
    assert_eq!(zenpi::tui::worker_grid_tier(2), (1, 2));
    assert_eq!(zenpi::tui::worker_grid_tier(3), (2, 4));
    assert_eq!(zenpi::tui::worker_grid_tier(8), (2, 4));
    assert_eq!(zenpi::tui::worker_grid_tier(9), (4, 8));
    assert_eq!(zenpi::tui::worker_grid_tier(32), (4, 8));
    assert_eq!(zenpi::tui::worker_grid_tier(33), (8, 16));
    assert_eq!(zenpi::tui::worker_grid_tier(128), (8, 16));
    assert_eq!(zenpi::tui::worker_grid_tier(129), (16, 32));
    assert_eq!(zenpi::tui::worker_grid_tier(512), (16, 32));
    assert_eq!(zenpi::tui::worker_grid_tier(513), (32, 64));
    assert_eq!(zenpi::tui::worker_grid_tier(2048), (32, 64));
    assert_eq!(zenpi::tui::worker_grid_tier(2049), (64, 128));
    // Beyond the table the doubling continues.
    assert_eq!(zenpi::tui::worker_grid_tier(32768), (128, 256));
    assert_eq!(zenpi::tui::worker_grid_tier(32769), (256, 512));
    for count in [1usize, 2, 3, 9, 33, 129, 513, 2049, 4000] {
        let (rows, columns) = zenpi::tui::worker_grid_tier(count);
        assert!(rows * columns >= count);
        assert!(rows.is_power_of_two() && columns.is_power_of_two());
        assert_eq!(columns, rows * 2);
    }
}

#[test]
fn resources_observation_mode_toggles_and_renders_square_tiles() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = observation_state(&dir, 5);
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    assert!(state.focus_workspace_pane(PaneId::Resources));
    let adapter = BentoBoxLayoutAdapter::new(state.workspace_layout(), state.workspace_area());
    let rect = adapter.pane(PaneId::Resources).unwrap().rect;
    let point = (rect.x + 2, rect.y + 2);
    let _ = state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        point.0,
        point.1,
    ));
    let _ = state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        point.0,
        point.1,
    ));
    assert!(
        state.resources_zoom_open(),
        "double-click opens observation mode"
    );
    // ZS1-185: 5 live workers -> 2x4 tier.
    assert_eq!(state.resources_zoom_worker_count(), 5);
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    let (columns, rows, _cell) = state.resources_zoom_grid_shape().expect("grid rendered");
    assert_eq!(
        (columns, rows),
        (4, 2),
        "7 workers round up to the 2x4 tier"
    );
    assert!(state.resources_zoom_tiles_square(), "tiles must be square");

    // A second double-click on the same overlay cell returns to the pane.
    let area = state.workspace_area();
    let inside = (area.x + 4, area.y + 3);
    let _ = state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        inside.0,
        inside.1,
    ));
    let _ = state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        inside.0,
        inside.1,
    ));
    assert!(
        !state.resources_zoom_open(),
        "double-click returns from observation mode"
    );
}

#[test]
fn no_hot_zone_blocks_text_and_tab_restores_a_zone() {
    let mut state = TuiState::default();
    assert!(state.set_hot_zone(HotZone::None));
    assert_eq!(state.hot_zone(), HotZone::None);
    for character in "typed".chars() {
        let _ = state.handle_key(key(KeyCode::Char(character)));
    }
    assert_eq!(state.input(), "", "no-hot-zone must not edit the draft");
    assert_eq!(
        state.handle_key(key(KeyCode::Tab)),
        TuiAction::Redraw,
        "Tab still cycles panes"
    );
    assert_ne!(state.hot_zone(), HotZone::None);
}

/// A refused key used to vanish without a trace, which reads as a dead
/// terminal.  It has to say so somewhere the production renderer actually
/// draws -- the footer, not `status`, which only the legacy host renders.
#[test]
fn a_key_refused_by_the_hot_zone_says_so_in_the_footer() {
    let mut state = TuiState::default();
    assert!(state.set_hot_zone(HotZone::None));
    // A fresh backend per frame: reusing one leaves cells from the previous
    // frame between the wide glyphs of the next.
    let footer = |state: &mut TuiState| {
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|frame| state.render_bentobox(frame, "zenpi"))
            .unwrap();
        let buffer = terminal.backend().buffer();
        // The renderer pads every wide glyph with a space, so compare on the
        // text with its spacing removed.
        (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .map(|(x, y)| buffer[(x, y)].symbol())
            .collect::<String>()
            .replace(' ', "")
    };
    assert!(
        footer(&mut state).contains("无热区"),
        "the zone hint is what the footer shows before anything is refused"
    );
    let _ = state.handle_key(key(KeyCode::Char('q')));
    let line = footer(&mut state);
    assert!(
        line.contains("不接受普通输入"),
        "a dropped key has to say so: {line:?}"
    );
    assert_eq!(state.input(), "", "the draft stays untouched");
}

/// The conflict net. Per-binding tests say what one key does; none of them says
/// that a key does nothing anywhere else. A shortcut added to the wrong branch,
/// or a zone that forgets to decline a key, shows up as a diff here.
///
/// Every case starts from a fresh state, so one key's action cannot leak into
/// the next case through accumulated state.
#[test]
fn a_key_sweep_leaves_drafts_alone_in_every_non_text_zone() {
    let mut ordinary = Vec::new();
    for character in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
        .chars()
        .chain(" /?-_.,;:'\"!@#$%^&*()[]{}<>|\\`~+=".chars())
    {
        ordinary.push(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    for code in [
        KeyCode::Enter,
        KeyCode::Esc,
        KeyCode::Backspace,
        KeyCode::Delete,
        KeyCode::Insert,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Tab,
        KeyCode::BackTab,
    ] {
        ordinary.push(KeyEvent::new(code, KeyModifiers::NONE));
    }
    // Ctrl-G spawns an external editor and Ctrl-D quits; neither belongs in a
    // draft-leak sweep.
    let mut chords = Vec::new();
    for character in 'a'..='z' {
        for modifier in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
            if modifier == KeyModifiers::CONTROL && matches!(character, 'g' | 'd') {
                continue;
            }
            chords.push(KeyEvent::new(KeyCode::Char(character), modifier));
        }
    }
    for code in [
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Delete,
        KeyCode::Backspace,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::PageUp,
        KeyCode::PageDown,
    ] {
        for modifier in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
            chords.push(KeyEvent::new(code, modifier));
        }
    }

    // Resources, Gantt and None own no text; Conversation and Arch do.
    // Collect every leak rather than stopping at the first: the point of the
    // sweep is the shape of the whole set.
    let mut leaks = Vec::new();
    for zone in [HotZone::Resources, HotZone::Gantt, HotZone::None] {
        for (label, keys) in [("ordinary", &ordinary), ("chord", &chords)] {
            for event in keys.iter() {
                let mut state = TuiState::default();
                state.set_input("discussion draft");
                assert!(state.set_left_prompt(LeftPrompt::Arch));
                let _ = state.handle_key(key(KeyCode::Char('z')));
                assert_eq!(state.arch_input(), "z", "arch draft seed failed");
                assert!(state.set_hot_zone(zone));
                // The cursor counts too: Ctrl-A/Ctrl-E move it without changing
                // the text, which is the same leak from a draft nobody can see.
                let cursor = state.cursor();

                let _ = state.handle_key(*event);

                if state.input() != "discussion draft" {
                    leaks.push(format!(
                        "{zone:?} {label} {event:?} → discussion draft became {:?}",
                        state.input()
                    ));
                }
                if state.arch_input() != "z" {
                    leaks.push(format!(
                        "{zone:?} {label} {event:?} → arch draft became {:?}",
                        state.arch_input()
                    ));
                }
                if state.cursor() != cursor {
                    leaks.push(format!(
                        "{zone:?} {label} {event:?} → discussion cursor moved {cursor} → {}",
                        state.cursor()
                    ));
                }
            }
        }
    }
    assert!(
        leaks.is_empty(),
        "{} bindings reached a draft from a zone that owns no text:\n{}",
        leaks.len(),
        leaks.join("\n")
    );
}

/// The other direction: the two text zones each own exactly one draft. A key
/// that reaches both is the same conflict seen from the other side.
#[test]
fn each_text_zone_edits_only_its_own_draft() {
    // The discussion prompt owns the discussion draft.
    let mut state = TuiState::default();
    state.set_input("seed");
    let _ = state.set_left_prompt(LeftPrompt::Discussion);
    let _ = state.handle_key(key(KeyCode::Char('x')));
    assert_eq!(state.input(), "seedx");
    assert_eq!(state.arch_input(), "");

    // The arch console owns the arch draft, even with a discussion draft in
    // progress: a character reaches exactly one of the two.
    let mut state = TuiState::default();
    state.set_input("seed");
    let _ = state.set_left_prompt(LeftPrompt::Arch);
    let _ = state.handle_key(key(KeyCode::Char('x')));
    assert_eq!(state.input(), "seed");
    assert_eq!(state.arch_input(), "x");
}

/// A navigation zone owns unmodified keys only. One chord meaning two
/// different things depending on which pane happens to be hot is the conflict
/// this rules out: Ctrl-Up moves pane focus everywhere, so it must not scroll
/// the Gantt just because the Gantt is focused.
#[test]
fn a_chord_keeps_its_global_meaning_while_a_navigation_zone_is_hot() {
    let mut state = TuiState::default();
    assert!(state.focus_workspace_pane(PaneId::Gantt));
    assert_eq!(state.hot_zone(), HotZone::Gantt);

    // The plain key still scrolls, so the zone is not simply inert.
    let before = state.pane_scroll_offset(PaneId::Gantt);
    let _ = state.handle_key(key(KeyCode::Down));
    let scrolled = state.pane_scroll_offset(PaneId::Gantt);
    assert!(
        scrolled > before,
        "a plain Down must still scroll the Gantt"
    );

    let _ = state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL));
    assert_eq!(
        state.pane_scroll_offset(PaneId::Gantt),
        scrolled,
        "Ctrl-Up scrolled the Gantt instead of doing its global job"
    );
    assert_ne!(
        state.hot_zone(),
        HotZone::Gantt,
        "Ctrl-Up should have moved pane focus"
    );
}

#[test]
fn resources_zone_owns_navigation_and_esc_leaves_to_no_hot_zone() {
    let mut state = TuiState::default();
    state.set_input("draft stays");
    assert!(state.set_hot_zone(HotZone::Resources));
    assert_eq!(state.hot_zone(), HotZone::Resources);
    let _ = state.handle_key(key(KeyCode::Down));
    assert_eq!(state.input(), "draft stays");
    let _ = state.handle_key(key(KeyCode::Esc));
    assert_eq!(state.hot_zone(), HotZone::None);
    assert_eq!(state.input(), "draft stays");
}

#[test]
fn gantt_zone_scrolls_with_keyboard() {
    let mut state = TuiState::default();
    assert!(state.set_hot_zone(HotZone::Gantt));
    assert_eq!(state.hot_zone(), HotZone::Gantt);
    for _ in 0..3 {
        let _ = state.handle_key(key(KeyCode::Down));
    }
    assert_eq!(state.pane_scroll_offset(PaneId::Gantt), 3);
    let _ = state.handle_key(key(KeyCode::PageUp));
    assert_eq!(state.pane_scroll_offset(PaneId::Gantt), 0);
    let _ = state.handle_key(key(KeyCode::Esc));
    assert_eq!(state.hot_zone(), HotZone::None);
}

#[cfg(unix)]
#[test]
fn shell_zone_single_ctrl_c_reaches_the_pty_and_double_escalates() {
    let mut state = TuiState::default();
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    assert!(state.has_shell());
    assert!(state.set_hot_zone(HotZone::Shell));
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        TuiAction::Redraw,
        "the first Ctrl-C is the PTY's own SIGINT"
    );
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        TuiAction::ForceKill,
        "the second Ctrl-C escalates"
    );
}

fn session_summary(path: &str, id: &str, name: Option<&str>) -> zenpi::session::SessionSummary {
    zenpi::session::SessionSummary {
        path: path.to_owned(),
        session_id: id.to_owned(),
        name: name.map(str::to_owned),
        created_at_ms: 0,
        turn_count: 0,
        handoff_count: 0,
        handoff_record_count: 0,
        runtime_intent_count: 0,
        event_count: 0,
        recovery_warnings: 0,
        next_seq: 0,
    }
}

#[test]
fn session_rename_editor_validates_and_emits_the_persisted_name() {
    let mut state = TuiState::default();
    assert!(state.begin_session_rename());
    assert!(state.session_rename_active());
    for _ in 0..32 {
        let _ = state.handle_key(key(KeyCode::Backspace));
    }
    for character in "my-session".chars() {
        let _ = state.handle_key(key(KeyCode::Char(character)));
    }
    assert_eq!(
        state.handle_key(key(KeyCode::Enter)),
        TuiAction::RenameSession("my-session".into())
    );
    assert!(!state.session_rename_active());
    assert_eq!(state.session_header_label().as_deref(), Some("my-session"));

    // The header shows the session label between the logo and workspaces.
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    let rows = screen_rows(&terminal);
    let row = rows
        .iter()
        .position(|line| line.contains("my-session"))
        .expect("the session label is rendered in the header");
    assert!(rows[row].contains("zenpi · my-session | workspaces"));

    // Double-clicking the label reopens the editor.
    let cells: Vec<char> = rows[row].chars().collect();
    let column = cells
        .windows(2)
        .position(|window| window == ['m', 'y'])
        .expect("the label starts with the name");
    let point = (column as u16, row as u16);
    let _ = state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        point.0,
        point.1,
    ));
    assert!(!state.session_rename_active());
    let _ = state.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        point.0,
        point.1,
    ));
    assert!(
        state.session_rename_active(),
        "double-click renames the session"
    );
}

#[test]
fn session_rename_rejects_duplicates_and_stays_open() {
    let mut state = TuiState::default();
    state.set_session_browser(vec![
        session_summary("a.jsonl", "id-a", Some("taken")),
        session_summary("b.jsonl", "id-b", None),
    ]);
    state.set_current_session_path(Some("b.jsonl".into()));
    assert!(state.begin_session_rename());
    for character in "taken".chars() {
        let _ = state.handle_key(key(KeyCode::Char(character)));
    }
    assert_eq!(state.handle_key(key(KeyCode::Enter)), TuiAction::Redraw);
    assert!(
        state.session_rename_active(),
        "a duplicate keeps the editor open for re-input"
    );
    assert!(state.status().contains("已存在"));
    assert_eq!(state.handle_key(key(KeyCode::Esc)), TuiAction::Redraw);
    assert!(!state.session_rename_active());
}

/// Wide CJK glyphs occupy a lead cell plus a blank continuation cell, so the
/// joined buffer reads "思 考 中". Normalize before matching.
fn contains_cjk(screen: &str, needle: &str) -> bool {
    screen.replace(' ', "").contains(needle)
}

fn rendered_screen(state: &mut TuiState, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    screen_rows(&terminal).join("\n")
}

#[test]
fn thinking_animation_appears_and_advances_while_busy() {
    let mut state = TuiState::default();
    let idle = rendered_screen(&mut state, 160, 40);
    assert!(
        !contains_cjk(&idle, "思考中"),
        "idle shows no thinking animation"
    );

    state.set_busy(true);
    let first = state.thinking_frame().expect("busy exposes a frame");
    let screen = rendered_screen(&mut state, 160, 40);
    assert!(contains_cjk(&screen, "思考中"), "{screen}");
    assert!(screen.contains(first), "{screen}");

    state.tick();
    let second = state.thinking_frame().unwrap();
    assert_ne!(first, second, "the frame advances with the tick");
    let screen = rendered_screen(&mut state, 160, 40);
    assert!(screen.contains(second), "{screen}");

    state.set_busy(false);
    assert!(state.thinking_frame().is_none());
}

#[test]
fn arch_lane_shows_the_thinking_animation() {
    let mut state = TuiState::default();
    let idle = rendered_screen(&mut state, 160, 40);
    assert!(idle.contains("Arch · master session"), "{idle}");

    state.set_master_busy(true);
    let frame = state.thinking_frame().expect("arch busy exposes a frame");
    let screen = rendered_screen(&mut state, 160, 40);
    assert!(contains_cjk(&screen, "思考中"), "{screen}");
    assert!(screen.contains(frame), "{screen}");

    state.set_master_busy(false);
    assert!(state.thinking_frame().is_none());
}

#[test]
fn swarm_sized_worker_matrix_renders_the_16x32_tier_with_square_tiles() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = observation_state(&dir, 510);
    // 510 live workers -> tier 16x32.
    state.open_resources_zoom();
    assert_eq!(state.resources_zoom_worker_count(), 510);
    let mut terminal = Terminal::new(TestBackend::new(240, 80)).unwrap();
    terminal
        .draw(|f| state.render_bentobox(f, "zenpi"))
        .unwrap();
    let (columns, rows, _cell) = state.resources_zoom_grid_shape().expect("grid rendered");
    assert_eq!((columns, rows), (32, 16), "512 workers use the 16x32 tier");
    assert!(state.resources_zoom_tiles_square(), "every tile is square");
    // The Resources hot zone opens the observation mode with `z`.
    let mut hot = TuiState::default();
    assert!(hot.set_hot_zone(HotZone::Resources));
    let _ = hot.handle_key(key(KeyCode::Char('z')));
    assert!(hot.resources_zoom_open(), "z opens the worker matrix");
}

#[test]
fn worker_tiles_are_visually_square_adapt_to_window_and_busy_workers_blink() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = observation_state(&dir, 64);
    state.open_resources_zoom();
    assert!(
        state.resources_zoom_has_busy(),
        "the fixture workers are busy"
    );

    let mut wide = Terminal::new(TestBackend::new(240, 80)).unwrap();
    wide.draw(|f| state.render_bentobox(f, "zenpi")).unwrap();
    let (_, _, cell_wide) = state.resources_zoom_grid_shape().unwrap();
    assert!(
        state.resources_zoom_tiles_square(),
        "tiles are visually square (2 cells wide per cell of height)"
    );

    let mut tall = Terminal::new(TestBackend::new(120, 100)).unwrap();
    tall.draw(|f| state.render_bentobox(f, "zenpi")).unwrap();
    let (_, _, cell_tall) = state.resources_zoom_grid_shape().unwrap();
    assert!(state.resources_zoom_tiles_square());
    assert_ne!(
        cell_wide, cell_tall,
        "tile size adapts to the window aspect"
    );

    let before = state.resources_zoom_blink_on();
    state.tick();
    assert_ne!(
        before,
        state.resources_zoom_blink_on(),
        "busy workers blink with the animation tick"
    );
}
