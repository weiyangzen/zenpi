# Paired development build

This is a local functional-test build, not whole-Blueprint acceptance.

- Installed binary: `~/.local/share/zenpi-dev/bin/zenpi`, production defaults
  with no echo fixture feature.
- Launcher: `~/.local/bin/zenpi-dev`. It uses a separate `~/.zenpi-dev` home,
  `local-codex` profile, and default development session. `--session PATH`
  selects a separate conversation; the current working directory is the tool
  workspace.
- Pairing used `zenpi pair import-codex --profile local-codex` with the real
  `~/.codex/config.toml` and `~/.codex/auth.json`, via the existing import owner.
  Provider/model/API: selected `OpenAI`, `gpt-6-astra`, Responses. Credentials
  are stored only in the separate private auth file (mode 0600).
- Source Codex configuration and auth checksums were unchanged after import
  and live tests. No credential was put in a command argument or repository.

## Verified live behavior

`tools/live_user_smoke.py` is opt-in and makes real provider requests. It
disables retries for a bounded test, operates in an isolated fixture workspace,
and only approves the exact fixture read/write or explicit local echo command.

- Real streamed model reply, including text-delta events.
- Real model read of a previously unknown file value, approved write to a
  second file, and tool-result continuation.
- Process exit/restart and real model recall of the initial conversation.
- Explicit local `!echo` result with host approval.

All four passed before the display-only repair. The initial local receipt is
`~/.local/share/zenpi-dev/test-runs/live-5bo4__74/report.json`.
Initial binary SHA-256:
`116437f8d6e2ef13d6eba55824a75a87f1d7e192ddf4ec215bbd1fb8ddbe8548`.

The real TUI run uncovered intraword underscores being incorrectly rendered
as emphasis. The renderer now preserves identifiers and paths such as
`ZENPI_TUI_a522b536f7`, `src/my_file_name.rs`, and `foo__bar__baz`, while
retaining explicit emphasis. Eight focused Markdown/TUI tests and library
Clippy passed. A real PTY with terminal-cell emulation verified the persisted
real model response displays exactly, then exits with terminal restoration.
The earlier renderer-corrected binary SHA-256 was
`48d96a0f0a8ea4075ba635fe318547ff75303cdcc79c1402b967347d41e234d4`.
All four live-provider scenarios also passed again on that corrected binary;
the final receipt is
`~/.local/share/zenpi-dev/test-runs/live-36fqv0xc/report.json`.

The earlier upstream 502 did not recur in these live scenarios. That is
current executable evidence, not a claim about permanent service availability.

## Current TUI development revision

The current installed binary SHA-256 is
`3b7c388defc6512b9597a99c9533069eca763cb177ba52af0d1ad48689e6db26`.
All four live-provider checks passed again on this exact binary; receipt:
`~/.local/share/zenpi-dev/test-runs/live-b1uszg0v/report.json`.
The source Codex configuration/auth checksums remain unchanged.

- Conversation uses colored dots instead of speaker-name prefixes.
  `/persona` (alias `/personas`) selects and journals one of 16 MBTI writing
  preferences. The selected persona determines assistant-dot color.
- The prompt displays a slash palette with keyboard/mouse selection and
  secondary choices for persona, configured models, Goal, Learn, and sessions.
- Workspace names replace numbered tab labels and can be clicked. Pane focus
  and scroll positions are independent; divider dragging persists layout
  preferences. Column dragging preserves the uninvolved column ratio.
- The full all-target/all-feature test run passed before the final adjacent
  column drag correction. After that correction, all 23 layout/persistence/TUI
  interaction tests and strict all-target/all-feature Clippy passed.
- A real PTY on the installed binary verified palette rendering, persisted
  INTP selection, the paired model choice, mouse-event handling, 70x16 resize,
  and clean exit. Its journal is
  `~/.local/share/zenpi-dev/test-runs/tui-live-gm_97uav/session.jsonl`.
  Mouse geometry assertions are unit-test evidence, not a PTY visual assertion.

This is an interaction preview, not acceptance of the full TUI redesign.
The two independently focusable panes do not yet establish separate Goal and
Conversation execution contexts. The top navigation's semantic redesign and
matching the requested herdr reference remain open. Historical assistant dots
currently use the selected persona, not per-message historical persona data.

## Development limitations

Manual chat, tool approval, local shell, and saved conversations are the
functional-test surface. Fully automated Blueprint worker execution and live
recipient orchestration remain incomplete and do not block this development
build. Cross-platform release acceptance, portable reconnect-sidecar lifecycle,
and universal cancellation during DNS/connect/header waits remain open.
