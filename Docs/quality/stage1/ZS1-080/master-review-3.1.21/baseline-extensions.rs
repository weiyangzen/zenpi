//! Installable local-process tools and opaque capability handles.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
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
    pub tools: Vec<ExtensionToolManifest>,
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
struct LoadedExtension {
    manifest: ExtensionManifest,
    directory: PathBuf,
    executable: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct ExtensionCatalog {
    loaded: BTreeMap<String, LoadedExtension>,
    summaries: Vec<ExtensionSummary>,
}

impl ExtensionManifest {
    fn validate(&self) -> Result<(), ExtensionError> {
        validate_id(&self.name)?;
        validate_version(&self.version)?;
        if self.api_version != EXTENSION_API_VERSION {
            return Err(ExtensionError::Incompatible {
                name: self.name.clone(),
                found: self.api_version,
                supported: EXTENSION_API_VERSION,
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
        if self.tools.is_empty() || self.tools.len() > 64 {
            return Err(ExtensionError::Invalid(
                "extension must declare between 1 and 64 tools".into(),
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
            let text = fs::read_to_string(&manifest_path)?;
            if text.len() > 256 * 1024 {
                return Err(ExtensionError::Invalid(
                    "extension manifest is too large".into(),
                ));
            }
            let manifest: ExtensionManifest = toml::from_str(&text)?;
            let compatible = manifest.api_version == EXTENSION_API_VERSION;
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
        for extension in self.loaded.values() {
            for tool in &extension.manifest.tools {
                registry
                    .register_boxed(Box::new(LocalProcessTool {
                        definition: ToolDefinition {
                            name: tool.name.clone(),
                            description: tool.description.clone(),
                            input_schema: tool.input_schema.clone(),
                            side_effect: tool.side_effect,
                        },
                        extension: extension.manifest.name.clone(),
                        executable: extension.executable.clone(),
                        args: extension.manifest.args.clone(),
                        directory: extension.directory.clone(),
                        permissions: extension.manifest.permissions.clone(),
                        timeout: Duration::from_secs(30),
                    }))
                    .map_err(|error| ExtensionError::Tool(error.to_string()))?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct LocalProcessTool {
    definition: ToolDefinition,
    extension: String,
    executable: PathBuf,
    args: Vec<String>,
    directory: PathBuf,
    permissions: ExtensionPermissions,
    timeout: Duration,
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
        if !permission_allows(&self.permissions, self.definition.side_effect) {
            return Err(ToolError::PolicyDenied {
                tool: self.definition.name.clone(),
                side_effect: self.definition.side_effect,
            });
        }
        let mut command = Command::new(&self.executable);
        command
            .args(&self.args)
            .current_dir(&self.directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        command.env_clear();
        for (key, value) in crate::security::child_environment() {
            command.env(key, value);
        }
        command
            .env("ZENPI_EXTENSION_NAME", &self.extension)
            .env("ZENPI_WORKSPACE", context.workspace_root());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn()?;
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": self.definition.name,
                "arguments": arguments,
            }
        });
        let encoded = serde_json::to_vec(&request).map_err(ToolError::Json)?;
        if encoded.len() > MAX_EXTENSION_FRAME_BYTES {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ToolError::LimitExceeded(
                "extension request frame is too large".into(),
            ));
        }
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&encoded)?;
            stdin.write_all(b"\n")?;
            stdin.flush()?;
        }
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ToolError::CommandFailed("extension stdout unavailable".into()))?;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let reader_join = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut frame = Vec::new();
            let result = reader.read_until(b'\n', &mut frame).map(|_| frame);
            let _ = tx.send(result);
        });
        let frame = match rx.recv_timeout(self.timeout) {
            Ok(result) => result?,
            Err(_) => {
                terminate_extension_process(&mut child);
                let _ = child.wait();
                let _ = reader_join.join();
                return Err(ToolError::CommandTimeout(
                    self.timeout.as_millis().min(u128::from(u64::MAX)) as u64,
                ));
            }
        };
        if frame.len() > MAX_EXTENSION_FRAME_BYTES {
            terminate_extension_process(&mut child);
            let _ = child.wait();
            let _ = reader_join.join();
            return Err(ToolError::LimitExceeded(
                "extension response frame is too large".into(),
            ));
        }
        let status = child.wait()?;
        let _ = reader_join.join();
        terminate_extension_descendants(child.id());
        if !status.success() {
            return Err(ToolError::CommandFailed(format!(
                "extension exited with {status}"
            )));
        }
        let response: Value = serde_json::from_slice(&frame).map_err(ToolError::Json)?;
        if let Some(error) = response.get("error") {
            return Err(ToolError::CommandFailed(format!(
                "extension returned JSON-RPC error: {error}"
            )));
        }
        response
            .get("result")
            .cloned()
            .ok_or_else(|| ToolError::CommandFailed("extension response omitted result".into()))
    }
}

fn terminate_extension_process(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let process_group = i32::try_from(child.id()).unwrap_or(i32::MAX);
        // The child is placed in its own process group immediately before
        // spawn, so signaling the negative PID cannot affect zenpi's group.
        unsafe {
            libc::kill(-process_group, libc::SIGTERM);
        }
        std::thread::sleep(Duration::from_millis(25));
        unsafe {
            libc::kill(-process_group, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}

fn terminate_extension_descendants(process_group: u32) {
    #[cfg(unix)]
    {
        let process_group = i32::try_from(process_group).unwrap_or(i32::MAX);
        // The leader has exited; this cleans any inherited group members.
        unsafe {
            libc::kill(-process_group, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = process_group;
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
