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
fn newest_reply_remains_visible_beyond_the_transcript_line_budget() {
    let mut state = TuiState::default();
    for message in 0..3 {
        state.push_message(
            MessageRole::User,
            (0..6000)
                .map(|line| format!("history-{message}-{line:04}\n"))
                .collect::<String>(),
        );
    }
    state.push_message(MessageRole::Assistant, "LATEST_REPLY_AFTER_18000_ROWS");
    assert!(render(&mut state, 140, 40).contains("LATEST_REPLY_AFTER_18000_ROWS"));
    assert_eq!(
        state.message_count(),
        4,
        "rendering must not delete journal projections"
    );
    assert_eq!(
        state.copy_latest_answer().as_deref(),
        Some("LATEST_REPLY_AFTER_18000_ROWS")
    );
}

#[test]
fn rolling_render_window_keeps_the_reading_position_when_new_rows_arrive() {
    let mut state = TuiState::default();
    for message in 0..400 {
        state.push_message(
            MessageRole::User,
            (0..32)
                .map(|line| format!("history-{message:03}-{line:02}\n"))
                .collect::<String>(),
        );
    }
    render(&mut state, 140, 40);
    state.scroll_up(50);
    let before = render(&mut state, 140, 40);
    let first = before
        .lines()
        .find(|line| line.contains("history-"))
        .unwrap()
        .to_owned();
    state.push_message(MessageRole::Assistant, "NEW_ROW_AFTER_WINDOW_ROLLED");
    let after = render(&mut state, 140, 40);
    assert_eq!(
        after
            .lines()
            .find(|line| line.contains("history-"))
            .unwrap(),
        first
    );
    assert!(after.contains("Latest"));
    key(&mut state, KeyCode::End, KeyModifiers::NONE);
    assert!(render(&mut state, 140, 40).contains("NEW_ROW_AFTER_WINDOW_ROLLED"));
}

#[test]
fn reading_anchor_survives_queue_eviction_and_project_switch() {
    let mut state = TuiState::new(3);
    for message in 0..3 {
        state.push_message(
            MessageRole::User,
            (0..40)
                .map(|line| format!("retained-{message}-{line:02}\n"))
                .collect::<String>(),
        );
    }
    render(&mut state, 140, 40);
    state.scroll_up(20);
    let before = render(&mut state, 140, 40);
    let first = before
        .lines()
        .find(|line| line.contains("retained-"))
        .unwrap()
        .to_owned();
    state.push_message(MessageRole::Assistant, "replacement message evicts oldest");
    assert_eq!(state.message_count(), 3);
    assert!(state.open_project_tab("other"));
    state.push_message(MessageRole::Assistant, "other project");
    render(&mut state, 140, 40);
    state.select_project_tab(0);
    let after = render(&mut state, 140, 40);
    assert_eq!(
        after
            .lines()
            .find(|line| line.contains("retained-"))
            .unwrap(),
        first
    );
}

