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
    CpuSignal, DiskSignal, GpuDevice, GpuSignal, MemorySignal, NetworkSignal, ProcessClass,
    ProcessClassRow, ProcessSignal, ProcessSummary, ResourceSnapshot, SignalStatus,
    WorkspaceSummary,
};
use zenpi::tui::{
    ARCH_PROMPT_PANE_ROWS, BentoBoxLayoutAdapter, MAX_GANTT_PANE_BYTES, MessageRole,
    RESOURCE_REFRESH_INTERVAL, TuiAction, TuiState, collect_gantt_snapshot,
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
        network: NetworkSignal {
            received_bytes: Some(2 * 1024 * 1024 * 1024),
            transmitted_bytes: Some(512 * 1024 * 1024),
            status: SignalStatus::Available,
        },
        gpu: GpuSignal {
            devices: vec![GpuDevice {
                name: "RTX 4090".into(),
                utilization_percent: Some(45.0),
                memory_total_bytes: Some(24 * 1024 * 1024 * 1024),
                memory_used_bytes: Some(3 * 1024 * 1024 * 1024),
            }],
            truncated: false,
            status: SignalStatus::Available,
        },
        processes: ProcessSummary {
            total: 128,
            resident_bytes: 1_300 * 1024 * 1024,
            rows: vec![
                ProcessClassRow {
                    class: ProcessClass::Zenpi,
                    count: 2,
                    resident_bytes: 300 * 1024 * 1024,
                },
                ProcessClassRow {
                    class: ProcessClass::Lsp,
                    count: 5,
                    resident_bytes: 900 * 1024 * 1024,
                },
                ProcessClassRow {
                    class: ProcessClass::Mcp,
                    count: 3,
                    resident_bytes: 100 * 1024 * 1024,
                },
            ],
            truncated: false,
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
    assert!(output.contains("workspaces"));
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
    let mut terminal = Terminal::new(TestBackend::new(160, 60)).unwrap();
    let mut state = TuiState::default();
    state.set_resource_snapshot(resource_snapshot());
    state.set_context_usage(Some(12_000), Some(200_000));

    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();

    let output = rendered(&terminal);
    // Existing workspace/host contract is preserved.
    assert!(output.contains("Files 23"));
    assert!(output.contains("Dirs 7"));
    assert!(output.contains("3.0 MiB"));
    assert!(output.contains("CPU 8"));
    assert!(output.contains("load 1.25"));
    assert!(output.contains("6.0 GiB"));
    assert!(output.contains("12.0 MiB"));
    // New htop/nvidia-smi style signals: GPU, network, merged processes, and
    // the opencode-style context/lsp/mcp column.
    assert!(output.contains("GPU RTX 4090 45% 3.0 GiB/24.0 GiB"));
    assert!(output.contains("Net rx 2.0 GiB  tx 512.0 MiB"));
    assert!(output.contains("Processes 128 total"));
    assert!(output.contains("LSP 5 servers  MCP 3 servers"));
    assert!(output.contains("Context history 12000/200000 tokens"));
    assert!(output.contains("zenpi"));
}

#[test]
fn resource_monitor_colours_cpu_memory_gpu_and_network_sections() {
    // Taller viewport: the six-row header leaves less room for the resource
    // pane, and the network (blue) line must remain visible.
    let mut terminal = Terminal::new(TestBackend::new(200, 80)).unwrap();
    let mut state = TuiState::default();
    state.set_resource_snapshot(resource_snapshot());
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();

    let colors: Vec<ratatui::style::Color> = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.fg)
        .collect();
    for expected in [
        ratatui::style::Color::Cyan,
        ratatui::style::Color::Green,
        ratatui::style::Color::Magenta,
        ratatui::style::Color::Blue,
        ratatui::style::Color::Yellow,
    ] {
        assert!(colors.contains(&expected), "missing colour {expected:?}");
    }
}

