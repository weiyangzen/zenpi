//! Append-only JSONL session persistence.
//!
//! The session file is intentionally boring: a header followed by independent
//! records.  A process can recover all complete records after a crash, ignore a
//! malformed trailing line, and append again without rewriting the transcript.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    b3::{Handoff, HandoffRecord, RuntimeIntent, unix_ms_to_rfc3339},
    core::Turn,
};

pub const SESSION_VERSION: u32 = 1;
pub const MAX_SESSION_RECORD_BYTES: usize = 1024 * 1024;
/// Maximum journal bytes recovered into one process at startup. This is large
/// enough for long interactive histories while keeping untrusted paths from
/// causing an unbounded allocation before JSON validation begins.
pub const MAX_SESSION_BYTES: usize = 256 * 1024 * 1024;

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
    UnknownOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterruptedOperation {
    pub operation_id: String,
    pub kind: OperationKind,
    pub turn_id: String,
    pub retry_requires_confirmation: bool,
}

/// Durable recovery projection for an operation marker.  A marker is written
/// before dispatch and therefore an absent terminal record is never treated as
/// success.  The projection is deliberately separate from
/// [`InterruptedOperation`] so existing embedders can keep constructing the
/// compact legacy type while recovery owners gain idempotency metadata.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationRecoveryState {
    Interrupted,
    UnknownOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationRecovery {
    pub operation_id: String,
    pub kind: OperationKind,
    pub turn_id: String,
    pub retry_requires_confirmation: bool,
    pub idempotency_key: String,
    pub state: OperationRecoveryState,
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session path is empty")]
    EmptyPath,
    #[error("session path points to a directory: {0}")]
    Directory(PathBuf),
    #[error("session path is a symbolic link: {0}")]
    Symlink(PathBuf),
    #[error("session exceeds the {max} byte startup limit (found {actual} bytes)")]
    LimitExceeded { max: usize, actual: u64 },
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
    RuntimeIntent {
        intent: Box<RuntimeIntent>,
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
    runtime_intents: Vec<RuntimeIntent>,
    events: usize,
    event_values: Vec<Value>,
    records: Vec<SessionRecord>,
    warnings: Vec<RecoveryWarning>,
    needs_separator: bool,
    next_seq: u64,
    observed_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSummary {
    pub path: String,
    pub session_id: String,
    pub created_at_ms: u64,
    pub turn_count: usize,
    pub handoff_count: usize,
    pub handoff_record_count: usize,
    pub runtime_intent_count: usize,
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

/// One validated journal record with its durable sequence number.
///
/// The in-memory turn/event projections above intentionally hide the envelope
/// metadata.  Resume/recovery owners need that metadata to page through the
/// append-only journal without reparsing the file (and without accidentally
/// following a symlink), so the validated envelope is retained separately.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionRecord {
    pub sequence: u64,
    pub kind: String,
    pub value: Value,
}

impl SessionStore {
    /// Open or create a session at `path` and recover all valid records.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        Self::open_with_options(path, true)
    }

    /// Open an existing session without creating, chmodding, or appending to
    /// it. Read-only hosts such as `session list` and `session inspect` use
    /// this path so observation cannot mutate a journal as a side effect.
    pub fn open_existing(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        let path = path.as_ref();
        let metadata = session_path_metadata(path)?;
        if !metadata.is_some_and(|metadata| metadata.is_file()) {
            return Err(SessionError::InvalidRecord(
                "inspection source is not an existing session file".into(),
            ));
        }
        Self::open_with_options(path, false)
    }

    /// Open an existing, structurally valid journal for continued writes.
    /// Unlike [`Self::open`], this never creates a header for an empty,
    /// malformed, or unrelated regular file. Session-switch owners use this
    /// boundary so probing a user path cannot mutate it into a zenpi journal.
    pub fn open_existing_writable(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        let path = path.as_ref();
        let inspected = Self::open_existing(path)?;
        let valid_header = inspected
            .records
            .first()
            .is_some_and(|record| record.kind == "session" && record.sequence == 0);
        if !valid_header || !inspected.warnings.is_empty() {
            return Err(SessionError::InvalidRecord(
                "resume source is not a clean zenpi session journal".into(),
            ));
        }
        Self::open_with_options(path, true)
    }

    fn open_with_options(path: impl AsRef<Path>, writable: bool) -> Result<Self, SessionError> {
        let path = path.as_ref().to_path_buf();
        if path.as_os_str().is_empty() {
            return Err(SessionError::EmptyPath);
        }
        let existing = session_path_metadata(&path)?;
        if existing.as_ref().is_some_and(|metadata| metadata.is_dir()) {
            return Err(SessionError::Directory(path));
        }
        if writable
            && let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }

        // Re-check immediately before opening the file. The first metadata
        // probe prevents a normal symlink from being chmodded; this second
        // probe closes the common replacement race, while O_NOFOLLOW in
        // `read_session_bytes` handles the final open atomically on Unix.
        let _ = session_path_metadata(&path)?;
        let bytes = match read_session_bytes(&path) {
            Ok(bytes) => bytes,
            Err(ReadSessionError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                Vec::new()
            }
            Err(ReadSessionError::Io(error)) => return Err(error.into()),
            Err(ReadSessionError::LimitExceeded { actual }) => {
                return Err(SessionError::LimitExceeded {
                    max: MAX_SESSION_BYTES,
                    actual,
                });
            }
        };
        if writable && existing.is_some() {
            restrict_session_permissions(&path)?;
        }
        let observed_bytes = bytes.len() as u64;
        let had_content = !bytes.is_empty();
        let needs_separator = had_content && !bytes.ends_with(b"\n");
        let text = String::from_utf8(bytes)
            .map_err(|_| SessionError::InvalidRecord("session is not valid UTF-8".into()))?;
        let mut header: Option<SessionHeader> = None;
        let mut turns = Vec::new();
        let mut handoffs = Vec::new();
        let mut handoff_records = Vec::new();
        let mut runtime_intents = Vec::new();
        let mut events = 0;
        let mut event_values = Vec::new();
        let mut records = Vec::new();
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
            let raw_value = value.clone();
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
            if header.is_none() && !matches!(&record, DecodedRecord::Session { .. }) {
                warnings.push(RecoveryWarning {
                    line: line_number,
                    reason: "ignored record before session header".into(),
                });
                continue;
            }
            let record_sequence = sequence.unwrap_or(next_seq);
            let Some(following_sequence) = record_sequence.checked_add(1) else {
                warnings.push(RecoveryWarning {
                    line: line_number,
                    reason: "ignored record with exhausted sequence".into(),
                });
                continue;
            };
            let accepted = match record {
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
                        false
                    } else if header.is_some() {
                        warnings.push(RecoveryWarning {
                            line: line_number,
                            reason: "ignored duplicate session header".into(),
                        });
                        false
                    } else {
                        header = Some(SessionHeader {
                            version,
                            session_id,
                            created_at_ms,
                            cwd,
                        });
                        true
                    }
                }
                DecodedRecord::Turn { turn } => match turn.validate() {
                    Ok(()) => {
                        turns.push(turn);
                        true
                    }
                    Err(error) => {
                        warnings.push(RecoveryWarning {
                            line: line_number,
                            reason: format!("ignored invalid turn: {error}"),
                        });
                        false
                    }
                },
                DecodedRecord::Handoff { handoff } => {
                    if handoff.validate().is_err() {
                        warnings.push(RecoveryWarning {
                            line: line_number,
                            reason: "ignored invalid handoff".into(),
                        });
                        false
                    } else {
                        handoffs.push(handoff);
                        true
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
                        false
                    } else {
                        handoff_records.push(handoff);
                        true
                    }
                }
                DecodedRecord::RuntimeIntent { intent } => {
                    let intent = *intent;
                    if intent.validate().is_err()
                        || header
                            .as_ref()
                            .is_some_and(|item| item.session_id != intent.session_id)
                    {
                        warnings.push(RecoveryWarning {
                            line: line_number,
                            reason: "ignored invalid runtime intent".into(),
                        });
                        false
                    } else {
                        runtime_intents.push(intent);
                        true
                    }
                }
                DecodedRecord::Event { event } => {
                    event_values.push(event);
                    events += 1;
                    true
                }
            };
            if accepted {
                last_seq = Some(record_sequence);
                next_seq = following_sequence;
                let kind = raw_value
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned();
                records.push(SessionRecord {
                    sequence: record_sequence,
                    kind,
                    value: raw_value,
                });
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
            runtime_intents,
            events,
            event_values,
            records,
            warnings,
            needs_separator,
            next_seq,
            observed_bytes,
        };
        if writable && (!had_content || needs_header) {
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
            runtime_intent_count: self.runtime_intents.len(),
            event_count: self.events,
            recovery_warnings: self.warnings.len(),
            next_seq: self.next_seq,
        }
    }

    /// Persist and then retain a turn.  If the write fails, the in-memory
    /// projection is unchanged.
    pub fn append_turn(&mut self, turn: Turn) -> Result<(), SessionError> {
        turn.validate()
            .map_err(|error| SessionError::InvalidRecord(error.to_string()))?;
        if let Some(existing) = self.turns.iter().find(|existing| existing.id == turn.id) {
            if existing == &turn {
                return Ok(());
            }
            return Err(SessionError::InvalidRecord(
                "turn ID conflicts with its durable content".into(),
            ));
        }
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
        if handoff.session_id != self.session_id() {
            return Err(SessionError::InvalidRecord(
                "handoff session does not match journal".into(),
            ));
        }
        self.append_json(&json!({ "kind": "handoff_record", "handoff": &handoff }))?;
        self.handoff_records.push(handoff);
        Ok(())
    }

    pub fn handoff_records(&self) -> &[HandoffRecord] {
        &self.handoff_records
    }

    /// Return inert requests waiting for an external b3ehive runtime owner.
    pub fn runtime_intents(&self) -> &[RuntimeIntent] {
        &self.runtime_intents
    }

    /// Validate and durably append one external-runtime request. No scheduler,
    /// worker, process, or network call is started by this operation.
    pub fn append_runtime_intent(&mut self, intent: RuntimeIntent) -> Result<(), SessionError> {
        intent
            .validate()
            .map_err(|error| SessionError::InvalidRecord(error.to_string()))?;
        if intent.session_id != self.header.session_id {
            return Err(SessionError::InvalidRecord(
                "runtime intent session does not match journal".into(),
            ));
        }
        if let Some(source_request_id) = intent.source_request_id.as_deref()
            && let Some(existing) = self
                .runtime_intents
                .iter()
                .find(|existing| existing.source_request_id.as_deref() == Some(source_request_id))
        {
            if existing.request_fingerprint == intent.request_fingerprint {
                return Ok(());
            }
            return Err(SessionError::InvalidRecord(format!(
                "runtime request ID `{source_request_id}` conflicts with an existing intent"
            )));
        }
        self.append_json(&json!({ "kind": "runtime_intent", "intent": &intent }))?;
        self.runtime_intents.push(intent);
        Ok(())
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

    /// Return the validated append-only envelopes in durable sequence order.
    /// Callers must still apply their own response/display byte budgets before
    /// serializing the values across a transport boundary.
    pub fn records(&self) -> &[SessionRecord] {
        &self.records
    }

    /// Sequence that will be assigned to the next appended journal record.
    pub fn next_sequence(&self) -> u64 {
        self.next_seq
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
        self.begin_operation_with_key(operation, &operation.operation_id)
    }

    /// Persist the key used to identify this exact logical attempt. An
    /// explicit retry must use a new operation ID, even if a remote service
    /// supports reusing its own idempotency key. This API never dispatches it.
    pub fn begin_operation_with_key(
        &mut self,
        operation: &InterruptedOperation,
        idempotency_key: &str,
    ) -> Result<(), SessionError> {
        operation_id(&operation.operation_id)?;
        operation_id(&operation.turn_id)?;
        operation_id(idempotency_key)?;
        if let Some(existing) = operation_markers(&self.event_values).get(&operation.operation_id) {
            if existing.outcome.is_none()
                && existing.operation == *operation
                && existing.idempotency_key == idempotency_key
            {
                return Ok(());
            }
            return Err(SessionError::InvalidRecord(
                "operation ID conflicts with its durable marker or idempotency key".into(),
            ));
        }
        self.append_event(json!({
            "type": "operation_started",
            "operation_id": operation.operation_id,
            "operation_kind": operation.kind,
            "turn_id": operation.turn_id,
            "retry_requires_confirmation": operation.retry_requires_confirmation,
            "idempotency_key": idempotency_key,
        }))
    }

    pub fn finish_operation(
        &mut self,
        operation_id: &str,
        outcome: OperationOutcome,
    ) -> Result<(), SessionError> {
        self::operation_id(operation_id)?;
        if let Some(existing) = self.event_values.iter().rev().find(|event| {
            event["type"] == "operation_finished" && event["operation_id"] == operation_id
        }) {
            if existing["outcome"] == json!(outcome) {
                return Ok(());
            }
            return Err(SessionError::InvalidRecord(
                "operation terminal outcome conflicts with its durable record".into(),
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
            let outcome = self
                .durable_operation_outcome(&operation.operation_id)
                .unwrap_or(OperationOutcome::Interrupted);
            self.finish_operation(&operation.operation_id, outcome)?;
        }
        Ok(interrupted)
    }

    /// Read-only recovery state remains visible after startup acknowledgement.
    /// Local terminal evidence can repair an orphan marker, but an interrupted
    /// provider/tool effect remains unknown until an explicit host decision.
    pub fn operation_recovery(&self) -> Vec<OperationRecovery> {
        operation_markers(&self.event_values)
            .into_values()
            .filter(|marker| {
                !marker.decided
                    && matches!(
                        marker.outcome,
                        None | Some(
                            OperationOutcome::Interrupted | OperationOutcome::UnknownOutcome
                        )
                    )
                    && self
                        .durable_operation_outcome(&marker.operation.operation_id)
                        .is_none()
            })
            .map(|marker| OperationRecovery {
                operation_id: marker.operation.operation_id,
                kind: marker.operation.kind,
                turn_id: marker.operation.turn_id,
                retry_requires_confirmation: true,
                idempotency_key: marker.idempotency_key,
                state: if marker.operation.kind == OperationKind::Compaction {
                    OperationRecoveryState::Interrupted
                } else {
                    OperationRecoveryState::UnknownOutcome
                },
            })
            .collect()
    }

    /// A durable result precedes its terminal marker. Recognize that narrow
    /// crash window without repeating the handler or inventing a result.
    pub fn durable_operation_outcome(&self, operation_id: &str) -> Option<OperationOutcome> {
        let outcome = self.event_values.iter().rev().find_map(|event| {
            (event["operation_id"] == operation_id)
                .then(|| match event["type"].as_str() {
                    Some("tool_execution_finished" | "provider_request_finished") => {
                        known_operation_outcome(&event["outcome"])
                    }
                    Some("context_compacted") => Some(OperationOutcome::Succeeded),
                    _ => None,
                })
                .flatten()
        });
        let outcome = outcome.or_else(|| {
            self.turns.iter().rev().find_map(|turn| {
                let metadata = turn.metadata.as_ref()?;
                if metadata["operation_id"] != operation_id {
                    return None;
                }
                known_operation_outcome(&metadata["outcome"])
            })
        });
        outcome.filter(|value| {
            !matches!(
                value,
                OperationOutcome::UnknownOutcome | OperationOutcome::Interrupted
            )
        })
    }

    /// Store one explicit decision exactly once. It authorizes no immediate
    /// dispatch; the host must submit a new operation for a requested retry.
    pub fn decide_operation_recovery(
        &mut self,
        operation_id: &str,
        decision: crate::core::ToolRecoveryDecision,
    ) -> Result<(), SessionError> {
        self::operation_id(operation_id)?;
        if let Some(existing) = self.events().iter().rev().find(|event| {
            event["type"] == "operation_recovery_decided" && event["operation_id"] == operation_id
        }) {
            if existing["decision"] == json!(decision) {
                return Ok(());
            }
            return Err(SessionError::InvalidRecord(
                "recovery decision conflicts with durable decision".into(),
            ));
        }
        let pending = self
            .operation_recovery()
            .into_iter()
            .find(|pending| pending.operation_id == operation_id)
            .ok_or_else(|| {
                SessionError::InvalidRecord("operation recovery is not pending".into())
            })?;
        self.append_event(json!({
            "type": "operation_recovery_decided", "operation_id": operation_id,
            "idempotency_key": pending.idempotency_key, "decision": decision,
            "execution_started": false, "retry_requires_new_operation": true,
            "implementation_complete": false,
        }))
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
        reject_session_symlink(destination)?;
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
        let inspected = Self::open_existing(&self.path)?;
        require_clean_session(&inspected)?;
        let bytes = read_session_bytes(&self.path).map_err(read_error)?;
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        let mut file = options.open(destination)?;
        if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
            let _ = fs::remove_file(destination);
            return Err(error.into());
        }
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
        reject_session_symlink(destination)?;
        if destination.exists() {
            return Err(SessionError::InvalidRecord(
                "fork destination already exists".into(),
            ));
        }
        require_clean_session(self)?;
        if let Some(parent) = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        options.open(destination)?;
        let result = (|| {
            let mut fork = Self::open(destination)?;
            for record in &self.records {
                match record.kind.as_str() {
                    "turn" => {
                        fork.append_turn(serde_json::from_value(record.value["turn"].clone())?)?
                    }
                    "handoff" => fork
                        .append_handoff(serde_json::from_value(record.value["handoff"].clone())?)?,
                    "handoff_record" => {
                        let mut handoff: HandoffRecord =
                            serde_json::from_value(record.value["handoff"].clone())?;
                        handoff.session_id = fork.session_id().to_owned();
                        handoff.digest = handoff
                            .compute_digest()
                            .map_err(|error| SessionError::InvalidRecord(error.to_string()))?;
                        fork.append_handoff_record(handoff)?;
                    }
                    "event" => {
                        let mut event = record.value["event"].clone();
                        let kind = event["type"].as_str().unwrap_or_default();
                        // A fork copies history, never pending work or the original
                        // session's transport/lifecycle ownership and request ledger.
                        if kind.starts_with("operation_")
                            || kind.starts_with("session_lifecycle")
                            || kind.starts_with("mailbox_")
                            || kind.starts_with("reconnect_")
                            || kind.starts_with("protocol_")
                            || kind.starts_with("request_")
                        {
                            continue;
                        }
                        remap_session_identity(&mut event, self.session_id(), fork.session_id());
                        fork.append_event(event)?;
                    }
                    _ => {}
                }
            }
            Ok(fork)
        })();
        if result.is_err() {
            let _ = fs::remove_file(destination);
        }
        result
    }

    fn append_json(&mut self, value: &Value) -> Result<(), SessionError> {
        if !value.is_object() {
            return Err(SessionError::InvalidRecord(
                "session record must be an object".into(),
            ));
        }
        let sequence = self.next_seq;
        let following_sequence = sequence.checked_add(1).ok_or_else(|| {
            SessionError::InvalidRecord("session sequence space is exhausted".into())
        })?;
        let mut record = value.clone();
        if let Value::Object(fields) = &mut record {
            let now = now_ms();
            fields.insert("schema_version".into(), json!(SESSION_VERSION));
            fields.insert("session_id".into(), json!(self.header.session_id));
            fields.insert("seq".into(), json!(sequence));
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
        reject_session_symlink(&self.path)?;
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(&self.path)?;
        lock_exclusive(&file)?;
        let _transport = guard_transport_writer(&self.path)?;
        // A second writer must reopen its projection rather than append a
        // sequence computed from stale history. The OS releases this lock on
        // close or process death, so a crash cannot strand a PID-file lease.
        if file.metadata()?.len() != self.observed_bytes {
            return Err(SessionError::InvalidRecord(
                "session writer is stale; reopen the journal before retrying".into(),
            ));
        }
        if self.needs_separator {
            file.write_all(b"\n")?;
            self.needs_separator = false;
        }
        file.write_all(&encoded)?;
        file.flush()?;
        file.sync_data()?;
        self.observed_bytes = file.metadata()?.len();
        self.next_seq = following_sequence;
        let kind = record
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        self.records.push(SessionRecord {
            sequence,
            kind,
            value: record,
        });
        Ok(())
    }
}

fn lock_exclusive(file: &File) -> Result<(), SessionError> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Err(SessionError::InvalidRecord(
            "cross-process session locking is unsupported on this platform".into(),
        ))
    }
}

