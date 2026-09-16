# `src` directory integration learn report (ZS1-090)

- Folder: `src`
- Scope: 22 accepted Stage 1 file owners ZS1-070..088 plus ZS1-094..096
- Integration layer: L2; per-file reports remain authoritative

## Ownership and flow

The source tree separates durable domain/session/governance state, provider/runtime transport, resource and slash loading, and interactive TUI projection. `core::Agent` and `SessionStore` are the durable owner boundary; headless JSONL and TUI are hosts over the same validated state. Provider events normalize through `view_model` before either host renders them.

A TUI project intent canonicalizes the selected directory, binds tab label/cwd/session owner/tool root, and preserves per-project drafts, transcripts, approvals, layouts, cursors, and runtime metadata. The top `+` directory picker is therefore directory-first while BentoBox remains the pane pattern. Headless replay, slash actions, resource loaders, domain stores, budgets, approvals, recovery, and execution receipts join around the same owner identity and bounded path/event rules.

## Integration limits

`src` integration composes accepted child contracts; it does not replace their per-file evidence or itself spawn workers, call providers, grant credentials, or own layout side effects. Existing project-workspace, BentoBox, composer, interaction, and headless tests provide runtime evidence for the cross-module paths.
