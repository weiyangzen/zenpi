//! UI-independent agent trajectory projection.
//!
//! A trajectory is the ordered list of records an agent's context is built
//! from: user prompts, provider requests (assistant answers and tool-call
//! steps), tool results and compaction summaries, grouped by the user turn
//! that started them. The projection is pure: it reads persisted [`Turn`]s and
//! the per-request metadata written by the core loop, and never touches a
//! terminal. The TUI viewer renders it; tests assert on it directly.
//!
//! The KV-cache check compares each provider request with the previous one.
//! A provider can only reuse its prefix cache while the instructions, the tool
//! set and the already-sent context turns stay byte-identical, so every
//! request records a [`RequestFingerprint`] and the projection flags requests
//! whose prefix changed or whose reported cache reads fall short of the
//! reusable prefix.

use crate::backend::Usage;
use crate::core::{Turn, TurnRole};
use crate::tools::ToolDefinition;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Turn-metadata key holding a provider request's timing and fingerprint.
pub const REQUEST_METADATA_KEY: &str = "request";

/// Prompts shorter than this are not expected to be cached by providers.
const MIN_CACHEABLE_TOKENS: u64 = 1024;
/// A stable prefix should be served at least this share from cache.
const MIN_EXPECTED_CACHE_RATIO: f64 = 0.5;
const SUMMARY_CHARS: usize = 160;

/// Digests of everything a provider request puts in front of the new tokens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestFingerprint {
    instructions: String,
    tools: String,
    /// Chained digest after each context turn; entry `i` covers turns `0..=i`.
    chain: Vec<[u8; 32]>,
}

impl RequestFingerprint {
    pub fn new(instructions: &str, tools: &[ToolDefinition], turns: &[Turn]) -> Self {
        let tools = tools
            .iter()
            .map(|tool| json!([tool.name, tool.description, tool.input_schema]))
            .collect::<Vec<_>>();
        let mut previous = [0_u8; 32];
        let chain = turns
            .iter()
            .map(|turn| {
                let mut hasher = Sha256::new();
                hasher.update(previous);
                hasher.update(role_label(turn.role).as_bytes());
                hasher.update([0]);
                hasher.update(turn.content.as_bytes());
                if let Some(wire) = wire_metadata(turn) {
                    hasher.update([0]);
                    hasher.update(wire.to_string().as_bytes());
                }
                previous = hasher.finalize().into();
                previous
            })
            .collect();
        Self {
            instructions: short_digest(instructions.as_bytes()),
            tools: short_digest(Value::Array(tools).to_string().as_bytes()),
            chain,
        }
    }

    /// Number of leading context turns identical to `previous`'s.
    pub fn shared_turns(&self, previous: &Self) -> usize {
        self.chain
            .iter()
            .zip(&previous.chain)
            .take_while(|(left, right)| left == right)
            .count()
    }

    /// Metadata persisted on the assistant turn that answered this request.
    pub fn metadata(&self, previous: Option<&Self>, timing: RequestTiming) -> Value {
        let context = self.chain.last().map(|digest| hex(&digest[..8]));
        let mut value = json!({
            "started_ms": timing.started_ms,
            "completed_ms": timing.completed_ms,
            "instructions_digest": self.instructions,
            "tools_digest": self.tools,
            "context_digest": context,
            "context_turns": self.chain.len(),
        });
        if let Some(first) = timing.first_token_ms {
            value["first_token_ms"] = json!(first);
        }
        if let Some(previous) = previous {
            value["shared_turns"] = json!(self.shared_turns(previous));
            value["previous_context_turns"] = json!(previous.chain.len());
        }
        value
    }
}

/// Wall-clock milestones of one provider request, in Unix milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RequestTiming {
    pub started_ms: u64,
    pub first_token_ms: Option<u64>,
    pub completed_ms: u64,
}

