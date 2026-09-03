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

/// Maximum accepted input frame.  A bounded frame keeps a composed agent from
/// accidentally retaining an unbounded amount of memory on malformed input.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
/// Maximum text accepted for one prompt or handoff summary.
pub const MAX_TEXT_BYTES: usize = 256 * 1024;
/// Maximum number of artifact names in one handoff.
pub const MAX_HANDOFF_ARTIFACTS: usize = B3_MAX_HANDOFF_ARTIFACTS;
/// Version of the headless request/response envelope.
pub const PROTOCOL_VERSION: u16 = 1;
/// Version for clients that consume asynchronous lifecycle events. Version 1
/// remains accepted and receives the terminal-response projection.
pub const ASYNC_PROTOCOL_VERSION: u16 = 2;
/// Maximum bytes in a correlation identifier.
pub const MAX_ID_BYTES: usize = 128;

const fn default_protocol_version() -> u16 {
    PROTOCOL_VERSION
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
}

/// A validated command.  The command owns its payload so admission can move
/// it into the core without cloning potentially large prompt text.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Prompt {
        text: String,
        mode: TurnMode,
        expected_turn_id: Option<String>,
    },
    Steer {
        text: String,
        expected_turn_id: Option<String>,
    },
    Cancel {
        target_id: String,
    },
    Status,
    Handoff {
        to: Option<String>,
        summary: String,
        artifacts: Vec<String>,
    },
    Resume {
        path: Option<String>,
    },
    Shutdown,
}

impl StdioRequest {
    /// Validate and convert the wire request into a bounded command.
    pub fn into_command(self) -> Result<Command, ProtocolError> {
        if self.schema_version != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion {
                found: self.schema_version,
                expected: PROTOCOL_VERSION,
            });
        }
        validate_id(self.id.as_deref())?;
        validate_optional_field(self.expected_turn_id.as_deref(), "expected_turn_id", 256)?;
        validate_optional_field(self.path.as_deref(), "path", 4096)?;
        match self.kind.as_str() {
            "prompt" => Ok(Command::Prompt {
                text: bounded_text(self.text.or(self.message), "prompt")?,
                mode: self.mode.unwrap_or_default(),
                expected_turn_id: self.expected_turn_id,
            }),
            "steer" => Ok(Command::Steer {
                text: bounded_text(self.text.or(self.message), "steer")?,
                expected_turn_id: self.expected_turn_id,
            }),
            "cancel" => {
                let target_id = self
                    .target_id
                    .or(self.expected_turn_id)
                    .ok_or(ProtocolError::MissingField { field: "target_id" })?;
                validate_identifier(&target_id, "target_id")?;
                Ok(Command::Cancel { target_id })
            }
            "status" => Ok(Command::Status),
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
            "resume" => Ok(Command::Resume { path: self.path }),
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
    if id.chars().any(char::is_control) {
        return Err(ProtocolError::InvalidField { field: "id" });
    }
    Ok(())
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
    if value
        .chars()
        .any(|character| !(character.is_ascii_alphanumeric() || "._:/-".contains(character)))
    {
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
    #[error("too many handoff artifacts (maximum {max})")]
    TooManyArtifacts { max: usize },
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
            Self::TooManyArtifacts { .. } => "too_many_artifacts",
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
}

/// An event envelope emitted by asynchronous hosts. Keeping events separate
/// from terminal responses lets clients replay or acknowledge them without
/// treating progress as a second command result.
#[derive(Debug, Clone, Serialize)]
pub struct StdioEvent {
    pub schema_version: u16,
    pub sequence: u64,
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
            request_id,
            turn_id,
            event,
        }
    }
}

impl StdioResponse {
    pub fn success(id: Option<String>, command: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            schema_version: PROTOCOL_VERSION,
            id,
            kind: "response",
            command: command.into(),
            success: true,
            data,
            error: None,
            error_code: "ok".into(),
        }
    }

    pub fn error(id: Option<String>, command: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            schema_version: PROTOCOL_VERSION,
            id,
            kind: "response",
            command: command.into(),
            success: false,
            data: None,
            error: Some(error.into()),
            error_code: "error".into(),
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
        Command::Prompt { .. } => "prompt",
        Command::Steer { .. } => "steer",
        Command::Cancel { .. } => "cancel",
        Command::Status => "status",
        Command::Handoff { .. } => "handoff",
        Command::Resume { .. } => "resume",
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
