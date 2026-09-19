use std::{fs, path::Path};
use tempfile::tempdir;
use zenpi::project_workspace::{
    MAX_PROJECT_TABS, MAX_PROJECT_WORKSPACE_BYTES, OpenOutcome, ProjectWorkspace,
    ProjectWorkspaceError, normalize_directory,
};

#[test]
fn confirmation_prepares_a_directory_bound_candidate_without_mutating_current_state() {
    let root = tempdir().unwrap();
    let folder = root.path().join("工作 folder");
    fs::create_dir(&folder).unwrap();
    let current = ProjectWorkspace::default();
    let process_cwd = std::env::current_dir().unwrap();
    let (candidate, outcome) = current
        .with_directory(Some(Path::new("工作 folder")), root.path())
        .unwrap();
    let tab = candidate.active().unwrap();
    assert_eq!(outcome, OpenOutcome::Opened(tab.id().clone()));
    assert_eq!(tab.cwd(), folder.canonicalize().unwrap());
    assert_eq!(tab.title(), "工作 folder");
    assert_eq!(tab.id().as_str().len(), 64);
    assert!(current.tabs().is_empty());
    assert!(current.active().is_none());
    assert_eq!(std::env::current_dir().unwrap(), process_cwd);
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 0);
}

#[test]
fn cancellation_errors_and_failed_host_preparation_leave_selection_and_checkpoint_unchanged() {
    let root = tempdir().unwrap();
    let (current, _) = ProjectWorkspace::default()
        .with_directory(Some(root.path()), root.path())
        .unwrap();
    let before = current.to_json_bytes().unwrap();
    let (cancelled, outcome) = current
        .with_directory(None, Path::new("irrelevant"))
        .unwrap();
    assert_eq!(outcome, OpenOutcome::Cancelled);
    assert_eq!(cancelled, current);
    fs::write(root.path().join("file"), "not a directory").unwrap();
    for invalid in ["", "missing", "file", "bad\npath"] {
        assert!(
            current
                .with_directory(Some(Path::new(invalid)), root.path())
                .is_err()
        );
        assert_eq!(current.to_json_bytes().unwrap(), before);
    }
    assert!(normalize_directory(Path::new("."), Path::new("relative")).is_err());
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    let (prepared, _) = current.with_directory(Some(&other), root.path()).unwrap();
    assert_ne!(prepared, current);
    // A host can discard this candidate if opening its session/config fails.
    drop(prepared);
    assert_eq!(current.to_json_bytes().unwrap(), before);
}

#[test]
fn duplicate_basenames_remain_distinct_and_aliases_reselect_existing_project() {
    let root = tempdir().unwrap();
    let first = root.path().join("one/api");
    let second = root.path().join("two/api");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    let (current, _) = ProjectWorkspace::default()
        .with_directory(Some(&first), root.path())
        .unwrap();
    let first_id = current.active().unwrap().id().clone();
    let (current, _) = current.with_directory(Some(&second), root.path()).unwrap();
    assert_eq!(current.tabs().len(), 2);
    assert_eq!(current.tabs()[0].title(), current.tabs()[1].title());
    assert_ne!(first_id, *current.active().unwrap().id());
    let (current, outcome) = current
        .with_directory(Some(&first.join("../api/.")), root.path())
        .unwrap();
    assert_eq!(outcome, OpenOutcome::Selected(first_id.clone()));
    assert_eq!(current.active().unwrap().id(), &first_id);
    assert_eq!(current.tabs().len(), 2);
    #[cfg(unix)]
    {
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&first, &alias).unwrap();
        let (via_alias, outcome) = current.with_directory(Some(&alias), root.path()).unwrap();
        assert_eq!(outcome, OpenOutcome::Selected(first_id));
        assert_eq!(via_alias, current);
    }
}

