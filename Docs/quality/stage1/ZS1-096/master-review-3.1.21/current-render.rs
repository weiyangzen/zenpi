//! Lightweight rendering for assistant text.
//!
//! The interactive TUI deliberately does not pull in a full Markdown parser or
//! syntax-highlighting stack.  This module covers the small, predictable
//! subset that is useful in a terminal conversation while keeping the output
//! bounded and safe for a narrow terminal.  Unsupported Markdown is rendered
//! as ordinary text rather than dropped or interpreted as terminal control
//! sequences.

use std::collections::VecDeque;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
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
    parse_markdown_bounded(bounded)
}

fn parse_markdown_bounded(bounded: &str) -> Vec<MarkdownBlock> {
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

/// Render the latest visual rows of a bounded Markdown message.
///
/// Unlike the head API, this parses the whole accepted input before selecting
/// blocks from the end, so a retained code/inline continuation keeps its style.
/// Input beyond MAX_MARKDOWN_BYTES retains the existing head-byte policy. Tabs
/// expand during wrapping, not before byte bounding, so even a legal input full
/// of tabs cannot displace its final text. The role prefix labels the first
/// retained row; internal list/code markers retain their original row position.
pub fn render_markdown_tail_prefixed(
    prefix: &str,
    input: &str,
    width: usize,
    prefix_style: Style,
) -> Vec<Line<'static>> {
    render_markdown_tail_prefixed_with_metadata(prefix, input, width, prefix_style).lines
}

/// Plain-text counterpart of [`render_markdown_tail_prefixed`].
/// Markdown markers remain literal; controls and wrapping use the same bounds.
pub fn render_plain_tail_prefixed(
    prefix: &str,
    input: &str,
    width: usize,
    prefix_style: Style,
) -> Vec<Line<'static>> {
    render_plain_tail_prefixed_with_metadata(prefix, input, width, prefix_style).lines
}

/// A retained suffix with its exact visual-row origin in the accepted input.
/// `omitted_visual_lines + index` is the original row of `lines[index]` under
/// this renderer and width. A width change or a Markdown edit that reparses an
/// earlier block can change that layout; callers must invalidate such anchors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedTail {
    pub lines: Vec<Line<'static>>,
    pub omitted_visual_lines: usize,
}

/// Metadata variant of [`render_markdown_tail_prefixed`]. Counts all accepted
/// visual rows without constructing discarded lines or expanding rules.
pub fn render_markdown_tail_prefixed_with_metadata(
    prefix: &str,
    input: &str,
    width: usize,
    prefix_style: Style,
) -> RenderedTail {
    render_tail_metadata(prefix, input, width, prefix_style, true)
}

/// Metadata variant of [`render_plain_tail_prefixed`]. The row count includes
/// a final empty row after a newline, matching the plain head renderer.
pub fn render_plain_tail_prefixed_with_metadata(
    prefix: &str,
    input: &str,
    width: usize,
    prefix_style: Style,
) -> RenderedTail {
    render_tail_metadata(prefix, input, width, prefix_style, false)
}

#[cfg(test)]
fn render_tail_prefixed(
    prefix: &str,
    input: &str,
    width: usize,
    style: Style,
    markdown: bool,
) -> Vec<Line<'static>> {
    render_tail_metadata(prefix, input, width, style, markdown).lines
}

