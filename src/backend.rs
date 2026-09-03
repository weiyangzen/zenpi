//! Backend boundary for zenpi.
//!
//! The core deliberately knows nothing about HTTP, model SDKs, or streaming
//! transports.  A backend receives an immutable view of the session and
//! returns one normalized completion.  This keeps the headless and TUI modes
//! behaviorally identical and makes a deterministic backend useful in tests.

use std::{env, str::FromStr, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::core::{Turn, TurnRole};
use crate::tools::{ToolCall, ToolDefinition};

/// Keep provider responses bounded even when an endpoint omits a content
/// length.  The core applies the smaller per-turn text limit afterwards.
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

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
        }
    }

    pub fn with_instructions(mut self, instructions: Option<&'a str>) -> Self {
        self.instructions = instructions;
        self
    }

    pub fn with_metadata(mut self, metadata: Option<&'a Value>) -> Self {
        self.metadata = metadata;
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

/// Errors at the provider boundary.  They are surfaced as typed headless
/// responses rather than panics or silent fallback to another model.
#[derive(Debug, Error)]
pub enum BackendError {
    #[error("backend configuration: {0}")]
    Configuration(String),
    #[error("backend transport: {0}")]
    Transport(String),
    #[error("backend HTTP status: {status}")]
    HttpStatus { status: u16 },
    #[error("backend returned an invalid response: {0}")]
    InvalidResponse(String),
    #[error("backend returned an empty completion")]
    EmptyResponse,
    #[error("backend request was cancelled")]
    Cancelled,
    #[error("backend request was superseded by a steer")]
    Steered,
}

impl BackendError {
    /// Stable classification for hosts that need to decide whether a failed
    /// turn may be retried without parsing display text.
    pub const fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::HttpStatus { status } => {
                *status == 408
                    || *status == 409
                    || *status == 425
                    || *status == 429
                    || (*status >= 500 && *status <= 599)
            }
            Self::Configuration(_)
            | Self::InvalidResponse(_)
            | Self::EmptyResponse
            | Self::Cancelled
            | Self::Steered => false,
        }
    }

    pub const fn code(&self) -> &'static str {
        match self {
            Self::Configuration(_) => "backend_configuration",
            Self::Transport(_) => "backend_transport",
            Self::HttpStatus { .. } => "backend_http_status",
            Self::InvalidResponse(_) => "backend_invalid_response",
            Self::EmptyResponse => "backend_empty_response",
            Self::Cancelled => "backend_cancelled",
            Self::Steered => "backend_steered",
        }
    }
}

/// Synchronous provider contract.  A synchronous boundary is intentional:
/// the default binary stays tiny and callers that need concurrency can run
/// independent agents in their own processes.
pub trait Backend: Send + Sync {
    fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, BackendError>;

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

    fn name(&self) -> &str {
        "backend"
    }

    /// Return the configured model when the backend has one. Hosts use this
    /// for status snapshots without downcasting a provider implementation.
    fn model(&self) -> Option<&str> {
        None
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
}

impl OpenAiWireApi {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
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
            other => Err(BackendError::Configuration(format!(
                "unsupported OpenAI wire API {other:?}; use chat_completions or responses"
            ))),
        }
    }
}

/// A synchronous OpenAI-compatible HTTP backend.
///
/// Chat Completions uses a bounded JSON response; Responses uses the
/// provider's server-sent event stream and folds text deltas into the same
/// normalized completion. `ureq` is blocking and connection pooling keeps
/// the steady-state process smaller than an async runtime.
pub struct OpenAiCompatibleBackend {
    client: ureq::Agent,
    endpoint: String,
    api_key: Option<String>,
    model: String,
    wire_api: OpenAiWireApi,
    reasoning_effort: Option<String>,
    verbosity: Option<String>,
}

