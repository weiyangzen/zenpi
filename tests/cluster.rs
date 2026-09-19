use std::cell::RefCell;
use std::collections::BTreeSet;

use zenpi::cluster::{
    ClusterAuthorization, ClusterControlPlane, ClusterError, DispatchBackend, HostCapacity,
    MAX_BUS_HOST_IPS, MAX_WORKERS_PER_HOST, RemoteWorker, SshDispatchBackend, WorkerProbe,
    WorkerSpec, WorkerState, cluster_resource_bus, lan_bus_section, shell_quote,
};
use zenpi::net_probe::{HostResources, LanHost, LanSnapshot, NetCredential, NetCredentials};

fn resources(cpus: usize, memory: u64) -> HostResources {
    HostResources {
        os: Some("Ubuntu 24.04".into()),
        kernel: Some("6.8.0".into()),
        cpu: Some("AMD Ryzen 9 7945HX".into()),
        logical_cpus: Some(cpus),
        memory_total_bytes: Some(memory),
        disk_total_bytes: Some(3_700_000_000_000),
        disk_available_bytes: Some(2_000_000_000_000),
        gpus: Vec::new(),
        services: vec!["ssh".into()],
    }
}

fn host(ip: &str, ports: &[u16], resources: Option<HostResources>) -> LanHost {
    let mut host = LanHost::new(ip);
    host.mac = Some("58:47:ca:00:00:01".into());
    host.open_ports = ports.to_vec();
    host.resources = resources;
    host
}

fn snapshot(hosts: Vec<LanHost>) -> LanSnapshot {
    LanSnapshot {
        local_ip: "10.0.0.14".into(),
        gateway_ip: Some("10.0.0.1".into()),
        hosts,
        blocks: Vec::new(),
        scanned_at_ms: 0,
        truncated: false,
    }
}

fn credentials() -> NetCredentials {
    NetCredentials::from_entries(vec![NetCredential::new("netuser", "test-secret")])
}

#[derive(Default)]
struct FixtureBackend {
    state: RefCell<FixtureState>,
}

#[derive(Default)]
struct FixtureState {
    spawned: Vec<(String, WorkerSpec)>,
    alive: BTreeSet<String>,
    next_pid: u32,
    spawn_failure: Option<String>,
    reclaim_failure: Option<String>,
}

impl FixtureBackend {
    fn with_spawn_failure(reason: &str) -> Self {
        let backend = Self::default();
        backend.state.borrow_mut().spawn_failure = Some(reason.into());
        backend
    }

    fn spawned_hosts(&self) -> Vec<String> {
        self.state
            .borrow()
            .spawned
            .iter()
            .map(|(host, _)| host.clone())
            .collect()
    }

    fn alive_count(&self) -> usize {
        self.state.borrow().alive.len()
    }

    fn reclaim_with_failure(reason: &str) -> Self {
        let backend = Self::default();
        backend.state.borrow_mut().reclaim_failure = Some(reason.into());
        backend
    }
}

impl DispatchBackend for FixtureBackend {
    fn spawn_worker(&self, host: &str, spec: &WorkerSpec) -> Result<RemoteWorker, String> {
        let mut state = self.state.borrow_mut();
        if let Some(reason) = &state.spawn_failure {
            return Err(reason.clone());
        }
        state.next_pid += 1;
        let pid = 10_000 + state.next_pid;
        let remote_id = format!("{host}-{pid}");
        state.spawned.push((host.to_owned(), spec.clone()));
        state.alive.insert(remote_id.clone());
        Ok(RemoteWorker {
            remote_id,
            host: host.to_owned(),
            pid,
        })
    }

    fn probe_worker(&self, remote: &RemoteWorker) -> Result<WorkerProbe, String> {
        let state = self.state.borrow();
        Ok(WorkerProbe {
            running: state.alive.contains(&remote.remote_id),
            cpu_percent: Some(0.5),
            rss_bytes: Some(2 * 1024 * 1024),
        })
    }

    fn reclaim_worker(&self, remote: &RemoteWorker) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        if let Some(reason) = &state.reclaim_failure {
            return Err(reason.clone());
        }
        state.alive.remove(&remote.remote_id);
        Ok(())
    }
}

