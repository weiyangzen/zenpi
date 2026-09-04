//! Bounded tool contracts and safe, workspace-scoped read-only tools.
//!
//! Tools are deliberately separate from model/provider code. A host may expose
//! the definitions to a model and feed returned calls to [`ToolRegistry`], but
//! every call is validated again locally. Read-only tools are enabled by the
//! default policy; workspace writes and command execution require distinct,
//! explicit policy grants and are guarded by the host approval boundary.

use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use thiserror::Error;

pub const MAX_TOOL_CALL_BYTES: usize = 64 * 1024;
pub const MAX_TOOL_RESULT_BYTES: usize = 1024 * 1024;
pub const MAX_INLINE_TOOL_RESULT_BYTES: usize = 64 * 1024;
pub const TOOL_ARTIFACT_DIRECTORY: &str = ".zenpi/artifacts";
pub const MAX_TOOL_NAME_BYTES: usize = 64;
pub const MAX_TOOL_ID_BYTES: usize = 128;
// Leave room for the path/flags wrapper in the one-megabyte serialized result
// envelope, so every value accepted by the input schema can be returned.
pub const MAX_READ_BYTES: usize = 512 * 1024;
// 128 entries leaves room for long, nested relative paths in the serialized
// result envelope while keeping every schema-accepted call bounded.
pub const MAX_LIST_ENTRIES: usize = 128;
// 100 * 2 KiB line snippets keeps the largest valid search result below the
// one-megabyte result envelope even before JSON overhead is counted.
pub const MAX_SEARCH_MATCHES: usize = 100;
pub const MAX_SEARCH_FILES: usize = 20_000;
pub const MAX_SEARCH_NODES: usize = 50_000;
pub const MAX_SEARCH_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_WRITE_BYTES: usize = 512 * 1024;
pub const MAX_COMMAND_BYTES: usize = 16 * 1024;
pub const MAX_COMMAND_OUTPUT_BYTES: usize = 256 * 1024;
pub const DEFAULT_COMMAND_TIMEOUT_MS: u64 = 30_000;

const DEFAULT_READ_BYTES: usize = 256 * 1024;
const DEFAULT_LIST_ENTRIES: usize = 64;
const DEFAULT_SEARCH_MATCHES: usize = 100;

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(1);

/// The kind of side effect a tool may perform. Policy decisions are based on
/// this value, not on a tool's name or model-supplied arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSideEffect {
    ReadOnly,
    WorkspaceWrite,
    CommandExecution,
}

/// Explicit host policy for side-effecting tools. The secure default permits
/// inspection only. Writes and commands are separate grants so approving a
/// file edit can never implicitly approve a shell command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SideEffectPolicy {
    allow_workspace_writes: bool,
    allow_command_execution: bool,
}

impl SideEffectPolicy {
    pub const fn all_builtins() -> Self {
        Self {
            allow_workspace_writes: true,
            allow_command_execution: true,
        }
    }
}

impl SideEffectPolicy {
    pub const fn read_only() -> Self {
        Self {
            allow_workspace_writes: false,
            allow_command_execution: false,
        }
    }

    pub const fn with_workspace_writes(mut self, allowed: bool) -> Self {
        self.allow_workspace_writes = allowed;
        self
    }

    pub const fn with_command_execution(mut self, allowed: bool) -> Self {
        self.allow_command_execution = allowed;
        self
    }

    pub const fn allows(self, side_effect: ToolSideEffect) -> bool {
        match side_effect {
            ToolSideEffect::ReadOnly => true,
            ToolSideEffect::WorkspaceWrite => self.allow_workspace_writes,
            ToolSideEffect::CommandExecution => self.allow_command_execution,
        }
    }
}

/// Definition sent to a model. `input_schema` is JSON-Schema-shaped discovery
/// metadata; tool implementations remain the authoritative validators.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub side_effect: ToolSideEffect,
}

impl ToolDefinition {
    pub fn validate(&self) -> Result<(), ToolError> {
        validate_identifier(&self.name, "tool name", MAX_TOOL_NAME_BYTES)?;
        if self.description.trim().is_empty() || self.description.len() > 1_024 {
            return Err(ToolError::InvalidDefinition(
                "tool description must be non-empty and at most 1024 bytes".into(),
            ));
        }
        if self.input_schema.get("type").and_then(Value::as_str) != Some("object") {
            return Err(ToolError::InvalidDefinition(
                "tool input schema must describe an object".into(),
            ));
        }
        if serde_json::to_vec(&self.input_schema)
            .map_err(ToolError::Json)?
            .len()
            > MAX_TOOL_CALL_BYTES
        {
            return Err(ToolError::InvalidDefinition(
                "tool input schema is too large".into(),
            ));
        }
        Ok(())
    }
}

