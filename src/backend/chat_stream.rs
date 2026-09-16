//! Bounded Chat SSE folding. Tool calls become executable only after the
//! complete stream has a valid terminal reason and every argument object parses.
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
};

use super::{
    BackendError, Completion, MAX_RESPONSE_BYTES, ProviderEvent, ToolCall,
    is_recv_body_poll_timeout, parse_tool_call, parse_usage,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_FRAME_BYTES: usize = 256 * 1024;
const MAX_CALLS: usize = 128;
const MAX_REASONING_BYTES: usize = 64 * 1024;
const MAX_REASONING_DETAILS: usize = 128;

/// Visible aliases and opaque replay details have separate lifetimes. Details
/// never become visible deltas, and only a successful terminal publishes history.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Reasoning {
    field: Option<String>,
    text: String,
    details: Vec<Value>,
}

fn reasoning_field(field: &str) -> bool {
    matches!(field, "reasoning_content" | "reasoning" | "reasoning_text")
}

fn validate_detail(detail: &Value) -> Result<(), BackendError> {
    let object = detail
        .as_object()
        .ok_or_else(|| invalid("Chat reasoning detail must be an object"))?;
    for field in ["id", "format", "index"] {
        if let Some(value) = object.get(field) {
            let valid = match field {
                "id" => value.is_null() || value.is_string(),
                "format" => value.is_string(),
                _ => value.is_number(),
            };
            if !valid {
                return Err(invalid("invalid Chat reasoning detail metadata"));
            }
        }
    }
    let valid = match detail["type"].as_str() {
        Some("reasoning.text") => {
            detail["text"].is_string()
                && object
                    .get("signature")
                    .is_none_or(|v| v.is_null() || v.is_string())
        }
        Some("reasoning.summary") => detail["summary"].is_string(),
        Some("reasoning.encrypted") => detail["data"].is_string(),
        _ => false,
    };
    if !valid {
        return Err(invalid("invalid or unsupported Chat reasoning detail"));
    }
    Ok(())
}

impl Reasoning {
    fn validate(&self) -> Result<(), BackendError> {
        if self.field.as_deref().is_some_and(|f| !reasoning_field(f))
            || (self.field.is_some() == self.text.is_empty())
            || self.details.len() > MAX_REASONING_DETAILS
            || serde_json::to_vec(self)
                .map_err(|_| invalid("Chat reasoning encoding"))?
                .len()
                > MAX_REASONING_BYTES
        {
            return Err(invalid("invalid or oversized Chat reasoning state"));
        }
        for detail in &self.details {
            validate_detail(detail)?;
        }
        Ok(())
    }

    pub(super) fn fold(&mut self, delta: &Value) -> Result<Option<String>, BackendError> {
        let mut selected = None;
        // Preserve strict typing even for aliases after the selected one.
        for field in ["reasoning_content", "reasoning", "reasoning_text"] {
            if let Some(text) = string_field(delta, field)?.filter(|s| !s.is_empty())
                && selected.is_none()
            {
                selected = Some((field, text));
            }
        }
        if let Some((field, text)) = selected {
            if self.text.len().saturating_add(text.len()) > MAX_REASONING_BYTES {
                return Err(invalid("Chat reasoning text exceeds bound"));
            }
            self.field.get_or_insert_with(|| field.to_owned());
            self.text.push_str(text);
        }
        if let Some(details) = delta.get("reasoning_details").filter(|v| !v.is_null()) {
            let details = details
                .as_array()
                .ok_or_else(|| invalid("Chat reasoning_details must be an array"))?;
            if details.len() > MAX_REASONING_DETAILS {
                return Err(invalid("too many Chat reasoning details"));
            }
            for detail in details {
                validate_detail(detail)?;
                let key = match detail["type"].as_str() {
                    Some("reasoning.text") => Some("text"),
                    Some("reasoning.summary") => Some("summary"),
                    _ => None,
                };
                if let Some(key) = key
                    && let Some(last) = self
                        .details
                        .last_mut()
                        .filter(|last| last["type"] == detail["type"])
                {
                    let mut text = last[key].as_str().expect("validated detail").to_owned();
                    text.push_str(detail[key].as_str().expect("validated detail"));
                    last[key] = Value::String(text);
                    let fields: &[&str] = if key == "text" {
                        &["id", "format", "index", "signature"]
                    } else {
                        &["id", "format", "index"]
                    };
                    for &field in fields {
                        let missing = last.get(field).is_none_or(|v| {
                            v.is_null()
                                || (matches!(field, "format" | "signature")
                                    && v.as_str() == Some(""))
                        });
                        if missing && let Some(value) = detail.get(field) {
                            last[field] = value.clone();
                        }
                    }
                } else {
                    if self.details.len() >= MAX_REASONING_DETAILS {
                        return Err(invalid("too many Chat reasoning details"));
                    }
                    self.details.push(detail.clone());
                }
                self.validate()?;
            }
        }
        self.validate()?;
        Ok(selected.map(|(_, text)| text.to_owned()))
    }

