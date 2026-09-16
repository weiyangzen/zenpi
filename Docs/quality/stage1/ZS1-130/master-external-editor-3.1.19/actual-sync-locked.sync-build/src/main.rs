
use std::{fs, path::PathBuf};
use zenpi::tui::{run_with_state, TuiConfig, TuiState};
fn main() {
    let destination = PathBuf::from(std::env::args_os().nth(1).unwrap());
    let mut state = TuiState::default();
    state.set_input("SYNC_SEED");
    run_with_state(TuiConfig::default(), state, move |text, state| {
        fs::write(&destination, text)?;
        state.set_status("SYNC_SUBMITTED");
        Ok::<(), std::io::Error>(())
    }).unwrap();
}
