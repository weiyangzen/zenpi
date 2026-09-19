//! LAN headless cluster control plane (ZS1-160).
//!
//! The local process is the *control plane*: it consumes the bounded, read-only
//! LAN snapshot produced by [`crate::net_probe`], turns reachable SSH hosts into
//! schedulable capacity, dispatches headless workers to them and reclaims them
//! when they finish.
//!
//! Security contract:
//! - Discovery is read-only and never performs a write on a peer.
//! - Dispatch requires **explicit operator authorization** for the target host
//!   (`ZENPI_CLUSTER_AUTHORIZED_HOSTS`) *and* locally supplied credentials. A
//!   host that was merely discovered is never dispatched to implicitly.
//! - Only private LAN addresses are admitted; loopback, link-local, CGNAT and
//!   public addresses are rejected.
//! - Credentials are read from the same local secret source as `net_probe`,
//!   never serialized into a snapshot, and never placed on a remote command
//!   line. SSH dispatch relies on the local agent/key, never an inline password.
//! - Every collection is bounded: host count, worker count, per-host workers,
//!   label length, and reserved headroom.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::net_probe::{DeviceClass, HostResources, LanHost, LanSnapshot, NetCredentials};

/// Largest number of LAN hosts a cluster may retain.
pub const MAX_CLUSTER_HOSTS: usize = 256;
/// Upper bound on concurrent headless workers cluster-wide.
pub const MAX_CLUSTER_WORKERS: usize = 256;
/// Upper bound on workers a single host may run.
pub const MAX_WORKERS_PER_HOST: usize = 8;
/// Longest retained worker label.
pub const MAX_WORKER_LABEL_CHARS: usize = 64;
/// Longest retained failure message.
pub const MAX_CLUSTER_MESSAGE_CHARS: usize = 256;
/// A host must expose SSH (or have yielded resources over SSH) to be a host.
pub const HOST_SSH_PORT: u16 = 22;
/// One logical CPU is always left for the remote host's own work.
pub const RESERVED_CPU_SLOTS: usize = 1;
/// A quarter of total memory is always left for the remote host's own work.
pub const RESERVED_MEMORY_NUMERATOR: u64 = 1;
pub const RESERVED_MEMORY_DENOMINATOR: u64 = 4;
/// Fallbacks used only when a probe could not read the real value.
pub const DEFAULT_LOGICAL_CPUS: usize = 4;
pub const DEFAULT_MEMORY_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Default footprint of one headless worker.
pub const DEFAULT_WORKER_CPU_SLOTS: u32 = 1;
pub const DEFAULT_WORKER_MEMORY_BYTES: u64 = 1024 * 1024 * 1024;
/// Upper bound on GPUs counted from one probe.
pub const MAX_GPU_DEVICES: usize = 8;

/// Scheduling/dispatch failures. Every variant is explicit so a caller can
/// fail closed instead of silently degrading an unauthorized dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClusterError {
    #[error("host {0} is not explicitly authorized for dispatch")]
    Unauthorized(String),
    #[error("host {0} is not a private LAN address")]
    NonLanAddress(String),
    #[error("no local credentials are available for headless dispatch")]
    NoCredentials,
    #[error("no authorized host has capacity for worker {0}")]
    NoCapacity(String),
    #[error("host {0} already runs the per-host maximum of {1} workers")]
    HostWorkerLimit(String, usize),
    #[error("cluster already runs the maximum of {0} workers")]
    ClusterWorkerLimit(usize),
    #[error("invalid worker spec: {0}")]
    InvalidSpec(String),
    #[error("unknown worker {0}")]
    UnknownWorker(String),
    #[error("dispatch backend error: {0}")]
    Backend(String),
}

/// Usable capacity of one LAN host plus the accounting of what is reserved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HostCapacity {
    pub logical_cpus: usize,
    pub memory_total_bytes: u64,
    pub gpus: usize,
    pub cpu_slots_used: u32,
    pub memory_bytes_used: u64,
}