#[test]
fn resource_monitor_merges_process_classes_without_detail_rows() {
    let mut terminal = Terminal::new(TestBackend::new(160, 60)).unwrap();
    let mut state = TuiState::default();
    state.set_resource_snapshot(resource_snapshot());
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();

    let output = rendered(&terminal);
    assert!(output.contains("Processes 128 total"));
    assert!(output.contains("lsp"));
    assert!(output.contains("mcp"));
    // Merged statistics keep one row per class instead of per-process detail.
    assert!(!output.contains("pid 123"));
}

#[test]
fn resource_refresh_interval_is_five_seconds() {
    assert_eq!(RESOURCE_REFRESH_INTERVAL, std::time::Duration::from_secs(5));
}

fn lan_fixture_snapshot() -> zenpi::net_probe::LanSnapshot {
    use zenpi::net_probe::{DeviceClass, LanBlock, LanHost, LanSnapshot};
    let mut local = LanHost::new("10.20.30.14");
    local.mac = Some("9c:76:0e:7d:3c:27".into());
    local.vendor = Some("Apple".into());
    local.class = DeviceClass::Local;
    local.open_ports = vec![22, 5900];
    let mut gateway = LanHost::new("10.20.30.1");
    gateway.class = DeviceClass::Gateway;
    gateway.open_ports = vec![80];
    let mut nas = LanHost::new("10.20.30.177");
    nas.class = DeviceClass::Nas;
    nas.vendor = Some("Synology".into());
    nas.open_ports = vec![445, 5000, 5001];
    let hosts = vec![local, gateway, nas];
    let blocks = vec![
        LanBlock {
            class: DeviceClass::Local,
            title: "本机 (1)".into(),
            host_indices: vec![0],
        },
        LanBlock {
            class: DeviceClass::Gateway,
            title: "网关 (1)".into(),
            host_indices: vec![1],
        },
        LanBlock {
            class: DeviceClass::Nas,
            title: "存储(NAS) (1)".into(),
            host_indices: vec![2],
        },
    ];
    LanSnapshot {
        local_ip: "10.20.30.14".into(),
        gateway_ip: Some("10.20.30.1".into()),
        hosts,
        blocks,
        scanned_at_ms: 1,
        truncated: false,
    }
}

