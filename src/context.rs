//! Deterministic context accounting and compaction.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::{Turn, TurnRole};

pub const DEFAULT_CONTEXT_TOKEN_BUDGET: u64 = 128_000;
pub const DEFAULT_RESERVED_OUTPUT_TOKENS: u64 = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBudget {
    pub max_tokens: u64,
    pub reserved_output_tokens: u64,
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            max_tokens: DEFAULT_CONTEXT_TOKEN_BUDGET,
            reserved_output_tokens: DEFAULT_RESERVED_OUTPUT_TOKENS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenEstimate {
    pub input_tokens: u64,
    pub approximate: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedContext {
    pub turns: Vec<Turn>,
    pub estimate: TokenEstimate,
}

pub fn estimate_tokens(turns: &[Turn]) -> TokenEstimate {
    let bytes = turns.iter().fold(0_u64, |total, turn| {
        let metadata = turn.metadata.as_ref().map_or(0, |value| {
            let size = |value: &serde_json::Value| {
                serde_json::to_vec(value).map_or(u64::MAX, |data| data.len() as u64)
            };
            // The trajectory request record is local bookkeeping that is never
            // sent to a provider, so it must not move compaction thresholds.
            let bookkeeping =
                value
                    .get(crate::trajectory::REQUEST_METADATA_KEY)
                    .map_or(0, |request| {
                        let key = crate::trajectory::REQUEST_METADATA_KEY.len() as u64;
                        // `"request":` plus the separating comma.
                        size(request).saturating_add(key + 4)
                    });
            size(value).saturating_sub(bookkeeping)
        });
        total
            .saturating_add(turn.content.len() as u64)
            .saturating_add(16)
            .saturating_add(metadata)
    });
    TokenEstimate {
        input_tokens: bytes.div_ceil(4),
        approximate: true,
    }
}

/// Opaque protocol state must remain byte-for-byte available to its adapter.
/// Native signatures live inside blocks, not just a top-level signature field.
pub fn has_opaque_provider_state(turn: &Turn) -> bool {
    turn.metadata.as_ref().is_some_and(|metadata| {
        metadata.get("signature").is_some()
            || metadata.get("thinking_signature").is_some()
            || metadata["annotations"]
                .as_array()
                .is_some_and(|annotations| {
                    annotations.iter().any(|item| {
                        item["type"] == "native_history" || item.get("signature").is_some()
                    })
                })
    })
}

/// pi cut points: a user turn or a completed assistant turn. A tool result
/// must follow its call, so it never starts the retained tail. Cutting at an
/// assistant inside a turn splits it; the turn prefix is summarized apart.
fn cut_point(turn: &Turn) -> bool {
    turn.role == TurnRole::User
        || (turn.role == TurnRole::Assistant && !failed_context_assistant(turn))
}

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("context budget must reserve fewer tokens than its maximum")]
    InvalidBudget,
    #[error("context preparation was cancelled")]
    Cancelled,
    #[error("context needs approximately {estimated} tokens but only {available} are available")]
    BudgetExceeded { estimated: u64, available: u64 },
    #[error("context encoding failed: {0}")]
    Encoding(String),
    #[error("context checkpoint is invalid or stale")]
    InvalidCheckpoint,
}

