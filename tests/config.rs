use std::{collections::BTreeMap, fs, path::Path, process::Command};

use tempfile::tempdir;
use zenpi::config::{
    AuthFile, ConfigFile, ConfigOverrides, ConfigPaths, CredentialSource, load_auth, load_config,
    pair_from_codex, resolve, status,
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
