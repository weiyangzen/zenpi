//! Lightweight rendering for assistant text.
//!
//! The interactive TUI deliberately does not pull in a full Markdown parser or
//! syntax-highlighting stack.  This module covers the small, predictable
//! subset that is useful in a terminal conversation while keeping the output
//! bounded and safe for a narrow terminal.  Unsupported Markdown is rendered
//! as ordinary text rather than dropped or interpreted as terminal control
//! sequences.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Maximum input copied into the renderer.
pub const MAX_MARKDOWN_BYTES: usize = 256 * 1024;
/// Maximum number of output lines retained by [`render_markdown`].
pub const MAX_MARKDOWN_LINES: usize = 8_192;
/// Maximum approximate terminal cells retained for one rendered message. The
/// line cap alone would permit a pathological 65k-column test viewport to
/// allocate hundreds of megabytes of separators.
pub const MAX_MARKDOWN_CELLS: usize = 4 * 1024 * 1024;
/// Maximum viewport width accepted by the renderer. Ratatui terminal areas are
/// `u16` wide in practice; this extra bound prevents an accidental huge
/// allocation when the API is called outside a terminal.
pub const MAX_MARKDOWN_WIDTH: usize = u16::MAX as usize;

/// A deliberately small block-level Markdown model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownBlock {
    Paragraph(String),
    Heading {
        level: u8,
        text: String,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    Quote(String),
    ListItem {
        ordered: bool,
        marker: String,
        text: String,
    },
    Rule,
}

/// Parse the supported block subset.
///
/// The parser is intentionally loss-tolerant: an unclosed fence becomes a
/// code block at EOF, and unknown syntax remains paragraph text.  This is a
/// better failure mode for a conversation renderer than returning an error or
/// silently hiding a provider response.
pub fn parse_markdown(input: &str) -> Vec<MarkdownBlock> {
    let sanitized = sanitize_text(truncate_bytes(input, MAX_MARKDOWN_BYTES));
    let bounded = truncate_bytes(&sanitized, MAX_MARKDOWN_BYTES);
    let mut blocks = Vec::new();
    let mut paragraph = Vec::new();
    let mut code: Option<(char, usize, Option<String>, Vec<String>)> = None;

    for raw_line in bounded.lines() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if let Some((fence, fence_len, language, mut lines)) = code.take() {
            if let Some((close_fence, close_len, close_language)) = fence_info(line)
                && close_fence == fence
                && close_len >= fence_len
                && close_language.is_none()
            {
                blocks.push(MarkdownBlock::Code {
                    language,
                    text: lines.join("\n"),
                });
            } else {
                lines.push(line.to_owned());
                code = Some((fence, fence_len, language, lines));
            }
            continue;
        }

        if let Some((fence, fence_len, language)) = fence_info(line) {
            flush_paragraph(&mut blocks, &mut paragraph);
            code = Some((fence, fence_len, language, Vec::new()));
            continue;
        }

        if line.trim().is_empty() {
            flush_paragraph(&mut blocks, &mut paragraph);
            continue;
        }
        if let Some((level, text)) = heading_info(line) {
            flush_paragraph(&mut blocks, &mut paragraph);
            blocks.push(MarkdownBlock::Heading { level, text });
            continue;
        }
        if let Some(text) = quote_info(line) {
            flush_paragraph(&mut blocks, &mut paragraph);
            blocks.push(MarkdownBlock::Quote(text));
            continue;
        }
        if let Some((ordered, marker, text)) = list_info(line) {
            flush_paragraph(&mut blocks, &mut paragraph);
            blocks.push(MarkdownBlock::ListItem {
                ordered,
                marker,
                text,
            });
            continue;
        }
        if is_rule(line) {
            flush_paragraph(&mut blocks, &mut paragraph);
            blocks.push(MarkdownBlock::Rule);
            continue;
        }
        paragraph.push(line.to_owned());
    }

    if let Some((_fence, _fence_len, language, lines)) = code {
        blocks.push(MarkdownBlock::Code {
            language,
            text: lines.join("\n"),
        });
    }
    flush_paragraph(&mut blocks, &mut paragraph);
    blocks
}

