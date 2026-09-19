//! Bounded resource signals for agent governance and diagnostics.
//!
//! The collector intentionally uses only the standard library. Workspace
//! accounting is a bounded, symlink-free walk; host metrics are best-effort
//! snapshots and degrade to typed `Unavailable` fields on platforms without a
//! portable operating-system API. Callers that need to keep an interactive
//! provider loop responsive should run `ResourceCollector::collect` on a
//! background worker (the work itself has explicit node/file/byte limits).

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_WORKSPACE_FILES: usize = 100_000;
pub const MAX_WORKSPACE_DIRECTORIES: usize = 20_000;
pub const MAX_WORKSPACE_NODES: usize = 120_000;
pub const MAX_WORKSPACE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_WORKSPACE_DEPTH: usize = 64;

/// Process classes shown by the compact resource monitor. Classes are a closed
/// set so "same-class" merging can never grow unbounded on hostile process
/// names; anything unrecognised collapses into [`ProcessClass::Other`].
pub const MAX_PROCESS_ROWS: usize = 24;
/// Upper bound on host processes inspected for one summary. A machine with more
/// processes than this reports `truncated` instead of allocating without bound.
pub const MAX_PROCESSES_SCANNED: usize = 8_192;
/// Upper bound on discrete GPU devices reported by one probe.
pub const MAX_GPU_DEVICES: usize = 8;
/// Longest device/command label retained from an external probe.
pub const MAX_DEVICE_LABEL_CHARS: usize = 48;
/// Widest proportional utilization bar rendered by the compact monitor.
pub const MAX_BAR_WIDTH: usize = 24;

/// Number of headless processes retained in one footprint summary. A host with
/// more workers reports `truncated` instead of growing the snapshot without
/// bound.
pub const MAX_HEADLESS_FOOTPRINT_ROWS: usize = 64;
/// Hard ceiling for a programmatic CPU budget, expressed as a percentage of one
/// logical CPU. This is not a realistic target; it only keeps a hostile or
/// accidental policy from disabling the gate entirely.
pub const MAX_HEADLESS_CPU_PERCENT: f64 = 100.0 * 256.0;
/// Hard ceiling for a programmatic RSS budget (8 GiB). A headless worker above
/// this is treated as a policy error rather than an ordinary footprint.
pub const MAX_HEADLESS_RSS_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Clamp a used/total ratio into a `0.0..=100.0` percentage. A zero total is
/// reported as `0.0` rather than `NaN`/`inf`, so a caller can always render it.
pub fn utilization_percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    ((used as f64 / total as f64) * 100.0).clamp(0.0, 100.0)
}

/// Render a fixed-width proportional bar such as `█████░░░`. The result is
/// always exactly `width` display columns wide so a caller can align columns.
pub fn render_bar(percent: f64, width: usize) -> String {
    let width = width.min(MAX_BAR_WIDTH);
    if width == 0 {
        return String::new();
    }
    let clamped = percent.clamp(0.0, 100.0);
    let filled = ((clamped / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let mut bar = String::with_capacity(width * 3);
    for index in 0..width {
        bar.push(if index < filled { '█' } else { '░' });
    }
    bar
}

#[derive(Debug, Error)]
pub enum ResourceError {
    #[error("resource root does not exist: {0}")]
    NotFound(PathBuf),
    #[error("resource root is not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("resource path is denied: {0}")]
    #[allow(dead_code)]
    PathDenied(PathBuf),
    #[error("resource policy is invalid: {0}")]
    InvalidPolicy(String),
    #[error("resource I/O: {0}")]
    Io(#[from] io::Error),
}

impl ResourceError {
    #[allow(dead_code)]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "resource_not_found",
            Self::NotDirectory(_) => "resource_not_directory",
            Self::PathDenied(_) => "resource_path_denied",
            Self::InvalidPolicy(_) => "resource_invalid_policy",
            Self::Io(_) => "resource_io",
        }
    }
}

/// Limits for a single workspace walk. Values are deliberately bounded even
/// when a caller constructs a policy programmatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceScanPolicy {
    pub max_files: usize,
    pub max_directories: usize,
    pub max_nodes: usize,
    pub max_bytes: u64,
    pub max_depth: usize,
}

impl Default for WorkspaceScanPolicy {
    fn default() -> Self {
        Self {
            max_files: 10_000,
            max_directories: 2_000,
            max_nodes: 12_000,
            max_bytes: 512 * 1024 * 1024,
            max_depth: 32,
        }
    }
}