impl HostCapacity {
    /// Derive capacity from a probe result, falling back to conservative
    /// defaults when a field was unavailable. The fallback is never treated as
    /// evidence of a stronger host; it only prevents an unusable zero.
    pub fn from_resources(resources: Option<&HostResources>) -> Self {
        let logical_cpus = resources
            .and_then(|resources| resources.logical_cpus)
            .filter(|cpus| *cpus > 0)
            .unwrap_or(DEFAULT_LOGICAL_CPUS);
        let memory_total_bytes = resources
            .and_then(|resources| resources.memory_total_bytes)
            .filter(|bytes| *bytes > 0)
            .unwrap_or(DEFAULT_MEMORY_BYTES);
        let gpus = resources
            .map(|resources| resources.gpus.len().min(MAX_GPU_DEVICES))
            .unwrap_or(0);
        Self {
            logical_cpus,
            memory_total_bytes,
            gpus,
            cpu_slots_used: 0,
            memory_bytes_used: 0,
        }
    }

    /// Logical CPUs the cluster may schedule, after reserving one for the host.
    pub fn usable_cpu_slots(&self) -> u32 {
        self.logical_cpus
            .saturating_sub(RESERVED_CPU_SLOTS)
            .max(1)
            .min(u32::MAX as usize) as u32
    }

    /// Memory the cluster may schedule, after reserving headroom for the host.
    pub fn usable_memory_bytes(&self) -> u64 {
        let reserve =
            self.memory_total_bytes * RESERVED_MEMORY_NUMERATOR / RESERVED_MEMORY_DENOMINATOR;
        self.memory_total_bytes.saturating_sub(reserve)
    }

    pub fn available_cpu_slots(&self) -> u32 {
        self.usable_cpu_slots().saturating_sub(self.cpu_slots_used)
    }

    pub fn available_memory_bytes(&self) -> u64 {
        self.usable_memory_bytes()
            .saturating_sub(self.memory_bytes_used)
    }

    pub fn can_fit(&self, spec: &WorkerSpec) -> bool {
        spec.cpu_slots <= self.available_cpu_slots()
            && spec.memory_bytes <= self.available_memory_bytes()
    }

    pub fn reserve(&mut self, spec: &WorkerSpec) {
        self.cpu_slots_used = self.cpu_slots_used.saturating_add(spec.cpu_slots);
        self.memory_bytes_used = self.memory_bytes_used.saturating_add(spec.memory_bytes);
    }

    pub fn release(&mut self, spec: &WorkerSpec) {
        self.cpu_slots_used = self.cpu_slots_used.saturating_sub(spec.cpu_slots);
        self.memory_bytes_used = self.memory_bytes_used.saturating_sub(spec.memory_bytes);
    }
}

/// What to run on a remote host. This describes a bounded headless agent, not
/// an arbitrary command: the transport owns the actual argv.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerSpec {
    pub name: String,
    pub session_id: String,
    pub prompt: Option<String>,
    pub cpu_slots: u32,
    pub memory_bytes: u64,
    /// Prefer a host that advertises a GPU when one has capacity.
    pub prefer_gpu: bool,
}

impl WorkerSpec {
    pub fn headless(name: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            session_id: session_id.into(),
            prompt: None,
            cpu_slots: DEFAULT_WORKER_CPU_SLOTS,
            memory_bytes: DEFAULT_WORKER_MEMORY_BYTES,
            prefer_gpu: false,
        }
    }

    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    pub fn with_cpu_slots(mut self, cpu_slots: u32) -> Self {
        self.cpu_slots = cpu_slots;
        self
    }

    pub fn with_memory_bytes(mut self, memory_bytes: u64) -> Self {
        self.memory_bytes = memory_bytes;
        self
    }

    pub fn with_gpu(mut self) -> Self {
        self.prefer_gpu = true;
        self
    }

    pub fn validate(&self) -> Result<(), ClusterError> {
        if self.name.trim().is_empty() {
            return Err(ClusterError::InvalidSpec("empty worker name".into()));
        }
        if self.session_id.trim().is_empty() {
            return Err(ClusterError::InvalidSpec("empty session id".into()));
        }
        if self.cpu_slots == 0 {
            return Err(ClusterError::InvalidSpec("cpu_slots must be >= 1".into()));
        }
        if self.memory_bytes == 0 {
            return Err(ClusterError::InvalidSpec(
                "memory_bytes must be >= 1".into(),
            ));
        }
        Ok(())
    }
}

