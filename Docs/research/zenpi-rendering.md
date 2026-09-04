# zenpi Rendering Audit

## Baseline

The v1 TUI transcript renderer (`src/tui.rs::transcript_lines`) is bounded and
Unicode-cell aware, but it treats every message body as plain text. It does not
distinguish headings, paragraphs, quotes, lists, fenced code, or inline styles.
That is acceptable for a transport smoke test, but it is not yet the code/text/
basic-Markdown experience expected from a terminal coding agent.

## Lightweight v2 slice

`src/render.rs` adds a dependency-free, owned Ratatui renderer for the useful
conversation subset:

- block parsing: headings, paragraphs, fenced code, quotes, lists, rules;
- inline styling: bold, italic, inline code, and link labels (targets hidden);
- Unicode display-cell wrapping, bounded input/output, and narrow viewport
  handling;
- terminal-control sanitization before provider text reaches a span;
- loss-tolerant fallback: unsupported syntax is shown as ordinary text and an
  unclosed code fence remains visible through EOF.

It intentionally does not claim CommonMark compliance, syntax highlighting,
tables, HTML, nested list layout, images, or terminal hyperlinks. Those belong
behind a separately measured feature gate rather than becoming an implicit
dependency or an unbounded parser in the core TUI.

## Integration gate

The module is now wired into the TUI `transcript_lines` path. Assistant and
system messages use `render_markdown_prefixed`; user prompts, tool output, and
errors use `render_plain_prefixed`, so model prose gets useful formatting while
untrusted command/diff text remains literal. A Ratatui `TestBackend` assertion
covers the live integration. The remaining v2 gate is to carry the same block
model through headless events and add deterministic style snapshots for:

1. styled heading/code/list/quote output;
2. role-prefix plus body width never exceeding the viewport;
3. malformed fences and control bytes remaining visible but harmless;
4. fold/expand preserving the canonical message queue.

The standalone parser/renderer tests in `src/render.rs` and
`tests/render_markdown.rs`, plus `tests/tui_markdown.rs`, are the current
evidence for the bounded TUI slice. They do not substitute for headless block
serialization, diff blocks, or final golden snapshots.
