//! Shared, bounded view data for the TUI and headless transports.
//!
//! The agent and provider layers intentionally expose different event types.
//! This module is the small normalization boundary between those layers and a
//! renderer: both transports can carry the same sequence, request, turn, and
//! block associations without sharing a terminal implementation or a network
//! runtime. The existing wire events remain source-compatible; adapters can
//! opt into this model while the protocol migrates incrementally.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{backend::ProviderEvent, core::AgentEvent, render::MarkdownBlock};

/// Version of the normalized view model, independent of the JSONL protocol
/// version. Increment this only when the serialized shape is incompatible.
pub const VIEW_MODEL_VERSION: u16 = 1;
/// Maximum bytes retained by one identifier used for correlation.
pub const MAX_VIEW_ID_BYTES: usize = 128;
/// Maximum bytes retained by one text-bearing field.
pub const MAX_VIEW_TEXT_BYTES: usize = 256 * 1024;
/// Maximum blocks in one message.
pub const MAX_VIEW_BLOCKS: usize = 128;
/// Maximum list items in one list block.
pub const MAX_VIEW_LIST_ITEMS: usize = 128;
/// Defensive upper bound for one serialized event.
pub const MAX_VIEW_EVENT_BYTES: usize = 512 * 1024;

/// Validation and bounded-buffer failures at the view-model boundary.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ViewModelError {
    #[error("{field} is empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} bytes")]
    TooLarge { field: &'static str, max: usize },
    #[error("{field} contains a control character")]
    ControlCharacter { field: &'static str },
    #[error("{field} is invalid")]
    Invalid { field: &'static str },
    #[error("{field} requires a turn_id association")]
    MissingTurn { field: &'static str },
    #[error("{field} requires a block_id association")]
    MissingBlock { field: &'static str },
    #[error("event sequence expected {expected}, got {actual}")]
    SequenceDiscontinuity { expected: u64, actual: u64 },
    #[error("event sequence is exhausted")]
    SequenceExhausted,
    #[error("serialized event exceeds {max} bytes")]
    EventTooLarge { max: usize },
    #[error("cannot replay sequence {requested}; first available is {first_available}")]
    ReplayGap {
        requested: u64,
        first_available: u64,
    },
    #[error("cannot replay sequence {requested}; next sequence is {next_sequence}")]
    ReplayFuture { requested: u64, next_sequence: u64 },
    #[error("view model serialization failed: {0}")]
    Serialization(String),
}

/// The conversation role associated with a group of blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewRole {
    User,
    Assistant,
    Tool,
    System,
    Error,
}

/// Normalized status for a tool invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// State of an approval request as it appears in a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Pending,
    Allowed,
    Denied,
}

/// A bounded, renderer-neutral block. It intentionally contains no Ratatui
/// types and no provider-specific response object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewBlock {
    /// Literal text, used for prompts, tool output, and unknown syntax.
    PlainText {
        text: String,
    },
    /// Text that may be passed through the simple Markdown renderer.
    Paragraph {
        text: String,
    },
    Heading {
        level: u8,
        text: String,
    },
    /// A list is kept as one block so folding and replay do not depend on
    /// terminal line wrapping.
    List {
        ordered: bool,
        items: Vec<String>,
    },
    Quote {
        text: String,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    /// A bounded unified patch. The renderer may split this into hunks later.
    Diff {
        path: Option<String>,
        patch: String,
    },
    ToolStatus {
        call_id: String,
        name: String,
        status: ToolStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    Approval {
        approval_id: String,
        tool: String,
        arguments: String,
        state: ApprovalState,
    },
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        message: String,
        retryable: bool,
    },
    Rule,
}

impl ViewBlock {
    /// Validate bounds and terminal-safe text for one block.
    pub fn validate(&self) -> Result<(), ViewModelError> {
        match self {
            Self::PlainText { text } | Self::Paragraph { text } | Self::Quote { text } => {
                validate_text(text, "block text")
            }
            Self::Heading { level, text } => {
                if !(1..=6).contains(level) {
                    return Err(ViewModelError::Invalid {
                        field: "heading level",
                    });
                }
                validate_text(text, "heading text")
            }
            Self::List { items, .. } => {
                if items.is_empty() {
                    return Err(ViewModelError::Empty {
                        field: "list items",
                    });
                }
                if items.len() > MAX_VIEW_LIST_ITEMS {
                    return Err(ViewModelError::TooLarge {
                        field: "list items",
                        max: MAX_VIEW_LIST_ITEMS,
                    });
                }
                for item in items {
                    validate_text(item, "list item")?;
                }
                Ok(())
            }
            Self::Code { language, text } => {
                if let Some(language) = language {
                    validate_token(language, "code language", 64)?;
                }
                validate_text(text, "code text")
            }
            Self::Diff { path, patch } => {
                if let Some(path) = path {
                    validate_token(path, "diff path", 4 * 1024)?;
                }
                validate_text(patch, "diff patch")
            }
            Self::ToolStatus {
                call_id,
                name,
                output,
                ..
            } => {
                validate_id(call_id, "tool call_id")?;
                validate_token(name, "tool name", MAX_VIEW_ID_BYTES)?;
                if let Some(output) = output {
                    validate_text(output, "tool output")?;
                }
                Ok(())
            }
            Self::Approval {
                approval_id,
                tool,
                arguments,
                ..
            } => {
                validate_id(approval_id, "approval_id")?;
                validate_token(tool, "approval tool", MAX_VIEW_ID_BYTES)?;
                validate_text(arguments, "approval arguments")
            }
            Self::Error { code, message, .. } => {
                if let Some(code) = code {
                    validate_token(code, "error code", MAX_VIEW_ID_BYTES)?;
                }
                validate_text(message, "error message")
            }
            Self::Rule => Ok(()),
        }
    }

    /// Return the stable block discriminator used by renderers and tests.
    pub const fn kind(&self) -> ViewBlockKind {
        match self {
            Self::PlainText { .. } => ViewBlockKind::PlainText,
            Self::Paragraph { .. } => ViewBlockKind::Paragraph,
            Self::Heading { .. } => ViewBlockKind::Heading,
            Self::List { .. } => ViewBlockKind::List,
            Self::Quote { .. } => ViewBlockKind::Quote,
            Self::Code { .. } => ViewBlockKind::Code,
            Self::Diff { .. } => ViewBlockKind::Diff,
            Self::ToolStatus { .. } => ViewBlockKind::ToolStatus,
            Self::Approval { .. } => ViewBlockKind::Approval,
            Self::Error { .. } => ViewBlockKind::Error,
            Self::Rule => ViewBlockKind::Rule,
        }
    }

    /// Convert one parsed Markdown block without introducing a parser into
    /// either transport.
    pub fn from_markdown(block: &MarkdownBlock) -> Result<Self, ViewModelError> {
        let result = match block {
            MarkdownBlock::Paragraph(text) => Self::Paragraph { text: text.clone() },
            MarkdownBlock::Heading { level, text } => Self::Heading {
                level: *level,
                text: text.clone(),
            },
            MarkdownBlock::Code { language, text } => Self::Code {
                language: language.clone(),
                text: text.clone(),
            },
            MarkdownBlock::Quote(text) => Self::Quote { text: text.clone() },
            MarkdownBlock::ListItem { ordered, text, .. } => Self::List {
                ordered: *ordered,
                items: vec![text.clone()],
            },
            MarkdownBlock::Rule => Self::Rule,
        };
        result.validate()?;
        Ok(result)
    }

    /// Parse and normalize the bounded Markdown subset used by the TUI.
    pub fn from_markdown_text(text: &str) -> Result<Vec<Self>, ViewModelError> {
        let blocks = crate::render::parse_markdown(text);
        if blocks.len() > MAX_VIEW_BLOCKS {
            return Err(ViewModelError::TooLarge {
                field: "message blocks",
                max: MAX_VIEW_BLOCKS,
            });
        }
        blocks.iter().map(Self::from_markdown).collect()
    }
}

/// Stable discriminant for block consumers that do not need to match fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewBlockKind {
    PlainText,
    Paragraph,
    Heading,
    List,
    Quote,
    Code,
    Diff,
    ToolStatus,
    Approval,
    Error,
    Rule,
}

/// A conversation message represented as bounded blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewMessage {
    pub role: ViewRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub blocks: Vec<ViewBlock>,
}