/// Render supported Markdown to Ratatui lines constrained to `width` cells.
///
/// This function returns owned lines so callers can cache them between frames.
/// Every line is bounded by `width` (including a one-cell viewport), and the
/// result is capped at [`MAX_MARKDOWN_LINES`].
pub fn render_markdown(input: &str, width: usize) -> Vec<Line<'static>> {
    render_markdown_with_limit(input, width, MAX_MARKDOWN_LINES)
}

/// Render a message body with a fixed-width role/metadata prefix.
///
/// Conversation UIs commonly put `you: ` or `tool: ` on the first visual row
/// and indent wrapped rows beneath it.  Keeping that operation here avoids a
/// second, subtly different wrapping implementation in the TUI.  `prefix`
/// is cell-truncated when the viewport is narrow; at least one cell remains
/// available for body text.
pub fn render_markdown_prefixed(
    prefix: &str,
    input: &str,
    width: usize,
    prefix_style: Style,
) -> Vec<Line<'static>> {
    let width = width.clamp(1, MAX_MARKDOWN_WIDTH);
    let prefix_width = UnicodeWidthStr::width(prefix).min(width.saturating_sub(1));
    let prefix = truncate_to_width(prefix, width.saturating_sub(1));
    let continuation = " ".repeat(prefix_width);
    let body = render_markdown_with_limit(
        input,
        width.saturating_sub(prefix_width).max(1),
        bounded_line_limit(
            width.saturating_sub(prefix_width).max(1),
            MAX_MARKDOWN_LINES,
        ),
    );
    body.into_iter()
        .enumerate()
        .map(|(index, line)| {
            let mut spans = Vec::with_capacity(line.spans.len() + 1);
            spans.push(Span::styled(
                if index == 0 {
                    prefix.clone()
                } else {
                    continuation.clone()
                },
                prefix_style,
            ));
            spans.extend(line.spans);
            Line::from(spans)
        })
        .collect()
}

/// Render a message with a role prefix without interpreting Markdown syntax.
///
/// User-entered prompts, tool output, and error text are intentionally kept in
/// this path.  They still receive control-byte sanitization and cell-aware
/// wrapping, but a line beginning with `---` or `#` cannot unexpectedly change
/// its visual meaning.
pub fn render_plain_prefixed(
    prefix: &str,
    input: &str,
    width: usize,
    prefix_style: Style,
) -> Vec<Line<'static>> {
    let width = width.clamp(1, MAX_MARKDOWN_WIDTH);
    let prefix_width = UnicodeWidthStr::width(prefix).min(width.saturating_sub(1));
    let prefix = truncate_to_width(prefix, width.saturating_sub(1));
    let continuation = " ".repeat(prefix_width);
    let bounded = truncate_bytes(input, MAX_MARKDOWN_BYTES);
    let sanitized = sanitize_text(bounded);
    let body = wrap_segments(
        &[StyledSegment {
            text: sanitized,
            style: Style::default(),
        }],
        width.saturating_sub(prefix_width).max(1),
    );
    body.into_iter()
        .take(bounded_line_limit(
            width.saturating_sub(prefix_width).max(1),
            MAX_MARKDOWN_LINES,
        ))
        .enumerate()
        .map(|(index, line)| {
            let mut spans = Vec::with_capacity(line.spans.len() + 1);
            spans.push(Span::styled(
                if index == 0 {
                    prefix.clone()
                } else {
                    continuation.clone()
                },
                prefix_style,
            ));
            spans.extend(line.spans);
            Line::from(spans)
        })
        .collect()
}

