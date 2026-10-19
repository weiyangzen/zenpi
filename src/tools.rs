//! Bounded tool contracts and safe, workspace-scoped read-only tools.
//!
//! Tools are deliberately separate from model/provider code. A host may expose
//! the definitions to a model and feed returned calls to [`ToolRegistry`], but
//! every call is validated again locally. Read-only tools are enabled by the
//! default policy; workspace writes and command execution require distinct,
//! explicit policy grants and are guarded by the host approval boundary.

use std::{
    borrow::Cow,
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

/// Maximum serialized size of one tool call (id, name and JSON arguments).
/// A full `MAX_WRITE_BYTES` write plus its path and JSON escaping overhead
/// must fit, otherwise large `write_file` calls could never succeed.
pub const MAX_TOOL_CALL_BYTES: usize = MAX_WRITE_BYTES + 64 * 1024;
/// Maximum serialized size of a tool input schema or Blueprint policy.
pub const MAX_TOOL_DEFINITION_BYTES: usize = 64 * 1024;
pub const MAX_TOOL_RESULT_BYTES: usize = 1024 * 1024;
pub const MAX_INLINE_TOOL_RESULT_BYTES: usize = 64 * 1024;
pub const TOOL_ARTIFACT_DIRECTORY: &str = ".zenpi/artifacts";
pub const MAX_TOOL_NAME_BYTES: usize = 64;
pub const MAX_TOOL_ID_BYTES: usize = 128;
// Leave room for the path/flags wrapper in the one-megabyte serialized result
// envelope, so every value accepted by the input schema can be returned.
pub const MAX_READ_BYTES: usize = 512 * 1024;
/// pi's read page: at most 2000 lines and 50 KiB unless `max_bytes` says otherwise.
pub const MAX_READ_LINES: usize = 2000;
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
/// Upper bound for an explicit model-requested `timeout_ms` (24 h); there is no default.
pub const MAX_COMMAND_TIMEOUT_MS: u64 = 86_400_000;

const DEFAULT_READ_BYTES: usize = 50 * 1024;
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
        if encoded.len() > MAX_TOOL_DEFINITION_BYTES {
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
        "read_file" | "read_tool_output" | "list_directory" | "search_text" | "find" => {
            Some(ToolSideEffect::ReadOnly)
        }
        "write_file" | "edit_file" => Some(ToolSideEffect::WorkspaceWrite),
        "run_command" => Some(ToolSideEffect::CommandExecution),
        _ => None,
    }
}

pub(crate) fn private_output_path(path: &Path) -> bool {
    path.components().any(|part| {
        part.as_os_str()
            .to_string_lossy()
            .to_ascii_lowercase()
            .starts_with(".zenpi-output-")
    })
}

pub(crate) const CREDENTIAL_NAMES: &[&str] = &[
    ".git",
    ".zenpi",
    ".ssh",
    ".aws",
    ".azure",
    ".config",
    ".codex",
    ".claude",
    ".netrc",
    ".npmrc",
    "credentials",
    "credentials.json",
    "auth.json",
    "id_rsa",
    "id_ed25519",
    ".env",
];
pub(crate) const CREDENTIAL_GLOBS: &[&str] = &[".env.*", "*.pem", "*.key", "*.p12"];

pub(crate) fn credential_path(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy().to_ascii_lowercase();
        CREDENTIAL_NAMES.contains(&name.as_str())
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
            > MAX_TOOL_DEFINITION_BYTES
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
    Unsupported,
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
        /// Typed content the model should receive for this result. When it is
        /// present it is the authority and `output` is only a compatibility
        /// record that must not be sent a second time. A tool that produces
        /// JSON alone leaves it absent, and nothing changes for it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<Vec<crate::backend::InputContentPart>>,
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
    #[error("unsupported tool capability: {0}")]
    Unsupported(String),
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
            Self::Unsupported(_) => ToolErrorCode::Unsupported,
        }
    }
}