/// The persisted form of [`RequestFingerprint::metadata`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestRecord {
    pub started_ms: u64,
    pub first_token_ms: Option<u64>,
    pub completed_ms: u64,
    pub instructions_digest: String,
    pub tools_digest: String,
    pub context_turns: usize,
    pub shared_turns: Option<usize>,
    pub previous_context_turns: Option<usize>,
}

impl RequestRecord {
    fn from_metadata(value: &Value) -> Option<Self> {
        let count = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
        };
        Some(Self {
            started_ms: value.get("started_ms")?.as_u64()?,
            first_token_ms: value.get("first_token_ms").and_then(Value::as_u64),
            completed_ms: value.get("completed_ms")?.as_u64()?,
            instructions_digest: string_field(value, "instructions_digest"),
            tools_digest: string_field(value, "tools_digest"),
            context_turns: count("context_turns").unwrap_or(0),
            shared_turns: count("shared_turns"),
            previous_context_turns: count("previous_context_turns"),
        })
    }

    pub fn duration_ms(&self) -> u64 {
        self.completed_ms.saturating_sub(self.started_ms)
    }

    pub fn ttft_ms(&self) -> Option<u64> {
        self.first_token_ms
            .map(|first| first.saturating_sub(self.started_ms))
    }

    /// Output tokens per second after the first token arrived.
    pub fn throughput(&self, output_tokens: u64) -> Option<f64> {
        let decode_ms = self.completed_ms.saturating_sub(self.first_token_ms?);
        (decode_ms > 0 && output_tokens > 0)
            .then(|| output_tokens as f64 * 1000.0 / decode_ms as f64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    /// A prompt typed by the user (or queued as a follow-up).
    User,
    /// Input steered into a running turn.
    Steer,
    /// A provider answer that ended the turn.
    Assistant,
    /// A provider step that requested tools.
    ToolCall,
    /// A tool result fed back to the provider.
    ToolResult,
    /// A compaction summary or other system context.
    System,
}

impl RecordKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "USER",
            Self::Steer => "STEER",
            Self::Assistant => "ASSISTANT",
            Self::ToolCall => "CALL",
            Self::ToolResult => "TOOL",
            Self::System => "SYSTEM",
        }
    }

    /// True for records that are one provider request each.
    pub fn is_request(self) -> bool {
        matches!(self, Self::Assistant | Self::ToolCall)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStatus {
    Success,
    Error,
    InProgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CacheSeverity {
    Ok,
    /// The prefix changed for a known, expected reason (e.g. compaction).
    Info,
    /// The prefix changed in a way that defeats caching.
    Warn,
    /// The prefix was stable but the provider reported too few cached tokens.
    Miss,
}

/// The KV-cache verdict for one provider request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheVerdict {
    pub severity: CacheSeverity,
    pub message: String,
    pub hint: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrajectoryRecord {
    /// 1-based position in the trajectory.
    pub index: usize,
    pub record_id: String,
    /// 1-based turn group number.
    pub turn: usize,
    pub kind: RecordKind,
    pub timestamp_ms: u64,
    pub summary: String,
    pub detail: String,
    pub tool_name: Option<String>,
    pub model: Option<String>,
    pub usage: Option<Usage>,
    pub request: Option<RequestRecord>,
    pub status: RecordStatus,
    pub error: Option<String>,
    pub cache: Option<CacheVerdict>,
    pub metadata: Option<Value>,
}

impl TrajectoryRecord {
    /// Rough token size of the record's own content.
    pub fn estimated_tokens(&self) -> u64 {
        u64::try_from(self.detail.len().div_ceil(4)).unwrap_or(u64::MAX)
    }

    pub fn cache_ratio(&self) -> Option<f64> {
        let usage = self.usage?;
        (usage.input_tokens > 0 && usage.reports_cache())
            .then(|| usage.cache_read_tokens as f64 / usage.input_tokens as f64)
    }

    fn haystack(&self) -> String {
        format!(
            "#{} {} {} {} {}",
            self.index,
            self.kind.label(),
            self.tool_name.as_deref().unwrap_or(""),
            self.summary,
            self.detail
        )
        .to_lowercase()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnGroup {
    pub number: usize,
    pub root_id: String,
    pub started_ms: u64,
    pub ended_ms: u64,
    /// Positions in [`Trajectory::records`], in journal order.
    pub records: Vec<usize>,
    pub in_progress: bool,
}

impl TurnGroup {
    pub fn duration_ms(&self) -> u64 {
        self.ended_ms.saturating_sub(self.started_ms)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrajectoryStats {
    pub requests: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// Requests whose provider reported any cache counter.
    pub cache_reporting_requests: usize,
    pub avg_ttft_ms: Option<u64>,
    pub warnings: usize,
    pub misses: usize,
}

impl TrajectoryStats {
    /// Share of prompt tokens served from cache, over requests that report it.
    pub fn cache_hit_ratio(&self) -> Option<f64> {
        (self.cache_reporting_requests > 0 && self.input_tokens > 0)
            .then(|| self.cache_read_tokens as f64 / self.input_tokens as f64)
    }

    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    pub fn kv_issues(&self) -> usize {
        self.warnings + self.misses
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trajectory {
    pub records: Vec<TrajectoryRecord>,
    pub turns: Vec<TurnGroup>,
    pub stats: TrajectoryStats,
}

impl Trajectory {
    /// Project persisted history. `active_root` names the user turn that is
    /// still running, if any; every other turn without a final answer is
    /// reported as failed or interrupted.
    pub fn from_turns(history: &[Turn], active_root: Option<&str>) -> Self {
        let mut trajectory = Self::default();
        let mut root_of: std::collections::HashMap<&str, usize> = Default::default();
        for turn in history {
            let joined = turn
                .parent_id
                .as_deref()
                .and_then(|parent| root_of.get(parent).copied())
                .or_else(|| {
                    turn.parent_id
                        .as_ref()
                        .and(trajectory.turns.len().checked_sub(1))
                });
            let group = match joined {
                Some(group) => group,
                None => {
                    trajectory.turns.push(TurnGroup {
                        number: trajectory.turns.len() + 1,
                        root_id: turn.id.clone(),
                        started_ms: turn.created_at_ms,
                        ended_ms: turn.created_at_ms,
                        records: Vec::new(),
                        in_progress: active_root == Some(turn.id.as_str()),
                    });
                    root_of.insert(&turn.id, trajectory.turns.len() - 1);
                    trajectory.turns.len() - 1
                }
            };
            let record = project_record(turn, trajectory.records.len() + 1, group + 1);
            let ended = record
                .request
                .as_ref()
                .map_or(record.timestamp_ms, |request| request.completed_ms);
            let turn_group = &mut trajectory.turns[group];
            turn_group.ended_ms = turn_group.ended_ms.max(ended);
            turn_group.records.push(trajectory.records.len());
            trajectory.records.push(record);
        }
        trajectory.mark_unfinished_turns();
        trajectory.check_cache();
        trajectory.stats = trajectory.compute_stats();
        trajectory
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn started_ms(&self) -> Option<u64> {
        self.records.iter().map(|record| record.timestamp_ms).min()
    }

    pub fn ended_ms(&self) -> Option<u64> {
        self.turns.iter().map(|turn| turn.ended_ms).max()
    }

    pub fn in_progress(&self) -> bool {
        self.turns.iter().any(|turn| turn.in_progress)
    }

    /// Record positions (0-based) whose text contains `query`, case-insensitively.
    pub fn search(&self, query: &str) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        self.records
            .iter()
            .enumerate()
            .filter(|(_, record)| record.haystack().contains(&query))
            .map(|(position, _)| position)
            .collect()
    }

    /// Record positions (0-based) with a KV-cache warning or miss.
    pub fn cache_issues(&self) -> Vec<usize> {
        self.records
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                record
                    .cache
                    .as_ref()
                    .is_some_and(|verdict| verdict.severity >= CacheSeverity::Warn)
            })
            .map(|(position, _)| position)
            .collect()
    }

    /// Machine-readable totals and KV-cache findings (headless `/trajectory`).
    pub fn summary_json(&self) -> Value {
        let stats = &self.stats;
        let issues = self
            .records
            .iter()
            .filter_map(|record| {
                let verdict = record.cache.as_ref()?;
                (verdict.severity != CacheSeverity::Ok).then(|| {
                    json!({
                        "index": record.index,
                        "record_id": record.record_id,
                        "turn": record.turn,
                        "severity": format!("{:?}", verdict.severity).to_lowercase(),
                        "message": verdict.message,
                        "hint": verdict.hint,
                    })
                })
            })
            .collect::<Vec<_>>();
        json!({
            "command": "trajectory",
            "records": self.records.len(),
            "turns": self.turns.len(),
            "requests": stats.requests,
            "usage": {
                "input_tokens": stats.input_tokens,
                "output_tokens": stats.output_tokens,
                "reasoning_tokens": stats.reasoning_tokens,
                "cache_read_tokens": stats.cache_read_tokens,
                "cache_write_tokens": stats.cache_write_tokens,
            },
            "cache_hit_ratio": stats.cache_hit_ratio(),
            "avg_ttft_ms": stats.avg_ttft_ms,
            "kv_cache": issues,
        })
    }

    fn mark_unfinished_turns(&mut self) {
        for group in &self.turns {
            let kind = |position: &usize| self.records[*position].kind;
            let answered = group
                .records
                .iter()
                .any(|p| kind(p) == RecordKind::Assistant);
            let asked = group
                .records
                .first()
                .is_some_and(|p| matches!(kind(p), RecordKind::User | RecordKind::Steer));
            if answered || !asked {
                continue;
            }
            if let Some(last) = group.records.last().map(|p| &mut self.records[*p]) {
                if group.in_progress {
                    last.status = RecordStatus::InProgress;
                } else if last.status == RecordStatus::Success {
                    last.status = RecordStatus::Error;
                    last.error =
                        Some("turn ended without an answer (failed or interrupted)".into());
                }
            }
        }
    }

    fn check_cache(&mut self) {
        let provider_caches = self
            .records
            .iter()
            .any(|record| record.usage.is_some_and(|usage| usage.reports_cache()));
        let mut previous: Option<(RequestRecord, Usage)> = None;
        let mut saw_compaction = false;
        for record in &mut self.records {
            if record.kind == RecordKind::System {
                saw_compaction = true;
            }
            if !record.kind.is_request() {
                continue;
            }
            let Some(request) = record.request.clone() else {
                continue;
            };
            let usage = record.usage.unwrap_or_default();
            record.cache = Some(cache_verdict(
                &request,
                usage,
                previous.as_ref(),
                provider_caches,
                std::mem::take(&mut saw_compaction),
            ));
            previous = Some((request, usage));
        }
    }

    fn compute_stats(&self) -> TrajectoryStats {
        let mut stats = TrajectoryStats::default();
        let mut ttft_sum = 0_u64;
        let mut ttft_count = 0_u64;
        for record in &self.records {
            if !record.kind.is_request() {
                continue;
            }
            if record.usage.is_some() || record.request.is_some() {
                stats.requests += 1;
            }
            if let Some(usage) = record.usage {
                stats.input_tokens = stats.input_tokens.saturating_add(usage.input_tokens);
                stats.output_tokens = stats.output_tokens.saturating_add(usage.output_tokens);
                stats.reasoning_tokens = stats
                    .reasoning_tokens
                    .saturating_add(usage.reasoning_tokens);
                stats.cache_read_tokens = stats
                    .cache_read_tokens
                    .saturating_add(usage.cache_read_tokens);
                stats.cache_write_tokens = stats
                    .cache_write_tokens
                    .saturating_add(usage.cache_write_tokens);
                stats.cache_reporting_requests += usize::from(usage.reports_cache());
            }
            if let Some(ttft) = record.request.as_ref().and_then(RequestRecord::ttft_ms) {
                ttft_sum = ttft_sum.saturating_add(ttft);
                ttft_count += 1;
            }
            match record.cache.as_ref().map(|verdict| verdict.severity) {
                Some(CacheSeverity::Warn) => stats.warnings += 1,
                Some(CacheSeverity::Miss) => stats.misses += 1,
                _ => {}
            }
        }
        stats.avg_ttft_ms = (ttft_count > 0).then(|| ttft_sum / ttft_count);
        stats
    }
}

fn cache_verdict(
    request: &RequestRecord,
    usage: Usage,
    previous: Option<&(RequestRecord, Usage)>,
    provider_caches: bool,
    after_compaction: bool,
) -> CacheVerdict {
    let verdict = |severity, message: String, hint| CacheVerdict {
        severity,
        message,
        hint,
    };
    let Some((before, before_usage)) = previous else {
        let message = match usage.reports_cache() {
            // The prefix can be warm from earlier sessions with the same prompt.
            true => format!(
                "first request: nothing earlier to compare; {:.0}% of the prompt served from cache",
                usage.cache_read_tokens as f64 / usage.input_tokens.max(1) as f64 * 100.0
            ),
            false => "first request: nothing earlier to compare".into(),
        };
        return verdict(CacheSeverity::Ok, message, None);
    };
    if !before.instructions_digest.is_empty()
        && before.instructions_digest != request.instructions_digest
    {
        return verdict(
            CacheSeverity::Warn,
            format!(
                "instructions changed ({} -> {}): the whole prompt is re-prefilled",
                before.instructions_digest, request.instructions_digest
            ),
            Some(
                "skills, persona or resource text in the system prompt changed between requests; \
                 keep volatile text out of the instructions or append it as a turn",
            ),
        );
    }
    if !before.tools_digest.is_empty() && before.tools_digest != request.tools_digest {
        return verdict(
            CacheSeverity::Warn,
            format!(
                "tool set changed ({} -> {}): cache invalidated after the instructions",
                before.tools_digest, request.tools_digest
            ),
            Some(
                "a skill or model switch filtered the tool list; keep the tool list stable within a session",
            ),
        );
    }
    if let (Some(shared), Some(previous_turns)) =
        (request.shared_turns, request.previous_context_turns)
        && shared < previous_turns
    {
        let message = format!(
            "context prefix diverged at turn {} of {previous_turns}: {} earlier turns were rewritten",
            shared + 1,
            previous_turns - shared
        );
        return if after_compaction {
            verdict(
                CacheSeverity::Info,
                format!("{message} by compaction"),
                Some("expected after compaction; the next request should hit the cache again"),
            )
        } else {
            verdict(
                CacheSeverity::Warn,
                message,
                Some(
                    "history was edited, re-rendered or reordered; check templates and attachments in earlier turns",
                ),
            )
        };
    }
    let reusable = before_usage
        .input_tokens
        .saturating_add(before_usage.output_tokens);
    if provider_caches && usage.input_tokens > 0 && reusable >= MIN_CACHEABLE_TOKENS {
        let expected = reusable.min(usage.input_tokens);
        if (usage.cache_read_tokens as f64) < expected as f64 * MIN_EXPECTED_CACHE_RATIO {
            return verdict(
                CacheSeverity::Miss,
                format!(
                    "prefix stable but only {} of ~{} reusable tokens were cached",
                    usage.cache_read_tokens, expected
                ),
                Some(
                    "the provider evicted the cache or routed to another replica; check the gap \
                     since the previous request and the provider's cache TTL",
                ),
            );
        }
    }
    match usage
        .reports_cache()
        .then(|| usage.cache_read_tokens as f64 / usage.input_tokens.max(1) as f64 * 100.0)
    {
        Some(percent) => verdict(
            CacheSeverity::Ok,
            format!("prefix stable, {percent:.0}% of the prompt served from cache"),
            None,
        ),
        None => verdict(
            CacheSeverity::Ok,
            "prefix stable (provider reports no cache counters)".into(),
            None,
        ),
    }
}

fn project_record(turn: &Turn, index: usize, group: usize) -> TrajectoryRecord {
    let metadata = turn.metadata.as_ref();
    let field = |key: &str| metadata.and_then(|m| m.get(key));
    let kind = match turn.role {
        TurnRole::System => RecordKind::System,
        TurnRole::Tool => RecordKind::ToolResult,
        TurnRole::User if field("origin").and_then(Value::as_str) == Some("user_shell") => {
            RecordKind::ToolResult
        }
        TurnRole::User if turn.parent_id.is_some() => RecordKind::Steer,
        TurnRole::User => RecordKind::User,
        TurnRole::Assistant if field("tool_calls").is_some_and(Value::is_array) => {
            RecordKind::ToolCall
        }
        TurnRole::Assistant => RecordKind::Assistant,
    };
    let tool_name = field("tool_name")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let outcome = field("outcome").and_then(Value::as_str);
    let (status, error) = match outcome {
        Some("failed" | "denied" | "cancelled" | "unknown_outcome") => (
            RecordStatus::Error,
            Some(format!("tool outcome: {}", outcome.unwrap_or_default())),
        ),
        _ if field("refusal").is_some_and(|value| !value.is_null()) => (
            RecordStatus::Error,
            field("refusal")
                .and_then(Value::as_str)
                .map(|refusal| format!("refusal: {refusal}")),
        ),
        _ => (RecordStatus::Success, None),
    };
    let summary = match kind {
        RecordKind::ToolCall => {
            let calls = field("tool_calls")
                .and_then(Value::as_array)
                .map(|calls| {
                    calls
                        .iter()
                        .map(call_summary)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let text = one_line(&turn.content);
            if text.is_empty() {
                calls
            } else {
                format!("{calls} — {text}")
            }
        }
        RecordKind::ToolResult => format!(
            "{} {}",
            tool_name.as_deref().unwrap_or("result"),
            tool_result_summary(&turn.content)
        ),
        _ => one_line(&turn.content),
    };
    TrajectoryRecord {
        index,
        record_id: turn.id.clone(),
        turn: group,
        kind,
        timestamp_ms: turn.created_at_ms,
        summary: truncate_chars(&summary, SUMMARY_CHARS),
        detail: turn.content.clone(),
        tool_name,
        model: field("model").and_then(Value::as_str).map(str::to_owned),
        usage: field("usage").and_then(|usage| serde_json::from_value(usage.clone()).ok()),
        request: field(REQUEST_METADATA_KEY).and_then(RequestRecord::from_metadata),
        status,
        error,
        cache: None,
        metadata: turn.metadata.clone(),
    }
}

/// A tool result's status and payload without the `{call_id, output, ..}`
/// envelope, e.g. `success · name: zenpi feature: trajectory`.
fn tool_result_summary(content: &str) -> String {
    let Ok(Value::Object(envelope)) = serde_json::from_str::<Value>(content) else {
        return one_line(content);
    };
    let payload = ["error", "output", "result", "content", "text"]
        .iter()
        .find_map(|key| envelope.get(*key).filter(|value| !value.is_null()))
        .map(|value| match value {
            Value::String(text) => one_line(text),
            Value::Object(fields) => fields
                .get("content")
                .or_else(|| fields.get("stdout"))
                .or_else(|| fields.get("text"))
                .and_then(Value::as_str)
                .map_or_else(|| value.to_string(), one_line),
            other => other.to_string(),
        });
    let status = envelope.get("status").and_then(Value::as_str);
    match (status, payload) {
        (Some(status), Some(payload)) => format!("{status} · {payload}"),
        (None, Some(payload)) => payload,
        (Some(status), None) => status.to_owned(),
        (None, None) => one_line(content),
    }
}

/// The parts of turn metadata that reach the provider's wire format.
fn wire_metadata(turn: &Turn) -> Option<Value> {
    let metadata = turn.metadata.as_ref()?;
    if let Some(calls) = metadata.get("tool_calls").and_then(Value::as_array) {
        let calls = calls
            .iter()
            .map(|call| json!([call["id"], call["name"], call["arguments"]]))
            .collect();
        return Some(Value::Array(calls));
    }
    metadata.get("tool_call_id").cloned()
}

fn call_summary(call: &Value) -> String {
    let name = call["name"].as_str().unwrap_or("tool");
    let argument = call["arguments"].as_object().and_then(|arguments| {
        arguments.values().find_map(|value| match value {
            Value::String(text) => Some(one_line(text)),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
    });
    match argument {
        Some(argument) => format!("{name} {}", truncate_chars(&argument, 48)),
        None => name.to_owned(),
    }
}

fn role_label(role: TurnRole) -> &'static str {
    match role {
        TurnRole::System => "system",
        TurnRole::User => "user",
        TurnRole::Assistant => "assistant",
        TurnRole::Tool => "tool",
    }
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn short_digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes)[..6])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}

/// The most recent turn root, i.e. the user turn a running agent is answering.
pub fn open_root(history: &[Turn]) -> Option<&str> {
    history
        .iter()
        .rev()
        .find(|turn| turn.parent_id.is_none() && turn.role == TurnRole::User)
        .map(|turn| turn.id.as_str())
}

/// `950`, `2.5K`, `1.2M`.
pub fn format_tokens(tokens: u64) -> String {
    match tokens {
        0..1_000 => tokens.to_string(),
        1_000..1_000_000 => format!("{:.1}K", tokens as f64 / 1_000.0),
        _ => format!("{:.1}M", tokens as f64 / 1_000_000.0),
    }
}

/// `0.28s`, `12.4s`, `3m05s`, `1h02m`.
pub fn format_duration_ms(milliseconds: u64) -> String {
    let seconds = milliseconds / 1000;
    match milliseconds {
        0..10_000 => format!("{:.2}s", milliseconds as f64 / 1000.0),
        10_000..60_000 => format!("{:.1}s", milliseconds as f64 / 1000.0),
        60_000..3_600_000 => format!("{}m{:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h{:02}m", seconds / 3600, seconds / 60 % 60),
    }
}

/// `00:12:35`, measured from `origin_ms`.
pub fn format_offset_ms(timestamp_ms: u64, origin_ms: u64) -> String {
    let seconds = timestamp_ms.saturating_sub(origin_ms) / 1000;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(id: &str, parent: Option<&str>, role: TurnRole, content: &str, at: u64) -> Turn {
        let mut turn = Turn::new(id, role, content);
        turn.parent_id = parent.map(str::to_owned);
        turn.created_at_ms = at;
        turn
    }

    #[test]
    fn fingerprint_counts_the_shared_prefix() {
        let a = turn("u1", None, TurnRole::User, "hello", 1);
        let b = turn("a1", Some("u1"), TurnRole::Assistant, "hi", 2);
        let c = turn("u2", None, TurnRole::User, "again", 3);
        let first = RequestFingerprint::new("sys", &[], std::slice::from_ref(&a));
        let second = RequestFingerprint::new("sys", &[], &[a.clone(), b.clone(), c]);
        assert_eq!(second.shared_turns(&first), 1);
        let edited = turn("u1", None, TurnRole::User, "hello!", 1);
        let third = RequestFingerprint::new("sys", &[], &[edited, b]);
        assert_eq!(third.shared_turns(&second), 0);
    }

    #[test]
    fn formatting_is_compact() {
        assert_eq!(format_tokens(950), "950");
        assert_eq!(format_tokens(2_500), "2.5K");
        assert_eq!(format_duration_ms(280), "0.28s");
        assert_eq!(format_duration_ms(185_000), "3m05s");
        assert_eq!(format_offset_ms(755_000, 0), "00:12:35");
    }
}