impl std::fmt::Debug for OpenAiCompatibleBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiCompatibleBackend")
            .field("endpoint", &self.endpoint)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("model", &self.model)
            .field("wire_api", &self.wire_api)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("verbosity", &self.verbosity)
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
        let model = model.into();
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
            .timeout_recv_body(Some(Duration::from_millis(250)))
            .build();
        Ok(Self {
            client: ureq::Agent::new_with_config(config),
            endpoint,
            api_key,
            model,
            wire_api,
            reasoning_effort,
            verbosity,
        })
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
    let canonical_openai_host = is_openai_endpoint(&endpoint);
    let suffix = match wire_api {
        OpenAiWireApi::ChatCompletions => "/chat/completions",
        OpenAiWireApi::Responses => "/responses",
    };
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
        if matches!(wire_api, OpenAiWireApi::Responses) && canonical_openai_host {
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
        cancelled: &dyn Fn() -> bool,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        if cancelled() {
            return Err(BackendError::Cancelled);
        }
        let model = request.model.unwrap_or(&self.model);
        if model.trim().is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
            return Err(BackendError::Configuration(
                "model must be non-empty and at most 256 bytes".into(),
            ));
        }
        let body = match self.wire_api {
            OpenAiWireApi::ChatCompletions => {
                let messages: Vec<Value> = request.turns.iter().map(chat_message).collect();
                let mut body = json!({
                    "model": model,
                    "messages": messages,
                    "stream": false,
                });
                if !request.tools.is_empty() {
                    body["tools"] = Value::Array(
                        request
                            .tools
                            .iter()
                            .map(|tool| {
                                json!({
                                    "type": "function",
                                    "function": {
                                        "name": tool.name,
                                        "description": tool.description,
                                        "parameters": tool.input_schema,
                                    }
                                })
                            })
                            .collect(),
                    );
                    body["tool_choice"] = json!("auto");
                }
                if let Some(instructions) = request.instructions {
                    body["instructions"] = json!(instructions);
                }
                if let Some(metadata) = request.metadata {
                    body["metadata"] = metadata.clone();
                }
                body
            }
            OpenAiWireApi::Responses => {
                let input: Vec<Value> = request
                    .turns
                    .iter()
                    .flat_map(responses_input_items)
                    .collect();
                let mut body = json!({
                    "model": model,
                    "input": input,
                    "stream": true,
                    "store": false,
                });
                if let Some(effort) = &self.reasoning_effort {
                    body["reasoning"] = json!({"effort": effort});
                }
                if let Some(verbosity) = &self.verbosity {
                    body["text"] = json!({"verbosity": verbosity});
                }
                if !request.tools.is_empty() {
                    body["tools"] = Value::Array(
                        request
                            .tools
                            .iter()
                            .map(|tool| {
                                json!({
                                    "type": "function",
                                    "name": tool.name,
                                    "description": tool.description,
                                    "parameters": tool.input_schema,
                                })
                            })
                            .collect(),
                    );
                    body["tool_choice"] = json!("auto");
                }
                if let Some(instructions) = request.instructions {
                    body["instructions"] = json!(instructions);
                }
                if let Some(metadata) = request.metadata {
                    body["metadata"] = metadata.clone();
                }
                body
            }
        };
        let mut request_builder = self
            .client
            .post(&self.endpoint)
            .header("content-type", "application/json");
        if let Some(key) = &self.api_key {
            request_builder = request_builder.header("authorization", format!("Bearer {key}"));
        }
        let mut response = request_builder.send_json(&body).map_err(map_ureq_error)?;
        if cancelled() {
            return Err(BackendError::Cancelled);
        }
        match self.wire_api {
            OpenAiWireApi::ChatCompletions => {
                let payload: Value = response
                    .body_mut()
                    .with_config()
                    .limit(MAX_RESPONSE_BYTES as u64)
                    .read_json()
                    .map_err(map_ureq_error)?;
                let tool_calls = extract_chat_tool_calls(&payload)?;
                let content = match extract_content(&payload) {
                    Ok(content) => content,
                    Err(BackendError::InvalidResponse(_)) if !tool_calls.is_empty() => {
                        String::new()
                    }
                    Err(error) => return Err(error),
                };
                if content.trim().is_empty() && tool_calls.is_empty() {
                    return Err(BackendError::EmptyResponse);
                }
                let completion = Completion {
                    content,
                    usage: payload.get("usage").and_then(parse_usage),
                    model: payload
                        .get("model")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    tool_calls,
                    response_id: payload.get("id").and_then(Value::as_str).map(str::to_owned),
                    refusal: extract_chat_refusal(&payload),
                    annotations: extract_annotations(&payload),
                };
                emit_completion_events(&completion, sink)?;
                Ok(completion)
            }
            OpenAiWireApi::Responses => read_responses_stream(response.body_mut(), cancelled, sink),
        }
    }
}

