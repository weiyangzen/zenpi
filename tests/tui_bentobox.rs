use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use tempfile::tempdir;
use zenpi::b3::ResourceBudget;
use zenpi::domain_execution::{
    ExecutionReceipt, ExecutionStatus, ExecutionStore, deterministic_cost,
};
use zenpi::domain_store::{DomainStore, path_for_session};
use zenpi::domains::{Blueprint, BlueprintItem, Goal, GoalStatus, MAX_BLUEPRINT_ITEMS};
use zenpi::layout::{FocusDirection, LayoutModel, PaneCapabilities, PaneId, TabId, Visibility};
use zenpi::resources::{
    CpuSignal, DiskSignal, MemorySignal, ProcessSignal, ResourceSnapshot, SignalStatus,
    WorkspaceSummary,
};
use zenpi::tui::{
    BentoBoxLayoutAdapter, MAX_GANTT_PANE_BYTES, MessageRole, TuiAction, TuiState,
    collect_gantt_snapshot,
};

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
fn active_layout_edits_after_restore_replace_cached_project_layout() {
    let mut state = TuiState::new(32);
    state.focus_next_workspace_pane();
    assert!(state.adjust_workspace_split(FocusDirection::Right));
    let first = state.workspace_layout().clone();
    assert!(state.open_project_tab("second"));
    state.reset_workspace_layout();
    let second = state.workspace_layout().clone();
    assert!(state.select_project_tab(0));
    assert_eq!(state.workspace_layout(), &first);
    let mut restored = TuiState::new(32);
    assert!(restored.restore_project_checkpoint(&state.project_checkpoint()));
    assert_eq!(restored.workspace_layout(), &first);
    restored.reset_workspace_layout();
    restored.focus_next_workspace_pane();
    let edited = restored.workspace_layout().clone();
    assert_ne!(edited, first);
    let mut reopened = TuiState::new(32);
    assert!(reopened.restore_project_checkpoint(&restored.project_checkpoint()));
    assert_eq!(reopened.workspace_layout(), &edited);
    assert!(reopened.select_project_tab(1));
    assert_eq!(reopened.workspace_layout(), &second);
}

fn resource_snapshot() -> ResourceSnapshot {
    ResourceSnapshot {
        collected_at_ms: 42,
        workspace: WorkspaceSummary {
            root: "/workspace".into(),
            files: 23,
            directories: 7,
            nodes: 30,
            bytes: 3 * 1024 * 1024,
            truncated: false,
        },
        cpu: CpuSignal {
            logical_cpus: 8,
            load_one_minute: Some(1.25),
            status: SignalStatus::Available,
        },
        memory: MemorySignal {
            total_bytes: Some(16 * 1024 * 1024 * 1024),
            available_bytes: Some(6 * 1024 * 1024 * 1024),
            status: SignalStatus::Available,
        },
        disk: DiskSignal {
            total_bytes: None,
            available_bytes: None,
            status: SignalStatus::Unavailable,
        },
        process: ProcessSignal {
            pid: 123,
            resident_bytes: Some(12 * 1024 * 1024),
            status: SignalStatus::Available,
        },
    }
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
    assert!(output.contains("project"));
    // Goal is an in-workspace command/pane, not a peer top-level tab.
    assert!(!output.contains("1:project"));
    assert!(output.contains("projects:"));
    assert!(!output.contains("/goal"));
    assert!(!output.contains("/learn"));
    assert!(!output.contains("/review"));
    assert!(!output.contains("/session"));
    assert!(output.contains("Conversation"));
    assert!(output.contains("Resources"));
    assert!(output.contains("Gantt"));
    assert!(output.contains("answer"));
    assert!(output.contains("Prompt"));
}

#[test]
fn production_resources_pane_renders_completed_snapshot() {
    let mut terminal = Terminal::new(TestBackend::new(160, 44)).unwrap();
    let mut state = TuiState::default();
    state.set_resource_snapshot(resource_snapshot());

    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();

    let output = rendered(&terminal);
    assert!(output.contains("files 23"));
    assert!(output.contains("dirs 7"));
    assert!(output.contains("3.0 MiB"));
    assert!(output.contains("cpu 8"));
    assert!(output.contains("load 1.25"));
    assert!(output.contains("6.0 GiB"));
    assert!(output.contains("12.0 MiB"));
}

