use std::{collections::BTreeMap, fs, path::Path, process::Command};

use tempfile::tempdir;
use zenpi::config::{
    AuthFile, ConfigFile, ConfigOverrides, ConfigPaths, CredentialSource, ProviderProfile,
    list_profiles, load_auth, load_config, pair_from_codex, resolve, revoke, save_auth,
    save_config, status, use_profile,
};
use zenpi::core::parse_args;

fn explicit_config(text: &str) -> ConfigFile {
    let config: ConfigFile = toml::from_str(text).unwrap();
    config.validate().unwrap();
    config
}

#[test]
fn explicit_binding_ignores_ambient_credentials_and_route_environment() {
    let config = explicit_config(
        "provider='deepseek'\nmodel='deepseek-flash'\nwire_api='responses'\nauth_method='api_key'\nauth_ref='shared-key'",
    );
    let mut auth = AuthFile::default();
    auth.set_openai_api_key("synthetic-file-key").unwrap();
    let environment = BTreeMap::from([
        ("ZENPI_API_KEY".into(), "synthetic-env-key".into()),
        ("OPENAI_API_KEY".into(), "synthetic-openai-key".into()),
        ("ZENPI_PROVIDER".into(), "wrong-provider".into()),
        ("ZENPI_BACKEND".into(), "echo".into()),
        ("ZENPI_WIRE_API".into(), "chat".into()),
        ("ZENPI_BASE_URL".into(), "https://wrong.example/v1".into()),
        (
            "OPENAI_BASE_URL".into(),
            "https://also-wrong.example/v1".into(),
        ),
    ]);
    let resolved = resolve(&ConfigOverrides::default(), &config, &auth, &environment).unwrap();
    assert_eq!(resolved.backend, "openai");
    assert_eq!(resolved.provider.as_deref(), Some("deepseek"));
    assert_eq!(resolved.wire_api.as_deref(), Some("responses"));
    assert!(resolved.base_url.is_none());
    assert_eq!(resolved.auth_method.as_deref(), Some("api_key"));
    assert_eq!(resolved.auth_ref.as_deref(), Some("shared-key"));
    assert_eq!(resolved.credential_source, CredentialSource::None);
    assert!(resolved.api_key.is_none());
    assert!(!format!("{resolved:?}").contains("synthetic-"));
    let error = resolve(
        &ConfigOverrides {
            api_key: Some("synthetic-cli-secret".into()),
            ..Default::default()
        },
        &config,
        &auth,
        &environment,
    )
    .unwrap_err();
    assert!(error.to_string().contains("conflicts"));
    assert!(!error.to_string().contains("synthetic-cli-secret"));
}

#[test]
fn absent_and_named_legacy_method_keep_existing_precedence() {
    let mut config: ConfigFile = toml::from_str("provider='configured'\nmodel='configured-model'\nbase_url='https://configured.test/v1'\nauth_env='CUSTOM_KEY'").unwrap();
    let mut auth = AuthFile::default();
    auth.set_openai_api_key("synthetic-file").unwrap();
    let environment = BTreeMap::from([
        ("ZENPI_API_KEY".into(), "synthetic-env".into()),
        ("ZENPI_PROVIDER".into(), "env-provider".into()),
        ("ZENPI_WIRE_API".into(), "chat".into()),
        (
            "ZENPI_BASE_URL".into(),
            "https://environment.test/v1".into(),
        ),
    ]);
    for method in [None, Some("legacy_api_key".to_owned())] {
        config.auth_method = method;
        let r = resolve(&ConfigOverrides::default(), &config, &auth, &environment).unwrap();
        assert_eq!(r.api_key.as_deref(), Some("synthetic-env"));
        assert_eq!(r.provider.as_deref(), Some("env-provider"));
        assert_eq!(r.base_url.as_deref(), Some("https://environment.test/v1"));
        assert_eq!(r.wire_api.as_deref(), Some("chat"));
        let r = resolve(
            &ConfigOverrides {
                api_key: Some("synthetic-cli".into()),
                ..Default::default()
            },
            &config,
            &auth,
            &environment,
        )
        .unwrap();
        assert_eq!(r.api_key.as_deref(), Some("synthetic-cli"));
    }
}

