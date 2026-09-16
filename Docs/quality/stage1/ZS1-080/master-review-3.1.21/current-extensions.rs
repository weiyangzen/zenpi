//! Installable local-process tools and opaque capability handles.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::tools::{Tool, ToolContext, ToolDefinition, ToolError, ToolSideEffect};

pub const EXTENSION_MANIFEST: &str = "extension.toml";
pub const EXTENSION_API_VERSION: u32 = 1;
pub const EXTENSION_HOOK_API_VERSION: u32 = 2;
pub const MAX_EXTENSION_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExtensionManifest {
    pub name: String,
    pub version: String,
    pub api_version: u32,
    #[serde(default)]
    pub disabled: bool,
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub permissions: ExtensionPermissions,
    #[serde(default)]
    pub tools: Vec<ExtensionToolManifest>,
    #[serde(default)]
    pub hooks: Vec<crate::extension_runtime::HookKind>,
    #[serde(default = "default_hook_timeout")]
    pub hook_timeout_ms: u64,
}

fn default_hook_timeout() -> u64 {
    1000
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExtensionPermissions {
    #[serde(default)]
    pub workspace_read: bool,
    #[serde(default)]
    pub workspace_write: bool,
    #[serde(default)]
    pub command_execution: bool,
    #[serde(default)]
    pub network: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExtensionToolManifest {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub side_effect: ToolSideEffect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionSummary {
    pub name: String,
    pub version: String,
    pub disabled: bool,
    pub compatible: bool,
    pub path: String,
    pub tools: Vec<String>,
    pub permissions: ExtensionPermissions,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedExtension {
    pub(crate) manifest: ExtensionManifest,
    pub(crate) directory: PathBuf,
    pub(crate) executable: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct ExtensionCatalog {
    pub(crate) loaded: BTreeMap<String, LoadedExtension>,
    summaries: Vec<ExtensionSummary>,
}

impl ExtensionManifest {
    fn validate(&self) -> Result<(), ExtensionError> {
        validate_id(&self.name)?;
        validate_version(&self.version)?;
        if !matches!(
            self.api_version,
            EXTENSION_API_VERSION | EXTENSION_HOOK_API_VERSION
        ) {
            return Err(ExtensionError::Incompatible {
                name: self.name.clone(),
                found: self.api_version,
                supported: EXTENSION_HOOK_API_VERSION,
            });
        }
        validate_relative_component_path(&self.executable)?;
        if self.args.len() > 64
            || self
                .args
                .iter()
                .any(|arg| arg.len() > 4096 || arg.contains('\0'))
        {
            return Err(ExtensionError::Invalid("extension args are invalid".into()));
        }
        if (self.tools.is_empty() && self.hooks.is_empty()) || self.tools.len() > 64 {
            return Err(ExtensionError::Invalid(
                "extension must declare hooks or 1 to 64 tools".into(),
            ));
        }
        if (!self.hooks.is_empty() && self.api_version != EXTENSION_HOOK_API_VERSION)
            || self.hooks.len() > 8
            || self.hooks.iter().collect::<BTreeSet<_>>().len() != self.hooks.len()
            || !(1..=1000).contains(&self.hook_timeout_ms)
        {
            return Err(ExtensionError::Invalid(
                "invalid hook version, duplicates or timeout".into(),
            ));
        }
        let mut names = BTreeSet::new();
        for tool in &self.tools {
            validate_id(&tool.name)?;
            let definition = ToolDefinition {
                name: tool.name.clone(),
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
                side_effect: tool.side_effect,
            };
            definition
                .validate()
                .map_err(|error| ExtensionError::Invalid(error.to_string()))?;
            if !names.insert(tool.name.clone()) {
                return Err(ExtensionError::Invalid(format!(
                    "duplicate extension tool `{}`",
                    tool.name
                )));
            }
            if !permission_allows(&self.permissions, tool.side_effect) {
                return Err(ExtensionError::Invalid(format!(
                    "tool `{}` exceeds declared extension permissions",
                    tool.name
                )));
            }
        }
        Ok(())
    }
}

impl ExtensionCatalog {
    pub fn load(root: &Path) -> Result<Self, ExtensionError> {
        if !root.exists() {
            return Ok(Self::default());
        }
        if !root.is_dir() {
            return Err(ExtensionError::Path(root.to_owned()));
        }
        let canonical_root = root.canonicalize()?;
        let mut paths = fs::read_dir(root)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        paths.sort();
        let mut catalog = Self::default();
        for path in paths {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.'))
            {
                continue;
            }
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                continue;
            }
            let directory = path.canonicalize()?;
            if !directory.starts_with(&canonical_root) {
                return Err(ExtensionError::Path(path));
            }
            let manifest_path = directory.join(EXTENSION_MANIFEST);
            if !manifest_path.is_file() {
                continue;
            }
            if fs::symlink_metadata(&manifest_path)?
                .file_type()
                .is_symlink()
            {
                return Err(ExtensionError::Path(manifest_path));
            }
            let mut text = String::new();
            fs::File::open(&manifest_path)?
                .take(256 * 1024 + 1)
                .read_to_string(&mut text)?;
            if text.len() > 256 * 1024 {
                return Err(ExtensionError::Invalid(
                    "extension manifest is too large".into(),
                ));
            }
            let manifest: ExtensionManifest = toml::from_str(&text)?;
            let compatible = matches!(
                manifest.api_version,
                EXTENSION_API_VERSION | EXTENSION_HOOK_API_VERSION
            );
            let summary = ExtensionSummary {
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                disabled: manifest.disabled,
                compatible,
                path: directory.display().to_string(),
                tools: manifest
                    .tools
                    .iter()
                    .map(|tool| tool.name.clone())
                    .collect(),
                permissions: manifest.permissions.clone(),
            };
            if manifest.disabled {
                catalog.summaries.push(summary);
                continue;
            }
            manifest.validate()?;
            let executable = directory.join(&manifest.executable);
            let executable = executable
                .canonicalize()
                .map_err(|_| ExtensionError::Path(executable))?;
            if !executable.starts_with(&directory) || !executable.is_file() {
                return Err(ExtensionError::Path(executable));
            }
            if catalog.loaded.contains_key(&manifest.name) {
                return Err(ExtensionError::Invalid(format!(
                    "duplicate extension `{}`",
                    manifest.name
                )));
            }
            if catalog.loaded.len() >= 32 {
                return Err(ExtensionError::Invalid(
                    "at most 32 enabled extensions are allowed".into(),
                ));
            }
            catalog.loaded.insert(
                manifest.name.clone(),
                LoadedExtension {
                    manifest,
                    directory,
                    executable,
                },
            );
            catalog.summaries.push(summary);
        }
        Ok(catalog)
    }

    pub fn summaries(&self) -> &[ExtensionSummary] {
        &self.summaries
    }

    pub fn register_tools(
        &self,
        registry: &mut crate::tools::ToolRegistry,
    ) -> Result<(), ExtensionError> {
        if self
            .loaded
            .values()
            .any(|extension| extension.manifest.api_version == EXTENSION_HOOK_API_VERSION)
        {
            return Err(ExtensionError::Invalid(
                "API 2 extensions require a session runtime".into(),
            ));
        }
        self.register_with_lease(registry, None)
    }

    pub(crate) fn register_with_lease(
        &self,
        registry: &mut crate::tools::ToolRegistry,
        lease: Option<crate::extension_runtime::ExtensionLease>,
    ) -> Result<(), ExtensionError> {
        let mut candidate = registry.clone();
        for extension in self.loaded.values() {
            for tool in &extension.manifest.tools {
                candidate
                    .register_boxed(Box::new(LocalProcessTool {
                        definition: ToolDefinition {
                            name: tool.name.clone(),
                            description: tool.description.clone(),
                            input_schema: tool.input_schema.clone(),
                            side_effect: tool.side_effect,
                        },
                        extension: extension.clone(),
                        lease: lease.clone(),
                    }))
                    .map_err(|error| ExtensionError::Tool(error.to_string()))?;
            }
        }
        *registry = candidate;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct LocalProcessTool {
    definition: ToolDefinition,
    extension: LoadedExtension,
    lease: Option<crate::extension_runtime::ExtensionLease>,
}

impl Tool for LocalProcessTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn invoke(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ToolError> {
        self.invoke_cancellable(context, arguments, &|| false)
    }

    fn invoke_cancellable(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        if !permission_allows(
            &self.extension.manifest.permissions,
            self.definition.side_effect,
        ) {
            return Err(ToolError::PolicyDenied {
                tool: self.definition.name.clone(),
                side_effect: self.definition.side_effect,
            });
        }
        crate::extension_runtime::process_request(
            &self.extension,
            context.workspace_root(),
            self.lease.as_ref(),
            "tools/call",
            json!({"name":self.definition.name,"arguments":arguments}),
            Duration::from_secs(30),
            cancelled,
        )
    }
}

pub fn install(root: &Path, source: &Path) -> Result<ExtensionSummary, ExtensionError> {
    if !source.is_dir() {
        return Err(ExtensionError::Path(source.to_owned()));
    }
    let text = fs::read_to_string(source.join(EXTENSION_MANIFEST))?;
    let manifest: ExtensionManifest = toml::from_str(&text)?;
    manifest.validate()?;
    fs::create_dir_all(root)?;
    let destination = root.join(&manifest.name);
    let staging = root.join(format!(".{}.installing", manifest.name));
    if destination.exists() || staging.exists() {
        return Err(ExtensionError::AlreadyInstalled(manifest.name));
    }
    copy_tree(source, &staging)?;
    fs::rename(&staging, &destination)?;
    let catalog = ExtensionCatalog::load(root)?;
    catalog
        .summaries
        .into_iter()
        .find(|summary| summary.name == manifest.name)
        .ok_or_else(|| ExtensionError::Invalid("installed extension did not load".into()))
}

pub fn remove(root: &Path, name: &str) -> Result<bool, ExtensionError> {
    validate_id(name)?;
    let path = root.join(name);
    if !path.exists() {
        return Ok(false);
    }
    fs::remove_dir_all(path)?;
    Ok(true)
}

pub fn upgrade(root: &Path, source: &Path) -> Result<ExtensionSummary, ExtensionError> {
    if !source.is_dir() {
        return Err(ExtensionError::Path(source.to_owned()));
    }
    let text = fs::read_to_string(source.join(EXTENSION_MANIFEST))?;
    let manifest: ExtensionManifest = toml::from_str(&text)?;
    manifest.validate()?;
    let destination = root.join(&manifest.name);
    if !destination.is_dir() {
        return Err(ExtensionError::Path(destination));
    }
    let staging = root.join(format!(".{}.upgrading", manifest.name));
    let backup = root.join(format!(".{}.backup", manifest.name));
    if staging.exists() || backup.exists() {
        return Err(ExtensionError::Invalid(
            "a previous extension upgrade needs cleanup".into(),
        ));
    }
    copy_tree(source, &staging)?;
    fs::rename(&destination, &backup)?;
    if let Err(error) = fs::rename(&staging, &destination) {
        let _ = fs::rename(&backup, &destination);
        return Err(error.into());
    }
    let catalog = match ExtensionCatalog::load(root) {
        Ok(catalog) => catalog,
        Err(error) => {
            let _ = fs::remove_dir_all(&destination);
            let _ = fs::rename(&backup, &destination);
            return Err(error);
        }
    };
    fs::remove_dir_all(backup)?;
    catalog
        .summaries
        .into_iter()
        .find(|summary| summary.name == manifest.name)
        .ok_or_else(|| ExtensionError::Invalid("upgraded extension did not load".into()))
}

pub fn set_disabled(root: &Path, name: &str, disabled: bool) -> Result<bool, ExtensionError> {
    validate_id(name)?;
    let path = root.join(name).join(EXTENSION_MANIFEST);
    if !path.is_file() {
        return Err(ExtensionError::Path(path));
    }
    let text = fs::read_to_string(&path)?;
    let mut manifest: ExtensionManifest = toml::from_str(&text)?;
    if manifest.disabled == disabled {
        return Ok(false);
    }
    manifest.disabled = disabled;
    let encoded = toml::to_string_pretty(&manifest)
        .map_err(|error| ExtensionError::Invalid(error.to_string()))?;
    let temporary = path.with_extension("toml.tmp");
    fs::write(&temporary, encoded)?;
    fs::rename(temporary, path)?;
    Ok(true)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), ExtensionError> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let metadata = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if metadata.is_symlink() {
            let _ = fs::remove_dir_all(destination);
            return Err(ExtensionError::Path(entry.path()));
        }
        if metadata.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if metadata.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn permission_allows(permissions: &ExtensionPermissions, effect: ToolSideEffect) -> bool {
    match effect {
        ToolSideEffect::ReadOnly => permissions.workspace_read,
        ToolSideEffect::WorkspaceWrite => permissions.workspace_write,
        ToolSideEffect::CommandExecution => permissions.command_execution,
    }
}

fn validate_id(value: &str) -> Result<(), ExtensionError> {
    if value.is_empty()
        || value.len() > 128
        || value
            .chars()
            .any(|character| !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-')))
    {
        return Err(ExtensionError::Invalid(format!(
            "invalid identifier `{value}`"
        )));
    }
    Ok(())
}

fn validate_version(value: &str) -> Result<(), ExtensionError> {
    if value.is_empty()
        || value.len() > 64
        || value.chars().any(|character| {
            !(character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+'))
        })
    {
        return Err(ExtensionError::Invalid("invalid extension version".into()));
    }
    Ok(())
}

fn validate_relative_component_path(value: &str) -> Result<(), ExtensionError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > 4096
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(ExtensionError::Invalid(
            "extension executable path is invalid".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityScope {
    WorkspaceRead,
    WorkspaceWrite,
    CommandExecution,
    Network,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityHandle {
    pub id: String,
    pub subject: String,
    pub scopes: BTreeSet<CapabilityScope>,
    pub expires_at_ms: u64,
}

#[derive(Debug, Default)]
pub struct CapabilityBroker {
    handles: BTreeMap<String, CapabilityHandle>,
    counter: AtomicU64,
}

impl CapabilityBroker {
    pub fn issue(
        &mut self,
        subject: &str,
        scopes: BTreeSet<CapabilityScope>,
        ttl: Duration,
    ) -> Result<CapabilityHandle, ExtensionError> {
        validate_id(subject)?;
        if scopes.is_empty() || ttl.is_zero() || ttl > Duration::from_secs(24 * 60 * 60) {
            return Err(ExtensionError::Invalid(
                "capability scope or lifetime is invalid".into(),
            ));
        }
        let now = now_ms();
        let serial = self.counter.fetch_add(1, Ordering::Relaxed);
        let mut digest = Sha256::new();
        digest.update(b"zenpi-capability-v1\0");
        digest.update(subject.as_bytes());
        digest.update(now.to_le_bytes());
        digest.update(serial.to_le_bytes());
        let handle = CapabilityHandle {
            id: format!("cap_{:x}", digest.finalize()),
            subject: subject.to_owned(),
            scopes,
            expires_at_ms: now.saturating_add(ttl.as_millis().min(u128::from(u64::MAX)) as u64),
        };
        self.handles.insert(handle.id.clone(), handle.clone());
        Ok(handle)
    }

    pub fn authorize(&self, id: &str, subject: &str, scope: CapabilityScope) -> bool {
        self.handles.get(id).is_some_and(|handle| {
            handle.subject == subject
                && handle.expires_at_ms >= now_ms()
                && handle.scopes.contains(&scope)
        })
    }

    pub fn revoke(&mut self, id: &str) -> bool {
        self.handles.remove(id).is_some()
    }

    pub fn revoke_subject(&mut self, subject: &str) -> usize {
        let before = self.handles.len();
        self.handles.retain(|_, handle| handle.subject != subject);
        before - self.handles.len()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[derive(Debug, Error)]
pub enum ExtensionError {
    #[error("extension I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("extension TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("invalid extension: {0}")]
    Invalid(String),
    #[error("extension path is denied: {0}")]
    Path(PathBuf),
    #[error("extension `{name}` requires API {found}; supported API is {supported}")]
    Incompatible {
        name: String,
        found: u32,
        supported: u32,
    },
    #[error("extension `{0}` is already installed")]
    AlreadyInstalled(String),
    #[error("extension tool: {0}")]
    Tool(String),
}
