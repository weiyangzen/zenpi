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
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use crate::auth::store::{
    CredentialKind, CredentialState, CredentialStore, LockWait, Mutation, Replacement, STORE_KEY,
};
use crate::auth::{AllowedDestination, AuthBinding};
use crate::layout::{
    LayoutError, LayoutModel, LayoutPreferences, MAX_LAYOUT_PREFERENCES_BYTES, TabId,
};
use crate::providers::connection::{
    ModelRoute, ProviderConnection, api_key_destinations, validate_model_routes,
};
use crate::providers::{AuthHeaderPolicy, EndpointRule, Protocol, get_provider_definition};
use crate::security::{SecretHandle, SecretRevocation};
use crate::view_model::ZoneModels;

/// The directory name created below the user's home directory.
pub const ZENPI_DIR: &str = ".zenpi";
pub const CONFIG_FILE: &str = "config.toml";
pub const AUTH_FILE: &str = "auth.json";
pub const SESSIONS_DIR: &str = "sessions";
pub const SKILLS_DIR: &str = "skills";
pub const EXTENSIONS_DIR: &str = "extensions";
/// User-owned BentoBox state is kept separate from provider configuration and
/// credentials. This lets a corrupt layout fail closed without making a
/// working profile unusable.
pub const LAYOUT_FILE: &str = "layout.json";
pub const OPENAI_API_KEY: &str = "OPENAI_API_KEY";

/// A named provider profile. The legacy flat fields in [`ConfigFile`] remain
/// readable as the implicit `default` profile, while new writes can use this
/// explicit shape without putting credentials in TOML.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderProfile {
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
    pub auth_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_header: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_routes: Vec<ModelRoute>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_overrides: Vec<crate::providers::registry::ModelOverride>,
}

impl ProviderProfile {
    fn from_flat(config: &ConfigFile) -> Self {
        Self {
            backend: config.backend.clone(),
            provider: config.provider.clone(),
            model: config.model.clone(),
            base_url: config.base_url.clone(),
            wire_api: config.wire_api.clone(),
            auth_env: config.auth_env.clone(),
            auth_method: config.auth_method.clone(),
            auth_ref: config.auth_ref.clone(),
            auth_header: config.auth_header.clone(),
            model_routes: config.model_routes.clone(),
            model_reasoning_effort: config.model_reasoning_effort.clone(),
            model_verbosity: config.model_verbosity.clone(),
            timeout_seconds: config.timeout_seconds,
            max_retries: config.max_retries,
            requires_openai_auth: config.requires_openai_auth,
            supports_websockets: config.supports_websockets,
            model_overrides: config.model_overrides.clone(),
        }
    }

    fn into_flat(self) -> ConfigFile {
        ConfigFile {
            backend: self.backend,
            provider: self.provider,
            model: self.model,
            base_url: self.base_url,
            wire_api: self.wire_api,
            auth_env: self.auth_env,
            auth_method: self.auth_method,
            auth_ref: self.auth_ref,
            auth_header: self.auth_header,
            model_routes: self.model_routes,
            model_reasoning_effort: self.model_reasoning_effort,
            model_verbosity: self.model_verbosity,
            timeout_seconds: self.timeout_seconds,
            max_retries: self.max_retries,
            requires_openai_auth: self.requires_openai_auth,
            supports_websockets: self.supports_websockets,
            model_overrides: self.model_overrides,
            default_profile: None,
            profiles: BTreeMap::new(),
            zone_models: ZoneModels::default(),
        }
    }

    fn validate(&self) -> Result<(), ConfigError> {
        self.clone().into_flat().validate_flat()
    }
}

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

    /// Path of the bounded, non-secret BentoBox preference snapshot. It is a
    /// method rather than another public struct field so existing callers that
    /// construct `ConfigPaths` literals remain source-compatible.
    pub fn layout_path(&self) -> PathBuf {
        self.root.join(LAYOUT_FILE)
    }

    /// Short alias used by hosts that treat paths as named resources.
    pub fn layout(&self) -> PathBuf {
        self.layout_path()
    }

    /// Non-secret checkpoint for the Wave-style project tab strip.
    pub fn project_tabs_path(&self) -> PathBuf {
        self.root.join("project-tabs.json")
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
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, ProviderProfile>,
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
    pub auth_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_header: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_routes: Vec<ModelRoute>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_overrides: Vec<crate::providers::registry::ModelOverride>,
    /// Per-zone model overrides for the discussion and arch TUI regions
    /// (ZS1-152). These are non-secret and survive a restart; the worker pool
    /// continues to use the global `model`.
    #[serde(default, skip_serializing_if = "ZoneModels::is_empty")]
    pub zone_models: ZoneModels,
}