#[test]
fn production_gantt_pane_renders_bounded_domain_projection() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.jsonl");
    let blueprint = Blueprint::new(
        "release-plan",
        "2",
        vec![
            BlueprintItem::new("build", 120),
            BlueprintItem::new("verify", 80).with_dependencies(["build"]),
        ],
    )
    .unwrap();
    let mut goal = Goal::new("ship-release", &blueprint, ResourceBudget::default(), None).unwrap();
    goal.transition_to(GoalStatus::Running).unwrap();
    let mut store = DomainStore::open(path_for_session(&session_path)).unwrap();
    store.put_blueprint(blueprint).unwrap();
    store.put_goal(goal).unwrap();

    let snapshot = collect_gantt_snapshot(&session_path).unwrap();
    assert_eq!(snapshot.blueprint_count(), 1);
    assert_eq!(snapshot.goal_count(), 1);
    assert!(!snapshot.truncated());
    assert!(snapshot.content().len() <= MAX_GANTT_PANE_BYTES);

    let mut terminal = Terminal::new(TestBackend::new(180, 44)).unwrap();
    let mut state = TuiState::default();
    state.set_gantt_snapshot(snapshot);
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let output = rendered(&terminal);
    assert!(output.contains("Blueprints 1"));
    assert!(output.contains("[running] ship-release"));
    assert!(output.contains("release-plan@2"));
    assert!(output.contains("verify"));
    assert!(output.contains("after 1"));
    assert!(!output.contains("No blueprint selected"));
}

#[test]
fn gantt_projection_annotates_each_item_with_latest_execution_status_and_attempt() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.jsonl");
    let blueprint = Blueprint::new(
        "receipt-plan",
        "1",
        vec![
            BlueprintItem::new("build", 12),
            BlueprintItem::new("verify", 34).with_dependencies(["build"]),
        ],
    )
    .unwrap();
    let goal = Goal::new("receipt-goal", &blueprint, ResourceBudget::default(), None).unwrap();
    let mut domains = DomainStore::open(path_for_session(&session_path)).unwrap();
    domains.put_blueprint(blueprint.clone()).unwrap();
    domains.put_goal(goal.clone()).unwrap();

    let mut execution =
        ExecutionStore::open(zenpi::domain_execution::path_for_session(&session_path)).unwrap();
    // The newest attempt is authoritative. The projection must not keep
    // showing an older success once a later attempt is cancelled.
    execution
        .upsert_receipt(ExecutionReceipt {
            execution_id: "build-attempt-1".into(),
            goal_id: goal.id.clone(),
            blueprint_id: blueprint.id.clone(),
            blueprint_version: blueprint.version.clone(),
            blueprint_digest: blueprint.digest.clone(),
            item_id: "build".into(),
            attempt: 1,
            status: ExecutionStatus::Succeeded,
            cost: deterministic_cost(&blueprint.items[0]),
            evidence: "deterministic evidence one".into(),
            external_work_executed: false,
            manifest_checksum: None,
            error: None,
        })
        .unwrap();
    execution
        .upsert_receipt(ExecutionReceipt {
            execution_id: "build-attempt-2".into(),
            goal_id: goal.id.clone(),
            blueprint_id: blueprint.id.clone(),
            blueprint_version: blueprint.version.clone(),
            blueprint_digest: blueprint.digest.clone(),
            item_id: "build".into(),
            attempt: 2,
            status: ExecutionStatus::Cancelled,
            cost: deterministic_cost(&blueprint.items[0]),
            evidence: "deterministic evidence two".into(),
            external_work_executed: false,
            manifest_checksum: None,
            error: Some("operator cancelled".into()),
        })
        .unwrap();
    execution
        .upsert_receipt(ExecutionReceipt {
            execution_id: "verify-running".into(),
            goal_id: goal.id.clone(),
            blueprint_id: blueprint.id.clone(),
            blueprint_version: blueprint.version.clone(),
            blueprint_digest: blueprint.digest.clone(),
            item_id: "verify".into(),
            attempt: 3,
            status: ExecutionStatus::Running,
            cost: deterministic_cost(&blueprint.items[1]),
            evidence: "deterministic evidence running".into(),
            external_work_executed: false,
            manifest_checksum: None,
            error: None,
        })
        .unwrap();

    let snapshot = collect_gantt_snapshot(&session_path).unwrap();
    let content = snapshot.content();
    assert!(
        content
            .contains("build  ready  12 LOC  status cancelled attempt 2 error operator cancelled")
    );
    assert!(content.contains("verify  after 1  34 LOC  status running attempt 3"));
}

