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
            (true, Some(checksum))
                if checksum.len() == 64 && checksum.bytes().all(|b| b.is_ascii_hexdigit()) => {}
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

/// A request for an external b3ehive/agent owner to execute one declarative
/// [`BlueprintTask`].  This is intentionally a handoff, not a completion
/// receipt: zenpi never executes the instruction or its acceptance commands
/// while creating this record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BlueprintHandoff {
    pub handoff_id: String,
    pub goal_id: String,
    pub blueprint_id: String,
    pub blueprint_version: String,
    pub blueprint_digest: String,
    pub item_id: String,
    pub attempt: u32,
    pub depends_on: Vec<String>,
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
        let expected = make_handoff_request(goal, blueprint, item, 1, task)?;
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
}

#[derive(Debug, Serialize)]
struct UnsignedSnapshot<'a> {
    receipts: &'a [ExecutionReceipt],
}

/// Atomic, private, bounded persistence for execution receipts.
#[derive(Debug, Clone)]
pub struct ExecutionStore {
    path: PathBuf,
    generation: u64,
    receipts: Vec<ExecutionReceipt>,
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
        digest_receipts(&self.receipts)
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

    /// Attach one validated external result manifest to a terminal receipt.
    /// This is the only path that can turn control-plane bookkeeping into
    /// externally evidenced work; the local executor cannot set the flag.
    pub fn attach_external_manifest(
        &mut self,
        execution_id: &str,
        manifest_checksum: String,
    ) -> Result<ExecutionStoreChange, ExecutionError> {
        let Some(index) = self
            .receipts
            .iter()
            .position(|receipt| receipt.execution_id == execution_id)
        else {
            return Err(ExecutionError::InvalidReceipt(
                "execution receipt is not found".into(),
            ));
        };
        let mut next = self.receipts[index].clone();
        if next.external_work_executed
            && next.manifest_checksum.as_deref() == Some(&manifest_checksum)
        {
            return Ok(ExecutionStoreChange::Unchanged);
        }
        if next.external_work_executed || !next.status.is_terminal() {
            return Err(ExecutionError::ReceiptConflict {
                execution_id: execution_id.to_owned(),
            });
        }
        next.external_work_executed = true;
        next.manifest_checksum = Some(manifest_checksum);
        next.validate()?;
        let mut replacement = self.clone();
        replacement.receipts[index] = next;
        replacement.commit(ExecutionStoreChange::Updated)?;
        *self = replacement;
        Ok(ExecutionStoreChange::Updated)
    }

    fn empty(path: PathBuf) -> Self {
        Self {
            path,
            generation: 0,
            receipts: Vec::new(),
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
        let snapshot = Snapshot {
            schema_version: EXECUTION_SCHEMA_VERSION,
            generation: self.generation,
            digest: digest_receipts(&self.receipts)?,
            receipts: self.receipts.clone(),
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
        if snapshot.digest != digest_receipts(&snapshot.receipts)? {
            return Err(ExecutionError::DigestMismatch);
        }
        Ok(Self {
            path,
            generation: snapshot.generation,
            receipts: snapshot.receipts,
        })
    }
}

/// Owner for one bounded local Blueprint step.
#[derive(Debug)]
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
        let Some(selection) = self.select_item(goal, blueprint)? else {
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

fn make_handoff_request(
    goal: &Goal,
    blueprint: &Blueprint,
    item: &BlueprintItem,
    attempt: u32,
    task: &BlueprintTask,
) -> Result<BlueprintHandoff, ExecutionError> {
    let request = BlueprintHandoff {
        handoff_id: handoff_id(goal, blueprint, item, attempt),
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

fn digest_receipts(receipts: &[ExecutionReceipt]) -> Result<String, ExecutionError> {
    let bytes = serde_json::to_vec(&UnsignedSnapshot { receipts })?;
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
