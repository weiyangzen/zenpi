//! Durable, monotonic resource accounting.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::session::{SessionError, SessionStore};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceLimits {
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_wall_ms: u64,
    pub max_disk_bytes: u64,
    pub max_processes: u64,
    pub max_concurrency: u64,
    pub max_network_requests: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_input_tokens: 10_000_000,
            max_output_tokens: 10_000_000,
            max_wall_ms: 24 * 60 * 60 * 1_000,
            max_disk_bytes: 10 * 1024 * 1024 * 1024,
            max_processes: 10_000,
            max_concurrency: 32,
            max_network_requests: 10_000,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub wall_ms: u64,
    pub disk_bytes: u64,
    pub processes: u64,
    pub concurrency: u64,
    pub network_requests: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    InputTokens,
    OutputTokens,
    WallTime,
    Disk,
    Processes,
    Concurrency,
    NetworkRequests,
}

#[derive(Debug)]
pub struct BudgetLedger {
    limits: ResourceLimits,
    usage: ResourceUsage,
    started: Instant,
}

impl BudgetLedger {
    pub fn new(limits: ResourceLimits, usage: ResourceUsage) -> Result<Self, GovernanceError> {
        let ledger = Self {
            limits,
            usage,
            started: Instant::now(),
        };
        ledger.check_all()?;
        Ok(ledger)
    }

    pub fn restore(
        session: &SessionStore,
        limits: ResourceLimits,
    ) -> Result<Self, GovernanceError> {
        let usage = session
            .events()
            .iter()
            .rev()
            .find(|event| event["type"] == "resource_usage")
            .map(|event| {
                serde_json::from_value(event["usage"].clone())
                    .map_err(|error| GovernanceError::InvalidSnapshot(error.to_string()))
            })
            .transpose()?
            .unwrap_or_default();
        Self::new(limits, usage)
    }

    pub const fn limits(&self) -> ResourceLimits {
        self.limits
    }

    pub fn usage(&self) -> ResourceUsage {
        let mut usage = self.usage;
        usage.wall_ms = usage
            .wall_ms
            .saturating_add(duration_ms(self.started.elapsed()));
        usage
    }

    pub fn charge(&mut self, kind: ResourceKind, amount: u64) -> Result<(), GovernanceError> {
        let previous = *self.usage_mut(kind);
        {
            let usage = self.usage_mut(kind);
            *usage = usage
                .checked_add(amount)
                .ok_or(GovernanceError::AccountingOverflow(kind))?;
        }
        if let Err(error) = self.check(kind) {
            let usage = self.usage_mut(kind);
            *usage = previous;
            return Err(error);
        }
        Ok(())
    }

    pub fn release(&mut self, kind: ResourceKind, amount: u64) {
        if matches!(kind, ResourceKind::Concurrency) {
            let usage = self.usage_mut(kind);
            *usage = usage.saturating_sub(amount);
        }
    }

    pub fn check_all(&self) -> Result<(), GovernanceError> {
        for kind in [
            ResourceKind::InputTokens,
            ResourceKind::OutputTokens,
            ResourceKind::WallTime,
            ResourceKind::Disk,
            ResourceKind::Processes,
            ResourceKind::Concurrency,
            ResourceKind::NetworkRequests,
        ] {
            self.check(kind)?;
        }
        Ok(())
    }

    pub fn persist(&mut self, session: &mut SessionStore) -> Result<(), GovernanceError> {
        let elapsed = duration_ms(self.started.elapsed());
        self.usage.wall_ms = self.usage.wall_ms.saturating_add(elapsed);
        self.started = Instant::now();
        self.check_all()?;
        session.append_event(serde_json::json!({
            "type": "resource_usage",
            "usage": self.usage,
            "limits": self.limits,
        }))?;
        Ok(())
    }

