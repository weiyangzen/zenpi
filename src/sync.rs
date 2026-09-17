//! `/sync`: make one user requirement a first-class row in the project's
//! single-authority blueprint, idempotently, and hand its execution to the
//! host's runtime-intent owner.
//!
//! The blueprint stays the only checklist authority. This module only appends
//! a new row (stable `ZS1-NNN` id) plus a durable ledger entry; it never
//! rewrites or reinterprets existing rows.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("no blueprint found (set ZENPI_BLUEPRINT or run inside a project with Docs/*blueprint*.md)")]
    NoBlueprint,
    #[error("requirement is empty")]
    Empty,
    #[error("blueprint exceeds the bounded size")]
    TooLarge,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("ledger: {0}")]
    Ledger(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReceipt {
    pub item_id: String,
    pub blueprint: String,
    pub digest: String,
    pub duplicate: bool,
    pub queued: bool,
}

const MAX_BLUEPRINT_BYTES: usize = 4 * 1024 * 1024;
const MAX_REQUIREMENT_BYTES: usize = 8 * 1024;

/// Resolve the authoritative blueprint for `workspace`.
pub fn discover_blueprint(workspace: &Path) -> Result<PathBuf, SyncError> {
    if let Some(path) = std::env::var_os("ZENPI_BLUEPRINT") {
        let path = PathBuf::from(path);
        let path = if path.is_absolute() { path } else { workspace.join(path) };
        if path.is_file() {
            return Ok(path);
        }
    }
    let docs = workspace.join("Docs");
    if let Ok(entries) = fs::read_dir(&docs) {
        let mut candidates: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().and_then(|e| e.to_str()) == Some("md")
                    && path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.to_ascii_lowercase().contains("blueprint"))
            })
            .collect();
        candidates.sort();
        if let Some(path) = candidates.into_iter().next() {
            return Ok(path);
        }
    }
    Err(SyncError::NoBlueprint)
}

fn digest_of(requirement: &str) -> String {
    let normalized = requirement.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn next_item_id(blueprint: &str) -> Result<String, SyncError> {
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let mut rest = blueprint;
    while let Some(pos) = rest.find("ZS1-") {
        let tail = &rest[pos + 4..];
        if tail.len() >= 3
            && tail[..3].chars().all(|c| c.is_ascii_digit())
            && let Ok(number) = tail[..3].parse::<u32>()
        {
            seen.insert(number);
        }
        rest = &rest[pos + 4..];
    }
    // Reserve a dedicated sync range so generated ids never collide with the
    // hand-authored catalogue.
    let mut candidate = 900u32;
    while seen.contains(&candidate) {
        candidate += 1;
        if candidate > 999 {
            return Err(SyncError::Ledger("sync id range exhausted".into()));
        }
    }
    Ok(format!("ZS1-{candidate:03}"))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), SyncError> {
    let dir = path.parent().ok_or(SyncError::NoBlueprint)?;
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".sync-{}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(SyncError::Io)
}

/// Append `requirement` to the blueprint and record it in the sync ledger.
pub fn sync_requirement(workspace: &Path, requirement: &str) -> Result<SyncReceipt, SyncError> {
    let requirement = requirement.trim();
    if requirement.is_empty() {
        return Err(SyncError::Empty);
    }
    if requirement.len() > MAX_REQUIREMENT_BYTES {
        return Err(SyncError::TooLarge);
    }
    let blueprint_path = discover_blueprint(workspace)?;
    let blueprint = fs::read_to_string(&blueprint_path)?;
    if blueprint.len() > MAX_BLUEPRINT_BYTES {
        return Err(SyncError::TooLarge);
    }
    let digest = digest_of(requirement);
    let ledger_path = workspace.join("Docs/execution/sync_ledger.json");
    let mut ledger = read_ledger(&ledger_path);
    if let Some(existing) = ledger.iter().find(|entry| entry.digest == digest) {
        return Ok(SyncReceipt {
            item_id: existing.item_id.clone(),
            blueprint: blueprint_path.display().to_string(),
            digest,
            duplicate: true,
            queued: false,
        });
    }
    let item_id = next_item_id(&blueprint)?;
    let summary = summarize(requirement);
    let mut updated = blueprint;
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&format!(
        "\n- [ ] **{item_id}** — {summary}；layer `L3` | Depends: — | Owner scope: /sync 追加的用户要求 | Owned paths: — | Validators: G-CODE | Rollback: 撤回本项 | Estimate: 由 /sync 追加 | Estimated LOC: 0\n"
    ));
    updated.push_str(&format!(
        "\n  /sync {digest}: {requirement}\n"
    ));
    atomic_write(&blueprint_path, updated.as_bytes())?;
    ledger.push(SyncEntry {
        item_id: item_id.clone(),
        digest: digest.clone(),
        requirement: requirement.to_owned(),
        appended_at: now_iso(),
    });
    write_ledger(&ledger_path, &ledger)?;
    Ok(SyncReceipt {
        item_id,
        blueprint: blueprint_path.display().to_string(),
        digest,
        duplicate: false,
        queued: true,
    })
}

fn summarize(requirement: &str) -> String {
    const MAX: usize = 90;
    let single = requirement.split_whitespace().collect::<Vec<_>>().join(" ");
    if single.chars().count() <= MAX {
        single
    } else {
        let mut out: String = single.chars().take(MAX).collect();
        out.push('…');
        out
    }
}

fn now_iso() -> String {
    // Second-resolution UTC without extra dependencies.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    crate::b3::unix_ms_to_rfc3339(now.saturating_mul(1000))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SyncEntry {
    item_id: String,
    digest: String,
    requirement: String,
    appended_at: String,
}

fn read_ledger(path: &Path) -> Vec<SyncEntry> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<SyncEntry>>(&text).ok())
        .unwrap_or_default()
}

fn write_ledger(path: &Path, ledger: &[SyncEntry]) -> Result<(), SyncError> {
    let text = serde_json::to_string_pretty(ledger).map_err(|e| SyncError::Ledger(e.to_string()))?;
    atomic_write(path, text.as_bytes())
}
