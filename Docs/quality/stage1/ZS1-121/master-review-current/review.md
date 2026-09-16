# ZS1-121 current master review

Verified the directory-first top `+` picker and real project tab wiring in `src/tui.rs`, `src/directory_picker.rs`, `src/project_workspace.rs`, and the TUI workspace tests. The targeted suite passed 16/16, including `topmost_plus_opens_picker_and_cancel_never_creates_a_tab_or_changes_draft`, picker confirmation with duplicate basenames, real tool binding, restart restore, cwd/session mismatch protection, and project checkpoint atomicity. The existing PTY smoke path remains the required production check for this item.
