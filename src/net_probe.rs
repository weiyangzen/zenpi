//! Bounded, read-only LAN resource sensing.
//!
//! The probe deliberately separates *pure* interpretation (ARP parsing,
//! credential redaction, device classification, block grouping) from the
//! operating-system calls that gather raw facts. That keeps the interpretation
//! fully unit-testable from fixtures while the real backend stays a thin,
//! bounded shell around `arp`, `ping`, TCP connect, and SSH.
//!
//! Security contract:
//! - Scans are limited to a single `/24` and a fixed port set/timeout/bounds.
//! - The probe is read-only: it never writes to a peer and never guesses
//!   credentials.
//! - Credentials are read only from an explicit local secret source, are never
//!   serialized into a snapshot, and their `Debug`/`Display` representations
//!   are redacted.

use std::{
    collections::BTreeMap,
    fmt,
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

/// Largest number of hosts a single scan may retain.
pub const MAX_SCAN_HOSTS: usize = 256;
/// Ports probed per reachable host (a small, stable fingerprint set).
pub const MAX_PORTS_PER_HOST: usize = 14;
/// Upper bound on concurrent TCP probes.
pub const MAX_PROBE_CONCURRENCY: usize = 32;
/// Per-probe connect timeout.
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(350);
/// Longest banner/title retained from a peer.
pub const MAX_BANNER_CHARS: usize = 160;

/// The ports fingerprinted without credentials. Kept intentionally small and
/// read-only; every entry is a well-known service rather than a wide sweep.
pub const PROBE_PORTS: [u16; MAX_PORTS_PER_HOST] = [
    22, 80, 443, 445, 139, 548, 3389, 5900, 5000, 5001, 8080, 3000, 1883, 32400,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClass {
    Local,
    Gateway,
    Mac,
    Linux,
    Nas,
    Printer,
    Router,
    Iot,
    Thor,
    GpuNode,
    Unknown,
}

impl DeviceClass {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Local => "本机",
            Self::Gateway => "网关",
            Self::Mac => "mac",
            Self::Linux => "linux",
            Self::Nas => "存储(NAS)",
            Self::Printer => "打印机",
            Self::Router => "路由器/AP",
            Self::Iot => "IoT/移动",
            Self::Thor => "thor",
            Self::GpuNode => "GPU 节点",
            Self::Unknown => "未知",
        }
    }

    /// Blocks shown by the resource pane, in display order. Local and gateway
    /// come first because they anchor the topology; the remaining classes are
    /// the host groups.
    pub const fn block_order() -> [Self; 11] {
        [
            Self::Local,
            Self::Gateway,
            Self::Mac,
            Self::Linux,
            Self::Nas,
            Self::Thor,
            Self::GpuNode,
            Self::Printer,
            Self::Router,
            Self::Iot,
            Self::Unknown,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArpEntry {
    pub ip: String,
    pub mac: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceFingerprint {
    pub port: u16,
    pub service: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HostResources {
    pub os: Option<String>,
    pub kernel: Option<String>,
    pub cpu: Option<String>,
    pub logical_cpus: Option<usize>,
    pub memory_total_bytes: Option<u64>,
    pub disk_total_bytes: Option<u64>,
    pub disk_available_bytes: Option<u64>,
    pub gpus: Vec<String>,
    pub services: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanHost {
    pub ip: String,
    pub mac: Option<String>,
    pub vendor: Option<String>,
    pub hostname: Option<String>,
    pub open_ports: Vec<u16>,
    pub services: Vec<ServiceFingerprint>,
    pub class: DeviceClass,
    pub resources: Option<HostResources>,
    pub credentials_used: bool,
}

impl LanHost {
    pub fn new(ip: impl Into<String>) -> Self {
        Self {
            ip: ip.into(),
            mac: None,
            vendor: None,
            hostname: None,
            open_ports: Vec::new(),
            services: Vec::new(),
            class: DeviceClass::Unknown,
            resources: None,
            credentials_used: false,
        }
    }

    /// Short one-line summary used by a collapsed block row.
    pub fn summary(&self) -> String {
        let name = self
            .hostname
            .clone()
            .or_else(|| self.vendor.clone())
            .unwrap_or_else(|| self.class.label().to_owned());
        if self.open_ports.is_empty() {
            name
        } else {
            format!("{name} :{}", self.open_ports[0])
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanBlock {
    pub class: DeviceClass,
    pub title: String,
    pub host_indices: Vec<usize>,
}

impl LanBlock {
    pub fn count(&self) -> usize {
        self.host_indices.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LanSnapshot {
    pub local_ip: String,
    pub gateway_ip: Option<String>,
    pub hosts: Vec<LanHost>,
    pub blocks: Vec<LanBlock>,
    pub scanned_at_ms: u64,
    pub truncated: bool,
}

impl LanSnapshot {
    pub fn block(&self, class: DeviceClass) -> Option<&LanBlock> {
        self.blocks.iter().find(|block| block.class == class)
    }
}

/// A credential entry. Never serialized with a snapshot and always redacted in
/// `Debug`/`Display`.
#[derive(Clone, PartialEq, Eq)]
pub struct NetCredential {
    username: String,
    password: String,
}

impl NetCredential {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    #[allow(dead_code)]
    pub(crate) fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for NetCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NetCredential")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// Credential set loaded from an explicit local source. Only three forms are
/// accepted, in order: `ZENPI_NET_CREDENTIALS` JSON, the file named by
/// `ZENPI_NET_CREDENTIALS_FILE`, else none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetCredentials {
    entries: Vec<NetCredential>,
}

impl NetCredentials {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_entries(entries: Vec<NetCredential>) -> Self {
        Self { entries }
    }

    pub fn from_env() -> Self {
        if let Ok(raw) = std::env::var("ZENPI_NET_CREDENTIALS") {
            return Self::parse_json(&raw);
        }
        if let Ok(path) = std::env::var("ZENPI_NET_CREDENTIALS_FILE")
            && let Ok(raw) = std::fs::read_to_string(path)
        {
            return Self::parse_json(&raw);
        }
        Self::empty()
    }

    pub fn parse_json(raw: &str) -> Self {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(raw);
        let Ok(value) = parsed else {
            return Self::empty();
        };
        let Some(array) = value.get("credentials").and_then(|v| v.as_array()) else {
            return Self::empty();
        };
        let mut entries = Vec::new();
        for item in array {
            let username = item.get("username").and_then(|v| v.as_str());
            let password = item.get("password").and_then(|v| v.as_str());
            if let (Some(username), Some(password)) = (username, password) {
                entries.push(NetCredential::new(username, password));
            }
        }
        Self { entries }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[NetCredential] {
        &self.entries
    }

    /// A redacted summary safe to surface in a snapshot or a log line.
    pub fn masked_summary(&self) -> String {
        if self.entries.is_empty() {
            return "no credentials".to_owned();
        }
        let users: Vec<&str> = self.entries.iter().map(|e| e.username()).collect();
        format!("{} credential(s): {}", users.len(), users.join(", "))
    }
}

/// Parse the output of `arp -an`. Only well-formed IPv4/MAC pairs are kept and
/// incomplete entries are skipped.
pub fn parse_arp_table(text: &str) -> Vec<ArpEntry> {
    let mut entries = Vec::new();
    for line in text.lines() {
        if line.contains("incomplete") {
            continue;
        }
        let Some(ip) = extract_ipv4(line) else {
            continue;
        };
        let Some(mac) = extract_mac(line) else {
            continue;
        };
        if mac == "ff:ff:ff:ff:ff:ff" {
            continue;
        }
        entries.push(ArpEntry { ip, mac });
    }
    entries
}

fn extract_ipv4(line: &str) -> Option<String> {
    for token in line.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
        if token.matches('.').count() == 3 {
            let candidate = token.trim_matches('.');
            if candidate.parse::<Ipv4Addr>().is_ok() {
                return Some(candidate.to_owned());
            }
        }
    }
    None
}

fn extract_mac(line: &str) -> Option<String> {
    for token in line.split_whitespace() {
        let trimmed = token.trim_matches(|c: char| c == '(' || c == ')' || c == ',');
        let parts: Vec<&str> = trimmed.split(':').collect();
        if parts.len() == 6
            && parts
                .iter()
                .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
        {
            return Some(trimmed.to_ascii_lowercase());
        }
    }
    None
}

/// Vendor from the OUI with a small, curated table for the classes the pane
/// needs to distinguish. Unknown OUIs are `None` rather than a guess.
pub fn vendor_for_mac(mac: &str) -> Option<&'static str> {
    let prefix = mac.get(0..8)?.to_ascii_lowercase();
    let vendor = match prefix.as_str() {
        "00:11:32" | "90:09:d0" | "00:1b:21" => "Synology",
        "3c:6d:66" => "NVIDIA",
        "9c:76:0e" | "a4:fc:14" | "1c:e2:09" | "70:8c:f2" | "80:5f:c5" | "70:13:84" => "Apple",
        "24:4b:fe" | "7c:10:c9" | "fc:34:97" => "ASUS",
        "00:4b:f3" => "Mercury",
        "4c:49:68" => "Ruijie",
        "9c:6b:00" => "ASRock",
        "10:ff:e0" => "GIGABYTE",
        "f4:1e:57" => "Routerboard",
        "7c:01:3e" => "GSD",
        "58:47:ca" => "IEEE-RA",
        _ => return None,
    };
    Some(vendor)
}

/// Classify a host from the facts the probe gathered. Ordering matters: an
/// explicit GPU/thor signal wins over a generic linux classification, and a
/// NAS/printer is never reported as a plain server.
pub fn classify_device(
    is_local: bool,
    is_gateway: bool,
    mac: Option<&str>,
    ports: &[u16],
    os: Option<&str>,
) -> DeviceClass {
    if is_local {
        return DeviceClass::Local;
    }
    if is_gateway {
        return DeviceClass::Gateway;
    }
    let vendor = mac.and_then(vendor_for_mac);
    let has = |port: u16| ports.contains(&port);
    let nvidia = vendor == Some("NVIDIA");
    let mac_os = os.is_some_and(|value| {
        let lower = value.to_ascii_lowercase();
        lower.contains("darwin") || lower.contains("macos") || lower.contains("mac os x")
    }) || vendor == Some("Apple");
    let nas_ports = has(5000) && has(5001) || (has(445) && has(139) && has(548));
    if nvidia {
        return DeviceClass::Thor;
    }
    if mac_os {
        return DeviceClass::Mac;
    }
    if vendor == Some("Synology") || nas_ports {
        return DeviceClass::Nas;
    }
    if vendor == Some("Deli") || vendor == Some("GSD") || has(631) || has(9100) {
        return DeviceClass::Printer;
    }
    if vendor == Some("Routerboard")
        || vendor == Some("Ruijie")
        || vendor == Some("Mercury")
        || vendor == Some("ASUS")
    {
        return DeviceClass::Router;
    }
    if os.is_some_and(|value| {
        let lower = value.to_ascii_lowercase();
        [
            "linux", "ubuntu", "debian", "centos", "fedora", "alpine", "rocky",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
    }) {
        return DeviceClass::Linux;
    }
    if vendor.is_none() && ports.is_empty() {
        return DeviceClass::Iot;
    }
    DeviceClass::Unknown
}

/// Group hosts into the display blocks. A host classified as GPU node is
/// promoted out of the generic linux block.
pub fn group_blocks(hosts: &[LanHost]) -> Vec<LanBlock> {
    let mut by_class: BTreeMap<DeviceClass, Vec<usize>> = BTreeMap::new();
    for (index, host) in hosts.iter().enumerate() {
        let class = if host.resources.as_ref().is_some_and(|r| !r.gpus.is_empty()) {
            DeviceClass::GpuNode
        } else {
            host.class
        };
        by_class.entry(class).or_default().push(index);
    }
    let mut blocks = Vec::new();
    for class in DeviceClass::block_order() {
        if let Some(indices) = by_class.remove(&class) {
            blocks.push(LanBlock {
                class,
                title: format!("{} ({})", class.label(), indices.len()),
                host_indices: indices,
            });
        }
    }
    blocks
}

/// A raw probe backend. The real implementation shells out to read-only OS
/// tools; tests substitute a fixture backend.
pub trait ProbeBackend {
    fn arp_table(&self) -> Vec<ArpEntry>;
    fn ping(&self, ip: &str) -> bool;
    fn open_port(&self, ip: &str, port: u16) -> bool;
    fn ssh_banner(&self, ip: &str) -> Option<String>;
    fn remote_resources(&self, ip: &str, credentials: &NetCredentials) -> Option<HostResources>;
}

/// Orchestrates a bounded scan over one `/24`.
pub struct LanScanner {
    local_ip: String,
    gateway_ip: Option<String>,
    ports: Vec<u16>,
    budget: Duration,
}

impl LanScanner {
    pub fn new(local_ip: impl Into<String>) -> Self {
        let local_ip = local_ip.into();
        let gateway_ip = default_gateway_for(&local_ip);
        Self {
            local_ip,
            gateway_ip,
            ports: PROBE_PORTS.to_vec(),
            budget: Duration::from_secs(8),
        }
    }

    /// Cap the wall-clock time spent probing; when exceeded the scan stops and
    /// reports `truncated` rather than running unbounded.
    pub fn with_budget(mut self, budget: Duration) -> Self {
        self.budget = budget;
        self
    }

    /// All usable addresses in the local `/24` (network and broadcast excluded).
    pub fn subnet_candidates(&self) -> Vec<String> {
        let Ok(ip) = self.local_ip.parse::<Ipv4Addr>() else {
            return Vec::new();
        };
        let octets = ip.octets();
        (1u16..=254)
            .map(|last| format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], last))
            .collect()
    }

    pub fn with_gateway(mut self, gateway: Option<String>) -> Self {
        self.gateway_ip = gateway;
        self
    }

    pub fn local_ip(&self) -> &str {
        &self.local_ip
    }

    pub fn gateway_ip(&self) -> Option<&str> {
        self.gateway_ip.as_deref()
    }

    /// Discover and classify hosts. `hosts` bounds the candidate set; callers
    /// pass the ARP table plus any additional pinged addresses.
    pub fn scan(
        &self,
        backend: &dyn ProbeBackend,
        candidates: &[String],
        credentials: &NetCredentials,
    ) -> LanSnapshot {
        let mut truncated = false;
        let arp = backend.arp_table();
        let mac_by_ip: BTreeMap<String, String> =
            arp.iter().map(|e| (e.ip.clone(), e.mac.clone())).collect();

        let mut ordered = candidates.to_vec();
        for entry in &arp {
            if !ordered.contains(&entry.ip) {
                ordered.push(entry.ip.clone());
            }
        }
        ordered.retain(|ip| same_subnet(ip, &self.local_ip));
        ordered.sort();
        ordered.dedup();
        if ordered.len() > MAX_SCAN_HOSTS {
            ordered.truncate(MAX_SCAN_HOSTS);
            truncated = true;
        }

        let started = Instant::now();
        let mut hosts = Vec::new();
        for ip in ordered {
            if started.elapsed() > self.budget {
                truncated = true;
                break;
            }
            let mac = mac_by_ip.get(&ip).cloned();
            // Without an ARP entry a host is reachable only if it answers an
            // explicit ping; otherwise it is noise from a full /24 list.
            if mac.is_none() && !backend.ping(&ip) {
                continue;
            }
            let mut host = LanHost::new(ip.clone());
            host.mac = mac.clone();
            host.vendor = mac.as_deref().and_then(vendor_for_mac).map(str::to_owned);
            for port in &self.ports {
                if backend.open_port(&ip, *port) {
                    host.open_ports.push(*port);
                    host.services.push(ServiceFingerprint {
                        port: *port,
                        service: service_name(*port).to_owned(),
                    });
                }
            }
            if host.mac.is_none() && host.open_ports.is_empty() {
                // A host that answered ping but exposes nothing is noise.
                continue;
            }
            if host.open_ports.contains(&22) {
                let banner = backend.ssh_banner(&ip);
                if let Some(banner) = banner {
                    let lower = banner.to_ascii_lowercase();
                    host.resources = Some(HostResources {
                        os: Some(banner.clone()),
                        ..HostResources::default()
                    });
                    if lower.contains("darwin") {
                        host.class = DeviceClass::Mac;
                    } else if lower.contains("linux") {
                        host.class = DeviceClass::Linux;
                    }
                }
            }
            host.class = classify_device(
                ip == self.local_ip,
                Some(ip.as_str()) == self.gateway_ip.as_deref(),
                host.mac.as_deref(),
                &host.open_ports,
                host.resources.as_ref().and_then(|r| r.os.as_deref()),
            );
            if !credentials.is_empty()
                && let Some(resources) = backend.remote_resources(&ip, credentials)
            {
                host.credentials_used = true;
                // Credentials reveal the real OS, which can upgrade a
                // banner-less host (e.g. an Apple device) to its true class.
                let os = resources.os.clone();
                host.resources = Some(resources);
                host.class = classify_device(
                    ip == self.local_ip,
                    Some(ip.as_str()) == self.gateway_ip.as_deref(),
                    host.mac.as_deref(),
                    &host.open_ports,
                    os.as_deref(),
                );
            }
            hosts.push(host);
        }

        let blocks = group_blocks(&hosts);
        LanSnapshot {
            local_ip: self.local_ip.clone(),
            gateway_ip: self.gateway_ip.clone(),
            hosts,
            blocks,
            scanned_at_ms: unix_time_ms(),
            truncated,
        }
    }
}

fn service_name(port: u16) -> &'static str {
    match port {
        22 => "ssh",
        80 => "http",
        443 => "https",
        445 => "smb",
        139 => "netbios",
        548 => "afp",
        3389 => "rdp",
        5900 => "vnc",
        5000 | 5001 => "nas-web",
        8006 => "proxmox",
        8080 | 9090 | 3000 => "http-alt",
        1883 => "mqtt",
        2375 => "docker",
        6443 => "k8s-api",
        32400 => "plex",
        8123 => "home-assistant",
        9000 | 4000 => "app",
        7000 => "airplay",
        2049 => "nfs",
        111 => "rpcbind",
        _ => "tcp",
    }
}

fn same_subnet(candidate: &str, local: &str) -> bool {
    let (Ok(candidate), Ok(local)) = (candidate.parse::<Ipv4Addr>(), local.parse::<Ipv4Addr>())
    else {
        return false;
    };
    let mask = Ipv4Addr::new(255, 255, 255, 0);
    (u32::from(candidate) & u32::from(mask)) == (u32::from(local) & u32::from(mask))
}

fn default_gateway_for(local_ip: &str) -> Option<String> {
    let ip: Ipv4Addr = local_ip.parse().ok()?;
    let octets = ip.octets();
    Some(format!("{}.{}.{}.1", octets[0], octets[1], octets[2]))
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

/// Read-only system backend. Every operation is bounded and ignores output
/// beyond a fixed size; failures degrade to `false`/`None`.
pub struct SystemProbeBackend {
    timeout: Duration,
}

impl Default for SystemProbeBackend {
    fn default() -> Self {
        Self {
            timeout: PROBE_TIMEOUT,
        }
    }
}

impl SystemProbeBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ProbeBackend for SystemProbeBackend {
    fn arp_table(&self) -> Vec<ArpEntry> {
        let output = std::process::Command::new("arp").arg("-an").output();
        match output {
            Ok(output) => parse_arp_table(&String::from_utf8_lossy(&output.stdout)),
            Err(_) => Vec::new(),
        }
    }

    fn ping(&self, ip: &str) -> bool {
        let timeout_ms = self.timeout.as_millis().to_string();
        std::process::Command::new("ping")
            .args(["-c", "1", "-W", &timeout_ms, ip])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn open_port(&self, ip: &str, port: u16) -> bool {
        connect(ip, port).is_some()
    }

    fn ssh_banner(&self, ip: &str) -> Option<String> {
        let mut stream = connect(ip, 22)?;
        let mut buffer = [0u8; MAX_BANNER_CHARS];
        let read = stream.read(&mut buffer).ok()?;
        let text = String::from_utf8_lossy(&buffer[..read]).trim().to_owned();
        if text.is_empty() { None } else { Some(text) }
    }

    fn remote_resources(&self, _ip: &str, _credentials: &NetCredentials) -> Option<HostResources> {
        // Password SSH extraction is intentionally not performed by the default
        // backend; callers that provision a key/agent use `RemoteShellBackend`.
        None
    }
}

fn connect(ip: &str, port: u16) -> Option<TcpStream> {
    let address: IpAddr = ip.parse().ok()?;
    let socket = SocketAddr::new(address, port);
    let stream = TcpStream::connect_timeout(&socket, PROBE_TIMEOUT).ok()?;
    let _ = stream.set_read_timeout(Some(PROBE_TIMEOUT));
    let _ = stream.set_write_timeout(Some(PROBE_TIMEOUT));
    Some(stream)
}

/// Pick the host's primary IPv4 address for a `/24` scan. Preference order is
/// `192.168.*`, then other RFC1918 ranges, then any non-loopback address;
/// loopback, link-local, and CGNAT (`100.64/10`) addresses are ignored.
#[cfg(unix)]
pub fn local_ipv4() -> Option<String> {
    let mut interfaces: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: `getifaddrs` fills a heap list released by `freeifaddrs`.
    if unsafe { libc::getifaddrs(&mut interfaces) } != 0 || interfaces.is_null() {
        return None;
    }
    let mut best: Option<(u8, String)> = None;
    let mut cursor = interfaces;
    while !cursor.is_null() {
        // SAFETY: `cursor` walks the list returned by `getifaddrs`.
        let entry = unsafe { &*cursor };
        if !entry.ifa_addr.is_null()
            && i32::from(unsafe { (*entry.ifa_addr).sa_family }) == libc::AF_INET
        {
            // SAFETY: the family matched AF_INET, so the sockaddr is a
            // `sockaddr_in`.
            let sockaddr = unsafe { &*(entry.ifa_addr as *const libc::sockaddr_in) };
            let raw = u32::from_be(sockaddr.sin_addr.s_addr);
            let octets = raw.to_be_bytes();
            let address = Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]);
            let score = private_score(address, octets);
            if let Some(score) = score
                && best.as_ref().is_none_or(|(current, _)| score < *current)
            {
                best = Some((score, address.to_string()));
            }
        }
        cursor = entry.ifa_next;
    }
    // SAFETY: `interfaces` is the head of the list allocated above.
    unsafe { libc::freeifaddrs(interfaces) };
    best.map(|(_, address)| address)
}

#[cfg(not(unix))]
pub fn local_ipv4() -> Option<String> {
    None
}

#[cfg(unix)]
fn private_score(address: Ipv4Addr, octets: [u8; 4]) -> Option<u8> {
    if address.is_loopback() || address.is_link_local() || address.is_unspecified() {
        return None;
    }
    let cgnat = octets[0] == 100 && (64..128).contains(&octets[1]);
    if cgnat {
        return None;
    }
    let score = match octets {
        [192, 168, _, _] => 0,
        [10, _, _, _] => 1,
        [172, second, _, _] if (16..32).contains(&second) => 2,
        _ => 3,
    };
    Some(score)
}

/// HTTP title probe used by richer backends; bounded and read-only.
pub fn http_title(ip: &str, port: u16) -> Option<String> {
    let mut stream = connect(ip, port)?;
    let request = format!("GET / HTTP/1.0\r\nHost: {ip}\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut buffer = [0u8; 2048];
    let read = stream.read(&mut buffer).ok()?;
    let text = String::from_utf8_lossy(&buffer[..read]);
    let lower = text.to_ascii_lowercase();
    let start = lower.find("<title>")?;
    let end = lower[start + 7..].find("</title>")?;
    let title = text[start + 7..start + 7 + end].trim().to_owned();
    if title.is_empty() { None } else { Some(title) }
}
