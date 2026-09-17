use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{Terminal, backend::TestBackend};
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
};
use tempfile::tempdir;
use zenpi::{
    core::Agent,
    session::SessionStore,
    tools::{SideEffectPolicy, ToolContext, ToolRegistry},
    tui::{MessageRole, ProjectIntent, ProjectRuntimeHost, ProjectTabMetadata, TuiState},
};

fn host(root: &Path) -> (TuiState, ProjectRuntimeHost) {
    let session = SessionStore::open_in_workspace(root.join("initial.jsonl"), root).unwrap();
    let mut agent = Agent::with_echo(session);
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(root).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    let mut state = TuiState::default();
    state.set_active_project_metadata(ProjectTabMetadata::from_session(agent.session()));
    let host = ProjectRuntimeHost::new(&mut state, Arc::new(Mutex::new(agent))).unwrap();
    (state, host)
}
fn key(state: &mut TuiState, code: KeyCode, modifiers: KeyModifiers) {
    state.handle_key(KeyEvent::new(code, modifiers));
}

#[test]
fn project_status_footer_keeps_selected_directory_tail_on_narrow_terminal() {
    let mut state = TuiState::default();
    state.set_status("Project ready · /Users/example/workspaces/project-a");
    let mut terminal = Terminal::new(TestBackend::new(48, 10)).unwrap();
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let row: String = (0..48)
        .map(|x| terminal.backend().buffer()[(x, 9)].symbol())
        .collect();
    assert!(
        row.contains("project-a"),
        "footer hid selected directory: {row:?}"
    );
}

#[test]
fn asynchronous_transcript_mutations_schedule_checkpoints_without_keyboard_input() {
    use zenpi::tui::ToolRunStatus;
    let mut state = TuiState::default();
    let checkpoint = |state: &mut TuiState, marker: &str| {
        assert!(
            state.project_checkpoint_dirty(),
            "missing checkpoint for {marker}"
        );
        let value = state.project_checkpoint();
        assert!(value.to_string().contains(marker));
        let mut restored = TuiState::default();
        assert!(restored.restore_project_checkpoint(&value));
        assert!(restored.project_checkpoint().to_string().contains(marker));
        state.clear_project_checkpoint_dirty();
    };
    state.clear_project_checkpoint_dirty();
    state.push_message(MessageRole::System, "owner notice");
    checkpoint(&mut state, "owner notice");
    state.push_message_block(
        MessageRole::Tool,
        "block notice",
        zenpi::view_model::ViewBlock::Paragraph {
            text: "block payload".into(),
        },
    );
    checkpoint(&mut state, "block payload");
    state.append_stream(MessageRole::Assistant, "legacy first ");
    checkpoint(&mut state, "legacy first");
    state.append_stream(MessageRole::Assistant, "legacy second");
    checkpoint(&mut state, "legacy second");
    state.finish_stream(MessageRole::Assistant, "legacy final");
    checkpoint(&mut state, "legacy final");
    state.begin_stream_for_job(7);
    state.append_stream_for_job(7, MessageRole::Assistant, "first chunk ");
    checkpoint(&mut state, "first chunk");
    state.append_stream_for_job(7, MessageRole::Assistant, "second chunk");
    checkpoint(&mut state, "second chunk");
    state.append_reasoning_for_job(7, "first reasoning ");
    checkpoint(&mut state, "first reasoning");
    state.append_reasoning_for_job(7, "second reasoning");
    checkpoint(&mut state, "second reasoning");
    state.tool_call_started("call", "fixture_tool");
    checkpoint(&mut state, "fixture_tool");
    state.tool_call_finished("call", ToolRunStatus::Failed);
    checkpoint(&mut state, "failed");
    state.finish_stream_for_job(7, MessageRole::Assistant, "final answer");
    checkpoint(&mut state, "final answer");
    state.append_stream_for_job(6, MessageRole::Assistant, "stale");
    assert!(!state.project_checkpoint_dirty());
    assert!(state.open_project_tab("other"));
    state.clear_project_checkpoint_dirty();
    assert!(state.select_project_tab(0));
    state.push_message(MessageRole::System, "background result");
    assert!(state.select_project_tab(1));
    checkpoint(&mut state, "background result");
    assert!(
        !state
            .messages()
            .any(|m| m.text.contains("background result"))
    );
}