/// Variant of [`render_markdown`] with an explicit output-line limit.
pub fn render_markdown_with_limit(
    input: &str,
    width: usize,
    max_lines: usize,
) -> Vec<Line<'static>> {
    let width = width.clamp(1, MAX_MARKDOWN_WIDTH);
    let max_lines = bounded_line_limit(width, max_lines);
    let blocks = parse_markdown(input);
    let mut output = Vec::new();

    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            push_line(&mut output, Line::default(), max_lines);
        }
        match block {
            MarkdownBlock::Paragraph(text) => {
                render_inline_text(text, Style::default(), width, &mut output, max_lines);
            }
            MarkdownBlock::Heading { level, text } => {
                let style = heading_style(*level);
                render_inline_text(text, style, width, &mut output, max_lines);
            }
            MarkdownBlock::Code { language, text } => {
                render_code(language.as_deref(), text, width, &mut output, max_lines);
            }
            MarkdownBlock::Quote(text) => {
                render_prefixed_inline(
                    "| ",
                    text,
                    Style::default().fg(Color::DarkGray),
                    width,
                    &mut output,
                    max_lines,
                );
            }
            MarkdownBlock::ListItem {
                ordered,
                marker,
                text,
            } => {
                let prefix = if *ordered {
                    format!("{marker} ")
                } else {
                    "- ".to_owned()
                };
                render_prefixed_inline(
                    &prefix,
                    text,
                    Style::default(),
                    width,
                    &mut output,
                    max_lines,
                );
            }
            MarkdownBlock::Rule => {
                push_line(
                    &mut output,
                    Line::from(Span::styled(
                        "-".repeat(width),
                        Style::default().fg(Color::DarkGray),
                    )),
                    max_lines,
                );
            }
        }
        if output.len() >= max_lines {
            break;
        }
    }

    if output.is_empty() {
        output.push(Line::default());
    }
    output
}

fn flush_paragraph(blocks: &mut Vec<MarkdownBlock>, paragraph: &mut Vec<String>) {
    if paragraph.is_empty() {
        return;
    }
    blocks.push(MarkdownBlock::Paragraph(paragraph.join("\n")));
    paragraph.clear();
}

fn heading_info(line: &str) -> Option<(u8, String)> {
    let trimmed = line.trim_start();
    let level = trimmed
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &trimmed[level..];
    if !starts_with_whitespace(rest) {
        return None;
    }
    Some((level as u8, rest.trim().to_owned()))
}

fn quote_info(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest).to_owned())
}

fn list_info(line: &str) -> Option<(bool, String, String)> {
    let trimmed = line.trim_start();
    let mut chars = trimmed.char_indices();
    let first = chars.next()?.1;
    if matches!(first, '-' | '*' | '+') {
        let marker_end = first.len_utf8();
        let rest = &trimmed[marker_end..];
        if starts_with_whitespace(rest) {
            return Some((false, first.to_string(), rest.trim().to_owned()));
        }
    }

    let digits_end = trimmed
        .char_indices()
        .take_while(|(_, character)| character.is_ascii_digit())
        .map(|(index, character)| index + character.len_utf8())
        .last()
        .unwrap_or(0);
    if digits_end > 0 {
        let rest = &trimmed[digits_end..];
        if let Some(rest) = rest.strip_prefix('.')
            && starts_with_whitespace(rest)
        {
            return Some((
                true,
                format!("{}.", &trimmed[..digits_end]),
                rest.trim().to_owned(),
            ));
        }
    }
    None
}

fn is_rule(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < 3 {
        return false;
    }
    let mut chars = trimmed.chars();
    let first = chars.next().unwrap_or_default();
    matches!(first, '-' | '*' | '_')
        && chars.all(|character| character == first || character.is_whitespace())
}

fn starts_with_whitespace(text: &str) -> bool {
    text.chars().next().is_some_and(char::is_whitespace)
}

fn fence_info(line: &str) -> Option<(char, usize, Option<String>)> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let count = trimmed
        .chars()
        .take_while(|character| *character == marker)
        .count();
    if count < 3 {
        return None;
    }
    let remainder = trimmed
        .char_indices()
        .nth(count)
        .map_or("", |(index, _)| &trimmed[index..]);
    let remainder = remainder.trim();
    let language = (!remainder.is_empty()).then(|| {
        remainder
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned()
    });
    Some((marker, count, language))
}

#[derive(Debug, Clone)]
struct StyledSegment {
    text: String,
    style: Style,
}

fn render_inline_text(
    text: &str,
    base_style: Style,
    width: usize,
    output: &mut Vec<Line<'static>>,
    max_lines: usize,
) {
    let segments = inline_segments(text, base_style);
    let lines = wrap_segments(&segments, width);
    for line in lines {
        push_line(output, line, max_lines);
        if output.len() >= max_lines {
            break;
        }
    }
}