#[test]
fn explicit_auth_configuration_rejects_conflicts_without_resolving_secrets() {
    for text in [
        "auth_method='unknown'",
        "auth_ref='cred'",
        "auth_header='bearer'",
        "auth_method='legacy_api_key'\nauth_ref='cred'",
        "[[model_routes]]\nmodel='model'\nwire_api='responses'",
        "provider='openai'\nwire_api='responses'\nauth_method='api_key'",
        "provider='openai'\nwire_api='responses'\nauth_method='api_key'\nauth_ref='../cred'",
        "provider='openai'\nwire_api='responses'\nauth_method='api_key'\nauth_ref='cred'\nauth_env='CUSTOM_KEY'",
        "provider='openai'\nwire_api='responses'\nauth_method='api_key'\nauth_ref='cred'\nrequires_openai_auth=false",
        "provider='openai'\nauth_method='api_key'\nauth_ref='cred'",
        "provider='openai-codex'\nauth_method='api_key'\nauth_ref='cred'",
        "provider='openai'\nwire_api='responses'\nauth_method='oauth'\nauth_ref='cred'",
        "provider='openai-codex'\nauth_method='oauth'\nauth_ref='cred'\nauth_header='bearer'",
        "provider='openai-codex'\nwire_api='responses'\nauth_method='oauth'\nauth_ref='cred'",
        "provider='openai-codex'\nauth_method='oauth'\nauth_ref='cred'\nbase_url='https://chatgpt.com'",
        "provider='openai'\nwire_api='responses'\nauth_method='api_key'\nauth_ref='cred'\nauth_header='x_api_key'",
    ] {
        let c: ConfigFile = toml::from_str(text).unwrap();
        assert!(c.validate().is_err(), "accepted {text}");
    }
}