fn render_tail_metadata(
    prefix: &str,
    input: &str,
    width: usize,
    prefix_style: Style,
    markdown: bool,
) -> RenderedTail {
    let width = width.clamp(1, MAX_MARKDOWN_WIDTH);
    let prefix = sanitize_text(truncate_bytes(prefix, MAX_MARKDOWN_BYTES)).replace('\n', " ");
    let prefix = tail_prefix_to_width(&prefix, width.saturating_sub(1));
    let prefix_width = UnicodeWidthStr::width(prefix.as_str());
    let body_width = width.saturating_sub(prefix_width).max(1);
    // Count the role indentation in the cell budget, not only body columns.
    let limit = bounded_line_limit(width, MAX_MARKDOWN_LINES);
    let compact = sanitize_compact(truncate_bytes(input, MAX_MARKDOWN_BYTES));
    let mut output = VecDeque::new();
    let total;
    if markdown {
        let blocks = parse_markdown_bounded(&compact);
        total = (blocks
            .iter()
            .map(|block| block_visual_lines(block, body_width))
            .sum::<usize>()
            + blocks.len().saturating_sub(1))
        .max(1);
        for (index, block) in blocks.iter().enumerate().rev() {
            let lines = render_block_tail(block, body_width, limit - output.len());
            for line in lines.into_iter().rev() {
                output.push_front(line);
            }
            if output.len() == limit {
                break;
            }
            if index > 0 {
                output.push_front(Line::default());
                if output.len() == limit {
                    break;
                }
            }
        }
    } else {
        (output, total) = wrap_tail_segments(
            &[StyledSegment {
                text: compact,
                style: Style::default(),
            }],
            body_width,
            limit,
        );
    }
    if output.is_empty() {
        output.push_back(Line::default());
    }
    let continuation = " ".repeat(prefix_width);
    let omitted_visual_lines = total - output.len();
    let lines = output
        .into_iter()
        .enumerate()
        .map(|(index, mut line)| {
            line.spans.insert(
                0,
                Span::styled(
                    if index == 0 {
                        prefix.clone()
                    } else {
                        continuation.clone()
                    },
                    prefix_style,
                ),
            );
            line
        })
        .collect();
    RenderedTail {
        lines,
        omitted_visual_lines,
    }
}

// Normalization cannot expand the bounded source: retain tabs as a compact
// token until wrapping. CRLF/lone CR and other controls match sanitize_text.
fn sanitize_compact(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\r' && chars.peek() == Some(&'\n') {
            continue;
        }
        output.push(match character {
            '\r' => '\n',
            '\n' | '\t' => character,
            c if c.is_control() => '?',
            c => c,
        });
    }
    output
}

fn retain_tail(lines: &mut VecDeque<Line<'static>>, line: Line<'static>, limit: usize) {
    if lines.len() == limit {
        lines.pop_front();
    }
    lines.push_back(line);
}

// At most limit completed rows plus one in-progress row are allocated. Text
// spans are bounded by the input; tab expansion is emitted one cell at a time.
// The total count lets callers distinguish an original first row from a tail
// continuation without attaching a potentially huge marker to discarded rows.
fn wrap_tail_segments(
    segments: &[StyledSegment],
    width: usize,
    limit: usize,
) -> (VecDeque<Line<'static>>, usize) {
    let mut lines = VecDeque::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let total = walk_wrapped_segments(segments, width, |event| {
        if let Some((text, style)) = event {
            if let Some(last) = spans.last_mut()
                && last.style == style
            {
                last.content.to_mut().push_str(text);
            } else {
                spans.push(Span::styled(text.to_owned(), style));
            }
        } else {
            retain_tail(&mut lines, Line::from(std::mem::take(&mut spans)), limit);
        }
    });
    (lines, total)
}

// One wrapping state machine for counting and retaining. None completes a row;
// the counting caller ignores events and allocates no Line, Span or separator.
fn walk_wrapped_segments(
    segments: &[StyledSegment],
    width: usize,
    mut emit: impl FnMut(Option<(&str, Style)>),
) -> usize {
    let mut used = 0usize;
    let mut total = 0usize;
    // Graphemes can cross Markdown style boundaries (for example a styled
    // VS16 after a plain heart). Join at most the bounded source bytes and
    // assign each indivisible grapheme the style of its first byte.
    let joined = segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<String>();
    let mut segment_index = 0usize;
    let mut segment_end = segments.first().map_or(0, |segment| segment.text.len());
    for (offset, raw) in joined.grapheme_indices(true) {
        while segment_index + 1 < segments.len() && offset >= segment_end {
            segment_index += 1;
            segment_end += segments[segment_index].text.len();
        }
        let style = segments[segment_index].style;
        for _ in 0..if raw == "\t" { 4 } else { 1 } {
            let text = if raw == "\t" { " " } else { raw };
            if text == "\n" {
                emit(None);
                total += 1;
                used = 0;
                continue;
            }
            let cells = UnicodeWidthStr::width(text);
            let (text, cells) = if cells > width || (cells == 0 && used == 0) {
                ("?", 1)
            } else {
                (text, cells)
            };
            if cells > 0 && used > 0 && used.saturating_add(cells) > width {
                emit(None);
                total += 1;
                used = 0;
            }
            emit(Some((text, style)));
            used += cells;
        }
    }
    emit(None);
    total + 1
}

