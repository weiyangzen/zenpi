use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use zenpi::tui::{MessageRole, ToolRunStatus, TuiAction, TuiState};

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
fn tool_lifecycle_updates_one_bounded_transcript_entry() {
    let mut state = TuiState::new(8);
    state.tool_call_started("call-1", "run_command");
    assert_eq!(state.message_count(), 1);
    assert!(state.messages().next().unwrap().text.contains("[running]"));

    state.tool_call_finished("call-1", ToolRunStatus::Succeeded);
    assert_eq!(state.message_count(), 1);
    let message = state.messages().next().unwrap();
    assert_eq!(message.role, MessageRole::Tool);
    assert!(message.text.contains("run_command"));
    assert!(message.text.contains("[ok]"));
}

#[test]
fn folded_tool_logs_render_a_summary_and_restore_on_toggle() {
    let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
    let mut state = TuiState::new(8);
    state.push_message(MessageRole::Assistant, "answer");
    state.tool_call_started("call-1", "read_file");
    state.tool_call_finished("call-1", ToolRunStatus::Failed);
    terminal.draw(|frame| state.render(frame, "zenpi")).unwrap();
    let expanded = rendered(&terminal);
    assert!(expanded.contains("read_file"));
    assert!(expanded.contains("[failed]"));

    state.set_tool_logs_folded(true);
    terminal.draw(|frame| state.render(frame, "zenpi")).unwrap();
    let folded = rendered(&terminal);
    assert!(!folded.contains("read_file"));
    assert!(folded.contains("1 tool log folded"));

    state.set_tool_logs_folded(false);
    terminal.draw(|frame| state.render(frame, "zenpi")).unwrap();
    assert!(rendered(&terminal).contains("read_file"));
}

#[test]
fn ctrl_o_toggles_tool_log_folding_without_submitting_prompt() {
    let mut state = TuiState::default();
    let key = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL);
    assert_eq!(state.handle_key(key), TuiAction::Redraw);
    assert!(state.tool_logs_folded());
    assert_eq!(state.handle_key(key), TuiAction::Redraw);
    assert!(!state.tool_logs_folded());
}

#[test]
fn unresolved_tool_entries_can_be_marked_cancelled() {
    let mut state = TuiState::default();
    state.tool_call_started("call-2", "run_command");
    state.finish_running_tools(ToolRunStatus::Cancelled);
    assert!(
        state
            .messages()
            .next()
            .unwrap()
            .text
            .contains("[cancelled]")
    );
}

#[test]
fn tool_ids_with_brackets_do_not_update_a_prefix_collision() {
    let mut state = TuiState::default();
    state.tool_call_started("a", "first");
    state.tool_call_started("a] extra", "second");
    state.tool_call_finished("a] extra", ToolRunStatus::Succeeded);
    let messages = state
        .messages()
        .map(|message| message.text.clone())
        .collect::<Vec<_>>();
    assert!(messages.iter().any(|text| text.contains("first [running]")));
    assert!(messages.iter().any(|text| text.contains("second [ok]")));
}

#[test]
fn folded_logs_do_not_hide_a_pending_approval_prompt() {
    let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
    let mut state = TuiState::default();
    state.push_message(
        MessageRole::System,
        "Approval required: run_command {\"command\":\"ls\"}",
    );
    state.tool_call_started("call-3", "run_command");
    state.set_tool_logs_folded(true);
    terminal.draw(|frame| state.render(frame, "zenpi")).unwrap();
    let output = rendered(&terminal);
    assert!(output.contains("Approval required"));
    assert!(output.contains("1 tool log folded"));
}
