# zenpi Architecture Decision

Status: frozen design derived from the pi-agent, b3ehive, and Rust deslop
audits. The implementation root is `/Users/mac/Github/zenpi`; the normative
execution policy is [`../Zenpi_Execution_Spec.md`](../Zenpi_Execution_Spec.md).

## Ownership map

| Module | Sole responsibility | Must not own |
|---|---|---|
| `src/main.rs` and `src/lib.rs` | Parse process options and dispatch one selected mode | Session formats, provider protocol, or a hidden mode |
| `src/error.rs` | Top-level typed error conversion and display | String-based state decisions |
| `src/core.rs` | One turn state machine and mode-independent orchestration | Terminal control sequences or JSONL framing |
| `src/backend.rs` | Provider-independent completion trait and adapters (`echo` first) | Session writes, CLI dispatch, or UI rendering |
| `src/session.rs` | Append-only journal, sequence/recovery, sole-owner append and sync | Backend selection or terminal output |
| `src/protocol.rs` | Headless request/response types, validation, and LF frame codec | Reading stdin or invoking a backend |
| `src/headless.rs` | Stdio loop and response ordering over shared owners | A second core, server, broker, or daemon |
| `src/b3.rs` | Bounded handoff, budget/lease, gate, route, evidence, estimator, and feedback records | Worker scheduling, nested-agent spawning, or automatic optimization |
| `src/tui.rs` | Interactive input, view state, Ratatui/crossterm rendering, and cleanup guard | JSONL framing or a duplicate turn/session implementation |

## Data flow

The binary validates `--mode` before opening a session or backend. Both public
modes construct the same core, session, and backend objects. A prompt enters
the typed turn state machine, is durably recorded, and is sent through the
backend trait. The assistant result is recorded before a completion response or
TUI update is emitted. A handoff is validated by `b3`, appended and synced by
the session owner, and can be consumed by another process without a broker.
Errors carry a stable variant and preserve the last valid durable prefix.

## Public boundary

The mode set is exactly `tui` and `headless`. Headless requests are LF-delimited
JSON objects with request IDs and the commands `prompt`, `steer`, `status`,
`handoff`, `resume`, and `shutdown`. Stdout contains protocol records only;
diagnostics go to stderr. TUI owns human-readable control sequences and never
pretends to be a machine protocol. `print`, `json`, `rpc`, `server`, `daemon`,
and unknown values are rejected before side effects.

## TUI rendering decision

Use one Ratatui in-memory double buffer with the crossterm backend. A bounded
render scheduler coalesces updates at about 16 ms, compares the previous and
current buffers, and writes only changed cells/rows. A width or height change
invalidates the buffer and causes one full redraw; dimensions are clamped and
text wrapping uses Unicode display width. The provider callback is synchronous
in this release, while the terminal guard restores raw mode, cursor visibility,
alternate screen, and bracketed paste on success, error, interrupt, and EOF.

## Persistence and handoff limits

Session records are versioned LF-JSON with a session ID and monotonic sequence.
The recovery path accepts a valid prefix, warns on malformed or out-of-order
records, and treats a truncated final line as recoverable; it never rewrites
earlier records. Handoff and evidence records are bounded, digestable, and
artifact-reference-only. Artifact absolute paths,
traversal, secrets, binary blobs, oversized fields, invalid IDs, expired leases,
and over-budget updates fail without mutation.

## Resource and dependency decisions

The synchronous core avoids an async runtime for the standalone case. A single
HTTP adapter may be compiled for an OpenAI-compatible endpoint; it is behind
the backend trait and never changes mode semantics. Ratatui plus crossterm is
the sole UI stack. No scheduler, persistent pool, plugin host, remote queue,
dashboard, or competing protocol is linked into the binary. The physical Rust
source budget is 5,000 lines across production and tests; generated/build
artifacts are excluded.

## Verification seams

Tests are organized around ownership seams: core/session state and recovery,
headless framing and stdio separation, b3 record validation and digest, TUI
resize/diff/cleanup, and executable mode rejection. Each seam has success and
refusal cases and can run with the deterministic echo backend. This keeps a
single-agent process cheap while giving an embedding b3ehive host durable
handoff and accounting evidence.