#[test]
fn codex_unique_wire_and_anonymous_local_binding_are_explicit() {
    let codex =
        explicit_config("provider='openai-codex'\nauth_method='oauth'\nauth_ref='codex-account'");
    let c = resolve(
        &ConfigOverrides::default(),
        &codex,
        &AuthFile::default(),
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(c.wire_api.as_deref(), Some("openai_codex_responses"));
    assert!(c.model.is_none() && c.api_key.is_none());
    for base in [
        "http://127.0.0.1:9999/v1",
        "http://localhost:9999/v1",
        "http://[::1]:9999/v1",
    ] {
        let config = explicit_config(&format!(
            "provider='local'\nwire_api='chat'\nbase_url='{base}'\nauth_method='none'"
        ));
        let c = resolve(
            &ConfigOverrides::default(),
            &config,
            &AuthFile::default(),
            &BTreeMap::new(),
        )
        .unwrap();
        assert!(!c.requires_openai_auth);
        assert!(c.api_key.is_none());
    }
    for extra in [
        "base_url='https://remote.test/v1'",
        "base_url='http://remote.test/v1'",
        "base_url='http://127.1/v1'",
        "base_url='http://127.0.0.1/v1'\nauth_ref='cred'",
        "base_url='http://127.0.0.1/v1'\nauth_env='KEY'",
    ] {
        let c: ConfigFile = toml::from_str(&format!(
            "provider='local'\nwire_api='chat'\nauth_method='none'\n{extra}"
        ))
        .unwrap();
        assert!(c.validate().is_err(), "{extra}");
    }
}

#[test]
fn deepseek_three_profiles_and_exact_routes_round_trip_without_secrets() {
    let c = explicit_config(
        r#"
default_profile='chat'
[profiles.chat]
provider='deepseek'
model='deepseek-flash'
wire_api='chat'
base_url='https://api.deepseek.com'
auth_method='api_key'
auth_ref='shared-deepseek'
[[profiles.chat.model_routes]]
model='deepseek-v4-pro'
wire_api='responses'
[profiles.responses]
provider='deepseek'
model='deepseek-flash'
wire_api='responses'
auth_method='api_key'
auth_ref='shared-deepseek'
[profiles.messages]
provider='deepseek'
model='deepseek-flash'
wire_api='anthropic_messages'
base_url='https://api.deepseek.com/anthropic/v1'
auth_method='api_key'
auth_ref='shared-deepseek'
auth_header='x_api_key'
"#,
    );
    let serialized = toml::to_string(&c).unwrap();
    assert!(!serialized.contains("api_key ="));
    assert_eq!(toml::from_str::<ConfigFile>(&serialized).unwrap(), c);
    for (name, wire) in [
        ("chat", "chat_completions"),
        ("responses", "responses"),
        ("messages", "anthropic_messages"),
    ] {
        let r = resolve(
            &ConfigOverrides {
                profile: Some(name.into()),
                ..Default::default()
            },
            &c,
            &AuthFile::default(),
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(r.wire_api.as_deref(), Some(wire));
        assert_eq!(r.auth_ref.as_deref(), Some("shared-deepseek"));
        assert!(r.api_key.is_none());
    }
    let (_, flat) = c.selected_profile(Some("chat")).unwrap();
    assert_eq!(flat.model_routes.len(), 1);
    assert_eq!(flat.model_routes[0].model, "deepseek-v4-pro");
    let mut duplicate = flat.clone();
    duplicate
        .model_routes
        .push(duplicate.model_routes[0].clone());
    assert!(duplicate.validate().is_err());
    let mut invalid = flat;
    invalid.model_routes[0].base_url = Some("https://api.deepseek.com/v1".into());
    assert!(invalid.validate().is_err());
}

#[test]
fn explicit_cli_route_overrides_are_revalidated_and_model_override_is_allowed() {
    let c = explicit_config(
        "provider='deepseek'\nmodel='old'\nwire_api='responses'\nauth_method='api_key'\nauth_ref='cred'",
    );
    let r = resolve(
        &ConfigOverrides {
            model: Some("new".into()),
            ..Default::default()
        },
        &c,
        &AuthFile::default(),
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(r.model.as_deref(), Some("new"));
    for overrides in [
        ConfigOverrides {
            base_url: Some("https://evil.test".into()),
            ..Default::default()
        },
        ConfigOverrides {
            wire_api: Some("openai_codex_responses".into()),
            ..Default::default()
        },
        ConfigOverrides {
            backend: Some("echo".into()),
            ..Default::default()
        },
        ConfigOverrides {
            timeout_seconds: Some(3601),
            ..Default::default()
        },
    ] {
        assert!(resolve(&overrides, &c, &AuthFile::default(), &BTreeMap::new()).is_err());
    }
}

/// Write an auth file the way zenpi does.  The credential store refuses a
/// credential file that is not owner-only, so a fixture has to be private too.
fn write_private(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[test]
fn explicit_doctor_is_unresolved_and_never_reads_legacy_auth_or_codex_fallback() {
    let temp = tempdir().unwrap();
    write_codex_fixture(temp.path(), "synthetic-codex-secret");
    let paths = ConfigPaths::for_home(temp.path());
    fs::create_dir_all(&paths.root).unwrap();
    // The credential store refuses a root it does not consider private, and an
    // explicit profile's status now consults it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&paths.root, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(
        &paths.config,
        "provider='openai-codex'\nauth_method='oauth'\nauth_ref='codex-account'\n",
    )
    .unwrap();
    // A legacy key that the explicit diagnostic path must ignore: it is
    // readable, so a report of "no API key" proves it was not consumed, rather
    // than proving only that a corrupt file was skipped.
    write_private(
        &paths.auth,
        "{\"OPENAI_API_KEY\":\"synthetic-legacy-file-secret\"}\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_zenpi"))
        .args(["config", "doctor", "--json"])
        .env_clear()
        .env("HOME", temp.path())
        .env("ZENPI_HOME", &paths.root)
        .env("CODEX_HOME", temp.path().join(".codex"))
        .env("OPENAI_API_KEY", "synthetic-ambient-secret")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let status: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{error}; stderr={}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
    assert_eq!(status["auth_method"], "oauth");
    // The profile names a credential the store does not have: that is
    // unconfigured, not "fields are complete, so assume it works".
    assert_eq!(status["auth_binding_state"], "unconfigured");
    assert_eq!(status["api_key_present"], false);
    assert_eq!(status["wire_api"], "openai_codex_responses");
    assert!(status["model"].is_null());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("synthetic-"));
}

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
fn effective_config_can_issue_policy_bound_secret_handle() {
    let config = zenpi::config::EffectiveConfig {
        profile: None,
        backend: "openai".into(),
        provider: Some("openai".into()),
        model: Some("fixture".into()),
        base_url: Some("https://example.test/v1".into()),
        wire_api: Some("chat".into()),
        auth_method: None,
        auth_ref: None,
        auth_header: None,
        model_routes: Vec::new(),
        api_key: Some("secret-config-key".into()),
        credential_source: CredentialSource::AuthFile,
        model_reasoning_effort: None,
        model_verbosity: None,
        timeout_seconds: None,
        max_retries: None,
        requires_openai_auth: true,
        supports_websockets: false,
        model_overrides: Vec::new(),
        zone_models: Default::default(),
    };
    let digest = "d".repeat(64);
    let (handle, revoke) = config.issue_secret_handle(digest.clone()).unwrap().unwrap();
    assert_eq!(handle.policy_digest(), digest);
    assert!(!format!("{handle:?}").contains("secret-config-key"));
    revoke.revoke();
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
fn config_doctor_fails_when_provider_is_not_ready_without_creating_state() {
    let home = tempdir().unwrap();
    let zenpi_home = home.path().join("zenpi");
    let output = Command::new(env!("CARGO_BIN_EXE_zenpi"))
        .args(["config", "doctor", "--json"])
        .env("HOME", home.path())
        .env("ZENPI_HOME", &zenpi_home)
        .env_remove("CODEX_HOME")
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL")
        .env_remove("ZENPI_API_KEY")
        .env_remove("ZENPI_BASE_URL")
        .env_remove("ZENPI_MODEL")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["api_key_present"], false);
    assert_eq!(report["base_url"], serde_json::Value::Null);
    assert_eq!(report["model"], serde_json::Value::Null);
    assert!(String::from_utf8_lossy(&output.stderr).contains("not ready"));
    assert!(!zenpi_home.exists());
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

#[test]
fn runtime_profile_is_a_validated_first_class_override() {
    let options = parse_args([
        "--mode",
        "tui",
        "--profile=codex_work",
        "--model",
        "gpt-test",
    ])
    .unwrap();
    assert_eq!(options.profile.as_deref(), Some("codex_work"));
    assert_eq!(options.model.as_deref(), Some("gpt-test"));

    for invalid in ["", "has space", "../escape", "line\nbreak"] {
        assert!(
            parse_args(["--profile", invalid]).is_err(),
            "accepted invalid profile {invalid:?}"
        );
    }
}

fn editor_environment(items: &[(&str, &str)]) -> BTreeMap<std::ffi::OsString, std::ffi::OsString> {
    items
        .iter()
        .map(|(k, v)| ((*k).into(), (*v).into()))
        .collect()
}

#[test]
fn editor_selection_has_no_invalid_visual_fallback_or_provider_configuration() {
    use zenpi::config::resolve_editor_command;
    let mut env = editor_environment(&[("VISUAL", "visual --wait"), ("EDITOR", "fallback")]);
    let result = resolve_editor_command(&env).unwrap();
    assert_eq!(result.source, "VISUAL");
    assert_eq!(result.argv, ["visual", "--wait"]);
    assert_eq!(result.timeout.as_secs(), 1800);
    for invalid in ["", "  \t", "'unfinished", "private-secret\nargument"] {
        env.insert("VISUAL".into(), invalid.into());
        let error = resolve_editor_command(&env).err().unwrap();
        assert!(!error.contains("private-secret"));
    }
    env.remove(std::ffi::OsStr::new("VISUAL"));
    assert_eq!(resolve_editor_command(&env).unwrap().source, "EDITOR");
    env.remove(std::ffi::OsStr::new("EDITOR"));
    assert!(resolve_editor_command(&env).is_err());
}

#[test]
fn editor_argv_quotes_escapes_and_shell_syntax_are_literal() {
    use zenpi::config::resolve_editor_command;
    let cases = [
        (
            "'/path with 空格/editor' --wait '' a\"b\"'c'",
            vec!["/path with 空格/editor", "--wait", "", "abc"],
        ),
        (
            r#"ed a\ b 'C:\Users\name' "keep\q" "\$VAR" "\`id\`" "a\\b" "a\"b""#,
            vec![
                "ed",
                "a b",
                r"C:\Users\name",
                r"keep\q",
                "$VAR",
                "`id`",
                r"a\b",
                "a\"b",
            ],
        ),
        (
            "ed $VAR ~ '*.rs' '$(touch sentinel)' ';' '|' '>'",
            vec![
                "ed",
                "$VAR",
                "~",
                "*.rs",
                "$(touch sentinel)",
                ";",
                "|",
                ">",
            ],
        ),
        ("ed 'quoted\ttab'", vec!["ed", "quoted\ttab"]),
    ];
    for (raw, expected) in cases {
        let parsed = resolve_editor_command(&editor_environment(&[("EDITOR", raw)])).unwrap();
        assert_eq!(parsed.argv, expected, "{raw}");
    }
    for raw in [
        "ed \\",
        "ed \"open",
        "'' arg",
        "ed\0arg",
        "ed\rarg",
        "ed\narg",
        "ed\u{1b}arg",
    ] {
        assert!(resolve_editor_command(&editor_environment(&[("EDITOR", raw)])).is_err());
    }
}

#[test]
fn editor_argument_and_environment_budgets_reject_without_truncation() {
    use zenpi::config::resolve_editor_command;
    for raw in [
        format!("ed {}", "a".repeat(2049)),
        "a".repeat(4097),
        format!("ed {}", "x ".repeat(32)),
    ] {
        assert!(resolve_editor_command(&editor_environment(&[("EDITOR", &raw)])).is_err());
    }
    let mut env = editor_environment(&[
        ("EDITOR", "ed"),
        ("PATH", "/usr/bin:/bin"),
        ("HOME", "/private/user home"),
        ("TERM", "xterm-256color"),
        ("OPENAI_API_KEY", "must-not-pass"),
        ("ZENPI_HOME", "/private/runtime"),
        ("SSH_AUTH_SOCK", "must-not-pass"),
        ("LC_NOT_REGISTERED", "must-not-pass"),
    ]);
    let before = env.clone();
    let command = resolve_editor_command(&env).unwrap();
    assert_eq!(
        command.environment,
        editor_environment(&[
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/private/user home"),
            ("TERM", "xterm-256color")
        ])
    );
    assert_eq!(env, before);
    env.insert("TERM".into(), "t".repeat(4097).into());
    assert!(resolve_editor_command(&env).is_err());
    let mut crowded = editor_environment(&[("EDITOR", "ed")]);
    for key in [
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "SHELL",
        "TERM",
        "COLORTERM",
        "LANG",
        "TMPDIR",
        "TERMINFO",
        "TERMINFO_DIRS",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
    ] {
        crowded.insert(key.into(), "x".repeat(4096).into());
    }
    assert!(resolve_editor_command(&crowded).is_err());
}

#[test]
fn editor_deadline_is_separate_bounded_and_never_forwarded() {
    use zenpi::config::resolve_editor_command;
    for value in ["1", "1800", "7200"] {
        let env = editor_environment(&[("EDITOR", "ed"), ("ZENPI_EDITOR_TIMEOUT_SECONDS", value)]);
        let command = resolve_editor_command(&env).unwrap();
        assert_eq!(command.timeout.as_secs(), value.parse::<u64>().unwrap());
        assert!(
            !command
                .environment
                .contains_key(std::ffi::OsStr::new("ZENPI_EDITOR_TIMEOUT_SECONDS"))
        );
    }
    for value in [
        "0",
        "7201",
        "",
        "-1",
        "1.5",
        " 2",
        "+2",
        "999999999999999999999999",
    ] {
        assert!(
            resolve_editor_command(&editor_environment(&[
                ("EDITOR", "ed"),
                ("ZENPI_EDITOR_TIMEOUT_SECONDS", value)
            ]))
            .is_err()
        );
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_visual_is_rejected_instead_of_using_editor() {
    use std::os::unix::ffi::OsStringExt;
    let mut env = editor_environment(&[("EDITOR", "fallback")]);
    env.insert("VISUAL".into(), std::ffi::OsString::from_vec(vec![0xff]));
    assert!(zenpi::config::resolve_editor_command(&env).is_err());
}

#[test]
fn zone_models_persist_and_resolve_with_a_global_fallback() {
    use zenpi::view_model::{Zone, ZoneModels};

    let home = tempdir().unwrap();
    let paths = ConfigPaths::for_home(home.path());
    let config = ConfigFile {
        model: Some("global-model".into()),
        zone_models: ZoneModels {
            discussion: Some("discussion-model".into()),
            arch: Some("arch-model".into()),
        },
        ..ConfigFile::default()
    };
    config.validate().unwrap();
    save_config(&paths, &config).unwrap();
    let loaded = load_config(&paths).unwrap();
    assert_eq!(loaded.zone_models, config.zone_models);

    let resolved = resolve(
        &ConfigOverrides::default(),
        &loaded,
        &AuthFile::default(),
        &BTreeMap::new(),
    )
    .unwrap();
    // Discussion and arch keep their independent choices; the worker pool uses
    // the global model.
    assert_eq!(
        resolved.zone_model(Zone::Discussion),
        Some("discussion-model")
    );
    assert_eq!(resolved.zone_model(Zone::Arch), Some("arch-model"));
    assert_eq!(resolved.zone_model(Zone::Worker), Some("global-model"));

    // Concurrency semantics: talk zones are single-concurrency, workers run at
    // the project-defined count (clamped to at least one).
    assert_eq!(resolved.zone_concurrency(Zone::Discussion, 8), 1);
    assert_eq!(resolved.zone_concurrency(Zone::Arch, 8), 1);
    assert_eq!(resolved.zone_concurrency(Zone::Worker, 8), 8);
    assert_eq!(resolved.zone_concurrency(Zone::Worker, 0), 1);

    // A zone with no override falls back to the global model.
    let mut partial = loaded.clone();
    partial.zone_models.discussion = None;
    let resolved = resolve(
        &ConfigOverrides::default(),
        &partial,
        &AuthFile::default(),
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(resolved.zone_model(Zone::Discussion), Some("global-model"));

    // A CLI/env model override still wins for the zones that fall back to it.
    let overrides = ConfigOverrides {
        model: Some("cli-model".into()),
        ..ConfigOverrides::default()
    };
    let resolved = resolve(&overrides, &partial, &AuthFile::default(), &BTreeMap::new()).unwrap();
    assert_eq!(resolved.zone_model(Zone::Discussion), Some("cli-model"));
}

#[test]
fn zone_models_reject_blank_control_and_unknown_identities() {
    let home = tempdir().unwrap();
    let paths = ConfigPaths::for_home(home.path());

    for invalid in ["", "   ", "bad\nmodel", "line\rmodel", &"x".repeat(257)] {
        let mut config = ConfigFile::default();
        config.zone_models.arch = Some(invalid.to_owned());
        assert!(
            config.validate().is_err(),
            "accepted invalid zone model {invalid:?}"
        );
    }

    // The durable format rejects unknown keys instead of silently ignoring an
    // uncleared or mistyped zone.
    save_config(&paths, &ConfigFile::default()).unwrap();
    fs::write(&paths.config, "[zone_models]\nworker = \"never\"\n").unwrap();
    assert!(load_config(&paths).is_err());
}

#[test]
fn short_session_reference_parses_without_changing_the_path_form() {
    let options = parse_args(["-s", "my-session"]).unwrap();
    assert_eq!(options.session_ref.as_deref(), Some("my-session"));
    let options = parse_args(["-s=other"]).unwrap();
    assert_eq!(options.session_ref.as_deref(), Some("other"));
    let options = parse_args(["--session", "/tmp/journal.jsonl"]).unwrap();
    assert_eq!(options.session_ref, None);
    assert!(options.session.ends_with("journal.jsonl"));
    assert!(parse_args(["-s", "  "]).is_err());
}

/// Run the built binary against an isolated home, so the test exercises the
/// same command entry point a user does.  `HOME`, `ZENPI_HOME`, and
/// `CODEX_HOME` all point inside the temporary directory and the environment is
/// otherwise cleared, so no user configuration or credential can be reached.
fn zenpi(home: &Path) -> Command {
    let root = home.canonicalize().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_zenpi"));
    command
        .env_clear()
        .env("HOME", &root)
        .env("ZENPI_HOME", root.join(".zenpi"))
        .env("CODEX_HOME", root.join("codex"))
        .current_dir(&root);
    command
}

fn run(command: &mut Command, stdin: &str) -> (bool, String, String) {
    use std::io::Write as _;
    use std::process::Stdio;
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn auth_list_is_empty_and_read_only_before_any_credential_exists() {
    let home = tempdir().unwrap();
    let (ok, stdout, _) = run(
        zenpi(home.path()).args(["config", "auth", "list", "--json"]),
        "",
    );
    assert!(ok, "listing an absent store must succeed");
    assert_eq!(stdout.trim(), "[]");
    // A read-only command must not create the state directory.
    assert!(!home.path().join(".zenpi").exists());
}

#[test]
fn adding_an_api_key_stores_a_scoped_credential_and_never_echoes_the_key() {
    let home = tempdir().unwrap();
    let secret = "synthetic-cli-secret-key";
    let (ok, stdout, stderr) = run(
        zenpi(home.path()).args([
            "config",
            "add",
            "auth",
            "apikey",
            "https://api.deepseek.com",
            "deepseek",
            "--stdin",
            "--model",
            "deepseek-flash",
            "--json",
        ]),
        &format!("{secret}\n"),
    );
    assert!(ok, "add failed: {stderr}");
    assert!(!stdout.contains(secret), "the key must never reach stdout");
    assert!(!stderr.contains(secret), "the key must never reach stderr");

    let report: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let credential_id = report["credential_id"].as_str().unwrap().to_owned();
    assert_eq!(report["credential_committed"], true);
    assert_eq!(report["profile_bound"], true);
    assert_eq!(report["binding_error"], serde_json::Value::Null);
    // A built-in multi-protocol provider authorizes every wire it serves, so
    // the key works on all of them without a second login.
    let mut prefixes: Vec<&str> = report["destinations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|grant| grant["path_prefix"].as_str().unwrap())
        .collect();
    prefixes.sort_unstable();
    assert_eq!(
        prefixes,
        ["/anthropic/v1/messages", "/chat/completions", "/responses"]
    );

    let paths = ConfigPaths::for_home(home.path());
    let stored = fs::read_to_string(&paths.auth).unwrap();
    assert!(
        stored.contains(secret),
        "the credential is stored, not dropped"
    );
    assert_eq!(load_auth(&paths).unwrap().openai_api_key(), None);
    assert!(
        load_auth(&paths)
            .unwrap()
            .api_key_for_profile(Some("deepseek"))
            .is_none(),
        "a stored credential is not also written as a legacy profile key"
    );

    let entries = zenpi::config::auth_list(&paths).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].credential_id, credential_id);
    assert_eq!(entries[0].state, "ready");
    assert_eq!(entries[0].profiles, ["deepseek"]);

    let profile = load_config(&paths).unwrap().profiles["deepseek"].clone();
    assert_eq!(profile.auth_method.as_deref(), Some("api_key"));
    assert_eq!(profile.auth_ref.as_deref(), Some(credential_id.as_str()));
    assert_eq!(profile.provider.as_deref(), Some("deepseek"));
    assert_eq!(profile.model.as_deref(), Some("deepseek-flash"));
    assert_eq!(profile.wire_api.as_deref(), Some("chat_completions"));
    // The URL of a built-in provider comes from its definition.
    assert_eq!(profile.base_url, None);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&paths.auth).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the credential file stays owner-only");
    }
}

#[test]
fn api_keys_are_refused_from_the_argument_list_or_a_non_https_endpoint() {
    let home = tempdir().unwrap();
    // There is no flag that accepts a key, so an attempt to pass one is just an
    // unsupported argument rather than a silent acceptance.
    let (ok, _, stderr) = run(
        zenpi(home.path()).args([
            "config",
            "add",
            "auth",
            "apikey",
            "https://api.deepseek.com",
            "deepseek",
            "--api-key",
            "synthetic-argv-secret",
        ]),
        "",
    );
    assert!(!ok);
    assert!(!stderr.contains("synthetic-argv-secret"));
    assert!(!home.path().join(".zenpi").exists());

    let (ok, _, stderr) = run(
        zenpi(home.path()).args([
            "config",
            "add",
            "auth",
            "apikey",
            "https://api.deepseek.com",
            "deepseek",
        ]),
        "synthetic-secret\n",
    );
    assert!(!ok, "--stdin is required");
    assert!(stderr.contains("never taken from the argument list"));
    assert!(!home.path().join(".zenpi").exists());

    let (ok, _, stderr) = run(
        zenpi(home.path()).args([
            "config",
            "add",
            "auth",
            "apikey",
            "http://gateway.test/v1",
            "custom",
            "--stdin",
            "--wire",
            "responses",
            "--header",
            "bearer",
        ]),
        "synthetic-secret\n",
    );
    assert!(!ok);
    assert!(stderr.contains("require HTTPS"));

    // A built-in name may not be pointed at someone else's service.
    let (ok, _, stderr) = run(
        zenpi(home.path()).args([
            "config",
            "add",
            "auth",
            "apikey",
            "https://not-deepseek.test",
            "deepseek",
            "--stdin",
        ]),
        "synthetic-secret\n",
    );
    assert!(!ok);
    assert!(stderr.contains("must address that provider's own service"));
}

#[test]
fn revoking_a_profile_revokes_its_whole_credential_and_reports_the_scope() {
    let home = tempdir().unwrap();
    let paths = ConfigPaths::for_home(home.path());

    let (ok, _, stderr) = run(
        zenpi(home.path()).args([
            "config",
            "add",
            "auth",
            "apikey",
            "https://gateway.test/v1",
            "custom",
            "--stdin",
            "--wire",
            "responses",
            "--header",
            "bearer",
            "--alias",
            "first",
        ]),
        "synthetic-secret\n",
    );
    assert!(ok, "add failed: {stderr}");

    // Two profiles sharing one credential: revoking either must report the
    // other as affected rather than silently changing a single binding.
    let mut config = load_config(&paths).unwrap();
    let shared = config.profiles["first"].auth_ref.clone().unwrap();
    let mut second = config.profiles["first"].clone();
    second.auth_ref = Some(shared.clone());
    config.profiles.insert("second".into(), second);
    save_config(&paths, &config).unwrap();

    let (ok, stdout, stderr) = run(
        zenpi(home.path()).args(["pair", "revoke", "--profile", "first", "--yes", "--json"]),
        "",
    );
    assert!(ok, "revoke failed: {stderr}");
    let receipt: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(receipt["credential_id"], shared.as_str());
    assert_eq!(receipt["local_revoked"], true);
    assert_eq!(receipt["remote_revoked"], false);
    assert_eq!(receipt["profiles"], serde_json::json!(["first", "second"]));

    let entries = zenpi::config::auth_list(&paths).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].state, "revoked");
    // Revoking is not unbinding: the profiles still name the credential, which
    // is how the operator sees why the connection stopped working.
    assert_eq!(entries[0].profiles, ["first", "second"]);
    assert_eq!(load_config(&paths).unwrap().profiles.len(), 2);

    // The tombstone keeps the identifier from being revived by a later add.
    assert!(
        zenpi::config::profile_credential(&paths, "first")
            .unwrap()
            .is_some()
    );
}

#[test]
fn auth_management_commands_reject_unrecognized_flags() {
    // A stray flag must never be absorbed as an optional positional argument:
    // for `add auth codex` that positional is an email, and absorbing it would
    // start a real login on a typo.
    for arguments in [
        vec!["config", "add", "auth", "codex", "--json"],
        vec!["config", "add", "auth", "codex", "--bogus"],
        vec!["config", "auth", "bogus"],
        vec!["config", "add", "bogus"],
        vec!["config", "add", "auth"],
    ] {
        assert!(
            parse_args(arguments.clone()).is_err(),
            "{arguments:?} must be rejected"
        );
    }
    assert!(parse_args(["config", "add", "auth", "codex", "--device", "--no-browser"]).is_err());
    assert!(parse_args(["config", "auth", "list", "--json"]).is_ok());
    assert!(parse_args(["config", "auth", "list", "--profile", "x"]).is_err());
    let options = parse_args([
        "config",
        "add",
        "auth",
        "apikey",
        "https://gateway.test/v1",
        "custom",
        "--stdin",
        "--alias",
        "gateway",
        "--model",
        "gateway-model",
    ])
    .unwrap();
    assert_eq!(
        options.command_value.as_deref(),
        Some("https://gateway.test/v1")
    );
    assert_eq!(options.command_value2.as_deref(), Some("custom"));
    assert_eq!(options.alias.as_deref(), Some("gateway"));
    assert_eq!(options.model.as_deref(), Some("gateway-model"));
    assert!(options.stdin);
}

#[test]
fn a_corrupt_credential_store_is_reported_rather_than_read_as_unconfigured() {
    let home = tempdir().unwrap();
    let paths = ConfigPaths::for_home(home.path());
    fs::create_dir_all(&paths.root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&paths.root, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(
        &paths.config,
        "provider='openai-codex'\nauth_method='oauth'\nauth_ref='codex-account'\n",
    )
    .unwrap();
    // Bad JSON must not be treated as an empty store: a profile would then look
    // merely unconfigured while the real file is unreadable.
    write_private(&paths.auth, "not legacy JSON");

    let (ok, stdout, stderr) = run(zenpi(home.path()).args(["config", "doctor", "--json"]), "");
    assert!(!ok);
    assert!(
        stdout.trim().is_empty(),
        "no status is claimed for a corrupt store"
    );
    assert!(
        stderr.contains("credential store"),
        "the reason must name the store: {stderr}"
    );
    assert!(!stderr.contains("synthetic-"));
}

#[test]
fn auto_flag_parses_and_rejects_inline_values() {
    let options = parse_args(["--auto"]).unwrap();
    assert!(options.auto);
    assert!(parse_args(["--auto=yes"]).is_err());
    let options = parse_args(["--session", "x.jsonl"]).unwrap();
    assert!(!options.auto);
}

#[test]
fn max_tool_iterations_flag_is_bounded_and_defaults_high() {
    let options = parse_args(["--auto"]).unwrap();
    assert_eq!(options.max_tool_iterations, 1000);
    let options = parse_args(["--max-tool-iterations", "2500"]).unwrap();
    assert_eq!(options.max_tool_iterations, 2500);
    assert!(parse_args(["--max-tool-iterations", "0"]).is_err());
    assert!(parse_args(["--max-tool-iterations", "10001"]).is_err());
    assert!(parse_args(["--max-tool-iterations", "many"]).is_err());
}
