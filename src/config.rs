//! Persistent configuration and credential discovery for zenpi.
//!
//! The command line keeps its normal precedence rule explicit and boring:
//! command-line overrides win over environment variables, which win over
//! `~/.zenpi/config.toml`, which win over built-in defaults.  Credentials are
//! never written to the TOML file.  They live in `~/.zenpi/auth.json` (mode
//! `0600` on Unix) and are only imported from the user's Codex auth file when
//! `pair_from_codex` is requested.

use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// The directory name created below the user's home directory.
pub const ZENPI_DIR: &str = ".zenpi";
pub const CONFIG_FILE: &str = "config.toml";
pub const AUTH_FILE: &str = "auth.json";
pub const SESSIONS_DIR: &str = "sessions";
pub const SKILLS_DIR: &str = "skills";
pub const EXTENSIONS_DIR: &str = "extensions";
pub const OPENAI_API_KEY: &str = "OPENAI_API_KEY";

/// Paths for the zenpi configuration and auth files.  Tests can construct
/// this with `for_home`; normal callers should use `discover`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub auth: PathBuf,
    pub sessions: PathBuf,
    pub skills: PathBuf,
    pub extensions: PathBuf,
}

impl ConfigPaths {
    pub fn for_home(home: impl AsRef<Path>) -> Self {
        let root = home.as_ref().join(ZENPI_DIR);
        Self {
            config: root.join(CONFIG_FILE),
            auth: root.join(AUTH_FILE),
            sessions: root.join(SESSIONS_DIR),
            skills: root.join(SKILLS_DIR),
            extensions: root.join(EXTENSIONS_DIR),
            root,
        }
    }

    pub fn discover() -> Result<Self, ConfigError> {
        if let Some(root) = env::var_os("ZENPI_HOME") {
            let root = PathBuf::from(root);
            return Ok(Self {
                config: root.join(CONFIG_FILE),
                auth: root.join(AUTH_FILE),
                sessions: root.join(SESSIONS_DIR),
                skills: root.join(SKILLS_DIR),
                extensions: root.join(EXTENSIONS_DIR),
                root,
            });
        }
        Ok(Self::for_home(home_dir()?))
    }

    /// Create the directory with owner-only permissions and tighten an
    /// existing directory before any credential file is read or written.
    pub fn ensure_root(&self) -> Result<(), ConfigError> {
        reject_symlink(&self.root)?;
        if !self.root.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                fs::DirBuilder::new()
                    .mode(0o700)
                    .recursive(true)
                    .create(&self.root)?;
            }
            #[cfg(not(unix))]
            fs::create_dir_all(&self.root)?;
        }
        let metadata = fs::metadata(&self.root)?;
        if !metadata.is_dir() {
            return Err(ConfigError::NotDirectory(self.root.clone()));
        }
        restrict_permissions(&self.root, 0o700)?;
        for directory in [&self.sessions, &self.skills, &self.extensions] {
            reject_symlink(directory)?;
            if !directory.exists() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    fs::DirBuilder::new().mode(0o700).create(directory)?;
                }
                #[cfg(not(unix))]
                fs::create_dir(directory)?;
            }
            if !fs::metadata(directory)?.is_dir() {
                return Err(ConfigError::NotDirectory(directory.to_path_buf()));
            }
            restrict_permissions(directory, 0o700)?;
        }
        Ok(())
    }
}

/// The non-secret TOML representation.  `api_key` is deliberately absent:
/// putting a key in config.toml makes accidental logging and source control
/// leaks much too easy.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire_api: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_verbosity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_openai_auth: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_websockets: Option<bool>,
}