#[test]
fn lan_address_helper_rejects_public_and_cgnat() {
    assert!(zenpi::net_probe::is_lan_address("10.20.30.38"));
    assert!(zenpi::net_probe::is_lan_address("10.0.0.55"));
    assert!(zenpi::net_probe::is_lan_address("172.16.5.4"));
    assert!(!zenpi::net_probe::is_lan_address("172.32.5.4"));
    assert!(!zenpi::net_probe::is_lan_address("8.8.8.8"));
    assert!(!zenpi::net_probe::is_lan_address("100.64.0.1"));
    assert!(!zenpi::net_probe::is_lan_address("127.0.0.1"));
}

#[test]
fn capacity_reserves_headroom_for_the_host() {
    let capacity = HostCapacity::from_resources(Some(&resources(8, 8 * 1024 * 1024 * 1024)));
    assert_eq!(capacity.usable_cpu_slots(), 7);
    assert_eq!(capacity.usable_memory_bytes(), 6 * 1024 * 1024 * 1024);
    assert_eq!(capacity.available_cpu_slots(), 7);

    let spec = WorkerSpec::headless("w", "s").with_memory_bytes(1024 * 1024 * 1024);
    let mut capacity = capacity;
    capacity.reserve(&spec);
    assert_eq!(capacity.available_cpu_slots(), 6);
    capacity.release(&spec);
    assert_eq!(capacity.available_cpu_slots(), 7);
    assert_eq!(capacity.available_memory_bytes(), 6 * 1024 * 1024 * 1024);
}

#[test]
fn capacity_falls_back_to_defaults_without_probe_data() {
    let capacity = HostCapacity::from_resources(None);
    assert!(capacity.logical_cpus >= 1);
    assert!(capacity.memory_total_bytes >= 1024 * 1024 * 1024);
    assert_eq!(capacity.gpus, 0);
}

#[test]
fn admit_filters_non_lan_and_non_ssh_hosts() {
    let mut plane = ClusterControlPlane::new(
        credentials(),
        ClusterAuthorization::from_entries(["10.0.0.55", "10.0.0.56"]),
    );
    let admitted = plane.admit(&snapshot(vec![
        host("10.0.0.55", &[22], Some(resources(32, 96_000_000_000))),
        host("10.0.0.56", &[22], Some(resources(32, 96_000_000_000))),
        host("8.8.8.8", &[22], None),
        host("10.0.0.60", &[80, 5000], None),
    ]));
    assert_eq!(admitted, 2);
    assert!(plane.host("10.0.0.55").unwrap().authorized);
    assert!(plane.host("10.0.0.55").unwrap().has_credentials);
    assert!(plane.host("8.8.8.8").is_none());
    assert!(plane.host("10.0.0.60").is_none());
}

#[test]
fn dispatch_requires_explicit_authorization() {
    let mut plane = ClusterControlPlane::new(credentials(), ClusterAuthorization::none());
    plane.admit(&snapshot(vec![host(
        "10.0.0.55",
        &[22],
        Some(resources(32, 96_000_000_000)),
    )]));
    let backend = FixtureBackend::default();
    let error = plane
        .dispatch(&WorkerSpec::headless("job", "s1"), &backend)
        .unwrap_err();
    assert!(matches!(error, ClusterError::Unauthorized(_)), "{error:?}");
    assert!(backend.spawned_hosts().is_empty());
}

#[test]
fn dispatch_requires_credentials() {
    let mut plane = ClusterControlPlane::new(
        NetCredentials::empty(),
        ClusterAuthorization::from_entries(["10.0.0.55"]),
    );
    plane.admit(&snapshot(vec![host(
        "10.0.0.55",
        &[22],
        Some(resources(32, 96_000_000_000)),
    )]));
    let backend = FixtureBackend::default();
    let error = plane
        .dispatch(&WorkerSpec::headless("job", "s1"), &backend)
        .unwrap_err();
    assert_eq!(error, ClusterError::NoCredentials);
}

