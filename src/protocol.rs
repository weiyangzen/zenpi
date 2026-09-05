//! The small, versioned JSONL protocol used by headless zenpi.
//!
//! A protocol line is one JSON object terminated by `\n`.  Newlines inside a
//! value are escaped by JSON and therefore never split a frame.  Diagnostics
//! are intentionally not part of this module; callers should keep stdout
//! machine-readable and send diagnostics to stderr.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::b3::{
    MAX_ARTIFACT_PATH_BYTES, MAX_HANDOFF_ARTIFACTS as B3_MAX_HANDOFF_ARTIFACTS,
    validate_artifact_path,
};
use crate::backend::InputAttachment;

/// Maximum accepted input frame.  A bounded frame keeps a composed agent from
/// accidentally retaining an unbounded amount of memory on malformed input.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
/// Maximum text accepted for one prompt or handoff summary.
pub const MAX_TEXT_BYTES: usize = 256 * 1024;
/// Maximum number of artifact names in one handoff.
pub const MAX_HANDOFF_ARTIFACTS: usize = B3_MAX_HANDOFF_ARTIFACTS;
/// Current headless request/event protocol.
pub const PROTOCOL_VERSION: u16 = 2;
/// Missing versions and explicit v1 requests retain the legacy projection.
pub const LEGACY_PROTOCOL_VERSION: u16 = 1;
pub const ASYNC_PROTOCOL_VERSION: u16 = PROTOCOL_VERSION;
/// Maximum bytes in a correlation identifier.
pub const MAX_ID_BYTES: usize = 128;
pub const MAX_MAILBOX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_MAILBOX_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;
pub const MAX_USER_SHELL_BYTES: usize = 16 * 1024;
pub const MAX_CHECKPOINT_PAGE_RECORDS: u16 = 128;

const fn default_protocol_version() -> u16 {
    LEGACY_PROTOCOL_VERSION
}

/// How an incoming user message is admitted to the current turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TurnMode {
    /// Start a turn when idle, otherwise steer the active turn.
    #[serde(alias = "start")]
    #[default]
    StartOrSteer,
    /// Refuse the request unless the agent is idle.
    StartIfIdle,
    /// Refuse the request unless a compatible active turn exists.
    Steer,
}

/// This cursor addresses the durable session journal, not process-local
/// stdout events. A transport reconnect owner must not conflate the two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointCursor {
    pub session_id: String,
    pub next_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum CheckpointRequest {
    Inspect,
    Acknowledge {
        cursor: CheckpointCursor,
    },
    Replay {
        cursor: CheckpointCursor,
        limit: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxOutcome {
    Succeeded,
    Failed,
    Abandoned,
}

/// Sender identity and workspace authorization are deliberately absent from
/// client-controlled fields; those belong to the eventual mailbox owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum MailboxRequest {
    Send {
        recipient_session_id: String,
        message_id: String,
        text: String,
        ttl_ms: u64,
    },
    Receive {
        after_sequence: u64,
        limit: u16,
    },
    Acknowledge {
        message_id: String,
    },
    Claim {
        message_id: String,
    },
    Complete {
        message_id: String,
        outcome: MailboxOutcome,
        result: Option<String>,
    },
}

/// Parsing an explicit user-shell request does not grant shell execution.
/// The host must still have a shell owner and an applicable side-effect gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserShellRequest {
    pub input: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionState {
    Untracked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointCursorScope {
    SessionJournal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointInspection {
    pub cursor: CheckpointCursor,
    pub cursor_scope: CheckpointCursorScope,
    pub reconnect_supported: bool,
    pub acknowledgement_supported: bool,
}

/// A decoded request from stdin.  Optional fields are kept here so malformed
/// requests can receive a correlated, typed error instead of terminating the
/// process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StdioRequest {
    /// Missing versions are treated as v1 for compatibility with early
    /// clients; an explicitly unsupported version is rejected before dispatch.
    #[serde(default = "default_protocol_version", alias = "version")]
    pub schema_version: u16,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub mode: Option<TurnMode>,
    #[serde(default)]
    pub expected_turn_id: Option<String>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default, alias = "approval_request_id")]
    pub approval_id: Option<String>,
    #[serde(default)]
    pub decision: Option<crate::approval::ApprovalDecision>,
    #[serde(default)]
    pub remember: bool,
    #[serde(default)]
    pub attachments: Vec<InputAttachment>,
    #[serde(default)]
    pub from_sequence: Option<u64>,
    #[serde(default)]
    pub checkpoint: Option<CheckpointRequest>,
    #[serde(default)]
    pub mailbox: Option<MailboxRequest>,
}

