//! Bounded tool contracts and safe, workspace-scoped read-only tools.
//!
//! Tools are deliberately separate from model/provider code. A host may expose
//! the definitions to a model and feed returned calls to [`ToolRegistry`], but
//! every call is validated again locally. Read-only tools are enabled by the
//! default policy; workspace writes and command execution require distinct,
//! explicit policy grants and are guarded by the host approval boundary.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
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
/// Maximum number of bytes returned in a write/edit unified diff.  The file
/// contents accepted by the tools are larger than this, so a diff can never
/// turn a small tool call into an unbounded model response.
pub const MAX_DIFF_BYTES: usize = 64 * 1024;
pub const MAX_COMMAND_BYTES: usize = 16 * 1024;
pub const MAX_COMMAND_OUTPUT_BYTES: usize = 256 * 1024;
pub const DEFAULT_COMMAND_TIMEOUT_MS: u64 = 30_000;

const DEFAULT_READ_BYTES: usize = 256 * 1024;
const DEFAULT_LIST_ENTRIES: usize = 64;
const DEFAULT_SEARCH_MATCHES: usize = 100;
const DIFF_CONTEXT_LINES: usize = 3;
const DIFF_TRUNCATION_MARKER: &str = "[diff truncated]";

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

/// Set by the host route, never parsed from model-supplied tool arguments.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOrigin {
    #[default]
    AgentTool,
    UserShell,
    BlueprintWorker,
}

