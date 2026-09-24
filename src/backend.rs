//! Backend boundary for zenpi.
//!
//! The core deliberately knows nothing about HTTP, model SDKs, or streaming
//! transports.  A backend receives an immutable view of the session and
//! returns one normalized completion.  This keeps the headless and TUI modes
//! behaviorally identical and makes a deterministic backend useful in tests.

use std::{
    env,
    str::FromStr,
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::auth::resolve::{AuthContext, AuthResolver};
use crate::auth::store::CredentialStore;
use crate::auth::{AuthBinding, AuthError, AuthIdentitySnapshot};
use crate::core::{Turn, TurnRole};
use crate::protocols::{self, chat};
use crate::providers::connection::{ProviderConnection, ValidatedRoute, resolve_connection};
use crate::providers::{AuthHeaderPolicy, Dialect};
use crate::security::{SecretError, SecretHandle, SecretScope};
use crate::tools::{ToolCall, ToolDefinition};

#[cfg(all(test, unix))]
mod explicit_tests;
pub(crate) mod transport;

/// Keep provider responses bounded even when an endpoint omits a content
/// length.  The core applies the smaller per-turn text limit afterwards.
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_ATTACHMENTS_PER_TURN: usize = 8;
pub const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_TOTAL_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
/// Maximum time a blocking Responses body read may hide a cancellation
/// request. `ureq` has no external abort handle, so the synchronous adapter
/// uses short receive-body slices and resumes after ordinary slice expiry.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    Image,
    File,
}

/// Bounds shared by host input and the stored content DTO.  One reference must
/// not be valid on the way in and invalid on the way out, so both sides read the
/// same limits.
pub(crate) const MAX_ATTACHMENT_MIME_BYTES: usize = 255;
pub(crate) const MAX_ATTACHMENT_PATH_BYTES: usize = 4_096;
pub(crate) const MAX_ATTACHMENT_URL_BYTES: usize = 2_048;
pub(crate) const MAX_ATTACHMENT_FILE_ID_BYTES: usize = 512;

/// Audio and video are explicitly unsupported: a host must reject them before
/// any bytes move, never encode them as an image or drop them silently.
pub(crate) fn unsupported_media_type(mime_type: &str) -> bool {
    mime_type.starts_with("audio/") || mime_type.starts_with("video/")
}

/// The MIME rules a bounded reference must satisfy, shared by host input and
/// the stored content DTO so a type cannot be valid on one side only.
pub(crate) fn validate_attachment_mime(
    kind: AttachmentKind,
    mime_type: &str,
) -> Result<(), BackendError> {
    if mime_type.trim().is_empty()
        || mime_type.len() > MAX_ATTACHMENT_MIME_BYTES
        || !mime_type.contains('/')
        || mime_type.chars().any(char::is_control)
    {
        return Err(BackendError::Configuration(
            "attachment MIME type is invalid".into(),
        ));
    }
    if kind == AttachmentKind::Image && !mime_type.starts_with("image/") {
        return Err(BackendError::Configuration(
            "image attachment requires an image/* MIME type".into(),
        ));
    }
    Ok(())
}

/// A provider file ID is a reference, not a path or a token: it is bounded and
/// free of control characters wherever it is recorded.
pub(crate) fn validate_provider_file_id(file_id: &str) -> Result<(), BackendError> {
    if file_id.trim().is_empty()
        || file_id.len() > MAX_ATTACHMENT_FILE_ID_BYTES
        || file_id.chars().any(char::is_control)
    {
        return Err(BackendError::Configuration(
            "provider file ID is invalid".into(),
        ));
    }
    Ok(())
}

/// A workspace reference must stay inside the workspace by syntax alone, before
/// any path resolution can be tempted to follow `..` or an absolute path.
pub(crate) fn validate_relative_attachment_path(path: &str) -> Result<(), BackendError> {
    use std::path::{Component, Path};

    let parsed = Path::new(path);
    if path.is_empty()
        || path.len() > MAX_ATTACHMENT_PATH_BYTES
        || path.contains('\0')
        || path.chars().any(char::is_control)
        || parsed.is_absolute()
        || parsed.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(BackendError::Configuration(
            "attachment path is not a relative workspace path".into(),
        ));
    }
    Ok(())
}

/// A user-visible attachment reference. Exactly one source is accepted. A
/// workspace source is persisted as a relative path and materialized only for
/// the current bounded provider request; bytes never enter the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputAttachment {
    pub kind: AttachmentKind,
    pub mime_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
}

impl InputAttachment {
    pub fn validate(&self) -> Result<(), BackendError> {
        validate_attachment_mime(self.kind, &self.mime_type)?;
        let source_count = usize::from(self.path.is_some())
            + usize::from(self.url.is_some())
            + usize::from(self.file_id.is_some());
        if source_count != 1 {
            return Err(BackendError::Configuration(
                "attachment requires exactly one of path, url, or file_id".into(),
            ));
        }
        if let Some(path) = &self.path
            && (path.trim().is_empty()
                || path.len() > MAX_ATTACHMENT_PATH_BYTES
                || path.contains('\0'))
        {
            return Err(BackendError::Configuration(
                "attachment path is invalid".into(),
            ));
        }
        if let Some(url) = &self.url
            && (self.kind != AttachmentKind::Image
                || url.len() > MAX_ATTACHMENT_URL_BYTES
                || !url.starts_with("https://")
                || url.chars().any(char::is_whitespace))
        {
            return Err(BackendError::Configuration(
                "remote attachments must be HTTPS image URLs".into(),
            ));
        }
        if let Some(file_id) = &self.file_id {
            validate_provider_file_id(file_id)?;
        }
        Ok(())
    }
}

/// Ordered content submitted by a host for a prompt or a typed tool result.
///
/// This is the only public content shape.  A client never names an internal
/// handle, an identity scope or a hash: the trusted entry binds those itself,
/// which is what keeps a provider file ID from being re-scoped by its caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputContentPart {
    Text {
        text: String,
    },
    Image {
        mime_type: String,
        source: InputContentSource,
    },
    File {
        mime_type: String,
        source: InputContentSource,
    },
}

/// Where the bytes of an ordered content part come from.  Exactly one source,
/// by construction rather than by counting optional fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputContentSource {
    Path { value: String },
    Url { value: String },
    FileId { value: String },
}

impl InputContentPart {
    pub fn validate(&self) -> Result<(), BackendError> {
        match self {
            Self::Text { text } => {
                if text.len() > crate::protocol::MAX_TEXT_BYTES {
                    return Err(BackendError::Configuration(format!(
                        "content text exceeds {} bytes",
                        crate::protocol::MAX_TEXT_BYTES
                    )));
                }
                Ok(())
            }
            Self::Image { mime_type, source } => {
                validate_content_reference(AttachmentKind::Image, mime_type, source)
            }
            Self::File { mime_type, source } => {
                validate_content_reference(AttachmentKind::File, mime_type, source)
            }
        }
    }

    /// The existing attachment form of this part.  Text is carried by the prompt
    /// itself, so it has no attachment to map onto.
    pub fn to_attachment(&self) -> Result<InputAttachment, BackendError> {
        match self {
            Self::Text { .. } => Err(BackendError::Configuration(
                "text content has no attachment source".into(),
            )),
            Self::Image { mime_type, source } => {
                source.to_attachment(AttachmentKind::Image, mime_type)
            }
            Self::File { mime_type, source } => {
                source.to_attachment(AttachmentKind::File, mime_type)
            }
        }
    }
}

impl InputContentSource {
    fn to_attachment(
        &self,
        kind: AttachmentKind,
        mime_type: &str,
    ) -> Result<InputAttachment, BackendError> {
        let mut attachment = InputAttachment {
            kind,
            mime_type: mime_type.to_owned(),
            path: None,
            url: None,
            file_id: None,
        };
        match self {
            Self::Path { value } => attachment.path = Some(value.clone()),
            Self::Url { value } => attachment.url = Some(value.clone()),
            Self::FileId { value } => attachment.file_id = Some(value.clone()),
        }
        Ok(attachment)
    }
}

