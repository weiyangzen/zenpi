//! A small, durable owner for one local Blueprint execution step.
//!
//! The public domain records in [`crate::domains`] deliberately do not start
//! workers.  This module is the first executable slice on top of those
//! records: it selects one dependency-ready item, records a bounded
//! deterministic evidence operation, and persists the operation's lifecycle
//! (`running` then terminal) in an atomic snapshot.  It does not invoke a
//! shell, a provider, or another agent.  A host can use the same store after a
//! restart to resume a receipt that was left `running` by a crashed process.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::b3::ResourceBudget;
use crate::domains::{
    Blueprint, BlueprintItem, BlueprintTask, DomainError, Goal, GoalStatus, MAX_DOMAIN_RECORD_BYTES,
};

/// On-disk schema for the bounded execution snapshot.
pub const EXECUTION_SCHEMA_VERSION: u16 = 1;
/// Optional process-level override used by hosts that keep execution state in
/// a workspace-owned location.  The default remains next to a custom session
/// journal so a session can be copied without touching the user's global
/// state.
pub const EXECUTION_STORE_ENV: &str = "ZENPI_EXECUTION_STORE";
/// Filename used when no explicit execution-store override is configured.
pub const EXECUTION_STORE_FILE_NAME: &str = "execution.json";
/// Filename for immutable handoff requests consumed by an external worker.
/// The local owner writes requests but never claims that they were executed.
pub const HANDOFF_STORE_FILE_NAME: &str = "handoff.json";
/// Optional process-level override for the external-owner handoff snapshot.
pub const HANDOFF_STORE_ENV: &str = "ZENPI_HANDOFF_STORE";
/// A malformed execution path must not turn startup into an unbounded read.
pub const MAX_EXECUTION_STORE_BYTES: usize = 16 * 1024 * 1024;
/// One goal can have many retries, but the owner still needs a hard ceiling.
pub const MAX_EXECUTION_RECEIPTS: usize = 4_096;
/// Keep queued external work bounded even if a host repeatedly retries.
pub const MAX_HANDOFF_REQUESTS: usize = 4_096;
/// Handoff snapshots have the same conservative read/write bound as receipts.
pub const MAX_HANDOFF_STORE_BYTES: usize = 16 * 1024 * 1024;
/// Cost charged for the deterministic local evidence operation.  These are
/// accounting units, not provider tokens or an estimate of wall time.
pub const DETERMINISTIC_WALL_CLOCK_MS: u64 = 1;
pub const DETERMINISTIC_DISK_BYTES: u64 = 512;

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("execution store path is empty")]
    EmptyPath,
    #[error("execution store path points to a directory: {0}")]
    Directory(PathBuf),
    #[error("execution store path is a symbolic link: {0}")]
    Symlink(PathBuf),
    #[error("execution store is too large (maximum {max} bytes, found {actual} bytes)")]
    StoreTooLong { max: usize, actual: u64 },
    #[error("execution store has no valid snapshot")]
    MissingSnapshot,
    #[error("execution store JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("execution store I/O: {0}")]
    Io(#[from] io::Error),
    #[error("unsupported execution schema {found} (expected {expected})")]
    SchemaVersion { found: u16, expected: u16 },
    #[error("execution store digest does not match its receipts")]
    DigestMismatch,
    #[error("execution store has too many receipts (maximum {max})")]
    TooManyReceipts { max: usize },
    #[error("execution receipt is invalid: {0}")]
    InvalidReceipt(String),
    #[error("domain record is invalid: {0}")]
    Domain(#[from] DomainError),
    #[error("goal status {status:?} cannot execute a Blueprint item")]
    GoalNotRunnable { status: GoalStatus },
    #[error("goal is not linked to the supplied Blueprint")]
    BlueprintLinkMismatch,
    #[error(
        "execution budget exceeded for {field}: used {used}, requested {requested}, limit {limit}"
    )]
    BudgetExceeded {
        field: &'static str,
        used: u64,
        requested: u64,
        limit: u64,
    },
    #[error("execution budget arithmetic overflowed for {field}")]
    BudgetOverflow { field: &'static str },
    #[error("execution receipt identity conflicts with an existing receipt: {execution_id}")]
    ReceiptConflict { execution_id: String },
    #[error("Blueprint item `{item_id}` has no declarative task to hand off")]
    HandoffTaskMissing { item_id: String },
    #[error("handoff request identity conflicts with an existing request: {handoff_id}")]
    HandoffConflict { handoff_id: String },
    #[error("handoff store has too many requests (maximum {max})")]
    TooManyHandoffs { max: usize },
    #[error("handoff store digest does not match its requests")]
    HandoffDigestMismatch,
}

impl ExecutionError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::GoalNotRunnable { .. } => "execution_goal_not_runnable",
            Self::BlueprintLinkMismatch => "execution_blueprint_link_mismatch",
            Self::BudgetExceeded { .. } | Self::BudgetOverflow { .. } => {
                "execution_budget_exceeded"
            }
            Self::ReceiptConflict { .. } => "execution_receipt_conflict",
            Self::HandoffTaskMissing { .. } => "execution_task_missing",
            Self::HandoffConflict { .. } => "execution_handoff_conflict",
            Self::TooManyHandoffs { .. } | Self::HandoffDigestMismatch => {
                "execution_handoff_store_error"
            }
            Self::InvalidReceipt(_) => "execution_invalid_receipt",
            _ => "execution_store_error",
        }
    }
}

/// Lifecycle state for one deterministic control-plane evidence attempt.
///
/// `Succeeded` means the receipt owner persisted and finalized its bounded
/// local bookkeeping operation. It does not mean the Blueprint item's
/// described implementation or acceptance command was executed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl ExecutionStatus {
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// One bounded, durable execution receipt.  The evidence is descriptive only:
/// the local owner never interprets it as a command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReceipt {
    pub execution_id: String,
    pub goal_id: String,
    pub blueprint_id: String,
    pub blueprint_version: String,
    pub blueprint_digest: String,
    pub item_id: String,
    pub attempt: u32,
    pub status: ExecutionStatus,
    pub cost: ResourceBudget,
    pub evidence: String,
    /// Set only after an external b3 worker imports a validated result
    /// manifest. The local receipt owner never sets this flag.
    #[serde(default)]
    pub external_work_executed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Minimal result contract emitted by an external Blueprint worker.  The
/// manifest is data only: zenpi verifies identity and acceptance evidence, but
/// never executes commands from it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExternalResultManifest {
    pub execution_id: String,
    #[serde(default)]
    pub claim_digest: String,
    pub blueprint_digest: String,
    pub item_id: String,
    pub status: ExecutionStatus,
    pub acceptance_passed: bool,
    pub acceptance_evidence: Vec<String>,
}

impl ExecutionReceipt {
    pub fn validate(&self) -> Result<(), ExecutionError> {
        bounded_id(&self.execution_id, "execution_id")?;
        bounded_id(&self.goal_id, "goal_id")?;
        bounded_id(&self.blueprint_id, "blueprint_id")?;
        bounded_id(&self.blueprint_version, "blueprint_version")?;
        bounded_id(&self.item_id, "item_id")?;
        validate_digest(&self.blueprint_digest)?;
        if self.attempt == 0 {
            return Err(ExecutionError::InvalidReceipt(
                "attempt must be at least one".into(),
            ));
        }
        if self.cost.attempts != 1
            || self.cost.wall_clock_ms != DETERMINISTIC_WALL_CLOCK_MS
            || self.cost.disk_bytes != DETERMINISTIC_DISK_BYTES
            || self.cost.tokens >= 5_000
        {
            return Err(ExecutionError::InvalidReceipt(
                "cost is outside the deterministic local-operation bounds".into(),
            ));
        }
        bounded_text(&self.evidence, "evidence", MAX_DOMAIN_RECORD_BYTES)?;
        match (self.external_work_executed, &self.manifest_checksum) {
            (false, None) => {}
            (true, Some(checksum)) if is_lowercase_sha256(checksum) => {}
            _ => {
                return Err(ExecutionError::InvalidReceipt(
                    "external evidence must carry a SHA-256 manifest checksum".into(),
                ));
            }
        }
        if let Some(error) = &self.error {
            bounded_text(error, "error", 4_096)?;
        }
        if self.status == ExecutionStatus::Running && self.error.is_some() {
            return Err(ExecutionError::InvalidReceipt(
                "running receipt cannot contain a terminal error".into(),
            ));
        }
        let encoded = serde_json::to_vec(self)?;
        if encoded.len() > MAX_DOMAIN_RECORD_BYTES {
            return Err(ExecutionError::InvalidReceipt(format!(
                "receipt exceeds {} bytes",
                MAX_DOMAIN_RECORD_BYTES
            )));
        }
        Ok(())
    }

    fn terminalized(mut self, status: ExecutionStatus, error: Option<String>) -> Self {
        self.status = status;
        self.error = error;
        self
    }
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A request for an external b3ehive/agent owner to execute one declarative
/// [`BlueprintTask`].  This is intentionally a handoff, not a completion
/// receipt: zenpi never executes the instruction or its acceptance commands
/// while creating this record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BlueprintHandoff {
    pub handoff_id: String,
    /// Immutable claim identity shared by the handoff and imported result.
    pub claim_digest: String,
    pub goal_id: String,
    pub blueprint_id: String,
    pub blueprint_version: String,
    pub blueprint_digest: String,
    pub item_id: String,
    pub attempt: u32,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub estimated_loc: u32,
    pub instruction: String,
    pub acceptance_commands: Vec<String>,
    /// Always `queued` for requests emitted by zenpi. An external owner may
    /// copy this record into its own result protocol without mutating zenpi's
    /// immutable request snapshot.
    pub status: HandoffStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HandoffStatus {
    Queued,
}

impl BlueprintHandoff {
    pub fn validate(&self) -> Result<(), ExecutionError> {
        bounded_id(&self.handoff_id, "handoff_id")?;
        validate_digest(&self.claim_digest)?;
        bounded_id(&self.goal_id, "goal_id")?;
        bounded_id(&self.blueprint_id, "blueprint_id")?;
        bounded_id(&self.blueprint_version, "blueprint_version")?;
        bounded_id(&self.item_id, "item_id")?;
        validate_digest(&self.blueprint_digest)?;
        BlueprintItem::new(&self.item_id, self.estimated_loc)
            .with_dependencies(self.depends_on.clone())
            .with_task(BlueprintTask {
                instruction: self.instruction.clone(),
                acceptance_commands: self.acceptance_commands.clone(),
            })
            .validate()
            .map_err(ExecutionError::Domain)?;
        if self.status != HandoffStatus::Queued {
            return Err(ExecutionError::InvalidReceipt(
                "unknown handoff status".into(),
            ));
        }
        let encoded = serde_json::to_vec(self)?;
        if encoded.len() > MAX_DOMAIN_RECORD_BYTES {
            return Err(ExecutionError::InvalidReceipt(format!(
                "handoff exceeds {} bytes",
                MAX_DOMAIN_RECORD_BYTES
            )));
        }
        Ok(())
    }