impl WorkspaceScanPolicy {
    pub fn validate(self) -> Result<(), ResourceError> {
        if self.max_files == 0 || self.max_files > MAX_WORKSPACE_FILES {
            return Err(ResourceError::InvalidPolicy(format!(
                "max_files must be between 1 and {MAX_WORKSPACE_FILES}"
            )));
        }
        if self.max_directories == 0 || self.max_directories > MAX_WORKSPACE_DIRECTORIES {
            return Err(ResourceError::InvalidPolicy(format!(
                "max_directories must be between 1 and {MAX_WORKSPACE_DIRECTORIES}"
            )));
        }
        if self.max_nodes == 0 || self.max_nodes > MAX_WORKSPACE_NODES {
            return Err(ResourceError::InvalidPolicy(format!(
                "max_nodes must be between 1 and {MAX_WORKSPACE_NODES}"
            )));
        }
        if self.max_bytes == 0 || self.max_bytes > MAX_WORKSPACE_BYTES {
            return Err(ResourceError::InvalidPolicy(format!(
                "max_bytes must be between 1 and {MAX_WORKSPACE_BYTES}"
            )));
        }
        if self.max_depth == 0 || self.max_depth > MAX_WORKSPACE_DEPTH {
            return Err(ResourceError::InvalidPolicy(format!(
                "max_depth must be between 1 and {MAX_WORKSPACE_DEPTH}"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalStatus {
    Available,
    #[default]
    Unavailable,
}

/// Bounded workspace inventory. `truncated` means at least one policy limit
/// prevented the walk from visiting every node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSummary {
    pub root: String,
    pub files: usize,
    pub directories: usize,
    pub nodes: usize,
    pub bytes: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuSignal {
    pub logical_cpus: usize,
    pub load_one_minute: Option<f64>,
    pub status: SignalStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySignal {
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub status: SignalStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskSignal {
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub status: SignalStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSignal {
    pub pid: u32,
    pub resident_bytes: Option<u64>,
    pub status: SignalStatus,
}

/// Coarse process taxonomy used by the merged monitor. Declared in display
/// order so `Ord` also yields a stable presentation order.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ProcessClass {
    Zenpi,
    OpenCode,
    Agent,
    Lsp,
    Mcp,
    Node,
    Rust,
    Shell,
    Git,
    Search,
    #[default]
    Other,
}

impl ProcessClass {
    #[allow(dead_code)]
    pub const ALL: [Self; 11] = [
        Self::Zenpi,
        Self::OpenCode,
        Self::Agent,
        Self::Lsp,
        Self::Mcp,
        Self::Node,
        Self::Rust,
        Self::Shell,
        Self::Git,
        Self::Search,
        Self::Other,
    ];

    #[allow(dead_code)]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Zenpi => "zenpi",
            Self::OpenCode => "opencode",
            Self::Agent => "agent",
            Self::Lsp => "lsp",
            Self::Mcp => "mcp",
            Self::Node => "node",
            Self::Rust => "rust",
            Self::Shell => "shell",
            Self::Git => "git",
            Self::Search => "search",
            Self::Other => "other",
        }
    }

    /// Classify a raw process command/path (e.g. `comm` from `ps`). The matcher
    /// is case-insensitive and ordered so language servers and MCP bridges are
    /// never mistaken for a generic worker.
    pub fn classify(command: &str) -> Self {
        let lower = command.to_ascii_lowercase();
        let base = lower.rsplit(['/', '\\']).next().unwrap_or(lower.as_str());
        let base = base.split_whitespace().next().unwrap_or(base);
        let base = base.strip_suffix(".exe").unwrap_or(base);
        let has = |needle: &str| lower.contains(needle);

        if has("rust-analyzer")
            || has("typescript-language-server")
            || has("tsserver")
            || has("gopls")
            || has("clangd")
            || has("pyright")
            || has("pylsp")
            || has("jedi-language-server")
            || has("lua-language-server")
            || has("language-server")
            || has("language_server")
            || has("-lsp")
            || base == "lsp"
        {
            return Self::Lsp;
        }
        if has("mcp") {
            return Self::Mcp;
        }
        if base.contains("zenpi") || base.contains("pi_agent") || base.contains("pi-agent") {
            return Self::Zenpi;
        }
        if base.contains("opencode") {
            return Self::OpenCode;
        }
        if matches!(base, "cargo" | "rustc" | "rustup" | "rustdoc" | "rustfmt")
            || base.starts_with("cargo-")
            || base.starts_with("clippy")
        {
            return Self::Rust;
        }
        if matches!(
            base,
            "node" | "deno" | "bun" | "npm" | "npx" | "pnpm" | "yarn" | "ts-node"
        ) {
            return Self::Node;
        }
        if matches!(
            base,
            "zsh" | "bash" | "sh" | "dash" | "fish" | "tcsh" | "csh" | "login" | "sudo" | "env"
        ) {
            return Self::Shell;
        }
        if base.starts_with("git") {
            return Self::Git;
        }
        if matches!(
            base,
            "rg" | "ripgrep" | "grep" | "egrep" | "fgrep" | "find" | "fd" | "ag" | "ack"
        ) {
            return Self::Search;
        }
        if has("agent") || has("harness") || has("worker") {
            return Self::Agent;
        }
        Self::Other
    }
}

/// One merged line: every process of the same class is summed rather than
/// listed individually.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessClassRow {
    pub class: ProcessClass,
    pub count: usize,
    pub resident_bytes: u64,
}

/// Merged process view. `rows` holds at most [`MAX_PROCESS_ROWS`] classes,
/// highest resident memory first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProcessSummary {
    pub total: usize,
    pub resident_bytes: u64,
    pub rows: Vec<ProcessClassRow>,
    pub truncated: bool,
    pub status: SignalStatus,
}

impl ProcessSummary {
    pub fn unavailable() -> Self {
        Self {
            status: SignalStatus::Unavailable,
            ..Self::default()
        }
    }

    #[allow(dead_code)]
    pub fn row(&self, class: ProcessClass) -> Option<&ProcessClassRow> {
        self.rows.iter().find(|row| row.class == class)
    }

    #[allow(dead_code)]
    pub fn count_for(&self, class: ProcessClass) -> usize {
        self.row(class).map_or(0, |row| row.count)
    }
}

/// Whether a headless process is waiting (idle) or executing a provider turn or
/// tool batch (busy). The two phases have separate ceilings because an agent
/// that is merely blocked on stdin must not be allowed to leak memory, while a
/// working agent legitimately needs CPU and context headroom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FootprintPhase {
    Idle,
    Busy,
}

impl FootprintPhase {
    #[allow(dead_code)]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Busy => "busy",
        }
    }
}

/// Outcome of evaluating one footprint against its phase budget. Kept as a
/// closed set so a gate consumer can fail closed on anything unrecognised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadlessFootprintVerdict {
    #[default]
    Within,
    CpuExceeded,
    RssExceeded,
    CpuAndRssExceeded,
    Unavailable,
}

impl HeadlessFootprintVerdict {
    #[allow(dead_code)]
    pub const fn within_budget(self) -> bool {
        matches!(self, Self::Within)
    }

    #[allow(dead_code)]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Within => "footprint_within_budget",
            Self::CpuExceeded => "footprint_cpu_exceeded",
            Self::RssExceeded => "footprint_rss_exceeded",
            Self::CpuAndRssExceeded => "footprint_cpu_and_rss_exceeded",
            Self::Unavailable => "footprint_unavailable",
        }
    }

    pub const fn denied(self) -> bool {
        matches!(
            self,
            Self::CpuExceeded | Self::RssExceeded | Self::CpuAndRssExceeded | Self::Unavailable
        )
    }
}

/// Per-headless-process CPU/RSS ceilings. `idle_*` bounds a worker blocked on
/// input or provider I/O; `busy_*` bounds one executing a turn. Both CPU values
/// are percentages of a single logical CPU, matched to the averaging used by
/// [`HeadlessProcessFootprint::cpu_percent`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HeadlessFootprintBudget {
    pub idle_cpu_percent_max: f64,
    pub idle_rss_bytes_max: u64,
    pub busy_cpu_percent_max: f64,
    pub busy_rss_bytes_max: u64,
}

