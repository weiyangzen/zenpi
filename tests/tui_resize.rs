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
