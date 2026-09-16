use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use zenpi::tui::{MAX_MESSAGE_BYTES, TuiAction, TuiState};
fn key(s: &mut TuiState, code: KeyCode, modifiers: KeyModifiers) -> TuiAction {
    s.handle_key(KeyEvent::new(code, modifiers))
}
fn ctrl(s: &mut TuiState, c: char) -> TuiAction {
    key(s, KeyCode::Char(c), KeyModifiers::CONTROL)
}
fn seed(s: &mut TuiState, text: &str) {
    s.set_input(text);
    assert!(matches!(
        key(s, KeyCode::Enter, KeyModifiers::NONE),
        TuiAction::Submit(_)
    ));
}
#[test]
fn grapheme_cursor_and_deletion_keep_combining_emoji_and_flags_whole() {
    let mut s = TuiState::default();
    s.set_input("a👨‍👩‍👧‍👦e\u{301}🇯🇵z");
    key(&mut s, KeyCode::Left, KeyModifiers::NONE);
    key(&mut s, KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(s.input(), "a👨‍👩‍👧‍👦e\u{301}z");
    key(&mut s, KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(s.input(), "a👨‍👩‍👧‍👦z");
    key(&mut s, KeyCode::Left, KeyModifiers::NONE);
    assert_eq!(s.cursor(), 1);
    key(&mut s, KeyCode::Delete, KeyModifiers::NONE);
    assert_eq!(s.input(), "az");
}
#[test]
fn multiline_word_and_line_commands_keep_wide_columns() {
    let mut s = TuiState::default();
    s.set_input("alpha 世界 beta\n中ab\n12345");
    ctrl(&mut s, 'a');
    assert_eq!(s.cursor(), "alpha 世界 beta\n中ab\n".len());
    key(&mut s, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(s.cursor(), "alpha 世界 beta\n".len());
    ctrl(&mut s, 'e');
    key(&mut s, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(&s.input()[..s.cursor()], "alph");
    key(&mut s, KeyCode::Right, KeyModifiers::CONTROL);
    assert_eq!(&s.input()[..s.cursor()], "alpha ");
    key(&mut s, KeyCode::Delete, KeyModifiers::CONTROL);
    assert!(s.input().starts_with("alpha beta"));
    ctrl(&mut s, 'k');
    assert!(s.input().starts_with("alpha \n"));
}
#[test]
fn history_navigation_restores_original_draft_and_cursor() {
    let mut s = TuiState::default();
    seed(&mut s, "first");
    seed(&mut s, "second");
    s.set_input("unsent 世界 draft");
    key(&mut s, KeyCode::Left, KeyModifiers::NONE);
    let cursor = s.cursor();
    ctrl(&mut s, 'p');
    assert_eq!(s.input(), "second");
    key(&mut s, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(s.input(), "first");
    key(&mut s, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(s.input(), "second");
    ctrl(&mut s, 'n');
    assert_eq!(s.input(), "unsent 世界 draft");
    assert_eq!(s.cursor(), cursor);
    ctrl(&mut s, 'p');
    key(&mut s, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(s.input(), "unsent 世界 draft");
}
#[test]
fn search_accept_never_submits_and_cancel_restores_persisted_draft() {
    let mut s = TuiState::default();
    seed(&mut s, "alpha old");
    seed(&mut s, "beta");
    seed(&mut s, "alpha new");
    s.set_input("saved draft");
    ctrl(&mut s, 'r');
    s.handle_event(Event::Paste("alpha".into()));
    assert_eq!(s.input(), "alpha new");
    ctrl(&mut s, 'r');
    assert_eq!(s.input(), "alpha old");
    let checkpoint = s.project_checkpoint();
    let mut restored = TuiState::default();
    assert!(restored.restore_project_tabs(&checkpoint));
    assert_eq!(restored.input(), "saved draft");
    assert!(matches!(
        key(&mut s, KeyCode::Enter, KeyModifiers::NONE),
        TuiAction::Redraw
    ));
    assert_eq!(s.input(), "alpha old");
    assert!(matches!(
        key(&mut s, KeyCode::Enter, KeyModifiers::NONE),
        TuiAction::Submit(_)
    ));
    s.set_input("keep me");
    ctrl(&mut s, 'r');
    s.handle_event(Event::Paste("missing value".into()));
    key(&mut s, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(s.input(), "keep me");
}
#[test]
fn history_preview_cannot_leak_across_projects() {
    let mut s = TuiState::default();
    seed(&mut s, "first project secret");
    s.set_input("original draft");
    ctrl(&mut s, 'r');
    assert_eq!(s.input(), "first project secret");
    s.open_project_tab("second");
    ctrl(&mut s, 'p');
    assert_eq!(s.input(), "");
    s.select_project_tab(0);
    assert_eq!(s.input(), "first project secret");
    key(&mut s, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(s.input(), "original draft");
}
#[test]
fn paste_is_literal_and_limit_failure_is_atomic() {
    let mut s = TuiState::default();
    assert!(matches!(
        s.handle_event(Event::Paste("/exit\r\n!touch forbidden\u{1b}[A".into())),
        TuiAction::None
    ));
    assert_eq!(s.input(), "/exit\n!touch forbidden?[A");
    assert!(s.slash_choices().is_empty());
    s.set_input("draft");
    s.handle_event(Event::Paste("界".repeat(MAX_MESSAGE_BYTES)));
    assert_eq!(s.input(), "draft");
    assert!(s.status().contains("draft unchanged"));
    s.set_input("a".repeat(MAX_MESSAGE_BYTES - 1));
    s.handle_event(Event::Paste("界".into()));
    assert_eq!(s.input().len(), MAX_MESSAGE_BYTES - 1);
}
#[test]
fn escape_keeps_draft_and_key_release_never_types_or_submits() {
    let mut s = TuiState::default();
    s.set_input("keep this");
    key(&mut s, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(s.input(), "keep this");
    let mut event = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    event.kind = KeyEventKind::Release;
    assert!(matches!(s.handle_key(event), TuiAction::None));
    assert_eq!(s.input(), "keep this");
    ctrl(&mut s, 'j');
    assert_eq!(s.input(), "keep this\n");
}

#[test]
fn scheduled_fifo_edit_cancel_and_revision_use_actual_project_bound_entries() {
    use zenpi::tui::TuiPendingInputs;
    let mut s = TuiState::default();
    let first_project = s.active_project().to_owned();
    let mut queue = TuiPendingInputs::default();
    queue.admit_prompt("original text".into(), &mut s);
    let entry = s.project_checkpoint()["scheduled_inputs"][0].clone();
    let id = entry["id"].as_str().unwrap();
    assert!(!entry["interrupted"].as_bool().unwrap());
    queue.scheduled_command(&mut s, &format!("/scheduled edit {id} 0   changed\ntext\n"));
    let entry = s.project_checkpoint()["scheduled_inputs"][0].clone();
    assert_eq!(entry["text"], "  changed\ntext\n");
    assert_eq!(entry["revision"], 1);
    let stale = format!("/scheduled edit {id} 0 wrong");
    queue.scheduled_command(&mut s, &stale);
    assert_eq!(s.input(), stale);
    assert_eq!(s.project_checkpoint()["scheduled_inputs"][0], entry);
    s.open_project_tab("other");
    queue.scheduled_command(&mut s, &format!("/scheduled cancel {id}"));
    assert_eq!(s.project_checkpoint()["scheduled_inputs"][0], entry);
    s.select_project_tab(s.project_index(&first_project).unwrap());
    queue.scheduled_command(&mut s, &format!("/scheduled cancel {id}"));
    assert!(
        s.project_checkpoint()["scheduled_inputs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    queue.scheduled_command(&mut s, &format!("/scheduled edit {id} 1 too late"));
    assert!(s.input().ends_with("too late"));
}

#[test]
fn scheduled_restart_is_explicitly_unconfirmed_and_never_automatically_readmitted() {
    use zenpi::tui::TuiPendingInputs;
    let mut s = TuiState::default();
    let mut queue = TuiPendingInputs::default();
    queue.admit_prompt("retained future work".into(), &mut s);
    let checkpoint = s.project_checkpoint();
    let id = checkpoint["scheduled_inputs"][0]["id"].as_str().unwrap();
    let mut restored = TuiState::default();
    assert!(restored.restore_project_checkpoint(&checkpoint));
    assert_eq!(
        restored.project_checkpoint()["scheduled_inputs"][0]["interrupted"],
        true
    );
    let mut new_queue = TuiPendingInputs::default();
    new_queue.scheduled_command(&mut restored, "/scheduled list");
    assert!(
        restored
            .messages()
            .any(|message| message.text.contains("execution unconfirmed"))
    );
    new_queue.scheduled_command(&mut restored, &format!("/scheduled retry {id}"));
    let rows = restored.project_checkpoint()["scheduled_inputs"].clone();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["text"], "retained future work");
    assert_eq!(rows[0]["interrupted"], false);
    assert_ne!(rows[0]["id"], id);
}

#[test]
fn actual_owner_receipts_drive_queue_menu_and_lossless_edits() {
    use std::sync::{Arc, Mutex};
    use zenpi::{
        core::Agent, input_queue::InputKind, protocol::InputQueueAction as Action,
        session::SessionStore, tui::TuiInputControls,
    };
    let temp = tempfile::tempdir().unwrap();
    let session =
        SessionStore::open_in_workspace(temp.path().join("inputs.jsonl"), temp.path()).unwrap();
    let owner = Arc::new(Mutex::new(Agent::with_echo(session)));
    let mut state = TuiState::default();
    let mut controls = TuiInputControls::default();
    controls.submit(
        &mut state,
        &owner,
        Action::Enqueue {
            input_id: "real-input".into(),
            kind: InputKind::FollowUp,
            text: "  original\n".into(),
        },
        "/input follow-up real-input   original\n".into(),
    );
    assert!(
        state
            .messages()
            .any(|message| message.text.contains("Input owner confirmed")
                && message.text.contains("real-input"))
    );
    state.set_input("/input edit real");
    assert!(
        state
            .slash_choices()
            .iter()
            .any(|choice| choice == "/input edit real-input 0   original\n")
    );
    let command = "/input edit real-input 0   edited\r\n";
    let action = zenpi::slash::input_queue_control(command).unwrap().unwrap();
    controls.submit(&mut state, &owner, action, command.into());
    state.set_input("/input edit real");
    assert!(
        state
            .slash_choices()
            .iter()
            .any(|choice| choice == "/input edit real-input 1   edited\r\n")
    );
    let before = std::fs::read(owner.lock().unwrap().session().path()).unwrap();
    controls.submit(
        &mut state,
        &owner,
        Action::Edit {
            input_id: "real-input".into(),
            expected_revision: 0,
            text: "stale".into(),
        },
        "invalid revision".into(),
    );
    assert_eq!(
        std::fs::read(owner.lock().unwrap().session().path()).unwrap(),
        before
    );
    controls.submit(
        &mut state,
        &owner,
        Action::Cancel {
            input_id: "real-input".into(),
        },
        "/input cancel real-input".into(),
    );
    state.set_input("/input cancel real");
    assert!(state.slash_choices().is_empty());
    state.open_project_tab("other");
    state.set_input("/input edit ");
    assert!(state.slash_choices().is_empty());
}

#[test]
fn text_only_input_owner_never_silently_loses_file_attachments() {
    use std::sync::{Arc, Mutex};
    use zenpi::{
        core::Agent, input_queue::InputKind, protocol::InputQueueAction, session::SessionStore,
        tui::TuiInputControls,
    };
    let temp = tempfile::tempdir().unwrap();
    let session =
        SessionStore::open_in_workspace(temp.path().join("inputs.jsonl"), temp.path()).unwrap();
    let owner = Arc::new(Mutex::new(Agent::with_echo(session)));
    let before = std::fs::read(owner.lock().unwrap().session().path()).unwrap();
    let mut state = TuiState::default();
    let mut controls = TuiInputControls::default();
    let original = "/input steer files inspect @file.txt";
    controls.submit(
        &mut state,
        &owner,
        InputQueueAction::Enqueue {
            input_id: "files".into(),
            kind: InputKind::Steer,
            text: "inspect @file.txt".into(),
        },
        original.into(),
    );
    assert_eq!(state.input(), original);
    assert_eq!(
        std::fs::read(owner.lock().unwrap().session().path()).unwrap(),
        before
    );
    assert!(
        state
            .messages()
            .any(|message| message.text.contains("text-only"))
    );
}

#[test]
fn legacy_checkpoint_cursor_inside_combined_glyph_is_normalized() {
    let mut state = TuiState::default();
    state.set_input("a👨‍👩‍👧‍👦z");
    let mut checkpoint = state.project_checkpoint();
    checkpoint["project_state"][0]["draft"]["cursor"] = serde_json::json!(5);
    let mut restored = TuiState::default();
    assert!(restored.restore_project_checkpoint(&checkpoint));
    assert_eq!(restored.cursor(), 1);
    key(&mut restored, KeyCode::Delete, KeyModifiers::NONE);
    assert_eq!(restored.input(), "az");
}

#[test]
fn maximum_size_single_word_navigation_is_bounded_and_complete() {
    let mut state = TuiState::default();
    state.set_input("a".repeat(MAX_MESSAGE_BYTES));
    key(&mut state, KeyCode::Left, KeyModifiers::CONTROL);
    assert_eq!(state.cursor(), 0);
    key(&mut state, KeyCode::Right, KeyModifiers::CONTROL);
    assert_eq!(state.cursor(), MAX_MESSAGE_BYTES);
}

#[test]
fn interrupted_and_live_scheduled_jobs_share_one_byte_budget() {
    use zenpi::tui::TuiPendingInputs;
    const CAP: usize = 256 * 1024;
    let mut state = TuiState::default();
    let mut queue = TuiPendingInputs::default();
    queue.admit_prompt("r".repeat(CAP - 8), &mut state);
    let mut restored = TuiState::default();
    assert!(restored.restore_project_checkpoint(&state.project_checkpoint()));
    let mut live = TuiPendingInputs::default();
    live.admit_prompt("live".into(), &mut restored);
    let before = restored.project_checkpoint()["scheduled_inputs"].clone();
    assert_eq!(before.as_array().unwrap().len(), 2);
    let id = before[1]["id"].as_str().unwrap();
    live.admit_prompt("too large".into(), &mut restored);
    assert_eq!(restored.project_checkpoint()["scheduled_inputs"], before);
    let edit = format!("/scheduled edit {id} 0 oversized");
    live.scheduled_command(&mut restored, &edit);
    assert_eq!(restored.project_checkpoint()["scheduled_inputs"], before);
    assert_eq!(restored.input(), edit);
    live.scheduled_command(&mut restored, &format!("/scheduled edit {id} 0 12345678"));
    let full = restored.project_checkpoint()["scheduled_inputs"].clone();
    assert_eq!(full[1]["revision"], 1);
    let mut reopened = TuiState::default();
    assert!(reopened.restore_project_checkpoint(&restored.project_checkpoint()));
    let retained = reopened.project_checkpoint()["scheduled_inputs"].clone();
    assert_eq!(retained.as_array().unwrap().len(), 2);
    assert_eq!(retained[0]["text"], full[0]["text"]);
    assert_eq!(retained[1]["text"], full[1]["text"]);
}

fn timed(s: &mut TuiState, code: KeyCode, at: std::time::Instant) -> TuiAction {
    s.handle_event_at(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)), at)
}
#[test]
fn ordinary_ascii_hold_flush_and_deliberate_enter_have_bounded_latency() {
    use std::time::{Duration, Instant};
    let mut s = TuiState::default();
    let start = Instant::now();
    timed(&mut s, KeyCode::Char('a'), start);
    assert_eq!(s.input(), "");
    let interval = if cfg!(windows) { 30 } else { 8 };
    assert!(!s.flush_ordinary_paste(start + Duration::from_millis(interval)));
    assert!(s.flush_ordinary_paste(start + Duration::from_millis(interval + 1)));
    assert_eq!(s.input(), "a");
    assert!(
        matches!(timed(&mut s, KeyCode::Enter, start + Duration::from_millis(200)), TuiAction::Submit(text) if text == "a")
    );
}
#[test]
fn ordinary_multiline_burst_and_trailing_enter_never_submit_before_quiet_window() {
    use std::time::{Duration, Instant};
    let mut s = TuiState::default();
    let start = Instant::now();
    let literal = "/exit\n!touch never-run\n世界";
    for (i, ch) in literal.chars().enumerate() {
        let code = if ch == '\n' {
            KeyCode::Enter
        } else {
            KeyCode::Char(ch)
        };
        assert!(matches!(
            timed(&mut s, code, start + Duration::from_micros(i as u64)),
            TuiAction::None
        ));
    }
    assert_eq!(s.input(), "");
    assert!(s.flush_ordinary_paste(start + Duration::from_millis(70)));
    assert_eq!(s.input(), literal);
    assert!(matches!(
        timed(&mut s, KeyCode::Enter, start + Duration::from_millis(90)),
        TuiAction::None
    ));
    s.flush_ordinary_paste(start + Duration::from_millis(160));
    assert_eq!(s.input(), format!("{literal}\n"));
    assert!(
        matches!(timed(&mut s, KeyCode::Enter, start + Duration::from_millis(300)), TuiAction::Submit(text) if text == format!("{literal}\n"))
    );
}
#[test]
fn ordinary_unicode_ime_is_immediate_and_modified_keys_flush_without_losing_text() {
    use std::time::{Duration, Instant};
    let mut s = TuiState::default();
    let start = Instant::now();
    timed(&mut s, KeyCode::Char('中'), start);
    assert_eq!(s.input(), "中");
    for ch in "文e\u{301}👨‍👩‍👧‍👦".chars() {
        timed(&mut s, KeyCode::Char(ch), start);
    }
    timed(&mut s, KeyCode::Left, start);
    assert_eq!(s.input(), "中文e\u{301}👨‍👩‍👧‍👦");
    timed(&mut s, KeyCode::Backspace, start);
    assert_eq!(s.input(), "中文👨‍👩‍👧‍👦");
    timed(
        &mut s,
        KeyCode::Char('x'),
        start + Duration::from_millis(200),
    );
    timed(&mut s, KeyCode::Esc, start + Duration::from_millis(201));
    assert_eq!(s.input(), "中文x👨‍👩‍👧‍👦");
    assert!(!s.flush_ordinary_paste(start + Duration::from_secs(1)));
}
#[test]
fn ordinary_burst_cancel_history_and_project_switch_preserve_the_correct_draft() {
    use std::time::{Duration, Instant};
    let mut s = TuiState::default();
    seed(&mut s, "old history");
    s.set_input("draft ");
    let start = Instant::now();
    for ch in "pending".chars() {
        timed(&mut s, KeyCode::Char(ch), start);
    }
    let interrupt = s.handle_event_at(
        Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        start,
    );
    assert!(matches!(interrupt, TuiAction::Interrupt));
    assert_eq!(s.input(), "draft pending");
    s.handle_event_at(
        Event::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)),
        start,
    );
    timed(&mut s, KeyCode::Char('o'), start);
    timed(&mut s, KeyCode::Esc, start);
    assert_eq!(s.input(), "draft pending");
    timed(&mut s, KeyCode::Char('!'), start + Duration::from_secs(1));
    assert!(s.open_project_tab("second"));
    assert_eq!(s.input(), "");
    assert!(s.select_project_tab(0));
    assert_eq!(s.input(), "draft pending!");
    let mut restored = TuiState::default();
    assert!(restored.restore_project_tabs(&s.project_checkpoint()));
    assert_eq!(restored.input(), "draft pending!");
    timed(&mut s, KeyCode::Char('x'), start + Duration::from_secs(2));
    s.set_input("restored after failure");
    s.flush_ordinary_paste(start + Duration::from_secs(3));
    assert_eq!(s.input(), "restored after failure");
}
#[test]
fn ordinary_burst_is_bounded_and_rejects_overflow_without_partial_insertion() {
    use std::time::{Duration, Instant};
    let mut s = TuiState::default();
    let before = "z".repeat(MAX_MESSAGE_BYTES - 4);
    s.set_input(&before);
    let start = Instant::now();
    for _ in 0..MAX_MESSAGE_BYTES {
        timed(&mut s, KeyCode::Char('x'), start);
    }
    assert!(s.flush_ordinary_paste(start + Duration::from_millis(70)));
    assert_eq!(s.input(), before);
    assert!(s.status().contains("draft unchanged"));
}

#[test]
fn deliberate_tab_reopens_completion_after_a_literal_ordinary_burst() {
    let mut s = TuiState::default();
    let start = std::time::Instant::now();
    for ch in "/per".chars() {
        timed(&mut s, KeyCode::Char(ch), start);
    }
    timed(&mut s, KeyCode::Tab, start);
    assert!(s.input().starts_with("/persona"), "{}", s.input());
}

#[test]
fn isolated_unicode_ime_followed_by_enter_submits_normally() {
    let mut state = TuiState::default();
    let now = std::time::Instant::now();
    timed(&mut state, KeyCode::Char('界'), now);
    assert_eq!(state.input(), "界");
    assert!(
        matches!(timed(&mut state, KeyCode::Enter, now), TuiAction::Submit(text) if text == "界")
    );
}

fn saved_draft(s: &TuiState) -> serde_json::Value {
    let checkpoint = s.project_checkpoint();
    checkpoint["project_state"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["name"] == checkpoint["projects"][checkpoint["active"].as_u64().unwrap() as usize]
        })
        .unwrap()["draft"]
        .clone()
}

#[test]
fn large_paste_threshold_and_atomic_identity_keep_full_payload() {
    let mut s = TuiState::default();
    s.handle_event(Event::Paste("a".repeat(1000)));
    assert_eq!(saved_draft(&s)["paste_folds"], serde_json::json!([]));
    s.set_input("");
    let first = "界".repeat(1001);
    s.handle_event(Event::Paste(first.clone()));
    let fold = saved_draft(&s)["paste_folds"][0].clone();
    assert_eq!(fold["char_count"], 1001);
    assert_eq!(fold["end_byte"], first.len());
    assert_eq!(s.input(), first);
    s.handle_event(Event::Paste("b".repeat(1001)));
    let second_id = saved_draft(&s)["paste_folds"][1]["id"].as_u64().unwrap();
    key(&mut s, KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(s.input(), first);
    s.handle_event(Event::Paste("c".repeat(1001)));
    assert!(saved_draft(&s)["paste_folds"][1]["id"].as_u64().unwrap() > second_id);
    let whole = s.input().to_owned();
    assert!(
        matches!(key(&mut s, KeyCode::Enter, KeyModifiers::NONE), TuiAction::Submit(text) if text == whole)
    );
}

#[test]
fn adjacent_alt_enter_expands_only_that_fold_and_keeps_bytes() {
    let mut s = TuiState::default();
    let first = "a".repeat(1001);
    let second = "界".repeat(1001);
    s.handle_event(Event::Paste(first.clone()));
    s.handle_event(Event::Paste(second.clone()));
    assert!(!matches!(
        key(&mut s, KeyCode::Enter, KeyModifiers::ALT),
        TuiAction::Submit(_)
    ));
    assert_eq!(s.input(), format!("{first}{second}"));
    assert_eq!(s.cursor(), first.len());
    assert_eq!(saved_draft(&s)["paste_folds"].as_array().unwrap().len(), 1);
    key(&mut s, KeyCode::Delete, KeyModifiers::NONE);
    assert_eq!(s.input(), format!("{first}{}", "界".repeat(1000)));
}

fn composer_screen(s: &mut TuiState, width: u16) -> String {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 40)).unwrap();
    terminal
        .draw(|frame| s.render_bentobox(frame, "zenpi"))
        .unwrap();
    (0..40)
        .map(|y| {
            (0..width)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn folded_projection_resize_and_visible_line_navigation_never_enter_hidden_lines() {
    let mut s = TuiState::default();
    s.set_input("head\n");
    let payload = "HIDDEN_LINE\n".repeat(100);
    s.handle_event(Event::Paste(payload.clone()));
    s.handle_event(Event::Paste("tail".into()));
    for width in [140, 32, 1, 140] {
        let screen = composer_screen(&mut s, width);
        if width == 140 {
            assert!(screen.contains("[Pasted Content 1200 chars] #1"));
        }
        assert!(!screen.contains("HIDDEN_LINE"));
        assert_eq!(s.input(), format!("head\n{payload}tail"));
    }
    key(&mut s, KeyCode::Home, KeyModifiers::NONE);
    assert_eq!(s.cursor(), 5);
    key(&mut s, KeyCode::Right, KeyModifiers::NONE);
    assert_eq!(s.cursor(), 5 + payload.len());
    key(&mut s, KeyCode::Left, KeyModifiers::NONE);
    assert_eq!(s.cursor(), 5);
    key(&mut s, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(s.cursor(), 0);
    key(&mut s, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(s.cursor(), 5);
    ctrl(&mut s, 'k');
    assert_eq!(s.input(), "head\n");
    assert_eq!(saved_draft(&s)["paste_folds"], serde_json::json!([]));
}

#[test]
fn folded_word_and_line_deletion_are_atomic_and_keep_literal_labels() {
    for (code, modifiers, before) in [
        (KeyCode::Char('w'), KeyModifiers::CONTROL, false),
        (KeyCode::Delete, KeyModifiers::CONTROL, true),
        (KeyCode::Char('u'), KeyModifiers::CONTROL, false),
    ] {
        let mut s = TuiState::default();
        let literal = "[Pasted Content 1001 chars] #1 ";
        s.set_input(literal);
        s.handle_event(Event::Paste("x".repeat(1001)));
        if before {
            key(&mut s, KeyCode::Left, KeyModifiers::NONE);
        }
        key(&mut s, code, modifiers);
        assert_eq!(
            s.input(),
            if code == KeyCode::Char('u') {
                ""
            } else {
                literal
            }
        );
        assert_eq!(saved_draft(&s)["paste_folds"], serde_json::json!([]));
        s.handle_event(Event::Paste("y".repeat(1001)));
        assert_eq!(saved_draft(&s)["paste_folds"][0]["id"], 2);
    }
}

#[test]
fn cross_boundary_combining_and_zwj_unfold_without_dropping_characters() {
    let mut s = TuiState::default();
    s.set_input("e");
    let combining = format!("\u{301}{}", "a".repeat(1001));
    s.handle_event(Event::Paste(combining.clone()));
    assert_eq!(s.input(), format!("e{combining}"));
    assert_eq!(saved_draft(&s)["paste_folds"], serde_json::json!([]));
    s.set_input("👩");
    let joined = format!("\u{200d}👩{}", "a".repeat(1001));
    s.handle_event(Event::Paste(joined.clone()));
    assert_eq!(s.input(), format!("👩{joined}"));
    assert_eq!(saved_draft(&s)["paste_folds"], serde_json::json!([]));
    s.set_input("");
    s.handle_event(Event::Paste("e".repeat(1001)));
    key(&mut s, KeyCode::Char('\u{301}'), KeyModifiers::NONE);
    assert_eq!(s.input(), format!("{}\u{301}", "e".repeat(1001)));
    assert_eq!(saved_draft(&s)["paste_folds"], serde_json::json!([]));
}

#[test]
fn history_project_and_checkpoint_preserve_unsent_folds_but_recall_full_text() {
    let mut s = TuiState::default();
    seed(&mut s, "older history");
    let payload = "界".repeat(1001);
    s.handle_event(Event::Paste(payload.clone()));
    key(&mut s, KeyCode::Left, KeyModifiers::NONE);
    let draft = saved_draft(&s);
    ctrl(&mut s, 'r');
    s.handle_event(Event::Paste("older".into()));
    assert_eq!(s.input(), "older history");
    assert_eq!(saved_draft(&s)["paste_folds"], draft["paste_folds"]);
    key(&mut s, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(s.cursor(), 0);
    assert_eq!(s.input(), payload);
    s.open_project_tab("other");
    s.handle_event(Event::Paste("b".repeat(1001)));
    s.select_project_tab(0);
    assert_eq!(saved_draft(&s)["paste_folds"], draft["paste_folds"]);
    let mut restored = TuiState::default();
    assert!(restored.restore_project_checkpoint(&s.project_checkpoint()));
    assert_eq!(restored.input(), payload);
    assert_eq!(restored.cursor(), 0);
    assert_eq!(saved_draft(&restored)["paste_folds"], draft["paste_folds"]);
    key(&mut restored, KeyCode::Enter, KeyModifiers::NONE);
    ctrl(&mut restored, 'p');
    assert_eq!(restored.input(), payload);
    // Preview persists the original unsent empty draft; selecting history
    // commits expanded canonical text without fabricating new paste elements.
    key(&mut restored, KeyCode::Char('!'), KeyModifiers::NONE);
    assert_eq!(saved_draft(&restored)["paste_folds"], serde_json::json!([]));
}

#[test]
fn corrupt_fold_metadata_restores_full_text_with_notice_and_legacy_compatibility() {
    let mut s = TuiState::default();
    let payload = "界".repeat(1001);
    s.handle_event(Event::Paste(payload.clone()));
    let good = s.project_checkpoint();
    for change in 0..6 {
        let mut value = good.clone();
        let draft = &mut value["project_state"][0]["draft"];
        match change {
            0 => draft["paste_folds"][0]["start_byte"] = 1.into(),
            1 => draft["paste_folds"][0]["end_byte"] = (payload.len() + 1).into(),
            2 => {
                let fold = draft["paste_folds"][0].clone();
                draft["paste_folds"].as_array_mut().unwrap().push(fold);
            }
            3 => draft["paste_folds"][0]["char_count"] = 1002.into(),
            4 => draft["next_paste_id"] = 0.into(),
            _ => draft["cursor"] = 1.into(),
        }
        let mut restored = TuiState::default();
        assert!(restored.restore_project_checkpoint(&value));
        assert_eq!(restored.input(), payload);
        assert_eq!(saved_draft(&restored)["paste_folds"], serde_json::json!([]));
        assert!(
            restored
                .messages()
                .any(|m| m.text.contains("Invalid paste metadata"))
        );
    }
    let mut legacy = good;
    legacy["project_state"][0]["draft"]
        .as_object_mut()
        .unwrap()
        .remove("paste_folds");
    legacy["project_state"][0]["draft"]
        .as_object_mut()
        .unwrap()
        .remove("next_paste_id");
    let mut restored = TuiState::default();
    assert!(restored.restore_project_checkpoint(&legacy));
    assert_eq!(restored.input(), payload);
    assert!(
        restored
            .messages()
            .all(|m| !m.text.contains("Invalid paste metadata"))
    );
}

#[test]
fn folded_payload_counts_toward_input_and_pretty_checkpoint_budgets() {
    let mut s = TuiState::default();
    s.handle_event(Event::Paste("x".repeat(MAX_MESSAGE_BYTES)));
    let full = s.input().to_owned();
    let metadata = saved_draft(&s)["paste_folds"].clone();
    s.handle_event(Event::Paste("z".into()));
    assert_eq!(s.input(), full);
    assert_eq!(saved_draft(&s)["paste_folds"], metadata);
    let mut crowded = TuiState::default();
    for _ in 0..15 {
        crowded.push_message(
            zenpi::tui::MessageRole::System,
            "m".repeat(MAX_MESSAGE_BYTES),
        );
    }
    crowded.set_input("keep");
    assert!(
        serde_json::to_vec_pretty(&crowded.project_checkpoint())
            .unwrap()
            .len()
            < zenpi::tui::MAX_PROJECT_CHECKPOINT_BYTES
    );
    crowded.handle_event(Event::Paste("x".repeat(MAX_MESSAGE_BYTES - 4)));
    assert_eq!(crowded.input(), "keep");
    assert_eq!(saved_draft(&crowded)["next_paste_id"], 1);
    assert!(composer_screen(&mut crowded, 140).contains("4 MiB"));
}

#[test]
fn fold_suppresses_hidden_completion_but_preserves_visible_file_suffix_completion() {
    use zenpi::tui::{ProjectTabMetadata, complete_file_query};
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("note file.txt"), "content").unwrap();
    let session =
        zenpi::session::SessionStore::open_in_workspace(root.path().join("s.jsonl"), root.path())
            .unwrap();
    let mut s = TuiState::default();
    s.set_active_project_metadata(ProjectTabMetadata::from_session(&session));
    let payload = format!("{} @hidden", "a".repeat(1001));
    s.handle_event(Event::Paste(payload.clone()));
    s.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    assert!(s.file_completion_query().is_none());
    s.handle_event(Event::Paste(" @note".into()));
    s.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    let q = s.file_completion_query().unwrap();
    s.apply_file_completion(q.clone(), complete_file_query(&q, || false).unwrap());
    let screen = composer_screen(&mut s, 140);
    assert!(!screen.contains("@hidden"));
    assert!(screen.contains("Pasted Content"));
    key(&mut s, KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(s.input(), format!("{payload} @\"note file.txt\" "));
    assert_eq!(saved_draft(&s)["paste_folds"].as_array().unwrap().len(), 1);
}

#[test]
fn normalized_paste_batches_fold_but_normal_keys_never_do() {
    let mut s = TuiState::default();
    s.handle_event(Event::Paste("x\r\n".repeat(501)));
    assert_eq!(s.input(), "x\n".repeat(501));
    assert_eq!(saved_draft(&s)["paste_folds"][0]["char_count"], 1002);
    s.set_input("");
    for _ in 0..1001 {
        key(&mut s, KeyCode::Char('x'), KeyModifiers::NONE);
    }
    assert_eq!(saved_draft(&s)["paste_folds"], serde_json::json!([]));
    s.set_input("");
    let now = std::time::Instant::now();
    for _ in 0..1001 {
        timed(&mut s, KeyCode::Char('y'), now);
    }
    s.flush_ordinary_paste(now + std::time::Duration::from_millis(70));
    assert_eq!(saved_draft(&s)["paste_folds"][0]["char_count"], 1001);
}

#[test]
fn kill_yank_is_single_nonempty_buffer_and_plain_deletion_does_not_replace_it() {
    let mut s = TuiState::default();
    s.set_input("alpha beta");
    ctrl(&mut s, 'w');
    assert_eq!(s.input(), "alpha ");
    ctrl(&mut s, 'y');
    ctrl(&mut s, 'y');
    assert_eq!(s.input(), "alpha betabeta");
    s.set_input("fresh word");
    key(&mut s, KeyCode::Backspace, KeyModifiers::CONTROL);
    s.set_input("x");
    key(&mut s, KeyCode::Backspace, KeyModifiers::NONE);
    ctrl(&mut s, 'u'); // Empty kill must keep "word".
    key(&mut s, KeyCode::Delete, KeyModifiers::NONE);
    ctrl(&mut s, 'y');
    assert_eq!(s.input(), "word");
    s.set_input("left right end");
    key(&mut s, KeyCode::Home, KeyModifiers::CONTROL);
    key(&mut s, KeyCode::Delete, KeyModifiers::CONTROL);
    ctrl(&mut s, 'y');
    assert_eq!(s.input(), "left right end");
    assert_eq!(s.cursor(), 5);
}

#[test]
fn ctrl_u_and_k_use_logical_lines_and_newlines_even_when_soft_wrapped() {
    let mut s = TuiState::default();
    s.set_input("first\nbeta");
    ctrl(&mut s, 'a');
    key(&mut s, KeyCode::Right, KeyModifiers::NONE);
    ctrl(&mut s, 'u');
    assert_eq!(s.input(), "first\neta");
    ctrl(&mut s, 'y');
    assert_eq!(s.input(), "first\nbeta");
    ctrl(&mut s, 'a');
    ctrl(&mut s, 'u');
    assert_eq!(s.input(), "firstbeta");
    ctrl(&mut s, 'y');
    assert_eq!(s.input(), "first\nbeta");
    key(&mut s, KeyCode::Home, KeyModifiers::CONTROL);
    ctrl(&mut s, 'u');
    ctrl(&mut s, 'e');
    ctrl(&mut s, 'k');
    assert_eq!(s.input(), "firstbeta");
    ctrl(&mut s, 'y');
    assert_eq!(s.input(), "first\nbeta");
    s.set_input(format!("first\n{}", "界".repeat(100)));
    composer_screen(&mut s, 20);
    ctrl(&mut s, 'u');
    assert_eq!(s.input(), "first\n");
}

#[test]
fn kill_yank_restores_canonical_fold_payload_as_plain_text_without_reusing_ids() {
    let mut s = TuiState::default();
    let literal = "[Pasted Content 1001 chars] #1 ";
    let payload = "界e\u{301}👨‍👩‍👧‍👦".repeat(150);
    s.set_input(format!("head\n{literal}"));
    s.handle_event(Event::Paste(payload.clone()));
    let next = saved_draft(&s)["next_paste_id"].clone();
    ctrl(&mut s, 'w');
    assert_eq!(s.input(), format!("head\n{literal}"));
    s.handle_event(Event::Paste("z".repeat(1001)));
    key(&mut s, KeyCode::Left, KeyModifiers::NONE);
    ctrl(&mut s, 'y');
    assert_eq!(
        s.input(),
        format!("head\n{literal}{payload}{}", "z".repeat(1001))
    );
    let folds = saved_draft(&s)["paste_folds"].clone();
    assert_eq!(folds.as_array().unwrap().len(), 1);
    assert_eq!(folds[0]["id"], next);
    assert_eq!(folds[0]["start_byte"], s.cursor());
    let text = s.input().to_owned();
    for width in [1, 20, 140] {
        composer_screen(&mut s, width);
        assert_eq!(s.input(), text);
    }
    // Projection hides payload newlines: U kills the whole visible second line.
    ctrl(&mut s, 'u');
    assert_eq!(s.input(), format!("head\n{}", "z".repeat(1001)));
    ctrl(&mut s, 'y');
    assert_eq!(s.input(), text);
}

#[test]
fn yank_combines_unicode_safely_and_paste_control_bytes_do_not_execute_it() {
    let mut s = TuiState::default();
    s.set_input("\u{301}");
    ctrl(&mut s, 'u');
    s.handle_event(Event::Paste("e".repeat(1001)));
    ctrl(&mut s, 'y');
    assert_eq!(s.input(), format!("{}\u{301}", "e".repeat(1001)));
    assert_eq!(saved_draft(&s)["paste_folds"], serde_json::json!([]));
    s.handle_event(Event::Paste("\u{19}".into()));
    assert!(s.input().ends_with('?'));
}

#[test]
fn yank_buffer_survives_submission_slash_history_but_is_project_local_and_ephemeral() {
    let mut s = TuiState::default();
    s.set_input("project A kill");
    ctrl(&mut s, 'u');
    seed(&mut s, "other prompt");
    seed(&mut s, "/status");
    s.set_input("draft");
    ctrl(&mut s, 'r');
    ctrl(&mut s, 'y'); // Search owns focus; this is not a composer yank.
    key(&mut s, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(s.input(), "draft");
    s.set_input("");
    ctrl(&mut s, 'y');
    assert_eq!(s.input(), "project A kill");
    s.open_project_tab("B");
    ctrl(&mut s, 'y');
    assert_eq!(s.input(), "");
    s.set_input("project B kill");
    ctrl(&mut s, 'u');
    s.select_project_tab(0);
    s.set_input("");
    ctrl(&mut s, 'y');
    assert_eq!(s.input(), "project A kill");
    let checkpoint = s.project_checkpoint();
    assert!(!checkpoint.to_string().contains("kill_buffer"));
    let mut restarted = TuiState::default();
    assert!(restarted.restore_project_checkpoint(&checkpoint));
    ctrl(&mut restarted, 'y');
    assert_eq!(restarted.input(), "project A kill");
    s.close_project_tab("B");
    s.open_project_tab("B");
    ctrl(&mut s, 'y');
    assert_eq!(s.input(), "");
}

#[test]
fn yank_flushes_held_ascii_before_editing_without_new_paste_classification() {
    let mut s = TuiState::default();
    s.set_input("kept");
    ctrl(&mut s, 'u');
    let now = std::time::Instant::now();
    timed(&mut s, KeyCode::Char('A'), now);
    assert_eq!(s.input(), "");
    s.handle_event_at(
        Event::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL)),
        now,
    );
    assert_eq!(s.input(), "Akept");
    assert!(matches!(timed(&mut s, KeyCode::Enter, now), TuiAction::Submit(text) if text=="Akept"));
}

#[test]
fn yank_input_and_pretty_checkpoint_rejections_preserve_full_buffer_and_draft() {
    let mut s = TuiState::default();
    s.set_input("x".repeat(MAX_MESSAGE_BYTES));
    ctrl(&mut s, 'u');
    s.set_input("keep");
    let before = saved_draft(&s);
    ctrl(&mut s, 'y');
    assert_eq!(saved_draft(&s), before);
    assert!(composer_screen(&mut s, 140).contains("256 KiB"));
    s.set_input("");
    ctrl(&mut s, 'y');
    assert_eq!(s.input().len(), MAX_MESSAGE_BYTES);
    ctrl(&mut s, 'u');
    for _ in 0..15 {
        s.push_message(
            zenpi::tui::MessageRole::System,
            "m".repeat(MAX_MESSAGE_BYTES),
        );
    }
    let before = saved_draft(&s);
    assert!(
        serde_json::to_vec_pretty(&s.project_checkpoint())
            .unwrap()
            .len()
            < zenpi::tui::MAX_PROJECT_CHECKPOINT_BYTES
    );
    ctrl(&mut s, 'y');
    assert_eq!(saved_draft(&s), before);
    assert!(composer_screen(&mut s, 140).contains("4 MiB"));
    // Failure kept the complete buffer; switching away and back does not drop it.
    s.open_project_tab("B");
    s.select_project_tab(0);
    ctrl(&mut s, 'y');
    assert_eq!(saved_draft(&s), before);
    s.clear_messages();
    ctrl(&mut s, 'y');
    assert_eq!(s.input(), "x".repeat(MAX_MESSAGE_BYTES));
}

#[test]
fn ctrl_u_uses_fold_projection_instead_of_hidden_payload_newlines() {
    let mut s = TuiState::default();
    s.set_input("head\n");
    let payload = "hidden\n".repeat(200);
    s.handle_event(Event::Paste(payload.clone()));
    ctrl(&mut s, 'u');
    assert_eq!(s.input(), "head\n");
    ctrl(&mut s, 'y');
    assert_eq!(s.input(), format!("head\n{payload}"));
    assert_eq!(saved_draft(&s)["paste_folds"], serde_json::json!([]));
}

#[test]
fn external_edit_changes_are_plain_exact_and_keep_kill_buffer_and_ids() {
    let mut s = TuiState::default();
    s.set_input("kill");
    ctrl(&mut s, 'u');
    s.handle_event(Event::Paste("界".repeat(1001)));
    let before = saved_draft(&s);
    let token = s.begin_external_edit().unwrap();
    assert!(s.begin_external_edit().is_err());
    assert!(
        s.finish_external_edit(token, Ok("/exit\n!touch never-run  \n".into()))
            .unwrap()
    );
    assert_eq!(s.input(), "/exit\n!touch never-run  \n");
    assert_eq!(saved_draft(&s)["paste_folds"], serde_json::json!([]));
    assert_eq!(saved_draft(&s)["next_paste_id"], before["next_paste_id"]);
    ctrl(&mut s, 'y');
    assert!(s.input().ends_with("\nkill"));
}

#[test]
fn external_edit_noop_cancel_empty_and_capacity_failure_are_atomic() {
    let mut s = TuiState::default();
    s.handle_event(Event::Paste("x".repeat(1001)));
    key(&mut s, KeyCode::Left, KeyModifiers::NONE);
    let before = saved_draft(&s);
    let token = s.begin_external_edit().unwrap();
    assert!(
        !s.finish_external_edit(token.clone(), Ok("x".repeat(1001)))
            .unwrap()
    );
    assert_eq!(saved_draft(&s), before);
    assert!(
        s.finish_external_edit(token, Ok("stale duplicate".into()))
            .is_err()
    );
    for outcome in [
        Err("cancelled".into()),
        Ok("x".repeat(MAX_MESSAGE_BYTES + 1)),
    ] {
        let token = s.begin_external_edit().unwrap();
        assert!(s.finish_external_edit(token, outcome).is_err());
        assert_eq!(saved_draft(&s), before);
    }
    let token = s.begin_external_edit().unwrap();
    assert!(s.finish_external_edit(token, Ok(String::new())).unwrap());
    assert_eq!(s.input(), "");
    for _ in 0..15 {
        s.push_message(
            zenpi::tui::MessageRole::System,
            "m".repeat(MAX_MESSAGE_BYTES),
        );
    }
    let before = saved_draft(&s);
    let token = s.begin_external_edit().unwrap();
    assert!(
        s.finish_external_edit(token, Ok("x".repeat(MAX_MESSAGE_BYTES)))
            .is_err()
    );
    assert_eq!(saved_draft(&s), before);
}

#[test]
fn external_edit_revision_rejects_new_text_cursor_fold_history_and_project_instances() {
    for change in 0..6 {
        let mut s = TuiState::default();
        seed(&mut s, "history");
        s.handle_event(Event::Paste("界".repeat(1001)));
        let token = s.begin_external_edit().unwrap();
        match change {
            0 => s.set_input("newer draft"),
            1 => {
                key(&mut s, KeyCode::Left, KeyModifiers::NONE);
            }
            2 => {
                key(&mut s, KeyCode::Enter, KeyModifiers::ALT);
            }
            3 => {
                ctrl(&mut s, 'p');
                key(&mut s, KeyCode::Esc, KeyModifiers::NONE);
            }
            4 => {
                s.set_project_session_cursor(s.active_project().to_owned(), "new-session", 1);
            }
            _ => {
                let project = s.active_project().to_owned();
                s.open_project_tab("other");
                s.close_project_tab(&project);
                s.open_project_tab(project);
            }
        }
        let before = saved_draft(&s);
        assert!(
            s.finish_external_edit(token, Ok("old external result".into()))
                .is_err()
        );
        assert_eq!(saved_draft(&s), before);
    }
}

#[test]
fn external_edit_owner_revision_survives_unrelated_background_projection() {
    let mut s = TuiState::default();
    s.open_project_tab("background");
    s.select_project_tab(0);
    s.set_input("original");
    s.set_project_session_cursor(s.active_project().to_owned(), "session", 1);
    let token = s.begin_external_edit().unwrap();
    s.select_project_tab(1);
    s.push_message(zenpi::tui::MessageRole::Assistant, "background reply");
    s.select_project_tab(0);
    s.set_project_session_cursor(s.active_project().to_owned(), "session", 2);
    assert!(s.finish_external_edit(token, Ok("edited".into())).unwrap());
    assert_eq!(s.input(), "edited");
}

#[test]
fn external_edit_rejects_explicit_project_roundtrip_and_checkpoint_restore() {
    for change in 0..4 {
        let mut s = TuiState::default();
        s.open_project_tab("other");
        s.select_project_tab(0);
        s.set_input("retained");
        let origin = s.active_project().to_owned();
        let token = s.begin_external_edit().unwrap();
        match change {
            0 => {
                s.next_project_tab(false);
                s.next_project_tab(true);
            }
            1 => {
                s.project_switch_context(1);
                s.project_switch_context(0);
            }
            2 => {
                s.select_project_tab(1);
                assert!(s.rename_project_tab(&origin, "temporary"));
                assert!(s.rename_project_tab("temporary", &origin));
                s.select_project_tab(0);
            }
            _ => {
                let checkpoint = s.project_checkpoint();
                assert!(s.restore_project_checkpoint(&checkpoint));
            }
        }
        let before = saved_draft(&s);
        assert!(
            s.finish_external_edit(token, Ok("obsolete".into()))
                .is_err()
        );
        assert_eq!(saved_draft(&s), before);
    }
}

#[test]
fn external_editor_shortcut_respects_focus_and_explicit_popup_dismissal() {
    let mut s = TuiState::default();
    s.set_input("/mo");
    assert_ne!(ctrl(&mut s, 'g'), TuiAction::OpenExternalEditor);
    key(&mut s, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(ctrl(&mut s, 'g'), TuiAction::OpenExternalEditor);
    s.set_input("normal draft");
    s.set_busy(true);
    assert_eq!(ctrl(&mut s, 'g'), TuiAction::OpenExternalEditor);
    ctrl(&mut s, 'r');
    assert_ne!(ctrl(&mut s, 'g'), TuiAction::OpenExternalEditor);
    key(&mut s, KeyCode::Esc, KeyModifiers::NONE);
    s.open_directory_picker();
    assert_ne!(ctrl(&mut s, 'g'), TuiAction::OpenExternalEditor);
    key(&mut s, KeyCode::Esc, KeyModifiers::NONE);
    s.open_transcript_browser();
    assert_ne!(ctrl(&mut s, 'g'), TuiAction::OpenExternalEditor);
}

#[test]
fn ctrl_u_after_one_cell_resize_keeps_following_fold_and_yanks_exact_unicode() {
    let mut s = TuiState::default();
    let literal = "[Pasted Content 1001 chars] #1 ";
    let payload = "界e\u{301}👨‍👩‍👧‍👦".repeat(150);
    s.handle_event(Event::Paste(literal.into()));
    s.handle_event(Event::Paste(payload.clone()));
    s.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(s.input(), literal);
    s.handle_event(Event::Paste("z".repeat(1001)));
    s.handle_event(Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)));
    s.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    )));
    let before = saved_draft(&s);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
    for (width, height) in [(140, 40), (1, 1), (140, 40)] {
        terminal.backend_mut().resize(width, height);
        terminal.autoresize().unwrap();
        s.handle_event(Event::Resize(width, height));
        terminal
            .draw(|frame| s.render_bentobox(frame, "zenpi"))
            .unwrap();
        assert_eq!(saved_draft(&s), before);
    }
    s.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(s.input(), "z".repeat(1001));
    assert_eq!(s.cursor(), 0);
    let killed = saved_draft(&s);
    assert_eq!(killed["paste_folds"][0]["start_byte"], 0);
    assert_eq!(killed["paste_folds"][0]["end_byte"], 1001);
    assert_eq!(
        killed["paste_folds"][0]["id"],
        before["paste_folds"][0]["id"]
    );
    s.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(saved_draft(&s), before);
}

#[test]
fn terminal_shortcut_modified_characters_never_become_draft_text() {
    for modifiers in [
        KeyModifiers::SUPER,
        KeyModifiers::SUPER | KeyModifiers::SHIFT,
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ] {
        for kind in [
            KeyEventKind::Press,
            KeyEventKind::Repeat,
            KeyEventKind::Release,
        ] {
            for character in ['x', '界'] {
                let mut s = TuiState::default();
                s.set_input("keep 世界");
                let before = (s.input().to_owned(), s.cursor());
                let mut event = KeyEvent::new(KeyCode::Char(character), modifiers);
                event.kind = kind;
                assert_eq!(s.handle_event(Event::Key(event)), TuiAction::None);
                assert_eq!(
                    (s.input().to_owned(), s.cursor()),
                    before,
                    "{modifiers:?} {kind:?} {character:?} must not insert shortcut text"
                );
            }
        }
    }
}

#[test]
fn terminal_shortcut_plain_unicode_keeps_press_repeat_and_release_semantics() {
    for kind in [
        KeyEventKind::Press,
        KeyEventKind::Repeat,
        KeyEventKind::Release,
    ] {
        for character in ['界', 'é', '🙂'] {
            let mut s = TuiState::default();
            s.set_input("draft ");
            let mut event = KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE);
            event.kind = kind;
            assert_eq!(s.handle_event(Event::Key(event)), TuiAction::None);
            let expected = if kind == KeyEventKind::Release {
                "draft ".to_owned()
            } else {
                format!("draft {character}")
            };
            assert_eq!((s.input(), s.cursor()), (expected.as_str(), expected.len()));
        }
    }
}

#[test]
fn terminal_shortcut_super_key_keeps_preceding_held_ascii_without_submitting() {
    use std::time::{Duration, Instant};
    for kind in [
        KeyEventKind::Press,
        KeyEventKind::Repeat,
        KeyEventKind::Release,
    ] {
        let mut s = TuiState::default();
        let start = Instant::now();
        assert_eq!(
            s.handle_event_at(
                Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
                start
            ),
            TuiAction::None
        );
        assert_eq!(s.input(), "");
        let mut event = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::SUPER);
        event.kind = kind;
        assert_eq!(
            s.handle_event_at(Event::Key(event), start + Duration::from_millis(1)),
            TuiAction::None
        );
        assert_eq!(
            s.handle_event_at(Event::Resize(140, 40), start + Duration::from_millis(200)),
            TuiAction::Redraw
        );
        assert_eq!((s.input(), s.cursor()), ("a", 1), "{kind:?}");
    }
}

#[test]
fn terminal_shortcut_existing_control_and_alt_editing_still_dispatch() {
    for (code, modifiers, expected, cursor) in [
        (KeyCode::Char('a'), KeyModifiers::CONTROL, "one two", 0),
        (KeyCode::Char('j'), KeyModifiers::CONTROL, "one two\n", 8),
        (
            KeyCode::Char('j'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            "one two\n",
            8,
        ),
        (KeyCode::Left, KeyModifiers::ALT, "one two", 4),
    ] {
        let mut s = TuiState::default();
        s.set_input("one two");
        let action = s.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
        assert!(!matches!(action, TuiAction::Submit(_) | TuiAction::Quit));
        assert_eq!(
            (s.input(), s.cursor()),
            (expected, cursor),
            "{code:?} {modifiers:?}"
        );
    }
}

#[test]
fn terminal_shortcut_editor_hint_agrees_with_action_in_the_rendered_state() {
    fn check(s: &mut TuiState, available: bool, label: &str) {
        let screen = composer_screen(s, 200);
        assert_eq!(
            screen.contains("Ctrl-G editor"),
            available,
            "{label}: {screen}"
        );
        let before = (s.input().to_owned(), s.cursor());
        let action = s.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('g'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(
            action == TuiAction::OpenExternalEditor,
            available,
            "{label}: {action:?}"
        );
        assert_eq!((s.input().to_owned(), s.cursor()), before, "{label}");
    }
    let mut s = TuiState::default();
    s.set_input("normal draft");
    check(&mut s, true, "plain draft");
    s.set_input("/mo");
    check(&mut s, false, "visible command choices");
    s.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    check(&mut s, true, "choices explicitly dismissed");
    s.set_input("normal draft");
    s.set_busy(true);
    check(&mut s, true, "busy draft remains editable");
    s.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('r'),
        KeyModifiers::CONTROL,
    )));
    check(&mut s, false, "history search");
    s.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    s.open_directory_picker();
    check(&mut s, false, "directory picker");
    s.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    s.open_transcript_browser();
    check(&mut s, false, "transcript browser");
}

fn history_super_fixture() -> TuiState {
    let mut state = TuiState::default();
    for text in ["alpha old", "alpha new", "界A old", "界A new"] {
        state.set_input(text);
        assert!(matches!(
            state.handle_event(Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::NONE
            ))),
            TuiAction::Submit(_)
        ));
    }
    state.handle_event(Event::Paste("未发送".repeat(400)));
    state.handle_event(Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)));
    assert_eq!(state.cursor(), 0);
    assert_eq!(
        saved_draft(&state)["paste_folds"].as_array().unwrap().len(),
        1
    );
    state
}