#[test]
fn resources_pane_groups_lan_hosts_into_blocks_with_drilldown() {
    let mut terminal = Terminal::new(TestBackend::new(180, 70)).unwrap();
    let mut state = TuiState::default();
    state.set_lan_snapshot(lan_fixture_snapshot());
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();

    let output: String = rendered(&terminal)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(output.contains("LAN10.20.30.0/24"), "{output}");
    assert!(output.contains("本机(1)"), "{output}");
    assert!(output.contains("网关(1)"), "{output}");
    assert!(output.contains("存储(NAS)(1)"), "{output}");

    // Drill into the storage block and verify the detail table columns.
    state.move_resource_block(2);
    state.activate_resource_block();
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let detail = rendered(&terminal);
    assert!(detail.contains("10.20.30.177"));
    assert!(detail.contains("Synology"));
    assert!(detail.contains("5000,5001"), "{detail}");

    state.close_resource_block();
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    assert!(rendered(&terminal).contains("LAN 10.20.30.0/24"));
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

#[test]
fn discussion_prompt_is_grouped_with_the_left_column_conversation() {
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    let mut state = TuiState::default();
    state.set_input("draft");
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let output = rendered(&terminal);
    assert!(output.contains("Conversation"));
    assert!(output.contains("Prompt"));
    assert!(output.contains("Alt-G edit"));
    assert!(output.contains("draft"));
    // The prompt is rendered inside the conversation pane, so it keeps the
    // left column width and never spills into the center/right columns.
    let adapter = BentoBoxLayoutAdapter::new(state.workspace_layout(), Rect::new(0, 5, 140, 29));
    let conversation = adapter.pane(PaneId::ProjectConversation).unwrap();
    let (_, prompt) = zenpi::layout::conversation_prompt_group(
        zenpi::layout::PaneRect::new(
            conversation.rect.x,
            conversation.rect.y,
            conversation.rect.width,
            conversation.rect.height,
        ),
        zenpi::tui::PROMPT_PANE_ROWS,
    );
    assert_eq!(prompt.x, conversation.rect.x);
    assert_eq!(prompt.width, conversation.rect.width);
}

#[test]
fn inline_goal_edit_is_bounded_and_emits_a_persist_intent() {
    let mut state = TuiState::default();
    assert!(!state.goal_edit_active());
    assert!(state.begin_goal_edit());
    assert!(state.goal_edit_active());
    for character in "ship release".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    assert_eq!(state.goal_edit_buffer(), Some("ship release"));
    assert_eq!(state.commit_goal_edit().as_deref(), Some("ship release"));
    assert!(!state.goal_edit_active());
    assert_eq!(state.goal_text(), "ship release");
    assert_eq!(
        state.take_goal_edit_intent().as_deref(),
        Some("ship release")
    );
    assert!(state.take_goal_edit_intent().is_none());
}

#[test]
fn inline_goal_edit_rejects_empty_and_escape_restores_the_previous_goal() {
    let mut state = TuiState::default();
    assert!(state.begin_goal_edit());
    assert!(state.commit_goal_edit().is_none());
    assert!(state.goal_edit_active());
    assert!(state.cancel_goal_edit());
    assert!(!state.goal_edit_active());
    assert!(state.goal_text().is_empty());
    assert!(state.take_goal_edit_intent().is_none());
}

#[test]
fn open_goal_editor_replaces_the_docked_prompt_in_place() {
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    let mut state = TuiState::default();
    state.set_input("hidden draft");
    assert!(state.begin_goal_edit());
    for character in "bounded".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let output = rendered(&terminal);
    assert!(output.contains("Goal · Enter save"));
    assert!(output.contains("bounded"));
    assert!(!output.contains("hidden draft"));
}

#[test]
fn narrow_viewport_falls_back_to_the_bottom_prompt_strip() {
    let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
    let mut state = TuiState::default();
    state.set_input("kept");
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let output = rendered(&terminal);
    assert!(output.contains("Prompt"));
    assert!(output.contains("kept"));
}

#[test]
fn arch_prompt_group_tiles_the_pane_at_left_column_width() {
    use zenpi::layout::{PaneRect, arch_prompt_group};

    let pane = PaneRect::new(5, 10, 30, 12);
    let (conversation, prompt) = arch_prompt_group(pane, ARCH_PROMPT_PANE_ROWS);
    // Arch + Prompt are one group: the prompt keeps the pane's x/width and sits
    // directly beneath the transcript, so arch never spills into the center
    // column.
    assert_eq!(prompt.x, pane.x);
    assert_eq!(prompt.width, pane.width);
    assert_eq!(prompt.y, conversation.bottom());
    assert_eq!(conversation.height + prompt.height, pane.height);
    assert!(!conversation.intersects(prompt));

    // Degenerate panes never overlap and always keep one transcript row.
    let (conversation, prompt) =
        arch_prompt_group(PaneRect::new(0, 0, 10, 1), ARCH_PROMPT_PANE_ROWS);
    assert_eq!(prompt.height, 0);
    assert_eq!(conversation.height, 1);

    let (conversation, prompt) =
        arch_prompt_group(PaneRect::new(0, 0, 10, 4), ARCH_PROMPT_PANE_ROWS);
    assert_eq!(prompt.height, 3);
    assert_eq!(conversation.height, 1);
}

#[test]
fn arch_master_prompt_is_grouped_in_the_left_column() {
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    let mut state = TuiState::default();
    state.push_arch_message(MessageRole::User, "!echo hi");
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let output = rendered(&terminal);
    assert!(output.contains("Arch · master session"));
    assert!(output.contains("Arch prompt"));
    assert!(output.contains("!echo hi"));

    // The rendered prompt shares the arch pane's exact left-column width.
    let adapter = BentoBoxLayoutAdapter::new(state.workspace_layout(), Rect::new(0, 5, 140, 29));
    let arch = adapter.pane(PaneId::Arch).unwrap().rect;
    let (_, prompt) = zenpi::layout::arch_prompt_group(
        zenpi::layout::PaneRect::new(arch.x, arch.y, arch.width, arch.height),
        ARCH_PROMPT_PANE_ROWS,
    );
    assert_eq!(prompt.x, arch.x);
    assert_eq!(prompt.width, arch.width);
}

#[test]
fn arch_console_classifies_bash_and_steering_boundedly() {
    use zenpi::tool_runtime::{
        MAX_MASTER_SESSION_INPUT_BYTES, MasterSessionCommand, classify_master_session_input,
    };

    assert_eq!(
        classify_master_session_input("!echo hi").unwrap(),
        MasterSessionCommand::Bash("echo hi".into())
    );
    assert_eq!(
        classify_master_session_input("  pause the workers  ").unwrap(),
        MasterSessionCommand::Steer("pause the workers".into())
    );
    assert!(classify_master_session_input("   ").is_err());
    assert!(classify_master_session_input("!   ").is_err());
    assert!(classify_master_session_input("bad\u{7}escape").is_err());
    let oversized = "x".repeat(MAX_MASTER_SESSION_INPUT_BYTES + 1);
    assert!(classify_master_session_input(&oversized).is_err());
}

#[test]
fn arch_master_console_serializes_bash_but_lets_steering_join() {
    use zenpi::tool_runtime::{MasterSessionCommand, MasterSessionInputError};

    let mut state = TuiState::default();
    state.set_arch_input("!echo one");
    assert_eq!(
        state.submit_arch_prompt().unwrap(),
        TuiAction::SubmitArch(MasterSessionCommand::Bash("echo one".into()))
    );
    assert!(state.master_busy());
    assert_eq!(state.arch_message_count(), 1);

    // A second bash submission is rejected while the master turn is active,
    // and the operator's draft is preserved.
    state.set_arch_input("!echo two");
    assert_eq!(
        state.submit_arch_prompt().unwrap_err(),
        MasterSessionInputError::Busy
    );
    assert_eq!(state.arch_input(), "!echo two");

    // Steering is single-concurrency too: it joins the active turn rather than
    // forking a second master worker.
    state.set_arch_input("keep going");
    assert_eq!(
        state.submit_arch_prompt().unwrap(),
        TuiAction::SubmitArch(MasterSessionCommand::Steer("keep going".into()))
    );
    assert!(state.master_busy());
    state.complete_master_turn(MessageRole::System, "done");
    assert!(!state.master_busy());
    assert_eq!(
        state.take_arch_intent(),
        Some(MasterSessionCommand::Steer("keep going".into()))
    );
    assert_eq!(state.take_arch_intent(), None);
}

#[test]
fn headless_arch_route_maps_bash_and_steer_to_bounded_commands() {
    use zenpi::protocol::Command;

    match zenpi::headless::master_session_command("!echo hi", None).unwrap() {
        Command::UserShell(request) => assert_eq!(request.input, "!echo hi"),
        other => panic!("expected a user-shell command, got {other:?}"),
    }
    match zenpi::headless::master_session_command("pause the workers", Some("turn-7".into()))
        .unwrap()
    {
        Command::Steer {
            text,
            expected_turn_id,
        } => {
            assert_eq!(text, "pause the workers");
            assert_eq!(expected_turn_id.as_deref(), Some("turn-7"));
        }
        other => panic!("expected a steer command, got {other:?}"),
    }
    assert!(zenpi::headless::master_session_command("   ", None).is_err());
    assert!(zenpi::headless::master_session_command("!  ", None).is_err());
}