#[test]
fn dispatch_places_on_authorized_host_by_capacity() {
    let mut plane = ClusterControlPlane::new(
        credentials(),
        ClusterAuthorization::from_entries(["10.0.0.55", "10.0.0.56"]),
    );
    plane.admit(&snapshot(vec![
        host("10.0.0.55", &[22], Some(resources(16, 32_000_000_000))),
        host("10.0.0.56", &[22], Some(resources(64, 186_000_000_000))),
    ]));
    let backend = FixtureBackend::default();
    let id = plane
        .dispatch(&WorkerSpec::headless("train", "s1"), &backend)
        .unwrap();
    assert_eq!(backend.spawned_hosts(), vec!["10.0.0.56".to_owned()]);
    let record = plane.worker(&id).unwrap();
    assert_eq!(record.state, WorkerState::Running);
    assert_eq!(record.host, "10.0.0.56");
    assert!(record.remote.is_some());
    assert_eq!(plane.host_worker_count("10.0.0.56"), 1);
    assert!(
        plane
            .host("10.0.0.56")
            .unwrap()
            .capacity
            .available_cpu_slots()
            < 64
    );
}

#[test]
fn dispatch_prefers_gpu_host_when_requested() {
    let mut with_gpu = resources(32, 96_000_000_000);
    with_gpu.gpus = vec!["RTX 4090".into()];
    let mut plane = ClusterControlPlane::new(
        credentials(),
        ClusterAuthorization::from_entries(["10.0.0.55", "10.0.0.56"]),
    );
    plane.admit(&snapshot(vec![
        host("10.0.0.55", &[22], Some(resources(64, 186_000_000_000))),
        host("10.0.0.56", &[22], Some(with_gpu)),
    ]));
    let backend = FixtureBackend::default();
    plane
        .dispatch(&WorkerSpec::headless("train", "s1").with_gpu(), &backend)
        .unwrap();
    assert_eq!(backend.spawned_hosts(), vec!["10.0.0.56".to_owned()]);
}

#[test]
fn reclaim_releases_capacity_and_marks_reclaimed() {
    let mut plane = ClusterControlPlane::new(
        credentials(),
        ClusterAuthorization::from_entries(["10.0.0.55"]),
    );
    plane.admit(&snapshot(vec![host(
        "10.0.0.55",
        &[22],
        Some(resources(4, 8 * 1024 * 1024 * 1024)),
    )]));
    let backend = FixtureBackend::default();
    let id = plane
        .dispatch(&WorkerSpec::headless("job", "s1"), &backend)
        .unwrap();
    assert!(plane.host("10.0.0.55").unwrap().capacity.cpu_slots_used > 0);
    assert_eq!(backend.alive_count(), 1);

    let state = plane.reclaim(&id, &backend).unwrap();
    assert_eq!(state, WorkerState::Reclaimed);
    assert_eq!(plane.worker(&id).unwrap().state, WorkerState::Reclaimed);
    assert_eq!(backend.alive_count(), 0);
    let host = plane.host("10.0.0.55").unwrap();
    assert_eq!(host.capacity.cpu_slots_used, 0);
    assert_eq!(host.capacity.memory_bytes_used, 0);
    assert_eq!(
        host.capacity.available_cpu_slots(),
        host.capacity.usable_cpu_slots()
    );
    // A repeated reclaim is idempotent.
    assert_eq!(
        plane.reclaim(&id, &backend).unwrap(),
        WorkerState::Reclaimed
    );
}

#[test]
fn reconcile_reclaims_exited_workers() {
    let mut plane = ClusterControlPlane::new(
        credentials(),
        ClusterAuthorization::from_entries(["10.0.0.55"]),
    );
    plane.admit(&snapshot(vec![host(
        "10.0.0.55",
        &[22],
        Some(resources(8, 16 * 1024 * 1024 * 1024)),
    )]));
    let backend = FixtureBackend::default();
    let id = plane
        .dispatch(&WorkerSpec::headless("job", "s1"), &backend)
        .unwrap();
    // Simulate the remote worker exiting on its own.
    backend.state.borrow_mut().alive.clear();
    assert_eq!(plane.reconcile(&backend), 1);
    assert_eq!(plane.worker(&id).unwrap().state, WorkerState::Reclaimed);
    assert_eq!(plane.host_worker_count("10.0.0.55"), 0);
}