impl ViewMessage {
    pub fn new(
        role: ViewRole,
        turn_id: Option<String>,
        blocks: Vec<ViewBlock>,
    ) -> Result<Self, ViewModelError> {
        let message = Self {
            role,
            turn_id,
            blocks,
        };
        message.validate()?;
        Ok(message)
    }

    pub fn from_markdown(
        role: ViewRole,
        turn_id: Option<String>,
        text: &str,
    ) -> Result<Self, ViewModelError> {
        Self::new(role, turn_id, ViewBlock::from_markdown_text(text)?)
    }

    pub fn validate(&self) -> Result<(), ViewModelError> {
        if let Some(turn_id) = &self.turn_id {
            validate_id(turn_id, "message turn_id")?;
        }
        if self.blocks.is_empty() {
            return Err(ViewModelError::Empty {
                field: "message blocks",
            });
        }
        if self.blocks.len() > MAX_VIEW_BLOCKS {
            return Err(ViewModelError::TooLarge {
                field: "message blocks",
                max: MAX_VIEW_BLOCKS,
            });
        }
        for block in &self.blocks {
            block.validate()?;
        }
        Ok(())
    }
}

/// Admission mode normalized from the core protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewTurnMode {
    StartOrSteer,
    StartIfIdle,
    Steer,
}

