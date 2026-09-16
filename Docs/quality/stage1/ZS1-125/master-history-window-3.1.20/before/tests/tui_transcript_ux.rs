use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{Terminal, backend::TestBackend};
use zenpi::{
    backend::ProviderEvent,
    tui::{MessageRole, TuiAction, TuiState, terminal_clipboard_sequence},
    view_model::ViewEvent,
};
fn key(state: &mut TuiState, code: KeyCode, mods: KeyModifiers) -> TuiAction {
    state.handle_key(KeyEvent::new(code, mods))
}
fn render(state: &mut TuiState, width: u16, height: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(width, height)).unwrap();
    t.draw(|f| state.render_bentobox(f, "zenpi")).unwrap();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| t.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn event(state: &mut TuiState, job: u64, event: ProviderEvent) {
    let v = ViewEvent::from_provider_event(0, None, Some("turn-fixture"), None, &event).unwrap();
    state.apply_view_event_for_job(job, &v);
}
#[test]
fn reasoning_is_independent_foldable_and_never_part_of_answer_copy() {
    let mut state = TuiState::default();
    state.begin_stream_for_job(1);
    event(
        &mut state,
        1,
        ProviderEvent::ReasoningDelta {
            delta: "separate reasoning content".into(),
        },
    );
    event(
        &mut state,
        1,
        ProviderEvent::TextDelta {
            delta: "answer ".into(),
        },
    );
    event(
        &mut state,
        1,
        ProviderEvent::ReasoningDelta {
            delta: " continued".into(),
        },
    );
    event(
        &mut state,
        1,
        ProviderEvent::TextDelta {
            delta: "only".into(),
        },
    );
    assert_eq!(
        state
            .messages()
            .filter(|m| m.role == MessageRole::Assistant)
            .count(),
        1
    );
    state.finish_stream_for_job(1, MessageRole::Assistant, "final answer only");
    assert_eq!(
        state.copy_latest_answer().as_deref(),
        Some("final answer only")
    );
    let collapsed = render(&mut state, 140, 40);
    assert!(collapsed.contains("Reasoning"));
    assert!(!collapsed.contains("separate reasoning content"));
    key(&mut state, KeyCode::Char('r'), KeyModifiers::ALT);
    assert!(render(&mut state, 140, 40).contains("separate reasoning content"));
    state.begin_stream_for_job(2);
    event(
        &mut state,
        1,
        ProviderEvent::ReasoningDelta {
            delta: "stale discarded".into(),
        },
    );
    assert!(!state.messages().any(|m| m.text.contains("stale discarded")));
    assert_eq!(
        key(&mut state, KeyCode::Char('c'), KeyModifiers::ALT),
        TuiAction::Copy("final answer only".into())
    );
}
#[test]
fn scroll_anchor_stays_on_history_when_new_messages_arrive_and_end_resumes_following() {
    let mut state = TuiState::default();
    for i in 0..60 {
        state.push_message(MessageRole::Assistant, format!("line-{i:03}"));
    }
    render(&mut state, 140, 40);
    state.scroll_up(12);
    let before = render(&mut state, 140, 40);
    let first = before
        .lines()
        .find(|line| line.contains("line-"))
        .unwrap()
        .to_owned();
    state.push_message(MessageRole::Assistant, "new-live-line");
    let after = render(&mut state, 140, 40);
    assert_eq!(
        after.lines().find(|line| line.contains("line-")).unwrap(),
        first
    );
    assert!(after.contains("Latest"));
    key(&mut state, KeyCode::End, KeyModifiers::NONE);
    assert_eq!(state.scroll(), 0);
    assert!(render(&mut state, 140, 40).contains("new-live-line"));
}
#[test]
fn block_browser_copies_code_payload_and_escape_preserves_draft() {
    let mut state = TuiState::default();
    state.push_message(
        MessageRole::Assistant,
        "Answer\n```rust\nfn main() {}\n```\nDone",
    );
    state.set_input("draft untouched");
    key(&mut state, KeyCode::Char('b'), KeyModifiers::ALT);
    assert!(render(&mut state, 140, 40).contains("Inspect / copy"));
    key(&mut state, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(
        key(&mut state, KeyCode::Enter, KeyModifiers::NONE),
        TuiAction::Copy("fn main() {}".into())
    );
    assert_eq!(state.input(), "draft untouched");
    key(&mut state, KeyCode::Char('b'), KeyModifiers::ALT);
    state.handle_event(Event::Paste("must not enter draft".into()));
    key(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(state.input(), "draft untouched");
}
#[test]
fn clipboard_control_encoding_is_bounded_and_never_inlines_source_escape_sequences() {
    use base64::Engine;
    let text = "code 世界\n\x1b]0;untrusted\x07";
    let sequence = terminal_clipboard_sequence(text).unwrap();
    let payload = sequence
        .strip_prefix("\x1b]52;c;")
        .unwrap()
        .strip_suffix('\x07')
        .unwrap();
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(payload)
            .unwrap(),
        text.as_bytes()
    );
    assert!(!payload.contains('\x1b'));
    assert!(terminal_clipboard_sequence(&"a".repeat(64 * 1024 + 1)).is_err());
}
#[test]
fn reasoning_preference_survives_project_switch_and_checkpoint() {
    let mut state = TuiState::default();
    state.push_message(MessageRole::Reasoning, "first reasoning");
    state.toggle_reasoning();
    state.open_project_tab("second");
    state.push_message(MessageRole::Reasoning, "second reasoning");
    assert!(!render(&mut state, 140, 40).contains("second reasoning"));
    state.select_project_tab(0);
    assert!(render(&mut state, 140, 40).contains("first reasoning"));
    let saved = state.project_checkpoint();
    let mut restored = TuiState::default();
    assert!(restored.restore_project_checkpoint(&saved));
    assert!(render(&mut restored, 140, 40).contains("first reasoning"));
}
#[test]
fn narrow_project_arrows_remain_visible_and_mouse_navigates_hidden_previous_tabs() {
    let mut state = TuiState::default();
    state.open_project_tab("one");
    state.open_project_tab("two");
    let screen = render(&mut state, 40, 20);
    let top = screen.lines().next().unwrap();
    assert!(top.contains("[<]"));
    assert!(top.contains("[>]"));
    assert!(top.contains("[+]"));
    state.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 30,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(state.active_project(), "one");
    for w in 1..45 {
        let _ = render(&mut state, w, 8);
    }
}
#[test]
fn actual_usage_is_visible_and_stale_usage_does_not_replace_current_job() {
    let mut state = TuiState::default();
    state.begin_stream_for_job(8);
    event(
        &mut state,
        8,
        ProviderEvent::Usage {
            usage: zenpi::backend::Usage {
                input_tokens: 123,
                output_tokens: 45,
                total_tokens: 168,
            },
        },
    );
    event(
        &mut state,
        7,
        ProviderEvent::Usage {
            usage: zenpi::backend::Usage {
                input_tokens: 999,
                output_tokens: 999,
                total_tokens: 1998,
            },
        },
    );
    let screen = render(&mut state, 140, 40);
    assert!(screen.contains("in 123 out 45"));
    assert!(!screen.contains("999"));
}
#[test]
fn error_details_are_readable_in_scrollable_inspector_and_interrupt_still_works() {
    let mut state = TuiState::default();
    state.push_message(
        MessageRole::Error,
        (0..80)
            .map(|i| format!("failure detail {i:03}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    key(&mut state, KeyCode::Char('b'), KeyModifiers::ALT);
    for _ in 0..20 {
        key(&mut state, KeyCode::PageDown, KeyModifiers::NONE);
        render(&mut state, 80, 20);
    }
    assert!(render(&mut state, 80, 20).contains("failure detail 079"));
    state.set_busy(true);
    assert_eq!(
        key(&mut state, KeyCode::Char('c'), KeyModifiers::CONTROL),
        TuiAction::Interrupt
    );
}

#[test]
fn directory_picker_owns_transcript_shortcuts_until_closed() {
    let mut state = TuiState::default();
    state.push_message(MessageRole::Assistant, "existing answer");
    state.open_directory_picker();
    assert_ne!(
        key(&mut state, KeyCode::Char('c'), KeyModifiers::ALT),
        TuiAction::Copy("existing answer".into())
    );
    key(&mut state, KeyCode::Char('b'), KeyModifiers::ALT);
    let screen = render(&mut state, 140, 40);
    assert!(screen.contains("Open project folder"));
    assert!(!screen.contains("Inspect / copy"));
    key(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(
        key(&mut state, KeyCode::Char('c'), KeyModifiers::ALT),
        TuiAction::Copy("existing answer".into())
    );
}
