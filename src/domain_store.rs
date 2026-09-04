//! Small, durable stores for the first-class b3ehive domains.
//!
//! `domains` contains the data model and validation rules; this module owns
//! persistence only.  The store is deliberately a single bounded JSONL
//! snapshot rather than a scheduler or an event bus.  Every mutation writes a
//! complete next snapshot to a private temporary file, fsyncs it, and then
//! renames it into place.  A process therefore observes either the old valid
//! snapshot or the new valid snapshot, never a half-written entity.

use std::{
    collections::BTreeMap,
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

use crate::domains::{Blueprint, DomainError, Goal, GoalStatus, Learn, MAX_DOMAIN_RECORD_BYTES};

/// On-disk schema version for a domain snapshot.  It is intentionally
/// independent from both the session and blueprint product versions.
pub const DOMAIN_STORE_SCHEMA_VERSION: u16 = 1;

/// Environment override used by headless and TUI hosts when they need a
/// workspace-local domain store instead of the default user store.
pub const DOMAIN_STORE_ENV: &str = "ZENPI_DOMAIN_STORE";

/// Default location under the user's private zenpi directory.
pub const DOMAIN_STORE_FILE_NAME: &str = "domains.jsonl";

/// A store is deliberately bounded.  The limits apply before JSON parsing and
/// during serialization so a hostile file cannot cause an unbounded read or
/// write.  Individual domain records retain the stricter bounds in
/// [`crate::domains`].
pub const MAX_DOMAIN_STORE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_DOMAIN_STORE_LINES: usize = 512;
pub const MAX_BLUEPRINT_RECORDS: usize = 128;
pub const MAX_GOAL_RECORDS: usize = 256;
pub const MAX_LEARN_RECORDS: usize = 256;

/// Resolve the domain snapshot associated with a session journal.  A caller
/// may override this with [`DOMAIN_STORE_ENV`]; otherwise a custom session
/// keeps its domain records beside the journal, while the default session
/// uses the conventional `~/.zenpi/domains.jsonl` location.  Keeping this
/// rule in the persistence layer lets TUI and headless hosts address the same
/// store without duplicating path policy.
pub fn path_for_session(session_path: impl AsRef<Path>) -> PathBuf {
    if let Ok(path) = std::env::var(DOMAIN_STORE_ENV)
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    let session_path = session_path.as_ref();
    let default_session = crate::session::SessionStore::default_path();
    if session_path == default_session {
        return DomainStore::default_path();
    }
    session_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.join(DOMAIN_STORE_FILE_NAME))
        .unwrap_or_else(|| PathBuf::from(DOMAIN_STORE_FILE_NAME))
}

