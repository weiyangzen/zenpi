# pi-agent TypeScript Migration Notes

Status: observed evidence and a frozen migration decision for zenpi. This note
does not copy upstream source. The evidence was inspected locally on 2026-09-03
at revision `23282f60782f02b9e22b787e4b22af441454fa16` in
`/Users/mac/Github/pi-mono`.

## Evidence map

The links below are immutable GitHub views of the pinned revision. Local line
references are retained so a reviewer can reproduce the mapping without
copying upstream source.

| Upstream evidence | Observation | zenpi decision |
|---|---|---|
| [`cli/args.ts:10,56-73`](https://github.com/badlogic/pi-mono/blob/23282f60782f02b9e22b787e4b22af441454fa16/packages/coding-agent/src/cli/args.ts#L10-L73) | The parser names `text`, `json`, and `rpc` modes and validates the value before dispatch. | Use typed `RunMode::{Tui,Headless}`; reject every other value before side effects. |
| [`main.ts:749-767,781-823`](https://github.com/badlogic/pi-mono/blob/23282f60782f02b9e22b787e4b22af441454fa16/packages/coding-agent/src/main.ts#L749-L823) | Mode selection and one session-manager creation precede the mode loop. | Keep one core/session owner and validate transport before opening a session. |
| [`main.ts:848-895`](https://github.com/badlogic/pi-mono/blob/23282f60782f02b9e22b787e4b22af441454fa16/packages/coding-agent/src/main.ts#L848-L895) | Interactive, RPC, and print paths have different output assumptions. | Preserve interactive behavior as TUI; make headless strict JSONL stdio, not print/RPC/server. |
| [`session-manager.ts:27-80,200-280`](https://github.com/badlogic/pi-mono/blob/23282f60782f02b9e22b787e4b22af441fa16/packages/coding-agent/src/core/session-manager.ts#L27-L80) | Headers, JSONL records, migrations, append, and resume are session-manager responsibilities. | `src/session.rs` is the sole journal owner; compatibility stays at this boundary. |
| [`jsonl.ts:4-19,21-57`](https://github.com/badlogic/pi-mono/blob/23282f60782f02b9e22b787e4b22af441fa16/packages/coding-agent/src/modes/rpc/jsonl.ts#L4-L57) | Framing is LF-only; readline is avoided because U+2028/U+2029 may be payload. | Use bounded `BufRead` LF framing and keep stdout machine-readable. |
| [`rpc-mode.ts:42-66`](https://github.com/badlogic/pi-mono/blob/23282f60782f02b9e22b787e4b22af441fa16/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L42-L66) | A long-lived loop correlates responses by request ID and emits events. | Retain correlation and typed events, while omitting extension-UI and a second RPC/server surface. |
| [`terminal.ts:52-103,249-290`](https://github.com/badlogic/pi-mono/blob/23282f60782f02b9e22b787e4b22af441fa16/packages/tui/src/terminal.ts#L52-L103) | Raw mode, paste mode, resize listeners, cursor state, and teardown are explicit lifecycle operations. | Give `src/tui.rs` one owned cleanup guard and test success/error/interrupt/EOF paths. |
| [`tui.ts:464-480`](https://github.com/badlogic/pi-mono/blob/23282f60782f02b9e22b787e4b22af441fa16/packages/tui/src/tui.ts#L464-L480) | Render requests are coalesced on the next tick. | Use a bounded scheduler instead of rendering each notification. |
| [`tui.ts:873-958`](https://github.com/badlogic/pi-mono/blob/23282f60782f02b9e22b787e4b22af441fa16/packages/tui/src/tui.ts#L873-L958) | Viewport dimensions are tracked, old/new lines are compared, and width changes force a full redraw. | Use one Ratatui double buffer, Unicode display-width wrapping, one resize redraw, then diffs. |

## Preserved behavior matrix

| Behavior | Evidence and acceptance implication |
|---|---|
| Turn admission and steering | `main.ts:754-782`; typed Rust refusals leave the backend and journal untouched. |
| Session resume | `session-manager.ts:27-80,200-280`; complete records remain readable after a malformed or truncated tail and a warning is retained. |
| Event-driven input and shutdown | `rpc-mode.ts:42-66`; EOF, shutdown, and interrupt close the owned process without duplicate completion. |
| Terminal restoration | `terminal.ts:52-103,249-290`; raw mode, cursor, alternate screen, and paste mode are restored by one guard. |
| Coalesced rendering | `tui.ts:464-480,873-958`; rapid updates are coalesced and unchanged cells are not redrawn by the view layer. |

## Deliberate omissions

zenpi does not carry upstream print mode, RPC mode, HTTP/server mode, daemon
mode, plugin marketplace, image protocol, or a second transport. It does not
promise byte-for-byte TypeScript session compatibility; any future adapter must
live at the persistence boundary and have a fixture test.

## Migration risks and tests

The high-risk differences are terminal teardown during errors, resize while a
completion is in progress, Unicode line-separator framing, and a partial final
journal line. The provider adapter is synchronous in this small release, so a
network call can delay input handling; the renderer remains bounded and
resize-safe. Tests control framing and terminal interleavings rather than rely
on sleeps. A preserved behavior requires both success and refusal evidence.

Related decisions: [`b3ehive-occam.md`](b3ehive-occam.md),
[`codex-rust-deslop.md`](codex-rust-deslop.md), and
[`zenpi-architecture.md`](zenpi-architecture.md). The executable contract is
[`../Zenpi_Execution_Spec.md`](../Zenpi_Execution_Spec.md).