fn validate_content_reference(
    kind: AttachmentKind,
    mime_type: &str,
    source: &InputContentSource,
) -> Result<(), BackendError> {
    // Unsupported media is refused first, so an audio or video file is never
    // reported as a mere MIME mismatch and never reaches a provider as bytes.
    if unsupported_media_type(mime_type) {
        return Err(BackendError::Configuration(
            "audio and video content are not supported".into(),
        ));
    }
    source.to_attachment(kind, mime_type)?.validate()?;
    if let InputContentSource::Path { value } = source {
        validate_relative_attachment_path(value)?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct RequestAttachment {
    pub turn_id: String,
    pub input: InputAttachment,
    pub filename: Option<String>,
    pub data: Option<Vec<u8>>,
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProviderCapabilities {
    pub text: bool,
    pub images: bool,
    pub files: bool,
    pub tools: bool,
    pub structured_output: bool,
    pub streaming: bool,
    pub reasoning: bool,
}

impl ProviderCapabilities {
    pub const fn for_wire_api(wire_api: OpenAiWireApi) -> Self {
        match wire_api {
            OpenAiWireApi::AnthropicMessages => crate::providers::anthropic::CAPABILITIES,
            OpenAiWireApi::GoogleGenerativeAi => crate::providers::google::CAPABILITIES,
            OpenAiWireApi::Responses => Self {
                text: true,
                images: true,
                files: true,
                tools: true,
                structured_output: true,
                streaming: true,
                reasoning: true,
            },
            OpenAiWireApi::ChatCompletions => Self {
                text: true,
                images: true,
                files: false,
                tools: true,
                structured_output: true,
                streaming: true,
                reasoning: false,
            },
        }
    }
}

#[derive(Debug, Default)]
struct CircuitState {
    consecutive_failures: u32,
    open_until: Option<Instant>,
}

/// A normalized request independent of the provider protocol.
#[derive(Debug)]
pub struct CompletionRequest<'a> {
    pub turn_id: &'a str,
    pub turns: &'a [Turn],
    pub model: Option<&'a str>,
    /// Tools advertised for this turn. Providers must not infer or invent
    /// capabilities that are absent from this bounded list.
    pub tools: &'a [ToolDefinition],
    pub instructions: Option<&'a str>,
    pub metadata: Option<&'a Value>,
    pub attachments: &'a [RequestAttachment],
    pub response_format: Option<&'a Value>,
    pub max_output_tokens: Option<u64>,
}

impl<'a> CompletionRequest<'a> {
    pub fn new(
        turn_id: &'a str,
        turns: &'a [Turn],
        model: Option<&'a str>,
        tools: &'a [ToolDefinition],
    ) -> Self {
        Self {
            turn_id,
            turns,
            model,
            tools,
            instructions: None,
            metadata: None,
            attachments: &[],
            response_format: None,
            max_output_tokens: None,
        }
    }

    pub fn with_instructions(mut self, instructions: Option<&'a str>) -> Self {
        self.instructions = instructions;
        self
    }

    pub fn with_max_output_tokens(mut self, tokens: u64) -> Self {
        self.max_output_tokens = Some(tokens);
        self
    }

    pub fn with_response_format(mut self, format: Option<&'a Value>) -> Self {
        self.response_format = format;
        self
    }

    pub fn with_metadata(mut self, metadata: Option<&'a Value>) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_attachments(mut self, attachments: &'a [RequestAttachment]) -> Self {
        self.attachments = attachments;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderEvent {
    ResponseCreated {
        response_id: Option<String>,
        model: Option<String>,
    },
    TextDelta {
        delta: String,
    },
    ReasoningDelta {
        delta: String,
    },
    TextDone {
        text: String,
    },
    Refusal {
        text: String,
    },
    ToolCallDelta {
        call_id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    ToolCallDone {
        call: ToolCall,
    },
    Usage {
        usage: Usage,
    },
    Warning {
        message: String,
    },
    Completed {
        response_id: Option<String>,
        model: Option<String>,
    },
    Failed {
        message: String,
    },
}

impl ProviderEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ResponseCreated { .. } => "response_created",
            Self::TextDelta { .. } => "text_delta",
            Self::ReasoningDelta { .. } => "reasoning_delta",
            Self::TextDone { .. } => "text_done",
            Self::Refusal { .. } => "refusal",
            Self::ToolCallDelta { .. } => "tool_call_delta",
            Self::ToolCallDone { .. } => "tool_call_done",
            Self::Usage { .. } => "usage",
            Self::Warning { .. } => "warning",
            Self::Completed { .. } => "completed",
            Self::Failed { .. } => "failed",
        }
    }
}

/// Optional usage information returned by a provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Provider-independent completion result.
#[derive(Debug, Default, PartialEq)]
pub struct Completion {
    pub content: String,
    pub usage: Option<Usage>,
    pub model: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub response_id: Option<String>,
    pub refusal: Option<String>,
    pub annotations: Vec<Value>,
}

impl Completion {
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            usage: None,
            model: None,
            tool_calls: Vec::new(),
            response_id: None,
            refusal: None,
            annotations: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestPurpose {
    Turn,
    ToolContinuation,
    SemanticCompaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpRequestKind {
    Inference,
    AuthRefresh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendRequestBinding {
    pub route_digest: String,
    pub identity_scope: String,
}

/// Non-secret description of the connection a backend was built for.
///
/// A selection event has to describe what was selected without carrying a
/// token or a credential, so this is the only form of "which account" that may
/// reach the journal.  `credential_ref` is an identifier, never material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionSnapshot {
    pub profile: String,
    pub provider: String,
    pub protocol: String,
    pub auth_kind: String,
    pub credential_ref: Option<String>,
    pub identity_scope: String,
    pub route_digest: String,
    pub definition_version: u32,
    pub config_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestScope {
    pub owner_id: String,
    pub session_id: String,
    pub operation_id: String,
    pub purpose: RequestPurpose,
    pub route_digest: String,
    pub identity_scope: String,
    pub policy_digest: Option<String>,
    pub lease_id: Option<String>,
}

impl RequestScope {
    fn legacy(binding: BackendRequestBinding, request: &CompletionRequest<'_>) -> Self {
        Self {
            owner_id: "legacy".into(),
            session_id: "legacy".into(),
            operation_id: request.turn_id.into(),
            purpose: if request
                .metadata
                .is_some_and(|value| value["purpose"] == "semantic_compaction")
            {
                RequestPurpose::SemanticCompaction
            } else {
                RequestPurpose::Turn
            },
            route_digest: binding.route_digest,
            identity_scope: binding.identity_scope,
            policy_digest: None,
            lease_id: None,
        }
    }
}

pub struct RequestControl<'a> {
    pub cancelled: &'a dyn Fn() -> bool,
    pub deadline: Option<Instant>,
    pub scope: RequestScope,
    pub before_send: &'a mut dyn FnMut(HttpRequestKind, &RequestScope) -> Result<(), BackendError>,
}

impl RequestControl<'_> {
    pub fn check_cancelled(&self) -> Result<(), BackendError> {
        if (self.cancelled)() {
            Err(BackendError::Cancelled)
        } else if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            Err(BackendError::DeadlineExceeded)
        } else {
            Ok(())
        }
    }

    pub fn before_send(&mut self, kind: HttpRequestKind) -> Result<(), BackendError> {
        self.check_cancelled()?;
        (self.before_send)(kind, &self.scope).map_err(|error| match error {
            BackendError::Cancelled
            | BackendError::Steered
            | BackendError::DeadlineExceeded
            | BackendError::AdmissionDenied(_) => error,
            other => BackendError::AdmissionDenied(other.to_string()),
        })?;
        self.check_cancelled()
    }
}

/// Errors at the provider boundary.  They are surfaced as typed headless
/// responses rather than panics or silent fallback to another model.
#[derive(Debug, Error)]
pub enum BackendError {
    #[error("backend configuration: {0}")]
    Configuration(String),
    #[error("backend transport: {0}")]
    Transport(String),
    #[error("backend HTTP status: {status}")]
    HttpStatus {
        status: u16,
        /// Provider-directed retry delay. The value is bounded while parsing
        /// so an endpoint cannot stall the runtime indefinitely.
        retry_after_ms: Option<u64>,
    },
    #[error("backend circuit is open for another {retry_after_ms} ms")]
    CircuitOpen { retry_after_ms: u64 },
    #[error("backend returned an invalid response: {0}")]
    InvalidResponse(String),
    #[error("backend returned an empty completion")]
    EmptyResponse,
    #[error("backend request was cancelled")]
    Cancelled,
    #[error("backend request was superseded by a steer")]
    Steered,
    #[error("backend request admission denied: {0}")]
    AdmissionDenied(String),
    #[error("backend request deadline exceeded")]
    DeadlineExceeded,
    #[error("backend authentication: {code}")]
    Authentication { code: &'static str },
}

impl BackendError {
    /// Stable classification for hosts that need to decide whether a failed
    /// turn may be retried without parsing display text.
    pub const fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::HttpStatus { status, .. } => {
                *status == 408
                    || *status == 409
                    || *status == 425
                    || *status == 429
                    || (*status >= 500 && *status <= 599)
            }
            Self::Configuration(_)
            | Self::CircuitOpen { .. }
            | Self::InvalidResponse(_)
            | Self::EmptyResponse
            | Self::Cancelled
            | Self::Steered
            | Self::AdmissionDenied(_)
            | Self::DeadlineExceeded
            | Self::Authentication { .. } => false,
        }
    }

    pub const fn code(&self) -> &'static str {
        match self {
            Self::Configuration(_) => "backend_configuration",
            Self::Transport(_) => "backend_transport",
            // Refine the statuses a host must act on differently.  A 401 is
            // the credential being refused, a 403 is the account not being
            // allowed to do this, and a 429 is a quota the operator can wait
            // out; lumping them under one code hides which remedy applies.
            Self::HttpStatus { status: 401, .. } => "auth_login_required",
            Self::HttpStatus { status: 403, .. } => "provider_permission_denied",
            Self::HttpStatus { status: 429, .. } => "provider_usage_limit",
            Self::HttpStatus { .. } => "backend_http_status",
            Self::CircuitOpen { .. } => "backend_circuit_open",
            Self::InvalidResponse(_) => "backend_invalid_response",
            Self::EmptyResponse => "backend_empty_response",
            Self::Cancelled => "backend_cancelled",
            Self::Steered => "backend_steered",
            Self::AdmissionDenied(_) => "backend_admission_denied",
            Self::DeadlineExceeded => "backend_deadline_exceeded",
            Self::Authentication { code } => code,
        }
    }
}