#[test]
fn display_only_ticks_do_not_schedule_project_writes_but_layout_changes_do() {
    let mut state = TuiState::default();
    state.clear_project_checkpoint_dirty();
    state.clear_layout_dirty();
    state.set_busy(true);
    state.tick();
    state.set_status("Working");
    assert!(!state.project_checkpoint_dirty());
    assert!(!state.layout_dirty());
    state.focus_next_workspace_pane();
    assert!(state.layout_dirty());
    state.clear_layout_dirty();
    state.set_workspace_pane_collapsed(zenpi::layout::PaneId::Resources, true);
    assert!(state.layout_dirty());
}

#[test]
fn topmost_plus_opens_picker_and_cancel_never_creates_a_tab_or_changes_draft() {
    let root = tempdir().unwrap();
    let (mut state, _host) = host(root.path());
    state.handle_event(Event::Paste("draft 未发送".into()));
    let before = state.project_checkpoint();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let row: String = (0..100)
        .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
        .collect();
    assert!(row.contains("[+]"));
    state.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 98,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert!(state.directory_picker_open());
    assert_eq!(state.project_tab_count(), 1);
    key(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!state.directory_picker_open());
    assert!(state.take_project_intent().is_none());
    assert_eq!(state.project_checkpoint(), before);
    assert_eq!(state.input(), "draft 未发送");
}

#[test]
fn picker_confirmation_and_real_tools_bind_two_equal_basename_directories() {
    let root = tempdir().unwrap();
    let first = root.path().join("one/工作 folder");
    let second = root.path().join("two/工作 folder");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    let (mut state, mut host) = host(root.path());
    let process_cwd = std::env::current_dir().unwrap();
    for (directory, content) in [(&first, "first"), (&second, "second")] {
        key(&mut state, KeyCode::Char('t'), KeyModifiers::CONTROL);
        assert!(state.directory_picker_open());
        key(&mut state, KeyCode::Char('u'), KeyModifiers::CONTROL);
        state.handle_event(Event::Paste(directory.display().to_string()));
        key(&mut state, KeyCode::Enter, KeyModifiers::NONE);
        let intent = state.take_project_intent().expect("confirmed path");
        host.apply(&mut state, intent).unwrap();
        let active = host.active(&state);
        let mut agent = active.lock().unwrap();
        let label = state.project_label(state.active_project_index());
        assert!(label.starts_with("工作 folder"));
        if state.project_tab_count() > 2 {
            assert!(label.contains("one") || label.contains("two"));
            assert!(label.contains('#'));
        }
        assert_eq!(
            state
                .project_metadata(state.active_project())
                .expect("confirmed project metadata")
                .cwd,
            directory.canonicalize().unwrap().display().to_string()
        );
        assert_eq!(
            Path::new(&agent.session().header().cwd),
            directory.canonicalize().unwrap()
        );
        assert_eq!(
            agent.attachment_workspace_root().unwrap(),
            directory.canonicalize().unwrap()
        );
        agent.set_approval_policy(zenpi::approval::ApprovalPolicy {
            mode: zenpi::approval::ApprovalMode::Never,
            ..Default::default()
        });
        let result = agent
            .run_user_shell_with_cancel(&format!("!printf {content} > marker.txt"), || false)
            .unwrap();
        assert_eq!(result["exit_code"], 0);
        assert_eq!(
            fs::read_to_string(directory.join("marker.txt")).unwrap(),
            content
        );
    }
    assert_ne!(state.project_tabs()[1], state.project_tabs()[2]);
    assert_ne!(state.project_label(1), state.project_label(2));
    assert_eq!(
        fs::read_to_string(first.join("marker.txt")).unwrap(),
        "first"
    );
    host.apply(
        &mut state,
        ProjectIntent::Open(first.join("../工作 folder")),
    )
    .unwrap();
    assert_eq!(state.project_tab_count(), 3);
    assert_eq!(std::env::current_dir().unwrap(), process_cwd);
}