/// A validated command.  The command owns its payload so admission can move
/// it into the core without cloning potentially large prompt text.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// A typed local slash command carried over the headless transport.  The
    /// command text is parsed by the shared slash grammar and is never
    /// submitted as a provider prompt.
    Slash {
        input: String,
    },
    Prompt {
        text: String,
        mode: TurnMode,
        expected_turn_id: Option<String>,
        attachments: Vec<InputAttachment>,
    },
    Steer {
        text: String,
        expected_turn_id: Option<String>,
    },
    Cancel {
        target_id: String,
    },
    Status,
    /// Collect a bounded workspace/resource snapshot without invoking the
    /// provider. The optional path is interpreted by the host as the
    /// workspace root; omitted means the host's current workspace.
    Resources {
        path: Option<String>,
    },
    Handoff {
        to: Option<String>,
        summary: String,
        artifacts: Vec<String>,
    },
    Resume {
        path: Option<String>,
        from_sequence: Option<u64>,
    },
    Checkpoint(CheckpointRequest),
    Mailbox(MailboxRequest),
    UserShell(UserShellRequest),
    Approve {
        approval_id: String,
        decision: crate::approval::ApprovalDecision,
        remember: bool,
    },
    Shutdown,
}

impl StdioRequest {
    /// Validate and convert the wire request into a bounded command.
    pub fn into_command(self) -> Result<Command, ProtocolError> {
        if !matches!(
            self.schema_version,
            LEGACY_PROTOCOL_VERSION | PROTOCOL_VERSION
        ) {
            return Err(ProtocolError::UnsupportedVersion {
                found: self.schema_version,
                expected: PROTOCOL_VERSION,
            });
        }
        validate_id(self.id.as_deref())?;
        validate_optional_field(self.expected_turn_id.as_deref(), "expected_turn_id", 256)?;
        validate_optional_field(self.path.as_deref(), "path", 4096)?;
        match self.kind.as_str() {
            "command" | "slash" => {
                let input = bounded_text(self.text.or(self.message), "command")?;
                if input.trim_start().starts_with('!') {
                    if !self.attachments.is_empty()
                        || self.mode.is_some()
                        || self.expected_turn_id.is_some()
                    {
                        return Err(ProtocolError::InvalidField {
                            field: "user_shell",
                        });
                    }
                    return Ok(Command::UserShell(parse_user_shell_input(
                        input, "command",
                    )?));
                }
                if !input.trim_start().starts_with('/') {
                    return Err(ProtocolError::InvalidField { field: "command" });
                }
                Ok(Command::Slash { input })
            }
            "prompt" => {
                let text = bounded_text(self.text.or(self.message), "prompt")?;
                if text.trim_start().starts_with('!') {
                    if !self.attachments.is_empty()
                        || self.mode.is_some()
                        || self.expected_turn_id.is_some()
                    {
                        return Err(ProtocolError::InvalidField {
                            field: "user_shell",
                        });
                    }
                    return Ok(Command::UserShell(parse_user_shell_input(text, "prompt")?));
                }
                Ok(Command::Prompt {
                    text,
                    mode: self.mode.unwrap_or_default(),
                    expected_turn_id: self.expected_turn_id,
                    attachments: validate_attachments(self.attachments)?,
                })
            }
            "steer" => {
                let text = bounded_text(self.text.or(self.message), "steer")?;
                if text.trim_start().starts_with('!') {
                    if self.expected_turn_id.is_some() {
                        return Err(ProtocolError::InvalidField {
                            field: "user_shell",
                        });
                    }
                    return Ok(Command::UserShell(parse_user_shell_input(text, "steer")?));
                }
                Ok(Command::Steer {
                    text,
                    expected_turn_id: self.expected_turn_id,
                })
            }
            "cancel" => {
                let target_id = self
                    .target_id
                    .or(self.expected_turn_id)
                    .ok_or(ProtocolError::MissingField { field: "target_id" })?;
                validate_identifier(&target_id, "target_id")?;
                Ok(Command::Cancel { target_id })
            }
            "status" => Ok(Command::Status),
            "resources" | "resource" => Ok(Command::Resources { path: self.path }),
            "handoff" => {
                let summary = bounded_text(self.summary.or(self.text), "handoff summary")?;
                if summary.contains(['\r', '\n']) {
                    return Err(ProtocolError::InvalidField {
                        field: "handoff summary",
                    });
                }
                if self.artifacts.len() > MAX_HANDOFF_ARTIFACTS {
                    return Err(ProtocolError::TooManyArtifacts {
                        max: MAX_HANDOFF_ARTIFACTS,
                    });
                }
                for artifact in &self.artifacts {
                    if artifact.len() > MAX_ARTIFACT_PATH_BYTES {
                        return Err(ProtocolError::FieldTooLong {
                            field: "artifact",
                            max: MAX_ARTIFACT_PATH_BYTES,
                        });
                    }
                    validate_artifact_path(artifact)
                        .map_err(|_| ProtocolError::InvalidField { field: "artifact" })?;
                }
                if let Some(to) = &self.to {
                    validate_identifier(to, "handoff recipient")?;
                }
                Ok(Command::Handoff {
                    to: self.to,
                    summary,
                    artifacts: self.artifacts,
                })
            }
            "resume" => {
                if self.path.is_some() && self.from_sequence.is_some() {
                    return Err(ProtocolError::InvalidField { field: "resume" });
                }
                Ok(Command::Resume {
                    path: self.path,
                    from_sequence: self.from_sequence,
                })
            }
            "checkpoint" => {
                let checkpoint = self.checkpoint.ok_or(ProtocolError::MissingField {
                    field: "checkpoint",
                })?;
                match &checkpoint {
                    CheckpointRequest::Inspect => {}
                    CheckpointRequest::Acknowledge { cursor } => validate_cursor(cursor)?,
                    CheckpointRequest::Replay { cursor, limit } => {
                        validate_cursor(cursor)?;
                        validate_page_limit(*limit)?;
                    }
                }
                Ok(Command::Checkpoint(checkpoint))
            }
            "mailbox" => {
                let mailbox = self
                    .mailbox
                    .ok_or(ProtocolError::MissingField { field: "mailbox" })?;
                validate_mailbox(&mailbox)?;
                Ok(Command::Mailbox(mailbox))
            }
            "user_shell" => {
                if !self.attachments.is_empty()
                    || self.mode.is_some()
                    || self.expected_turn_id.is_some()
                {
                    return Err(ProtocolError::InvalidField {
                        field: "user_shell",
                    });
                }
                let input = bounded_text_allow_bare_bang(self.text.or(self.message), "user_shell")?;
                Ok(Command::UserShell(parse_user_shell_input(
                    input,
                    "user_shell",
                )?))
            }
            "approve" | "approval" => {
                let approval_id = self.approval_id.ok_or(ProtocolError::MissingField {
                    field: "approval_id",
                })?;
                validate_identifier(&approval_id, "approval_id")?;
                let decision = self
                    .decision
                    .ok_or(ProtocolError::MissingField { field: "decision" })?;
                Ok(Command::Approve {
                    approval_id,
                    decision,
                    remember: self.remember,
                })
            }
            "shutdown" => Ok(Command::Shutdown),
            other => Err(ProtocolError::UnknownCommand(other.to_owned())),
        }
    }