impl ConfigFile {
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.validate_flat()?;
        validate_optional("default_profile", self.default_profile.as_deref(), 128)?;
        for (name, profile) in &self.profiles {
            validate_profile_name(name)?;
            profile.validate()?;
        }
        if let Some(selected) = &self.default_profile
            && !self.profiles.contains_key(selected)
        {
            return Err(ConfigError::Invalid(format!(
                "default_profile `{selected}` does not exist"
            )));
        }
        Ok(())
    }

    fn validate_flat(&self) -> Result<(), ConfigError> {
        crate::providers::registry::ModelRegistry::with_overrides(&self.model_overrides)
            .map_err(|error| ConfigError::Invalid(error.to_string()))?;
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
        self.zone_models
            .validate()
            .map_err(|error| ConfigError::Invalid(format!("zone_models: {error}")))?;
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
            .is_some_and(|backend| !matches!(backend, "echo" | "openai" | "anthropic" | "google"))
        {
            return Err(ConfigError::Invalid(
                "backend must be `echo`, `openai`, `anthropic` or `google`".into(),
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
        self.connection_settings(None, 0)?;
        Ok(())
    }

    fn connection_settings(
        &self,
        profile: Option<&str>,
        config_revision: u64,
    ) -> Result<Option<ProviderConnection>, ConfigError> {
        let method = self.auth_method.as_deref().unwrap_or("legacy_api_key");
        if method == "legacy_api_key" {
            if self.auth_ref.is_some()
                || self.auth_header.is_some()
                || !self.model_routes.is_empty()
            {
                return Err(ConfigError::Invalid(
                    "legacy authentication does not accept auth_ref, auth_header or model_routes"
                        .into(),
                ));
            }
            return Ok(None);
        }
        if !matches!(method, "api_key" | "oauth" | "none") {
            return Err(ConfigError::Invalid("unknown auth_method".into()));
        }
        if self.auth_env.is_some() {
            return Err(ConfigError::Invalid(
                "auth_env is only valid for legacy authentication".into(),
            ));
        }
        let provider = self.provider.as_deref().ok_or_else(|| {
            ConfigError::Invalid("explicit authentication requires provider".into())
        })?;
        crate::providers::registry::validate_identity(
            provider,
            self.model.as_deref().unwrap_or("config-validation"),
        )
        .map_err(|_| ConfigError::Invalid("invalid explicit provider/model identity".into()))?;
        let definition = get_provider_definition(provider);
        let protocol = match self.wire_api.as_deref() {
            Some(wire) => Protocol::parse(wire).map_err(config_backend_error)?,
            None => definition
                .filter(|d| d.routes.len() == 1)
                .map(|d| d.routes[0].protocol)
                .ok_or_else(|| {
                    ConfigError::Invalid(
                        "explicit connection requires wire_api when provider has no unique route"
                            .into(),
                    )
                })?,
        };
        let credential_id = || {
            let id = self.auth_ref.as_deref().ok_or_else(|| {
                ConfigError::Invalid("api_key/oauth authentication requires auth_ref".into())
            })?;
            validate_profile_name(id)
                .map_err(|_| ConfigError::Invalid("invalid auth_ref".into()))?;
            Ok::<_, ConfigError>(id.to_owned())
        };
        let auth = match method {
            "api_key" => AuthBinding::StoredApiKey {
                credential_id: credential_id()?,
            },
            "oauth" => AuthBinding::CodexOAuth {
                credential_id: credential_id()?,
            },
            "none" => {
                if self.auth_ref.is_some() || self.auth_header.is_some() {
                    return Err(ConfigError::Invalid(
                        "anonymous authentication cannot have auth_ref or auth_header".into(),
                    ));
                }
                AuthBinding::Anonymous
            }
            _ => unreachable!(),
        };
        if method != "none" && self.requires_openai_auth == Some(false) {
            return Err(ConfigError::Invalid(
                "explicit credential binding cannot disable authentication".into(),
            ));
        }
        if method == "oauth" && (provider != "openai-codex" || self.auth_header.is_some()) {
            return Err(ConfigError::Invalid(
                "OAuth requires the Codex provider without header overrides".into(),
            ));
        }
        let header_policy = self
            .auth_header
            .as_deref()
            .map(AuthHeaderPolicy::parse)
            .transpose()
            .map_err(config_backend_error)?;
        validate_model_routes(&self.model_routes).map_err(config_backend_error)?;
        let connection = ProviderConnection {
            profile: profile.unwrap_or("default").into(),
            provider: provider.into(),
            protocol,
            base_url: self.base_url.clone(),
            auth,
            header_policy,
            config_revision,
            model_routes: self.model_routes.clone(),
        };
        validate_configured_route(
            &connection,
            protocol,
            self.base_url.as_deref(),
            self.backend.as_deref(),
        )?;
        for route in &self.model_routes {
            let protocol = route
                .wire_api
                .as_deref()
                .map(Protocol::parse)
                .transpose()
                .map_err(config_backend_error)?
                .unwrap_or(protocol);
            validate_configured_route(
                &connection,
                protocol,
                route.base_url.as_deref().or(self.base_url.as_deref()),
                self.backend.as_deref(),
            )?;
        }
        Ok(Some(connection))
    }

    pub fn selected_profile(
        &self,
        requested: Option<&str>,
    ) -> Result<(Option<String>, Self), ConfigError> {
        let selected = requested.or(self.default_profile.as_deref());
        let Some(name) = selected else {
            let mut flat = self.clone();
            flat.default_profile = None;
            flat.profiles.clear();
            return Ok((None, flat));
        };
        validate_profile_name(name)?;
        let profile = self.profiles.get(name).ok_or_else(|| {
            ConfigError::Invalid(format!("provider profile `{name}` does not exist"))
        })?;
        // Zone model overrides are a global TUI policy, not a provider-profile
        // field, so they are carried through profile selection unchanged.
        let mut flat = profile.clone().into_flat();
        flat.zone_models = self.zone_models.clone();
        Ok((Some(name.to_owned()), flat))
    }
}

fn explicit_auth(method: Option<&str>) -> bool {
    method.is_some_and(|method| method != "legacy_api_key")
}

fn config_backend_error(error: crate::backend::BackendError) -> ConfigError {
    ConfigError::Invalid(error.to_string())
}

fn validate_configured_route(
    connection: &ProviderConnection,
    protocol: Protocol,
    configured_base: Option<&str>,
    backend: Option<&str>,
) -> Result<(), ConfigError> {
    if backend == Some("echo")
        || backend == Some("anthropic") && protocol != Protocol::AnthropicMessages
        || backend == Some("google") && protocol != Protocol::GoogleGenerativeAi
    {
        return Err(ConfigError::Invalid(
            "backend does not match explicit connection protocol".into(),
        ));
    }
    let definition = get_provider_definition(&connection.provider);
    let rule = definition
        .unwrap_or(&crate::providers::CUSTOM)
        .routes
        .iter()
        .find(|rule| rule.protocol == protocol)
        .ok_or_else(|| ConfigError::Invalid("unsupported explicit provider protocol".into()))?;
    if matches!(connection.auth, AuthBinding::CodexOAuth { .. })
        != (protocol == Protocol::OpenAiCodexResponses)
    {
        return Err(ConfigError::Invalid(
            "Codex wire and OAuth binding must be selected together".into(),
        ));
    }
    let base = match rule.endpoint {
        EndpointRule::Fixed(endpoint) => {
            if configured_base.is_some() {
                return Err(ConfigError::Invalid(
                    "fixed OAuth service rejects base_url overrides".into(),
                ));
            }
            endpoint
        }
        EndpointRule::PrefixAndOperation {
            default_prefix,
            canonical_prefix,
            ..
        } => {
            let base = configured_base.or(default_prefix).ok_or_else(|| {
                ConfigError::Invalid("custom connection requires base_url".into())
            })?;
            validate_model_routes(&[ModelRoute {
                model: "config-validation".into(),
                wire_api: None,
                base_url: Some(base.into()),
            }])
            .map_err(config_backend_error)?;
            if canonical_prefix
                .is_some_and(|canonical| base.strip_suffix('/').unwrap_or(base) != canonical)
            {
                return Err(ConfigError::Invalid(
                    "noncanonical builtin API prefix; use an explicitly scoped custom provider"
                        .into(),
                ));
            }
            base
        }
    };
    let url =
        url::Url::parse(base).map_err(|_| ConfigError::Invalid("invalid connection URL".into()))?;
    if matches!(connection.auth, AuthBinding::Anonymous) {
        let loopback = match url.host() {
            Some(url::Host::Domain("localhost")) => true,
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
        if definition.is_some() || url.scheme() != "http" || !loopback {
            return Err(ConfigError::Invalid(
                "none authentication requires an explicit custom loopback HTTP connection".into(),
            ));
        }
    } else {
        if url.scheme() != "https" {
            return Err(ConfigError::Invalid(
                "explicit credentials require HTTPS".into(),
            ));
        }
        if let Some(header) = connection.header_policy
            && !rule.allowed_headers.contains(&header)
        {
            return Err(ConfigError::Invalid(
                "auth_header is not permitted by the service route".into(),
            ));
        }
    }
    Ok(())
}

fn validate_project_auth_sources(
    user: &ConfigFile,
    project: &toml::Value,
) -> Result<(), ConfigError> {
    fn check(source: &toml::value::Table, existing_explicit: bool) -> Result<(), ConfigError> {
        if source
            .get("auth_method")
            .and_then(toml::Value::as_str)
            .is_some_and(|m| m != "legacy_api_key")
        {
            return Err(ConfigError::Invalid(
                "project config cannot introduce explicit authentication".into(),
            ));
        }
        if existing_explicit
            && [
                "backend",
                "provider",
                "wire_api",
                "base_url",
                "auth_method",
                "auth_ref",
                "auth_header",
                "auth_env",
                "model_routes",
                "requires_openai_auth",
                "supports_websockets",
            ]
            .iter()
            .any(|key| source.contains_key(*key))
        {
            return Err(ConfigError::Invalid(
                "project config cannot rebind an approved explicit connection".into(),
            ));
        }
        Ok(())
    }
    let table = project
        .as_table()
        .ok_or_else(|| ConfigError::Invalid("project config must be a table".into()))?;
    if let Some(target) = table.get("default_profile").and_then(toml::Value::as_str) {
        let (_, current) = user.selected_profile(None)?;
        let target_explicit = user
            .profiles
            .get(target)
            .is_some_and(|profile| explicit_auth(profile.auth_method.as_deref()));
        if explicit_auth(current.auth_method.as_deref()) || target_explicit {
            return Err(ConfigError::Invalid(
                "project config cannot select or change explicit authentication profiles".into(),
            ));
        }
    }
    check(table, explicit_auth(user.auth_method.as_deref()))?;
    if let Some(profiles) = table.get("profiles").and_then(toml::Value::as_table) {
        for (name, source) in profiles {
            if let Some(source) = source.as_table() {
                check(
                    source,
                    user.profiles
                        .get(name)
                        .is_some_and(|p| explicit_auth(p.auth_method.as_deref())),
                )?;
            }
        }
    }
    Ok(())
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
        validate_api_key(&key)?;
        self.0.insert(OPENAI_API_KEY.to_owned(), Value::String(key));
        Ok(())
    }

    pub fn remove_openai_api_key(&mut self) -> bool {
        self.0.remove(OPENAI_API_KEY).is_some()
    }

    pub fn api_key_for_profile(&self, profile: Option<&str>) -> Option<&str> {
        match profile {
            Some(name) => self
                .0
                .get("profiles")
                .and_then(Value::as_object)
                .and_then(|profiles| profiles.get(name))
                .and_then(Value::as_object)
                .and_then(|profile| {
                    profile
                        .get("api_key")
                        .or_else(|| profile.get(OPENAI_API_KEY))
                })
                .and_then(Value::as_str),
            None => self.openai_api_key(),
        }
    }

    pub fn set_profile_api_key(
        &mut self,
        profile: &str,
        key: impl Into<String>,
    ) -> Result<(), ConfigError> {
        validate_profile_name(profile)?;
        let key = key.into();
        validate_api_key(&key)?;
        let profiles = self
            .0
            .entry("profiles".into())
            .or_insert_with(|| Value::Object(serde_json::Map::new()))
            .as_object_mut()
            .ok_or_else(|| ConfigError::Invalid("auth profiles must be an object".into()))?;
        let profile_value = profiles
            .entry(profile.to_owned())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let fields = profile_value
            .as_object_mut()
            .ok_or_else(|| ConfigError::Invalid("auth profile must be an object".into()))?;
        fields.insert("api_key".into(), Value::String(key));
        Ok(())
    }

    pub fn remove_profile_api_key(&mut self, profile: &str) -> Result<bool, ConfigError> {
        validate_profile_name(profile)?;
        let Some(profiles) = self.0.get_mut("profiles").and_then(Value::as_object_mut) else {
            return Ok(false);
        };
        let Some(fields) = profiles.get_mut(profile).and_then(Value::as_object_mut) else {
            return Ok(false);
        };
        let changed = fields.remove("api_key").is_some() | fields.remove(OPENAI_API_KEY).is_some();
        if fields.is_empty() {
            profiles.remove(profile);
        }
        if profiles.is_empty() {
            self.0.remove("profiles");
        }
        Ok(changed)
    }
}

/// Explicit command-line overrides.  `None` means no override, not an empty
/// value.  The CLI parser can fill this directly without reading secrets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigOverrides {
    pub profile: Option<String>,
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
    pub profile: Option<String>,
    pub backend: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub wire_api: Option<String>,
    pub auth_method: Option<String>,
    pub auth_ref: Option<String>,
    pub auth_header: Option<String>,
    pub model_routes: Vec<ModelRoute>,
    pub api_key: Option<String>,
    pub credential_source: CredentialSource,
    pub model_reasoning_effort: Option<String>,
    pub model_verbosity: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub max_retries: Option<u32>,
    pub requires_openai_auth: bool,
    pub supports_websockets: bool,
    pub model_overrides: Vec<crate::providers::registry::ModelOverride>,
    /// Resolved per-zone model overrides for the discussion and arch regions
    /// (ZS1-152). Use [`EffectiveConfig::zone_model`] to apply the global
    /// fallback.
    pub zone_models: ZoneModels,
}

impl EffectiveConfig {
    /// Prepare only a non-secret connection. Destination grants and credential
    /// state are checked later by the connection/auth request boundary.
    pub(crate) fn provider_connection(
        &self,
        config_revision: u64,
    ) -> Result<Option<ProviderConnection>, ConfigError> {
        if !explicit_auth(self.auth_method.as_deref()) {
            if self.auth_ref.is_some()
                || self.auth_header.is_some()
                || !self.model_routes.is_empty()
            {
                return Err(ConfigError::Invalid(
                    "legacy authentication does not accept explicit connection fields".into(),
                ));
            }
            return Ok(None);
        }
        let flat = ConfigFile {
            backend: Some(self.backend.clone()),
            provider: self.provider.clone(),
            model: self.model.clone(),
            base_url: self.base_url.clone(),
            wire_api: self.wire_api.clone(),
            auth_method: self.auth_method.clone(),
            auth_ref: self.auth_ref.clone(),
            auth_header: self.auth_header.clone(),
            model_routes: self.model_routes.clone(),
            timeout_seconds: self.timeout_seconds,
            max_retries: self.max_retries,
            requires_openai_auth: Some(self.requires_openai_auth),
            ..ConfigFile::default()
        };
        flat.validate_flat()?;
        flat.connection_settings(self.profile.as_deref(), config_revision)
    }

    /// Effective model for one TUI zone. Discussion and arch prefer an explicit
    /// zone override; the worker pool and any zone without an override use the
    /// resolved global `model`.
    pub fn zone_model(&self, zone: crate::view_model::Zone) -> Option<&str> {
        self.zone_models.effective(zone, self.model.as_deref())
    }

    /// Concurrency quota for one zone. Discussion and arch are single
    /// concurrency; workers use the supplied project-defined count.
    pub const fn zone_concurrency(
        &self,
        zone: crate::view_model::Zone,
        project_workers: u32,
    ) -> u32 {
        zone.concurrency(project_workers)
    }

    /// Issue an opaque credential capability for a single Blueprint policy.
    /// The API key remains private to configuration/backend setup and is never
    /// serialized as part of the handle or policy evidence.
    pub fn issue_secret_handle(
        &self,
        policy_digest: impl Into<String>,
    ) -> Result<Option<(SecretHandle, SecretRevocation)>, ConfigError> {
        let Some(key) = self.api_key.as_deref() else {
            return Ok(None);
        };
        SecretHandle::new(key, policy_digest.into())
            .map(Some)
            .map_err(|error| ConfigError::Invalid(error.to_string()))
    }
}

impl std::fmt::Debug for EffectiveConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EffectiveConfig")
            .field("profile", &self.profile)
            .field("backend", &self.backend)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field("wire_api", &self.wire_api)
            .field("auth_method", &self.auth_method)
            .field("auth_ref", &self.auth_ref)
            .field("auth_header", &self.auth_header)
            .field("model_routes", &self.model_routes)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("credential_source", &self.credential_source)
            .field("model_reasoning_effort", &self.model_reasoning_effort)
            .field("model_verbosity", &self.model_verbosity)
            .field("timeout_seconds", &self.timeout_seconds)
            .field("max_retries", &self.max_retries)
            .field("requires_openai_auth", &self.requires_openai_auth)
            .field("supports_websockets", &self.supports_websockets)
            .field("zone_models", &self.zone_models)
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
    let requested_profile = overrides
        .profile
        .as_deref()
        .or_else(|| environment.get("ZENPI_PROFILE").map(String::as_str));
    let (profile, selected) = config.selected_profile(requested_profile)?;
    let config = &selected;
    let explicit = explicit_auth(config.auth_method.as_deref());
    if explicit && overrides.api_key.is_some() {
        return Err(ConfigError::Invalid(
            "explicit authentication conflicts with a command-line API key".into(),
        ));
    }
    let route_environment = |name: &str| {
        (!explicit)
            .then(|| environment.get(name).map(String::as_str))
            .flatten()
    };
    let backend = choose(
        overrides.backend.as_deref(),
        route_environment("ZENPI_BACKEND"),
        config.backend.as_deref(),
        "openai",
    );
    let provider = choose_optional(
        overrides.provider.as_deref(),
        route_environment("ZENPI_PROVIDER"),
        config.provider.as_deref(),
    );
    let wire_api = choose_optional(
        overrides.wire_api.as_deref(),
        route_environment("ZENPI_WIRE_API"),
        config.wire_api.as_deref(),
    );
    let anthropic = backend == "anthropic"
        || wire_api
            .as_deref()
            .is_some_and(|v| matches!(v, "anthropic_messages" | "anthropic-messages"));
    let google = backend == "google"
        || wire_api
            .as_deref()
            .is_some_and(|v| matches!(v, "google_generative_ai" | "google-generative-ai"));
    let native = anthropic || google;
    let provider = provider.or_else(|| {
        if anthropic {
            Some("anthropic".into())
        } else if google {
            Some("google".into())
        } else {
            None
        }
    });
    let model = choose_optional(
        overrides.model.as_deref(),
        environment
            .get("ZENPI_MODEL")
            .or_else(|| {
                environment.get(if google {
                    "GEMINI_MODEL"
                } else if anthropic {
                    "ANTHROPIC_MODEL"
                } else {
                    "OPENAI_MODEL"
                })
            })
            .map(String::as_str),
        config.model.as_deref(),
    );
    let base_url = choose_optional(
        overrides.base_url.as_deref(),
        route_environment("ZENPI_BASE_URL").or_else(|| {
            route_environment(if google {
                "GEMINI_BASE_URL"
            } else if anthropic {
                "ANTHROPIC_BASE_URL"
            } else {
                "OPENAI_BASE_URL"
            })
        }),
        config.base_url.as_deref(),
    )
    .or_else(|| google.then(|| "https://generativelanguage.googleapis.com/v1beta".into()));
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
    let requires_openai_auth = if explicit {
        config.auth_method.as_deref() != Some("none")
    } else {
        config.requires_openai_auth.unwrap_or(true)
    };
    let supports_websockets = config.supports_websockets.unwrap_or(false);
    let (api_key, credential_source) = if explicit {
        (None, CredentialSource::None)
    } else if let Some(key) = overrides.api_key.clone() {
        (Some(key), CredentialSource::CommandLine)
    } else if let Some(key) = environment
        .get("ZENPI_API_KEY")
        .or_else(|| {
            environment.get(if google {
                "GEMINI_API_KEY"
            } else if anthropic {
                "ANTHROPIC_API_KEY"
            } else {
                "OPENAI_API_KEY"
            })
        })
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
            if native && profile.is_none() {
                None
            } else {
                auth.api_key_for_profile(profile.as_deref())
                    .map(str::to_owned)
            },
            if (!native || profile.is_some())
                && auth.api_key_for_profile(profile.as_deref()).is_some()
            {
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
    if let Some(key) = &api_key {
        crate::security::register_secret_value(key);
    }
    let mut effective = EffectiveConfig {
        profile,
        backend,
        provider,
        model,
        base_url,
        wire_api,
        auth_method: config.auth_method.clone(),
        auth_ref: config.auth_ref.clone(),
        auth_header: config.auth_header.clone(),
        model_routes: config.model_routes.clone(),
        api_key,
        credential_source,
        model_reasoning_effort,
        model_verbosity,
        timeout_seconds,
        max_retries,
        requires_openai_auth,
        supports_websockets,
        model_overrides: config.model_overrides.clone(),
        zone_models: config.zone_models.clone(),
    };
    if explicit && let Some(connection) = effective.provider_connection(0)? {
        effective.wire_api = Some(connection.protocol.as_str().into());
    }
    Ok(effective)
}

/// Resolve the files in the default `~/.zenpi` directory using the current
/// process environment.
pub fn resolve_default(overrides: &ConfigOverrides) -> Result<EffectiveConfig, ConfigError> {
    resolve_workspace(overrides, None)
}

/// Read a bounded project overlay over user settings without changing process
/// environment or files. Validation follows merging so project selection can
/// refer to an inherited profile. Unknown TOML fields remain errors.
pub fn load_workspace_config(
    paths: &ConfigPaths,
    workspace: Option<&Path>,
) -> Result<ConfigFile, ConfigError> {
    let mut config = load_config(paths)?;
    if let Some(workspace) = workspace {
        let path = workspace.join(ZENPI_DIR).join(CONFIG_FILE);
        if path.try_exists()? {
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            options.custom_flags(libc::O_NOFOLLOW);
            let file = options.open(&path)?;
            if !file.metadata()?.is_file() {
                return Err(ConfigError::Invalid(
                    "project config must be a regular file".into(),
                ));
            }
            let mut text = String::new();
            file.take(262145).read_to_string(&mut text)?;
            if text.len() > 262144 {
                return Err(ConfigError::Invalid(
                    "project config exceeds 256 KiB".into(),
                ));
            }
            let project_source: toml::Value = toml::from_str(&text)?;
            validate_project_auth_sources(&config, &project_source)?;
            let project: ConfigFile = project_source.try_into()?;
            let mut merged =
                serde_json::to_value(&config).map_err(|e| ConfigError::Invalid(e.to_string()))?;
            let overlay =
                serde_json::to_value(project).map_err(|e| ConfigError::Invalid(e.to_string()))?;
            for (key, value) in overlay.as_object().expect("config object") {
                if !value.is_null() && !value.as_object().is_some_and(|map| map.is_empty()) {
                    if key == "profiles" {
                        // Profiles inherit by name, then by explicitly present field.
                        // An empty table is an overlay with no changes, not deletion.
                        if merged["profiles"].is_null() {
                            merged["profiles"] = serde_json::json!({});
                        }
                        for (name, fields) in value.as_object().expect("profiles object") {
                            if merged["profiles"][name].is_null() {
                                merged["profiles"][name] = fields.clone();
                            } else {
                                for (field, setting) in fields.as_object().expect("profile object")
                                {
                                    merged["profiles"][name][field] = setting.clone();
                                }
                            }
                        }
                    } else {
                        merged[key] = value.clone();
                    }
                }
            }
            config =
                serde_json::from_value(merged).map_err(|e| ConfigError::Invalid(e.to_string()))?;
        }
    }
    config.validate()?;
    Ok(config)
}

/// Resolve user defaults plus <workspace>/.zenpi/config.toml, then environment
/// and CLI overrides. Auth always remains in the user credential store.
/// A malformed project config fails preparation before any owner is switched.
pub fn resolve_workspace(
    overrides: &ConfigOverrides,
    workspace: Option<&Path>,
) -> Result<EffectiveConfig, ConfigError> {
    let paths = ConfigPaths::discover()?;
    let mut config = load_workspace_config(&paths, workspace)?;
    // A fresh zenpi install should work with the provider the user already
    // configured for Codex. This fallback is read-only; `config import-codex`
    // remains the explicit persistence command.
    let environment_profile = env::var("ZENPI_PROFILE").ok();
    let requested_profile = overrides
        .profile
        .as_deref()
        .or(environment_profile.as_deref());
    let environment = text_environment();
    let (_, selected) = config.selected_profile(requested_profile)?;
    // Explicit connections never consult legacy credentials or Codex fallback.
    if explicit_auth(selected.auth_method.as_deref()) {
        return resolve(overrides, &config, &AuthFile::default(), &environment);
    }
    let mut auth = load_auth(&paths)?;
    let initial = resolve(overrides, &config, &auth, &environment)?;
    if initial.backend == "openai"
        && !initial.wire_api.as_deref().is_some_and(|v| {
            matches!(
                v,
                "anthropic_messages"
                    | "anthropic-messages"
                    | "google_generative_ai"
                    | "google-generative-ai"
            )
        })
        && (initial.base_url.is_none()
            || initial.model.is_none()
            || (initial.requires_openai_auth && initial.api_key.is_none()))
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
        if auth
            .api_key_for_profile(requested_profile.or(config.default_profile.as_deref()))
            .is_none()
            && let Some(key) = import.api_key
        {
            auth.set_openai_api_key(key)?;
        }
    }
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
        default_profile: None,
        profiles: BTreeMap::new(),
        backend: Some("openai".into()),
        provider,
        model,
        base_url,
        wire_api,
        auth_env: Some(OPENAI_API_KEY.into()),
        auth_method: None,
        auth_ref: None,
        auth_header: None,
        model_routes: Vec::new(),
        model_reasoning_effort,
        model_verbosity,
        timeout_seconds,
        max_retries,
        requires_openai_auth,
        supports_websockets,
        model_overrides: Vec::new(),
        zone_models: ZoneModels::default(),
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

/// Import Codex into an explicit profile and make that profile active. This
/// is the preferred new-file representation; the legacy flat import remains
/// available so existing installations are not rewritten unexpectedly.
pub fn import_codex_profile(profile: &str) -> Result<ConfigSummary, ConfigError> {
    validate_profile_name(profile)?;
    let paths = ConfigPaths::discover()?;
    let import = import_codex_from_root(codex_root()?)?;
    paths.ensure_root()?;
    let mut config = load_config(&paths)?;
    config.profiles.insert(
        profile.to_owned(),
        ProviderProfile::from_flat(&import.config),
    );
    config.default_profile = Some(profile.to_owned());
    config.validate()?;
    // The credential file is updated under its own lock rather than by
    // rewriting a snapshot loaded before the TOML write above.
    let imported_key = import.api_key;
    let config_changed = save_config(&paths, &config)?;
    let auth_changed = update_auth_legacy(&paths, |auth| match imported_key {
        Some(key) => {
            auth.set_profile_api_key(profile, key)?;
            Ok(true)
        }
        None => Ok(false),
    })?;
    let mut summary = doctor_for_profile(&paths, Some(profile))?;
    summary.operation = "import-codex".into();
    summary.changed = Some(config_changed || auth_changed);
    Ok(summary)
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
    let imported_key = import.api_key;
    let config_changed = write_config_if_changed(paths, &config)?;
    let mut key_imported = false;
    let auth_changed = update_auth_legacy(paths, |auth| match imported_key {
        Some(key) => {
            // Report a real value change, not merely a requested import.
            key_imported = auth.openai_api_key() != Some(key.as_str());
            auth.set_openai_api_key(key)?;
            Ok(key_imported)
        }
        None => Ok(false),
    })?;
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
    doctor_for_profile(&paths, None)
}

pub fn doctor_for_profile(
    paths: &ConfigPaths,
    profile: Option<&str>,
) -> Result<ConfigSummary, ConfigError> {
    Ok(ConfigSummary {
        operation: "doctor".into(),
        changed: None,
        status: status_for_profile(paths, profile)?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileSummary {
    pub name: String,
    pub active: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub wire_api: Option<String>,
    pub api_key_present: bool,
}

pub fn list_profiles(paths: &ConfigPaths) -> Result<Vec<ProfileSummary>, ConfigError> {
    let config = load_config(paths)?;
    let auth = if config
        .profiles
        .values()
        .all(|p| explicit_auth(p.auth_method.as_deref()))
    {
        AuthFile::default()
    } else {
        load_auth(paths)?
    };
    let mut profiles = Vec::with_capacity(config.profiles.len());
    for (name, profile) in &config.profiles {
        profiles.push(ProfileSummary {
            name: name.clone(),
            active: config.default_profile.as_deref() == Some(name),
            provider: profile.provider.clone(),
            model: profile.model.clone(),
            base_url: profile.base_url.as_deref().map(redacted_endpoint),
            wire_api: profile.wire_api.clone(),
            api_key_present: !explicit_auth(profile.auth_method.as_deref())
                && auth.api_key_for_profile(Some(name)).is_some(),
        });
    }
    Ok(profiles)
}

pub fn use_profile(paths: &ConfigPaths, profile: &str) -> Result<bool, ConfigError> {
    validate_profile_name(profile)?;
    let mut config = load_config(paths)?;
    if !config.profiles.contains_key(profile) {
        return Err(ConfigError::Invalid(format!(
            "provider profile `{profile}` does not exist"
        )));
    }
    config.default_profile = Some(profile.to_owned());
    save_config(paths, &config)
}

/// Remove only credentials owned by zenpi. Codex config/auth files are never
/// opened for writing by this path.
pub fn revoke(paths: &ConfigPaths, profile: Option<&str>) -> Result<bool, ConfigError> {
    update_auth_legacy(paths, |auth| match profile {
        Some(profile) => auth.remove_profile_api_key(profile),
        None => Ok(auth.remove_openai_api_key()),
    })
}

/// The stored credential a profile names, if it names one.  Read-only.
///
/// A legacy profile keeps its key in the auth file rather than referencing a
/// stored credential, and is reported as `None` so callers take the legacy path
/// instead of pretending there is a credential to revoke.
pub fn profile_credential(
    paths: &ConfigPaths,
    profile: &str,
) -> Result<Option<String>, ConfigError> {
    validate_profile_name(profile)?;
    Ok(load_config(paths)?
        .profiles
        .get(profile)
        .and_then(|entry| entry.auth_ref.clone()))
}

/// Receipt for revoking a stored credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CredentialRevocation {
    pub credential_id: String,
    /// Every profile that names this credential — the whole scope `--yes`
    /// confirms.  Revoking is not the same as unbinding one profile.
    pub profiles: Vec<String>,
    pub local_revoked: bool,
    /// zenpi has no server-side revocation, so this is always false.  It is
    /// reported explicitly rather than left to be inferred from silence.
    pub remote_revoked: bool,
}

/// Revoke the whole credential a profile names.
///
/// Revocation keeps a tombstone, so a later login cannot silently revive this
/// identity, and it clears the secrets rather than leaving them readable.
pub fn revoke_credential(
    paths: &ConfigPaths,
    profile: &str,
) -> Result<CredentialRevocation, ConfigError> {
    let credential_id = profile_credential(paths, profile)?.ok_or_else(|| {
        ConfigError::Invalid(format!(
            "profile `{profile}` is not bound to a stored credential"
        ))
    })?;
    let config = load_config(paths)?;
    let profiles: Vec<String> = config
        .profiles
        .iter()
        .filter(|(_, entry)| entry.auth_ref.as_deref() == Some(credential_id.as_str()))
        .map(|(name, _)| name.clone())
        .collect();
    let store = credential_store(paths, true)?;
    let revision = store
        .list_status()?
        .into_iter()
        .find(|status| status.credential_id == credential_id)
        .map(|status| status.revision)
        .ok_or_else(|| {
            ConfigError::Invalid(format!("credential `{credential_id}` is not stored"))
        })?;
    store.modify(
        &credential_id,
        Some(revision),
        Mutation::Revoke,
        &LockWait::default(),
    )?;
    Ok(CredentialRevocation {
        credential_id,
        profiles,
        local_revoked: true,
        remote_revoked: false,
    })
}

/// One stored credential and the profiles that refer to it.  Never a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthListEntry {
    pub credential_id: String,
    pub provider: String,
    pub kind: &'static str,
    pub state: &'static str,
    pub revision: u64,
    pub expires_at_ms: Option<u64>,
    pub profiles: Vec<String>,
}

/// Project the credential a profile names onto the state a reader is shown.
///
/// Local only: it reads a non-secret snapshot and never refreshes, probes, or
/// repairs anything.  A profile that names a credential the store does not have
/// is `unconfigured` rather than an error, because that is exactly the state a
/// half-finished setup is in.
fn credential_binding_state(
    paths: &ConfigPaths,
    credential_id: &str,
) -> Result<String, ConfigError> {
    let store = credential_store(paths, false)?;
    Ok(store
        .list_status()?
        .into_iter()
        .find(|status| status.credential_id == credential_id)
        .map_or_else(
            || "unconfigured".to_owned(),
            |status| credential_state_label(status.state, status.expires_at_ms).to_owned(),
        ))
}

/// Project a stored credential onto the states a user is shown.  This is a
/// clock-only projection of a non-secret snapshot; it never refreshes, probes,
/// or repairs anything.
pub(crate) fn credential_state_label(
    state: CredentialState,
    expires_at_ms: Option<u64>,
) -> &'static str {
    match state {
        CredentialState::Active => match expires_at_ms {
            Some(expiry) if u128::from(expiry) <= now_ms() => "expired",
            _ => "ready",
        },
        CredentialState::RefreshInFlight => "refreshing",
        CredentialState::RefreshUncertain => "uncertain",
        CredentialState::LoginRequired => "login_required",
        CredentialState::Revoked => "revoked",
    }
}

/// Read-only credential listing.  Local only: no refresh, no probe, no command.
pub fn auth_list(paths: &ConfigPaths) -> Result<Vec<AuthListEntry>, ConfigError> {
    let store = credential_store(paths, false)?;
    let statuses = store.list_status()?;
    let config = load_config(paths)?;
    Ok(statuses
        .into_iter()
        .map(|status| AuthListEntry {
            profiles: config
                .profiles
                .iter()
                .filter(|(_, profile)| profile.auth_ref.as_deref() == Some(&status.credential_id))
                .map(|(name, _)| name.clone())
                .collect(),
            credential_id: status.credential_id,
            provider: status.provider,
            kind: match status.kind {
                CredentialKind::ApiKey => "api_key",
                CredentialKind::Oauth => "oauth",
            },
            state: credential_state_label(status.state, status.expires_at_ms),
            revision: status.revision,
            expires_at_ms: status.expires_at_ms,
        })
        .collect())
}

/// Outcome of adding a credential.  The credential and the profile that names
/// it live in different files, so a partial success is reported as one rather
/// than hidden behind a single success flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthAddReport {
    pub credential_id: String,
    pub alias: String,
    pub provider: String,
    pub kind: &'static str,
    /// The non-secret destinations the credential is authorized for.
    pub destinations: Vec<AllowedDestination>,
    pub credential_committed: bool,
    pub profile_bound: bool,
    /// Set when the credential committed but its profile did not: the account
    /// exists and can be rebound by ID, so this is not an authentication
    /// failure.
    pub binding_error: Option<String>,
}

