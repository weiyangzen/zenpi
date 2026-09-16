use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;
use zenpi::render::{MarkdownBlock, parse_markdown, render_markdown, render_markdown_with_limit};

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

#[test]
fn identifiers_keep_literal_underscores_without_losing_emphasis() {
    for text in [
        "ZENPI_TUI_a522b536f7",
        "src/my_file_name.rs",
        "foo__bar__baz",
    ] {
        assert_eq!(
            render_markdown(text, 100)
                .iter()
                .map(line_text)
                .collect::<String>(),
            text
        );
    }
    assert_eq!(
        render_markdown("_italic_ __bold__", 100)
            .iter()
            .map(line_text)
            .collect::<String>(),
        "italic bold"
    );
}

#[test]
fn block_parser_preserves_code_and_separates_conversation_blocks() {
    let blocks = parse_markdown(
        "## Plan\n\nUse **small** steps.\n\n```rust\nfn main() {}\n```\n\n- ship it",
    );
    assert!(matches!(
        &blocks[0],
        MarkdownBlock::Heading { level: 2, .. }
    ));
    assert!(matches!(&blocks[1], MarkdownBlock::Paragraph(_)));
    assert!(
        matches!(&blocks[2], MarkdownBlock::Code { language: Some(language), text } if language == "rust" && text == "fn main() {}")
    );
    assert!(matches!(
        &blocks[3],
        MarkdownBlock::ListItem { ordered: false, .. }
    ));
}

#[test]
fn renderer_is_safe_for_narrow_terminal_cells() {
    let lines = render_markdown("# heading\n\n世界 **wide**", 1);
    assert!(!lines.is_empty());
    assert!(
        lines
            .iter()
            .all(|line| { UnicodeWidthStr::width(line_text(line).as_str()) <= 1 })
    );
}

#[test]
fn renderer_caps_output_without_dropping_the_first_line() {
    let lines = render_markdown_with_limit("one\n\n two\n\n three", 20, 2);
    assert_eq!(lines.len(), 2);
    assert_eq!(line_text(&lines[0]), "one");
}
