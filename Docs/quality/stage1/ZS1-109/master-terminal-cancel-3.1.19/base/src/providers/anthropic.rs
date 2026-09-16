//! Native Messages wire. Opaque thinking stays in ordered assistant blocks;
//! no tool is executable until the entire response has a valid terminal state.
use crate::{
    backend::{
        BackendError, Completion, CompletionRequest, MAX_RESPONSE_BYTES, ProviderCapabilities,
        ProviderEvent, Usage,
    },
    core::{Turn, TurnRole},
    tools::ToolCall,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
};

pub const CAPABILITIES: ProviderCapabilities = ProviderCapabilities {
    text: true,
    images: true,
    files: false,
    tools: true,
    structured_output: false,
    streaming: true,
    reasoning: true,
};
const WIRE: &str = "anthropic_messages";
const MAX_BLOCKS: usize = 128;
const MAX_FRAME: usize = 256 * 1024;
const MAX_HISTORY: usize = 256 * 1024;
fn invalid(s: impl Into<String>) -> BackendError {
    BackendError::InvalidResponse(s.into())
}
fn config(s: impl Into<String>) -> BackendError {
    BackendError::Configuration(s.into())
}
fn string<'a>(v: &'a Value, k: &str) -> Result<&'a str, BackendError> {
    v.get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("Anthropic missing string {k}")))
}
fn identity(v: &Value, k: &str) -> Result<String, BackendError> {
    let s = string(v, k)?;
    if s.is_empty() || s.len() > 256 || s.chars().any(char::is_control) {
        return Err(invalid(format!("invalid Anthropic {k}")));
    }
    Ok(s.into())
}
fn call(block: &Value) -> Result<ToolCall, BackendError> {
    let id = identity(block, "id")?;
    let name = identity(block, "name")?;
    if id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(invalid("invalid Anthropic tool ID"));
    }
    let arguments = block
        .get("input")
        .filter(|v| v.is_object())
        .ok_or_else(|| invalid("Anthropic tool input must be an object"))?
        .clone();
    let c = ToolCall {
        id,
        name,
        arguments,
    };
    if c.name.len() > 64
        || !c
            .name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(invalid("invalid tool name"));
    }
    Ok(c)
}

/// Runtime effort is native adaptive effort only for the frozen supported
/// model. Unknown models still work as conservative text-only registry entries.
pub(crate) fn validate_effort(model: &str, effort: Option<&str>) -> Result<(), BackendError> {
    if let Some(e) = effort
        && (model != "claude-sonnet-4-6"
            || !matches!(e, "none" | "low" | "medium" | "high" | "max"))
    {
        return Err(config("unsupported Anthropic model/effort combination"));
    }
    Ok(())
}

