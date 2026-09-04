//! Inert `/compete` and `/loop` handoff owner.
//!
//! Both public modes call this module. It validates a small command grammar,
//! constructs bounded b3 records, and appends them to the current session.
//! No scheduler, child process, network request, or provider turn is started.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    b3::{
        EstimatorPolicy, MAX_RUNTIME_ATTEMPTS, MAX_RUNTIME_DISK_BYTES, MAX_RUNTIME_TOKENS,
        MAX_RUNTIME_WALL_CLOCK_MS, ParentLeaseRef, ResourceBudget, ResourceEnvelope, RouteDecision,
        RuntimeIntent, RuntimeIntentKind,
    },
    core::{Agent, AgentError},
    session::unix_time_ms,
};

pub const MAX_RUNTIME_INTENTS_VIEW: usize = 64;

const DEFAULT_COMPETE_TOKENS: u64 = 400_000;
const DEFAULT_LOOP_TOKENS: u64 = 250_000;
const DEFAULT_WALL_CLOCK_MS: u64 = 60 * 60 * 1_000;
const DEFAULT_DISK_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum RuntimeIntentError {
    #[error("runtime intent requires an action or bounded task")]
    MissingAction,
    #[error("runtime status does not accept extra arguments")]
    StatusArguments,
    #[error("runtime parent `{0}` is not a valid bounded identifier")]
    InvalidParent(String),
    #[error("runtime lease requires both --parent-lease and --nested-run")]
    IncompleteParentLease,
    #[error("runtime option `{0}` requires a value")]
    MissingOptionValue(String),
    #[error("runtime option `{option}` has an invalid numeric value: {value}")]
    InvalidNumber { option: String, value: String },
    #[error("runtime option `{0}` is not supported")]
    UnsupportedOption(String),
    #[error("runtime intent failed: {0}")]
    Agent(#[from] AgentError),
    #[error("runtime intent failed: {0}")]
    B3(#[from] crate::b3::B3Error),
}

#[derive(Debug, Clone)]
struct SubmitOptions {
    parent_ref: String,
    parent_lease_id: Option<String>,
    nested_run_id: Option<String>,
    tokens: u64,
    wall_clock_ms: u64,
    attempts: u32,
    disk_bytes: u64,
    forwarded: Vec<String>,
}

impl SubmitOptions {
    fn defaults(kind: RuntimeIntentKind) -> Self {
        Self {
            parent_ref: "standalone".into(),
            parent_lease_id: None,
            nested_run_id: None,
            tokens: match kind {
                RuntimeIntentKind::Compete => DEFAULT_COMPETE_TOKENS,
                RuntimeIntentKind::Loop => DEFAULT_LOOP_TOKENS,
            },
            wall_clock_ms: DEFAULT_WALL_CLOCK_MS,
            attempts: 1,
            disk_bytes: DEFAULT_DISK_BYTES,
            forwarded: Vec::new(),
        }
    }
}

/// Execute one external-runtime owner action and return the shared TUI/headless
/// projection. `status` is read-only; every other non-empty form creates one
/// durable typed intent and reports `started:false` truthfully.
pub fn runtime_intent_value(
    agent: &mut Agent,
    kind: RuntimeIntentKind,
    args: &[String],
) -> Result<Value, RuntimeIntentError> {
    let (action, remaining) = split_action(args)?;
    if action == "status" {
        if !remaining.is_empty() {
            return Err(RuntimeIntentError::StatusArguments);
        }
        return Ok(status_value(agent, kind));
    }

    let options = parse_submit_options(kind, remaining)?;
    let ordinal = agent.session().next_sequence();
    let session_id = agent.session().session_id().to_owned();
    let id_prefix = kind.as_str();
    let intent_id = format!("{id_prefix}-intent-{ordinal}");
    let route_id = format!("{id_prefix}-route-{ordinal}");
    let envelope_id = format!("{id_prefix}-envelope-{ordinal}");
    let budget = ResourceBudget {
        tokens: options.tokens,
        wall_clock_ms: options.wall_clock_ms,
        attempts: options.attempts,
        disk_bytes: options.disk_bytes,
    };
    let route = RouteDecision {
        route_id,
        parent_ref: options.parent_ref.clone(),
        route_class: format!("external_{id_prefix}"),
        runner: "external_b3ehive".into(),
        validator_strength: "host_selected".into(),
    };
    let envelope = ResourceEnvelope::new(envelope_id, id_prefix, budget)?;
    let parent_lease = match (options.parent_lease_id, options.nested_run_id) {
        (Some(parent_lease_id), Some(nested_run_id)) => Some(ParentLeaseRef {
            parent_lease_id,
            nested_run_id,
            max_tokens: budget.tokens,
        }),
        (None, None) => None,
        _ => return Err(RuntimeIntentError::IncompleteParentLease),
    };
    let intent = RuntimeIntent::new(
        intent_id,
        kind,
        options.parent_ref,
        options.forwarded,
        route,
        envelope,
        parent_lease,
        session_id,
        unix_time_ms(),
    )?;

    // Validate the companion estimate before mutating the journal. It is
    // returned with the intent so an external owner can retain or enrich it.
    let estimator = estimator_for(&intent)?;
    agent.append_runtime_intent(intent.clone())?;
    Ok(json!({
        "command": kind.as_str(),
        "route": "runtime_intent",
        "action": action,
        "accepted": true,
        "persisted": true,
        "durable": true,
        "delivery": "journal_only",
        "zenpi_started": false,
        "execution_state": "untracked",
        "intent": intent,
        "estimator": estimator,
        "next_sequence": agent.session().next_sequence(),
        "message": "runtime intent persisted for an external b3ehive owner",
    }))
}

fn split_action(args: &[String]) -> Result<(&str, &[String]), RuntimeIntentError> {
    let Some(first) = args.first() else {
        return Err(RuntimeIntentError::MissingAction);
    };
    if first.eq_ignore_ascii_case("status") {
        Ok(("status", &args[1..]))
    } else if first.eq_ignore_ascii_case("submit") || first.eq_ignore_ascii_case("start") {
        if args.len() == 1 {
            Err(RuntimeIntentError::MissingAction)
        } else {
            Ok(("submit", &args[1..]))
        }
    } else {
        // Preserve the original compact grammar: `/compete audit this` and
        // `/loop repair ITEM-1` are submit aliases rather than model prompts.
        Ok(("submit", args))
    }
}

fn parse_submit_options(
    kind: RuntimeIntentKind,
    args: &[String],
) -> Result<SubmitOptions, RuntimeIntentError> {
    let mut options = SubmitOptions::defaults(kind);
    let mut index = 0;
    let mut options_enabled = true;
    while index < args.len() {
        let argument = &args[index];
        if argument == "--" && options_enabled {
            options_enabled = false;
            index += 1;
            continue;
        }
        if !options_enabled || !argument.starts_with("--") {
            options.forwarded.push(argument.clone());
            index += 1;
            continue;
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| RuntimeIntentError::MissingOptionValue(argument.clone()))?;
        if value.starts_with("--") {
            return Err(RuntimeIntentError::MissingOptionValue(argument.clone()));
        }
        match argument.as_str() {
            "--parent" => options.parent_ref = bounded_id(value)?,
            "--parent-lease" => options.parent_lease_id = Some(bounded_id(value)?),
            "--nested-run" => options.nested_run_id = Some(bounded_id(value)?),
            "--tokens" => options.tokens = parse_u64(argument, value, MAX_RUNTIME_TOKENS)?,
            "--wall-ms" => {
                options.wall_clock_ms = parse_u64(argument, value, MAX_RUNTIME_WALL_CLOCK_MS)?
            }
            "--attempts" => options.attempts = parse_u32(argument, value, MAX_RUNTIME_ATTEMPTS)?,
            "--disk-bytes" => {
                options.disk_bytes = parse_u64(argument, value, MAX_RUNTIME_DISK_BYTES)?
            }
            _ => return Err(RuntimeIntentError::UnsupportedOption(argument.clone())),
        }
        index += 2;
    }
    if options.forwarded.is_empty() {
        return Err(RuntimeIntentError::MissingAction);
    }
    if options.parent_lease_id.is_some() != options.nested_run_id.is_some() {
        return Err(RuntimeIntentError::IncompleteParentLease);
    }
    Ok(options)
}

fn bounded_id(value: &str) -> Result<String, RuntimeIntentError> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._:/-".contains(character))
    {
        return Err(RuntimeIntentError::InvalidParent(value.to_owned()));
    }
    Ok(value.to_owned())
}

fn parse_u64(option: &str, value: &str, maximum: u64) -> Result<u64, RuntimeIntentError> {
    value
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0 && *value <= maximum)
        .ok_or_else(|| RuntimeIntentError::InvalidNumber {
            option: option.to_owned(),
            value: value.to_owned(),
        })
}