    /// Confirm that the request still refers to exactly the immutable source
    /// task. External consumers should perform this check before admission.
    pub fn validate_against(
        &self,
        goal: &Goal,
        blueprint: &Blueprint,
    ) -> Result<(), ExecutionError> {
        self.validate()?;
        goal.validate_against(blueprint)?;
        let item = blueprint
            .items
            .iter()
            .find(|item| item.id == self.item_id)
            .ok_or_else(|| ExecutionError::InvalidReceipt("handoff item is missing".into()))?;
        let task = item
            .task
            .as_ref()
            .ok_or_else(|| ExecutionError::HandoffTaskMissing {
                item_id: item.id.clone(),
            })?;
        let expected = make_handoff_request(goal, blueprint, item, self.attempt, task)?;
        if self != &expected {
            return Err(ExecutionError::HandoffConflict {
                handoff_id: self.handoff_id.clone(),
            });
        }
        Ok(())
    }
}

/// Result of one call to [`BlueprintExecutor::run_next`]. These outcomes
/// describe receipt-owner progress, not completion of external product work.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// One item reached a durable terminal state in this call.
    Executed {
        receipt: ExecutionReceipt,
        resumed_running: bool,
    },
    /// Cancellation was observed before starting (or before resuming) an
    /// item.  A pre-start cancellation does not consume an attempt.
    Cancelled {
        item_id: Option<String>,
        attempt: Option<u32>,
    },
    /// At least one item remains, but its dependencies are not successful.
    Blocked {
        item_id: String,
        waiting_on: Vec<String>,
    },
    /// Every item has a successful durable receipt.
    Complete,
}

/// Result of admitting one declarative task to an external worker owner.
/// Nothing in this outcome implies that the instruction or acceptance command
/// ran locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffOutcome {
    pub request: BlueprintHandoff,
    pub already_queued: bool,
}

/// A bounded result for an idempotent store mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStoreChange {
    Inserted,
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Snapshot {
    schema_version: u16,
    generation: u64,
    digest: String,
    receipts: Vec<ExecutionReceipt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    external: Vec<ExternalEvidence>,
}

#[derive(Debug, Serialize)]
struct UnsignedSnapshot<'a> {
    receipts: &'a [ExecutionReceipt],
    #[serde(skip_serializing_if = "<[ExternalEvidence]>::is_empty")]
    external: &'a [ExternalEvidence],
}

/// Atomic, private, bounded persistence for execution receipts.
#[derive(Debug, Clone)]
pub struct ExecutionStore {
    path: PathBuf,
    generation: u64,
    receipts: Vec<ExecutionReceipt>,
    external: Vec<ExternalEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HandoffSnapshot {
    schema_version: u16,
    generation: u64,
    digest: String,
    requests: Vec<BlueprintHandoff>,
}

#[derive(Debug, Serialize)]
struct UnsignedHandoffSnapshot<'a> {
    requests: &'a [BlueprintHandoff],
}

/// Private, bounded persistence for immutable requests to an external worker.
/// A separate snapshot keeps receipt compatibility stable while making the
/// handoff boundary explicit and inspectable by a future b3ehive adapter.
#[derive(Debug, Clone)]
pub struct HandoffStore {
    path: PathBuf,
    generation: u64,
    requests: Vec<BlueprintHandoff>,
}

impl HandoffStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ExecutionError> {
        let path = normalize_path(path.as_ref())?;
        ensure_parent(&path)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(ExecutionError::Symlink(path));
                }
                if !metadata.is_file() {
                    return Err(ExecutionError::Directory(path));
                }
                crate::security::restrict_private_file(&path)?;
                let bytes = read_bounded_with_limit(&path, MAX_HANDOFF_STORE_BYTES)?;
                if bytes.iter().all(u8::is_ascii_whitespace) {
                    return Err(ExecutionError::MissingSnapshot);
                }
                Self::decode(path, &bytes)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let store = Self::empty(path);
                store.persist_snapshot()?;
                Ok(store)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, ExecutionError> {
        let path = normalize_path(path.as_ref())?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(ExecutionError::Symlink(path));
                }
                if !metadata.is_file() {
                    return Err(ExecutionError::Directory(path));
                }
                let bytes = read_bounded_with_limit(&path, MAX_HANDOFF_STORE_BYTES)?;
                Self::decode(path, &bytes)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::empty(path)),
            Err(error) => Err(error.into()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn requests(&self) -> &[BlueprintHandoff] {
        &self.requests
    }

    pub fn find(&self, handoff_id: &str) -> Option<&BlueprintHandoff> {
        self.requests
            .iter()
            .find(|request| request.handoff_id == handoff_id)
    }

    pub fn find_claim(&self, claim_digest: &str) -> Option<&BlueprintHandoff> {
        self.requests
            .iter()
            .find(|request| request.claim_digest == claim_digest)
    }

    /// Insert one immutable request. Repeating the exact request is
    /// idempotent; changing any field under the same identity fails closed.
    pub fn insert(
        &mut self,
        request: BlueprintHandoff,
    ) -> Result<ExecutionStoreChange, ExecutionError> {
        request.validate()?;
        if let Some(existing) = self.find(&request.handoff_id) {
            if existing == &request {
                return Ok(ExecutionStoreChange::Unchanged);
            }
            return Err(ExecutionError::HandoffConflict {
                handoff_id: request.handoff_id,
            });
        }
        if self.requests.len() >= MAX_HANDOFF_REQUESTS {
            return Err(ExecutionError::TooManyHandoffs {
                max: MAX_HANDOFF_REQUESTS,
            });
        }
        if self.requests.iter().any(|existing| {
            existing.goal_id == request.goal_id
                && existing.blueprint_id == request.blueprint_id
                && existing.blueprint_version == request.blueprint_version
                && existing.blueprint_digest == request.blueprint_digest
                && existing.item_id == request.item_id
                && existing.attempt == request.attempt
        }) {
            return Err(ExecutionError::HandoffConflict {
                handoff_id: request.handoff_id,
            });
        }
        let mut next = self.clone();
        next.requests.push(request);
        next.commit()?;
        *self = next;
        Ok(ExecutionStoreChange::Inserted)
    }

    /// Remove a queued request during admission compensation. The identity
    /// check prevents a failed attempt from deleting a later replacement.
    fn remove_exact(&mut self, handoff_id: &str) -> Result<(), ExecutionError> {
        let Some(index) = self
            .requests
            .iter()
            .position(|request| request.handoff_id == handoff_id)
        else {
            return Ok(());
        };
        let mut next = self.clone();
        next.requests.remove(index);
        next.commit()?;
        *self = next;
        Ok(())
    }

    fn empty(path: PathBuf) -> Self {
        Self {
            path,
            generation: 0,
            requests: Vec::new(),
        }
    }

    fn commit(&mut self) -> Result<(), ExecutionError> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(ExecutionError::BudgetOverflow {
                field: "handoff_store_generation",
            })?;
        if let Err(error) = self.persist_snapshot() {
            self.generation = self.generation.saturating_sub(1);
            return Err(error);
        }
        Ok(())
    }

    fn persist_snapshot(&self) -> Result<(), ExecutionError> {
        for request in &self.requests {
            request.validate()?;
        }
        let snapshot = HandoffSnapshot {
            schema_version: EXECUTION_SCHEMA_VERSION,
            generation: self.generation,
            digest: digest_handoffs(&self.requests)?,
            requests: self.requests.clone(),
        };
        let bytes = serde_json::to_vec(&snapshot)?;
        if bytes.len() > MAX_HANDOFF_STORE_BYTES {
            return Err(ExecutionError::StoreTooLong {
                max: MAX_HANDOFF_STORE_BYTES,
                actual: bytes.len() as u64,
            });
        }
        atomic_replace_with_limit(&self.path, &bytes, MAX_HANDOFF_STORE_BYTES)
    }

    fn decode(path: PathBuf, bytes: &[u8]) -> Result<Self, ExecutionError> {
        let snapshot: HandoffSnapshot = serde_json::from_slice(bytes)?;
        if snapshot.schema_version != EXECUTION_SCHEMA_VERSION {
            return Err(ExecutionError::SchemaVersion {
                found: snapshot.schema_version,
                expected: EXECUTION_SCHEMA_VERSION,
            });
        }
        if snapshot.requests.len() > MAX_HANDOFF_REQUESTS {
            return Err(ExecutionError::TooManyHandoffs {
                max: MAX_HANDOFF_REQUESTS,
            });
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut attempts = std::collections::BTreeSet::new();
        for request in &snapshot.requests {
            request.validate()?;
            if !ids.insert(request.handoff_id.as_str()) {
                return Err(ExecutionError::InvalidReceipt(
                    "duplicate handoff_id".into(),
                ));
            }
            let identity = (
                request.goal_id.as_str(),
                request.blueprint_id.as_str(),
                request.blueprint_version.as_str(),
                request.blueprint_digest.as_str(),
                request.item_id.as_str(),
                request.attempt,
            );
            if !attempts.insert(identity) {
                return Err(ExecutionError::HandoffConflict {
                    handoff_id: request.handoff_id.clone(),
                });
            }
        }
        if snapshot.digest != digest_handoffs(&snapshot.requests)? {
            return Err(ExecutionError::HandoffDigestMismatch);
        }
        Ok(Self {
            path,
            generation: snapshot.generation,
            requests: snapshot.requests,
        })
    }
}

/// Resolve the receipt store associated with one session journal.
///
/// A caller may set [`EXECUTION_STORE_ENV`] to an explicit path (useful for a
/// workspace owner or an integration test).  Otherwise custom session files
/// keep their receipts beside the journal, while the conventional user
/// session uses `~/.zenpi/execution.json`.
pub fn path_for_session(session_path: impl AsRef<Path>) -> PathBuf {
    if let Ok(path) = std::env::var(EXECUTION_STORE_ENV)
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    let session_path = session_path.as_ref();
    let default_session = crate::session::SessionStore::default_path();
    if session_path == default_session {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        return home.join(".zenpi").join(EXECUTION_STORE_FILE_NAME);
    }
    session_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.join(EXECUTION_STORE_FILE_NAME))
        .unwrap_or_else(|| PathBuf::from(EXECUTION_STORE_FILE_NAME))
}

/// Resolve the immutable external-owner request store associated with one
/// session journal. An explicit override keeps host tests and worker adapters
/// from touching the user's global directory.
pub fn handoff_path_for_session(session_path: impl AsRef<Path>) -> PathBuf {
    if let Ok(path) = std::env::var(HANDOFF_STORE_ENV)
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    let session_path = session_path.as_ref();
    let default_session = crate::session::SessionStore::default_path();
    if session_path == default_session {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        return home.join(".zenpi").join(HANDOFF_STORE_FILE_NAME);
    }
    session_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.join(HANDOFF_STORE_FILE_NAME))
        .unwrap_or_else(|| PathBuf::from(HANDOFF_STORE_FILE_NAME))
}

