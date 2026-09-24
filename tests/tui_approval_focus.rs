use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use zenpi::approval::{ApprovalCoordinator, ApprovalPolicy, ApprovalRequest};
use zenpi::tools::{ToolOrigin, ToolPreview, ToolSideEffect};
use zenpi::tui::{TuiAction, TuiState, respond_tui_approval};
fn request(id: &str) -> ApprovalRequest {
    ApprovalRequest {
        request_id: id.into(),
        turn_id: "turn".into(),
        call_id: format!("call-{id}"),
        tool: "write_file".into(),
        side_effect: ToolSideEffect::WorkspaceWrite,
        arguments: serde_json::json!({"path":"approved.txt","content":"approved"}),
        preview: Some(ToolPreview::Diff {
            path: "approved.txt".into(),
            patch: (0..150)
                .map(|i| format!("+ actual proposed line {i}\n"))
                .collect(),
            changed: true,
            truncated: false,
            before_bytes: 0,
            after_bytes: 8,
            source_sha256: "a".repeat(64),
        }),
        origin: ToolOrigin::AgentTool,
        policy_digest: None,
        lease_id: None,
    }
}
fn key(s: &mut TuiState, c: KeyCode, m: KeyModifiers) -> TuiAction {
    s.handle_key(KeyEvent::new(c, m))
}
fn plain(s: &mut TuiState, c: KeyCode) -> TuiAction {
    key(s, c, KeyModifiers::NONE)
}
#[test]
fn deny_default_and_explicit_allow_keep_draft_cursor() {
    let mut s = TuiState::default();
    s.set_input("unfinished draft");
    plain(&mut s, KeyCode::Left);
    let cursor = s.cursor();
    s.present_approval(request("a"));
    // A bare Enter decides in one press: denial is the default selection.
    assert!(matches!(
        plain(&mut s, KeyCode::Enter),
        TuiAction::RespondApproval {
            allow: false,
            remember: false,
            ..
        }
    ));
    plain(&mut s, KeyCode::Char('y'));
    assert!(matches!(
        plain(&mut s, KeyCode::Enter),
        TuiAction::RespondApproval { allow: true, .. }
    ));
    assert_eq!(s.input(), "unfinished draft");
    assert_eq!(s.cursor(), cursor);
    s.retire_approval("a");
    assert_eq!(s.approval_count(), 0);
    assert_eq!(s.cursor(), cursor);
}
#[test]
fn escape_and_paste_never_grant_or_discard_draft() {
    let mut s = TuiState::default();
    s.set_input("draft ");
    s.present_approval(request("a"));
    assert!(matches!(
        key(&mut s, KeyCode::Char('y'), KeyModifiers::CONTROL),
        TuiAction::None
    ));
    // Leaving a note for the model is still reachable, just explicit (`n`).
    plain(&mut s, KeyCode::Char('n'));
    assert!(matches!(
        plain(&mut s, KeyCode::Enter),
        TuiAction::RespondApproval { allow: false, .. }
    ));
    s.handle_event(Event::Paste("y\n/approve a always".into()));
    assert_eq!(s.input(), "draft y\n/approve a always");
    plain(&mut s, KeyCode::Esc);
    assert_eq!(s.approval_count(), 1);
    key(&mut s, KeyCode::Char('a'), KeyModifiers::ALT);
    assert!(matches!(
        key(&mut s, KeyCode::Char('c'), KeyModifiers::CONTROL),
        TuiAction::Interrupt
    ));
}
#[test]
fn multiple_requests_reset_selection_and_projects_do_not_share_focus() {
    let mut s = TuiState::default();
    s.present_approval(request("a"));
    s.present_approval(request("b"));
    plain(&mut s, KeyCode::Char('y'));
    plain(&mut s, KeyCode::Tab);
    assert!(
        matches!(plain(&mut s,KeyCode::Enter),TuiAction::RespondApproval{request_id,allow:false,..} if request_id=="b")
    );
    s.open_project_tab("other");
    assert_eq!(s.approval_count(), 0);
    assert!(!matches!(
        plain(&mut s, KeyCode::Enter),
        TuiAction::RespondApproval { .. }
    ));
    s.select_project_tab(0);
    assert_eq!(s.approval_count(), 2);
    s.retire_approval("b");
    assert!(
        matches!(plain(&mut s,KeyCode::Enter),TuiAction::RespondApproval{request_id,allow:false,..} if request_id=="a")
    );
}