/// Mutable input to preflight. The compiled gate retains a private snapshot.
/// An allow list cannot override the built-in credential/prohibition rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlueprintPolicySpec {
    pub blueprint_digest: String,
    pub goal_digest: String,
    pub item_id: String,
    pub allowed_tools: BTreeSet<String>,
    pub readable_paths: BTreeSet<String>,
    pub writable_paths: BTreeSet<String>,
    pub denied_paths: BTreeSet<String>,
    pub protected_paths: BTreeSet<String>,
    pub allowed_commands: BTreeSet<String>,
    pub denied_commands: BTreeSet<String>,
    pub network_hosts: BTreeSet<String>,
    pub max_actions: u64,
    pub max_command_timeout_ms: u64,
    pub max_command_output_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlueprintLease {
    pub lease_id: String,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyEvidence {
    pub origin: ToolOrigin,
    pub policy_digest: String,
    pub blueprint_digest: String,
    pub goal_digest: String,
    pub item_id: String,
    pub lease_id: String,
    pub expires_at_ms: u64,
    pub prohibition_gate_enforced: bool,
}

#[derive(Debug, Clone)]
pub struct BlueprintGate {
    policy: BlueprintPolicySpec,
    lease: BlueprintLease,
    workspace_root: PathBuf,
    digest: String,
    revoked: Arc<AtomicBool>,
    actions: Arc<AtomicU64>,
}

/// Host-owned cancellation capability. Workers receive only the compiled gate.
#[derive(Debug, Clone)]
pub struct BlueprintRevocation(Arc<AtomicBool>);

impl BlueprintRevocation {
    pub fn revoke(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl BlueprintGate {
    pub fn compile(
        context: &ToolContext,
        policy: BlueprintPolicySpec,
        lease: BlueprintLease,
        now_ms: u64,
    ) -> Result<(Self, BlueprintRevocation), ToolError> {
        let invalid = |reason: &str| ToolError::InvalidDefinition(reason.into());
        for digest in [&policy.blueprint_digest, &policy.goal_digest] {
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(invalid(
                    "Blueprint and Goal digests must be lowercase SHA-256",
                ));
            }
        }
        validate_identifier(&policy.item_id, "Blueprint item", MAX_TOOL_ID_BYTES)?;
        validate_identifier(&lease.lease_id, "Blueprint lease", MAX_TOOL_ID_BYTES)?;
        if lease.issued_at_ms > now_ms
            || lease.expires_at_ms <= now_ms
            || lease.expires_at_ms.saturating_sub(lease.issued_at_ms) > 86_400_000
        {
            return Err(invalid(
                "Blueprint lease must be current and at most 24 hours",
            ));
        }
        if policy.max_actions == 0
            || policy.max_actions > 1_000_000
            || !(1..=120_000).contains(&policy.max_command_timeout_ms)
            || !(1..=MAX_COMMAND_OUTPUT_BYTES).contains(&policy.max_command_output_bytes)
        {
            return Err(invalid(
                "Blueprint budgets are missing or exceed host limits",
            ));
        }
        if policy.allowed_tools.is_empty()
            || policy
                .allowed_tools
                .iter()
                .any(|name| builtin_effect(name).is_none())
        {
            return Err(invalid("Blueprint policy contains unknown tool effects"));
        }
        for paths in [
            &policy.readable_paths,
            &policy.writable_paths,
            &policy.denied_paths,
            &policy.protected_paths,
        ] {
            if paths.len() > 256 {
                return Err(invalid("Blueprint path list exceeds 256 entries"));
            }
            for path in paths {
                validate_relative_path(path)?;
                if path.len() > 4096 || normalized_relative(path) != *path {
                    return Err(invalid(
                        "Blueprint paths must be bounded workspace-relative paths",
                    ));
                }
            }
        }
        // No supported builtin can enforce arbitrary network access. Refuse a
        // grant rather than treating a declared hostname as a network sandbox.
        if !policy.network_hosts.is_empty() {
            return Err(invalid(
                "network grants require an unavailable confined executor",
            ));
        }
        if policy.allowed_commands.len() > 128 || policy.denied_commands.len() > 128 {
            return Err(invalid("Blueprint command list exceeds 128 entries"));
        }
        for command in &policy.allowed_commands {
            confined_command(command)
                .map_err(|_| invalid("worker command requires an unavailable confined executor"))?;
        }
        let encoded = serde_json::to_vec(&(&policy, &lease, context.workspace_root()))
            .map_err(ToolError::Json)?;
        if encoded.len() > MAX_TOOL_CALL_BYTES {
            return Err(invalid("Blueprint policy exceeds 64 KiB"));
        }
        let mut digest = Sha256::new();
        digest.update(b"zenpi-blueprint-prohibition-v1\0");
        digest.update(encoded);
        let revoked = Arc::new(AtomicBool::new(false));
        let revocation = BlueprintRevocation(Arc::clone(&revoked));
        let gate = Self {
            policy,
            lease,
            workspace_root: context.workspace_root().to_owned(),
            digest: format!("{:x}", digest.finalize()),
            revoked,
            actions: Arc::new(AtomicU64::new(0)),
        };
        Ok((gate, revocation))
    }

    pub fn evidence(&self) -> PolicyEvidence {
        PolicyEvidence {
            origin: ToolOrigin::BlueprintWorker,
            policy_digest: self.digest.clone(),
            blueprint_digest: self.policy.blueprint_digest.clone(),
            goal_digest: self.policy.goal_digest.clone(),
            item_id: self.policy.item_id.clone(),
            lease_id: self.lease.lease_id.clone(),
            expires_at_ms: self.lease.expires_at_ms,
            prohibition_gate_enforced: true,
        }
    }

    fn deny(&self, reason: &str) -> ToolError {
        ToolError::GateDenied {
            reason: reason.into(),
            policy_digest: self.digest.clone(),
            lease_id: self.lease.lease_id.clone(),
        }
    }

    fn check_live(&self) -> Result<(), ToolError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| self.deny("clock_before_epoch"))?
            .as_millis();
        if self.revoked.load(Ordering::SeqCst) {
            return Err(self.deny("lease_revoked"));
        }
        if now < u128::from(self.lease.issued_at_ms) || now >= u128::from(self.lease.expires_at_ms)
        {
            return Err(self.deny("lease_expired_or_clock_rollback"));
        }
        Ok(())
    }

    fn check_path(&self, requested: &str, write: bool) -> Result<(), ToolError> {
        self.check_live()?;
        validate_relative_path(requested)?;
        let normalized = normalized_relative(requested);
        let path = Path::new(&normalized);
        let matches = |paths: &BTreeSet<String>| {
            paths
                .iter()
                .any(|entry| entry == "." || path.starts_with(entry))
        };
        let denied_matches = |paths: &BTreeSet<String>| {
            let lower = normalized.to_ascii_lowercase();
            paths.iter().any(|entry| {
                entry == "." || Path::new(&lower).starts_with(entry.to_ascii_lowercase())
            })
        };
        if credential_path(path)
            || denied_matches(&self.policy.denied_paths)
            || (write
                && (denied_matches(&self.policy.protected_paths)
                    || path.components().any(|part| {
                        part.as_os_str()
                            .to_string_lossy()
                            .to_ascii_lowercase()
                            .contains("blueprint")
                    })))
        {
            return Err(self.deny("prohibited_path"));
        }
        let allowed = if write {
            &self.policy.writable_paths
        } else {
            &self.policy.readable_paths
        };
        if !matches(allowed) {
            return Err(self.deny("undeclared_path"));
        }
        // Worker file tools do not accept aliases to another ownership scope.
        // Refuse links in every existing component, including dangling links.
        let mut current = self.workspace_root.clone();
        for component in path.components() {
            current.push(component);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(self.deny("symlink_path"));
                }
                Ok(metadata) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        if metadata.is_file() && metadata.nlink() > 1 {
                            return Err(self.deny("hardlinked_file"));
                        }
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                Err(error) => return Err(ToolError::Io(error)),
            }
        }
        Ok(())
    }

    fn check_call(
        &self,
        name: &str,
        effect: ToolSideEffect,
        arguments: &Map<String, Value>,
    ) -> Result<(), ToolError> {
        self.check_live()?;
        if builtin_effect(name) != Some(effect) || !self.policy.allowed_tools.contains(name) {
            return Err(self.deny("unknown_or_undeclared_tool_effect"));
        }
        match effect {
            ToolSideEffect::ReadOnly | ToolSideEffect::WorkspaceWrite => {
                let path = optional_string(arguments, "path", ".", 4096)?;
                self.check_path(path, effect == ToolSideEffect::WorkspaceWrite)?;
            }
            ToolSideEffect::CommandExecution => {
                let command = required_string(arguments, "command", MAX_COMMAND_BYTES)?;
                if self.policy.denied_commands.contains(command) {
                    return Err(self.deny("prohibited_command"));
                }
                if !self.policy.allowed_commands.contains(command) {
                    return Err(self.deny("undeclared_command"));
                }
                confined_command(command).map_err(|_| self.deny("unknown_command_effect"))?;
                let timeout = arguments
                    .get("timeout_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(self.policy.max_command_timeout_ms);
                if timeout > self.policy.max_command_timeout_ms {
                    return Err(self.deny("command_timeout_budget"));
                }
            }
        }
        Ok(())
    }
}

fn builtin_effect(name: &str) -> Option<ToolSideEffect> {
    match name {
        "read_file" | "list_directory" | "search_text" => Some(ToolSideEffect::ReadOnly),
        "write_file" | "edit_file" => Some(ToolSideEffect::WorkspaceWrite),
        "run_command" => Some(ToolSideEffect::CommandExecution),
        _ => None,
    }
}

