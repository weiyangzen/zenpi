#[path = "../src/resources.rs"]
mod resources;

use std::fs;

use resources::{
    MAX_PROCESS_ROWS, MAX_WORKSPACE_FILES, ProcessClass, ResourceCollector, ResourceError,
    SignalStatus, WorkspaceScanPolicy,
};
use tempfile::tempdir;

#[test]
fn bounded_workspace_summary_counts_files_and_bytes() {
    let directory = tempdir().unwrap();
    fs::create_dir(directory.path().join("src")).unwrap();
    fs::write(directory.path().join("README.md"), "hello").unwrap();
    fs::write(directory.path().join("src/lib.rs"), "fn main() {}\n").unwrap();
    let collector = ResourceCollector::new(directory.path()).unwrap();
    let summary = collector.workspace_summary().unwrap();
    assert_eq!(summary.files, 2);
    assert_eq!(summary.directories, 2);
    assert_eq!(summary.bytes, 18);
    assert!(!summary.truncated);
}

#[test]
fn scan_limits_are_enforced_without_unbounded_walk() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("a.txt"), "12345").unwrap();
    fs::write(directory.path().join("b.txt"), "67890").unwrap();
    let policy = WorkspaceScanPolicy {
        max_files: 1,
        max_directories: 2,
        max_nodes: 10,
        max_bytes: 5,
        max_depth: 4,
    };
    let collector = ResourceCollector::with_policy(directory.path(), policy).unwrap();
    let summary = collector.workspace_summary().unwrap();
    assert_eq!(summary.files, 1);
    assert_eq!(summary.bytes, 5);
    assert!(summary.truncated);
}

#[test]
fn invalid_policy_and_roots_are_typed() {
    let directory = tempdir().unwrap();
    let error = ResourceCollector::with_policy(
        directory.path(),
        WorkspaceScanPolicy {
            max_files: MAX_WORKSPACE_FILES + 1,
            ..WorkspaceScanPolicy::default()
        },
    )
    .unwrap_err();
    assert!(matches!(error, ResourceError::InvalidPolicy(_)));
    let missing = directory.path().join("missing");
    assert!(matches!(
        ResourceCollector::new(missing),
        Err(ResourceError::NotFound(_))
    ));
}

#[test]
fn collection_returns_process_and_cpu_signals_with_graceful_fallbacks() {
    let directory = tempdir().unwrap();
    let snapshot = ResourceCollector::new(directory.path())
        .unwrap()
        .collect()
        .unwrap();
    assert!(snapshot.cpu.logical_cpus >= 1);
    assert!(snapshot.process.pid > 0);
    assert!(matches!(
        snapshot.disk.status,
        SignalStatus::Available | SignalStatus::Unavailable
    ));
    if snapshot.disk.status == SignalStatus::Available {
        assert!(
            snapshot.disk.total_bytes.unwrap_or(0) >= snapshot.disk.available_bytes.unwrap_or(0)
        );
    }
    assert!(snapshot.collected_at_ms > 0);
}

#[test]
fn process_classes_merge_lsp_mcp_and_common_workers() {
    assert_eq!(
        ProcessClass::classify("/usr/local/bin/rust-analyzer"),
        ProcessClass::Lsp
    );
    assert_eq!(ProcessClass::classify("gopls"), ProcessClass::Lsp);
    assert_eq!(
        ProcessClass::classify("typescript-language-server"),
        ProcessClass::Lsp
    );
    assert_eq!(ProcessClass::classify("my-mcp-bridge"), ProcessClass::Mcp);
    assert_eq!(
        ProcessClass::classify("/opt/homebrew/bin/node"),
        ProcessClass::Node
    );
    assert_eq!(ProcessClass::classify("cargo build"), ProcessClass::Rust);
    assert_eq!(ProcessClass::classify("/bin/zsh"), ProcessClass::Shell);
    assert_eq!(ProcessClass::classify("/usr/bin/git"), ProcessClass::Git);
    assert_eq!(ProcessClass::classify("zenpi"), ProcessClass::Zenpi);
    assert_eq!(ProcessClass::classify("opencode"), ProcessClass::OpenCode);
    // Unknown processes collapse into the bounded catch-all class.
    assert_eq!(ProcessClass::classify("coredns"), ProcessClass::Other);
}

#[test]
fn collection_exposes_network_gpu_and_merged_process_signals() {
    let directory = tempdir().unwrap();
    let snapshot = ResourceCollector::new(directory.path())
        .unwrap()
        .collect()
        .unwrap();
    assert!(matches!(
        snapshot.network.status,
        SignalStatus::Available | SignalStatus::Unavailable
    ));
    if snapshot.network.status == SignalStatus::Available {
        assert!(snapshot.network.received_bytes.is_some());
        assert!(snapshot.network.transmitted_bytes.is_some());
    }
    assert!(matches!(
        snapshot.gpu.status,
        SignalStatus::Available | SignalStatus::Unavailable
    ));
    if snapshot.gpu.status == SignalStatus::Available {
        assert!(!snapshot.gpu.devices.is_empty());
    }
    assert!(snapshot.processes.rows.len() <= MAX_PROCESS_ROWS);
    if snapshot.processes.status == SignalStatus::Available {
        assert!(snapshot.processes.total >= 1);
        // Same-class statistics are complete and merged: the row counts add up
        // to the process total, with one row per class.
        let summed: usize = snapshot.processes.rows.iter().map(|row| row.count).sum();
        assert_eq!(summed, snapshot.processes.total);
        for row in &snapshot.processes.rows {
            assert!(row.count >= 1);
        }
    }
}

#[cfg(unix)]
#[test]
fn workspace_walk_does_not_follow_symlinks() {
    use std::os::unix::fs::symlink;

    let workspace = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    symlink(outside.path(), workspace.path().join("external")).unwrap();
    let summary = ResourceCollector::new(workspace.path())
        .unwrap()
        .workspace_summary()
        .unwrap();
    assert_eq!(summary.files, 0);
    assert!(summary.truncated);
}