    fn usage_mut(&mut self, kind: ResourceKind) -> &mut u64 {
        match kind {
            ResourceKind::InputTokens => &mut self.usage.input_tokens,
            ResourceKind::OutputTokens => &mut self.usage.output_tokens,
            ResourceKind::WallTime => &mut self.usage.wall_ms,
            ResourceKind::Disk => &mut self.usage.disk_bytes,
            ResourceKind::Processes => &mut self.usage.processes,
            ResourceKind::Concurrency => &mut self.usage.concurrency,
            ResourceKind::NetworkRequests => &mut self.usage.network_requests,
        }
    }

    fn check(&self, kind: ResourceKind) -> Result<(), GovernanceError> {
        let usage = self.usage();
        let (used, limit) = match kind {
            ResourceKind::InputTokens => (usage.input_tokens, self.limits.max_input_tokens),
            ResourceKind::OutputTokens => (usage.output_tokens, self.limits.max_output_tokens),
            ResourceKind::WallTime => (usage.wall_ms, self.limits.max_wall_ms),
            ResourceKind::Disk => (usage.disk_bytes, self.limits.max_disk_bytes),
            ResourceKind::Processes => (usage.processes, self.limits.max_processes),
            ResourceKind::Concurrency => (usage.concurrency, self.limits.max_concurrency),
            ResourceKind::NetworkRequests => {
                (usage.network_requests, self.limits.max_network_requests)
            }
        };
        if used > limit {
            Err(GovernanceError::BudgetExceeded { kind, used, limit })
        } else {
            Ok(())
        }
    }
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

const WORKER_SNAPSHOT_VERSION: u32 = 1;
const MAX_WORKER_LEASES: usize = 32;
const MAX_WORKER_OPERATIONS: usize = 512;
const MAX_WORKER_SNAPSHOT_BYTES: usize = 900_000;

const RESOURCE_KINDS: [ResourceKind; 7] = [
    ResourceKind::InputTokens,
    ResourceKind::OutputTokens,
    ResourceKind::WallTime,
    ResourceKind::Disk,
    ResourceKind::Processes,
    ResourceKind::Concurrency,
    ResourceKind::NetworkRequests,
];

impl ResourceUsage {
    fn get(self, kind: ResourceKind) -> u64 {
        match kind {
            ResourceKind::InputTokens => self.input_tokens,
            ResourceKind::OutputTokens => self.output_tokens,
            ResourceKind::WallTime => self.wall_ms,
            ResourceKind::Disk => self.disk_bytes,
            ResourceKind::Processes => self.processes,
            ResourceKind::Concurrency => self.concurrency,
            ResourceKind::NetworkRequests => self.network_requests,
        }
    }

