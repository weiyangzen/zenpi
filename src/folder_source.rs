//! Folder sources for project tabs: a local directory or a read-only SSH
//! remote directory. Remote probing never mutates the remote host and never
//! persists credentials; explicit `user@host:port/path` and `~/.ssh/config`
//! aliases are both accepted.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const MAX_REMOTE_ENTRIES: usize = 512;
pub const MAX_REMOTE_OUTPUT_BYTES: usize = 64 * 1024;
pub const MAX_SPEC_BYTES: usize = 2 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FolderSourceError {
    #[error("folder spec is empty or too long")]
    InvalidSpec,
    #[error("local directory is not accessible: {0}")]
    Local(String),
    #[error("remote probe failed: {0}")]
    Remote(String),
    #[error("remote output exceeded the bound")]
    TooLarge,
}

/// A resolved folder source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FolderSource {
    Local { path: PathBuf },
    Remote { spec: RemoteSpec },
}

impl FolderSource {
    pub fn label(&self) -> String {
        match self {
            Self::Local { path } => format!("local:{}", path.display()),
            Self::Remote { spec } => format!("ssh:{}", spec.display()),
        }
    }
    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Remote { .. })
    }

    /// Resolve a user spec to a folder source: a local path when it has no
    /// remote marker, otherwise a remote spec.
    pub fn resolve(spec: &str) -> Result<FolderSource, FolderSourceError> {
        let spec = spec.trim();
        let looks_remote = spec.starts_with("ssh://")
            || (spec.contains('@') && spec.contains(':'))
            || spec
                .split_once(':')
                .is_some_and(|(host, _)| !host.contains('/') && !host.contains('\\'));
        if looks_remote && let Ok(remote) = RemoteSpec::parse(spec) {
            return Ok(FolderSource::Remote { spec: remote });
        }
        let path = PathBuf::from(spec);
        if !path.is_dir() {
            return Err(FolderSourceError::Local(spec.to_owned()));
        }
        Ok(FolderSource::Local { path })
    }
}

/// One SSH remote directory. `host` may be an `~/.ssh/config` alias.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSpec {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<PathBuf>,
    pub path: String,
}

impl RemoteSpec {
    pub fn display(&self) -> String {
        let user = self.user.as_deref().map(|u| format!("{u}@")).unwrap_or_default();
        let port = self.port.map(|p| format!(":{p}")).unwrap_or_default();
        format!("{user}{host}{port}:{path}", host = self.host, path = self.path)
    }

    /// Accepts `ssh://[user@]host[:port]/path`, `[user@]host:/path`, and an
    /// optional `?identity=/path/to/key` query. A bare `alias:/path` resolves
    /// through the user's SSH config.
    pub fn parse(spec: &str) -> Result<Self, FolderSourceError> {
        let spec = spec.trim();
        if spec.is_empty() || spec.len() > MAX_SPEC_BYTES {
            return Err(FolderSourceError::InvalidSpec);
        }
        let (body, identity) = match spec.split_once("?identity=") {
            Some((body, key)) => (body, Some(PathBuf::from(key))),
            None => (spec, None),
        };
        if let Some(rest) = body.strip_prefix("ssh://") {
            let (authority, path) = rest
                .split_once('/')
                .ok_or(FolderSourceError::InvalidSpec)?;
            let (user, host_port) = match authority.split_once('@') {
                Some((user, host_port)) => (Some(user.to_owned()), host_port),
                None => (None, authority),
            };
            let (host, port) = match host_port.rsplit_once(':') {
                Some((host, port)) => (
                    host.to_owned(),
                    Some(port.parse::<u16>().map_err(|_| FolderSourceError::InvalidSpec)?),
                ),
                None => (host_port.to_owned(), None),
            };
            if host.is_empty() {
                return Err(FolderSourceError::InvalidSpec);
            }
            return Ok(Self {
                host,
                user,
                port,
                identity,
                path: format!("/{path}"),
            });
        }
        // scp-like `[user@]host:path` (host may be an ssh alias).
        let (left, path) = body.split_once(':').ok_or(FolderSourceError::InvalidSpec)?;
        let (user, host) = match left.split_once('@') {
            Some((user, host)) => (Some(user.to_owned()), host.to_owned()),
            None => (None, left.to_owned()),
        };
        if host.is_empty() || path.is_empty() {
            return Err(FolderSourceError::InvalidSpec);
        }
        Ok(Self {
            host,
            user,
            port: None,
            identity,
            path: path.to_owned(),
        })
    }

    /// Build the argv for a bounded, read-only `ls` probe. `-o BatchMode=yes`
    /// forbids interactive prompts so a missing credential fails fast.
    pub fn probe_argv(&self, path: &str) -> Vec<String> {
        let mut argv = vec![
            "ssh".to_owned(),
            "-o".to_owned(),
            "BatchMode=yes".to_owned(),
            "-o".to_owned(),
            "ConnectTimeout=5".to_owned(),
        ];
        if let Some(port) = self.port {
            argv.push("-p".to_owned());
            argv.push(port.to_string());
        }
        if let Some(identity) = &self.identity {
            argv.push("-i".to_owned());
            argv.push(identity.display().to_string());
        }
        let target = match &self.user {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        };
        argv.push("--".to_owned());
        argv.push(target);
        argv.push(format!("ls -1Ap -- {}", shell_quote(path)));
        argv
    }

    /// Read-only directory listing. Uses `ZENPI_SSH_BIN` when set so tests can
    /// substitute a deterministic stub.
    pub fn probe(&self, path: &str) -> Result<Vec<String>, FolderSourceError> {
        let ssh = std::env::var("ZENPI_SSH_BIN").unwrap_or_else(|_| "ssh".to_owned());
        let argv = self.probe_argv(path);
        let output = Command::new(ssh)
            .args(&argv[1..])
            .output()
            .map_err(|error| FolderSourceError::Remote(error.to_string()))?;
        if !output.status.success() {
            return Err(FolderSourceError::Remote(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        if output.stdout.len() > MAX_REMOTE_OUTPUT_BYTES {
            return Err(FolderSourceError::TooLarge);
        }
        Ok(parse_listing(&output.stdout))
    }
}

pub fn parse_listing(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "./" && *line != "../")
        .take(MAX_REMOTE_ENTRIES)
        .map(|line| line.to_owned())
        .collect()
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '~'))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// Bounded local listing used to keep the picker and the remote probe
/// consistent.
pub fn list_local(path: &Path) -> Result<Vec<String>, FolderSourceError> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path).map_err(|error| FolderSourceError::Local(error.to_string()))? {
        if entries.len() >= MAX_REMOTE_ENTRIES {
            break;
        }
        if let Ok(entry) = entry {
            let mut name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                name.push('/');
            }
            entries.push(name);
        }
    }
    entries.sort();
    Ok(entries)
}
