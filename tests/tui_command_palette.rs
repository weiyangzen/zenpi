use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use std::{fs, path::Path};
use tempfile::tempdir;
use zenpi::{
    resource_loader::{ResourceLoader, ResourcePaths},
    slash::InputRoute,
    tui::{ProjectTabMetadata, TuiState, complete_file_query},
};
fn state(root: &Path) -> TuiState {
    let mut state = TuiState::default();
    let session =
        zenpi::session::SessionStore::open_in_workspace(root.join("session.jsonl"), root).unwrap();
    state.set_active_project_metadata(ProjectTabMetadata::from_session(&session));
    state
}
fn snapshot(root: &Path) -> std::sync::Arc<zenpi::resource_loader::ResourceSnapshot> {
    fs::create_dir_all(root.join("skills/example")).unwrap();
    fs::create_dir_all(root.join("prompts")).unwrap();
    fs::write(root.join("skills/example/SKILL.md"),"---\nname: example\ndescription: Example resource\ndisable-model-invocation: true\n---\nSECRET_BODY_NOT_IN_MENU\n").unwrap();
    fs::write(
        root.join("prompts/greet.md"),
        "---\ndescription: Greeting template\n---\nHello $1\n",
    )
    .unwrap();
    fs::write(root.join("prompts/review.md"), "Reserved name body\n").unwrap();
    ResourceLoader::new(ResourcePaths {
        user_skills: root.join("missing-user"),
        project_skills: root.join("skills"),
        skill_paths: vec![],
        user_templates: root.join("missing-templates"),
        project_templates: root.join("prompts"),
        template_paths: vec![],
        text_resources: vec![],
    })
    .unwrap()
    .reload(|| false)
    .unwrap()
}
fn render(state: &mut TuiState) -> String {
    let mut t = Terminal::new(TestBackend::new(140, 40)).unwrap();
    t.draw(|f| state.render_bentobox(f, "zenpi")).unwrap();
    (0..40)
        .map(|y| {
            (0..140)
                .map(|x| t.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn file_completion_partial_menu_explains_hidden_matches() {
    let root = tempdir().unwrap();
    for index in 0..200 {
        fs::write(root.path().join(format!("candidate-{index:03}.txt")), "").unwrap();
    }
    let mut s = state(root.path());
    s.set_input("/attach candidate-");
    let query = s.file_completion_query().unwrap();
    s.apply_file_completion(
        query.clone(),
        complete_file_query(&query, || false).unwrap(),
    );
    assert_eq!(s.slash_choices().len(), 128);
    let screen = render(&mut s);
    assert!(screen.contains("Partial results"), "{screen}");
    assert!(screen.contains("display limit"), "{screen}");
    assert!(
        screen.contains("Narrow the path or filename prefix"),
        "{screen}"
    );
    assert_eq!(s.input(), "/attach candidate-");
}

#[test]
fn file_completion_complete_empty_menu_reports_only_this_directory() {
    let root = tempdir().unwrap();
    let mut s = state(root.path());
    s.set_input("/attach absent-prefix");
    let query = s.file_completion_query().unwrap();
    s.apply_file_completion(
        query.clone(),
        complete_file_query(&query, || false).unwrap(),
    );
    let screen = render(&mut s);
    assert!(
        screen.contains("No matching files in this directory"),
        "{screen}"
    );
    assert!(!screen.contains("Partial results"), "{screen}");
    assert_eq!(s.input(), "/attach absent-prefix");
}

#[test]
fn file_completion_partial_status_is_query_and_project_bound() {
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    for index in 0..200 {
        fs::write(first.path().join(format!("candidate-{index:03}")), "").unwrap();
    }
    fs::write(second.path().join("candidate-only"), "").unwrap();
    let mut s = state(first.path());
    s.set_input("/attach candidate-");
    let a = s.file_completion_query().unwrap();
    let result_a = complete_file_query(&a, || false).unwrap();
    assert!(s.apply_file_completion(a.clone(), result_a.clone()));
    assert!(render(&mut s).contains("Partial results"));
    s.set_input("/attach different");
    assert!(!s.apply_file_completion(a.clone(), result_a.clone()));
    assert!(!render(&mut s).contains("Partial results"));
    s.open_project_tab("second");
    let session = zenpi::session::SessionStore::open_in_workspace(
        second.path().join("session.jsonl"),
        second.path(),
    )
    .unwrap();
    s.set_active_project_metadata(ProjectTabMetadata::from_session(&session));
    s.set_input("/attach candidate-");
    assert!(!s.apply_file_completion(a.clone(), result_a));
    assert!(!render(&mut s).contains("Partial results"));
    let b = s.file_completion_query().unwrap();
    assert_ne!(a, b, "same text must still retain root identity");
    let result_b = complete_file_query(&b, || false).unwrap();
    assert!(s.apply_file_completion(b.clone(), result_b.clone()));
    assert_eq!(s.slash_choices(), ["/attach \"candidate-only\""]);
    assert!(!render(&mut s).contains("Partial results"));
    s.select_project_tab(0);
    s.set_input("/attach candidate-");
    assert!(!s.apply_file_completion(b, result_b));
    assert!(!render(&mut s).contains("Partial results"));
}

#[test]
fn file_completion_notice_rows_cannot_be_selected_and_escape_keeps_draft() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let root = tempdir().unwrap();
    for index in 0..200 {
        fs::write(root.path().join(format!("candidate-{index:03}")), "").unwrap();
    }
    let mut s = state(root.path());
    s.set_input("/attach candidate-");
    let query = s.file_completion_query().unwrap();
    s.apply_file_completion(
        query.clone(),
        complete_file_query(&query, || false).unwrap(),
    );
    let screen = render(&mut s);
    let row = screen
        .lines()
        .position(|line| line.contains("Partial results"))
        .unwrap();
    s.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 4,
        row: row as u16,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(s.input(), "/attach candidate-");
    s.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    render(&mut s);
    s.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(s.input(), "/attach \"candidate-001\" ");
    assert!(!render(&mut s).contains("Partial results"));
    s.set_input("/attach none-");
    let q = s.file_completion_query().unwrap();
    s.apply_file_completion(q.clone(), complete_file_query(&q, || false).unwrap());
    assert!(render(&mut s).contains("No matching files"));
    s.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(s.input(), "/attach none-");
    assert!(!render(&mut s).contains("No matching files"));
}

#[test]
fn bare_approval_selectors_defer_owner_changes_until_explicit_submit() {
    use zenpi::{
        approval::{ApprovalDecision, ApprovalMode},
        core::Agent,
        tools::{SideEffectPolicy, ToolContext, ToolRegistry},
        tui::{TuiAction, dispatch_slash_command},
    };
    for alias in ["/approval", "/approvals"] {
        let root = tempdir().unwrap();
        let mut state = state(root.path());
        let session = zenpi::session::SessionStore::open(root.path().join("owner.jsonl")).unwrap();
        let mut owner = Agent::with_echo(session);
        owner.set_tools(
            ToolRegistry::with_all_builtins().unwrap(),
            ToolContext::new(root.path()).unwrap(),
            SideEffectPolicy::all_builtins(),
        );
        let mut policy = owner.configured_approval_policy().unwrap();
        policy.mode = ApprovalMode::Never;
        policy
            .per_tool
            .insert("write_file".into(), ApprovalDecision::Deny);
        owner.set_approval_policy(policy.clone());
        let project = state.active_project().to_owned();
        let layout = state.active_project_workspace().clone();
        let press =
            |state: &mut TuiState, key| state.handle_key(KeyEvent::new(key, KeyModifiers::NONE));

        state.set_input(alias);
        assert!(
            matches!(press(&mut state, KeyCode::Enter), TuiAction::Redraw),
            "{alias}"
        );
        assert_eq!(state.input(), format!("{alias} "));
        let screen = render(&mut state);
        for mode in ["ask", "always", "never"] {
            assert!(screen.contains(&format!("{alias} {mode}")));
        }
        press(&mut state, KeyCode::Esc);
        assert_eq!(state.input(), format!("{alias} "));
        assert!(state.slash_choices().is_empty());
        assert_eq!(owner.configured_approval_policy().unwrap(), policy);
        assert_eq!(state.active_project(), project);
        assert_eq!(state.active_project_workspace(), &layout);

        state.set_input(alias);
        assert!(matches!(
            press(&mut state, KeyCode::Enter),
            TuiAction::Redraw
        ));
        press(&mut state, KeyCode::Down);
        assert!(matches!(
            press(&mut state, KeyCode::Enter),
            TuiAction::Redraw
        ));
        assert_eq!(state.input(), format!("{alias} always "));
        assert_eq!(owner.configured_approval_policy().unwrap(), policy);
        let TuiAction::Submit(text) = press(&mut state, KeyCode::Enter) else {
            panic!("explicit confirmation must submit the selected policy for {alias}");
        };
        let InputRoute::Slash(command) = zenpi::slash::route_input(&text).unwrap() else {
            panic!("policy selection must use the real slash owner");
        };
        dispatch_slash_command(command, &mut state, Some(&mut owner));
        policy.mode = ApprovalMode::Always;
        assert_eq!(owner.configured_approval_policy().unwrap(), policy);
        assert_eq!(
            owner.approval_policy().unwrap().per_tool["write_file"],
            ApprovalDecision::Deny
        );

        // Explicit arguments keep the existing path; busy policy changes
        // remain advertised as unavailable instead of gaining a new owner.
        state.set_input(format!("{alias} never"));
        assert!(matches!(
            press(&mut state, KeyCode::Enter),
            TuiAction::Submit(_)
        ));
        state.set_busy(true);
        assert!(!state.command_available(alias));
        assert!(!state.command_available(&format!("{alias} always")));
    }
}

#[test]
fn catalog_contains_actual_explicit_resources_and_source_but_not_bodies() {
    let root = tempdir().unwrap();
    let mut state = state(root.path());
    state.update_resource_catalog(&snapshot(root.path()));
    state.set_input("/skill:e");
    assert_eq!(state.slash_choices(), vec!["/skill:example"]);
    let screen = render(&mut state);
    assert!(screen.contains("Example resource"));
    assert!(screen.contains("SKILL.md"));
    assert!(!screen.contains("SECRET_BODY"));
    assert!(matches!(
        state.route_catalog_input("/skill:example literal $()"),
        Ok(InputRoute::Prompt(_))
    ));
    assert!(matches!(
        state.route_catalog_input("/greet one"),
        Ok(InputRoute::Prompt(_))
    ));
    assert!(matches!(
        state.route_catalog_input("/review"),
        Ok(InputRoute::Slash(_))
    ));
    assert!(state.route_catalog_input("/not-a-resource").is_err());
}

#[test]
fn long_unicode_provenance_keeps_filename_hash_and_real_source() {
    let root = tempdir().unwrap();
    let long = root
        .path()
        .join(format!("工作 e\u{301} {}", "a".repeat(180)))
        .join(format!("来源 space {}", "b".repeat(180)))
        .join(format!("项目 {}", "c".repeat(180)));
    fs::create_dir_all(&long).unwrap();
    let resources = snapshot(&long);
    let metadata = resources.skills.metadata().next().unwrap();
    assert!(metadata.source.to_string_lossy().chars().count() > 512);
    assert!(
        fs::read_to_string(&metadata.source)
            .unwrap()
            .contains("SECRET_BODY_NOT_IN_MENU")
    );
    let mut s = state(&long);
    s.update_resource_catalog(&resources);
    s.set_input("/skill:e");
    for width in [140, 80, 42, 16, 1] {
        let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
        terminal.draw(|f| s.render_bentobox(f, "zenpi")).unwrap();
        let screen = (0..40)
            .map(|y| {
                (0..width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        if width >= 16 {
            assert!(screen.contains("SKILL.md"), "width={width}: {screen}");
            assert!(!screen.contains("SECRET_BODY_NOT_IN_MENU"));
        }
        if width >= 42 {
            assert!(screen.contains(&metadata.source_hash[..12]), "{screen}");
        }
        assert_eq!(s.input(), "/skill:e");
    }
    s.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(s.input(), "/skill:example ");
    assert!(matches!(
        s.route_catalog_input(s.input()),
        Ok(InputRoute::Prompt(_))
    ));
    assert_eq!(metadata.source.file_name().unwrap(), "SKILL.md");
}
#[test]
fn project_catalogue_isolated_and_escape_keeps_literal_input() {
    let root = tempdir().unwrap();
    let mut s = state(root.path());
    s.update_resource_catalog(&snapshot(root.path()));
    s.set_input("/skill:");
    s.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(s.input(), "/skill:");
    assert!(s.slash_choices().is_empty());
    s.open_project_tab("second");
    s.set_input("/skill:");
    assert!(s.slash_choices().is_empty());
    s.select_project_tab(0);
    s.set_input("/skill:");
    assert_eq!(s.slash_choices(), vec!["/skill:example"]);
}
#[test]
fn busy_palette_declares_idle_commands_and_queueable_resources() {
    let root = tempdir().unwrap();
    let mut s = state(root.path());
    s.set_busy(true);
    s.set_input("/model");
    assert!(!s.command_available("/model"));
    assert!(render(&mut s).contains("idle only"));
    assert!(s.command_available("/reload"));
    assert!(s.command_available("/cancel"));
    assert!(!s.command_available("/attach"));
}
#[test]
fn file_completion_is_cwd_bound_quoted_and_stale_query_safe() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("工作 folder")).unwrap();
    fs::write(root.path().join("工作 folder/a file.txt"), "content").unwrap();
    let mut s = state(root.path());
    s.set_input("explain @工作");
    let q = s.file_completion_query().unwrap();
    let entries = complete_file_query(&q, || false).unwrap();
    assert!(s.apply_file_completion(q.clone(), entries));
    assert_eq!(s.slash_choices(), vec!["explain @\"工作 folder/\""]);
    s.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    let next = s.file_completion_query().unwrap();
    let entries = complete_file_query(&next, || false).unwrap();
    s.apply_file_completion(next, entries);
    assert_eq!(
        s.slash_choices(),
        vec!["explain @\"工作 folder/a file.txt\""]
    );
    let stale = complete_file_query(&q, || false).unwrap();
    assert!(!s.apply_file_completion(q, stale));
    s.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(s.input(), "explain @\"工作 folder/a file.txt\" ");
    assert!(s.file_completion_query().is_none());
    s.set_input("@file.txt explain this");
    assert!(s.file_completion_query().is_none());
}
#[test]
fn slash_paths_and_negative_boundaries_do_not_mutate_draft() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("my file.txt"), "content").unwrap();
    let mut s = state(root.path());
    s.set_input("/attach my");
    let q = s.file_completion_query().unwrap();
    s.apply_file_completion(q.clone(), complete_file_query(&q, || false).unwrap());
    assert_eq!(s.slash_choices(), vec!["/attach \"my file.txt\""]);
    s.set_input("/attach ../");
    let q = s.file_completion_query().unwrap();
    assert!(complete_file_query(&q, || false).is_err());
    assert_eq!(s.input(), "/attach ../");
    s.set_input("/attach ");
    let q = s.file_completion_query().unwrap();
    assert!(complete_file_query(&q, || true).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.path(), root.path().join("linked")).unwrap();
        s.set_input("/attach linked/");
        assert!(complete_file_query(&s.file_completion_query().unwrap(), || false).is_err());
    }
}

fn model_agent(root: &Path, wire: zenpi::backend::OpenAiWireApi) -> zenpi::core::Agent {
    let backend = zenpi::backend::OpenAiCompatibleBackend::new_with_settings(
        "http://127.0.0.1:9/v1",
        None,
        "gpt-4.1",
        wire,
        None,
        None,
    )
    .unwrap()
    .with_model_registry(
        "openai".into(),
        zenpi::providers::registry::ModelRegistry::default(),
    )
    .unwrap();
    zenpi::core::Agent::new(
        zenpi::session::SessionStore::open_in_workspace(root.join("model.jsonl"), root).unwrap(),
        Box::new(backend),
    )
}
#[test]
fn actual_model_registry_and_effort_setter_drive_project_menu() {
    use zenpi::{
        backend::OpenAiWireApi,
        tui::{dispatch_reasoning_input, dispatch_slash_command},
    };
    let root = tempdir().unwrap();
    let mut agent = model_agent(root.path(), OpenAiWireApi::Responses);
    let mut s = state(root.path());
    s.update_model_catalog(&agent).unwrap();
    s.set_input("/model gpt-5");
    assert!(s.slash_choices().contains(&"/model gpt-5.2".into()));
    assert!(render(&mut s).contains("context 400000"));
    dispatch_slash_command(
        zenpi::slash::SlashCommand::Model {
            name: Some("gpt-5.2".into()),
        },
        &mut s,
        Some(&mut agent),
    );
    assert_eq!(agent.snapshot().model.as_deref(), Some("gpt-5.2"));
    assert!(dispatch_reasoning_input(
        &mut s,
        Some(&mut agent),
        "/reasoning"
    ));
    let choices = s.slash_choices();
    assert!(choices.contains(&"/reasoning high".into()));
    assert!(choices.contains(&"/reasoning none".into()));
    assert!(choices.contains(&"/reasoning default".into()));
    assert!(!choices.contains(&"/reasoning ultra".into()));
    assert!(render(&mut s).contains("default omits the request field"));
    dispatch_reasoning_input(&mut s, Some(&mut agent), "/reasoning high");
    assert_eq!(agent.reasoning_effort(), Some("high"));
    let before = fs::read(agent.session().path()).unwrap();
    dispatch_reasoning_input(&mut s, Some(&mut agent), "/reasoning ultra");
    assert_eq!(s.input(), "/reasoning ultra");
    assert_eq!(agent.reasoning_effort(), Some("high"));
    assert_eq!(fs::read(agent.session().path()).unwrap(), before);
    s.set_busy(true);
    dispatch_reasoning_input(&mut s, Some(&mut agent), "/reasoning low");
    assert_eq!(agent.reasoning_effort(), Some("high"));
    assert_eq!(s.input(), "/reasoning low");
    s.set_busy(false);
    s.open_project_tab("other");
    s.set_input("/model ");
    assert!(s.slash_choices().is_empty());
    s.select_project_tab(0);
    s.set_input("/reasoning ");
    assert!(s.slash_choices().contains(&"/reasoning high".into()));
    dispatch_reasoning_input(&mut s, Some(&mut agent), "/reasoning default");
    assert_eq!(agent.reasoning_effort(), None);
    dispatch_slash_command(
        zenpi::slash::SlashCommand::Model {
            name: Some("gpt-4.1".into()),
        },
        &mut s,
        Some(&mut agent),
    );
    s.set_input("/reasoning ");
    assert_eq!(s.slash_choices(), vec!["/reasoning default"]);
}

#[test]
fn model_catalog_arrival_requests_a_paint_without_idle_queue_updates() {
    let root = tempdir().unwrap();
    let agent = model_agent(root.path(), zenpi::backend::OpenAiWireApi::Responses);
    let mut s = state(root.path());
    s.set_input("/reasoning ");
    assert!(s.slash_choices().is_empty());
    s.take_dirty();
    s.update_model_catalog(&agent).unwrap();
    assert!(
        s.take_dirty(),
        "new model choices must become visible while idle"
    );
    assert!(render(&mut s).contains("/reasoning default"));
    s.update_model_catalog(&agent).unwrap();
    assert!(
        !s.take_dirty(),
        "identical catalog must not continuously repaint"
    );
    s.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(s.input(), "/reasoning default ");
}
#[test]
fn reasoning_menu_intersects_wire_capability_and_registry() {
    use zenpi::{backend::OpenAiWireApi, tui::dispatch_reasoning_input};
    let root = tempdir().unwrap();
    let mut agent = model_agent(root.path(), OpenAiWireApi::ChatCompletions);
    agent.set_model(Some("gpt-5.2".into())).unwrap();
    let mut s = state(root.path());
    s.update_model_catalog(&agent).unwrap();
    s.set_input("/reasoning ");
    assert_eq!(s.slash_choices(), vec!["/reasoning default"]);
    dispatch_reasoning_input(&mut s, Some(&mut agent), "/reasoning high");
    assert_eq!(agent.reasoning_effort(), None);
    assert_eq!(s.input(), "/reasoning high");
    dispatch_reasoning_input(&mut s, Some(&mut agent), "/reasoning high extra");
    assert_eq!(s.input(), "/reasoning high extra");
}

#[test]
fn completion_result_and_path_limits_remain_bounded() {
    let root = tempdir().unwrap();
    for index in 0..200 {
        fs::write(root.path().join(format!("file-{index:03}.txt")), "").unwrap();
    }
    let mut s = state(root.path());
    s.set_input("/attach file-");
    let query = s.file_completion_query().unwrap();
    let entries = complete_file_query(&query, || false).unwrap();
    assert_eq!(entries.entries.len(), 128);
    assert!(s.apply_file_completion(query, entries));
    assert_eq!(s.slash_choices().len(), 128);
    s.set_input(format!("/attach {}", "a".repeat(4097)));
    assert!(complete_file_query(&s.file_completion_query().unwrap(), || false).is_err());
    assert_eq!(s.input().len(), 4105);
}

#[test]
fn tab_selects_the_visible_highlight_without_guessing_an_unpainted_menu() {
    let mut s = TuiState::default();
    s.set_input("/m");
    let choices = s.slash_choices();
    assert!(choices.len() > 1);
    s.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(s.input(), "/m");
    let screen = render(&mut s);
    assert!(screen.contains("Commands / files"));
    s.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(s.input(), format!("{} ", choices[0]));
    s.set_input("/m");
    s.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(
        s.input(),
        "/m",
        "old painted rows must not choose a new draft"
    );
}

#[test]
fn idle_queue_refresh_preserves_the_painted_keyboard_selection() {
    use zenpi::input_queue::{InputQueueReply, QueueMode};
    let mut s = TuiState::default();
    s.set_input("/persona ");
    render(&mut s);
    s.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    render(&mut s);
    for _ in 0..20 {
        s.update_input_queue(&InputQueueReply::Page {
            inputs: vec![],
            next_sequence: None,
        });
        s.update_input_queue(&InputQueueReply::Modes {
            steer: QueueMode::All,
            follow_up: QueueMode::All,
        });
    }
    // Tab must still choose the row the user actually saw, even before another paint.
    s.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(s.input(), "/persona INTP ");
}

#[test]
fn queue_changes_keep_unrelated_selection_and_require_paint_for_changed_choices() {
    use zenpi::input_queue::{InputKind, InputQueueReply, InputStatus, QueuedInput};
    let mut s = TuiState::default();
    s.set_input("/persona ");
    render(&mut s);
    s.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    render(&mut s);
    let input = QueuedInput {
        id: "pending-1".into(),
        kind: InputKind::Steer,
        text: "original".into(),
        sequence: 1,
        revision: 1,
        status: InputStatus::Received,
        applied_turn_id: None,
        applied_parent_id: None,
        submitted_sha256: "fixture".into(),
    };
    s.update_input_queue(&InputQueueReply::Input {
        input: input.clone(),
        duplicate: false,
    });
    s.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(s.input(), "/persona INTP ");

    s.set_input("/input ");
    let choices = s.slash_choices();
    let selected = choices
        .iter()
        .position(|choice| choice == "/input cancel pending-1")
        .unwrap();
    for _ in 0..selected {
        s.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    render(&mut s);
    let mut second = input.clone();
    second.id = "pending-2".into();
    second.sequence = 2;
    s.update_input_queue(&InputQueueReply::Page {
        inputs: vec![second, input.clone()],
        next_sequence: None,
    });
    s.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(
        s.input(),
        "/input ",
        "changed menu must be painted before Tab"
    );
    render(&mut s);
    // An identical per-input retry must not reorder the queue or erase the paint.
    s.update_input_queue(&InputQueueReply::Input {
        input,
        duplicate: true,
    });
    s.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(s.input(), "/input cancel pending-1 ");
}

#[test]
fn session_search_and_selection_survive_idle_owner_refresh() {
    use zenpi::{
        core::{Turn, TurnRole},
        session::SessionStore,
    };
    let root = tempdir().unwrap();
    let owner = SessionStore::open(root.path().join("owner.jsonl")).unwrap();
    for name in ["a", "b", "c"] {
        let mut store = SessionStore::open(root.path().join(format!("{name}.jsonl"))).unwrap();
        store
            .append_turn(Turn::new("u", TurnRole::User, "search-needle"))
            .unwrap();
    }
    let mut s = TuiState::default();
    s.refresh_session_snapshot(&owner);
    assert_eq!(
        s.search_session_browser(&owner, "search-needle").unwrap(),
        3
    );
    s.move_session_browser_cursor(1);
    let selected = s.selected_session_browser_path().unwrap().to_owned();
    for _ in 0..20 {
        s.refresh_session_snapshot(&owner);
    }
    assert_eq!(s.session_browser_cursor(), 1);
    assert_eq!(s.selected_session_browser_path(), Some(selected.as_str()));
    assert_eq!(
        s.search_session_browser(&owner, "no-matching-turn")
            .unwrap(),
        0
    );
    s.refresh_session_snapshot(&owner);
    assert_eq!(s.selected_session_browser_path(), None);
}

#[test]
fn selected_source_after_sixteenth_resource_is_complete_without_body_or_draft_change() {
    use zenpi::tui::TuiAction;
    let root = tempdir().unwrap();
    let long = root
        .path()
        .join(format!("来源 {}", "a".repeat(180)))
        .join(format!("工作 space {}", "b".repeat(180)))
        .join(format!("e\u{301} {}", "c".repeat(180)));
    for index in 0..20 {
        let skill = long.join(format!("skills/zz-extra-{index:02}"));
        fs::create_dir_all(&skill).unwrap();
        fs::create_dir_all(long.join("prompts")).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            format!(
                "---\nname: zz-extra-{index:02}\ndescription: Extra skill\n---\nSECRET_EXTRA_BODY\n"
            ),
        )
        .unwrap();
        fs::write(
            long.join(format!("prompts/zz-extra-{index:02}.md")),
            "---\ndescription: Extra template\n---\nSECRET_TEMPLATE_BODY\n",
        )
        .unwrap();
    }
    let resources = snapshot(&long);
    assert!(
        resources
            .skills
            .metadata()
            .position(|m| m.name == "zz-extra-19")
            .unwrap()
            >= 16
    );
    assert!(
        resources
            .templates
            .templates()
            .position(|m| m.name == "zz-extra-19")
            .unwrap()
            >= 16
    );
    let mut s = state(&long);
    s.update_resource_catalog(&resources);
    for (command, path) in [
        (
            "/skill:zz-extra-19",
            long.join("skills/zz-extra-19/SKILL.md"),
        ),
        ("/zz-extra-19", long.join("prompts/zz-extra-19.md")),
    ] {
        s.set_input(command);
        let before = s.project_checkpoint();
        assert!(render(&mut s).contains("F2 source"));
        let TuiAction::InspectResource {
            command: selected,
            source_hash,
        } = s.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE))
        else {
            panic!("F2 must inspect the selected actual resource")
        };
        assert_eq!(selected, command);
        s.inspect_resource_source(&resources, &selected, &source_hash)
            .unwrap();
        let screen = render(&mut s);
        assert!(screen.contains("Resource source"));
        assert!(!screen.contains("SECRET_"));
        let TuiAction::Copy(text) = s.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("inspector exposes complete source for copying")
        };
        assert!(text.contains(&serde_json::to_string(&path.canonicalize().unwrap()).unwrap()));
        assert!(text.contains(&source_hash));
        assert!(!text.contains("SECRET_"));
        assert_eq!(s.project_checkpoint(), before);
        s.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(s.input(), command);
        assert!(matches!(
            s.route_catalog_input(s.input()),
            Ok(InputRoute::Prompt(_))
        ));
    }
    s.set_input("/model");
    assert!(!matches!(
        s.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE)),
        TuiAction::InspectResource { .. }
    ));
}