#[derive(Debug, Error)]
pub enum DomainStoreError {
    #[error("domain store path is empty")]
    EmptyPath,
    #[error("domain store path points to a directory: {0}")]
    Directory(PathBuf),
    #[error("domain store path is a symbolic link: {0}")]
    Symlink(PathBuf),
    #[error("domain store file is too large (maximum {max} bytes)")]
    StoreTooLong { max: usize },
    #[error("domain store has too many records (maximum {max})")]
    TooManyRecords { max: usize },
    #[error("domain store record exceeds {max} bytes")]
    RecordTooLong { max: usize },
    #[error("domain store JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("domain record is invalid: {0}")]
    Domain(#[from] DomainError),
    #[error("domain store I/O: {0}")]
    Io(#[from] io::Error),
    #[error("domain store has no header")]
    MissingHeader,
    #[error("domain store has more than one header")]
    DuplicateHeader,
    #[error("unsupported domain store schema {found} (expected {expected})")]
    SchemaVersion { found: u16, expected: u16 },
    #[error("domain store digest does not match its records")]
    DigestMismatch,
    #[error("domain store record has a duplicate key: {kind}:{id}")]
    DuplicateRecord { kind: &'static str, id: String },
    #[error("domain store record has an invalid envelope: {0}")]
    InvalidEnvelope(String),
    #[error("blueprint `{id}@{version}` is not found")]
    BlueprintNotFound { id: String, version: String },
    #[error("goal `{0}` is not found")]
    GoalNotFound(String),
    #[error("learn record `{0}` is not found")]
    LearnNotFound(String),
    #[error("blueprint `{id}@{version}` is still referenced by a goal")]
    BlueprintReferenced { id: String, version: String },
    #[error("blueprint `{id}@{version}` cannot be replaced because its digest is immutable")]
    ImmutableBlueprint { id: String, version: String },
    #[error("goal `{id}` cannot change its immutable blueprint link")]
    ImmutableGoalBlueprint { id: String },
    #[error("goal `{id}` cannot transition from {from:?} to {to:?}")]
    InvalidGoalTransition {
        id: String,
        from: GoalStatus,
        to: GoalStatus,
    },
    #[error("domain store generation overflow")]
    GenerationOverflow,
}

/// Result of an idempotent mutation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StoreChange {
    Inserted,
    Updated,
    Unchanged,
    Removed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DomainStoreSummary {
    pub path: String,
    pub schema_version: u16,
    pub generation: u64,
    pub digest: String,
    pub blueprint_count: usize,
    pub goal_count: usize,
    pub learn_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoreHeader {
    pub kind: String,
    pub schema_version: u16,
    pub generation: u64,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", deny_unknown_fields)]
enum StoreLine {
    #[serde(rename = "domain_store")]
    Header {
        schema_version: u16,
        generation: u64,
        digest: String,
    },
    #[serde(rename = "blueprint")]
    Blueprint { record: Blueprint },
    #[serde(rename = "goal")]
    Goal { record: Goal },
    #[serde(rename = "learn")]
    Learn { record: Learn },
}

#[derive(Debug, Clone, Serialize)]
struct CanonicalState<'a> {
    blueprints: Vec<&'a Blueprint>,
    goals: Vec<&'a Goal>,
    learns: Vec<&'a Learn>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BlueprintKey {
    id: String,
    version: String,
}

impl BlueprintKey {
    fn new(blueprint: &Blueprint) -> Self {
        Self {
            id: blueprint.id.clone(),
            version: blueprint.version.clone(),
        }
    }

    fn from_parts(id: &str, version: &str) -> Self {
        Self {
            id: id.to_owned(),
            version: version.to_owned(),
        }
    }
}

/// A bounded, content-addressed store for Blueprint, Goal, and Learn records.
///
/// The maps are kept private so callers cannot bypass validation or mutate a
/// record without a durable write.  `&[T]`-like snapshots are exposed as small
/// vectors because the backing map must retain versioned blueprints.
#[derive(Debug, Clone)]
pub struct DomainStore {
    path: PathBuf,
    generation: u64,
    blueprints: BTreeMap<BlueprintKey, Blueprint>,
    goals: BTreeMap<String, Goal>,
    learns: BTreeMap<String, Learn>,
}

impl DomainStore {
    /// Open and validate a store, creating an empty valid snapshot if absent.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DomainStoreError> {
        let path = normalize_path(path.as_ref())?;
        ensure_parent(&path)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(DomainStoreError::Symlink(path));
                }
                if !metadata.is_file() {
                    return Err(DomainStoreError::Directory(path));
                }
                crate::security::restrict_private_file(&path)?;
                let bytes = read_bounded(&path)?;
                if bytes.iter().all(u8::is_ascii_whitespace) {
                    let store = Self::empty(path);
                    store.persist_snapshot()?;
                    return Ok(store);
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

    /// Open and validate an existing store without creating or changing
    /// anything on disk.  Read-only commands (for example blueprint
    /// validation) use this entry point so a typo cannot create a new store
    /// and inspecting a plain user file cannot chmod it as a domain snapshot.
    pub fn open_existing(path: impl AsRef<Path>) -> Result<Self, DomainStoreError> {
        let path = normalize_path(path.as_ref())?;
        let metadata = fs::symlink_metadata(&path).map_err(DomainStoreError::Io)?;
        if metadata.file_type().is_symlink() {
            return Err(DomainStoreError::Symlink(path));
        }
        if !metadata.is_file() {
            return Err(DomainStoreError::Directory(path));
        }
        let bytes = read_bounded(&path)?;
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Err(DomainStoreError::MissingHeader);
        }
        Self::decode(path, &bytes)
    }

    /// Open a snapshot for inspection without creating or modifying a file.
    /// A missing path is represented by an empty in-memory store; callers that
    /// need durable mutation must use [`Self::open`] explicitly. This keeps
    /// read-only status/blueprint views free of filesystem side effects.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, DomainStoreError> {
        let path = normalize_path(path.as_ref())?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(DomainStoreError::Symlink(path));
                }
                if !metadata.is_file() {
                    return Err(DomainStoreError::Directory(path));
                }
                let bytes = read_bounded(&path)?;
                Self::decode(path, &bytes)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::empty(path)),
            Err(error) => Err(error.into()),
        }
    }

    /// Alias useful to hosts that construct a new workspace store.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, DomainStoreError> {
        Self::open(path)
    }

    /// Conventional load spelling for callers that already distinguish
    /// construction from recovery.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, DomainStoreError> {
        Self::open(path)
    }

    /// Open the conventional user-level store.
    pub fn open_default() -> Result<Self, DomainStoreError> {
        Self::open(Self::default_path())
    }

    pub fn default_path() -> PathBuf {
        if let Ok(path) = std::env::var(DOMAIN_STORE_ENV)
            && !path.trim().is_empty()
        {
            return PathBuf::from(path);
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join(".zenpi").join(DOMAIN_STORE_FILE_NAME)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Digest of the canonical, header-independent domain state.
    pub fn digest(&self) -> Result<String, DomainStoreError> {
        self.state_digest()
    }

    pub fn summary(&self) -> Result<DomainStoreSummary, DomainStoreError> {
        Ok(DomainStoreSummary {
            path: self.path.display().to_string(),
            schema_version: DOMAIN_STORE_SCHEMA_VERSION,
            generation: self.generation,
            digest: self.digest()?,
            blueprint_count: self.blueprints.len(),
            goal_count: self.goals.len(),
            learn_count: self.learns.len(),
        })
    }

    pub fn blueprints(&self) -> Vec<&Blueprint> {
        self.blueprints.values().collect()
    }

    pub fn goals(&self) -> Vec<&Goal> {
        self.goals.values().collect()
    }

    pub fn learns(&self) -> Vec<&Learn> {
        self.learns.values().collect()
    }

    pub fn blueprint(&self, id: &str, version: &str) -> Option<&Blueprint> {
        self.blueprints.get(&BlueprintKey::from_parts(id, version))
    }

    pub fn blueprint_versions(&self, id: &str) -> Vec<&Blueprint> {
        self.blueprints
            .iter()
            .filter(|(key, _)| key.id == id)
            .map(|(_, value)| value)
            .collect()
    }

    pub fn goal(&self, id: &str) -> Option<&Goal> {
        self.goals.get(id)
    }

    pub fn learn(&self, id: &str) -> Option<&Learn> {
        self.learns.get(id)
    }

    /// Validate every in-memory record and all Goal-to-Blueprint links.
    pub fn validate(&self) -> Result<(), DomainStoreError> {
        let line_count = 1_usize
            .saturating_add(self.blueprints.len())
            .saturating_add(self.goals.len())
            .saturating_add(self.learns.len());
        if line_count > MAX_DOMAIN_STORE_LINES {
            return Err(DomainStoreError::TooManyRecords {
                max: MAX_DOMAIN_STORE_LINES,
            });
        }
        if self.blueprints.len() > MAX_BLUEPRINT_RECORDS
            || self.goals.len() > MAX_GOAL_RECORDS
            || self.learns.len() > MAX_LEARN_RECORDS
        {
            return Err(DomainStoreError::TooManyRecords {
                max: MAX_DOMAIN_STORE_LINES,
            });
        }
        for blueprint in self.blueprints.values() {
            blueprint.validate()?;
        }
        for goal in self.goals.values() {
            let blueprint = self
                .blueprint(&goal.blueprint_id, &goal.blueprint_version)
                .ok_or_else(|| DomainStoreError::BlueprintNotFound {
                    id: goal.blueprint_id.clone(),
                    version: goal.blueprint_version.clone(),
                })?;
            goal.validate_against(blueprint)?;
        }
        for learn in self.learns.values() {
            learn.validate()?;
        }
        let _ = self.state_digest()?;
        Ok(())
    }

    /// Insert a new Blueprint or idempotently re-submit the same digest.
    /// Blueprint `(id, version)` is immutable once referenced or stored.
    pub fn put_blueprint(&mut self, blueprint: Blueprint) -> Result<StoreChange, DomainStoreError> {
        blueprint.validate()?;
        let key = BlueprintKey::new(&blueprint);
        if let Some(existing) = self.blueprints.get(&key) {
            if existing == &blueprint {
                return Ok(StoreChange::Unchanged);
            }
            return Err(DomainStoreError::ImmutableBlueprint {
                id: blueprint.id,
                version: blueprint.version,
            });
        }
        let mut next = self.clone();
        next.blueprints.insert(key, blueprint);
        let change = next.commit(StoreChange::Inserted)?;
        *self = next;
        Ok(change)
    }

    /// Insert or update a Goal.  The linked Blueprint digest/version is
    /// immutable; changing status must obey `Goal::transition_to` rules.
    pub fn put_goal(&mut self, goal: Goal) -> Result<StoreChange, DomainStoreError> {
        goal.validate()?;
        let blueprint = self
            .blueprint(&goal.blueprint_id, &goal.blueprint_version)
            .ok_or_else(|| DomainStoreError::BlueprintNotFound {
                id: goal.blueprint_id.clone(),
                version: goal.blueprint_version.clone(),
            })?;
        goal.validate_against(blueprint)?;
        if let Some(existing) = self.goals.get(&goal.id) {
            if existing == &goal {
                return Ok(StoreChange::Unchanged);
            }
            if existing.blueprint_id != goal.blueprint_id
                || existing.blueprint_version != goal.blueprint_version
                || existing.blueprint_digest != goal.blueprint_digest
            {
                return Err(DomainStoreError::ImmutableGoalBlueprint { id: goal.id });
            }
            if existing.status != goal.status {
                existing.clone().transition_to(goal.status).map_err(|_| {
                    DomainStoreError::InvalidGoalTransition {
                        id: goal.id.clone(),
                        from: existing.status,
                        to: goal.status,
                    }
                })?;
            }
        }
        let change = if self.goals.contains_key(&goal.id) {
            StoreChange::Updated
        } else {
            StoreChange::Inserted
        };
        let mut next = self.clone();
        next.goals.insert(goal.id.clone(), goal);
        let change = next.commit(change)?;
        *self = next;
        Ok(change)
    }

    /// Transition a Goal with the domain's finite-state rules and persist the
    /// result atomically.
    pub fn transition_goal(
        &mut self,
        id: &str,
        next_status: GoalStatus,
    ) -> Result<StoreChange, DomainStoreError> {
        let current = self
            .goals
            .get(id)
            .ok_or_else(|| DomainStoreError::GoalNotFound(id.to_owned()))?;
        if current.status == next_status {
            return Ok(StoreChange::Unchanged);
        }
        let mut goal = current.clone();
        goal.transition_to(next_status)
            .map_err(|_| DomainStoreError::InvalidGoalTransition {
                id: id.to_owned(),
                from: current.status,
                to: next_status,
            })?;
        self.put_goal(goal)
    }

    /// Insert or replace a Learn record.  Replacing it is safe because Learn
    /// records have no executable side effects and are fully validated.
    pub fn put_learn(&mut self, learn: Learn) -> Result<StoreChange, DomainStoreError> {
        learn.validate()?;
        if self.learns.get(&learn.id) == Some(&learn) {
            return Ok(StoreChange::Unchanged);
        }
        let change = if self.learns.contains_key(&learn.id) {
            StoreChange::Updated
        } else {
            StoreChange::Inserted
        };
        let mut next = self.clone();
        next.learns.insert(learn.id.clone(), learn);
        let change = next.commit(change)?;
        *self = next;
        Ok(change)
    }

    /// Add one evidence reference to an existing Learn record and persist the
    /// resulting snapshot atomically. Re-submitting an existing reference is
    /// an idempotent no-op; a missing Learn ID is rejected before a store is
    /// opened for mutation by the host owner.
    pub fn add_learn_evidence(
        &mut self,
        id: &str,
        reference: impl Into<String>,
    ) -> Result<StoreChange, DomainStoreError> {
        let current = self
            .learns
            .get(id)
            .ok_or_else(|| DomainStoreError::LearnNotFound(id.to_owned()))?;
        let mut learn = current.clone();
        learn.add_evidence(reference)?;
        self.put_learn(learn)
    }

    pub fn remove_blueprint(
        &mut self,
        id: &str,
        version: &str,
    ) -> Result<StoreChange, DomainStoreError> {
        let key = BlueprintKey::from_parts(id, version);
        if !self.blueprints.contains_key(&key) {
            return Ok(StoreChange::Unchanged);
        }
        if self
            .goals
            .values()
            .any(|goal| goal.blueprint_id == id && goal.blueprint_version == version)
        {
            return Err(DomainStoreError::BlueprintReferenced {
                id: id.to_owned(),
                version: version.to_owned(),
            });
        }
        let mut next = self.clone();
        next.blueprints.remove(&key);
        let change = next.commit(StoreChange::Removed)?;
        *self = next;
        Ok(change)
    }

    pub fn remove_goal(&mut self, id: &str) -> Result<StoreChange, DomainStoreError> {
        if !self.goals.contains_key(id) {
            return Ok(StoreChange::Unchanged);
        }
        let mut next = self.clone();
        next.goals.remove(id);
        let change = next.commit(StoreChange::Removed)?;
        *self = next;
        Ok(change)
    }

    pub fn remove_learn(&mut self, id: &str) -> Result<StoreChange, DomainStoreError> {
        if !self.learns.contains_key(id) {
            return Ok(StoreChange::Unchanged);
        }
        let mut next = self.clone();
        next.learns.remove(id);
        let change = next.commit(StoreChange::Removed)?;
        *self = next;
        Ok(change)
    }

    /// Re-read the on-disk snapshot, retaining this handle's path.
    pub fn reload(&mut self) -> Result<(), DomainStoreError> {
        let replacement = Self::open(&self.path)?;
        *self = replacement;
        Ok(())
    }

    fn empty(path: PathBuf) -> Self {
        Self {
            path,
            generation: 0,
            blueprints: BTreeMap::new(),
            goals: BTreeMap::new(),
            learns: BTreeMap::new(),
        }
    }

    fn commit(&mut self, change: StoreChange) -> Result<StoreChange, DomainStoreError> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(DomainStoreError::GenerationOverflow)?;
        if let Err(error) = self.validate().and_then(|_| self.persist_snapshot()) {
            // The caller works on a clone for all public mutations.  This
            // guard keeps the helper safe if a future caller invokes it on a
            // live value and persistence fails.
            self.generation = self.generation.saturating_sub(1);
            return Err(error);
        }
        Ok(change)
    }

    fn state_digest(&self) -> Result<String, DomainStoreError> {
        let state = CanonicalState {
            blueprints: self.blueprints.values().collect(),
            goals: self.goals.values().collect(),
            learns: self.learns.values().collect(),
        };
        let bytes = serde_json::to_vec(&state)?;
        if bytes.len() > MAX_DOMAIN_STORE_BYTES {
            return Err(DomainStoreError::StoreTooLong {
                max: MAX_DOMAIN_STORE_BYTES,
            });
        }
        Ok(digest_hex(&bytes))
    }

    fn persist_snapshot(&self) -> Result<(), DomainStoreError> {
        let bytes = self.encode_snapshot()?;
        atomic_replace(&self.path, &bytes)
    }

    fn encode_snapshot(&self) -> Result<Vec<u8>, DomainStoreError> {
        self.validate()?;
        let digest = self.state_digest()?;
        let mut lines =
            Vec::with_capacity(1 + self.blueprints.len() + self.goals.len() + self.learns.len());
        lines.push(StoreLine::Header {
            schema_version: DOMAIN_STORE_SCHEMA_VERSION,
            generation: self.generation,
            digest,
        });
        lines.extend(
            self.blueprints
                .values()
                .cloned()
                .map(|record| StoreLine::Blueprint { record }),
        );
        lines.extend(
            self.goals
                .values()
                .cloned()
                .map(|record| StoreLine::Goal { record }),
        );
        lines.extend(
            self.learns
                .values()
                .cloned()
                .map(|record| StoreLine::Learn { record }),
        );
        if lines.len() > MAX_DOMAIN_STORE_LINES {
            return Err(DomainStoreError::TooManyRecords {
                max: MAX_DOMAIN_STORE_LINES,
            });
        }
        let mut output = Vec::new();
        for line in lines {
            let encoded = serde_json::to_vec(&line)?;
            if encoded.len() + 1 > MAX_DOMAIN_RECORD_BYTES {
                return Err(DomainStoreError::RecordTooLong {
                    max: MAX_DOMAIN_RECORD_BYTES,
                });
            }
            output.extend_from_slice(&encoded);
            output.push(b'\n');
            if output.len() > MAX_DOMAIN_STORE_BYTES {
                return Err(DomainStoreError::StoreTooLong {
                    max: MAX_DOMAIN_STORE_BYTES,
                });
            }
        }
        Ok(output)
    }

    fn decode(path: PathBuf, bytes: &[u8]) -> Result<Self, DomainStoreError> {
        if bytes.len() > MAX_DOMAIN_STORE_BYTES {
            return Err(DomainStoreError::StoreTooLong {
                max: MAX_DOMAIN_STORE_BYTES,
            });
        }
        let mut header: Option<StoreHeader> = None;
        let mut blueprints = BTreeMap::new();
        let mut goals = BTreeMap::new();
        let mut learns = BTreeMap::new();
        let mut lines = 0_usize;
        for raw_line in bytes.split(|byte| *byte == b'\n') {
            if raw_line.is_empty() {
                continue;
            }
            lines = lines.saturating_add(1);
            if lines > MAX_DOMAIN_STORE_LINES {
                return Err(DomainStoreError::TooManyRecords {
                    max: MAX_DOMAIN_STORE_LINES,
                });
            }
            let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
            if line.len() > MAX_DOMAIN_RECORD_BYTES {
                return Err(DomainStoreError::RecordTooLong {
                    max: MAX_DOMAIN_RECORD_BYTES,
                });
            }
            let decoded: StoreLine = serde_json::from_slice(line)?;
            match decoded {
                StoreLine::Header {
                    schema_version,
                    generation,
                    digest,
                } => {
                    if header.is_some() {
                        return Err(DomainStoreError::DuplicateHeader);
                    }
                    if lines != 1 {
                        return Err(DomainStoreError::InvalidEnvelope(
                            "domain_store header must be the first record".into(),
                        ));
                    }
                    header = Some(StoreHeader {
                        kind: "domain_store".into(),
                        schema_version,
                        generation,
                        digest,
                    });
                }
                StoreLine::Blueprint { record } => {
                    record.validate()?;
                    if blueprints
                        .insert(BlueprintKey::new(&record), record.clone())
                        .is_some()
                    {
                        return Err(DomainStoreError::DuplicateRecord {
                            kind: "blueprint",
                            id: format!("{}@{}", record.id, record.version),
                        });
                    }
                    if blueprints.len() > MAX_BLUEPRINT_RECORDS {
                        return Err(DomainStoreError::TooManyRecords {
                            max: MAX_DOMAIN_STORE_LINES,
                        });
                    }
                }
                StoreLine::Goal { record } => {
                    record.validate()?;
                    if goals.insert(record.id.clone(), record.clone()).is_some() {
                        return Err(DomainStoreError::DuplicateRecord {
                            kind: "goal",
                            id: record.id,
                        });
                    }
                    if goals.len() > MAX_GOAL_RECORDS {
                        return Err(DomainStoreError::TooManyRecords {
                            max: MAX_DOMAIN_STORE_LINES,
                        });
                    }
                }
                StoreLine::Learn { record } => {
                    record.validate()?;
                    if learns.insert(record.id.clone(), record.clone()).is_some() {
                        return Err(DomainStoreError::DuplicateRecord {
                            kind: "learn",
                            id: record.id,
                        });
                    }
                    if learns.len() > MAX_LEARN_RECORDS {
                        return Err(DomainStoreError::TooManyRecords {
                            max: MAX_DOMAIN_STORE_LINES,
                        });
                    }
                }
            }
        }
        let header = header.ok_or(DomainStoreError::MissingHeader)?;
        if header.kind != "domain_store" {
            return Err(DomainStoreError::InvalidEnvelope(header.kind));
        }
        if header.schema_version != DOMAIN_STORE_SCHEMA_VERSION {
            return Err(DomainStoreError::SchemaVersion {
                found: header.schema_version,
                expected: DOMAIN_STORE_SCHEMA_VERSION,
            });
        }
        let store = Self {
            path,
            generation: header.generation,
            blueprints,
            goals,
            learns,
        };
        store.validate()?;
        if store.state_digest()? != header.digest {
            return Err(DomainStoreError::DigestMismatch);
        }
        Ok(store)
    }
}

