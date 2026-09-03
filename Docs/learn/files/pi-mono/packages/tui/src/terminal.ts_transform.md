# Transform Note: `pi-mono/packages/tui/src/terminal.ts`

source_path: `pi-mono/packages/tui/src/terminal.ts`
source_hash: `3b006ac13d7674f2eb64108df8938a3b5092c15cfbc358d0b7e6592a160d01d6`

The source makes raw mode, bracketed paste, resize callbacks, cursor state,
and teardown explicit. zenpi maps these responsibilities to one
`TerminalGuard` in `tui.rs`; `Drop` restores raw mode, alternate screen,
cursor, and paste state even when rendering or the callback returns an error.
The guard has no relationship to headless framing.