/// A model-requested tool invocation. Arguments must be a JSON object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    #[serde(default = "empty_object")]
    pub arguments: Value,
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolErrorCode {
    InvalidDefinition,
    InvalidCall,
    InvalidArguments,
    UnknownTool,
    PolicyDenied,
    PathDenied,
    NotFound,
    NotAFile,
    NotADirectory,
    InvalidUtf8,
    LimitExceeded,
    Io,
    Internal,
    CommandFailed,
    CommandTimeout,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolFailure {
    pub code: ToolErrorCode,
    pub message: String,
}

/// One terminal result for one call. Failures are values so an agent loop can
/// return them to the model without losing correlation or parsing stderr.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolResult {
    Success {
        call_id: String,
        tool: String,
        output: Value,
    },
    Error {
        call_id: String,
        tool: String,
        error: ToolFailure,
    },
}

impl ToolResult {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("invalid tool definition: {0}")]
    InvalidDefinition(String),
    #[error("invalid tool call: {0}")]
    InvalidCall(String),
    #[error("invalid tool arguments: {0}")]
    InvalidArguments(String),
    #[error("unknown tool `{0}`")]
    UnknownTool(String),
    #[error("tool `{tool}` requires denied side effect `{side_effect:?}`")]
    PolicyDenied {
        tool: String,
        side_effect: ToolSideEffect,
    },
    #[error("workspace path is denied: {0}")]
    PathDenied(String),
    #[error("path does not exist: {0}")]
    NotFound(String),
    #[error("path is not a regular file: {0}")]
    NotAFile(String),
    #[error("path is not a directory: {0}")]
    NotADirectory(String),
    #[error("file is not valid UTF-8: {0}")]
    InvalidUtf8(String),
    #[error("tool limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("tool I/O: {0}")]
    Io(#[from] io::Error),
    #[error("tool JSON: {0}")]
    Json(serde_json::Error),
    #[error("command failed: {0}")]
    CommandFailed(String),
    #[error("command timed out after {0} ms")]
    CommandTimeout(u64),
    #[error("tool execution was cancelled")]
    Cancelled,
}

impl ToolError {
    pub const fn code(&self) -> ToolErrorCode {
        match self {
            Self::InvalidDefinition(_) => ToolErrorCode::InvalidDefinition,
            Self::InvalidCall(_) => ToolErrorCode::InvalidCall,
            Self::InvalidArguments(_) => ToolErrorCode::InvalidArguments,
            Self::UnknownTool(_) => ToolErrorCode::UnknownTool,
            Self::PolicyDenied { .. } => ToolErrorCode::PolicyDenied,
            Self::PathDenied(_) => ToolErrorCode::PathDenied,
            Self::NotFound(_) => ToolErrorCode::NotFound,
            Self::NotAFile(_) => ToolErrorCode::NotAFile,
            Self::NotADirectory(_) => ToolErrorCode::NotADirectory,
            Self::InvalidUtf8(_) => ToolErrorCode::InvalidUtf8,
            Self::LimitExceeded(_) => ToolErrorCode::LimitExceeded,
            Self::Io(_) => ToolErrorCode::Io,
            Self::Json(_) => ToolErrorCode::Internal,
            Self::CommandFailed(_) => ToolErrorCode::CommandFailed,
            Self::CommandTimeout(_) => ToolErrorCode::CommandTimeout,
            Self::Cancelled => ToolErrorCode::Cancelled,
        }
    }
}

/// Filesystem context fixed by the host, never supplied by the model.
#[derive(Debug, Clone)]
pub struct ToolContext {
    workspace_root: PathBuf,
}