impl ExecutionStore {
    /// Open an existing store or create an empty valid snapshot.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ExecutionError> {
        let path = normalize_path(path.as_ref())?;
        ensure_parent(&path)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(ExecutionError::Symlink(path));
                }
                if !metadata.is_file() {
                    return Err(ExecutionError::Directory(path));
                }
                crate::security::restrict_private_file(&path)?;
                let bytes = read_bounded(&path)?;
                if bytes.iter().all(u8::is_ascii_whitespace) {
                    return Err(ExecutionError::MissingSnapshot);
                }
                Self::decode(path, &bytes)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let store = Self::empty(path);
                store.persist_snapshot()?;
                Ok(store)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Open without creating or changing a missing path.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, ExecutionError> {
        let path = normalize_path(path.as_ref())?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(ExecutionError::Symlink(path));
                }
                if !metadata.is_file() {
                    return Err(ExecutionError::Directory(path));
                }
                let bytes = read_bounded(&path)?;
                Self::decode(path, &bytes)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::empty(path)),
            Err(error) => Err(error.into()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn receipts(&self) -> &[ExecutionReceipt] {
        &self.receipts
    }

    pub fn receipt(&self, execution_id: &str) -> Option<&ExecutionReceipt> {
        self.receipts
            .iter()
            .find(|receipt| receipt.execution_id == execution_id)
    }

    /// Cancel every still-running attempt belonging to one Goal. Completed
    /// and externally evidenced receipts remain immutable. The update is
    /// committed as one bounded snapshot and is idempotent on repeat calls.
    pub fn cancel_running_for_goal(
        &mut self,
        goal_id: &str,
    ) -> Result<ExecutionStoreChange, ExecutionError> {
        bounded_id(goal_id, "goal_id")?;
        let mut next = self.clone();
        let mut changed = false;
        for receipt in &mut next.receipts {
            if receipt.goal_id == goal_id && receipt.status == ExecutionStatus::Running {
                receipt.status = ExecutionStatus::Cancelled;
                receipt.error = Some("cancelled by Goal owner".into());
                receipt.validate()?;
                changed = true;
            }
        }
        if !changed {
            return Ok(ExecutionStoreChange::Unchanged);
        }
        next.commit(ExecutionStoreChange::Updated)?;
        *self = next;
        Ok(ExecutionStoreChange::Updated)
    }

    /// Return the highest-attempt receipt for one item in the exact immutable
    /// Goal/Blueprint execution scope.
    ///
    /// A successful older attempt does not make an item complete after a
    /// newer attempt has failed or been cancelled. Hosts should use this
    /// projection rather than searching for any historical success.
    pub fn latest_receipt_for<'a>(
        &'a self,
        goal: &Goal,
        blueprint: &Blueprint,
        item_id: &str,
    ) -> Option<&'a ExecutionReceipt> {
        self.receipts
            .iter()
            .filter(|receipt| receipt_matches_item(receipt, goal, blueprint, item_id))
            .max_by_key(|receipt| receipt.attempt)
    }

    pub fn digest(&self) -> Result<String, ExecutionError> {
        digest_receipts(&self.receipts, &self.external)
    }

    /// Insert a receipt or replace the same attempt's lifecycle state.  A
    /// different payload for an existing execution ID is rejected, preventing
    /// a retry from silently changing the meaning of a durable operation.
    pub fn upsert_receipt(
        &mut self,
        receipt: ExecutionReceipt,
    ) -> Result<ExecutionStoreChange, ExecutionError> {
        receipt.validate()?;
        if let Some(index) = self
            .receipts
            .iter()
            .position(|existing| existing.execution_id == receipt.execution_id)
        {
            let existing = &self.receipts[index];
            if existing.goal_id != receipt.goal_id
                || existing.blueprint_id != receipt.blueprint_id
                || existing.blueprint_version != receipt.blueprint_version
                || existing.blueprint_digest != receipt.blueprint_digest
                || existing.item_id != receipt.item_id
                || existing.attempt != receipt.attempt
                || existing.cost != receipt.cost
            {
                return Err(ExecutionError::ReceiptConflict {
                    execution_id: receipt.execution_id,
                });
            }
            if existing == &receipt {
                return Ok(ExecutionStoreChange::Unchanged);
            }
            // A receipt is an append-like lifecycle even though its compact
            // snapshot representation is updated in place. Once a terminal
            // state has been observed, a replay cannot reopen it or rewrite
            // its outcome under the same id.
            if existing.status.is_terminal() {
                return Err(ExecutionError::ReceiptConflict {
                    execution_id: receipt.execution_id,
                });
            }
            let mut next = self.clone();
            next.receipts[index] = receipt;
            next.commit(ExecutionStoreChange::Updated)?;
            *self = next;
            return Ok(ExecutionStoreChange::Updated);
        }
        if self
            .receipts
            .iter()
            .any(|existing| same_attempt_identity(existing, &receipt))
        {
            return Err(ExecutionError::ReceiptConflict {
                execution_id: receipt.execution_id,
            });
        }
        if self.receipts.len() >= MAX_EXECUTION_RECEIPTS {
            return Err(ExecutionError::TooManyReceipts {
                max: MAX_EXECUTION_RECEIPTS,
            });
        }
        let mut next = self.clone();
        next.receipts.push(receipt);
        next.commit(ExecutionStoreChange::Inserted)?;
        *self = next;
        Ok(ExecutionStoreChange::Inserted)
    }

    /// Re-read the snapshot from disk, retaining this handle's path.
    pub fn reload(&mut self) -> Result<(), ExecutionError> {
        let replacement = Self::open(&self.path)?;
        *self = replacement;
        Ok(())
    }

    /// A checksum alone is never proof of host acceptance. Kept as a rejecting
    /// compatibility entry point so old callers cannot promote bookkeeping.
    pub fn attach_external_manifest(
        &mut self,
        _execution_id: &str,
        _manifest_checksum: String,
    ) -> Result<ExecutionStoreChange, ExecutionError> {
        Err(ExecutionError::InvalidReceipt(
            "host validators and verified file artifacts are required".into(),
        ))
    }
    /// Importing a declaration only creates a candidate. No worker-controlled
    /// Boolean, string, exit code or checksum can produce accepted work.
    pub fn import_external_manifest(
        &mut self,
        path: impl AsRef<Path>,
        expected_blueprint_digest: &str,
    ) -> Result<ExecutionStoreChange, ExecutionError> {
        let candidate = read_external_candidate(path.as_ref())?;
        self.import_external_candidate(candidate, expected_blueprint_digest)
    }
    pub(crate) fn import_external_candidate(
        &mut self,
        candidate: ExternalCandidate,
        expected_blueprint_digest: &str,
    ) -> Result<ExecutionStoreChange, ExecutionError> {
        let manifest = &candidate.declaration;
        validate_digest(expected_blueprint_digest)?;
        if manifest.blueprint_digest != expected_blueprint_digest {
            return Err(ExecutionError::BlueprintLinkMismatch);
        }
        let receipt = self
            .receipt(&manifest.execution_id)
            .ok_or_else(|| invalid_evidence("execution receipt is missing"))?;
        if receipt.blueprint_digest != manifest.blueprint_digest
            || receipt.item_id != manifest.item_id
            || claim_digest(
                &receipt.goal_id,
                &receipt.blueprint_digest,
                &receipt.item_id,
                receipt.attempt,
            ) != manifest.claim_digest
        {
            return Err(ExecutionError::BlueprintLinkMismatch);
        }
        if !matches!(
            receipt.status,
            ExecutionStatus::Running | ExecutionStatus::Succeeded
        ) {
            return Err(invalid_evidence(
                "terminal failed/cancelled claim cannot import",
            ));
        }
        if self.receipts.iter().any(|r| {
            r.goal_id == receipt.goal_id
                && r.blueprint_digest == receipt.blueprint_digest
                && r.item_id == receipt.item_id
                && r.attempt > receipt.attempt
        }) {
            return Err(invalid_evidence("claim was superseded"));
        }
        let mut record = self
            .external_evidence(&manifest.execution_id)
            .cloned()
            .unwrap_or(ExternalEvidence {
                execution_id: manifest.execution_id.clone(),
                contract: None,
                candidate: None,
                status: EvidenceStatus::Candidate,
                observations: Vec::new(),
                reason: None,
            });
        if let Some(old) = &record.candidate {
            return if old == &candidate {
                Ok(ExecutionStoreChange::Unchanged)
            } else {
                Err(invalid_evidence(
                    "claim replay conflicts with its immutable candidate",
                ))
            };
        }
        if let Some(verified) = &candidate.verifiable {
            let contract = record
                .contract
                .as_ref()
                .ok_or_else(|| invalid_evidence("host preparation must precede worker changes"))?;
            validate_manifest_contract(verified, contract)?;
        }
        record.candidate = Some(candidate);
        record.status = EvidenceStatus::Candidate;
        self.save_external(record)
    }
    pub fn external_evidence(&self, execution_id: &str) -> Option<&ExternalEvidence> {
        self.external
            .iter()
            .find(|r| r.execution_id == execution_id)
    }
    pub fn is_master_accepted(&self, receipt: &ExecutionReceipt) -> bool {
        receipt.status == ExecutionStatus::Succeeded
            && self
                .external_evidence(&receipt.execution_id)
                .is_some_and(|e| {
                    e.status == EvidenceStatus::Accepted
                        && e.contract.is_some()
                        && e.candidate.as_ref().is_some_and(|c| c.verifiable.is_some())
                })
    }
    fn save_external(
        &mut self,
        record: ExternalEvidence,
    ) -> Result<ExecutionStoreChange, ExecutionError> {
        let mut next = self.clone();
        if let Some(index) = next
            .external
            .iter()
            .position(|r| r.execution_id == record.execution_id)
        {
            next.external[index] = record;
        } else {
            next.external.push(record);
        }
        next.commit(ExecutionStoreChange::Updated)?;
        *self = next;
        Ok(ExecutionStoreChange::Updated)
    }

    fn empty(path: PathBuf) -> Self {
        Self {
            path,
            generation: 0,
            receipts: Vec::new(),
            external: Vec::new(),
        }
    }

    fn commit(
        &mut self,
        change: ExecutionStoreChange,
    ) -> Result<ExecutionStoreChange, ExecutionError> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(ExecutionError::BudgetOverflow {
                field: "store_generation",
            })?;
        if let Err(error) = self.persist_snapshot() {
            self.generation = self.generation.saturating_sub(1);
            return Err(error);
        }
        Ok(change)
    }

    fn persist_snapshot(&self) -> Result<(), ExecutionError> {
        let bytes = self.encode_snapshot()?;
        atomic_replace(&self.path, &bytes)
    }

    fn encode_snapshot(&self) -> Result<Vec<u8>, ExecutionError> {
        validate_receipts(&self.receipts)?;
        validate_external_records(&self.receipts, &self.external)?;
        let snapshot = Snapshot {
            schema_version: EXECUTION_SCHEMA_VERSION,
            generation: self.generation,
            digest: digest_receipts(&self.receipts, &self.external)?,
            receipts: self.receipts.clone(),
            external: self.external.clone(),
        };
        let bytes = serde_json::to_vec(&snapshot)?;
        if bytes.len() > MAX_EXECUTION_STORE_BYTES {
            return Err(ExecutionError::StoreTooLong {
                max: MAX_EXECUTION_STORE_BYTES,
                actual: bytes.len() as u64,
            });
        }
        Ok(bytes)
    }

    fn decode(path: PathBuf, bytes: &[u8]) -> Result<Self, ExecutionError> {
        if bytes.len() > MAX_EXECUTION_STORE_BYTES {
            return Err(ExecutionError::StoreTooLong {
                max: MAX_EXECUTION_STORE_BYTES,
                actual: bytes.len() as u64,
            });
        }
        let snapshot: Snapshot = serde_json::from_slice(bytes)?;
        if snapshot.schema_version != EXECUTION_SCHEMA_VERSION {
            return Err(ExecutionError::SchemaVersion {
                found: snapshot.schema_version,
                expected: EXECUTION_SCHEMA_VERSION,
            });
        }
        validate_receipts(&snapshot.receipts)?;
        validate_external_records(&snapshot.receipts, &snapshot.external)?;
        if snapshot.digest != digest_receipts(&snapshot.receipts, &snapshot.external)? {
            return Err(ExecutionError::DigestMismatch);
        }
        Ok(Self {
            path,
            generation: snapshot.generation,
            receipts: snapshot.receipts,
            external: snapshot.external,
        })
    }
}

/// Owner for one bounded local Blueprint step.
#[derive(Debug, Clone)]
pub struct BlueprintExecutor {
    store: ExecutionStore,
}

