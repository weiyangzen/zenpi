# `src/headless.rs` per-file learn report (ZS1-084)

- Source: `src/headless.rs`
- Complete read ranges: byte `[0,262144)` and `[262144,370207)`; 9,334 lines
- Bytes: `370207`
- SHA-256: `bf252fec95a859b26f9aea402c918e9efbf6c2c55d3e24b403831f86d80ec06e`

## JSONL transport and replay

`run_headless`, `run_stdio`, and the frame reader implement bounded JSONL transport. Frames are consumed incrementally, reject overlong or invalid UTF-8 input, preserve final unterminated JSONL frames, and never allocate in proportion to an attacker-controlled line. `ReplayState` journals owner epochs, request reservations, terminal responses, event sequences, acknowledgements, and per-request project context. Fingerprints normalize aliases and runtime-intent projections; replay, conflict, in-flight duplicate, unknown-outcome, and expired-replay cases are explicit and side-effect safe.

Event and terminal mailboxes have independent count and byte caps. FIFO eviction reports replay gaps, terminal replay remains bounded, and session switching reopens the target journal while preserving pending request identity. EOF drains normally; explicit shutdown uses a short cancellation grace period. Live-owner registration and durable reconnect markers keep session ownership observable across restarts.

## Async streams and steering

The async host separates reader, agent, provider, approval, input, and output paths with bounded mailboxes. Admission markers are prioritized over ordinary events, oversized provider/agent events are counted and dropped, and dropped-byte receipts are emitted. Pending steer requests are bounded, cancellable, and reissued with supersession identity. Runtime lifecycle, tool output, approval, user-shell, tree, resource-control, and retryable error responses are projected into protocol-safe blocks without unbounded buffering.

## Workspace, session, and domain control plane

Read-only resource and domain projections are workspace-relative, bounded, and path-safe. Explicit inspection paths reject absolute paths, parent traversal, control characters, symlinks, and escapes. Blueprint and domain-store validation reads bounded files without creating or replacing them; serialized records are capped deterministically and redact absolute store paths. Session list/search/open/resume/recovery/GC/compact operations enforce idle ownership, clean journal invariants, confirmation, retention bounds, replay cursors, and redacted receipts. Mailbox, checkpoint, goal, blueprint, learn, evidence, and external-manifest commands share the same host-side owner and validation boundaries.

## Command dispatch and boundaries

The command dispatcher handles slash actions, runtime intents, session lifecycle, recovery, approvals, mailbox operations, goal transitions, blueprint execution/handoff, learn evidence import, and external evidence actions. Request admission occurs before lifecycle work, durable decisions are checked against prior journal entries, and errors are typed with stable protocol codes. The module owns transport and host coordination only: it does not itself become a provider, grant credentials, or bypass the core/session/domain authorization gates. TUI adapters call the bounded view helpers rather than duplicating headless state transitions.