impl Backend for OpenAiCompatibleBackend {
    fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        self.complete_openai(request, &|| false, &mut |_| Ok(()))
    }

    fn complete_with_control(
        &self,
        request: CompletionRequest<'_>,
        cancelled: &dyn Fn() -> bool,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        self.complete_openai(request, cancelled, sink)
    }

    fn name(&self) -> &str {
        "openai-compatible"
    }

    fn model(&self) -> Option<&str> {
        Some(&self.model)
    }
}

fn chat_message(turn: &Turn) -> Value {
    let role = match turn.role {
        TurnRole::System => "system",
        TurnRole::User => "user",
        TurnRole::Assistant => "assistant",
        TurnRole::Tool => "tool",
    };
    let mut message = json!({"role": role, "content": turn.content});
    if turn.role == TurnRole::Tool {
        if let Some(call_id) = turn
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("tool_call_id"))
        {
            message["tool_call_id"] = call_id.clone();
        }
    } else if turn.role == TurnRole::Assistant
        && let Some(calls) = turn
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("tool_calls"))
    {
        message["tool_calls"] = Value::Array(
            calls
                .as_array()
                .into_iter()
                .flatten()
                .map(|item| {
                    json!({
                        "type": "function",
                        "id": item.get("id"),
                        "function": {
                            "name": item.get("name"),
                            "arguments": item.get("arguments").map_or_else(|| "{}".into(), Value::to_string),
                        }
                    })
                })
                .collect(),
        );
    }
    message
}

fn responses_input_items(turn: &Turn) -> Vec<Value> {
    if turn.role == TurnRole::Tool {
        let call_id = turn
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("tool_call_id"))
            .and_then(Value::as_str)
            .unwrap_or(&turn.id);
        return vec![json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": turn.content,
        })];
    }
    if turn.role == TurnRole::Assistant
        && let Some(calls) = turn
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("tool_calls"))
            .and_then(Value::as_array)
    {
        return calls
            .iter()
            .filter_map(|item| {
                Some(json!({
                    "type": "function_call",
                    "call_id": item.get("id")?.as_str()?,
                    "name": item.get("name")?.as_str()?,
                    "arguments": item.get("arguments")?.to_string(),
                }))
            })
            .collect();
    }
    let role = match turn.role {
        TurnRole::System => "system",
        TurnRole::User => "user",
        TurnRole::Assistant => "assistant",
        TurnRole::Tool => unreachable!(),
    };
    vec![json!({"role": role, "content": turn.content})]
}

fn map_ureq_error(error: ureq::Error) -> BackendError {
    match error {
        ureq::Error::StatusCode(status) => BackendError::HttpStatus { status },
        other => BackendError::Transport(other.to_string()),
    }
}

fn extract_content(payload: &Value) -> Result<String, BackendError> {
    let choice = payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| BackendError::InvalidResponse("missing choices[0]".into()))?;
    let content = choice
        .get("message")
        .and_then(|message| message.get("content"))
        .or_else(|| choice.get("text"));
    match content {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Array(parts)) => Ok(parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("")),
        Some(other) => Err(BackendError::InvalidResponse(format!(
            "completion content must be a string or text parts, got {other}"
        ))),
        None => Err(BackendError::InvalidResponse(
            "missing completion content".into(),
        )),
    }
}

