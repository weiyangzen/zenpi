use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, style::Style};
use zenpi::{
    approval::ApprovalRequest,
    render::render_plain_prefixed,
    tools::{ToolOrigin, ToolPreview, ToolSideEffect},
    tui::{MessageRole, TuiAction, TuiState},
};

fn screen(state: &mut TuiState, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn transcript_inspector_keeps_character_after_wide_emoji_visible_and_copy_exact() {
    let mut state = TuiState::default();
    let text = "ab❤️X";
    state.push_message(MessageRole::Assistant, text);
    state.set_input("draft survives");
    state.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT));
    // The inspector body is four cells wide. The heart occupies two cells,
    // so X must wrap to the following row instead of being clipped by Paragraph.
    let rendered = screen(&mut state, 10, 12);
    eprintln!("inspector narrow frame:\n{rendered}");
    let copy = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(copy, TuiAction::Copy(text.into()));
    assert_eq!(state.input(), "draft survives");
    assert!(
        rendered.contains('X'),
        "last character disappeared:\n{rendered}"
    );
    assert!(rendered.contains("❤️"));
}

#[test]
fn approval_preview_keeps_character_after_wide_emoji_visible_without_allowing() {
    let mut state = TuiState::default();
    state.set_input("approval draft");
    state.present_approval(ApprovalRequest {
        request_id: "review".into(),
        turn_id: "turn".into(),
        call_id: "call".into(),
        tool: "write_file".into(),
        side_effect: ToolSideEffect::WorkspaceWrite,
        arguments: serde_json::json!({"path": "file.txt"}),
        preview: Some(ToolPreview::Diff {
            path: "file.txt".into(),
            patch: "ab❤️X".into(),
            changed: true,
            truncated: false,
            before_bytes: 0,
            after_bytes: 9,
            source_sha256: "a".repeat(64),
        }),
        origin: ToolOrigin::AgentTool,
        policy_digest: None,
        lease_id: None,
    });
    state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    // Approval body is four cells wide; End scrolls to the actual preview.
    let rendered = screen(&mut state, 8, 20);
    eprintln!("approval narrow frame:\n{rendered}");
    assert_eq!(state.approval_count(), 1);
    assert_eq!(state.input(), "approval draft");
    // A bare Enter decides in one press, and denial is the default selection.
    // What this test is about — the wide emoji staying visible while the draft
    // survives — is asserted on the frame captured above.
    assert!(matches!(
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        TuiAction::RespondApproval { allow: false, .. }
    ));
    assert!(
        rendered.contains('X'),
        "last character disappeared:\n{rendered}"
    );
    assert!(rendered.contains("❤️"));
}

#[test]
fn plain_preview_wraps_indivisible_graphemes_with_cell_exact_rows() {
    for emoji in ["❤️", "👨‍👩‍👧‍👦", "🇨🇳"] {
        let input = format!("ab{emoji}X");
        let lines = render_plain_prefixed("", &input, 4, Style::default());
        let text: Vec<String> = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect();
        assert_eq!(text, [format!("ab{emoji}"), "X".into()], "{emoji}");
        assert!(lines.iter().all(|line| line.width() <= 4));
    }
}