pub(crate) fn validate_history_wire(turns: &[Turn], wire: &str) -> Result<(), BackendError> {
    for turn in turns {
        for a in turn
            .metadata
            .as_ref()
            .and_then(|m| m.get("annotations"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if a["type"] == "native_history" && a["wire"] != wire {
                return Err(config(
                    "native signed history cannot be represented by the selected wire",
                ));
            }
        }
    }
    Ok(())
}
fn history_calls(turn: &Turn) -> Result<Vec<ToolCall>, BackendError> {
    let Some(v) = turn.metadata.as_ref().and_then(|m| m.get("tool_calls")) else {
        return Ok(vec![]);
    };
    let arr = v
        .as_array()
        .ok_or_else(|| config("invalid saved tool calls"))?;
    arr.iter()
        .map(|v| {
            let c = ToolCall {
                id: identity(v, "id")?,
                name: identity(v, "name")?,
                arguments: v
                    .get("arguments")
                    .cloned()
                    .ok_or_else(|| config("saved call arguments missing"))?,
            };
            if !c.arguments.is_object() {
                return Err(config("saved tool input must be object"));
            }
            Ok(c)
        })
        .collect()
}
fn validate_blocks(blocks: &[Value]) -> Result<(String, Vec<ToolCall>), BackendError> {
    if blocks.len() > MAX_BLOCKS
        || serde_json::to_vec(blocks)
            .map_err(|_| invalid("block encoding failed"))?
            .len()
            > MAX_HISTORY
    {
        return Err(invalid("Anthropic native history exceeds bound"));
    }
    let mut text = String::new();
    let mut calls = vec![];
    let mut ids = BTreeSet::new();
    for b in blocks {
        let kind = string(b, "type")?;
        let allowed: &[&str] = match kind {
            "text" => &["type", "text"],
            "thinking" => &["type", "thinking", "signature"],
            "redacted_thinking" => &["type", "data"],
            "tool_use" => &["type", "id", "name", "input"],
            _ => return Err(invalid("unsupported Anthropic content block")),
        };
        if b.as_object()
            .is_none_or(|o| o.keys().any(|k| !allowed.contains(&k.as_str())))
        {
            return Err(invalid("unsupported Anthropic block field"));
        }
        match kind {
            "text" => text.push_str(string(b, "text")?),
            "thinking" => {
                string(b, "thinking")?;
                if string(b, "signature")?.trim().is_empty() {
                    return Err(invalid("Anthropic thinking signature missing"));
                }
            }
            "redacted_thinking" => {
                if string(b, "data")?.trim().is_empty() {
                    return Err(invalid("Anthropic redacted thinking data missing"));
                }
            }
            "tool_use" => {
                let c = call(b)?;
                if !ids.insert(c.id.clone()) {
                    return Err(invalid("duplicate Anthropic tool ID"));
                }
                calls.push(c);
            }
            _ => return Err(invalid("unsupported Anthropic content block")),
        }
    }
    Ok((text, calls))
}
fn assistant_blocks(turn: &Turn, provider: &str, model: &str) -> Result<Vec<Value>, BackendError> {
    let calls = history_calls(turn)?;
    let annotations = turn
        .metadata
        .as_ref()
        .and_then(|m| m.get("annotations"))
        .and_then(Value::as_array);
    let native = annotations
        .into_iter()
        .flatten()
        .filter(|a| a["type"] == "native_history")
        .collect::<Vec<_>>();
    if native.len() > 1 {
        return Err(config("duplicate native history"));
    }
    if let Some(a) = native.first() {
        if a["wire"] != WIRE || a["provider"] != provider {
            return Err(config("native history provider/wire mismatch"));
        }
        let blocks = a["blocks"]
            .as_array()
            .ok_or_else(|| config("native history blocks missing"))?;
        let (text, saved_calls) = validate_blocks(blocks)?;
        if text != turn.content || calls != saved_calls {
            return Err(config(
                "native history disagrees with saved text or tool calls",
            ));
        }
        if blocks
            .iter()
            .any(|b| matches!(b["type"].as_str(), Some("thinking" | "redacted_thinking")))
            && a["model"] != model
        {
            return Err(config("signed Anthropic history belongs to another model"));
        }
        return Ok(blocks.clone());
    }
    if annotations
        .into_iter()
        .flatten()
        .any(|a| a["type"] == "reasoning")
    {
        return Err(config(
            "unsigned reasoning history cannot be converted to Anthropic thinking",
        ));
    }
    let mut blocks = vec![];
    if !turn.content.is_empty() {
        blocks.push(json!({"type":"text","text":turn.content}));
    }
    for c in calls {
        blocks.push(json!({"type":"tool_use","id":c.id,"name":c.name,"input":c.arguments}));
    }
    validate_blocks(&blocks)?;
    Ok(blocks)
}

pub(crate) fn request_body(
    request: &CompletionRequest<'_>,
    model: &str,
    effort: Option<&str>,
    provider: &str,
) -> Result<Value, BackendError> {
    validate_effort(model, effort)?;
    if request.attachments.len() > crate::backend::MAX_ATTACHMENTS_PER_TURN
        || request
            .attachments
            .iter()
            .filter_map(|a| a.data.as_ref())
            .map(Vec::len)
            .sum::<usize>()
            > crate::backend::MAX_TOTAL_ATTACHMENT_BYTES
    {
        return Err(config("Anthropic attachments exceed bound"));
    }
    if request.response_format.is_some() {
        return Err(config(
            "Anthropic structured output is not enabled in this adapter",
        ));
    }
    let mut system = vec![];
    if let Some(s) = request.instructions
        && !s.is_empty()
    {
        system.push(json!({"type":"text","text":s}));
    }
    let mut messages: Vec<Value> = vec![];
    let mut pending = BTreeSet::new();
    let mut seen_ids = BTreeSet::new();
    for turn in request.turns {
        let mut blocks = vec![];
        let role = match turn.role {
            TurnRole::System => {
                if !messages.is_empty() {
                    return Err(config(
                        "mid-conversation system message unsupported by Anthropic",
                    ));
                }
                if !turn.content.is_empty() {
                    system.push(json!({"type":"text","text":turn.content}));
                }
                continue;
            }
            TurnRole::User => {
                if !pending.is_empty() {
                    return Err(config(
                        "Anthropic tool results are missing before next user message",
                    ));
                }
                if !turn.content.is_empty() {
                    blocks.push(json!({"type":"text","text":turn.content}));
                }
                for a in request.attachments.iter().filter(|a| a.turn_id == turn.id) {
                    a.input.validate()?;
                    if a.input.kind != crate::backend::AttachmentKind::Image
                        || !matches!(
                            a.input.mime_type.as_str(),
                            "image/png" | "image/jpeg" | "image/gif" | "image/webp"
                        )
                        || a.input.file_id.is_some()
                    {
                        return Err(config("unsupported Anthropic attachment type"));
                    }
                    let source = if let Some(data) = &a.data {
                        if data.is_empty() || data.len() > crate::backend::MAX_ATTACHMENT_BYTES {
                            return Err(config("Anthropic image size invalid"));
                        }
                        json!({"type":"base64","media_type":a.input.mime_type,"data":STANDARD.encode(data)})
                    } else if let Some(url) = &a.input.url {
                        json!({"type":"url","url":url})
                    } else {
                        return Err(config("Anthropic image bytes unavailable"));
                    };
                    blocks.push(json!({"type":"image","source":source}));
                }
                "user"
            }
            TurnRole::Assistant => {
                if !pending.is_empty() {
                    return Err(config(
                        "Anthropic tool results missing before assistant continuation",
                    ));
                }
                blocks = assistant_blocks(turn, provider, model)?;
                for b in &blocks {
                    if b["type"] == "tool_use" {
                        let id = identity(b, "id")?;
                        if !seen_ids.insert(id.clone()) {
                            return Err(config("duplicate tool ID in history"));
                        }
                        pending.insert(id);
                    }
                }
                "assistant"
            }
            TurnRole::Tool => {
                let id = turn
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("tool_call_id"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| config("orphan tool result"))?;
                if !pending.remove(id) {
                    return Err(config("unexpected or duplicate Anthropic tool result"));
                }
                let is_error = turn
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("outcome"))
                    .and_then(Value::as_str)
                    .is_some_and(|v| v != "succeeded");
                blocks.push(json!({"type":"tool_result","tool_use_id":id,"content":turn.content,"is_error":is_error}));
                "user"
            }
        };
        if blocks.is_empty() {
            return Err(config("empty Anthropic message"));
        }
        if messages.last().is_some_and(|m| m["role"] == role) {
            messages.last_mut().expect("last message")["content"]
                .as_array_mut()
                .expect("array")
                .extend(blocks);
        } else {
            messages.push(json!({"role":role,"content":blocks}));
        }
    }
    if !pending.is_empty()
        || messages.first().is_none_or(|m| m["role"] != "user")
        || messages.last().is_none_or(|m| m["role"] != "user")
    {
        return Err(config(
            "Anthropic requires a complete history ending in user input or tool results",
        ));
    }
    let max = request
        .max_output_tokens
        .unwrap_or(crate::context::DEFAULT_RESERVED_OUTPUT_TOKENS);
    if max == 0 {
        return Err(config("Anthropic max_tokens must be positive"));
    }
    let mut body = json!({"model":model,"messages":messages,"stream":true,"max_tokens":max});
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if !request.tools.is_empty() {
        body["tools"]=json!(request.tools.iter().map(|t|json!({"name":t.name,"description":t.description,"input_schema":t.input_schema})).collect::<Vec<_>>());
    }
    if let Some(e) = effort {
        if e == "none" {
            body["thinking"] = json!({"type":"disabled"});
        } else {
            body["thinking"] = json!({"type":"adaptive","display":"summarized"});
            body["output_config"] = json!({"effort":e});
        }
    }
    if let Some(m) = request.metadata {
        let obj = m
            .as_object()
            .ok_or_else(|| config("Anthropic metadata must be object"))?;
        if obj.keys().any(|k| k != "user_id" && k != "purpose") {
            return Err(config("unsupported Anthropic metadata field"));
        }
        if obj
            .get("purpose")
            .is_some_and(|value| value != "semantic_compaction")
        {
            return Err(config("unsupported internal request purpose"));
        }
        // This owner marker is local routing context, not provider metadata.
        if obj.contains_key("user_id") {
            let id = string(m, "user_id")?;
            if id.len() > 256 {
                return Err(config("Anthropic user_id too long"));
            }
            body["metadata"] = json!({"user_id":id});
        }
    }
    if serde_json::to_vec(&body)
        .map_err(|_| config("request encoding failed"))?
        .len()
        > 32 * 1024 * 1024
    {
        return Err(config("Anthropic request exceeds bound"));
    }
    Ok(body)
}