impl BlueprintExecutor {
    pub fn new(store: ExecutionStore) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &ExecutionStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut ExecutionStore {
        &mut self.store
    }

    pub fn into_store(self) -> ExecutionStore {
        self.store
    }

    /// Persist the resumable receipt that backs an external handoff. This is
    /// idempotent so a host restart between queue and receipt persistence can
    /// repair the admission without allocating a second attempt.
    pub fn ensure_external_pending_receipt(
        &mut self,
        goal: &Goal,
        blueprint: &Blueprint,
        item_id: &str,
        attempt: u32,
    ) -> Result<ExecutionStoreChange, ExecutionError> {
        let Some(item) = blueprint.items.iter().find(|item| item.id == item_id) else {
            return Err(ExecutionError::InvalidReceipt(
                "handoff item is missing".into(),
            ));
        };
        let execution_id = execution_id(goal, blueprint, item, attempt);
        if let Some(existing) = self.store.receipt(&execution_id) {
            if existing.status == ExecutionStatus::Running && !existing.external_work_executed {
                return Ok(ExecutionStoreChange::Unchanged);
            }
            return Err(ExecutionError::ReceiptConflict { execution_id });
        }
        let receipt = make_external_pending_receipt(goal, blueprint, item, attempt)?;
        let used = self.spent_for(goal, blueprint);
        ensure_budget(goal.budget, used, receipt.cost)?;
        self.store.upsert_receipt(receipt)
    }

    /// Admit an external worker claim with a resumable receipt, using bounded
    /// compensation if either independently atomic snapshot fails.
    pub fn admit_external_handoff(
        &mut self,
        handoffs: &mut HandoffStore,
        goal: &Goal,
        blueprint: &Blueprint,
    ) -> Result<HandoffOutcome, ExecutionError> {
        let mut next_executor = self.clone();
        let mut next_handoffs = handoffs.clone();
        let outcome = next_executor.handoff_next(&mut next_handoffs, goal, blueprint)?;
        if let Err(error) = next_executor.ensure_external_pending_receipt(
            goal,
            blueprint,
            &outcome.request.item_id,
            outcome.request.attempt,
        ) {
            let _ = next_handoffs.remove_exact(&outcome.request.handoff_id);
            return Err(error);
        }
        *self = next_executor;
        *handoffs = next_handoffs;
        Ok(outcome)
    }

    /// Admit one dependency-ready declarative task for an external owner.
    ///
    /// This method deliberately does not create an execution receipt or
    /// change Goal status. A retry of the same immutable item/attempt returns
    /// the existing queued request, while the actual worker and acceptance
    /// evidence must be supplied through a separate b3ehive adapter.
    pub fn handoff_next(
        &self,
        handoffs: &mut HandoffStore,
        goal: &Goal,
        blueprint: &Blueprint,
    ) -> Result<HandoffOutcome, ExecutionError> {
        goal.validate_against(blueprint)
            .map_err(|error| match error {
                DomainError::BlueprintLinkMismatch => ExecutionError::BlueprintLinkMismatch,
                other => ExecutionError::Domain(other),
            })?;
        if !matches!(goal.status, GoalStatus::Queued | GoalStatus::Running) {
            return Err(ExecutionError::GoalNotRunnable {
                status: goal.status,
            });
        }
        let Some(selection) = self.select_handoff_item(goal, blueprint)? else {
            // A control-plane receipt is not external acceptance evidence.
            // Refuse to claim a later item is ready until a future importer
            // records that evidence; never turn this into a fake completion.
            return Err(ExecutionError::InvalidReceipt(
                "no dependency-ready Blueprint item; external acceptance evidence is required"
                    .into(),
            ));
        };
        let task =
            selection
                .item
                .task
                .as_ref()
                .ok_or_else(|| ExecutionError::HandoffTaskMissing {
                    item_id: selection.item.id.clone(),
                })?;
        let request =
            make_handoff_request(goal, blueprint, selection.item, selection.attempt, task)?;
        if let Some(existing) = handoffs.find(&request.handoff_id) {
            return Ok(HandoffOutcome {
                request: existing.clone(),
                already_queued: true,
            });
        }
        handoffs.insert(request.clone())?;
        Ok(HandoffOutcome {
            request,
            already_queued: false,
        })
    }

    /// Select and record at most one dependency-ready item.
    ///
    /// The local operation is intentionally deterministic: it creates an
    /// evidence string and no external side effect.  The cancellation closure
    /// is checked before the `running` write and after it; a cancellation
    /// observed after admission becomes a durable `cancelled` terminal receipt.
    pub fn run_next<F>(
        &mut self,
        goal: &Goal,
        blueprint: &Blueprint,
        cancelled: F,
    ) -> Result<RunOutcome, ExecutionError>
    where
        F: Fn() -> bool,
    {
        goal.validate_against(blueprint)
            .map_err(|error| match error {
                DomainError::BlueprintLinkMismatch => ExecutionError::BlueprintLinkMismatch,
                other => ExecutionError::Domain(other),
            })?;
        if !matches!(goal.status, GoalStatus::Queued | GoalStatus::Running) {
            return Err(ExecutionError::GoalNotRunnable {
                status: goal.status,
            });
        }

        let selection = self.select_item(goal, blueprint)?;
        let Some(selection) = selection else {
            if blueprint.items.iter().all(|item| {
                self.latest_receipt(goal, blueprint, &item.id)
                    .is_some_and(|receipt| receipt.status == ExecutionStatus::Succeeded)
            }) {
                return Ok(RunOutcome::Complete);
            }
            let item = blueprint
                .items
                .iter()
                .find(|item| {
                    !self
                        .latest_receipt(goal, blueprint, &item.id)
                        .is_some_and(|receipt| receipt.status == ExecutionStatus::Succeeded)
                })
                .expect("non-empty validated Blueprint has an unresolved item");
            let waiting_on = item
                .depends_on
                .iter()
                .filter(|dependency| {
                    !self
                        .latest_receipt(goal, blueprint, dependency)
                        .is_some_and(|receipt| receipt.status == ExecutionStatus::Succeeded)
                })
                .cloned()
                .collect();
            return Ok(RunOutcome::Blocked {
                item_id: item.id.clone(),
                waiting_on,
            });
        };

        if cancelled() {
            return Ok(RunOutcome::Cancelled {
                item_id: Some(selection.item.id.clone()),
                attempt: Some(selection.attempt),
            });
        }

        let (receipt, resumed_running) = if let Some(running) = selection.running {
            let used = self.spent_for(goal, blueprint);
            ensure_budget(goal.budget, used, ResourceBudget::default())?;
            (running, true)
        } else {
            let cost = deterministic_cost(selection.item);
            let used = self.spent_for(goal, blueprint);
            ensure_budget(goal.budget, used, cost)?;
            let receipt =
                make_running_receipt(goal, blueprint, selection.item, selection.attempt, cost)?;
            self.store.upsert_receipt(receipt.clone())?;
            (receipt, false)
        };

        let terminal = if cancelled() {
            receipt.terminalized(
                ExecutionStatus::Cancelled,
                Some("cancelled before deterministic local evidence commit".into()),
            )
        } else {
            receipt.terminalized(ExecutionStatus::Succeeded, None)
        };
        self.store.upsert_receipt(terminal.clone())?;
        Ok(if terminal.status == ExecutionStatus::Cancelled {
            RunOutcome::Cancelled {
                item_id: Some(terminal.item_id),
                attempt: Some(terminal.attempt),
            }
        } else {
            RunOutcome::Executed {
                receipt: terminal,
                resumed_running,
            }
        })
    }

    fn select_item<'a>(
        &self,
        goal: &Goal,
        blueprint: &'a Blueprint,
    ) -> Result<Option<Selection<'a>>, ExecutionError> {
        for item in &blueprint.items {
            let latest = self.latest_receipt(goal, blueprint, &item.id);
            if latest.is_some_and(|receipt| receipt.status == ExecutionStatus::Succeeded) {
                continue;
            }
            let waiting_on = item
                .depends_on
                .iter()
                .filter(|dependency| {
                    !self
                        .latest_receipt(goal, blueprint, dependency)
                        .is_some_and(|receipt| receipt.status == ExecutionStatus::Succeeded)
                })
                .collect::<Vec<_>>();
            if !waiting_on.is_empty() {
                continue;
            }
            let running = latest.filter(|receipt| receipt.status == ExecutionStatus::Running);
            let attempt = if let Some(running) = running {
                running.attempt
            } else {
                latest.map_or(Ok(1), |receipt| {
                    receipt
                        .attempt
                        .checked_add(1)
                        .ok_or(ExecutionError::BudgetOverflow {
                            field: "execution_attempt",
                        })
                })?
            };
            return Ok(Some(Selection {
                item,
                attempt,
                running: running.cloned(),
            }));
        }
        Ok(None)
    }

    /// Select an item for an external worker only when every dependency has
    /// externally evidenced completion. A local control-plane receipt is
    /// intentionally insufficient for this path.
    fn select_handoff_item<'a>(
        &self,
        goal: &Goal,
        blueprint: &'a Blueprint,
    ) -> Result<Option<Selection<'a>>, ExecutionError> {
        for item in &blueprint.items {
            let latest = self.latest_receipt(goal, blueprint, &item.id);
            if latest.is_some_and(|receipt| self.store.is_master_accepted(receipt)) {
                continue;
            }
            let mut waiting_on = item.depends_on.iter().filter(|dependency| {
                !self
                    .latest_receipt(goal, blueprint, dependency)
                    .is_some_and(|receipt| self.store.is_master_accepted(receipt))
            });
            if waiting_on.next().is_some() {
                continue;
            }
            let running = latest.filter(|receipt| receipt.status == ExecutionStatus::Running);
            let attempt = if let Some(running) = running {
                running.attempt
            } else {
                latest.map_or(Ok(1), |receipt| {
                    receipt
                        .attempt
                        .checked_add(1)
                        .ok_or(ExecutionError::BudgetOverflow {
                            field: "handoff_attempt",
                        })
                })?
            };
            return Ok(Some(Selection {
                item,
                attempt,
                running: running.cloned(),
            }));
        }
        Ok(None)
    }

    fn latest_receipt(
        &self,
        goal: &Goal,
        blueprint: &Blueprint,
        item_id: &str,
    ) -> Option<&ExecutionReceipt> {
        self.store.latest_receipt_for(goal, blueprint, item_id)
    }

    fn spent_for(&self, goal: &Goal, blueprint: &Blueprint) -> ResourceBudget {
        self.store
            .receipts()
            .iter()
            .filter(|receipt| {
                receipt.goal_id == goal.id
                    && receipt.blueprint_id == blueprint.id
                    && receipt.blueprint_version == blueprint.version
                    && receipt.blueprint_digest == blueprint.digest
            })
            .fold(ResourceBudget::default(), |spent, receipt| {
                spent.checked_add(receipt.cost).unwrap_or(ResourceBudget {
                    tokens: u64::MAX,
                    wall_clock_ms: u64::MAX,
                    attempts: u32::MAX,
                    disk_bytes: u64::MAX,
                })
            })
    }
}

struct Selection<'a> {
    item: &'a BlueprintItem,
    attempt: u32,
    running: Option<ExecutionReceipt>,
}

/// Return the fixed accounting cost for the local evidence operation.
pub fn deterministic_cost(item: &BlueprintItem) -> ResourceBudget {
    ResourceBudget {
        tokens: u64::from(item.estimated_loc),
        wall_clock_ms: DETERMINISTIC_WALL_CLOCK_MS,
        attempts: 1,
        disk_bytes: DETERMINISTIC_DISK_BYTES,
    }
}