fn render_prefixed_inline(
    prefix: &str,
    text: &str,
    base_style: Style,
    width: usize,
    output: &mut Vec<Line<'static>>,
    max_lines: usize,
) {
    let prefix_width = UnicodeWidthStr::width(prefix).min(width.saturating_sub(1));
    let prefix = truncate_to_width(prefix, width.saturating_sub(1));
    let body_width = width.saturating_sub(prefix_width).max(1);
    let segments = inline_segments(text, base_style);
    let lines = wrap_segments(&segments, body_width);
    for (index, line) in lines.into_iter().enumerate() {
        let mut spans = Vec::with_capacity(line.spans.len() + 1);
        let marker_style = if index == 0 {
            base_style.add_modifier(Modifier::BOLD)
        } else {
            base_style
        };
        spans.push(Span::styled(
            if index == 0 {
                prefix.clone()
            } else {
                " ".repeat(prefix_width)
            },
            marker_style,
        ));
        spans.extend(line.spans);
        push_line(output, Line::from(spans), max_lines);
        if output.len() >= max_lines {
            break;
        }
    }
}

fn render_code(
    language: Option<&str>,
    text: &str,
    width: usize,
    output: &mut Vec<Line<'static>>,
    max_lines: usize,
) {
    let style = Style::default().fg(Color::LightYellow);
    if let Some(language) = language {
        let label = truncate_to_width(&format!("[{language}]"), width);
        push_line(
            output,
            Line::from(Span::styled(label, style.add_modifier(Modifier::BOLD))),
            max_lines,
        );
    }
    let code_lines = if text.is_empty() {
        vec![String::new()]
    } else {
        text.split('\n').map(str::to_owned).collect()
    };
    for code_line in code_lines {
        let mut prefix = "| ".to_owned();
        let prefix_width = UnicodeWidthStr::width(prefix.as_str()).min(width.saturating_sub(1));
        prefix = truncate_to_width(&prefix, width.saturating_sub(1));
        let body_width = width.saturating_sub(prefix_width).max(1);
        let chunks = wrap_plain(&code_line, body_width);
        for (index, chunk) in chunks.into_iter().enumerate() {
            let marker = if index == 0 {
                prefix.clone()
            } else {
                " ".repeat(prefix_width)
            };
            push_line(
                output,
                Line::from(vec![
                    Span::styled(marker, style),
                    Span::styled(chunk, style),
                ]),
                max_lines,
            );
            if output.len() >= max_lines {
                return;
            }
        }
    }
}

fn inline_segments(text: &str, base_style: Style) -> Vec<StyledSegment> {
    let mut segments = Vec::new();
    let mut plain = String::new();
    let mut index = 0usize;
    while index < text.len() {
        let rest = &text[index..];
        let special = rest.as_bytes().first().copied().unwrap_or_default();
        let parsed = if special == b'`' {
            parse_delimited(rest, '`', "`")
                .map(|(content, consumed)| (content, consumed, base_style.fg(Color::LightYellow)))
        } else if rest.starts_with("**") {
            parse_delimited(rest, '*', "**").map(|(content, consumed)| {
                (content, consumed, base_style.add_modifier(Modifier::BOLD))
            })
        } else if rest.starts_with("__") {
            parse_delimited(rest, '_', "__").map(|(content, consumed)| {
                (content, consumed, base_style.add_modifier(Modifier::BOLD))
            })
        } else if special == b'*' {
            parse_delimited(rest, '*', "*").map(|(content, consumed)| {
                (content, consumed, base_style.add_modifier(Modifier::ITALIC))
            })
        } else if special == b'_' {
            parse_delimited(rest, '_', "_").map(|(content, consumed)| {
                (content, consumed, base_style.add_modifier(Modifier::ITALIC))
            })
        } else if special == b'[' {
            parse_link(rest).map(|(content, consumed)| {
                (
                    content,
                    consumed,
                    base_style
                        .fg(Color::LightBlue)
                        .add_modifier(Modifier::UNDERLINED),
                )
            })
        } else {
            None
        };

        if let Some((content, consumed, style)) = parsed {
            if !plain.is_empty() {
                segments.push(StyledSegment {
                    text: std::mem::take(&mut plain),
                    style: base_style,
                });
            }
            segments.push(StyledSegment {
                text: content,
                style,
            });
            index += consumed;
            continue;
        }

        let character = rest.chars().next().unwrap_or_default();
        plain.push(character);
        index += character.len_utf8();
    }
    if !plain.is_empty() {
        segments.push(StyledSegment {
            text: plain,
            style: base_style,
        });
    }
    if segments.is_empty() {
        segments.push(StyledSegment {
            text: String::new(),
            style: base_style,
        });
    }
    segments
}