    /// Return the correlation id without requiring callers to retain the
    /// entire request.  IDs are validated when `into_command` is called.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
}

fn validate_cursor(cursor: &CheckpointCursor) -> Result<(), ProtocolError> {
    validate_identifier(&cursor.session_id, "session_id")?;
    if cursor.next_sequence == u64::MAX {
        return Err(ProtocolError::InvalidField {
            field: "next_sequence",
        });
    }
    Ok(())
}

fn validate_page_limit(limit: u16) -> Result<(), ProtocolError> {
    if limit == 0 || limit > MAX_CHECKPOINT_PAGE_RECORDS {
        return Err(ProtocolError::InvalidField { field: "limit" });
    }
    Ok(())
}

fn validate_mailbox(mailbox: &MailboxRequest) -> Result<(), ProtocolError> {
    match mailbox {
        MailboxRequest::Send {
            recipient_session_id,
            message_id,
            text,
            ttl_ms,
        } => {
            validate_identifier(recipient_session_id, "recipient_session_id")?;
            validate_identifier(message_id, "message_id")?;
            validate_mailbox_text(text, "mailbox text")?;
            if *ttl_ms == 0 || *ttl_ms > MAX_MAILBOX_TTL_MS {
                return Err(ProtocolError::InvalidField { field: "ttl_ms" });
            }
        }
        MailboxRequest::Receive {
            after_sequence,
            limit,
        } => {
            validate_page_limit(*limit)?;
            if *after_sequence == u64::MAX {
                return Err(ProtocolError::InvalidField {
                    field: "after_sequence",
                });
            }
        }
        MailboxRequest::Acknowledge { message_id } | MailboxRequest::Claim { message_id } => {
            validate_identifier(message_id, "message_id")?;
        }
        MailboxRequest::Complete {
            message_id, result, ..
        } => {
            validate_identifier(message_id, "message_id")?;
            if let Some(result) = result {
                validate_mailbox_text(result, "mailbox result")?;
            }
        }
    }
    Ok(())
}

