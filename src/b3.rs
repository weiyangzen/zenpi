//! Small, typed b3ehive bridge records.
//!
//! zenpi does not embed a scheduler or daemon.  These records are deliberately
//! inert: they carry bounded budgets, handoff context, and validation evidence
//! between agents while leaving admission and execution to the host.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// The on-disk handoff contract is intentionally small.  Keep these limits
/// public so protocol and integration tests can assert the same boundary
/// without duplicating magic numbers.
pub const HANDOFF_SCHEMA_VERSION: u16 = 1;
pub const MAX_HANDOFF_RECORD_BYTES: usize = 64 * 1024;
pub const MAX_HANDOFF_SUMMARY_BYTES: usize = 16 * 1024;
pub const MAX_HANDOFF_ARTIFACTS: usize = 64;
pub const MAX_ARTIFACT_PATH_BYTES: usize = 1024;

/// Format a Unix millisecond timestamp as UTC RFC3339 without pulling a time
/// runtime into the small binary. The conversion is deterministic and has no
/// fallible branches.
pub fn unix_ms_to_rfc3339(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    let days = (seconds / 86_400).min(i64::MAX as u64) as i64;
    let day_seconds = seconds % 86_400;
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    let (year, month, day) = civil_date_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_date_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    // Howard Hinnant's proleptic Gregorian conversion, shifted to 1970-01-01.
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    (year, month as u32, day as u32)
}

