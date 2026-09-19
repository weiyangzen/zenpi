use std::collections::{BTreeMap, BTreeSet};

use zenpi::net_probe::{
    ArpEntry, DeviceClass, HostResources, LanScanner, NetCredential, NetCredentials, ProbeBackend,
    parse_arp_table, parse_remote_facts,
};

struct FixtureBackend {
    arp: Vec<ArpEntry>,
    ports: BTreeMap<String, Vec<u16>>,
    banners: BTreeMap<String, String>,
    resources: BTreeMap<String, HostResources>,
    reachable: BTreeSet<String>,
}

impl ProbeBackend for FixtureBackend {
    fn arp_table(&self) -> Vec<ArpEntry> {
        self.arp.clone()
    }

    fn ping(&self, ip: &str) -> bool {
        self.reachable.contains(ip)
    }

    fn open_port(&self, ip: &str, port: u16) -> bool {
        self.ports
            .get(ip)
            .is_some_and(|ports| ports.contains(&port))
    }

    fn ssh_banner(&self, ip: &str) -> Option<String> {
        self.banners.get(ip).cloned()
    }

    fn remote_resources(&self, ip: &str, credentials: &NetCredentials) -> Option<HostResources> {
        if credentials.is_empty() {
            return None;
        }
        self.resources.get(ip).cloned()
    }
}

fn real_topology_fixture() -> FixtureBackend {
    let mut ports: BTreeMap<String, Vec<u16>> = BTreeMap::new();
    let mut banners: BTreeMap<String, String> = BTreeMap::new();
    let mut resources: BTreeMap<String, HostResources> = BTreeMap::new();
    let mut arp = Vec::new();

    let mut add = |ip: &str, mac: &str, open: &[u16], banner: Option<&str>| {
        arp.push(ArpEntry {
            ip: ip.to_owned(),
            mac: mac.to_owned(),
        });
        ports.insert(ip.to_owned(), open.to_vec());
        if let Some(banner) = banner {
            banners.insert(ip.to_owned(), banner.to_owned());
        }
    };

    let ubuntu = "SSH-2.0-OpenSSH_9.6p1 Ubuntu-3ubuntu13.19";
    let darwin = "SSH-2.0-OpenSSH_10.2";

    add("10.0.0.1", "f4:1e:57:00:00:01", &[22, 80], None);
    add("10.0.0.2", "7c:10:c9:00:00:02", &[80], None);
    add("10.0.0.3", "24:4b:fe:00:00:03", &[80], None);
    add("10.0.0.4", "fc:34:97:00:00:04", &[80], None);
    add("10.0.0.12", "6e:19:d2:00:00:05", &[], None);
    add(
        "10.0.0.13",
        "7c:01:3e:00:00:06",
        &[80, 443, 5000, 8080, 7000],
        None,
    );
    add(
        "10.0.0.14",
        "9c:76:0e:00:00:07",
        &[22, 5900, 5000, 7000],
        Some(darwin),
    );
    add(
        "10.0.0.15",
        "9c:76:0e:00:00:08",
        &[22, 5900, 5000, 7000],
        Some(darwin),
    );
    add(
        "10.0.0.16",
        "a4:fc:14:00:00:09",
        &[22, 80, 445, 5900, 5000, 7000],
        Some(darwin),
    );
    add("10.0.0.19", "00:4b:f3:00:00:0a", &[80], None);
    add("10.0.0.20", "00:4b:f3:00:00:0b", &[80], None);
    add("10.0.0.21", "58:47:ca:00:00:0c", &[22], Some(ubuntu));
    add("10.0.0.38", "58:47:ca:00:00:0d", &[22], Some(ubuntu));
    add("10.0.0.55", "9c:6b:00:00:00:0e", &[22], Some(ubuntu));
    add("10.0.0.56", "58:47:ca:00:00:0f", &[22], Some(ubuntu));
    add("10.0.0.123", "70:13:84:00:00:10", &[5000, 7000], None);
    add(
        "10.0.0.155",
        "58:47:ca:00:00:11",
        &[22, 8080, 3000],
        Some(ubuntu),
    );
    add("10.0.0.165", "58:47:ca:00:00:12", &[22], Some(ubuntu));
    add("10.0.0.167", "3c:6d:66:00:00:13", &[22], Some(ubuntu));
    add("10.0.0.168", "58:47:ca:00:00:14", &[22], Some(ubuntu));
    add(
        "10.0.0.177",
        "00:11:32:00:00:15",
        &[22, 80, 443, 445, 139, 548, 5000, 5001],
        Some("SSH-2.0-OpenSSH_7.4"),
    );
    add(
        "10.0.0.182",
        "ba:48:a0:00:00:16",
        &[22, 5900, 5000, 7000],
        Some(darwin),
    );
    add(
        "10.0.0.185",
        "90:09:d0:00:00:17",
        &[80, 443, 445, 139, 5000, 5001],
        None,
    );
    add("10.0.0.220", "58:47:ca:00:00:18", &[22], Some(ubuntu));
    add("10.0.0.228", "58:47:ca:00:00:19", &[22], Some(ubuntu));
    add("10.0.0.249", "10:ff:e0:00:00:1a", &[22], Some(ubuntu));
    add("10.0.0.254", "4c:49:68:00:00:1b", &[80, 1883], None);

    let gpu = |cpu: &str, cpus: usize, mem: u64, gpus: &[&str]| HostResources {
        os: Some("Ubuntu 24.04".into()),
        hostname: Some("gpu-node".into()),
        kernel: Some("6.8.0".into()),
        cpu: Some(cpu.into()),
        logical_cpus: Some(cpus),
        memory_total_bytes: Some(mem),
        disk_total_bytes: Some(3_700_000_000_000),
        disk_available_bytes: Some(2_000_000_000_000),
        gpus: gpus.iter().map(|g| (*g).to_owned()).collect(),
        services: vec!["ssh".into()],
    };
    resources.insert(
        "10.0.0.55".into(),
        gpu(
            "AMD EPYC 7B12",
            128,
            811_000_000_000,
            &["RTX 3080", "RTX 3080", "RTX 3080", "RTX 3080"],
        ),
    );
    resources.insert(
        "10.0.0.56".into(),
        gpu("AMD Ryzen 9 7945HX", 32, 96_000_000_000, &["RTX 4090 D"]),
    );
    resources.insert(
        "10.0.0.228".into(),
        gpu("AMD Ryzen 9 7945HX", 32, 95_000_000_000, &["RTX 4090"]),
    );
    resources.insert(
        "10.0.0.249".into(),
        gpu(
            "AMD Ryzen 9 9950X3D",
            32,
            186_000_000_000,
            &["RTX 3090", "RTX 3090"],
        ),
    );

    let mac = |model: &str, mem: u64| HostResources {
        os: Some(format!("macOS 26.5 ({model})")),
        hostname: None,
        kernel: Some("Darwin 25.5.0".into()),
        cpu: Some(model.into()),
        logical_cpus: Some(20),
        memory_total_bytes: Some(mem),
        disk_total_bytes: Some(3_600_000_000_000),
        disk_available_bytes: Some(800_000_000_000),
        gpus: Vec::new(),
        services: vec!["ssh".into(), "screen-sharing".into()],
    };
    resources.insert(
        "10.0.0.15".into(),
        mac("Apple M1 Ultra", 64_000_000_000),
    );
    resources.insert("10.0.0.16".into(), mac("Apple M2 Max", 64_000_000_000));
    resources.insert(
        "10.0.0.182".into(),
        mac("Apple M1 Ultra", 64_000_000_000),
    );

    FixtureBackend {
        arp,
        ports,
        banners,
        resources,
        reachable: BTreeSet::new(),
    }
}