#[test]
fn invalid_project_config_is_rejected_before_creating_session_or_mutating_visible_owner() {
    let root = tempdir().unwrap();
    let invalid = root.path().join("invalid");
    fs::create_dir_all(invalid.join(".zenpi")).unwrap();
    fs::write(invalid.join(".zenpi/config.toml"), "model = [ broken").unwrap();
    let (mut state, mut host) = host(root.path());
    let before = state.project_checkpoint();
    let owner = host.active(&state);
    assert!(
        host.apply(&mut state, ProjectIntent::Open(invalid))
            .is_err()
    );
    assert_eq!(state.project_checkpoint(), before);
    assert!(Arc::ptr_eq(&owner, &host.active(&state)));
    assert!(!root.path().join("projects").exists());
}

#[test]
fn switching_while_original_owner_is_locked_preserves_drafts_layout_and_owner() {
    let root = tempdir().unwrap();
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    let (mut state, mut host) = host(root.path());
    state.handle_event(Event::Paste("draft one".into()));
    state.push_message(MessageRole::Assistant, "answer one");
    let initial = host.active(&state);
    let initial_layout = state.workspace_layout().clone();
    let running = initial.lock().unwrap();
    host.apply(&mut state, ProjectIntent::Open(other)).unwrap();
    assert_eq!(state.input(), "");
    assert!(!Arc::ptr_eq(&initial, &host.active(&state)));
    state.handle_event(Event::Paste("draft two".into()));
    host.apply(&mut state, ProjectIntent::Select(0)).unwrap();
    assert_eq!(state.input(), "draft one");
    assert_eq!(state.workspace_layout(), &initial_layout);
    assert!(Arc::ptr_eq(&initial, &host.active(&state)));
    drop(running);
    let inactive = state.project_tabs()[1].clone();
    host.apply(&mut state, ProjectIntent::Close(inactive))
        .unwrap();
    assert_eq!(state.input(), "draft one");
    assert!(state.messages().any(|m| m.text == "answer one"));
}

#[test]
fn closing_a_locked_project_is_rejected_without_mutating_tabs_or_draft() {
    let root = tempdir().unwrap();
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    let (mut state, mut host) = host(root.path());
    host.apply(&mut state, ProjectIntent::Open(other)).unwrap();
    host.apply(&mut state, ProjectIntent::Select(0)).unwrap();
    state.handle_event(Event::Paste("keep me".into()));
    let target = state.active_project().to_owned();
    let owner = host.active(&state);
    let _lock = owner.lock().unwrap();
    let before = state.project_checkpoint();
    let result = host.apply(&mut state, ProjectIntent::Close(target));
    assert!(result.is_err());
    assert_eq!(state.project_checkpoint(), before);
    assert_eq!(state.input(), "keep me");
}

#[test]
fn restart_restores_selected_runtime_and_drafts_without_reusing_startup_owner() {
    let root = tempdir().unwrap();
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    let (mut state, mut host) = host(root.path());
    state.handle_event(Event::Paste("draft initial".into()));
    host.apply(&mut state, ProjectIntent::Open(other.clone()))
        .unwrap();
    state.handle_event(Event::Paste("draft other".into()));
    let saved = state.project_checkpoint();
    drop(host);
    let session =
        SessionStore::open_in_workspace(root.path().join("initial.jsonl"), root.path()).unwrap();
    let mut agent = Agent::with_echo(session);
    agent.set_attachment_workspace(ToolContext::new(root.path()).unwrap());
    let mut restored = TuiState::default();
    assert!(restored.restore_project_checkpoint(&saved));
    let mut host = ProjectRuntimeHost::new(&mut restored, Arc::new(Mutex::new(agent))).unwrap();
    assert_eq!(restored.input(), "draft other");
    assert_eq!(
        host.active(&restored)
            .lock()
            .unwrap()
            .attachment_workspace_root()
            .unwrap(),
        other.canonicalize().unwrap()
    );
    host.apply(&mut restored, ProjectIntent::Select(0)).unwrap();
    assert_eq!(restored.input(), "draft initial");
}