/// Filesystem context fixed by the host, never supplied by the model.
#[derive(Debug, Clone)]
pub struct ToolContext {
    workspace_root: PathBuf,
    origin: ToolOrigin,
    blueprint_gate: Option<Arc<BlueprintGate>>,
    search_backend: crate::search::SearchBackend,
    output_capture: Option<Arc<crate::tool_output::CommandOutputCapture>>,
    output_read: Option<Arc<crate::tool_output::OutputReadAccess>>,
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
            search_backend: crate::search::SearchBackend::discover(),
            output_capture: None,
            output_read: None,
        })
    }

    pub fn with_search_backend(mut self, backend: crate::search::SearchBackend) -> Self {
        self.search_backend = backend;
        self
    }

    /// Attach a host-owned, single-call raw capture after its output budget
    /// has been granted. Current worker gates have no raw artifact grant.
    pub fn with_output_capture(
        mut self,
        capture: crate::tool_output::CommandOutputCapture,
    ) -> Result<Self, ToolError> {
        if self.output_capture.is_some() {
            return Err(ToolError::InvalidArguments(
                "output capture cannot be replaced".into(),
            ));
        }
        if let Some(gate) = self.checked_gate()? {
            return Err(gate.deny("worker_raw_artifact_write_not_granted"));
        }
        self.output_capture = Some(Arc::new(capture));
        Ok(self)
    }

    pub(crate) fn with_output_read(
        mut self,
        access: crate::tool_output::OutputReadAccess,
    ) -> Result<Self, ToolError> {
        if self.output_read.is_some() {
            return Err(ToolError::InvalidCall(
                "output read capability cannot be replaced".into(),
            ));
        }
        if let Some(gate) = self.checked_gate()? {
            return Err(gate.deny("worker_output_read_not_granted"));
        }
        self.output_read = Some(Arc::new(access));
        Ok(self)
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Rebind ordinary host tools without transferring a worker gate or active raw capture.
    pub(crate) fn for_workspace(&self, root: &Path) -> Result<Self, ToolError> {
        if self.blueprint_gate.is_some()
            || self.origin == ToolOrigin::BlueprintWorker
            || self.output_capture.is_some()
        {
            return Err(ToolError::InvalidCall(
                "tool workspace cannot change under a worker grant or output capture".into(),
            ));
        }
        let canonical = ToolContext::new(root)?.workspace_root;
        if canonical != root {
            return Err(ToolError::InvalidCall(
                "execution workspace is no longer canonical".into(),
            ));
        }
        let mut next = self.clone();
        next.workspace_root = canonical;
        Ok(next)
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
        if self.output_capture.is_some() {
            return Err(gate.deny("worker_raw_artifact_write_not_granted"));
        }
        if self.output_read.is_some() {
            return Err(gate.deny("worker_output_read_not_granted"));
        }
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

    pub(crate) fn checked_policy_evidence(&self) -> Result<Option<PolicyEvidence>, ToolError> {
        self.checked_gate()
            .map(|gate| gate.map(BlueprintGate::evidence))
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
        if let Some(gate) = self.blueprint_gate.as_deref() {
            // Claim one action within the budget (a compare-exchange loop:
            // `fetch_update` is deprecated and `try_update` is above MSRV).
            let mut count = gate.actions.load(Ordering::SeqCst);
            loop {
                if count >= gate.policy.max_actions {
                    return Err(gate.deny("action_budget_exhausted"));
                }
                match gate.actions.compare_exchange(
                    count,
                    count.saturating_add(1),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) => break,
                    Err(actual) => count = actual,
                }
            }
        }
        Ok(())
    }

    fn check_path_gate(&self, requested: &str, write: bool) -> Result<(), ToolError> {
        let requested = self.workspace_path(requested)?;
        self.check_relative_path_gate(&requested, write)
    }

    fn check_relative_path_gate(&self, relative: &str, write: bool) -> Result<(), ToolError> {
        if let Some(gate) = self.checked_gate()? {
            gate.check_path(relative, write)?;
        }
        Ok(())
    }

    /// Map a model-supplied path to a workspace-relative one (pi path rules).
    /// A leading `@` is dropped, `~` and `~/x` expand to the home directory,
    /// and an absolute path is accepted only when it names something inside
    /// the workspace, comparing canonical prefixes so symlinked spellings of
    /// the root (for example `/var` vs `/private/var`) still match. The
    /// result goes through the usual relative checks, so `..`, symlink
    /// escapes and private output paths stay rejected.
    fn workspace_path<'a>(&self, requested: &'a str) -> Result<Cow<'a, str>, ToolError> {
        let denied = || ToolError::PathDenied(requested.to_owned());
        let path = requested.strip_prefix('@').unwrap_or(requested);
        let absolute = if path == "~" || path.starts_with("~/") {
            let home = std::env::var_os("HOME")
                .filter(|home| Path::new(home).is_absolute())
                .ok_or_else(denied)?;
            Path::new(&home).join(path[1..].trim_start_matches('/'))
        } else if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            return Ok(Cow::Borrowed(path));
        };
        if path.contains(['\0', '\r', '\n'])
            || absolute
                .components()
                .any(|component| component == Component::ParentDir)
        {
            return Err(denied());
        }
        let relative = match absolute.strip_prefix(&self.workspace_root) {
            Ok(relative) => relative.to_path_buf(),
            Err(_) => {
                // Canonicalize the deepest existing ancestor; components
                // below it do not exist yet and cannot be symlinks.
                let existing = absolute
                    .ancestors()
                    .find(|ancestor| ancestor.exists())
                    .ok_or_else(denied)?;
                let canonical = existing.canonicalize().map_err(|_| denied())?;
                let base = canonical
                    .strip_prefix(&self.workspace_root)
                    .map_err(|_| denied())?;
                let rest = absolute.strip_prefix(existing).map_err(|_| denied())?;
                base.join(rest)
            }
        };
        let relative = relative.to_str().ok_or_else(denied)?;
        Ok(Cow::Owned(if relative.is_empty() {
            ".".to_owned()
        } else {
            relative.to_owned()
        }))
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
        let file = File::open(&resolved.canonical)?;
        if crate::tool_output::private_output_inode(&file)? {
            return Err(ToolError::PathDenied(requested.into()));
        }
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > max_bytes {
            return Err(ToolError::LimitExceeded(format!(
                "attachment exceeds {max_bytes} bytes"
            )));
        }
        Ok((resolved.relative, bytes))
    }

    fn resolve_for_write(&self, requested: &str) -> Result<PathBuf, ToolError> {
        let requested = &*self.workspace_path(requested)?;
        self.check_relative_path_gate(requested, true)?;
        validate_relative_path(requested)?;
        if private_output_path(Path::new(requested)) {
            return Err(ToolError::PathDenied(requested.into()));
        }
        let candidate = self.workspace_root.join(requested);
        if candidate.exists() {
            self.check_relative_path_gate(requested, false)?;
            let canonical = candidate
                .canonicalize()
                .map_err(|error| map_path_io(&candidate, error))?;
            if !canonical.starts_with(&self.workspace_root)
                || private_output_path(&canonical)
                || crate::tool_output::private_output_path_inode(&canonical)?
            {
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
        if !canonical_existing.starts_with(&self.workspace_root)
            || private_output_path(&canonical_existing)
        {
            return Err(ToolError::PathDenied(requested.to_owned()));
        }
        Ok(candidate)
    }

    fn resolve_existing(&self, requested: &str) -> Result<ResolvedPath, ToolError> {
        let requested = &*self.workspace_path(requested)?;
        self.check_relative_path_gate(requested, false)?;
        validate_relative_path(requested)?;
        if private_output_path(Path::new(requested)) {
            return Err(ToolError::PathDenied(requested.into()));
        }
        let candidate = self.workspace_root.join(requested);
        let canonical = candidate
            .canonicalize()
            .map_err(|error| map_path_io(&candidate, error))?;
        if !canonical.starts_with(&self.workspace_root)
            || private_output_path(&canonical)
            || crate::tool_output::private_output_path_inode(&canonical)?
        {
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

/// Host-owned scheduling declaration, captured once at registration.
/// Parallel is honored only for read-only tools; one sequential call is a
/// barrier for its entire batch. Extensions must explicitly opt in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecutionMode {
    Sequential,
    Parallel,
}

pub trait Tool: Send + Sync {
    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    /// Long-running implementations should poll cancellation and finish all
    /// owned child work before returning. The default preserves old tools;
    /// the batch owner still joins them, never detaches unfinished work.
    fn invoke_cancellable(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
        self.invoke(context, arguments)
    }

    fn definition(&self) -> ToolDefinition;
    fn invoke(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ToolError>;
}

#[derive(Clone)]
struct RegisteredTool {
    definition: ToolDefinition,
    handler: Arc<dyn Tool>,
    builtin: bool,
    execution_mode: ToolExecutionMode,
}

/// Every builtin tool name, in registration order.
pub const BUILTIN_TOOL_NAMES: &[&str] = &[
    "read_file",
    "read_tool_output",
    "list_directory",
    "search_text",
    "find",
    "write_file",
    "edit_file",
    "run_command",
    "websearch",
    "dag_status",
    "dag_send",
    "dag_recv",
    "dag_finish",
    "dag_spawn",
];

/// Which builtins are advertised to the model (ZE-214).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuiltinSelection {
    /// Advertise `dag_*` (DAG-spawned worker or explicit opt-in).
    pub dag: bool,
    /// Advertise `websearch` (its API key resolves).
    pub websearch: bool,
    /// `--tools a,b`: exactly these builtins, overriding the flags above.
    pub only: Option<Vec<String>>,
}

/// Process-wide `--tools` allow-list so every owner a host prepares (CLI
/// agent and project owners alike) advertises the same builtin set.
static BUILTIN_ALLOWLIST: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

/// Install (or clear) the process-wide `--tools` allow-list.
pub fn set_builtin_allowlist(only: Option<Vec<String>>) {
    if let Ok(mut guard) = BUILTIN_ALLOWLIST.lock() {
        *guard = only;
    }
}

/// Return the process-wide `--tools` allow-list, if the host installed one.
pub fn current_builtin_allowlist() -> Option<Vec<String>> {
    BUILTIN_ALLOWLIST
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

/// Parse a `--tools a,b` value into known builtin names (unknown → error).
pub fn parse_builtin_allowlist(value: &str) -> Result<Vec<String>, String> {
    if matches!(value.trim(), "none" | "off" | "no-tools") {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = Vec::new();
    for name in value
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        if !BUILTIN_TOOL_NAMES.contains(&name) {
            return Err(format!(
                "unknown tool `{name}`; known tools: {}",
                BUILTIN_TOOL_NAMES.join(",")
            ));
        }
        if !names.iter().any(|existing| existing == name) {
            names.push(name.to_owned());
        }
    }
    if names.is_empty() {
        return Err("--tools requires at least one tool name".into());
    }
    Ok(names)
}

impl BuiltinSelection {
    /// `dag_*` only for DAG-spawned workers (`ZENPI_DAG_NODE`) or when
    /// `ZENPI_DAG_TOOLS=1`; `websearch` only when [`exa_api_key`] resolves;
    /// plus the process-wide `--tools` allow-list, if any.
    pub fn from_environment() -> Self {
        Self {
            dag: crate::dag::dag_tools_enabled(),
            websearch: exa_api_key().is_some(),
            only: BUILTIN_ALLOWLIST
                .lock()
                .ok()
                .and_then(|guard| guard.clone()),
        }
    }
}

#[derive(Default, Clone)]
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
        registry.register_builtin(ReadToolOutputTool)?;
        registry.register_builtin(ListDirectoryTool)?;
        registry.register_builtin(SearchTextTool)?;
        registry.register_builtin(FindFilesTool)?;
        Ok(registry)
    }

    pub fn with_all_builtins() -> Result<Self, ToolError> {
        let mut registry = Self::with_read_only_builtins()?;
        registry.register_builtin(WriteFileTool)?;
        registry.register_builtin(EditFileTool)?;
        registry.register_builtin(RunCommandTool)?;
        registry.register_builtin(WebSearchTool)?;
        registry.register_builtin(DagStatusTool)?;
        registry.register_builtin(DagSendTool)?;
        registry.register_builtin(DagRecvTool)?;
        registry.register_builtin(DagFinishTool)?;
        registry.register_builtin(DagSpawnTool)?;
        Ok(registry)
    }

    /// Builtins scoped by `selection` (ZE-214): an explicit `only` list keeps
    /// exactly the named builtins; otherwise `dag_*` and `websearch` are kept
    /// only when their selection flag is set.
    pub fn with_builtin_selection(selection: &BuiltinSelection) -> Result<Self, ToolError> {
        let mut registry = Self::with_all_builtins()?;
        if let Some(only) = &selection.only {
            for name in only {
                if !registry.tools.contains_key(name) {
                    return Err(ToolError::InvalidDefinition(format!(
                        "unknown builtin tool `{name}`"
                    )));
                }
            }
            registry.tools.retain(|name, _| only.contains(name));
            return Ok(registry);
        }
        registry.tools.retain(|name, _| {
            if name.starts_with("dag_") {
                selection.dag
            } else if name == "websearch" {
                selection.websearch
            } else {
                true
            }
        });
        Ok(registry)
    }

    /// Builtins a host advertises by default: see
    /// [`BuiltinSelection::from_environment`].
    pub fn with_default_builtins() -> Result<Self, ToolError> {
        Self::with_builtin_selection(&BuiltinSelection::from_environment())
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
                execution_mode: tool.execution_mode(),
                handler: Arc::new(tool),
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
                execution_mode: tool.execution_mode(),
                handler: Arc::from(tool),
                builtin: false,
            },
        );
        Ok(())
    }

    pub(crate) fn remove_extension_tools(&mut self, names: &[String]) {
        for name in names {
            self.tools.remove(name);
        }
    }

    /// Strict, bounded schema subset for extension rewrites. Unsupported schema
    /// keywords fail closed instead of pretending the schema was checked.
    pub(crate) fn validate_hook_call(
        &self,
        context: &ToolContext,
        call: &ToolCall,
    ) -> Result<(), ToolError> {
        validate_call(call)?;
        let registered = self
            .tools
            .get(&call.name)
            .ok_or_else(|| ToolError::UnknownTool(call.name.clone()))?;
        validate_hook_schema(&registered.definition.input_schema, &call.arguments, 0)?;
        context.check_call_gate(
            &call.name,
            registered.definition.side_effect,
            call.arguments.as_object().expect("validated object"),
        )?;
        if let Some(gate) = context.checked_gate()?
            && !registered.builtin
        {
            return Err(gate.deny("untrusted_tool_implementation"));
        }
        if registered.builtin
            && let Some(path) = call.arguments.get("path").and_then(Value::as_str)
        {
            if registered.definition.side_effect == ToolSideEffect::WorkspaceWrite {
                context.resolve_for_write(path)?;
            } else {
                context.resolve_existing(path)?;
            }
        }
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

    pub fn execution_mode(&self, name: &str) -> ToolExecutionMode {
        match self.tools.get(name) {
            Some(tool) if tool.definition.side_effect == ToolSideEffect::ReadOnly => {
                tool.execution_mode
            }
            _ => ToolExecutionMode::Sequential,
        }
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
        self.execute_cancellable(context, policy, call, &|| false)
    }

    /// Same registry, immutable gate, validation, redaction and bounds as
    /// ordinary execution, with cancellation passed through to the handler.
    pub fn execute_cancellable(
        &self,
        context: &ToolContext,
        policy: SideEffectPolicy,
        call: ToolCall,
        cancelled: &dyn Fn() -> bool,
    ) -> ToolResult {
        let call_id = call.id.clone();
        let tool_name = call.name.clone();
        let outcome = self.execute_inner(context, policy, &call, cancelled);
        match outcome {
            Ok(output) => ToolResult::Success {
                call_id,
                tool: tool_name,
                output,
                content: None,
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
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        validate_call(call)?;
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
        if context
            .output_capture
            .as_ref()
            .is_some_and(|capture| capture.call_id() != call.id)
        {
            return Err(ToolError::InvalidCall(
                "output capture belongs to a different call".into(),
            ));
        }
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
        let mut output = registered
            .handler
            .invoke_cancellable(context, arguments, cancelled)?;
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
        content,
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
            content,
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
            // Compaction rewrites the compatibility record only: typed content
            // must never be truncated into an artifact reference.
            content,
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
        content,
    })
}

/// Plain text the model receives for one tool result (pi `content`). The
/// structured [`ToolResult`] stays the journal and UI record (pi `details`);
/// providers get this text plus an `is_error` flag, never the JSON envelope.
/// Built-in tools render a compact human form; anything else (extension
/// tools, hook-rewritten or unexpected shapes) keeps the serialized record.
pub fn model_facing_text(
    result: &ToolResult,
    capture: Option<&crate::tool_output::CaptureReport>,
) -> String {
    let (tool, output) = match result {
        ToolResult::Error { error, .. } => return error.message.clone(),
        ToolResult::Success { tool, output, .. } => (tool, output),
    };
    let rendered = if output.get("compacted").is_some() && output.get("preview").is_some() {
        compacted_model_text(output, capture)
    } else {
        match tool.as_str() {
            "read_file" => read_file_model_text(output),
            "run_command" => run_command_model_text(output, capture),
            "write_file" => write_file_model_text(output),
            "edit_file" => edit_file_model_text(output),
            "search_text" => search_model_text(output),
            "find" => find_model_text(output),
            "list_directory" => list_model_text(output),
            _ => None,
        }
    };
    rendered.unwrap_or_else(|| {
        let mut value = serde_json::to_value(result).unwrap_or(Value::Null);
        if let Some(report) = capture
            && let Some(object) = value.as_object_mut()
        {
            object.insert(
                "output_capture".into(),
                serde_json::to_value(report).unwrap_or(Value::Null),
            );
        }
        value.to_string()
    })
}

/// How the model can page through one captured raw output stream.
fn artifact_hint(artifact: &crate::tool_output::ArtifactRef) -> String {
    format!(
        "read_tool_output artifact_id={} call_id={}",
        artifact.artifact_id, artifact.call_id
    )
}

fn compacted_model_text(
    output: &Value,
    capture: Option<&crate::tool_output::CaptureReport>,
) -> Option<String> {
    let preview = output.get("preview")?.as_str()?;
    let bytes = output.get("bytes").and_then(Value::as_u64).unwrap_or(0);
    let note = match output.get("artifact").and_then(Value::as_str) {
        Some(artifact) => format!(
            "[Result too large ({bytes} bytes); full JSON stored at {artifact}. Preview follows]"
        ),
        None => format!("[Result too large ({bytes} bytes); showing a preview only]"),
    };
    let mut text = format!("{note}\n{preview}");
    for artifact in capture.into_iter().flat_map(|report| &report.artifacts) {
        let stream = match artifact.stream {
            crate::tool_output::OutputStream::Stdout => "stdout",
            crate::tool_output::OutputStream::Stderr => "stderr",
        };
        text.push_str(&format!("\n[Full {stream}: {}]", artifact_hint(artifact)));
    }
    Some(text)
}

fn output_flag(output: &Value, key: &str) -> bool {
    output.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn read_file_model_text(output: &Value) -> Option<String> {
    let content = output.get("content")?.as_str()?;
    if !output_flag(output, "truncated") {
        return Some(content.to_owned());
    }
    let mut text = content.to_owned();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    let number = |key: &str| output.get(key).and_then(Value::as_u64).unwrap_or(0);
    match output.get("next_offset").and_then(Value::as_u64) {
        Some(next) => text.push_str(&format!(
            "\n[Showing lines {}-{} of {}. Use offset={next} to continue]",
            number("start_line"),
            number("end_line"),
            number("total_lines")
        )),
        None => text.push_str(&format!(
            "\n[Line {} was cut at {} bytes; use run_command to inspect it]",
            number("end_line"),
            content.len()
        )),
    }
    Some(text)
}

struct LinePage {
    content: String,
    truncated: bool,
    start_line: usize,
    end_line: usize,
    total_lines: usize,
    next_offset: Option<usize>,
}

/// Read lines `offset..offset+limit` (1-based) within `max_bytes`, counting the
/// file's total lines. Lines are never split except a single over-long line,
/// which is cut on a character boundary.
fn read_line_page<R: io::BufRead>(
    mut reader: R,
    offset: usize,
    limit: usize,
    max_bytes: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<LinePage, ToolError> {
    let mut content = String::new();
    let mut line = Vec::new();
    let mut total = 0usize;
    let mut end = offset.saturating_sub(1);
    let mut full = false;
    let mut cut_line = false;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        total += 1;
        if total.is_multiple_of(4096) && cancelled() {
            return Err(ToolError::Cancelled);
        }
        if total < offset || full {
            continue;
        }
        let text = std::str::from_utf8(&line).map_err(|_| ToolError::InvalidUtf8(String::new()))?;
        if total - offset >= limit || content.len() + text.len() > max_bytes {
            full = true;
            if content.is_empty() {
                // One line larger than the page: return its head, cut cleanly.
                let mut cut = max_bytes.min(text.len());
                while !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                content.push_str(&text[..cut]);
                end = total;
                cut_line = true;
            }
            continue;
        }
        content.push_str(text);
        end = total;
    }
    let truncated = full || cut_line;
    Ok(LinePage {
        content,
        truncated,
        start_line: offset.min(total.max(1)),
        end_line: end,
        total_lines: total,
        next_offset: (truncated && end < total).then_some(end + 1),
    })
}

fn run_command_model_text(
    output: &Value,
    capture: Option<&crate::tool_output::CaptureReport>,
) -> Option<String> {
    let stdout = output.get("stdout")?.as_str()?;
    let stderr = output.get("stderr")?.as_str()?;
    let mut combined = String::new();
    for stream in [stdout, stderr] {
        if stream.is_empty() {
            continue;
        }
        combined.push_str(stream);
        if !stream.ends_with('\n') {
            combined.push('\n');
        }
    }
    let (tail, cut) = model_command_tail(&combined);
    let mut text = String::new();
    if cut {
        text.push_str("[Showing the last part of the output]\n");
    }
    text.push_str(tail);
    if !text.is_empty() {
        text.push('\n');
    }
    if output_flag(output, "cancelled") {
        text.push_str("Command was cancelled");
    } else if output_flag(output, "timed_out") {
        let timeout = output
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        text.push_str(&format!("Command timed out after {timeout} ms"));
    } else if let Some(code) = output.get("exit_code").and_then(Value::as_i64) {
        text.push_str(&format!("Command exited with code {code}"));
    } else if let Some(signal) = output.get("signal").and_then(Value::as_i64) {
        text.push_str(&format!("Command terminated by signal {signal}"));
    } else {
        text.push_str("Command finished without an exit code");
    }
    let artifacts: Vec<crate::tool_output::ArtifactRef> = output
        .get("output_artifacts")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .or_else(|| capture.map(|report| report.artifacts.clone()))
        .unwrap_or_default();
    for (stream, key, label) in [
        (
            crate::tool_output::OutputStream::Stdout,
            "stdout_truncated",
            "stdout",
        ),
        (
            crate::tool_output::OutputStream::Stderr,
            "stderr_truncated",
            "stderr",
        ),
    ] {
        if !output_flag(output, key) {
            continue;
        }
        text.push_str(&format!("\n[{label} truncated: earlier output omitted]"));
        if let Some(artifact) = artifacts.iter().find(|artifact| artifact.stream == stream) {
            text.push_str(&format!(" Full {label}: {}", artifact_hint(artifact)));
        }
    }
    Some(text)
}

fn write_file_model_text(output: &Value) -> Option<String> {
    let path = output.get("path")?.as_str()?;
    let bytes = output.get("bytes")?.as_u64()?;
    let verb = if output_flag(output, "created") {
        "Created"
    } else {
        "Wrote"
    };
    Some(format!("{verb} {path} ({bytes} bytes)"))
}

fn edit_file_model_text(output: &Value) -> Option<String> {
    let path = output.get("path")?.as_str()?;
    let replacements = output
        .get("replacements")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let noun = if replacements == 1 {
        "replacement"
    } else {
        "replacements"
    };
    Some(format!("Edited {path} ({replacements} {noun})"))
}

fn finish_listing(mut lines: Vec<String>, empty: &str, output: &Value) -> String {
    if lines.is_empty() {
        lines.push(empty.into());
    }
    if output_flag(output, "truncated") {
        let reasons: Vec<&str> = output
            .get("truncation_reasons")
            .and_then(Value::as_array)
            .map(|reasons| reasons.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        lines.push(if reasons.is_empty() {
            "[Results truncated]".into()
        } else {
            format!("[Results truncated: {}]", reasons.join(", "))
        });
    }
    lines.join("\n")
}

fn search_model_text(output: &Value) -> Option<String> {
    let record = |item: &Value, matched: bool| -> Option<(String, u64, bool, String)> {
        Some((
            item.get("path")?.as_str()?.to_owned(),
            item.get("line")?.as_u64()?,
            matched,
            item.get("text")?.as_str()?.to_owned(),
        ))
    };
    let mut records = Vec::new();
    for item in output.get("matches")?.as_array()? {
        records.push(record(item, true)?);
    }
    for item in output
        .get("context")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        records.push(record(item, false)?);
    }
    // Matches precede context for the same line, so dedup keeps the match.
    records.sort_by(|left, right| (&left.0, left.1).cmp(&(&right.0, right.1)));
    records.dedup_by(|later, earlier| earlier.0 == later.0 && earlier.1 == later.1);
    let lines = records
        .into_iter()
        .map(|(path, line, matched, text)| {
            let separator = if matched { ':' } else { '-' };
            format!(
                "{path}{separator}{line}{separator} {}",
                text.trim_end_matches(['\r', '\n'])
            )
        })
        .collect();
    Some(finish_listing(lines, "No matches found", output))
}

fn find_model_text(output: &Value) -> Option<String> {
    let mut lines = Vec::new();
    for path in output.get("paths")?.as_array()? {
        lines.push(path.as_str()?.to_owned());
    }
    Some(finish_listing(lines, "No files found", output))
}

fn list_model_text(output: &Value) -> Option<String> {
    let mut lines = Vec::new();
    for entry in output.get("entries")?.as_array()? {
        let name = entry.get("name")?.as_str()?;
        let suffix = match entry.get("kind").and_then(Value::as_str) {
            Some("directory") => "/",
            Some("symlink") => "@",
            _ => "",
        };
        lines.push(format!("{name}{suffix}"));
    }
    Some(finish_listing(lines, "(empty directory)", output))
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ReadToolOutputTool;
impl Tool for ReadToolOutputTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition { name:"read_tool_output".into(), description:"Read a bounded redacted range of a recorded output artifact in this session. Use the artifact_id and call_id from its capture result; offsets count original raw bytes.".into(),
            input_schema:json!({"type":"object","properties":{"artifact_id":{"type":"string","minLength":64,"maxLength":64},"call_id":{"type":"string","minLength":1,"maxLength":128},"offset":{"type":"integer","minimum":0},"length":{"type":"integer","minimum":1,"maximum":crate::tool_output::MAX_PUBLIC_OUTPUT_READ_BYTES}},"required":["artifact_id","call_id","offset","length"],"additionalProperties":false}), side_effect:ToolSideEffect::ReadOnly }
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
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
        context.admit_builtin("read_tool_output", arguments)?;
        let request: crate::tool_output::OutputReadRequest =
            serde_json::from_value(Value::Object(arguments.clone()))
                .map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
        let access = context.output_read.as_ref().ok_or_else(|| {
            ToolError::InvalidCall("read_tool_output requires the current session owner".into())
        })?;
        let reply = access
            .read(
                &request,
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .min(u64::MAX as u128) as u64,
                cancelled,
            )
            .map_err(|e| {
                if matches!(e, crate::tool_output::OutputError::Cancelled) {
                    ToolError::Cancelled
                } else {
                    ToolError::InvalidArguments(format!("{}: {e}", e.code()))
                }
            })?;
        serde_json::to_value(reply).map_err(ToolError::Json)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ReadFileTool;

impl Tool for ReadFileTool {
    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".into(),
            description: "Read a UTF-8 text file inside the workspace by lines. Returns at most `limit` lines (default 2000) and 50 KiB per call; continue with `offset` (1-based line) as the result says.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 1 },
                    "limit": { "type": "integer", "minimum": 1, "maximum": MAX_READ_LINES },
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
        self.invoke_cancellable(context, arguments, &|| false)
    }

    fn invoke_cancellable(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
        context.admit_builtin("read_file", arguments)?;
        reject_unknown(arguments, &["path", "offset", "limit", "max_bytes"])?;
        let requested = required_string(arguments, "path", 4_096)?;
        let offset = bounded_usize(arguments, "offset", 1, 1, usize::MAX)?;
        let limit = bounded_usize(arguments, "limit", MAX_READ_LINES, 1, MAX_READ_LINES)?;
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
        let file = File::open(&path.canonical)?;
        if crate::tool_output::private_output_inode(&file)? {
            return Err(ToolError::PathDenied(requested.into()));
        }
        let page = read_line_page(
            io::BufReader::new(file),
            offset,
            limit,
            max_bytes,
            cancelled,
        )
        .map_err(|error| match error {
            ToolError::InvalidUtf8(_) => ToolError::InvalidUtf8(requested.to_owned()),
            error => error,
        })?;
        Ok(json!({
            "path": relative_display(&path.relative),
            "content": page.content,
            "truncated": page.truncated,
            "start_line": page.start_line,
            "end_line": page.end_line,
            "total_lines": page.total_lines,
            "next_offset": page.next_offset
        }))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ListDirectoryTool;

impl Tool for ListDirectoryTool {
    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

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
        self.invoke_cancellable(context, arguments, &|| false)
    }

    fn invoke_cancellable(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
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
            if cancelled() {
                return Err(ToolError::Cancelled);
            }
            let entry = entry?;
            if private_output_path(&entry.path()) {
                continue;
            }
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
    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "search_text".into(),
            description: "Search file contents with one installed ripgrep run. The query is a regex by default; set literal=true (or regex=false) for a fixed-string search. glob/ignore/hidden/context map to ripgrep flags. Without ripgrep only literal search without glob/ignore/hidden/context is available.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "path": { "type": "string", "default": "." },
                    "case_sensitive": { "type": "boolean", "default": true },
                    "max_matches": { "type": "integer", "minimum": 1, "maximum": MAX_SEARCH_MATCHES },
                    "regex": { "type": "boolean", "default": true },
                    "literal": { "type": "boolean", "default": false },
                    "glob": { "type": "string" },
                    "ignore": { "type": "boolean", "default": true },
                    "hidden": { "type": "boolean", "default": false },
                    "context": { "type": "integer", "minimum": 0, "maximum": 10 }
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
        self.invoke_cancellable(context, arguments, &|| false)
    }

    fn invoke_cancellable(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
        context.admit_builtin("search_text", arguments)?;
        reject_unknown(
            arguments,
            &[
                "query",
                "path",
                "case_sensitive",
                "max_matches",
                "regex",
                "literal",
                "glob",
                "ignore",
                "hidden",
                "context",
            ],
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
        let literal = optional_bool(arguments, "literal", false)?;
        let regex = optional_bool(arguments, "regex", !literal)?;
        if literal && regex {
            return Err(ToolError::InvalidArguments(
                "literal=true conflicts with regex=true".into(),
            ));
        }
        let options = crate::search::SearchOptions {
            query: query.into(),
            regex,
            glob: arguments
                .get("glob")
                .map(|_| required_string(arguments, "glob", 4096).map(str::to_owned))
                .transpose()?,
            ignore: optional_bool(arguments, "ignore", true)?,
            hidden: optional_bool(arguments, "hidden", false)?,
            context: bounded_usize(arguments, "context", 0, 0, 10)?,
            case_sensitive,
            max_matches,
        };
        let gate = context.checked_gate()?;
        if gate.is_none() && context.search_backend.available() {
            return context.search_backend.search(
                context.workspace_root(),
                &path.canonical,
                &options,
                cancelled,
            );
        }
        // Fallback when ripgrep is not installed, or when a worker gate needs
        // per-path checks ripgrep cannot honor: a literal in-process scan.
        let ripgrep_only = ["glob", "ignore", "hidden", "context"]
            .iter()
            .any(|key| arguments.contains_key(*key));
        if ripgrep_only || (options.regex && !crate::search::regex_is_literal(query)) {
            if let Some(gate) = gate {
                return Err(gate.deny("advanced_search_requires_confined_traversal"));
            }
            return Err(ToolError::Unsupported(
                "ripgrep is not installed; regex/glob/ignore/hidden/context search needs it (set literal=true for a fixed-string search)".into(),
            ));
        }
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
            if cancelled() {
                return Err(ToolError::Cancelled);
            }
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
                    if cancelled() {
                        return Err(ToolError::Cancelled);
                    }
                    if children.len() >= remaining {
                        node_limit_reached = true;
                        break;
                    }
                    children.push(child?);
                }
                children.sort_by_key(|entry| entry.file_name());
                for child in children {
                    let file_type = child.file_type()?;
                    if file_type.is_symlink()
                        || private_output_path(&child.path())
                        || ignored_directory(&child.path(), file_type)
                    {
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
            let mut file = File::open(&current)?;
            if crate::tool_output::private_output_inode(&file)? {
                continue;
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            if bytes.contains(&0) {
                continue;
            }
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue;
            };
            for (index, line) in text.lines().enumerate() {
                if cancelled() {
                    return Err(ToolError::Cancelled);
                }
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

/// Conservative total process starts for one built-in call. A `search_text`
/// call starts at most one ripgrep child (none for the in-process fallback);
/// `find` runs its path listing plus a glob listing. The host reserves this
/// before dispatch; unused starts are not refunded.
pub fn search_process_budget(name: &str, _arguments: &Map<String, Value>) -> u64 {
    match name {
        "find" => 2,
        "search_text" => 1,
        _ => 0,
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FindFilesTool;
impl Tool for FindFilesTool {
    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {name:"find".into(),description:"Find workspace files by glob using installed ripgrep, with explicit ignore/hidden options and bounded results.".into(),
            input_schema:json!({"type":"object","properties":{
                "pattern":{"type":"string"},"path":{"type":"string","default":"."},
                "ignore":{"type":"boolean","default":true},"hidden":{"type":"boolean","default":false},
                "limit":{"type":"integer","minimum":1,"maximum":256}},"required":["pattern"],"additionalProperties":false}),
            side_effect:ToolSideEffect::ReadOnly}
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
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
        context.admit_builtin("find", arguments)?;
        reject_unknown(arguments, &["pattern", "path", "ignore", "hidden", "limit"])?;
        let pattern = required_string(arguments, "pattern", 4096)?;
        let requested = optional_string(arguments, "path", ".", 4096)?;
        let path = context.resolve_existing(requested)?;
        if !path.canonical.is_dir() {
            return Err(ToolError::NotADirectory(requested.into()));
        }
        if let Some(gate) = context.checked_gate()? {
            return Err(gate.deny("advanced_search_requires_confined_traversal"));
        }
        context.search_backend.find(
            context.workspace_root(),
            &path.canonical,
            pattern,
            optional_bool(arguments, "ignore", true)?,
            optional_bool(arguments, "hidden", false)?,
            bounded_usize(arguments, "limit", 100, 1, 256)?,
            cancelled,
        )
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
        let edited = apply_text_edit(&original, old, new)?;
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
                "Replace one unique UTF-8 text occurrence atomically and return a bounded unified diff. \
                 Matching ignores CRLF vs LF, a UTF-8 BOM, trailing whitespace and Unicode quote, \
                 dash and space variants; the file keeps its line endings."
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
        let edited = apply_text_edit(&original, old, new)?;
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
    fn encode_outcome(context: &ToolContext, outcome: CommandOutcome) -> Result<Value, ToolError> {
        let mut value = serde_json::to_value(outcome).map_err(ToolError::Json)?;
        if let Some(capture) = &context.output_capture {
            let object = value.as_object_mut().expect("command outcome is an object");
            object.insert(
                "output_artifacts".into(),
                serde_json::to_value(capture.artifacts()).map_err(ToolError::Json)?,
            );
            object.insert(
                "output_capture_errors".into(),
                serde_json::to_value(capture.errors()).map_err(ToolError::Json)?,
            );
        }
        Ok(value)
    }

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
            // pi semantics: a failing command is an error result that still
            // carries its output tail and exit code; output is never dropped.
            let value = Self::encode_outcome(context, outcome)?;
            let text = run_command_model_text(&value, None)
                .unwrap_or_else(|| "Command failed without output".into());
            return Err(ToolError::CommandFailed(text));
        }
        Self::encode_outcome(context, outcome)
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
        Self::encode_outcome(context, outcome)
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
        // pi semantics: timeout is optional with no default. Worker gates keep
        // their policy ceiling as the default and maximum.
        let timeout_ms = match gate {
            Some(gate) => Some(bounded_usize(
                arguments,
                "timeout_ms",
                gate.policy.max_command_timeout_ms as usize,
                1,
                gate.policy.max_command_timeout_ms as usize,
            )? as u64),
            None if arguments.contains_key("timeout_ms") => Some(bounded_usize(
                arguments,
                "timeout_ms",
                1,
                1,
                MAX_COMMAND_TIMEOUT_MS as usize,
            )? as u64),
            None => None,
        };
        let output_cap = gate.map_or(MAX_COMMAND_OUTPUT_BYTES, |gate| {
            gate.policy.max_command_output_bytes
        });
        if cancelled() {
            // Preserve a structured user-shell outcome even when cancellation
            // wins the race before the child is spawned.  The host owner
            // journals this evidence and can report a terminal, reaped
            // operation instead of collapsing it into a transient tool error.
            return Ok(CommandOutcome {
                status: "cancelled".into(),
                origin: context.origin(),
                exit_code: None,
                signal: None,
                stdout: String::new(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                timed_out: false,
                cancelled: true,
                timeout_ms: timeout_ms.unwrap_or(0),
                child_reaped: true,
                process_group_terminated: false,
                policy_evidence: context.policy_evidence(),
            });
        }
        struct CloseCapture<'a>(Option<&'a crate::tool_output::CommandOutputCapture>);
        impl Drop for CloseCapture<'_> {
            fn drop(&mut self) {
                if let Some(capture) = self.0 {
                    capture.close();
                }
            }
        }
        let capture = context.output_capture.as_deref();
        let _capture_guard = CloseCapture(capture);
        let (stdout_capture, stderr_capture) = match capture {
            Some(capture) => {
                let (stdout, stderr) = capture.begin_streams().map_err(|error| {
                    ToolError::InvalidArguments(format!("output capture preflight failed: {error}"))
                })?;
                (Some(stdout), Some(stderr))
            }
            None => (None, None),
        };
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
        // One thread pumps both pipes and watches the child: it sleeps in
        // poll(2) until output, the deadline or the next cancellation check.
        let mut pumps = [
            StreamPump::new(stdout.into(), output_cap, stdout_capture),
            StreamPump::new(stderr.into(), output_cap, stderr_capture),
        ];
        let deadline = timeout_ms.map(|ms| Instant::now() + Duration::from_millis(ms));
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
            let now = Instant::now();
            if deadline.is_some_and(|deadline| now >= deadline) {
                timed_out = true;
                break Ok(None);
            }
            let mut wait = deadline.map_or(CANCEL_POLL, |deadline| {
                deadline.saturating_duration_since(now).min(CANCEL_POLL)
            });
            if pumps.iter().all(StreamPump::done) {
                // Both pipes closed: the child is exiting, so reap it promptly.
                wait = wait.min(EXIT_POLL);
            }
            pump_streams(&mut pumps, wait);
        };
        // A command's descendants do not outlive its operation, even if the
        // shell exits successfully or closes its own stdout before they do.
        child.cleanup();
        // Drain what the killed group left behind; a pipe still held open by
        // an escaped descendant ends the drain as truncated.
        let drain_until = Instant::now() + DRAIN_GRACE;
        while !pumps.iter().all(StreamPump::done) {
            let left = drain_until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            pump_streams(&mut pumps, left);
        }
        let [stdout, stderr] = pumps;
        let mut stdout = stdout.finish();
        let mut stderr = stderr.finish();
        let command_complete = !was_cancelled && !timed_out && status.is_ok();
        if let Some(capture) = capture {
            for stream in [&mut stdout, &mut stderr] {
                if let Some(writer) = &mut stream.artifact
                    && let Err(error) = capture.finish_stream(
                        writer,
                        command_complete && stream.eof && stream.error.is_none(),
                    )
                {
                    capture.record_error(error.to_string());
                }
            }
        }
        // Capture failures remain visible in the outcome while the command's
        // observed exit evidence remains intact. They never claim full output.
        if let Some(error) = stdout.error.take().or_else(|| stderr.error.take()) {
            return Err(ToolError::Io(error));
        }
        let stdout_truncated = stdout.truncated;
        let stderr_truncated = stderr.truncated;
        let stdout = stdout.bytes;
        let stderr = stderr.bytes;
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
            stdout: tail_utf8(&String::from_utf8_lossy(&stdout), output_cap).to_owned(),
            stderr: tail_utf8(&String::from_utf8_lossy(&stderr), output_cap).to_owned(),
            stdout_truncated,
            stderr_truncated,
            timed_out,
            cancelled: was_cancelled,
            timeout_ms: timeout_ms.unwrap_or(0),
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

pub(crate) struct SupervisedChild {
    pub(crate) child: std::process::Child,
    cleaned: bool,
}

impl SupervisedChild {
    pub(crate) fn new(child: std::process::Child) -> Self {
        Self {
            child,
            cleaned: false,
        }
    }
    pub(crate) fn cleanup(&mut self) {
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
    if crate::tool_output::private_output_inode(&file)? {
        return Err(ToolError::PathDenied(display_path(path)));
    }
    let mut bytes = Vec::with_capacity(MAX_WRITE_BYTES.min(64 * 1024));
    file.take(MAX_WRITE_BYTES.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() > MAX_WRITE_BYTES;
    bytes.truncate(MAX_WRITE_BYTES);
    let text = String::from_utf8(bytes).map_err(|_| ToolError::InvalidUtf8(display_path(path)))?;
    Ok((text, truncated))
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
    if crate::tool_output::private_output_inode(&file)? {
        return Err(ToolError::PathDenied(display_path(path)));
    }
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
/// Replace the single occurrence of `old` in `original` (pi edit semantics).
///
/// Matching runs on LF-normalized text without a leading BOM. An exact match
/// is preferred; otherwise a fuzzy match ignores trailing whitespace per line
/// and folds Unicode quotes, dashes, spaces and full-width forms to ASCII.
/// Uniqueness is always judged on the fuzzy form, so ambiguous text is
/// rejected. Only the matched source span is rewritten: the BOM, the rest of
/// the file and its line endings are preserved, and `new` is written with the
/// file's line ending.
fn apply_text_edit(original: &str, old: &str, new: &str) -> Result<String, ToolError> {
    let (bom, body) = match original.strip_prefix('\u{feff}') {
        Some(body) => ("\u{feff}", body),
        None => ("", original),
    };
    let old = old.strip_prefix('\u{feff}').unwrap_or(old);
    let exact = NormalizedText::new(body, false);
    let fuzzy = NormalizedText::new(body, true);
    let exact_old = NormalizedText::new(old, false).text;
    let fuzzy_old = NormalizedText::new(old, true).text;
    if fuzzy_old.trim().is_empty() {
        return Err(ToolError::InvalidArguments(
            "old text must contain non-whitespace characters".into(),
        ));
    }
    let occurrences = fuzzy.text.matches(fuzzy_old.as_str()).count();
    if occurrences != 1 {
        let hint = if occurrences == 0 {
            "; re-read the file and copy the text exactly"
        } else {
            "; include more surrounding lines to make it unique"
        };
        return Err(ToolError::InvalidArguments(format!(
            "old text must occur exactly once (found {occurrences}){hint}"
        )));
    }
    let (start, end) = match exact.text.find(exact_old.as_str()) {
        Some(index) => exact.source_span(index, exact_old.len()),
        None => {
            let index = fuzzy.text.find(fuzzy_old.as_str()).unwrap_or_default();
            fuzzy.source_span(index, fuzzy_old.len())
        }
    };
    let crlf = body
        .find('\n')
        .is_some_and(|index| body[..index].ends_with('\r'));
    let new = NormalizedText::new(new, false).text;
    let new = if crlf { new.replace('\n', "\r\n") } else { new };
    let mut edited = String::with_capacity(original.len() + new.len());
    edited.push_str(bom);
    edited.push_str(&body[..start]);
    edited.push_str(&new);
    edited.push_str(&body[end..]);
    Ok(edited)
}

/// Text with normalized line endings (and, when fuzzy, folded characters and
/// no trailing whitespace) plus a map from each output character back to the
/// source bytes that produced it.
struct NormalizedText {
    text: String,
    /// (output byte offset, source start, source end) for every output char.
    spans: Vec<(usize, usize, usize)>,
}

impl NormalizedText {
    fn new(source: &str, fuzzy: bool) -> Self {
        let mut normalized = Self {
            text: String::with_capacity(source.len()),
            spans: Vec::new(),
        };
        let mut line_start = 0;
        loop {
            let rest = &source[line_start..];
            let (line_len, break_len) = match rest.find(['\r', '\n']) {
                Some(index) if rest[index..].starts_with("\r\n") => (index, 2),
                Some(index) => (index, 1),
                None => (rest.len(), 0),
            };
            let line = &rest[..line_len];
            let kept = if fuzzy { line.trim_end() } else { line };
            for (offset, character) in kept.char_indices() {
                let start = line_start + offset;
                let end = start + character.len_utf8();
                if fuzzy {
                    normalized.push_folded(character, start, end);
                } else {
                    normalized.push(character, start, end);
                }
            }
            if break_len == 0 {
                return normalized;
            }
            let start = line_start + line_len;
            normalized.push('\n', start, start + break_len);
            line_start = start + break_len;
        }
    }

    fn push(&mut self, character: char, start: usize, end: usize) {
        self.spans.push((self.text.len(), start, end));
        self.text.push(character);
    }

    /// NFKC-like folding for the characters models most often rewrite.
    fn push_folded(&mut self, character: char, start: usize, end: usize) {
        let folded = match character {
            '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' => '\'',
            '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{201f}' => '"',
            '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
            '\u{00a0}' | '\u{2002}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' => ' ',
            '\u{2026}' => {
                for _ in 0..3 {
                    self.push('.', start, end);
                }
                return;
            }
            // Full-width ASCII forms fold to ASCII, as NFKC does.
            '\u{ff01}'..='\u{ff5e}' => {
                char::from_u32(u32::from(character) - 0xff01 + 0x21).unwrap_or(character)
            }
            other => other,
        };
        self.push(folded, start, end);
    }

    /// Source byte range covered by `len` output bytes starting at `index`.
    fn source_span(&self, index: usize, len: usize) -> (usize, usize) {
        let first = self.spans.partition_point(|span| span.0 < index);
        let last = self.spans.partition_point(|span| span.0 < index + len);
        let start = self.spans.get(first).map_or(0, |span| span.1);
        let end = last
            .checked_sub(1)
            .and_then(|last| self.spans.get(last))
            .map_or(start, |span| span.2);
        (start, end.max(start))
    }
}

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
        // Preserve CR bytes: normalizing CRLF here would hide real file changes.
        text.split('\n').map(str::to_owned).collect()
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
    // The terminating newline is part of the final line's identity. Include
    // that line in the change even when its text also matches a prefix/suffix.
    if old.final_newline != new.final_newline {
        if !old.final_newline && !old.lines.is_empty() {
            prefix = prefix.min(old.lines.len() - 1);
        }
        if !new.final_newline && !new.lines.is_empty() {
            prefix = prefix.min(new.lines.len() - 1);
        }
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

    let mut push_line = |prefix: char, line: &str, no_newline: bool| {
        rendered.push(prefix);
        rendered.push_str(line);
        rendered.push('\n');
        if !source_truncated && no_newline {
            rendered.push_str("\\ No newline at end of file\n");
        }
    };
    for index in old_start..prefix {
        push_line(' ', &old.lines[index], false);
    }
    for index in prefix..old_change_end {
        push_line(
            '-',
            &old.lines[index],
            !old.final_newline && index + 1 == old.lines.len(),
        );
    }
    for index in prefix..new_change_end {
        push_line(
            '+',
            &new.lines[index],
            !new.final_newline && index + 1 == new.lines.len(),
        );
    }
    for index in old_change_end..old_end {
        push_line(
            ' ',
            &old.lines[index],
            !old.final_newline && index + 1 == old.lines.len(),
        );
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
            // Negative PID addresses the process group created above. Like
            // pi's process-tree kill: SIGKILL with no grace sleep.
            // SAFETY: `pid` is the child ID returned by `std::process`; the
            // child leads its own process group, and a group ID is not reused
            // while any member lives, so this is sound even after the leader
            // was reaped. `kill` borrows no Rust memory and errors are
            // tolerated because the group may already be gone.
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
    fn invoke_cancellable(
        &self,
        context: &ToolContext,
        arguments: &Map<String, Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        Self::invoke_with_cancel(context, arguments, cancelled)
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "run_command".into(),
            description: "Run a bounded shell command in the workspace.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "maxLength": MAX_COMMAND_BYTES },
                    "timeout_ms": { "type": "integer", "minimum": 1, "maximum": 86400000 }
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
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        match fs::metadata(path) {
            // Keep ordinary access/execute bits, never propagate set-ID bits
            // onto replacement content.
            Ok(metadata) => Some(fs::Permissions::from_mode(
                metadata.permissions().mode() & 0o777,
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(ToolError::Io(error)),
        }
    };
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(permissions) = &permissions {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(permissions.mode() & 0o600);
    }
    // A failed create_new gives us no ownership of the existing path.
    let mut file = options.open(&temporary)?;
    let write_result = (|| {
        file.write_all(bytes)?;
        #[cfg(unix)]
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        crate::session::durable_sync(&file)
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(ToolError::Io(error));
    }
    drop(file);
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(ToolError::Io(error));
    }
    // Persist the directory entry after the atomic replacement. On Unix this
    // closes the rename durability window after the temporary file was synced.
    #[cfg(unix)]
    {
        crate::session::durable_sync(&File::open(parent)?)?;
    }
    Ok(())
}

/// Keep the last `max` bytes of `text`, starting on a character boundary.
fn tail_utf8(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// pi's model-facing command output budget: the last 2000 lines / 50 KB.
const MODEL_COMMAND_OUTPUT_LINES: usize = 2000;
const MODEL_COMMAND_OUTPUT_BYTES: usize = 50 * 1024;

fn model_command_tail(text: &str) -> (&str, bool) {
    let mut cut = tail_utf8(text, MODEL_COMMAND_OUTPUT_BYTES);
    if let Some((index, _)) = cut.rmatch_indices('\n').nth(MODEL_COMMAND_OUTPUT_LINES) {
        cut = &cut[index + 1..];
    }
    (cut, cut.len() < text.len())
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
pub(crate) fn make_pipe_nonblocking(pipe: &impl std::os::fd::AsRawFd) -> Result<(), ToolError> {
    let fd = pipe.as_raw_fd();
    // SAFETY: the borrowed pipe owns an open descriptor for both fcntl calls.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(ToolError::Io(io::Error::last_os_error()));
    }
    Ok(())
}

struct LimitedStream {
    bytes: Vec<u8>,
    truncated: bool,
    eof: bool,
    error: Option<io::Error>,
    artifact: Option<crate::tool_output::ArtifactWriter>,
}

/// Longest sleep between cancellation checks while a child runs.
pub(crate) const CANCEL_POLL: Duration = Duration::from_millis(100);
/// Reap interval once both pipes closed and the child is exiting.
const EXIT_POLL: Duration = Duration::from_millis(5);
/// Bound on draining pipes after the process group was killed.
const DRAIN_GRACE: Duration = Duration::from_millis(100);

/// Block until any of `read` is readable or closed, any of `write` is
/// writable, or `timeout` elapses. With no descriptors this is a plain sleep.
#[cfg(unix)]
pub(crate) fn wait_ready(
    read: &[std::os::fd::RawFd],
    write: &[std::os::fd::RawFd],
    timeout: Duration,
) {
    if read.is_empty() && write.is_empty() {
        std::thread::sleep(timeout);
        return;
    }
    let record = |fd, events| libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    let mut polls: Vec<libc::pollfd> = read
        .iter()
        .map(|&fd| record(fd, libc::POLLIN))
        .chain(write.iter().map(|&fd| record(fd, libc::POLLOUT)))
        .collect();
    let millis = timeout.as_millis().clamp(1, i32::MAX as u128) as i32;
    // SAFETY: `polls` is an initialized array of `polls.len()` records whose
    // descriptors the caller keeps open for the duration of the call. Errors
    // (EINTR included) just end the wait early; callers re-check their state.
    unsafe {
        libc::poll(polls.as_mut_ptr(), polls.len() as libc::nfds_t, millis);
    }
}

#[cfg(not(unix))]
pub(crate) fn wait_ready(_read: &[i32], _write: &[i32], timeout: Duration) {
    std::thread::sleep(timeout.min(Duration::from_millis(5)));
}

/// Read every pump that has data, sleeping in poll(2) up to `timeout` first.
fn pump_streams(pumps: &mut [StreamPump], timeout: Duration) {
    use std::os::fd::AsRawFd;
    let fds: Vec<_> = pumps
        .iter()
        .filter(|pump| !pump.done())
        .map(|pump| pump.file.as_raw_fd())
        .collect();
    wait_ready(&fds, &[], timeout);
    for pump in pumps.iter_mut().filter(|pump| !pump.done()) {
        pump.read_available();
    }
}

/// A non-blocking pipe reader that keeps the tail, where errors usually are.
struct StreamPump {
    file: std::fs::File,
    max: usize,
    bytes: Vec<u8>,
    truncated: bool,
    eof: bool,
    error: Option<io::Error>,
    artifact: Option<crate::tool_output::ArtifactWriter>,
}

impl StreamPump {
    fn new(
        fd: std::os::fd::OwnedFd,
        max: usize,
        artifact: Option<crate::tool_output::ArtifactWriter>,
    ) -> Self {
        Self {
            file: fd.into(),
            max,
            bytes: Vec::with_capacity(max.min(64 * 1024)),
            truncated: false,
            eof: false,
            error: None,
            artifact,
        }
    }

    fn done(&self) -> bool {
        self.eof || self.error.is_some()
    }

    fn read_available(&mut self) {
        let mut buffer = [0_u8; 8192];
        loop {
            let count = match self.file.read(&mut buffer) {
                Ok(0) => {
                    self.eof = true;
                    return;
                }
                Ok(count) => count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            };
            if let Some(writer) = &mut self.artifact {
                // Continue draining after a spill failure. The writer permanently
                // records the failure and marks its terminal artifact incomplete.
                let _ = writer.append(&buffer[..count]);
            }
            // Trim in amortized batches.
            self.bytes.extend_from_slice(&buffer[..count]);
            if self.bytes.len() > self.max.saturating_mul(2).max(16 * 1024) {
                let excess = self.bytes.len() - self.max;
                self.bytes.drain(..excess);
                self.truncated = true;
            }
        }
    }

    fn finish(mut self) -> LimitedStream {
        // Stopped before EOF: the command's output is not complete.
        if !self.done() {
            self.truncated = true;
        }
        if self.bytes.len() > self.max {
            let excess = self.bytes.len() - self.max;
            self.bytes.drain(..excess);
            self.truncated = true;
        }
        LimitedStream {
            bytes: self.bytes,
            truncated: self.truncated,
            eof: self.eof,
            error: self.error,
            artifact: self.artifact,
        }
    }
}

pub(crate) fn validate_call(call: &ToolCall) -> Result<(), ToolError> {
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
        return Err(ToolError::LimitExceeded(format!(
            "tool call arguments are {} bytes; the limit is {MAX_TOOL_CALL_BYTES} bytes. \
             Split the content into smaller writes or edits",
            serialized.len()
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

/// `dag_status` — read the current node's subtree, close gate and mailbox depth.
#[derive(Debug, Clone, Copy, Default)]
pub struct DagStatusTool;

impl Tool for DagStatusTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "dag_status".into(),
            description: "Read the DAG node you own: parent, children statuses, whether the node may close (node green and all descendants green), unfinished descendants, and unread message count.".into(),
            input_schema: json!({"type":"object","properties":{
                "node":{"type":"string"}},"additionalProperties":false}),
            side_effect: ToolSideEffect::ReadOnly,
        }
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
        _: &ToolContext,
        arguments: &Map<String, Value>,
        _: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        reject_unknown(arguments, &["node"])?;
        let node = optional_string(arguments, "node", "", 128)?.to_owned();
        let node = if node.is_empty() {
            crate::dag::current_node().ok_or_else(|| {
                ToolError::InvalidArguments("set ZENPI_DAG_NODE or pass node".into())
            })?
        } else {
            node
        };
        let store = crate::dag::DagStore::default_store();
        let _ = store.touch_worker(&node);
        crate::dag::status_view(&store, &node).map_err(ToolError::Unsupported)
    }
}

/// `dag_send` — message parent/grandparent/siblings/children (or `all`).
#[derive(Debug, Clone, Copy, Default)]
pub struct DagSendTool;

impl Tool for DagSendTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "dag_send".into(),
            description: "Send a bounded message to another DAG worker: `to` is a node id or one of parent, grandparent, sibling, child, all (parent + grandparent + direct siblings + direct children). Messages to a child spawned by this process are also piped to its live headless stdin.".into(),
            input_schema: json!({"type":"object","properties":{
                "to":{"type":"string"},"body":{"type":"string"},"from":{"type":"string"}},
                "required":["to","body"],"additionalProperties":false}),
            side_effect: ToolSideEffect::WorkspaceWrite,
        }
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
        _: &ToolContext,
        arguments: &Map<String, Value>,
        _: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        reject_unknown(arguments, &["to", "body", "from"])?;
        let to = required_string(arguments, "to", 128)?;
        let body = required_string(arguments, "body", crate::dag::MAX_DAG_BODY_BYTES)?;
        let from_arg = optional_string(arguments, "from", "", 128)?.to_owned();
        let from = if from_arg.is_empty() {
            crate::dag::current_node().ok_or_else(|| {
                ToolError::InvalidArguments("set ZENPI_DAG_NODE or pass from".into())
            })?
        } else {
            from_arg
        };
        let store = crate::dag::DagStore::default_store();
        let delivered = store
            .send(&from, to, body)
            .map_err(ToolError::Unsupported)?;
        let mut piped = Vec::new();
        for target in &delivered {
            if crate::dag::send_to_worker(target, body).unwrap_or(false) {
                piped.push(target.clone());
            }
        }
        Ok(json!({"from": from, "delivered": delivered, "piped": piped}))
    }
}

/// `dag_recv` — bounded poll for messages addressed to this node.
#[derive(Debug, Clone, Copy, Default)]
pub struct DagRecvTool;

impl Tool for DagRecvTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "dag_recv".into(),
            description: "Wait (bounded, <=30s) for messages addressed to your DAG node and claim them (ack/claim semantics: claimed-but-unacked messages are redelivered on the next call; pass ack:[ids] once processed). Use it to stay alive while children or parents still need you.".into(),
            input_schema: json!({"type":"object","properties":{
                "node":{"type":"string"},"wait_seconds":{"type":"integer","minimum":0,"maximum":30},
                "ack":{"type":"array","items":{"type":"integer"},"description":"message ids already processed"}},
                "additionalProperties":false}),
            side_effect: ToolSideEffect::ReadOnly,
        }
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
        _: &ToolContext,
        arguments: &Map<String, Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        reject_unknown(arguments, &["node", "wait_seconds", "ack"])?;
        let node = optional_string(arguments, "node", "", 128)?.to_owned();
        let node = if node.is_empty() {
            crate::dag::current_node().ok_or_else(|| {
                ToolError::InvalidArguments("set ZENPI_DAG_NODE or pass node".into())
            })?
        } else {
            node
        };
        let wait = bounded_usize(arguments, "wait_seconds", 0, 0, 30)? as u64;
        let store = crate::dag::DagStore::default_store();
        let _ = store.touch_worker(&node);
        let acked = match arguments.get("ack") {
            None | Some(Value::Null) => 0,
            Some(Value::Array(ids)) => {
                let mut acked = Vec::new();
                for id in ids {
                    let Some(id) = id.as_u64() else {
                        return Err(ToolError::InvalidArguments(
                            "ack ids must be unsigned integers".into(),
                        ));
                    };
                    acked.push(id);
                }
                store.ack(&node, &acked).map_err(ToolError::Unsupported)?
            }
            Some(_) => {
                return Err(ToolError::InvalidArguments(
                    "ack must be an array of message ids".into(),
                ));
            }
        };
        let messages =
            crate::dag::wait_for_message(&store, &node, Duration::from_secs(wait), cancelled)
                .map_err(ToolError::Unsupported)?;
        Ok(json!({"node": node, "acked": acked, "messages": messages, "count": messages.len()}))
    }
}

/// `dag_finish` — report node status; the close gate enforces the subtree.
#[derive(Debug, Clone, Copy, Default)]
pub struct DagFinishTool;

impl Tool for DagFinishTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "dag_finish".into(),
            description: "Report your DAG node status (green/red/open). The result says whether the node may close: it may close only when the node is green and every child/grandchild is green. When can_close is false the worker must stay alive and handle the unfinished work (spawn workers, send messages, keep polling).".into(),
            input_schema: json!({"type":"object","properties":{
                "node":{"type":"string"},"status":{"type":"string","enum":["open","green","red"]},
                "summary":{"type":"string"}},"required":["status"],"additionalProperties":false}),
            side_effect: ToolSideEffect::WorkspaceWrite,
        }
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
        _: &ToolContext,
        arguments: &Map<String, Value>,
        _: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        reject_unknown(arguments, &["node", "status", "summary"])?;
        let node = optional_string(arguments, "node", "", 128)?.to_owned();
        let node = if node.is_empty() {
            crate::dag::current_node().ok_or_else(|| {
                ToolError::InvalidArguments("set ZENPI_DAG_NODE or pass node".into())
            })?
        } else {
            node
        };
        let status = required_string(arguments, "status", 16)?;
        let summary =
            optional_string(arguments, "summary", "", crate::dag::MAX_DAG_STATUS_BYTES)?.to_owned();
        let store = crate::dag::DagStore::default_store();
        let _ = store.upsert(&node, None).map_err(ToolError::Unsupported)?;
        let node_view = store
            .set_status(
                &node,
                status,
                (!summary.is_empty()).then_some(summary.as_str()),
            )
            .map_err(ToolError::Unsupported)?;
        let (can_close, unfinished) = store.can_close(&node).map_err(ToolError::Unsupported)?;
        Ok(json!({
            "node": node_view.id,
            "status": node_view.status,
            "can_close": can_close,
            "unfinished": unfinished,
            "instruction": if can_close {
                "Node and every descendant are green: the worker may close."
            } else {
                "Do NOT close: keep this worker alive, spawn workers for the unfinished nodes (dag_spawn) and communicate (dag_send/dag_recv) until the whole subtree is green."
            },
        }))
    }
}

/// `dag_spawn` — create a child node and start a live headless worker for it.
#[derive(Debug, Clone, Copy, Default)]
pub struct DagSpawnTool;

impl Tool for DagSpawnTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "dag_spawn".into(),
            description: "Create a child DAG node and spawn a live headless worker for it. The child gets ZENPI_DAG_NODE/ZENPI_DAG_STORE, and this process keeps its stdin so dag_send can deliver follow-up work while it stays alive. Bounded: spawn depth <= 3 below the root and at most 8 children per node.".into(),
            input_schema: json!({"type":"object","properties":{
                "node":{"type":"string"},"parent":{"type":"string"},"prompt":{"type":"string"},
                "session_dir":{"type":"string"}},
                "required":["node","prompt"],"additionalProperties":false}),
            side_effect: ToolSideEffect::CommandExecution,
        }
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
        _: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        reject_unknown(arguments, &["node", "parent", "prompt", "session_dir"])?;
        let node = required_string(arguments, "node", 128)?;
        let prompt = required_string(arguments, "prompt", 16_384)?;
        let parent_arg = optional_string(arguments, "parent", "", 128)?.to_owned();
        let parent = if parent_arg.is_empty() {
            crate::dag::current_node()
        } else {
            Some(parent_arg)
        };
        let store = crate::dag::DagStore::default_store();
        let parent_ref = parent.as_deref();
        store
            .spawn_child(node, parent_ref)
            .map_err(ToolError::Unsupported)?;
        let session_dir = match optional_string(arguments, "session_dir", "", 1_024)? {
            "" => {
                let base = store
                    .path()
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| context.workspace_root().to_path_buf());
                base.join("workers")
            }
            value => {
                let path = PathBuf::from(value);
                if path.is_absolute() {
                    path
                } else {
                    context.workspace_root().join(path)
                }
            }
        };
        let pid = crate::dag::spawn_worker(&store, node, prompt, &session_dir)
            .map_err(ToolError::Unsupported)?;
        store
            .assign_worker(node, &format!("pid:{pid}"))
            .map_err(ToolError::Unsupported)?;
        Ok(json!({
            "node": node,
            "parent": parent,
            "pid": pid,
            "session_dir": session_dir.display().to_string(),
        }))
    }
}

/// One web search result set is bounded before it reaches the model.
const WEB_SEARCH_MAX_BYTES: usize = 6_000;
const WEB_SEARCH_TIMEOUT_SECONDS: u64 = 20;
/// A local rate-limit proxy queues bursts, so swarms may need a longer wait.
const WEB_SEARCH_MAX_TIMEOUT_SECONDS: u64 = 300;

/// `websearch` builtin: Exa-backed web search for current facts, papers and
/// news. Read-only; under `--auto` it runs without approval. The API key is
/// resolved from `ZENPI_EXA_API_KEY`, then `EXA_API_KEY`, then
/// `~/.zenpi/exa.env` so a host can configure it without exporting env vars.
#[derive(Debug, Clone, Copy, Default)]
pub struct WebSearchTool;

impl WebSearchTool {
    fn invoke_search(
        &self,
        query: &str,
        num_results: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
        let key = exa_api_key().ok_or_else(|| {
            ToolError::Unsupported(
                "web search is not configured; set EXA_API_KEY (e.g. ~/.zenpi/exa.env)".into(),
            )
        })?;
        let endpoint = std::env::var("ZENPI_EXA_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "https://api.exa.ai/search".into());
        let payload = json!({
            "query": query,
            "numResults": num_results,
            "contents": {"text": {"maxCharacters": 800}},
        });
        let timeout_seconds = std::env::var("ZENPI_EXA_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(WEB_SEARCH_TIMEOUT_SECONDS)
            .clamp(1, WEB_SEARCH_MAX_TIMEOUT_SECONDS);
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(timeout_seconds)))
            .http_status_as_error(false)
            .build();
        let agent = ureq::Agent::new_with_config(config);
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
        let mut response = agent
            .post(&endpoint)
            .header("content-type", "application/json")
            .header("x-api-key", &key)
            .send(payload.to_string())
            .map_err(|error| {
                ToolError::Unsupported(format!("web search request failed: {error}"))
            })?;
        let status = response.status();
        let text = response.body_mut().read_to_string().map_err(|error| {
            ToolError::Unsupported(format!("web search response failed: {error}"))
        })?;
        if !status.is_success() {
            return Err(ToolError::Unsupported(format!(
                "web search HTTP {}: {}",
                status.as_u16(),
                truncate_line(&text, 200)
            )));
        }
        let value: Value = serde_json::from_str(&text).map_err(|error| {
            ToolError::Unsupported(format!("web search returned invalid JSON: {error}"))
        })?;
        if cancelled() {
            return Err(ToolError::Cancelled);
        }
        Ok(format_web_search_results(&value, num_results))
    }
}

impl Tool for WebSearchTool {
    fn execution_mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "websearch".into(),
            description: "Search the web (Exa) for current information and return bounded title/url/snippet results. Use it for up-to-date facts, papers and news; cite the returned URLs.".into(),
            input_schema: json!({"type":"object","properties":{
                "query":{"type":"string"},
                "num_results":{"type":"integer","minimum":1,"maximum":10,"default":5}},
                "required":["query"],"additionalProperties":false}),
            side_effect: ToolSideEffect::ReadOnly,
        }
    }

    fn invoke(
        &self,
        _context: &ToolContext,
        arguments: &Map<String, Value>,
    ) -> Result<Value, ToolError> {
        self.invoke_cancellable(_context, arguments, &|| false)
    }

    fn invoke_cancellable(
        &self,
        _context: &ToolContext,
        arguments: &Map<String, Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        reject_unknown(arguments, &["query", "num_results"])?;
        let query = required_string(arguments, "query", 4_096)?;
        let num_results = bounded_usize(arguments, "num_results", 5, 1, 10)?;
        self.invoke_search(query, num_results, cancelled)
    }
}

/// Resolve the Exa API key: explicit env vars first, then the user's
/// `~/.zenpi/exa.env` file.
pub fn exa_api_key() -> Option<String> {
    for name in ["ZENPI_EXA_API_KEY", "EXA_API_KEY"] {
        if let Ok(value) = std::env::var(name) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    let home = std::env::var_os("HOME")?;
    let text = fs::read_to_string(PathBuf::from(home).join(".zenpi").join("exa.env")).ok()?;
    parse_exa_env(&text)
}

/// Parse `EXA_API_KEY=...` (optionally `export`ed or quoted) from an env file.
pub fn parse_exa_env(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "EXA_API_KEY" {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if !value.is_empty() && !value.chars().any(char::is_control) {
            return Some(value.to_owned());
        }
    }
    None
}

/// Bound and normalize an Exa response before it reaches the model.
pub fn format_web_search_results(value: &Value, num_results: usize) -> Value {
    let mut results = Vec::new();
    let mut used = 0usize;
    if let Some(items) = value.get("results").and_then(Value::as_array) {
        for item in items.iter().take(num_results.max(1)) {
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let url = item.get("url").and_then(Value::as_str).unwrap_or("").trim();
            let text = item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if title.is_empty() && url.is_empty() && text.is_empty() {
                continue;
            }
            let snippet = truncate_line(text, 600);
            let cost = title.len() + url.len() + snippet.len();
            if used + cost > WEB_SEARCH_MAX_BYTES {
                break;
            }
            used += cost;
            results.push(json!({"title": title, "url": url, "snippet": snippet}));
        }
    }
    let count = results.len();
    json!({"results": results, "count": count})
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

// Only used at the hook rewrite boundary; existing tool handlers remain the
// authoritative validators for their ordinary discovery-only schemas.
fn validate_hook_schema(schema: &Value, value: &Value, depth: usize) -> Result<(), ToolError> {
    let invalid =
        || ToolError::InvalidArguments("extension rewrite does not satisfy the tool schema".into());
    if depth > 16 {
        return Err(invalid());
    }
    let object = schema.as_object().ok_or_else(invalid)?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "type"
                | "description"
                | "title"
                | "default"
                | "properties"
                | "required"
                | "additionalProperties"
                | "items"
                | "enum"
                | "minimum"
                | "maximum"
                | "minLength"
                | "maxLength"
                | "minItems"
                | "maxItems"
        )
    }) {
        return Err(ToolError::Unsupported(
            "extension rewrite schema keyword".into(),
        ));
    }
    if let Some(kind) = object.get("type") {
        let valid = match kind.as_str() {
            Some("object") => value.is_object(),
            Some("array") => value.is_array(),
            Some("string") => value.is_string(),
            Some("boolean") => value.is_boolean(),
            Some("null") => value.is_null(),
            Some("number") => value.is_number(),
            Some("integer") => value.is_i64() || value.is_u64(),
            _ => false,
        };
        if !valid {
            return Err(invalid());
        }
    }
    if let Some(choices) = object.get("enum")
        && !choices
            .as_array()
            .is_some_and(|choices| choices.contains(value))
    {
        return Err(invalid());
    }
    if let Some(map) = value.as_object() {
        let properties = object
            .get("properties")
            .map(|v| v.as_object().ok_or_else(invalid))
            .transpose()?;
        if let Some(required) = object.get("required") {
            for key in required.as_array().ok_or_else(invalid)? {
                if !map.contains_key(key.as_str().ok_or_else(invalid)?) {
                    return Err(invalid());
                }
            }
        }
        for (key, val) in map {
            if let Some(child) = properties.and_then(|p| p.get(key)) {
                validate_hook_schema(child, val, depth + 1)?;
            } else {
                match object.get("additionalProperties") {
                    Some(Value::Bool(false)) => return Err(invalid()),
                    Some(Value::Object(_)) => {
                        validate_hook_schema(&object["additionalProperties"], val, depth + 1)?
                    }
                    Some(Value::Bool(true)) | None => {}
                    _ => return Err(invalid()),
                }
            }
        }
    }
    if let Some(array) = value.as_array() {
        for (key, too_small) in [("minItems", true), ("maxItems", false)] {
            if let Some(bound) = object.get(key) {
                let bound = bound.as_u64().ok_or_else(invalid)?;
                if if too_small {
                    (array.len() as u64) < bound
                } else {
                    (array.len() as u64) > bound
                } {
                    return Err(invalid());
                }
            }
        }
        if let Some(items) = object.get("items") {
            for val in array {
                validate_hook_schema(items, val, depth + 1)?;
            }
        }
    }
    if let Some(string) = value.as_str() {
        for (key, too_small) in [("minLength", true), ("maxLength", false)] {
            if let Some(bound) = object.get(key) {
                let bound = bound.as_u64().ok_or_else(invalid)?;
                let len = string.chars().count() as u64;
                if if too_small { len < bound } else { len > bound } {
                    return Err(invalid());
                }
            }
        }
    }
    if let Some(number) = value.as_f64() {
        for (key, lower) in [("minimum", true), ("maximum", false)] {
            if let Some(bound) = object.get(key) {
                let bound = bound.as_f64().ok_or_else(invalid)?;
                if if lower {
                    number < bound
                } else {
                    number > bound
                } {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(())
}