/// Version 2 stores a provider-generated summary separately from legacy digest
/// checkpoints. A caller must supply the selected branch's complete ancestry.
pub const SEMANTIC_CHECKPOINT_VERSION: u16 = 2;
pub const MAX_SUMMARY_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointSource {
    pub session_id: String,
    pub branch_id: String,
    pub tip_id: String,
    pub records: usize,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnresolvedContextCall {
    pub turn_id: String,
    pub call_id: String,
    pub tool: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticCheckpoint {
    pub schema_version: u16,
    pub source: CheckpointSource,
    /// Exclusive end of the summarized prefix; always a user or assistant record.
    pub source_end: usize,
    pub covered_sha256: String,
    pub retained_ids: Vec<String>,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
    pub unresolved_calls: Vec<UnresolvedContextCall>,
    pub summary: String,
    pub summary_sha256: String,
    pub summary_provider: String,
    pub summary_model: String,
    pub summary_usage: crate::backend::Usage,
}

/// Immutable preparation. Finalization repeats validation against the journal,
/// so a request that raced a new turn cannot publish a stale checkpoint.
#[derive(Debug, Clone)]
pub struct SemanticCompactionPlan {
    checkpoint: SemanticCheckpoint,
}

fn context_digest(turns: &[Turn]) -> Result<String, ContextError> {
    let bytes = serde_json::to_vec(turns).map_err(|e| ContextError::Encoding(e.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn valid_context_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= crate::protocol::MAX_ID_BYTES
        && !value.chars().any(char::is_control)
}

fn failed_context_assistant(turn: &Turn) -> bool {
    if turn.role != TurnRole::Assistant {
        return false;
    }
    let Some(metadata) = &turn.metadata else {
        return false;
    };
    ["stop_reason", "stopReason", "finish_reason"]
        .iter()
        .any(|key| {
            matches!(
                metadata[*key].as_str(),
                Some("error" | "aborted" | "deferred" | "cancelled")
            )
        })
        || metadata["annotations"]
            .as_array()
            .is_some_and(|annotations| {
                annotations.iter().any(|a| {
                    matches!(
                        a["finish_reason"].as_str(),
                        Some("error" | "aborted" | "deferred" | "cancelled")
                    )
                })
            })
}

/// Validate a full ancestry and a proposed cut without looking at model text.
/// Calls are keyed by user-turn ID as providers may reuse call IDs in later turns.
fn checkpoint_skeleton(
    turns: &[Turn],
    session_id: &str,
    branch_id: &str,
    end: usize,
) -> Result<SemanticCheckpoint, ContextError> {
    use std::collections::{BTreeMap, BTreeSet};
    if !valid_context_id(session_id)
        || !valid_context_id(branch_id)
        || end == 0
        || end >= turns.len()
        || !cut_point(&turns[end])
        || !turns[..end]
            .iter()
            .any(|turn| turn.role != TurnRole::System)
    {
        return Err(ContextError::InvalidCheckpoint);
    }
    let mut ids = BTreeSet::new();
    let mut calls = BTreeMap::<(String, String), (usize, String, Option<usize>, bool)>::new();
    let mut paths = BTreeMap::new();
    let mut reads = BTreeSet::new();
    let mut writes = BTreeSet::new();
    for (index, turn) in turns.iter().enumerate() {
        if !valid_context_id(&turn.id)
            || ids.contains(&turn.id)
            || turn.parent_id.as_ref().is_some_and(|id| !ids.contains(id))
        {
            return Err(ContextError::InvalidCheckpoint);
        }
        ids.insert(turn.id.clone());
        let Some(meta) = turn.metadata.as_ref() else {
            if turn.role == TurnRole::Tool {
                return Err(ContextError::InvalidCheckpoint);
            }
            continue;
        };
        if let Some(raw) = meta.get("tool_calls") {
            if turn.role != TurnRole::Assistant || failed_context_assistant(turn) {
                return Err(ContextError::InvalidCheckpoint);
            }
            let parent = turn
                .parent_id
                .as_ref()
                .ok_or(ContextError::InvalidCheckpoint)?;
            for call in raw.as_array().ok_or(ContextError::InvalidCheckpoint)? {
                let id = call["id"]
                    .as_str()
                    .filter(|s| valid_context_id(s))
                    .ok_or(ContextError::InvalidCheckpoint)?;
                let name = call["name"]
                    .as_str()
                    .filter(|s| valid_context_id(s))
                    .ok_or(ContextError::InvalidCheckpoint)?;
                if !call["arguments"].is_object()
                    || calls
                        .insert(
                            (parent.clone(), id.into()),
                            (index, name.into(), None, false),
                        )
                        .is_some()
                {
                    return Err(ContextError::InvalidCheckpoint);
                }
                if let Some(path) = call["arguments"]["path"].as_str() {
                    paths.insert((parent.clone(), id.to_owned()), path.to_owned());
                }
            }
        }
        if turn.role == TurnRole::Tool {
            let parent = turn
                .parent_id
                .as_ref()
                .ok_or(ContextError::InvalidCheckpoint)?;
            let id = meta["tool_call_id"]
                .as_str()
                .ok_or(ContextError::InvalidCheckpoint)?;
            let call = calls
                .get_mut(&(parent.clone(), id.into()))
                .ok_or(ContextError::InvalidCheckpoint)?;
            if call.2.is_some() || meta["tool_name"].as_str().is_some_and(|n| n != call.1) {
                return Err(ContextError::InvalidCheckpoint);
            }
            call.2 = Some(index);
            call.3 = meta["outcome"] == "unknown_outcome";
            if index < end
                && meta["outcome"] == "succeeded"
                && let Some(path) = paths.get(&(parent.clone(), id.to_owned()))
            {
                if matches!(
                    call.1.as_str(),
                    "read_file" | "list_directory" | "search_text"
                ) {
                    reads.insert(path.clone());
                }
                if matches!(call.1.as_str(), "write_file" | "edit_file") {
                    writes.insert(path.clone());
                }
            }
        }
    }
    let mut unresolved = Vec::new();
    for ((turn_id, call_id), (start, tool, result, unknown)) in calls {
        if start < end && (result.is_none_or(|index| index >= end) || unknown) {
            return Err(ContextError::InvalidCheckpoint);
        }
        if result.is_none() || unknown {
            unresolved.push(UnresolvedContextCall {
                turn_id,
                call_id,
                tool,
            });
        }
    }
    Ok(SemanticCheckpoint {
        schema_version: SEMANTIC_CHECKPOINT_VERSION,
        source: CheckpointSource {
            session_id: session_id.into(),
            branch_id: branch_id.into(),
            tip_id: turns.last().unwrap().id.clone(),
            records: turns.len(),
            sha256: context_digest(turns)?,
        },
        source_end: end,
        covered_sha256: context_digest(&turns[..end])?,
        retained_ids: turns[end..].iter().map(|t| t.id.clone()).collect(),
        read_files: reads.into_iter().collect(),
        modified_files: writes.into_iter().collect(),
        unresolved_calls: unresolved,
        summary: String::new(),
        summary_sha256: String::new(),
        summary_provider: String::new(),
        summary_model: String::new(),
        summary_usage: crate::backend::Usage::default(),
    })
}

pub fn prepare_semantic_compaction(
    turns: &[Turn],
    session_id: &str,
    branch_id: &str,
    keep_recent_tokens: u64,
    cancelled: &dyn Fn() -> bool,
) -> Result<SemanticCompactionPlan, ContextError> {
    if cancelled() {
        return Err(ContextError::Cancelled);
    }
    // pi `findCutPoint`: the newest cut point whose retained tail reaches the
    // desired size. Never split an assistant call from any of its tool results.
    for end in (1..turns.len()).rev().filter(|i| cut_point(&turns[*i])) {
        if cancelled() {
            return Err(ContextError::Cancelled);
        }
        if estimate_tokens(&turns[end..]).input_tokens >= keep_recent_tokens
            && let Ok(checkpoint) = checkpoint_skeleton(turns, session_id, branch_id, end)
        {
            return Ok(SemanticCompactionPlan { checkpoint });
        }
    }
    Err(ContextError::InvalidCheckpoint)
}

impl SemanticCompactionPlan {
    pub fn source_end(&self) -> usize {
        self.checkpoint.source_end
    }
    pub fn source(&self) -> &CheckpointSource {
        &self.checkpoint.source
    }

    /// The host supplies actual provider usage; generating the summary belongs
    /// to the same budget/cancellation owner as other provider requests.
    // Preserve the public 103 API: source, provider usage, budget and cancellation
    // are deliberately independent owner inputs, not an unchecked option bag.
    #[allow(clippy::too_many_arguments)]
    pub fn finalize(
        &self,
        turns: &[Turn],
        summary: String,
        provider: String,
        model: String,
        usage: crate::backend::Usage,
        budget: ContextBudget,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<SemanticCheckpoint, ContextError> {
        if cancelled() {
            return Err(ContextError::Cancelled);
        }
        let mut checkpoint = checkpoint_skeleton(
            turns,
            &self.checkpoint.source.session_id,
            &self.checkpoint.source.branch_id,
            self.checkpoint.source_end,
        )?;
        if checkpoint != self.checkpoint {
            return Err(ContextError::InvalidCheckpoint);
        }
        checkpoint.summary_sha256 = format!("{:x}", Sha256::digest(summary.as_bytes()));
        checkpoint.summary = summary;
        checkpoint.summary_provider = provider;
        checkpoint.summary_model = model;
        checkpoint.summary_usage = usage;
        restore_semantic_checkpoint(
            turns,
            &checkpoint,
            &checkpoint.source.session_id,
            &checkpoint.source.branch_id,
            budget,
        )?;
        if cancelled() {
            return Err(ContextError::Cancelled);
        }
        Ok(checkpoint)
    }
}

/// Restore only the matching branch and immutable source prefix; additional
/// turns appended after the checkpoint are retained without a second request.
pub fn restore_semantic_checkpoint(
    turns: &[Turn],
    checkpoint: &SemanticCheckpoint,
    session_id: &str,
    branch_id: &str,
    budget: ContextBudget,
) -> Result<Vec<Turn>, ContextError> {
    if budget.max_tokens <= budget.reserved_output_tokens {
        return Err(ContextError::InvalidBudget);
    }
    if checkpoint.schema_version != SEMANTIC_CHECKPOINT_VERSION
        || checkpoint.source.session_id != session_id
        || checkpoint.source.branch_id != branch_id
        || checkpoint.source.records > turns.len()
        || checkpoint.summary.trim().is_empty()
        || checkpoint.summary.len() > MAX_SUMMARY_BYTES
        || !valid_context_id(&checkpoint.summary_provider)
        || !valid_context_id(&checkpoint.summary_model)
        || checkpoint.summary_sha256
            != format!("{:x}", Sha256::digest(checkpoint.summary.as_bytes()))
    {
        return Err(ContextError::InvalidCheckpoint);
    }
    let mut expected = checkpoint_skeleton(
        &turns[..checkpoint.source.records],
        session_id,
        branch_id,
        checkpoint.source_end,
    )?;
    expected.summary = checkpoint.summary.clone();
    expected.summary_sha256 = checkpoint.summary_sha256.clone();
    expected.summary_provider = checkpoint.summary_provider.clone();
    expected.summary_model = checkpoint.summary_model.clone();
    expected.summary_usage = checkpoint.summary_usage;
    if &expected != checkpoint {
        return Err(ContextError::InvalidCheckpoint);
    }
    // Validate new records too, including parent and tool call/result pairing.
    checkpoint_skeleton(turns, session_id, branch_id, checkpoint.source_end)?;
    let mut output: Vec<Turn> = turns[..checkpoint.source_end]
        .iter()
        .filter(|t| t.role == TurnRole::System)
        .cloned()
        .collect();
    // A split turn keeps an assistant first; its summary then speaks as the
    // user (pi `compactionSummary`) so every wire still opens with a user turn.
    let role = if turns[checkpoint.source_end].role == TurnRole::User {
        TurnRole::System
    } else {
        TurnRole::User
    };
    let mut summary = Turn::new(
        format!("semantic-{}", &checkpoint.summary_sha256[..24]),
        role,
        format!(
            "{}{}",
            checkpoint.summary,
            file_operations(&checkpoint.read_files, &checkpoint.modified_files)
        ),
    );
    summary.created_at_ms = turns[checkpoint.source_end - 1].created_at_ms;
    summary.metadata = Some(
        serde_json::json!({"semantic_checkpoint": {"schema_version": SEMANTIC_CHECKPOINT_VERSION, "source": checkpoint.source, "read_files": checkpoint.read_files, "modified_files": checkpoint.modified_files, "unresolved_calls": checkpoint.unresolved_calls}}),
    );
    output.push(summary);
    output.extend(
        turns[checkpoint.source_end..]
            .iter()
            .filter(|t| !failed_context_assistant(t))
            .cloned(),
    );
    let estimated = estimate_tokens(&output).input_tokens;
    let available = budget.max_tokens - budget.reserved_output_tokens;
    if estimated > available {
        return Err(ContextError::BudgetExceeded {
            estimated,
            available,
        });
    }
    Ok(output)
}

/// pi `estimateContextTokens`: the newest usable real provider usage anchors
/// the count and only the turns after it are estimated. Without an anchor the
/// whole context is estimated and `overhead` (instructions) is added.
pub fn estimate_context_tokens(
    turns: &[Turn],
    overhead: u64,
    usable: &dyn Fn(&Turn) -> bool,
) -> u64 {
    for (index, turn) in turns.iter().enumerate().rev() {
        if turn.role != TurnRole::Assistant || failed_context_assistant(turn) || !usable(turn) {
            continue;
        }
        let Some(usage) = turn
            .metadata
            .as_ref()
            .and_then(|m| m.get("usage"))
            .and_then(|u| serde_json::from_value::<crate::backend::Usage>(u.clone()).ok())
        else {
            continue;
        };
        // `input_tokens` already includes cached prompt tokens.
        let used = if usage.total_tokens > 0 {
            usage.total_tokens
        } else {
            usage.input_tokens.saturating_add(usage.output_tokens)
        };
        return used.saturating_add(estimate_tokens(&turns[index + 1..]).input_tokens);
    }
    estimate_tokens(turns).input_tokens.saturating_add(overhead)
}

/// Tool results are cut to this many characters in summary input (pi).
pub const TOOL_RESULT_MAX_CHARS: usize = 2000;

pub const SUMMARY_INSTRUCTIONS: &str = "You are a context summarization assistant. Your task is to read a conversation between a user and an AI coding assistant, then produce a structured summary following the exact format specified.\n\nDo NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY output the structured summary.";

const SUMMARY_FORMAT: &str = "## Goal\n[What is the user trying to accomplish? Can be multiple items if the session covers different tasks.]\n\n## Constraints & Preferences\n- [Any constraints, preferences, or requirements mentioned by user]\n- [Or \"(none)\" if none were mentioned]\n\n## Progress\n### Done\n- [x] [Completed tasks/changes]\n\n### In Progress\n- [ ] [Current work]\n\n### Blocked\n- [Issues preventing progress, if any]\n\n## Key Decisions\n- **[Decision]**: [Brief rationale]\n\n## Next Steps\n1. [Ordered list of what should happen next]\n\n## Critical Context\n- [Any data, examples, or references needed to continue]\n- [Or \"(none)\" if not applicable]\n\nKeep each section concise. Preserve exact file paths, function names, and error messages.";

const SUMMARY_PROMPT: &str = "The messages above are a conversation to summarize. Create a structured context checkpoint summary that another LLM will use to continue the work.\n\nUse this EXACT format:\n\n";

const UPDATE_SUMMARY_PROMPT: &str = "The messages above are NEW conversation messages to incorporate into the existing summary provided in <previous-summary> tags.\n\nUpdate the existing structured summary with new information. RULES:\n- PRESERVE all existing information from the previous summary\n- ADD new progress, decisions, and context from the new messages\n- UPDATE the Progress section: move items from \"In Progress\" to \"Done\" when completed\n- UPDATE \"Next Steps\" based on what was accomplished\n- PRESERVE exact file paths, function names, and error messages\n- If something is no longer relevant, you may remove it\n\nUse this EXACT format:\n\n";

const TURN_PREFIX_PROMPT: &str = "This is the PREFIX of a turn that was too large to keep. The SUFFIX (recent work) is retained.\n\nSummarize the prefix to provide context for the retained suffix:\n\n## Original Request\n[What did the user ask for in this turn?]\n\n## Early Progress\n- [Key decisions and work done in the prefix]\n\n## Context for Suffix\n- [Information needed to understand the retained recent work]\n\nBe concise. Focus on what's needed to understand the kept suffix.";

/// Role-labelled text, so the model summarizes rather than continues the
/// conversation. Metadata, signatures and opaque reasoning are dropped; only
/// visible text and tool calls remain, and tool results are truncated.
pub fn serialize_conversation(turns: &[Turn]) -> String {
    let mut parts = Vec::new();
    for turn in turns.iter().filter(|turn| !failed_context_assistant(turn)) {
        if turn.content.is_empty() && turn.role != TurnRole::Assistant {
            continue;
        }
        match turn.role {
            TurnRole::System => {}
            TurnRole::User => parts.push(format!("[User]: {}", turn.content)),
            TurnRole::Assistant => {
                if !turn.content.is_empty() {
                    parts.push(format!("[Assistant]: {}", turn.content));
                }
                let calls: Vec<String> = turn
                    .metadata
                    .as_ref()
                    .and_then(|m| m["tool_calls"].as_array())
                    .into_iter()
                    .flatten()
                    .map(|call| {
                        let arguments = call["arguments"]
                            .as_object()
                            .into_iter()
                            .flatten()
                            .map(|(key, value)| format!("{key}={value}"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{}({arguments})", call["name"].as_str().unwrap_or(""))
                    })
                    .collect();
                if !calls.is_empty() {
                    parts.push(format!("[Assistant tool calls]: {}", calls.join("; ")));
                }
            }
            TurnRole::Tool => {
                let total = turn.content.chars().count();
                let mut text: String = turn.content.chars().take(TOOL_RESULT_MAX_CHARS).collect();
                if total > TOOL_RESULT_MAX_CHARS {
                    text.push_str(&format!(
                        "\n\n[... {} more characters truncated]",
                        total - TOOL_RESULT_MAX_CHARS
                    ));
                }
                parts.push(format!("[Tool result]: {text}"));
            }
        }
    }
    parts.join("\n\n")
}

fn file_operations(read: &[String], modified: &[String]) -> String {
    let sections: Vec<String> = [("read-files", read), ("modified-files", modified)]
        .into_iter()
        .filter(|(_, files)| !files.is_empty())
        .map(|(tag, files)| format!("<{tag}>\n{}\n</{tag}>", files.join("\n")))
        .collect();
    if sections.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", sections.join("\n\n"))
    }
}

/// The user-message texts of one compaction: the history request and, for a
/// split turn, the turn-prefix request. They are sent one after the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryRequests {
    /// `None` when nothing new precedes a split turn.
    pub history: Option<String>,
    pub turn_prefix: Option<String>,
    /// Stands in for the history summary when `history` is `None`.
    pub carried_summary: String,
}

/// pi `compact`: one summary text, with the split-turn context appended.
pub fn merge_summaries(history: &str, turn_prefix: Option<&str>) -> String {
    match turn_prefix {
        Some(prefix) => format!("{history}\n\n---\n\n**Turn Context (split turn):**\n\n{prefix}"),
        None => history.to_owned(),
    }
}

impl SemanticCompactionPlan {
    /// A subsequent summary updates the earlier summary with only newly covered
    /// records; the commit remains bound to the complete original ancestry.
    pub fn summary_request_data(
        &self,
        turns: &[Turn],
        previous: Option<&SemanticCheckpoint>,
    ) -> Result<SummaryRequests, ContextError> {
        let expected = checkpoint_skeleton(
            turns,
            &self.checkpoint.source.session_id,
            &self.checkpoint.source.branch_id,
            self.checkpoint.source_end,
        )?;
        if expected != self.checkpoint {
            return Err(ContextError::InvalidCheckpoint);
        }
        let start = if let Some(previous) = previous {
            restore_semantic_checkpoint(
                turns,
                previous,
                &self.checkpoint.source.session_id,
                &self.checkpoint.source.branch_id,
                ContextBudget {
                    max_tokens: u64::MAX,
                    reserved_output_tokens: 0,
                },
            )?;
            if previous.source_end >= self.source_end() {
                return Err(ContextError::InvalidCheckpoint);
            }
            previous.source_end
        } else {
            0
        };
        let end = self.source_end();
        // pi `findTurnStartIndex`: cutting at an assistant splits the turn
        // that began at the nearest earlier user record.
        let turn_start = (turns[end].role != TurnRole::User)
            .then(|| {
                turns[start..end]
                    .iter()
                    .rposition(|t| t.role == TurnRole::User)
            })
            .flatten()
            .map(|offset| start + offset);
        let history_end = turn_start.unwrap_or(end);
        let previous = previous.map(|checkpoint| checkpoint.summary.as_str());
        let history = (history_end > start).then(|| {
            let conversation = serialize_conversation(&turns[start..history_end]);
            let prompt = match previous {
                Some(previous) => format!(
                    "<previous-summary>\n{previous}\n</previous-summary>\n\n{UPDATE_SUMMARY_PROMPT}"
                ),
                None => SUMMARY_PROMPT.to_owned(),
            };
            format!("<conversation>\n{conversation}\n</conversation>\n\n{prompt}{SUMMARY_FORMAT}")
        });
        let turn_prefix = turn_start.map(|turn_start| {
            format!(
                "<conversation>\n{}\n</conversation>\n\n{TURN_PREFIX_PROMPT}",
                serialize_conversation(&turns[turn_start..end])
            )
        });
        Ok(SummaryRequests {
            history,
            turn_prefix,
            carried_summary: previous.unwrap_or("No prior history.").to_owned(),
        })
    }

    /// Validate a complete provider reply before attempting the journal commit.
    /// The summary is free text (pi); low-level adapters also reject wire
    /// truncation, and custom Backend impls cannot bypass the owner with a
    /// length annotation or missing usage.
    pub fn validate_summary_completion(
        &self,
        completion: &crate::backend::Completion,
        output_limit: u64,
    ) -> Result<(String, crate::backend::Usage), ContextError> {
        let usage = completion.usage.ok_or(ContextError::InvalidCheckpoint)?;
        if completion.refusal.is_some()
            || !completion.tool_calls.is_empty()
            || completion.content.trim().is_empty()
            || completion.content.len() > MAX_SUMMARY_BYTES
            || usage.input_tokens == 0
            || usage.output_tokens == 0
            || usage.output_tokens > output_limit
            || usage
                .input_tokens
                .checked_add(usage.output_tokens)
                .is_none_or(|n| usage.total_tokens < n)
            || completion.annotations.iter().any(|value| {
                value["truncated"] == true
                    || ["finish_reason", "stop_reason", "stopReason", "status"]
                        .iter()
                        .any(|key| {
                            matches!(
                                value[*key].as_str(),
                                Some(
                                    "length"
                                        | "max_tokens"
                                        | "error"
                                        | "aborted"
                                        | "deferred"
                                        | "cancelled"
                                        | "incomplete"
                                )
                            )
                        })
            })
        {
            return Err(ContextError::InvalidCheckpoint);
        }
        Ok((completion.content.clone(), usage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_usage_anchors_the_trigger_and_only_later_turns_are_estimated() {
        let mut answered = Turn::new("a", TurnRole::Assistant, "ok");
        answered.metadata = Some(serde_json::json!({"usage":{
            "input_tokens":9000,"output_tokens":100,"total_tokens":9100}}));
        let later = Turn::new("u", TurnRole::User, "x".repeat(400));
        let turns = [
            Turn::new("q", TurnRole::User, "hi"),
            answered,
            later.clone(),
        ];
        let tail = estimate_tokens(std::slice::from_ref(&later)).input_tokens;
        assert_eq!(estimate_context_tokens(&turns, 7, &|_| true), 9100 + tail);
        // Stale usage (before the latest compaction) is not an anchor.
        assert_eq!(
            estimate_context_tokens(&turns, 7, &|t| t.id != "a"),
            estimate_tokens(&turns).input_tokens + 7
        );
    }

    #[test]
    fn summary_input_is_role_labelled_text_with_truncated_tool_results() {
        let mut call = Turn::new("c", TurnRole::Assistant, "");
        call.metadata = Some(serde_json::json!({"tool_calls":[
            {"id":"1","name":"read_file","arguments":{"path":"a.rs"}}],
            "annotations":[{"signature":"opaque"}]}));
        let result = Turn::new("r", TurnRole::Tool, "y".repeat(TOOL_RESULT_MAX_CHARS + 5));
        let text = serialize_conversation(&[Turn::new("u", TurnRole::User, "go"), call, result]);
        assert!(text.starts_with("[User]: go\n\n[Assistant tool calls]: read_file(path=\"a.rs\")"));
        assert!(text.ends_with("\n\n[... 5 more characters truncated]"));
        assert!(!text.contains("opaque"));
    }
}