impl Default for HeadlessFootprintBudget {
    fn default() -> Self {
        // Idle reflects the measured "≈2 MB RSS / 0% CPU" steady state with a
        // generous multiple, while busy allows a full core and a context-sized
        // address space without letting one worker exhaust the host.
        Self {
            idle_cpu_percent_max: 5.0,
            idle_rss_bytes_max: 32 * 1024 * 1024,
            busy_cpu_percent_max: 100.0,
            busy_rss_bytes_max: 512 * 1024 * 1024,
        }
    }
}

impl HeadlessFootprintBudget {
    pub fn validate(self) -> Result<(), ResourceError> {
        for (name, value) in [
            ("idle_cpu_percent_max", self.idle_cpu_percent_max),
            ("busy_cpu_percent_max", self.busy_cpu_percent_max),
        ] {
            if !value.is_finite() || value <= 0.0 || value > MAX_HEADLESS_CPU_PERCENT {
                return Err(ResourceError::InvalidPolicy(format!(
                    "{name} must be finite and between 0 and {MAX_HEADLESS_CPU_PERCENT}"
                )));
            }
        }
        for (name, value) in [
            ("idle_rss_bytes_max", self.idle_rss_bytes_max),
            ("busy_rss_bytes_max", self.busy_rss_bytes_max),
        ] {
            if value == 0 || value > MAX_HEADLESS_RSS_BYTES {
                return Err(ResourceError::InvalidPolicy(format!(
                    "{name} must be between 1 and {MAX_HEADLESS_RSS_BYTES}"
                )));
            }
        }
        Ok(())
    }

    /// The `(cpu_percent, resident_bytes)` ceiling for one phase.
    pub fn limits_for(self, phase: FootprintPhase) -> (f64, u64) {
        match phase {
            FootprintPhase::Idle => (self.idle_cpu_percent_max, self.idle_rss_bytes_max),
            FootprintPhase::Busy => (self.busy_cpu_percent_max, self.busy_rss_bytes_max),
        }
    }

    /// Fail closed: an unavailable sample is never treated as within budget, so
    /// a host that cannot measure a worker does not silently admit it.
    pub fn evaluate(self, footprint: &HeadlessProcessFootprint) -> HeadlessFootprintVerdict {
        if footprint.status != SignalStatus::Available {
            return HeadlessFootprintVerdict::Unavailable;
        }
        let (cpu_max, rss_max) = self.limits_for(footprint.phase);
        let cpu_over = footprint.cpu_percent > cpu_max;
        let rss_over = footprint.resident_bytes > rss_max;
        match (cpu_over, rss_over) {
            (false, false) => HeadlessFootprintVerdict::Within,
            (true, false) => HeadlessFootprintVerdict::CpuExceeded,
            (false, true) => HeadlessFootprintVerdict::RssExceeded,
            (true, true) => HeadlessFootprintVerdict::CpuAndRssExceeded,
        }
    }
}

/// One headless process footprint. `cpu_percent` is the average CPU used since
/// the process (or sampler) started, so a short burst is smoothed the same way
/// an operator's `ps` view would show it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeadlessProcessFootprint {
    pub pid: u32,
    pub phase: FootprintPhase,
    pub cpu_percent: f64,
    pub resident_bytes: u64,
    pub status: SignalStatus,
    #[serde(default)]
    pub verdict: HeadlessFootprintVerdict,
}

/// Per-process footprint rows plus their aggregate against one budget. The
/// summary is part of [`ResourceSnapshot`], so the same resource gate that
/// renders host CPU/RSS also reports every headless worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HeadlessFootprintSummary {
    pub total: usize,
    pub resident_bytes: u64,
    pub cpu_percent: f64,
    pub rows: Vec<HeadlessProcessFootprint>,
    /// True when at least one process exceeded its phase ceiling or could not
    /// be measured.
    pub exceeded: bool,
    pub truncated: bool,
    pub status: SignalStatus,
}

impl HeadlessFootprintSummary {
    pub fn unavailable() -> Self {
        Self {
            status: SignalStatus::Unavailable,
            ..Self::default()
        }
    }

    /// Score every process against `budget` and aggregate the survivors. Rows
    /// are bounded by [`MAX_HEADLESS_FOOTPRINT_ROWS`]; a budget error degrades to
    /// an explicit `Unavailable` summary instead of panicking.
    pub fn from_processes(
        budget: HeadlessFootprintBudget,
        mut rows: Vec<HeadlessProcessFootprint>,
    ) -> Self {
        if budget.validate().is_err() {
            return Self::unavailable();
        }
        let truncated = rows.len() > MAX_HEADLESS_FOOTPRINT_ROWS;
        rows.truncate(MAX_HEADLESS_FOOTPRINT_ROWS);
        let mut total = 0usize;
        let mut resident_bytes = 0u64;
        let mut cpu_percent = 0.0f64;
        let mut exceeded = false;
        let mut measured = false;
        for row in &mut rows {
            row.verdict = budget.evaluate(row);
            exceeded |= row.verdict.denied();
            if row.status == SignalStatus::Available {
                measured = true;
                total = total.saturating_add(1);
                resident_bytes = resident_bytes.saturating_add(row.resident_bytes);
                cpu_percent += row.cpu_percent.max(0.0);
            }
        }
        let status = if measured {
            SignalStatus::Available
        } else if rows.is_empty() {
            // An empty but successful scan means "no headless workers", which is
            // a valid observation rather than a missing signal.
            SignalStatus::Available
        } else {
            SignalStatus::Unavailable
        };
        Self {
            total,
            resident_bytes,
            cpu_percent,
            rows,
            exceeded,
            truncated,
            status,
        }
    }

    #[allow(dead_code)]
    pub fn row(&self, pid: u32) -> Option<&HeadlessProcessFootprint> {
        self.rows.iter().find(|row| row.pid == pid)
    }

    #[allow(dead_code)]
    pub fn exceeded_pids(&self) -> Vec<u32> {
        self.rows
            .iter()
            .filter(|row| row.verdict.denied())
            .map(|row| row.pid)
            .collect()
    }