impl ConfigFile {
    pub fn validate(&self) -> Result<(), ConfigError> {
        validate_optional("backend", self.backend.as_deref(), 64)?;
        validate_optional("provider", self.provider.as_deref(), 128)?;
        validate_optional("model", self.model.as_deref(), 256)?;
        validate_optional("base_url", self.base_url.as_deref(), 2048)?;
        validate_optional("wire_api", self.wire_api.as_deref(), 64)?;
        validate_optional("auth_env", self.auth_env.as_deref(), 128)?;
        validate_optional(
            "model_reasoning_effort",
            self.model_reasoning_effort.as_deref(),
            64,
        )?;
        validate_optional("model_verbosity", self.model_verbosity.as_deref(), 64)?;
        if self
            .timeout_seconds
            .is_some_and(|seconds| !(1..=3600).contains(&seconds))
        {
            return Err(ConfigError::Invalid(
                "timeout_seconds must be between 1 and 3600".into(),
            ));
        }
        if self.max_retries.is_some_and(|retries| retries > 10) {
            return Err(ConfigError::Invalid(
                "max_retries must be at most 10".into(),
            ));
        }
        if self
            .backend
            .as_deref()
            .is_some_and(|backend| !matches!(backend, "echo" | "openai"))
        {
            return Err(ConfigError::Invalid(
                "backend must be `echo` or `openai`".into(),
            ));
        }
        if let Some(base_url) = self.base_url.as_deref()
            && (!base_url.starts_with("http://") && !base_url.starts_with("https://")
                || base_url.contains(['?', '#'])
                || base_url.chars().any(char::is_whitespace))
        {
            return Err(ConfigError::Invalid(
                "base_url must be an http or https URL without query or fragment".into(),
            ));
        }
        Ok(())
    }
}

/// JSON auth storage is intentionally a map so pairing preserves credentials
/// for other providers/tools.  Only `OPENAI_API_KEY` is interpreted by zenpi.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct AuthFile(pub BTreeMap<String, Value>);

impl std::fmt::Debug for AuthFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys: Vec<&str> = self.0.keys().map(String::as_str).collect();
        formatter
            .debug_struct("AuthFile")
            .field("keys", &keys)
            .field("openai_api_key_present", &self.openai_api_key().is_some())
            .finish()
    }
}

impl AuthFile {
    pub fn openai_api_key(&self) -> Option<&str> {
        self.0.get(OPENAI_API_KEY).and_then(Value::as_str)
    }

    pub fn set_openai_api_key(&mut self, key: impl Into<String>) -> Result<(), ConfigError> {
        let key = key.into();
        if key.trim().is_empty() || key.chars().any(char::is_control) || key.len() > 16 * 1024 {
            return Err(ConfigError::Invalid(
                "OPENAI_API_KEY must be non-empty, bounded, and contain no control characters"
                    .into(),
            ));
        }
        self.0.insert(OPENAI_API_KEY.to_owned(), Value::String(key));
        Ok(())
    }

    pub fn remove_openai_api_key(&mut self) -> bool {
        self.0.remove(OPENAI_API_KEY).is_some()
    }
}

/// Explicit command-line overrides.  `None` means no override, not an empty
/// value.  The CLI parser can fill this directly without reading secrets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigOverrides {
    pub backend: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub wire_api: Option<String>,
    pub api_key: Option<String>,
    pub model_reasoning_effort: Option<String>,
    pub model_verbosity: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub max_retries: Option<u32>,
}

/// Where the selected credential came from.  This enum is safe to display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    CommandLine,
    Environment,
    AuthFile,
    None,
}

/// Fully resolved runtime configuration.  The API key is private in logs via
/// the custom Debug implementation, but remains available to backend setup.
#[derive(Clone, PartialEq, Eq)]
pub struct EffectiveConfig {
    pub backend: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub wire_api: Option<String>,
    pub api_key: Option<String>,
    pub credential_source: CredentialSource,
    pub model_reasoning_effort: Option<String>,
    pub model_verbosity: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub max_retries: Option<u32>,
    pub requires_openai_auth: bool,
    pub supports_websockets: bool,
}

impl std::fmt::Debug for EffectiveConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EffectiveConfig")
            .field("backend", &self.backend)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field("wire_api", &self.wire_api)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("credential_source", &self.credential_source)
            .field("model_reasoning_effort", &self.model_reasoning_effort)
            .field("model_verbosity", &self.model_verbosity)
            .field("timeout_seconds", &self.timeout_seconds)
            .field("max_retries", &self.max_retries)
            .field("requires_openai_auth", &self.requires_openai_auth)
            .field("supports_websockets", &self.supports_websockets)
            .finish()
    }
}

