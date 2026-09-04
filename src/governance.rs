//! Durable, monotonic resource accounting.

use std::time::{Duration, Instant};

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
            .and_then(|event| serde_json::from_value(event["usage"].clone()).ok())
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
        {
            let usage = self.usage_mut(kind);
            *usage = usage.saturating_add(amount);
        }
        if let Err(error) = self.check(kind) {
            let usage = self.usage_mut(kind);
            *usage = usage.saturating_sub(amount);
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

#[derive(Debug, Error)]
pub enum GovernanceError {
    #[error("{kind:?} budget exceeded: used {used}, limit {limit}")]
    BudgetExceeded {
        kind: ResourceKind,
        used: u64,
        limit: u64,
    },
    #[error("resource journal: {0}")]
    Session(#[from] SessionError),
}