fn parse_delimited(rest: &str, _marker: char, delimiter: &str) -> Option<(String, usize)> {
    if !rest.starts_with(delimiter) {
        return None;
    }
    let content_start = delimiter.len();
    let close = rest[content_start..].find(delimiter)?;
    if close == 0 {
        return None;
    }
    let content_end = content_start + close;
    let consumed = content_end + delimiter.len();
    Some((rest[content_start..content_end].to_owned(), consumed))
}

fn parse_link(rest: &str) -> Option<(String, usize)> {
    let label_end = rest[1..].find(']')? + 1;
    if !rest[label_end..].starts_with("](") {
        return None;
    }
    let target_start = label_end + 2;
    let target_end = rest[target_start..].find(')')? + target_start;
    if target_end == target_start {
        return None;
    }
    Some((rest[1..label_end].to_owned(), target_end + 1))
}

fn wrap_segments(segments: &[StyledSegment], width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;

    for segment in segments {
        for character in segment.text.chars() {
            if character == '\n' {
                lines.push(Line::from(std::mem::take(&mut spans)));
                used = 0;
                continue;
            }
            let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
            let (character, character_width) = if character_width > width {
                ('?', 1)
            } else {
                (character, character_width)
            };
            if character_width > 0 && used > 0 && used.saturating_add(character_width) > width {
                lines.push(Line::from(std::mem::take(&mut spans)));
                used = 0;
            }
            if let Some(last) = spans.last_mut()
                && last.style == segment.style
            {
                last.content.to_mut().push(character);
            } else {
                spans.push(Span::styled(character.to_string(), segment.style));
            }
            used = used.saturating_add(character_width);
        }
    }
    lines.push(Line::from(spans));
    if lines.is_empty() {
        lines.push(Line::default());
    }
    lines
}

fn wrap_plain(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0usize;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if character_width > width {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            line.push('?');
            used = 1;
            continue;
        }
        if character_width > 0 && used > 0 && used.saturating_add(character_width) > width {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        line.push(character);
        used = used.saturating_add(character_width);
    }
    lines.push(line);
    lines
}