const MAX_ID: usize = 256;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum B3Error {
    #[error("{field} must be non-empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} bytes")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains a line break")]
    Newline { field: &'static str },
    #[error("{field} appears to contain secret material")]
    SecretPayload { field: &'static str },
    #[error("{field} contains an invalid identifier")]
    InvalidId { field: &'static str },
    #[error("{field} has too many entries (maximum {max})")]
    TooMany { field: &'static str, max: usize },
    #[error("resource budget exceeded for {field}")]
    BudgetExceeded { field: &'static str },
    #[error("lease is not active")]
    LeaseInactive,
    #[error("unsupported b3 schema version {found} (expected {expected})")]
    SchemaVersion { found: u16, expected: u16 },
    #[error("handoff record exceeds {max} bytes")]
    RecordTooLong { max: usize },
    #[error("handoff summary exceeds {max} bytes")]
    SummaryTooLong { max: usize },
    #[error("handoff contains too many artifacts (maximum {max})")]
    ArtifactLimit { max: usize },
    #[error("artifact path is not repository-relative: {path}")]
    InvalidArtifactPath { path: String },
    #[error("handoff digest does not match canonical content")]
    DigestMismatch,
    #[error("handoff recipient mismatch: expected {expected}, got {actual}")]
    RecipientMismatch { expected: String, actual: String },
    #[error("handoff session mismatch: expected {expected}, got {actual}")]
    SessionMismatch { expected: String, actual: String },
    #[error("invalid RFC3339 timestamp")]
    InvalidTimestamp,
    #[error("side-effect gate is required")]
    GateRequired,
    #[error("side-effect was denied by gate")]
    SideEffectDenied,
    #[error("side-effect gate does not cover the requested kind")]
    GateKindMismatch,
    #[error("only the canonical master may accept a result manifest")]
    MasterAuthorityRequired,
    #[error("manifest state transition is invalid")]
    InvalidManifestState,
    #[error("manifest checksum does not match canonical content")]
    ManifestChecksumMismatch,
}

fn id(value: &str, field: &'static str) -> Result<(), B3Error> {
    if value.is_empty() {
        return Err(B3Error::Empty { field });
    }
    if value.len() > MAX_ID {
        return Err(B3Error::TooLong { field, max: MAX_ID });
    }
    if value
        .chars()
        .any(|c| !(c.is_ascii_alphanumeric() || "._:/-".contains(c)))
    {
        return Err(B3Error::InvalidId { field });
    }
    Ok(())
}

fn bounded_single_line(value: &str, field: &'static str, max: usize) -> Result<(), B3Error> {
    if value.trim().is_empty() {
        return Err(B3Error::Empty { field });
    }
    if value.len() > max {
        return Err(B3Error::TooLong { field, max });
    }
    if value.contains(['\r', '\n', '\0']) {
        return Err(B3Error::Newline { field });
    }
    Ok(())
}

/// Validate a repository-relative path without touching the filesystem.
///
/// Paths are references in a handoff, never commands or file contents.  We
/// reject both Unix and Windows absolute/traversal spellings so a record
/// produced on one platform cannot become dangerous on another.
pub fn validate_artifact_path(path: &str) -> Result<(), B3Error> {
    if path.is_empty() || path.len() > MAX_ARTIFACT_PATH_BYTES {
        return Err(B3Error::InvalidArtifactPath {
            path: path.to_owned(),
        });
    }
    if path.starts_with('/')
        || path.starts_with('\\')
        || path.contains(':')
        || path.contains(['\r', '\n', '\0'])
        || path.contains("://")
    {
        return Err(B3Error::InvalidArtifactPath {
            path: path.to_owned(),
        });
    }
    let mut saw_component = false;
    for component in path.split(['/', '\\']) {
        if component.is_empty() || component == "." || component == ".." {
            return Err(B3Error::InvalidArtifactPath {
                path: path.to_owned(),
            });
        }
        if component.chars().any(char::is_control) {
            return Err(B3Error::InvalidArtifactPath {
                path: path.to_owned(),
            });
        }
        saw_component = true;
    }
    if !saw_component {
        return Err(B3Error::InvalidArtifactPath {
            path: path.to_owned(),
        });
    }
    // Handoffs may describe source artifacts, but must never carry common
    // credential files.  This is deliberately a small deny-list; callers can
    // apply stricter repository policy before accepting a record.
    let lower = path.to_ascii_lowercase();
    let basename = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    if matches!(
        basename,
        ".env" | ".env.local" | ".npmrc" | ".pypirc" | "id_rsa" | "id_ed25519"
    ) || basename.ends_with(".pem")
        || basename.ends_with(".key")
    {
        return Err(B3Error::InvalidArtifactPath {
            path: path.to_owned(),
        });
    }
    Ok(())
}

fn validate_timestamp(value: &str) -> Result<(), B3Error> {
    // Keep the boundary dependency-free while still rejecting ambiguous or
    // line-bearing values. Accept RFC3339 UTC/offset forms only.
    if value.len() < 20 || value.contains(['\r', '\n', '\0']) {
        return Err(B3Error::InvalidTimestamp);
    }
    let bytes = value.as_bytes();
    let separators = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    for (index, expected) in separators {
        if bytes.get(index).copied() != Some(expected) {
            return Err(B3Error::InvalidTimestamp);
        }
    }
    for index in [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18] {
        if !bytes.get(index).is_some_and(u8::is_ascii_digit) {
            return Err(B3Error::InvalidTimestamp);
        }
    }
    let month = value[5..7].parse::<u32>().ok();
    let day = value[8..10].parse::<u32>().ok();
    let hour = value[11..13].parse::<u32>().ok();
    let minute = value[14..16].parse::<u32>().ok();
    let second = value[17..19].parse::<u32>().ok();
    if !matches!(
        (month, day, hour, minute, second),
        (
            Some(1..=12),
            Some(1..=31),
            Some(0..=23),
            Some(0..=59),
            Some(0..=60)
        )
    ) {
        return Err(B3Error::InvalidTimestamp);
    }
    let zone_start = if bytes.get(19) == Some(&b'Z') {
        19
    } else if bytes.get(19) == Some(&b'.') {
        let end = value[20..]
            .bytes()
            .position(|byte| matches!(byte, b'Z' | b'+' | b'-'))
            .map(|offset| offset + 20)
            .ok_or(B3Error::InvalidTimestamp)?;
        if end == 20 || !value[20..end].bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(B3Error::InvalidTimestamp);
        }
        end
    } else {
        return Err(B3Error::InvalidTimestamp);
    };
    if bytes.get(zone_start) == Some(&b'Z') {
        if zone_start != value.len() - 1 {
            return Err(B3Error::InvalidTimestamp);
        }
    } else {
        if value.len() != zone_start + 6
            || !matches!(bytes.get(zone_start), Some(b'+') | Some(b'-'))
            || bytes.get(zone_start + 3) != Some(&b':')
            || !bytes
                .get(zone_start + 1..zone_start + 3)
                .is_some_and(|digits| digits.iter().all(u8::is_ascii_digit))
            || !bytes
                .get(zone_start + 4..zone_start + 6)
                .is_some_and(|digits| digits.iter().all(u8::is_ascii_digit))
        {
            return Err(B3Error::InvalidTimestamp);
        }
        let offset_hour = value[zone_start + 1..zone_start + 3].parse::<u32>().ok();
        let offset_minute = value[zone_start + 4..zone_start + 6].parse::<u32>().ok();
        if !matches!((offset_hour, offset_minute), (Some(0..=23), Some(0..=59))) {
            return Err(B3Error::InvalidTimestamp);
        }
    }
    Ok(())
}

fn digest_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        // Writing into a String cannot fail, so avoid an unwrap/expect on this
        // infallible formatting path.
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// A compact cross-agent handoff.  Artifacts are references, not file content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Handoff {
    pub handoff_id: String,
    pub from: String,
    pub to: Option<String>,
    pub summary: String,
    #[serde(default)]
    pub artifacts: Vec<String>,
    pub created_at_ms: u64,
}

impl Handoff {
    pub fn new(
        handoff_id: impl Into<String>,
        from: impl Into<String>,
        to: Option<String>,
        summary: impl Into<String>,
        artifacts: Vec<String>,
        created_at_ms: u64,
    ) -> Result<Self, B3Error> {
        let handoff = Self {
            handoff_id: handoff_id.into(),
            from: from.into(),
            to,
            summary: summary.into(),
            artifacts,
            created_at_ms,
        };
        handoff.validate()?;
        Ok(handoff)
    }

    pub fn validate(&self) -> Result<(), B3Error> {
        id(&self.handoff_id, "handoff_id")?;
        id(&self.from, "from")?;
        if let Some(to) = &self.to {
            id(to, "to")?;
        }
        bounded_single_line(&self.summary, "summary", MAX_HANDOFF_SUMMARY_BYTES)?;
        if self.artifacts.len() > MAX_HANDOFF_ARTIFACTS {
            return Err(B3Error::ArtifactLimit {
                max: MAX_HANDOFF_ARTIFACTS,
            });
        }
        for artifact in &self.artifacts {
            validate_artifact_path(artifact)?;
        }
        Ok(())
    }
}

/// Versioned handoff exchanged by headless agents.
///
/// `digest` is the lowercase SHA-256 of the canonical JSON representation of
/// every field except `digest`.  The digest is computed over borrowed views,
/// so validating or signing a record does not clone its potentially large
/// summary or artifact list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffRecord {
    pub schema_version: u16,
    pub from: String,
    pub to: String,
    pub claim_id: String,
    pub summary: String,
    pub artifacts: Vec<String>,
    pub session_id: String,
    pub created_at: String,
    pub digest: String,
}

#[derive(Debug, Serialize)]
struct HandoffUnsigned<'a> {
    schema_version: u16,
    from: &'a str,
    to: &'a str,
    claim_id: &'a str,
    summary: &'a str,
    artifacts: &'a [String],
    session_id: &'a str,
    created_at: &'a str,
}

impl HandoffRecord {
    /// Construct and sign a record in one operation.
    pub fn new(
        from: impl Into<String>,
        to: impl Into<String>,
        claim_id: impl Into<String>,
        summary: impl Into<String>,
        artifacts: Vec<String>,
        session_id: impl Into<String>,
        created_at: impl Into<String>,
    ) -> Result<Self, B3Error> {
        let mut record = Self {
            schema_version: HANDOFF_SCHEMA_VERSION,
            from: from.into(),
            to: to.into(),
            claim_id: claim_id.into(),
            summary: summary.into(),
            artifacts,
            session_id: session_id.into(),
            created_at: created_at.into(),
            digest: String::new(),
        };
        record.validate_without_digest()?;
        record.digest = record.compute_digest()?;
        record.validate()?;
        Ok(record)
    }

    /// Build a record from an existing wire value and verify its digest.
    pub fn from_wire(value: Self) -> Result<Self, B3Error> {
        value.validate()?;
        Ok(value)
    }

    pub fn decode_line(line: &str) -> Result<Self, B3Error> {
        if line.len() > MAX_HANDOFF_RECORD_BYTES
            || line.bytes().filter(|byte| *byte == b'\n').count() > 1
        {
            return Err(B3Error::RecordTooLong {
                max: MAX_HANDOFF_RECORD_BYTES,
            });
        }
        let line = line.strip_suffix('\n').unwrap_or(line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        let value: Self = serde_json::from_str(line).map_err(|_| B3Error::InvalidId {
            field: "handoff_json",
        })?;
        Self::from_wire(value)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, B3Error> {
        serde_json::to_vec(&HandoffUnsigned {
            schema_version: self.schema_version,
            from: &self.from,
            to: &self.to,
            claim_id: &self.claim_id,
            summary: &self.summary,
            artifacts: &self.artifacts,
            session_id: &self.session_id,
            created_at: &self.created_at,
        })
        .map_err(|_| B3Error::RecordTooLong {
            max: MAX_HANDOFF_RECORD_BYTES,
        })
    }

    pub fn compute_digest(&self) -> Result<String, B3Error> {
        Ok(digest_hex(&self.canonical_bytes()?))
    }

    pub fn verify_digest(&self) -> Result<(), B3Error> {
        let expected = self.compute_digest()?;
        if self.digest == expected {
            Ok(())
        } else {
            Err(B3Error::DigestMismatch)
        }
    }

    /// Validate schema, bounds, paths, and digest without mutating the record.
    pub fn validate(&self) -> Result<(), B3Error> {
        self.validate_without_digest()?;
        // An empty digest is only valid during construction, never on the
        // wire.  Enforce the exact SHA-256 text shape to reject ambiguity.
        if self.digest.len() != 64 || !self.digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(B3Error::DigestMismatch);
        }
        self.verify_digest()?;
        let wire_len = serde_json::to_vec(self)
            .map_err(|_| B3Error::RecordTooLong {
                max: MAX_HANDOFF_RECORD_BYTES,
            })?
            .len();
        if wire_len > MAX_HANDOFF_RECORD_BYTES {
            return Err(B3Error::RecordTooLong {
                max: MAX_HANDOFF_RECORD_BYTES,
            });
        }
        Ok(())
    }

    fn validate_without_digest(&self) -> Result<(), B3Error> {
        if self.schema_version != HANDOFF_SCHEMA_VERSION {
            return Err(B3Error::SchemaVersion {
                found: self.schema_version,
                expected: HANDOFF_SCHEMA_VERSION,
            });
        }
        id(&self.from, "from")?;
        id(&self.to, "to")?;
        id(&self.claim_id, "claim_id")?;
        id(&self.session_id, "session_id")?;
        if self.summary.trim().is_empty() {
            return Err(B3Error::Empty { field: "summary" });
        }
        if self.summary.len() > MAX_HANDOFF_SUMMARY_BYTES {
            return Err(B3Error::SummaryTooLong {
                max: MAX_HANDOFF_SUMMARY_BYTES,
            });
        }
        if self.summary.contains(['\r', '\n', '\0']) {
            return Err(B3Error::Newline { field: "summary" });
        }
        if looks_like_secret(&self.summary) {
            return Err(B3Error::SecretPayload { field: "summary" });
        }
        if self.artifacts.len() > MAX_HANDOFF_ARTIFACTS {
            return Err(B3Error::ArtifactLimit {
                max: MAX_HANDOFF_ARTIFACTS,
            });
        }
        for artifact in &self.artifacts {
            validate_artifact_path(artifact)?;
        }
        validate_timestamp(&self.created_at)?;
        Ok(())
    }

    /// Check recipient and session identity before a caller appends the
    /// record.  This method has no filesystem side effects.
    pub fn validate_for(&self, recipient: &str, session_id: &str) -> Result<(), B3Error> {
        self.validate()?;
        if self.to != recipient {
            return Err(B3Error::RecipientMismatch {
                expected: recipient.to_owned(),
                actual: self.to.clone(),
            });
        }
        if self.session_id != session_id {
            return Err(B3Error::SessionMismatch {
                expected: session_id.to_owned(),
                actual: self.session_id.clone(),
            });
        }
        Ok(())
    }

    /// Encode one bounded JSONL record.  The final newline is included.
    pub fn encode_line(&self) -> Result<String, B3Error> {
        self.validate()?;
        let mut bytes = serde_json::to_vec(self).map_err(|_| B3Error::RecordTooLong {
            max: MAX_HANDOFF_RECORD_BYTES,
        })?;
        if bytes.len() + 1 > MAX_HANDOFF_RECORD_BYTES {
            return Err(B3Error::RecordTooLong {
                max: MAX_HANDOFF_RECORD_BYTES,
            });
        }
        bytes.push(b'\n');
        String::from_utf8(bytes).map_err(|_| B3Error::RecordTooLong {
            max: MAX_HANDOFF_RECORD_BYTES,
        })
    }
}

fn looks_like_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("begin private key")
        || lower.contains("api_key=")
        || lower.contains("apikey=")
        || lower.contains("access_token=")
        || lower.contains("secret=")
        || value.contains("sk-")
        || value.contains("ghp_")
        || value.contains("AKIA")
}

/// Result-manifest schema version.  It is separate from the handoff version
/// so either record can evolve independently at its persistence boundary.
pub const MANIFEST_SCHEMA_VERSION: u16 = 1;
pub const MAX_MANIFEST_BYTES: usize = MAX_HANDOFF_RECORD_BYTES;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManifestStatus {
    Candidate,
    SelfTested,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ValidationOutcome {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationRecord {
    pub command: String,
    pub outcome: ValidationOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ValidationRecord {
    pub fn new(command: impl Into<String>, outcome: ValidationOutcome) -> Result<Self, B3Error> {
        let record = Self {
            command: command.into(),
            outcome,
            detail: None,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), B3Error> {
        bounded_single_line(&self.command, "validation command", 1024)?;
        if let Some(detail) = &self.detail {
            bounded_single_line(detail, "validation detail", 4096)?;
        }
        Ok(())
    }
}

/// A truthful, bounded result summary.  It records evidence; it does not
/// itself mutate a Blueprint or perform integration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResultManifest {
    pub schema_version: u16,
    pub claim_id: String,
    pub worker: String,
    pub changed_paths: Vec<String>,
    pub validations: Vec<ValidationRecord>,
    pub status: ManifestStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_by: Option<String>,
    pub checksum: String,
}

#[derive(Debug, Serialize)]
struct ManifestUnsigned<'a> {
    schema_version: u16,
    claim_id: &'a str,
    worker: &'a str,
    changed_paths: &'a [String],
    validations: &'a [ValidationRecord],
    status: ManifestStatus,
    accepted_by: &'a Option<String>,
}

impl ResultManifest {
    pub fn new(
        claim_id: impl Into<String>,
        worker: impl Into<String>,
        changed_paths: Vec<String>,
        validations: Vec<ValidationRecord>,
    ) -> Result<Self, B3Error> {
        let mut manifest = Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            claim_id: claim_id.into(),
            worker: worker.into(),
            changed_paths,
            validations,
            status: ManifestStatus::Candidate,
            accepted_by: None,
            checksum: String::new(),
        };
        manifest.validate_without_checksum()?;
        manifest.checksum = manifest.compute_checksum()?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, B3Error> {
        serde_json::to_vec(&ManifestUnsigned {
            schema_version: self.schema_version,
            claim_id: &self.claim_id,
            worker: &self.worker,
            changed_paths: &self.changed_paths,
            validations: &self.validations,
            status: self.status,
            accepted_by: &self.accepted_by,
        })
        .map_err(|_| B3Error::RecordTooLong {
            max: MAX_MANIFEST_BYTES,
        })
    }

    pub fn compute_checksum(&self) -> Result<String, B3Error> {
        Ok(digest_hex(&self.canonical_bytes()?))
    }

    pub fn validate(&self) -> Result<(), B3Error> {
        self.validate_without_checksum()?;
        if self.checksum.len() != 64 || !self.checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(B3Error::ManifestChecksumMismatch);
        }
        if self.checksum != self.compute_checksum()? {
            return Err(B3Error::ManifestChecksumMismatch);
        }
        if serde_json::to_vec(self)
            .map_err(|_| B3Error::RecordTooLong {
                max: MAX_MANIFEST_BYTES,
            })?
            .len()
            > MAX_MANIFEST_BYTES
        {
            return Err(B3Error::RecordTooLong {
                max: MAX_MANIFEST_BYTES,
            });
        }
        Ok(())
    }

    fn validate_without_checksum(&self) -> Result<(), B3Error> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(B3Error::SchemaVersion {
                found: self.schema_version,
                expected: MANIFEST_SCHEMA_VERSION,
            });
        }
        id(&self.claim_id, "claim_id")?;
        id(&self.worker, "worker")?;
        if self.changed_paths.len() > MAX_HANDOFF_ARTIFACTS {
            return Err(B3Error::ArtifactLimit {
                max: MAX_HANDOFF_ARTIFACTS,
            });
        }
        for (index, path) in self.changed_paths.iter().enumerate() {
            validate_artifact_path(path)?;
            if self.changed_paths[..index]
                .iter()
                .any(|prior| prior == path)
            {
                return Err(B3Error::InvalidArtifactPath { path: path.clone() });
            }
        }
        if self.validations.len() > MAX_HANDOFF_ARTIFACTS {
            return Err(B3Error::TooMany {
                field: "validations",
                max: MAX_HANDOFF_ARTIFACTS,
            });
        }
        for validation in &self.validations {
            validation.validate()?;
        }
        match self.status {
            ManifestStatus::Accepted if self.accepted_by.as_deref() != Some("master") => {
                return Err(B3Error::MasterAuthorityRequired);
            }
            ManifestStatus::Accepted => {}
            _ if self.accepted_by.is_some() => return Err(B3Error::InvalidManifestState),
            _ => {}
        }
        Ok(())
    }

    /// Mark a candidate as worker self-tested.  Failed checks are refused and
    /// the manifest remains a candidate, so callers cannot accidentally emit a
    /// misleading success state.
    pub fn mark_self_tested(&mut self) -> Result<(), B3Error> {
        self.validate()?;
        if self.status != ManifestStatus::Candidate
            || self
                .validations
                .iter()
                .any(|validation| validation.outcome == ValidationOutcome::Failed)
        {
            return Err(B3Error::InvalidManifestState);
        }
        self.status = ManifestStatus::SelfTested;
        self.checksum = self.compute_checksum()?;
        Ok(())
    }

    /// Only the canonical Master can accept a self-tested result.  This is a
    /// local authority check, not a scheduler or remote authorization system.
    pub fn accept_by(&mut self, actor: &str) -> Result<(), B3Error> {
        self.validate()?;
        if actor != "master" {
            return Err(B3Error::MasterAuthorityRequired);
        }
        if self.status != ManifestStatus::SelfTested {
            return Err(B3Error::InvalidManifestState);
        }
        self.status = ManifestStatus::Accepted;
        self.accepted_by = Some("master".into());
        self.checksum = self.compute_checksum()?;
        Ok(())
    }

    pub fn reject(&mut self) -> Result<(), B3Error> {
        self.validate()?;
        if matches!(
            self.status,
            ManifestStatus::Accepted | ManifestStatus::Rejected
        ) {
            return Err(B3Error::InvalidManifestState);
        }
        self.status = ManifestStatus::Rejected;
        self.checksum = self.compute_checksum()?;
        Ok(())
    }

    pub fn is_accepted(&self) -> bool {
        self.status == ManifestStatus::Accepted
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectKind {
    ProtectedPath,
    DangerousCommand,
    LargeDiff,
    SecretExposure,
    PushOrPublish,
    DeleteOrDestructiveWrite,
    NetworkOrSpend,
    IdentityWrite,
    #[serde(alias = "blueprint_write")]
    AuthoritativeBlueprintWrite,
}

/// Decision record for a risky side effect.  Ordinary local reads/writes do
/// not require a gate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SideEffectGate {
    pub gate_id: String,
    pub kind: SideEffectKind,
    pub allowed: bool,
    pub decided_by: String,
    pub reason: String,
    pub decided_at_ms: u64,
}

impl SideEffectGate {
    pub fn allow(
        gate_id: impl Into<String>,
        kind: SideEffectKind,
        decided_by: impl Into<String>,
        reason: impl Into<String>,
        decided_at_ms: u64,
    ) -> Result<Self, B3Error> {
        let gate = Self {
            gate_id: gate_id.into(),
            kind,
            allowed: true,
            decided_by: decided_by.into(),
            reason: reason.into(),
            decided_at_ms,
        };
        gate.validate()?;
        Ok(gate)
    }

    pub fn deny(
        gate_id: impl Into<String>,
        kind: SideEffectKind,
        decided_by: impl Into<String>,
        reason: impl Into<String>,
        decided_at_ms: u64,
    ) -> Result<Self, B3Error> {
        let gate = Self {
            gate_id: gate_id.into(),
            kind,
            allowed: false,
            decided_by: decided_by.into(),
            reason: reason.into(),
            decided_at_ms,
        };
        gate.validate()?;
        Ok(gate)
    }

    pub fn validate(&self) -> Result<(), B3Error> {
        id(&self.gate_id, "gate_id")?;
        id(&self.decided_by, "decided_by")?;
        bounded_single_line(&self.reason, "reason", MAX_HANDOFF_SUMMARY_BYTES)?;
        Ok(())
    }

    /// Apply a gate to one risky operation.  A missing, malformed, denied, or
    /// mismatched gate always fails closed; callers must explicitly opt in to
    /// this check for operations that cross a side-effect boundary.
    pub fn authorize(&self, requested_kind: SideEffectKind) -> Result<(), B3Error> {
        self.validate()?;
        if self.kind != requested_kind {
            return Err(B3Error::GateKindMismatch);
        }
        if !self.allowed {
            return Err(B3Error::SideEffectDenied);
        }
        Ok(())
    }
}

pub fn require_side_effect_gate(
    gate: Option<&SideEffectGate>,
    requested_kind: SideEffectKind,
) -> Result<(), B3Error> {
    let Some(gate) = gate else {
        return Err(B3Error::GateRequired);
    };
    gate.authorize(requested_kind)
}

/// Small accounting records understood by a b3ehive host. They are data-only:
/// zenpi never starts workers, schedules retries, or hides nested agents.
pub const MAX_B3_LIST: usize = 64;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceBudget {
    pub tokens: u64,
    pub wall_clock_ms: u64,
    pub attempts: u32,
    pub disk_bytes: u64,
}

impl ResourceBudget {
    pub fn fits_within(self, limit: Self) -> bool {
        self.tokens <= limit.tokens
            && self.wall_clock_ms <= limit.wall_clock_ms
            && self.attempts <= limit.attempts
            && self.disk_bytes <= limit.disk_bytes
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            tokens: self.tokens.checked_add(other.tokens)?,
            wall_clock_ms: self.wall_clock_ms.checked_add(other.wall_clock_ms)?,
            attempts: self.attempts.checked_add(other.attempts)?,
            disk_bytes: self.disk_bytes.checked_add(other.disk_bytes)?,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeStatus {
    Active,
    Exhausted,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceEnvelope {
    pub envelope_id: String,
    pub owner: String,
    pub limit: ResourceBudget,
    pub spent: ResourceBudget,
    pub status: EnvelopeStatus,
}

impl ResourceEnvelope {
    pub fn new(
        envelope_id: impl Into<String>,
        owner: impl Into<String>,
        limit: ResourceBudget,
    ) -> Result<Self, B3Error> {
        let value = Self {
            envelope_id: envelope_id.into(),
            owner: owner.into(),
            limit,
            spent: ResourceBudget::default(),
            status: EnvelopeStatus::Active,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), B3Error> {
        id(&self.envelope_id, "envelope_id")?;
        id(&self.owner, "owner")?;
        if !self.spent.fits_within(self.limit) {
            return Err(B3Error::BudgetExceeded { field: "envelope" });
        }
        Ok(())
    }

    pub fn record(&mut self, cost: ResourceBudget) -> Result<(), B3Error> {
        if self.status != EnvelopeStatus::Active {
            return Err(B3Error::LeaseInactive);
        }
        let next = self
            .spent
            .checked_add(cost)
            .ok_or(B3Error::BudgetExceeded { field: "envelope" })?;
        if !next.fits_within(self.limit) {
            return Err(B3Error::BudgetExceeded { field: "envelope" });
        }
        self.spent = next;
        if self.spent == self.limit {
            self.status = EnvelopeStatus::Exhausted;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParentLeaseRef {
    pub parent_lease_id: String,
    pub nested_run_id: String,
    pub max_tokens: u64,
}

impl ParentLeaseRef {
    pub fn validate(&self) -> Result<(), B3Error> {
        id(&self.parent_lease_id, "parent_lease_id")?;
        id(&self.nested_run_id, "nested_run_id")
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LeaseStatus {
    Leased,
    Released,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceLease {
    pub lease_id: String,
    pub owner: String,
    pub workspace: String,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
    pub budget: ResourceBudget,
    pub spent: ResourceBudget,
    pub status: LeaseStatus,
    #[serde(default)]
    pub parent: Option<ParentLeaseRef>,
}

impl ResourceLease {
    pub fn validate(&self) -> Result<(), B3Error> {
        id(&self.lease_id, "lease_id")?;
        id(&self.owner, "owner")?;
        bounded_single_line(&self.workspace, "workspace", 1024)?;
        if self.expires_at_ms < self.issued_at_ms || !self.spent.fits_within(self.budget) {
            return Err(B3Error::BudgetExceeded { field: "lease" });
        }
        if let Some(parent) = &self.parent {
            parent.validate()?;
        }
        Ok(())
    }

    pub fn is_live(&self, now_ms: u64) -> bool {
        self.status == LeaseStatus::Leased && now_ms < self.expires_at_ms
    }

    pub fn validate_at(&self, now_ms: u64) -> Result<(), B3Error> {
        self.validate()?;
        if self.status == LeaseStatus::Leased && now_ms >= self.expires_at_ms {
            return Err(B3Error::LeaseInactive);
        }
        Ok(())
    }

    pub fn heartbeat(&mut self, now_ms: u64) -> Result<(), B3Error> {
        if !self.is_live(now_ms) {
            self.status = LeaseStatus::Expired;
            return Err(B3Error::LeaseInactive);
        }
        Ok(())
    }

    pub fn record(&mut self, cost: ResourceBudget) -> Result<(), B3Error> {
        if self.status != LeaseStatus::Leased {
            return Err(B3Error::LeaseInactive);
        }
        let next = self
            .spent
            .checked_add(cost)
            .ok_or(B3Error::BudgetExceeded { field: "lease" })?;
        if !next.fits_within(self.budget) {
            return Err(B3Error::BudgetExceeded { field: "lease" });
        }
        self.spent = next;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceRecord {
    pub evidence_id: String,
    pub claim_id: String,
    pub changed_paths: Vec<String>,
    pub commands: Vec<String>,
    pub validation: String,
    pub master_state: String,
}

impl EvidenceRecord {
    pub fn validate(&self) -> Result<(), B3Error> {
        id(&self.evidence_id, "evidence_id")?;
        id(&self.claim_id, "claim_id")?;
        if self.changed_paths.len() > MAX_B3_LIST || self.commands.len() > MAX_B3_LIST {
            return Err(B3Error::TooMany {
                field: "evidence",
                max: MAX_B3_LIST,
            });
        }
        for path in &self.changed_paths {
            validate_artifact_path(path)?;
        }
        for command in &self.commands {
            bounded_single_line(command, "command", 2048)?;
        }
        bounded_single_line(&self.validation, "validation", 4096)?;
        bounded_single_line(&self.master_state, "master_state", 64)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteDecision {
    pub route_id: String,
    pub parent_ref: String,
    pub route_class: String,
    pub runner: String,
    pub validator_strength: String,
}

impl RouteDecision {
    pub fn validate(&self) -> Result<(), B3Error> {
        id(&self.route_id, "route_id")?;
        id(&self.parent_ref, "parent_ref")?;
        bounded_single_line(&self.route_class, "route_class", 128)?;
        bounded_single_line(&self.runner, "runner", 128)?;
        bounded_single_line(&self.validator_strength, "validator_strength", 128)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EstimatorPolicy {
    pub estimate_id: String,
    pub task_ref: String,
    #[serde(default)]
    pub estimated_parameters: BTreeMap<String, u64>,
    pub hard_caps: ResourceBudget,
    pub rationale: String,
}

impl EstimatorPolicy {
    pub fn validate(&self) -> Result<(), B3Error> {
        id(&self.estimate_id, "estimate_id")?;
        id(&self.task_ref, "task_ref")?;
        if self.estimated_parameters.len() > MAX_B3_LIST {
            return Err(B3Error::TooMany {
                field: "estimated_parameters",
                max: MAX_B3_LIST,
            });
        }
        for key in self.estimated_parameters.keys() {
            bounded_single_line(key, "estimate parameter", 128)?;
        }
        bounded_single_line(&self.rationale, "rationale", 4096)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogGrain {
    Micro,
    Skill,
    Composition,
    Scaffold,
    Tool,
    Task,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObservedEffect {
    Helped,
    Neutral,
    Blocked,
    Wasted,
    UnderValidated,
    OverComplicated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LooperLog {
    pub log_id: String,
    pub grain: LogGrain,
    pub target_ref: String,
    pub instrument_ref: String,
    pub observed_effect: ObservedEffect,
    pub target_movement: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub master_state: String,
}

impl LooperLog {
    pub fn validate(&self) -> Result<(), B3Error> {
        id(&self.log_id, "log_id")?;
        id(&self.target_ref, "target_ref")?;
        id(&self.instrument_ref, "instrument_ref")?;
        bounded_single_line(&self.target_movement, "target_movement", 4096)?;
        if self.evidence_refs.len() > MAX_B3_LIST {
            return Err(B3Error::TooMany {
                field: "evidence_refs",
                max: MAX_B3_LIST,
            });
        }
        for reference in &self.evidence_refs {
            id(reference, "evidence_ref")?;
        }
        bounded_single_line(&self.master_state, "master_state", 64)
    }
}
