use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use std::time::{Duration, Instant};
use zenpi::tui::{MessageRole, RenderScheduler, TuiAction, TuiState};

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn zero_and_narrow_resize_are_panic_free() {
    let mut terminal = Terminal::new(TestBackend::new(4, 3)).unwrap();
    let mut state = TuiState::default();
    state.push_message(MessageRole::Assistant, "wide 世界");
    for (width, height) in [(4, 3), (1, 1), (0, 0)] {
        terminal.backend_mut().resize(width, height);
        terminal.draw(|frame| state.render(frame, "zenpi")).unwrap();
    }
}

#[test]
fn scheduler_coalesces_until_deadline() {
    let mut scheduler = RenderScheduler::new(Duration::from_millis(10));
    let start = Instant::now();
    assert!(scheduler.due(start));
    scheduler.rendered(start);
    for _ in 0..100 {
        scheduler.request();
    }
    assert!(!scheduler.due(start + Duration::from_millis(1)));
    assert!(scheduler.due(start + Duration::from_millis(10)));
}

#[test]
fn stream_chunks_are_bounded_and_coalesced() {
    let mut state = TuiState::new(2);
    state.append_stream(MessageRole::Assistant, "a");
    state.append_stream(MessageRole::Assistant, "b");
    assert_eq!(state.messages().next().unwrap().text, "ab");
}

#[test]
fn shifted_enter_and_ctrl_j_insert_multiline_text_without_submitting() {
    let mut state = TuiState::default();
    state.handle_key(key(KeyCode::Char('a'), KeyModifiers::NONE));
    assert_eq!(
        state.handle_key(key(KeyCode::Enter, KeyModifiers::SHIFT)),
        TuiAction::None
    );
    state.handle_key(key(KeyCode::Char('世'), KeyModifiers::NONE));
    assert_eq!(
        state.handle_key(key(KeyCode::Char('j'), KeyModifiers::CONTROL)),
        TuiAction::None
    );
    state.handle_key(key(KeyCode::Char('界'), KeyModifiers::NONE));
    assert_eq!(state.input(), "a\n世\n界");
    assert_eq!(state.cursor(), "a\n世\n界".len());

    let submitted = state.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(submitted, TuiAction::Submit("a\n世\n界".into()));
    assert!(state.input().is_empty());
}

#[test]
fn ctrl_c_interrupts_a_busy_turn_without_quitting_the_tui() {
    let mut state = TuiState::default();
    state.set_busy(true);

    assert_eq!(
        state.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        TuiAction::Interrupt
    );
    assert!(
        state.is_busy(),
        "the owner clears busy after processing cancel"
    );

    // Ctrl-D remains the explicit empty-prompt quit binding, while an idle
    // Ctrl-C keeps its historical quit behavior for terminal ergonomics.
    assert_eq!(
        state.handle_key(key(KeyCode::Char('d'), KeyModifiers::CONTROL)),
        TuiAction::Quit
    );
    state.set_busy(false);
    assert_eq!(
        state.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        TuiAction::Quit
    );
}

#[test]
fn unbound_control_keys_never_insert_printable_characters() {
    let mut state = TuiState::default();
    state.set_input("draft");

    assert_eq!(
        state.handle_key(key(KeyCode::Char('d'), KeyModifiers::CONTROL)),
        TuiAction::None
    );
    assert_eq!(
        state.handle_key(key(KeyCode::Char('a'), KeyModifiers::CONTROL)),
        TuiAction::None
    );
    assert_eq!(state.input(), "draft");
}

#[test]
fn tab_completes_slash_commands_without_touching_prompt_text() {
    let mut state = TuiState::default();
    state.set_input("/doc");
    assert_eq!(
        state.handle_key(key(KeyCode::Tab, KeyModifiers::NONE)),
        TuiAction::Redraw
    );
    assert_eq!(state.input(), "/doctor ");

    // A command prefix with several matches expands only to the common
    // canonical prefix; a second Tab completes the exact command and leaves
    // its argument slot ready for typing.
    state.set_input("/m");
    assert_eq!(
        state.handle_key(key(KeyCode::Tab, KeyModifiers::NONE)),
        TuiAction::Redraw
    );
    assert_eq!(state.input(), "/model");
    state.set_input("/mo");
    state.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(state.input(), "/model");
    state.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(state.input(), "/model ");

    // Tab remains inert for ordinary prompts, commands with an argument, and
    // a cursor positioned before the end of a draft.
    state.set_input("ordinary prompt");
    assert_eq!(
        state.handle_key(key(KeyCode::Tab, KeyModifiers::NONE)),
        TuiAction::None
    );
    assert_eq!(state.input(), "ordinary prompt");
    state.set_input("/doctor now");
    assert_eq!(
        state.handle_key(key(KeyCode::Tab, KeyModifiers::NONE)),
        TuiAction::None
    );
    assert_eq!(state.input(), "/doctor now");
    state.set_input("/doc");
    state.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(
        state.handle_key(key(KeyCode::Tab, KeyModifiers::NONE)),
        TuiAction::None
    );
    assert_eq!(state.input(), "/doc");
}

#[test]
fn tab_does_not_complete_after_multiline_leading_whitespace() {
    let mut state = TuiState::default();
    state.set_input("  /doc\nnext");
    assert_eq!(
        state.handle_key(key(KeyCode::Tab, KeyModifiers::NONE)),
        TuiAction::None
    );
    assert_eq!(state.input(), "  /doc\nnext");
}

#[test]
fn submit_preserves_prompt_whitespace() {
    let mut state = TuiState::default();
    state.set_input("  indented code  \n");
    assert_eq!(
        state.handle_key(key(KeyCode::Enter, KeyModifiers::NONE)),
        TuiAction::Submit("  indented code  \n".into())
    );
}