    pub(super) fn annotate(
        mut self,
        provider: &str,
        model: &str,
        annotations: &mut Vec<Value>,
    ) -> Result<(), BackendError> {
        // Provider annotations cannot supply host-owned replay envelopes.
        if annotations
            .iter()
            .any(|a| a["type"] == "native_history" || a.get("chat_reasoning").is_some())
        {
            return Err(invalid(
                "provider supplied reserved Chat history annotation",
            ));
        }
        if self.text.is_empty() && self.details.is_empty() {
            return Ok(());
        }
        if provider == "opencode-go" && self.field.as_deref() == Some("reasoning") {
            self.field = Some("reasoning_content".into());
        }
        self.validate()?;
        if !self.text.is_empty() {
            annotations.push(json!({"type":"reasoning", "text":self.text}));
        }
        let signature = history_digest(provider, model, &self)?;
        annotations.push(
            json!({"type":"native_history", "wire":"chat_completions", "provider":provider,
            "model":model, "chat_reasoning":self, "signature":signature}),
        );
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReasoningHistory {
    #[serde(rename = "type")]
    kind: String,
    wire: String,
    provider: String,
    model: String,
    chat_reasoning: Reasoning,
    signature: String,
}

fn history_digest(
    provider: &str,
    model: &str,
    reasoning: &Reasoning,
) -> Result<String, BackendError> {
    Ok(format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&("chat_reasoning_v1", provider, model, reasoning))
                .map_err(|_| invalid("Chat reasoning encoding"))?
        )
    ))
}

fn history(
    turn: &crate::core::Turn,
    provider: &str,
    model: &str,
    wire: &str,
) -> Result<Option<Reasoning>, BackendError> {
    let mut found = None;
    for value in turn
        .metadata
        .as_ref()
        .and_then(|m| m["annotations"].as_array())
        .into_iter()
        .flatten()
        .filter(|a| {
            a.get("chat_reasoning").is_some()
                || (a["type"] == "native_history" && a["wire"] == "chat_completions")
        })
    {
        if found.is_some() || turn.role != crate::core::TurnRole::Assistant {
            return Err(invalid("duplicate or misplaced Chat reasoning history"));
        }
        if serde_json::to_vec(value)
            .map_err(|_| invalid("Chat history encoding"))?
            .len()
            > MAX_REASONING_BYTES + 4096
        {
            return Err(invalid("Chat reasoning history exceeds bound"));
        }
        let saved: ReasoningHistory = serde_json::from_value(value.clone())
            .map_err(|_| invalid("invalid Chat reasoning history"))?;
        saved.chat_reasoning.validate()?;
        if saved.kind != "native_history"
            || saved.wire != "chat_completions"
            || wire != saved.wire
            || saved.provider != provider
            || saved.model != model
        {
            return Err(BackendError::Configuration(
                "Chat reasoning history cannot move to another provider/model/wire".into(),
            ));
        }
        if saved.signature != history_digest(provider, model, &saved.chat_reasoning)? {
            return Err(invalid("Chat reasoning history integrity mismatch"));
        }
        found = Some(saved.chat_reasoning);
    }
    Ok(found)
}

pub(super) fn validate_history(
    turn: &crate::core::Turn,
    provider: &str,
    model: &str,
    wire: &str,
) -> Result<(), BackendError> {
    history(turn, provider, model, wire).map(|_| ())
}

