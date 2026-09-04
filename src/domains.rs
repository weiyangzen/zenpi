//! First-class, data-only b3ehive domain records.
//!
//! This module deliberately contains no scheduler, worker, or nested-agent
//! implementation.  A host can persist and route these records, while zenpi
//! itself only validates their bounded shape and (for blueprints) the DAG.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::b3::ResourceBudget;

/// Schema version for local domain records.  This is independent from the
/// wire/session schemas owned by the protocol and session modules.
pub const DOMAIN_SCHEMA_VERSION: u16 = 1;

/// All limits are intentionally conservative.  They keep a malformed local
/// record from turning validation or digest computation into an unbounded
/// allocation.
pub const MAX_DOMAIN_RECORD_BYTES: usize = 64 * 1024;
pub const MAX_ID_BYTES: usize = 128;
pub const MAX_VERSION_BYTES: usize = 64;
pub const MAX_TEXT_BYTES: usize = 16 * 1024;
pub const MAX_BLUEPRINT_ITEMS: usize = 256;
pub const MAX_DEPENDENCIES_PER_ITEM: usize = 256;
pub const MAX_LEARN_EVIDENCE: usize = 256;
pub const MAX_ESTIMATED_LOC_EXCLUSIVE: u32 = 5_000;

pub const MAX_GOAL_TOKENS: u64 = 1_000_000_000;
pub const MAX_GOAL_WALL_CLOCK_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
pub const MAX_GOAL_ATTEMPTS: u32 = 100_000;
pub const MAX_GOAL_DISK_BYTES: u64 = 1 << 40;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    #[error("{field} must be non-empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} bytes")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains a control character or line break")]
    InvalidText { field: &'static str },
    #[error("{field} is not a valid identifier")]
    InvalidId { field: &'static str },
    #[error("{field} is not a valid version")]
    InvalidVersion { field: &'static str },
    #[error("{field} has too many entries (maximum {max})")]
    TooMany { field: &'static str, max: usize },
    #[error("blueprint contains duplicate item `{id}`")]
    DuplicateItem { id: String },
    #[error("item `{item}` depends on missing item `{dependency}`")]
    MissingDependency { item: String, dependency: String },
    #[error("blueprint dependency cycle includes `{item}`")]
    DependencyCycle { item: String },
    #[error("item `{item}` has Estimated LOC {estimated}; it must be < {limit}")]
    EstimatedLocExceeded {
        item: String,
        estimated: u32,
        limit: u32,
    },
    #[error("digest is not a lowercase SHA-256 value")]
    InvalidDigest,
    #[error("domain digest does not match canonical content")]
    DigestMismatch,
    #[error("domain record exceeds {max} bytes")]
    RecordTooLong { max: usize },
    #[error("budget exceeds the bounded goal limit for {field}")]
    BudgetExceeded { field: &'static str },
    #[error("goal blueprint link does not match the supplied blueprint")]
    BlueprintLinkMismatch,
    #[error("invalid goal status transition from {from:?} to {to:?}")]
    InvalidStatusTransition { from: GoalStatus, to: GoalStatus },
}

fn bounded_text(value: &str, field: &'static str, max: usize) -> Result<(), DomainError> {
    if value.trim().is_empty() {
        return Err(DomainError::Empty { field });
    }
    if value.len() > max {
        return Err(DomainError::TooLong { field, max });
    }
    if value.chars().any(char::is_control) {
        return Err(DomainError::InvalidText { field });
    }
    Ok(())
}

fn bounded_id(value: &str, field: &'static str) -> Result<(), DomainError> {
    bounded_text(value, field, MAX_ID_BYTES)?;
    if value
        .chars()
        .any(|character| !(character.is_ascii_alphanumeric() || "._:/-".contains(character)))
    {
        return Err(DomainError::InvalidId { field });
    }
    Ok(())
}

fn bounded_version(value: &str, field: &'static str) -> Result<(), DomainError> {
    bounded_text(value, field, MAX_VERSION_BYTES)?;
    // A version is deliberately not parsed as semver: blueprint authors may
    // use a product or migration label, but whitespace and control bytes are
    // never accepted.
    if value.chars().any(char::is_whitespace) {
        return Err(DomainError::InvalidVersion { field });
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<(), DomainError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(DomainError::InvalidDigest);
    }
    Ok(())
}

fn digest_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn record_len<T: Serialize>(value: &T) -> Result<usize, DomainError> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|_| DomainError::RecordTooLong {
            max: MAX_DOMAIN_RECORD_BYTES,
        })
}

/// One independently estimated node in a Blueprint DAG.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BlueprintItem {
    pub id: String,
    #[serde(default, alias = "dependencies")]
    pub depends_on: Vec<String>,
    /// Strictly less than [`MAX_ESTIMATED_LOC_EXCLUSIVE`].
    pub estimated_loc: u32,
}

impl BlueprintItem {
    pub fn new(id: impl Into<String>, estimated_loc: u32) -> Self {
        Self {
            id: id.into(),
            depends_on: Vec::new(),
            estimated_loc,
        }
    }

    pub fn with_dependencies<I, S>(mut self, dependencies: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.depends_on = dependencies.into_iter().map(Into::into).collect();
        self
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        bounded_id(&self.id, "item_id")?;
        if self.estimated_loc >= MAX_ESTIMATED_LOC_EXCLUSIVE {
            return Err(DomainError::EstimatedLocExceeded {
                item: self.id.clone(),
                estimated: self.estimated_loc,
                limit: MAX_ESTIMATED_LOC_EXCLUSIVE,
            });
        }
        if self.depends_on.len() > MAX_DEPENDENCIES_PER_ITEM {
            return Err(DomainError::TooMany {
                field: "dependencies",
                max: MAX_DEPENDENCIES_PER_ITEM,
            });
        }
        let mut seen = BTreeMap::new();
        for dependency in &self.depends_on {
            bounded_id(dependency, "dependency_id")?;
            if seen.insert(dependency, ()).is_some() {
                return Err(DomainError::DependencyCycle {
                    item: self.id.clone(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct BlueprintUnsigned<'a> {
    id: &'a str,
    version: &'a str,
    items: &'a [BlueprintItem],
}

/// A versioned, content-addressed DAG of bounded work items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Blueprint {
    pub id: String,
    pub version: String,
    pub items: Vec<BlueprintItem>,
    pub digest: String,
}

impl Blueprint {
    pub fn new(
        id: impl Into<String>,
        version: impl Into<String>,
        items: Vec<BlueprintItem>,
    ) -> Result<Self, DomainError> {
        let mut blueprint = Self {
            id: id.into(),
            version: version.into(),
            items,
            digest: String::new(),
        };
        blueprint.validate_without_digest()?;
        blueprint.digest = blueprint.compute_digest()?;
        blueprint.validate()?;
        Ok(blueprint)
    }

    /// Validate a deserialized value and its content digest.
    pub fn from_wire(value: Self) -> Result<Self, DomainError> {
        value.validate()?;
        Ok(value)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, DomainError> {
        serde_json::to_vec(&BlueprintUnsigned {
            id: &self.id,
            version: &self.version,
            items: &self.items,
        })
        .map_err(|_| DomainError::RecordTooLong {
            max: MAX_DOMAIN_RECORD_BYTES,
        })
    }

    pub fn compute_digest(&self) -> Result<String, DomainError> {
        Ok(digest_hex(&self.canonical_bytes()?))
    }

    pub fn verify_digest(&self) -> Result<(), DomainError> {
        if self.digest == self.compute_digest()? {
            Ok(())
        } else {
            Err(DomainError::DigestMismatch)
        }
    }

    pub fn encode_json(&self) -> Result<String, DomainError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| DomainError::RecordTooLong {
            max: MAX_DOMAIN_RECORD_BYTES,
        })?;
        if bytes.len() > MAX_DOMAIN_RECORD_BYTES {
            return Err(DomainError::RecordTooLong {
                max: MAX_DOMAIN_RECORD_BYTES,
            });
        }
        String::from_utf8(bytes).map_err(|_| DomainError::RecordTooLong {
            max: MAX_DOMAIN_RECORD_BYTES,
        })
    }

    pub fn decode_json(input: &str) -> Result<Self, DomainError> {
        if input.len() > MAX_DOMAIN_RECORD_BYTES {
            return Err(DomainError::RecordTooLong {
                max: MAX_DOMAIN_RECORD_BYTES,
            });
        }
        let value = serde_json::from_str(input).map_err(|_| DomainError::InvalidText {
            field: "blueprint_json",
        })?;
        Self::from_wire(value)
    }

    pub fn contains_item(&self, id: &str) -> bool {
        self.items.iter().any(|item| item.id == id)
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        self.validate_without_digest()?;
        validate_digest(&self.digest)?;
        self.verify_digest()?;
        if record_len(self)? > MAX_DOMAIN_RECORD_BYTES {
            return Err(DomainError::RecordTooLong {
                max: MAX_DOMAIN_RECORD_BYTES,
            });
        }
        Ok(())
    }

    fn validate_without_digest(&self) -> Result<(), DomainError> {
        bounded_id(&self.id, "blueprint_id")?;
        bounded_version(&self.version, "blueprint_version")?;
        if self.items.is_empty() {
            return Err(DomainError::Empty { field: "items" });
        }
        if self.items.len() > MAX_BLUEPRINT_ITEMS {
            return Err(DomainError::TooMany {
                field: "items",
                max: MAX_BLUEPRINT_ITEMS,
            });
        }

        let mut indexes = BTreeMap::new();
        for (index, item) in self.items.iter().enumerate() {
            item.validate()?;
            if indexes.insert(item.id.as_str(), index).is_some() {
                return Err(DomainError::DuplicateItem {
                    id: item.id.clone(),
                });
            }
        }
        for item in &self.items {
            for dependency in &item.depends_on {
                if !indexes.contains_key(dependency.as_str()) {
                    return Err(DomainError::MissingDependency {
                        item: item.id.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
        }
        validate_acyclic(&self.items, &indexes)
    }
}

fn validate_acyclic(
    items: &[BlueprintItem],
    indexes: &BTreeMap<&str, usize>,
) -> Result<(), DomainError> {
    // 0 = unseen, 1 = currently visiting, 2 = complete.
    let mut marks = vec![0_u8; items.len()];
    for index in 0..items.len() {
        visit_item(index, items, indexes, &mut marks)?;
    }
    Ok(())
}

fn visit_item(
    index: usize,
    items: &[BlueprintItem],
    indexes: &BTreeMap<&str, usize>,
    marks: &mut [u8],
) -> Result<(), DomainError> {
    match marks[index] {
        1 => {
            return Err(DomainError::DependencyCycle {
                item: items[index].id.clone(),
            });
        }
        2 => return Ok(()),
        _ => {}
    }
    marks[index] = 1;
    for dependency in &items[index].depends_on {
        // Missing dependencies were checked by the caller.  Keep this lookup
        // defensive so the helper remains safe if reused in the future.
        let Some(&dependency_index) = indexes.get(dependency.as_str()) else {
            return Err(DomainError::MissingDependency {
                item: items[index].id.clone(),
                dependency: dependency.clone(),
            });
        };
        visit_item(dependency_index, items, indexes, marks)?;
    }
    marks[index] = 2;
    Ok(())
}

/// A bounded lease reference.  The host may resolve this ID to a richer
/// b3ehive lease; zenpi does not acquire or renew it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LeaseRef {
    pub lease_id: String,
    pub owner: String,
    pub expires_at_ms: u64,
}

pub type Lease = LeaseRef;

impl LeaseRef {
    pub fn new(
        lease_id: impl Into<String>,
        owner: impl Into<String>,
        expires_at_ms: u64,
    ) -> Result<Self, DomainError> {
        let lease = Self {
            lease_id: lease_id.into(),
            owner: owner.into(),
            expires_at_ms,
        };
        lease.validate()?;
        Ok(lease)
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        bounded_id(&self.lease_id, "lease_id")?;
        bounded_id(&self.owner, "lease_owner")
    }

    pub fn is_live_at(&self, now_ms: u64) -> bool {
        now_ms < self.expires_at_ms
    }
}

pub type GoalBudget = ResourceBudget;
pub type Budget = GoalBudget;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    #[default]
    Queued,
    Running,
    Paused,
    Blocked,
    Cancelled,
    Done,
}

impl GoalStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Cancelled | Self::Done)
    }

    /// Parse the stable, wire-level spelling used by owner commands.
    ///
    /// Keeping this conversion next to the enum prevents each host from
    /// inventing a subtly different status vocabulary.  The parser is
    /// deliberately ASCII case-insensitive, while serialization remains the
    /// canonical lower-case `snake_case` form provided by serde.
    pub fn parse_token(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("queued") {
            Some(Self::Queued)
        } else if value.eq_ignore_ascii_case("running") {
            Some(Self::Running)
        } else if value.eq_ignore_ascii_case("paused") {
            Some(Self::Paused)
        } else if value.eq_ignore_ascii_case("blocked") {
            Some(Self::Blocked)
        } else if value.eq_ignore_ascii_case("cancelled") || value.eq_ignore_ascii_case("canceled")
        {
            Some(Self::Cancelled)
        } else if value.eq_ignore_ascii_case("done") {
            Some(Self::Done)
        } else {
            None
        }
    }

    /// Return the canonical lower-case spelling without allocating.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
            Self::Done => "done",
        }
    }
}

/// A user-facing execution intent linked to one immutable Blueprint digest.
/// The lease is a reference only; scheduling remains the host's concern.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Goal {
    pub id: String,
    pub blueprint_id: String,
    pub blueprint_version: String,
    pub blueprint_digest: String,
    #[serde(default)]
    pub status: GoalStatus,
    pub budget: GoalBudget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<LeaseRef>,
}

impl Goal {
    pub fn new(
        id: impl Into<String>,
        blueprint: &Blueprint,
        budget: GoalBudget,
        lease: Option<LeaseRef>,
    ) -> Result<Self, DomainError> {
        blueprint.validate()?;
        let goal = Self {
            id: id.into(),
            blueprint_id: blueprint.id.clone(),
            blueprint_version: blueprint.version.clone(),
            blueprint_digest: blueprint.digest.clone(),
            status: GoalStatus::Queued,
            budget,
            lease,
        };
        goal.validate()?;
        Ok(goal)
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        bounded_id(&self.id, "goal_id")?;
        bounded_id(&self.blueprint_id, "blueprint_id")?;
        bounded_version(&self.blueprint_version, "blueprint_version")?;
        validate_digest(&self.blueprint_digest)?;
        validate_budget(self.budget)?;
        if let Some(lease) = &self.lease {
            lease.validate()?;
        }
        if record_len(self)? > MAX_DOMAIN_RECORD_BYTES {
            return Err(DomainError::RecordTooLong {
                max: MAX_DOMAIN_RECORD_BYTES,
            });
        }
        Ok(())
    }

    pub fn validate_against(&self, blueprint: &Blueprint) -> Result<(), DomainError> {
        self.validate()?;
        if self.blueprint_id != blueprint.id
            || self.blueprint_version != blueprint.version
            || self.blueprint_digest != blueprint.digest
        {
            return Err(DomainError::BlueprintLinkMismatch);
        }
        Ok(())
    }

    pub fn transition_to(&mut self, next: GoalStatus) -> Result<(), DomainError> {
        self.validate()?;
        let valid = match (self.status, next) {
            (
                GoalStatus::Queued,
                GoalStatus::Running | GoalStatus::Blocked | GoalStatus::Cancelled,
            )
            | (
                GoalStatus::Running,
                GoalStatus::Paused | GoalStatus::Blocked | GoalStatus::Cancelled | GoalStatus::Done,
            )
            | (GoalStatus::Paused, GoalStatus::Running | GoalStatus::Cancelled)
            | (
                GoalStatus::Blocked,
                GoalStatus::Queued | GoalStatus::Running | GoalStatus::Cancelled,
            ) => true,
            (from, to) if from == to => true,
            _ => false,
        };
        if !valid {
            return Err(DomainError::InvalidStatusTransition {
                from: self.status,
                to: next,
            });
        }
        self.status = next;
        Ok(())
    }
}

fn validate_budget(budget: GoalBudget) -> Result<(), DomainError> {
    if budget.tokens > MAX_GOAL_TOKENS {
        return Err(DomainError::BudgetExceeded { field: "tokens" });
    }
    if budget.wall_clock_ms > MAX_GOAL_WALL_CLOCK_MS {
        return Err(DomainError::BudgetExceeded {
            field: "wall_clock_ms",
        });
    }
    if budget.attempts > MAX_GOAL_ATTEMPTS {
        return Err(DomainError::BudgetExceeded { field: "attempts" });
    }
    if budget.disk_bytes > MAX_GOAL_DISK_BYTES {
        return Err(DomainError::BudgetExceeded {
            field: "disk_bytes",
        });
    }
    Ok(())
}

/// Evidence references are intentionally opaque and bounded.  They may point
/// to a repository artifact, a test receipt, or an external b3ehive handoff.
pub type EvidenceRef = String;
pub type LearnEvidence = EvidenceRef;

/// A source-to-target learning/transformation intent.  Evidence is a list of
/// references, not executable instructions, so this type cannot start a
/// worker or silently invoke another agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Learn {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
}

