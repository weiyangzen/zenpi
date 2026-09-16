use std::{collections::BTreeMap, fs};
use tempfile::tempdir;
use zenpi::config::{self, AuthFile, ConfigOverrides, ConfigPaths};

#[test]
fn project_profile_selection_and_field_overlay_preserve_user_profiles_and_precedence() {
    let root = tempdir().unwrap();
    let paths = ConfigPaths::for_home(root.path());
    fs::create_dir_all(&paths.root).unwrap();
    let user = "default_profile='primary'\n[profiles.primary]\nmodel='user-model'\nbase_url='http://localhost:1234/v1'\nwire_api='chat'\n[profiles.spare]\nmodel='spare-model'\n";
    fs::write(&paths.config, user).unwrap();
    let workspace = root.path().join("project");
    fs::create_dir_all(workspace.join(".zenpi")).unwrap();
    let project_path = workspace.join(".zenpi/config.toml");
    fs::write(&project_path, "default_profile='spare'\n").unwrap();
    let selected = config::load_workspace_config(&paths, Some(&workspace)).unwrap();
    assert_eq!(selected.default_profile.as_deref(), Some("spare"));
    assert_eq!(selected.profiles.len(), 2);
    fs::write(
        &project_path,
        "[profiles.primary]\nmodel='project-model'\n[profiles.third]\nmodel='third-model'\n",
    )
    .unwrap();
    let merged = config::load_workspace_config(&paths, Some(&workspace)).unwrap();
    assert_eq!(merged.profiles.len(), 3);
    assert_eq!(merged.default_profile.as_deref(), Some("primary"));
    assert_eq!(
        merged.profiles["primary"].model.as_deref(),
        Some("project-model")
    );
    assert_eq!(
        merged.profiles["primary"].base_url.as_deref(),
        Some("http://localhost:1234/v1")
    );
    assert_eq!(merged.profiles["primary"].wire_api.as_deref(), Some("chat"));
    let env = BTreeMap::from([("ZENPI_MODEL".into(), "environment-model".into())]);
    let resolved = config::resolve(
        &ConfigOverrides::default(),
        &merged,
        &AuthFile::default(),
        &env,
    )
    .unwrap();
    assert_eq!(resolved.model.as_deref(), Some("environment-model"));
    let resolved = config::resolve(
        &ConfigOverrides {
            model: Some("cli-model".into()),
            ..Default::default()
        },
        &merged,
        &AuthFile::default(),
        &env,
    )
    .unwrap();
    assert_eq!(resolved.model.as_deref(), Some("cli-model"));
    assert_eq!(fs::read_to_string(&paths.config).unwrap(), user);
}

#[test]
fn project_config_rejects_invalid_merged_selection_unknown_fields_and_oversize() {
    let root = tempdir().unwrap();
    let paths = ConfigPaths::for_home(root.path());
    let workspace = root.path().join("project");
    fs::create_dir_all(workspace.join(".zenpi")).unwrap();
    let path = workspace.join(".zenpi/config.toml");
    for invalid in [
        "default_profile='missing'",
        "api_key='never-accepted'",
        "[profiles.bad]\nunknown_field=true",
        "[profiles.bad]\nmodel=''",
        "[profiles.'bad name']\nmodel='x'",
    ] {
        fs::write(&path, invalid).unwrap();
        assert!(
            config::load_workspace_config(&paths, Some(&workspace)).is_err(),
            "{invalid}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
    }
    fs::write(&path, " ".repeat(262145)).unwrap();
    assert!(
        config::load_workspace_config(&paths, Some(&workspace))
            .unwrap_err()
            .to_string()
            .contains("256 KiB")
    );
    #[cfg(unix)]
    {
        fs::remove_file(&path).unwrap();
        let target = root.path().join("outside.toml");
        fs::write(&target, "model='outside'").unwrap();
        std::os::unix::fs::symlink(target, &path).unwrap();
        assert!(config::load_workspace_config(&paths, Some(&workspace)).is_err());
    }
}