fn validate_mailbox_text(value: &str, field: &'static str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() {
        return Err(ProtocolError::EmptyField { field });
    }
    if value.len() > MAX_MAILBOX_TEXT_BYTES {
        return Err(ProtocolError::FieldTooLong {
            field,
            max: MAX_MAILBOX_TEXT_BYTES,
        });
    }
    if value.contains('\0') {
        return Err(ProtocolError::InvalidField { field });
    }
    Ok(())
}

fn validate_id(id: Option<&str>) -> Result<(), ProtocolError> {
    let Some(id) = id else {
        return Err(ProtocolError::MissingField { field: "id" });
    };
    if id.trim().is_empty() {
        return Err(ProtocolError::EmptyField { field: "id" });
    }
    if id.len() > MAX_ID_BYTES {
        return Err(ProtocolError::FieldTooLong {
            field: "id",
            max: MAX_ID_BYTES,
        });
    }
    validate_identifier(id, "id")
}

fn validate_identifier(value: &str, field: &'static str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() {
        return Err(ProtocolError::EmptyField { field });
    }
    if value.len() > MAX_ID_BYTES {
        return Err(ProtocolError::FieldTooLong {
            field,
            max: MAX_ID_BYTES,
        });
    }
    if value.chars().any(char::is_control) {
        return Err(ProtocolError::InvalidField { field });
    }
    Ok(())
}

fn validate_optional_field(
    value: Option<&str>,
    field: &'static str,
    max: usize,
) -> Result<(), ProtocolError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.trim().is_empty() {
        return Err(ProtocolError::EmptyField { field });
    }
    if value.len() > max {
        return Err(ProtocolError::FieldTooLong { field, max });
    }
    if value.contains(['\r', '\n', '\0']) {
        return Err(ProtocolError::InvalidField { field });
    }
    Ok(())
}

fn bounded_text(value: Option<String>, field: &'static str) -> Result<String, ProtocolError> {
    let value = value.ok_or(ProtocolError::MissingField { field })?;
    if value.trim().is_empty() {
        return Err(ProtocolError::EmptyField { field });
    }
    if value.len() > MAX_TEXT_BYTES {
        return Err(ProtocolError::FieldTooLong {
            field,
            max: MAX_TEXT_BYTES,
        });
    }
    if value.contains('\0') {
        return Err(ProtocolError::InvalidField { field });
    }
    Ok(value)
}

fn bounded_text_allow_bare_bang(
    value: Option<String>,
    field: &'static str,
) -> Result<String, ProtocolError> {
    // A bare `!` is a valid local-shell help request, unlike ordinary text
    // fields which reject whitespace-only values.
    let value = value.ok_or(ProtocolError::MissingField { field })?;
    if value.len() > MAX_TEXT_BYTES {
        return Err(ProtocolError::FieldTooLong {
            field,
            max: MAX_TEXT_BYTES,
        });
    }
    if value.contains('\0') {
        return Err(ProtocolError::InvalidField { field });
    }
    Ok(value)
}