impl ToolContext {
    pub fn new(workspace_root: impl AsRef<Path>) -> Result<Self, ToolError> {
        let requested = workspace_root.as_ref();
        let root = requested
            .canonicalize()
            .map_err(|error| map_path_io(requested, error))?;
        if !root.is_dir() {
            return Err(ToolError::NotADirectory(display_path(requested)));
        }
        Ok(Self {
            workspace_root: root,
        })
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Resolve a model-independent user attachment inside the fixed workspace.
    /// Symlinks that leave the workspace are rejected by canonicalization.
    pub fn read_attachment(
        &self,
        requested: &str,
        max_bytes: usize,
    ) -> Result<(PathBuf, Vec<u8>), ToolError> {
        let resolved = self.resolve_existing(requested)?;
        if !resolved.canonical.is_file() {
            return Err(ToolError::NotAFile(requested.to_owned()));
        }
        let metadata = fs::metadata(&resolved.canonical)?;
        if metadata.len() > u64::try_from(max_bytes).unwrap_or(u64::MAX) {
            return Err(ToolError::LimitExceeded(format!(
                "attachment exceeds {max_bytes} bytes"
            )));
        }
        let bytes = fs::read(&resolved.canonical)?;
        if bytes.len() > max_bytes {
            return Err(ToolError::LimitExceeded(format!(
                "attachment exceeds {max_bytes} bytes"
            )));
        }
        Ok((resolved.relative, bytes))
    }

    fn resolve_for_write(&self, requested: &str) -> Result<PathBuf, ToolError> {
        validate_relative_path(requested)?;
        let candidate = self.workspace_root.join(requested);
        if candidate.exists() {
            let canonical = candidate
                .canonicalize()
                .map_err(|error| map_path_io(&candidate, error))?;
            if !canonical.starts_with(&self.workspace_root) {
                return Err(ToolError::PathDenied(requested.to_owned()));
            }
            return Ok(canonical);
        }
        let mut existing = candidate
            .parent()
            .ok_or_else(|| ToolError::PathDenied(requested.to_owned()))?;
        while !existing.exists() {
            existing = existing
                .parent()
                .ok_or_else(|| ToolError::PathDenied(requested.to_owned()))?;
        }
        let canonical_existing = existing
            .canonicalize()
            .map_err(|error| map_path_io(existing, error))?;
        if !canonical_existing.starts_with(&self.workspace_root) {
            return Err(ToolError::PathDenied(requested.to_owned()));
        }
        Ok(candidate)
    }

    fn resolve_existing(&self, requested: &str) -> Result<ResolvedPath, ToolError> {
        validate_relative_path(requested)?;
        let candidate = self.workspace_root.join(requested);
        let canonical = candidate
            .canonicalize()
            .map_err(|error| map_path_io(&candidate, error))?;
        if !canonical.starts_with(&self.workspace_root) {
            return Err(ToolError::PathDenied(requested.to_owned()));
        }
        let relative = canonical
            .strip_prefix(&self.workspace_root)
            .map_err(|_| ToolError::PathDenied(requested.to_owned()))?
            .to_path_buf();
        Ok(ResolvedPath {
            canonical,
            relative,
        })
    }
}

struct ResolvedPath {
    canonical: PathBuf,
    relative: PathBuf,
}

pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    fn invoke(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ToolError>;
}

struct RegisteredTool {
    definition: ToolDefinition,
    handler: Box<dyn Tool>,
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, RegisteredTool>,
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ToolRegistry")
            .field("tools", &self.tools.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_read_only_builtins() -> Result<Self, ToolError> {
        let mut registry = Self::new();
        registry.register(ReadFileTool)?;
        registry.register(ListDirectoryTool)?;
        registry.register(SearchTextTool)?;
        Ok(registry)
    }

    pub fn with_all_builtins() -> Result<Self, ToolError> {
        let mut registry = Self::with_read_only_builtins()?;
        registry.register(WriteFileTool)?;
        registry.register(EditFileTool)?;
        registry.register(RunCommandTool)?;
        Ok(registry)
    }

    pub fn register<T>(&mut self, tool: T) -> Result<(), ToolError>
    where
        T: Tool + 'static,
    {
        let definition = tool.definition();
        definition.validate()?;
        if self.tools.contains_key(&definition.name) {
            return Err(ToolError::InvalidDefinition(format!(
                "duplicate tool name `{}`",
                definition.name
            )));
        }
        self.tools.insert(
            definition.name.clone(),
            RegisteredTool {
                definition,
                handler: Box::new(tool),
            },
        );
        Ok(())
    }

    pub fn register_boxed(&mut self, tool: Box<dyn Tool>) -> Result<(), ToolError> {
        let definition = tool.definition();
        definition.validate()?;
        if self.tools.contains_key(&definition.name) {
            return Err(ToolError::InvalidDefinition(format!(
                "duplicate tool name `{}`",
                definition.name
            )));
        }
        self.tools.insert(
            definition.name.clone(),
            RegisteredTool {
                definition,
                handler: tool,
            },
        );
        Ok(())
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|tool| tool.definition.clone())
            .collect()
    }

    pub fn definition(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.get(name).map(|tool| &tool.definition)
    }

    pub fn execute(
        &self,
        context: &ToolContext,
        policy: SideEffectPolicy,
        call: ToolCall,
    ) -> ToolResult {
        let call_id = call.id.clone();
        let tool_name = call.name.clone();
        let outcome = self.execute_inner(context, policy, &call);
        match outcome {
            Ok(output) => ToolResult::Success {
                call_id,
                tool: tool_name,
                output,
            },
            Err(error) => ToolResult::Error {
                call_id,
                tool: tool_name,
                error: ToolFailure {
                    code: error.code(),
                    message: error.to_string(),
                },
            },
        }
    }