fn credential_path(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy().to_ascii_lowercase();
        matches!(
            name.as_str(),
            ".git"
                | ".zenpi"
                | ".ssh"
                | ".aws"
                | ".azure"
                | ".config"
                | ".codex"
                | ".claude"
                | ".netrc"
                | ".npmrc"
                | "credentials"
                | "credentials.json"
                | "auth.json"
                | "id_rsa"
                | "id_ed25519"
        ) || name == ".env"
            || name.starts_with(".env.")
            || name.ends_with(".pem")
            || name.ends_with(".key")
            || name.ends_with(".p12")
    })
}

fn normalized_relative(path: &str) -> String {
    let normalized: PathBuf = Path::new(path)
        .components()
        .filter(|component| *component != Component::CurDir)
        .collect();
    relative_display(&normalized)
}

/// Closed, effect-free argv profiles, not a shell-language denylist. Shells,
/// interpreters, scripts and general-purpose executables require OS confinement
/// which is deliberately not inferred from an operator-supplied command string.
fn confined_command(command: &str) -> Result<(&'static str, Vec<&str>), ToolError> {
    let invalid = || ToolError::InvalidArguments("unsupported confined command".into());
    if command.is_empty()
        || command.len() > MAX_COMMAND_BYTES
        || !command
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" _-.,:/".contains(&b))
    {
        return Err(invalid());
    }
    let mut words = command.split_ascii_whitespace();
    let name = words.next().ok_or_else(invalid)?;
    let args: Vec<_> = words.collect();
    match name {
        "echo" => Ok(("/bin/echo", args)),
        "true" if args.is_empty() => Ok(("/usr/bin/true", args)),
        "false" if args.is_empty() => Ok(("/usr/bin/false", args)),
        _ => Err(invalid()),
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
    StalePreview,
}

/// A bounded, renderer-neutral preview attached to an approval request before
/// a workspace mutation can run. The raw model arguments remain available for
/// audit, while this value gives a human the actual proposed file change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolPreview {
    Diff {
        path: String,
        patch: String,
        changed: bool,
        truncated: bool,
        before_bytes: usize,
        after_bytes: usize,
        source_sha256: String,
    },
}

impl ToolPreview {
    pub fn validate(&self) -> Result<(), ToolError> {
        match self {
            Self::Diff {
                path,
                patch,
                before_bytes: _,
                after_bytes,
                source_sha256,
                ..
            } => {
                validate_relative_path(path)?;
                if path.len() > 4_096 {
                    return Err(ToolError::LimitExceeded(
                        "preview path exceeds 4096 bytes".into(),
                    ));
                }
                if patch.len() > MAX_DIFF_BYTES {
                    return Err(ToolError::LimitExceeded(format!(
                        "preview diff exceeds {MAX_DIFF_BYTES} bytes"
                    )));
                }
                // Write previews may replace an existing oversized file from a
                // bounded prefix; `truncated` makes that fact explicit. The
                // proposed post-write content is always strictly bounded.
                if *after_bytes > MAX_WRITE_BYTES {
                    return Err(ToolError::LimitExceeded(format!(
                        "preview output size exceeds {MAX_WRITE_BYTES} bytes"
                    )));
                }
                if source_sha256.len() != 64
                    || !source_sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                {
                    return Err(ToolError::InvalidArguments(
                        "preview source digest is invalid".into(),
                    ));
                }
                Ok(())
            }
        }
    }