#[test]
fn history_super_shortcuts_preserve_query_preview_and_original_fold() {
    let mut failures = Vec::new();
    for direct in [false, true] {
        for modifiers in [
            KeyModifiers::SUPER,
            KeyModifiers::SUPER | KeyModifiers::SHIFT,
        ] {
            for character in ['x', '界'] {
                for kind in [
                    KeyEventKind::Press,
                    KeyEventKind::Repeat,
                    KeyEventKind::Release,
                ] {
                    let mut state = history_super_fixture();
                    let original = saved_draft(&state);
                    state.handle_event(Event::Key(KeyEvent::new(
                        KeyCode::Char('r'),
                        KeyModifiers::CONTROL,
                    )));
                    state.handle_event(Event::Paste("alpha".into()));
                    state.handle_event(Event::Key(KeyEvent::new(
                        KeyCode::Char('r'),
                        KeyModifiers::CONTROL,
                    )));
                    assert_eq!(state.input(), "alpha old");
                    let before = (state.input().to_owned(), state.cursor());
                    let mut event = KeyEvent::new(KeyCode::Char(character), modifiers);
                    event.kind = kind;
                    let action = if direct {
                        state.handle_key(event)
                    } else {
                        state.handle_event(Event::Key(event))
                    };
                    let screen = composer_screen(&mut state, 200);
                    if (state.input().to_owned(), state.cursor()) != before
                        || !screen.contains("History search: alpha · match")
                        || saved_draft(&state) != original
                        || matches!(action, TuiAction::Submit(_) | TuiAction::Quit)
                    {
                        failures.push(format!(
                            "direct={direct} {modifiers:?} {kind:?} {character:?}: input_bytes={} prefix={:?}",
                            state.input().len(), state.input().chars().take(20).collect::<String>()
                        ));
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "modified history text changed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn history_super_controls_keep_unicode_shift_cycle_and_explicit_accept() {
    let mut state = history_super_fixture();
    let original = saved_draft(&state);
    let event = |state: &mut TuiState, code, modifiers| {
        state.handle_event(Event::Key(KeyEvent::new(code, modifiers)))
    };
    event(&mut state, KeyCode::Char('r'), KeyModifiers::CONTROL);
    event(&mut state, KeyCode::Char('界'), KeyModifiers::NONE);
    event(&mut state, KeyCode::Char('A'), KeyModifiers::SHIFT);
    assert_eq!(state.input(), "界A new");
    let screen = composer_screen(&mut state, 200);
    let title = screen
        .lines()
        .find(|line| line.contains("History search:"))
        .unwrap();
    // TestBackend retains the padding cell of a wide glyph in this text projection.
    let title: String = title.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(title.contains("Historysearch:界A·match"), "{title}");
    event(&mut state, KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert_eq!(state.input(), "界A old");
    assert_eq!(saved_draft(&state), original);
    assert_eq!(
        event(&mut state, KeyCode::Enter, KeyModifiers::NONE),
        TuiAction::Redraw
    );
    assert_eq!(state.input(), "界A old");
    assert!(!composer_screen(&mut state, 200).contains("History search:"));
    assert_eq!(saved_draft(&state)["paste_folds"], serde_json::json!([]));
    assert_eq!(
        event(&mut state, KeyCode::Enter, KeyModifiers::NONE),
        TuiAction::Submit("界A old".into())
    );
}

#[test]
fn history_super_escape_restores_exact_fold_cursor_after_ignored_shortcut() {
    let mut state = history_super_fixture();
    let original = saved_draft(&state);
    state.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('r'),
        KeyModifiers::CONTROL,
    )));
    state.handle_event(Event::Paste("alpha".into()));
    state.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::SUPER | KeyModifiers::SHIFT,
    )));
    state.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    assert_eq!(saved_draft(&state), original);
    assert_eq!(state.cursor(), 0);
    assert_eq!(state.input(), "未发送".repeat(400));
    assert!(composer_screen(&mut state, 200).contains("Pasted Content 1200 chars"));
}
