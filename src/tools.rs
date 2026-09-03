//! Bounded tool contracts and safe, workspace-scoped read-only tools.
//!
//! Tools are deliberately separate from model/provider code. A host may expose
//! the definitions to a model and feed returned calls to [`ToolRegistry`], but
//! every call is validated again locally. Read-only tools are enabled by the
//! default policy; workspace writes and command execution require distinct,
//! explicit policy grants and have no built-in implementation here.

use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use thiserror::Error;

pub const MAX_TOOL_CALL_BYTES: usize = 64 * 1024;
pub const MAX_TOOL_RESULT_BYTES: usize = 1024 * 1024;
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

const DEFAULT_READ_BYTES: usize = 256 * 1024;
const DEFAULT_LIST_ENTRIES: usize = 64;
const DEFAULT_SEARCH_MATCHES: usize = 100;

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
