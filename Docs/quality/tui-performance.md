# TUI Performance and Resize Gate

This is a reproducible renderer gate, not a claim about provider latency. The
release uses Ratatui's single `TestBackend`/terminal buffer and a 16 ms dirty
scheduler. Production provider work runs on the owned background runtime, so
input, resize, approval, cancellation, and streaming rendering remain live
while a remote request is active.

## Checks

Run from the repository root:

```text
cargo test --test tui_resize -- --nocapture
cargo test --all-targets
```

The integration test renders 4x3, 1x1, and 0x0 buffers with wide Unicode text,
then sends 100 dirty notifications and verifies that the scheduler is not due
again until its 10 ms deadline. The same invariants are exercised by the
library's bounded `TuiState` and `RenderScheduler` APIs; a separate test proves
adjacent local stream chunks coalesce. No sleep or wall-clock
p95 assertion is used: the test checks the deterministic deadline relation and
the terminal backend reports panics directly.

## Acceptance evidence

On the final validation host, `cargo test --test tui_resize -- --nocapture`
passes all three tests. `TuiState::render` invalidates its cached transcript on a
dimension change, clamps zero/one-cell areas, and lets Ratatui perform the
buffer diff. `TerminalGuard` owns raw mode, alternate screen, cursor, and
bracketed-paste cleanup; its `Drop` path is used on errors and normal exits.
PTY smoke runs of the installed release binary cover resize, a streamed
Responses delta, cancellation while streaming, a deterministic local prompt,
exit status 0, and alternate-screen/raw-mode restoration sequences.