/// Explicit operator authorization set. Without at least one entry no dispatch
/// can happen, even if hosts were discovered and credentials are present.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClusterAuthorization {
    hosts: BTreeSet<String>,
}

impl ClusterAuthorization {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn from_entries<I, S>(entries: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut authorization = Self::none();
        for entry in entries {
            authorization.authorize(entry.as_ref());
        }
        authorization
    }

    /// Read the explicit authorization list from
    /// `ZENPI_CLUSTER_AUTHORIZED_HOSTS` (comma or whitespace separated). Only
    /// valid private LAN addresses are accepted; anything else is ignored.
    pub fn from_env() -> Self {
        match std::env::var("ZENPI_CLUSTER_AUTHORIZED_HOSTS") {
            Ok(raw) => Self::parse(&raw),
            Err(_) => Self::none(),
        }
    }

    pub fn parse(raw: &str) -> Self {
        let entries: Vec<&str> = raw
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|entry| !entry.is_empty())
            .collect();
        Self::from_entries(entries)
    }

    /// Authorize an address. Only a private LAN address is retained; a public
    /// or malformed entry is ignored rather than stored and silently trusted.
    pub fn authorize(&mut self, ip: &str) {
        let ip = ip.trim();
        if crate::net_probe::is_lan_address(ip) {
            self.hosts.insert(ip.to_owned());
        }
    }

    pub fn revoke(&mut self, ip: &str) {
        self.hosts.remove(ip.trim());
    }

    pub fn clear(&mut self) {
        self.hosts.clear();
    }

    pub fn is_authorized(&self, ip: &str) -> bool {
        self.hosts.contains(ip.trim())
    }

    pub fn is_empty(&self) -> bool {
        self.hosts.is_empty()
    }

    pub fn len(&self) -> usize {
        self.hosts.len()
    }

    pub fn entries(&self) -> impl Iterator<Item = &str> {
        self.hosts.iter().map(String::as_str)
    }

    pub fn summary(&self) -> String {
        if self.hosts.is_empty() {
            return "no authorized hosts".to_owned();
        }
        format!(
            "{} authorized host(s): {}",
            self.hosts.len(),
            self.hosts.iter().cloned().collect::<Vec<_>>().join(", ")
        )
    }
}

/// A remote worker handle returned by a transport after a successful spawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteWorker {
    pub remote_id: String,
    pub host: String,
    pub pid: u32,
}

/// One bounded observation of a running worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorkerProbe {
    pub running: bool,
    pub cpu_percent: Option<f64>,
    pub rss_bytes: Option<u64>,
}

/// Transport contract. The control plane never builds shell strings itself; a
/// backend owns the (bounded) remote execution.
pub trait DispatchBackend {
    fn spawn_worker(&self, host: &str, spec: &WorkerSpec) -> Result<RemoteWorker, String>;
    fn probe_worker(&self, remote: &RemoteWorker) -> Result<WorkerProbe, String>;
    fn reclaim_worker(&self, remote: &RemoteWorker) -> Result<(), String>;
}

/// Lifecycle of one worker. `Failed`/`Rejected`/`Reclaimed` are terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerState {
    Pending,
    Running,
    Reclaiming,
    Reclaimed,
    Failed,
    Rejected,
}

impl WorkerState {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Reclaimed | Self::Failed | Self::Rejected)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Reclaiming => "reclaiming",
            Self::Reclaimed => "reclaimed",
            Self::Failed => "failed",
            Self::Rejected => "rejected",
        }
    }
}

/// Durable, redacted record of one worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRecord {
    pub id: String,
    pub host: String,
    pub state: WorkerState,
    pub spec: WorkerSpec,
    pub remote: Option<RemoteWorker>,
    pub message: Option<String>,
    pub created_ms: u64,
    pub updated_ms: u64,
    pub attempts: u32,
}

/// A schedulable LAN host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterHost {
    pub ip: String,
    pub class: DeviceClass,
    pub hostname: Option<String>,
    pub has_credentials: bool,
    pub authorized: bool,
    pub capacity: HostCapacity,
}