/// Private transport WAL, separate from transcript sequence space. Holding
/// the file owns the session's headless transport until close/process death.
/// Reservations are committed before dispatch; outcomes before stdout.
#[derive(Debug)]
pub(crate) struct ReconnectJournal {
    file: File,
    pub session_path: PathBuf,
    session_id: String,
    sequence: u64,
    bytes: u64,
}

fn local_transport_owners() -> &'static Mutex<BTreeSet<PathBuf>> {
    static OWNERS: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();
    OWNERS.get_or_init(Mutex::default)
}

fn reconnect_path(session: &Path) -> PathBuf {
    let mut path = session.as_os_str().to_owned();
    path.push(".reconnect");
    PathBuf::from(path)
}

fn guard_transport_writer(path: &Path) -> Result<Option<File>, SessionError> {
    let path = fs::canonicalize(path)?;
    if local_transport_owners()
        .lock()
        .map_err(|_| io::Error::other("transport ownership lock poisoned"))?
        .contains(&path)
    {
        return Ok(None);
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let file = match options.open(reconnect_path(&path)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    lock_exclusive(&file)?;
    Ok(Some(file))
}

impl Drop for ReconnectJournal {
    fn drop(&mut self) {
        if let Ok(mut owners) = local_transport_owners().lock() {
            owners.remove(&self.session_path);
        }
    }
}

impl ReconnectJournal {
    const MAX_BYTES: u64 = 128 * 1024 * 1024;

    pub fn open(session: &SessionStore) -> Result<(Self, Vec<Value>), SessionError> {
        let session_path = fs::canonicalize(session.path())?;
        // Match the session append lock order, including startup recovery.
        // A second process cannot mark a live owner's operation interrupted.
        let session_guard = File::open(&session_path)?;
        lock_exclusive(&session_guard)?;
        let path = reconnect_path(&session_path);
        reject_session_symlink(&path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        let mut file = options.open(path)?;
        if !file.metadata()?.is_file() {
            return Err(SessionError::InvalidRecord(
                "reconnect WAL is not a file".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = file.metadata()?;
            if metadata.mode() & 0o077 != 0 || metadata.uid() != unsafe { libc::geteuid() } {
                return Err(SessionError::InvalidRecord(
                    "reconnect WAL must be private and owned by the current user".into(),
                ));
            }
        }
        lock_exclusive(&file)?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(Self::MAX_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > Self::MAX_BYTES {
            return Err(SessionError::LimitExceeded {
                max: Self::MAX_BYTES as usize,
                actual: bytes.len() as u64,
            });
        }
        let valid_len = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |pos| pos + 1);
        let mut records = Vec::new();
        let mut sequence = 0;
        for line in bytes[..valid_len]
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let record: Value = serde_json::from_slice(line)?;
            if record["schema_version"] != 1
                || record["session_id"] != session.session_id()
                || record["sequence"] != sequence
                || !record["event"].is_object()
            {
                return Err(SessionError::InvalidRecord(
                    "invalid reconnect WAL identity or sequence".into(),
                ));
            }
            let payload = serde_json::to_vec(&record["event"])?;
            if record["sha256"] != reconnect_digest(&payload) {
                return Err(SessionError::InvalidRecord(
                    "reconnect WAL checksum mismatch".into(),
                ));
            }
            records.push(record["event"].clone());
            sequence += 1;
        }
        // Only a torn final append is recoverable. A missing terminal after
        // a durable reservation remains unknown, never an implicit retry.
        if valid_len < bytes.len() {
            file.set_len(valid_len as u64)?;
            file.sync_data()?;
        }
        file.seek(SeekFrom::End(0))?;
        local_transport_owners()
            .lock()
            .map_err(|_| io::Error::other("transport ownership lock poisoned"))?
            .insert(session_path.clone());
        Ok((
            Self {
                file,
                session_path,
                session_id: session.session_id().into(),
                sequence,
                bytes: valid_len as u64,
            },
            records,
        ))
    }

    pub fn append(&mut self, event: Value) -> Result<(), SessionError> {
        let digest = reconnect_digest(&serde_json::to_vec(&event)?);
        let mut bytes = serde_json::to_vec(&json!({
            "schema_version": 1, "session_id": self.session_id,
            "sequence": self.sequence, "event": event, "sha256": digest,
        }))?;
        bytes.push(b'\n');
        if bytes.len() > MAX_SESSION_RECORD_BYTES
            || self.bytes + bytes.len() as u64 > Self::MAX_BYTES
        {
            return Err(SessionError::InvalidRecord(
                "reconnect WAL capacity exhausted; refusing untracked execution".into(),
            ));
        }
        self.file.write_all(&bytes)?;
        self.file.sync_data()?;
        self.bytes += bytes.len() as u64;
        self.sequence += 1;
        Ok(())
    }
}

fn reconnect_digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
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

#[derive(Debug, Clone)]
struct OperationMarker {
    operation: InterruptedOperation,
    idempotency_key: String,
    outcome: Option<OperationOutcome>,
    decided: bool,
}

fn operation_id(value: &str) -> Result<(), SessionError> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(SessionError::InvalidRecord(
            "operation ID or idempotency key is empty, too large, or contains control characters"
                .into(),
        ));
    }
    Ok(())
}

fn known_operation_outcome(value: &Value) -> Option<OperationOutcome> {
    match value.as_str()? {
        "succeeded" => Some(OperationOutcome::Succeeded),
        "failed" => Some(OperationOutcome::Failed),
        "cancelled" => Some(OperationOutcome::Cancelled),
        "interrupted" => Some(OperationOutcome::Interrupted),
        "unknown_outcome" => Some(OperationOutcome::UnknownOutcome),
        _ => None,
    }
}

fn operation_markers(events: &[Value]) -> BTreeMap<String, OperationMarker> {
    let mut markers = BTreeMap::new();
    for event in events {
        match event.get("type").and_then(Value::as_str) {
            Some("operation_started") => {
                let (Some(operation_id), Some(turn_id), Some(kind)) = (
                    event.get("operation_id").and_then(Value::as_str),
                    event.get("turn_id").and_then(Value::as_str),
                    event
                        .get("operation_kind")
                        .cloned()
                        .and_then(|v| serde_json::from_value(v).ok()),
                ) else {
                    continue;
                };
                let operation = InterruptedOperation {
                    operation_id: operation_id.to_owned(),
                    kind,
                    turn_id: turn_id.to_owned(),
                    retry_requires_confirmation: event
                        .get("retry_requires_confirmation")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                };
                let idempotency_key = event
                    .get("idempotency_key")
                    .and_then(Value::as_str)
                    .unwrap_or(operation_id)
                    .to_owned();
                markers
                    .entry(operation_id.to_owned())
                    .or_insert(OperationMarker {
                        operation,
                        idempotency_key,
                        outcome: None,
                        decided: false,
                    });
            }
            Some(
                "operation_finished" | "tool_execution_finished" | "provider_request_finished",
            ) => {
                if let Some(operation_id) = event.get("operation_id").and_then(Value::as_str)
                    && let Some(marker) = markers.get_mut(operation_id)
                {
                    marker.outcome = known_operation_outcome(&event["outcome"]);
                }
            }
            Some("operation_recovery_decided" | "tool_recovery_decided") => {
                if let Some(operation_id) = event.get("operation_id").and_then(Value::as_str)
                    && let Some(marker) = markers.get_mut(operation_id)
                {
                    marker.decided = true;
                }
            }
            _ => {}
        }
    }
    markers
}

#[cfg(unix)]
fn restrict_session_permissions(path: &Path) -> io::Result<()> {
    crate::security::restrict_private_file(path)
}

#[cfg(not(unix))]
fn restrict_session_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Inspect the final session path without following a symbolic link. A
/// missing path is represented by `None` so callers can create a new journal;
/// an existing link is always rejected before any read, chmod, or append.
fn session_path_metadata(path: &Path) -> Result<Option<fs::Metadata>, SessionError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(SessionError::Symlink(path.to_owned()))
        }
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn reject_session_symlink(path: &Path) -> Result<(), SessionError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(SessionError::Symlink(path.to_owned()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

enum ReadSessionError {
    Io(io::Error),
    LimitExceeded { actual: u64 },
}

fn read_session_bytes(path: &Path) -> Result<Vec<u8>, ReadSessionError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let file = options.open(path).map_err(ReadSessionError::Io)?;
    let metadata = file.metadata().map_err(ReadSessionError::Io)?;
    if metadata.len() > MAX_SESSION_BYTES as u64 {
        return Err(ReadSessionError::LimitExceeded {
            actual: metadata.len(),
        });
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((MAX_SESSION_BYTES as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(ReadSessionError::Io)?;
    if bytes.len() > MAX_SESSION_BYTES {
        return Err(ReadSessionError::LimitExceeded {
            actual: bytes.len() as u64,
        });
    }
    Ok(bytes)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn generate_id(prefix: &str) -> String {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "{prefix}-{}-{}-{}",
        now_ms(),
        std::process::id(),
        SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
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

/// Upper bound for one GC directory scan. Refusing a larger directory is
/// safer than retaining an unbounded candidate list or partially deleting it.
pub const MAX_GC_CANDIDATES: usize = 4_096;
/// Keep owner receipts small enough for both the TUI transcript and JSONL
/// replay cache. The collector refuses to delete more than this many files in
/// one invocation so every deletion can be reported exactly.
pub const MAX_GC_REMOVALS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GarbageCollectionReport {
    pub removed: Vec<PathBuf>,
    pub inspected: usize,
    pub skipped_unowned: usize,
}

pub fn list_sessions(directory: impl AsRef<Path>) -> Result<Vec<SessionSummary>, SessionError> {
    let directory = directory.as_ref();
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(SessionError::Symlink(directory.to_owned()));
        }
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() {
        return Err(SessionError::Directory(directory.to_owned()));
    }
    let mut paths = Vec::new();
    for (index, entry) in fs::read_dir(directory)?.enumerate() {
        if index >= MAX_GC_CANDIDATES {
            return Err(SessionError::InvalidRecord(
                "session directory entry limit exceeded".into(),
            ));
        }
        let path = entry?.path();
        // A workspace-local domain store may live beside its session journal.
        // It is JSONL too, but it is not a session and opening it through
        // `SessionStore` would append a session header and corrupt the store.
        if path.file_name().and_then(|name| name.to_str())
            != Some(crate::domain_store::DOMAIN_STORE_FILE_NAME)
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
        {
            paths.push(path);
        }
    }
    paths.sort();
    paths
        .into_iter()
        .map(|path| inspect_session(path).map(|inspection| inspection.summary))
        .collect()
}

/// List sessions in a configured directory and include an optional legacy
/// default journal. Older zenpi releases wrote `~/.zenpi/session.jsonl` while
/// the session browser scans `~/.zenpi/sessions/`; keeping the fallback here
/// makes both layouts discoverable without moving or rewriting either file.
pub fn list_sessions_with_fallback(
    directory: impl AsRef<Path>,
    fallback: impl AsRef<Path>,
) -> Result<Vec<SessionSummary>, SessionError> {
    let fallback = fallback.as_ref();
    let mut sessions = list_sessions(directory)?;
    match fs::symlink_metadata(fallback) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(SessionError::Symlink(fallback.to_owned()));
        }
        Ok(metadata) if metadata.is_file() => {
            let summary = inspect_session(fallback)?.summary;
            let duplicate = sessions.iter().any(|item| {
                Path::new(&item.path)
                    .canonicalize()
                    .ok()
                    .zip(fallback.canonicalize().ok())
                    .is_some_and(|(left, right)| left == right)
            });
            if !duplicate {
                sessions.push(summary);
            }
        }
        Ok(_) => {
            return Err(SessionError::InvalidRecord(
                "session fallback is not a regular file".into(),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    sessions.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(sessions)
}

/// Open an existing journal for inspection. Unlike `SessionStore::open`, this
/// rejects a missing source, preventing a typo in a read-only CLI command from
/// creating an empty session.
pub fn inspect_session(path: impl AsRef<Path>) -> Result<SessionInspection, SessionError> {
    let path = path.as_ref();
    let metadata = session_path_metadata(path)?;
    if !metadata.is_some_and(|metadata| metadata.is_file()) {
        return Err(SessionError::InvalidRecord(
            "inspection source is not an existing session file".into(),
        ));
    }
    let store = SessionStore::open_existing(path)?;
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
    let source = SessionStore::open_existing(source)?;
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
    let active = SessionStore::default_path();
    garbage_collect_sessions_with_active(directory, policy, Some(&active), now_ms)
        .map(|report| report.removed)
}

/// Garbage collect only clean, zenpi-owned session journals.
///
/// `active_session` is checked by path and (where available) file identity;
/// an active journal is never removed. The scan is fail-closed for symlinks,
/// unexpected file types, candidate-count overflow, and a removal set larger
/// than the bounded receipt can describe. Domain snapshots are skipped rather
/// than treated as sessions. No source is opened writable and no parent path
/// is followed through a final symlink.
pub fn garbage_collect_sessions_with_active(
    directory: impl AsRef<Path>,
    policy: GarbageCollectionPolicy,
    active_session: Option<&Path>,
    now_ms: u64,
) -> Result<GarbageCollectionReport, SessionError> {
    if policy.retain_newest == 0 && policy.older_than_ms == 0 {
        return Err(SessionError::InvalidRecord(
            "garbage collection requires an explicit retention policy".into(),
        ));
    }
    let directory = directory.as_ref();
    let directory_metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(GarbageCollectionReport {
                removed: Vec::new(),
                inspected: 0,
                skipped_unowned: 0,
            });
        }
        Err(error) => return Err(error.into()),
    };
    if directory_metadata.file_type().is_symlink() {
        return Err(SessionError::Symlink(directory.to_owned()));
    }
    if !directory_metadata.is_dir() {
        return Err(SessionError::Directory(directory.to_owned()));
    }
    let mut candidates = Vec::new();
    let mut skipped_unowned = 0_usize;
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        // Inspect every directory entry before filtering by suffix. A
        // symlink with a non-JSONL name is still an alias in the managed
        // directory and must fail closed rather than being silently ignored.
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(SessionError::Symlink(path));
        }
        // Domain snapshots use JSONL too, but are not sessions. Never let a
        // retention command remove the store that owns Blueprint/Goal/Learn
        // records.
        if path.file_name().and_then(|name| name.to_str())
            == Some(crate::domain_store::DOMAIN_STORE_FILE_NAME)
            || path.extension().and_then(|value| value.to_str()) != Some("jsonl")
        {
            continue;
        }
        // Fail closed for aliases and unexpected node types. In particular,
        // do not follow a symlink while selecting an item for deletion.
        if !metadata.is_file() {
            return Err(SessionError::InvalidRecord(
                "garbage collection candidate is not a regular file".into(),
            ));
        }
        // A `.jsonl` suffix is not ownership proof. Only a clean journal with
        // a valid session header may be selected; malformed or unrelated
        // files remain untouched and are counted in the bounded receipt.
        let owned = clean_owned_session(&path)?;
        if !owned {
            skipped_unowned = skipped_unowned.saturating_add(1);
            continue;
        }
        if active_session.is_some_and(|active| same_file_or_path(&path, active)) {
            return Err(SessionError::InvalidRecord(
                "garbage collection cannot remove the active session".into(),
            ));
        }
        let modified = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| {
                SessionError::InvalidRecord(
                    "garbage collection candidate has an invalid mtime".into(),
                )
            })?;
        candidates.push((path, modified.as_millis().min(u128::from(u64::MAX)) as u64));
        if candidates.len() > MAX_GC_CANDIDATES {
            return Err(SessionError::InvalidRecord(format!(
                "garbage collection candidate limit exceeded ({MAX_GC_CANDIDATES})"
            )));
        }
    }
    candidates.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let mut selected = Vec::new();
    for (index, (path, modified)) in candidates.iter().enumerate() {
        if index < policy.retain_newest || now_ms.saturating_sub(*modified) < policy.older_than_ms {
            continue;
        }
        selected.push(path.clone());
    }
    if selected.len() > MAX_GC_REMOVALS {
        return Err(SessionError::InvalidRecord(format!(
            "garbage collection removal limit exceeded ({MAX_GC_REMOVALS})"
        )));
    }
    // Recheck every selected path immediately before mutation. This closes
    // the common replacement race where a valid journal is swapped for a
    // symlink or unrelated JSONL file after the initial directory scan.
    for path in &selected {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(SessionError::Symlink(path.clone()));
        }
        if !metadata.is_file() || !clean_owned_session(path)? {
            return Err(SessionError::InvalidRecord(
                "garbage collection candidate changed before removal".into(),
            ));
        }
        if active_session.is_some_and(|active| same_file_or_path(path, active)) {
            return Err(SessionError::InvalidRecord(
                "garbage collection cannot remove the active session".into(),
            ));
        }
        if mailbox_path(path)?.try_exists()? {
            return Err(SessionError::InvalidRecord(
                "garbage collection cannot orphan a durable session mailbox".into(),
            ));
        }
    }
    for path in selected.iter() {
        fs::remove_file(path)?;
    }
    Ok(GarbageCollectionReport {
        removed: selected,
        inspected: candidates.len(),
        skipped_unowned,
    })
}

fn same_file_or_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(left), Ok(right)) = (fs::metadata(left), fs::metadata(right))
            && left.dev() == right.dev()
            && left.ino() == right.ino()
        {
            return true;
        }
    }
    left.canonicalize().ok() == right.canonicalize().ok()
}