fn make_running_receipt(
    goal: &Goal,
    blueprint: &Blueprint,
    item: &BlueprintItem,
    attempt: u32,
    cost: ResourceBudget,
) -> Result<ExecutionReceipt, ExecutionError> {
    let execution_id = execution_id(goal, blueprint, item, attempt);
    let receipt = ExecutionReceipt {
        execution_id,
        goal_id: goal.id.clone(),
        blueprint_id: blueprint.id.clone(),
        blueprint_version: blueprint.version.clone(),
        blueprint_digest: blueprint.digest.clone(),
        item_id: item.id.clone(),
        attempt,
        status: ExecutionStatus::Running,
        cost,
        evidence: format!(
            "control_plane_only external_work_executed=false blueprint={} item={} estimated_loc={}",
            blueprint.digest, item.id, item.estimated_loc
        ),
        external_work_executed: false,
        manifest_checksum: None,
        error: None,
    };
    receipt.validate()?;
    Ok(receipt)
}

fn make_external_pending_receipt(
    goal: &Goal,
    blueprint: &Blueprint,
    item: &BlueprintItem,
    attempt: u32,
) -> Result<ExecutionReceipt, ExecutionError> {
    let receipt = ExecutionReceipt {
        execution_id: execution_id(goal, blueprint, item, attempt),
        goal_id: goal.id.clone(),
        blueprint_id: blueprint.id.clone(),
        blueprint_version: blueprint.version.clone(),
        blueprint_digest: blueprint.digest.clone(),
        item_id: item.id.clone(),
        attempt,
        status: ExecutionStatus::Running,
        cost: deterministic_cost(item),
        evidence: "external_owner_pending external_work_executed=false".into(),
        external_work_executed: false,
        manifest_checksum: None,
        error: None,
    };
    receipt.validate()?;
    Ok(receipt)
}

fn make_handoff_request(
    goal: &Goal,
    blueprint: &Blueprint,
    item: &BlueprintItem,
    attempt: u32,
    task: &BlueprintTask,
) -> Result<BlueprintHandoff, ExecutionError> {
    let request = BlueprintHandoff {
        handoff_id: handoff_id(goal, blueprint, item, attempt),
        claim_digest: claim_digest(&goal.id, &blueprint.digest, &item.id, attempt),
        goal_id: goal.id.clone(),
        blueprint_id: blueprint.id.clone(),
        blueprint_version: blueprint.version.clone(),
        blueprint_digest: blueprint.digest.clone(),
        item_id: item.id.clone(),
        attempt,
        depends_on: item.depends_on.clone(),
        estimated_loc: item.estimated_loc,
        instruction: task.instruction.clone(),
        acceptance_commands: task.acceptance_commands.clone(),
        status: HandoffStatus::Queued,
    };
    request.validate()?;
    Ok(request)
}

fn claim_digest(goal_id: &str, blueprint_digest: &str, item_id: &str, attempt: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"zenpi-blueprint-claim-v1\0");
    for part in [goal_id, blueprint_digest, item_id] {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    hasher.update(attempt.to_be_bytes());
    format!("{:x}", hasher.finalize())
}

fn execution_id(goal: &Goal, blueprint: &Blueprint, item: &BlueprintItem, attempt: u32) -> String {
    // The source identifiers are each bounded, but concatenating them would
    // still exceed the receipt identifier limit for otherwise valid records.
    // Hash the tuple instead so retries remain deterministic and compact.
    let mut hasher = Sha256::new();
    hasher.update(goal.id.as_bytes());
    hasher.update([0]);
    hasher.update(blueprint.digest.as_bytes());
    hasher.update([0]);
    hasher.update(item.id.as_bytes());
    hasher.update([0]);
    hasher.update(attempt.to_be_bytes());
    let digest = hasher.finalize();
    let short = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("exec-{short}")
}

fn handoff_id(goal: &Goal, blueprint: &Blueprint, item: &BlueprintItem, attempt: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"handoff\0");
    hasher.update(goal.id.as_bytes());
    hasher.update([0]);
    hasher.update(blueprint.digest.as_bytes());
    hasher.update([0]);
    hasher.update(item.id.as_bytes());
    hasher.update([0]);
    hasher.update(attempt.to_be_bytes());
    let digest = hasher.finalize();
    let short = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("handoff-{short}")
}

fn ensure_budget(
    limit: ResourceBudget,
    used: ResourceBudget,
    requested: ResourceBudget,
) -> Result<(), ExecutionError> {
    let next = used
        .checked_add(requested)
        .ok_or(ExecutionError::BudgetOverflow {
            field: "execution_budget",
        })?;
    for (field, used_value, requested_value, limit_value, next_value) in [
        (
            "tokens",
            used.tokens,
            requested.tokens,
            limit.tokens,
            next.tokens,
        ),
        (
            "wall_clock_ms",
            used.wall_clock_ms,
            requested.wall_clock_ms,
            limit.wall_clock_ms,
            next.wall_clock_ms,
        ),
        (
            "attempts",
            u64::from(used.attempts),
            u64::from(requested.attempts),
            u64::from(limit.attempts),
            u64::from(next.attempts),
        ),
        (
            "disk_bytes",
            used.disk_bytes,
            requested.disk_bytes,
            limit.disk_bytes,
            next.disk_bytes,
        ),
    ] {
        if next_value > limit_value {
            return Err(ExecutionError::BudgetExceeded {
                field,
                used: used_value,
                requested: requested_value,
                limit: limit_value,
            });
        }
    }
    Ok(())
}