/// Read-only projection of the cluster for the Resources pane. It never
/// carries credentials and never carries a raw remote command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ClusterSnapshot {
    pub hosts: Vec<ClusterHostView>,
    pub total_workers: usize,
    pub running_workers: usize,
    pub reclaimed_workers: usize,
    pub failed_workers: usize,
    pub authorized_hosts: usize,
    pub generated_ms: u64,
}

impl ClusterSnapshot {
    pub fn host(&self, ip: &str) -> Option<&ClusterHostView> {
        self.hosts.iter().find(|host| host.ip == ip)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterHostView {
    pub ip: String,
    pub class: DeviceClass,
    pub hostname: Option<String>,
    pub authorized: bool,
    pub has_credentials: bool,
    pub logical_cpus: usize,
    pub available_cpu_slots: u32,
    pub memory_total_bytes: u64,
    pub available_memory_bytes: u64,
    pub gpus: usize,
    pub worker_count: usize,
    pub workers: Vec<String>,
}

/// The local control plane: admits hosts, schedules by capacity, dispatches,
/// reconciles and reclaims. It owns all lifecycle accounting.
pub struct ClusterControlPlane {
    credentials: NetCredentials,
    authorization: ClusterAuthorization,
    hosts: BTreeMap<String, ClusterHost>,
    workers: BTreeMap<String, WorkerRecord>,
    next_worker_seq: u64,
}

impl fmt::Debug for ClusterControlPlane {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClusterControlPlane")
            .field("authorization", &self.authorization.summary())
            .field("hosts", &self.hosts.keys())
            .field("workers", &self.workers.len())
            .finish()
    }
}

impl ClusterControlPlane {
    pub fn new(credentials: NetCredentials, authorization: ClusterAuthorization) -> Self {
        Self {
            credentials,
            authorization,
            hosts: BTreeMap::new(),
            workers: BTreeMap::new(),
            next_worker_seq: 1,
        }
    }

    /// Convenience constructor reading both the credentials and the explicit
    /// authorization from the environment.
    pub fn from_env() -> Self {
        Self::new(NetCredentials::from_env(), ClusterAuthorization::from_env())
    }

    pub fn credentials(&self) -> &NetCredentials {
        &self.credentials
    }

    pub fn authorization(&self) -> &ClusterAuthorization {
        &self.authorization
    }

    pub fn hosts(&self) -> impl Iterator<Item = &ClusterHost> {
        self.hosts.values()
    }

    pub fn host(&self, ip: &str) -> Option<&ClusterHost> {
        self.hosts.get(ip.trim())
    }

    pub fn workers(&self) -> impl Iterator<Item = &WorkerRecord> {
        self.workers.values()
    }

    pub fn worker(&self, id: &str) -> Option<&WorkerRecord> {
        self.workers.get(id)
    }

    /// Admit hosts from a bounded LAN snapshot. Only private LAN addresses that
    /// expose SSH (or already yielded resources over SSH) become hosts, and
    /// each one records whether the operator explicitly authorized it.
    /// Returns the number of admitted hosts.
    pub fn admit(&mut self, snapshot: &LanSnapshot) -> usize {
        let mut admitted = 0;
        for host in &snapshot.hosts {
            if self.hosts.len() >= MAX_CLUSTER_HOSTS {
                break;
            }
            if self.admit_host(host).is_ok() {
                admitted += 1;
            }
        }
        admitted
    }

    /// Admit a single host. A non-LAN address or a host without SSH is
    /// rejected with a typed error and never inserted.
    pub fn admit_host(&mut self, host: &LanHost) -> Result<(), ClusterError> {
        if !crate::net_probe::is_lan_address(&host.ip) {
            return Err(ClusterError::NonLanAddress(host.ip.clone()));
        }
        let ssh = host.open_ports.contains(&HOST_SSH_PORT) || host.credentials_used;
        if !ssh {
            return Err(ClusterError::InvalidSpec(format!(
                "host {} has no SSH endpoint",
                host.ip
            )));
        }
        let entry = ClusterHost {
            ip: host.ip.clone(),
            class: host.class,
            hostname: host.hostname.clone(),
            has_credentials: !self.credentials.is_empty(),
            authorized: self.authorization.is_authorized(&host.ip),
            capacity: HostCapacity::from_resources(host.resources.as_ref()),
        };
        self.hosts.insert(host.ip.clone(), entry);
        Ok(())
    }

