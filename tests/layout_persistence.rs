use std::fs;

use tempfile::tempdir;
use zenpi::config::{
    ConfigPaths, LAYOUT_FILE, load_layout, load_layout_preferences, migrate_layout_preferences,
    reset_layout, reset_layout_profile, save_layout,
};
use zenpi::layout::{
    ColumnRatios, LayoutModel, LayoutPreferences, MAX_LAYOUT_PREFERENCES_BYTES, PaneId, TabId,
};

#[test]
fn profile_and_tab_states_round_trip_independently_and_reset_is_idempotent() {
    let home = tempdir().unwrap();
    let paths = ConfigPaths::for_home(home.path());
    let mut project = LayoutModel::new(TabId::Project);
    project.set_ratios(ColumnRatios::new(35, 40, 25));
    project.set_collapsed(PaneId::Resources, true);
    project.set_focused(Some(PaneId::Gantt));

    assert!(save_layout(&paths, Some("alpha"), &project).unwrap());
    assert_eq!(
        load_layout(&paths, Some("alpha"), TabId::Project).unwrap(),
        project
    );
    assert_eq!(
        load_layout(&paths, Some("alpha"), TabId::Goal)
            .unwrap()
            .ratios,
        LayoutModel::new(TabId::Goal).ratios
    );
    assert_eq!(
        load_layout(&paths, Some("beta"), TabId::Project)
            .unwrap()
            .ratios,
        LayoutModel::new(TabId::Project).ratios
    );

    assert!(reset_layout(&paths, Some("alpha"), TabId::Project).unwrap());
    assert!(!reset_layout(&paths, Some("alpha"), TabId::Project).unwrap());
    assert_eq!(
        load_layout(&paths, Some("alpha"), TabId::Project)
            .unwrap()
            .ratios,
        LayoutModel::new(TabId::Project).ratios
    );
    assert!(!reset_layout_profile(&paths, Some("missing")).unwrap());
}

#[test]
fn invalid_state_is_rejected_without_replacing_a_valid_snapshot() {
    let home = tempdir().unwrap();
    let paths = ConfigPaths::for_home(home.path());
    let mut valid = LayoutModel::new(TabId::Project);
    valid.set_ratios(ColumnRatios::new(40, 35, 25));
    assert!(save_layout(&paths, None, &valid).unwrap());
    let before = fs::read(paths.layout_path()).unwrap();

    let mut invalid = valid.clone();
    invalid.set_ratios(ColumnRatios::new(0, 100, 0));
    assert!(save_layout(&paths, None, &invalid).is_err());
    assert_eq!(fs::read(paths.layout_path()).unwrap(), before);
    assert_eq!(load_layout(&paths, None, TabId::Project).unwrap(), valid);
}

#[test]
fn corrupt_and_oversized_documents_fail_closed_without_mutation() {
    let home = tempdir().unwrap();
    let paths = ConfigPaths::for_home(home.path());
    let mut valid = LayoutModel::new(TabId::Project);
    valid.set_ratios(ColumnRatios::new(35, 40, 25));
    save_layout(&paths, None, &valid).unwrap();

    let corrupt = b"{ definitely not layout json".to_vec();
    fs::write(paths.layout_path(), &corrupt).unwrap();
    assert!(load_layout_preferences(&paths).is_err());
    assert_eq!(fs::read(paths.layout_path()).unwrap(), corrupt);

    let oversized = vec![b' '; MAX_LAYOUT_PREFERENCES_BYTES + 1];
    fs::write(paths.layout_path(), &oversized).unwrap();
    assert!(load_layout_preferences(&paths).is_err());
    assert_eq!(
        fs::metadata(paths.layout_path()).unwrap().len() as usize,
        oversized.len()
    );
}

#[test]
fn out_of_range_and_unknown_pane_values_are_rejected() {
    let home = tempdir().unwrap();
    let paths = ConfigPaths::for_home(home.path());
    let mut valid = LayoutModel::new(TabId::Project);
    valid.set_ratios(ColumnRatios::new(35, 40, 25));
    save_layout(&paths, None, &valid).unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(paths.layout_path()).unwrap()).unwrap();

    value["profiles"]["default"]["tabs"]["project"]["ratios"]["left"] = serde_json::json!(4);
    fs::write(paths.layout_path(), serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(load_layout_preferences(&paths).is_err());

    value["profiles"]["default"]["tabs"]["project"]["ratios"]["left"] = serde_json::json!(35);
    value["profiles"]["default"]["tabs"]["project"]["collapsed"] =
        serde_json::json!(["not_a_real_pane"]);
    fs::write(paths.layout_path(), serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(load_layout_preferences(&paths).is_err());
}

#[test]
fn legacy_layout_model_is_migrated_to_default_profile() {
    let home = tempdir().unwrap();
    let paths = ConfigPaths::for_home(home.path());
    paths.ensure_root().unwrap();
    let mut legacy = LayoutModel::new(TabId::Goal);
    legacy.set_ratios(ColumnRatios::new(45, 30, 25));
    legacy.set_collapsed(PaneId::Resources, true);
    legacy.set_focused(Some(PaneId::Gantt));
    fs::write(paths.layout_path(), serde_json::to_vec(&legacy).unwrap()).unwrap();

    let migrated = load_layout(&paths, None, TabId::Goal).unwrap();
    assert_eq!(migrated, legacy);
    assert!(migrate_layout_preferences(&paths).unwrap());
    assert!(!migrate_layout_preferences(&paths).unwrap());
    let preferences = load_layout_preferences(&paths).unwrap();
    assert_eq!(
        preferences.schema_version,
        zenpi::layout::LAYOUT_SCHEMA_VERSION
    );
    assert!(preferences.profiles.contains_key("default"));
}

#[cfg(unix)]
#[test]
fn layout_symlink_is_rejected_for_read_write_and_reset() {
    use std::os::unix::fs::symlink;

    let home = tempdir().unwrap();
    let paths = ConfigPaths::for_home(home.path());
    let model = LayoutModel::new(TabId::Project);
    save_layout(&paths, None, &model).unwrap();
    let original = fs::read(paths.layout_path()).unwrap();
    let target = home.path().join("outside-layout.json");
    fs::write(&target, b"outside").unwrap();
    fs::remove_file(paths.layout_path()).unwrap();
    symlink(&target, paths.layout_path()).unwrap();

    assert!(load_layout_preferences(&paths).is_err());
    assert!(save_layout(&paths, None, &model).is_err());
    assert!(reset_layout(&paths, None, TabId::Project).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"outside");
    assert_ne!(fs::read(paths.layout_path()).unwrap(), original);
    assert_eq!(paths.layout_path().file_name().unwrap(), LAYOUT_FILE);
}

#[test]
fn layout_preferences_json_round_trip_is_bounded_and_deterministic() {
    let preferences = LayoutPreferences::default();
    let first = preferences.to_json_bytes().unwrap();
    let second = preferences.to_json_bytes().unwrap();
    assert_eq!(first, second);
    assert!(first.len() < MAX_LAYOUT_PREFERENCES_BYTES);
    assert_eq!(
        LayoutPreferences::from_json_bytes(&first).unwrap(),
        preferences
    );
}