    /// Gate decision for the whole host: any denied worker denies the gate.
    pub fn verdict(&self) -> HeadlessFootprintVerdict {
        if self.status != SignalStatus::Available {
            return HeadlessFootprintVerdict::Unavailable;
        }
        let cpu = self.rows.iter().any(|row| {
            matches!(
                row.verdict,
                HeadlessFootprintVerdict::CpuExceeded | HeadlessFootprintVerdict::CpuAndRssExceeded
            )
        });
        let rss = self.rows.iter().any(|row| {
            matches!(
                row.verdict,
                HeadlessFootprintVerdict::RssExceeded | HeadlessFootprintVerdict::CpuAndRssExceeded
            )
        });
        let unavailable = self
            .rows
            .iter()
            .any(|row| row.verdict == HeadlessFootprintVerdict::Unavailable);
        if unavailable || (cpu && rss) {
            return HeadlessFootprintVerdict::CpuAndRssExceeded;
        }
        if cpu {
            return HeadlessFootprintVerdict::CpuExceeded;
        }
        if rss {
            return HeadlessFootprintVerdict::RssExceeded;
        }
        HeadlessFootprintVerdict::Within
    }

    /// True when the summary denies admission, either because a worker exceeded
    /// a budget, a worker could not be measured, or the row cap was hit.
    pub fn denied(&self) -> bool {
        self.status != SignalStatus::Available || self.exceeded || self.truncated
    }
}

/// Average CPU percentage from cumulative CPU milliseconds over wall-clock
/// milliseconds. A zero interval reports `0.0` instead of `NaN`/`inf`.
pub fn footprint_cpu_percent(cpu_ms: u64, wall_ms: u64) -> f64 {
    if wall_ms == 0 {
        return 0.0;
    }
    (cpu_ms as f64 / wall_ms as f64) * 100.0
}

/// Sample the current process' own CPU/RSS since construction. The baseline is
/// captured once so a long-lived worker reports an average CPU load instead of
/// a measurement spike on first use.
#[derive(Debug, Clone, Copy)]
pub struct HeadlessFootprintSampler {
    started_at_ms: u64,
    baseline_cpu_ms: Option<u64>,
}

impl Default for HeadlessFootprintSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl HeadlessFootprintSampler {
    pub fn new() -> Self {
        Self {
            started_at_ms: unix_time_ms(),
            baseline_cpu_ms: process_cpu_time_ms(),
        }
    }

    pub fn sample(&self, phase: FootprintPhase) -> HeadlessProcessFootprint {
        let pid = std::process::id();
        let now_ms = unix_time_ms();
        let wall_ms = now_ms.saturating_sub(self.started_at_ms);
        let cpu_delta_ms = process_cpu_time_ms()
            .zip(self.baseline_cpu_ms)
            .map(|(now, baseline)| now.saturating_sub(baseline));
        let resident_bytes = process_resident_bytes(pid);
        let measured = resident_bytes.is_some() && cpu_delta_ms.is_some();
        HeadlessProcessFootprint {
            pid,
            phase,
            cpu_percent: cpu_delta_ms
                .map(|cpu| footprint_cpu_percent(cpu, wall_ms))
                .unwrap_or(0.0),
            resident_bytes: resident_bytes.unwrap_or(0),
            status: if measured {
                SignalStatus::Available
            } else {
                SignalStatus::Unavailable
            },
            verdict: HeadlessFootprintVerdict::Within,
        }
    }
}

/// Process-wide baseline for self sampling. Initialized once so repeated
/// snapshots report an average CPU load over the process lifetime instead of
/// resetting to zero on every collection.
fn global_self_sampler() -> &'static HeadlessFootprintSampler {
    static SAMPLER: std::sync::OnceLock<HeadlessFootprintSampler> = std::sync::OnceLock::new();
    SAMPLER.get_or_init(HeadlessFootprintSampler::new)
}

/// Host network counters, cumulative since boot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NetworkSignal {
    pub received_bytes: Option<u64>,
    pub transmitted_bytes: Option<u64>,
    pub status: SignalStatus,
}

impl NetworkSignal {
    pub fn unavailable() -> Self {
        Self::default()
    }
}

/// One GPU reported by an `nvidia-smi` style probe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuDevice {
    pub name: String,
    pub utilization_percent: Option<f64>,
    pub memory_total_bytes: Option<u64>,
    pub memory_used_bytes: Option<u64>,
}

/// Best-effort discrete GPU inventory. Hosts without `nvidia-smi` report an
/// explicit typed `Unavailable` instead of fabricating zeroed devices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GpuSignal {
    pub devices: Vec<GpuDevice>,
    pub truncated: bool,
    pub status: SignalStatus,
}

impl GpuSignal {
    pub fn unavailable() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    pub collected_at_ms: u64,
    pub workspace: WorkspaceSummary,
    pub cpu: CpuSignal,
    pub memory: MemorySignal,
    pub disk: DiskSignal,
    pub process: ProcessSignal,
    #[serde(default)]
    pub network: NetworkSignal,
    #[serde(default)]
    pub gpu: GpuSignal,
    #[serde(default)]
    pub processes: ProcessSummary,
    /// Per-headless-process CPU/RSS footprint scored against the collector's
    /// budget. Kept in the same snapshot as host CPU/RSS so the resource gate
    /// sees worker consumption, not just whole-host totals.
    #[serde(default)]
    pub headless: HeadlessFootprintSummary,
}

/// Resource collector bound to one canonical workspace root.
#[derive(Debug, Clone)]
pub struct ResourceCollector {
    root: PathBuf,
    policy: WorkspaceScanPolicy,
    headless_budget: HeadlessFootprintBudget,
}

impl ResourceCollector {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ResourceError> {
        Self::with_policy(root, WorkspaceScanPolicy::default())
    }