pub(super) fn replay(
    turn: &crate::core::Turn,
    provider: &str,
    model: &str,
    message: &mut Value,
) -> Result<(), BackendError> {
    if let Some(saved) = history(turn, provider, model, "chat_completions")? {
        if !saved.details.is_empty() {
            message["reasoning_details"] = Value::Array(saved.details);
        } else if let Some(field) = saved.field {
            message[field] = Value::String(saved.text);
        }
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> BackendError {
    BackendError::InvalidResponse(message.into())
}

#[derive(Default)]
struct PendingCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

#[derive(Default)]
struct Stream {
    completion: Completion,
    calls: BTreeMap<u64, PendingCall>,
    reasoning: Reasoning,
    finish_reason: Option<String>,
    started: bool,
    done: bool,
}

fn string_field<'a>(value: &'a Value, field: &str) -> Result<Option<&'a str>, BackendError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        _ => Err(invalid(format!("Chat {field} must be a string"))),
    }
}

fn bind(slot: &mut Option<String>, value: Option<&str>, label: &str) -> Result<(), BackendError> {
    if let Some(value) = value {
        if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
            return Err(invalid(format!("invalid Chat {label}")));
        }
        if slot.as_deref().is_some_and(|old| old != value) {
            return Err(invalid(format!("Chat {label} changed during stream")));
        }
        *slot = Some(value.to_owned());
    }
    Ok(())
}

// JSON fallback and SSE share completed-call validation before any ready tool.
fn completed_call(
    id: &str,
    name: &str,
    arguments: &str,
    ids: &mut BTreeSet<String>,
) -> Result<ToolCall, BackendError> {
    bind(&mut None, Some(id), "tool call ID")?;
    bind(&mut None, Some(name), "tool function name")?;
    if !ids.insert(id.to_owned()) {
        return Err(invalid("duplicate Chat tool call ID"));
    }
    let call = parse_tool_call(id, name, arguments)?;
    if !call.arguments.is_object() {
        return Err(invalid("Chat tool arguments must be an object"));
    }
    Ok(call)
}

pub(super) fn validate_json(payload: &Value) -> Result<Vec<ToolCall>, BackendError> {
    if payload.get("error").is_some()
        || payload.get("status").is_some_and(|v| v != "completed")
        || payload
            .get("incomplete_details")
            .is_some_and(|v| !v.is_null())
    {
        return Err(invalid("Chat JSON provider error or incomplete status"));
    }
    let choices = payload
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("Chat JSON lacks choices"))?;
    if choices.len() != 1
        || choices[0]
            .get("index")
            .is_some_and(|v| v.as_u64() != Some(0))
    {
        return Err(invalid(
            "Chat JSON must return one choice with index zero when supplied",
        ));
    }
    let choice = &choices[0];
    let message = choice
        .get("message")
        .filter(|v| v.is_object())
        .ok_or_else(|| invalid("Chat JSON message must be an object"))?;
    for field in ["id", "model"] {
        bind(&mut None, string_field(payload, field)?, field)?;
    }
    string_field(message, "refusal")?;
    if payload
        .get("usage")
        .is_some_and(|v| !v.is_null() && !v.is_object())
    {
        return Err(invalid("invalid Chat usage"));
    }
    let mut calls = Vec::new();
    let mut ids = BTreeSet::new();
    if let Some(values) = message.get("tool_calls").filter(|v| !v.is_null()) {
        let values = values
            .as_array()
            .ok_or_else(|| invalid("Chat tool_calls must be an array"))?;
        if values.len() > MAX_CALLS {
            return Err(invalid("Chat tool call limit exceeded"));
        }
        for value in values {
            if string_field(value, "type")?.is_some_and(|v| v != "function") {
                return Err(invalid("unsupported Chat tool call type"));
            }
            let id =
                string_field(value, "id")?.ok_or_else(|| invalid("Chat tool call has no ID"))?;
            let function = value
                .get("function")
                .filter(|v| v.is_object())
                .ok_or_else(|| invalid("Chat function must be an object"))?;
            let name = string_field(function, "name")?
                .ok_or_else(|| invalid("Chat tool call has no name"))?;
            let arguments = string_field(function, "arguments")?
                .ok_or_else(|| invalid("Chat tool call has no arguments"))?;
            calls.push(completed_call(id, name, arguments, &mut ids)?);
        }
    }
    match string_field(choice, "finish_reason")? {
        Some("tool_calls") if !calls.is_empty() => {}
        Some("stop") if calls.is_empty() => {}
        _ => return Err(invalid("Chat JSON missing or incomplete finish_reason")),
    }
    Ok(calls)
}