/// Resolve CLI > environment > config > defaults.  This map-based function
/// keeps precedence deterministic and testable without mutating process-wide
/// environment variables.
pub fn resolve(
    overrides: &ConfigOverrides,
    config: &ConfigFile,
    auth: &AuthFile,
    environment: &BTreeMap<String, String>,
) -> Result<EffectiveConfig, ConfigError> {
    config.validate()?;
    let backend = choose(
        overrides.backend.as_deref(),
        environment.get("ZENPI_BACKEND").map(String::as_str),
        config.backend.as_deref(),
        "openai",
    );
    let provider = choose_optional(
        overrides.provider.as_deref(),
        environment.get("ZENPI_PROVIDER").map(String::as_str),
        config.provider.as_deref(),
    );
    let model = choose_optional(
        overrides.model.as_deref(),
        environment
            .get("ZENPI_MODEL")
            .or_else(|| environment.get("OPENAI_MODEL"))
            .map(String::as_str),
        config.model.as_deref(),
    );
    let base_url = choose_optional(
        overrides.base_url.as_deref(),
        environment
            .get("ZENPI_BASE_URL")
            .or_else(|| environment.get("OPENAI_BASE_URL"))
            .map(String::as_str),
        config.base_url.as_deref(),
    );
    let wire_api = choose_optional(
        overrides.wire_api.as_deref(),
        environment.get("ZENPI_WIRE_API").map(String::as_str),
        config.wire_api.as_deref(),
    );
    let model_reasoning_effort = choose_optional(
        overrides.model_reasoning_effort.as_deref(),
        environment
            .get("ZENPI_MODEL_REASONING_EFFORT")
            .map(String::as_str),
        config.model_reasoning_effort.as_deref(),
    );
    let model_verbosity = choose_optional(
        overrides.model_verbosity.as_deref(),
        environment.get("ZENPI_MODEL_VERBOSITY").map(String::as_str),
        config.model_verbosity.as_deref(),
    );
    let timeout_seconds = overrides
        .timeout_seconds
        .or_else(|| {
            environment
                .get("ZENPI_TIMEOUT_SECONDS")
                .and_then(|value| value.parse().ok())
        })
        .or(config.timeout_seconds);
    let max_retries = overrides
        .max_retries
        .or_else(|| {
            environment
                .get("ZENPI_MAX_RETRIES")
                .and_then(|value| value.parse().ok())
        })
        .or(config.max_retries);
    let requires_openai_auth = config.requires_openai_auth.unwrap_or(true);
    let supports_websockets = config.supports_websockets.unwrap_or(false);
    let (api_key, credential_source) = if let Some(key) = overrides.api_key.clone() {
        (Some(key), CredentialSource::CommandLine)
    } else if let Some(key) = environment
        .get("ZENPI_API_KEY")
        .or_else(|| environment.get("OPENAI_API_KEY"))
        .or_else(|| {
            config
                .auth_env
                .as_deref()
                .and_then(|name| environment.get(name))
        })
    {
        (Some(key.clone()), CredentialSource::Environment)
    } else {
        (
            auth.openai_api_key().map(str::to_owned),
            if auth.openai_api_key().is_some() {
                CredentialSource::AuthFile
            } else {
                CredentialSource::None
            },
        )
    };
    if let Some(key) = &api_key
        && (key.trim().is_empty() || key.chars().any(char::is_control))
    {
        return Err(ConfigError::Invalid("API key is invalid".into()));
    }
    Ok(EffectiveConfig {
        backend,
        provider,
        model,
        base_url,
        wire_api,
        api_key,
        credential_source,
        model_reasoning_effort,
        model_verbosity,
        timeout_seconds,
        max_retries,
        requires_openai_auth,
        supports_websockets,
    })
}