#[test]
fn multiline_cursor_editing_stays_on_utf8_boundaries() {
    let mut state = TuiState::default();
    state.set_input("ab\n世界");

    // End/Home are logical-line operations; vertical movement retains the
    // display column and never points inside a multibyte character.
    state.handle_key(key(KeyCode::Home, KeyModifiers::NONE));
    assert_eq!(state.cursor(), 3, "start of the second logical line");
    state.handle_key(key(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(state.cursor(), 6, "after the first wide glyph");
    state.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(state.cursor(), 2, "clamped to the first line's end");
    state.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(state.cursor(), 6, "back to the same display column");

    state.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(state.input(), "ab\n界");
    assert!(state.input().is_char_boundary(state.cursor()));
}

#[test]
fn multiline_prompt_grows_and_scrolls_with_narrow_viewport() {
    let mut terminal = Terminal::new(TestBackend::new(18, 12)).unwrap();
    let mut state = TuiState::default();
    state.set_input("one\ntwo\nthree\nfour\nfive\nsix\n世界");
    terminal.draw(|frame| state.render(frame, "zenpi")).unwrap();

    // Rendering a prompt taller than the historical one-row viewport must
    // remain panic-free and leave visible content in the test buffer.
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains('世'));
    assert!(rendered.contains('界'));
}

#[test]
fn session_projection_is_bounded_and_does_not_render_event_payloads() {
    use zenpi::session::SessionStore;
    use zenpi::tui::{MAX_SESSION_PANE_RECORDS, SessionPaneSnapshot};

    let directory = tempfile::tempdir().unwrap();
    let mut session = SessionStore::open(directory.path().join("projection.jsonl")).unwrap();
    for index in 0..100 {
        session
            .append_event(serde_json::json!({
                "type": "operation_finished\u{1b}[2J\nforged line",
                "payload": "secret-not-for-the-side-pane",
                "index": index,
            }))
            .unwrap();
    }
    let snapshot = SessionPaneSnapshot::from_session(&session);
    assert_eq!(snapshot.timeline().len(), MAX_SESSION_PANE_RECORDS);
    assert_eq!(snapshot.earlier_records(), 101 - MAX_SESSION_PANE_RECORDS);
    assert_eq!(snapshot.journal_next_sequence(), session.next_sequence());
    for line in snapshot.timeline() {
        assert!(!line.chars().any(char::is_control));
        assert!(!line.contains("secret-not-for-the-side-pane"));
        assert!(line.len() <= 20 + 1 + 64 * 4);
    }
}

#[test]
fn recovered_session_cursor_is_journal_evidence_not_a_transport_receipt() {
    use zenpi::layout::TabId;
    use zenpi::session::SessionStore;
    use zenpi::tui::SessionPaneSnapshot;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("journal.jsonl");
    let mut session = SessionStore::open(&path).unwrap();
    session
        .append_event(serde_json::json!({"type": "session_resumed", "replay_cursor": 0}))
        .unwrap();
    let before = SessionPaneSnapshot::from_session(&session);
    drop(session);
    let bytes = std::fs::read(&path).unwrap();
    let recovered = SessionStore::open_existing(&path).unwrap();
    assert_eq!(before, SessionPaneSnapshot::from_session(&recovered));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);

    let mut state = TuiState::default();
    state.refresh_session_snapshot(&recovered);
    state.set_workspace_tab(TabId::Session);
    let mut terminal = Terminal::new(TestBackend::new(200, 64)).unwrap();
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("Journal next sequence: 2"));
    assert!(rendered.contains("Transport ACK: unavailable"));
    assert!(rendered.contains("Reconnect: owner required"));
    assert!(rendered.contains("Mailbox: owner required"));
    assert!(!rendered.contains("delivered"));
}

#[test]
fn resume_and_session_switch_refresh_owned_snapshot_without_losing_draft() {
    use zenpi::core::Agent;
    use zenpi::session::SessionStore;
    use zenpi::slash::{SessionAction, SlashCommand};
    use zenpi::tui::dispatch_slash_command;

    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.jsonl");
    let second = directory.path().join("second.jsonl");
    let second_store = SessionStore::open(&second).unwrap();
    let second_id = second_store.session_id().to_owned();
    drop(second_store);
    let mut agent = Agent::with_echo(SessionStore::open(&first).unwrap());
    let mut state = TuiState::default();
    state.set_input("unfinished draft");
    state.push_message(MessageRole::User, "previous session text");
    dispatch_slash_command(
        SlashCommand::Resume { sequence: Some(0) },
        &mut state,
        Some(&mut agent),
    );
    assert_eq!(
        state.session_snapshot().unwrap().journal_next_sequence(),
        agent.session().next_sequence(),
    );
    let checkpoint = zenpi::headless::inspect_checkpoint(&agent);
    assert_eq!(
        state.session_snapshot().unwrap().journal_next_sequence(),
        checkpoint.cursor.next_sequence,
    );
    assert_eq!(
        state.session_snapshot().unwrap().session_id(),
        checkpoint.cursor.session_id,
    );
    assert!(!checkpoint.reconnect_supported);
    assert!(!checkpoint.acknowledgement_supported);
    assert_eq!(state.input(), "unfinished draft");
    dispatch_slash_command(
        SlashCommand::Session {
            action: SessionAction::Open {
                path: second.display().to_string(),
            },
        },
        &mut state,
        Some(&mut agent),
    );
    assert_eq!(state.session_snapshot().unwrap().session_id(), second_id);
    assert_eq!(state.input(), "unfinished draft");
    assert!(
        state
            .messages()
            .all(|message| !message.text.contains("previous session text"))
    );
}