    /// Execute a call and move an oversized successful payload into a private,
    /// repository-relative artifact. The model receives a small reference and
    /// preview rather than an unbounded tool response.
    pub fn execute_compact(
        &self,
        context: &ToolContext,
        policy: SideEffectPolicy,
        call: ToolCall,
    ) -> ToolResult {
        let result = self.execute(context, policy, call);
        compact_tool_result(context, result).unwrap_or_else(|error| ToolResult::Error {
            call_id: "artifact".into(),
            tool: "artifact".into(),
            error: ToolFailure {
                code: error.code(),
                message: error.to_string(),
            },
        })
    }

    fn execute_inner(
        &self,
        context: &ToolContext,
        policy: SideEffectPolicy,
        call: &ToolCall,
    ) -> Result<Value, ToolError> {
        validate_call(call)?;
        let registered = self
            .tools
            .get(&call.name)
            .ok_or_else(|| ToolError::UnknownTool(call.name.clone()))?;
        if !policy.allows(registered.definition.side_effect) {
            return Err(ToolError::PolicyDenied {
                tool: call.name.clone(),
                side_effect: registered.definition.side_effect,
            });
        }
        let arguments = call.arguments.as_object().ok_or_else(|| {
            ToolError::InvalidArguments("tool arguments must be a JSON object".into())
        })?;
        let output = registered.handler.invoke(context, arguments)?;
        let output_size = serde_json::to_vec(&output).map_err(ToolError::Json)?.len();
        if output_size > MAX_TOOL_RESULT_BYTES {
            return Err(ToolError::LimitExceeded(format!(
                "serialized result exceeds {MAX_TOOL_RESULT_BYTES} bytes"
            )));
        }
        Ok(output)
    }
}

pub fn compact_tool_result(
    context: &ToolContext,
    result: ToolResult,
) -> Result<ToolResult, ToolError> {
    let ToolResult::Success {
        call_id,
        tool,
        output,
    } = result
    else {
        return Ok(result);
    };
    let encoded = serde_json::to_vec(&output).map_err(ToolError::Json)?;
    if encoded.len() <= MAX_INLINE_TOOL_RESULT_BYTES {
        return Ok(ToolResult::Success {
            call_id,
            tool,
            output,
        });
    }
    use sha2::{Digest, Sha256};
    let digest = format!("{:x}", Sha256::digest(&encoded));
    let relative = format!("{TOOL_ARTIFACT_DIRECTORY}/{digest}.json");
    let path = context.resolve_for_write(&relative)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write_text(&path, &encoded)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    let preview = truncate_utf8(&String::from_utf8_lossy(&encoded), 4096);
    Ok(ToolResult::Success {
        call_id,
        tool,
        output: json!({
            "artifact": relative,
            "sha256": digest,
            "bytes": encoded.len(),
            "preview": preview,
            "compacted": true,
        }),
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ReadFileTool;

impl Tool for ReadFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".into(),
            description: "Read a UTF-8 text file inside the workspace with a byte limit.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "max_bytes": { "type": "integer", "minimum": 1, "maximum": MAX_READ_BYTES }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            side_effect: ToolSideEffect::ReadOnly,
        }
    }

    fn invoke(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ToolError> {
        reject_unknown(arguments, &["path", "max_bytes"])?;
        let requested = required_string(arguments, "path", 4_096)?;
        let max_bytes = bounded_usize(
            arguments,
            "max_bytes",
            DEFAULT_READ_BYTES,
            1,
            MAX_READ_BYTES,
        )?;
        let path = context.resolve_existing(requested)?;
        let metadata =
            fs::metadata(&path.canonical).map_err(|error| map_path_io(&path.canonical, error))?;
        if !metadata.is_file() {
            return Err(ToolError::NotAFile(requested.to_owned()));
        }
        let mut bytes = Vec::with_capacity(max_bytes.min(metadata.len() as usize));
        File::open(&path.canonical)?
            .take(max_bytes.saturating_add(1) as u64)
            .read_to_end(&mut bytes)?;
        let truncated = bytes.len() > max_bytes;
        bytes.truncate(max_bytes);
        let content =
            String::from_utf8(bytes).map_err(|_| ToolError::InvalidUtf8(requested.to_owned()))?;
        Ok(json!({
            "path": relative_display(&path.relative),
            "content": content,
            "truncated": truncated
        }))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ListDirectoryTool;

impl Tool for ListDirectoryTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_directory".into(),
            description: "List one workspace directory without recursively following links.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "default": "." },
                    "max_entries": { "type": "integer", "minimum": 1, "maximum": MAX_LIST_ENTRIES }
                },
                "additionalProperties": false
            }),
            side_effect: ToolSideEffect::ReadOnly,
        }
    }

    fn invoke(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ToolError> {
        reject_unknown(arguments, &["path", "max_entries"])?;
        let requested = optional_string(arguments, "path", ".", 4_096)?;
        let max_entries = bounded_usize(
            arguments,
            "max_entries",
            DEFAULT_LIST_ENTRIES,
            1,
            MAX_LIST_ENTRIES,
        )?;
        let path = context.resolve_existing(requested)?;
        if !path.canonical.is_dir() {
            return Err(ToolError::NotADirectory(requested.to_owned()));
        }
        // Keep only the lexicographically smallest `max_entries` names so a
        // directory containing millions of entries cannot force unbounded
        // memory allocation while still producing deterministic output.
        let mut entries = BTreeMap::<String, Value>::new();
        let mut truncated = false;
        for entry in fs::read_dir(&path.canonical)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            // Do not follow a symlink while describing a directory entry. A
            // link may point outside the workspace even though its own name
            // is safely contained here.
            let metadata = fs::symlink_metadata(entry.path())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = path.relative.join(&name);
            let value = json!({
                "name": name.clone(),
                "path": relative_display(&relative),
                "kind": file_kind(file_type),
                "size": if metadata.is_file() { Some(metadata.len()) } else { None }
            });
            if entries.len() < max_entries {
                entries.insert(name, value);
            } else {
                truncated = true;
                let replace = entries
                    .keys()
                    .next_back()
                    .is_some_and(|largest| name.as_str() < largest.as_str());
                if replace {
                    entries.pop_last();
                    entries.insert(name, value);
                }
            }
        }
        let entries: Vec<Value> = entries.into_values().collect();
        Ok(json!({
            "path": relative_display(&path.relative),
            "entries": entries,
            "truncated": truncated
        }))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SearchTextTool;

impl Tool for SearchTextTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "search_text".into(),
            description: "Search workspace UTF-8 files for literal text with bounded traversal."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "path": { "type": "string", "default": "." },
                    "case_sensitive": { "type": "boolean", "default": true },
                    "max_matches": { "type": "integer", "minimum": 1, "maximum": MAX_SEARCH_MATCHES }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            side_effect: ToolSideEffect::ReadOnly,
        }
    }

    fn invoke(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ToolError> {
        reject_unknown(
            arguments,
            &["query", "path", "case_sensitive", "max_matches"],
        )?;
        let query = required_string(arguments, "query", 4_096)?;
        let requested = optional_string(arguments, "path", ".", 4_096)?;
        let case_sensitive = optional_bool(arguments, "case_sensitive", true)?;
        let max_matches = bounded_usize(
            arguments,
            "max_matches",
            DEFAULT_SEARCH_MATCHES,
            1,
            MAX_SEARCH_MATCHES,
        )?;
        let path = context.resolve_existing(requested)?;
        let needle = if case_sensitive {
            query.to_owned()
        } else {
            query.to_lowercase()
        };
        let mut queue = VecDeque::from([path.canonical]);
        let mut nodes_visited = 0_usize;
        let mut files_visited = 0_usize;
        let mut matches = Vec::new();
        let mut match_limit_reached = false;
        let mut file_limit_reached = false;
        let mut node_limit_reached = false;

        while let Some(current) = queue.pop_front() {
            if nodes_visited >= MAX_SEARCH_NODES {
                node_limit_reached = true;
                break;
            }
            nodes_visited += 1;
            if current.is_dir() {
                let remaining = MAX_SEARCH_NODES.saturating_sub(nodes_visited + queue.len());
                let mut children = Vec::new();
                for child in fs::read_dir(&current)? {
                    if children.len() >= remaining {
                        node_limit_reached = true;
                        break;
                    }
                    children.push(child?);
                }
                children.sort_by_key(|entry| entry.file_name());
                for child in children {
                    let file_type = child.file_type()?;
                    if file_type.is_symlink() || ignored_directory(&child.path(), file_type) {
                        continue;
                    }
                    queue.push_back(child.path());
                }
                if node_limit_reached {
                    break;
                }
                continue;
            }
            if !current.is_file() {
                continue;
            }
            if files_visited >= MAX_SEARCH_FILES {
                file_limit_reached = true;
                break;
            }
            files_visited += 1;
            let metadata = fs::metadata(&current)?;
            if metadata.len() > MAX_SEARCH_FILE_BYTES as u64 {
                continue;
            }
            let bytes = fs::read(&current)?;
            if bytes.contains(&0) {
                continue;
            }
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue;
            };
            for (index, line) in text.lines().enumerate() {
                let haystack = if case_sensitive {
                    line.to_owned()
                } else {
                    line.to_lowercase()
                };
                if haystack.contains(&needle) {
                    let relative = current
                        .strip_prefix(context.workspace_root())
                        .map_err(|_| ToolError::PathDenied(display_path(&current)))?;
                    matches.push(json!({
                        "path": relative_display(relative),
                        "line": index + 1,
                        "text": truncate_line(line, 2_048)
                    }));
                    if matches.len() >= max_matches {
                        match_limit_reached = true;
                        break;
                    }
                }
            }
            if match_limit_reached {
                break;
            }
        }

        Ok(json!({
            "query": query,
            "matches": matches,
            "nodes_visited": nodes_visited,
            "files_visited": files_visited,
            "truncated": match_limit_reached || file_limit_reached || node_limit_reached
        }))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WriteFileTool;

impl WriteFileTool {
    pub fn preview(
        context: &ToolContext,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ToolError> {
        reject_unknown(arguments, &["path", "content"])?;
        let requested = required_string(arguments, "path", 4_096)?;
        let content = string_argument(arguments, "content", MAX_WRITE_BYTES, true)?;
        let path = context.resolve_for_write(requested)?;
        let before = fs::read_to_string(&path).unwrap_or_default();
        Ok(json!({
            "path": relative_display(path.strip_prefix(context.workspace_root()).map_err(|_| ToolError::PathDenied(requested.to_owned()))?),
            "before_bytes": before.len(),
            "after_bytes": content.len(),
            "changed": before != content,
        }))
    }
}

impl Tool for WriteFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write_file".into(),
            description: "Atomically write UTF-8 text inside the workspace.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string", "maxLength": MAX_WRITE_BYTES }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
            side_effect: ToolSideEffect::WorkspaceWrite,
        }
    }

    fn invoke(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ToolError> {
        reject_unknown(arguments, &["path", "content"])?;
        let requested = required_string(arguments, "path", 4_096)?;
        let content = string_argument(arguments, "content", MAX_WRITE_BYTES, true)?;
        if content.as_bytes().contains(&0) {
            return Err(ToolError::InvalidArguments("content contains NUL".into()));
        }
        let path = context.resolve_for_write(requested)?;
        let created = !path.exists();
        atomic_write_text(&path, content.as_bytes())?;
        Ok(json!({
            "path": relative_display(path.strip_prefix(context.workspace_root()).map_err(|_| ToolError::PathDenied(requested.to_owned()))?),
            "bytes": content.len(),
            "created": created
        }))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EditFileTool;

impl Tool for EditFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "edit_file".into(),
            description: "Replace one exact UTF-8 text occurrence atomically.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old": { "type": "string", "maxLength": MAX_WRITE_BYTES },
                    "new": { "type": "string", "maxLength": MAX_WRITE_BYTES }
                },
                "required": ["path", "old", "new"],
                "additionalProperties": false
            }),
            side_effect: ToolSideEffect::WorkspaceWrite,
        }
    }

    fn invoke(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ToolError> {
        reject_unknown(arguments, &["path", "old", "new"])?;
        let requested = required_string(arguments, "path", 4_096)?;
        let old = required_string(arguments, "old", MAX_WRITE_BYTES)?;
        let new = string_argument(arguments, "new", MAX_WRITE_BYTES, true)?;
        if old.is_empty() {
            return Err(ToolError::InvalidArguments("old must be non-empty".into()));
        }
        let path = context.resolve_existing(requested)?.canonical;
        let original = fs::read_to_string(&path).map_err(|error| map_path_io(&path, error))?;
        let occurrences = original.matches(old).count();
        if occurrences != 1 {
            return Err(ToolError::InvalidArguments(format!(
                "old text must occur exactly once (found {occurrences})"
            )));
        }
        let edited = original.replacen(old, new, 1);
        if edited.len() > MAX_WRITE_BYTES {
            return Err(ToolError::LimitExceeded(format!(
                "edited content exceeds {MAX_WRITE_BYTES} bytes"
            )));
        }
        atomic_write_text(&path, edited.as_bytes())?;
        Ok(json!({
            "path": relative_display(path.strip_prefix(context.workspace_root()).map_err(|_| ToolError::PathDenied(requested.to_owned()))?),
            "replacements": 1,
            "bytes": edited.len()
        }))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RunCommandTool;

impl RunCommandTool {
    /// Execute a command while polling a host cancellation predicate. This is
    /// used by the agent loop so a cancelled turn also reaps its child.
    pub fn invoke_with_cancel(
        context: &ToolContext,
        arguments: &Map<String, Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        reject_unknown(arguments, &["command", "timeout_ms"])?;
        let command = required_string(arguments, "command", MAX_COMMAND_BYTES)?;
        let timeout_ms = bounded_usize(
            arguments,
            "timeout_ms",
            DEFAULT_COMMAND_TIMEOUT_MS as usize,
            1,
            120_000,
        )? as u64;
        let mut builder = Command::new(if cfg!(windows) { "cmd" } else { "sh" });
        if cfg!(windows) {
            builder.args(["/C", command]);
        } else {
            builder.args(["-c", command]);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Create a fresh process group so timeout/cancellation can reap
            // descendants launched by the shell, not just the shell itself.
            builder.process_group(0);
        }
        let builder = builder.current_dir(context.workspace_root());
        builder.env_clear();
        for (key, value) in crate::security::child_environment() {
            builder.env(key, value);
        }
        let mut child = builder
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(ToolError::Io)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ToolError::CommandFailed("command stdout pipe unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ToolError::CommandFailed("command stderr pipe unavailable".into()))?;
        let stdout_thread =
            std::thread::spawn(move || read_limited_stream(stdout, MAX_COMMAND_OUTPUT_BYTES));
        let stderr_thread =
            std::thread::spawn(move || read_limited_stream(stderr, MAX_COMMAND_OUTPUT_BYTES));
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            if cancelled() {
                terminate_process_tree(&mut child);
                let _ = child.wait();
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(ToolError::Cancelled);
            }
            if let Some(status) = child.try_wait().map_err(ToolError::Io)? {
                let stdout = stdout_thread
                    .join()
                    .map_err(|_| ToolError::CommandFailed("stdout reader panicked".into()))??;
                let stderr = stderr_thread
                    .join()
                    .map_err(|_| ToolError::CommandFailed("stderr reader panicked".into()))??;
                let stdout = String::from_utf8_lossy(&stdout).into_owned();
                let stderr = String::from_utf8_lossy(&stderr).into_owned();
                if !status.success() {
                    return Err(ToolError::CommandFailed(format!(
                        "exit={status}; stderr={}",
                        truncate_utf8(&stderr, MAX_COMMAND_OUTPUT_BYTES)
                    )));
                }
                return Ok(json!({
                    "status": "ok",
                    "exit_code": status.code(),
                    "stdout": truncate_utf8(&stdout, MAX_COMMAND_OUTPUT_BYTES),
                    "stderr": truncate_utf8(&stderr, MAX_COMMAND_OUTPUT_BYTES)
                }));
            }
            if Instant::now() >= deadline {
                terminate_process_tree(&mut child);
                let _ = child.wait();
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(ToolError::CommandTimeout(timeout_ms));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn terminate_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        if let Ok(pid) = i32::try_from(child.id()) {
            // Negative PID addresses the process group created above.
            // SAFETY: `pid` is the live child ID returned by `std::process`;
            // the child was placed in its own process group before spawning.
            // `kill` borrows no Rust memory and errors are intentionally
            // tolerated because the process may have exited concurrently.
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
            std::thread::sleep(Duration::from_millis(50));
            if child.try_wait().ok().flatten().is_none() {
                // SAFETY: same process-group invariant as the SIGTERM call.
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}

impl Tool for RunCommandTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "run_command".into(),
            description: "Run a bounded shell command in the workspace.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "maxLength": MAX_COMMAND_BYTES },
                    "timeout_ms": { "type": "integer", "minimum": 1, "maximum": 120000 }
                },
                "required": ["command"],
                "additionalProperties": false
            }),
            side_effect: ToolSideEffect::CommandExecution,
        }
    }

    fn invoke(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ToolError> {
        Self::invoke_with_cancel(context, arguments, &|| false)
    }
}

fn atomic_write_text(path: &Path, bytes: &[u8]) -> Result<(), ToolError> {
    let parent = path
        .parent()
        .ok_or_else(|| ToolError::PathDenied(path.display().to_string()))?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ToolError::PathDenied(path.display().to_string()))?;
    let temporary = parent.join(format!(
        ".{name}.zenpi-{}-{}",
        std::process::id(),
        NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(&temporary).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(ToolError::Io(error));
    }
    drop(file);
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        ToolError::Io(error)
    })
}