/// Synchronous provider contract.  A synchronous boundary is intentional:
/// the default binary stays tiny and callers that need concurrency can run
/// independent agents in their own processes.
pub trait Backend: Send + Sync {
    fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, BackendError>;

    fn complete_with_request_control(
        &self,
        _request: CompletionRequest<'_>,
        _control: &mut RequestControl<'_>,
        _sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        Err(BackendError::Configuration(
            "controlled_request_unsupported".into(),
        ))
    }

    fn complete_with_control(
        &self,
        request: CompletionRequest<'_>,
        cancelled: &dyn Fn() -> bool,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        if cancelled() {
            return Err(BackendError::Cancelled);
        }
        let completion = self.complete(request)?;
        if cancelled() {
            return Err(BackendError::Cancelled);
        }
        if !completion.content.is_empty() {
            sink(ProviderEvent::TextDelta {
                delta: completion.content.clone(),
            })?;
        }
        sink(ProviderEvent::Completed {
            response_id: completion.response_id.clone(),
            model: completion.model.clone(),
        })?;
        Ok(completion)
    }

    /// The connection this backend serves, when it is bound to an explicit one.
    ///
    /// `None` means the backend is not connection bound (a legacy or test
    /// backend), which is what a selection event records as "no explicit
    /// connection" rather than inventing one.
    fn connection_snapshot(
        &self,
        _model: Option<&str>,
    ) -> Result<Option<ConnectionSnapshot>, BackendError> {
        Ok(None)
    }

    fn request_binding(&self, model: Option<&str>) -> Result<BackendRequestBinding, BackendError> {
        legacy_request_binding(&json!({
            "kind": "legacy_custom_backend",
            "backend": self.name(),
            "model": model.or_else(|| self.model()),
        }))
    }

    fn name(&self) -> &str {
        "backend"
    }

    /// Return the configured model when the backend has one. Hosts use this
    /// for status snapshots without downcasting a provider implementation.
    fn model(&self) -> Option<&str> {
        None
    }

    /// None explicitly means this low-level backend is not registry-bound.
    fn model_descriptor(
        &self,
        _model: Option<&str>,
    ) -> Result<Option<crate::providers::registry::ModelDescriptor>, BackendError> {
        Ok(None)
    }

    fn model_catalog(&self) -> Vec<crate::providers::registry::ModelDescriptor> {
        Vec::new()
    }

    fn model_capabilities(
        &self,
        _model: Option<&str>,
    ) -> Result<Option<ProviderCapabilities>, BackendError> {
        Ok(None)
    }

    fn reasoning_effort(&self) -> Option<&str> {
        None
    }

    /// Validate without changing state. Hosts validate, persist the selection,
    /// then call the infallible commit hook below; failed journal writes leave
    /// the effective provider settings untouched.
    fn validate_reasoning_effort(
        &self,
        _model: Option<&str>,
        _effort: Option<&str>,
    ) -> Result<(), BackendError> {
        Err(BackendError::Configuration(
            "runtime reasoning settings require a registry-bound provider".into(),
        ))
    }

    /// Commit only a value accepted by validate_reasoning_effort. This must
    /// perform no I/O and cannot fail after the owner's durable event append.
    fn commit_reasoning_effort(&mut self, _effort: Option<String>) {}

    /// Reject unrepresentable opaque history before a durable model selection.
    fn validate_history_model(
        &self,
        _turns: &[Turn],
        model: Option<&str>,
    ) -> Result<(), BackendError> {
        self.validate_model(model)
    }

    fn validate_model(&self, model: Option<&str>) -> Result<(), BackendError> {
        if let Some(model) = model {
            crate::providers::registry::validate_identity("backend", model)
                .map_err(|error| BackendError::Configuration(error.to_string()))?;
        }
        Ok(())
    }
}

/// Deterministic backend used by default.  It returns the latest user text
/// unchanged, making protocol wiring testable without credentials or network.
#[derive(Debug, Clone, Copy, Default)]
pub struct EchoBackend;

impl Backend for EchoBackend {
    fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        let text = request
            .turns
            .iter()
            .rev()
            .find(|turn| turn.role == TurnRole::User)
            .map(|turn| turn.content.as_str())
            .ok_or_else(|| BackendError::InvalidResponse("request has no user turn".into()))?;
        Ok(Completion::text(text))
    }

    fn complete_with_request_control(
        &self,
        request: CompletionRequest<'_>,
        control: &mut RequestControl<'_>,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        control.check_cancelled()?;
        let result = self.complete_with_control(
            request,
            &|| control.check_cancelled().is_err(),
            &mut |event| {
                control.check_cancelled()?;
                sink(event)
            },
        );
        control.check_cancelled()?;
        result
    }

    fn name(&self) -> &str {
        "echo"
    }
}

/// Wire protocol used by an OpenAI-compatible HTTP endpoint.
///
/// Chat Completions remains the default for backwards compatibility with
/// existing local proxies.  The Responses API is selected explicitly (or by
/// `ZENPI_WIRE_API=responses`) and is the protocol used by current Codex
/// configuration files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OpenAiWireApi {
    #[default]
    ChatCompletions,
    Responses,
    AnthropicMessages,
    GoogleGenerativeAi,
}

impl OpenAiWireApi {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
            Self::AnthropicMessages => "anthropic_messages",
            Self::GoogleGenerativeAi => "google_generative_ai",
        }
    }
}

impl FromStr for OpenAiWireApi {
    type Err = BackendError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "chat" | "chat_completions" | "chat-completions" | "chatcompletions" => {
                Ok(Self::ChatCompletions)
            }
            "responses" | "response" => Ok(Self::Responses),
            "anthropic_messages" | "anthropic-messages" => Ok(Self::AnthropicMessages),
            "google_generative_ai" | "google-generative-ai" => Ok(Self::GoogleGenerativeAi),
            other => Err(BackendError::Configuration(format!(
                "unsupported provider wire API {other:?}; use chat_completions, responses, anthropic_messages or google_generative_ai"
            ))),
        }
    }
}

/// A synchronous OpenAI-compatible HTTP backend.
///
/// Both wire APIs consume bounded server-sent event streams. Chat also accepts
/// a JSON response from compatible gateways that ignore streaming. Deltas fold into the same
/// normalized completion. `ureq` is blocking and connection pooling keeps
/// the steady-state process smaller than an async runtime.
pub struct OpenAiCompatibleBackend {
    client: ureq::Agent,
    endpoint: String,
    api_key: Option<String>,
    secret_handle: Option<SecretHandle>,
    secret_policy_digest: Option<String>,
    model: String,
    wire_api: OpenAiWireApi,
    reasoning_effort: Option<String>,
    verbosity: Option<String>,
    request_timeout: Duration,
    max_retries: u32,
    circuit_failure_threshold: u32,
    circuit_cooldown: Duration,
    circuit: Mutex<CircuitState>,
    registry: Option<(String, crate::providers::registry::ModelRegistry)>,
    explicit: Option<ExplicitProviderState>,
}

struct ExplicitProviderState {
    connection: ProviderConnection,
    identity: Option<AuthIdentitySnapshot>,
    resolver: AuthResolver,
}