#[test]
fn clipped_history_has_a_visible_notice_and_latest_survives_narrow_resize() {
    let mut state = TuiState::default();
    for _ in 0..3 {
        state.push_message(MessageRole::User, "old row\n".repeat(6000));
    }
    state.push_message(MessageRole::Assistant, "LATEST_AFTER_RESIZE");
    render(&mut state, 140, 40);
    state.scroll_up(usize::MAX);
    assert!(render(&mut state, 140, 40).contains("Earlier rows omitted"));
    render(&mut state, 1, 8);
    key(&mut state, KeyCode::End, KeyModifiers::NONE);
    assert!(render(&mut state, 140, 40).contains("LATEST_AFTER_RESIZE"));
    assert_eq!(state.message_count(), 4);
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

#[test]
fn single_message_tail_is_visible_for_plain_markdown_and_fenced_code() {
    for (role, opening, closing) in [
        (MessageRole::User, "", ""),
        (MessageRole::Assistant, "# Heading\n", ""),
        (MessageRole::Assistant, "```rust\n", "\n```"),
    ] {
        let text = format!(
            "{opening}{}SINGLE_MESSAGE_LATEST{closing}",
            (0..9000)
                .map(|i| format!("row-{i:04}\n"))
                .collect::<String>()
        );
        assert!(text.len() < 256 * 1024);
        let mut state = TuiState::default();
        state.push_message(role, text.clone());
        assert!(render(&mut state, 140, 40).contains("SINGLE_MESSAGE_LATEST"));
        assert_eq!(state.messages().next().unwrap().text, text);
        assert_eq!(state.message_count(), 1);
        state.scroll_up(usize::MAX);
        let oldest = render(&mut state, 140, 40);
        assert!(oldest.contains("Earlier rows omitted"));
        assert!(!oldest.contains("row-0000"));
        key(&mut state, KeyCode::End, KeyModifiers::NONE);
        assert!(render(&mut state, 140, 40).contains("SINGLE_MESSAGE_LATEST"));
    }
}

#[test]
fn single_stream_keeps_its_reading_anchor_across_tail_roll_and_budget_crossing() {
    for initial_rows in [8180, 9000] {
        for (role, opening) in [
            (MessageRole::User, ""),
            (MessageRole::Assistant, "```text\n"),
        ] {
            let mut state = TuiState::default();
            state.begin_stream_for_job(77);
            state.append_stream_for_job(
                77,
                role,
                &format!(
                    "{opening}{}",
                    (0..initial_rows)
                        .map(|i| format!("stream-{i:05}\n"))
                        .collect::<String>()
                ),
            );
            render(&mut state, 140, 40);
            state.scroll_up(40);
            let before = render(&mut state, 140, 40);
            let first = before
                .lines()
                .find(|line| line.contains("stream-"))
                .unwrap()
                .to_owned();
            for batch in 0..3 {
                state.append_stream_for_job(
                    77,
                    role,
                    &(0..100)
                        .map(|i| format!("append-{batch}-{i:03}\n"))
                        .collect::<String>(),
                );
                let after = render(&mut state, 140, 40);
                assert_eq!(
                    after.lines().find(|line| line.contains("stream-")).unwrap(),
                    first
                );
                assert!(after.contains("Latest"));
                assert_eq!(state.message_count(), 1);
            }
            state.append_stream_for_job(77, role, "STREAM_TAIL_LATEST");
            key(&mut state, KeyCode::End, KeyModifiers::NONE);
            assert!(render(&mut state, 140, 40).contains("STREAM_TAIL_LATEST"));
            assert_eq!(state.scroll(), 0);
        }
    }
}

#[test]
fn wrapped_single_physical_line_and_narrow_reflow_retain_latest_payload() {
    for role in [MessageRole::User, MessageRole::Assistant] {
        let text = format!("{}TAIL_WRAP_LATEST", "界e\u{301} ".repeat(20000));
        assert!(text.len() < 256 * 1024);
        let mut state = TuiState::default();
        state.push_message(role, text.clone());
        // One source line wraps beyond 8192 visual rows in these narrow panes.
        for width in [3, 4, 5, 8, 12] {
            let mut terminal = Terminal::new(TestBackend::new(width, 16)).unwrap();
            terminal.draw(|f| state.render(f, "zenpi")).unwrap();
            let cells = terminal.backend().buffer();
            let screen = (0..16)
                .map(|y| {
                    (1..width - 1)
                        .map(|x| cells[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<String>();
            assert!(
                screen
                    .chars()
                    .filter(|ch| !ch.is_whitespace())
                    .collect::<String>()
                    .contains("LATEST"),
                "latest suffix should survive at width {width}"
            );
            assert_eq!(state.messages().next().unwrap().text, text);
        }
        // Width changes rebuild the layout; End requests its actual latest row.
        key(&mut state, KeyCode::End, KeyModifiers::NONE);
        assert!(render(&mut state, 140, 40).contains("TAIL_WRAP_LATEST"));
    }
}