/// Reasons an input request did not reach the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewRejection {
    EmptyInput,
    NotIdle,
    NoActiveTurn,
    ExpectedTurnMismatch,
    ActiveTurnNotSteerable,
    Closed,
    Invalid,
}

/// Stream whose bounded mailbox reported dropped records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewStream {
    Provider,
    Agent,
    Terminal,
    Control,
}

/// Normalized lifecycle payload. The envelope below supplies ordering and
/// association; this enum carries only typed, bounded content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewEventKind {
    RequestAccepted {
        mode: ViewTurnMode,
    },
    RequestRejected {
        reason: ViewRejection,
    },
    TurnStarted {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    TextDelta {
        delta: String,
    },
    Block {
        block: ViewBlock,
    },
    ToolStarted {
        call_id: String,
        name: String,
    },
    ToolCallDelta {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        arguments_delta: String,
    },
    ToolCallReady {
        call_id: String,
        name: String,
        arguments: String,
    },
    ToolFinished {
        call_id: String,
        status: ToolStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    ApprovalRequired {
        approval_id: String,
        tool: String,
        arguments: String,
    },
    ApprovalResolved {
        approval_id: String,
        state: ApprovalState,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
    },
    Handoff {
        handoff_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
    },
    Warning {
        message: String,
    },
    TurnCompleted {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    TurnFailed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        message: String,
        retryable: bool,
    },
    TurnCancelled {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Dropped {
        stream: ViewStream,
        count: u64,
    },
    Closed,
}

impl ViewEventKind {
    pub fn validate(&self) -> Result<(), ViewModelError> {
        match self {
            Self::RequestAccepted { .. }
            | Self::RequestRejected { .. }
            | Self::Usage { .. }
            | Self::Closed => Ok(()),
            Self::TurnStarted { response_id, model }
            | Self::TurnCompleted { response_id, model } => {
                validate_optional_id(response_id, "response_id")?;
                if let Some(model) = model {
                    validate_token(model, "model", MAX_VIEW_ID_BYTES)?;
                }
                Ok(())
            }
            Self::TextDelta { delta } => validate_text(delta, "text delta"),
            Self::Block { block } => block.validate(),
            Self::ToolStarted { call_id, name } => {
                validate_id(call_id, "tool call_id")?;
                validate_token(name, "tool name", MAX_VIEW_ID_BYTES)
            }
            Self::ToolCallDelta {
                call_id,
                name,
                arguments_delta,
            } => {
                validate_optional_id(call_id, "tool call_id")?;
                if let Some(name) = name {
                    validate_token(name, "tool name", MAX_VIEW_ID_BYTES)?;
                }
                validate_text(arguments_delta, "tool arguments delta")
            }
            Self::ToolCallReady {
                call_id,
                name,
                arguments,
            } => {
                validate_id(call_id, "tool call_id")?;
                validate_token(name, "tool name", MAX_VIEW_ID_BYTES)?;
                validate_text(arguments, "tool arguments")
            }
            Self::ToolFinished {
                call_id, output, ..
            } => {
                validate_id(call_id, "tool call_id")?;
                if let Some(output) = output {
                    validate_text(output, "tool output")?;
                }
                Ok(())
            }
            Self::ApprovalRequired {
                approval_id,
                tool,
                arguments,
            } => {
                validate_id(approval_id, "approval_id")?;
                validate_token(tool, "approval tool", MAX_VIEW_ID_BYTES)?;
                validate_text(arguments, "approval arguments")
            }
            Self::ApprovalResolved { approval_id, .. } => validate_id(approval_id, "approval_id"),
            Self::Handoff { handoff_id, to } => {
                validate_id(handoff_id, "handoff_id")?;
                validate_optional_id(to, "handoff target")
            }
            Self::Warning { message } => validate_text(message, "warning message"),
            Self::TurnFailed { code, message, .. } => {
                validate_optional_id(code, "error code")?;
                validate_text(message, "error message")
            }
            Self::TurnCancelled { reason } => validate_optional_text(reason, "cancel reason"),
            Self::Dropped { count, .. } => {
                if *count == 0 {
                    return Err(ViewModelError::Invalid {
                        field: "drop count",
                    });
                }
                Ok(())
            }
        }
    }

    fn requires_turn_id(&self) -> bool {
        matches!(
            self,
            Self::RequestAccepted { .. }
                | Self::TurnStarted { .. }
                | Self::TextDelta { .. }
                | Self::Block { .. }
                | Self::ToolStarted { .. }
                | Self::ToolCallDelta { .. }
                | Self::ToolCallReady { .. }
                | Self::ToolFinished { .. }
                | Self::ApprovalRequired { .. }
                | Self::ApprovalResolved { .. }
                | Self::TurnCompleted { .. }
        )
    }

    fn requires_block_id(&self) -> bool {
        matches!(self, Self::TextDelta { .. } | Self::Block { .. })
    }
}

/// Ordered event envelope shared by headless replay and TUI adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewEvent {
    #[serde(default = "default_view_model_version")]
    pub schema_version: u16,
    pub sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(flatten)]
    pub event: ViewEventKind,
}