impl std::fmt::Debug for OpenAiCompatibleBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiCompatibleBackend")
            .field("endpoint", &self.endpoint)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("secret_handle", &self.secret_handle)
            .field("model", &self.model)
            .field("wire_api", &self.wire_api)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("verbosity", &self.verbosity)
            .field("request_timeout", &self.request_timeout)
            .field("max_retries", &self.max_retries)
            .field("circuit_failure_threshold", &self.circuit_failure_threshold)
            .field("circuit_cooldown", &self.circuit_cooldown)
            .field("registry_bound", &self.registry.is_some())
            .field("explicit_connection", &self.explicit.is_some())
            .field("capabilities", &self.capabilities())
            .finish()
    }
}

impl OpenAiCompatibleBackend {
    pub fn new(
        endpoint: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
    ) -> Result<Self, BackendError> {
        Self::new_with_wire_api(endpoint, api_key, model, OpenAiWireApi::ChatCompletions)
    }

    pub fn new_with_wire_api(
        endpoint: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
        wire_api: OpenAiWireApi,
    ) -> Result<Self, BackendError> {
        Self::new_with_settings(endpoint, api_key, model, wire_api, None, None)
    }

    pub fn new_with_settings(
        endpoint: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
        wire_api: OpenAiWireApi,
        reasoning_effort: Option<String>,
        verbosity: Option<String>,
    ) -> Result<Self, BackendError> {
        Self::new_with_settings_and_timeout(
            endpoint,
            api_key,
            model,
            wire_api,
            reasoning_effort,
            verbosity,
            Duration::from_secs(120),
        )
    }

    pub fn new_with_settings_and_timeout(
        endpoint: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
        wire_api: OpenAiWireApi,
        reasoning_effort: Option<String>,
        verbosity: Option<String>,
        timeout: Duration,
    ) -> Result<Self, BackendError> {
        let endpoint = normalize_endpoint(endpoint.into(), wire_api)?;
        Self::from_validated_endpoint(
            endpoint,
            api_key,
            model.into(),
            wire_api,
            reasoning_effort,
            verbosity,
            timeout,
        )
    }