#[test]
fn per_host_limit_blocks_extra_dispatch() {
    let mut plane = ClusterControlPlane::new(
        credentials(),
        ClusterAuthorization::from_entries(["10.0.0.55"]),
    );
    plane.admit(&snapshot(vec![host(
        "10.0.0.55",
        &[22],
        Some(resources(64, 128 * 1024 * 1024 * 1024)),
    )]));
    let backend = FixtureBackend::default();
    for index in 0..MAX_WORKERS_PER_HOST {
        plane
            .dispatch(
                &WorkerSpec::headless(format!("job{index}"), format!("s{index}"))
                    .with_memory_bytes(256 * 1024 * 1024),
                &backend,
            )
            .unwrap();
    }
    let error = plane
        .dispatch(&WorkerSpec::headless("overflow", "s"), &backend)
        .unwrap_err();
    assert!(
        matches!(error, ClusterError::HostWorkerLimit(_, limit) if limit == MAX_WORKERS_PER_HOST),
        "{error:?}"
    );
}

#[test]
fn revoked_authorization_blocks_new_dispatch() {
    let mut plane = ClusterControlPlane::new(
        credentials(),
        ClusterAuthorization::from_entries(["10.0.0.55"]),
    );
    plane.admit(&snapshot(vec![host(
        "10.0.0.55",
        &[22],
        Some(resources(32, 64 * 1024 * 1024 * 1024)),
    )]));
    assert!(plane.revoke("10.0.0.55"));
    assert!(!plane.host("10.0.0.55").unwrap().authorized);
    let backend = FixtureBackend::default();
    let error = plane
        .dispatch(&WorkerSpec::headless("job", "s1"), &backend)
        .unwrap_err();
    assert!(matches!(error, ClusterError::Unauthorized(_)), "{error:?}");
    // Re-authorizing restores dispatch without an admit round-trip.
    assert!(plane.authorize("10.0.0.55"));
    plane
        .dispatch(&WorkerSpec::headless("job", "s1"), &backend)
        .unwrap();
}

#[test]
fn backend_spawn_failure_is_fail_closed_and_releases_capacity() {
    let mut plane = ClusterControlPlane::new(
        credentials(),
        ClusterAuthorization::from_entries(["10.0.0.55"]),
    );
    plane.admit(&snapshot(vec![host(
        "10.0.0.55",
        &[22],
        Some(resources(8, 16 * 1024 * 1024 * 1024)),
    )]));
    let backend = FixtureBackend::with_spawn_failure("ssh refused");
    let error = plane
        .dispatch(&WorkerSpec::headless("job", "s1"), &backend)
        .unwrap_err();
    assert!(matches!(error, ClusterError::Backend(_)), "{error:?}");
    let failed = plane
        .workers()
        .find(|record| record.state == WorkerState::Failed)
        .expect("failed record");
    assert!(failed.message.as_deref().unwrap().contains("ssh refused"));
    assert_eq!(plane.host("10.0.0.55").unwrap().capacity.cpu_slots_used, 0);
}

#[test]
fn reclaim_failure_keeps_capacity_reserved() {
    let mut plane = ClusterControlPlane::new(
        credentials(),
        ClusterAuthorization::from_entries(["10.0.0.55"]),
    );
    plane.admit(&snapshot(vec![host(
        "10.0.0.55",
        &[22],
        Some(resources(8, 16 * 1024 * 1024 * 1024)),
    )]));
    let spawner = FixtureBackend::default();
    let id = plane
        .dispatch(&WorkerSpec::headless("job", "s1"), &spawner)
        .unwrap();
    let reserved = plane.host("10.0.0.55").unwrap().capacity.cpu_slots_used;
    assert!(reserved > 0);
    let failing = FixtureBackend::reclaim_with_failure("kill denied");
    let error = plane.reclaim(&id, &failing).unwrap_err();
    assert!(matches!(error, ClusterError::Backend(_)), "{error:?}");
    // Fail closed: the worker may still be alive, so capacity stays reserved.
    assert_eq!(
        plane.host("10.0.0.55").unwrap().capacity.cpu_slots_used,
        reserved
    );
    assert_eq!(plane.worker(&id).unwrap().state, WorkerState::Failed);
}

