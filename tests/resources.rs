#[path = "../src/resources.rs"]
mod resources;

use std::fs;

use resources::{
    FootprintPhase, HeadlessFootprintBudget, HeadlessFootprintSummary, HeadlessFootprintVerdict,
    HeadlessProcessFootprint, MAX_HEADLESS_CPU_PERCENT, MAX_HEADLESS_FOOTPRINT_ROWS,
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

#[test]
fn utilization_bars_are_fixed_width_and_clamped() {
    use resources::{MAX_BAR_WIDTH, render_bar, utilization_percent};

    assert_eq!(utilization_percent(0, 0), 0.0);
    assert_eq!(utilization_percent(1, 2), 50.0);
    assert_eq!(utilization_percent(3, 2), 100.0);

    let bar = render_bar(50.0, 8);
    assert_eq!(bar.chars().count(), 8);
    assert!(bar.starts_with("████"), "{bar}");
    assert!(render_bar(0.0, 4).chars().all(|c| c == '░'));
    assert!(render_bar(100.0, 4).chars().all(|c| c == '█'));
    assert_eq!(render_bar(50.0, 0), "");
    assert!(render_bar(50.0, MAX_BAR_WIDTH + 5).chars().count() <= MAX_BAR_WIDTH);
}

fn footprint(
    pid: u32,
    phase: FootprintPhase,
    cpu_percent: f64,
    resident_bytes: u64,
) -> HeadlessProcessFootprint {
    HeadlessProcessFootprint {
        pid,
        phase,
        cpu_percent,
        resident_bytes,
        status: SignalStatus::Available,
        verdict: HeadlessFootprintVerdict::Within,
    }
}

#[test]
fn headless_footprint_budget_rejects_unbounded_or_non_finite_limits() {
    let budget = HeadlessFootprintBudget::default();
    assert!(budget.validate().is_ok());
    assert!(matches!(
        HeadlessFootprintBudget {
            idle_rss_bytes_max: 0,
            ..budget
        }
        .validate(),
        Err(ResourceError::InvalidPolicy(_))
    ));
    assert!(matches!(
        HeadlessFootprintBudget {
            busy_rss_bytes_max: resources::MAX_HEADLESS_RSS_BYTES + 1,
            ..budget
        }
        .validate(),
        Err(ResourceError::InvalidPolicy(_))
    ));
    assert!(matches!(
        HeadlessFootprintBudget {
            idle_cpu_percent_max: f64::NAN,
            ..budget
        }
        .validate(),
        Err(ResourceError::InvalidPolicy(_))
    ));
    assert!(matches!(
        HeadlessFootprintBudget {
            busy_cpu_percent_max: MAX_HEADLESS_CPU_PERCENT + 1.0,
            ..budget
        }
        .validate(),
        Err(ResourceError::InvalidPolicy(_))
    ));
}

#[test]
fn headless_footprint_budget_classifies_cpu_and_rss_excess() {
    let budget = HeadlessFootprintBudget {
        idle_cpu_percent_max: 1.0,
        idle_rss_bytes_max: 1024,
        busy_cpu_percent_max: 50.0,
        busy_rss_bytes_max: 4096,
    };
    assert_eq!(
        budget.evaluate(&footprint(1, FootprintPhase::Idle, 0.5, 1024)),
        HeadlessFootprintVerdict::Within
    );
    assert_eq!(
        budget.evaluate(&footprint(2, FootprintPhase::Idle, 5.0, 1024)),
        HeadlessFootprintVerdict::CpuExceeded
    );
    assert_eq!(
        budget.evaluate(&footprint(3, FootprintPhase::Busy, 10.0, 8192)),
        HeadlessFootprintVerdict::RssExceeded
    );
    assert_eq!(
        budget.evaluate(&footprint(4, FootprintPhase::Busy, 99.0, 8192)),
        HeadlessFootprintVerdict::CpuAndRssExceeded
    );
    let mut unavailable = footprint(5, FootprintPhase::Idle, 0.0, 0);
    unavailable.status = SignalStatus::Unavailable;
    assert_eq!(
        budget.evaluate(&unavailable),
        HeadlessFootprintVerdict::Unavailable
    );
    assert!(HeadlessFootprintVerdict::Unavailable.denied());
    assert!(!HeadlessFootprintVerdict::Within.denied());
}

#[test]
fn headless_summary_aggregates_per_process_stats_and_flags_excess() {
    let budget = HeadlessFootprintBudget {
        idle_cpu_percent_max: 1.0,
        idle_rss_bytes_max: 1024,
        busy_cpu_percent_max: 50.0,
        busy_rss_bytes_max: 4096,
    };
    let summary = HeadlessFootprintSummary::from_processes(
        budget,
        vec![
            footprint(10, FootprintPhase::Idle, 0.1, 512),
            footprint(11, FootprintPhase::Idle, 0.2, 512),
            footprint(12, FootprintPhase::Busy, 75.0, 1024),
        ],
    );
    assert_eq!(summary.status, SignalStatus::Available);
    assert_eq!(summary.total, 3);
    assert_eq!(summary.resident_bytes, 2048);
    assert!((summary.cpu_percent - 75.3).abs() < 1e-9);
    assert!(summary.exceeded);
    assert_eq!(summary.exceeded_pids(), vec![12]);
    assert_eq!(summary.verdict(), HeadlessFootprintVerdict::CpuExceeded);
    assert!(!summary.truncated);
    assert!(summary.row(10).is_some());
    assert!(summary.row(99).is_none());
}

#[test]
fn headless_summary_bounds_process_rows_and_marks_truncation() {
    let rows = (0..MAX_HEADLESS_FOOTPRINT_ROWS + 5)
        .map(|index| {
            footprint(
                u32::try_from(index).unwrap() + 1,
                FootprintPhase::Idle,
                0.0,
                1024,
            )
        })
        .collect();
    let summary =
        HeadlessFootprintSummary::from_processes(HeadlessFootprintBudget::default(), rows);
    assert_eq!(summary.rows.len(), MAX_HEADLESS_FOOTPRINT_ROWS);
    assert!(summary.truncated);
    assert!(!summary.exceeded);
    assert!(summary.denied());
    assert_eq!(summary.total, MAX_HEADLESS_FOOTPRINT_ROWS);
    assert_eq!(summary.verdict(), HeadlessFootprintVerdict::Within);
}

#[test]
fn headless_summary_without_rows_is_a_clean_available_scan() {
    let summary =
        HeadlessFootprintSummary::from_processes(HeadlessFootprintBudget::default(), Vec::new());
    assert_eq!(summary.status, SignalStatus::Available);
    assert!(!summary.exceeded);
    assert_eq!(summary.total, 0);
    assert_eq!(summary.verdict(), HeadlessFootprintVerdict::Within);
    assert_eq!(
        HeadlessFootprintSummary::unavailable().verdict(),
        HeadlessFootprintVerdict::Unavailable
    );
}

#[test]
fn footprint_cpu_percent_is_zero_safe_and_average_based() {
    use resources::footprint_cpu_percent;

    assert_eq!(footprint_cpu_percent(0, 0), 0.0);
    assert_eq!(footprint_cpu_percent(500, 1000), 50.0);
    assert_eq!(footprint_cpu_percent(2000, 1000), 200.0);
    assert!(footprint_cpu_percent(500, 0).is_finite());
}

#[test]
fn footprint_sampler_reports_self_with_a_phase() {
    use resources::HeadlessFootprintSampler;

    let sampler = HeadlessFootprintSampler::new();
    let sample = sampler.sample(FootprintPhase::Busy);
    assert_eq!(sample.pid, std::process::id());
    assert_eq!(sample.phase, FootprintPhase::Busy);
    assert!(matches!(
        sample.status,
        SignalStatus::Available | SignalStatus::Unavailable
    ));
    if sample.status == SignalStatus::Available {
        assert!(sample.resident_bytes > 0);
    }
}

#[test]
fn collection_embeds_the_headless_footprint_gate() {
    let directory = tempdir().unwrap();
    let snapshot = ResourceCollector::new(directory.path())
        .unwrap()
        .collect()
        .unwrap();
    assert!(matches!(
        snapshot.headless.status,
        SignalStatus::Available | SignalStatus::Unavailable
    ));
    if snapshot.headless.status == SignalStatus::Available {
        assert!(snapshot.headless.rows.iter().any(|row| row.pid == std::process::id()));
    }
    let value = serde_json::to_value(&snapshot).unwrap();
    assert!(value.get("headless").is_some());
    assert!(value["headless"].get("rows").is_some());
}

#[test]
fn headless_gate_scores_the_current_process_against_a_budget() {
    let summary = resources::headless_gate(HeadlessFootprintBudget::default());
    assert!(matches!(
        summary.status,
        SignalStatus::Available | SignalStatus::Unavailable
    ));
    if summary.status == SignalStatus::Available {
        assert!(summary.rows.iter().any(|row| row.pid == std::process::id()));
    }
    let sample = resources::sample_own_footprint(FootprintPhase::Idle);
    assert_eq!(sample.pid, std::process::id());
}