    /// Pick the best authorized host for a spec: a GPU host first when the
    /// worker prefers a GPU, then the host with the most available memory.
    /// Ties break on IP so placement is deterministic and testable.
    pub fn select_host(&self, spec: &WorkerSpec) -> Result<String, ClusterError> {
        let mut candidates: Vec<&ClusterHost> = self
            .hosts
            .values()
            .filter(|host| host.authorized)
            .filter(|host| self.host_worker_count(&host.ip) < MAX_WORKERS_PER_HOST)
            .filter(|host| host.capacity.can_fit(spec))
            .collect();
        if candidates.is_empty() {
            // Distinguish "authorized but full" from "not authorized" so the
            // operator sees why placement failed.
            if !self.hosts.values().any(|host| host.authorized) {
                return Err(ClusterError::Unauthorized("<no authorized host>".into()));
            }
            if self
                .hosts
                .values()
                .filter(|host| host.authorized)
                .all(|host| self.host_worker_count(&host.ip) >= MAX_WORKERS_PER_HOST)
            {
                return Err(ClusterError::HostWorkerLimit(
                    "<authorized hosts>".into(),
                    MAX_WORKERS_PER_HOST,
                ));
            }
            return Err(ClusterError::NoCapacity(spec.name.clone()));
        }
        candidates.sort_by(|left, right| {
            right
                .capacity
                .available_memory_bytes()
                .cmp(&left.capacity.available_memory_bytes())
                .then_with(|| {
                    right
                        .capacity
                        .available_cpu_slots()
                        .cmp(&left.capacity.available_cpu_slots())
                })
                .then_with(|| left.ip.cmp(&right.ip))
        });
        if spec.prefer_gpu
            && let Some(host) = candidates.iter().find(|host| host.capacity.gpus > 0)
        {
            return Ok(host.ip.clone());
        }
        Ok(candidates[0].ip.clone())
    }

    /// Dispatch a worker to the best host. Fails closed before touching the
    /// transport when credentials are missing, authorization is absent, or the
    /// worker count bound is reached.
    pub fn dispatch<B: DispatchBackend>(
        &mut self,
        spec: &WorkerSpec,
        backend: &B,
    ) -> Result<String, ClusterError> {
        spec.validate()?;
        if self.credentials.is_empty() {
            return Err(ClusterError::NoCredentials);
        }
        if self.workers.len() >= MAX_CLUSTER_WORKERS {
            return Err(ClusterError::ClusterWorkerLimit(MAX_CLUSTER_WORKERS));
        }
        let host_ip = self.select_host(spec)?;
        let now = unix_time_ms();
        let id = self.next_worker_id(&spec.name);
        let record = WorkerRecord {
            id: id.clone(),
            host: host_ip.clone(),
            state: WorkerState::Pending,
            spec: spec.clone(),
            remote: None,
            message: None,
            created_ms: now,
            updated_ms: now,
            attempts: 0,
        };
        self.workers.insert(id.clone(), record);
        if let Some(host) = self.hosts.get_mut(&host_ip) {
            host.capacity.reserve(spec);
        }
        match backend.spawn_worker(&host_ip, spec) {
            Ok(remote) => {
                if let Some(record) = self.workers.get_mut(&id) {
                    record.state = WorkerState::Running;
                    record.remote = Some(remote);
                    record.attempts = 1;
                    record.updated_ms = unix_time_ms();
                }
                Ok(id)
            }
            Err(error) => {
                self.fail_worker(&id, format!("spawn failed: {error}"));
                Err(ClusterError::Backend(error))
            }
        }
    }

