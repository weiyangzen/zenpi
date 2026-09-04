use std::fs;

use tempfile::tempdir;
use zenpi::{
    b3::ResourceBudget,
    domain_store::{
        DomainStore, DomainStoreError, MAX_DOMAIN_STORE_BYTES, StoreChange, path_for_session,
    },
    domains::{Blueprint, BlueprintItem, DomainError, Goal, GoalStatus, Learn},
};

fn blueprint(version: &str) -> Blueprint {
    Blueprint::new(
        "agent-plan",
        version,
        vec![
            BlueprintItem::new("build", 120),
            BlueprintItem::new("test", 240).with_dependencies(["build"]),
        ],
    )
    .unwrap()
}

fn goal(plan: &Blueprint, id: &str) -> Goal {
    Goal::new(
        id,
        plan,
        ResourceBudget {
            tokens: 100,
            wall_clock_ms: 1_000,
            attempts: 2,
            disk_bytes: 4_096,
        },
        None,
    )
    .unwrap()
}

#[test]
fn missing_store_is_created_as_a_private_valid_jsonl_snapshot() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("domains.jsonl");
    let store = DomainStore::open(&path).unwrap();

    assert!(path.is_file());
    assert_eq!(store.generation(), 0);
    assert_eq!(store.summary().unwrap().blueprint_count, 0);
    assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 1);
    assert!(
        fs::read_to_string(path)
            .unwrap()
            .contains("\"kind\":\"domain_store\"")
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(dir.path().join("domains.jsonl"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn read_only_open_does_not_create_missing_store() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("not-created.jsonl");
    let store = DomainStore::open_read_only(&path).unwrap();
    assert!(!path.exists());
    assert_eq!(store.generation(), 0);
    assert_eq!(store.blueprints().len(), 0);
}

#[test]
fn blueprint_goal_and_learn_round_trip_with_idempotent_writes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("domains.jsonl");
    let mut store = DomainStore::open(&path).unwrap();
    let plan = blueprint("2.0.0");

    assert_eq!(
        store.put_blueprint(plan.clone()).unwrap(),
        StoreChange::Inserted
    );
    let generation = store.generation();
    assert_eq!(
        store.put_blueprint(plan.clone()).unwrap(),
        StoreChange::Unchanged
    );
    assert_eq!(store.generation(), generation);

    let initial_goal = goal(&plan, "goal-1");
    assert_eq!(store.put_goal(initial_goal).unwrap(), StoreChange::Inserted);
    assert_eq!(
        store
            .transition_goal("goal-1", GoalStatus::Running)
            .unwrap(),
        StoreChange::Updated
    );
    assert_eq!(
        store
            .transition_goal("goal-1", GoalStatus::Running)
            .unwrap(),
        StoreChange::Unchanged
    );

    let learn = Learn::new("learn-1", "src", "notes", vec!["evidence.md".into()]).unwrap();
    assert_eq!(
        store.put_learn(learn.clone()).unwrap(),
        StoreChange::Inserted
    );
    assert_eq!(store.put_learn(learn).unwrap(), StoreChange::Unchanged);

    let digest = store.digest().unwrap();
    let reopened = DomainStore::open(&path).unwrap();
    assert_eq!(reopened.digest().unwrap(), digest);
    assert_eq!(reopened.generation(), store.generation());
    assert_eq!(reopened.blueprint("agent-plan", "2.0.0").unwrap(), &plan);
    assert_eq!(reopened.goal("goal-1").unwrap().status, GoalStatus::Running);
    assert_eq!(reopened.learn("learn-1").unwrap().source, "src");
}

#[test]
fn versioned_blueprints_are_kept_and_old_goal_links_remain_valid() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("domains.jsonl");
    let mut store = DomainStore::open(&path).unwrap();
    let old = blueprint("1.0.0");
    let new = blueprint("2.0.0");
    store.put_blueprint(old.clone()).unwrap();
    store.put_blueprint(new.clone()).unwrap();
    store.put_goal(goal(&old, "old-goal")).unwrap();
    store.put_goal(goal(&new, "new-goal")).unwrap();

    assert_eq!(store.blueprint_versions("agent-plan").len(), 2);
    assert!(matches!(
        store.remove_blueprint("agent-plan", "1.0.0"),
        Err(DomainStoreError::BlueprintReferenced {
            id,
            version
        }) if id == "agent-plan" && version == "1.0.0"
    ));
    assert_eq!(store.remove_goal("old-goal").unwrap(), StoreChange::Removed);
    assert_eq!(
        store.remove_blueprint("agent-plan", "1.0.0").unwrap(),
        StoreChange::Removed
    );
}

