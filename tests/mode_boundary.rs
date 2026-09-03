use std::process::Command;
use tempfile::tempdir;
use zenpi::core::{RunMode, parse_args};

#[test]
fn parser_has_exactly_two_modes() {
    assert!(matches!(parse_args(["--mode", "tui"]), Ok(options) if options.mode == RunMode::Tui));
    assert!(
        matches!(parse_args(["--mode", "headless"]), Ok(options) if options.mode == RunMode::Headless)
    );
    for mode in ["print", "json", "rpc", "server", "daemon", "other"] {
        assert!(parse_args(["--mode", mode]).is_err(), "accepted {mode}");
    }
}

#[test]
fn executable_rejects_mode_before_session_creation() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("must-not-exist.jsonl");
    let result = Command::new(env!("CARGO_BIN_EXE_zenpi"))
        .args(["--mode", "rpc", "--session"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!path.exists());
}
