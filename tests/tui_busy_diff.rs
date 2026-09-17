use std::{fs, path::Path, process::Command};

use tempfile::tempdir;
use zenpi::{
    slash,
    tui::{MessageRole, ProjectTabMetadata, TuiState, dispatch_slash_command},
};

fn metadata(root: &Path) -> ProjectTabMetadata {
    ProjectTabMetadata {
        cwd: root.canonicalize().unwrap().display().to_string(),
        session_path: None,
        approval_mode: Default::default(),
        style: None,
    }
}

fn repository(root: &Path) {
    fs::create_dir(root).unwrap();
    let output = Command::new("git")
        .args(["init", "--quiet"])
        .arg(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn busy_palette_keeps_local_diff_and_review_available() {
    let mut state = TuiState::default();
    state.set_busy(true);
    for command in ["/diff", "/review", "/diff note.txt", "/review note.txt"] {
        assert!(
            state.command_available(command),
            "local inspection unavailable while busy: {command}"
        );
    }
    assert!(!state.command_available("/attach note.txt"));
    assert!(!state.command_available("/model"));
}

#[test]
fn unborrowed_agent_diff_stays_in_selected_project_and_preserves_draft() {
    let root = tempdir().unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    repository(&first);
    repository(&second);
    let name = format!(
        "busy-diff-{}.txt",
        root.path().file_name().unwrap().to_string_lossy()
    );
    assert!(!std::env::current_dir().unwrap().join(&name).exists());
    fs::write(first.join(&name), "FIRST_PROJECT_ONLY\n").unwrap();
    fs::write(second.join(&name), "SECOND_PROJECT_ONLY\n").unwrap();
    for alias in ["diff", "review"] {
        let mut state = TuiState::default();
        state.set_active_project_metadata(metadata(&first));
        assert!(state.open_project_tab("second"));
        state.set_active_project_metadata(metadata(&second));
        state.set_busy(true);
        state.set_input("draft remains 中文");
        let layout = state.active_project_workspace().clone();
        let selected = state.active_project().to_owned();
        let command = slash::parse(&format!("/{alias} {name}")).unwrap().unwrap();
        // The host cannot borrow the Agent while its provider owns the lock.
        dispatch_slash_command(command, &mut state, None);
        let last = state.messages().last().unwrap();
        assert_eq!(last.role, MessageRole::System, "{}", last.text);
        assert!(last.text.contains("+SECOND_PROJECT_ONLY"), "{}", last.text);
        assert!(!last.text.contains("FIRST_PROJECT_ONLY"));
        assert_eq!(state.active_project(), selected);
        assert_eq!(state.active_project_workspace(), &layout);
        assert_eq!(state.input(), "draft remains 中文");
        assert_eq!(
            fs::read_to_string(first.join(&name)).unwrap(),
            "FIRST_PROJECT_ONLY\n"
        );
        assert_eq!(
            fs::read_to_string(second.join(&name)).unwrap(),
            "SECOND_PROJECT_ONLY\n"
        );
    }
}

#[test]
fn local_diff_rejects_parent_escape_without_changing_project_or_draft() {
    let root = tempdir().unwrap();
    let workspace = root.path().join("project");
    repository(&workspace);
    fs::write(root.path().join("outside.txt"), "OUTSIDE_PROJECT\n").unwrap();
    let mut state = TuiState::default();
    state.set_active_project_metadata(metadata(&workspace));
    state.set_busy(true);
    state.set_input("keep draft");
    let selected = state.active_project().to_owned();
    let layout = state.active_project_workspace().clone();
    let command = slash::parse("/diff ../outside.txt").unwrap().unwrap();
    dispatch_slash_command(command, &mut state, None);
    let last = state.messages().last().unwrap();
    assert_eq!(last.role, MessageRole::Error);
    assert!(!last.text.contains("OUTSIDE_PROJECT"));
    assert_eq!(state.active_project(), selected);
    assert_eq!(state.active_project_workspace(), &layout);
    assert_eq!(state.input(), "keep draft");
    assert_eq!(
        fs::read_to_string(root.path().join("outside.txt")).unwrap(),
        "OUTSIDE_PROJECT\n"
    );
}