/// The profile a newly added API key is bound to.  Grouped so the call sites
/// name what each optional value is for instead of trailing positional `None`s.
#[derive(Debug, Clone, Copy, Default)]
pub struct ApiKeyProfile<'a> {
    pub alias: Option<&'a str>,
    pub model: Option<&'a str>,
    pub wire: Option<&'a str>,
    pub auth_header: Option<&'a str>,
}

/// Add a provider API key.
///
/// The key is passed in already read from standard input; this function never
/// takes it from an argument list, never logs it, and never returns it.  The
/// credential is committed before the profile that names it, because the two
/// live in different files.
pub fn add_auth_apikey(
    paths: &ConfigPaths,
    base_url: &str,
    provider: &str,
    key: String,
    profile: ApiKeyProfile<'_>,
) -> Result<AuthAddReport, ConfigError> {
    let ApiKeyProfile {
        alias,
        model,
        wire,
        auth_header,
    } = profile;
    validate_api_key(&key)?;
    let destinations = api_key_destinations(provider, Some(base_url), wire, auth_header)
        .map_err(config_backend_error)?;
    let Some(first) = destinations.first() else {
        return Err(ConfigError::Invalid(
            "provider has no route to authorize".into(),
        ));
    };
    // Without an explicit wire the profile names the provider's first route;
    // the credential is authorized for all of them, so switching later is a
    // profile edit, not a new login.
    let profile_wire = wire
        .map(str::to_owned)
        .or_else(|| (destinations.len() > 1).then(|| first.protocol.as_str().to_owned()));
    let alias = alias.unwrap_or(provider).to_owned();
    let credential_id = CredentialStore::new_credential_id()?;
    let pending = crate::auth::api_key_credential(
        provider,
        first.definition_version,
        key,
        destinations
            .iter()
            .map(|destination| destination.grant.clone())
            .collect(),
    );
    let store = credential_store(paths, true)?;
    store.modify(
        &credential_id,
        None,
        Mutation::Replace(Replacement::Login(pending)),
        &LockWait::default(),
    )?;
    let binding = bind_credential_profile(
        paths,
        &alias,
        provider,
        model,
        // A built-in provider's endpoints come from its definition; every
        // other provider — including an unknown one, which routes as custom —
        // needs its URL recorded in the profile.
        if is_builtin_provider(provider) {
            None
        } else {
            Some(base_url)
        },
        "api_key",
        &credential_id,
        profile_wire.as_deref(),
        auth_header,
    );
    Ok(AuthAddReport {
        credential_id,
        alias,
        provider: provider.to_owned(),
        kind: "api_key",
        destinations: destinations
            .into_iter()
            .map(|destination| destination.grant)
            .collect(),
        credential_committed: true,
        profile_bound: binding.is_ok(),
        binding_error: binding.err().map(|error| error.to_string()),
    })
}