fn clean_owned_session(path: &Path) -> Result<bool, SessionError> {
    match SessionStore::open_existing(path) {
        Ok(store) => Ok(store.records().first().is_some_and(|record| {
            record.kind == "session" && record.sequence == 0 && store.recovery_warnings().is_empty()
        })),
        Err(SessionError::Symlink(path)) => Err(SessionError::Symlink(path)),
        Err(_) => Ok(false),
    }
}

fn require_clean_session(store: &SessionStore) -> Result<(), SessionError> {
    if store
        .records
        .first()
        .is_none_or(|record| record.kind != "session" || record.sequence != 0)
        || !store.warnings.is_empty()
    {
        return Err(SessionError::InvalidRecord(
            "operation requires a clean session journal".into(),
        ));
    }
    Ok(())
}

fn read_error(error: ReadSessionError) -> SessionError {
    match error {
        ReadSessionError::Io(error) => error.into(),
        ReadSessionError::LimitExceeded { actual } => SessionError::LimitExceeded {
            max: MAX_SESSION_BYTES,
            actual,
        },
    }
}

fn remap_session_identity(value: &mut Value, source: &str, destination: &str) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if (key == "session_id" || key.ends_with("_session_id"))
                    && value.as_str() == Some(source)
                {
                    *value = json!(destination);
                } else {
                    remap_session_identity(value, source, destination);
                }
            }
        }
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| remap_session_identity(value, source, destination)),
        _ => {}
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionCatalogEntry {
    pub summary: SessionSummary,
    pub archived: bool,
    pub last_activity_ms: u64,
}

