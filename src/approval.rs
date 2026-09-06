//! Explicit approval policy primitives shared by headless and interactive hosts.
//!
//! A tool invocation is never allowed to infer approval from its name or from
//! a provider response.  Hosts receive a stable request, make a local policy
//! decision, and can persist the decision without retaining credentials.

use std::collections::BTreeMap;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicU64, Ordering},
};
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
    /// Explicit interactive opt-in; Blueprint and sandbox gates still apply.
    Never,
    /// Automatic only after a host binding matches a live per-action gate.
    WorkerAllowAfterPreflight,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub request_id: String,
    pub turn_id: String,
    pub call_id: String,
    pub tool: String,
    pub side_effect: ToolSideEffect,
    pub arguments: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<crate::tools::ToolPreview>,
    #[serde(default)]
    pub origin: crate::tools::ToolOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_id: Option<String>,
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
        if let Some(digest) = &self.policy_digest
            && (digest.len() != 64
                || !digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        {
            return Err(ApprovalError::Invalid("policy digest is invalid".into()));
        }
        if let Some(lease) = &self.lease_id
            && (lease.trim().is_empty()
                || lease.len() > MAX_APPROVAL_ID_BYTES
                || lease.chars().any(char::is_control))
        {
            return Err(ApprovalError::Invalid("lease ID is invalid".into()));
        }
        if self.origin == crate::tools::ToolOrigin::BlueprintWorker
            && (self.policy_digest.is_none() || self.lease_id.is_none())
        {
            return Err(ApprovalError::Invalid(
                "worker approval requires policy and lease correlation".into(),
            ));
        }
        if let Some(preview) = &self.preview {
            preview
                .validate()
                .map_err(|error| ApprovalError::Invalid(error.to_string()))?;
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
    cancellation_epoch: Arc<AtomicU64>,
}

#[derive(Debug, Default)]
struct CoordinatorState {
    pending: BTreeMap<String, ApprovalRequest>,
    visible: BTreeMap<String, ApprovalRequest>,
    decisions: BTreeMap<String, ApprovalResponse>,
    accepted: Vec<AcceptedApproval>,
}

/// One host response accepted by the coordinator.  The worker drains these
/// records into the session journal while it already owns the mutable agent,
/// before it may consume the matching decision or execute a side effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedApproval {
    pub request: ApprovalRequest,
    pub response: ApprovalResponse,
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
            cancellation_epoch: Arc::new(AtomicU64::new(0)),
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
        self.request_inner(request, policy, cancelled, false)
            .map(|response| response.decision)
    }

    /// Register a request and return the complete host response.  The richer
    /// form lets the core durably audit both the decision and whether it was
    /// remembered before a permitted side effect starts.  The response stays
    /// in [`Self::accepted`] until the caller writes that record and invokes
    /// [`Self::mark_persisted`].
    pub fn request_response(
        &self,
        request: ApprovalRequest,
        policy: &mut ApprovalPolicy,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ApprovalResponse, ApprovalError> {
        self.request_inner(request, policy, cancelled, true)
    }

    fn request_inner(
        &self,
        request: ApprovalRequest,
        policy: &mut ApprovalPolicy,
        cancelled: &dyn Fn() -> bool,
        retain_accepted: bool,
    ) -> Result<ApprovalResponse, ApprovalError> {
        request.validate()?;
        let id = request.request_id.clone();
        let tool = request.tool.clone();
        let (lock, wake) = &*self.inner;
        let mut state = lock
            .lock()
            .map_err(|_| ApprovalError::Invalid("approval coordinator poisoned".into()))?;
        if state.pending.contains_key(&id) || state.decisions.contains_key(&id) {
            return Err(ApprovalError::Invalid(
                "request_id is already pending".into(),
            ));
        }
        state.visible.remove(&id);
        state.pending.insert(id.clone(), request);
        loop {
            if cancelled() {
                state.pending.remove(&id);
                state.visible.remove(&id);
                state.decisions.remove(&id);
                state
                    .accepted
                    .retain(|accepted| accepted.response.request_id != id);
                return Err(ApprovalError::Cancelled);
            }
            if let Some(response) = state.decisions.get(&id).cloned() {
                if !retain_accepted {
                    state
                        .accepted
                        .retain(|accepted| accepted.response.request_id != id);
                }
                state.decisions.remove(&id);
                state.pending.remove(&id);
                state.visible.remove(&id);
                if response.remember && !retain_accepted {
                    // The caller owns the policy, so remembering a decision
                    // never persists credentials or mutates global state.
                    policy.remember(tool.clone(), response.decision);
                }
                return Ok(response);
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

    /// Mark one request as visible and return it without consuming unrelated
    /// pending prompts.  Slash owners use this before responding so their
    /// behavior matches hosts that first emitted the request to a client.
    pub fn reveal(&self, request_id: &str) -> Result<ApprovalRequest, ApprovalError> {
        if request_id.trim().is_empty()
            || request_id.len() > MAX_APPROVAL_ID_BYTES
            || request_id.chars().any(char::is_control)
        {
            return Err(ApprovalError::Invalid("request_id is invalid".into()));
        }
        let (lock, _) = &*self.inner;
        let mut state = lock
            .lock()
            .map_err(|_| ApprovalError::Invalid("approval coordinator poisoned".into()))?;
        let request = state
            .pending
            .get(request_id)
            .cloned()
            .ok_or(ApprovalError::UnknownRequest)?;
        state.visible.insert(request_id.to_owned(), request.clone());
        Ok(request)
    }

    /// Answer one visible request. A response for an unknown request is
    /// rejected instead of being buffered for a future tool call.
    pub fn respond(&self, response: ApprovalResponse) -> Result<(), ApprovalError> {
        self.respond_with_request(response).map(|_| ())
    }

    /// Return the exact request atomically claimed by this response. Hosts
    /// echo its digest rather than accepting a client-supplied policy grant.
    pub fn respond_with_request(
        &self,
        response: ApprovalResponse,
    ) -> Result<ApprovalRequest, ApprovalError> {
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
        if !state.pending.contains_key(&response.request_id)
            || state.decisions.contains_key(&response.request_id)
        {
            return Err(ApprovalError::UnknownRequest);
        }
        if response.remember
            && state.pending[&response.request_id].origin
                == crate::tools::ToolOrigin::BlueprintWorker
        {
            return Err(ApprovalError::Invalid(
                "worker decisions cannot become remembered tool grants".into(),
            ));
        }
        // An answered request is no longer host-actionable even if the worker
        // has not woken up yet.  Removing it here makes duplicate responses
        // fail closed instead of allowing a later command to overwrite the
        // first decision.
        let request = state
            .pending
            .remove(&response.request_id)
            .ok_or(ApprovalError::UnknownRequest)?;
        state.accepted.push(AcceptedApproval {
            request: request.clone(),
            response: response.clone(),
        });
        state
            .decisions
            .insert(response.request_id.clone(), response);
        wake.notify_all();
        Ok(request)
    }

    /// Whether a request still needs a host decision.  This is intentionally
    /// narrower than worker completion: once a response is accepted, another
    /// `/approve` must not be able to replace it.
    pub fn is_pending(&self, request_id: &str) -> bool {
        let (lock, _) = &*self.inner;
        lock.lock()
            .is_ok_and(|state| state.pending.contains_key(request_id))
    }

    /// Snapshot the accepted response for the currently waiting request.  The
    /// core uses this after the condition-variable wait returns to durably
    /// record the exact decision before entering a side effect.
    pub fn accepted(&self, request_id: &str) -> Option<AcceptedApproval> {
        let (lock, _) = &*self.inner;
        lock.lock().ok().and_then(|state| {
            state
                .accepted
                .iter()
                .find(|accepted| accepted.response.request_id == request_id)
                .cloned()
        })
    }

    /// Persist one accepted decision through the supplied journal callback and
    /// only then release it to the waiting worker.  If persistence fails, the
    /// response is retracted and the request becomes pending again, so a write
    /// or command can never run on an unaudited host decision.
    pub fn persist_accepted<E>(
        &self,
        request_id: &str,
        persist: impl FnOnce(&AcceptedApproval) -> Result<(), E>,
    ) -> Result<AcceptedApproval, PersistApprovalError<E>> {
        let accepted = self
            .accepted(request_id)
            .ok_or(PersistApprovalError::Approval(
                ApprovalError::UnknownRequest,
            ))?;
        if let Err(error) = persist(&accepted) {
            let _ = self.retract_response(request_id);
            return Err(PersistApprovalError::Persistence(error));
        }
        self.mark_persisted(request_id)
            .map_err(PersistApprovalError::Approval)?;
        Ok(accepted)
    }

    pub fn mark_persisted(&self, request_id: &str) -> Result<(), ApprovalError> {
        let (lock, wake) = &*self.inner;
        let mut state = lock
            .lock()
            .map_err(|_| ApprovalError::Invalid("approval coordinator poisoned".into()))?;
        let before = state.accepted.len();
        state
            .accepted
            .retain(|accepted| accepted.response.request_id != request_id);
        if state.accepted.len() == before {
            return Err(ApprovalError::UnknownRequest);
        }
        wake.notify_all();
        Ok(())
    }

    pub fn cancel_all(&self) {
        let (lock, wake) = &*self.inner;
        if let Ok(mut state) = lock.lock() {
            for request_id in state.pending.keys().cloned().collect::<Vec<_>>() {
                if let Some(request) = state.pending.remove(&request_id) {
                    state.accepted.push(AcceptedApproval {
                        request,
                        response: ApprovalResponse {
                            request_id: request_id.clone(),
                            decision: ApprovalDecision::Deny,
                            remember: false,
                        },
                    });
                }
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

    /// Out-of-band host stop, including an already allowed running command.
    /// A new explicit turn captures a new epoch; no worker can clear a stop.
    pub fn emergency_cancel(&self) {
        self.cancellation_epoch.fetch_add(1, Ordering::SeqCst);
        self.cancel_all();
    }

    pub fn cancellation_epoch(&self) -> u64 {
        self.cancellation_epoch.load(Ordering::SeqCst)
    }

    /// Remove an accepted-but-not-yet-consumed response when its durable host
    /// record could not be written.  This keeps a storage failure fail-closed:
    /// the waiting worker remains blocked and cannot enter the side effect.
    pub fn retract_response(&self, request_id: &str) -> Result<(), ApprovalError> {
        let (lock, wake) = &*self.inner;
        let mut state = lock
            .lock()
            .map_err(|_| ApprovalError::Invalid("approval coordinator poisoned".into()))?;
        if state.decisions.remove(request_id).is_none() {
            return Err(ApprovalError::UnknownRequest);
        }
        state
            .accepted
            .retain(|accepted| accepted.response.request_id != request_id);
        if let Some(request) = state.visible.get(request_id).cloned() {
            state.pending.insert(request_id.to_owned(), request);
        }
        wake.notify_all();
        Ok(())
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
        self.decide_after_preflight(side_effect, tool, false)
    }

    /// `worker_preflight` is supplied only by the core after checking the
    /// immutable gate and its binding, never from provider arguments.
    pub fn decide_after_preflight(
        &self,
        side_effect: ToolSideEffect,
        tool: &str,
        worker_preflight: bool,
    ) -> Option<ApprovalDecision> {
        if self.per_tool.get(tool) == Some(&ApprovalDecision::Deny) {
            return Some(ApprovalDecision::Deny);
        }
        if self.mode == ApprovalMode::WorkerAllowAfterPreflight {
            return Some(if worker_preflight {
                ApprovalDecision::Allow
            } else {
                ApprovalDecision::Deny
            });
        }
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
            ApprovalMode::Never => Some(ApprovalDecision::Allow),
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

#[derive(Debug, thiserror::Error)]
pub enum PersistApprovalError<E> {
    #[error("approval response failed: {0}")]
    Approval(ApprovalError),
    #[error("approval decision could not be persisted: {0}")]
    Persistence(E),
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use super::{
        ApprovalCoordinator, ApprovalDecision, ApprovalError, ApprovalMode, ApprovalPolicy,
        ApprovalRequest, ApprovalResponse,
    };
    use crate::tools::ToolSideEffect;

    #[test]
    fn explicit_deny_beats_read_only_trust_and_worker_allow() {
        for mode in [
            ApprovalMode::ReadOnly,
            ApprovalMode::TrustedWorkspace,
            ApprovalMode::WorkerAllowAfterPreflight,
        ] {
            let policy = ApprovalPolicy {
                mode,
                trusted_tools: vec!["read_file".into()],
                per_tool: [("read_file".into(), ApprovalDecision::Deny)].into(),
            };
            assert_eq!(
                policy.decide_after_preflight(ToolSideEffect::ReadOnly, "read_file", true),
                Some(ApprovalDecision::Deny)
            );
        }
    }

    #[test]
    fn worker_mode_without_preflight_denies_even_remembered_and_readonly_tools() {
        let policy = ApprovalPolicy {
            mode: ApprovalMode::WorkerAllowAfterPreflight,
            per_tool: [("read_file".into(), ApprovalDecision::Allow)].into(),
            ..Default::default()
        };
        assert_eq!(
            policy.decide(ToolSideEffect::ReadOnly, "read_file"),
            Some(ApprovalDecision::Deny)
        );
    }

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
                    preview: None,
                    origin: crate::tools::ToolOrigin::AgentTool,
                    policy_digest: None,
                    lease_id: None,
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
                request_id: request.request_id.clone(),
                decision: ApprovalDecision::Allow,
                remember: false,
            })
            .unwrap();
        assert_eq!(join.join().unwrap().unwrap(), ApprovalDecision::Allow);
    }

    #[test]
    fn accepted_response_is_retained_for_durable_ack_and_first_decision_wins() {
        let coordinator = ApprovalCoordinator::new();
        let worker = coordinator.clone();
        let join = thread::spawn(move || {
            let mut policy = ApprovalPolicy::default();
            worker.request_response(
                ApprovalRequest {
                    request_id: "approval-call-2".into(),
                    turn_id: "turn-2".into(),
                    call_id: "call-2".into(),
                    tool: "write_file".into(),
                    side_effect: ToolSideEffect::WorkspaceWrite,
                    arguments: serde_json::json!({"path":"x"}),
                    preview: None,
                    origin: crate::tools::ToolOrigin::AgentTool,
                    policy_digest: None,
                    lease_id: None,
                },
                &mut policy,
                &|| false,
            )
        });
        let request = loop {
            if let Some(request) = coordinator.drain_pending().pop() {
                break request;
            }
            thread::sleep(Duration::from_millis(5));
        };
        coordinator
            .respond(ApprovalResponse {
                request_id: request.request_id.clone(),
                decision: ApprovalDecision::Allow,
                remember: true,
            })
            .unwrap();
        assert!(matches!(
            coordinator.respond(ApprovalResponse {
                request_id: request.request_id.clone(),
                decision: ApprovalDecision::Deny,
                remember: false,
            }),
            Err(ApprovalError::UnknownRequest)
        ));
        let accepted = coordinator.accepted(&request.request_id).unwrap();
        assert_eq!(accepted.response.decision, ApprovalDecision::Allow);
        coordinator.mark_persisted(&request.request_id).unwrap();
        let response = join.join().unwrap().unwrap();
        assert_eq!(response.decision, ApprovalDecision::Allow);
        assert!(response.remember);
    }

    #[test]
    fn cancel_all_retains_an_unrevealed_denial_for_durable_ack() {
        let coordinator = ApprovalCoordinator::new();
        let worker = coordinator.clone();
        let join = thread::spawn(move || {
            let mut policy = ApprovalPolicy::default();
            worker.request_response(
                ApprovalRequest {
                    request_id: "approval-unrevealed".into(),
                    turn_id: "turn-unrevealed".into(),
                    call_id: "call-unrevealed".into(),
                    tool: "write_file".into(),
                    side_effect: ToolSideEffect::WorkspaceWrite,
                    arguments: serde_json::json!({"path":"x"}),
                    preview: None,
                    origin: crate::tools::ToolOrigin::AgentTool,
                    policy_digest: None,
                    lease_id: None,
                },
                &mut policy,
                &|| false,
            )
        });
        while !coordinator.is_pending("approval-unrevealed") {
            thread::sleep(Duration::from_millis(1));
        }

        // Do not call drain_pending/reveal first. EOF and other host-close
        // paths can race with publication of a newly pending request.
        coordinator.cancel_all();
        let accepted = coordinator.accepted("approval-unrevealed").unwrap();
        assert_eq!(accepted.response.decision, ApprovalDecision::Deny);
        coordinator.mark_persisted("approval-unrevealed").unwrap();
        let response = join.join().unwrap().unwrap();
        assert_eq!(response.decision, ApprovalDecision::Deny);
        assert!(!response.remember);
    }
}