#[test]
fn approval_view_keeps_same_request_id_when_turn_or_call_differs() {
    let first = request("reused");
    let mut second = request("reused");
    second.turn_id = "turn-2".into();
    second.call_id = "call-2".into();
    let mut s = TuiState::default();
    s.present_approval(first.clone());
    s.present_approval(second);
    assert_eq!(s.approval_count(), 2);
}

#[test]
fn background_approvals_are_visible_even_when_the_source_tab_is_offscreen() {
    use ratatui::{Terminal, backend::TestBackend};
    let mut state = TuiState::default();
    let source = state.project_label(0);
    state.present_approval(request("background-a"));
    state.present_approval(request("background-b"));
    // Dismiss only the modal: the real requests must remain pending.
    plain(&mut state, KeyCode::Esc);
    for name in ["second", "third", "fourth"] {
        state.open_project_tab(name);
    }
    state.set_input("active draft 中文");
    let label = state.project_label(0);
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    let render = |state: &mut TuiState, terminal: &mut Terminal<TestBackend>, bentobox: bool| {
        terminal
            .draw(|frame| {
                if bentobox {
                    state.render_bentobox(frame, "test")
                } else {
                    state.render(frame, "test")
                }
            })
            .unwrap();
        let b = terminal.backend().buffer();
        (0..b.area.height)
            .map(|y| {
                (0..b.area.width)
                    .map(|x| b[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
    };
    for bentobox in [true, false] {
        let lines = render(&mut state, &mut terminal, bentobox);
        assert!(
            lines
                .last()
                .unwrap()
                .contains(&format!("Approval waiting: {source} (2)")),
            "{lines:?}"
        );
        assert!(lines.last().unwrap().contains("Alt-A review"));
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("Approval required 1/2"))
        );
        assert_eq!(state.approval_count(), 0);
        assert_eq!(state.input(), "active draft 中文");
    }
    assert_eq!(
        state.project_label(0),
        label,
        "attention must not rename the project"
    );
    state.select_project_tab(0);
    let lines = render(&mut state, &mut terminal, true);
    assert!(lines[0].contains("!2"));
    state.retire_approval("background-a");
    assert!(render(&mut state, &mut terminal, true)[0].contains("!1"));
    state.retire_approval("background-b");
    assert!(!render(&mut state, &mut terminal, true)[0].contains("!1"));
    state.select_project_tab(3);
    assert!(
        !render(&mut state, &mut terminal, true)
            .last()
            .unwrap()
            .contains("Approval waiting:")
    );
    assert_eq!(state.input(), "active draft 中文");
}
#[test]
fn worker_never_offers_remember_and_release_cannot_confirm() {
    let mut s = TuiState::default();
    let mut r = request("worker");
    r.origin = ToolOrigin::BlueprintWorker;
    r.policy_digest = Some("a".repeat(64));
    r.lease_id = Some("lease".into());
    s.present_approval(r);
    plain(&mut s, KeyCode::Char('y'));
    plain(&mut s, KeyCode::Char('r'));
    assert!(matches!(
        plain(&mut s, KeyCode::Enter),
        TuiAction::RespondApproval {
            remember: false,
            ..
        }
    ));
    let mut k = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    k.kind = crossterm::event::KeyEventKind::Release;
    assert_eq!(s.handle_key(k), TuiAction::None);
}
#[test]
fn full_diff_scroll_is_independent_from_folded_transcript_and_resize() {
    use ratatui::{Terminal, backend::TestBackend};
    let mut s = TuiState::default();
    s.present_approval(request("review-exact"));
    let mut t = Terminal::new(TestBackend::new(100, 30)).unwrap();
    let render = |s: &mut TuiState, t: &mut Terminal<TestBackend>| {
        t.draw(|f| s.render_bentobox(f, "test")).unwrap();
        t.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
    };
    assert!(render(&mut s, &mut t).contains("review-exact"));
    plain(&mut s, KeyCode::End);
    assert!(render(&mut s, &mut t).contains("actual proposed line 149"));
    for (w, h) in [(1, 1), (4, 5), (20, 10), (100, 30)] {
        t.backend_mut().resize(w, h);
        t.resize(ratatui::layout::Rect::new(0, 0, w, h)).unwrap();
        render(&mut s, &mut t);
    }
}
#[test]
fn actual_coordinator_rejects_cross_project_wrong_call_and_duplicate() {
    let owner = ApprovalCoordinator::default();
    let thread_owner = owner.clone();
    let r = request("exact");
    let worker = std::thread::spawn(move || {
        thread_owner.request_response(r, &mut ApprovalPolicy::default(), &|| false)
    });
    for _ in 0..1000 {
        if owner.is_pending("exact") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let mut s = TuiState::default();
    s.present_approval(owner.drain_pending().pop().unwrap());
    let project = s.active_project().to_owned();
    assert!(
        respond_tui_approval(
            &mut s,
            &owner,
            Some("other"),
            (&project, "exact", "turn", "call-exact"),
            (true, false)
        )
        .is_err()
    );
    assert!(
        respond_tui_approval(
            &mut s,
            &owner,
            Some(&project),
            (&project, "exact", "turn", "wrong"),
            (true, false)
        )
        .is_err()
    );
    assert!(owner.is_pending("exact"));
    respond_tui_approval(
        &mut s,
        &owner,
        Some(&project),
        (&project, "exact", "turn", "call-exact"),
        (false, false),
    )
    .unwrap();
    assert_eq!(
        worker.join().unwrap().unwrap().decision,
        zenpi::approval::ApprovalDecision::Deny
    );
    assert!(
        respond_tui_approval(
            &mut s,
            &owner,
            Some(&project),
            (&project, "exact", "turn", "call-exact"),
            (true, false)
        )
        .is_err()
    );
}
#[test]
fn cancelled_owner_rejects_stale_visible_request() {
    let owner = ApprovalCoordinator::default();
    let thread_owner = owner.clone();
    let worker = std::thread::spawn(move || {
        thread_owner.request_response(request("expired"), &mut ApprovalPolicy::default(), &|| {
            false
        })
    });
    for _ in 0..1000 {
        if owner.is_pending("expired") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let mut s = TuiState::default();
    s.present_approval(owner.drain_pending().pop().unwrap());
    let project = s.active_project().to_owned();
    owner.emergency_cancel();
    assert!(
        respond_tui_approval(
            &mut s,
            &owner,
            Some(&project),
            (&project, "expired", "turn", "call-expired"),
            (true, false)
        )
        .is_err()
    );
    assert_eq!(
        worker.join().unwrap().unwrap().decision,
        zenpi::approval::ApprovalDecision::Deny
    );
}

#[test]
fn invalid_requests_and_view_capacity_are_bounded() {
    let mut s = TuiState::default();
    let mut invalid = request("invalid");
    invalid.call_id = "\n".into();
    s.present_approval(invalid);
    assert_eq!(s.approval_count(), 0);
    for i in 0..140 {
        s.present_approval(request(&format!("request-{i}")));
    }
    assert_eq!(s.approval_count(), 128);
    s.present_approval(request("request-0"));
    assert_eq!(s.approval_count(), 128);
}

#[test]
fn remembered_policy_reopens_only_its_session_and_preserves_configured_deny() {
    use zenpi::{
        approval::ApprovalDecision,
        core::Agent,
        session::SessionStore,
        tools::{SideEffectPolicy, ToolContext, ToolRegistry},
    };
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first.jsonl");
    let second = root.path().join("second.jsonl");
    let mut journal = SessionStore::open(&first).unwrap();
    journal.append_event(serde_json::json!({"type":"approval_resolved","request_id":"request","turn_id":"turn","call_id":"call","origin":"agent_tool","tool":"write_file","remember":true,"decision":"deny"})).unwrap();
    drop(journal);
    drop(SessionStore::open(&second).unwrap());
    let mut agent = Agent::with_echo(SessionStore::open(&first).unwrap());
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(root.path()).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    assert_eq!(
        agent.approval_policy().unwrap().per_tool.get("write_file"),
        Some(&ApprovalDecision::Deny)
    );
    let mut configured = agent.configured_approval_policy().unwrap();
    configured
        .per_tool
        .insert("run_command".into(), ApprovalDecision::Deny);
    agent.set_approval_policy(configured);
    agent.resume_session(&second).unwrap();
    assert!(
        !agent
            .approval_policy()
            .unwrap()
            .per_tool
            .contains_key("write_file")
    );
    assert_eq!(
        agent.approval_policy().unwrap().per_tool.get("run_command"),
        Some(&ApprovalDecision::Deny)
    );
    agent.resume_session(&first).unwrap();
    assert_eq!(
        agent.approval_policy().unwrap().per_tool.get("write_file"),
        Some(&ApprovalDecision::Deny)
    );
}
#[test]
fn replay_never_promotes_worker_or_malformed_records_or_overrides_configured_deny() {
    use zenpi::approval::ApprovalDecision;
    let mut policy = ApprovalPolicy::default();
    policy
        .per_tool
        .insert("run_command".into(), ApprovalDecision::Deny);
    let event = serde_json::json!({"type":"approval_resolved","request_id":"request","turn_id":"turn","call_id":"call","origin":"agent_tool","tool":"run_command","remember":true,"decision":"allow"});
    let mut worker = event.clone();
    worker["tool"] = serde_json::json!("write_file");
    worker["origin"] = serde_json::json!("blueprint_worker");
    let mut missing = event.clone();
    missing["tool"] = serde_json::json!("read_file");
    missing.as_object_mut().unwrap().remove("call_id");
    let restored = policy.with_remembered_events(&[event, worker, missing]);
    assert_eq!(restored.per_tool, policy.per_tool);
}

#[test]
fn only_a_host_answer_source_creates_a_standing_grant() {
    use zenpi::approval::ApprovalDecision;
    let policy = ApprovalPolicy::default();
    let base = serde_json::json!({"type":"approval_resolved","request_id":"request","turn_id":"turn","call_id":"call","origin":"agent_tool","tool":"write_file","remember":true,"decision":"allow"});
    // Records written before the field existed came from the host path, so a
    // missing source still restores.
    assert_eq!(
        policy
            .with_remembered_events(&[base.clone()])
            .per_tool
            .get("write_file"),
        Some(&ApprovalDecision::Allow)
    );
    let mut host = base.clone();
    host["source"] = serde_json::json!("host_answer");
    assert_eq!(
        policy
            .with_remembered_events(&[host])
            .per_tool
            .get("write_file"),
        Some(&ApprovalDecision::Allow)
    );
    // Any other source is a decision an agent reached on its own.  Recording
    // it under the same event type must not promote it into a standing grant.
    for source in [
        "policy_never",
        "policy_read_only",
        "policy_deny",
        "per_tool_grant",
        "worker_allow_after_preflight",
    ] {
        let mut impostor = base.clone();
        impostor["source"] = serde_json::json!(source);
        assert!(
            !policy
                .with_remembered_events(&[impostor])
                .per_tool
                .contains_key("write_file"),
            "{source} must not mint a standing grant"
        );
    }
}

#[test]
fn directory_picker_owns_keys_over_existing_or_new_approval_view() {
    let mut s = TuiState::default();
    s.present_approval(request("first"));
    s.open_directory_picker();
    assert!(s.directory_picker_open());
    plain(&mut s, KeyCode::Char('y'));
    assert!(!matches!(
        plain(&mut s, KeyCode::Enter),
        TuiAction::RespondApproval { .. }
    ));
    assert_eq!(s.approval_count(), 1);
    plain(&mut s, KeyCode::Esc);
    key(&mut s, KeyCode::Char('a'), KeyModifiers::ALT);
    assert!(matches!(
        plain(&mut s, KeyCode::Enter),
        TuiAction::RespondApproval { allow: false, .. }
    ));
    let mut other = TuiState::default();
    other.open_directory_picker();
    other.present_approval(request("later"));
    assert!(!matches!(
        plain(&mut other, KeyCode::Enter),
        TuiAction::RespondApproval { .. }
    ));
}

#[test]
fn remember_confirm_and_reject_feedback_are_explicit_steps() {
    let mut s = TuiState::default();
    s.present_approval(request("stage"));
    // Remember asks for a confirmation step; Esc returns to the selector.
    plain(&mut s, KeyCode::Char('r'));
    assert_eq!(plain(&mut s, KeyCode::Esc), TuiAction::Redraw);
    plain(&mut s, KeyCode::Char('r'));
    assert!(matches!(
        plain(&mut s, KeyCode::Enter),
        TuiAction::RespondApproval {
            allow: true,
            remember: true,
            ..
        }
    ));

    // Reject collects optional feedback for the model.
    s.present_approval(request("stage2"));
    plain(&mut s, KeyCode::Char('n'));
    for character in "use a safer path".chars() {
        plain(&mut s, KeyCode::Char(character));
    }
    assert!(matches!(
        plain(&mut s, KeyCode::Enter),
        TuiAction::RespondApproval { allow: false, .. }
    ));
    assert_eq!(
        s.approval_reject_message("stage2").as_deref(),
        Some("use a safer path")
    );
}