    fn from_validated_endpoint(
        endpoint: String,
        api_key: Option<String>,
        model: String,
        wire_api: OpenAiWireApi,
        reasoning_effort: Option<String>,
        verbosity: Option<String>,
        timeout: Duration,
    ) -> Result<Self, BackendError> {
        if model.trim().is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
            return Err(BackendError::Configuration(
                "model must be non-empty and at most 256 bytes".into(),
            ));
        }
        if api_key
            .as_deref()
            .is_some_and(|key| key.trim().is_empty() || key.chars().any(char::is_control))
        {
            return Err(BackendError::Configuration(
                "API key must be non-empty and contain no control characters".into(),
            ));
        }
        if let Some(key) = api_key.as_deref() {
            crate::security::register_secret_value(key);
        }
        for (name, value) in [
            ("reasoning effort", reasoning_effort.as_deref()),
            ("verbosity", verbosity.as_deref()),
        ] {
            if value.is_some_and(|value| {
                value.trim().is_empty() || value.len() > 64 || value.chars().any(char::is_control)
            }) {
                return Err(BackendError::Configuration(format!(
                    "{name} must be non-empty, at most 64 bytes, and contain no control characters"
                )));
            }
        }
        if timeout.is_zero() || timeout > Duration::from_secs(3600) {
            return Err(BackendError::Configuration(
                "timeout must be between 1 second and 1 hour".into(),
            ));
        }
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            // Status responses must remain inspectable so Retry-After can be
            // honored. They are converted to the typed error below before a
            // response body reaches any parser.
            .http_status_as_error(false)
            .build();
        Ok(Self {
            client: ureq::Agent::new_with_config(config),
            endpoint,
            api_key,
            secret_handle: None,
            secret_policy_digest: None,
            model,
            wire_api,
            reasoning_effort,
            verbosity,
            request_timeout: timeout,
            max_retries: 0,
            circuit_failure_threshold: 3,
            circuit_cooldown: Duration::from_secs(2),
            circuit: Mutex::new(CircuitState::default()),
            registry: None,
            explicit: None,
        })
    }

    pub(crate) fn from_connection(
        connection: ProviderConnection,
        store: CredentialStore,
        registry: crate::providers::registry::ModelRegistry,
        model: String,
        reasoning_effort: Option<String>,
        verbosity: Option<String>,
        timeout: Duration,
    ) -> Result<Self, BackendError> {
        let resolver = AuthResolver::new(store);
        let identity = resolver
            .initial_identity(&connection.auth)
            .map_err(auth_error)?;
        let route = resolve_connection(&connection, &model, &registry, identity.as_ref(), true)?;
        let mut backend = Self::from_validated_endpoint(
            route.url().into(),
            None,
            model,
            route.protocol().wire_api(),
            reasoning_effort,
            verbosity,
            timeout,
        )?;
        backend.registry = Some((connection.provider.clone(), registry));
        backend.explicit = Some(ExplicitProviderState {
            connection,
            identity,
            resolver,
        });
        backend.validate_model(None)?;
        Ok(backend)
    }

    fn explicit_route(&self, model: &str) -> Result<Option<ValidatedRoute>, BackendError> {
        let Some(state) = &self.explicit else {
            return Ok(None);
        };
        let (_, registry) = self.registry.as_ref().ok_or_else(|| {
            BackendError::Configuration("explicit connection requires a model registry".into())
        })?;
        resolve_connection(
            &state.connection,
            model,
            registry,
            state.identity.as_ref(),
            true,
        )
        .map(Some)
    }

    /// Build a provider backend from an opaque, policy-bound credential. The
    /// handle is verified before admission and unwrapped only while adding the
    /// Authorization header to a host-owned request.
    pub fn new_with_secret_handle(
        endpoint: impl Into<String>,
        secret: SecretHandle,
        policy_digest: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, BackendError> {
        Self::new_with_secret_handle_and_settings(
            endpoint,
            secret,
            policy_digest,
            model,
            OpenAiWireApi::ChatCompletions,
            None,
            None,
        )
    }

    /// Settings-preserving variant for Responses streaming and provider
    /// reasoning/verbosity controls.
    pub fn new_with_secret_handle_and_settings(
        endpoint: impl Into<String>,
        secret: SecretHandle,
        policy_digest: impl Into<String>,
        model: impl Into<String>,
        wire_api: OpenAiWireApi,
        reasoning_effort: Option<String>,
        verbosity: Option<String>,
    ) -> Result<Self, BackendError> {
        let policy_digest = policy_digest.into();
        secret
            .verify_policy_digest(&policy_digest)
            .map_err(secret_error)?;
        let mut backend =
            Self::new_with_settings(endpoint, None, model, wire_api, reasoning_effort, verbosity)?;
        backend.secret_handle = Some(secret);
        backend.secret_policy_digest = Some(policy_digest);
        Ok(backend)
    }

    pub fn from_env() -> Result<Self, BackendError> {
        let endpoint = env::var("ZENPI_BASE_URL")
            .or_else(|_| env::var("OPENAI_BASE_URL"))
            .unwrap_or_else(|_| "https://api.openai.com/v1".into());
        let api_key = env::var("ZENPI_API_KEY")
            .or_else(|_| env::var("OPENAI_API_KEY"))
            .ok();
        let model = env::var("ZENPI_MODEL")
            .or_else(|_| env::var("OPENAI_MODEL"))
            .unwrap_or_else(|_| "gpt-4o-mini".into());
        let wire_api = match env::var("ZENPI_WIRE_API").or_else(|_| env::var("OPENAI_WIRE_API")) {
            Ok(value) => value.parse()?,
            Err(_) => OpenAiWireApi::default(),
        };
        Self::from_values_with_wire_api(endpoint, api_key, model, wire_api)
    }

    pub fn from_values_with_wire_api(
        endpoint: String,
        api_key: Option<String>,
        model: String,
        wire_api: OpenAiWireApi,
    ) -> Result<Self, BackendError> {
        let normalized_endpoint = normalize_endpoint(endpoint, wire_api)?;
        if is_openai_endpoint(&normalized_endpoint) && api_key.is_none() {
            return Err(BackendError::Configuration(
                "ZENPI_API_KEY or OPENAI_API_KEY is required for api.openai.com".into(),
            ));
        }
        Self::new_with_wire_api(normalized_endpoint, api_key, model, wire_api)
    }

    pub fn from_values_with_settings(
        endpoint: String,
        api_key: Option<String>,
        model: String,
        wire_api: OpenAiWireApi,
        reasoning_effort: Option<String>,
        verbosity: Option<String>,
    ) -> Result<Self, BackendError> {
        Self::from_values_with_settings_and_timeout(
            endpoint,
            api_key,
            model,
            wire_api,
            reasoning_effort,
            verbosity,
            Duration::from_secs(120),
        )
    }

    pub fn from_values_with_settings_and_timeout(
        endpoint: String,
        api_key: Option<String>,
        model: String,
        wire_api: OpenAiWireApi,
        reasoning_effort: Option<String>,
        verbosity: Option<String>,
        timeout: Duration,
    ) -> Result<Self, BackendError> {
        let normalized_endpoint = normalize_endpoint(endpoint, wire_api)?;
        if is_openai_endpoint(&normalized_endpoint) && api_key.is_none() {
            return Err(BackendError::Configuration(
                "ZENPI_API_KEY or OPENAI_API_KEY is required for api.openai.com".into(),
            ));
        }
        Self::new_with_settings_and_timeout(
            normalized_endpoint,
            api_key,
            model,
            wire_api,
            reasoning_effort,
            verbosity,
            timeout,
        )
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub const fn wire_api(&self) -> OpenAiWireApi {
        self.wire_api
    }

    /// Legacy constructors are wire-only. Production factories must call
    /// `with_model_registry`; their effective capabilities are model × wire.
    pub fn capabilities(&self) -> ProviderCapabilities {
        self.model_capabilities(None)
            .expect("registry binding validates the immutable model identity")
            .unwrap_or_else(|| ProviderCapabilities::for_wire_api(self.wire_api))
    }

    pub fn with_model_registry(
        mut self,
        provider: String,
        registry: crate::providers::registry::ModelRegistry,
    ) -> Result<Self, BackendError> {
        crate::providers::registry::validate_identity(&provider, &self.model)
            .map_err(|error| BackendError::Configuration(error.to_string()))?;
        self.registry = Some((provider, registry));
        self.validate_model(None)?;
        Ok(self)
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Result<Self, BackendError> {
        if max_retries > 10 {
            return Err(BackendError::Configuration(
                "max retries must be at most 10".into(),
            ));
        }
        self.max_retries = max_retries;
        Ok(self)
    }

    /// Configure the per-backend circuit breaker. This is public primarily so
    /// deterministic hosts and tests can choose a cooldown compatible with
    /// their wall-clock budget; production defaults remain conservative.
    pub fn with_circuit_breaker(
        mut self,
        failure_threshold: u32,
        cooldown: Duration,
    ) -> Result<Self, BackendError> {
        if failure_threshold == 0 || failure_threshold > 100 {
            return Err(BackendError::Configuration(
                "circuit failure threshold must be between 1 and 100".into(),
            ));
        }
        if cooldown.is_zero() || cooldown > Duration::from_secs(300) {
            return Err(BackendError::Configuration(
                "circuit cooldown must be between 1 millisecond and 5 minutes".into(),
            ));
        }
        self.circuit_failure_threshold = failure_threshold;
        self.circuit_cooldown = cooldown;
        Ok(self)
    }
}

/// Bounded jitter for provider retry backoff: at most +25% of the base delay.
/// Pure so the swarm-resilience behavior is unit-testable.
pub fn jittered_retry_delay(base: Duration, jitter_nanos: u64) -> Duration {
    let span = base.as_millis().max(1) as u64 / 4 + 1;
    base + Duration::from_millis(jitter_nanos % span)
}

fn normalize_endpoint(
    mut endpoint: String,
    wire_api: OpenAiWireApi,
) -> Result<String, BackendError> {
    if endpoint.trim().is_empty() || endpoint.len() > 2048 {
        return Err(BackendError::Configuration(
            "endpoint must be non-empty and at most 2048 bytes".into(),
        ));
    }
    if endpoint
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
        || !(endpoint.starts_with("http://") || endpoint.starts_with("https://"))
        || endpoint.contains(['?', '#'])
    {
        return Err(BackendError::Configuration(
            "endpoint must be an http or https URL without whitespace, query, or fragment".into(),
        ));
    }
    while endpoint.ends_with('/') {
        endpoint.pop();
    }
    if wire_api == OpenAiWireApi::GoogleGenerativeAi {
        if endpoint.contains("/models/") {
            return Err(BackendError::Configuration(
                "Gemini base URL must precede /models, without a model operation".into(),
            ));
        }
        if endpoint
            .strip_prefix("http://")
            .or_else(|| endpoint.strip_prefix("https://"))
            .is_some_and(|rest| !rest.contains('/'))
        {
            endpoint.push_str("/v1beta");
        }
        return Ok(endpoint);
    }
    let canonical_openai_host = is_openai_endpoint(&endpoint);
    let suffix = match wire_api {
        OpenAiWireApi::ChatCompletions => "/chat/completions",
        OpenAiWireApi::Responses => "/responses",
        OpenAiWireApi::AnthropicMessages => "/messages",
        OpenAiWireApi::GoogleGenerativeAi => unreachable!("handled above"),
    };
    if wire_api == OpenAiWireApi::AnthropicMessages && endpoint.ends_with("/messages") {
        return Ok(endpoint);
    }
    if endpoint.ends_with("/chat/completions") {
        endpoint.truncate(endpoint.len() - "/chat/completions".len());
    } else if endpoint.ends_with("/responses") {
        endpoint.truncate(endpoint.len() - "/responses".len());
    }
    while endpoint.ends_with('/') {
        endpoint.pop();
    }
    if endpoint.ends_with("/v1") {
        endpoint.push_str(suffix);
    } else if endpoint
        .strip_prefix("http://")
        .or_else(|| endpoint.strip_prefix("https://"))
        .is_some_and(|rest| !rest.contains('/'))
    {
        // OpenAI's canonical Responses endpoint lives under `/v1`, while
        // preserving the historical Chat Completions behavior for generic
        // OpenAI-compatible hosts that supplied an origin-only URL.
        if (matches!(wire_api, OpenAiWireApi::Responses) && canonical_openai_host)
            || wire_api == OpenAiWireApi::AnthropicMessages
        {
            endpoint.push_str("/v1");
        }
        endpoint.push_str(suffix);
    } else {
        endpoint.push_str(suffix);
    }
    Ok(endpoint)
}

fn is_openai_endpoint(endpoint: &str) -> bool {
    endpoint
        .strip_prefix("https://")
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        == Some("api.openai.com")
}

impl OpenAiCompatibleBackend {
    fn complete_openai(
        &self,
        request: CompletionRequest<'_>,
        control: &mut RequestControl<'_>,
        auth: &mut Option<AuthContext>,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        control.check_cancelled()?;
        let model = request.model.unwrap_or(&self.model);
        if model.trim().is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
            return Err(BackendError::Configuration(
                "model must be non-empty and at most 256 bytes".into(),
            ));
        }
        self.validate_model(Some(model))?;
        let route = self.explicit_route(model)?;
        let wire_api = route
            .as_ref()
            .map_or(self.wire_api, |route| route.protocol().wire_api());
        let descriptor = self.model_descriptor(Some(model))?;
        let capabilities = self
            .model_capabilities(Some(model))?
            .unwrap_or_else(|| ProviderCapabilities::for_wire_api(self.wire_api));
        let options = protocols::RequestOptions {
            wire_api,
            model,
            provider: self
                .registry
                .as_ref()
                .map(|(provider, _)| provider.as_str()),
            reasoning_effort: self.reasoning_effort.as_deref(),
            verbosity: self.verbosity.as_deref(),
            capabilities,
            descriptor: descriptor.as_ref(),
        };
        let body = match &route {
            Some(route) => protocols::encode_request_for_route(&request, &options, route)?,
            None => protocols::encode_request(&request, &options)?,
        };
        let endpoint = if let Some(route) = &route {
            route.url().to_owned()
        } else if wire_api == OpenAiWireApi::GoogleGenerativeAi {
            crate::protocols::google::endpoint(&self.endpoint, model, capabilities.streaming)?
        } else {
            self.endpoint.clone()
        };
        let binding = self.request_binding(Some(model))?;
        if control.scope.route_digest != binding.route_digest
            || control.scope.identity_scope != binding.identity_scope
        {
            return Err(BackendError::AdmissionDenied(
                "request scope does not match the backend route".into(),
            ));
        }
        let secret_scope = SecretScope {
            route_digest: control.scope.route_digest.clone(),
            identity_scope: control.scope.identity_scope.clone(),
        };
        if let (Some(state), Some(route)) = (&self.explicit, &route)
            && auth.is_none()
        {
            *auth = Some(
                state
                    .resolver
                    .resolve(route, control, 1_000)
                    .map_err(auth_error)?,
            );
        }
        if let Some(secret) = &self.secret_handle {
            let policy = self.secret_policy_digest.as_deref().ok_or_else(|| {
                BackendError::Configuration("secret policy binding missing".into())
            })?;
            if control
                .scope
                .policy_digest
                .as_deref()
                .is_some_and(|value| value != policy)
            {
                return Err(BackendError::AdmissionDenied(
                    "request policy binding mismatch".into(),
                ));
            }
            if secret.is_scoped() {
                secret
                    .verify_scope(policy, &secret_scope)
                    .map_err(secret_error)?;
            } else {
                secret.verify_policy_digest(policy).map_err(secret_error)?;
            }
        }
        control.before_send(HttpRequestKind::Inference)?;
        if let (Some(state), Some(route), Some(auth)) = (&self.explicit, &route, auth.as_ref()) {
            state
                .resolver
                .final_preflight(route, auth, control)
                .map_err(auth_error)?;
        }
        let cancelled = || control.check_cancelled().is_err();
        let request_timeout = control.deadline.map_or(self.request_timeout, |deadline| {
            deadline
                .saturating_duration_since(Instant::now())
                .min(self.request_timeout)
        });
        if request_timeout.is_zero() {
            return Err(BackendError::DeadlineExceeded);
        }
        let (request_client, cancellation) = transport::client(self.client.config().clone());
        let mut request_builder = request_client
            .post(&endpoint)
            .header("content-type", "application/json")
            .header(
                "x-idempotency-key",
                idempotency_key(&endpoint, request.turn_id, &body)?,
            );
        if wire_api == OpenAiWireApi::AnthropicMessages {
            request_builder = request_builder.header("anthropic-version", "2023-06-01");
        }
        if let (Some(route), Some(auth)) = (&route, auth.as_ref()) {
            request_builder = auth
                .with_secret(auth.policy_digest(), &secret_scope, |secret| {
                    match (route.header_policy(), secret) {
                        (AuthHeaderPolicy::None, None) => Ok(request_builder),
                        (AuthHeaderPolicy::Bearer, Some(key)) => {
                            Ok(request_builder.header("authorization", format!("Bearer {key}")))
                        }
                        (AuthHeaderPolicy::XApiKey, Some(key)) => {
                            Ok(request_builder.header("x-api-key", key))
                        }
                        (AuthHeaderPolicy::GoogleApiKey, Some(key)) => {
                            Ok(request_builder.header("x-goog-api-key", key))
                        }
                        (AuthHeaderPolicy::Codex, Some(token)) => {
                            let account = auth.account_id().ok_or_else(|| {
                                BackendError::Configuration(
                                    "Codex account identity is missing".into(),
                                )
                            })?;
                            Ok(request_builder
                                .header("authorization", format!("Bearer {token}"))
                                .header("chatgpt-account-id", account)
                                .header("accept", "text/event-stream")
                                .header("openai-beta", "responses=experimental")
                                .header("originator", crate::providers::codex::ORIGINATOR)
                                .header(
                                    "user-agent",
                                    format!(
                                        "zenpi/{} ({}; {})",
                                        env!("CARGO_PKG_VERSION"),
                                        std::env::consts::OS,
                                        std::env::consts::ARCH,
                                    ),
                                ))
                        }
                        _ => Err(BackendError::Configuration(
                            "credential header policy mismatch".into(),
                        )),
                    }
                })
                .map_err(auth_error)??;
        } else if let Some(secret) = &self.secret_handle {
            let digest = self.secret_policy_digest.as_deref().ok_or_else(|| {
                BackendError::Configuration("secret policy binding missing".into())
            })?;
            let header_value = |key: &str| {
                if self.wire_api == OpenAiWireApi::GoogleGenerativeAi {
                    key.to_owned()
                } else {
                    format!("Bearer {key}")
                }
            };
            let header = if secret.is_scoped() {
                secret.with_scoped_secret(digest, &secret_scope, header_value)
            } else {
                secret.with_secret(digest, header_value)
            }
            .map_err(secret_error)?;
            request_builder = request_builder.header(
                if self.wire_api == OpenAiWireApi::GoogleGenerativeAi {
                    "x-goog-api-key"
                } else {
                    "authorization"
                },
                header,
            );
        } else if let Some(key) = &self.api_key {
            request_builder = if self.wire_api == OpenAiWireApi::GoogleGenerativeAi {
                request_builder.header("x-goog-api-key", key)
            } else {
                request_builder.header("authorization", format!("Bearer {key}"))
            };
        }
        // Both adapters use short receive slices so host cancellation can be
        // observed while a provider is sending a slow body. The Chat JSON
        // reader below resumes the same response after a slice timeout; it
        // never opens a second request.
        if self.wire_api == OpenAiWireApi::Responses
            || self.wire_api == OpenAiWireApi::ChatCompletions
            || self.wire_api == OpenAiWireApi::AnthropicMessages
            || self.wire_api == OpenAiWireApi::GoogleGenerativeAi
        {
            request_builder = request_builder
                // A decompressor may not be resumable after an underlying
                // timeout. SSE is already text and benefits little from
                // compression, so keep cancellation polling on raw bytes.
                .header("accept-encoding", "identity")
                .config()
                .max_redirects(0)
                .timeout_global(Some(request_timeout))
                // Bound each receive slice to the cancellation poll interval:
                // the body readers treat a RecvBody timeout as a poll and resume
                // the same response, so host cancellation is observed within this
                // deadline instead of waiting for the provider to finish.
                .timeout_recv_body(Some(Duration::from_millis(100).min(request_timeout)))
                .build();
        }
        let response = transport::send_json(request_builder, &body, &cancelled, cancellation);
        control.check_cancelled()?;
        let mut response = response?;
        let status = response.status().as_u16();
        if status >= 300 {
            return Err(BackendError::HttpStatus {
                status,
                retry_after_ms: response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(parse_retry_after),
            });
        }
        control.check_cancelled()?;
        let sse = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/event-stream"))
            });
        let mut checked_sink = |event| {
            control.check_cancelled()?;
            sink(event)
        };
        let completion = match &route {
            Some(route) => protocols::read_response_for_route(
                response.body_mut(),
                sse,
                &options,
                route,
                &cancelled,
                &mut checked_sink,
            ),
            None => protocols::read_response(
                response.body_mut(),
                sse,
                &options,
                &cancelled,
                &mut checked_sink,
            ),
        };
        control.check_cancelled()?;
        completion
    }
}