impl SessionStore {
    pub fn is_archived(&self) -> bool {
        self.events()
            .iter()
            .rev()
            .find(|event| event["type"] == "session_lifecycle")
            .is_some_and(|event| event["archived"] == true)
    }

    pub fn catalog_entry(&self) -> SessionCatalogEntry {
        SessionCatalogEntry {
            summary: self.summary(),
            archived: self.is_archived(),
            last_activity_ms: self
                .records
                .iter()
                .filter_map(|record| record.value["timestamp_ms"].as_u64())
                .max()
                .unwrap_or(self.header.created_at_ms),
        }
    }

    pub fn set_archived(&mut self, archived: bool) -> Result<(), SessionError> {
        require_clean_session(self)?;
        if self.is_archived() != archived {
            self.append_event(json!({"type": "session_lifecycle", "archived": archived}))?;
        }
        Ok(())
    }
}

/// A local catalog, not a claim that these sessions have running agents.
pub fn session_catalog(
    directory: impl AsRef<Path>,
    include_archived: bool,
) -> Result<Vec<SessionCatalogEntry>, SessionError> {
    let mut entries = Vec::new();
    for summary in list_sessions(directory)? {
        if entries.len() >= MAX_GC_CANDIDATES {
            return Err(SessionError::InvalidRecord(
                "session catalog limit exceeded".into(),
            ));
        }
        let store = SessionStore::open_existing(summary.path)?;
        require_clean_session(&store)?;
        if include_archived || !store.is_archived() {
            entries.push(store.catalog_entry());
        }
    }
    entries.sort_by(|left, right| {
        right
            .last_activity_ms
            .cmp(&left.last_activity_ms)
            .then_with(|| left.summary.path.cmp(&right.summary.path))
    });
    Ok(entries)
}