fn block_visual_lines(block: &MarkdownBlock, width: usize) -> usize {
    let inline_count = |text: &str, prefix: &str| {
        let prefix = tail_prefix_to_width(prefix, width.saturating_sub(1));
        walk_wrapped_segments(
            &inline_segments(text, Style::default()),
            width - UnicodeWidthStr::width(prefix.as_str()),
            |_| {},
        )
    };
    match block {
        MarkdownBlock::Paragraph(text) | MarkdownBlock::Heading { text, .. } => {
            inline_count(text, "")
        }
        MarkdownBlock::Quote(text) => inline_count(text, "| "),
        MarkdownBlock::ListItem {
            ordered,
            marker,
            text,
        } => inline_count(
            text,
            &if *ordered {
                format!("{marker} ")
            } else {
                "- ".to_owned()
            },
        ),
        MarkdownBlock::Rule => 1,
        MarkdownBlock::Code { language, text } => {
            let body_width = width - 2.min(width.saturating_sub(1));
            usize::from(language.is_some())
                + text
                    .split('\n')
                    .map(|text| {
                        walk_wrapped_segments(
                            &[StyledSegment {
                                text: text.to_owned(),
                                style: Style::default(),
                            }],
                            body_width,
                            |_| {},
                        )
                    })
                    .sum::<usize>()
        }
    }
}

fn tail_inline(
    prefix: &str,
    text: &str,
    style: Style,
    width: usize,
    limit: usize,
) -> VecDeque<Line<'static>> {
    let prefix = tail_prefix_to_width(prefix, width.saturating_sub(1));
    let prefix_width = UnicodeWidthStr::width(prefix.as_str());
    let segments = inline_segments(text, style);
    let (mut lines, total) = wrap_tail_segments(&segments, width - prefix_width, limit);
    let first = total - lines.len();
    // Decorate only retained rows. A long ordered-list marker cannot multiply
    // allocations by the total number of discarded one-cell body rows.
    for (index, line) in lines.iter_mut().enumerate() {
        let original_first = first + index == 0;
        line.spans.insert(
            0,
            Span::styled(
                if original_first {
                    prefix.clone()
                } else {
                    " ".repeat(prefix_width)
                },
                if original_first {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    style
                },
            ),
        );
    }
    lines
}

fn render_block_tail(block: &MarkdownBlock, width: usize, limit: usize) -> VecDeque<Line<'static>> {
    match block {
        MarkdownBlock::Paragraph(text) => {
            wrap_tail_segments(&inline_segments(text, Style::default()), width, limit).0
        }
        MarkdownBlock::Heading { level, text } => {
            wrap_tail_segments(&inline_segments(text, heading_style(*level)), width, limit).0
        }
        MarkdownBlock::Quote(text) => tail_inline(
            "| ",
            text,
            Style::default().fg(Color::DarkGray),
            width,
            limit,
        ),
        MarkdownBlock::ListItem {
            ordered,
            marker,
            text,
        } => tail_inline(
            &if *ordered {
                format!("{marker} ")
            } else {
                "- ".to_owned()
            },
            text,
            Style::default(),
            width,
            limit,
        ),
        MarkdownBlock::Rule => VecDeque::from([Line::from(Span::styled(
            "-".repeat(width),
            Style::default().fg(Color::DarkGray),
        ))]),
        MarkdownBlock::Code { language, text } => {
            let style = Style::default().fg(Color::LightYellow);
            let prefix = tail_prefix_to_width("| ", width.saturating_sub(1));
            let prefix_width = UnicodeWidthStr::width(prefix.as_str());
            let mut output = VecDeque::new();
            // Select physical code rows backwards, while wrapping each row
            // forwards. This retains the original fence state and row markers.
            for text in text.rsplit('\n') {
                let (mut lines, total) = wrap_tail_segments(
                    &[StyledSegment {
                        text: text.to_owned(),
                        style,
                    }],
                    width - prefix_width,
                    limit - output.len(),
                );
                let first = total - lines.len();
                for (index, line) in lines.iter_mut().enumerate() {
                    if line.spans.is_empty() {
                        line.spans.push(Span::styled("", style));
                    }
                    line.spans.insert(
                        0,
                        Span::styled(
                            if first + index == 0 {
                                prefix.clone()
                            } else {
                                " ".repeat(prefix_width)
                            },
                            style,
                        ),
                    );
                }
                for line in lines.into_iter().rev() {
                    output.push_front(line);
                }
                if output.len() == limit {
                    return output;
                }
            }
            if let Some(language) = language {
                let label = tail_prefix_to_width(&format!("[{language}]"), width);
                // Tabs in a fence info string are separators, so the language
                // token itself contains no delayed tab expansion.
                output.push_front(Line::from(Span::styled(
                    label,
                    style.add_modifier(Modifier::BOLD),
                )));
            }
            output
        }
    }
}