    /// Reclaim a worker. A backend failure is fail-closed: capacity is kept
    /// reserved because the remote process may still be alive, and the record
    /// is marked `Failed` for visibility.
    pub fn reclaim<B: DispatchBackend>(
        &mut self,
        id: &str,
        backend: &B,
    ) -> Result<WorkerState, ClusterError> {
        let remote = match self.workers.get(id) {
            Some(record) if record.state.is_terminal() => return Ok(record.state),
            Some(record) => record
                .remote
                .clone()
                .ok_or_else(|| ClusterError::Backend("worker has no remote handle".into()))?,
            None => return Err(ClusterError::UnknownWorker(id.to_owned())),
        };
        if let Some(record) = self.workers.get_mut(id) {
            record.state = WorkerState::Reclaiming;
            record.updated_ms = unix_time_ms();
        }
        match backend.reclaim_worker(&remote) {
            Ok(()) => {
                self.release_host_capacity(id);
                if let Some(record) = self.workers.get_mut(id) {
                    record.state = WorkerState::Reclaimed;
                    record.updated_ms = unix_time_ms();
                }
                Ok(WorkerState::Reclaimed)
            }
            Err(error) => {
                let message = format!("reclaim failed: {error}");
                if let Some(record) = self.workers.get_mut(id) {
                    record.state = WorkerState::Failed;
                    record.message = Some(truncate_message(&message));
                    record.updated_ms = unix_time_ms();
                }
                Err(ClusterError::Backend(error))
            }
        }
    }

    /// Poll every running worker and reclaim the ones that have exited.
    /// Returns the number of workers reclaimed in this pass.
    pub fn reconcile<B: DispatchBackend>(&mut self, backend: &B) -> usize {
        let running: Vec<String> = self
            .workers
            .values()
            .filter(|record| record.state == WorkerState::Running)
            .map(|record| record.id.clone())
            .collect();
        let mut reclaimed = 0;
        for id in running {
            let Some(remote) = self
                .workers
                .get(&id)
                .and_then(|record| record.remote.clone())
            else {
                continue;
            };
            match backend.probe_worker(&remote) {
                Ok(probe) if !probe.running => {
                    if self.reclaim(&id, backend).is_ok() {
                        reclaimed += 1;
                    }
                }
                Ok(_) | Err(_) => {}
            }
        }
        reclaimed
    }

    /// Mark a worker failed and release whatever capacity it reserved.
    pub fn fail_worker(&mut self, id: &str, reason: impl Into<String>) {
        let reason = truncate_message(&reason.into());
        let mut marked = false;
        if let Some(record) = self.workers.get_mut(id)
            && !record.state.is_terminal()
        {
            record.state = WorkerState::Failed;
            record.message = Some(reason);
            record.updated_ms = unix_time_ms();
            marked = true;
        }
        if marked {
            self.release_host_capacity(id);
        }
    }

    /// Withdraw explicit authorization for a host. Existing workers are left
    /// running so the operator can reclaim them deliberately; only new
    /// dispatch is blocked.
    pub fn revoke(&mut self, ip: &str) -> bool {
        self.authorization.revoke(ip);
        let mut changed = false;
        if let Some(host) = self.hosts.get_mut(ip.trim()) {
            host.authorized = false;
            changed = true;
        }
        changed
    }

    pub fn authorize(&mut self, ip: &str) -> bool {
        self.authorization.authorize(ip);
        let authorized = self.authorization.is_authorized(ip);
        if let Some(host) = self.hosts.get_mut(ip.trim()) {
            host.authorized = authorized;
        }
        authorized
    }

    pub fn host_worker_count(&self, ip: &str) -> usize {
        self.workers
            .values()
            .filter(|record| record.host == ip && !record.state.is_terminal())
            .count()
    }

    /// Build the read-only projection shown by the Resources pane.
    pub fn snapshot(&self) -> ClusterSnapshot {
        let mut hosts: Vec<ClusterHostView> = self
            .hosts
            .values()
            .map(|host| {
                let workers: Vec<String> = self
                    .workers
                    .values()
                    .filter(|record| record.host == host.ip)
                    .map(|record| record.id.clone())
                    .collect();
                ClusterHostView {
                    ip: host.ip.clone(),
                    class: host.class,
                    hostname: host.hostname.clone(),
                    authorized: host.authorized,
                    has_credentials: host.has_credentials,
                    logical_cpus: host.capacity.logical_cpus,
                    available_cpu_slots: host.capacity.available_cpu_slots(),
                    memory_total_bytes: host.capacity.memory_total_bytes,
                    available_memory_bytes: host.capacity.available_memory_bytes(),
                    gpus: host.capacity.gpus,
                    worker_count: workers.len(),
                    workers,
                }
            })
            .collect();
        hosts.sort_by(|left, right| left.ip.cmp(&right.ip));
        let count = |state: WorkerState| {
            self.workers
                .values()
                .filter(|record| record.state == state)
                .count()
        };
        ClusterSnapshot {
            hosts,
            total_workers: self.workers.len(),
            running_workers: count(WorkerState::Running)
                + count(WorkerState::Reclaiming)
                + count(WorkerState::Pending),
            reclaimed_workers: count(WorkerState::Reclaimed),
            failed_workers: count(WorkerState::Failed) + count(WorkerState::Rejected),
            authorized_hosts: self.hosts.values().filter(|host| host.authorized).count(),
            generated_ms: unix_time_ms(),
        }
    }