    pub fn with_policy(
        root: impl AsRef<Path>,
        policy: WorkspaceScanPolicy,
    ) -> Result<Self, ResourceError> {
        policy.validate()?;
        let requested = root.as_ref();
        let metadata = fs::symlink_metadata(requested).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                ResourceError::NotFound(requested.to_path_buf())
            } else {
                ResourceError::Io(error)
            }
        })?;
        if metadata.file_type().is_symlink() {
            return Err(ResourceError::PathDenied(requested.to_path_buf()));
        }
        let canonical = requested.canonicalize().map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                ResourceError::NotFound(requested.to_path_buf())
            } else {
                ResourceError::Io(error)
            }
        })?;
        if !canonical.is_dir() {
            return Err(ResourceError::NotDirectory(canonical));
        }
        Ok(Self {
            root: canonical,
            policy,
            headless_budget: HeadlessFootprintBudget::default(),
        })
    }

    /// Override the per-headless-process CPU/RSS ceilings used by
    /// [`collect`](Self::collect). Invalid limits are rejected before any
    /// sampling so a host cannot silently disable the gate.
    #[allow(dead_code)]
    pub fn with_headless_budget(
        mut self,
        budget: HeadlessFootprintBudget,
    ) -> Result<Self, ResourceError> {
        budget.validate()?;
        self.headless_budget = budget;
        Ok(self)
    }

    #[allow(dead_code)]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[allow(dead_code)]
    pub fn policy(&self) -> WorkspaceScanPolicy {
        self.policy
    }

    #[allow(dead_code)]
    pub fn headless_budget(&self) -> HeadlessFootprintBudget {
        self.headless_budget
    }

    pub fn workspace_summary(&self) -> Result<WorkspaceSummary, ResourceError> {
        let mut summary = WorkspaceSummary {
            root: self.root.to_string_lossy().into_owned(),
            files: 0,
            directories: 1,
            nodes: 0,
            bytes: 0,
            truncated: false,
        };
        self.walk(&self.root, 0, &mut summary)?;
        Ok(summary)
    }

    pub fn collect(&self) -> Result<ResourceSnapshot, ResourceError> {
        Ok(ResourceSnapshot {
            collected_at_ms: unix_time_ms(),
            workspace: self.workspace_summary()?,
            cpu: cpu_signal(),
            memory: memory_signal(),
            disk: disk_signal(&self.root),
            process: process_signal(),
            network: network_signal(),
            gpu: gpu_signal(),
            processes: process_summary(),
            headless: self.headless_footprint(),
        })
    }

    /// Score every `--mode headless` process against the configured budget. The
    /// sampling process itself is added when the host's `ps` view does not list
    /// it, so a restricted container still reports its own footprint rather
    /// than an empty (falsely clean) gate.
    pub fn headless_footprint(&self) -> HeadlessFootprintSummary {
        headless_gate(self.headless_budget)
    }

    fn walk(
        &self,
        directory: &Path,
        depth: usize,
        summary: &mut WorkspaceSummary,
    ) -> Result<(), ResourceError> {
        if summary.nodes >= self.policy.max_nodes {
            summary.truncated = true;
            return Ok(());
        }
        summary.nodes += 1;
        if depth >= self.policy.max_depth {
            // A directory at the depth boundary is intentionally not opened.
            if directory.is_dir() {
                summary.truncated = true;
            }
            return Ok(());
        }
        // Iterate the directory stream directly: collecting all entries just
        // to sort them would defeat the global node/memory bound for a very
        // large directory.
        for item in fs::read_dir(directory)? {
            let entry = item?;
            if summary.nodes >= self.policy.max_nodes {
                summary.truncated = true;
                break;
            }
            let path = entry.path();
            // Re-read link metadata instead of calling `metadata`: a file can
            // be swapped for a symlink between `read_dir` and this point, and
            // following it would both escape the root and report the target's
            // size.
            let metadata = fs::symlink_metadata(&path)?;
            let file_type = metadata.file_type();
            // Never follow symlinks: this both avoids cycles and prevents a
            // workspace summary from exposing files outside its root.
            if file_type.is_symlink() {
                summary.truncated = true;
                continue;
            }
            if file_type.is_dir() {
                if summary.directories >= self.policy.max_directories {
                    summary.truncated = true;
                    break;
                }
                summary.directories += 1;
                self.walk(&path, depth + 1, summary)?;
            } else if file_type.is_file() {
                if summary.files >= self.policy.max_files {
                    summary.truncated = true;
                    break;
                }
                summary.files += 1;
                let size = metadata.len();
                if summary.bytes.saturating_add(size) > self.policy.max_bytes {
                    summary.bytes = self.policy.max_bytes;
                    summary.truncated = true;
                    break;
                }
                summary.bytes = summary.bytes.saturating_add(size);
                summary.nodes += 1;
            }
        }
        Ok(())
    }
}

fn cpu_signal() -> CpuSignal {
    let logical_cpus = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let load_one_minute = load_one_minute();
    CpuSignal {
        logical_cpus,
        load_one_minute,
        status: if load_one_minute.is_some() {
            SignalStatus::Available
        } else {
            SignalStatus::Unavailable
        },
    }
}

fn memory_signal() -> MemorySignal {
    #[cfg(target_os = "linux")]
    {
        let values = read_proc_meminfo();
        let total_bytes = values
            .get("MemTotal")
            .and_then(|value| value.checked_mul(1024));
        let available_bytes = values
            .get("MemAvailable")
            .and_then(|value| value.checked_mul(1024));
        return MemorySignal {
            total_bytes,
            available_bytes,
            status: if total_bytes.is_some() && available_bytes.is_some() {
                SignalStatus::Available
            } else {
                SignalStatus::Unavailable
            },
        };
    }
    #[cfg(target_os = "macos")]
    {
        macos_memory_signal()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        MemorySignal {
            total_bytes: None,
            available_bytes: None,
            status: SignalStatus::Unavailable,
        }
    }
}

fn disk_signal(root: &Path) -> DiskSignal {
    #[cfg(unix)]
    {
        unix_disk_signal(root)
    }
    #[cfg(not(unix))]
    {
        // Windows and other targets retain an explicit typed fallback rather
        // than spawning a platform command or claiming zero capacity.
        DiskSignal {
            total_bytes: None,
            available_bytes: None,
            status: SignalStatus::Unavailable,
        }
    }
}