fn heading_style(level: u8) -> Style {
    let color = match level {
        1 => Color::LightCyan,
        2 => Color::Cyan,
        _ => Color::LightBlue,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn push_line(output: &mut Vec<Line<'static>>, line: Line<'static>, max_lines: usize) {
    if output.len() < max_lines {
        output.push(line);
    }
}

fn bounded_line_limit(width: usize, requested: usize) -> usize {
    requested
        .max(1)
        .min(MAX_MARKDOWN_CELLS.saturating_div(width.max(1)).max(1))
}

fn truncate_bytes(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Replace terminal control bytes before text reaches a Ratatui span.  A
/// provider response is untrusted input; allowing an ESC sequence through a
/// renderer would let it reprogram the user's terminal.  Newlines are kept as
/// structural Markdown separators and tabs become spaces for deterministic
/// cell-width accounting.
fn sanitize_text(text: &str) -> String {
    sanitize_text_with_limit(text, MAX_MARKDOWN_BYTES)
}

fn sanitize_text_with_limit(text: &str, max_bytes: usize) -> String {
    let mut result = String::with_capacity(text.len().min(max_bytes));
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        // Normalize both CRLF and lone CR before block parsing.  For CRLF the
        // following LF is retained, so exactly one structural newline occurs.
        if character == '\r' && characters.peek() == Some(&'\n') {
            continue;
        }
        let replacement = match character {
            '\n' | '\r' => "\n",
            '\t' => "    ",
            character if character.is_control() => "?",
            character => {
                let mut buffer = [0_u8; 4];
                let encoded = character.encode_utf8(&mut buffer);
                if result.len().saturating_add(encoded.len()) > max_bytes {
                    break;
                }
                result.push_str(encoded);
                continue;
            }
        };
        if result.len().saturating_add(replacement.len()) > max_bytes {
            break;
        }
        result.push_str(replacement);
    }
    result
}

fn truncate_to_width(text: &str, width: usize) -> String {
    let mut result = String::new();
    let mut used = 0usize;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used.saturating_add(character_width) > width {
            break;
        }
        result.push(character);
        used = used.saturating_add(character_width);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn parses_blocks_without_swallowing_unknown_text() {
        let blocks = parse_markdown(
            "# Title\n\nplain **bold**\n\n```rust\nlet x = 1;\n```\n\n> quote\n\n- item\n\n1. first\n\n---\nunknown *syntax",
        );
        assert!(matches!(blocks[0], MarkdownBlock::Heading { level: 1, .. }));
        assert!(matches!(blocks[1], MarkdownBlock::Paragraph(_)));
        assert!(
            matches!(blocks[2], MarkdownBlock::Code { ref language, .. } if language.as_deref() == Some("rust"))
        );
        assert!(matches!(blocks[3], MarkdownBlock::Quote(_)));
        assert!(matches!(
            blocks[4],
            MarkdownBlock::ListItem { ordered: false, .. }
        ));
        assert!(matches!(
            blocks[5],
            MarkdownBlock::ListItem { ordered: true, .. }
        ));
        assert!(matches!(blocks[6], MarkdownBlock::Rule));
        assert!(matches!(blocks[7], MarkdownBlock::Paragraph(_)));
    }

    #[test]
    fn renders_inline_styles_and_hides_link_target() {
        let lines = render_markdown(
            "**bold** *italic* `code` [label](https://example.test)",
            120,
        );
        let spans = &lines[0].spans;
        assert!(spans.iter().any(|span| {
            span.content == "bold" && span.style.add_modifier(Modifier::BOLD) == span.style
        }));
        assert!(spans.iter().any(|span| {
            span.content == "italic" && span.style.add_modifier(Modifier::ITALIC) == span.style
        }));
        assert!(spans.iter().any(|span| span.content == "label"));
        assert!(!line_text(&lines[0]).contains("https://"));
    }

    #[test]
    fn output_is_bounded_for_narrow_unicode_viewports() {
        let lines = render_markdown("世界 wide text", 1);
        assert!(!lines.is_empty());
        assert!(
            lines
                .iter()
                .all(|line| UnicodeWidthStr::width(line_text(line).as_str()) <= 1)
        );
    }

    #[test]
    fn unclosed_fence_is_visible_as_code() {
        let blocks = parse_markdown("```text\nnot hidden");
        assert!(
            matches!(blocks.as_slice(), [MarkdownBlock::Code { language: Some(language), text }] if language == "text" && text == "not hidden")
        );
    }

    #[test]
    fn line_limit_is_hard() {
        let lines = render_markdown_with_limit("a\n\n b\n\n c\n\n d", 80, 2);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn terminal_control_sequences_are_rendered_as_text() {
        let lines = render_markdown("before\u{1b}[2Jafter\tend", 80);
        let text = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("before?[2Jafter    end"));
    }

    #[test]
    fn crlf_input_does_not_leak_carriage_returns() {
        let blocks = parse_markdown("first\r\n\r\nsecond");
        assert!(matches!(&blocks[0], MarkdownBlock::Paragraph(text) if text == "first"));
        assert!(matches!(&blocks[1], MarkdownBlock::Paragraph(text) if text == "second"));
    }

    #[test]
    fn prefixed_rendering_keeps_role_and_body_inside_width() {
        let lines = render_markdown_prefixed("you: ", "a long **message**", 8, Style::default());
        assert!(
            lines
                .iter()
                .all(|line| { UnicodeWidthStr::width(line_text(line).as_str()) <= 8 })
        );
        assert!(line_text(&lines[0]).starts_with("you: "));
        assert!(line_text(&lines[1]).starts_with("     "));
    }

    #[test]
    fn plain_prefixed_rendering_does_not_interpret_markers() {
        let lines = render_plain_prefixed("user: ", "# heading\n---", 80, Style::default());
        let text = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(text.contains("# heading"));
        assert!(text.contains("---"));
    }

    #[test]
    fn sanitization_bound_applies_after_tab_expansion() {
        let input = "\t".repeat(MAX_MARKDOWN_BYTES);
        assert!(sanitize_text(&input).len() <= MAX_MARKDOWN_BYTES);
    }

    #[test]
    fn cell_budget_limits_wide_viewport_allocations() {
        let input = "---\n".repeat(128);
        let lines = render_markdown_with_limit(&input, u16::MAX as usize, MAX_MARKDOWN_LINES);
        assert!(lines.len() <= MAX_MARKDOWN_CELLS / u16::MAX as usize + 1);
    }
}