/// Resolve the files in the default `~/.zenpi` directory using the current
/// process environment.
pub fn resolve_default(overrides: &ConfigOverrides) -> Result<EffectiveConfig, ConfigError> {
    let paths = ConfigPaths::discover()?;
    let mut config = load_config(&paths)?;
    let mut auth = load_auth(&paths)?;
    // A fresh zenpi install should work with the provider the user already
    // configured for Codex. This fallback is read-only; `config import-codex`
    // remains the explicit persistence command.
    if (config.base_url.is_none() || config.model.is_none() || auth.openai_api_key().is_none())
        && let Ok(import) = import_codex_from_root(codex_root()?)
    {
        if config.provider.is_none() {
            config.provider = import.config.provider;
        }
        if config.model.is_none() {
            config.model = import.config.model;
        }
        if config.base_url.is_none() {
            config.base_url = import.config.base_url;
        }
        if config.wire_api.is_none() {
            config.wire_api = import.config.wire_api;
        }
        if config.model_reasoning_effort.is_none() {
            config.model_reasoning_effort = import.config.model_reasoning_effort;
        }
        if config.model_verbosity.is_none() {
            config.model_verbosity = import.config.model_verbosity;
        }
        if config.backend.is_none() {
            config.backend = import.config.backend;
        }
        if config.requires_openai_auth.is_none() {
            config.requires_openai_auth = import.config.requires_openai_auth;
        }
        if config.supports_websockets.is_none() {
            config.supports_websockets = import.config.supports_websockets;
        }
        if auth.openai_api_key().is_none()
            && let Some(key) = import.api_key
        {
            auth.set_openai_api_key(key)?;
        }
    }
    let environment = env::vars().collect::<BTreeMap<_, _>>();
    resolve(overrides, &config, &auth, &environment)
}

/// Result of importing Codex settings.  The key is retained for writing but
/// is deliberately absent from Debug output and status/reporting values.
#[derive(Clone, PartialEq, Eq)]
pub struct CodexImport {
    pub config: ConfigFile,
    pub api_key: Option<String>,
    pub source_config: PathBuf,
    pub source_auth: PathBuf,
}

impl std::fmt::Debug for CodexImport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CodexImport")
            .field("config", &self.config)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("source_config", &self.source_config)
            .field("source_auth", &self.source_auth)
            .finish()
    }
}

/// Read the provider/model/endpoint from `~/.codex/config.toml` and the
/// `OPENAI_API_KEY` credential from `~/.codex/auth.json` without printing it.
pub fn import_codex_from_home(home: impl AsRef<Path>) -> Result<CodexImport, ConfigError> {
    import_codex_from_root(home.as_ref().join(".codex"))
}

