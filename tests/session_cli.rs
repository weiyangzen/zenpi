use std::{fs, process::Command};

use tempfile::tempdir;
use zenpi::{
    core::{Turn, TurnRole},
    session::SessionStore,
};

fn zenpi(home: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_zenpi"));
    command.env("HOME", home).env_remove("ZENPI_HOME");
    command
}

#[test]
fn session_cli_lists_inspects_forks_exports_and_imports() {
    let home = tempdir().unwrap();
    let sessions = home.path().join(".zenpi/sessions");
    fs::create_dir_all(&sessions).unwrap();
    let source = sessions.join("source.jsonl");
    let mut store = SessionStore::open(&source).unwrap();
    store
        .append_turn(Turn::new("user-1", TurnRole::User, "hello"))
        .unwrap();

    let output = zenpi(home.path())
        .args(["session", "list", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let listed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["session_id"], store.session_id());

    let output = zenpi(home.path())
        .args(["session", "inspect", source.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let inspected: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(inspected["turns"][0]["content"], "hello");

    let fork = sessions.join("fork.jsonl");
    assert!(
        zenpi(home.path())
            .args([
                "session",
                "fork",
                source.to_str().unwrap(),
                fork.to_str().unwrap(),
            ])
            .status()
            .unwrap()
            .success()
    );
    let forked = SessionStore::open(&fork).unwrap();
    assert_ne!(forked.session_id(), store.session_id());
    assert_eq!(forked.turns(), store.turns());

    let export = home.path().join("export.jsonl");
    assert!(
        zenpi(home.path())
            .args([
                "session",
                "export",
                source.to_str().unwrap(),
                export.to_str().unwrap(),
            ])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(fs::read(&export).unwrap(), fs::read(&source).unwrap());

    let imported = home.path().join("import.jsonl");
    assert!(
        zenpi(home.path())
            .args([
                "session",
                "import",
                export.to_str().unwrap(),
                imported.to_str().unwrap(),
            ])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(fs::read(&imported).unwrap(), fs::read(&source).unwrap());
}

#[test]
fn session_gc_requires_complete_confirmation_and_retention_policy() {
    let home = tempdir().unwrap();
    assert!(
        !zenpi(home.path())
            .args(["session", "gc", "--yes"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        !zenpi(home.path())
            .args([
                "session",
                "gc",
                "--retain-newest",
                "0",
                "--older-than-seconds",
                "1",
            ])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn inspect_missing_path_does_not_create_a_session() {
    let home = tempdir().unwrap();
    let missing = home.path().join("missing.jsonl");
    assert!(
        !zenpi(home.path())
            .args(["session", "inspect", missing.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    assert!(!missing.exists());
}
