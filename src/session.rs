//! Append-only JSONL session persistence.
//!
//! The session file is intentionally boring: a header followed by independent
//! records.  A process can recover all complete records after a crash, ignore a
//! malformed trailing line, and append again without rewriting the transcript.

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    b3::{Handoff, HandoffRecord, unix_ms_to_rfc3339},
    core::Turn,
};

pub const SESSION_VERSION: u32 = 1;
pub const MAX_SESSION_RECORD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionHeader {
    pub version: u32,
    pub session_id: String,
    pub created_at_ms: u64,
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryWarning {
    pub line: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Provider,
    Tool,
    Compaction,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationOutcome {
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterruptedOperation {
    pub operation_id: String,
    pub kind: OperationKind,
    pub turn_id: String,
    pub retry_requires_confirmation: bool,
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session path is empty")]
    EmptyPath,
    #[error("session path points to a directory: {0}")]
    Directory(PathBuf),
    #[error("session I/O: {0}")]
    Io(#[from] io::Error),
    #[error("session JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("session record is invalid: {0}")]
    InvalidRecord(String),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DecodedRecord {
    #[serde(alias = "header")]
    Session {
        version: u32,
        session_id: String,
        created_at_ms: u64,
        cwd: String,
    },
    Turn {
        turn: Turn,
    },
    Handoff {
        handoff: Handoff,
    },
    HandoffRecord {
        handoff: HandoffRecord,
    },
    Event {
        event: Value,
    },
}

/// A recoverable session and its in-memory projection.
#[derive(Debug)]
pub struct SessionStore {
    path: PathBuf,
    header: SessionHeader,
    turns: Vec<Turn>,
    handoffs: Vec<Handoff>,
    handoff_records: Vec<HandoffRecord>,
    events: usize,
    event_values: Vec<Value>,
    warnings: Vec<RecoveryWarning>,
    needs_separator: bool,
    next_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSummary {
    pub path: String,
    pub session_id: String,
    pub created_at_ms: u64,
    pub turn_count: usize,
    pub handoff_count: usize,
    pub handoff_record_count: usize,
    pub event_count: usize,
    pub recovery_warnings: usize,
    pub next_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionInspection {
    pub summary: SessionSummary,
    pub turns: Vec<Turn>,
    pub events: Vec<Value>,
    pub recovery_warnings: Vec<RecoveryWarning>,
}

impl SessionStore {
    /// Open or create a session at `path` and recover all valid records.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        let path = path.as_ref().to_path_buf();
        if path.as_os_str().is_empty() {
            return Err(SessionError::EmptyPath);
        }
        if path.exists() && path.is_dir() {
            return Err(SessionError::Directory(path));
        }
        restrict_session_permissions(&path)?;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }

        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        let had_content = !bytes.is_empty();
        let needs_separator = had_content && !bytes.ends_with(b"\n");
        let text = String::from_utf8(bytes)
            .map_err(|_| SessionError::InvalidRecord("session is not valid UTF-8".into()))?;
        let mut header: Option<SessionHeader> = None;
        let mut turns = Vec::new();
        let mut handoffs = Vec::new();
        let mut handoff_records = Vec::new();
        let mut events = 0;
        let mut event_values = Vec::new();
        let mut warnings = Vec::new();
        let mut next_seq = 0_u64;
        let mut last_seq = None;

        for (index, raw_line) in text.split('\n').enumerate() {
            let line_number = index + 1;
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            if line.trim().is_empty() {
                continue;
            }
            let value = match serde_json::from_str::<Value>(line) {
                Ok(value) => value,
                Err(error) => {
                    warnings.push(RecoveryWarning {
                        line: line_number,
                        reason: format!("ignored malformed record: {error}"),
                    });
                    continue;
                }
            };
            if let Some(version) = value.get("schema_version").and_then(Value::as_u64)
                && version != u64::from(SESSION_VERSION)
            {
                warnings.push(RecoveryWarning {
                    line: line_number,
                    reason: "ignored unsupported session schema".into(),
                });
                continue;
            }
            if let Some(expected) = header.as_ref().map(|item| item.session_id.as_str())
                && value
                    .get("session_id")
                    .and_then(Value::as_str)
                    .is_some_and(|actual| actual != expected)
            {
                warnings.push(RecoveryWarning {
                    line: line_number,
                    reason: "ignored record from another session".into(),
                });
                continue;
            }
            let sequence = value.get("seq").and_then(Value::as_u64);
            if let Some(sequence) = sequence
                && last_seq.is_some_and(|last| sequence <= last)
            {
                warnings.push(RecoveryWarning {
                    line: line_number,
                    reason: "ignored non-monotonic sequence".into(),
                });
                continue;
            }
            let record = match serde_json::from_value::<DecodedRecord>(value) {
                Ok(record) => record,
                Err(error) => {
                    warnings.push(RecoveryWarning {
                        line: line_number,
                        reason: format!("ignored malformed record: {error}"),
                    });
                    continue;
                }
            };
            if let Some(sequence) = sequence {
                last_seq = Some(sequence);
                next_seq = sequence.saturating_add(1);
            } else {
                next_seq = next_seq.saturating_add(1);
                last_seq = next_seq.checked_sub(1);
            }
            match record {
                DecodedRecord::Session {
                    version,
                    session_id,
                    created_at_ms,
                    cwd,
                } => {
                    if version != SESSION_VERSION {
                        warnings.push(RecoveryWarning {
                            line: line_number,
                            reason: "ignored unsupported session version".into(),
                        });
                    } else if header.is_some() {
                        warnings.push(RecoveryWarning {
                            line: line_number,
                            reason: "ignored duplicate session header".into(),
                        });
                    } else {
                        header = Some(SessionHeader {
                            version,
                            session_id,
                            created_at_ms,
                            cwd,
                        });
                    }
                }
                DecodedRecord::Turn { turn } => match turn.validate() {
                    Ok(()) => turns.push(turn),
                    Err(error) => warnings.push(RecoveryWarning {
                        line: line_number,
                        reason: format!("ignored invalid turn: {error}"),
                    }),
                },
                DecodedRecord::Handoff { handoff } => {
                    if handoff.validate().is_err() {
                        warnings.push(RecoveryWarning {
                            line: line_number,
                            reason: "ignored invalid handoff".into(),
                        });
                    } else {
                        handoffs.push(handoff);
                    }
                }
                DecodedRecord::HandoffRecord { handoff } => {
                    if handoff.validate().is_err()
                        || header
                            .as_ref()
                            .is_some_and(|item| item.session_id != handoff.session_id)
                    {
                        warnings.push(RecoveryWarning {
                            line: line_number,
                            reason: "ignored invalid handoff record".into(),
                        });
                    } else {
                        handoff_records.push(handoff);
                    }
                }
                DecodedRecord::Event { event } => {
                    event_values.push(event);
                    events += 1;
                }
            }
        }

        let needs_header = header.is_none();
        let mut store = Self {
            path,
            header: header.unwrap_or_else(|| SessionHeader {
                version: SESSION_VERSION,
                session_id: generate_id("session"),
                created_at_ms: now_ms(),
                cwd: std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .display()
                    .to_string(),
            }),
            turns,
            handoffs,
            handoff_records,
            events,
            event_values,
            warnings,
            needs_separator,
            next_seq,
        };
        if !had_content || needs_header {
            // A header is the only record whose absence changes the meaning of
            // subsequent records.  Append one, preserving all existing bytes.
            let header = store.header.clone();
            store.append_json(&json!({
                "kind": "session",
                "version": header.version,
                "session_id": header.session_id,
                "created_at_ms": header.created_at_ms,
                "cwd": header.cwd,
            }))?;
        }
        Ok(store)
    }

    pub fn default_path() -> PathBuf {
        if let Ok(path) = std::env::var("ZENPI_SESSION")
            && !path.trim().is_empty()
        {
            return PathBuf::from(path);
        }
        let base = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(".zenpi").join("session.jsonl")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn header(&self) -> &SessionHeader {
        &self.header
    }

    pub fn session_id(&self) -> &str {
        &self.header.session_id
    }

    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    pub fn handoffs(&self) -> &[Handoff] {
        &self.handoffs
    }

    pub fn recovery_warnings(&self) -> &[RecoveryWarning] {
        &self.warnings
    }

    pub fn summary(&self) -> SessionSummary {
        SessionSummary {
            path: self.path.display().to_string(),
            session_id: self.header.session_id.clone(),
            created_at_ms: self.header.created_at_ms,
            turn_count: self.turns.len(),
            handoff_count: self.handoffs.len() + self.handoff_records.len(),
            handoff_record_count: self.handoff_records.len(),
            event_count: self.events,
            recovery_warnings: self.warnings.len(),
            next_seq: self.next_seq,
        }
    }

    /// Persist and then retain a turn.  If the write fails, the in-memory
    /// projection is unchanged.
    pub fn append_turn(&mut self, turn: Turn) -> Result<(), SessionError> {
        self.append_json(&json!({ "kind": "turn", "turn": &turn }))?;
        self.turns.push(turn);
        Ok(())
    }

    pub fn append_handoff(&mut self, handoff: Handoff) -> Result<(), SessionError> {
        handoff
            .validate()
            .map_err(|error| SessionError::InvalidRecord(error.to_string()))?;
        self.append_json(&json!({ "kind": "handoff", "handoff": &handoff }))?;
        self.handoffs.push(handoff);
        Ok(())
    }

    /// Persist a signed b3 handoff record without converting it to the legacy
    /// session shape. Validation happens before any file write.
    pub fn append_handoff_record(&mut self, handoff: HandoffRecord) -> Result<(), SessionError> {
        handoff
            .validate()
            .map_err(|error| SessionError::InvalidRecord(error.to_string()))?;
        self.append_json(&json!({ "kind": "handoff_record", "handoff": &handoff }))?;
        self.handoff_records.push(handoff);
        Ok(())
    }

    pub fn handoff_records(&self) -> &[HandoffRecord] {
        &self.handoff_records
    }

    /// Persist a normalized event.  Events are intentionally opaque to the
    /// session layer so extensions can add fields without migrations.
    pub fn append_event(&mut self, event: Value) -> Result<(), SessionError> {
        self.append_json(&json!({ "kind": "event", "event": &event }))?;
        self.event_values.push(event);
        self.events += 1;
        Ok(())
    }

    pub fn events(&self) -> &[Value] {
        &self.event_values
    }

    /// Return operations that reached durable `started` state without a
    /// matching terminal marker. No operation is retried here: callers must
    /// make an explicit policy decision, which prevents crash recovery from
    /// repeating a side effect.
    pub fn interrupted_operations(&self) -> Vec<InterruptedOperation> {
        interrupted_operations(&self.event_values)
    }

    pub fn begin_operation(
        &mut self,
        operation: &InterruptedOperation,
    ) -> Result<(), SessionError> {
        self.append_event(json!({
            "type": "operation_started",
            "operation_id": operation.operation_id,
            "operation_kind": operation.kind,
            "turn_id": operation.turn_id,
            "retry_requires_confirmation": operation.retry_requires_confirmation,
        }))
    }

    pub fn finish_operation(
        &mut self,
        operation_id: &str,
        outcome: OperationOutcome,
    ) -> Result<(), SessionError> {
        if operation_id.trim().is_empty() || operation_id.len() > 256 {
            return Err(SessionError::InvalidRecord(
                "operation ID is empty or too large".into(),
            ));
        }
        self.append_event(json!({
            "type": "operation_finished",
            "operation_id": operation_id,
            "outcome": outcome,
        }))
    }

    /// Convert unfinished markers from a previous process into durable
    /// interrupted terminals. The returned list remains available to the host
    /// for an explicit retry/abandon decision.
    pub fn mark_interrupted_operations(
        &mut self,
    ) -> Result<Vec<InterruptedOperation>, SessionError> {
        let interrupted = self.interrupted_operations();
        for operation in &interrupted {
            self.finish_operation(&operation.operation_id, OperationOutcome::Interrupted)?;
        }
        Ok(interrupted)
    }

    pub fn latest_assistant(&self) -> Option<&Turn> {
        self.turns
            .iter()
            .rev()
            .find(|turn| turn.role == crate::core::TurnRole::Assistant)
    }

    /// Copy this journal to a new destination without mutating the source.
    /// The destination is opened and recovered first, so malformed or
    /// incompatible input cannot silently become an exported session.
    pub fn export_to(&self, destination: impl AsRef<Path>) -> Result<(), SessionError> {
        let destination = destination.as_ref();
        if destination == self.path {
            return Err(SessionError::InvalidRecord(
                "cannot export a session over itself".into(),
            ));
        }
        if destination.exists() {
            return Err(SessionError::InvalidRecord(
                "export destination already exists".into(),
            ));
        }
        if let Some(parent) = destination.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&self.path, destination)?;
        restrict_session_permissions(destination)?;
        if file_sha256(&self.path)? != file_sha256(destination)? {
            let _ = fs::remove_file(destination);
            return Err(SessionError::InvalidRecord(
                "export digest does not match source".into(),
            ));
        }
        Ok(())
    }

    /// Build a new session from this journal's immutable prefix. The source
    /// remains untouched and the destination receives a fresh session header.
    pub fn fork_to(&self, destination: impl AsRef<Path>) -> Result<Self, SessionError> {
        let destination = destination.as_ref();
        if destination.exists() {
            return Err(SessionError::InvalidRecord(
                "fork destination already exists".into(),
            ));
        }
        let mut fork = Self::open(destination)?;
        for turn in &self.turns {
            fork.append_turn(turn.clone())?;
        }
        for handoff in &self.handoffs {
            fork.append_handoff(handoff.clone())?;
        }
        for record in &self.handoff_records {
            fork.append_handoff_record(record.clone())?;
        }
        for event in &self.event_values {
            fork.append_event(event.clone())?;
        }
        Ok(fork)
    }

    fn append_json(&mut self, value: &Value) -> Result<(), SessionError> {
        if !value.is_object() {
            return Err(SessionError::InvalidRecord(
                "session record must be an object".into(),
            ));
        }
        let mut record = value.clone();
        if let Value::Object(fields) = &mut record {
            let now = now_ms();
            fields.insert("schema_version".into(), json!(SESSION_VERSION));
            fields.insert("session_id".into(), json!(self.header.session_id));
            fields.insert("seq".into(), json!(self.next_seq));
            fields.insert("timestamp_ms".into(), json!(now));
            fields.insert("timestamp".into(), json!(unix_ms_to_rfc3339(now)));
        }
        let mut encoded = serde_json::to_vec(&record)?;
        if encoded.len() + 1 > MAX_SESSION_RECORD_BYTES {
            return Err(SessionError::InvalidRecord(format!(
                "record exceeds {MAX_SESSION_RECORD_BYTES} bytes"
            )));
        }
        encoded.push(b'\n');
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&self.path)?;
        if self.needs_separator {
            file.write_all(b"\n")?;
            self.needs_separator = false;
        }
        file.write_all(&encoded)?;
        file.flush()?;
        file.sync_data()?;
        self.next_seq = self.next_seq.saturating_add(1);
        Ok(())
    }
}

fn interrupted_operations(events: &[Value]) -> Vec<InterruptedOperation> {
    let mut active = BTreeMap::<String, InterruptedOperation>::new();
    for event in events {
        match event.get("type").and_then(Value::as_str) {
            Some("operation_started") => {
                let Some(operation_id) = event.get("operation_id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(turn_id) = event.get("turn_id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(kind) = event
                    .get("operation_kind")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok())
                else {
                    continue;
                };
                active.insert(
                    operation_id.to_owned(),
                    InterruptedOperation {
                        operation_id: operation_id.to_owned(),
                        kind,
                        turn_id: turn_id.to_owned(),
                        retry_requires_confirmation: event
                            .get("retry_requires_confirmation")
                            .and_then(Value::as_bool)
                            .unwrap_or(true),
                    },
                );
            }
            Some("operation_finished") => {
                if let Some(operation_id) = event.get("operation_id").and_then(Value::as_str) {
                    active.remove(operation_id);
                }
            }
            _ => {}
        }
    }
    active.into_values().collect()
}

#[cfg(unix)]
fn restrict_session_permissions(path: &Path) -> io::Result<()> {
    crate::security::restrict_private_file(path)
}

#[cfg(not(unix))]
fn restrict_session_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn generate_id(prefix: &str) -> String {
    // Time plus process id is sufficient for one append-only file and avoids a
    // heavyweight UUID dependency in the default binary.
    format!("{prefix}-{}-{}", now_ms(), std::process::id())
}

/// Exposed for callers that need a clock-compatible session timestamp.
pub fn unix_time_ms() -> u64 {
    now_ms()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GarbageCollectionPolicy {
    pub retain_newest: usize,
    pub older_than_ms: u64,
}

pub fn list_sessions(directory: impl AsRef<Path>) -> Result<Vec<SessionSummary>, SessionError> {
    let directory = directory.as_ref();
    if !directory.exists() {
        return Ok(Vec::new());
    }
    if !directory.is_dir() {
        return Err(SessionError::Directory(directory.to_owned()));
    }
    let mut paths = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("jsonl"))
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .map(|path| inspect_session(path).map(|inspection| inspection.summary))
        .collect()
}

/// Open an existing journal for inspection. Unlike `SessionStore::open`, this
/// rejects a missing source, preventing a typo in a read-only CLI command from
/// creating an empty session.
pub fn inspect_session(path: impl AsRef<Path>) -> Result<SessionInspection, SessionError> {
    let path = path.as_ref();
    if !path.is_file() {
        return Err(SessionError::InvalidRecord(
            "inspection source is not an existing session file".into(),
        ));
    }
    let store = SessionStore::open(path)?;
    Ok(SessionInspection {
        summary: store.summary(),
        turns: store.turns,
        events: store.event_values,
        recovery_warnings: store.warnings,
    })
}

pub fn import_session(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> Result<SessionStore, SessionError> {
    if !source.as_ref().is_file() {
        return Err(SessionError::InvalidRecord(
            "import source is not an existing session file".into(),
        ));
    }
    let source = SessionStore::open(source)?;
    source.export_to(&destination)?;
    let imported = match SessionStore::open(destination.as_ref()) {
        Ok(imported) => imported,
        Err(error) => {
            let _ = fs::remove_file(destination.as_ref());
            return Err(error);
        }
    };
    if imported.session_id() != source.session_id()
        || imported.summary().next_seq != source.summary().next_seq
    {
        let _ = fs::remove_file(destination.as_ref());
        return Err(SessionError::InvalidRecord(
            "imported journal identity or sequence differs from source".into(),
        ));
    }
    Ok(imported)
}

fn file_sha256(path: &Path) -> Result<String, SessionError> {
    use sha2::{Digest, Sha256};

    let bytes = fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn garbage_collect_sessions(
    directory: impl AsRef<Path>,
    policy: GarbageCollectionPolicy,
    now_ms: u64,
) -> Result<Vec<PathBuf>, SessionError> {
    if policy.retain_newest == 0 && policy.older_than_ms == 0 {
        return Err(SessionError::InvalidRecord(
            "garbage collection requires an explicit retention policy".into(),
        ));
    }
    let directory = directory.as_ref();
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut candidates = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("jsonl"))
        .filter_map(|path| {
            fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|modified| (path, modified.as_millis().min(u128::from(u64::MAX)) as u64))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let mut removed = Vec::new();
    for (index, (path, modified)) in candidates.into_iter().enumerate() {
        if index < policy.retain_newest || now_ms.saturating_sub(modified) < policy.older_than_ms {
            continue;
        }
        fs::remove_file(&path)?;
        removed.push(path);
    }
    Ok(removed)
}