impl Learn {
    pub fn new(
        id: impl Into<String>,
        source: impl Into<String>,
        target: impl Into<String>,
        evidence: Vec<EvidenceRef>,
    ) -> Result<Self, DomainError> {
        let learn = Self {
            id: id.into(),
            source: source.into(),
            target: target.into(),
            evidence,
        };
        learn.validate()?;
        Ok(learn)
    }

    pub fn add_evidence(&mut self, evidence: impl Into<String>) -> Result<(), DomainError> {
        let evidence = evidence.into();
        // Validate before mutating the record so a rejected evidence
        // reference cannot leave an in-memory Learn entity invalid.
        bounded_text(&evidence, "evidence_ref", MAX_TEXT_BYTES)?;
        // Evidence submission is an idempotent owner operation.  A retry of
        // the same validated reference must not consume another bounded slot
        // (especially when the list is already at its limit).
        if self.evidence.iter().any(|existing| existing == &evidence) {
            return Ok(());
        }
        if self.evidence.len() >= MAX_LEARN_EVIDENCE {
            return Err(DomainError::TooMany {
                field: "evidence",
                max: MAX_LEARN_EVIDENCE,
            });
        }
        let previous_len = self.evidence.len();
        self.evidence.push(evidence);
        if let Err(error) = self.validate() {
            self.evidence.truncate(previous_len);
            return Err(error);
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        bounded_id(&self.id, "learn_id")?;
        bounded_text(&self.source, "source", MAX_TEXT_BYTES)?;
        bounded_text(&self.target, "target", MAX_TEXT_BYTES)?;
        if self.evidence.len() > MAX_LEARN_EVIDENCE {
            return Err(DomainError::TooMany {
                field: "evidence",
                max: MAX_LEARN_EVIDENCE,
            });
        }
        for reference in &self.evidence {
            bounded_text(reference, "evidence_ref", MAX_TEXT_BYTES)?;
        }
        if record_len(self)? > MAX_DOMAIN_RECORD_BYTES {
            return Err(DomainError::RecordTooLong {
                max: MAX_DOMAIN_RECORD_BYTES,
            });
        }
        Ok(())
    }
}