#[test]
fn source_inspection_rejects_stale_or_missing_owner_metadata() {
    let root = tempdir().unwrap();
    let first = snapshot(root.path());
    let mut s = state(root.path());
    s.update_resource_catalog(&first);
    s.set_input("/skill:example");
    let hash = first.skills.metadata().next().unwrap().source_hash.clone();
    let mut changed = (*first).clone();
    changed.skills = Default::default();
    assert!(
        s.inspect_resource_source(&changed, "/skill:example", &hash)
            .unwrap_err()
            .contains("no longer exists")
    );
    assert_eq!(s.input(), "/skill:example");
    assert!(s.slash_choices().is_empty());
    assert!(
        s.inspect_resource_source(&first, "/skill:example", "stale-hash")
            .unwrap_err()
            .contains("Resource changed")
    );
    assert_eq!(s.slash_choices(), vec!["/skill:example"]);
    assert!(!render(&mut s).contains("Inspect / copy transcript blocks"));
}

#[test]
fn file_completion_partial_menu_keeps_candidates_in_short_terminals() {
    let root = tempdir().unwrap();
    for index in 0..200 {
        fs::write(root.path().join(format!("candidate-{index:03}")), "").unwrap();
    }
    let mut s = state(root.path());
    for height in [9, 10, 11] {
        s.set_input("/attach candidate-");
        let query = s.file_completion_query().unwrap();
        s.apply_file_completion(
            query.clone(),
            complete_file_query(&query, || false).unwrap(),
        );
        let mut terminal = Terminal::new(TestBackend::new(140, height)).unwrap();
        terminal
            .draw(|frame| s.render_bentobox(frame, "zenpi"))
            .unwrap();
        let screen = (0..height)
            .map(|y| {
                (0..140)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            screen.contains("/attach \"candidate-000\""),
            "height {height}: {screen}"
        );
        assert!(screen.contains("partial"), "height {height}: {screen}");
        s.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(s.input(), "/attach \"candidate-000\" ", "height {height}");
    }
}