fn normalize_path(path: &Path) -> Result<PathBuf, DomainStoreError> {
    if path.as_os_str().is_empty() {
        return Err(DomainStoreError::EmptyPath);
    }
    Ok(path.to_path_buf())
}

fn ensure_parent(path: &Path) -> Result<(), DomainStoreError> {
    // A relative filename such as `domains.jsonl` is a valid store in the
    // current directory. `Path::parent` reports an empty component for that
    // spelling, so normalize it to `.` rather than rejecting an otherwise
    // useful embedding path.
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    if !parent.is_dir() {
        return Err(DomainStoreError::Directory(parent.to_owned()));
    }
    Ok(())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, DomainStoreError> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if metadata.len() > MAX_DOMAIN_STORE_BYTES as u64 {
        return Err(DomainStoreError::StoreTooLong {
            max: MAX_DOMAIN_STORE_BYTES,
        });
    }
    let mut bytes = Vec::with_capacity(metadata.len().min(MAX_DOMAIN_STORE_BYTES as u64) as usize);
    file.take((MAX_DOMAIN_STORE_BYTES as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_DOMAIN_STORE_BYTES {
        return Err(DomainStoreError::StoreTooLong {
            max: MAX_DOMAIN_STORE_BYTES,
        });
    }
    Ok(bytes)
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), DomainStoreError> {
    if bytes.len() > MAX_DOMAIN_STORE_BYTES {
        return Err(DomainStoreError::StoreTooLong {
            max: MAX_DOMAIN_STORE_BYTES,
        });
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    ensure_parent(path)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(DomainStoreError::Symlink(path.to_owned()));
        }
        if !metadata.is_file() {
            return Err(DomainStoreError::Directory(path.to_owned()));
        }
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(DomainStoreError::EmptyPath)?;
    let temporary = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        now_ms()
    ));
    if let Ok(metadata) = fs::symlink_metadata(&temporary) {
        if metadata.file_type().is_symlink() {
            return Err(DomainStoreError::Symlink(temporary));
        }
        return Err(DomainStoreError::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "domain store temporary file already exists",
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
                "domain store destination became a symbolic link",
            ));
        }
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        {
            // A directory fsync makes the rename durable on filesystems that
            // otherwise may lose the new directory entry after a crash.
            // The rename already committed the new snapshot; a best-effort
            // directory sync avoids reporting an error after the in-memory
            // caller has logically committed its mutation.
            if let Ok(directory) = File::open(parent) {
                let _ = directory.sync_all();
            }
        }
        Ok::<(), io::Error>(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

fn digest_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_digest_is_stable_for_empty_state() {
        let path = PathBuf::from("domain-store-test.jsonl");
        let store = DomainStore::empty(path);
        assert_eq!(store.digest().unwrap().len(), 64);
    }
}