fn extract_chat_refusal(payload: &Value) -> Option<String> {
    payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("refusal"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn extract_responses_refusal(payload: &Value) -> Option<String> {
    if let Some(refusal) = payload.get("refusal").and_then(Value::as_str) {
        return Some(refusal.to_owned());
    }
    payload
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        .flat_map(|item| item.get("content").and_then(Value::as_array))
        .flatten()
        .find_map(|part| {
            if part.get("type").and_then(Value::as_str) == Some("refusal") {
                part.get("refusal")
                    .or_else(|| part.get("text"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            } else {
                None
            }
        })
}

fn extract_annotations(payload: &Value) -> Vec<Value> {
    let mut annotations = Vec::new();
    if let Some(values) = payload.get("annotations").and_then(Value::as_array) {
        annotations.extend(values.iter().cloned());
    }
    if let Some(output) = payload.get("output").and_then(Value::as_array) {
        for item in output {
            if let Some(content) = item.get("content").and_then(Value::as_array) {
                for part in content {
                    if let Some(values) = part.get("annotations").and_then(Value::as_array) {
                        annotations.extend(values.iter().cloned());
                    }
                }
            }
        }
    }
    annotations
}

fn extract_responses_content(payload: &Value) -> Result<String, BackendError> {
    if let Some(text) = payload.get("output_text").and_then(Value::as_str) {
        return Ok(text.to_owned());
    }
    let output = payload
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| BackendError::InvalidResponse("missing output".into()))?;
    let mut text = String::new();
    for item in output {
        let Some(content) = item.get("content") else {
            continue;
        };
        match content {
            Value::String(value) => text.push_str(value),
            Value::Array(parts) => {
                for part in parts {
                    if let Some(value) = part.get("text").and_then(Value::as_str) {
                        text.push_str(value);
                    }
                }
            }
            other => {
                return Err(BackendError::InvalidResponse(format!(
                    "response content must be a string or text parts, got {other}"
                )));
            }
        }
    }
    if text.is_empty() {
        return Err(BackendError::InvalidResponse(
            "missing response output text".into(),
        ));
    }
    Ok(text)
}

fn read_responses_stream(
    body: &mut ureq::Body,
    cancelled: &dyn Fn() -> bool,
    sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
) -> Result<Completion, BackendError> {
    use std::io::{BufRead, BufReader};

    let configured = body.with_config().limit(MAX_RESPONSE_BYTES as u64).reader();
    let mut reader = BufReader::new(configured);
    // Some OpenAI-compatible proxies ignore `stream:true` and return a
    // regular Responses JSON object. Accept that shape as a compatibility
    // fallback while keeping the normal path event-aware.
    let mut content = String::new();
    let mut usage = None;
    let mut model = None;
    let mut saw_completed = false;
    let mut tool_calls = Vec::new();
    let mut response_id: Option<String> = None;
    let mut refusal: Option<String> = None;
    let mut annotations = Vec::new();
    let mut line_buffer = String::new();
    while reader
        .read_line(&mut line_buffer)
        .map_err(|error| BackendError::Transport(error.to_string()))?
        > 0
    {
        if cancelled() {
            return Err(BackendError::Cancelled);
        }
        let line = std::mem::take(&mut line_buffer);
        let clean_line = line
            .trim_matches(|character: char| character == '\0' || character.is_ascii_whitespace());
        if clean_line.starts_with('{') && !clean_line.contains("data:") {
            let payload: Value = serde_json::from_str(clean_line).map_err(|error| {
                BackendError::InvalidResponse(format!("invalid Responses JSON: {error}"))
            })?;
            let completion = completion_from_responses_json(&payload)?;
            emit_completion_events(&completion, sink)?;
            return Ok(completion);
        }
        let Some(data) = clean_line.strip_prefix("data:") else {
            continue;
        };
        // Some compatible gateways pad SSE frames with NUL bytes between
        // events. They are transport padding, not part of the JSON payload.
        let data = data
            .trim_matches(|character: char| character == '\0' || character.is_ascii_whitespace());
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let event: Value = serde_json::from_str(data).map_err(|error| {
            BackendError::InvalidResponse(format!("invalid SSE event: {error}"))
        })?;
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    content.push_str(delta);
                    sink(ProviderEvent::TextDelta {
                        delta: delta.to_owned(),
                    })?;
                }
            }
            Some("response.output_text.done") => {
                if let Some(value) = event.get("text").and_then(Value::as_str) {
                    if content.is_empty() {
                        content.push_str(value);
                    }
                    sink(ProviderEvent::TextDone {
                        text: value.to_owned(),
                    })?;
                }
            }
            Some("response.created") => {
                let response = event.get("response").unwrap_or(&event);
                response_id = response
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                model = response
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                sink(ProviderEvent::ResponseCreated {
                    response_id: response_id.clone(),
                    model: model.clone(),
                })?;
            }
            Some("response.output_item.done") => {
                if let Some(item) = event.get("item")
                    && item.get("type").and_then(Value::as_str) == Some("function_call")
                    && let Some(call) = parse_response_function_call(item)
                {
                    push_unique_tool_call(&mut tool_calls, call);
                    if let Some(call) = tool_calls.last().cloned() {
                        sink(ProviderEvent::ToolCallDone { call })?;
                    }
                }
            }
            Some("response.function_call_arguments.delta") => {
                sink(ProviderEvent::ToolCallDelta {
                    call_id: event
                        .get("call_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    name: event.get("name").and_then(Value::as_str).map(str::to_owned),
                    arguments_delta: event
                        .get("delta")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                })?;
            }
            Some("response.function_call_arguments.done") => {
                if let Some(call_id) = event.get("call_id").and_then(Value::as_str)
                    && let Some(name) = event.get("name").and_then(Value::as_str)
                {
                    let arguments = event
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    let call = parse_tool_call(call_id, name, arguments)?;
                    push_unique_tool_call(&mut tool_calls, call.clone());
                    sink(ProviderEvent::ToolCallDone { call })?;
                }
            }
            Some("response.completed") => {
                saw_completed = true;
                let response = event.get("response").unwrap_or(&event);
                usage = response.get("usage").and_then(parse_usage);
                model = response
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                response_id = response
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or(response_id);
                refusal = extract_responses_refusal(response);
                annotations = extract_annotations(response);
                if let Some(usage_value) = response.get("usage")
                    && let Some(parsed) = parse_usage(usage_value)
                {
                    sink(ProviderEvent::Usage { usage: parsed })?;
                }
                sink(ProviderEvent::Completed {
                    response_id: response_id.clone(),
                    model: model.clone(),
                })?;
            }
            Some("response.refusal.delta") | Some("response.refusal.done") => {
                if let Some(value) = event
                    .get("delta")
                    .or_else(|| event.get("text"))
                    .and_then(Value::as_str)
                {
                    refusal.get_or_insert_with(String::new).push_str(value);
                    sink(ProviderEvent::Refusal {
                        text: value.to_owned(),
                    })?;
                }
            }
            Some("response.failed") | Some("error") => {
                let message = extract_responses_error(&event);
                let _ = sink(ProviderEvent::Failed {
                    message: message.clone(),
                });
                return Err(BackendError::InvalidResponse(message));
            }
            _ => {}
        }
    }
    if !saw_completed {
        return Err(BackendError::InvalidResponse(
            "Responses stream ended without response.completed".into(),
        ));
    }
    if content.trim().is_empty() {
        if !tool_calls.is_empty() {
            return Ok(Completion {
                content,
                usage,
                model,
                tool_calls,
                response_id,
                refusal,
                annotations,
            });
        }
        return Err(BackendError::EmptyResponse);
    }
    Ok(Completion {
        content,
        usage,
        model,
        tool_calls,
        response_id,
        refusal,
        annotations,
    })
}

fn completion_from_responses_json(payload: &Value) -> Result<Completion, BackendError> {
    let content = extract_responses_content(payload)?;
    if content.trim().is_empty() {
        return Err(BackendError::EmptyResponse);
    }
    Ok(Completion {
        content,
        usage: payload.get("usage").and_then(parse_usage),
        model: payload
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned),
        tool_calls: extract_responses_tool_calls(payload)?,
        response_id: payload.get("id").and_then(Value::as_str).map(str::to_owned),
        refusal: extract_responses_refusal(payload),
        annotations: extract_annotations(payload),
    })
}

