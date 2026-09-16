# ZS1-125 master verification checkpoint

The candidate patch was applied to the main worktree and rechecked with the native stable-aarch64-apple-darwin toolchain.

- `approval_cap`: 9 passed, 0 failed
- `tui_approval_focus`: 13 passed, 0 failed
- `tui_preview_graphemes`: 3 passed, 0 failed
- `tui_approval_smoke.py`: 14 production PTY checks passed, 12 requests, terminal restored
- `cargo check --locked`: passed
- `cargo clippy --locked --lib -- -D warnings`: passed; only the existing vendored crossterm warning remains

The patch is integrated, but this checkpoint does not promote the full blueprint item: dependency closure and the remaining product/release evidence still require independent master review.