#[test]
fn session_workspace_mismatch_does_not_rewrite_existing_journal() {
    let root = tempdir().unwrap();
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    let path = root.path().join("journal.jsonl");
    SessionStore::open_in_workspace(&path, root.path()).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(SessionStore::open_in_workspace(&path, &other).is_err());
    assert_eq!(fs::read(path).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn shared_resume_checkpoint_failure_preserves_actual_owner_draft_layout_and_metadata() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempdir().unwrap();
    let target = root.path().join("alternate.jsonl");
    let mut replacement =
        Agent::with_echo(SessionStore::open_in_workspace(&target, root.path()).unwrap());
    replacement.set_persona("ISTJ").unwrap();
    let target_id = replacement.session().session_id().to_owned();
    drop(replacement);
    let (mut state, mut host) = host(root.path());
    host.restore_shared_workspace(&mut state).unwrap();
    let owner = host.active(&state);
    owner.lock().unwrap().set_persona("ESFP").unwrap();
    let old_session = owner.lock().unwrap().session().session_id().to_owned();
    state.set_input("keep unsent draft 世界");
    state.push_message(MessageRole::Assistant, "keep old transcript");
    let before = state.project_checkpoint();
    let checkpoint = root.path().join("project-workspace.json");
    let saved = fs::read(&checkpoint).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let result = host.resume_session(
        &mut state,
        &zenpi::slash::SessionAction::Open {
            path: target.display().to_string(),
        },
    );
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err());
    assert_eq!(state.project_checkpoint(), before);
    assert_eq!(fs::read(&checkpoint).unwrap(), saved);
    assert!(Arc::ptr_eq(&owner, &host.active(&state)));
    assert_eq!(owner.lock().unwrap().session().session_id(), old_session);
    assert_eq!(owner.lock().unwrap().persona(), "ESFP");
    host.resume_session(
        &mut state,
        &zenpi::slash::SessionAction::Open {
            path: target.display().to_string(),
        },
    )
    .unwrap();
    assert_eq!(owner.lock().unwrap().session().session_id(), target_id);
    assert_eq!(owner.lock().unwrap().persona(), "ISTJ");
    assert_eq!(state.input(), "keep unsent draft 世界");
    assert_eq!(
        state
            .project_metadata(state.active_project())
            .unwrap()
            .session_path
            .as_deref(),
        target.canonicalize().unwrap().to_str()
    );
    let after = state.project_checkpoint();
    assert_eq!(
        before["project_state"][0]["layout"],
        after["project_state"][0]["layout"]
    );
    assert_ne!(fs::read(&checkpoint).unwrap(), saved);
}

#[test]
fn stale_resume_writer_and_busy_owner_leave_the_tui_checkpoint_unchanged() {
    let root = tempdir().unwrap();
    let target = root.path().join("alternate.jsonl");
    drop(SessionStore::open_in_workspace(&target, root.path()).unwrap());
    let (mut state, mut host) = host(root.path());
    host.restore_shared_workspace(&mut state).unwrap();
    state.set_input("retained draft");
    let before = state.project_checkpoint();
    let action = zenpi::slash::SessionAction::Open {
        path: target.display().to_string(),
    };
    state.set_busy(true);
    assert!(
        host.resume_session(&mut state, &action)
            .unwrap_err()
            .contains("busy")
    );
    state.set_busy(false);
    let owner = host.active(&state);
    let guard = owner.lock().unwrap();
    assert!(
        host.resume_session(&mut state, &action)
            .unwrap_err()
            .contains("busy")
    );
    drop(guard);
    assert_eq!(state.project_checkpoint(), before);
    let checkpoint = root.path().join("project-workspace.json");
    let mut changed = fs::read(&checkpoint).unwrap();
    changed.push(b'\n');
    fs::write(&checkpoint, &changed).unwrap();
    assert!(
        host.resume_session(&mut state, &action)
            .unwrap_err()
            .contains("another host")
    );
    assert_eq!(state.project_checkpoint(), before);
    assert_eq!(fs::read(checkpoint).unwrap(), changed);
}

