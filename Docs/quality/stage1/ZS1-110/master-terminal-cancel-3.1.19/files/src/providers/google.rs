//! Native generateContent wire. Ordered signature-bearing parts are opaque
//! history; executable calls are released only after a complete STOP response.
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
use sha2::{Digest, Sha256};
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
const WIRE: &str = "google_generative_ai";
const MAX_PARTS: usize = 2048;
const MAX_HISTORY: usize = 256 * 1024;
const MAX_FRAME: usize = 256 * 1024;
fn invalid(s: impl Into<String>) -> BackendError {
    BackendError::InvalidResponse(s.into())
}
fn config(s: impl Into<String>) -> BackendError {
    BackendError::Configuration(s.into())
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str, BackendError> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("Gemini missing string {key}")))
}
fn name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
pub(crate) fn validate_effort(model: &str, effort: Option<&str>) -> Result<(), BackendError> {
    if effort.is_some_and(|e| {
        model != "gemini-2.5-flash" || !matches!(e, "none" | "minimal" | "low" | "medium" | "high")
    }) {
        return Err(config("unsupported Gemini model/effort combination"));
    }
    Ok(())
}
pub(crate) fn endpoint(base: &str, model: &str, streaming: bool) -> Result<String, BackendError> {
    if model.is_empty()
        || model.len() > 128
        || !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err(config(
            "Gemini model must be a bounded model ID, without path/query components",
        ));
    }
    let suffix = if streaming {
        "streamGenerateContent?alt=sse"
    } else {
        "generateContent"
    };
    Ok(format!("{base}/models/{model}:{suffix}"))
}
fn validate_parts(parts: &[Value]) -> Result<String, BackendError> {
    if parts.len() > MAX_PARTS
        || serde_json::to_vec(parts)
            .map_err(|_| invalid("Gemini parts encoding failed"))?
            .len()
            > MAX_HISTORY
    {
        return Err(invalid("Gemini native parts exceed bound"));
    }
    let mut text = String::new();
    for p in parts {
        let obj = p
            .as_object()
            .ok_or_else(|| invalid("Gemini part must be object"))?;
        if obj.keys().any(|k| {
            !matches!(
                k.as_str(),
                "text" | "thought" | "thoughtSignature" | "functionCall"
            )
        }) {
            return Err(invalid("unsupported Gemini part type or field"));
        }
        if let Some(sig) = obj.get("thoughtSignature") {
            let sig = sig
                .as_str()
                .ok_or_else(|| invalid("Gemini signature must be string"))?;
            if sig.is_empty() || STANDARD.decode(sig).is_err() {
                return Err(invalid("Gemini signature must be nonempty base64"));
            }
        }
        if obj.get("thought").is_some_and(|v| !v.is_boolean()) {
            return Err(invalid("Gemini thought marker must be boolean"));
        }
        match (obj.get("text"), obj.get("functionCall")) {
            (Some(t), None) => {
                let t = t
                    .as_str()
                    .ok_or_else(|| invalid("Gemini text must be string"))?;
                if p["thought"] != true {
                    text.push_str(t);
                }
            }
            (None, Some(fc)) => {
                if p["thought"] == true
                    || fc.as_object().is_none_or(|o| {
                        o.keys()
                            .any(|k| !matches!(k.as_str(), "id" | "name" | "args"))
                    })
                    || !name(string(fc, "name")?)
                    || fc.get("args").is_some_and(|v| !v.is_object())
                {
                    return Err(invalid("invalid Gemini function call"));
                }
                if let Some(id) = fc.get("id")
                    && !id.as_str().is_some_and(name)
                {
                    return Err(invalid("invalid Gemini function call ID"));
                }
            }
            _ => {
                return Err(invalid(
                    "Gemini part must contain exactly one text or functionCall",
                ));
            }
        }
    }
    Ok(text)
}
fn parts_digest(parts: &[Value]) -> Result<String, BackendError> {
    Ok(format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(parts)
                .map_err(|_| invalid("Gemini history serialization failed"))?
        )
    ))
}
fn derive_calls(parts: &[Value], response_id: &str) -> Result<Vec<ToolCall>, BackendError> {
    let mut calls = vec![];
    let mut seen = BTreeSet::new();
    for (index, p) in parts.iter().enumerate() {
        if let Some(fc) = p.get("functionCall") {
            let id = fc
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    // Local correlation only. Never insert this synthetic ID into
                    // the opaque provider part or into an ID-less functionResponse.
                    let digest = format!(
                        "{:x}",
                        Sha256::digest(format!("{response_id}:{index}").as_bytes())
                    );
                    format!("gemini_{}", &digest[..32])
                });
            if !seen.insert(id.clone()) {
                return Err(invalid("duplicate Gemini function call ID"));
            }
            calls.push(ToolCall {
                id,
                name: string(fc, "name")?.into(),
                arguments: fc.get("args").cloned().unwrap_or_else(|| json!({})),
            });
        }
    }
    if calls.len() > 128 {
        return Err(invalid("too many Gemini calls"));
    }
    Ok(calls)
}
fn assistant_parts(
    turn: &Turn,
    provider: &str,
    model: &str,
) -> Result<(Vec<Value>, Vec<ToolCall>), BackendError> {
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
    let calls = turn
        .metadata
        .as_ref()
        .and_then(|m| m.get("tool_calls"))
        .map(|v| serde_json::from_value::<Vec<ToolCall>>(v.clone()))
        .transpose()
        .map_err(|_| config("invalid saved Gemini calls"))?
        .unwrap_or_default();
    if let Some(a) = native.first() {
        if a["wire"] != WIRE || a["provider"] != provider {
            return Err(config("Gemini history provider/wire mismatch"));
        }
        let parts = a["parts"]
            .as_array()
            .ok_or_else(|| config("Gemini history parts missing"))?;
        if a["parts_sha256"] != parts_digest(parts)? {
            return Err(config("Gemini native parts integrity mismatch"));
        }
        let text = validate_parts(parts)?;
        let saved = derive_calls(parts, string(a, "response_id")?)?;
        if text != turn.content || saved != calls {
            return Err(config(
                "Gemini native history disagrees with saved text/calls",
            ));
        }
        if parts
            .iter()
            .any(|p| p.get("thoughtSignature").is_some() || p["thought"] == true)
            && a["model"] != model
        {
            return Err(config("Gemini opaque history belongs to another model"));
        }
        return Ok((parts.clone(), calls));
    }
    if annotations
        .into_iter()
        .flatten()
        .any(|a| a["type"] == "reasoning")
    {
        return Err(config(
            "unsigned reasoning cannot be converted to Gemini history",
        ));
    }
    if !calls.is_empty() {
        return Err(config("Gemini tool history requires original native parts"));
    }
    Ok((vec![json!({"text":turn.content})], calls))
}
pub(crate) fn request_body(
    request: &CompletionRequest<'_>,
    model: &str,
    effort: Option<&str>,
    provider: &str,
) -> Result<Value, BackendError> {
    validate_effort(model, effort)?;
    if request.response_format.is_some() {
        return Err(config("Gemini structured output not enabled"));
    }
    if let Some(m) = request.metadata
        && (m
            .as_object()
            .is_none_or(|o| o.keys().any(|k| k != "purpose"))
            || m.get("purpose").is_some_and(|p| p != "semantic_compaction"))
    {
        return Err(config("unsupported Gemini request metadata"));
    }
    if request.attachments.len() > crate::backend::MAX_ATTACHMENTS_PER_TURN
        || request
            .attachments
            .iter()
            .filter_map(|a| a.data.as_ref())
            .map(Vec::len)
            .sum::<usize>()
            > crate::backend::MAX_TOTAL_ATTACHMENT_BYTES
    {
        return Err(config("Gemini attachments exceed bound"));
    }
    let mut system = vec![];
    if let Some(t) = request.instructions
        && !t.is_empty()
    {
        system.push(json!({"text":t}));
    }
    let mut contents: Vec<Value> = vec![];
    let mut pending: BTreeMap<String, (String, Option<String>)> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for turn in request.turns {
        let mut parts = vec![];
        let role = match turn.role {
            TurnRole::System => {
                if !contents.is_empty() {
                    return Err(config(
                        "mid-conversation system message unsupported by Gemini",
                    ));
                }
                system.push(json!({"text":turn.content}));
                continue;
            }
            TurnRole::User => {
                if !pending.is_empty() {
                    return Err(config("Gemini tool results missing before user input"));
                }
                if !turn.content.is_empty() {
                    parts.push(json!({"text":turn.content}));
                }
                for a in request.attachments.iter().filter(|a| a.turn_id == turn.id) {
                    a.input.validate()?;
                    if a.input.kind != crate::backend::AttachmentKind::Image
                        || !matches!(
                            a.input.mime_type.as_str(),
                            "image/png" | "image/jpeg" | "image/webp" | "image/heic" | "image/heif"
                        )
                    {
                        return Err(config("unsupported Gemini attachment type"));
                    }
                    // Host materialized files only. URL fetching/file upload is
                    // a separate capability and is not silently performed here.
                    let data = a
                        .data
                        .as_ref()
                        .filter(|d| {
                            !d.is_empty() && d.len() <= crate::backend::MAX_ATTACHMENT_BYTES
                        })
                        .ok_or_else(|| {
                            config("Gemini image requires bounded materialized bytes")
                        })?;
                    if a.input.file_id.is_some() || a.input.url.is_some() {
                        return Err(config("Gemini remote image reference unsupported"));
                    }
                    parts.push(json!({"inlineData":{"mimeType":a.input.mime_type,"data":STANDARD.encode(data)}}));
                }
                "user"
            }
            TurnRole::Assistant => {
                if !pending.is_empty() {
                    return Err(config(
                        "Gemini tool results missing before model continuation",
                    ));
                }
                let (saved, calls) = assistant_parts(turn, provider, model)?;
                parts = saved;
                let fc_parts = parts.iter().filter_map(|p| p.get("functionCall"));
                for (call, fc) in calls.into_iter().zip(fc_parts) {
                    if !seen.insert(call.id.clone()) {
                        return Err(config("duplicate Gemini history call ID"));
                    }
                    pending.insert(
                        call.id,
                        (
                            call.name,
                            fc.get("id").and_then(Value::as_str).map(str::to_owned),
                        ),
                    );
                }
                "model"
            }
            TurnRole::Tool => {
                let id = turn
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("tool_call_id"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| config("orphan Gemini tool result"))?;
                let (name, wire_id) = pending
                    .remove(id)
                    .ok_or_else(|| config("unexpected or duplicate Gemini tool result"))?;
                let failed = turn
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("outcome"))
                    .and_then(Value::as_str)
                    .is_some_and(|v| v != "succeeded");
                let mut fc = json!({"name":name,"response":if failed {json!({"error":turn.content})}else{json!({"output":turn.content})}});
                if let Some(id) = wire_id {
                    fc["id"] = json!(id);
                }
                parts.push(json!({"functionResponse":fc}));
                "user"
            }
        };
        if parts.is_empty() {
            return Err(config("empty Gemini content"));
        }
        if contents.last().is_some_and(|m| m["role"] == role) {
            contents.last_mut().expect("last")["parts"]
                .as_array_mut()
                .expect("parts")
                .extend(parts);
        } else {
            contents.push(json!({"role":role,"parts":parts}));
        }
    }
    if !pending.is_empty()
        || contents.first().is_none_or(|c| c["role"] != "user")
        || contents.last().is_none_or(|c| c["role"] != "user")
    {
        return Err(config(
            "Gemini requires complete history ending in user input or function responses",
        ));
    }
    let max = request
        .max_output_tokens
        .unwrap_or(crate::context::DEFAULT_RESERVED_OUTPUT_TOKENS);
    if max == 0 {
        return Err(config("Gemini maxOutputTokens must be positive"));
    }
    let mut body =
        json!({"contents":contents,"generationConfig":{"candidateCount":1,"maxOutputTokens":max}});
    if !system.is_empty() {
        body["systemInstruction"] = json!({"parts":system});
    }
    if !request.tools.is_empty() {
        body["tools"] = json!([{"functionDeclarations":request.tools.iter().map(|t|json!({"name":t.name,"description":t.description,"parametersJsonSchema":t.input_schema})).collect::<Vec<_>>()}]);
    }
    if let Some(e) = effort {
        let budget = match e {
            "none" => 0,
            "minimal" => 128,
            "low" => 2048,
            "medium" => 8192,
            "high" => 24576,
            _ => return Err(config("unsupported Gemini effort")),
        };
        body["generationConfig"]["thinkingConfig"] =
            json!({"thinkingBudget":budget,"includeThoughts":e!="none"});
    }
    if serde_json::to_vec(&body)
        .map_err(|_| config("Gemini request encoding failed"))?
        .len()
        > 32 * 1024 * 1024
    {
        return Err(config("Gemini request exceeds bound"));
    }
    Ok(body)
}
#[derive(Default)]
struct Fold {
    parts: Vec<Value>,
    response_id: Option<String>,
    model_version: Option<String>,
    stop: bool,
    usage: Option<Usage>,
    cached: u64,
    thoughts: u64,
    created: bool,
}
impl Fold {
    fn chunk(
        &mut self,
        v: Value,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<(), BackendError> {
        if !v.is_object() || v.get("error").is_some() {
            return Err(invalid("Gemini provider error or invalid response"));
        }
        if v.get("promptFeedback")
            .and_then(|p| p.get("blockReason"))
            .is_some()
        {
            return Err(invalid("Gemini prompt blocked"));
        }
        for (key, slot) in [
            ("responseId", &mut self.response_id),
            ("modelVersion", &mut self.model_version),
        ] {
            if let Some(value) = v.get(key) {
                let s = value
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
                    .ok_or_else(|| invalid("invalid Gemini response identity"))?;
                if slot.as_deref().is_some_and(|old| old != s) {
                    return Err(invalid("Gemini response identity changed during stream"));
                }
                *slot = Some(s.into());
            }
        }
        if !self.created {
            sink(ProviderEvent::ResponseCreated {
                response_id: self.response_id.clone(),
                model: self.model_version.clone(),
            })?;
            self.created = true;
        }
        if let Some(cs) = v.get("candidates") {
            let cs = cs
                .as_array()
                .ok_or_else(|| invalid("Gemini candidates must be array"))?;
            if cs.len() > 1 {
                return Err(invalid("Gemini requires one candidate"));
            }
            for c in cs {
                if c.get("index").is_some_and(|i| i != 0) || self.stop {
                    return Err(invalid(
                        "unexpected Gemini candidate or content after finish",
                    ));
                }
                if let Some(content) = c.get("content") {
                    if content.get("role").is_some_and(|r| r != "model") {
                        return Err(invalid("Gemini response role must be model"));
                    }
                    let parts = content["parts"]
                        .as_array()
                        .ok_or_else(|| invalid("Gemini candidate parts missing"))?;
                    validate_parts(parts)?;
                    for p in parts {
                        if let Some(t) = p.get("text").and_then(Value::as_str)
                            && !t.is_empty()
                        {
                            sink(if p["thought"] == true {
                                ProviderEvent::ReasoningDelta { delta: t.into() }
                            } else {
                                ProviderEvent::TextDelta { delta: t.into() }
                            })?;
                        }
                        if let Some(fc) = p.get("functionCall") {
                            sink(ProviderEvent::ToolCallDelta {
                                call_id: fc.get("id").and_then(Value::as_str).map(str::to_owned),
                                name: Some(string(fc, "name")?.into()),
                                arguments_delta: fc
                                    .get("args")
                                    .cloned()
                                    .unwrap_or_else(|| json!({}))
                                    .to_string(),
                            })?;
                        }
                        // Each received part remains separate; signatures never
                        // move to adjacent text or function-call parts.
                        self.parts.push(p.clone());
                    }
                    validate_parts(&self.parts)?;
                }
                if let Some(stop) = c.get("finishReason") {
                    if stop != "STOP" {
                        return Err(invalid(format!(
                            "Gemini incomplete/unsupported finish reason: {}",
                            stop.as_str().unwrap_or("invalid")
                        )));
                    }
                    self.stop = true;
                }
            }
        }
        if let Some(u) = v.get("usageMetadata") {
            let count = |k: &str| -> Result<u64, BackendError> {
                match u.get(k) {
                    None => Ok(0),
                    Some(v) => v
                        .as_u64()
                        .ok_or_else(|| invalid("invalid Gemini usage count")),
                }
            };
            let input = count("promptTokenCount")?;
            let cached = count("cachedContentTokenCount")?;
            let thoughts = count("thoughtsTokenCount")?;
            let output = count("candidatesTokenCount")?
                .checked_add(thoughts)
                .ok_or_else(|| invalid("Gemini usage overflow"))?;
            let total = input
                .checked_add(output)
                .ok_or_else(|| invalid("Gemini usage overflow"))?;
            if cached > input
                || u.get("totalTokenCount")
                    .is_some_and(|v| v.as_u64() != Some(total))
                || self
                    .usage
                    .is_some_and(|old| input < old.input_tokens || output < old.output_tokens)
                || cached < self.cached
                || thoughts < self.thoughts
            {
                return Err(invalid("inconsistent/decreasing Gemini usage"));
            }
            self.cached = cached;
            self.thoughts = thoughts;
            self.usage = Some(Usage {
                input_tokens: input,
                output_tokens: output,
                total_tokens: total,
            });
        }
        Ok(())
    }
    fn finish(
        self,
        provider: &str,
        model: &str,
        caps: ProviderCapabilities,
        sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
    ) -> Result<Completion, BackendError> {
        if !self.stop || self.parts.is_empty() {
            return Err(invalid("Gemini response missing complete terminal content"));
        }
        if self.model_version.as_deref().is_none_or(|version| {
            version != model
                && !version
                    .strip_prefix(&format!("{model}-"))
                    .is_some_and(|suffix| {
                        !suffix.is_empty()
                            && suffix.bytes().any(|b| b.is_ascii_digit())
                            && suffix.bytes().all(|b| b.is_ascii_digit() || b == b'-')
                    })
        }) {
            return Err(invalid(
                "Gemini response model version does not match requested model",
            ));
        }
        let response_id = self
            .response_id
            .ok_or_else(|| invalid("Gemini responseId missing"))?;
        let content = validate_parts(&self.parts)?;
        let calls = derive_calls(&self.parts, &response_id)?;
        if !caps.tools && !calls.is_empty() {
            return Err(invalid("Gemini returned tools for a no-tools model"));
        }
        if !caps.reasoning
            && self
                .parts
                .iter()
                .any(|p| p["thought"] == true || p.get("thoughtSignature").is_some())
        {
            return Err(invalid("Gemini returned reasoning for non-reasoning model"));
        }
        // Gemini 3+ requires a signature on the first function call of each
        // response step. 2.5 can omit it; an explicitly empty signature is bad.
        if model.starts_with("gemini-3")
            && self
                .parts
                .iter()
                .find(|p| p.get("functionCall").is_some())
                .is_some_and(|p| p.get("thoughtSignature").is_none())
        {
            return Err(invalid("Gemini first function call signature missing"));
        }
        let completion = Completion {
            content,
            tool_calls: calls,
            usage: self.usage,
            response_id: Some(response_id.clone()),
            model: Some(model.into()),
            refusal: None,
            annotations: vec![
                json!({"type":"native_history","wire":WIRE,"provider":provider,"model":model,"response_id":response_id,"parts_sha256":parts_digest(&self.parts)?,"parts":self.parts}),
                json!({"type":"gemini_finish","finish_reason":"STOP","model_version":self.model_version,"cached_input_tokens":self.cached,"thought_tokens":self.thoughts}),
            ],
        };
        if !completion.content.is_empty() {
            sink(ProviderEvent::TextDone {
                text: completion.content.clone(),
            })?;
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
    // The sink can cancel while one frame or the terminal batch is published.
    // Check each event so no later delta, ready tool or completion escapes.
    let sink = &mut |event| {
        if cancelled() {
            return Err(BackendError::Cancelled);
        }
        sink(event)
    };
    let mut fold = Fold::default();
    if !sse {
        fold.chunk(
            crate::backend::read_bounded_json_body(body, cancelled)?,
            sink,
        )?;
    } else {
        let mut reader = body.with_config().limit(MAX_RESPONSE_BYTES as u64).reader();
        let mut pending = vec![];
        let mut data = vec![];
        let mut chunk = [0; 8192];
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
            while let Some(end) = pending.iter().position(|b| matches!(*b, b'\r' | b'\n')) {
                if pending[end] == b'\r' && end + 1 == pending.len() {
                    break;
                }
                let consumed = end
                    + 1
                    + usize::from(pending[end] == b'\r' && pending.get(end + 1) == Some(&b'\n'));
                let raw = pending.drain(..consumed).collect::<Vec<_>>();
                let line = std::str::from_utf8(&raw)
                    .map_err(|_| invalid("Gemini SSE invalid UTF-8"))?
                    .trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    dispatch(&mut data, &mut fold, sink)?;
                } else if let Some(value) = line.strip_prefix("data:") {
                    if !data.is_empty() {
                        data.push(b'\n');
                    }
                    data.extend_from_slice(value.strip_prefix(' ').unwrap_or(value).as_bytes());
                } else if let Some(event) = line.strip_prefix("event:")
                    && !matches!(event.trim(), "" | "message")
                {
                    return Err(invalid("unsupported Gemini SSE event"));
                }
                if data.len() > MAX_FRAME {
                    return Err(invalid("Gemini SSE frame exceeds bound"));
                }
            }
            if pending.len() > MAX_FRAME {
                return Err(invalid("Gemini SSE line exceeds bound"));
            }
        }
        if pending == b"\r" {
            dispatch(&mut data, &mut fold, sink)?;
            pending.clear();
        }
        if !pending.is_empty() || !data.is_empty() {
            return Err(invalid("truncated Gemini SSE frame"));
        }
    }
    if cancelled() {
        return Err(BackendError::Cancelled);
    }
    fold.finish(provider, model, caps, sink)
}
fn dispatch(
    data: &mut Vec<u8>,
    fold: &mut Fold,
    sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
) -> Result<(), BackendError> {
    if !data.is_empty() {
        let v = serde_json::from_slice(data).map_err(|_| invalid("malformed Gemini SSE JSON"))?;
        fold.chunk(v, sink)?;
        data.clear();
    }
    Ok(())
}