    fn release_host_capacity(&mut self, id: &str) {
        let Some(record) = self.workers.get(id).cloned() else {
            return;
        };
        if let Some(host) = self.hosts.get_mut(&record.host) {
            host.capacity.release(&record.spec);
        }
    }

    fn next_worker_id(&mut self, name: &str) -> String {
        let seq = self.next_worker_seq;
        self.next_worker_seq = self.next_worker_seq.saturating_add(1);
        format!("{}-{seq:04}", sanitize_label(name))
    }
}

/// Keep an operator supplied label to a safe, bounded alphabet so it can be
/// embedded in a remote session path or file name.
pub fn sanitize_label(name: &str) -> String {
    let mut label: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(MAX_WORKER_LABEL_CHARS)
        .collect();
    if label.is_empty() {
        label.push_str("worker");
    }
    label
}

fn truncate_message(message: &str) -> String {
    message.chars().take(MAX_CLUSTER_MESSAGE_CHARS).collect()
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

/// Real SSH transport for headless workers. It relies exclusively on the local
/// SSH agent/key (`BatchMode=yes`); a password is never placed on the command
/// line or in argv. Reclaiming uses a signal only, never a write to the peer's
/// files beyond the worker's own session/log directory.
pub struct SshDispatchBackend {
    binary: String,
    remote_dir: String,
}

impl Default for SshDispatchBackend {
    fn default() -> Self {
        Self {
            binary: "zenpi".to_owned(),
            remote_dir: ".zenpi/cluster".to_owned(),
        }
    }
}

impl SshDispatchBackend {
    pub fn new(binary: impl Into<String>, remote_dir: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
            remote_dir: remote_dir.into(),
        }
    }

    /// Build the bounded remote command for a worker. Pure so the shell
    /// quoting and the absence of secrets can be asserted in tests.
    pub fn remote_command(&self, spec: &WorkerSpec, remote_id: &str) -> String {
        let dir = shell_quote(&self.remote_dir);
        let session = shell_quote(&format!("{}/{remote_id}.jsonl", self.remote_dir));
        let log = shell_quote(&format!("{}/{remote_id}.log", self.remote_dir));
        let binary = shell_quote(&self.binary);
        let stdin = match &spec.prompt {
            Some(prompt) => format!("printf '%s\\n' {} |", shell_quote(prompt)),
            None => "printf '' |".to_owned(),
        };
        format!(
            "mkdir -p {dir} && {{ {stdin} nohup {binary} --mode headless --session {session} >{log} 2>&1 & echo $!; }}"
        )
    }

    fn ssh_argv(&self, host: &str, command: &str) -> Vec<String> {
        vec![
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "ConnectTimeout=5".into(),
            host.to_owned(),
            command.to_owned(),
        ]
    }

    fn run_ssh(&self, host: &str, command: &str) -> Result<std::process::Output, String> {
        if !crate::net_probe::is_lan_address(host) {
            return Err(format!("refusing non-LAN host {host}"));
        }
        std::process::Command::new("ssh")
            .args(self.ssh_argv(host, command))
            .output()
            .map_err(|error| format!("ssh {host}: {error}"))
    }
}

impl DispatchBackend for SshDispatchBackend {
    fn spawn_worker(&self, host: &str, spec: &WorkerSpec) -> Result<RemoteWorker, String> {
        let remote_id = format!(
            "{}-{}",
            sanitize_label(&spec.name),
            truncate_message(&spec.session_id).replace(['/', ' '], "_")
        );
        let command = self.remote_command(spec, &remote_id);
        let output = self.run_ssh(host, &command)?;
        if !output.status.success() {
            return Err(format!(
                "ssh spawn exited with {}",
                output.status.code().unwrap_or(-1)
            ));
        }
        let pid = String::from_utf8_lossy(&output.stdout)
            .lines()
            .rev()
            .find_map(|line| line.trim().parse::<u32>().ok())
            .ok_or_else(|| "ssh spawn did not report a pid".to_owned())?;
        Ok(RemoteWorker {
            remote_id,
            host: host.to_owned(),
            pid,
        })
    }