fn parse_user_shell_input(
    input: String,
    field: &'static str,
) -> Result<UserShellRequest, ProtocolError> {
    if input.len() > MAX_USER_SHELL_BYTES {
        return Err(ProtocolError::FieldTooLong {
            field,
            max: MAX_USER_SHELL_BYTES,
        });
    }
    let trimmed = input.trim();
    if !trimmed.starts_with('!') {
        return Err(ProtocolError::InvalidField { field });
    }
    if trimmed.starts_with("!!") {
        return Err(ProtocolError::UnsupportedUserShellExtension);
    }
    if trimmed
        .chars()
        .any(|character| character != '\n' && character != '\t' && character.is_control())
    {
        return Err(ProtocolError::InvalidField { field });
    }
    Ok(UserShellRequest {
        input: trimmed.to_owned(),
    })
}

fn validate_attachments(
    attachments: Vec<InputAttachment>,
) -> Result<Vec<InputAttachment>, ProtocolError> {
    if attachments.len() > crate::backend::MAX_ATTACHMENTS_PER_TURN {
        return Err(ProtocolError::TooManyAttachments {
            max: crate::backend::MAX_ATTACHMENTS_PER_TURN,
        });
    }
    for attachment in &attachments {
        attachment
            .validate()
            .map_err(|_| ProtocolError::InvalidField {
                field: "attachment",
            })?;
    }
    Ok(attachments)
}

/// Protocol-level validation failures.  These are returned as JSON errors and
/// do not mutate the session.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("missing field `{field}`")]
    MissingField { field: &'static str },
    #[error("field `{field}` is empty")]
    EmptyField { field: &'static str },
    #[error("field `{field}` exceeds {max} bytes")]
    FieldTooLong { field: &'static str, max: usize },
    #[error("field `{field}` contains invalid characters")]
    InvalidField { field: &'static str },
    #[error("`!!` is not supported; use a single `!` for a local shell command")]
    UnsupportedUserShellExtension,
    #[error("too many handoff artifacts (maximum {max})")]
    TooManyArtifacts { max: usize },
    #[error("too many attachments (maximum {max})")]
    TooManyAttachments { max: usize },
    #[error("input frame exceeds {max} bytes")]
    LineTooLong { max: usize },
    #[error("invalid JSON: {0}")]
    InvalidJson(String),
    #[error("request must be a JSON object")]
    NotObject,
    #[error("unsupported protocol version {found} (expected {expected})")]
    UnsupportedVersion { found: u16, expected: u16 },
}

impl ProtocolError {
    /// Stable machine-readable code for a protocol failure.  Human-readable
    /// text remains available through `Display`, but callers should branch on
    /// this code instead of parsing diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownCommand(_) => "unknown_command",
            Self::MissingField { .. } => "missing_field",
            Self::EmptyField { .. } => "empty_field",
            Self::FieldTooLong { .. } => "field_too_long",
            Self::InvalidField { .. } => "invalid_field",
            Self::UnsupportedUserShellExtension => "unsupported_user_shell_extension",
            Self::TooManyArtifacts { .. } => "too_many_artifacts",
            Self::TooManyAttachments { .. } => "too_many_attachments",
            Self::LineTooLong { .. } => "line_too_long",
            Self::InvalidJson(_) => "invalid_json",
            Self::NotObject => "not_object",
            Self::UnsupportedVersion { .. } => "unsupported_version",
        }
    }
}

impl From<serde_json::Error> for ProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::InvalidJson(error.to_string())
    }
}

/// Parse one LF-delimited frame.  A trailing CR is accepted for clients that
/// use CRLF, while embedded line breaks remain invalid framing.
pub fn parse_line(line: &str) -> Result<StdioRequest, ProtocolError> {
    if line.len() > MAX_LINE_BYTES {
        return Err(ProtocolError::LineTooLong {
            max: MAX_LINE_BYTES,
        });
    }
    let line = line.strip_suffix('\r').unwrap_or(line);
    let value: Value = serde_json::from_str(line)?;
    if !value.is_object() {
        return Err(ProtocolError::NotObject);
    }
    Ok(serde_json::from_value(value)?)
}

