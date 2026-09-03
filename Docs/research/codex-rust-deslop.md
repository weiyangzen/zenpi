# codex-rust-deslop Practices Adopted by zenpi

Status: evidence-backed Rust quality contract. The local source is
`/Users/mac/Downloads/codex-rust-deslop` at revision
`648ca87e2add56a12081221c3e6f51e29fe04fbd`, report
`codex-0.148-rust-refactor-report.zh-CN.md`, SHA-256
`e92528d14bd26369ac7b7fbfc1edc87fa10fe3cbd8f22876a25d4c3cf5261be2`.
The corresponding source repository is [`weiyangzen/codex-rust-deslop`](https://github.com/weiyangzen/codex-rust-deslop).

## Observed practices

The report's structural examples and executable checklist support these
principles:

1. Give each concept one owner, one entry point, one state machine, and one
   safe failure path. Migrate callers and tests before deleting a legacy path.
2. Express legal states and refusal reasons in Rust types rather than strings;
   make ownership visible by moving or borrowing values instead of cloning
   large enums or wrapping everything in shared locks.
3. Optimize the data flow only after preserving failure semantics. Borrow JSON,
   use `Cow` where it removes a measured allocation, and avoid an optimization
   that can contaminate an old snapshot on an invalid patch.
4. Keep compatibility conversion at persistence/protocol boundaries. Preserve
   old data with explicit migration/fixture tests instead of weakening every
   domain type.
5. Treat sandbox, process, terminal, and resource cleanup as contracts. Unknown
   or unsafe conditions fail closed; child processes are reaped and terminal
   state is restored.
6. Test interleavings, refusal branches, cancellation, malformed/old formats,
   and resource ownership, not only successful output. A formatter or a single
   Clippy run is not architectural evidence.

The report sections on skills ownership, turn-input state machines, move-only
submission, borrowed JSON patches, settings snapshots, sandbox refusal, child
process waiting, and typed non-retryable provider errors are the primary
evidence points. The final checklist recommends a human-defined contract,
incremental migration, and deletion only after replacement tests pass.

## zenpi mapping

| Practice | zenpi rule | Gate/evidence |
|---|---|---|
| Single owner | Core, session, protocol, headless, TUI, and b3 each have one owner | Architecture note and ownership review |
| Typed state/errors | `TurnState`, mode enum, command enum, and stable error variants | Core/protocol tests include invalid transitions |
| Move/borrow discipline | Requests move payloads into core; clones/locks require a reason | Clippy plus code review scan |
| Boundary compatibility | Session recovery and optional backend adapters isolate formats | Round-trip, truncated-tail, and fixture tests |
| Fail closed | Invalid frame, path, digest, lease, terminal size, or backend response has no side effect | Headless, b3, and TUI negative tests |
| Explicit lifecycle | Terminal guard, file writes, cancellation, and EOF flush have owners | Cleanup and drop tests |
| Failure-first testing | Interleaving, resize, malformed input, and resource-limit cases are required | Blueprint V1 rows |
| Delete redundancy | No print/RPC/server/daemon mode, duplicate UI framework, or second scheduler | Executable mode/dependency scan |

## Non-goals

This note does not claim that Rust automatically improves architecture, that a
line count proves quality, or that the upstream report is a security audit.
Human review remains required for deletion, compatibility, concurrency
linearization, provider failures, and publication side effects. The practices
are guardrails for zenpi's small scope, not permission to add abstractions.

Related contract: [`../Zenpi_Execution_Spec.md`](../Zenpi_Execution_Spec.md).