    fn probe_worker(&self, remote: &RemoteWorker) -> Result<WorkerProbe, String> {
        let output = self.run_ssh(&remote.host, &format!("kill -0 {}", remote.pid))?;
        Ok(WorkerProbe {
            running: output.status.success(),
            cpu_percent: None,
            rss_bytes: None,
        })
    }

    fn reclaim_worker(&self, remote: &RemoteWorker) -> Result<(), String> {
        let output = self.run_ssh(&remote.host, &format!("kill {}", remote.pid))?;
        // A worker that already exited makes `kill` fail; that is still a
        // successful reclamation from the control plane's perspective.
        if output.status.success() {
            return Ok(());
        }
        match self.probe_worker(remote) {
            Ok(probe) if !probe.running => Ok(()),
            _ => Err(format!(
                "ssh reclaim exited with {}",
                output.status.code().unwrap_or(-1)
            )),
        }
    }
}

/// POSIX single-quote a string for safe shell embedding.
pub fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for c in value.chars() {
        if c == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(c);
        }
    }
    quoted.push('\'');
    quoted
}

// ---------------------------------------------------------------------------
// ZS1-161: bridges from the LAN/cluster projections into the unified resource
// and information bus. These are pure conversions of already-bounded values;
// they never touch the network, the transport, or credentials.
// ---------------------------------------------------------------------------

/// Upper bound on host addresses carried by one bus section.
pub const MAX_BUS_HOST_IPS: usize = 64;

impl ClusterSnapshot {
    /// Fold this cluster projection into the bus, keeping the credential-free
    /// aggregate plus a bounded list of admitted host addresses.
    pub fn bus_section(&self) -> crate::resources::ClusterBusSection {
        let truncated = self.hosts.len() > MAX_BUS_HOST_IPS;
        let host_ips = self
            .hosts
            .iter()
            .take(MAX_BUS_HOST_IPS)
            .map(|host| host.ip.clone())
            .collect();
        crate::resources::ClusterBusSection {
            host_count: self.hosts.len(),
            authorized_hosts: self.authorized_hosts,
            total_workers: self.total_workers,
            running_workers: self.running_workers,
            reclaimed_workers: self.reclaimed_workers,
            failed_workers: self.failed_workers,
            host_ips,
            truncated,
        }
    }
}

/// Fold a bounded LAN scan into the bus summary. Only counts and topology
/// labels are retained; per-host detail stays in the LAN projection.
pub fn lan_bus_section(snapshot: &LanSnapshot) -> crate::resources::LanBusSection {
    crate::resources::LanBusSection {
        local_ip: snapshot.local_ip.clone(),
        gateway_ip: snapshot.gateway_ip.clone(),
        host_count: snapshot.hosts.len(),
        block_count: snapshot.blocks.len(),
        truncated: snapshot.truncated,
    }
}

/// Assemble the unified bus from host hardware plus the LAN and cluster
/// projections the local control plane already holds. Budget and port leases
/// are attached by their owners through the builder methods, so this function
/// cannot invent accounting it was not given.
pub fn cluster_resource_bus(
    host: &crate::resources::ResourceSnapshot,
    lan: Option<&LanSnapshot>,
    cluster: Option<&ClusterSnapshot>,
    now_ms: u64,
) -> crate::resources::ResourceBusSnapshot {
    let mut bus = crate::resources::ResourceBusSnapshot::from_host(host, now_ms);
    if let Some(lan) = lan {
        bus = bus.with_lan(lan_bus_section(lan));
    }
    if let Some(cluster) = cluster {
        bus = bus.with_cluster(cluster.bus_section());
    }
    bus
}