fn truncate_utf8(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[output truncated]", &text[..end])
}

fn read_limited_stream<R: Read>(mut reader: R, max: usize) -> Result<Vec<u8>, ToolError> {
    let mut bytes = Vec::with_capacity(max.min(64 * 1024));
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = max.saturating_sub(bytes.len());
        if remaining > 0 {
            bytes.extend_from_slice(&buffer[..count.min(remaining)]);
        }
        if count > remaining {
            truncated = true;
        }
    }
    if truncated {
        bytes.extend_from_slice(b"\n[output truncated]");
    }
    Ok(bytes)
}

fn validate_call(call: &ToolCall) -> Result<(), ToolError> {
    validate_identifier(&call.id, "tool call id", MAX_TOOL_ID_BYTES)
        .map_err(|error| ToolError::InvalidCall(error.to_string()))?;
    validate_identifier(&call.name, "tool name", MAX_TOOL_NAME_BYTES)
        .map_err(|error| ToolError::InvalidCall(error.to_string()))?;
    if !call.arguments.is_object() {
        return Err(ToolError::InvalidArguments(
            "tool arguments must be a JSON object".into(),
        ));
    }
    let serialized = serde_json::to_vec(call).map_err(ToolError::Json)?;
    if serialized.len() > MAX_TOOL_CALL_BYTES {
        return Err(ToolError::InvalidCall(format!(
            "serialized call exceeds {MAX_TOOL_CALL_BYTES} bytes"
        )));
    }
    Ok(())
}