#[test]
fn canonical_project_checkpoint_restores_session_projection_without_feature_tabs() {
    use zenpi::layout::{PaneId, TabId};
    let root = tempdir().unwrap();
    let (mut state, _host) = host(root.path());
    state.set_workspace_tab(TabId::Session);
    state.focus_workspace_pane(PaneId::SessionList);
    state.set_workspace_pane_collapsed(PaneId::ReplayControls, true);
    let saved = state.project_checkpoint();
    let mut restored = TuiState::default();
    assert!(restored.restore_project_checkpoint(&saved));
    assert_eq!(restored.project_tabs(), state.project_tabs());
    assert_eq!(restored.workspace_layout(), state.workspace_layout());
    let mut legacy = saved;
    legacy["schema_version"] = serde_json::json!(2);
    let mut restored = TuiState::default();
    assert!(restored.restore_project_checkpoint(&legacy));
    assert_eq!(restored.workspace_tab(), TabId::Project);
}

fn session_selection_fixture() -> (
    tempfile::TempDir,
    TuiState,
    Vec<zenpi::session::SessionSummary>,
) {
    let root = tempdir().unwrap();
    for index in 0..20 {
        drop(SessionStore::open(root.path().join(format!("session-{index:02}.jsonl"))).unwrap());
    }
    let owner = SessionStore::open(root.path().join("session-00.jsonl")).unwrap();
    let mut state = TuiState::default();
    state.refresh_session_snapshot(&owner);
    state.set_workspace_tab(zenpi::layout::TabId::Session);
    state.focus_workspace_pane(zenpi::layout::PaneId::SessionList);
    let rows = zenpi::session::list_sessions(root.path()).unwrap();
    assert_eq!(rows.len(), 20);
    (root, state, rows)
}

fn draw_session_selection(
    state: &mut TuiState,
    terminal: &mut Terminal<TestBackend>,
) -> ratatui::layout::Rect {
    terminal
        .draw(|frame| state.render_bentobox(frame, "test"))
        .unwrap();
    let area = terminal.backend().buffer().area;
    // Top project row + layer-2 sub-tab row + header, single-line composer
    // with borders, footer.
    let workspace = ratatui::layout::Rect::new(0, 3, area.width, area.height - 7);
    zenpi::tui::BentoBoxLayoutAdapter::new(state.workspace_layout(), workspace)
        .visible_panes()
        .find(|pane| pane.id == zenpi::layout::PaneId::SessionList)
        .expect("visible session list")
        .rect
}

#[test]
fn session_list_click_tracks_the_visible_scrolled_row_and_open_path() {
    let (_root, mut state, rows) = session_selection_fixture();
    state.set_input("keep draft");
    let layout = state.workspace_layout().clone();
    state.move_session_browser_cursor(15);
    let mut terminal = Terminal::new(TestBackend::new(300, 30)).unwrap();
    let pane = draw_session_selection(&mut state, &mut terminal);
    let visible = usize::from(pane.height - 2);
    let first = 15 - (visible - 1);
    assert!(first > 0 && first < rows.len());
    let top: String = (pane.x + 1..pane.right() - 1)
        .map(|x| terminal.backend().buffer()[(x, pane.y + 1)].symbol())
        .collect();
    assert!(top.contains(&rows[first].session_id), "{top:?}");
    state.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pane.x + 5,
        row: pane.y + 1,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        state.selected_session_browser_path(),
        Some(rows[first].path.as_str())
    );
    assert_eq!(state.input(), "keep draft");
    assert_eq!(state.workspace_layout(), &layout);
    state.set_input("");
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        zenpi::tui::TuiAction::OpenSession(rows[first].path.clone())
    );
}