#[test]
fn stale_digest_and_invalid_status_transition_are_rejected_without_mutating_disk() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("domains.jsonl");
    let mut store = DomainStore::open(&path).unwrap();
    let plan = blueprint("2.0.0");
    store.put_blueprint(plan.clone()).unwrap();
    store.put_goal(goal(&plan, "goal-1")).unwrap();
    let before = fs::read(&path).unwrap();

    let mut stale = goal(&plan, "goal-stale");
    stale.blueprint_digest = "0".repeat(64);
    assert!(matches!(
        store.put_goal(stale),
        Err(DomainStoreError::Domain(DomainError::BlueprintLinkMismatch))
    ));
    assert_eq!(fs::read(&path).unwrap(), before);

    assert!(matches!(
        store.transition_goal("goal-1", GoalStatus::Done),
        Err(DomainStoreError::InvalidGoalTransition {
            from: GoalStatus::Queued,
            to: GoalStatus::Done,
            ..
        })
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(store.goal("goal-1").unwrap().status, GoalStatus::Queued);
}

#[test]
fn tampered_snapshot_digest_and_cross_record_links_fail_closed() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("domains.jsonl");
    let mut store = DomainStore::open(&path).unwrap();
    let plan = blueprint("2.0.0");
    store.put_blueprint(plan.clone()).unwrap();
    store.put_goal(goal(&plan, "goal-1")).unwrap();
    let mut text = fs::read_to_string(&path).unwrap();
    text = text.replace("\"status\":\"queued\"", "\"status\":\"done\"");
    fs::write(&path, text).unwrap();
    assert!(matches!(
        DomainStore::open(&path),
        Err(DomainStoreError::DigestMismatch)
    ));

    // A syntactically valid snapshot with a missing blueprint is rejected by
    // cross-record validation even when its header digest is recomputed.
    let missing_path = dir.path().join("missing.jsonl");
    let missing = DomainStore::open(&missing_path).unwrap();
    let orphan = goal(&plan, "orphan");
    // Build a valid goal line only to exercise the public persistence format;
    // the API itself never permits this state.
    let goal_line = serde_json::to_string(&serde_json::json!({
        "kind": "goal",
        "record": orphan,
    }))
    .unwrap();
    let header_line = fs::read_to_string(&missing_path)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_owned();
    fs::write(&missing_path, format!("{header_line}\n{goal_line}\n")).unwrap();
    assert!(matches!(
        DomainStore::open(&missing_path),
        Err(DomainStoreError::BlueprintNotFound { .. })
    ));
    // Keep the variable live until after the file assertion; this also makes
    // the test explicit that opening an empty store itself remains valid.
    assert_eq!(missing.goals().len(), 0);
}

#[test]
fn oversized_or_duplicate_jsonl_is_rejected_before_state_use() {
    let dir = tempdir().unwrap();
    let too_large = dir.path().join("too-large.jsonl");
    fs::write(&too_large, vec![b'x'; MAX_DOMAIN_STORE_BYTES + 1]).unwrap();
    assert!(matches!(
        DomainStore::open(&too_large),
        Err(DomainStoreError::StoreTooLong { .. })
    ));

    let duplicate = dir.path().join("duplicate.jsonl");
    let first = DomainStore::open(&duplicate).unwrap();
    let header = fs::read_to_string(&duplicate)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_owned();
    let plan = blueprint("2.0.0");
    let line = serde_json::to_string(&serde_json::json!({
        "kind": "blueprint",
        "record": plan,
    }))
    .unwrap();
    fs::write(&duplicate, format!("{header}\n{line}\n{line}\n")).unwrap();
    assert!(matches!(
        DomainStore::open(&duplicate),
        Err(DomainStoreError::DuplicateRecord {
            kind: "blueprint",
            ..
        })
    ));
    assert_eq!(first.blueprints().len(), 0);
}

#[test]
fn canonical_digest_does_not_depend_on_mutation_order() {
    let dir = tempdir().unwrap();
    let first_path = dir.path().join("first.jsonl");
    let second_path = dir.path().join("second.jsonl");
    let plan = blueprint("2.0.0");
    let learn = Learn::new("learn-1", "src", "notes", Vec::new()).unwrap();

    let mut first = DomainStore::open(&first_path).unwrap();
    first.put_blueprint(plan.clone()).unwrap();
    first.put_learn(learn.clone()).unwrap();

    let mut second = DomainStore::open(&second_path).unwrap();
    second.put_learn(learn).unwrap();
    second.put_blueprint(plan).unwrap();

    assert_eq!(first.digest().unwrap(), second.digest().unwrap());
    assert_eq!(first.generation(), 2);
    assert_eq!(second.generation(), 2);
}

#[cfg(unix)]
#[test]
fn symlinked_store_path_is_rejected_before_read_or_replace() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let target = dir.path().join("target.jsonl");
    let link = dir.path().join("link.jsonl");
    fs::write(&target, b"not a store\n").unwrap();
    symlink(&target, &link).unwrap();
    assert!(matches!(
        DomainStore::open(&link),
        Err(DomainStoreError::Symlink(path)) if path == link
    ));
    assert_eq!(fs::read_to_string(target).unwrap(), "not a store\n");
}

#[test]
fn custom_session_paths_share_a_sibling_domain_store() {
    let dir = tempdir().unwrap();
    let session = dir.path().join("sessions/session.jsonl");
    assert_eq!(
        path_for_session(&session),
        dir.path().join("sessions/domains.jsonl")
    );
}