impl Stream {
    fn event(
        &mut self,
        data: &[u8],
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<(), BackendError> {
        if data.is_empty() {
            return Ok(());
        }
        if self.done {
            return Err(invalid("Chat data after [DONE]"));
        }
        if data == b"[DONE]" {
            if self.finish_reason.is_none() {
                return Err(invalid("Chat [DONE] without finish_reason"));
            }
            self.done = true;
            return Ok(());
        }
        let value: Value = serde_json::from_slice(data)
            .map_err(|e| invalid(format!("invalid Chat SSE JSON: {e}")))?;
        if value.get("error").is_some() {
            return Err(invalid("Chat provider returned an error event"));
        }
        bind(
            &mut self.completion.response_id,
            string_field(&value, "id")?,
            "response ID",
        )?;
        bind(
            &mut self.completion.model,
            string_field(&value, "model")?,
            "model",
        )?;
        if !self.started {
            self.started = true;
            sink(ProviderEvent::ResponseCreated {
                response_id: self.completion.response_id.clone(),
                model: self.completion.model.clone(),
            })?;
        }
        if let Some(usage) = value.get("usage").filter(|v| !v.is_null()) {
            if !usage.is_object() {
                return Err(invalid("invalid Chat usage"));
            }
            self.completion.usage = parse_usage(usage);
        }
        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("Chat chunk lacks choices"))?;
        if choices.is_empty() {
            return Ok(());
        }
        if choices.len() != 1 || choices[0].get("index").and_then(Value::as_u64) != Some(0) {
            return Err(invalid("Chat must return one choice with index zero"));
        }
        if self.finish_reason.is_some() {
            return Err(invalid("Chat choice after finish_reason"));
        }
        let choice = &choices[0];
        let delta = choice
            .get("delta")
            .ok_or_else(|| invalid("Chat choice lacks delta"))?;
        if !delta.is_object() {
            return Err(invalid("Chat delta must be an object"));
        }
        if let Some(content) = string_field(delta, "content")?.filter(|s| !s.is_empty()) {
            self.completion.content.push_str(content);
            sink(ProviderEvent::TextDelta {
                delta: content.to_owned(),
            })?;
        }
        // Compatible reasoning fields remain separate from final answer text.
        if let Some(text) = self.reasoning.fold(delta)? {
            sink(ProviderEvent::ReasoningDelta { delta: text })?;
        }
        if let Some(text) = string_field(delta, "refusal")?.filter(|s| !s.is_empty()) {
            self.completion
                .refusal
                .get_or_insert_default()
                .push_str(text);
            sink(ProviderEvent::Refusal {
                text: text.to_owned(),
            })?;
        }
        if let Some(calls) = delta.get("tool_calls").filter(|v| !v.is_null()) {
            let calls = calls
                .as_array()
                .ok_or_else(|| invalid("Chat tool_calls must be an array"))?;
            for fragment in calls {
                let index = fragment
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid("Chat tool call lacks index"))?;
                if index >= MAX_CALLS as u64
                    || (!self.calls.contains_key(&index) && self.calls.len() >= MAX_CALLS)
                {
                    return Err(invalid("Chat tool call limit exceeded"));
                }
                if string_field(fragment, "type")?.is_some_and(|kind| kind != "function") {
                    return Err(invalid("unsupported Chat tool call type"));
                }
                let call = self.calls.entry(index).or_default();
                bind(&mut call.id, string_field(fragment, "id")?, "tool call ID")?;
                let mut arguments_delta = "";
                if let Some(function) = fragment.get("function") {
                    if !function.is_object() {
                        return Err(invalid("invalid Chat function delta"));
                    }
                    bind(
                        &mut call.name,
                        string_field(function, "name")?,
                        "tool function name",
                    )?;
                    arguments_delta = string_field(function, "arguments")?.unwrap_or_default();
                    call.arguments.push_str(arguments_delta);
                }
                sink(ProviderEvent::ToolCallDelta {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments_delta: arguments_delta.to_owned(),
                })?;
            }
        }
        if let Some(reason) = string_field(choice, "finish_reason")? {
            match reason {
                "stop" | "tool_calls" => self.finish_reason = Some(reason.to_owned()),
                "length" => return Err(invalid("Chat output truncated (finish_reason=length)")),
                "content_filter" => {
                    return Err(invalid(
                        "Chat output filtered (finish_reason=content_filter)",
                    ));
                }
                _ => return Err(invalid(format!("unsupported Chat finish_reason {reason}"))),
            }
        }
        Ok(())
    }

    fn finish(
        mut self,
        provider: &str,
        model: &str,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        let reason = self
            .finish_reason
            .as_deref()
            .ok_or_else(|| invalid("Chat stream ended without finish_reason"))?;
        if (reason == "tool_calls") == self.calls.is_empty() {
            return Err(invalid("Chat finish_reason does not match tool calls"));
        }
        let mut ids = BTreeSet::new();
        for (_, pending) in self.calls {
            let id = pending
                .id
                .ok_or_else(|| invalid("Chat tool call has no ID"))?;
            let name = pending
                .name
                .ok_or_else(|| invalid("Chat tool call has no name"))?;
            self.completion.tool_calls.push(completed_call(
                &id,
                &name,
                &pending.arguments,
                &mut ids,
            )?);
        }
        if self.completion.content.trim().is_empty()
            && self.completion.tool_calls.is_empty()
            && self.completion.refusal.is_none()
        {
            return Err(BackendError::EmptyResponse);
        }
        self.completion
            .annotations
            .push(json!({"type":"chat_finish_reason", "finish_reason":reason}));
        self.reasoning
            .annotate(provider, model, &mut self.completion.annotations)?;
        if !self.completion.content.is_empty() {
            sink(ProviderEvent::TextDone {
                text: self.completion.content.clone(),
            })?;
        }
        for call in &self.completion.tool_calls {
            sink(ProviderEvent::ToolCallDone { call: call.clone() })?;
        }
        if let Some(usage) = self.completion.usage {
            sink(ProviderEvent::Usage { usage })?;
        }
        sink(ProviderEvent::Completed {
            response_id: self.completion.response_id.clone(),
            model: self.completion.model.clone(),
        })?;
        Ok(self.completion)
    }
}

