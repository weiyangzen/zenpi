# `src/view_model.rs` per-file learn report (ZS1-086)

- Source: `src/view_model.rs`
- Complete read range: byte `[0,42009)`; 1,255 lines
- Bytes: `42009`
- SHA-256: `094833f051ab66d30ce3f6cc74fcc0334ed938e79f5fd8e9269341b62bed08fe`

## Renderer-neutral normalized view

The view model is the bounded normalization boundary shared by TUI and headless transports. `ViewRole`, `ViewBlock`, `ViewMessage`, `ViewTurnMode`, `ViewRejection`, `ViewStream`, and `ViewEventKind` represent prompts, markdown blocks, diffs, tool status, approvals, usage, handoffs, warnings, terminal outcomes, and dropped-event markers without Ratatui or provider-specific types.

Blocks and text are validated for size, control characters, identifiers, list counts, heading levels, and serialized event size. `redacted` applies the shared security policy before transport projection. Markdown conversion stays within the supported parser subset and preserves stable block discriminators for renderers and tests.

## Event association and lifecycle

`ViewEvent` carries sequence, request, turn, and block associations. Constructors normalize core/provider events into typed lifecycle data and derive deterministic stream block IDs. The event validator rejects missing associations, invalid terminal payloads, oversized content, and malformed identifiers. Terminal turn states are explicit; post-terminal deltas fail closed.

## Bounded replay buffer

`ViewEventBuffer` enforces sequence continuity, capacity and byte budgets, tracks dropped events, supports drain and replay cursors, and closes permanently on a `Closed` event. Replay reports future and gap errors rather than silently inventing history. A bounded terminal-turn set prevents stale retries from reopening completed work while allowing old terminal markers to age out deterministically.

## Integration boundary

This module lets TUI and headless hosts render the same ordered event semantics and loss signals. It does not own provider I/O, terminal layout, sessions, credentials, or execution; those remain in their respective owners. The model is therefore a compatibility and safety boundary rather than a second runtime.