fn import_codex_from_root(codex_root: impl AsRef<Path>) -> Result<CodexImport, ConfigError> {
    let codex_root = codex_root.as_ref();
    let source_config = codex_root.join(CONFIG_FILE);
    let source_auth = codex_root.join(AUTH_FILE);
    reject_symlink(&source_config)?;
    reject_symlink(&source_auth)?;
    let config_text = fs::read_to_string(&source_config).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            ConfigError::MissingCodex(source_config.clone())
        } else {
            ConfigError::Io(error)
        }
    })?;
    let root: toml::Value = toml::from_str(&config_text)?;
    let provider_name = root
        .get("model_provider")
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let provider_table = root
        .get("model_providers")
        .and_then(toml::Value::as_table)
        .and_then(|providers| {
            provider_name
                .as_deref()
                .and_then(|name| providers.get(name))
                .or_else(|| providers.get("OpenAI"))
                .or_else(|| providers.values().next())
        })
        .and_then(toml::Value::as_table);
    let model = root
        .get("model")
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let base_url = provider_table
        .and_then(|table| table.get("base_url"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let wire_api = provider_table
        .and_then(|table| table.get("wire_api"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let model_reasoning_effort = root
        .get("model_reasoning_effort")
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let model_verbosity = root
        .get("model_verbosity")
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let timeout_seconds = root
        .get("request_timeout_seconds")
        .or_else(|| root.get("timeout_seconds"))
        .and_then(toml::Value::as_integer)
        .and_then(|value| u64::try_from(value).ok());
    let max_retries = root
        .get("max_retries")
        .and_then(toml::Value::as_integer)
        .and_then(|value| u32::try_from(value).ok());
    let requires_openai_auth = provider_table
        .and_then(|table| table.get("requires_openai_auth"))
        .and_then(toml::Value::as_bool);
    let supports_websockets = provider_table
        .and_then(|table| table.get("supports_websockets"))
        .and_then(toml::Value::as_bool);
    let provider = provider_name.or_else(|| {
        provider_table
            .and_then(|table| table.get("name"))
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
    });
    let config = ConfigFile {
        backend: Some("openai".into()),
        provider,
        model,
        base_url,
        wire_api,
        auth_env: Some(OPENAI_API_KEY.into()),
        model_reasoning_effort,
        model_verbosity,
        timeout_seconds,
        max_retries,
        requires_openai_auth,
        supports_websockets,
    };
    config.validate()?;
    let api_key = match fs::read_to_string(&source_auth) {
        Ok(auth_text) => {
            let value: Value = serde_json::from_str(&auth_text)?;
            find_api_key(&value, config.provider.as_deref())
                .filter(|key| !key.trim().is_empty())
                .map(str::to_owned)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(ConfigError::Io(error)),
    };
    Ok(CodexImport {
        config,
        api_key,
        source_config,
        source_auth,
    })
}

/// Pair zenpi with Codex.  Existing non-secret settings and unrelated auth
/// keys are preserved; repeated calls are byte-idempotent and report
/// `changed == false` after the first successful import.
pub fn pair_from_codex(paths: &ConfigPaths) -> Result<PairReport, ConfigError> {
    let home = paths
        .root
        .parent()
        .ok_or_else(|| ConfigError::Invalid("zenpi root has no home parent".into()))?;
    pair_from_codex_with_source(paths, home)
}

/// Command-facing Codex pairing helper.  It intentionally performs the
/// import (rather than merely parsing Codex files), so `zenpi config
/// import-codex` is useful on a fresh machine and safe to repeat.
pub fn import_codex() -> Result<ConfigSummary, ConfigError> {
    let paths = ConfigPaths::discover()?;
    let pair = pair_from_codex_with_root(&paths, codex_root()?)?;
    let status = status(&paths)?;
    Ok(ConfigSummary {
        operation: "import-codex".into(),
        changed: Some(pair.changed),
        status,
    })
}

fn pair_from_codex_with_source(
    paths: &ConfigPaths,
    codex_home: impl AsRef<Path>,
) -> Result<PairReport, ConfigError> {
    pair_from_codex_with_root(paths, codex_home.as_ref().join(".codex"))
}

fn pair_from_codex_with_root(
    paths: &ConfigPaths,
    codex_root: impl AsRef<Path>,
) -> Result<PairReport, ConfigError> {
    let import = import_codex_from_root(codex_root)?;
    paths.ensure_root()?;
    let mut config = load_config(paths)?;
    // Import only fields actually present in Codex.  A partial Codex profile
    // must not erase a hand-edited zenpi setting.
    if import.config.backend.is_some() {
        config.backend = import.config.backend;
    }
    if import.config.provider.is_some() {
        config.provider = import.config.provider;
    }
    if import.config.model.is_some() {
        config.model = import.config.model;
    }
    if import.config.base_url.is_some() {
        config.base_url = import.config.base_url;
    }
    if import.config.wire_api.is_some() {
        config.wire_api = import.config.wire_api;
    }
    if import.config.auth_env.is_some() {
        config.auth_env = import.config.auth_env;
    }
    if import.config.model_reasoning_effort.is_some() {
        config.model_reasoning_effort = import.config.model_reasoning_effort;
    }
    if import.config.model_verbosity.is_some() {
        config.model_verbosity = import.config.model_verbosity;
    }
    if import.config.timeout_seconds.is_some() {
        config.timeout_seconds = import.config.timeout_seconds;
    }
    if import.config.max_retries.is_some() {
        config.max_retries = import.config.max_retries;
    }
    if import.config.requires_openai_auth.is_some() {
        config.requires_openai_auth = import.config.requires_openai_auth;
    }
    if import.config.supports_websockets.is_some() {
        config.supports_websockets = import.config.supports_websockets;
    }
    config.validate()?;
    let mut auth = load_auth(paths)?;
    let key_imported = if let Some(key) = import.api_key {
        let changed = auth.openai_api_key() != Some(key.as_str());
        auth.set_openai_api_key(key)?;
        changed
    } else {
        false
    };
    let config_changed = write_config_if_changed(paths, &config)?;
    let auth_changed = write_auth_if_changed(paths, &auth)?;
    let backend = config.backend.clone().unwrap_or_else(|| "openai".into());
    Ok(PairReport {
        changed: config_changed || auth_changed,
        config_changed,
        auth_changed,
        key_imported,
        backend,
        provider: config.provider,
        model: config.model,
        base_url: config.base_url,
        wire_api: config.wire_api,
    })
}

/// Command-facing, non-mutating diagnostics.  The returned summary contains
/// only provider metadata and credential presence/source, never a key.
pub fn doctor() -> Result<ConfigSummary, ConfigError> {
    let paths = ConfigPaths::discover()?;
    Ok(ConfigSummary {
        operation: "doctor".into(),
        changed: None,
        status: status(&paths)?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigSummary {
    pub operation: String,
    pub changed: Option<bool>,
    pub status: ConfigStatus,
}

impl ConfigSummary {
    /// Human-readable and secret-free output for command-line use.  URLs are
    /// intentionally retained because endpoint configuration is actionable;
    /// API credentials are represented only by presence/source.
    pub fn display(&self) -> String {
        let changed = self
            .changed
            .map_or_else(String::new, |value| format!(" changed={value}"));
        format!(
            "zenpi {operation}:{changed} backend={backend} provider={provider} model={model} base_url={base_url} wire_api={wire_api} api_key={key}",
            operation = self.operation,
            changed = changed,
            backend = self.status.backend,
            provider = self.status.provider.as_deref().unwrap_or("-"),
            model = self.status.model.as_deref().unwrap_or("-"),
            base_url = self
                .status
                .base_url
                .as_deref()
                .map(redacted_endpoint)
                .as_deref()
                .unwrap_or("-"),
            wire_api = self.status.wire_api.as_deref().unwrap_or("-"),
            key = if self.status.api_key_present {
                match self.status.api_key_source {
                    CredentialSource::CommandLine => "present(command_line)",
                    CredentialSource::Environment => "present(environment)",
                    CredentialSource::AuthFile => "present(auth_file)",
                    CredentialSource::None => "present",
                }
            } else {
                "missing"
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairReport {
    pub changed: bool,
    pub config_changed: bool,
    pub auth_changed: bool,
    pub key_imported: bool,
    pub backend: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub wire_api: Option<String>,
}

/// Redacted status data suitable for a `zenpi status` command.  It never
/// carries the key itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigStatus {
    pub config_exists: bool,
    pub auth_exists: bool,
    pub backend: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub wire_api: Option<String>,
    pub api_key_present: bool,
    pub api_key_source: CredentialSource,
    pub requires_openai_auth: bool,
    pub supports_websockets: bool,
}

pub fn status(paths: &ConfigPaths) -> Result<ConfigStatus, ConfigError> {
    let config_exists = paths.config.exists();
    let auth_exists = paths.auth.exists();
    // Use the same read-only Codex fallback as runtime resolution for the
    // process-wide profile.  Explicit test/custom paths remain isolated and
    // report only the files represented by that path.
    let resolved = if ConfigPaths::discover().ok().as_ref() == Some(paths) {
        resolve_default(&ConfigOverrides::default())?
    } else {
        let config = load_config(paths)?;
        let auth = load_auth(paths)?;
        let environment = env::vars().collect::<BTreeMap<_, _>>();
        resolve(&ConfigOverrides::default(), &config, &auth, &environment)?
    };
    Ok(ConfigStatus {
        config_exists,
        auth_exists,
        backend: resolved.backend,
        provider: resolved.provider,
        model: resolved.model,
        base_url: resolved.base_url,
        wire_api: resolved.wire_api,
        api_key_present: resolved.api_key.is_some(),
        api_key_source: resolved.credential_source,
        requires_openai_auth: resolved.requires_openai_auth,
        supports_websockets: resolved.supports_websockets,
    })
}

/// Return a path inside the configured zenpi root. Reject absolute paths and
/// parent traversal so session/skill/extension commands cannot escape it.
pub fn scoped_path(
    paths: &ConfigPaths,
    relative: impl AsRef<Path>,
) -> Result<PathBuf, ConfigError> {
    let relative = relative.as_ref();
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(ConfigError::Invalid(
            "path must stay inside ~/.zenpi".into(),
        ));
    }
    Ok(paths.root.join(relative))
}

pub fn load_config(paths: &ConfigPaths) -> Result<ConfigFile, ConfigError> {
    match fs::symlink_metadata(&paths.config) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ConfigFile::default());
        }
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    secure_regular_file(&paths.config)?;
    let text = fs::read_to_string(&paths.config)?;
    let config: ConfigFile = toml::from_str(&text)?;
    config.validate()?;
    Ok(config)
}

pub fn load_auth(paths: &ConfigPaths) -> Result<AuthFile, ConfigError> {
    match fs::symlink_metadata(&paths.auth) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(AuthFile::default());
        }
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    secure_regular_file(&paths.auth)?;
    let text = fs::read_to_string(&paths.auth)?;
    let auth: AuthFile = serde_json::from_str(&text)?;
    if let Some(key) = auth.openai_api_key()
        && (key.trim().is_empty() || key.chars().any(char::is_control))
    {
        return Err(ConfigError::Invalid("OPENAI_API_KEY is invalid".into()));
    }
    Ok(auth)
}

pub fn save_config(paths: &ConfigPaths, config: &ConfigFile) -> Result<bool, ConfigError> {
    config.validate()?;
    paths.ensure_root()?;
    let text = toml::to_string_pretty(config)?;
    atomic_write_if_changed(&paths.config, text.as_bytes())
}

pub fn save_auth(paths: &ConfigPaths, auth: &AuthFile) -> Result<bool, ConfigError> {
    if let Some(key) = auth.openai_api_key()
        && (key.trim().is_empty() || key.chars().any(char::is_control))
    {
        return Err(ConfigError::Invalid("OPENAI_API_KEY is invalid".into()));
    }
    paths.ensure_root()?;
    let text = serde_json::to_string_pretty(auth)? + "\n";
    atomic_write_if_changed(&paths.auth, text.as_bytes())
}

fn write_config_if_changed(paths: &ConfigPaths, config: &ConfigFile) -> Result<bool, ConfigError> {
    config.validate()?;
    paths.ensure_root()?;
    let text = toml::to_string_pretty(config)?;
    atomic_write_if_changed(&paths.config, text.as_bytes())
}

fn write_auth_if_changed(paths: &ConfigPaths, auth: &AuthFile) -> Result<bool, ConfigError> {
    if let Some(key) = auth.openai_api_key()
        && (key.trim().is_empty() || key.chars().any(char::is_control))
    {
        return Err(ConfigError::Invalid("OPENAI_API_KEY is invalid".into()));
    }
    paths.ensure_root()?;
    let text = serde_json::to_string_pretty(auth)? + "\n";
    atomic_write_if_changed(&paths.auth, text.as_bytes())
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("home directory is unavailable")]
    HomeUnavailable,
    #[error("configuration path is not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("refusing to use symlink configuration path: {0}")]
    Symlink(PathBuf),
    #[error("Codex configuration is missing: {0}")]
    MissingCodex(PathBuf),
    #[error("invalid configuration: {0}")]
    Invalid(String),
    #[error("configuration I/O: {0}")]
    Io(#[from] io::Error),
    #[error("configuration TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("configuration TOML serialization: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    #[error("configuration JSON: {0}")]
    Json(#[from] serde_json::Error),
}

fn choose<'a>(
    cli: Option<&'a str>,
    env: Option<&'a str>,
    file: Option<&'a str>,
    default: &'a str,
) -> String {
    cli.or(env).or(file).unwrap_or(default).to_owned()
}

/// Accept the direct Codex key field and the common nested/profile variants,
/// while intentionally ignoring unrelated token fields.  Only string values
/// under an exact key name are eligible for import.
fn find_api_key<'a>(value: &'a Value, provider: Option<&str>) -> Option<&'a str> {
    let object = value.as_object()?;
    if let Some(candidate) = object.get(OPENAI_API_KEY).and_then(Value::as_str) {
        return Some(candidate);
    }
    // Accept only explicit, provider-scoped credential maps. Recursing through
    // arbitrary extension data could import an unrelated API key or OAuth
    // access token as the model credential.
    let profiles = object
        .get("profiles")
        .or_else(|| object.get("providers"))
        .and_then(Value::as_object)?;
    provider
        .and_then(|name| {
            profiles
                .get(name)
                .or_else(|| profiles.get(&name.to_ascii_lowercase()))
        })
        .or_else(|| profiles.get("OpenAI"))
        .or_else(|| profiles.get("openai"))
        .and_then(Value::as_object)
        .and_then(|profile| {
            profile
                .get("api_key")
                .or_else(|| profile.get(OPENAI_API_KEY))
                .and_then(Value::as_str)
        })
}

fn redacted_endpoint(endpoint: &str) -> String {
    let Some(scheme_end) = endpoint.find("://") else {
        return endpoint.to_owned();
    };
    let authority_start = scheme_end + 3;
    let authority_end = endpoint[authority_start..]
        .find(['/', '?', '#'])
        .map_or(endpoint.len(), |offset| authority_start + offset);
    let authority = &endpoint[authority_start..authority_end];
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    format!("{}://{}", &endpoint[..scheme_end], host)
}

fn choose_optional(cli: Option<&str>, env: Option<&str>, file: Option<&str>) -> Option<String> {
    cli.or(env).or(file).map(str::to_owned)
}

fn validate_optional(name: &str, value: Option<&str>, max: usize) -> Result<(), ConfigError> {
    if let Some(value) = value
        && (value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control))
    {
        return Err(ConfigError::Invalid(format!("{name} is invalid")));
    }
    Ok(())
}

fn home_dir() -> Result<PathBuf, ConfigError> {
    #[cfg(windows)]
    let variable = "USERPROFILE";
    #[cfg(not(windows))]
    let variable = "HOME";
    env::var_os(variable)
        .map(PathBuf::from)
        .ok_or(ConfigError::HomeUnavailable)
}

fn codex_root() -> Result<PathBuf, ConfigError> {
    if let Some(root) = env::var_os("CODEX_HOME") {
        return Ok(PathBuf::from(root));
    }
    Ok(home_dir()?.join(".codex"))
}

fn reject_symlink(path: &Path) -> Result<(), ConfigError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                Err(ConfigError::Symlink(path.to_owned()))
            } else {
                Ok(())
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn secure_regular_file(path: &Path) -> Result<(), ConfigError> {
    reject_symlink(path)?;
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(ConfigError::NotDirectory(path.to_owned()));
    }
    restrict_permissions(path, 0o600)
}

fn restrict_permissions(path: &Path, mode: u32) -> Result<(), ConfigError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(mode);
        fs::set_permissions(path, permissions)?;
    }
    let _ = (path, mode);
    Ok(())
}

fn atomic_write_if_changed(path: &Path, bytes: &[u8]) -> Result<bool, ConfigError> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            secure_regular_file(path)?;
            if fs::read(path)? == bytes {
                return Ok(false);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let parent = path
        .parent()
        .ok_or_else(|| ConfigError::Invalid("configuration path has no parent".into()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ConfigError::Invalid("configuration filename is invalid".into()))?;
    let temp = parent.join(format!(".{file_name}.tmp-{}-{}", process::id(), now_ms()));
    reject_symlink(&temp)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file: File = options.open(&temp)?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    drop(file);
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    restrict_permissions(path, 0o600)?;
    Ok(true)
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