#[test]
fn session_list_narrow_resize_keeps_selected_row_visible_without_wrapping() {
    let (_root, mut state, rows) = session_selection_fixture();
    state.move_session_browser_cursor(15);
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    for (width, height) in [(120, 30), (120, 20), (300, 40)] {
        terminal.backend_mut().resize(width, height);
        terminal
            .resize(ratatui::layout::Rect::new(0, 0, width, height))
            .unwrap();
        let pane = draw_session_selection(&mut state, &mut terminal);
        let visible = usize::from(pane.height - 2);
        let first = 15_usize.saturating_sub(visible - 1);
        let selected_y = pane.y + 1 + (15 - first) as u16;
        assert_eq!(
            terminal.backend().buffer()[(pane.x + 1, selected_y)].symbol(),
            ">",
            "selected item must occupy one row at {width}x{height}"
        );
        state.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: pane.x + 5,
            row: selected_y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            state.selected_session_browser_path(),
            Some(rows[15].path.as_str())
        );
    }
}

#[test]
fn session_list_borders_and_empty_rows_do_not_change_selection() {
    let (_root, mut state, rows) = session_selection_fixture();
    state.move_session_browser_cursor(1);
    let mut terminal = Terminal::new(TestBackend::new(300, 160)).unwrap();
    let pane = draw_session_selection(&mut state, &mut terminal);
    assert!(usize::from(pane.height - 2) > rows.len());
    for (column, row) in [
        (pane.x, pane.y + 3),
        (pane.right() - 1, pane.y + 3),
        (pane.x + 5, pane.y),
        (pane.x + 5, pane.bottom() - 1),
        (pane.x + 5, pane.y + 1 + rows.len() as u16),
    ] {
        state.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
        state.handle_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            state.selected_session_browser_path(),
            Some(rows[1].path.as_str())
        );
    }
}

#[test]
fn layer1_project_tabs_move_rename_and_style() {
    use zenpi::slash::{ProjectAction, SlashCommand};
    // Parsing exposes the three new layer-1 operations.
    match zenpi::slash::parse("/project move alpha 2").unwrap() {
        Some(SlashCommand::Project { action: ProjectAction::Move { name, index } }) => {
            assert_eq!(name, "alpha");
            assert_eq!(index, 2);
        }
        other => panic!("unexpected: {other:?}"),
    }
    match zenpi::slash::parse("/project rename alpha beta").unwrap() {
        Some(SlashCommand::Project { action: ProjectAction::Rename { old, new } }) => {
            assert_eq!((old.as_str(), new.as_str()), ("alpha", "beta"));
        }
        other => panic!("unexpected: {other:?}"),
    }
    match zenpi::slash::parse("/project style alpha green").unwrap() {
        Some(SlashCommand::Project { action: ProjectAction::Style { name, style } }) => {
            assert_eq!((name.as_str(), style.as_str()), ("alpha", "green"));
        }
        other => panic!("unexpected: {other:?}"),
    }
    assert!(matches!(
        zenpi::slash::parse("/project move alpha x"),
        Err(zenpi::slash::SlashError::UnexpectedArgument { command: "project" })
    ));

    let mut state = TuiState::default();
    assert!(state.open_project_tab("alpha"));
    assert!(state.open_project_tab("beta"));
    assert!(state.open_project_tab("gamma"));
    let order = |s: &TuiState| s.project_tabs().to_vec();
    // "default" is the initial tab; move alpha (index 1) to the end.
    let alpha_index = state.project_index("alpha").unwrap();
    assert!(state.move_project_tab("alpha", state.project_tabs().len() - 1));
    assert_eq!(order(&state).last().unwrap(), "alpha");
    assert!(state.move_project_tab("beta", 0));
    assert_eq!(order(&state)[0], "beta");
    assert_eq!(state.active_project(), "gamma", "active project follows by name");
    let _ = alpha_index;

    assert!(state.rename_project_tab("beta", "beta-2"));
    assert!(state.project_index("beta-2").is_some());

    assert!(state.style_project_tab("alpha", "green"));
    assert!(state.style_project_tab("alpha", "CYAN"));
    assert!(!state.style_project_tab("alpha", "chartreuse"));
    assert!(!state.style_project_tab("missing", "green"));
}