/// Bind `alias` to a stored credential.
///
/// An alias that already names a different credential is never overwritten:
/// replacing it is an explicit `pair revoke` plus re-add, not a side effect of
/// adding a second credential.
#[allow(clippy::too_many_arguments)]
pub fn bind_credential_profile(
    paths: &ConfigPaths,
    alias: &str,
    provider: &str,
    model: Option<&str>,
    base_url: Option<&str>,
    auth_method: &str,
    credential_id: &str,
    wire_api: Option<&str>,
    auth_header: Option<&str>,
) -> Result<bool, ConfigError> {
    validate_profile_name(alias)?;
    let mut config = load_config(paths)?;
    if let Some(existing) = config.profiles.get(alias)
        && let Some(bound) = existing.auth_ref.as_deref()
        && bound != credential_id
    {
        return Err(ConfigError::Invalid(format!(
            "profile `{alias}` already names credential `{bound}`; revoke it before rebinding"
        )));
    }
    let profile = ProviderProfile {
        provider: Some(provider.into()),
        model: model.map(str::to_owned),
        base_url: base_url.map(str::to_owned),
        wire_api: wire_api.map(str::to_owned),
        auth_method: Some(auth_method.into()),
        auth_ref: Some(credential_id.into()),
        auth_header: auth_header.map(str::to_owned),
        ..ProviderProfile::default()
    };
    profile.validate()?;
    config.profiles.insert(alias.to_owned(), profile);
    save_config(paths, &config)
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
            "zenpi {operation}:{changed} profile={profile} backend={backend} provider={provider} model={model} base_url={base_url} wire_api={wire_api} api_key={key} auth_binding={auth_binding}",
            operation = self.operation,
            changed = changed,
            profile = self.status.profile.as_deref().unwrap_or("default"),
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
            auth_binding = self
                .status
                .auth_binding_state
                .as_deref()
                .unwrap_or("legacy"),
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
    pub profile: Option<String>,
    pub config_exists: bool,
    pub auth_exists: bool,
    pub backend: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub wire_api: Option<String>,
    pub auth_method: Option<String>,
    pub auth_ref: Option<String>,
    pub auth_header: Option<String>,
    pub auth_binding_state: Option<String>,
    pub api_key_present: bool,
    pub api_key_source: CredentialSource,
    pub requires_openai_auth: bool,
    pub supports_websockets: bool,
}