#[test]
fn gantt_projection_does_not_create_a_missing_execution_store_and_stays_bounded() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.jsonl");
    let execution_path = zenpi::domain_execution::path_for_session(&session_path);
    let items = (0..MAX_BLUEPRINT_ITEMS)
        .map(|index| {
            BlueprintItem::new(
                format!("item-{index:03}-{}", "x".repeat(96)),
                u32::try_from(index).unwrap(),
            )
        })
        .collect();
    let blueprint = Blueprint::new("pending-plan", "1", items).unwrap();
    let mut domains = DomainStore::open(path_for_session(&session_path)).unwrap();
    domains.put_blueprint(blueprint).unwrap();

    assert!(!execution_path.exists());
    let snapshot = collect_gantt_snapshot(&session_path).unwrap();
    assert!(!execution_path.exists());
    assert!(snapshot.content().len() <= MAX_GANTT_PANE_BYTES);
    assert!(snapshot.content().lines().count() <= zenpi::tui::MAX_GANTT_PANE_ROWS);
    assert!(snapshot.content().contains("status pending"));
    assert!(snapshot.truncated());
}

#[test]
fn gantt_refresh_failure_retains_last_valid_projection() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.jsonl");
    let blueprint = Blueprint::new(
        "retained-plan",
        "1",
        vec![BlueprintItem::new("bounded-item", 4_999)],
    )
    .unwrap();
    let mut store = DomainStore::open(path_for_session(&session_path)).unwrap();
    store.put_blueprint(blueprint).unwrap();
    let snapshot = collect_gantt_snapshot(&session_path).unwrap();

    let mut terminal = Terminal::new(TestBackend::new(180, 44)).unwrap();
    let mut state = TuiState::default();
    state.set_gantt_snapshot(snapshot);
    state.gantt_refresh_failed("mock store error\nwith control");
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let output = rendered(&terminal);
    assert!(output.contains("retained-plan@1"));
    assert!(output.contains("refresh failed: mock store errorwith control"));
}

#[test]
fn gantt_projection_truncates_maximal_valid_blueprint() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.jsonl");
    let items = (0..MAX_BLUEPRINT_ITEMS)
        .map(|index| {
            BlueprintItem::new(
                format!("item-{index:03}-{}", "x".repeat(96)),
                u32::try_from(index).unwrap(),
            )
        })
        .collect();
    let blueprint = Blueprint::new("large-plan", "1", items).unwrap();
    let mut store = DomainStore::open(path_for_session(&session_path)).unwrap();
    store.put_blueprint(blueprint).unwrap();

    let snapshot = collect_gantt_snapshot(&session_path).unwrap();
    assert!(snapshot.truncated());
    assert!(snapshot.content().len() <= MAX_GANTT_PANE_BYTES);
    assert!(snapshot.content().lines().count() <= zenpi::tui::MAX_GANTT_PANE_ROWS);
    assert!(snapshot.content().contains("Gantt projection truncated"));
}

#[test]
fn ctrl_number_switches_workspace_tab_without_submitting_prompt() {
    let mut state = TuiState::default();
    state.set_input("draft");
    let action = state.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::CONTROL));
    assert_eq!(action, TuiAction::Redraw);
    assert_eq!(state.active_project(), "default");
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
fn terminal_backtab_moves_workspace_focus_backwards() {
    let mut state = TuiState::default();
    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(state.focused_workspace_pane(), Some(PaneId::Resources));

    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
        TuiAction::Redraw
    );
    assert_eq!(
        state.focused_workspace_pane(),
        Some(PaneId::ProjectConversation)
    );
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

#[test]
fn tab_switch_keeps_independent_layout_state_and_dirty_bit() {
    let mut state = TuiState::default();
    assert!(!state.layout_dirty());

    assert!(state.adjust_workspace_split(FocusDirection::Right));
    assert!(state.layout_dirty());
    let project_ratios = state.workspace_layout().ratios;
    state.clear_layout_dirty();
    assert!(!state.layout_dirty());

    state.set_workspace_tab(TabId::Goal);
    assert_eq!(state.workspace_tab(), TabId::Goal);
    assert_eq!(
        state.workspace_layout().ratios,
        LayoutModel::new(TabId::Goal).ratios
    );
    assert!(!state.layout_dirty());
    assert!(state.adjust_workspace_split(FocusDirection::Right));
    let goal_ratios = state.workspace_layout().ratios;
    assert_ne!(goal_ratios, LayoutModel::new(TabId::Goal).ratios);

    state.set_workspace_tab(TabId::Project);
    assert_eq!(state.workspace_layout().ratios, project_ratios);
    state.reset_workspace_layout();
    assert!(state.layout_dirty());
    assert_eq!(
        state.workspace_layout().ratios,
        LayoutModel::new(TabId::Project).ratios
    );
}

