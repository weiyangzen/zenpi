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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionCheckpoint {
    pub source_start: usize,
    pub source_end: usize,
    pub source_sha256: String,
    pub summary: String,
    pub estimated_tokens_before: u64,
    pub estimated_tokens_after: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedContext {
    pub turns: Vec<Turn>,
    pub checkpoint: Option<CompactionCheckpoint>,
    pub estimate: TokenEstimate,
}

pub fn estimate_tokens(turns: &[Turn]) -> TokenEstimate {
    let bytes = turns
        .iter()
        .map(|turn| turn.content.len() + 16)
        .sum::<usize>();
    TokenEstimate {
        input_tokens: u64::try_from(bytes.div_ceil(4)).unwrap_or(u64::MAX),
        approximate: true,
    }
}

/// Keep system records and the newest conversational suffix. Older records
/// become a deterministic digest summary, so replay of the same journal yields
/// byte-identical provider input without another model call.
pub fn prepare_context(
    turns: &[Turn],
    budget: ContextBudget,
    cancelled: &dyn Fn() -> bool,
) -> Result<PreparedContext, ContextError> {
    if budget.max_tokens <= budget.reserved_output_tokens {
        return Err(ContextError::InvalidBudget);
    }
    let available = budget.max_tokens - budget.reserved_output_tokens;
    let before = estimate_tokens(turns);
    if before.input_tokens <= available {
        return Ok(PreparedContext {
            turns: turns.to_vec(),
            checkpoint: None,
            estimate: before,
        });
    }
    if cancelled() {
        return Err(ContextError::Cancelled);
    }
    let protected_system = turns
        .iter()
        .filter(|turn| turn.role == TurnRole::System)
        .cloned()
        .collect::<Vec<_>>();
    let mut suffix = Vec::new();
    let suffix_budget = available.saturating_mul(3) / 4;
    for turn in turns.iter().rev() {
        if turn.role == TurnRole::System {
            continue;
        }
        suffix.push(turn.clone());
        suffix.reverse();
        let estimate = estimate_tokens(&suffix).input_tokens;
        suffix.reverse();
        if estimate > suffix_budget && suffix.len() > 1 {
            suffix.pop();
            break;
        }
    }
    suffix.reverse();
    let kept_start = suffix
        .first()
        .and_then(|first| turns.iter().position(|turn| turn.id == first.id))
        .unwrap_or(turns.len());
    if kept_start == 0 {
        return Err(ContextError::BudgetExceeded {
            estimated: before.input_tokens,
            available,
        });
    }
    let compacted = &turns[..kept_start];
    let serialized =
        serde_json::to_vec(compacted).map_err(|error| ContextError::Encoding(error.to_string()))?;
    let digest = format!("{:x}", Sha256::digest(&serialized));
    let roles = compacted.iter().fold([0_usize; 4], |mut counts, turn| {
        counts[match turn.role {
            TurnRole::System => 0,
            TurnRole::User => 1,
            TurnRole::Assistant => 2,
            TurnRole::Tool => 3,
        }] += 1;
        counts
    });
    let summary = format!(
        "Compacted context checkpoint: {} records (system={}, user={}, assistant={}, tool={}), sha256={digest}.",
        compacted.len(),
        roles[0],
        roles[1],
        roles[2],
        roles[3]
    );
    let mut summary_turn = Turn::new(format!("context-{digest:.16}"), TurnRole::System, &summary);
    summary_turn.created_at_ms = compacted.last().map_or(0, |turn| turn.created_at_ms);
    summary_turn.metadata = Some(serde_json::json!({
        "context_checkpoint": {
            "source_start": 0,
            "source_end": kept_start,
            "source_sha256": digest,
        }
    }));
    let mut prepared = protected_system;
    prepared.push(summary_turn);
    prepared.extend(suffix);
    let after = estimate_tokens(&prepared);
    if after.input_tokens > available {
        return Err(ContextError::BudgetExceeded {
            estimated: after.input_tokens,
            available,
        });
    }
    Ok(PreparedContext {
        turns: prepared,
        checkpoint: Some(CompactionCheckpoint {
            source_start: 0,
            source_end: kept_start,
            source_sha256: digest,
            summary,
            estimated_tokens_before: before.input_tokens,
            estimated_tokens_after: after.input_tokens,
        }),
        estimate: after,
    })
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
}