impl ConfigStatus {
    /// Whether this configuration has the minimum needed to start the selected
    /// provider. This is deliberately local-only: it does not claim that the
    /// endpoint is reachable or that the credential has quota.
    ///
    /// An explicit credential binding is ready only when the stored credential
    /// is, so a profile whose fields are complete but whose account is missing,
    /// expired, or revoked never reports ready.
    pub fn is_ready(&self) -> bool {
        self.is_authenticated() && self.base_url.is_some() && self.model.is_some()
    }

    /// The authentication check on its own, so the per-check report and the
    /// overall readiness cannot disagree about why a configuration is not
    /// ready.  A legacy or anonymous setup is judged by its own fields; an
    /// explicit binding is judged by the stored credential.
    pub fn is_authenticated(&self) -> bool {
        match self.auth_method.as_deref() {
            None | Some("legacy_api_key") => !self.requires_openai_auth || self.api_key_present,
            Some("none") => true,
            Some(_) => self.auth_binding_state.as_deref() == Some("ready"),
        }
    }
}

/// A secret-free entry in the model picker exposed by the interactive hosts.
/// Profiles are listed rather than credentials, so `/models` remains useful
/// even when a provider is not currently reachable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCatalogEntry {
    pub profile: Option<String>,
    pub active: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub backend: String,
    pub configured: bool,
    pub metadata: Option<crate::providers::registry::ModelDescriptor>,
}

/// Return the bounded, redacted model/profile catalogue used by `/models`.
/// This is deliberately read-only and never contacts a provider. A legacy
/// flat config is represented by one `default` entry; named profiles remain
/// independently selectable in the output.
pub fn model_catalog(profile: Option<&str>) -> Result<Vec<ModelCatalogEntry>, ConfigError> {
    model_catalog_in_workspace(profile, None)
}

pub fn model_catalog_in_workspace(
    profile: Option<&str>,
    workspace: Option<&Path>,
) -> Result<Vec<ModelCatalogEntry>, ConfigError> {
    let paths = ConfigPaths::discover()?;
    let config = load_workspace_config(&paths, workspace)?;
    let metadata = |provider: Option<&str>,
                    id: Option<&str>,
                    overrides: &[crate::providers::registry::ModelOverride]| {
        let registry = crate::providers::registry::ModelRegistry::with_overrides(overrides)
            .map_err(|error| ConfigError::Invalid(error.to_string()))?;
        id.map(|id| registry.resolve(provider.unwrap_or("openai"), id))
            .transpose()
            .map_err(|error| ConfigError::Invalid(error.to_string()))
    };
    if config.profiles.is_empty() {
        let resolved = resolve_workspace(
            &ConfigOverrides {
                profile: profile.map(str::to_owned),
                ..ConfigOverrides::default()
            },
            workspace,
        )?;
        let configured = !explicit_auth(resolved.auth_method.as_deref())
            && resolved.model.is_some()
            && resolved.base_url.is_some()
            && (!resolved.requires_openai_auth || resolved.api_key.is_some());
        let descriptor = metadata(
            resolved.provider.as_deref(),
            resolved.model.as_deref(),
            &resolved.model_overrides,
        )?;
        return Ok(vec![ModelCatalogEntry {
            profile: resolved.profile,
            active: true,
            provider: resolved.provider,
            model: resolved.model,
            backend: resolved.backend,
            configured,
            metadata: descriptor,
        }]);
    }
    let active = profile
        .map(str::to_owned)
        .or_else(|| env::var("ZENPI_PROFILE").ok())
        .or_else(|| config.default_profile.clone());
    let auth = if config
        .profiles
        .values()
        .all(|p| explicit_auth(p.auth_method.as_deref()))
    {
        AuthFile::default()
    } else {
        load_auth(&paths)?
    };
    config
        .profiles
        .iter()
        .map(|(name, profile_config)| {
            let api_key_present = auth.api_key_for_profile(Some(name)).is_some();
            let backend = profile_config
                .backend
                .clone()
                .unwrap_or_else(|| "openai".into());
            let provider = profile_config.provider.clone().or_else(|| {
                if backend == "google"
                    || profile_config.wire_api.as_deref().is_some_and(|v| {
                        matches!(v, "google_generative_ai" | "google-generative-ai")
                    })
                {
                    return Some("google".into());
                }
                (backend == "anthropic"
                    || profile_config
                        .wire_api
                        .as_deref()
                        .is_some_and(|v| matches!(v, "anthropic_messages" | "anthropic-messages")))
                .then(|| "anthropic".into())
            });
            let configured = !explicit_auth(profile_config.auth_method.as_deref())
                && profile_config.model.is_some()
                && profile_config.base_url.is_some()
                && (!profile_config.requires_openai_auth.unwrap_or(true) || api_key_present);
            Ok(ModelCatalogEntry {
                profile: Some(name.clone()),
                active: active.as_deref() == Some(name.as_str()),
                provider: provider.clone(),
                model: profile_config.model.clone(),
                backend,
                configured,
                metadata: metadata(
                    provider.as_deref(),
                    profile_config.model.as_deref(),
                    &profile_config.model_overrides,
                )?,
            })
        })
        .collect()
}

