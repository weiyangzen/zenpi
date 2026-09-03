//! Backend boundary for zenpi.
//!
//! The core deliberately knows nothing about HTTP, model SDKs, or streaming
//! transports.  A backend receives an immutable view of the session and
//! returns one normalized completion.  This keeps the headless and TUI modes
//! behaviorally identical and makes a deterministic backend useful in tests.

use std::{env, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::core::{Turn, TurnRole};

/// Keep provider responses bounded even when an endpoint omits a content
/// length.  The core applies the smaller per-turn text limit afterwards.
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// A normalized request independent of the provider protocol.
#[derive(Debug)]
pub struct CompletionRequest<'a> {
    pub turn_id: &'a str,
    pub turns: &'a [Turn],
    pub model: Option<&'a str>,
}

/// Optional usage information returned by a provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Provider-independent completion result.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Completion {
    pub content: String,
    pub usage: Option<Usage>,
    pub model: Option<String>,
}

impl Completion {
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            usage: None,
            model: None,
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
            | Self::Cancelled => false,
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
        }
    }
}

/// Synchronous provider contract.  A synchronous boundary is intentional:
/// the default binary stays tiny and callers that need concurrency can run
/// independent agents in their own processes.
pub trait Backend: Send + Sync {
    fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, BackendError>;

    fn name(&self) -> &str {
        "backend"
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

/// A minimal OpenAI-compatible chat-completions backend.
///
/// It intentionally implements the non-streaming endpoint only.  Streaming
/// remains a UI concern and can be added behind this same trait without
/// changing session or protocol ownership.  `ureq` is blocking and connection
/// pooling keeps the steady-state process smaller than an async runtime.
pub struct OpenAiCompatibleBackend {
    client: ureq::Agent,
    endpoint: String,
    api_key: Option<String>,
    model: String,
}

impl std::fmt::Debug for OpenAiCompatibleBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiCompatibleBackend")
            .field("endpoint", &self.endpoint)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("model", &self.model)
            .finish()
    }
}

impl OpenAiCompatibleBackend {
    pub fn new(
        endpoint: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
    ) -> Result<Self, BackendError> {
        let endpoint = normalize_endpoint(endpoint.into())?;
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
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .build();
        Ok(Self {
            client: ureq::Agent::new_with_config(config),
            endpoint,
            api_key,
            model,
        })
    }

    pub fn from_env() -> Result<Self, BackendError> {
        let endpoint = env::var("ZENPI_BASE_URL")
            .or_else(|_| env::var("OPENAI_BASE_URL"))
            .unwrap_or_else(|_| "https://api.openai.com/v1/chat/completions".into());
        let api_key = env::var("ZENPI_API_KEY")
            .or_else(|_| env::var("OPENAI_API_KEY"))
            .ok();
        let model = env::var("ZENPI_MODEL")
            .or_else(|_| env::var("OPENAI_MODEL"))
            .unwrap_or_else(|_| "gpt-4o-mini".into());
        Self::from_values(endpoint, api_key, model)
    }

    fn from_values(
        endpoint: String,
        api_key: Option<String>,
        model: String,
    ) -> Result<Self, BackendError> {
        let normalized_endpoint = normalize_endpoint(endpoint)?;
        if is_openai_endpoint(&normalized_endpoint) && api_key.is_none() {
            return Err(BackendError::Configuration(
                "ZENPI_API_KEY or OPENAI_API_KEY is required for api.openai.com".into(),
            ));
        }
        Self::new(normalized_endpoint, api_key, model)
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

fn normalize_endpoint(mut endpoint: String) -> Result<String, BackendError> {
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
    if !endpoint.ends_with("/chat/completions") {
        endpoint.push_str("/chat/completions");
    }
    Ok(endpoint)
}

fn is_openai_endpoint(endpoint: &str) -> bool {
    endpoint
        .strip_prefix("https://")
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        == Some("api.openai.com")
}

impl Backend for OpenAiCompatibleBackend {
    fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        let model = request.model.unwrap_or(&self.model);
        if model.trim().is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
            return Err(BackendError::Configuration(
                "model must be non-empty and at most 256 bytes".into(),
            ));
        }
        let messages: Vec<Value> = request
            .turns
            .iter()
            .map(|turn| {
                let role = match turn.role {
                    TurnRole::System => "system",
                    TurnRole::User => "user",
                    TurnRole::Assistant => "assistant",
                    TurnRole::Tool => "tool",
                };
                json!({ "role": role, "content": turn.content })
            })
            .collect();
        let body = json!({
            "model": model,
            "messages": messages,
            "stream": false,
        });
        let mut request_builder = self
            .client
            .post(&self.endpoint)
            .header("content-type", "application/json");
        if let Some(key) = &self.api_key {
            request_builder = request_builder.header("authorization", format!("Bearer {key}"));
        }
        let mut response = request_builder.send_json(&body).map_err(map_ureq_error)?;
        let payload: Value = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES as u64)
            .read_json()
            .map_err(map_ureq_error)?;
        let content = extract_content(&payload)?;
        if content.trim().is_empty() {
            return Err(BackendError::EmptyResponse);
        }
        let usage = payload.get("usage").and_then(parse_usage);
        Ok(Completion {
            content,
            usage,
            model: payload
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    fn name(&self) -> &str {
        "openai-compatible"
    }
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
    use super::OpenAiCompatibleBackend;

    #[test]
    fn openai_host_requires_credentials_after_endpoint_normalization() {
        let error = OpenAiCompatibleBackend::from_values(
            "https://api.openai.com".into(),
            None,
            "gpt-4o-mini".into(),
        )
        .expect_err("the normalized OpenAI host must require an API key");
        assert!(error.to_string().contains("API_KEY"));

        let query_error = OpenAiCompatibleBackend::from_values(
            "https://api.openai.com?tenant=default".into(),
            None,
            "gpt-4o-mini".into(),
        )
        .expect_err("query-bearing endpoints must be rejected before any request");
        assert!(query_error.to_string().contains("endpoint"));
    }
}