impl Backend for OpenAiCompatibleBackend {
    fn connection_snapshot(
        &self,
        model: Option<&str>,
    ) -> Result<Option<ConnectionSnapshot>, BackendError> {
        let Some(state) = &self.explicit else {
            return Ok(None);
        };
        // The route digest and identity scope are per model, so a snapshot
        // without a model describes the connection but not a route.
        let model = model.unwrap_or(self.model.as_str());
        let route = self.explicit_route(model)?;
        let (auth_kind, credential_ref) = match &state.connection.auth {
            AuthBinding::LegacyApiKey => ("legacy_api_key", None),
            AuthBinding::StoredApiKey { credential_id } => ("api_key", Some(credential_id.clone())),
            AuthBinding::CodexOAuth { credential_id } => ("oauth", Some(credential_id.clone())),
            AuthBinding::Anonymous => ("anonymous", None),
        };
        Ok(Some(ConnectionSnapshot {
            profile: state.connection.profile.clone(),
            provider: state.connection.provider.clone(),
            protocol: state.connection.protocol.as_str().to_owned(),
            auth_kind: auth_kind.to_owned(),
            credential_ref,
            identity_scope: route
                .as_ref()
                .map(|route| route.identity_scope().to_owned())
                .unwrap_or_default(),
            route_digest: route
                .as_ref()
                .map(|route| route.route_digest().to_owned())
                .unwrap_or_default(),
            definition_version: route.as_ref().map_or(0, |route| route.definition_version()),
            config_revision: state.connection.config_revision,
        }))
    }

    fn request_binding(&self, model: Option<&str>) -> Result<BackendRequestBinding, BackendError> {
        let model = model.unwrap_or(&self.model);
        if let Some(route) = self.explicit_route(model)? {
            return Ok(BackendRequestBinding {
                route_digest: route.route_digest().into(),
                identity_scope: route.identity_scope().into(),
            });
        }
        let descriptor = self.model_descriptor(Some(model))?;
        let capabilities = self
            .model_capabilities(Some(model))?
            .unwrap_or_else(|| ProviderCapabilities::for_wire_api(self.wire_api));
        let endpoint = if self.wire_api == OpenAiWireApi::GoogleGenerativeAi {
            crate::protocols::google::endpoint(&self.endpoint, model, capabilities.streaming)?
        } else {
            self.endpoint.clone()
        };
        legacy_request_binding(&json!({
            "kind": "legacy_provider_backend",
            "endpoint": endpoint,
            "wire": self.wire_api.as_str(),
            "provider": self.registry.as_ref().map(|(provider, _)| provider.as_str()),
            "model": model,
            "model_descriptor": descriptor.as_ref().map(|value| value.digest()),
            "capabilities": capabilities,
            "reasoning_effort": self.reasoning_effort,
            "verbosity": self.verbosity,
            "timeout_ms": self.request_timeout.as_millis(),
            "max_retries": self.max_retries,
        }))
    }