// Keep extended graphemes intact, including VS16 and ZWJ sequences whose
// display width is not the sum of individual Unicode scalar widths.
fn tail_prefix_to_width(text: &str, width: usize) -> String {
    let mut result = String::new();
    let mut used = 0usize;
    for grapheme in text.graphemes(true) {
        let cells = UnicodeWidthStr::width(grapheme);
        let (grapheme, cells) = if cells == 0 && used == 0 {
            ("?", 1)
        } else {
            (grapheme, cells)
        };
        if used.saturating_add(cells) > width {
            break;
        }
        result.push_str(grapheme);
        used += cells;
    }
    result
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
    let mut previous_non_underscore: Option<char> = None;
    let mut link_scan = LinkScan::default();
    while index < text.len() {
        let rest = &text[index..];
        let special = rest.as_bytes().first().copied().unwrap_or_default();
        let intraword_underscore =
            special == b'_' && previous_non_underscore.is_some_and(char::is_alphanumeric);
        let parsed = if intraword_underscore {
            None
        } else if special == b'`' {
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
            link_scan.parse(text, index).map(|(content, consumed)| {
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
            if let Some(character) = rest[..consumed].trim_end_matches('_').chars().next_back() {
                previous_non_underscore = Some(character);
            }
            index += consumed;
            continue;
        }

        let character = rest.chars().next().unwrap_or_default();
        plain.push(character);
        if character != '_' {
            previous_non_underscore = Some(character);
        }
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

// Searches advance monotonically even when many '[' candidates share a
// missing/invalid closer. Cache EOF as well: unsuccessful searches are not
// restarted on each following '['. The successful-link grammar is unchanged.
#[derive(Default)]
struct LinkScan {
    label_end: usize,
    target_end: usize,
    #[cfg(test)]
    scanned_bytes: usize,
}

impl LinkScan {
    fn closer(&mut self, text: &str, start: usize, delimiter: char, cached: usize) -> usize {
        if cached >= start {
            return cached;
        }
        let relative = text[start..].find(delimiter);
        #[cfg(test)]
        {
            self.scanned_bytes += relative.map_or(text.len() - start, |offset| offset + 1);
        }
        relative.map_or(text.len(), |offset| start + offset)
    }

    fn parse(&mut self, text: &str, index: usize) -> Option<(String, usize)> {
        self.label_end = self.closer(text, index + 1, ']', self.label_end);
        if !text[self.label_end..].starts_with("](") {
            return None;
        }
        let target_start = self.label_end + 2;
        self.target_end = self.closer(text, target_start, ')', self.target_end);
        if self.target_end == text.len() || self.target_end == target_start {
            return None;
        }
        Some((
            text[index + 1..self.label_end].to_owned(),
            self.target_end + 1 - index,
        ))
    }
}

fn wrap_segments(segments: &[StyledSegment], width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    // Inspectors and approval previews use the head renderer too. Measure the
    // same indivisible graphemes as the transcript so a variation selector or
    // ZWJ cannot make Ratatui clip characters that were counted as fitting.
    walk_wrapped_segments(segments, width.max(1), |event| {
        if let Some((text, style)) = event {
            if let Some(last) = spans.last_mut()
                && last.style == style
            {
                last.content.to_mut().push_str(text);
            } else {
                spans.push(Span::styled(text.to_owned(), style));
            }
        } else {
            lines.push(Line::from(std::mem::take(&mut spans)));
        }
    });
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

    #[test]
    fn tail_retains_latest_wrapped_rows_while_head_stays_at_start() {
        let input = format!("{}LATEST", "x".repeat(MAX_MARKDOWN_BYTES - 6));
        for markdown in [false, true] {
            let tail = render_tail_prefixed("", &input, 1, Style::default(), markdown);
            assert_eq!(tail.len(), MAX_MARKDOWN_LINES);
            assert!(
                tail.iter()
                    .map(line_text)
                    .collect::<String>()
                    .ends_with("LATEST")
            );
            assert!(tail.iter().all(|line| line.width() <= 1));
        }
        let head = render_markdown_prefixed("", &input, 1, Style::default());
        assert_eq!(head.len(), MAX_MARKDOWN_LINES);
        assert!(head.iter().all(|line| line_text(line) == "x"));
        let plain_head = render_plain_prefixed("", &input, 1, Style::default());
        assert!(plain_head.iter().all(|line| line_text(line) == "x"));
    }

    #[test]
    fn tail_matches_uncapped_head_styles_and_block_spacing() {
        let inputs = [
            "# Heading\n\nhello **bold** _italic_ `code` [link](target)",
            "> quote wraps across rows\n\n12. ordered item\n\n- item\n---",
            "```rust\nlet x = 1;\n\nnext\n```\n\nlast",
            "```\n\n```",
            "",
            "a\r\n\r\nb\rc",
            "a\tb\n\n> c\td",
        ];
        for input in inputs {
            for width in [1, 3, 12, 80] {
                let style = Style::default().fg(Color::Green);
                assert_eq!(
                    render_markdown_tail_prefixed("ai: ", input, width, style),
                    render_markdown_prefixed("ai: ", input, width, style),
                    "{input:?}, {width}"
                );
                assert_eq!(
                    render_plain_tail_prefixed("ai: ", input, width, style),
                    render_plain_prefixed("ai: ", input, width, style),
                    "plain {input:?}, {width}"
                );
            }
        }
    }

    #[test]
    fn tail_preserves_unclosed_fence_and_inline_context() {
        let code = format!("```rust\n{}**literal**\n", "old\n".repeat(10_000));
        let lines = render_markdown_tail_prefixed("ai: ", &code, 40, Style::default());
        assert_eq!(lines.len(), MAX_MARKDOWN_LINES);
        let last = lines.last().unwrap();
        assert!(line_text(last).contains("| **literal**"));
        assert!(
            last.spans
                .iter()
                .any(|span| span.content.contains("**literal**")
                    && span.style.fg == Some(Color::LightYellow))
        );
        assert!(!lines.iter().any(|line| line_text(line).contains("[rust]")));
        let inline = format!("**{}LATEST**", "x".repeat(20_000));
        let lines = render_markdown_tail_prefixed("", &inline, 1, Style::default());
        assert!(
            lines
                .iter()
                .map(line_text)
                .collect::<String>()
                .ends_with("LATEST")
        );
        assert!(
            lines
                .iter()
                .flat_map(|line| &line.spans)
                .filter(|span| !span.content.is_empty())
                .all(|span| span.style.add_modifier.contains(Modifier::BOLD))
        );
    }

    #[test]
    fn tail_distinguishes_closed_fence_quote_list_and_plain_markers() {
        let input = format!(
            "~~~rust\n{}~~~\n\n# Final\n\n> quote\n\n1. last",
            "old\n".repeat(10_000)
        );
        let lines = render_markdown_tail_prefixed("", &input, 40, Style::default());
        assert_eq!(line_text(lines.last().unwrap()), "1. last");
        assert!(lines.iter().any(|line| line_text(line) == "| quote"));
        assert!(lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content == "Final" && span.style.fg == Some(Color::LightCyan))
        }));
        let plain = render_plain_tail_prefixed("", "# Final\n---\n**bold**", 80, Style::default());
        assert_eq!(
            plain.iter().map(line_text).collect::<Vec<_>>(),
            ["# Final", "---", "**bold**"]
        );
    }

    #[test]
    fn tail_tabs_do_not_consume_the_final_legal_input_bytes() {
        let input = format!("{}END", "\t".repeat(MAX_MARKDOWN_BYTES - 3));
        for markdown in [false, true] {
            let lines = render_tail_prefixed("", &input, 1, Style::default(), markdown);
            assert!(
                lines
                    .iter()
                    .map(line_text)
                    .collect::<String>()
                    .ends_with("END")
            );
            assert!(lines.len() <= MAX_MARKDOWN_LINES);
        }
        let over = format!("{}OUTSIDE", "x".repeat(MAX_MARKDOWN_BYTES));
        assert!(
            !render_plain_tail_prefixed("", &over, 80, Style::default())
                .iter()
                .map(line_text)
                .collect::<String>()
                .contains("OUTSIDE")
        );
    }

    #[test]
    fn tail_unicode_prefix_controls_and_cells_are_bounded() {
        let input = "世界 ❤️ 👨‍👩‍👧‍👦 e\u{301}\r\nnext\u{1b}[2J\tend\u{7f}";
        for width in [0, 1, 2, 8, 80, usize::MAX] {
            for markdown in [false, true] {
                let lines =
                    render_tail_prefixed("❤️\u{1b}\t", input, width, Style::default(), markdown);
                let bound = width.clamp(1, MAX_MARKDOWN_WIDTH);
                assert!(lines.iter().all(|line| line.width() <= bound));
                assert!(
                    lines
                        .iter()
                        .all(|line| UnicodeWidthStr::width(line_text(line).as_str()) <= bound)
                );
                assert!(
                    lines
                        .iter()
                        .flat_map(|line| &line.spans)
                        .all(|span| !span.content.chars().any(char::is_control))
                );
            }
        }
        let lines = render_plain_tail_prefixed("", "❤️", 1, Style::default());
        assert_eq!(line_text(&lines[0]), "?");
    }

    #[test]
    fn tail_wide_rules_and_role_indentation_share_the_cell_budget() {
        let input = format!("{}LATEST", "---\n".repeat(60_000));
        let prefix = "p".repeat(MAX_MARKDOWN_WIDTH - 2);
        for role in ["", prefix.as_str()] {
            let lines = render_markdown_tail_prefixed(role, &input, usize::MAX, Style::default());
            assert!(lines.len() <= MAX_MARKDOWN_CELLS / MAX_MARKDOWN_WIDTH);
            assert!(lines.iter().map(Line::width).sum::<usize>() <= MAX_MARKDOWN_CELLS);
            assert!(
                lines
                    .iter()
                    .map(line_text)
                    .collect::<String>()
                    .replace(' ', "")
                    .ends_with("LATEST")
            );
            assert!(
                lines
                    .iter()
                    .map(|line| line
                        .spans
                        .iter()
                        .map(|span| span.content.len())
                        .sum::<usize>())
                    .sum::<usize>()
                    <= MAX_MARKDOWN_CELLS * 4 + MAX_MARKDOWN_BYTES * 2
            );
        }
    }

    #[test]
    fn tail_continuation_does_not_reinsert_ordered_marker() {
        let input = format!("1. {}END", "x".repeat(30_000));
        let lines = render_markdown_tail_prefixed("ai: ", &input, 9, Style::default());
        assert_eq!(lines.len(), MAX_MARKDOWN_LINES);
        assert!(line_text(&lines[0]).starts_with("ai:    "));
        assert!(!lines.iter().any(|line| line_text(line).contains("1.")));
        assert!(
            lines
                .iter()
                .map(line_text)
                .collect::<String>()
                .replace(' ', "")
                .ends_with("END")
        );
    }

    #[test]
    fn unfinished_link_search_is_linear_and_valid_links_remain_styled() {
        for suffix in ["", "]no-target", "](missing-close", "]()", "]x]x](missing"] {
            let text = format!("{}{suffix}", "[".repeat(MAX_MARKDOWN_BYTES - suffix.len()));
            let mut scan = LinkScan::default();
            for (index, character) in text.char_indices() {
                if character == '[' {
                    assert!(scan.parse(&text, index).is_none());
                }
            }
            assert!(
                scan.scanned_bytes <= text.len() * 2,
                "{}",
                scan.scanned_bytes
            );
            let lines = render_markdown_tail_prefixed("", &text, 80, Style::default());
            assert!(!lines.is_empty());
        }
        let text = "[bad] [ok](target) [unfinished";
        let lines = render_markdown_tail_prefixed("", text, 80, Style::default());
        assert_eq!(line_text(&lines[0]), "[bad] ok [unfinished");
        assert!(
            lines[0].spans.iter().any(|span| span.content == "ok"
                && span.style.add_modifier.contains(Modifier::UNDERLINED))
        );
        let underscores = format!("word{} tail", "_".repeat(20_000));
        assert_eq!(
            inline_segments(&underscores, Style::default())
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<String>(),
            underscores
        );
    }

    #[test]
    fn tail_metadata_matches_full_small_render_and_large_exact_row_counts() {
        let input = format!(
            "{}\n\n```rust\ncode\n```\n\n> final",
            "row\n".repeat(10_000)
        );
        let full = render_markdown_with_limit(&input, 16, 30_000);
        let tail = render_markdown_tail_prefixed_with_metadata("", &input, 16, Style::default());
        assert_eq!(tail.omitted_visual_lines + tail.lines.len(), full.len());
        // Remove only the empty outer role span to compare the actual suffix.
        let body = tail
            .lines
            .iter()
            .cloned()
            .map(|mut line| {
                line.spans.remove(0);
                line
            })
            .collect::<Vec<_>>();
        assert_eq!(body, full[tail.omitted_visual_lines..]);
        let rules = format!("{}LATEST", "---\n".repeat(60_000));
        let tail = render_markdown_tail_prefixed_with_metadata(
            "",
            &rules,
            MAX_MARKDOWN_WIDTH,
            Style::default(),
        );
        assert_eq!(tail.omitted_visual_lines + tail.lines.len(), 120_001);
        let tail = render_plain_tail_prefixed_with_metadata("", "abc\n", 2, Style::default());
        assert_eq!(tail.omitted_visual_lines, 0);
        assert_eq!(
            tail.lines.iter().map(line_text).collect::<Vec<_>>(),
            ["ab", "c", ""]
        );
    }

    #[test]
    fn tail_metadata_keeps_surviving_streamed_rows_at_original_indices() {
        let input = format!("{}stable\n", "row\n".repeat(10_000));
        let before = render_plain_tail_prefixed_with_metadata("", &input, 20, Style::default());
        let after =
            render_plain_tail_prefixed_with_metadata("", &(input + "new\n"), 20, Style::default());
        assert_eq!(after.omitted_visual_lines, before.omitted_visual_lines + 1);
        for index in
            after.omitted_visual_lines..(before.omitted_visual_lines + before.lines.len() - 1)
        {
            assert_eq!(
                before.lines[index - before.omitted_visual_lines],
                after.lines[index - after.omitted_visual_lines]
            );
        }
        assert_eq!(line_text(&after.lines[after.lines.len() - 2]), "new");
    }

    #[test]
    fn tail_grapheme_safety_crosses_style_and_role_boundaries() {
        for input in [
            "❤**\u{fe0f}**x",
            "1**\u{20e3}**x",
            "a**\u{301}**",
            "\u{fe0f}x",
        ] {
            for prefix in ["", "❤", "\u{301}", "role\n\r\u{1b}"] {
                for width in [1, 2, 3, 16] {
                    let lines =
                        render_markdown_tail_prefixed(prefix, input, width, Style::default());
                    assert!(
                        lines
                            .iter()
                            .all(|line| UnicodeWidthStr::width(line_text(line).as_str()) <= width),
                        "{prefix:?}, {input:?}, {width}"
                    );
                    assert!(
                        lines
                            .iter()
                            .flat_map(|line| &line.spans)
                            .all(|span| !span.content.chars().any(char::is_control))
                    );
                }
            }
        }
        assert_eq!(
            line_text(&render_markdown_tail_prefixed("", "❤**\u{fe0f}**", 1, Style::default())[0]),
            "?"
        );
    }
}