#[test]
fn checkpoint_roundtrip_preserves_stable_identity_and_selected_order() {
    let root = tempdir().unwrap();
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    let (current, _) = ProjectWorkspace::default()
        .with_directory(Some(root.path()), root.path())
        .unwrap();
    let first_id = current.active().unwrap().id().clone();
    let (current, _) = current.with_directory(Some(&other), root.path()).unwrap();
    let current = current.with_active(&first_id).unwrap();
    let bytes = current.to_json_bytes().unwrap();
    assert_eq!(ProjectWorkspace::from_json_bytes(&bytes).unwrap(), current);
    assert_eq!(current.to_json_bytes().unwrap(), bytes);
    assert_eq!(
        ProjectWorkspace::from_json_bytes(&ProjectWorkspace::default().to_json_bytes().unwrap())
            .unwrap(),
        ProjectWorkspace::default()
    );
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value.get("layout").is_none());
    assert!(value.get("messages").is_none());
}

#[test]
fn invalid_checkpoint_is_rejected_atomically() {
    let root = tempdir().unwrap();
    let (current, _) = ProjectWorkspace::default()
        .with_directory(Some(root.path()), root.path())
        .unwrap();
    let bytes = current.to_json_bytes().unwrap();
    let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for mutation in 0..6 {
        let mut value = original.clone();
        match mutation {
            0 => value["schema_version"] = 999.into(),
            1 => value["active"] = "nonexistent".into(),
            2 => value["tabs"][0]["id"] = "forged".into(),
            3 => value["tabs"][0]["cwd"] = ".".into(),
            4 => {
                let tab = value["tabs"][0].clone();
                value["tabs"].as_array_mut().unwrap().push(tab);
            }
            _ => value["active"] = serde_json::Value::Null,
        }
        assert!(ProjectWorkspace::from_json_bytes(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    assert!(matches!(
        ProjectWorkspace::from_json_bytes(&vec![b' '; MAX_PROJECT_WORKSPACE_BYTES + 1]),
        Err(ProjectWorkspaceError::CheckpointTooLarge)
    ));
    assert_eq!(current.to_json_bytes().unwrap(), bytes);
}

#[test]
fn bounded_tabs_allow_reselection_at_capacity_and_closing_preserves_active_identity() {
    let root = tempdir().unwrap();
    let mut current = ProjectWorkspace::default();
    for index in 0..=MAX_PROJECT_TABS {
        let path = root.path().join(index.to_string());
        fs::create_dir(&path).unwrap();
        let next = current.with_directory(Some(&path), root.path());
        if index == MAX_PROJECT_TABS {
            assert!(matches!(next, Err(ProjectWorkspaceError::Capacity)));
        } else {
            current = next.unwrap().0;
        }
    }
    let active_id = current.active().unwrap().id().clone();
    let first = current.tabs()[0].clone();
    let closed = current.without_project(first.id()).unwrap();
    assert_eq!(closed.active().unwrap().id(), &active_id);
    assert_eq!(closed.tabs().len(), MAX_PROJECT_TABS - 1);
    assert!(closed.with_active(first.id()).is_err());
    let (reselected, outcome) = current
        .with_directory(Some(first.cwd()), root.path())
        .unwrap();
    assert_eq!(outcome, OpenOutcome::Selected(first.id().clone()));
    assert_eq!(reselected.tabs().len(), MAX_PROJECT_TABS);
    let closed_active = reselected.without_project(first.id()).unwrap();
    assert_eq!(closed_active.active().unwrap().id(), current.tabs()[1].id());
}

#[test]
fn last_tab_and_removed_directory_cannot_be_selected_or_restored() {
    let root = tempdir().unwrap();
    let path = root.path().join("removed");
    fs::create_dir(&path).unwrap();
    let (current, _) = ProjectWorkspace::default()
        .with_directory(Some(&path), root.path())
        .unwrap();
    let id = current.active().unwrap().id();
    assert!(matches!(
        current.without_project(id),
        Err(ProjectWorkspaceError::LastProject)
    ));
    let bytes = current.to_json_bytes().unwrap();
    fs::remove_dir(&path).unwrap();
    assert!(current.with_active(id).is_err());
    assert!(ProjectWorkspace::from_json_bytes(&bytes).is_err());
    assert_eq!(current.to_json_bytes().unwrap(), bytes);
}

#[cfg(unix)]
#[test]
fn permission_denied_selection_is_an_io_error_and_leaves_state_unchanged() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempdir().unwrap();
    let (current, _) = ProjectWorkspace::default()
        .with_directory(Some(root.path()), root.path())
        .unwrap();
    let before = current.to_json_bytes().unwrap();
    let locked = root.path().join("locked");
    fs::create_dir_all(locked.join("child")).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    // Privileged or permission-bit-bypassing hosts cannot exercise this path.
    if fs::read_dir(&locked).is_ok() {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let outcome = current.with_directory(Some(Path::new("locked/child")), root.path());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(outcome, Err(ProjectWorkspaceError::Io(_))));
    assert_eq!(current.to_json_bytes().unwrap(), before);
    // Restoring access lets the same selection succeed without touching prior state.
    let (recovered, outcome) = current
        .with_directory(Some(Path::new("locked/child")), root.path())
        .unwrap();
    assert_eq!(
        outcome,
        OpenOutcome::Opened(recovered.active().unwrap().id().clone())
    );
    assert_eq!(recovered.tabs().len(), current.tabs().len() + 1);
    assert_eq!(current.to_json_bytes().unwrap(), before);
}

#[test]
fn checkpoint_restores_in_an_independent_process() {
    let root = tempdir().unwrap();
    let one = root.path().join("one");
    let two = root.path().join("two");
    fs::create_dir_all(&one).unwrap();
    fs::create_dir_all(&two).unwrap();
    let (current, _) = ProjectWorkspace::default()
        .with_directory(Some(&one), root.path())
        .unwrap();
    let first = current.active().unwrap().id().clone();
    let (current, _) = current.with_directory(Some(&two), root.path()).unwrap();
    let current = current.with_active(&first).unwrap();
    let bytes = current.to_json_bytes().unwrap();
    let path = root.path().join("project-workspace.json");
    fs::write(&path, &bytes).unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "project_workspace_restart_child"])
        .env("ZENPI_PROJECT_WORKSPACE_CHECKPOINT", &path)
        .env("ZENPI_PROJECT_WORKSPACE_ACTIVE", first.as_str())
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
}

