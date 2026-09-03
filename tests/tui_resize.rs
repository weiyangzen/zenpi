use ratatui::{Terminal, backend::TestBackend};
use std::time::{Duration, Instant};
use zenpi::tui::{MessageRole, RenderScheduler, TuiState};

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