pub(super) fn read(
    body: &mut ureq::Body,
    provider: &str,
    model: &str,
    cancelled: &dyn Fn() -> bool,
    sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
) -> Result<Completion, BackendError> {
    // A sink may request cancellation while a single frame or the terminal
    // batch is being published. Do not expose later deltas or ready tools.
    let sink = &mut |event| {
        if cancelled() {
            return Err(BackendError::Cancelled);
        }
        sink(event)
    };
    let mut reader = body.as_reader();
    let mut state = Stream::default();
    let mut line = Vec::new();
    let mut data = Vec::new();
    let mut chunk = [0_u8; 16 * 1024];
    let mut total = 0usize;
    let mut frame = 0usize;
    loop {
        if cancelled() {
            return Err(BackendError::Cancelled);
        }
        let read = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if is_recv_body_poll_timeout(&e) => continue,
            // Once an event is observable a retry would duplicate deltas.
            Err(e) if state.started => {
                return Err(invalid(format!("Chat stream interrupted: {e}")));
            }
            Err(e) => return Err(BackendError::Transport(e.to_string())),
        };
        total += read;
        if total > MAX_RESPONSE_BYTES {
            return Err(invalid("Chat stream exceeds response byte limit"));
        }
        for byte in &chunk[..read] {
            frame += 1;
            if frame > MAX_FRAME_BYTES {
                return Err(invalid("Chat SSE frame too large"));
            }
            if *byte != b'\n' {
                line.push(*byte);
                continue;
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                if data.last() == Some(&b'\n') {
                    data.pop();
                }
                if cancelled() {
                    return Err(BackendError::Cancelled);
                }
                state.event(&data, sink)?;
                data.clear();
                frame = 0;
            } else if let Some(value) = line.strip_prefix(b"data:") {
                data.extend_from_slice(value.strip_prefix(b" ").unwrap_or(value));
                data.push(b'\n');
            }
            line.clear();
        }
        if state.done {
            break;
        }
    }
    if cancelled() {
        return Err(BackendError::Cancelled);
    }
    if !line.is_empty() || !data.is_empty() {
        return Err(invalid("Chat stream ended in a partial SSE frame"));
    }
    state.finish(provider, model, sink)
}