/// A correlated JSONL response.  `data` and `error` are mutually exclusive;
/// constructors below enforce that invariant.
#[derive(Debug, Serialize)]
pub struct StdioResponse {
    #[serde(default = "default_protocol_version")]
    pub schema_version: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub command: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Stable machine-readable response code.  It is present for both
    /// success (`ok`) and failure (`error` or a typed protocol code).
    #[serde(rename = "code")]
    pub error_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_state: Option<ExecutionState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_owner: Option<&'static str>,
}

/// An event envelope emitted by asynchronous hosts. Keeping events separate
/// from terminal responses lets clients replay or acknowledge them without
/// treating progress as a second command result.
#[derive(Debug, Clone, Serialize)]
pub struct StdioEvent {
    pub schema_version: u16,
    pub sequence: u64,
    /// Every stdout record has a discriminator so a client can safely mix
    /// terminal responses and progress events on one JSONL stream.
    #[serde(rename = "type")]
    pub kind: &'static str,
    #[serde(rename = "request_id", skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(rename = "turn_id", skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub event: Value,
}

impl StdioEvent {
    pub fn new(
        sequence: u64,
        request_id: Option<String>,
        turn_id: Option<String>,
        event: Value,
    ) -> Self {
        Self {
            schema_version: ASYNC_PROTOCOL_VERSION,
            sequence,
            kind: "event",
            request_id,
            turn_id,
            event,
        }
    }
}

impl StdioResponse {
    pub fn success(id: Option<String>, command: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            schema_version: LEGACY_PROTOCOL_VERSION,
            id,
            kind: "response",
            command: command.into(),
            success: true,
            data,
            error: None,
            error_code: "ok".into(),
            execution_state: None,
            required_owner: None,
        }
    }

    pub fn error(id: Option<String>, command: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            schema_version: LEGACY_PROTOCOL_VERSION,
            id,
            kind: "response",
            command: command.into(),
            success: false,
            data: None,
            error: Some(error.into()),
            error_code: "error".into(),
            execution_state: None,
            required_owner: None,
        }
    }

    /// Construct a failure with a stable code supplied by a typed boundary.
    pub fn error_with_code(
        id: Option<String>,
        command: impl Into<String>,
        code: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        let mut response = Self::error(id, command, error);
        response.error_code = code.into();
        response
    }

    pub fn for_version(mut self, version: u16) -> Self {
        self.schema_version = if version == PROTOCOL_VERSION {
            PROTOCOL_VERSION
        } else {
            LEGACY_PROTOCOL_VERSION
        };
        self
    }

    /// A decoded request is not an admission or an execution receipt when
    /// its side-effect owner is unavailable. Keep this a terminal error.
    pub fn owner_required(
        id: Option<String>,
        command: impl Into<String>,
        owner: &'static str,
    ) -> Self {
        let mut response = Self::error_with_code(
            id,
            command,
            "owner_required",
            "request was not executed; the required owner is unavailable",
        );
        response.execution_state = Some(ExecutionState::Untracked);
        response.required_owner = Some(owner);
        response
    }
}

/// Serialize one response/event as exactly one LF-terminated line.
pub fn encode_line<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    Ok(line)
}

/// A small helper used by tests and embedders that need a displayable command
/// name without matching every enum variant.
pub fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Slash { .. } => "command",
        Command::Prompt { .. } => "prompt",
        Command::Steer { .. } => "steer",
        Command::Cancel { .. } => "cancel",
        Command::Status => "status",
        Command::Resources { .. } => "resources",
        Command::Handoff { .. } => "handoff",
        Command::Resume { .. } => "resume",
        Command::Checkpoint(_) => "checkpoint",
        Command::Mailbox(_) => "mailbox",
        Command::UserShell(_) => "user_shell",
        Command::Approve { .. } => "approve",
        Command::Shutdown => "shutdown",
    }
}

impl fmt::Display for TurnMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::StartOrSteer => "start_or_steer",
            Self::StartIfIdle => "start_if_idle",
            Self::Steer => "steer",
        })
    }
}