#[test]
fn restoring_tab_models_preserves_each_tab_and_clears_dirty_state() {
    let mut project = LayoutModel::new(TabId::Project);
    project.set_ratios(zenpi::layout::ColumnRatios::new(40, 35, 25));
    let mut goal = LayoutModel::new(TabId::Goal);
    goal.set_ratios(zenpi::layout::ColumnRatios::new(25, 50, 25));
    let mut state = TuiState::default();
    state.restore_workspace_layouts([project.clone(), goal.clone()]);

    assert!(!state.layout_dirty());
    assert_eq!(state.workspace_layout().ratios, project.ratios);
    state.set_workspace_tab(TabId::Goal);
    assert_eq!(state.workspace_layout().ratios, goal.ratios);
    state.set_workspace_tab(TabId::Project);
    assert_eq!(state.workspace_layout().ratios, project.ratios);
}

#[test]
fn bentobox_focus_resize_collapse_persist_across_independent_restore() {
    let mut state = TuiState::default();
    assert!(state.focus_workspace_pane(PaneId::ProjectConversation));
    assert!(state.adjust_workspace_split(FocusDirection::Right));
    let resized = state.workspace_layout().ratios;
    assert_eq!(state.toggle_workspace_pane(PaneId::Resources), Some(true));
    assert!(state.focus_workspace_pane(PaneId::Gantt));
    let checkpoint = state.project_checkpoint();

    let mut restored = TuiState::new(32);
    assert!(restored.restore_project_checkpoint(&checkpoint));
    assert_eq!(restored.workspace_layout().ratios, resized);
    assert!(
        restored
            .workspace_layout()
            .collapsed
            .contains(&PaneId::Resources)
    );
    assert_eq!(restored.workspace_layout().focused, Some(PaneId::Gantt));

    assert_eq!(
        restored.toggle_workspace_pane(PaneId::Resources),
        Some(false)
    );
    restored.reset_workspace_layout();
    assert_eq!(
        restored.workspace_layout().ratios,
        LayoutModel::new(TabId::Project).ratios
    );
    assert!(restored.workspace_layout().collapsed.is_empty());
    assert_eq!(restored.workspace_layout().focused, None);

    let mut reopened = TuiState::new(32);
    assert!(reopened.restore_project_checkpoint(&restored.project_checkpoint()));
    assert_eq!(reopened.workspace_layout(), restored.workspace_layout());
}

#[test]
fn unavailable_pane_toggle_is_rejected_without_layout_mutation() {
    let mut state = TuiState::default();
    let before = state.workspace_layout().clone();
    assert_eq!(state.toggle_workspace_pane(PaneId::Browser), None);
    assert_eq!(state.toggle_workspace_pane(PaneId::Terminal), None);
    assert_eq!(state.workspace_layout(), &before);
}

#[test]
fn short_workspace_tab_cycle_skips_panes_that_have_no_screen_rows() {
    let mut terminal = Terminal::new(TestBackend::new(180, 7)).unwrap();
    let mut state = TuiState::default();
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let output = rendered(&terminal);
    assert!(output.contains("Gantt"));
    assert!(!output.contains("Resources"));
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        TuiAction::Redraw
    );
    assert_eq!(
        state.focused_workspace_pane(),
        Some(PaneId::ProjectConversation)
    );
    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(state.focused_workspace_pane(), Some(PaneId::Gantt));
    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(
        state.focused_workspace_pane(),
        Some(PaneId::ProjectConversation)
    );
    state.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(state.focused_workspace_pane(), Some(PaneId::Gantt));
    assert!(state.input().is_empty());
}

#[test]
fn short_workspace_direction_follows_the_panes_actually_drawn() {
    let mut terminal = Terminal::new(TestBackend::new(180, 7)).unwrap();
    let mut state = TuiState::default();
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    assert!(rendered(&terminal).contains("Gantt"));
    assert!(state.focus_workspace_pane(PaneId::Gantt));
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL)),
        TuiAction::Redraw
    );
    assert_eq!(
        state.focused_workspace_pane(),
        Some(PaneId::ProjectConversation)
    );
    state.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL));
    assert_eq!(state.focused_workspace_pane(), Some(PaneId::Gantt));
}

#[test]
fn short_workspace_narrow_cycle_reveals_each_selected_pane() {
    let mut terminal = Terminal::new(TestBackend::new(60, 7)).unwrap();
    let mut state = TuiState::default();
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    assert_eq!(state.focused_workspace_pane(), Some(PaneId::Resources));
    assert!(rendered(&terminal).contains("Resources"));
    state.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    assert_eq!(
        state.focused_workspace_pane(),
        Some(PaneId::ProjectConversation)
    );
    assert!(rendered(&terminal).contains("Conversation"));
}