fn parse_u32(option: &str, value: &str, maximum: u32) -> Result<u32, RuntimeIntentError> {
    value
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0 && *value <= maximum)
        .ok_or_else(|| RuntimeIntentError::InvalidNumber {
            option: option.to_owned(),
            value: value.to_owned(),
        })
}

fn estimator_for(intent: &RuntimeIntent) -> Result<EstimatorPolicy, crate::b3::B3Error> {
    let mut parameters = BTreeMap::new();
    parameters.insert("argument_count".into(), intent.args.len() as u64);
    let estimator = EstimatorPolicy {
        estimate_id: format!("{}-estimate", intent.intent_id),
        task_ref: intent.parent_ref.clone(),
        estimated_parameters: parameters,
        hard_caps: intent.envelope.limit,
        rationale: "operator-supplied bounds; external runtime owns route refinement".into(),
    };
    estimator.validate()?;
    Ok(estimator)
}

fn status_value(agent: &Agent, kind: RuntimeIntentKind) -> Value {
    let matching = agent
        .session()
        .runtime_intents()
        .iter()
        .filter(|intent| intent.kind == kind)
        .rev()
        .take(MAX_RUNTIME_INTENTS_VIEW)
        .cloned()
        .collect::<Vec<_>>();
    let total = agent
        .session()
        .runtime_intents()
        .iter()
        .filter(|intent| intent.kind == kind)
        .count();
    let mut intents = matching;
    intents.reverse();
    let latest_intent_id = intents.last().map(|intent| intent.intent_id.clone());
    json!({
        "command": kind.as_str(),
        "route": "runtime_intent",
        "action": "status",
        "accepted": true,
        "records_durable": true,
        "delivery": "journal_only",
        "zenpi_started": false,
        "execution_state": "untracked",
        "stored_count": total,
        "intents": intents,
        "truncated": total > MAX_RUNTIME_INTENTS_VIEW,
        "latest_intent_id": latest_intent_id,
        "message": "runtime intents are pending external ownership",
    })
}