impl ViewEvent {
    pub fn new(sequence: u64, event: ViewEventKind) -> Result<Self, ViewModelError> {
        Self::with_context(sequence, None, None, None, event)
    }

    pub fn with_context(
        sequence: u64,
        request_id: Option<String>,
        turn_id: Option<String>,
        block_id: Option<String>,
        event: ViewEventKind,
    ) -> Result<Self, ViewModelError> {
        let value = Self {
            schema_version: VIEW_MODEL_VERSION,
            sequence,
            request_id,
            turn_id,
            block_id,
            event,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ViewModelError> {
        if self.schema_version != VIEW_MODEL_VERSION {
            return Err(ViewModelError::Invalid {
                field: "view model schema_version",
            });
        }
        validate_optional_id(&self.request_id, "request_id")?;
        validate_optional_id(&self.turn_id, "turn_id")?;
        validate_optional_id(&self.block_id, "block_id")?;
        self.event.validate()?;
        if self.event.requires_turn_id() && self.turn_id.is_none() {
            return Err(ViewModelError::MissingTurn { field: "event" });
        }
        if self.event.requires_block_id() && self.block_id.is_none() {
            return Err(ViewModelError::MissingBlock { field: "event" });
        }
        let bytes = self.encoded_len()?;
        if bytes > MAX_VIEW_EVENT_BYTES {
            return Err(ViewModelError::EventTooLarge {
                max: MAX_VIEW_EVENT_BYTES,
            });
        }
        Ok(())
    }

    pub fn encoded_len(&self) -> Result<usize, ViewModelError> {
        serde_json::to_vec(self)
            .map(|value| value.len())
            .map_err(|error| ViewModelError::Serialization(error.to_string()))
    }

    pub fn from_agent_event(
        sequence: u64,
        request_id: Option<String>,
        source: &AgentEvent,
    ) -> Result<Self, ViewModelError> {
        match source {
            AgentEvent::TurnAccepted { turn_id, mode } => Self::with_context(
                sequence,
                request_id,
                Some(turn_id.clone()),
                None,
                ViewEventKind::RequestAccepted {
                    mode: (*mode).into(),
                },
            ),
            AgentEvent::TurnRejected { reason } => Self::with_context(
                sequence,
                request_id,
                None,
                None,
                ViewEventKind::RequestRejected {
                    reason: (*reason).into(),
                },
            ),
            AgentEvent::AssistantMessage { turn_id, content } => Self::with_context(
                sequence,
                request_id,
                Some(turn_id.clone()),
                Some(default_block_id(turn_id, "assistant")),
                ViewEventKind::Block {
                    block: ViewBlock::Paragraph {
                        text: content.clone(),
                    },
                },
            ),
            AgentEvent::Handoff { handoff_id, to } => Self::with_context(
                sequence,
                request_id,
                None,
                None,
                ViewEventKind::Handoff {
                    handoff_id: handoff_id.clone(),
                    to: to.clone(),
                },
            ),
            AgentEvent::ToolCall {
                turn_id,
                call_id,
                tool,
            } => Self::with_context(
                sequence,
                request_id,
                Some(turn_id.clone()),
                None,
                ViewEventKind::ToolStarted {
                    call_id: call_id.clone(),
                    name: tool.clone(),
                },
            ),
            AgentEvent::ToolResult {
                turn_id,
                call_id,
                success,
            } => Self::with_context(
                sequence,
                request_id,
                Some(turn_id.clone()),
                None,
                ViewEventKind::ToolFinished {
                    call_id: call_id.clone(),
                    status: if *success {
                        ToolStatus::Succeeded
                    } else {
                        ToolStatus::Failed
                    },
                    output: None,
                },
            ),
            AgentEvent::Provider { turn_id, event } => {
                Self::from_provider_event(sequence, request_id, Some(turn_id), None, event)
            }
            AgentEvent::Error { message } => Self::with_context(
                sequence,
                request_id,
                None,
                None,
                ViewEventKind::TurnFailed {
                    code: None,
                    message: message.clone(),
                    retryable: false,
                },
            ),
        }
    }

    pub fn from_provider_event(
        sequence: u64,
        request_id: Option<String>,
        turn_id: Option<&str>,
        block_id: Option<&str>,
        source: &ProviderEvent,
    ) -> Result<Self, ViewModelError> {
        let turn_id = turn_id.map(str::to_owned);
        let content_event = matches!(
            source,
            ProviderEvent::TextDelta { .. }
                | ProviderEvent::TextDone { .. }
                | ProviderEvent::Refusal { .. }
        );
        let block_id = block_id.map(str::to_owned).or_else(|| {
            content_event
                .then(|| {
                    turn_id
                        .as_deref()
                        .map(|id| default_block_id(id, "assistant"))
                })
                .flatten()
        });
        let event = match source {
            ProviderEvent::ResponseCreated { response_id, model } => ViewEventKind::TurnStarted {
                response_id: response_id.clone(),
                model: model.clone(),
            },
            ProviderEvent::TextDelta { delta } => ViewEventKind::TextDelta {
                delta: delta.clone(),
            },
            ProviderEvent::TextDone { text } => ViewEventKind::Block {
                block: ViewBlock::Paragraph { text: text.clone() },
            },
            ProviderEvent::Refusal { text } => ViewEventKind::Block {
                block: ViewBlock::Error {
                    code: Some("refusal".into()),
                    message: text.clone(),
                    retryable: false,
                },
            },
            ProviderEvent::ToolCallDelta {
                call_id,
                name,
                arguments_delta,
            } => ViewEventKind::ToolCallDelta {
                call_id: call_id.clone(),
                name: name.clone(),
                arguments_delta: arguments_delta.clone(),
            },
            ProviderEvent::ToolCallDone { call } => ViewEventKind::ToolCallReady {
                call_id: call.id.clone(),
                name: call.name.clone(),
                arguments: serde_json::to_string(&call.arguments)
                    .map_err(|error| ViewModelError::Serialization(error.to_string()))?,
            },
            ProviderEvent::Usage { usage } => ViewEventKind::Usage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
            },
            ProviderEvent::Warning { message } => ViewEventKind::Warning {
                message: message.clone(),
            },
            ProviderEvent::Completed { response_id, model } => ViewEventKind::TurnCompleted {
                response_id: response_id.clone(),
                model: model.clone(),
            },
            ProviderEvent::Failed { message } => ViewEventKind::TurnFailed {
                code: Some("provider_failed".into()),
                message: message.clone(),
                retryable: true,
            },
        };
        Self::with_context(sequence, request_id, turn_id, block_id, event)
    }
}

/// A bounded sequence-preserving event queue suitable for either transport.
/// Eviction never rewinds next_sequence; a replay caller can therefore
/// distinguish an old sequence gap from an empty stream.
#[derive(Debug, Clone)]
pub struct ViewEventBuffer {
    capacity: usize,
    max_bytes: usize,
    next_sequence: u64,
    bytes: usize,
    dropped: u64,
    events: VecDeque<ViewEvent>,
}

impl ViewEventBuffer {
    pub fn new(capacity: usize, max_bytes: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            max_bytes: max_bytes.max(1),
            next_sequence: 0,
            bytes: 0,
            dropped: 0,
            events: VecDeque::new(),
        }
    }

    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn take_dropped(&mut self) -> u64 {
        std::mem::take(&mut self.dropped)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ViewEvent> {
        self.events.iter()
    }

    pub fn push(
        &mut self,
        request_id: Option<String>,
        turn_id: Option<String>,
        block_id: Option<String>,
        event: ViewEventKind,
    ) -> Result<u64, ViewModelError> {
        let sequence = self.next_sequence;
        let value = ViewEvent::with_context(sequence, request_id, turn_id, block_id, event)?;
        self.append(value)?;
        Ok(sequence)
    }

    pub fn append(&mut self, event: ViewEvent) -> Result<(), ViewModelError> {
        event.validate()?;
        if event.sequence != self.next_sequence {
            return Err(ViewModelError::SequenceDiscontinuity {
                expected: self.next_sequence,
                actual: event.sequence,
            });
        }
        let encoded = event.encoded_len()?;
        if encoded > self.max_bytes {
            return Err(ViewModelError::EventTooLarge {
                max: self.max_bytes,
            });
        }
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(ViewModelError::SequenceExhausted)?;
        self.bytes = self.bytes.saturating_add(encoded);
        self.events.push_back(event);
        while self.events.len() > self.capacity || self.bytes > self.max_bytes {
            let Some(oldest) = self.events.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(oldest.encoded_len().unwrap_or(0));
            self.dropped = self.dropped.saturating_add(1);
        }
        Ok(())
    }

    pub fn drain(&mut self) -> Vec<ViewEvent> {
        self.bytes = 0;
        self.events.drain(..).collect()
    }

    pub fn replay_from(&self, sequence: u64) -> Result<Vec<ViewEvent>, ViewModelError> {
        if sequence > self.next_sequence {
            return Err(ViewModelError::ReplayFuture {
                requested: sequence,
                next_sequence: self.next_sequence,
            });
        }
        let Some(first) = self.events.front().map(|event| event.sequence) else {
            if sequence < self.next_sequence {
                return Err(ViewModelError::ReplayGap {
                    requested: sequence,
                    first_available: self.next_sequence,
                });
            }
            return Ok(Vec::new());
        };
        if sequence < first {
            return Err(ViewModelError::ReplayGap {
                requested: sequence,
                first_available: first,
            });
        }
        Ok(self
            .events
            .iter()
            .filter(|event| event.sequence >= sequence)
            .cloned()
            .collect())
    }
}

impl From<crate::protocol::TurnMode> for ViewTurnMode {
    fn from(value: crate::protocol::TurnMode) -> Self {
        match value {
            crate::protocol::TurnMode::StartOrSteer => Self::StartOrSteer,
            crate::protocol::TurnMode::StartIfIdle => Self::StartIfIdle,
            crate::protocol::TurnMode::Steer => Self::Steer,
        }
    }
}

impl From<crate::core::NotSubmittedReason> for ViewRejection {
    fn from(value: crate::core::NotSubmittedReason) -> Self {
        match value {
            crate::core::NotSubmittedReason::EmptyInput => Self::EmptyInput,
            crate::core::NotSubmittedReason::NotIdle => Self::NotIdle,
            crate::core::NotSubmittedReason::NoActiveTurn => Self::NoActiveTurn,
            crate::core::NotSubmittedReason::ExpectedTurnMismatch => Self::ExpectedTurnMismatch,
            crate::core::NotSubmittedReason::ActiveTurnNotSteerable => Self::ActiveTurnNotSteerable,
        }
    }
}

fn default_view_model_version() -> u16 {
    VIEW_MODEL_VERSION
}

fn default_block_id(turn_id: &str, suffix: &str) -> String {
    // Both inputs are validated by the caller; this deterministic key keeps
    // streaming deltas and their completed block associated across transports.
    let suffix = format!(":{suffix}");
    let max_prefix = MAX_VIEW_ID_BYTES.saturating_sub(suffix.len());
    let mut end = turn_id.len().min(max_prefix);
    while end > 0 && !turn_id.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &turn_id[..end], suffix)
}