#[test]
fn snapshot_aggregates_and_never_leaks_credentials() {
    let mut plane = ClusterControlPlane::new(
        credentials(),
        ClusterAuthorization::from_entries(["10.0.0.55"]),
    );
    plane.admit(&snapshot(vec![
        host(
            "10.0.0.55",
            &[22],
            Some(resources(32, 64 * 1024 * 1024 * 1024)),
        ),
        host(
            "10.0.0.56",
            &[22],
            Some(resources(32, 64 * 1024 * 1024 * 1024)),
        ),
    ]));
    let backend = FixtureBackend::default();
    plane
        .dispatch(&WorkerSpec::headless("job", "s1"), &backend)
        .unwrap();
    let snapshot = plane.snapshot();
    assert_eq!(snapshot.hosts.len(), 2);
    assert_eq!(snapshot.authorized_hosts, 1);
    assert_eq!(snapshot.running_workers, 1);
    assert_eq!(snapshot.total_workers, 1);
    let json = serde_json::to_string(&snapshot).unwrap();
    assert!(!json.contains("test-secret"), "credential leaked: {json}");
    assert!(
        !json.contains("password"),
        "credential field leaked: {json}"
    );
    let debug = format!("{plane:?}");
    assert!(!debug.contains("test-secret"));
    assert!(debug.contains("authorized"));
}

#[test]
fn worker_spec_validation_rejects_empty_fields() {
    let backend = FixtureBackend::default();
    let mut plane = ClusterControlPlane::new(
        credentials(),
        ClusterAuthorization::from_entries(["10.0.0.55"]),
    );
    plane.admit(&snapshot(vec![host(
        "10.0.0.55",
        &[22],
        Some(resources(8, 16 * 1024 * 1024 * 1024)),
    )]));
    for spec in [
        WorkerSpec::headless("", "s1"),
        WorkerSpec::headless("job", ""),
        WorkerSpec::headless("job", "s1").with_cpu_slots(0),
        WorkerSpec::headless("job", "s1").with_memory_bytes(0),
    ] {
        assert!(matches!(
            plane.dispatch(&spec, &backend).unwrap_err(),
            ClusterError::InvalidSpec(_)
        ));
    }
    assert!(plane.workers().next().is_none());
}

#[test]
fn authorization_parse_ignores_public_and_malformed_entries() {
    let authorization =
        ClusterAuthorization::parse("10.0.0.55, 10.20.30.38  8.8.8.8,not-an-ip 100.64.1.1");
    assert!(authorization.is_authorized("10.0.0.55"));
    assert!(authorization.is_authorized("10.20.30.38"));
    assert!(!authorization.is_authorized("8.8.8.8"));
    assert!(!authorization.is_authorized("not-an-ip"));
    assert!(!authorization.is_authorized("100.64.1.1"));
    assert_eq!(authorization.len(), 2);
}

#[test]
fn ssh_remote_command_quotes_prompts_and_omits_secrets() {
    let backend = SshDispatchBackend::new("zenpi", ".zenpi/cluster");
    let spec = WorkerSpec::headless("job", "s1").with_prompt("hello'; rm -rf /");
    let command = backend.remote_command(&spec, "job-0001");
    assert!(command.starts_with("mkdir -p "));
    assert!(command.contains("--mode headless"));
    assert!(command.contains("nohup"));
    // The dangerous quote is escaped rather than breaking out of the string.
    assert!(command.contains("'hello'\\''; rm -rf /'"));
    assert!(!command.contains("test-secret"));
    assert!(!command.contains("password"));

    // The generic quoter round-trips a hostile value safely.
    assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    assert_eq!(shell_quote("plain"), "'plain'");
}

#[test]
fn lan_and_cluster_project_into_the_unified_bus() {
    let lan = snapshot(vec![
        host(
            "10.0.0.55",
            &[22],
            Some(resources(8, 16 * 1024 * 1024 * 1024)),
        ),
        host("10.0.0.56", &[22], None),
    ]);
    let lan_section = lan_bus_section(&lan);
    assert_eq!(lan_section.local_ip, "10.0.0.14");
    assert_eq!(lan_section.gateway_ip.as_deref(), Some("10.0.0.1"));
    assert_eq!(lan_section.host_count, 2);
    assert!(!lan_section.truncated);

    let mut plane = ClusterControlPlane::new(
        credentials(),
        ClusterAuthorization::from_entries(["10.0.0.55"]),
    );
    plane.admit(&lan);
    let backend = FixtureBackend::default();
    plane
        .dispatch(&WorkerSpec::headless("job", "s1"), &backend)
        .unwrap();

    let cluster = plane.snapshot();
    let section = cluster.bus_section();
    assert_eq!(section.host_count, 2);
    assert_eq!(section.authorized_hosts, 1);
    assert_eq!(section.total_workers, 1);
    assert_eq!(section.running_workers, 1);
    assert_eq!(
        section.host_ips,
        vec!["10.0.0.55".to_owned(), "10.0.0.56".to_owned()]
    );
    assert!(!section.truncated);
}