#[test]
fn layer2_subtabs_default_reuse_and_manage() {
    use zenpi::slash::{SlashCommand, WorktreeAction};
    use zenpi::tui::SubTabKind;

    // Alias + add forms parse.
    match zenpi::slash::parse("/wt add --in-place").unwrap() {
        Some(SlashCommand::Worktree {
            action: WorktreeAction::Add { in_place, name },
        }) => {
            assert!(in_place);
            assert!(name.is_none());
        }
        other => panic!("unexpected: {other:?}"),
    }
    match zenpi::slash::parse("/worktree select 2").unwrap() {
        Some(SlashCommand::Worktree {
            action: WorktreeAction::Select { index },
        }) => assert_eq!(index, 2),
        other => panic!("unexpected: {other:?}"),
    }

    let mut state = TuiState::default();
    // Default layer-2 reuses the layer-1 information: exactly one main tab.
    let tabs = state.subtabs();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].kind, SubTabKind::Main);
    assert_eq!(state.active_subtab(), 0);

    // "work in the current place" adds without touching git.
    assert!(state.subtab_add_in_place(Some("scratch".into())));
    assert_eq!(state.active_subtab(), 1);
    assert_eq!(state.subtabs()[1].kind, SubTabKind::InPlace);
    assert!(state.subtab_add_in_place(None));
    assert!(state.subtab_move(1, 2));
    assert_eq!(state.subtabs()[2].name, "scratch");
    // The main tab cannot be closed or moved.
    assert!(!state.subtab_close(0));
    assert!(!state.subtab_move(0, 1));
    assert!(state.subtab_close(1));
}

#[test]
fn worktree_helpers_create_list_and_remove() {
    use std::process::Command;
    let repo = tempdir().unwrap();
    let root = repo.path();
    let git = |args: &[&str]| {
        let out = Command::new("git").arg("-C").arg(root).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@example.invalid"]);
    git(&["config", "user.name", "t"]);
    fs::write(root.join("f.txt"), "x").unwrap();
    git(&["add", "f.txt"]);
    git(&["commit", "-qm", "base"]);

    let before = zenpi::project_workspace::list_worktrees(root).unwrap();
    assert_eq!(before.len(), 1);

    let wt = root.join(".zenpi-worktrees").join("feature");
    fs::create_dir_all(wt.parent().unwrap()).unwrap();
    zenpi::project_workspace::add_worktree(root, &wt, "feature").unwrap();
    let entries = zenpi::project_workspace::list_worktrees(root).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(
        entries.iter().any(|e| e.branch.as_deref() == Some("feature")),
        "{entries:?}"
    );

    zenpi::project_workspace::remove_worktree(root, &wt).unwrap();
    assert_eq!(zenpi::project_workspace::list_worktrees(root).unwrap().len(), 1);
}

#[test]
fn interactive_double_row_tab_add_remove_reorder() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut state = TuiState::default();
    assert!(state.open_project_tab("a"));
    assert!(state.open_project_tab("b"));
    let order = |s: &TuiState| s.project_tabs().to_vec();
    // "b" is active; Ctrl-B moves it left, Ctrl-F right (wrapping).
    state.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    assert_eq!(order(&state)[1], "b");
    state.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
    assert!(state.active_project() == "b");

    // Layer-2: Alt-I adds in place, Alt-,/. reorder, Alt-W closes.
    state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT));
    state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT));
    assert_eq!(state.subtabs().len(), 3);
    let last = state.subtabs()[2].name.clone();
    state.handle_key(KeyEvent::new(KeyCode::Char(','), KeyModifiers::ALT));
    assert_eq!(state.subtabs()[1].name, last);
    state.handle_key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::ALT));
    assert_eq!(state.subtabs()[2].name, last);
    let active = state.active_subtab();
    state.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT));
    assert_eq!(state.subtabs().len(), 2);
    let _ = active;
}
