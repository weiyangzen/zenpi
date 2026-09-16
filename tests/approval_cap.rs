use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use zenpi::{
    approval::ApprovalRequest,
    tools::{ToolCall, ToolContext, ToolOrigin, ToolPreview, ToolRegistry, ToolSideEffect},
    tui::{TuiAction, TuiState},
};

fn request(content: String, id: &str) -> ApprovalRequest {
    request_with_source(content, id, None)
}
fn request_with_source(content: String, id: &str, before: Option<&str>) -> ApprovalRequest {
    let root = tempfile::tempdir().unwrap();
    if let Some(before) = before {
        std::fs::write(root.path().join("review.txt"), before).unwrap();
    }
    let context = ToolContext::new(root.path()).unwrap();
    let call = ToolCall {
        id: format!("call-{id}"),
        name: "write_file".into(),
        arguments: serde_json::json!({"path":"review.txt", "content":content}),
    };
    let preview = ToolRegistry::with_all_builtins()
        .unwrap()
        .approval_preview(&context, &call)
        .unwrap()
        .unwrap();
    assert!(
        if let Some(before) = before {
            std::fs::read_to_string(root.path().join("review.txt")).unwrap() == before
        } else {
            !root.path().join("review.txt").exists()
        },
        "preview must not execute write"
    );
    let request = ApprovalRequest {
        request_id: id.into(),
        turn_id: "turn-cap".into(),
        call_id: call.id,
        tool: call.name,
        arguments: call.arguments,
        preview: Some(preview),
        side_effect: ToolSideEffect::WorkspaceWrite,
        origin: ToolOrigin::AgentTool,
        policy_digest: None,
        lease_id: None,
    };
    request.validate().unwrap();
    request
}
fn key(state: &mut TuiState, code: KeyCode) -> TuiAction {
    state.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn frame(state: &mut TuiState, width: u16, height: u16, bento: bool, label: &str) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| {
            if bento {
                state.render_bentobox(f, "zenpi");
            } else {
                state.render(f, "zenpi");
            }
        })
        .unwrap();
    let screen = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    println!("SCREEN {label} width={width} height={height} bento={bento}\n{screen}\nEND_SCREEN");
    // Only modal body cells: no outer transcript, border or approval footer.
    (2..height.saturating_sub(4))
        .map(|y| {
            (2..width.saturating_sub(2))
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<String>()
}
fn complete_preview(request: &ApprovalRequest) {
    let ToolPreview::Diff {
        truncated,
        patch,
        after_bytes,
        ..
    } = request.preview.as_ref().unwrap();
    println!(
        "OWNER truncated={truncated} patch_bytes={} after_bytes={after_bytes}",
        patch.len()
    );
    assert!(!truncated, "fixture must be a real complete owner preview");
    assert!(patch.contains("CAP_END"));
}
fn check_large_end(bento: bool) {
    let request = request(format!("{}\nCAP_END\n", "A".repeat(40_000)), "large");
    complete_preview(&request);
    let mut state = TuiState::default();
    state.present_approval(request);
    key(&mut state, KeyCode::End);
    let body = frame(&mut state, 8, 24, bento, "large-end");
    assert!(
        body.contains("CAP_END"),
        "End must reach the actual complete diff tail; body={body:?}"
    );
}
#[test]
fn complete_owner_diff_end_is_reachable_classic() {
    check_large_end(false);
}
#[test]
fn complete_owner_diff_end_is_reachable_bento() {
    check_large_end(true);
}
#[test]
fn file_diff_is_not_buried_below_repeated_content_arguments() {
    let request = request(format!("{}\nCAP_END\n", "A".repeat(40_000)), "head");
    complete_preview(&request);
    let mut state = TuiState::default();
    state.present_approval(request);
    let body = frame(&mut state, 84, 30, false, "normal-head");
    assert!(
        body.contains("review.txt"),
        "path must be visible at the top"
    );
    assert!(
        body.contains("Proposed change:"),
        "diff header must be reachable in initial normal viewport; body={body:?}"
    );
}
#[test]
fn expanded_tabs_can_scroll_past_u16_rows_without_losing_tail() {
    let request = request(format!("{}\nCAP_END\n", "\t".repeat(18_000)), "tabs");
    complete_preview(&request);
    let mut state = TuiState::default();
    state.present_approval(request);
    key(&mut state, KeyCode::End);
    let body = frame(&mut state, 5, 30, false, "tabs-end");
    assert!(
        body.contains("CAP_END"),
        "End must pass 72000 tab cells; body={body:?}"
    );
}
#[test]
fn real_owner_truncation_is_visible_before_allowing() {
    let old = "Z".repeat(80_000);
    let request = request_with_source("replacement\n".into(), "truncated", Some(&old));
    assert!(matches!(
        request.preview,
        Some(ToolPreview::Diff {
            truncated: true,
            ..
        })
    ));
    let mut state = TuiState::default();
    state.present_approval(request);
    let body = frame(&mut state, 84, 30, false, "owner-truncated-head");
    assert!(
        body.contains("Preview truncated"),
        "owner truncation must be visible without scrolling through raw arguments"
    );
    assert!(matches!(
        key(&mut state, KeyCode::Enter),
        TuiAction::RespondApproval { allow: false, .. }
    ));
}
#[test]
fn approval_navigation_preserves_draft_defaults_and_request_identity() {
    let mut state = TuiState::default();
    state.set_input("draft kept");
    let project = state.active_project().to_owned();
    state.present_approval(request("first\n".into(), "one"));
    state.present_approval(request("second\n".into(), "two"));
    key(&mut state, KeyCode::End);
    key(&mut state, KeyCode::Char('y'));
    key(&mut state, KeyCode::Tab);
    frame(&mut state, 84, 30, true, "second-home");
    match key(&mut state, KeyCode::Enter) {
        TuiAction::RespondApproval {
            project: p,
            request_id,
            turn_id,
            call_id,
            allow,
            remember,
        } => {
            assert_eq!(p, project);
            assert_eq!(request_id, "two");
            assert_eq!(turn_id, "turn-cap");
            assert_eq!(call_id, "call-two");
            assert!(!allow);
            assert!(!remember);
        }
        other => panic!("unexpected action {other:?}"),
    }
    key(&mut state, KeyCode::Esc);
    assert_eq!(state.input(), "draft kept");
    assert_eq!(state.approval_count(), 2);
}
#[test]
fn approval_graphemes_do_not_clip_following_text() {
    let request = request("\u{2764}\u{fe0f}X\nCAP_END\n".into(), "unicode");
    complete_preview(&request);
    for bento in [false, true] {
        let mut state = TuiState::default();
        state.present_approval(request.clone());
        key(&mut state, KeyCode::End);
        let body = frame(&mut state, 8, 24, bento, "unicode-end");
        assert!(body.contains("X"));
        assert!(body.contains("CAP_END"));
    }
}

#[test]
fn window_keeps_middle_rows_and_final_tabs_without_retaining_the_whole_document() {
    let input = format!("{}MID\n{}END", "a\n".repeat(9_000), "\t".repeat(18_000));
    let middle = zenpi::render::render_plain_window(&input, 1, 9_000, 3);
    let text = |lines: &[ratatui::text::Line<'_>]| {
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .map(|span| span.content.as_ref())
            .collect::<String>()
    };
    assert_eq!(text(&middle.lines), "MID");
    assert_eq!(middle.lines.len(), 3);
    let end = zenpi::render::render_plain_window(&input, 1, usize::MAX, 4);
    assert!(end.offset > u16::MAX as usize);
    assert!(text(&end.lines).ends_with("END"));
    assert!(end.lines.len() <= 4);
    assert!(!end.input_truncated);
}

#[test]
fn window_reports_input_cap_and_uses_existing_grapheme_rules() {
    let input = "\u{2764}\u{fe0f}X\n\u{1b}end\tCAP_END";
    for width in [1, 2, 4, 80] {
        let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
        let window = zenpi::render::render_plain_window(input, width as usize, 0, 40);
        assert!(!window.input_truncated);
        assert!(
            window
                .lines
                .iter()
                .all(|line| line.width() <= width as usize)
        );
        terminal
            .draw(|frame| {
                frame.render_widget(ratatui::widgets::Paragraph::new(window.lines), frame.area())
            })
            .unwrap();
        let drawn: String = (0..40)
            .map(|y| {
                (0..width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect();
        assert!(drawn.contains("X"));
        assert!(drawn.contains("?end"));
        assert!(drawn.contains("CAP_END"));
    }
    let oversized = "x".repeat(zenpi::render::MAX_MARKDOWN_BYTES + 1);
    let capped = zenpi::render::render_plain_window(&oversized, 1, usize::MAX, 5);
    assert!(capped.input_truncated);
    assert!(capped.lines.len() <= 5);
    let empty = zenpi::render::render_plain_window("text", 0, usize::MAX, 0);
    assert!(empty.lines.is_empty());
}