fn validate_identifier(value: &str, field: &str, max: usize) -> Result<(), ToolError> {
    if value.is_empty() || value.len() > max {
        return Err(ToolError::InvalidDefinition(format!(
            "{field} must be non-empty and at most {max} bytes"
        )));
    }
    if value
        .chars()
        .any(|character| !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-')))
    {
        return Err(ToolError::InvalidDefinition(format!(
            "{field} contains invalid characters"
        )));
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<(), ToolError> {
    if path.is_empty() || path.len() > 4_096 || path.contains(['\0', '\r', '\n']) {
        return Err(ToolError::PathDenied(path.to_owned()));
    }
    let parsed = Path::new(path);
    if parsed.is_absolute()
        || parsed.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(ToolError::PathDenied(path.to_owned()));
    }
    Ok(())
}

fn required_string<'a>(
    arguments: &'a Map<String, Value>,
    field: &'static str,
    max: usize,
) -> Result<&'a str, ToolError> {
    let value = arguments
        .get(field)
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing `{field}`")))?
        .as_str()
        .ok_or_else(|| ToolError::InvalidArguments(format!("`{field}` must be a string")))?;
    if value.is_empty() || value.len() > max || value.contains('\0') {
        return Err(ToolError::InvalidArguments(format!(
            "`{field}` must be non-empty and at most {max} bytes"
        )));
    }
    Ok(value)
}