fn emit_completion_events(
    completion: &Completion,
    sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
) -> Result<(), BackendError> {
    if let Some(response_id) = completion.response_id.clone() {
        sink(ProviderEvent::ResponseCreated {
            response_id: Some(response_id),
            model: completion.model.clone(),
        })?;
    }
    if !completion.content.is_empty() {
        sink(ProviderEvent::TextDelta {
            delta: completion.content.clone(),
        })?;
        sink(ProviderEvent::TextDone {
            text: completion.content.clone(),
        })?;
    }
    if let Some(refusal) = completion.refusal.clone() {
        sink(ProviderEvent::Refusal { text: refusal })?;
    }
    for call in &completion.tool_calls {
        sink(ProviderEvent::ToolCallDone { call: call.clone() })?;
    }
    if let Some(usage) = completion.usage {
        sink(ProviderEvent::Usage { usage })?;
    }
    sink(ProviderEvent::Completed {
        response_id: completion.response_id.clone(),
        model: completion.model.clone(),
    })
}

fn push_unique_tool_call(calls: &mut Vec<ToolCall>, call: ToolCall) {
    if !calls.iter().any(|existing| existing.id == call.id) {
        calls.push(call);
    }
}

fn extract_chat_tool_calls(payload: &Value) -> Result<Vec<ToolCall>, BackendError> {
    let Some(calls) = payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("tool_calls"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    calls
        .iter()
        .map(|call| {
            let id = call
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| BackendError::InvalidResponse("tool call is missing id".into()))?;
            let function = call.get("function").ok_or_else(|| {
                BackendError::InvalidResponse("tool call is missing function".into())
            })?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    BackendError::InvalidResponse("tool call is missing function name".into())
                })?;
            let arguments = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            parse_tool_call(id, name, arguments)
        })
        .collect()
}