    fn model_descriptor(
        &self,
        model: Option<&str>,
    ) -> Result<Option<crate::providers::registry::ModelDescriptor>, BackendError> {
        if let Some(route) = self.explicit_route(model.unwrap_or(&self.model))? {
            return Ok(Some(route.model().clone()));
        }
        self.registry
            .as_ref()
            .map(|(provider, registry)| registry.resolve(provider, model.unwrap_or(&self.model)))
            .transpose()
            .map_err(|error| BackendError::Configuration(error.to_string()))
    }

    fn model_catalog(&self) -> Vec<crate::providers::registry::ModelDescriptor> {
        self.registry
            .as_ref()
            .map(|(provider, registry)| registry.list(provider))
            .unwrap_or_default()
    }

    fn model_capabilities(
        &self,
        model: Option<&str>,
    ) -> Result<Option<ProviderCapabilities>, BackendError> {
        if let Some(route) = self.explicit_route(model.unwrap_or(&self.model))? {
            return Ok(Some(route.capabilities()));
        }
        Ok(self.model_descriptor(model)?.map(|model| {
            model.effective_capabilities(ProviderCapabilities::for_wire_api(self.wire_api))
        }))
    }

    fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort.as_deref()
    }

    fn validate_reasoning_effort(
        &self,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<(), BackendError> {
        if let Some(route) = self.explicit_route(model.unwrap_or(&self.model))? {
            return validate_route_reasoning(&route, effort);
        }
        if self.wire_api == OpenAiWireApi::GoogleGenerativeAi {
            crate::protocols::google::validate_effort(model.unwrap_or(&self.model), effort)?;
        }
        if self.wire_api == OpenAiWireApi::AnthropicMessages {
            crate::protocols::anthropic::validate_effort(model.unwrap_or(&self.model), effort)?;
        }
        let model = self.model_descriptor(model)?.ok_or_else(|| {
            BackendError::Configuration(
                "runtime reasoning settings require a registry-bound provider".into(),
            )
        })?;
        model
            .validate_reasoning(ProviderCapabilities::for_wire_api(self.wire_api), effort)
            .map_err(|error| BackendError::Configuration(error.to_string()))
    }

    fn commit_reasoning_effort(&mut self, effort: Option<String>) {
        self.reasoning_effort = effort;
    }

    fn validate_history_model(
        &self,
        turns: &[Turn],
        model: Option<&str>,
    ) -> Result<(), BackendError> {
        let model = model.unwrap_or(&self.model);
        let route = self.explicit_route(model)?;
        let wire_api = route
            .as_ref()
            .map_or(self.wire_api, |route| route.protocol().wire_api());
        let provider =
            self.registry
                .as_ref()
                .map(|(p, _)| p.as_str())
                .unwrap_or(match self.wire_api {
                    OpenAiWireApi::AnthropicMessages => "anthropic",
                    OpenAiWireApi::GoogleGenerativeAi => "google",
                    _ => "openai",
                });
        for turn in turns {
            chat::validate_history(turn, provider, model, wire_api.as_str())?;
            for a in turn
                .metadata
                .as_ref()
                .and_then(|m| m.get("annotations"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|a| a["type"] == "native_history")
            {
                if a["wire"] != wire_api.as_str() || a["provider"] != provider {
                    return Err(BackendError::Configuration(
                        "native history provider/wire cannot represent selected model".into(),
                    ));
                }
                let opaque = a
                    .get("blocks")
                    .and_then(Value::as_array)
                    .is_some_and(|blocks| {
                        blocks.iter().any(|b| {
                            matches!(b["type"].as_str(), Some("thinking" | "redacted_thinking"))
                        })
                    })
                    || a.get("parts")
                        .and_then(Value::as_array)
                        .is_some_and(|parts| {
                            parts.iter().any(|p| {
                                p.get("thoughtSignature").is_some() || p["thought"] == true
                            })
                        });
                if opaque && a["model"] != model {
                    return Err(BackendError::Configuration(
                        "opaque signed history cannot move to another model".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_model(&self, model: Option<&str>) -> Result<(), BackendError> {
        crate::providers::registry::validate_identity("backend", model.unwrap_or(&self.model))
            .map_err(|error| BackendError::Configuration(error.to_string()))?;
        if let Some(route) = self.explicit_route(model.unwrap_or(&self.model))? {
            return validate_route_reasoning(&route, self.reasoning_effort.as_deref());
        }
        if self.wire_api == OpenAiWireApi::GoogleGenerativeAi {
            crate::protocols::google::validate_effort(
                model.unwrap_or(&self.model),
                self.reasoning_effort.as_deref(),
            )?;
        }
        if self.wire_api == OpenAiWireApi::AnthropicMessages {
            crate::protocols::anthropic::validate_effort(
                model.unwrap_or(&self.model),
                self.reasoning_effort.as_deref(),
            )?;
        }
        if let Some(model) = self.model_descriptor(model)? {
            model
                .validate_reasoning(
                    ProviderCapabilities::for_wire_api(self.wire_api),
                    self.reasoning_effort.as_deref(),
                )
                .map_err(|error| BackendError::Configuration(error.to_string()))?;
        }
        Ok(())
    }

    fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        let scope = RequestScope::legacy(self.request_binding(request.model)?, &request);
        self.complete_openai(
            request,
            &mut RequestControl {
                cancelled: &|| false,
                deadline: None,
                scope,
                before_send: &mut |_, _| Ok(()),
            },
            &mut None,
            &mut |_| Ok(()),
        )
    }

    fn complete_with_control(
        &self,
        request: CompletionRequest<'_>,
        cancelled: &dyn Fn() -> bool,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        let scope = RequestScope::legacy(self.request_binding(request.model)?, &request);
        self.complete_with_request_control(
            request,
            &mut RequestControl {
                cancelled,
                deadline: None,
                scope,
                before_send: &mut |_, _| Ok(()),
            },
            sink,
        )
    }

    fn complete_with_request_control(
        &self,
        request: CompletionRequest<'_>,
        control: &mut RequestControl<'_>,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        control.check_cancelled()?;
        {
            let mut circuit = self
                .circuit
                .lock()
                .map_err(|_| BackendError::Transport("provider circuit lock poisoned".into()))?;
            if let Some(until) = circuit.open_until
                && until > Instant::now()
            {
                let remaining = until.saturating_duration_since(Instant::now());
                return Err(BackendError::CircuitOpen {
                    retry_after_ms: duration_ms_ceil(remaining),
                });
            }
            circuit.open_until = None;
        }
        let model_tools = self
            .model_capabilities(request.model)?
            .map(|capabilities| capabilities.tools);
        let mut attempt = 0_u32;
        let mut auth = None;
        let mut recovered_unauthorized = false;
        loop {
            let mut emitted = false;
            let result = self.complete_openai(
                CompletionRequest {
                    turn_id: request.turn_id,
                    turns: request.turns,
                    model: request.model,
                    tools: request.tools,
                    instructions: request.instructions,
                    metadata: request.metadata,
                    attachments: request.attachments,
                    response_format: request.response_format,
                    max_output_tokens: request.max_output_tokens,
                },
                control,
                &mut auth,
                &mut |event| {
                    if model_tools == Some(false)
                        && matches!(
                            event,
                            ProviderEvent::ToolCallDelta { .. }
                                | ProviderEvent::ToolCallDone { .. }
                        )
                    {
                        return Err(BackendError::InvalidResponse(
                            "selected model returned an unsupported tool call".into(),
                        ));
                    }
                    emitted = true;
                    sink(event)
                },
            );
            if matches!(result, Err(BackendError::HttpStatus { status: 401, .. }))
                && !emitted
                && !recovered_unauthorized
                && attempt < self.max_retries
                && control.scope.purpose != RequestPurpose::SemanticCompaction
                && !request
                    .metadata
                    .is_some_and(|value| value["purpose"] == "semantic_compaction")
                && let Some(state) = &self.explicit
                && matches!(state.connection.auth, AuthBinding::CodexOAuth { .. })
            {
                let route = self
                    .explicit_route(request.model.unwrap_or(&self.model))?
                    .ok_or_else(|| {
                        BackendError::Configuration("explicit route is missing".into())
                    })?;
                let revision = auth
                    .as_ref()
                    .and_then(AuthContext::credential_revision)
                    .ok_or_else(|| {
                        BackendError::Configuration("credential revision is missing".into())
                    })?;
                auth = Some(
                    state
                        .resolver
                        .recover_unauthorized(&route, revision, control, 1_000)
                        .map_err(auth_error)?,
                );
                recovered_unauthorized = true;
                attempt = attempt.saturating_add(1);
                continue;
            }
            let should_retry = result.as_ref().is_err_and(|error| {
                !emitted
                    && error.is_retryable()
                    && attempt < self.max_retries
                    && control.scope.purpose != RequestPurpose::SemanticCompaction
                    && !request
                        .metadata
                        .is_some_and(|v| v["purpose"] == "semantic_compaction")
            });
            if !should_retry {
                let mut circuit = self.circuit.lock().map_err(|_| {
                    BackendError::Transport("provider circuit lock poisoned".into())
                })?;
                match &result {
                    Ok(_) => {
                        circuit.consecutive_failures = 0;
                        circuit.open_until = None;
                    }
                    Err(error) if error.is_retryable() => {
                        circuit.consecutive_failures =
                            circuit.consecutive_failures.saturating_add(1);
                        if circuit.consecutive_failures >= self.circuit_failure_threshold {
                            circuit.open_until = Some(Instant::now() + self.circuit_cooldown);
                        }
                    }
                    Err(_) => {}
                }
                return result;
            }
            attempt = attempt.saturating_add(1);
            auth = None;
            sink(ProviderEvent::Warning {
                message: format!("provider request retry {attempt}/{}", self.max_retries),
            })?;
            // Capped exponential backoff. Polling the cancellation predicate
            // avoids making an interrupt wait for the whole sleep.
            let exponential = Duration::from_millis(
                100_u64.saturating_mul(1_u64 << attempt.saturating_sub(1).min(5)),
            );
            let retry_after = match &result {
                Err(BackendError::HttpStatus {
                    retry_after_ms: Some(milliseconds),
                    ..
                }) => Duration::from_millis(*milliseconds),
                _ => Duration::ZERO,
            };
            // Add bounded jitter so a large swarm does not retry in lockstep
            // (opencode-style resilience; the cap keeps the delay bounded).
            let base = exponential.max(retry_after);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.subsec_nanos() as u64)
                .unwrap_or(0);
            let delay = jittered_retry_delay(base, nanos);
            let deadline = std::time::Instant::now() + delay;
            while std::time::Instant::now() < deadline {
                control.check_cancelled()?;
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    fn name(&self) -> &str {
        if self.wire_api == OpenAiWireApi::GoogleGenerativeAi {
            return "google-generative-ai";
        }
        if self.wire_api == OpenAiWireApi::AnthropicMessages {
            return "anthropic-messages";
        }
        "openai-compatible"
    }

    fn model(&self) -> Option<&str> {
        Some(&self.model)
    }
}

fn legacy_request_binding(value: &Value) -> Result<BackendRequestBinding, BackendError> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(value)
        .map_err(|_| BackendError::Configuration("invalid request binding".into()))?;
    let route_digest = format!("{:x}", Sha256::digest(bytes));
    Ok(BackendRequestBinding {
        identity_scope: format!("legacy:{route_digest}"),
        route_digest,
    })
}

fn idempotency_key(endpoint: &str, turn_id: &str, body: &Value) -> Result<String, BackendError> {
    use sha2::{Digest, Sha256};

    let payload = serde_json::to_vec(body).map_err(|_| {
        BackendError::Configuration("cannot encode provider request identity".into())
    })?;
    let mut digest = Sha256::new();
    // Retries of identical payloads share an identity; tool continuations in
    // the same outer turn do not. Length prefixes separate arbitrary inputs.
    digest.update(b"zenpi-provider-request-v2\0");
    digest.update((endpoint.len() as u64).to_be_bytes());
    digest.update(endpoint.as_bytes());
    digest.update((turn_id.len() as u64).to_be_bytes());
    digest.update(turn_id.as_bytes());
    digest.update(payload);
    Ok(format!("zenpi-{:x}", digest.finalize()))
}

const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

fn parse_retry_after(value: &str) -> Option<u64> {
    let delay = if let Ok(seconds) = value.trim().parse::<u64>() {
        Duration::from_secs(seconds)
    } else {
        let deadline = httpdate::parse_http_date(value.trim()).ok()?;
        deadline.duration_since(std::time::SystemTime::now()).ok()?
    }
    .min(MAX_RETRY_AFTER);
    Some(duration_ms_ceil(delay))
}

fn duration_ms_ceil(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis())
        .unwrap_or(u64::MAX)
        .max(u64::from(!duration.is_zero()))
}

fn secret_error(error: SecretError) -> BackendError {
    BackendError::Configuration(error.to_string())
}

fn auth_error(error: AuthError) -> BackendError {
    match error {
        AuthError::Cancelled => BackendError::Cancelled,
        AuthError::DeadlineExceeded => BackendError::DeadlineExceeded,
        AuthError::SendDenied | AuthError::BudgetExceeded => {
            BackendError::AdmissionDenied(error.to_string())
        }
        // Retrying an uncertain OAuth exchange as an inference retry is unsafe.
        other => BackendError::Authentication { code: other.code() },
    }
}

fn validate_route_reasoning(
    route: &ValidatedRoute,
    effort: Option<&str>,
) -> Result<(), BackendError> {
    if effort.is_some() && !route.options().reasoning_effort {
        return Err(BackendError::Configuration(
            "route does not support reasoning effort".into(),
        ));
    }
    route
        .model()
        .validate_reasoning(route.capabilities(), effort)
        .map_err(|error| BackendError::Configuration(error.to_string()))?;
    match (route.dialect(), route.protocol().wire_api()) {
        (Dialect::Default, OpenAiWireApi::GoogleGenerativeAi) => {
            crate::protocols::google::validate_effort(&route.model().id, effort)
        }
        (Dialect::Default, OpenAiWireApi::AnthropicMessages) => {
            crate::protocols::anthropic::validate_effort(&route.model().id, effort)
        }
        _ => Ok(()),
    }
}

fn map_ureq_error(error: ureq::Error) -> BackendError {
    match error {
        ureq::Error::StatusCode(status) => BackendError::HttpStatus {
            status,
            retry_after_ms: None,
        },
        other => BackendError::Transport(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenAiCompatibleBackend, OpenAiWireApi};

    #[test]
    fn openai_host_requires_credentials_after_endpoint_normalization() {
        let error = OpenAiCompatibleBackend::from_values_with_wire_api(
            "https://api.openai.com".into(),
            None,
            "gpt-4o-mini".into(),
            OpenAiWireApi::ChatCompletions,
        )
        .expect_err("the normalized OpenAI host must require an API key");
        assert!(error.to_string().contains("API_KEY"));

        let query_error = OpenAiCompatibleBackend::from_values_with_wire_api(
            "https://api.openai.com?tenant=default".into(),
            None,
            "gpt-4o-mini".into(),
            OpenAiWireApi::ChatCompletions,
        )
        .expect_err("query-bearing endpoints must be rejected before any request");
        assert!(query_error.to_string().contains("endpoint"));
    }

    #[test]
    fn responses_wire_api_normalizes_and_parses_names() {
        let backend = OpenAiCompatibleBackend::new_with_wire_api(
            "https://api.openai.com/v1",
            Some("secret".into()),
            "gpt-5",
            OpenAiWireApi::Responses,
        )
        .unwrap();
        assert_eq!(backend.endpoint(), "https://api.openai.com/v1/responses");
        assert_eq!(backend.wire_api(), OpenAiWireApi::Responses);
        assert_eq!(
            "responses".parse::<OpenAiWireApi>().unwrap(),
            OpenAiWireApi::Responses
        );
        assert!("wat".parse::<OpenAiWireApi>().is_err());

        let codex_proxy = OpenAiCompatibleBackend::new_with_wire_api(
            "http://10.20.30.168:18080",
            Some("secret".into()),
            "gpt-5",
            OpenAiWireApi::Responses,
        )
        .unwrap();
        assert_eq!(
            codex_proxy.endpoint(),
            "http://10.20.30.168:18080/responses"
        );

        let missing_key = OpenAiCompatibleBackend::from_values_with_wire_api(
            "https://api.openai.com".into(),
            None,
            "gpt-5".into(),
            OpenAiWireApi::Responses,
        )
        .expect_err("canonical OpenAI Responses host must require a key");
        assert!(missing_key.to_string().contains("API_KEY"));
    }
}