fn optional_string<'a>(
    arguments: &'a Map<String, Value>,
    field: &'static str,
    default: &'a str,
    max: usize,
) -> Result<&'a str, ToolError> {
    match arguments.get(field) {
        Some(_) => required_string(arguments, field, max),
        None => Ok(default),
    }
}

fn string_argument<'a>(
    arguments: &'a Map<String, Value>,
    field: &'static str,
    max: usize,
    allow_empty: bool,
) -> Result<&'a str, ToolError> {
    let value = arguments
        .get(field)
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing `{field}`")))?
        .as_str()
        .ok_or_else(|| ToolError::InvalidArguments(format!("`{field}` must be a string")))?;
    if (!allow_empty && value.is_empty()) || value.len() > max || value.contains('\0') {
        return Err(ToolError::InvalidArguments(format!(
            "`{field}` must be {}and at most {max} bytes",
            if allow_empty { "" } else { "non-empty " }
        )));
    }
    Ok(value)
}

fn optional_bool(
    arguments: &Map<String, Value>,
    field: &'static str,
    default: bool,
) -> Result<bool, ToolError> {
    arguments.get(field).map_or(Ok(default), |value| {
        value
            .as_bool()
            .ok_or_else(|| ToolError::InvalidArguments(format!("`{field}` must be a boolean")))
    })
}

