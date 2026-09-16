use ratatui::{Terminal, backend::TestBackend};
use zenpi::tui::{MessageRole, TuiState};

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
fn tui_transcript_renders_basic_markdown_blocks() {
    let mut terminal = Terminal::new(TestBackend::new(64, 18)).unwrap();
    let mut state = TuiState::default();
    state.push_message(
        MessageRole::Assistant,
        "# Plan\n\nUse **small** steps.\n\n```rust\nlet ready = true;\n```\n\n- ship it",
    );
    terminal.draw(|frame| state.render(frame, "zenpi")).unwrap();
    let text = rendered(&terminal);
    assert!(text.contains("Plan"));
    assert!(text.contains("small"));
    assert!(text.contains("let ready = true;"));
    assert!(text.contains("- ship it"));
    assert!(!text.contains("**"));
}

#[test]
fn tui_markdown_rendering_stays_inside_a_one_cell_viewport() {
    let mut terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
    let mut state = TuiState::default();
    state.push_message(MessageRole::Assistant, "# 世界 **wide**");
    terminal.draw(|frame| state.render(frame, "zenpi")).unwrap();
}

#[test]
fn user_and_tool_text_remain_literal() {
    // Leave enough transcript height for both literal messages; the real TUI
    // intentionally anchors the view at the newest line when it is shorter
    // than the viewport.
    let mut terminal = Terminal::new(TestBackend::new(48, 18)).unwrap();
    let mut state = TuiState::default();
    state.push_message(MessageRole::User, "# not a heading\n---");
    state.push_message(MessageRole::Tool, "```not a fence```\n- raw");
    terminal.draw(|frame| state.render(frame, "zenpi")).unwrap();
    let text = rendered(&terminal);
    assert!(text.contains("# not a heading"));
    assert!(text.contains("---"));
    assert!(text.contains("```not a fence```"));
}

#[test]
fn pasted_prompt_control_bytes_are_not_emitted_to_the_terminal() {
    let mut state = TuiState::default();
    state.set_input("before\u{1b}[2J\t after\r\nnext");
    assert_eq!(state.input(), "before?[2J     after\nnext");
}