fn validate_id(value: &str, field: &'static str) -> Result<(), ViewModelError> {
    validate_token(value, field, MAX_VIEW_ID_BYTES)
}

fn validate_optional_id(value: &Option<String>, field: &'static str) -> Result<(), ViewModelError> {
    if let Some(value) = value {
        validate_id(value, field)?;
    }
    Ok(())
}

fn validate_optional_text(
    value: &Option<String>,
    field: &'static str,
) -> Result<(), ViewModelError> {
    if let Some(value) = value {
        validate_text(value, field)?;
    }
    Ok(())
}

fn validate_token(value: &str, field: &'static str, max: usize) -> Result<(), ViewModelError> {
    if value.trim().is_empty() {
        return Err(ViewModelError::Empty { field });
    }
    if value.len() > max {
        return Err(ViewModelError::TooLarge { field, max });
    }
    if value.chars().any(char::is_control) {
        return Err(ViewModelError::ControlCharacter { field });
    }
    Ok(())
}

fn validate_text(value: &str, field: &'static str) -> Result<(), ViewModelError> {
    if value.len() > MAX_VIEW_TEXT_BYTES {
        return Err(ViewModelError::TooLarge {
            field,
            max: MAX_VIEW_TEXT_BYTES,
        });
    }
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(ViewModelError::ControlCharacter { field });
    }
    Ok(())
}