#[cfg(unix)]
fn unix_disk_signal(root: &Path) -> DiskSignal {
    let Some(path) = root
        .to_str()
        .and_then(|value| std::ffi::CString::new(value).ok())
    else {
        return DiskSignal {
            total_bytes: None,
            available_bytes: None,
            status: SignalStatus::Unavailable,
        };
    };
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is a valid NUL-terminated path and `stats` points to
    // writable storage for the OS-provided structure.
    let result = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return DiskSignal {
            total_bytes: None,
            available_bytes: None,
            status: SignalStatus::Unavailable,
        };
    }
    // SAFETY: statvfs initialized the structure when it returned success.
    let stats = unsafe { stats.assume_init() };
    let block_size = stats.f_frsize;
    let total_bytes = u64::from(stats.f_blocks).checked_mul(block_size);
    let available_bytes = u64::from(stats.f_bavail).checked_mul(block_size);
    DiskSignal {
        status: if total_bytes.is_some() && available_bytes.is_some() {
            SignalStatus::Available
        } else {
            SignalStatus::Unavailable
        },
        total_bytes,
        available_bytes,
    }
}

fn process_signal() -> ProcessSignal {
    let pid = std::process::id();
    #[cfg(target_os = "linux")]
    let resident_bytes = read_proc_statm_resident();
    #[cfg(all(unix, not(target_os = "linux")))]
    let resident_bytes = unix_ps_resident(pid);
    #[cfg(not(unix))]
    let resident_bytes: Option<u64> = None;
    ProcessSignal {
        pid,
        resident_bytes,
        status: if resident_bytes.is_some() {
            SignalStatus::Available
        } else {
            SignalStatus::Unavailable
        },
    }
}

fn network_signal() -> NetworkSignal {
    #[cfg(target_os = "linux")]
    {
        linux_network_signal()
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        bsd_network_signal()
    }
    #[cfg(not(unix))]
    {
        NetworkSignal::unavailable()
    }
}

#[cfg(target_os = "linux")]
fn linux_network_signal() -> NetworkSignal {
    let Ok(text) = fs::read_to_string("/proc/net/dev") else {
        return NetworkSignal::unavailable();
    };
    let mut received = 0u64;
    let mut transmitted = 0u64;
    let mut saw = false;
    for line in text.lines().skip(2) {
        let Some((_, counters)) = line.split_once(':') else {
            continue;
        };
        // Receive columns: bytes packets errs drop fifo frame compressed
        // multicast; transmit bytes are the ninth value.
        let values: Vec<&str> = counters.split_whitespace().collect();
        if values.len() < 16 {
            continue;
        }
        let (Ok(rx), Ok(tx)) = (values[0].parse::<u64>(), values[8].parse::<u64>()) else {
            continue;
        };
        received = received.saturating_add(rx);
        transmitted = transmitted.saturating_add(tx);
        saw = true;
    }
    if !saw {
        return NetworkSignal::unavailable();
    }
    NetworkSignal {
        received_bytes: Some(received),
        transmitted_bytes: Some(transmitted),
        status: SignalStatus::Available,
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn bsd_network_signal() -> NetworkSignal {
    let mut received = 0u64;
    let mut transmitted = 0u64;
    let mut saw = false;
    let mut interfaces: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: `getifaddrs` fills a heap list that `freeifaddrs` releases.
    if unsafe { libc::getifaddrs(&mut interfaces) } != 0 || interfaces.is_null() {
        return NetworkSignal::unavailable();
    }
    let mut cursor = interfaces;
    while !cursor.is_null() {
        // SAFETY: `cursor` walks the list returned by `getifaddrs`.
        let entry = unsafe { &*cursor };
        if !entry.ifa_addr.is_null()
            && !entry.ifa_data.is_null()
            && i32::from(unsafe { (*entry.ifa_addr).sa_family }) == libc::AF_LINK
        {
            // SAFETY: AF_LINK entries carry an `if_data` payload.
            let data = unsafe { &*(entry.ifa_data as *const libc::if_data) };
            received = received.saturating_add(u64::from(data.ifi_ibytes));
            transmitted = transmitted.saturating_add(u64::from(data.ifi_obytes));
            saw = true;
        }
        cursor = entry.ifa_next;
    }
    // SAFETY: `interfaces` is the head of the list allocated above.
    unsafe { libc::freeifaddrs(interfaces) };
    if !saw {
        return NetworkSignal::unavailable();
    }
    NetworkSignal {
        received_bytes: Some(received),
        transmitted_bytes: Some(transmitted),
        status: SignalStatus::Available,
    }
}

fn gpu_signal() -> GpuSignal {
    #[cfg(unix)]
    {
        nvidia_smi_signal()
    }
    #[cfg(not(unix))]
    {
        GpuSignal::unavailable()
    }
}

#[cfg(unix)]
fn nvidia_smi_signal() -> GpuSignal {
    if !command_on_path("nvidia-smi") {
        return GpuSignal::unavailable();
    }
    let Ok(output) = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,utilization.gpu,memory.total,memory.used",
            "--format=csv,noheader,nounits",
        ])
        .output()
    else {
        return GpuSignal::unavailable();
    };
    if !output.status.success() {
        return GpuSignal::unavailable();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();
    let mut truncated = false;
    for line in text.lines() {
        if devices.len() >= MAX_GPU_DEVICES {
            truncated = true;
            break;
        }
        let parts: Vec<&str> = line.split(',').map(str::trim).collect();
        if parts.len() < 4 {
            continue;
        }
        let name: String = parts[0].chars().take(MAX_DEVICE_LABEL_CHARS).collect();
        devices.push(GpuDevice {
            name,
            utilization_percent: parts[1].parse().ok(),
            memory_total_bytes: parts[2]
                .parse::<u64>()
                .ok()
                .and_then(|mb| mb.checked_mul(1024 * 1024)),
            memory_used_bytes: parts[3]
                .parse::<u64>()
                .ok()
                .and_then(|mb| mb.checked_mul(1024 * 1024)),
        });
    }
    if devices.is_empty() {
        return GpuSignal::unavailable();
    }
    GpuSignal {
        devices,
        truncated,
        status: SignalStatus::Available,
    }
}

#[cfg(unix)]
fn command_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| directory.join(name).is_file())
}

fn process_summary() -> ProcessSummary {
    #[cfg(unix)]
    {
        unix_process_summary()
    }
    #[cfg(not(unix))]
    {
        ProcessSummary::unavailable()
    }
}