fn validate_receipts(receipts: &[ExecutionReceipt]) -> Result<(), ExecutionError> {
    if receipts.len() > MAX_EXECUTION_RECEIPTS {
        return Err(ExecutionError::TooManyReceipts {
            max: MAX_EXECUTION_RECEIPTS,
        });
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut attempts = std::collections::BTreeSet::new();
    for receipt in receipts {
        receipt.validate()?;
        if !ids.insert(receipt.execution_id.as_str()) {
            return Err(ExecutionError::InvalidReceipt(format!(
                "duplicate execution_id {}",
                receipt.execution_id
            )));
        }
        let attempt_identity = (
            receipt.goal_id.as_str(),
            receipt.blueprint_id.as_str(),
            receipt.blueprint_version.as_str(),
            receipt.blueprint_digest.as_str(),
            receipt.item_id.as_str(),
            receipt.attempt,
        );
        if !attempts.insert(attempt_identity) {
            return Err(ExecutionError::InvalidReceipt(format!(
                "duplicate execution attempt for goal {} blueprint {}@{} item {} attempt {}",
                receipt.goal_id,
                receipt.blueprint_id,
                receipt.blueprint_version,
                receipt.item_id,
                receipt.attempt
            )));
        }
    }
    Ok(())
}

fn receipt_matches_item(
    receipt: &ExecutionReceipt,
    goal: &Goal,
    blueprint: &Blueprint,
    item_id: &str,
) -> bool {
    receipt.goal_id == goal.id
        && receipt.blueprint_id == blueprint.id
        && receipt.blueprint_version == blueprint.version
        && receipt.blueprint_digest == blueprint.digest
        && receipt.item_id == item_id
}

fn same_attempt_identity(left: &ExecutionReceipt, right: &ExecutionReceipt) -> bool {
    left.goal_id == right.goal_id
        && left.blueprint_id == right.blueprint_id
        && left.blueprint_version == right.blueprint_version
        && left.blueprint_digest == right.blueprint_digest
        && left.item_id == right.item_id
        && left.attempt == right.attempt
}

fn digest_receipts(
    receipts: &[ExecutionReceipt],
    external: &[ExternalEvidence],
) -> Result<String, ExecutionError> {
    let bytes = serde_json::to_vec(&UnsignedSnapshot { receipts, external })?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn digest_handoffs(requests: &[BlueprintHandoff]) -> Result<String, ExecutionError> {
    let bytes = serde_json::to_vec(&UnsignedHandoffSnapshot { requests })?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn bounded_text(value: &str, field: &'static str, max: usize) -> Result<(), ExecutionError> {
    if value.trim().is_empty() {
        return Err(ExecutionError::InvalidReceipt(format!(
            "{field} must be non-empty"
        )));
    }
    if value.len() > max || value.chars().any(char::is_control) {
        return Err(ExecutionError::InvalidReceipt(format!(
            "{field} is outside its bounded text policy"
        )));
    }
    Ok(())
}

fn bounded_id(value: &str, field: &'static str) -> Result<(), ExecutionError> {
    bounded_text(value, field, 256)?;
    if value
        .chars()
        .any(|character| !(character.is_ascii_alphanumeric() || "._:/-".contains(character)))
    {
        return Err(ExecutionError::InvalidReceipt(format!(
            "{field} is not a valid identifier"
        )));
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<(), ExecutionError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ExecutionError::InvalidReceipt(
            "blueprint_digest is not a lowercase SHA-256 value".into(),
        ));
    }
    Ok(())
}

fn normalize_path(path: &Path) -> Result<PathBuf, ExecutionError> {
    if path.as_os_str().is_empty() {
        return Err(ExecutionError::EmptyPath);
    }
    Ok(path.to_path_buf())
}

fn ensure_parent(path: &Path) -> Result<(), ExecutionError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    if !parent.is_dir() {
        return Err(ExecutionError::Directory(parent.to_path_buf()));
    }
    Ok(())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, ExecutionError> {
    read_bounded_with_limit(path, MAX_EXECUTION_STORE_BYTES)
}

fn read_bounded_with_limit(path: &Path, max_bytes: usize) -> Result<Vec<u8>, ExecutionError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if metadata.len() > max_bytes as u64 {
        return Err(ExecutionError::StoreTooLong {
            max: max_bytes,
            actual: metadata.len(),
        });
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((max_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(ExecutionError::StoreTooLong {
            max: max_bytes,
            actual: bytes.len() as u64,
        });
    }
    Ok(bytes)
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), ExecutionError> {
    atomic_replace_with_limit(path, bytes, MAX_EXECUTION_STORE_BYTES)
}

fn atomic_replace_with_limit(
    path: &Path,
    bytes: &[u8],
    max_bytes: usize,
) -> Result<(), ExecutionError> {
    if bytes.len() > max_bytes {
        return Err(ExecutionError::StoreTooLong {
            max: max_bytes,
            actual: bytes.len() as u64,
        });
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    ensure_parent(path)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(ExecutionError::Symlink(path.to_path_buf()));
        }
        if !metadata.is_file() {
            return Err(ExecutionError::Directory(path.to_path_buf()));
        }
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ExecutionError::EmptyPath)?;
    let temporary = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        now_ms()
    ));
    if let Ok(metadata) = fs::symlink_metadata(&temporary) {
        if metadata.file_type().is_symlink() {
            return Err(ExecutionError::Symlink(temporary));
        }
        return Err(ExecutionError::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "execution store temporary file already exists",
        )));
    }
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if let Ok(metadata) = fs::symlink_metadata(path)
            && metadata.file_type().is_symlink()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "execution store destination became a symbolic link",
            ));
        }
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok::<(), io::Error>(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

/// Host-verifiable output is a separate state from a worker's declaration.
/// All records stay in the existing execution snapshot and session owner.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileEvidence {
    pub path: String,
    pub bytes: u64,
    pub mode: u32,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileChange {
    pub path: String,
    pub before: Option<FileEvidence>,
    pub after: Option<FileEvidence>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorktreeEvidence {
    /// SHA256 of the bounded canonical inventory, not a Git commit ID.
    pub revision: String,
    pub files: Vec<FileEvidence>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ValidatorSpec {
    pub id: String,
    pub argv: Vec<String>,
    pub timeout_ms: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExternalPreparation {
    pub claim_digest: String,
    pub lease_id: String,
    pub policy_digest: String,
    pub owned_paths: Vec<String>,
    pub validator_timeout_ms: u64,
    pub policy: crate::tools::BlueprintPolicySpec,
    pub lease: crate::tools::BlueprintLease,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExternalContract {
    pub session_id: String,
    pub execution_id: String,
    pub claim_digest: String,
    pub lease_id: String,
    pub policy_digest: String,
    pub workspace: PathBuf,
    pub owned_paths: Vec<String>,
    /// Explicit host control files excluded from product inventory. These
    /// are derived by the host; worker manifests cannot add exclusions.
    pub protected_paths: Vec<PathBuf>,
    pub baseline: WorktreeEvidence,
    pub validators: Vec<ValidatorSpec>,
    pub prepared_at_ms: u64,
    pub policy: crate::tools::BlueprintPolicySpec,
    pub lease: crate::tools::BlueprintLease,
    pub digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VerifiableResultManifest {
    pub schema_version: u16,
    pub declaration: ExternalResultManifest,
    pub contract_digest: String,
    pub lease_id: String,
    pub policy_digest: String,
    pub baseline_revision: String,
    pub output_revision: String,
    pub diff_sha256: String,
    pub changes: Vec<FileChange>,
    pub validator_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExternalCandidate {
    pub checksum: String,
    pub declaration: ExternalResultManifest,
    pub verifiable: Option<VerifiableResultManifest>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Prepared,
    Candidate,
    Validating,
    Rejected,
    Cancelled,
    Accepted,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ValidatorObservation {
    pub validator_id: String,
    pub operation_id: String,
    pub argv: Vec<String>,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub cancelled: bool,
    pub timed_out: bool,
    pub child_reaped: bool,
    pub output: Vec<crate::tool_output::ArtifactRef>,
    pub capture_errors: Vec<String>,
    pub observed_at_ms: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExternalEvidence {
    pub execution_id: String,
    pub contract: Option<ExternalContract>,
    pub candidate: Option<ExternalCandidate>,
    pub status: EvidenceStatus,
    pub observations: Vec<ValidatorObservation>,
    pub reason: Option<String>,
}

fn invalid_evidence(reason: &str) -> ExecutionError {
    ExecutionError::InvalidReceipt(reason.into())
}
fn evidence_hash<T: Serialize>(value: &T) -> Result<String, ExecutionError> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}
pub fn read_external_candidate(path: &Path) -> Result<ExternalCandidate, ExecutionError> {
    let bytes = read_bounded_with_limit(path, MAX_DOMAIN_RECORD_BYTES)?;
    parse_external_candidate(&bytes)
}
pub(crate) fn parse_external_candidate(bytes: &[u8]) -> Result<ExternalCandidate, ExecutionError> {
    if bytes.len() > MAX_DOMAIN_RECORD_BYTES {
        return Err(invalid_evidence("external manifest exceeds byte limit"));
    }
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let (declaration, verifiable) = if value.get("schema_version").is_some() {
        let verified: VerifiableResultManifest = serde_json::from_value(value)?;
        if verified.schema_version != 2 {
            return Err(invalid_evidence("unsupported external manifest version"));
        }
        (verified.declaration.clone(), Some(verified))
    } else {
        (
            serde_json::from_value::<ExternalResultManifest>(value)?,
            None,
        )
    };
    bounded_id(&declaration.execution_id, "execution_id")?;
    bounded_id(&declaration.item_id, "item_id")?;
    validate_digest(&declaration.claim_digest)?;
    validate_digest(&declaration.blueprint_digest)?;
    if declaration.status != ExecutionStatus::Succeeded
        || !declaration.acceptance_passed
        || declaration.acceptance_evidence.is_empty()
        || declaration.acceptance_evidence.len() > 8
    {
        return Err(invalid_evidence(
            "worker declaration is not a bounded successful candidate",
        ));
    }
    for entry in &declaration.acceptance_evidence {
        bounded_text(entry, "worker_evidence", 4096)?;
    }
    Ok(ExternalCandidate {
        checksum: format!("{:x}", Sha256::digest(bytes)),
        declaration,
        verifiable,
    })
}
fn validate_manifest_contract(
    m: &VerifiableResultManifest,
    c: &ExternalContract,
) -> Result<(), ExecutionError> {
    if m.contract_digest != c.digest
        || m.declaration.claim_digest != c.claim_digest
        || m.declaration.execution_id != c.execution_id
        || m.lease_id != c.lease_id
        || m.policy_digest != c.policy_digest
        || m.baseline_revision != c.baseline.revision
        || m.validator_ids
            != c.validators
                .iter()
                .map(|v| v.id.clone())
                .collect::<Vec<_>>()
    {
        return Err(invalid_evidence(
            "manifest differs from the frozen host contract",
        ));
    }
    validate_digest(&m.output_revision)?;
    validate_digest(&m.diff_sha256)?;
    if m.changes.is_empty() || m.changes.len() > 64 || evidence_hash(&m.changes)? != m.diff_sha256 {
        return Err(invalid_evidence("manifest diff is missing or corrupt"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for change in &m.changes {
        evidence_relative_path(&change.path)?;
        if !seen.insert(&change.path)
            || !c.owned_paths.contains(&change.path)
            || change.before == change.after
            || (change.before.is_none() && change.after.is_none())
        {
            return Err(invalid_evidence(
                "manifest diff is duplicate, unchanged or outside owned files",
            ));
        }
        if change.before.as_ref() != c.baseline.files.iter().find(|f| f.path == change.path) {
            return Err(invalid_evidence(
                "manifest input bytes/hash differ from baseline",
            ));
        }
        for file in [&change.before, &change.after].into_iter().flatten() {
            if file.path != change.path || file.bytes > 8 * 1024 * 1024 {
                return Err(invalid_evidence("invalid file artifact"));
            }
            validate_digest(&file.sha256)?;
        }
    }
    Ok(())
}
fn evidence_relative_path(path: &str) -> Result<(), ExecutionError> {
    if path.is_empty()
        || path.len() > 4096
        || path.chars().any(char::is_control)
        || path.contains('\\')
        || Path::new(path).is_absolute()
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        || path
            .split('/')
            .any(|c| c.is_empty() || c == ".git" || c.starts_with(".zenpi-output-"))
    {
        return Err(invalid_evidence(
            "artifact path must be a normalized product file",
        ));
    }
    Ok(())
}
#[cfg(unix)]
fn evidence_open(root: &Path, relative: &Path) -> Result<File, ExecutionError> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    };
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut directory = options.open(root)?;
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let std::path::Component::Normal(name) = component else {
            return Err(invalid_evidence("unsafe artifact component"));
        };
        let name = std::ffi::CString::new(name.as_bytes())
            .map_err(|_| invalid_evidence("artifact NUL"))?;
        let last = index + 1 == components.len();
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if last { 0 } else { libc::O_DIRECTORY };
        // Each component is opened relative to a pinned no-follow parent.
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(io::Error::last_os_error().into());
        }
        directory = unsafe { File::from_raw_fd(fd) };
        if last {
            let metadata = directory.metadata()?;
            if !metadata.is_file() || metadata.nlink() != 1 {
                return Err(invalid_evidence(
                    "artifact is not a singly-linked regular file",
                ));
            }
        }
    }
    Ok(directory)
}
#[cfg(not(unix))]
fn evidence_open(_root: &Path, _relative: &Path) -> Result<File, ExecutionError> {
    Err(invalid_evidence(
        "verified artifact containment requires Unix",
    ))
}
/// Bounded content revision of product files. Git metadata and exact host
/// control files are excluded; arbitrary symlinks and hardlinks fail closed.
pub fn inspect_external_worktree(
    root: &Path,
    protected: &[PathBuf],
    cancelled: &dyn Fn() -> bool,
) -> Result<WorktreeEvidence, ExecutionError> {
    let root = fs::canonicalize(root)?;
    let mut queue = vec![PathBuf::new()];
    let mut files = Vec::new();
    let mut total = 0u64;
    let mut entries = 0;
    while let Some(relative) = queue.pop() {
        if cancelled() {
            return Err(invalid_evidence("external verification cancelled"));
        }
        if relative.components().count() > 64 {
            return Err(invalid_evidence("artifact tree depth exceeded"));
        }
        for entry in fs::read_dir(root.join(&relative))? {
            entries += 1;
            if entries > 8192 {
                return Err(invalid_evidence("artifact inventory exceeds8192 entries"));
            }
            let entry = entry?;
            let relative = relative.join(entry.file_name());
            let full = root.join(&relative);
            if relative == Path::new(".git")
                || protected
                    .iter()
                    .any(|p| full == *p || full.starts_with(p) && p.is_dir())
            {
                continue;
            }
            let metadata = entry.file_type()?;
            if metadata.is_symlink() {
                return Err(invalid_evidence("symlink in product inventory"));
            }
            if metadata.is_dir() {
                queue.push(relative);
                continue;
            }
            if files.len() >= 4096 {
                return Err(invalid_evidence("artifact inventory exceeds4096 files"));
            }
            let path = relative
                .to_str()
                .ok_or_else(|| invalid_evidence("non-UTF8 artifact path"))?
                .to_owned();
            evidence_relative_path(&path)?;
            let mut file = evidence_open(&root, &relative)?;
            let declared = file.metadata()?.len();
            if declared > 8 * 1024 * 1024 {
                return Err(invalid_evidence("artifact exceeds8MiB"));
            }
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 8192];
            let mut bytes = 0;
            loop {
                if cancelled() {
                    return Err(invalid_evidence("external verification cancelled"));
                }
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                bytes += n as u64;
                total += n as u64;
                if bytes > 8 * 1024 * 1024 || total > 64 * 1024 * 1024 {
                    return Err(invalid_evidence("artifact inventory exceeds byte budget"));
                }
                hash.update(&buffer[..n]);
            }
            if bytes != declared || file.metadata()?.len() != bytes {
                return Err(invalid_evidence("artifact changed during read"));
            }
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                file.metadata()?.permissions().mode() & 0o777
            };
            #[cfg(not(unix))]
            let mode = 0;
            files.push(FileEvidence {
                path,
                bytes,
                mode,
                sha256: format!("{:x}", hash.finalize()),
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(WorktreeEvidence {
        revision: evidence_hash(&files)?,
        files,
    })
}
pub fn external_file_diff(before: &WorktreeEvidence, after: &WorktreeEvidence) -> Vec<FileChange> {
    let keys: std::collections::BTreeSet<_> = before
        .files
        .iter()
        .chain(&after.files)
        .map(|f| f.path.clone())
        .collect();
    keys.into_iter()
        .filter_map(|path| {
            let old = before.files.iter().find(|f| f.path == path).cloned();
            let new = after.files.iter().find(|f| f.path == path).cloned();
            (old != new).then_some(FileChange {
                path,
                before: old,
                after: new,
            })
        })
        .collect()
}

impl ExecutionStore {
    /// Freeze authority and input bytes before the external worker changes files.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_external_evidence(
        &mut self,
        session: &crate::session::SessionStore,
        ledger: &crate::governance::WorkerBudgetLedger,
        handoff: &BlueprintHandoff,
        goal: &Goal,
        blueprint: &Blueprint,
        workspace: &Path,
        request: &ExternalPreparation,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ExternalEvidence, ExecutionError> {
        handoff.validate_against(goal, blueprint)?;
        if request.claim_digest != handoff.claim_digest
            || request.policy.blueprint_digest != blueprint.digest
            || request.policy.item_id != handoff.item_id
            || request.lease_id != request.lease.lease_id
        {
            return Err(invalid_evidence(
                "preparation authority differs from existing claim",
            ));
        }
        let now = now_ms();
        let lease = ledger
            .active_lease(
                &request.lease_id,
                &request.policy_digest,
                &handoff.item_id,
                now,
            )
            .map_err(|e| invalid_evidence(&e.to_string()))?;
        if request.validator_timeout_ms == 0
            || request.validator_timeout_ms > 120_000
            || request.owned_paths.is_empty()
            || request.owned_paths.len() > 64
            || request.lease.expires_at_ms > lease.expires_at_ms
        {
            return Err(invalid_evidence("preparation exceeds bounds"));
        }
        let workspace = fs::canonicalize(workspace)?;
        let context = crate::tools::ToolContext::new(&workspace)
            .map_err(|e| invalid_evidence(&e.to_string()))?;
        let (gate, _) = crate::tools::BlueprintGate::compile(
            &context,
            request.policy.clone(),
            request.lease.clone(),
            now,
        )
        .map_err(|e| invalid_evidence(&e.to_string()))?;
        if gate.evidence().policy_digest != request.policy_digest {
            return Err(invalid_evidence(
                "supplied policy differs from host lease digest",
            ));
        }
        let context = context
            .with_blueprint_gate(gate)
            .map_err(|e| invalid_evidence(&e.to_string()))?;
        let mut owned = request.owned_paths.clone();
        owned.sort();
        owned.dedup();
        if owned.len() != request.owned_paths.len() {
            return Err(invalid_evidence("duplicate owned paths"));
        }
        for path in &owned {
            evidence_relative_path(path)?;
            context
                .check_call_gate(
                    "write_file",
                    crate::tools::ToolSideEffect::WorkspaceWrite,
                    serde_json::json!({"path":path}).as_object().unwrap(),
                )
                .map_err(|e| invalid_evidence(&e.to_string()))?;
        }
        let receipt = self
            .latest_receipt_for(goal, blueprint, &handoff.item_id)
            .ok_or_else(|| invalid_evidence("claim has no receipt"))?;
        if receipt.attempt != handoff.attempt
            || receipt.status != ExecutionStatus::Running
            || receipt.evidence != "external_owner_pending external_work_executed=false"
        {
            return Err(invalid_evidence(
                "preparation requires a pending external claim, never control_plane_only",
            ));
        }
        let execution_id = receipt.execution_id.clone();
        if let Some(existing) = self.external_evidence(&execution_id) {
            return if existing.contract.as_ref().is_some_and(|c| {
                c.claim_digest == request.claim_digest
                    && c.owned_paths == owned
                    && c.lease_id == request.lease_id
                    && c.policy_digest == request.policy_digest
                    && c.workspace == workspace
                    && c.validators
                        .iter()
                        .all(|v| v.timeout_ms == request.validator_timeout_ms)
            }) {
                Ok(existing.clone())
            } else {
                Err(invalid_evidence("external preparation cannot be rebound"))
            };
        }
        let journal = fs::canonicalize(session.path())?;
        let parent = journal
            .parent()
            .ok_or_else(|| invalid_evidence("journal has no parent"))?;
        let mut protected = vec![
            journal.clone(),
            fs::canonicalize(&self.path)?,
            absolute_control_path(&handoff_path_for_session(&journal))?,
            absolute_control_path(&crate::domain_store::path_for_session(&journal))?,
            parent.join(format!(
                ".zenpi-output-{:x}",
                Sha256::digest(session.session_id().as_bytes())
            )),
            workspace
                .join(".zenpi-results")
                .join(format!("{}.json", request.claim_digest)),
        ];
        protected.sort();
        protected.dedup();
        let result_path = workspace
            .join(".zenpi-results")
            .join(format!("{}.json", request.claim_digest));
        for path in &protected {
            if *path == result_path {
                continue;
            }
            if let Ok(relative) = path.strip_prefix(&workspace) {
                let args = serde_json::json!({"path":relative.to_string_lossy()});
                if context
                    .check_call_gate(
                        "write_file",
                        crate::tools::ToolSideEffect::WorkspaceWrite,
                        args.as_object().unwrap(),
                    )
                    .is_ok()
                {
                    return Err(invalid_evidence(
                        "worker policy permits writing host control state",
                    ));
                }
            }
        }
        for path in &owned {
            let full = workspace.join(path);
            if protected.iter().any(|p| full == *p || full.starts_with(p)) {
                return Err(invalid_evidence(
                    "product ownership overlaps host control files",
                ));
            }
        }
        let baseline = inspect_external_worktree(&workspace, &protected, cancelled)?;
        let validators = handoff
            .acceptance_commands
            .iter()
            .enumerate()
            .map(|(index, command)| {
                if command.trim().is_empty() || command.starts_with("external-owner:") {
                    return Err(invalid_evidence(
                        "placeholder acceptance command is not a validator",
                    ));
                }
                let argv = vec!["/bin/sh".into(), "-c".into(), command.clone()];
                Ok(ValidatorSpec {
                    id: format!("validator-{index}-{}", &evidence_hash(&argv)?[..16]),
                    argv,
                    timeout_ms: request.validator_timeout_ms,
                })
            })
            .collect::<Result<Vec<_>, ExecutionError>>()?;
        if cancelled() {
            return Err(invalid_evidence("external preparation cancelled"));
        }
        let mut contract = ExternalContract {
            session_id: session.session_id().into(),
            execution_id: execution_id.clone(),
            claim_digest: request.claim_digest.clone(),
            lease_id: request.lease_id.clone(),
            policy_digest: request.policy_digest.clone(),
            workspace,
            owned_paths: owned,
            protected_paths: protected,
            baseline,
            validators,
            prepared_at_ms: now,
            policy: request.policy.clone(),
            lease: request.lease.clone(),
            digest: String::new(),
        };
        contract.digest = evidence_hash(&contract)?;
        let record = ExternalEvidence {
            execution_id,
            contract: Some(contract),
            candidate: None,
            status: EvidenceStatus::Prepared,
            observations: Vec::new(),
            reason: None,
        };
        self.save_external(record.clone())?;
        Ok(record)
    }
}
fn absolute_control_path(path: &Path) -> Result<PathBuf, ExecutionError> {
    if path.exists() {
        Ok(fs::canonicalize(path)?)
    } else {
        let parent = path.parent().unwrap_or(Path::new("."));
        Ok(fs::canonicalize(parent)?.join(
            path.file_name()
                .ok_or_else(|| invalid_evidence("invalid control path"))?,
        ))
    }
}

impl ExecutionStore {
    /// Execute only the host-frozen validators. The candidate contains no
    /// executable commands or host observations. SessionStore excludes writers.
    #[allow(clippy::too_many_arguments)]
    pub fn accept_external_candidate(
        &mut self,
        session: &mut crate::session::SessionStore,
        ledger: &mut crate::governance::WorkerBudgetLedger,
        output: &mut crate::tool_output::SessionOutputStore,
        handoff: &BlueprintHandoff,
        goal: &Goal,
        blueprint: &Blueprint,
        workspace: &Path,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ExternalEvidence, ExecutionError> {
        use crate::governance::{BudgetCompletion, BudgetOrigin, BudgetReservation, ResourceUsage};
        use crate::session::{InterruptedOperation, OperationKind, OperationOutcome};
        handoff.validate_against(goal, blueprint)?;
        let receipt = self
            .latest_receipt_for(goal, blueprint, &handoff.item_id)
            .ok_or_else(|| invalid_evidence("claim receipt missing"))?;
        if receipt.attempt != handoff.attempt {
            return Err(invalid_evidence("stale claim attempt"));
        }
        let mut record = self
            .external_evidence(&receipt.execution_id)
            .cloned()
            .ok_or_else(|| invalid_evidence("candidate missing"))?;
        let contract = record
            .contract
            .clone()
            .ok_or_else(|| invalid_evidence("legacy declaration is candidate/history only"))?;
        // A replay skips validator execution, but still belongs to the exact
        // session and workspace that produced the durable acceptance.
        if contract.session_id != session.session_id()
            || contract.claim_digest != handoff.claim_digest
            || contract.workspace != fs::canonicalize(workspace)?
        {
            return Err(invalid_evidence(
                "acceptance belongs to another session/worktree",
            ));
        }
        if record.status == EvidenceStatus::Accepted {
            return Ok(record);
        }
        if record.status == EvidenceStatus::Validating {
            return Err(invalid_evidence(
                "validator outcome unknown; explicit reconcile required, no automatic replay",
            ));
        }
        if record.status != EvidenceStatus::Candidate
            || receipt.status != ExecutionStatus::Running
            || receipt.evidence != "external_owner_pending external_work_executed=false"
        {
            return Err(invalid_evidence(
                "only an external candidate can be accepted; control_plane_only is insufficient",
            ));
        }
        let candidate = record
            .candidate
            .clone()
            .ok_or_else(|| invalid_evidence("candidate missing"))?;
        let manifest = candidate
            .verifiable
            .as_ref()
            .ok_or_else(|| invalid_evidence("file artifact manifest required"))?;
        validate_manifest_contract(manifest, &contract)?;
        if !matches!(goal.status, GoalStatus::Queued | GoalStatus::Running) {
            return Err(invalid_evidence("goal is not accepting external work"));
        }
        ledger
            .active_lease(
                &contract.lease_id,
                &contract.policy_digest,
                &handoff.item_id,
                now_ms(),
            )
            .map_err(|e| invalid_evidence(&e.to_string()))?;
        ledger
            .require_settled_item(&contract.lease_id)
            .map_err(|e| invalid_evidence(&e.to_string()))?;
        let expected_commands: Vec<_> = contract
            .validators
            .iter()
            .map(|v| v.argv.get(2).cloned().unwrap_or_default())
            .collect();
        if expected_commands != handoff.acceptance_commands {
            return Err(invalid_evidence(
                "validator contract no longer matches Blueprint",
            ));
        }
        if cancelled() {
            return self.external_terminal(
                record,
                EvidenceStatus::Cancelled,
                "cancelled before validation",
            );
        }
        let current =
            inspect_external_worktree(&contract.workspace, &contract.protected_paths, cancelled)?;
        if current.revision != manifest.output_revision
            || external_file_diff(&contract.baseline, &current) != manifest.changes
        {
            return Err(invalid_evidence(
                "actual artifact bytes/hash/revision/diff differ from candidate",
            ));
        }
        record.status = EvidenceStatus::Validating;
        self.save_external(record.clone())?;
        for (index, validator) in contract.validators.iter().enumerate() {
            if cancelled() {
                return self.external_terminal(
                    record,
                    EvidenceStatus::Cancelled,
                    "cancelled before validator dispatch",
                );
            }
            let lease = ledger
                .active_lease(
                    &contract.lease_id,
                    &contract.policy_digest,
                    &handoff.item_id,
                    now_ms(),
                )
                .map_err(|e| invalid_evidence(&e.to_string()))?;
            let expires = lease.expires_at_ms;
            let operation = format!("validator-{}-{index}", &candidate.checksum[..32]);
            let reservation = BudgetReservation {
                operation_id: operation.clone(),
                lease_id: contract.lease_id.clone(),
                policy_digest: contract.policy_digest.clone(),
                origin: BudgetOrigin::AgentTool,
                resources: ResourceUsage {
                    wall_ms: validator.timeout_ms + 1000,
                    processes: 1,
                    concurrency: 1,
                    disk_bytes: crate::tool_output::command_disk_reservation(
                        crate::tool_output::OutputLimits::default(),
                    ),
                    ..Default::default()
                },
                gate_decision_id: validator.id.clone(),
                network_host: None,
                credential_handles: Vec::new(),
            };
            if let Err(error) = ledger.reserve(session, reservation, now_ms()) {
                return self.external_terminal(
                    record,
                    EvidenceStatus::Rejected,
                    &error.to_string(),
                );
            }
            session
                .begin_operation(&InterruptedOperation {
                    operation_id: operation.clone(),
                    kind: OperationKind::Tool,
                    turn_id: operation.clone(),
                    retry_requires_confirmation: true,
                })
                .map_err(|e| invalid_evidence(&e.to_string()))?;
            session.append_event(serde_json::json!({"type":"tool_execution_started","operation_id":operation,"turn_id":operation,"call_id":operation,"tool":"run_command","validator_id":validator.id,"argv":validator.argv,"contract_digest":contract.digest})).map_err(|e|invalid_evidence(&e.to_string()))?;
            let now = now_ms();
            let store = output
                .store(session.path(), session.session_id(), now)
                .map_err(|e| invalid_evidence(&e.to_string()))?;
            let capture = crate::tool_output::CommandOutputCapture::new(
                store,
                session.session_id(),
                &operation,
                now,
            )
            .map_err(|e| invalid_evidence(&e.to_string()))?;
            let context = crate::tools::ToolContext::new(&contract.workspace)
                .map_err(|e| invalid_evidence(&e.to_string()))?
                .with_origin(crate::tools::ToolOrigin::UserShell)
                .with_output_capture(capture.clone())
                .map_err(|e| invalid_evidence(&e.to_string()))?;
            let arguments =
                serde_json::json!({"command":validator.argv[2],"timeout_ms":validator.timeout_ms});
            let result = crate::tools::RunCommandTool::invoke_user_shell_with_cancel(
                &context,
                arguments.as_object().unwrap(),
                &|| cancelled() || now_ms() >= expires,
            );
            let value = result.as_ref().ok();
            let flag = |name: &str| value.and_then(|v| v[name].as_bool()).unwrap_or(false);
            let observation = ValidatorObservation {
                validator_id: validator.id.clone(),
                operation_id: operation.clone(),
                argv: validator.argv.clone(),
                exit_code: value
                    .and_then(|v| v["exit_code"].as_i64())
                    .map(|v| v as i32),
                signal: value.and_then(|v| v["signal"].as_i64()).map(|v| v as i32),
                cancelled: flag("cancelled") || cancelled() || now_ms() >= expires,
                timed_out: flag("timed_out"),
                child_reaped: flag("child_reaped"),
                output: capture.artifacts(),
                capture_errors: capture.errors(),
                observed_at_ms: now_ms(),
            };
            let successful = result.is_ok()
                && observation.exit_code == Some(0)
                && observation.signal.is_none()
                && !observation.cancelled
                && !observation.timed_out
                && observation.child_reaped
                && observation.output.len() == 2
                && observation.output.iter().all(|r| r.finalized && r.complete)
                && observation.capture_errors.is_empty();
            session.append_event(serde_json::json!({"type":"tool_output_captured","operation_id":operation,"turn_id":operation,"call_id":operation,"output_capture":{"artifacts":observation.output,"errors":observation.capture_errors}})).map_err(|e|invalid_evidence(&e.to_string()))?;
            session.append_event(serde_json::json!({"type":"external_validator_observed","claim_digest":contract.claim_digest,"candidate_checksum":candidate.checksum,"contract_digest":contract.digest,"observation":observation})).map_err(|e|invalid_evidence(&e.to_string()))?;
            record.observations.push(observation.clone());
            self.save_external(record.clone())?;
            let completion = if observation.cancelled {
                BudgetCompletion::Cancelled
            } else if successful {
                BudgetCompletion::Completed
            } else {
                BudgetCompletion::Failed
            };
            let outcome = if observation.cancelled {
                OperationOutcome::Cancelled
            } else if successful {
                OperationOutcome::Succeeded
            } else {
                OperationOutcome::Failed
            };
            session
                .finish_operation(&operation, outcome)
                .map_err(|e| invalid_evidence(&e.to_string()))?;
            let directive = ledger
                .settle(
                    session,
                    &operation,
                    ResourceUsage {
                        wall_ms: now_ms().saturating_sub(now),
                        processes: 1,
                        disk_bytes: observation
                            .output
                            .iter()
                            .map(|r| r.bytes_stored + 8192)
                            .sum(),
                        ..Default::default()
                    },
                    completion,
                    now_ms(),
                )
                .map_err(|e| invalid_evidence(&e.to_string()))?;
            if !successful || directive.is_some() {
                return self.external_terminal(
                    record,
                    if observation.cancelled {
                        EvidenceStatus::Cancelled
                    } else {
                        EvidenceStatus::Rejected
                    },
                    "host validator failed, was cancelled, or exceeded budget",
                );
            }
        }
        if cancelled() {
            return self.external_terminal(
                record,
                EvidenceStatus::Cancelled,
                "cancelled before acceptance commit",
            );
        }
        ledger
            .active_lease(
                &contract.lease_id,
                &contract.policy_digest,
                &handoff.item_id,
                now_ms(),
            )
            .map_err(|e| invalid_evidence(&e.to_string()))?;
        if inspect_external_worktree(&contract.workspace, &contract.protected_paths, cancelled)?
            != current
        {
            return self.external_terminal(
                record,
                EvidenceStatus::Rejected,
                "worktree changed during validator execution",
            );
        }
        record.status = EvidenceStatus::Accepted;
        record.reason = None;
        let mut next = self.clone();
        let index = next
            .external
            .iter()
            .position(|r| r.execution_id == record.execution_id)
            .unwrap();
        next.external[index] = record.clone();
        let receipt = next
            .receipts
            .iter_mut()
            .find(|r| r.execution_id == record.execution_id)
            .unwrap();
        receipt.status = ExecutionStatus::Succeeded;
        receipt.external_work_executed = true;
        receipt.manifest_checksum = Some(candidate.checksum.clone());
        receipt.evidence = format!(
            "host_verified validators={} candidate={}",
            record.observations.len(),
            candidate.checksum
        );
        next.commit(ExecutionStoreChange::Updated)?;
        *self = next;
        session.append_event(serde_json::json!({"type":"external_candidate_accepted","execution_id":record.execution_id,"claim_digest":contract.claim_digest,"candidate_checksum":candidate.checksum,"output_revision":manifest.output_revision})).map_err(|e|invalid_evidence(&e.to_string()))?;
        Ok(record)
    }
    fn external_terminal(
        &mut self,
        mut record: ExternalEvidence,
        status: EvidenceStatus,
        reason: &str,
    ) -> Result<ExternalEvidence, ExecutionError> {
        record.status = status;
        record.reason = Some(crate::security::redact_text(reason, &[]));
        self.save_external(record.clone())?;
        Ok(record)
    }
}

fn validate_external_records(
    receipts: &[ExecutionReceipt],
    records: &[ExternalEvidence],
) -> Result<(), ExecutionError> {
    if records.len() > receipts.len() {
        return Err(invalid_evidence("too many external evidence records"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for record in records {
        if !seen.insert(&record.execution_id) {
            return Err(invalid_evidence("duplicate evidence identity"));
        }
        let receipt = receipts
            .iter()
            .find(|r| r.execution_id == record.execution_id)
            .ok_or_else(|| invalid_evidence("evidence has no existing receipt"))?;
        if record.observations.len() > 8 {
            return Err(invalid_evidence("too many validator observations"));
        }
        if let Some(contract) = &record.contract {
            let mut unsigned = contract.clone();
            unsigned.digest.clear();
            if evidence_hash(&unsigned)? != contract.digest
                || contract.execution_id != record.execution_id
                || contract.claim_digest
                    != claim_digest(
                        &receipt.goal_id,
                        &receipt.blueprint_digest,
                        &receipt.item_id,
                        receipt.attempt,
                    )
                || contract.validators.is_empty()
                || contract.validators.len() > 8
                || contract.owned_paths.is_empty()
                || contract.owned_paths.len() > 64
                || contract.baseline.files.len() > 4096
                || evidence_hash(&contract.baseline.files)? != contract.baseline.revision
            {
                return Err(invalid_evidence("corrupt host evidence contract"));
            }
            for validator in &contract.validators {
                if validator.argv.len() != 3
                    || validator.argv[0] != "/bin/sh"
                    || validator.argv[1] != "-c"
                    || validator.timeout_ms == 0
                    || validator.timeout_ms > 120000
                {
                    return Err(invalid_evidence("invalid frozen validator argv or limit"));
                }
            }
        }
        if let Some(candidate) = &record.candidate {
            validate_digest(&candidate.checksum)?;
            if candidate.declaration.execution_id != record.execution_id
                || candidate.declaration.claim_digest
                    != claim_digest(
                        &receipt.goal_id,
                        &receipt.blueprint_digest,
                        &receipt.item_id,
                        receipt.attempt,
                    )
                || candidate.declaration.blueprint_digest != receipt.blueprint_digest
                || candidate.declaration.item_id != receipt.item_id
            {
                return Err(invalid_evidence("candidate identity differs from receipt"));
            }
            if let Some(verified) = &candidate.verifiable {
                if verified.declaration != candidate.declaration {
                    return Err(invalid_evidence("conflicting worker declarations"));
                }
                validate_manifest_contract(
                    verified,
                    record
                        .contract
                        .as_ref()
                        .ok_or_else(|| invalid_evidence("verified manifest has no contract"))?,
                )?;
            }
        }
        if record.status == EvidenceStatus::Prepared
            && (record.contract.is_none()
                || record.candidate.is_some()
                || !record.observations.is_empty())
        {
            return Err(invalid_evidence("invalid prepared state"));
        }
        if record.status != EvidenceStatus::Prepared && record.candidate.is_none() {
            return Err(invalid_evidence("evidence state lacks candidate"));
        }
        if record.status == EvidenceStatus::Accepted {
            let contract = record
                .contract
                .as_ref()
                .ok_or_else(|| invalid_evidence("accepted without contract"))?;
            let candidate = record
                .candidate
                .as_ref()
                .ok_or_else(|| invalid_evidence("accepted without candidate"))?;
            if candidate.verifiable.is_none()
                || record.observations.len() != contract.validators.len()
                || !receipt.external_work_executed
                || receipt.status != ExecutionStatus::Succeeded
                || receipt.manifest_checksum.as_ref() != Some(&candidate.checksum)
            {
                return Err(invalid_evidence("acceptance lacks host verification"));
            }
            for (observed, spec) in record.observations.iter().zip(&contract.validators) {
                if observed.validator_id != spec.id
                    || observed.argv != spec.argv
                    || observed.exit_code != Some(0)
                    || observed.signal.is_some()
                    || observed.cancelled
                    || observed.timed_out
                    || !observed.child_reaped
                    || !observed.capture_errors.is_empty()
                    || observed.output.len() != 2
                    || observed.output.iter().any(|r| {
                        !r.complete
                            || !r.finalized
                            || r.session_id != contract.session_id
                            || r.call_id != observed.operation_id
                    })
                {
                    return Err(invalid_evidence(
                        "acceptance contains unsuccessful host observations",
                    ));
                }
            }
        }
    }
    Ok(())
}