fn extract_responses_tool_calls(payload: &Value) -> Result<Vec<ToolCall>, BackendError> {
    let Some(output) = payload.get("output").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    output
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .map(parse_response_function_call)
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| BackendError::InvalidResponse("response function call is incomplete".into()))
}

fn parse_response_function_call(item: &Value) -> Option<ToolCall> {
    let id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)?;
    let name = item.get("name").and_then(Value::as_str)?;
    let arguments = item
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}");
    parse_tool_call(id, name, arguments).ok()
}

fn parse_tool_call(id: &str, name: &str, arguments: &str) -> Result<ToolCall, BackendError> {
    let arguments = serde_json::from_str(arguments).map_err(|error| {
        BackendError::InvalidResponse(format!("invalid tool call arguments: {error}"))
    })?;
    Ok(ToolCall {
        id: id.to_owned(),
        name: name.to_owned(),
        arguments,
    })
}

fn extract_responses_error(event: &Value) -> String {
    let error = event.get("error").or_else(|| {
        event
            .get("response")
            .and_then(|response| response.get("error"))
    });
    match error {
        Some(Value::String(message)) => message.to_owned(),
        Some(Value::Object(error)) => error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.get("code").and_then(Value::as_str))
            .unwrap_or("Responses stream failed")
            .to_owned(),
        _ => "Responses stream failed".to_owned(),
    }
}

fn parse_usage(value: &Value) -> Option<Usage> {
    let input = value
        .get("prompt_tokens")
        .or_else(|| value.get("input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = value
        .get("completion_tokens")
        .or_else(|| value.get("output_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total = value
        .get("total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(input.saturating_add(output));
    Some(Usage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: total,
    })
}

#[cfg(test)]
mod tests {
    use super::{OpenAiCompatibleBackend, OpenAiWireApi, extract_responses_content};
    use serde_json::json;

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
            "http://192.168.50.168:18080",
            Some("secret".into()),
            "gpt-5",
            OpenAiWireApi::Responses,
        )
        .unwrap();
        assert_eq!(
            codex_proxy.endpoint(),
            "http://192.168.50.168:18080/responses"
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

    #[test]
    fn responses_output_text_variants_are_extracted() {
        assert_eq!(
            extract_responses_content(&json!({"output_text":"top-level"})).unwrap(),
            "top-level"
        );
        assert_eq!(
            extract_responses_content(&json!({
                "output": [
                    {"type":"reasoning","summary":[]},
                    {"type":"message","content":[
                        {"type":"output_text","text":"first"},
                        {"type":"output_text","text":" second"}
                    ]}
                ]
            }))
            .unwrap(),
            "first second"
        );
    }
}
