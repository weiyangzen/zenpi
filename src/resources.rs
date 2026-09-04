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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalStatus {
    Available,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    pub collected_at_ms: u64,
    pub workspace: WorkspaceSummary,
    pub cpu: CpuSignal,
    pub memory: MemorySignal,
    pub disk: DiskSignal,
    pub process: ProcessSignal,
}

/// Resource collector bound to one canonical workspace root.
#[derive(Debug, Clone)]
pub struct ResourceCollector {
    root: PathBuf,
    policy: WorkspaceScanPolicy,
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
        })
    }

    #[allow(dead_code)]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[allow(dead_code)]
    pub fn policy(&self) -> WorkspaceScanPolicy {
        self.policy
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
            disk: disk_signal(),
            process: process_signal(),
        })
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
    let load_one_minute = linux_load_one_minute();
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
    #[cfg(not(target_os = "linux"))]
    {
        MemorySignal {
            total_bytes: None,
            available_bytes: None,
            status: SignalStatus::Unavailable,
        }
    }
}

fn disk_signal() -> DiskSignal {
    // Portable std APIs expose file metadata but not filesystem capacity. A
    // missing disk signal is preferable to spawning `df` or linking libc.
    DiskSignal {
        total_bytes: None,
        available_bytes: None,
        status: SignalStatus::Unavailable,
    }
}

fn process_signal() -> ProcessSignal {
    let pid = std::process::id();
    #[cfg(target_os = "linux")]
    let resident_bytes = read_proc_statm_resident();
    #[cfg(not(target_os = "linux"))]
    let resident_bytes = None;
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

fn linux_load_one_minute() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        let text = fs::read_to_string("/proc/loadavg").ok()?;
        return text.split_whitespace().next()?.parse().ok();
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
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
    let text = fs::read_to_string("/proc/self/statm").ok()?;
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