fn credentials() -> NetCredentials {
    NetCredentials::from_entries(vec![
        NetCredential::new("netuser", "test-secret"),
        NetCredential::new("mac", "test-secret"),
    ])
}

#[test]
fn arp_table_parsing_skips_incomplete_and_broadcast() {
    let text = "\
? (10.0.0.1) at f4:1e:57:00:00:01 on en0 ifscope [ethernet]
? (10.0.0.5) at (incomplete) on en0 ifscope [ethernet]
? (10.0.0.255) at ff:ff:ff:ff:ff:ff on en0 ifscope [ethernet]
? (10.0.0.38) at 58:47:ca:00:00:0d on en0 ifscope [ethernet]";
    let entries = parse_arp_table(text);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].ip, "10.0.0.1");
    assert_eq!(entries[1].mac, "58:47:ca:00:00:0d");
}

#[test]
fn fixture_scan_reproduces_real_topology_blocks() {
    let backend = real_topology_fixture();
    let candidates: Vec<String> = backend.arp.iter().map(|entry| entry.ip.clone()).collect();
    let scanner = LanScanner::new("10.0.0.14");
    let snapshot = scanner.scan(&backend, &candidates, &credentials());

    assert_eq!(snapshot.local_ip, "10.0.0.14");
    assert_eq!(snapshot.gateway_ip.as_deref(), Some("10.0.0.1"));

    let block_count = |class| {
        snapshot
            .block(class)
            .map(|block| block.count())
            .unwrap_or(0)
    };
    assert_eq!(block_count(DeviceClass::Gateway), 1, "gateway");
    assert_eq!(block_count(DeviceClass::Nas), 2, "two NAS");
    assert_eq!(block_count(DeviceClass::Thor), 1, "nvidia thor");
    assert_eq!(block_count(DeviceClass::GpuNode), 4, "four gpu nodes");
    assert_eq!(block_count(DeviceClass::Printer), 1, "deli printer");
    assert!(block_count(DeviceClass::Mac) >= 4, "mac group");
    assert!(block_count(DeviceClass::Linux) >= 5, "linux group");

    // The four GPU hosts are promoted out of the linux block.
    let gpu_ips: Vec<&str> = snapshot
        .block(DeviceClass::GpuNode)
        .unwrap()
        .host_indices
        .iter()
        .map(|index| snapshot.hosts[*index].ip.as_str())
        .collect();
    for expected in [
        "10.0.0.55",
        "10.0.0.56",
        "10.0.0.228",
        "10.0.0.249",
    ] {
        assert!(gpu_ips.contains(&expected), "missing gpu host {expected}");
    }

    let thor = snapshot
        .hosts
        .iter()
        .find(|host| host.class == DeviceClass::Thor)
        .expect("thor host");
    assert_eq!(thor.ip, "10.0.0.167");
    assert_eq!(thor.vendor.as_deref(), Some("NVIDIA"));

    assert!(snapshot.hosts.iter().any(|host| host.credentials_used));
    assert!(!snapshot.truncated);
}

