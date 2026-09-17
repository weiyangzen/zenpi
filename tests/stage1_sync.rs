use std::fs;
use tempfile::tempdir;
use zenpi::slash::{SlashCommand, SlashError};
use zenpi::sync::{discover_blueprint, sync_requirement};

#[test]
fn parse_sync_requires_and_keeps_requirement() {
    let parsed = zenpi::slash::parse("/sync add ssh remote folder tabs").unwrap();
    match parsed {
        Some(SlashCommand::Sync { requirement }) => {
            assert_eq!(requirement, "add ssh remote folder tabs")
        }
        other => panic!("unexpected parse: {other:?}"),
    }
    assert_eq!(
        SlashCommand::Sync {
            requirement: "x".into()
        }
        .name(),
        "sync"
    );
    assert!(matches!(
        zenpi::slash::parse("/sync"),
        Err(SlashError::MissingArgument { command: "sync" })
    ));
    assert!(zenpi::slash::complete("sy").contains(&"sync"));
}

#[test]
fn sync_appends_once_and_is_idempotent() {
    let dir = tempdir().unwrap();
    let docs = dir.path().join("Docs");
    fs::create_dir_all(&docs).unwrap();
    let blueprint = docs.join("stage_1_v3_pi_mono_blueprint.md");
    fs::write(
        &blueprint,
        "# blueprint\n\n- [x] **ZS1-001** — base；layer `L0` | Depends: — | Owner scope: x | Owned paths: — | Validators: — | Rollback: — | Estimate: — | Estimated LOC: 0\n",
    )
    .unwrap();

    assert_eq!(discover_blueprint(dir.path()).unwrap(), blueprint);

    let first = sync_requirement(dir.path(), "  add   SSH   remote  folder tabs ").unwrap();
    assert!(!first.duplicate);
    assert!(first.queued);
    assert_eq!(first.item_id, "ZS1-900");
    let text = fs::read_to_string(&blueprint).unwrap();
    assert!(text.contains("**ZS1-900**"), "{text}");
    assert!(text.contains("/sync "), "{text}");
    // The original row is untouched.
    assert!(text.contains("**ZS1-001**"));

    // Same normalized requirement is a duplicate: no second row, no re-queue.
    let second = sync_requirement(dir.path(), "add SSH remote folder tabs").unwrap();
    assert!(second.duplicate);
    assert!(!second.queued);
    assert_eq!(second.item_id, "ZS1-900");
    assert_eq!(fs::read_to_string(&blueprint).unwrap().matches("ZS1-900").count(), text.matches("ZS1-900").count());

    // A different requirement gets a fresh id.
    let third = sync_requirement(dir.path(), "second requirement").unwrap();
    assert!(!third.duplicate);
    assert_eq!(third.item_id, "ZS1-901");

    assert!(dir.path().join("Docs/execution/sync_ledger.json").is_file());
    assert!(matches!(
        sync_requirement(dir.path(), "   "),
        Err(zenpi::sync::SyncError::Empty)
    ));
}
