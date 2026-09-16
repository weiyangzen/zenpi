# `src/tui.rs` per-file learn report (ZS1-085)

- Source: `src/tui.rs`
- Complete read ranges: byte `[0,262144)`, `[262144,524288)`, and `[524288,681440)`; 16,652 lines
- Bytes: `681440`
- SHA-256: `83f5aa5b91f83cdcde80cc43936baafe8c681eb3f885b4fe9b213affa0217681`

## Interaction model and BentoBox preservation

`TuiState` remains the central interactive projection: messages, streaming jobs, reasoning, approvals, tool logs, resource/gantt panes, session browser, input draft, history, completions, model menu, layout focus, and dirty/render scheduling all converge there. `render_bentobox`, `BentoBoxLayoutAdapter`, pane capability checks, workspace split/focus/collapse controls, and per-tab layout persistence keep the existing BentoBox pattern intact while exposing compact transcript, input, tool, replay, resource, gantt, and session panes.

The header/footer and event loop provide the short paths expected from a CLI TUI: keyboard and mouse routing, command completion, slash choices, history search, external editor, multiline editing, grapheme-safe cursor movement, ordinary paste bursts, large-paste folding, clipboard copy, transcript browser, approvals, and explicit busy/idle feedback. Render scheduling coalesces updates without hiding pending approvals or stream completion.

## Project tabs and directory-first creation

Project state is isolated through `ProjectTabMetadata`, per-project session cursors, layouts, drafts, messages, tool logs, approvals, and runtime owner bindings. `open_project_tab`, `select_project_tab`, `next_project_tab`, `close_project_tab`, rename/checkpoint/restore helpers, and project JSON snapshots provide browser-like tabs without sharing the wrong cwd or session owner.

The top `+` path is represented by `open_directory_picker`, picker result handling, `ProjectIntent`, and `ProjectRuntimeHost::apply`. Confirming a directory canonicalizes it, derives the project label/tab identity, attaches the working folder and tool root, creates or resumes the matching session owner, and refreshes the active project context. Picker cancellation leaves the active tab unchanged. This is the minimum-step directory-first flow while retaining BentoBox pane state.

## Composer and human feedback

The composer tracks draft epochs and paste metadata so rejected submissions restore the exact prior folded input without overwriting newer terminal input. It handles scheduled shell rejection, file references/completion, slash command menus, history preview/search, word and line operations, vertical movement, external editing, and checkpoint limits. Streaming and reasoning are job-scoped, so superseded work cannot append into another project. Approval overlays retain per-project queues and background hints; transcript browser and copy affordances expose completed answers without disrupting the active pane.

## Session, runtime, and control-plane adapters

TUI dispatchers route tree, reasoning, slash, goal, mailbox, resource, blueprint, learn, evidence, output, approval, and runtime-intent commands through shared owner functions. `ProjectRuntimeHost` switches/resumes agents, applies directory/session intents, binds agent metadata, persists checkpoints, and rejects unsafe cross-project reuse. Session browser, local diff, resource and gantt refreshers run through bounded background hosts and report errors into the active project.

## Scope and limits

This file is the interactive host and renderer; it does not implement provider transport or independently execute worker side effects. Headless and core owners remain authoritative for durable protocol, authorization, budgets, and execution. The TUI adds ergonomic projections and event routing while preserving those boundaries.