#[test]
fn unified_bus_never_carries_cluster_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let host_snapshot = zenpi::resources::ResourceCollector::new(directory.path())
        .unwrap()
        .collect()
        .unwrap();
    let lan = snapshot(vec![host(
        "10.0.0.55",
        &[22],
        Some(resources(4, 8 * 1024 * 1024 * 1024)),
    )]);
    let mut plane = ClusterControlPlane::new(
        credentials(),
        ClusterAuthorization::from_entries(["10.0.0.55"]),
    );
    plane.admit(&lan);
    let backend = FixtureBackend::default();
    plane
        .dispatch(&WorkerSpec::headless("job", "s1"), &backend)
        .unwrap();

    let bus = cluster_resource_bus(&host_snapshot, Some(&lan), Some(&plane.snapshot()), 42);
    assert_eq!(bus.generated_ms, 42);
    assert!(bus.lan.is_some());
    assert!(bus.cluster.is_some());
    assert_eq!(
        bus.section_keys(),
        vec!["host", "budget", "ports", "lan", "cluster"]
    );
    let json = serde_json::to_string(&bus).unwrap();
    assert!(!json.contains("test-secret"), "credential leaked: {json}");
    assert!(
        !json.contains("password"),
        "credential field leaked: {json}"
    );
}

#[test]
fn cluster_bus_section_bounds_host_addresses() {
    let mut hosts = Vec::new();
    for index in 0..MAX_BUS_HOST_IPS + 6 {
        hosts.push(host(&format!("10.0.1.{}", index + 10), &[22], None));
    }
    let mut plane = ClusterControlPlane::new(credentials(), ClusterAuthorization::none());
    assert_eq!(plane.admit(&snapshot(hosts)), MAX_BUS_HOST_IPS + 6);
    let section = plane.snapshot().bus_section();
    assert_eq!(section.host_count, MAX_BUS_HOST_IPS + 6);
    assert_eq!(section.host_ips.len(), MAX_BUS_HOST_IPS);
    assert!(section.truncated);
    assert_eq!(
        section.host_ips.first().map(String::as_str),
        Some("10.0.1.10")
    );
}

#[test]
fn unified_bus_projects_to_bounded_credential_free_text() {
    let directory = tempfile::tempdir().unwrap();
    let host_snapshot = zenpi::resources::ResourceCollector::new(directory.path())
        .unwrap()
        .collect()
        .unwrap();
    let mut leases = zenpi::resources::PortLeaseRegistry::new();
    leases
        .lease(
            3000,
            "agent-a",
            zenpi::resources::DEFAULT_PORT_LEASE_MS,
            100,
        )
        .unwrap();
    let budget = zenpi::resources::AgentBudgetSignal::from_usage(
        (25, 100),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    );

    let bus = zenpi::cluster::cluster_resource_bus(
        &host_snapshot,
        Some(&snapshot(vec![host(
            "10.0.0.55",
            &[22],
            Some(resources(4, 8 * 1024 * 1024 * 1024)),
        )])),
        None,
        100,
    )
    .with_budget(budget)
    .with_ports(leases.snapshot(100));

    let projection = zenpi::view_model::ResourceBusProjection::from_bus(&bus);
    assert_eq!(projection.version, zenpi::view_model::VIEW_MODEL_VERSION);
    for key in ["host", "budget", "ports", "lan"] {
        assert!(projection.section(key).is_some(), "missing section {key}");
    }
    assert!(projection.section("cluster").is_none());
    assert_eq!(projection.generated_ms, 100);
    let lines = projection.lines();
    assert!(lines.len() <= zenpi::view_model::MAX_RESOURCE_BUS_LINES);
    let text = projection.to_text();
    assert!(text.contains("Host"));
    assert!(text.contains("Budget"));
    assert!(text.contains("Ports"));
    assert!(text.contains("port 3000"));
    // The projection is serializable and carries no credential material.
    let json = serde_json::to_string(&projection).unwrap();
    assert!(!json.contains("test-secret"), "credential leaked: {json}");
    assert!(
        !json.contains("password"),
        "credential field leaked: {json}"
    );
}