#[derive(Default)]
struct Block {
    value: Value,
    partial: String,
    closed: bool,
}
#[derive(Default)]
struct Fold {
    blocks: BTreeMap<u64, Block>,
    id: Option<String>,
    model: Option<String>,
    stop: Option<String>,
    done: bool,
    usage: [u64; 4],
    saw_usage: bool,
}
impl Fold {
    fn usage(&mut self, v: &Value) -> Result<(), BackendError> {
        if v.is_null() {
            return Ok(());
        }
        if !v.is_object() {
            return Err(invalid("Anthropic usage must be object"));
        }
        for (i, k) in [
            "input_tokens",
            "output_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
        ]
        .iter()
        .enumerate()
        {
            if let Some(x) = v.get(k).filter(|v| !v.is_null()) {
                let n = x
                    .as_u64()
                    .ok_or_else(|| invalid("invalid Anthropic usage counter"))?;
                if n < self.usage[i] {
                    return Err(invalid("Anthropic cumulative usage decreased"));
                }
                self.usage[i] = n;
                self.saw_usage = true;
            }
        }
        Ok(())
    }
    fn event(
        &mut self,
        v: Value,
        expected_model: &str,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<(), BackendError> {
        let kind = string(&v, "type")?;
        if kind == "ping" {
            return Ok(());
        }
        if self.done {
            return Err(invalid("Anthropic data after message_stop"));
        }
        if kind == "error" {
            return Err(invalid("Anthropic error event"));
        }
        if kind == "message_start" {
            if self.id.is_some() {
                return Err(invalid("duplicate Anthropic message_start"));
            }
            let m = &v["message"];
            if m["role"] != "assistant"
                || m["type"] != "message"
                || m["content"].as_array().is_none_or(|a| !a.is_empty())
                || !m["stop_reason"].is_null()
            {
                return Err(invalid("invalid Anthropic message_start"));
            }
            let model = identity(m, "model")?;
            if model != expected_model {
                return Err(invalid("unexpected Anthropic serving model"));
            }
            self.id = Some(identity(m, "id")?);
            self.model = Some(model);
            self.usage(&m["usage"])?;
            sink(ProviderEvent::ResponseCreated {
                response_id: self.id.clone(),
                model: self.model.clone(),
            })?;
            return Ok(());
        }
        if self.id.is_none() {
            return Err(invalid("Anthropic event before message_start"));
        }
        match kind {
            "content_block_start" => {
                if self.stop.is_some() {
                    return Err(invalid("block after stop reason"));
                }
                let index = v["index"]
                    .as_u64()
                    .ok_or_else(|| invalid("invalid block index"))?;
                if index != self.blocks.len() as u64 || self.blocks.len() >= MAX_BLOCKS {
                    return Err(invalid("noncontiguous or excessive Anthropic block index"));
                }
                let b = v["content_block"].clone();
                match string(&b, "type")? {
                    "text" => {
                        let t = string(&b, "text")?;
                        if !t.is_empty() {
                            sink(ProviderEvent::TextDelta { delta: t.into() })?;
                        }
                    }
                    "thinking" => {
                        let t = string(&b, "thinking")?;
                        string(&b, "signature")?;
                        if !t.is_empty() {
                            sink(ProviderEvent::ReasoningDelta { delta: t.into() })?;
                        }
                    }
                    "redacted_thinking" => {
                        string(&b, "data")?;
                    }
                    "tool_use" => {
                        let c = call(&b)?;
                        sink(ProviderEvent::ToolCallDelta {
                            call_id: Some(c.id),
                            name: Some(c.name),
                            arguments_delta: String::new(),
                        })?;
                    }
                    _ => return Err(invalid("unsupported Anthropic content block")),
                }
                self.blocks.insert(
                    index,
                    Block {
                        value: b,
                        ..Block::default()
                    },
                );
            }
            "content_block_delta" | "content_block_stop" => {
                if self.stop.is_some() {
                    return Err(invalid("block event after stop reason"));
                }
                let index = v["index"]
                    .as_u64()
                    .ok_or_else(|| invalid("invalid block index"))?;
                let b = self
                    .blocks
                    .get_mut(&index)
                    .filter(|b| !b.closed)
                    .ok_or_else(|| invalid("event for missing/closed Anthropic block"))?;
                if kind == "content_block_stop" {
                    if !b.partial.is_empty() {
                        b.value["input"] = serde_json::from_str(&b.partial)
                            .map_err(|_| invalid("malformed Anthropic tool input JSON"))?;
                    }
                    validate_blocks(std::slice::from_ref(&b.value))?;
                    b.closed = true;
                    b.partial.clear();
                } else {
                    let d = &v["delta"];
                    let (field, key) = match (b.value["type"].as_str(), string(d, "type")?) {
                        (Some("text"), "text_delta") => ("text", "text"),
                        (Some("thinking"), "thinking_delta") => ("thinking", "thinking"),
                        (Some("thinking"), "signature_delta") => ("signature", "signature"),
                        (Some("tool_use"), "input_json_delta") => ("input", "partial_json"),
                        _ => return Err(invalid("Anthropic delta type does not match block")),
                    };
                    let t = string(d, key)?;
                    if field == "input" {
                        if b.value["input"].as_object().is_none_or(|o| !o.is_empty()) {
                            return Err(invalid("tool input delta conflicts with initial input"));
                        }
                        b.partial.push_str(t);
                        if b.partial.len() > MAX_FRAME {
                            return Err(invalid("Anthropic tool arguments exceed bound"));
                        }
                        sink(ProviderEvent::ToolCallDelta {
                            call_id: Some(identity(&b.value, "id")?),
                            name: Some(identity(&b.value, "name")?),
                            arguments_delta: t.into(),
                        })?;
                    } else {
                        let mut text = string(&b.value, field)?.to_owned();
                        text.push_str(t);
                        if text.len() > MAX_HISTORY {
                            return Err(invalid("Anthropic block exceeds bound"));
                        }
                        b.value[field] = json!(text);
                        if field == "text" {
                            sink(ProviderEvent::TextDelta { delta: t.into() })?;
                        } else if field == "thinking" {
                            sink(ProviderEvent::ReasoningDelta { delta: t.into() })?;
                        }
                    }
                }
            }
            "message_delta" => {
                if self.blocks.values().any(|b| !b.closed) {
                    return Err(invalid("Anthropic message_delta before block completion"));
                }
                if let Some(reason) = v["delta"].get("stop_reason").filter(|v| !v.is_null()) {
                    if self.stop.is_some() {
                        return Err(invalid("duplicate Anthropic stop reason"));
                    }
                    let reason = reason
                        .as_str()
                        .ok_or_else(|| invalid("invalid Anthropic stop reason"))?;
                    if !matches!(
                        reason,
                        "end_turn" | "tool_use" | "stop_sequence" | "refusal"
                    ) {
                        return Err(invalid(format!(
                            "incomplete/unsupported Anthropic stop reason {reason}"
                        )));
                    }
                    self.stop = Some(reason.into());
                }
                self.usage(&v["usage"])?;
            }
            "message_stop" => {
                if self.stop.is_none() || self.blocks.values().any(|b| !b.closed) {
                    return Err(invalid(
                        "Anthropic message_stop before complete terminal state",
                    ));
                }
                self.done = true;
            }
            _ => return Err(invalid("unsupported Anthropic stream event")),
        }
        Ok(())
    }
    fn finish(
        self,
        provider: &str,
        caps: ProviderCapabilities,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        if !self.done {
            return Err(invalid("Anthropic stream missing message_stop"));
        }
        let mut blocks = self
            .blocks
            .into_values()
            .map(|b| b.value)
            .collect::<Vec<_>>();
        let (mut content, tool_calls) = validate_blocks(&blocks)?;
        let stop = self
            .stop
            .as_deref()
            .ok_or_else(|| invalid("missing Anthropic stop reason"))?;
        if (stop == "tool_use") == tool_calls.is_empty() {
            return Err(invalid("Anthropic stop reason does not match tool calls"));
        }
        if (!caps.tools && !tool_calls.is_empty())
            || (!caps.reasoning
                && blocks
                    .iter()
                    .any(|b| matches!(b["type"].as_str(), Some("thinking" | "redacted_thinking"))))
        {
            return Err(invalid("Anthropic response exceeds model capabilities"));
        }
        let refusal = (stop == "refusal").then(|| {
            if content.is_empty() {
                "Anthropic refused the request".into()
            } else {
                content.clone()
            }
        });
        // Normalize the provider's empty refusal once so the durable visible
        // text and replay blocks stay consistent on a later user turn.
        if content.is_empty()
            && let Some(text) = &refusal
        {
            content = text.clone();
            blocks.push(json!({"type":"text","text":text}));
        }
        if content.trim().is_empty() && tool_calls.is_empty() && refusal.is_none() {
            return Err(BackendError::EmptyResponse);
        }
        let input = self.usage[0]
            .checked_add(self.usage[2])
            .and_then(|n| n.checked_add(self.usage[3]))
            .ok_or_else(|| invalid("usage overflow"))?;
        let total = input
            .checked_add(self.usage[1])
            .ok_or_else(|| invalid("usage overflow"))?;
        let usage = self.saw_usage.then_some(Usage {
            input_tokens: input,
            output_tokens: self.usage[1],
            total_tokens: total,
        });
        let completion = Completion {
            content,
            tool_calls,
            usage,
            response_id: self.id,
            model: self.model.clone(),
            refusal,
            annotations: vec![
                json!({"type":"native_history","wire":WIRE,"provider":provider,"model":self.model,"blocks":blocks}),
                json!({"type":"anthropic_stop_reason","stop_reason":stop}),
            ],
        };
        if !completion.content.is_empty() {
            sink(ProviderEvent::TextDone {
                text: completion.content.clone(),
            })?;
        }
        for c in &completion.tool_calls {
            sink(ProviderEvent::ToolCallDone { call: c.clone() })?;
        }
        if let Some(usage) = completion.usage {
            sink(ProviderEvent::Usage { usage })?;
        }
        if let Some(text) = &completion.refusal {
            sink(ProviderEvent::Refusal { text: text.clone() })?;
        }
        sink(ProviderEvent::Completed {
            response_id: completion.response_id.clone(),
            model: completion.model.clone(),
        })?;
        Ok(completion)
    }
}

pub(crate) fn read_response(
    body: &mut ureq::Body,
    sse: bool,
    provider: &str,
    model: &str,
    caps: ProviderCapabilities,
    cancelled: &dyn Fn() -> bool,
    sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
) -> Result<Completion, BackendError> {
    let mut fold = Fold::default();
    if !sse {
        let v = crate::backend::read_bounded_json_body(body, cancelled)?;
        if v["type"] != "message" || v["role"] != "assistant" {
            return Err(invalid("invalid Anthropic JSON message"));
        }
        let mut start = v.clone();
        start["content"] = json!([]);
        start["stop_reason"] = Value::Null;
        fold.event(json!({"type":"message_start","message":start}), model, sink)?;
        for (index, b) in v["content"]
            .as_array()
            .ok_or_else(|| invalid("Anthropic JSON content missing"))?
            .iter()
            .enumerate()
        {
            fold.event(
                json!({"type":"content_block_start","index":index,"content_block":b}),
                model,
                sink,
            )?;
            fold.event(
                json!({"type":"content_block_stop","index":index}),
                model,
                sink,
            )?;
        }
        fold.event(json!({"type":"message_delta","delta":{"stop_reason":v["stop_reason"]},"usage":v["usage"]}),model,sink)?;
        fold.event(json!({"type":"message_stop"}), model, sink)?;
    } else {
        let mut reader = body.with_config().limit(MAX_RESPONSE_BYTES as u64).reader();
        let mut pending = vec![];
        let mut data = vec![];
        let mut event: Option<String> = None;
        let mut chunk = [0u8; 8192];
        loop {
            if cancelled() {
                return Err(BackendError::Cancelled);
            }
            let n = match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if crate::backend::is_recv_body_poll_timeout(&e) => continue,
                Err(e) => return Err(BackendError::Transport(e.to_string())),
            };
            pending.extend_from_slice(&chunk[..n]);
            while let Some(end) = pending.iter().position(|b| *b == b'\n' || *b == b'\r') {
                if pending[end] == b'\r' && end + 1 == pending.len() {
                    break;
                }
                let consumed = end
                    + 1
                    + usize::from(pending[end] == b'\r' && pending.get(end + 1) == Some(&b'\n'));
                let raw = pending.drain(..consumed).collect::<Vec<_>>();
                let line = std::str::from_utf8(&raw)
                    .map_err(|_| invalid("Anthropic SSE invalid UTF-8"))?
                    .trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    dispatch_frame(&data, event.as_deref(), &mut fold, model, sink)?;
                    data.clear();
                    event = None;
                } else if let Some(value) = line.strip_prefix("data:") {
                    if !data.is_empty() {
                        data.push(b'\n');
                    }
                    data.extend_from_slice(value.strip_prefix(' ').unwrap_or(value).as_bytes());
                } else if let Some(value) = line.strip_prefix("event:") {
                    if event.is_some() {
                        return Err(invalid("duplicate SSE event name"));
                    }
                    event = Some(value.trim().into());
                }
                if data.len() > MAX_FRAME {
                    return Err(invalid("Anthropic SSE frame exceeds bound"));
                }
            }
            if pending.len() > MAX_FRAME {
                return Err(invalid("Anthropic SSE line exceeds bound"));
            }
        }
        if pending == b"\r" {
            dispatch_frame(&data, event.as_deref(), &mut fold, model, sink)?;
            pending.clear();
            data.clear();
            event = None;
        }
        if !pending.is_empty() || !data.is_empty() || event.is_some() {
            return Err(invalid("truncated Anthropic SSE frame"));
        }
    }
    if cancelled() {
        return Err(BackendError::Cancelled);
    }
    fold.finish(provider, caps, sink)
}

fn dispatch_frame(
    data: &[u8],
    event: Option<&str>,
    fold: &mut Fold,
    model: &str,
    sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
) -> Result<(), BackendError> {
    if data.is_empty() {
        return Ok(());
    }
    let v: Value =
        serde_json::from_slice(data).map_err(|_| invalid("malformed Anthropic SSE JSON"))?;
    if event.is_some_and(|e| Some(e) != v["type"].as_str()) {
        return Err(invalid("Anthropic event name/type mismatch"));
    }
    fold.event(v, model, sink)
}