    pub fn display_text(&self) -> &str {
        match self {
            Self::Diff { patch, .. } => patch,
        }
    }
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
    #[error("Blueprint gate denied: {reason}; policy={policy_digest}; lease={lease_id}")]
    GateDenied {
        reason: String,
        policy_digest: String,
        lease_id: String,
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
    #[error("workspace changed after the tool preview was approved")]
    StalePreview,
}

impl ToolError {
    pub const fn code(&self) -> ToolErrorCode {
        match self {
            Self::InvalidDefinition(_) => ToolErrorCode::InvalidDefinition,
            Self::InvalidCall(_) => ToolErrorCode::InvalidCall,
            Self::InvalidArguments(_) => ToolErrorCode::InvalidArguments,
            Self::UnknownTool(_) => ToolErrorCode::UnknownTool,
            Self::PolicyDenied { .. } | Self::GateDenied { .. } => ToolErrorCode::PolicyDenied,
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
            Self::StalePreview => ToolErrorCode::StalePreview,
        }
    }
}

/// Filesystem context fixed by the host, never supplied by the model.
#[derive(Debug, Clone)]
pub struct ToolContext {
    workspace_root: PathBuf,
    origin: ToolOrigin,
    blueprint_gate: Option<Arc<BlueprintGate>>,
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
            origin: ToolOrigin::AgentTool,
            blueprint_gate: None,
        })
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn origin(&self) -> ToolOrigin {
        self.origin
    }

    /// Changing origin never removes a gate or transfers a worker grant to the
    /// user's shell. A mismatched origin fails at the next action boundary.
    pub fn with_origin(mut self, origin: ToolOrigin) -> Self {
        self.origin = origin;
        self
    }

    pub fn with_blueprint_gate(mut self, gate: BlueprintGate) -> Result<Self, ToolError> {
        if self.blueprint_gate.is_some() || gate.workspace_root != self.workspace_root {
            return Err(gate.deny("gate_replacement_or_workspace_mismatch"));
        }
        gate.check_live()?;
        self.origin = ToolOrigin::BlueprintWorker;
        self.blueprint_gate = Some(Arc::new(gate));
        Ok(self)
    }

    pub fn policy_evidence(&self) -> Option<PolicyEvidence> {
        self.blueprint_gate.as_ref().map(|gate| gate.evidence())
    }

    fn checked_gate(&self) -> Result<Option<&BlueprintGate>, ToolError> {
        match (self.origin, self.blueprint_gate.as_deref()) {
            (ToolOrigin::BlueprintWorker, Some(gate)) => {
                gate.check_live()?;
                Ok(Some(gate))
            }
            (_, Some(gate)) => Err(gate.deny("worker_grant_origin_mismatch")),
            (ToolOrigin::BlueprintWorker, None) => Err(ToolError::GateDenied {
                reason: "worker_preflight_required".into(),
                policy_digest: "unbound".into(),
                lease_id: "unbound".into(),
            }),
            (_, None) => Ok(None),
        }
    }

    /// Preview/approval callers use the same prohibition check as execution.
    /// This does not consume an action or grant an approval decision.
    pub fn check_call_gate(
        &self,
        name: &str,
        effect: ToolSideEffect,
        arguments: &Map<String, Value>,
    ) -> Result<Option<PolicyEvidence>, ToolError> {
        let Some(gate) = self.checked_gate()? else {
            return Ok(None);
        };
        gate.check_call(name, effect, arguments)?;
        Ok(Some(gate.evidence()))
    }

    fn admit_builtin(&self, name: &str, arguments: &Map<String, Value>) -> Result<(), ToolError> {
        let effect = builtin_effect(name).ok_or_else(|| ToolError::UnknownTool(name.into()))?;
        self.check_call_gate(name, effect, arguments)?;
        if let Some(gate) = self.blueprint_gate.as_deref()
            && gate
                .actions
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                    (count < gate.policy.max_actions).then_some(count.saturating_add(1))
                })
                .is_err()
        {
            return Err(gate.deny("action_budget_exhausted"));
        }
        Ok(())
    }

    fn check_path_gate(&self, requested: &str, write: bool) -> Result<(), ToolError> {
        if let Some(gate) = self.checked_gate()? {
            gate.check_path(requested, write)?;
        }
        Ok(())
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
        self.check_path_gate(requested, true)?;
        validate_relative_path(requested)?;
        let candidate = self.workspace_root.join(requested);
        if candidate.exists() {
            self.check_path_gate(requested, false)?;
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
        self.check_path_gate(requested, false)?;
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
    builtin: bool,
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
        registry.register_builtin(ReadFileTool)?;
        registry.register_builtin(ListDirectoryTool)?;
        registry.register_builtin(SearchTextTool)?;
        Ok(registry)
    }

    pub fn with_all_builtins() -> Result<Self, ToolError> {
        let mut registry = Self::with_read_only_builtins()?;
        registry.register_builtin(WriteFileTool)?;
        registry.register_builtin(EditFileTool)?;
        registry.register_builtin(RunCommandTool)?;
        Ok(registry)
    }

    fn register_builtin<T: Tool + 'static>(&mut self, tool: T) -> Result<(), ToolError> {
        let name = tool.definition().name;
        self.register(tool)?;
        self.tools
            .get_mut(&name)
            .expect("just registered builtin")
            .builtin = true;
        Ok(())
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
                builtin: false,
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
                builtin: false,
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

    /// Build the human-facing preview for a built-in mutating call without
    /// executing it. Extension tools do not receive an invented preview: they
    /// must add an explicit preview contract before a host can display one.
    pub fn approval_preview(
        &self,
        context: &ToolContext,
        call: &ToolCall,
    ) -> Result<Option<ToolPreview>, ToolError> {
        validate_call(call)?;
        let registered = self
            .tools
            .get(&call.name)
            .ok_or_else(|| ToolError::UnknownTool(call.name.clone()))?;
        let arguments = call.arguments.as_object().ok_or_else(|| {
            ToolError::InvalidArguments("tool arguments must be a JSON object".into())
        })?;
        context.check_call_gate(&call.name, registered.definition.side_effect, arguments)?;
        if registered.definition.side_effect != ToolSideEffect::WorkspaceWrite {
            return Ok(None);
        }
        let arguments = call.arguments.as_object().ok_or_else(|| {
            ToolError::InvalidArguments("tool arguments must be a JSON object".into())
        })?;
        let preview = match call.name.as_str() {
            "write_file" => Some(WriteFileTool::approval_preview(context, arguments)?),
            "edit_file" => Some(EditFileTool::approval_preview(context, arguments)?),
            _ => None,
        };
        if let Some(preview) = &preview {
            preview.validate()?;
        }
        Ok(preview)
    }

    /// Execute only if a mutating call still has the exact source snapshot the
    /// user approved. Revalidation closes the wait-between-preview-and-write
    /// race without giving read-only or command tools a fabricated diff.
    pub fn execute_approved(
        &self,
        context: &ToolContext,
        policy: SideEffectPolicy,
        call: ToolCall,
        approved_preview: Option<&ToolPreview>,
    ) -> ToolResult {
        match (approved_preview, self.approval_preview(context, &call)) {
            (Some(expected), Ok(Some(actual))) if expected == &actual => {
                self.execute_compact(context, policy, call)
            }
            (Some(_), Ok(Some(_))) | (Some(_), Ok(None)) | (None, Ok(Some(_))) => {
                ToolResult::Error {
                    call_id: call.id,
                    tool: call.name,
                    error: ToolFailure {
                        code: ToolErrorCode::StalePreview,
                        message: "workspace changed after approval; review the new diff and retry"
                            .into(),
                    },
                }
            }
            (_, Err(error)) => ToolResult::Error {
                call_id: call.id,
                tool: call.name,
                error: ToolFailure {
                    code: error.code(),
                    message: crate::security::redact_text(
                        &format!("approved preview revalidation failed: {error}"),
                        &[],
                    ),
                },
            },
            (None, Ok(None)) => self.execute_compact(context, policy, call),
        }
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
                    message: crate::security::redact_text(&error.to_string(), &[]),
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
        context.check_call_gate(&call.name, registered.definition.side_effect, arguments)?;
        if let Some(gate) = context.checked_gate()?
            && !registered.builtin
        {
            return Err(gate.deny("untrusted_tool_implementation"));
        }
        let mut output = registered.handler.invoke(context, arguments)?;
        // Tool output may be persisted in a session, handoff, or compact
        // artifact. Redact active host-owned credential handles before it can
        // cross that boundary, even when a third-party tool returns a secret
        // under an unrecognised JSON key.
        output = crate::security::redact_json(&output, &[]);
        if let (Some(evidence), Some(object)) = (context.policy_evidence(), output.as_object_mut())
        {
            object.insert(
                "policy_evidence".into(),
                serde_json::to_value(evidence).map_err(ToolError::Json)?,
            );
        }
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
    if context.checked_gate()?.is_some() {
        return Ok(ToolResult::Success {
            call_id,
            tool,
            output: json!({
                "preview": truncate_utf8(&String::from_utf8_lossy(&encoded), 4096),
                "bytes": encoded.len(), "truncated": true, "compacted": false,
                "artifact": null, "reason": "worker_artifact_write_not_granted",
                "policy_evidence": context.policy_evidence(),
            }),
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
        context.admit_builtin("read_file", arguments)?;
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
        context.admit_builtin("list_directory", arguments)?;
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
            if let Some(gate) = context.checked_gate()?
                && gate
                    .check_path(&relative_display(&relative), false)
                    .is_err()
            {
                continue;
            }
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
        context.admit_builtin("search_text", arguments)?;
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
            if let Some(gate) = context.checked_gate()? {
                let relative = current
                    .strip_prefix(context.workspace_root())
                    .map_err(|_| gate.deny("search_path_escape"))?;
                if gate.check_path(&relative_display(relative), false).is_err() {
                    continue;
                }
            }
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
    fn approval_preview(
        context: &ToolContext,
        arguments: &Map<String, Value>,
    ) -> Result<ToolPreview, ToolError> {
        reject_unknown(arguments, &["path", "content"])?;
        let requested = required_string(arguments, "path", 4_096)?;
        let content = string_argument(arguments, "content", MAX_WRITE_BYTES, true)?;
        if content.as_bytes().contains(&0) {
            return Err(ToolError::InvalidArguments("content contains NUL".into()));
        }
        let path = context.resolve_for_write(requested)?;
        let (before, source_truncated) = read_diff_source(&path)?;
        let relative = relative_path_for_output(context, &path, requested)?;
        let (patch, truncated) = render_unified_diff(&relative, &before, content, source_truncated);
        let before_bytes = file_byte_len(&path).unwrap_or(before.len());
        let source_sha256 = source_digest(&path)?;
        Ok(ToolPreview::Diff {
            path: relative,
            before_bytes,
            after_bytes: content.len(),
            source_sha256,
            changed: before != content || source_truncated,
            patch,
            truncated,
        })
    }

    pub fn preview(
        context: &ToolContext,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ToolError> {
        let preview = Self::approval_preview(context, arguments)?;
        let ToolPreview::Diff {
            path,
            patch,
            changed,
            truncated,
            before_bytes,
            after_bytes,
            source_sha256: _,
        } = preview;
        Ok(json!({
            "path": path,
            "before_bytes": before_bytes,
            "after_bytes": after_bytes,
            "changed": changed,
            "diff": patch,
            "diff_truncated": truncated,
        }))
    }
}

impl Tool for WriteFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write_file".into(),
            description:
                "Atomically write UTF-8 text inside the workspace and return a bounded unified diff."
                    .into(),
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
        context.admit_builtin("write_file", arguments)?;
        reject_unknown(arguments, &["path", "content"])?;
        let requested = required_string(arguments, "path", 4_096)?;
        let content = string_argument(arguments, "content", MAX_WRITE_BYTES, true)?;
        if content.as_bytes().contains(&0) {
            return Err(ToolError::InvalidArguments("content contains NUL".into()));
        }
        let path = context.resolve_for_write(requested)?;
        let created = !path.exists();
        let (before, source_truncated) = read_diff_source(&path)?;
        let relative = relative_path_for_output(context, &path, requested)?;
        let (diff, diff_truncated) =
            render_unified_diff(&relative, &before, content, source_truncated);
        context.check_path_gate(requested, true)?;
        atomic_write_text(&path, content.as_bytes())?;
        Ok(json!({
            "path": relative,
            "bytes": content.len(),
            "created": created,
            "diff": diff,
            "diff_truncated": diff_truncated,
        }))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EditFileTool;

impl EditFileTool {
    fn approval_preview(
        context: &ToolContext,
        arguments: &Map<String, Value>,
    ) -> Result<ToolPreview, ToolError> {
        reject_unknown(arguments, &["path", "old", "new"])?;
        let requested = required_string(arguments, "path", 4_096)?;
        let old = required_string(arguments, "old", MAX_WRITE_BYTES)?;
        let new = string_argument(arguments, "new", MAX_WRITE_BYTES, true)?;
        if old.is_empty() {
            return Err(ToolError::InvalidArguments("old must be non-empty".into()));
        }
        let path = context.resolve_existing(requested)?.canonical;
        let original = read_bounded_text(&path, MAX_WRITE_BYTES)?;
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
        let relative = relative_path_for_output(context, &path, requested)?;
        let (patch, truncated) = render_unified_diff(&relative, &original, &edited, false);
        let source_sha256 = source_digest(&path)?;
        Ok(ToolPreview::Diff {
            path: relative,
            patch,
            changed: original != edited,
            truncated,
            before_bytes: original.len(),
            after_bytes: edited.len(),
            source_sha256,
        })
    }
}

impl Tool for EditFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "edit_file".into(),
            description:
                "Replace one exact UTF-8 text occurrence atomically and return a bounded unified diff."
                    .into(),
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
        context.admit_builtin("edit_file", arguments)?;
        reject_unknown(arguments, &["path", "old", "new"])?;
        let requested = required_string(arguments, "path", 4_096)?;
        let old = required_string(arguments, "old", MAX_WRITE_BYTES)?;
        let new = string_argument(arguments, "new", MAX_WRITE_BYTES, true)?;
        if old.is_empty() {
            return Err(ToolError::InvalidArguments("old must be non-empty".into()));
        }
        let path = context.resolve_existing(requested)?.canonical;
        // An edit needs the complete source to prove that `old` occurs exactly
        // once, but that does not justify an unbounded `read_to_string`.
        // Read at most one byte past the write budget so oversized sources are
        // rejected before they can consume unbounded memory.
        let original = read_bounded_text(&path, MAX_WRITE_BYTES)?;
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
        let relative = relative_path_for_output(context, &path, requested)?;
        let (diff, diff_truncated) = render_unified_diff(&relative, &original, &edited, false);
        context.check_path_gate(requested, true)?;
        atomic_write_text(&path, edited.as_bytes())?;
        Ok(json!({
            "path": relative,
            "replacements": 1,
            "bytes": edited.len(),
            "diff": diff,
            "diff_truncated": diff_truncated,
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
        let outcome = Self::run_supervised(context, arguments, cancelled)?;
        if outcome.cancelled {
            return Err(ToolError::Cancelled);
        }
        if outcome.timed_out {
            return Err(ToolError::CommandTimeout(outcome.timeout_ms));
        }
        if outcome.exit_code != Some(0) {
            return Err(ToolError::CommandFailed(format!(
                "exit={:?}; signal={:?}; stderr={}",
                outcome.exit_code, outcome.signal, outcome.stderr
            )));
        }
        serde_json::to_value(outcome).map_err(ToolError::Json)
    }

    /// A user-shell owner can retain nonzero exit, signal and cancellation
    /// evidence instead of converting it into a model-tool error. Host approval
    /// is still required before calling this API; an origin is not a grant.
    pub fn invoke_user_shell_with_cancel(
        context: &ToolContext,
        arguments: &Map<String, Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        if context.origin() != ToolOrigin::UserShell {
            return Err(ToolError::InvalidArguments(
                "user-shell origin required".into(),
            ));
        }
        let outcome = Self::run_supervised(context, arguments, cancelled)?;
        serde_json::to_value(outcome).map_err(ToolError::Json)
    }

    #[cfg(not(unix))]
    fn run_supervised(
        _context: &ToolContext,
        _arguments: &Map<String, Value>,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<CommandOutcome, ToolError> {
        Err(ToolError::InvalidArguments(
            "command containment is unavailable on this platform".into(),
        ))
    }

    #[cfg(unix)]
    fn run_supervised(
        context: &ToolContext,
        arguments: &Map<String, Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<CommandOutcome, ToolError> {
        use std::os::unix::process::{CommandExt, ExitStatusExt};

        context.admit_builtin("run_command", arguments)?;
        reject_unknown(arguments, &["command", "timeout_ms"])?;
        let command = required_string(arguments, "command", MAX_COMMAND_BYTES)?;
        if command.as_bytes().contains(&0) {
            return Err(ToolError::InvalidArguments("command contains NUL".into()));
        }
        let gate = context.checked_gate()?;
        let default_timeout = gate.map_or(DEFAULT_COMMAND_TIMEOUT_MS, |gate| {
            gate.policy.max_command_timeout_ms
        });
        let timeout_ms = bounded_usize(
            arguments,
            "timeout_ms",
            default_timeout as usize,
            1,
            120_000,
        )? as u64;
        let output_cap = gate.map_or(MAX_COMMAND_OUTPUT_BYTES, |gate| {
            gate.policy.max_command_output_bytes
        });
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
        let mut builder = if gate.is_some() {
            let (program, argv) = confined_command(command)?;
            let mut builder = Command::new(program);
            builder.args(argv);
            builder
        } else {
            let mut builder = Command::new("/bin/sh");
            builder.args(["-c", command]);
            builder
        };
        builder.process_group(0);
        let builder = builder.current_dir(context.workspace_root());
        builder.env_clear();
        for (key, value) in crate::security::child_environment() {
            builder.env(key, value);
        }
        let child = builder
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(ToolError::Io)?;
        let mut child = SupervisedChild {
            child,
            cleaned: false,
        };
        let stdout =
            child.child.stdout.take().ok_or_else(|| {
                ToolError::CommandFailed("command stdout pipe unavailable".into())
            })?;
        let stderr =
            child.child.stderr.take().ok_or_else(|| {
                ToolError::CommandFailed("command stderr pipe unavailable".into())
            })?;
        make_pipe_nonblocking(&stdout)?;
        make_pipe_nonblocking(&stderr)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stdout_stop = Arc::clone(&stop);
        let stderr_stop = Arc::clone(&stop);
        let stdout_thread =
            std::thread::spawn(move || read_limited_stream(stdout, output_cap, &stdout_stop));
        let stderr_thread =
            std::thread::spawn(move || read_limited_stream(stderr, output_cap, &stderr_stop));
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let mut was_cancelled = false;
        let mut timed_out = false;
        let status = loop {
            if cancelled() || gate.is_some_and(|gate| gate.check_live().is_err()) {
                was_cancelled = true;
                break Ok(None);
            }
            match child.child.try_wait() {
                Ok(Some(status)) => break Ok(Some(status)),
                Ok(None) => {}
                Err(error) => break Err(ToolError::Io(error)),
            }
            if Instant::now() >= deadline {
                timed_out = true;
                break Ok(None);
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        // A command's descendants do not outlive its operation, even if the
        // shell exits successfully or closes its own stdout before they do.
        child.cleanup();
        stop.store(true, Ordering::SeqCst);
        let stdout = stdout_thread
            .join()
            .map_err(|_| ToolError::CommandFailed("stdout reader panicked".into()))?;
        let stderr = stderr_thread
            .join()
            .map_err(|_| ToolError::CommandFailed("stderr reader panicked".into()))?;
        let (stdout, stdout_truncated) = stdout?;
        let (stderr, stderr_truncated) = stderr?;
        let status = status?;
        Ok(CommandOutcome {
            status: if was_cancelled {
                "cancelled"
            } else if timed_out {
                "timed_out"
            } else if status.is_some_and(|s| s.success()) {
                "ok"
            } else {
                "failed"
            }
            .into(),
            origin: context.origin(),
            exit_code: status.and_then(|status| status.code()),
            signal: status.and_then(|status| status.signal()),
            stdout: truncate_utf8(&String::from_utf8_lossy(&stdout), output_cap),
            stderr: truncate_utf8(&String::from_utf8_lossy(&stderr), output_cap),
            stdout_truncated,
            stderr_truncated,
            timed_out,
            cancelled: was_cancelled,
            timeout_ms,
            child_reaped: true,
            process_group_terminated: true,
            policy_evidence: context.policy_evidence(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandOutcome {
    pub status: String,
    pub origin: ToolOrigin,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub timed_out: bool,
    pub cancelled: bool,
    pub timeout_ms: u64,
    pub child_reaped: bool,
    pub process_group_terminated: bool,
    pub policy_evidence: Option<PolicyEvidence>,
}

struct SupervisedChild {
    child: std::process::Child,
    cleaned: bool,
}

impl SupervisedChild {
    fn cleanup(&mut self) {
        if !self.cleaned {
            terminate_process_tree(&mut self.child);
            let _ = self.child.wait();
            self.cleaned = true;
        }
    }
}

impl Drop for SupervisedChild {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Return a workspace-relative path for a tool response. Write targets may
/// not exist yet, so this intentionally uses the lexical path returned by
/// `resolve_for_write` rather than requiring a canonicalized file.
fn relative_path_for_output(
    context: &ToolContext,
    path: &Path,
    requested: &str,
) -> Result<String, ToolError> {
    path.strip_prefix(context.workspace_root())
        .map(relative_display)
        .map_err(|_| ToolError::PathDenied(requested.to_owned()))
}

/// Read only a bounded prefix of a file for a diff. A write is allowed to
/// replace an existing file larger than `MAX_WRITE_BYTES`; reading that file
/// in full merely to render a preview would make the tool's memory use
/// unbounded. The boolean reports that the source was clipped.
fn read_diff_source(path: &Path) -> Result<(String, bool), ToolError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            return Err(ToolError::NotAFile(display_path(path)));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((String::new(), false));
        }
        Err(error) => return Err(ToolError::Io(error)),
    }
    let file = File::open(path)?;
    let mut bytes = Vec::with_capacity(MAX_WRITE_BYTES.min(64 * 1024));
    file.take(MAX_WRITE_BYTES.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() > MAX_WRITE_BYTES;
    bytes.truncate(MAX_WRITE_BYTES);
    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}

fn source_digest(path: &Path) -> Result<String, ToolError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut digest = Sha256::new();
            digest.update(b"zenpi-source-absent-v1\0");
            return Ok(format!("{:x}", digest.finalize()));
        }
        Err(error) => return Err(ToolError::Io(error)),
        Ok(metadata) if !metadata.is_file() => {
            return Err(ToolError::NotAFile(display_path(path)));
        }
        Ok(_) => {}
    }
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Read a UTF-8 file while enforcing a hard byte bound.  Unlike
/// [`read_diff_source`], this helper rejects a clipped source because an exact
/// edit cannot safely determine occurrence count from a prefix.
fn read_bounded_text(path: &Path, max_bytes: usize) -> Result<String, ToolError> {
    let file = File::open(path).map_err(|error| map_path_io(path, error))?;
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    file.take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| map_path_io(path, error))?;
    if bytes.len() > max_bytes {
        return Err(ToolError::LimitExceeded(format!(
            "source file exceeds {max_bytes} bytes"
        )));
    }
    String::from_utf8(bytes).map_err(|_| ToolError::InvalidUtf8(display_path(path)))
}

fn file_byte_len(path: &Path) -> Option<usize> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| usize::try_from(metadata.len()).ok())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffText {
    lines: Vec<String>,
    final_newline: bool,
}

fn split_diff_text(text: &str) -> DiffText {
    let final_newline = text.ends_with('\n');
    let mut lines: Vec<String> = if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
            .collect()
    };
    if final_newline {
        // `split` produces an extra empty item after the terminating newline;
        // the newline itself is represented by `final_newline` instead.
        lines.pop();
    }
    DiffText {
        lines,
        final_newline,
    }
}

fn diff_range_start(index: usize, count: usize) -> usize {
    if count == 0 { index } else { index + 1 }
}

/// Render a compact, standard unified diff. The implementation deliberately
/// uses one coarse hunk (common prefix/suffix plus three context lines) rather
/// than an O(n^2) LCS algorithm: tool inputs are model-controlled and must
/// remain cheap even when a file contains many short lines.
fn render_unified_diff(
    path: &str,
    before: &str,
    after: &str,
    source_truncated: bool,
) -> (String, bool) {
    let old = split_diff_text(before);
    let new = split_diff_text(after);
    let mut prefix = 0_usize;
    while prefix < old.lines.len()
        && prefix < new.lines.len()
        && old.lines[prefix] == new.lines[prefix]
    {
        prefix += 1;
    }
    let mut suffix = 0_usize;
    while suffix < old.lines.len().saturating_sub(prefix)
        && suffix < new.lines.len().saturating_sub(prefix)
        && old.lines[old.lines.len() - suffix - 1] == new.lines[new.lines.len() - suffix - 1]
    {
        suffix += 1;
    }
    // A final-newline-only edit still needs a real +/- hunk; emitting only a
    // marker after a context line would not give a patch consumer anything to
    // apply. Treat the final line as changed in that case.
    if old.lines == new.lines && old.final_newline != new.final_newline && !old.lines.is_empty() {
        prefix = old.lines.len() - 1;
        suffix = 0;
    }
    let content_unchanged = prefix == old.lines.len().min(new.lines.len())
        && old.lines.len() == new.lines.len()
        && old.final_newline == new.final_newline;
    if content_unchanged && !source_truncated {
        return (String::new(), false);
    }

    let old_change_end = old.lines.len().saturating_sub(suffix);
    let new_change_end = new.lines.len().saturating_sub(suffix);
    let context_before = prefix.min(DIFF_CONTEXT_LINES);
    let context_after = suffix.min(DIFF_CONTEXT_LINES);
    let old_start = prefix.saturating_sub(context_before);
    let new_start = prefix.saturating_sub(context_before);
    let old_end = (old_change_end + context_after).min(old.lines.len());
    let new_end = (new_change_end + context_after).min(new.lines.len());
    let old_count = old_end.saturating_sub(old_start);
    let new_count = new_end.saturating_sub(new_start);

    let mut rendered = String::new();
    rendered.push_str("--- ");
    rendered.push_str(path);
    rendered.push('\n');
    rendered.push_str("+++ ");
    rendered.push_str(path);
    rendered.push('\n');
    rendered.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        diff_range_start(old_start, old_count),
        old_count,
        diff_range_start(new_start, new_count),
        new_count
    ));

    for line in &old.lines[old_start..prefix] {
        rendered.push(' ');
        rendered.push_str(line);
        rendered.push('\n');
    }
    for line in &old.lines[prefix..old_change_end] {
        rendered.push('-');
        rendered.push_str(line);
        rendered.push('\n');
    }
    if !source_truncated && !old.final_newline {
        rendered.push_str("\\ No newline at end of file\n");
    }
    for line in &new.lines[prefix..new_change_end] {
        rendered.push('+');
        rendered.push_str(line);
        rendered.push('\n');
    }
    if !source_truncated && !new.final_newline {
        rendered.push_str("\\ No newline at end of file\n");
    }
    for line in &old.lines[old_change_end..old_end] {
        rendered.push(' ');
        rendered.push_str(line);
        rendered.push('\n');
    }
    if source_truncated {
        rendered.push_str(DIFF_TRUNCATION_MARKER);
        rendered.push('\n');
    }
    let (diff, output_truncated) = bound_diff(rendered);
    (diff, source_truncated || output_truncated)
}

fn bound_diff(mut diff: String) -> (String, bool) {
    if diff.len() <= MAX_DIFF_BYTES {
        return (diff, false);
    }
    let marker = format!("\n{DIFF_TRUNCATION_MARKER}\n");
    let max_content = MAX_DIFF_BYTES.saturating_sub(marker.len());
    let mut end = max_content.min(diff.len());
    while end > 0 && !diff.is_char_boundary(end) {
        end -= 1;
    }
    diff.truncate(end);
    diff.push_str(&marker);
    (diff, true)
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
            // The leader may have died while a descendant ignored SIGTERM.
            // SAFETY: same process-group invariant as the SIGTERM call.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
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
    let marker = "\n[output truncated]";
    let mut end = max.saturating_sub(marker.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if max < marker.len() {
        text.chars()
            .scan(0, |bytes, character| {
                *bytes += character.len_utf8();
                (*bytes <= max).then_some(character)
            })
            .collect()
    } else {
        format!("{}{marker}", &text[..end])
    }
}

#[cfg(unix)]
fn make_pipe_nonblocking(pipe: &impl std::os::fd::AsRawFd) -> Result<(), ToolError> {
    let fd = pipe.as_raw_fd();
    // SAFETY: the borrowed pipe owns an open descriptor for both fcntl calls.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(ToolError::Io(io::Error::last_os_error()));
    }
    Ok(())
}

fn read_limited_stream<R: Read>(
    mut reader: R,
    max: usize,
    stop: &AtomicBool,
) -> Result<(Vec<u8>, bool), ToolError> {
    let mut bytes = Vec::with_capacity(max.min(64 * 1024));
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    loop {
        if stop.load(Ordering::SeqCst) {
            truncated = true;
            break;
        }
        let count = match reader.read(&mut buffer) {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(ToolError::Io(error)),
        };
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
    Ok((bytes, truncated))
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