fn bounded_usize(
    arguments: &Map<String, Value>,
    field: &'static str,
    default: usize,
    minimum: usize,
    maximum: usize,
) -> Result<usize, ToolError> {
    let Some(value) = arguments.get(field) else {
        return Ok(default);
    };
    let value = value.as_u64().ok_or_else(|| {
        ToolError::InvalidArguments(format!("`{field}` must be a positive integer"))
    })?;
    let value = usize::try_from(value).map_err(|_| {
        ToolError::InvalidArguments(format!("`{field}` is outside the supported range"))
    })?;
    if !(minimum..=maximum).contains(&value) {
        return Err(ToolError::InvalidArguments(format!(
            "`{field}` must be between {minimum} and {maximum}"
        )));
    }
    Ok(value)
}

fn reject_unknown(arguments: &Map<String, Value>, allowed: &[&str]) -> Result<(), ToolError> {
    if let Some(field) = arguments
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(ToolError::InvalidArguments(format!(
            "unknown argument `{field}`"
        )));
    }
    Ok(())
}

fn map_path_io(path: &Path, error: io::Error) -> ToolError {
    if error.kind() == io::ErrorKind::NotFound {
        ToolError::NotFound(display_path(path))
    } else {
        ToolError::Io(error)
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn relative_display(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        ".".into()
    } else {
        path.to_string_lossy().replace('\\', "/")
    }
}

fn file_kind(file_type: fs::FileType) -> &'static str {
    if file_type.is_file() {
        "file"
    } else if file_type.is_dir() {
        "directory"
    } else if file_type.is_symlink() {
        "symlink"
    } else {
        "other"
    }
}

fn ignored_directory(path: &Path, file_type: fs::FileType) -> bool {
    if !file_type.is_dir() {
        return false;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, ".git" | "target" | "node_modules" | ".venv"))
}

fn truncate_line(line: &str, max_bytes: usize) -> &str {
    if line.len() <= max_bytes {
        return line;
    }
    let mut boundary = max_bytes;
    while !line.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &line[..boundary]
}