    fn checked_add(self, other: Self) -> Result<Self, GovernanceError> {
        let add = |kind| {
            self.get(kind)
                .checked_add(other.get(kind))
                .ok_or(GovernanceError::AccountingOverflow(kind))
        };
        Ok(Self {
            input_tokens: add(ResourceKind::InputTokens)?,
            output_tokens: add(ResourceKind::OutputTokens)?,
            wall_ms: add(ResourceKind::WallTime)?,
            disk_bytes: add(ResourceKind::Disk)?,
            processes: add(ResourceKind::Processes)?,
            concurrency: add(ResourceKind::Concurrency)?,
            network_requests: add(ResourceKind::NetworkRequests)?,
        })
    }
}

impl ResourceLimits {
    fn get(self, kind: ResourceKind) -> u64 {
        match kind {
            ResourceKind::InputTokens => self.max_input_tokens,
            ResourceKind::OutputTokens => self.max_output_tokens,
            ResourceKind::WallTime => self.max_wall_ms,
            ResourceKind::Disk => self.max_disk_bytes,
            ResourceKind::Processes => self.max_processes,
            ResourceKind::Concurrency => self.max_concurrency,
            ResourceKind::NetworkRequests => self.max_network_requests,
        }
    }
}

/// Origins share one ledger; a shell escape cannot obtain a fresh budget.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BudgetOrigin {
    BlueprintWorker,
    AgentTool,
    UserShell,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkerLease {
    pub lease_id: String,
    pub blueprint_item: String,
    pub policy_digest: String,
    pub expires_at_ms: u64,
    pub limits: ResourceLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BudgetReservation {
    pub operation_id: String,
    pub lease_id: String,
    pub policy_digest: String,
    pub origin: BudgetOrigin,
    /// Upper bounds, not observed counters; held durably before side effects.
    pub resources: ResourceUsage,
    pub gate_decision_id: String,
    pub network_host: Option<String>,
    /// Opaque handle identifiers only. Never store credentials or environment values.
    pub credential_handles: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BudgetCompletion {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BudgetTerminal {
    Cancelled {
        reason: String,
    },
    Exhausted {
        kind: ResourceKind,
        used: u64,
        limit: u64,
    },
    LeaseExpired {
        lease_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BudgetSettlement {
    pub actual: ResourceUsage,
    pub completion: BudgetCompletion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkerOperation {
    pub reservation: BudgetReservation,
    pub reserved_at_ms: u64,
    pub unknown_outcome: bool,
    pub cancel_requested: bool,
    pub settlement: Option<BudgetSettlement>,
}

/// A terminal event instructs the host to cancel and reap every listed operation.
/// Their reservations remain held until the host confirms completion via `settle`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BudgetDirective {
    pub terminal: BudgetTerminal,
    pub cancel_operations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerBudgetSnapshot {
    version: u32,
    session_id: String,
    limits: ResourceLimits,
    last_observed_ms: u64,
    leases: BTreeMap<String, WorkerLease>,
    revoked_leases: BTreeMap<String, BudgetTerminal>,
    operations: BTreeMap<String, WorkerOperation>,
    terminal: Option<BudgetTerminal>,
}

/// Single-host accounting. SessionStore owns cross-process writer exclusion.
/// All mutating methods journal a candidate snapshot before replacing live state.
#[derive(Debug)]
pub struct WorkerBudgetLedger {
    snapshot: WorkerBudgetSnapshot,
}

impl WorkerBudgetLedger {
    pub fn restore(
        session: &mut SessionStore,
        limits: ResourceLimits,
    ) -> Result<Self, GovernanceError> {
        let mut stored: Option<WorkerBudgetSnapshot> = None;
        for event in session
            .events()
            .iter()
            .filter(|event| event["type"] == "worker_budget_snapshot")
        {
            let parsed: WorkerBudgetSnapshot = serde_json::from_value(event["snapshot"].clone())
                .map_err(|error| GovernanceError::InvalidSnapshot(error.to_string()))?;
            if parsed.session_id != session.session_id() || parsed.limits != limits {
                return Err(GovernanceError::InvalidSnapshot(
                    "session or budget identity changed".into(),
                ));
            }
            Self {
                snapshot: parsed.clone(),
            }
            .validate()?;
            if let Some(previous) = &stored {
                validate_transition(previous, &parsed)?;
            }
            stored = Some(parsed);
        }
        let snapshot = stored.unwrap_or_else(|| WorkerBudgetSnapshot {
            version: WORKER_SNAPSHOT_VERSION,
            session_id: session.session_id().into(),
            limits,
            last_observed_ms: 0,
            leases: BTreeMap::new(),
            revoked_leases: BTreeMap::new(),
            operations: BTreeMap::new(),
            terminal: None,
        });
        let mut ledger = Self { snapshot };
        ledger.validate()?;
        let mut candidate = ledger.snapshot.clone();
        let mut changed = false;
        for operation in candidate
            .operations
            .values_mut()
            .filter(|op| op.settlement.is_none())
        {
            changed |= !operation.unknown_outcome;
            operation.unknown_outcome = true;
        }
        if changed {
            ledger.commit(session, candidate, None)?;
        }
        Ok(ledger)
    }

    pub fn open_lease(
        &mut self,
        session: &mut SessionStore,
        lease: WorkerLease,
        now_ms: u64,
    ) -> Result<(), GovernanceError> {
        self.require_open(now_ms)?;
        validate_lease(&lease)?;
        if lease.expires_at_ms <= now_ms {
            return Err(GovernanceError::LeaseUnavailable(lease.lease_id));
        }
        if self.snapshot.leases.contains_key(&lease.lease_id) {
            return Err(GovernanceError::DuplicateLease(lease.lease_id));
        }
        for previous in self
            .snapshot
            .leases
            .values()
            .filter(|previous| previous.blueprint_item == lease.blueprint_item)
        {
            if previous.policy_digest != lease.policy_digest || previous.limits != lease.limits {
                return Err(GovernanceError::PolicyMismatch);
            }
            if matches!(
                self.snapshot.revoked_leases.get(&previous.lease_id),
                Some(BudgetTerminal::Exhausted { .. })
            ) {
                return Err(GovernanceError::LeaseUnavailable(previous.lease_id.clone()));
            }
        }
        if self.snapshot.leases.len() >= MAX_WORKER_LEASES {
            return Err(GovernanceError::SnapshotCapacity);
        }
        for kind in RESOURCE_KINDS {
            if lease.limits.get(kind) > self.snapshot.limits.get(kind) {
                return Err(GovernanceError::InvalidSnapshot(
                    "lease exceeds host limits".into(),
                ));
            }
        }
        let mut candidate = self.snapshot.clone();
        candidate.last_observed_ms = now_ms;
        candidate.leases.insert(lease.lease_id.clone(), lease);
        self.commit(session, candidate, None)
    }

    pub fn reserve(
        &mut self,
        session: &mut SessionStore,
        reservation: BudgetReservation,
        now_ms: u64,
    ) -> Result<(), GovernanceError> {
        self.require_open(now_ms)?;
        validate_reservation(&reservation)?;
        if self
            .snapshot
            .operations
            .contains_key(&reservation.operation_id)
        {
            return Err(GovernanceError::DuplicateOperation(
                reservation.operation_id,
            ));
        }
        if self.snapshot.operations.len() >= MAX_WORKER_OPERATIONS {
            return Err(GovernanceError::SnapshotCapacity);
        }
        let lease = self
            .snapshot
            .leases
            .get(&reservation.lease_id)
            .ok_or_else(|| GovernanceError::LeaseUnavailable(reservation.lease_id.clone()))?;
        if self.snapshot.revoked_leases.contains_key(&lease.lease_id) {
            return Err(GovernanceError::LeaseUnavailable(lease.lease_id.clone()));
        }
        if lease.expires_at_ms <= now_ms {
            let lease_id = lease.lease_id.clone();
            let terminal = BudgetTerminal::LeaseExpired {
                lease_id: lease_id.clone(),
            };
            self.stop(session, Some(&lease_id), terminal.clone(), now_ms)?;
            return Err(GovernanceError::WorkerStopped(terminal));
        }
        if reservation.policy_digest != lease.policy_digest {
            return Err(GovernanceError::PolicyMismatch);
        }
        let lease_id = lease.lease_id.clone();
        let lease_limits = lease.limits;
        let mut candidate = self.snapshot.clone();
        candidate.last_observed_ms = now_ms;
        candidate.operations.insert(
            reservation.operation_id.clone(),
            WorkerOperation {
                reservation,
                reserved_at_ms: now_ms,
                unknown_outcome: false,
                cancel_requested: false,
                settlement: None,
            },
        );
        let proposed = Self {
            snapshot: candidate,
        };
        for (scope, limits) in [
            (None, self.snapshot.limits),
            (Some(lease_id.as_str()), lease_limits),
        ] {
            let usage = proposed.committed_usage(scope)?;
            if let Some(terminal) = budget_exhaustion(usage, limits) {
                self.stop(session, scope, terminal.clone(), now_ms)?;
                return Err(GovernanceError::WorkerStopped(terminal));
            }
        }
        self.commit(session, proposed.snapshot, None)
    }

    /// Call only after the host has observed the result and reaped owned children.
    /// Uncertain effects stay reserved; they must not be automatically retried.
    pub fn settle(
        &mut self,
        session: &mut SessionStore,
        operation_id: &str,
        mut actual: ResourceUsage,
        completion: BudgetCompletion,
        now_ms: u64,
    ) -> Result<Option<BudgetDirective>, GovernanceError> {
        self.check_clock(now_ms)?;
        if actual.concurrency != 0 {
            return Err(GovernanceError::InvalidSnapshot(
                "settlement must release all concurrency after reap".into(),
            ));
        }
        let operation = self
            .snapshot
            .operations
            .get(operation_id)
            .ok_or_else(|| GovernanceError::UnknownOperation(operation_id.into()))?;
        if operation.settlement.is_some() {
            return Err(GovernanceError::DuplicateOperation(operation_id.into()));
        }
        // Measure elapsed time from the durable reservation, including host downtime.
        actual.wall_ms = actual
            .wall_ms
            .max(now_ms.saturating_sub(operation.reserved_at_ms));
        let reserved = operation.reservation.resources;
        let lease_id = operation.reservation.lease_id.clone();
        let lease_limits = self.snapshot.leases[&lease_id].limits;
        let mut candidate = self.snapshot.clone();
        candidate.last_observed_ms = now_ms;
        candidate
            .operations
            .get_mut(operation_id)
            .expect("operation checked")
            .settlement = Some(BudgetSettlement { actual, completion });
        let mut proposed = Self {
            snapshot: candidate,
        };
        let overshoot = RESOURCE_KINDS
            .into_iter()
            .find(|kind| actual.get(*kind) > reserved.get(*kind))
            .map(|kind| BudgetTerminal::Exhausted {
                kind,
                used: actual.get(kind),
                limit: reserved.get(kind),
            });
        let terminal = overshoot
            .or(budget_exhaustion(
                proposed.committed_usage(None)?,
                self.snapshot.limits,
            ))
            .or(budget_exhaustion(
                proposed.committed_usage(Some(&lease_id))?,
                lease_limits,
            ));
        let directive = terminal.map(|terminal| proposed.stop_candidate(None, terminal));
        self.commit(session, proposed.snapshot, directive.as_ref())?;
        Ok(directive)
    }

    pub fn revoke_lease(
        &mut self,
        session: &mut SessionStore,
        lease_id: &str,
        reason: &str,
        now_ms: u64,
    ) -> Result<BudgetDirective, GovernanceError> {
        if !self.snapshot.leases.contains_key(lease_id) {
            return Err(GovernanceError::LeaseUnavailable(lease_id.into()));
        }
        validate_identifier(reason)?;
        self.stop(
            session,
            Some(lease_id),
            BudgetTerminal::Cancelled {
                reason: reason.into(),
            },
            now_ms,
        )
    }

    pub fn cancel(
        &mut self,
        session: &mut SessionStore,
        reason: &str,
        now_ms: u64,
    ) -> Result<BudgetDirective, GovernanceError> {
        validate_identifier(reason)?;
        self.stop(
            session,
            None,
            BudgetTerminal::Cancelled {
                reason: reason.into(),
            },
            now_ms,
        )
    }

    /// Host timer hook. No operation is released until `settle` confirms reap.
    pub fn expire(
        &mut self,
        session: &mut SessionStore,
        now_ms: u64,
    ) -> Result<Vec<BudgetDirective>, GovernanceError> {
        self.check_clock(now_ms)?;
        let expired: Vec<_> = self
            .snapshot
            .leases
            .values()
            .filter(|lease| {
                lease.expires_at_ms <= now_ms
                    && !self.snapshot.revoked_leases.contains_key(&lease.lease_id)
            })
            .map(|lease| lease.lease_id.clone())
            .collect();
        let mut candidate = Self {
            snapshot: self.snapshot.clone(),
        };
        candidate.snapshot.last_observed_ms = now_ms;
        let mut directives = Vec::new();
        if candidate.snapshot.terminal.is_none()
            && let Some(operation) = candidate.snapshot.operations.values().find(|operation| {
                operation.settlement.is_none()
                    && now_ms.saturating_sub(operation.reserved_at_ms)
                        > operation.reservation.resources.wall_ms
            })
        {
            let terminal = BudgetTerminal::Exhausted {
                kind: ResourceKind::WallTime,
                used: now_ms.saturating_sub(operation.reserved_at_ms),
                limit: operation.reservation.resources.wall_ms,
            };
            directives.push(candidate.stop_candidate(None, terminal));
        }
        for lease_id in expired {
            directives.push(candidate.stop_candidate(
                Some(&lease_id),
                BudgetTerminal::LeaseExpired {
                    lease_id: lease_id.clone(),
                },
            ));
        }
        self.commit(session, candidate.snapshot, None)?;
        Ok(directives)
    }

    pub fn operations(&self) -> &BTreeMap<String, WorkerOperation> {
        &self.snapshot.operations
    }

    pub fn terminal(&self) -> Option<&BudgetTerminal> {
        self.snapshot.terminal.as_ref()
    }

    /// A lease scope includes every generation of its Blueprint item.
    pub fn committed_usage(
        &self,
        lease_id: Option<&str>,
    ) -> Result<ResourceUsage, GovernanceError> {
        let item = lease_id
            .map(|id| {
                self.snapshot
                    .leases
                    .get(id)
                    .map(|lease| lease.blueprint_item.as_str())
                    .ok_or_else(|| GovernanceError::LeaseUnavailable(id.into()))
            })
            .transpose()?;
        self.snapshot
            .operations
            .values()
            .filter(|op| {
                item.is_none_or(|item| {
                    self.snapshot.leases[&op.reservation.lease_id].blueprint_item == item
                })
            })
            .try_fold(ResourceUsage::default(), |sum, op| {
                sum.checked_add(
                    op.settlement
                        .as_ref()
                        .map_or(op.reservation.resources, |settlement| settlement.actual),
                )
            })
    }

    fn require_open(&self, now_ms: u64) -> Result<(), GovernanceError> {
        self.check_clock(now_ms)?;
        if let Some(terminal) = &self.snapshot.terminal {
            return Err(GovernanceError::WorkerStopped(terminal.clone()));
        }
        if self
            .snapshot
            .operations
            .values()
            .any(|op| op.unknown_outcome && op.settlement.is_none())
        {
            return Err(GovernanceError::RecoveryRequired);
        }
        Ok(())
    }

    fn check_clock(&self, now_ms: u64) -> Result<(), GovernanceError> {
        if now_ms < self.snapshot.last_observed_ms {
            return Err(GovernanceError::ClockRegressed);
        }
        Ok(())
    }

    fn stop(
        &mut self,
        session: &mut SessionStore,
        lease_id: Option<&str>,
        terminal: BudgetTerminal,
        now_ms: u64,
    ) -> Result<BudgetDirective, GovernanceError> {
        self.check_clock(now_ms)?;
        let mut candidate = Self {
            snapshot: self.snapshot.clone(),
        };
        candidate.snapshot.last_observed_ms = now_ms;
        let directive = candidate.stop_candidate(lease_id, terminal);
        self.commit(session, candidate.snapshot, Some(&directive))?;
        Ok(directive)
    }

    fn stop_candidate(
        &mut self,
        lease_id: Option<&str>,
        terminal: BudgetTerminal,
    ) -> BudgetDirective {
        if let Some(id) = lease_id {
            self.snapshot
                .revoked_leases
                .entry(id.into())
                .or_insert(terminal.clone());
        } else if self.snapshot.terminal.is_none() {
            self.snapshot.terminal = Some(terminal.clone());
        }
        let mut cancel_operations = Vec::new();
        for (id, op) in &mut self.snapshot.operations {
            if op.settlement.is_none() && lease_id.is_none_or(|id| op.reservation.lease_id == id) {
                op.cancel_requested = true;
                cancel_operations.push(id.clone());
            }
        }
        BudgetDirective {
            terminal,
            cancel_operations,
        }
    }

    fn commit(
        &mut self,
        session: &mut SessionStore,
        snapshot: WorkerBudgetSnapshot,
        directive: Option<&BudgetDirective>,
    ) -> Result<(), GovernanceError> {
        if snapshot.session_id != session.session_id() {
            return Err(GovernanceError::InvalidSnapshot(
                "session identity changed".into(),
            ));
        }
        let event = serde_json::json!({
            "type": "worker_budget_snapshot", "snapshot": snapshot, "directive": directive,
        });
        if serde_json::to_vec(&event)
            .map_err(|error| GovernanceError::InvalidSnapshot(error.to_string()))?
            .len()
            > MAX_WORKER_SNAPSHOT_BYTES
        {
            return Err(GovernanceError::SnapshotCapacity);
        }
        session.append_event(event)?;
        self.snapshot = snapshot;
        Ok(())
    }

    fn validate(&self) -> Result<(), GovernanceError> {
        let snapshot = &self.snapshot;
        if snapshot.version != WORKER_SNAPSHOT_VERSION
            || snapshot.leases.len() > MAX_WORKER_LEASES
            || snapshot.operations.len() > MAX_WORKER_OPERATIONS
        {
            return Err(GovernanceError::InvalidSnapshot(
                "unsupported or oversized snapshot".into(),
            ));
        }
        for (id, lease) in &snapshot.leases {
            validate_lease(lease)?;
            if id != &lease.lease_id
                || RESOURCE_KINDS
                    .into_iter()
                    .any(|kind| lease.limits.get(kind) > snapshot.limits.get(kind))
            {
                return Err(GovernanceError::InvalidSnapshot(
                    "lease identity or limits changed".into(),
                ));
            }
        }
        if snapshot
            .revoked_leases
            .keys()
            .any(|id| !snapshot.leases.contains_key(id))
        {
            return Err(GovernanceError::InvalidSnapshot(
                "unknown revoked lease".into(),
            ));
        }
        for (id, operation) in &snapshot.operations {
            validate_reservation(&operation.reservation)?;
            let lease = snapshot
                .leases
                .get(&operation.reservation.lease_id)
                .ok_or_else(|| {
                    GovernanceError::InvalidSnapshot("operation without lease".into())
                })?;
            if id != &operation.reservation.operation_id
                || operation.reservation.policy_digest != lease.policy_digest
                || operation.reserved_at_ms > snapshot.last_observed_ms
                || operation.reserved_at_ms >= lease.expires_at_ms
                || operation
                    .settlement
                    .as_ref()
                    .is_some_and(|settlement| settlement.actual.concurrency != 0)
                || ((snapshot.terminal.is_some()
                    || snapshot.revoked_leases.contains_key(&lease.lease_id))
                    && operation.settlement.is_none()
                    && !operation.cancel_requested)
            {
                return Err(GovernanceError::InvalidSnapshot(
                    "inconsistent operation".into(),
                ));
            }
        }
        if budget_exhaustion(self.committed_usage(None)?, snapshot.limits).is_some()
            && snapshot.terminal.is_none()
        {
            return Err(GovernanceError::InvalidSnapshot(
                "unrecorded global exhaustion".into(),
            ));
        }
        for lease in snapshot.leases.values() {
            if budget_exhaustion(self.committed_usage(Some(&lease.lease_id))?, lease.limits)
                .is_some()
                && snapshot.terminal.is_none()
                && !snapshot.revoked_leases.contains_key(&lease.lease_id)
            {
                return Err(GovernanceError::InvalidSnapshot(
                    "unrecorded lease exhaustion".into(),
                ));
            }
        }
        Ok(())
    }
}

fn validate_transition(
    previous: &WorkerBudgetSnapshot,
    next: &WorkerBudgetSnapshot,
) -> Result<(), GovernanceError> {
    let invalid = previous.session_id != next.session_id
        || previous.limits != next.limits
        || previous.last_observed_ms > next.last_observed_ms
        || previous
            .terminal
            .as_ref()
            .is_some_and(|terminal| next.terminal.as_ref() != Some(terminal))
        || previous
            .leases
            .iter()
            .any(|(id, lease)| next.leases.get(id) != Some(lease))
        || previous
            .revoked_leases
            .iter()
            .any(|(id, terminal)| next.revoked_leases.get(id) != Some(terminal))
        || previous.operations.iter().any(|(id, operation)| {
            next.operations.get(id).is_none_or(|updated| {
                operation.reservation != updated.reservation
                    || operation.reserved_at_ms != updated.reserved_at_ms
                    || (operation.unknown_outcome && !updated.unknown_outcome)
                    || (operation.cancel_requested && !updated.cancel_requested)
                    || operation
                        .settlement
                        .as_ref()
                        .is_some_and(|settlement| updated.settlement.as_ref() != Some(settlement))
            })
        });
    if invalid {
        return Err(GovernanceError::InvalidSnapshot(
            "non-monotonic worker accounting history".into(),
        ));
    }
    Ok(())
}

fn validate_identifier(value: &str) -> Result<(), GovernanceError> {
    if value.is_empty()
        || value.len() > 128
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'@')
        })
    {
        return Err(GovernanceError::InvalidSnapshot(
            "invalid bounded identifier".into(),
        ));
    }
    Ok(())
}

fn validate_lease(lease: &WorkerLease) -> Result<(), GovernanceError> {
    validate_identifier(&lease.lease_id)?;
    validate_identifier(&lease.blueprint_item)?;
    if lease.policy_digest.len() != 64
        || !lease
            .policy_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(GovernanceError::InvalidSnapshot(
            "policy digest must be lowercase sha256".into(),
        ));
    }
    Ok(())
}

fn validate_reservation(reservation: &BudgetReservation) -> Result<(), GovernanceError> {
    validate_identifier(&reservation.operation_id)?;
    validate_identifier(&reservation.lease_id)?;
    validate_identifier(&reservation.gate_decision_id)?;
    if reservation.resources.concurrency == 0
        || reservation.resources.wall_ms == 0
        || reservation.credential_handles.len() > 16
    {
        return Err(GovernanceError::InvalidSnapshot(
            "operation needs bounded time and concurrency".into(),
        ));
    }
    if let Some(host) = &reservation.network_host {
        validate_identifier(host)?;
        if reservation.resources.network_requests == 0 {
            return Err(GovernanceError::InvalidSnapshot(
                "network host requires network reservation".into(),
            ));
        }
    } else if reservation.resources.network_requests > 0 {
        return Err(GovernanceError::InvalidSnapshot(
            "network reservation requires explicit host".into(),
        ));
    }
    for handle in &reservation.credential_handles {
        validate_identifier(handle)?;
    }
    Ok(())
}

fn budget_exhaustion(usage: ResourceUsage, limits: ResourceLimits) -> Option<BudgetTerminal> {
    RESOURCE_KINDS.into_iter().find_map(|kind| {
        let used = usage.get(kind);
        let limit = limits.get(kind);
        (used > limit).then_some(BudgetTerminal::Exhausted { kind, used, limit })
    })
}

#[derive(Debug, Error)]
pub enum GovernanceError {
    #[error("{kind:?} budget exceeded: used {used}, limit {limit}")]
    BudgetExceeded {
        kind: ResourceKind,
        used: u64,
        limit: u64,
    },
    #[error("resource accounting overflow: {0:?}")]
    AccountingOverflow(ResourceKind),
    #[error("invalid resource snapshot: {0}")]
    InvalidSnapshot(String),
    #[error("worker budget stopped: {0:?}")]
    WorkerStopped(BudgetTerminal),
    #[error("worker lease unavailable: {0}")]
    LeaseUnavailable(String),
    #[error("duplicate worker lease: {0}")]
    DuplicateLease(String),
    #[error("operation was already reserved or settled; use a new retry ID: {0}")]
    DuplicateOperation(String),
    #[error("unknown resource operation: {0}")]
    UnknownOperation(String),
    #[error("operation policy digest differs from immutable lease")]
    PolicyMismatch,
    #[error("pending operation outcome must be reconciled before new work")]
    RecoveryRequired,
    #[error("host clock regressed below durable checkpoint")]
    ClockRegressed,
    #[error("worker accounting snapshot capacity exhausted")]
    SnapshotCapacity,
    #[error("resource journal: {0}")]
    Session(#[from] SessionError),
}
