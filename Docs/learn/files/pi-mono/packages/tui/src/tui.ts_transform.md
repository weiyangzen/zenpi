# Transform Note: `pi-mono/packages/tui/src/tui.ts`

source_path: `pi-mono/packages/tui/src/tui.ts`
source_hash: `c66359caeee468529dae65905401d67b7ba3dfaaae19a145e94bae7601bb1d31`

The source coalesces render requests, tracks dimensions, performs differential
updates, and forces a safe redraw when wrapping changes. zenpi uses one
Ratatui double buffer with `RenderScheduler`, bounded transcript lines, and
Unicode display-width wrapping. A resize invalidates layout and the next
frame uses the current area; unchanged bursts do not trigger extra frames.
`TuiState` and `TestBackend` tests cover narrow and one-cell areas.