#[test]
fn scan_is_bounded_to_the_local_subnet() {
    let backend = real_topology_fixture();
    let mut candidates: Vec<String> = backend.arp.iter().map(|entry| entry.ip.clone()).collect();
    candidates.push("10.0.0.5".into());
    candidates.push("10.0.1.9".into());
    let snapshot =
        LanScanner::new("10.0.0.14").scan(&backend, &candidates, &NetCredentials::empty());
    assert!(
        snapshot
            .hosts
            .iter()
            .all(|host| host.ip.starts_with("10.0.0."))
    );
    assert!(!snapshot.hosts.iter().any(|host| host.ip == "10.0.0.5"));
}

#[test]
fn credentials_are_redacted_and_parse_from_json() {
    let credential = NetCredential::new("netuser", "test-secret");
    let debug = format!("{credential:?}");
    assert!(debug.contains("netuser"));
    assert!(!debug.contains("test-secret"));

    let parsed = NetCredentials::parse_json(
        r#"{"credentials":[{"username":"netuser","password":"test-secret"}]}"#,
    );
    assert_eq!(parsed.entries().len(), 1);
    let summary = parsed.masked_summary();
    assert!(summary.contains("netuser"));
    assert!(!summary.contains("test-secret"));

    // The serialized snapshot never carries a credential.
    let backend = real_topology_fixture();
    let candidates: Vec<String> = backend.arp.iter().map(|entry| entry.ip.clone()).collect();
    let snapshot = LanScanner::new("10.0.0.14").scan(&backend, &candidates, &parsed);
    let json = serde_json::to_string(&snapshot).unwrap();
    assert!(!json.contains("test-secret"));
    assert!(!json.contains("password"));
}

#[test]
fn remote_facts_parse_into_host_resources() {
    let text = "os=Ubuntu 24.04.4 LTS\n\
hostname=node-2-7945hx\n\
kernel=6.8.0-45-generic\n\
cpu=AMD Ryzen 9 7950X 16-Core Processor\n\
cpus=32\n\
mem=134217728000\n\
disk_total=2000000000000\n\
disk_avail=750000000000\n\
gpus=NVIDIA GeForce RTX 4090 D,NVIDIA GeForce RTX 3080\n\
unknown=ignored\n\
empty=\n";
    let resources = parse_remote_facts(text);
    assert_eq!(resources.os.as_deref(), Some("Ubuntu 24.04.4 LTS"));
    assert_eq!(resources.hostname.as_deref(), Some("node-2-7945hx"));
    assert_eq!(resources.kernel.as_deref(), Some("6.8.0-45-generic"));
    assert_eq!(resources.logical_cpus, Some(32));
    assert_eq!(resources.memory_total_bytes, Some(134_217_728_000));
    assert_eq!(resources.disk_total_bytes, Some(2_000_000_000_000));
    assert_eq!(resources.disk_available_bytes, Some(750_000_000_000));
    assert_eq!(resources.gpus.len(), 2);
    assert_eq!(resources.gpus[0], "NVIDIA GeForce RTX 4090 D");

    // A probe that returns only partial facts still yields a usable record.
    let partial = parse_remote_facts("cpus=8\ngpus=\n");
    assert_eq!(partial.logical_cpus, Some(8));
    assert!(partial.gpus.is_empty());
    assert!(partial.os.is_none());
}
