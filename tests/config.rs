use std::{collections::BTreeMap, fs, path::Path, process::Command};

use tempfile::tempdir;
use zenpi::config::{
    AuthFile, ConfigFile, ConfigOverrides, ConfigPaths, CredentialSource, ProviderProfile,
    list_profiles, load_auth, load_config, pair_from_codex, resolve, revoke, save_auth,
    save_config, status, use_profile,
};

fn write_codex_fixture(home: &Path, key: &str) {
    let codex = home.join(".codex");
    fs::create_dir_all(&codex).unwrap();
    fs::write(
        codex.join("config.toml"),
        r#"model_provider = "OpenAI"
model = "gpt-test"

[model_providers.OpenAI]
name = "OpenAI"
base_url = "http://127.0.0.1:9000/v1"
wire_api = "chat"
requires_openai_auth = true
"#,
    )
    .unwrap();
    fs::write(
        codex.join("auth.json"),
        format!("{{\"OPENAI_API_KEY\":\"{key}\"}}\n"),
    )
    .unwrap();
}

#[test]
fn pairing_imports_codex_and_is_idempotent_without_leaking_key() {
    let home = tempdir().unwrap();
    write_codex_fixture(home.path(), "secret-test-key");
    let paths = ConfigPaths::for_home(home.path());

    let first = pair_from_codex(&paths).unwrap();
    assert!(first.changed);
    assert!(first.config_changed);
    assert!(first.auth_changed);
    assert!(first.key_imported);
    assert_eq!(first.backend, "openai");
    assert_eq!(first.model.as_deref(), Some("gpt-test"));
    assert_eq!(first.base_url.as_deref(), Some("http://127.0.0.1:9000/v1"));
    assert!(!format!("{first:?}").contains("secret-test-key"));

    let config_bytes = fs::read(&paths.config).unwrap();
    let auth_bytes = fs::read(&paths.auth).unwrap();
    let second = pair_from_codex(&paths).unwrap();
    assert!(!second.changed);
    assert_eq!(config_bytes, fs::read(&paths.config).unwrap());
    assert_eq!(auth_bytes, fs::read(&paths.auth).unwrap());

    let config = load_config(&paths).unwrap();
    let auth = load_auth(&paths).unwrap();
    assert_eq!(config.backend.as_deref(), Some("openai"));
    assert_eq!(auth.openai_api_key(), Some("secret-test-key"));
    assert!(!format!("{auth:?}").contains("secret-test-key"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&paths.root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&paths.auth).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&paths.config).unwrap().permissions().mode() & 0o777,
            0o600
        );
        for directory in [&paths.sessions, &paths.skills, &paths.extensions] {
            assert_eq!(
                fs::metadata(directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }
}

#[test]
fn cli_import_honors_codex_home_as_the_profile_root() {
    let temp = tempdir().unwrap();
    let codex_home = temp.path().join("alternate-codex");
    let zenpi_home = temp.path().join("zenpi-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        r#"model_provider = "OpenAI"
model = "codex-home-model"

[model_providers.OpenAI]
base_url = "http://127.0.0.1:9911"
wire_api = "responses"
"#,
    )
    .unwrap();
    fs::write(
        codex_home.join("auth.json"),
        "{\"OPENAI_API_KEY\":\"codex-home-secret\"}\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_zenpi"))
        .args(["config", "import-codex"])
        .env("CODEX_HOME", &codex_home)
        .env("ZENPI_HOME", &zenpi_home)
        .env_remove("ZENPI_MODEL")
        .env_remove("OPENAI_MODEL")
        .env_remove("ZENPI_BASE_URL")
        .env_remove("OPENAI_BASE_URL")
        .env_remove("ZENPI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("model=codex-home-model"));
    assert!(stdout.contains("base_url=http://127.0.0.1:9911"));
    assert!(!stdout.contains("codex-home-secret"));

    let paths = ConfigPaths {
        config: zenpi_home.join("config.toml"),
        auth: zenpi_home.join("auth.json"),
        sessions: zenpi_home.join("sessions"),
        skills: zenpi_home.join("skills"),
        extensions: zenpi_home.join("extensions"),
        root: zenpi_home,
    };
    assert_eq!(
        load_config(&paths).unwrap().model.as_deref(),
        Some("codex-home-model")
    );
    assert_eq!(
        load_auth(&paths).unwrap().openai_api_key(),
        Some("codex-home-secret")
    );
}

#[test]
fn precedence_is_cli_then_env_then_file_then_defaults() {
    let config = ConfigFile {
        backend: Some("openai".into()),
        provider: Some("file-provider".into()),
        model: Some("file-model".into()),
        base_url: Some("http://file.example/v1".into()),
        wire_api: Some("chat".into()),
        auth_env: Some("OPENAI_API_KEY".into()),
        ..ConfigFile::default()
    };
    let mut auth = AuthFile::default();
    auth.set_openai_api_key("file-key").unwrap();
    let environment = BTreeMap::from([
        ("ZENPI_MODEL".into(), "env-model".into()),
        ("ZENPI_BASE_URL".into(), "http://env.example/v1".into()),
        ("ZENPI_API_KEY".into(), "env-key".into()),
    ]);
    let overrides = ConfigOverrides {
        backend: Some("echo".into()),
        model: Some("cli-model".into()),
        ..ConfigOverrides::default()
    };
    let resolved = resolve(&overrides, &config, &auth, &environment).unwrap();
    assert_eq!(resolved.backend, "echo");
    assert_eq!(resolved.model.as_deref(), Some("cli-model"));
    assert_eq!(resolved.base_url.as_deref(), Some("http://env.example/v1"));
    assert_eq!(resolved.api_key.as_deref(), Some("env-key"));
    assert_eq!(resolved.credential_source, CredentialSource::Environment);

    let custom_auth = ConfigFile {
        auth_env: Some("CUSTOM_PROVIDER_KEY".into()),
        ..ConfigFile::default()
    };
    let custom_env = BTreeMap::from([("CUSTOM_PROVIDER_KEY".into(), "custom-key".into())]);
    let custom = resolve(
        &ConfigOverrides::default(),
        &custom_auth,
        &AuthFile::default(),
        &custom_env,
    )
    .unwrap();
    assert_eq!(custom.api_key.as_deref(), Some("custom-key"));
    assert_eq!(custom.credential_source, CredentialSource::Environment);

    let defaults = resolve(
        &ConfigOverrides::default(),
        &ConfigFile::default(),
        &AuthFile::default(),
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(defaults.backend, "openai");
    assert_eq!(defaults.api_key, None);
}

#[test]
fn status_is_redacted_and_reports_auth_source() {
    let home = tempdir().unwrap();
    write_codex_fixture(home.path(), "secret-status-key");
    let paths = ConfigPaths::for_home(home.path());
    pair_from_codex(&paths).unwrap();
    let report = status(&paths).unwrap();
    assert!(report.config_exists);
    assert!(report.auth_exists);
    assert_eq!(report.backend, "openai");
    assert!(report.api_key_present);
    assert_eq!(report.api_key_source, CredentialSource::AuthFile);
    assert!(!format!("{report:?}").contains("secret-status-key"));
}

#[test]
fn codex_import_ignores_unrelated_nested_credentials() {
    let home = tempdir().unwrap();
    let codex = home.path().join(".codex");
    fs::create_dir_all(&codex).unwrap();
    fs::write(
        codex.join("config.toml"),
        r#"model_provider = "OpenAI"
model = "test"
[model_providers.OpenAI]
base_url = "http://localhost:9000"
wire_api = "responses"
"#,
    )
    .unwrap();
    fs::write(
        codex.join("auth.json"),
        r#"{"extension":{"api_key":"must-not-import"},"tokens":{"access_token":"oauth"}}"#,
    )
    .unwrap();
    let paths = ConfigPaths::for_home(home.path());
    let report = pair_from_codex(&paths).unwrap();
    assert!(!report.key_imported);
    assert_eq!(load_auth(&paths).unwrap().openai_api_key(), None);
}

#[test]
fn named_profiles_select_list_and_revoke_without_touching_other_credentials() {
    let home = tempdir().unwrap();
    let paths = ConfigPaths::for_home(home.path());
    let mut profiles = BTreeMap::new();
    profiles.insert(
        "first".into(),
        ProviderProfile {
            backend: Some("openai".into()),
            provider: Some("First".into()),
            model: Some("model-1".into()),
            base_url: Some("http://first.example/v1".into()),
            wire_api: Some("responses".into()),
            requires_openai_auth: Some(true),
            ..ProviderProfile::default()
        },
    );
    profiles.insert(
        "second".into(),
        ProviderProfile {
            backend: Some("openai".into()),
            provider: Some("Second".into()),
            model: Some("model-2".into()),
            base_url: Some("http://second.example/v1".into()),
            wire_api: Some("chat".into()),
            requires_openai_auth: Some(true),
            ..ProviderProfile::default()
        },
    );
    save_config(
        &paths,
        &ConfigFile {
            default_profile: Some("first".into()),
            profiles,
            ..ConfigFile::default()
        },
    )
    .unwrap();
    let mut auth = AuthFile::default();
    auth.set_profile_api_key("first", "secret-first").unwrap();
    auth.set_profile_api_key("second", "secret-second").unwrap();
    auth.0.insert("unrelated".into(), serde_json::json!("keep"));
    save_auth(&paths, &auth).unwrap();

    let selected = resolve(
        &ConfigOverrides::default(),
        &load_config(&paths).unwrap(),
        &load_auth(&paths).unwrap(),
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(selected.profile.as_deref(), Some("first"));
    assert_eq!(selected.model.as_deref(), Some("model-1"));
    assert_eq!(selected.api_key.as_deref(), Some("secret-first"));

    assert!(use_profile(&paths, "second").unwrap());
    let listed = list_profiles(&paths).unwrap();
    assert_eq!(listed.len(), 2);
    assert!(
        listed
            .iter()
            .any(|profile| profile.name == "second" && profile.active)
    );
    assert!(!format!("{listed:?}").contains("secret-"));

    assert!(revoke(&paths, Some("second")).unwrap());
    let auth = load_auth(&paths).unwrap();
    assert_eq!(auth.api_key_for_profile(Some("second")), None);
    assert_eq!(
        auth.api_key_for_profile(Some("first")),
        Some("secret-first")
    );
    assert_eq!(auth.0.get("unrelated"), Some(&serde_json::json!("keep")));
}

#[test]
fn config_cli_supports_profile_json_list_use_and_confirmed_revoke() {
    let temp = tempdir().unwrap();
    let codex_home = temp.path().join("codex");
    let zenpi_home = temp.path().join("zenpi");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        r#"model_provider = "OpenAI"
model = "profile-model"
[model_providers.OpenAI]
base_url = "http://127.0.0.1:9991"
wire_api = "responses"
requires_openai_auth = true
"#,
    )
    .unwrap();
    fs::write(
        codex_home.join("auth.json"),
        r#"{"OPENAI_API_KEY":"profile-secret"}"#,
    )
    .unwrap();
    let base = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zenpi"));
        command
            .env("CODEX_HOME", &codex_home)
            .env("ZENPI_HOME", &zenpi_home)
            .env_remove("OPENAI_API_KEY")
            .env_remove("ZENPI_API_KEY");
        command
    };
    let imported = base()
        .args(["config", "import-codex", "--profile", "codex"])
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(!String::from_utf8_lossy(&imported.stdout).contains("profile-secret"));

    let doctor = base()
        .args(["config", "doctor", "--profile", "codex", "--json"])
        .output()
        .unwrap();
    assert!(doctor.status.success());
    let doctor: serde_json::Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(doctor["profile"], "codex");
    assert_eq!(doctor["api_key_present"], true);
    assert!(!String::from_utf8_lossy(&doctor.to_string().into_bytes()).contains("profile-secret"));

    let listed = base().args(["config", "list", "--json"]).output().unwrap();
    assert!(listed.status.success());
    let profiles: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(profiles[0]["name"], "codex");
    assert_eq!(profiles[0]["active"], true);

    let unconfirmed = base()
        .args(["pair", "revoke", "--profile", "codex"])
        .output()
        .unwrap();
    assert!(!unconfirmed.status.success());
    let revoked = base()
        .args(["pair", "revoke", "--profile", "codex", "--yes"])
        .output()
        .unwrap();
    assert!(revoked.status.success());
    assert_eq!(
        load_auth(&ConfigPaths {
            root: zenpi_home.clone(),
            config: zenpi_home.join("config.toml"),
            auth: zenpi_home.join("auth.json"),
            sessions: zenpi_home.join("sessions"),
            skills: zenpi_home.join("skills"),
            extensions: zenpi_home.join("extensions"),
        })
        .unwrap()
        .api_key_for_profile(Some("codex")),
        None
    );
}