#[cfg(unix)]
fn unix_process_summary() -> ProcessSummary {
    // `ps` is the portable read-only owner on both Linux and macOS. The
    // query is bounded by `MAX_PROCESSES_SCANNED` lines regardless of output.
    let Ok(output) = std::process::Command::new("ps")
        .args(["-axo", "pid=,rss=,comm="])
        .output()
    else {
        return ProcessSummary::unavailable();
    };
    if !output.status.success() {
        return ProcessSummary::unavailable();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut totals = std::collections::BTreeMap::<ProcessClass, (usize, u64)>::new();
    let mut total = 0usize;
    let mut resident_bytes = 0u64;
    let mut truncated = false;
    for line in text.lines() {
        if total >= MAX_PROCESSES_SCANNED {
            truncated = true;
            break;
        }
        let Some((pid, rest)) = line.trim_start().split_once(char::is_whitespace) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some((rss, command)) = rest.split_once(char::is_whitespace) else {
            continue;
        };
        let (Ok(_pid), Ok(rss_kib)) = (pid.parse::<u32>(), rss.parse::<u64>()) else {
            continue;
        };
        let class = ProcessClass::classify(command.trim());
        let bytes = rss_kib.saturating_mul(1024);
        let entry = totals.entry(class).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.saturating_add(bytes);
        total = total.saturating_add(1);
        resident_bytes = resident_bytes.saturating_add(bytes);
    }
    if total == 0 {
        return ProcessSummary::unavailable();
    }
    let mut rows: Vec<ProcessClassRow> = totals
        .into_iter()
        .map(|(class, (count, bytes))| ProcessClassRow {
            class,
            count,
            resident_bytes: bytes,
        })
        .collect();
    rows.sort_by(|left, right| {
        right
            .resident_bytes
            .cmp(&left.resident_bytes)
            .then_with(|| left.class.cmp(&right.class))
    });
    if rows.len() > MAX_PROCESS_ROWS {
        rows.truncate(MAX_PROCESS_ROWS);
        truncated = true;
    }
    ProcessSummary {
        total,
        resident_bytes,
        rows,
        truncated,
        status: SignalStatus::Available,
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn unix_ps_resident(pid: u32) -> Option<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let kib: u64 = text.trim().parse().ok()?;
    kib.checked_mul(1024)
}

/// Resident bytes for any PID, not just the current process. Falls back to the
/// platform owner (`/proc` on Linux, `ps` elsewhere) and degrades to `None`
/// rather than fabricating a value.
fn process_resident_bytes(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        read_proc_pid_statm_resident(pid)
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        unix_ps_resident(pid)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

/// Cumulative CPU time (user + system) of the current process in milliseconds.
/// Best-effort: platforms without `getrusage` return `None` and the caller
/// reports the footprint as unavailable instead of claiming zero CPU.
fn process_cpu_time_ms() -> Option<u64> {
    #[cfg(unix)]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        // SAFETY: `getrusage` writes the requested `RUSAGE_SELF` accounting into
        // the provided structure and returns 0 on success.
        let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
        if result != 0 {
            return None;
        }
        // SAFETY: a successful `getrusage` initialized the structure.
        let usage = unsafe { usage.assume_init() };
        timeval_to_ms(&usage.ru_utime).checked_add(timeval_to_ms(&usage.ru_stime))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

#[cfg(unix)]
fn timeval_to_ms(value: &libc::timeval) -> u64 {
    let seconds = u64::try_from(value.tv_sec).unwrap_or(0);
    let micros = u64::try_from(value.tv_usec).unwrap_or(0);
    seconds
        .saturating_mul(1000)
        .saturating_add(micros / 1000)
}

/// Parse one `ps`-style duration (`MM:SS`, `HH:MM:SS`, or `DD-HH:MM:SS`) into
/// seconds. Unknown spellings return `None` so a malformed row is skipped
/// rather than producing a bogus CPU percentage.
fn parse_ps_duration_seconds(text: &str) -> Option<f64> {
    let (days, clock) = match text.split_once('-') {
        Some((days, rest)) => (days.parse::<u64>().ok()?, rest),
        None => (0, text),
    };
    let mut parts = clock.split(':').rev();
    let seconds: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next().unwrap_or("0").parse().ok()?;
    let hours: f64 = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let total = (days as f64) * 86_400.0 + hours * 3_600.0 + minutes * 60.0 + seconds;
    total.is_finite().then_some(total)
}

/// True when a `ps` command line belongs to a `--mode headless` worker. The
/// match is deliberately token-bounded so an unrelated process that merely
/// mentions the word (for example a shell editing this file) is ignored.
fn is_headless_command_line(args: &str) -> bool {
    args.split_whitespace().any(|token| token == "headless")
}

/// Enumerate `--mode headless` processes and score each against `budget`. The
/// scan is bounded by [`MAX_PROCESSES_SCANNED`] rows and degrades to an
/// explicit `Unavailable` summary when `ps` is missing or unrestricted.
pub fn headless_process_summary(budget: HeadlessFootprintBudget) -> HeadlessFootprintSummary {
    if budget.validate().is_err() {
        return HeadlessFootprintSummary::unavailable();
    }
    #[cfg(unix)]
    {
        let Ok(output) = std::process::Command::new("ps")
            .args(["-axo", "pid=,rss=,time=,etime=,args="])
            .output()
        else {
            return HeadlessFootprintSummary::unavailable();
        };
        if !output.status.success() {
            return HeadlessFootprintSummary::unavailable();
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let rows = parse_headless_rows(&text, budget);
        HeadlessFootprintSummary::from_processes(budget, rows)
    }
    #[cfg(not(unix))]
    {
        HeadlessFootprintSummary::unavailable()
    }
}

/// Sample the current process with the process-wide CPU baseline and score it
/// against `phase`. Public so a headless host can report its own footprint
/// without enumerating unrelated workers.
pub fn sample_own_footprint(phase: FootprintPhase) -> HeadlessProcessFootprint {
    global_self_sampler().sample(phase)
}

/// Gate decision for the running host: every `--mode headless` process plus the
/// current process scored against one budget. This is the same view
/// [`ResourceCollector::collect`] embeds in a snapshot; a caller that only has
/// a budget (for example the headless runtime) can use it directly.
pub fn headless_gate(budget: HeadlessFootprintBudget) -> HeadlessFootprintSummary {
    if budget.validate().is_err() {
        return HeadlessFootprintSummary::unavailable();
    }
    let mut summary = headless_process_summary(budget);
    if summary.status == SignalStatus::Available
        && !summary.rows.iter().any(|row| row.pid == std::process::id())
    {
        let mut row = global_self_sampler().sample(FootprintPhase::Idle);
        if row.cpu_percent > budget.idle_cpu_percent_max {
            row.phase = FootprintPhase::Busy;
        }
        let mut rows = summary.rows;
        rows.push(row);
        summary = HeadlessFootprintSummary::from_processes(budget, rows);
    }
    summary
}

#[cfg(unix)]
fn parse_headless_rows(
    text: &str,
    budget: HeadlessFootprintBudget,
) -> Vec<HeadlessProcessFootprint> {
    let mut rows = Vec::new();
    for (scanned, line) in text.lines().enumerate() {
        if rows.len() >= MAX_HEADLESS_FOOTPRINT_ROWS || scanned >= MAX_PROCESSES_SCANNED {
            break;
        }
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(rss), Some(cpu), Some(elapsed)) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            continue;
        };
        let command: Vec<&str> = fields.collect();
        if command.is_empty() || !is_headless_command_line(&command.join(" ")) {
            continue;
        }
        let (Ok(pid), Some(rss_kib), Some(cpu_seconds), Some(elapsed_seconds)) = (
            pid.parse::<u32>(),
            rss.parse::<u64>().ok(),
            parse_ps_duration_seconds(cpu),
            parse_ps_duration_seconds(elapsed),
        ) else {
            continue;
        };
        let wall_ms = (elapsed_seconds * 1000.0).max(0.0) as u64;
        let cpu_ms = (cpu_seconds * 1000.0).max(0.0) as u64;
        let cpu_percent = footprint_cpu_percent(cpu_ms, wall_ms);
        let phase = if cpu_percent > budget.idle_cpu_percent_max {
            FootprintPhase::Busy
        } else {
            FootprintPhase::Idle
        };
        rows.push(HeadlessProcessFootprint {
            pid,
            phase,
            cpu_percent,
            resident_bytes: rss_kib.saturating_mul(1024),
            status: SignalStatus::Available,
            verdict: HeadlessFootprintVerdict::Within,
        });
    }
    rows
}

fn load_one_minute() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        let text = fs::read_to_string("/proc/loadavg").ok()?;
        return text.split_whitespace().next()?.parse().ok();
    }
    #[cfg(target_os = "macos")]
    {
        macos_load_one_minute()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn macos_load_one_minute() -> Option<f64> {
    // `vm.loadavg` is an array of three C `double`s (1/5/15 minute). Reading it
    // through `sysctlbyname` avoids spawning a process on every refresh.
    let mut value = [0.0f64; 3];
    let mut size = std::mem::size_of_val(&value);
    let name = b"vm.loadavg\0";
    // SAFETY: `name` is NUL-terminated, `value`/`size` describe a writable
    // buffer of exactly `size` bytes, and the fourth/fifth arguments are null.
    let result = unsafe {
        libc::sysctlbyname(
            name.as_ptr() as *const libc::c_char,
            value.as_mut_ptr() as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 || size < std::mem::size_of::<f64>() {
        return None;
    }
    Some(value[0])
}

#[cfg(target_os = "macos")]
fn macos_sysctl_u64(name: &str) -> Option<u64> {
    let c_name = std::ffi::CString::new(name).ok()?;
    let mut value = 0u64;
    let mut size = std::mem::size_of::<u64>();
    // SAFETY: `c_name` is NUL-terminated and `value`/`size` describe a
    // writable buffer of exactly `size` bytes.
    let result = unsafe {
        libc::sysctlbyname(
            c_name.as_ptr(),
            &mut value as *mut u64 as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 || size < std::mem::size_of::<u64>() {
        return None;
    }
    Some(value)
}

#[cfg(target_os = "macos")]
fn macos_memory_signal() -> MemorySignal {
    let total_bytes = macos_sysctl_u64("hw.memsize");
    let page_size = macos_sysctl_u64("hw.pagesize").unwrap_or(4096);
    let available_bytes = macos_available_bytes(page_size);
    MemorySignal {
        status: if total_bytes.is_some() && available_bytes.is_some() {
            SignalStatus::Available
        } else {
            SignalStatus::Unavailable
        },
        total_bytes,
        available_bytes,
    }
}

/// Best-effort "available" memory from `vm_stat`: free plus inactive plus
/// speculative pages, which approximates what the OS can reclaim without
/// swapping. Missing counters degrade to `None` instead of a fabricated value.
#[cfg(target_os = "macos")]
fn macos_available_bytes(page_size: u64) -> Option<u64> {
    let output = std::process::Command::new("vm_stat").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut pages = 0u64;
    let mut saw = false;
    for line in text.lines() {
        let Some((label, rest)) = line.split_once(':') else {
            continue;
        };
        let label = label.trim();
        if !matches!(
            label,
            "Pages free" | "Pages inactive" | "Pages speculative" | "Pages purgeable"
        ) {
            continue;
        }
        let value: u64 = rest
            .trim()
            .trim_end_matches('.')
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .ok()?;
        pages = pages.saturating_add(value);
        saw = true;
    }
    if !saw {
        return None;
    }
    pages.checked_mul(page_size)
}

#[cfg(target_os = "linux")]
fn read_proc_meminfo() -> std::collections::BTreeMap<String, u64> {
    let mut values = std::collections::BTreeMap::new();
    let Ok(text) = fs::read_to_string("/proc/meminfo") else {
        return values;
    };
    for line in text.lines() {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let Some(value) = rest.split_whitespace().next().and_then(|v| v.parse().ok()) else {
            continue;
        };
        values.insert(name.to_owned(), value);
    }
    values
}

#[cfg(target_os = "linux")]
fn read_proc_statm_resident() -> Option<u64> {
    read_proc_pid_statm_resident(std::process::id())
}

#[cfg(target_os = "linux")]
fn read_proc_pid_statm_resident(pid: u32) -> Option<u64> {
    let path = if pid == std::process::id() {
        "/proc/self/statm".to_owned()
    } else {
        format!("/proc/{pid}/statm")
    };
    let text = fs::read_to_string(path).ok()?;
    let pages = text.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    pages.checked_mul(page_size())
}

#[cfg(target_os = "linux")]
fn page_size() -> u64 {
    // Linux exposes this in the proc ABI only indirectly.  The conventional
    // 4 KiB value is safe as an approximation for governance signals and is
    // clearly marked as a best-effort metric in the public status field.
    4096
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
