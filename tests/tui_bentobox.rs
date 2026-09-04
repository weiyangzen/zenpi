use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use zenpi::layout::{LayoutModel, PaneCapabilities, PaneId, TabId, Visibility};
use zenpi::tui::{BentoBoxLayoutAdapter, MessageRole, TuiAction, TuiState};

fn rendered(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn adapter_translates_and_clips_model_rectangles() {
    let model = LayoutModel::new(TabId::Project).with_capabilities(PaneCapabilities {
        browser: true,
        terminal: true,
    });
    let area = Rect::new(3, 2, 120, 30);
    let adapter = BentoBoxLayoutAdapter::new(&model, area);
    assert_eq!(adapter.area(), area);
    assert_eq!(adapter.breakpoint(), zenpi::layout::Breakpoint::Standard);
    assert!(adapter.visible_panes().all(|pane| {
        pane.rect.x >= area.x
            && pane.rect.y >= area.y
            && pane.rect.x.saturating_add(pane.rect.width) <= area.x.saturating_add(area.width)
            && pane.rect.y.saturating_add(pane.rect.height) <= area.y.saturating_add(area.height)
    }));
    assert!(adapter.snapshot().visible_rects_non_overlapping());
}

#[test]
fn disabled_optional_panes_are_not_rendered_or_allocated() {
    let model = LayoutModel::new(TabId::Project);
    let adapter = BentoBoxLayoutAdapter::new(&model, Rect::new(0, 0, 180, 40));
    assert_eq!(
        adapter.pane(PaneId::Browser).unwrap().visibility,
        Visibility::Unavailable
    );
    assert_eq!(
        adapter.pane(PaneId::Terminal).unwrap().visibility,
        Visibility::Unavailable
    );
    assert!(
        adapter
            .visible_panes()
            .all(|pane| pane.id != PaneId::Browser && pane.id != PaneId::Terminal)
    );
}

#[test]
fn production_workspace_renders_tabs_and_existing_transcript_prompt() {
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    let mut state = TuiState::default();
    state.push_message(MessageRole::Assistant, "# answer\n\nUse **bounded** panes.");
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let output = rendered(&terminal);
    assert!(output.contains("1:project"));
    assert!(output.contains("2:goal"));
    assert!(output.contains("Conversation"));
    assert!(output.contains("Resources"));
    assert!(output.contains("Gantt"));
    assert!(output.contains("answer"));
    assert!(output.contains("Prompt"));
}

#[test]
fn ctrl_number_switches_workspace_tab_without_submitting_prompt() {
    let mut state = TuiState::default();
    state.set_input("draft");
    let action = state.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::CONTROL));
    assert_eq!(action, TuiAction::Redraw);
    assert_eq!(state.workspace_tab(), TabId::Goal);
    assert_eq!(state.input(), "draft");
}

#[test]
fn workspace_keyboard_controls_focus_and_split_without_touching_prompt() {
    let mut state = TuiState::default();
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        TuiAction::Redraw
    );
    assert_eq!(
        state.focused_workspace_pane(),
        Some(PaneId::ProjectConversation)
    );

    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL)),
        TuiAction::Redraw
    );
    assert_eq!(state.focused_workspace_pane(), Some(PaneId::Resources));

    // Ctrl-Shift-Right changes a bounded split only; no prompt text is
    // consumed while the workspace is being adjusted.
    let before = state.workspace_layout().ratios;
    assert_eq!(
        state.handle_key(KeyEvent::new(
            KeyCode::Right,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )),
        TuiAction::Redraw
    );
    assert_ne!(state.workspace_layout().ratios, before);
    assert!(state.input().is_empty());

    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Char('0'), KeyModifiers::CONTROL)),
        TuiAction::Redraw
    );
    assert_eq!(state.focused_workspace_pane(), None);
    assert_eq!(
        state.workspace_layout().ratios,
        LayoutModel::new(TabId::Project).ratios
    );

    // With prompt text present, arrows retain their editing semantics.
    state.set_input("draft");
    state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL)),
        TuiAction::None
    );
    assert_eq!(state.input(), "draft");
    assert_eq!(state.focused_workspace_pane(), None);
}

#[test]
fn production_workspace_is_resize_safe_at_tiny_viewports() {
    let mut terminal = Terminal::new(TestBackend::new(4, 3)).unwrap();
    let mut state = TuiState::default();
    state.push_message(MessageRole::Assistant, "wide 世界");
    for (width, height) in [(4, 3), (1, 1), (0, 0)] {
        terminal.backend_mut().resize(width, height);
        terminal
            .draw(|frame| state.render_bentobox(frame, "zenpi"))
            .unwrap();
    }
}