/// Build the structured `/doctor` response. Endpoint values are reduced to
/// scheme plus host by `status_for_profile`; credentials are represented only
/// by presence/source. The checks are intentionally deterministic and local.
pub fn doctor_value(profile: Option<&str>) -> Result<Value, ConfigError> {
    let paths = ConfigPaths::discover()?;
    let status = status_for_profile(&paths, profile)?;
    let ready = status.is_ready();
    let mut checks = BTreeMap::new();
    checks.insert("config_file".to_owned(), status.config_exists);
    checks.insert("auth".to_owned(), status.is_authenticated());
    checks.insert("endpoint".to_owned(), status.base_url.is_some());
    checks.insert("model".to_owned(), status.model.is_some());
    Ok(serde_json::json!({
        "command": "doctor",
        "route": "local",
        "accepted": true,
        "ready": ready,
        "profile": status.profile,
        "backend": status.backend,
        "provider": status.provider,
        "model": status.model,
        "base_url": status.base_url.as_deref().map(redacted_endpoint),
        "wire_api": status.wire_api,
        "auth_method": status.auth_method,
        "auth_ref": status.auth_ref,
        "auth_header": status.auth_header,
        "auth_binding_state": status.auth_binding_state,
        "api_key_present": status.api_key_present,
        "api_key_source": status.api_key_source,
        "requires_openai_auth": status.requires_openai_auth,
        "supports_websockets": status.supports_websockets,
        "checks": checks,
    }))
}

pub fn status(paths: &ConfigPaths) -> Result<ConfigStatus, ConfigError> {
    status_for_profile(paths, None)
}

fn diagnostic_route(
    effective: &EffectiveConfig,
) -> Result<(Option<String>, Option<String>), ConfigError> {
    let Some(connection) = effective.provider_connection(0)? else {
        return Ok((effective.wire_api.clone(), effective.base_url.clone()));
    };
    let selected = connection
        .model_routes
        .iter()
        .find(|route| Some(route.model.as_str()) == effective.model.as_deref());
    let protocol = selected
        .and_then(|r| r.wire_api.as_deref())
        .map(Protocol::parse)
        .transpose()
        .map_err(config_backend_error)?
        .unwrap_or(connection.protocol);
    let base = selected
        .and_then(|r| r.base_url.clone())
        .or(connection.base_url);
    let base = base.or_else(|| {
        get_provider_definition(&connection.provider)
            .and_then(|d| d.routes.iter().find(|r| r.protocol == protocol))
            .and_then(|r| match r.endpoint {
                EndpointRule::Fixed(url) => Some(url),
                EndpointRule::PrefixAndOperation { default_prefix, .. } => default_prefix,
            })
            .map(str::to_owned)
    });
    Ok((Some(protocol.as_str().into()), base))
}

pub fn status_for_profile(
    paths: &ConfigPaths,
    profile: Option<&str>,
) -> Result<ConfigStatus, ConfigError> {
    let config_exists = paths.config.exists();
    let auth_exists = paths.auth.exists();
    // Use the same read-only Codex fallback as runtime resolution for the
    // process-wide profile.  Explicit test/custom paths remain isolated and
    // report only the files represented by that path.
    let resolved = if ConfigPaths::discover().ok().as_ref() == Some(paths) {
        resolve_default(&ConfigOverrides {
            profile: profile.map(str::to_owned),
            ..ConfigOverrides::default()
        })?
    } else {
        let config = load_config(paths)?;
        let environment = text_environment();
        let (_, selected) = config.selected_profile(
            profile.or_else(|| environment.get("ZENPI_PROFILE").map(String::as_str)),
        )?;
        let auth = if explicit_auth(selected.auth_method.as_deref()) {
            AuthFile::default()
        } else {
            load_auth(paths)?
        };
        resolve(
            &ConfigOverrides {
                profile: profile.map(str::to_owned),
                ..ConfigOverrides::default()
            },
            &config,
            &auth,
            &environment,
        )?
    };
    let (wire_api, base_url) = diagnostic_route(&resolved)?;
    let auth_binding_state = match resolved.auth_method.as_deref() {
        None | Some("legacy_api_key") => None,
        Some("none") => Some("anonymous_pending_route".to_owned()),
        // The state comes from the stored credential, never from the presence
        // of the config fields that name it.
        Some(_) => Some(match resolved.auth_ref.as_deref() {
            Some(id) => credential_binding_state(paths, id)?,
            None => "unresolved".to_owned(),
        }),
    };
    Ok(ConfigStatus {
        profile: resolved.profile,
        config_exists,
        auth_exists,
        backend: resolved.backend,
        provider: resolved.provider,
        model: resolved.model,
        base_url,
        wire_api,
        auth_method: resolved.auth_method,
        auth_ref: resolved.auth_ref,
        auth_header: resolved.auth_header,
        auth_binding_state,
        api_key_present: resolved.api_key.is_some(),
        api_key_source: resolved.credential_source,
        requires_openai_auth: resolved.requires_openai_auth,
        supports_websockets: resolved.supports_websockets,
    })
}

/// Load the user-owned BentoBox preference document. A missing file is an
/// empty preference set (all tabs use their built-in presets); an existing
/// malformed, oversized, stale, or symlinked file is an error and is never
/// rewritten as a side effect.
pub fn load_layout_preferences(paths: &ConfigPaths) -> Result<LayoutPreferences, ConfigError> {
    let path = paths.layout_path();
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(LayoutPreferences::default());
        }
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    secure_regular_file(&path)?;
    let bytes = read_layout_bytes(&path)?;
    LayoutPreferences::from_json_bytes(&bytes).map_err(ConfigError::from)
}

/// Migrate a legacy single-`LayoutModel` document in place. The operation is
/// explicit so a read-only status or startup path never rewrites user files;
/// parsing and validation complete before the atomic replacement begins.
pub fn migrate_layout_preferences(paths: &ConfigPaths) -> Result<bool, ConfigError> {
    let path = paths.layout_path();
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    secure_regular_file(&path)?;
    let bytes = read_layout_bytes(&path)?;
    let (preferences, migrated) =
        LayoutPreferences::from_json_bytes_with_migration(&bytes).map_err(ConfigError::from)?;
    if !migrated {
        return Ok(false);
    }
    save_layout_preferences(paths, &preferences)
}

/// Atomically save a validated BentoBox preference document. Serialization and
/// validation happen before any filesystem mutation, while the common config
/// writer preserves the previous valid snapshot if a write or rename fails.
pub fn save_layout_preferences(
    paths: &ConfigPaths,
    preferences: &LayoutPreferences,
) -> Result<bool, ConfigError> {
    let bytes = preferences.to_json_bytes().map_err(ConfigError::from)?;
    paths.ensure_root()?;
    atomic_write_if_changed(&paths.layout_path(), &bytes)
}

/// Resolve one profile/tab to a layout model, falling back to the tab preset
/// when no customization has been saved.
pub fn load_layout(
    paths: &ConfigPaths,
    profile: Option<&str>,
    tab: TabId,
) -> Result<LayoutModel, ConfigError> {
    load_layout_preferences(paths).and_then(|preferences| {
        preferences
            .model_for(profile, tab)
            .map_err(ConfigError::from)
    })
}

/// Persist only user-owned state for one profile/tab. Capabilities remain a
/// host concern and are intentionally not serialized.
pub fn save_layout(
    paths: &ConfigPaths,
    profile: Option<&str>,
    model: &LayoutModel,
) -> Result<bool, ConfigError> {
    let mut preferences = load_layout_preferences(paths)?;
    let changed = preferences
        .set_model(profile, model)
        .map_err(ConfigError::from)?;
    if !changed {
        return Ok(false);
    }
    save_layout_preferences(paths, &preferences)
}

/// Reset one profile/tab to its built-in preset. Reset is idempotent and does
/// not create a preference file when no customization exists.
pub fn reset_layout(
    paths: &ConfigPaths,
    profile: Option<&str>,
    tab: TabId,
) -> Result<bool, ConfigError> {
    let mut preferences = load_layout_preferences(paths)?;
    let changed = preferences
        .reset_tab(profile, tab)
        .map_err(ConfigError::from)?;
    if !changed {
        return Ok(false);
    }
    save_layout_preferences(paths, &preferences)
}

/// Reset every saved tab for one profile. This is useful for a profile-level
/// `/layout reset` command while retaining other profiles' preferences.
pub fn reset_layout_profile(
    paths: &ConfigPaths,
    profile: Option<&str>,
) -> Result<bool, ConfigError> {
    let mut preferences = load_layout_preferences(paths)?;
    let changed = preferences
        .reset_profile(profile)
        .map_err(ConfigError::from)?;
    if !changed {
        return Ok(false);
    }
    save_layout_preferences(paths, &preferences)
}

