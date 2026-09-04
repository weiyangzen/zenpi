//! Explicit approval policy primitives shared by headless and interactive hosts.
//!
//! A tool invocation is never allowed to infer approval from its name or from
//! a provider response.  Hosts receive a stable request, make a local policy
//! decision, and can persist the decision without retaining credentials.

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::tools::ToolSideEffect;

pub const MAX_APPROVAL_ID_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    /// Every side-effecting call must receive an explicit response.
    #[default]
    Always,
    /// Read-only calls are automatic; writes and commands still prompt.
    ReadOnly,
    /// Allow calls only for tools explicitly listed as trusted.
    TrustedWorkspace,
    /// Apply a separate decision for each tool name.
    PerTool,
    /// Never prompt; deny anything not already allowed by the host.
    Headless,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub request_id: String,
    pub turn_id: String,
    pub call_id: String,
    pub tool: String,
    pub side_effect: ToolSideEffect,
    pub arguments: serde_json::Value,
}

impl ApprovalRequest {
    pub fn validate(&self) -> Result<(), ApprovalError> {
        for (name, value) in [
            ("request_id", self.request_id.as_str()),
            ("turn_id", self.turn_id.as_str()),
            ("call_id", self.call_id.as_str()),
            ("tool", self.tool.as_str()),
        ] {
            if value.trim().is_empty()
                || value.len() > MAX_APPROVAL_ID_BYTES
                || value.chars().any(char::is_control)
            {
                return Err(ApprovalError::Invalid(format!("{name} is invalid")));
            }
        }
        if !self.arguments.is_object() {
            return Err(ApprovalError::Invalid("arguments must be an object".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalResponse {
    pub request_id: String,
    pub decision: ApprovalDecision,
    #[serde(default)]
    pub remember: bool,
}

/// A process-local rendezvous between the agent worker and its host.  Tool
/// execution is synchronous by design, but approval must not make stdin
/// unreadable: the host can drain pending requests and answer them while the
/// worker waits on this bounded condition variable.
#[derive(Debug, Clone)]
pub struct ApprovalCoordinator {
    inner: Arc<(Mutex<CoordinatorState>, Condvar)>,
}

#[derive(Debug, Default)]
struct CoordinatorState {
    pending: BTreeMap<String, ApprovalRequest>,
    visible: BTreeMap<String, ApprovalRequest>,
    decisions: BTreeMap<String, ApprovalResponse>,
}

impl Default for ApprovalCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl ApprovalCoordinator {
    pub fn new() -> Self {
        Self {
            inner: Arc::new((Mutex::new(CoordinatorState::default()), Condvar::new())),
        }
    }

    /// Register a request and wait until the host answers or the turn is
    /// cancelled. Read-only calls should be resolved by [`ApprovalPolicy`]
    /// before reaching this method.
    pub fn request(
        &self,
        request: ApprovalRequest,
        policy: &mut ApprovalPolicy,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ApprovalDecision, ApprovalError> {
        request.validate()?;
        let id = request.request_id.clone();
        let (lock, wake) = &*self.inner;
        let mut state = lock
            .lock()
            .map_err(|_| ApprovalError::Invalid("approval coordinator poisoned".into()))?;
        state.pending.insert(id.clone(), request);
        loop {
            if cancelled() {
                state.pending.remove(&id);
                state.visible.remove(&id);
                state.decisions.remove(&id);
                return Err(ApprovalError::Cancelled);
            }
            if let Some(response) = state.decisions.remove(&id) {
                let tool = state
                    .pending
                    .get(&id)
                    .or_else(|| state.visible.get(&id))
                    .map(|request| request.tool.clone())
                    .unwrap_or_else(|| "unknown".into());
                state.pending.remove(&id);
                state.visible.remove(&id);
                if response.remember {
                    // The caller owns the policy, so remembering a decision
                    // never persists credentials or mutates global state.
                    policy.remember(tool, response.decision);
                }
                return Ok(response.decision);
            }
            let (next, _) = wake
                .wait_timeout(state, Duration::from_millis(50))
                .map_err(|_| ApprovalError::Invalid("approval coordinator poisoned".into()))?;
            state = next;
        }
    }

    /// Return each newly pending request once. Calling this is what marks a
    /// request as visible to a host, preventing a slow renderer from emitting
    /// duplicate approval prompts.
    pub fn drain_pending(&self) -> Vec<ApprovalRequest> {
        let (lock, _) = &*self.inner;
        let Ok(mut state) = lock.lock() else {
            return Vec::new();
        };
        let fresh = state
            .pending
            .iter()
            .filter(|(id, _)| !state.visible.contains_key(*id))
            .map(|(id, request)| (id.clone(), request.clone()))
            .collect::<Vec<_>>();
        let mut result = Vec::with_capacity(fresh.len());
        for (id, request) in fresh {
            state.visible.insert(id, request.clone());
            result.push(request);
        }
        result
    }

    /// Answer one visible request. A response for an unknown request is
    /// rejected instead of being buffered for a future tool call.
    pub fn respond(&self, response: ApprovalResponse) -> Result<(), ApprovalError> {
        if response.request_id.trim().is_empty()
            || response.request_id.len() > MAX_APPROVAL_ID_BYTES
            || response.request_id.chars().any(char::is_control)
        {
            return Err(ApprovalError::Invalid("request_id is invalid".into()));
        }
        let (lock, wake) = &*self.inner;
        let mut state = lock
            .lock()
            .map_err(|_| ApprovalError::Invalid("approval coordinator poisoned".into()))?;
        if !state.pending.contains_key(&response.request_id) {
            return Err(ApprovalError::UnknownRequest);
        }
        state
            .decisions
            .insert(response.request_id.clone(), response);
        wake.notify_all();
        Ok(())
    }

    pub fn cancel_all(&self) {
        let (lock, wake) = &*self.inner;
        if let Ok(mut state) = lock.lock() {
            for request_id in state.pending.keys().cloned().collect::<Vec<_>>() {
                state.decisions.insert(
                    request_id.clone(),
                    ApprovalResponse {
                        request_id,
                        decision: ApprovalDecision::Deny,
                        remember: false,
                    },
                );
            }
            wake.notify_all();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalPolicy {
    pub mode: ApprovalMode,
    #[serde(default)]
    pub trusted_tools: Vec<String>,
    #[serde(default)]
    pub per_tool: BTreeMap<String, ApprovalDecision>,
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self {
            mode: ApprovalMode::Always,
            trusted_tools: Vec::new(),
            per_tool: BTreeMap::new(),
        }
    }
}

impl ApprovalPolicy {
    pub fn decide(&self, side_effect: ToolSideEffect, tool: &str) -> Option<ApprovalDecision> {
        if side_effect == ToolSideEffect::ReadOnly {
            return Some(ApprovalDecision::Allow);
        }
        if let Some(decision) = self.per_tool.get(tool) {
            return Some(*decision);
        }
        match self.mode {
            ApprovalMode::TrustedWorkspace
                if self.trusted_tools.iter().any(|item| item == tool) =>
            {
                Some(ApprovalDecision::Allow)
            }
            ApprovalMode::Headless => Some(ApprovalDecision::Deny),
            _ => None,
        }
    }

    pub fn remember(&mut self, tool: impl Into<String>, decision: ApprovalDecision) {
        self.per_tool.insert(tool.into(), decision);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    #[error("invalid approval request: {0}")]
    Invalid(String),
    #[error("approval response does not match a pending request")]
    UnknownRequest,
    #[error("approval wait was cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use super::{
        ApprovalCoordinator, ApprovalDecision, ApprovalMode, ApprovalPolicy, ApprovalRequest,
        ApprovalResponse,
    };
    use crate::tools::ToolSideEffect;

    #[test]
    fn read_only_is_always_allowed_and_headless_denies_side_effects() {
        let policy = ApprovalPolicy {
            mode: ApprovalMode::Headless,
            ..ApprovalPolicy::default()
        };
        assert_eq!(
            policy.decide(ToolSideEffect::ReadOnly, "read_file"),
            Some(ApprovalDecision::Allow)
        );
        assert_eq!(
            policy.decide(ToolSideEffect::WorkspaceWrite, "write_file"),
            Some(ApprovalDecision::Deny)
        );
    }

    #[test]
    fn coordinator_emits_once_and_correlates_response() {
        let coordinator = ApprovalCoordinator::new();
        let worker = coordinator.clone();
        let join = thread::spawn(move || {
            let mut policy = ApprovalPolicy::default();
            worker.request(
                ApprovalRequest {
                    request_id: "approval-call-1".into(),
                    turn_id: "turn-1".into(),
                    call_id: "call-1".into(),
                    tool: "write_file".into(),
                    side_effect: ToolSideEffect::WorkspaceWrite,
                    arguments: serde_json::json!({"path":"x"}),
                },
                &mut policy,
                &|| false,
            )
        });
        let request = loop {
            let mut pending = coordinator.drain_pending();
            if let Some(request) = pending.pop() {
                break request;
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(request.call_id, "call-1");
        assert!(coordinator.drain_pending().is_empty());
        coordinator
            .respond(ApprovalResponse {
                request_id: request.request_id,
                decision: ApprovalDecision::Allow,
                remember: false,
            })
            .unwrap();
        assert_eq!(join.join().unwrap().unwrap(), ApprovalDecision::Allow);
    }
}