pub fn resume_last_session(directory: impl AsRef<Path>) -> Result<SessionStore, SessionError> {
    let latest = session_catalog(directory, false)?
        .into_iter()
        .next()
        .ok_or_else(|| SessionError::InvalidRecord("no unarchived session to resume".into()))?;
    SessionStore::open_existing_writable(latest.summary.path)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SessionLifecycleRequest {
    List {
        directory: PathBuf,
        #[serde(default)]
        include_archived: bool,
    },
    Agents {
        directory: PathBuf,
    },
    Inspect {
        path: PathBuf,
    },
    ResumeLast {
        directory: PathBuf,
    },
    Fork {
        source: PathBuf,
        destination: PathBuf,
    },
    Export {
        source: PathBuf,
        destination: PathBuf,
    },
    Import {
        source: PathBuf,
        destination: PathBuf,
    },
    /// Migration writes a verified copy in the current schema, preserving
    /// identity and source. Removing the source is a separate confirmed delete.
    Migrate {
        source: PathBuf,
        destination: PathBuf,
    },
    Archive {
        path: PathBuf,
    },
    Unarchive {
        path: PathBuf,
    },
    Delete {
        path: PathBuf,
        #[serde(default)]
        confirmed: bool,
    },
    Queue {
        source: PathBuf,
        recipient: PathBuf,
        request_id: String,
        payload: Value,
        ttl_ms: u64,
    },
    Gc {
        directory: PathBuf,
        retain_newest: usize,
        older_than_ms: u64,
        #[serde(default)]
        confirmed: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionLifecycleReceipt {
    Catalog {
        sessions: Vec<SessionCatalogEntry>,
    },
    Inspection {
        inspection: SessionInspection,
    },
    Session {
        session: SessionCatalogEntry,
    },
    Deleted {
        path: PathBuf,
    },
    Queued {
        message: MailboxMessage,
    },
    GarbageCollected {
        removed: Vec<PathBuf>,
        inspected: usize,
        skipped_unowned: usize,
    },
}

pub fn execute_session_lifecycle(
    request: SessionLifecycleRequest,
    active_session: Option<&Path>,
    now_ms: u64,
) -> Result<SessionLifecycleReceipt, SessionError> {
    use SessionLifecycleReceipt as Receipt;
    use SessionLifecycleRequest as Request;
    let session = match request {
        Request::List {
            directory,
            include_archived,
        } => {
            return Ok(Receipt::Catalog {
                sessions: session_catalog(directory, include_archived)?,
            });
        }
        Request::Agents { directory } => {
            return Ok(Receipt::Catalog {
                sessions: session_catalog(directory, false)?,
            });
        }
        Request::Inspect { path } => {
            return Ok(Receipt::Inspection {
                inspection: inspect_session(path)?,
            });
        }
        Request::ResumeLast { directory } => resume_last_session(directory)?,
        Request::Fork {
            source,
            destination,
        } => SessionStore::open_existing(source)?.fork_to(destination)?,
        Request::Export {
            source,
            destination,
        } => {
            SessionStore::open_existing(source)?.export_to(&destination)?;
            SessionStore::open_existing(destination)?
        }
        Request::Import {
            source,
            destination,
        }
        | Request::Migrate {
            source,
            destination,
        } => import_session(source, destination)?,
        Request::Archive { path } => {
            return Ok(Receipt::Session {
                session: set_session_archived(path, true, active_session)?,
            });
        }
        Request::Unarchive { path } => {
            return Ok(Receipt::Session {
                session: set_session_archived(path, false, active_session)?,
            });
        }
        Request::Delete { path, confirmed } => {
            if !confirmed {
                return Err(SessionError::InvalidRecord(
                    "delete requires explicit confirmation".into(),
                ));
            }
            reject_active_session(&path, active_session)?;
            let store = SessionStore::open_existing(&path)?;
            require_clean_session(&store)?;
            // Refuse to orphan durable communication. Archive is available
            // until a separate mailbox retention decision has been made.
            if mailbox_path(&path)?.exists() {
                return Err(SessionError::InvalidRecord(
                    "delete requires retaining or removing the session mailbox first".into(),
                ));
            }
            fs::remove_file(&path)?;
            return Ok(Receipt::Deleted { path });
        }
        Request::Queue {
            source,
            recipient,
            request_id,
            payload,
            ttl_ms,
        } => {
            let sender = SessionStore::open_existing(source)?;
            let mailbox = SessionMailbox::open(recipient)?;
            return Ok(Receipt::Queued {
                message: mailbox.enqueue(&sender, &request_id, payload, ttl_ms, now_ms)?,
            });
        }
        Request::Gc {
            directory,
            retain_newest,
            older_than_ms,
            confirmed,
        } => {
            if !confirmed {
                return Err(SessionError::InvalidRecord(
                    "GC requires explicit confirmation".into(),
                ));
            }
            let report = garbage_collect_sessions_with_active(
                directory,
                GarbageCollectionPolicy {
                    retain_newest,
                    older_than_ms,
                },
                active_session,
                now_ms,
            )?;
            return Ok(Receipt::GarbageCollected {
                removed: report.removed,
                inspected: report.inspected,
                skipped_unowned: report.skipped_unowned,
            });
        }
    };
    Ok(Receipt::Session {
        session: session.catalog_entry(),
    })
}

fn reject_active_session(path: &Path, active: Option<&Path>) -> Result<(), SessionError> {
    if active.is_some_and(|active| same_file_or_path(path, active)) {
        return Err(SessionError::InvalidRecord(
            "operation cannot modify the active session lifecycle".into(),
        ));
    }
    Ok(())
}

pub fn set_session_archived(
    path: impl AsRef<Path>,
    archived: bool,
    active: Option<&Path>,
) -> Result<SessionCatalogEntry, SessionError> {
    let path = path.as_ref();
    if archived {
        reject_active_session(path, active)?;
    }
    let mut session = SessionStore::open_existing_writable(path)?;
    session.set_archived(archived)?;
    Ok(session.catalog_entry())
}

pub const MAX_MAILBOX_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_MAILBOX_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_MAILBOX_MESSAGES: usize = 256;
pub const MAX_MAILBOX_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MailboxStatus {
    Queued,
    Acknowledged,
    Claimed,
    Succeeded,
    Failed,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MailboxMessage {
    pub sender_session_id: String,
    pub recipient_session_id: String,
    pub request_id: String,
    pub digest: String,
    pub payload: Value,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    /// The first delivery sequence, stable across ACK and result updates.
    pub sequence: u64,
    pub status: MailboxStatus,
    pub claim_token: Option<String>,
    pub result: Option<Value>,
}

impl MailboxMessage {
    fn digest(&self) -> Result<String, SessionError> {
        use sha2::{Digest, Sha256};
        let unsigned = json!({"sender": self.sender_session_id, "recipient": self.recipient_session_id,
            "request_id": self.request_id, "payload": self.payload, "created_at_ms": self.created_at_ms,
            "expires_at_ms": self.expires_at_ms, "sequence": self.sequence});
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&unsigned)?)
        ))
    }

    fn validate(&self, recipient: &str) -> Result<(), SessionError> {
        mailbox_id(&self.sender_session_id)?;
        mailbox_id(&self.recipient_session_id)?;
        mailbox_id(&self.request_id)?;
        validate_mailbox_payload(&self.payload)?;
        if let Some(result) = &self.result {
            validate_mailbox_payload(result)?;
        }
        if let Some(token) = &self.claim_token {
            mailbox_id(token)?;
        }
        if self.recipient_session_id != recipient
            || self.sequence == 0
            || self.expires_at_ms <= self.created_at_ms
            || self.expires_at_ms - self.created_at_ms > MAX_MAILBOX_TTL_MS
            || self.digest != self.digest()?
        {
            return Err(SessionError::InvalidRecord(
                "invalid mailbox identity, TTL, sequence, or digest".into(),
            ));
        }
        let valid_state = match self.status {
            MailboxStatus::Queued | MailboxStatus::Acknowledged => {
                self.claim_token.is_none() && self.result.is_none()
            }
            MailboxStatus::Claimed => self.claim_token.is_some() && self.result.is_none(),
            MailboxStatus::Succeeded | MailboxStatus::Failed => {
                self.claim_token.is_some() && self.result.is_some()
            }
            MailboxStatus::Expired => false, // Expiration is a clock-derived view, not a new execution receipt.
        };
        if !valid_state {
            return Err(SessionError::InvalidRecord("invalid mailbox state".into()));
        }
        Ok(())
    }

    fn at_time(mut self, now: u64) -> Self {
        if now >= self.expires_at_ms
            && !matches!(
                self.status,
                MailboxStatus::Succeeded | MailboxStatus::Failed
            )
        {
            self.status = MailboxStatus::Expired;
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MailboxAction {
    Acknowledge,
    Claim { token: String },
    Complete { token: String, result: Value },
    Fail { token: String, error: Value },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MailboxPage {
    pub messages: Vec<MailboxMessage>,
    /// Pass this value as `after_sequence` to read the following page.
    pub next_sequence: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MailboxRecord {
    schema_version: u32,
    sequence: u64,
    message: MailboxMessage,
}

/// Private, local, addressed communication with no provider call or daemon.
/// Every mutation takes the same inode lock, reloads the bounded journal, and
/// appends one synced transition. A claimed message is never retried implicitly.
#[derive(Debug)]
pub struct SessionMailbox {
    path: PathBuf,
    recipient_path: PathBuf,
    recipient_session_id: String,
    workspace: PathBuf,
}

impl SessionMailbox {
    pub fn open(recipient_path: impl AsRef<Path>) -> Result<Self, SessionError> {
        let recipient_path = recipient_path.as_ref();
        let recipient = SessionStore::open_existing(recipient_path)?;
        require_clean_session(&recipient)?;
        Ok(Self {
            path: mailbox_path(recipient_path)?,
            recipient_path: recipient_path.to_owned(),
            recipient_session_id: recipient.session_id().to_owned(),
            workspace: fs::canonicalize(&recipient.header.cwd)?,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn check_access(&self, actor: &SessionStore, recipient_only: bool) -> Result<(), SessionError> {
        let expected_actor = actor.session_id();
        let actor = SessionStore::open_existing(actor.path())?;
        let recipient = SessionStore::open_existing(&self.recipient_path)?;
        require_clean_session(&actor)?;
        require_clean_session(&recipient)?;
        if actor.session_id() != expected_actor
            || recipient.session_id() != self.recipient_session_id
            || fs::canonicalize(&recipient.header.cwd)? != self.workspace
            || fs::canonicalize(&actor.header.cwd)? != self.workspace
            || (recipient_only && actor.session_id() != self.recipient_session_id)
        {
            return Err(SessionError::InvalidRecord(
                "mailbox access requires the addressed session in the same workspace".into(),
            ));
        }
        Ok(())
    }

    /// Address a message by its immutable digest, avoiding ambiguity when
    /// multiple senders choose the same request ID.
    pub fn find_message(
        &self,
        recipient: &SessionStore,
        digest: &str,
        now: u64,
    ) -> Result<MailboxMessage, SessionError> {
        self.check_access(recipient, true)?;
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(SessionError::InvalidRecord(
                "mailbox message ID must be an envelope digest".into(),
            ));
        }
        let mut journal = self.locked(false)?;
        let (messages, _) = self.recover(&mut journal.file)?;
        messages
            .into_iter()
            .find(|message| message.digest == digest)
            .map(|message| message.at_time(now))
            .ok_or_else(|| SessionError::InvalidRecord("mailbox message does not exist".into()))
    }

    pub fn enqueue(
        &self,
        sender: &SessionStore,
        request_id: &str,
        payload: Value,
        ttl_ms: u64,
        now: u64,
    ) -> Result<MailboxMessage, SessionError> {
        self.check_access(sender, false)?;
        mailbox_id(request_id)?;
        validate_mailbox_payload(&payload)?;
        if ttl_ms == 0 || ttl_ms > MAX_MAILBOX_TTL_MS {
            return Err(SessionError::InvalidRecord(
                "mailbox TTL must be positive and at most seven days".into(),
            ));
        }
        if SessionStore::open_existing(&self.recipient_path)?.is_archived() {
            return Err(SessionError::InvalidRecord(
                "cannot queue work for an archived session".into(),
            ));
        }
        let mut journal = self.locked(true)?;
        let (messages, next_sequence) = self.recover(&mut journal.file)?;
        if let Some(existing) = messages.iter().find(|message| {
            message.sender_session_id == sender.session_id() && message.request_id == request_id
        }) {
            if existing.payload != payload
                || existing.expires_at_ms - existing.created_at_ms != ttl_ms
            {
                return Err(SessionError::InvalidRecord(
                    "mailbox request ID conflicts with its durable payload or TTL".into(),
                ));
            }
            return Ok(existing.clone().at_time(now));
        }
        if messages.len() >= MAX_MAILBOX_MESSAGES {
            return Err(SessionError::InvalidRecord(
                "mailbox message limit exceeded".into(),
            ));
        }
        let mut message = MailboxMessage {
            sender_session_id: sender.session_id().into(),
            recipient_session_id: self.recipient_session_id.clone(),
            request_id: request_id.into(),
            digest: String::new(),
            payload,
            created_at_ms: now,
            expires_at_ms: now
                .checked_add(ttl_ms)
                .ok_or_else(|| SessionError::InvalidRecord("mailbox expiry overflow".into()))?,
            sequence: next_sequence,
            status: MailboxStatus::Queued,
            claim_token: None,
            result: None,
        };
        message.digest = message.digest()?;
        self.append(&mut journal.file, next_sequence, &message)?;
        Ok(message)
    }

    pub fn list(
        &self,
        recipient: &SessionStore,
        after_sequence: u64,
        limit: usize,
        now: u64,
    ) -> Result<MailboxPage, SessionError> {
        self.check_access(recipient, true)?;
        if limit == 0 || limit > 64 {
            return Err(SessionError::InvalidRecord(
                "mailbox page limit must be 1..=64".into(),
            ));
        }
        if !self.path.try_exists()? {
            return Ok(MailboxPage {
                messages: Vec::new(),
                next_sequence: None,
            });
        }
        let mut journal = self.locked(false)?;
        let (messages, _) = self.recover(&mut journal.file)?;
        let mut page = Vec::new();
        let mut bytes = 0;
        let mut next_sequence = None;
        for message in messages
            .into_iter()
            .filter(|message| message.sequence > after_sequence)
        {
            let message = message.at_time(now);
            let size = serde_json::to_vec(&message)?.len();
            if page.len() >= limit || bytes + size > 256 * 1024 {
                next_sequence = page.last().map(|message: &MailboxMessage| message.sequence);
                break;
            }
            bytes += size;
            page.push(message);
        }
        Ok(MailboxPage {
            messages: page,
            next_sequence,
        })
    }

    pub fn update(
        &self,
        recipient: &SessionStore,
        sender_session_id: &str,
        request_id: &str,
        action: MailboxAction,
        now: u64,
    ) -> Result<MailboxMessage, SessionError> {
        self.check_access(recipient, true)?;
        mailbox_id(sender_session_id)?;
        mailbox_id(request_id)?;
        let mut journal = self.locked(false)?;
        let (messages, next_sequence) = self.recover(&mut journal.file)?;
        let existing = messages
            .iter()
            .find(|message| {
                message.sender_session_id == sender_session_id && message.request_id == request_id
            })
            .ok_or_else(|| SessionError::InvalidRecord("mailbox request not found".into()))?;
        let mut updated = existing.clone();
        match action {
            MailboxAction::Acknowledge => updated.status = MailboxStatus::Acknowledged,
            MailboxAction::Claim { token } => {
                mailbox_id(&token)?;
                updated.status = MailboxStatus::Claimed;
                updated.claim_token = Some(token);
            }
            MailboxAction::Complete { token, result } => {
                mailbox_id(&token)?;
                validate_mailbox_payload(&result)?;
                updated.status = MailboxStatus::Succeeded;
                updated.claim_token = Some(token);
                updated.result = Some(result);
            }
            MailboxAction::Fail { token, error } => {
                mailbox_id(&token)?;
                validate_mailbox_payload(&error)?;
                updated.status = MailboxStatus::Failed;
                updated.claim_token = Some(token);
                updated.result = Some(error);
            }
        }
        if updated == *existing {
            return Ok(existing.clone().at_time(now));
        }
        if existing.clone().at_time(now).status == MailboxStatus::Expired {
            return Err(SessionError::InvalidRecord(
                "expired mailbox request cannot be executed or implicitly retried".into(),
            ));
        }
        validate_mailbox_transition(existing, &updated)?;
        self.append(&mut journal.file, next_sequence, &updated)?;
        Ok(updated)
    }

    fn locked(&self, create: bool) -> Result<MailboxLock, SessionError> {
        reject_session_symlink(&self.path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(create);
        #[cfg(unix)]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        let file = options.open(&self.path)?;
        if !file.metadata()?.is_file() {
            return Err(SessionError::InvalidRecord(
                "mailbox is not a regular file".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::{fd::AsRawFd, unix::fs::MetadataExt};
            let metadata = file.metadata()?;
            if metadata.mode() & 0o077 != 0 || metadata.uid() != unsafe { libc::geteuid() } {
                return Err(SessionError::InvalidRecord(
                    "mailbox must be private and owned by the current user".into(),
                ));
            }
            // Nonblocking locks keep the headless and TUI owners responsive.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(io::Error::last_os_error().into());
            }
            Ok(MailboxLock { file })
        }
        #[cfg(not(unix))]
        {
            let _ = file;
            Err(SessionError::InvalidRecord(
                "mailbox locking is unsupported on this platform".into(),
            ))
        }
    }

    fn recover(&self, file: &mut File) -> Result<(Vec<MailboxMessage>, u64), SessionError> {
        if file.metadata()?.len() > MAX_MAILBOX_BYTES as u64 {
            return Err(SessionError::LimitExceeded {
                max: MAX_MAILBOX_BYTES,
                actual: file.metadata()?.len(),
            });
        }
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.take(MAX_MAILBOX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_MAILBOX_BYTES {
            return Err(SessionError::LimitExceeded {
                max: MAX_MAILBOX_BYTES,
                actual: bytes.len() as u64,
            });
        }
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            return Err(SessionError::InvalidRecord(
                "mailbox has an interrupted append; explicit repair required".into(),
            ));
        }
        let mut messages: Vec<MailboxMessage> = Vec::new();
        let mut next = 1;
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            if line.len() > MAX_SESSION_RECORD_BYTES {
                return Err(SessionError::InvalidRecord(
                    "mailbox record limit exceeded".into(),
                ));
            }
            let record: MailboxRecord = serde_json::from_slice(line)?;
            if record.schema_version != 1 || record.sequence != next {
                return Err(SessionError::InvalidRecord(
                    "mailbox schema or sequence gap".into(),
                ));
            }
            record.message.validate(&self.recipient_session_id)?;
            if let Some(existing) = messages.iter_mut().find(|message| {
                message.sender_session_id == record.message.sender_session_id
                    && message.request_id == record.message.request_id
            }) {
                validate_mailbox_transition(existing, &record.message)?;
                *existing = record.message;
            } else {
                if messages.len() >= MAX_MAILBOX_MESSAGES
                    || record.message.status != MailboxStatus::Queued
                    || record.message.sequence != next
                {
                    return Err(SessionError::InvalidRecord(
                        "mailbox must start with a bounded queued envelope".into(),
                    ));
                }
                messages.push(record.message);
            }
            next += 1;
        }
        Ok((messages, next))
    }

    fn append(
        &self,
        file: &mut File,
        sequence: u64,
        message: &MailboxMessage,
    ) -> Result<(), SessionError> {
        message.validate(&self.recipient_session_id)?;
        let mut encoded = serde_json::to_vec(&MailboxRecord {
            schema_version: 1,
            sequence,
            message: message.clone(),
        })?;
        encoded.push(b'\n');
        if file.metadata()?.len() + encoded.len() as u64 > MAX_MAILBOX_BYTES as u64 {
            return Err(SessionError::InvalidRecord(
                "mailbox byte limit exceeded".into(),
            ));
        }
        file.seek(SeekFrom::End(0))?;
        file.write_all(&encoded)?;
        file.sync_all()?;
        Ok(())
    }
}

struct MailboxLock {
    file: File,
}
impl Drop for MailboxLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            unsafe {
                libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}

fn mailbox_path(session_path: &Path) -> Result<PathBuf, SessionError> {
    let name = session_path.file_name().ok_or(SessionError::EmptyPath)?;
    let mut name = name.to_os_string();
    name.push(".mailbox");
    Ok(session_path.with_file_name(name))
}

fn mailbox_id(id: &str) -> Result<(), SessionError> {
    if id.is_empty()
        || id.len() > 256
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:@-".contains(&byte))
    {
        return Err(SessionError::InvalidRecord(
            "invalid mailbox identifier".into(),
        ));
    }
    Ok(())
}

fn validate_mailbox_payload(payload: &Value) -> Result<(), SessionError> {
    if serde_json::to_vec(payload)?.len() > MAX_MAILBOX_PAYLOAD_BYTES {
        return Err(SessionError::InvalidRecord(
            "mailbox payload exceeds 64 KiB".into(),
        ));
    }
    if crate::security::redact_json(payload, &[]) != *payload {
        return Err(SessionError::InvalidRecord(
            "mailbox payload contains a credential or secret".into(),
        ));
    }
    Ok(())
}

fn validate_mailbox_transition(
    previous: &MailboxMessage,
    next: &MailboxMessage,
) -> Result<(), SessionError> {
    use MailboxStatus as Status;
    let valid = matches!(
        (previous.status, next.status),
        (Status::Queued, Status::Acknowledged | Status::Claimed)
            | (Status::Acknowledged, Status::Claimed)
            | (Status::Claimed, Status::Succeeded | Status::Failed)
    );
    if !valid
        || previous.digest != next.digest
        || (previous.claim_token.is_some() && previous.claim_token != next.claim_token)
    {
        return Err(SessionError::InvalidRecord(
            "mailbox transition conflicts with its durable claim or envelope".into(),
        ));
    }
    Ok(())
}