fn read_layout_bytes(path: &Path) -> Result<Vec<u8>, ConfigError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(path)?;
    let mut bytes = Vec::new();
    // Read one byte beyond the limit so an oversized file is rejected without
    // allocating the complete hostile payload.
    Read::by_ref(&mut file)
        .take((MAX_LAYOUT_PREFERENCES_BYTES as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_LAYOUT_PREFERENCES_BYTES {
        return Err(LayoutError::Json(format!(
            "layout preferences exceed {} bytes",
            MAX_LAYOUT_PREFERENCES_BYTES
        ))
        .into());
    }
    Ok(bytes)
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
    if let Some(key) = auth.openai_api_key() {
        crate::security::register_secret_value(key);
    }
    Ok(auth)
}

pub fn save_config(paths: &ConfigPaths, config: &ConfigFile) -> Result<bool, ConfigError> {
    config.validate()?;
    paths.ensure_root()?;
    let text = toml::to_string_pretty(config)?;
    atomic_write_if_changed(&paths.config, text.as_bytes())
}

/// Whether the provider comes from a built-in definition.  An unknown provider
/// name is not a built-in: it routes as an explicitly configured custom service
/// and must therefore carry its own endpoints and protocol.
fn is_builtin_provider(provider: &str) -> bool {
    get_provider_definition(provider).is_some()
}

/// Credential store over this configuration's auth file.
///
/// `create` makes the owner-only root directory when it is missing; read-only
/// commands pass `false` so `auth list` and `doctor` never create state.  The
/// store walks every component with `O_NOFOLLOW`, so an ancestor that is a link
/// (macOS `/var` and `/tmp`) has to be resolved first — a missing root is left
/// as an absolute path, which the store reports as an empty document.
pub(crate) fn credential_store(
    paths: &ConfigPaths,
    create: bool,
) -> Result<CredentialStore, ConfigError> {
    if create {
        paths.ensure_root()?;
    }
    Ok(CredentialStore::new(resolve_existing_prefix(&paths.auth)?)?)
}

/// Resolve the deepest existing ancestor of `path` and re-append the rest.
///
/// The store walks every component with `O_NOFOLLOW`, so a symlinked ancestor
/// (macOS `/var` and `/tmp`) has to be resolved before it opens the path.  The
/// path itself may not exist yet — a read of a missing store is an empty store,
/// not an error — so only the part that exists can be canonicalized.
fn resolve_existing_prefix(path: &Path) -> Result<PathBuf, ConfigError> {
    let absolute = std::path::absolute(path)?;
    let mut missing = Vec::new();
    let mut current = absolute.as_path();
    loop {
        if let Ok(resolved) = current.canonicalize() {
            let mut out = resolved;
            for part in missing.iter().rev() {
                out.push(part);
            }
            return Ok(out);
        }
        match (current.parent(), current.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name.to_os_string());
                current = parent;
            }
            // Nothing on this path exists; the store reports it as empty.
            _ => return Ok(absolute),
        }
    }
}

/// Mutate the legacy root of `auth.json` through the credential store's stable
/// lock.  Reading and writing under that lock is what keeps a legacy write from
/// reverting a credential that another process committed in the meantime, and
/// it is why the `zenpi_auth_v1` namespace is always taken from the live
/// document instead of a caller's snapshot.
///
/// The closure reports whether it changed the document.  Returning `false`, or
/// failing, leaves the stored bytes untouched.
fn update_auth_legacy(
    paths: &ConfigPaths,
    update: impl FnOnce(&mut AuthFile) -> Result<bool, ConfigError>,
) -> Result<bool, ConfigError> {
    let store = credential_store(paths, true)?;
    let (outcome, _) = store.update_legacy(&LockWait::default(), |legacy| {
        let before = legacy.clone();
        let mut auth = AuthFile(before.clone().into_iter().collect());
        let outcome = update(&mut auth);
        // Only a reported change may replace the live roots; a rejected or
        // no-op mutation must not rewrite the file at all.
        *legacy = if outcome.as_ref().is_ok_and(|changed| *changed) {
            auth.0.into_iter().collect()
        } else {
            before
        };
        Ok(outcome)
    })?;
    outcome
}

/// Whole-file legacy write.  The reserved namespace is stripped from the
/// caller's snapshot: the store owns that key and re-reads its live value.
pub fn save_auth(paths: &ConfigPaths, auth: &AuthFile) -> Result<bool, ConfigError> {
    if let Some(key) = auth.openai_api_key()
        && (key.trim().is_empty() || key.chars().any(char::is_control))
    {
        return Err(ConfigError::Invalid("OPENAI_API_KEY is invalid".into()));
    }
    let mut roots = auth.clone();
    roots.0.remove(STORE_KEY);
    update_auth_legacy(paths, |legacy| {
        if *legacy == roots {
            return Ok(false);
        }
        *legacy = roots;
        Ok(true)
    })
}