#[test]
#[ignore = "invoked in a fresh process by checkpoint_restores_in_an_independent_process"]
fn project_workspace_restart_child() {
    let path = std::env::var_os("ZENPI_PROJECT_WORKSPACE_CHECKPOINT").unwrap();
    let bytes = fs::read(path).unwrap();
    let restored = ProjectWorkspace::from_json_bytes(&bytes).unwrap();
    assert_eq!(restored.tabs().len(), 2);
    assert_eq!(
        restored.active().unwrap().id().as_str(),
        std::env::var("ZENPI_PROJECT_WORKSPACE_ACTIVE").unwrap()
    );
    assert!(restored.active().unwrap().cwd().is_dir());
    // A fresh process reserializes byte-identically and re-validates deterministically.
    assert_eq!(restored.to_json_bytes().unwrap(), bytes);
    assert_eq!(
        ProjectWorkspace::from_json_bytes(&restored.to_json_bytes().unwrap()).unwrap(),
        restored
    );
}

#[test]
fn arch_owner_uses_an_independent_journal() {
    use std::sync::{Arc, Mutex};

    let root = tempdir().unwrap();
    let cwd = root.path().canonicalize().unwrap();
    let session = root.path().join("session.jsonl");
    let agent = zenpi::core::Agent::prepare_project_with_options(
        &session,
        &cwd,
        zenpi::config::ConfigOverrides::default(),
        true,
    )
    .unwrap();
    let discussion_path = agent.session().path().to_path_buf();
    let mut pool =
        zenpi::project_workspace::ProjectOwnerPool::new(Arc::new(Mutex::new(agent))).unwrap();
    let id = pool.workspace().active().unwrap().id().as_str().to_owned();

    let arch = pool.arch_agent(&id).unwrap();
    let arch_path = arch.lock().unwrap().session().path().to_path_buf();
    assert_ne!(
        arch_path, discussion_path,
        "arch must not share the discussion journal"
    );
    assert_eq!(arch_path.file_name().unwrap(), "arch.jsonl");

    // Preparing arch never rewrites the discussion owner's session path.
    let discussion = pool.owner(&id).unwrap();
    assert_eq!(
        discussion.lock().unwrap().session().path(),
        discussion_path.as_path()
    );
    // Repeated calls reuse the same independent owner handle.
    let again = pool.arch_agent(&id).unwrap();
    assert!(Arc::ptr_eq(&arch, &again));
}