fn write_config_if_changed(paths: &ConfigPaths, config: &ConfigFile) -> Result<bool, ConfigError> {
    config.validate()?;
    paths.ensure_root()?;
    let text = toml::to_string_pretty(config)?;
    atomic_write_if_changed(&paths.config, text.as_bytes())
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
    #[error("layout preferences: {0}")]
    Layout(#[from] LayoutError),
    #[error("credential store: {0}")]
    Credential(#[from] crate::auth::store::StoreError),
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

fn validate_profile_name(name: &str) -> Result<(), ConfigError> {
    if name.is_empty()
        || name.len() > 128
        || name
            .chars()
            .any(|character| !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-')))
    {
        return Err(ConfigError::Invalid(
            "profile name must use 1-128 ASCII letters, digits, `_`, or `-`".into(),
        ));
    }
    Ok(())
}

fn validate_api_key(key: &str) -> Result<(), ConfigError> {
    if key.trim().is_empty() || key.chars().any(char::is_control) || key.len() > 16 * 1024 {
        return Err(ConfigError::Invalid(
            "API key must be non-empty, bounded, and contain no control characters".into(),
        ));
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

pub(crate) fn atomic_write_if_changed(path: &Path, bytes: &[u8]) -> Result<bool, ConfigError> {
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

/// A user-selected interactive editor, separate from provider configuration.
/// Deliberately has no Debug implementation: argv/environment may be private.
#[derive(Clone)]
pub struct EditorCommand {
    pub source: &'static str,
    pub argv: Vec<String>,
    pub environment: BTreeMap<std::ffi::OsString, std::ffi::OsString>,
    pub timeout: std::time::Duration,
}

/// The complete allowlist for the explicitly requested local editor process.
const EDITOR_ENVIRONMENT: &[&str] = &[
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
    "XDG_STATE_HOME",
    "XDG_CONFIG_DIRS",
    "XDG_DATA_DIRS",
    "LC_ALL",
    "LC_CTYPE",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "LC_PAPER",
    "LC_NAME",
    "LC_ADDRESS",
    "LC_TELEPHONE",
    "LC_MEASUREMENT",
    "LC_IDENTIFICATION",
];

/// Resolve only the startup environment, never a workspace/provider setting.
/// Error strings describe the boundary without echoing user command arguments.
pub fn resolve_editor_command(
    environment: &BTreeMap<std::ffi::OsString, std::ffi::OsString>,
) -> Result<EditorCommand, String> {
    use std::ffi::OsStr;
    let (source, raw) = environment
        .get(OsStr::new("VISUAL"))
        .map(|value| ("VISUAL", value))
        .or_else(|| environment.get(OsStr::new("EDITOR")).map(|v| ("EDITOR", v)))
        .ok_or("External editor unavailable: set VISUAL or EDITOR before starting Zenpi")?;
    let raw = raw.to_str().ok_or("Editor command must be UTF-8")?;
    if raw.len() > 4096 || raw.chars().any(|c| c.is_control() && c != '\t') {
        return Err("Editor command exceeds 4096 bytes or contains a forbidden control".into());
    }
    let argv = parse_editor_argv(raw)?;
    let timeout = match environment.get(OsStr::new("ZENPI_EDITOR_TIMEOUT_SECONDS")) {
        None => 1800,
        Some(value) => {
            let value = value
                .to_str()
                .ok_or("Editor timeout must be UTF-8 decimal seconds")?;
            if value.is_empty() || value.len() > 20 || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err("Editor timeout must be an integer from 1 to 7200 seconds".into());
            }
            value
                .parse::<u64>()
                .ok()
                .filter(|n| (1..=7200).contains(n))
                .ok_or("Editor timeout must be an integer from 1 to 7200 seconds")?
        }
    };
    let mut child_environment = BTreeMap::new();
    let mut total = 0usize;
    for key in EDITOR_ENVIRONMENT {
        if let Some(value) = environment.get(OsStr::new(key)) {
            let bytes = value.as_encoded_bytes();
            total += key.len() + bytes.len() + 2;
            if bytes.len() > 4096 || bytes.contains(&0) || total > 64 * 1024 {
                return Err(
                    "Editor environment exceeds its private allowlist budget or contains NUL"
                        .into(),
                );
            }
            child_environment.insert((*key).into(), value.clone());
        }
    }
    Ok(EditorCommand {
        source,
        argv,
        environment: child_environment,
        timeout: std::time::Duration::from_secs(timeout),
    })
}

fn parse_editor_argv(raw: &str) -> Result<Vec<String>, String> {
    let mut chars = raw.chars().peekable();
    let mut args = Vec::new();
    let mut argument = String::new();
    let mut quote = None;
    let mut started = false;
    let finish = |argument: &mut String, args: &mut Vec<String>| -> Result<(), String> {
        if argument.len() > 2048 || args.len() >= 32 {
            return Err("Editor command exceeds 32 arguments or 2048 bytes per argument".into());
        }
        args.push(std::mem::take(argument));
        Ok(())
    };
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => argument.push(ch),
            (Some('"'), '\\') => match chars.peek() {
                Some('"' | '\\' | '$' | '`') => argument.push(chars.next().unwrap()),
                _ => argument.push('\\'),
            },
            (Some(_), _) => argument.push(ch),
            (None, '\'' | '"') => {
                quote = Some(ch);
                started = true;
            }
            (None, '\\') => {
                argument.push(chars.next().ok_or("Editor command has a trailing escape")?);
                started = true;
            }
            (None, ' ' | '\t') => {
                if started {
                    finish(&mut argument, &mut args)?;
                    started = false;
                }
            }
            (None, _) => {
                argument.push(ch);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err("Editor command has an unterminated quote".into());
    }
    if started {
        finish(&mut argument, &mut args)?;
    }
    if args.first().is_none_or(String::is_empty) {
        return Err("Editor command is empty".into());
    }
    Ok(args)
}

// Provider resolution consumes textual variables only. An unrelated non-UTF8
// VISUAL/EDITOR must reach the separate OsString editor validator, not panic
// while an earlier provider configuration snapshot iterates the environment.
fn text_environment() -> BTreeMap<String, String> {
    text_environment_values(env::vars_os())
}

fn text_environment_values(
    values: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> BTreeMap<String, String> {
    values
        .into_iter()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .collect()
}

#[cfg(all(test, unix))]
mod environment_snapshot_tests {
    use super::*;
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    #[test]
    fn valid_text_values_are_unchanged_and_editor_bytes_reach_separate_validator() {
        let original: BTreeMap<OsString, OsString> = [
            ("OPENAI_API_KEY".into(), "fixture token  ".into()),
            ("ZENPI_MODEL".into(), "unicode-界".into()),
            ("EDITOR".into(), "fallback --wait".into()),
            ("VISUAL".into(), OsString::from_vec(vec![0xff])),
            (
                OsString::from_vec(vec![0xfe]),
                "unrelated invalid key".into(),
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            text_environment_values(original.clone()),
            BTreeMap::from([
                ("OPENAI_API_KEY".into(), "fixture token  ".into()),
                ("ZENPI_MODEL".into(), "unicode-界".into()),
                ("EDITOR".into(), "fallback --wait".into()),
            ])
        );
        assert!(resolve_editor_command(&original).is_err());
        assert_eq!(
            original.get(std::ffi::OsStr::new("VISUAL")).unwrap(),
            &OsString::from_vec(vec![0xff])
        );
    }
}

/// The legacy writers share one stable lock with the credential store.  These
/// tests pin the two properties that lock exists for: a legacy write never
/// carries a stale namespace over a newer credential, and it never drops a
/// credential it did not own.
#[cfg(all(test, unix))]
mod legacy_lock_tests {
    use super::*;
    use crate::auth::AllowedDestination;
    use crate::auth::store::{CredentialKind, LockWait, Mutation, PendingCredential, Replacement};
    use std::os::unix::fs::PermissionsExt;

    fn fixture() -> (tempfile::TempDir, ConfigPaths) {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let paths = ConfigPaths::for_home(directory.path());
        paths.ensure_root().unwrap();
        (directory, paths)
    }

    fn store_for(paths: &ConfigPaths) -> CredentialStore {
        credential_store(paths, true).unwrap()
    }

    fn pending(refresh_token: &str) -> PendingCredential {
        PendingCredential {
            kind: CredentialKind::Oauth,
            provider: "synthetic-provider".into(),
            definition_version: 1,
            issuer: "https://issuer.example.test".into(),
            client_id: "synthetic-client".into(),
            account_id: Some("synthetic-account".into()),
            user_id: Some("synthetic-user".into()),
            allowed_destinations: vec![AllowedDestination {
                origin: "https://issuer.example.test".into(),
                path_prefix: "/v1".into(),
                protocols: vec!["responses".into()],
                headers: vec!["authorization".into()],
            }],
            api_key: None,
            access_token: Some("synthetic-access-token".into()),
            refresh_token: Some(refresh_token.into()),
            expires_at_ms: Some(1),
        }
    }

    /// The stored document as written.  Reading the file directly keeps the
    /// assertions about on-disk state independent of the store's private API.
    fn document(paths: &ConfigPaths) -> Value {
        match fs::read(&paths.auth) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Value::Null,
            Err(error) => panic!("unreadable auth file: {error}"),
        }
    }

    fn namespace_revision(paths: &ConfigPaths) -> u64 {
        document(paths)[STORE_KEY]["revision"].as_u64().unwrap()
    }

    fn credential(paths: &ConfigPaths) -> Value {
        document(paths)[STORE_KEY]["accounts"]["credential"].clone()
    }

    fn stored_refresh_token(paths: &ConfigPaths) -> String {
        credential(paths)["refresh_token"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Log in the way the CLI does: commit against the revision that is
    /// actually on disk, so a second login replaces the first.
    fn relogin(paths: &ConfigPaths, refresh_token: &str, expires_at_ms: Option<u64>) {
        let expected = credential(paths)["revision"].as_u64();
        let mut pending = pending(refresh_token);
        pending.expires_at_ms = expires_at_ms;
        store_for(paths)
            .modify(
                "credential",
                expected,
                Mutation::Replace(Replacement::Login(pending)),
                &LockWait::default(),
            )
            .unwrap();
    }

    fn login(paths: &ConfigPaths, refresh_token: &str) {
        relogin(paths, refresh_token, Some(1));
    }

    #[test]
    fn legacy_write_preserves_a_credential_committed_after_its_snapshot() {
        let (_directory, paths) = fixture();
        login(&paths, "synthetic-first-refresh-token");

        // What a command that reads credentials before mutating legacy roots
        // would hold: a snapshot that still carries the namespace.
        let snapshot = load_auth(&paths).unwrap();
        assert!(
            snapshot.0.contains_key(STORE_KEY),
            "the snapshot must really carry the namespace for this test to mean anything"
        );

        // Another process logs in while that command is running.
        login(&paths, "synthetic-second-refresh-token");
        let committed = credential(&paths);

        save_auth(&paths, &snapshot).unwrap();

        assert_eq!(
            stored_refresh_token(&paths),
            "synthetic-second-refresh-token",
            "a legacy write must not revert a newer credential"
        );
        assert_eq!(
            credential(&paths),
            committed,
            "the namespace must survive a legacy write untouched"
        );
    }

    #[test]
    fn revoke_and_import_reach_the_same_locked_document() {
        let (_directory, paths) = fixture();
        login(&paths, "synthetic-refresh-token");
        let before = credential(&paths);
        let revision = namespace_revision(&paths);

        let mut auth = load_auth(&paths).unwrap();
        auth.set_openai_api_key("synthetic-legacy-key").unwrap();
        assert!(save_auth(&paths, &auth).unwrap());

        assert_eq!(document(&paths)["OPENAI_API_KEY"], "synthetic-legacy-key");
        assert_eq!(credential(&paths), before);
        // The write went through the store, so the file revision advanced even
        // though only legacy roots changed.
        assert_eq!(namespace_revision(&paths), revision + 1);

        assert!(revoke(&paths, None).unwrap());
        assert!(document(&paths).get("OPENAI_API_KEY").is_none());
        assert_eq!(namespace_revision(&paths), revision + 2);

        // Both directions of the command-facing read survive legacy traffic.
        let store = store_for(&paths);
        let stored = store.list_status().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].credential_id, "credential");
        assert!(store.identity("credential").unwrap().credential_revision > 0);
    }

    #[test]
    fn no_op_legacy_writes_leave_the_file_untouched() {
        let (_directory, paths) = fixture();
        login(&paths, "synthetic-refresh-token");
        let before = fs::read(&paths.auth).unwrap();

        assert!(!save_auth(&paths, &load_auth(&paths).unwrap()).unwrap());
        assert!(!revoke(&paths, None).unwrap());
        assert!(!revoke(&paths, Some("codex")).unwrap());
        assert_eq!(fs::read(&paths.auth).unwrap(), before);

        // A profile entry that owns no key is still not a change: the mutation
        // reports nothing removed, so the document is left alone.
        let mut auth = load_auth(&paths).unwrap();
        auth.0
            .insert("profiles".into(), serde_json::json!({"codex": {}}));
        assert!(save_auth(&paths, &auth).unwrap());
        let with_empty_profile = fs::read(&paths.auth).unwrap();
        assert!(!revoke(&paths, Some("codex")).unwrap());
        assert_eq!(fs::read(&paths.auth).unwrap(), with_empty_profile);

        assert_eq!(stored_refresh_token(&paths), "synthetic-refresh-token");
    }

    #[test]
    fn status_projects_the_stored_credential_not_the_config_fields() {
        let (_directory, paths) = fixture();
        login(&paths, "synthetic-refresh-token");
        let mut config = load_config(&paths).unwrap();
        config.profiles.insert(
            "bound".into(),
            ProviderProfile {
                provider: Some("openai-codex".into()),
                model: Some("gpt-test".into()),
                auth_method: Some("oauth".into()),
                auth_ref: Some("credential".into()),
                ..ProviderProfile::default()
            },
        );
        save_config(&paths, &config).unwrap();

        // The stored credential here expired in 1970, so the projection reports
        // the clock rather than the presence of the fields.
        let status = status_for_profile(&paths, Some("bound")).unwrap();
        assert_eq!(status.auth_binding_state.as_deref(), Some("expired"));
        assert!(!status.is_ready());

        let future = u64::try_from(now_ms()).unwrap() + 3_600_000;
        relogin(&paths, "synthetic-refresh-token", Some(future));
        let status = status_for_profile(&paths, Some("bound")).unwrap();
        assert_eq!(status.auth_binding_state.as_deref(), Some("ready"));
        assert!(status.is_authenticated());
        assert!(status.is_ready());

        // A profile that merely names a credential is not ready on the
        // strength of having named one.
        let mut config = load_config(&paths).unwrap();
        config.profiles.get_mut("bound").unwrap().auth_ref = Some("missing".into());
        save_config(&paths, &config).unwrap();
        let status = status_for_profile(&paths, Some("bound")).unwrap();
        assert_eq!(status.auth_binding_state.as_deref(), Some("unconfigured"));
        assert!(!status.is_authenticated());
        assert!(!status.is_ready());

        // And revoking the credential is what changes the answer back.
        let mut config = load_config(&paths).unwrap();
        config.profiles.get_mut("bound").unwrap().auth_ref = Some("credential".into());
        save_config(&paths, &config).unwrap();
        revoke_credential(&paths, "bound").unwrap();
        let status = status_for_profile(&paths, Some("bound")).unwrap();
        assert_eq!(status.auth_binding_state.as_deref(), Some("revoked"));
        assert!(!status.is_ready());
    }

    #[test]
    fn a_forged_namespace_in_the_caller_snapshot_cannot_replace_the_store() {
        let (_directory, paths) = fixture();
        let store = store_for(&paths);
        login(&paths, "synthetic-refresh-token");

        let mut forged = AuthFile::default();
        forged.0.insert(
            STORE_KEY.into(),
            serde_json::json!({"version": 1, "revision": 99, "accounts": {}}),
        );
        save_auth(&paths, &forged).unwrap();

        assert_eq!(
            stored_refresh_token(&paths),
            "synthetic-refresh-token",
            "the reserved namespace belongs to the store, not to a caller snapshot"
        );
        assert_eq!(store.list_status().unwrap().len(), 1);
    }
}
