//! A bounded entry-parent index over the existing SessionStore journal.
//! Turn.parent_id remains the active model/tool turn correlation. This graph
//! links transcript entries independently and never owns a second writer.
use crate::{
    core::{Turn, TurnRole},
    session::{SessionError, SessionRecord},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const TREE_VERSION: u16 = 1;
pub const MAX_TREE_ENTRIES: usize = 16_384;
pub const MAX_TREE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_TREE_PAGE: usize = 128;
pub const MAX_TREE_PAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeLimits {
    pub max_entries: usize,
    pub max_depth: usize,
    pub max_bytes: usize,
}
impl Default for TreeLimits {
    fn default() -> Self {
        Self {
            max_entries: MAX_TREE_ENTRIES,
            max_depth: MAX_TREE_ENTRIES,
            max_bytes: MAX_TREE_BYTES,
        }
    }
}
impl TreeLimits {
    fn validate(self) -> Result<(), SessionError> {
        if self.max_entries == 0
            || self.max_entries > MAX_TREE_ENTRIES
            || self.max_depth == 0
            || self.max_depth > MAX_TREE_ENTRIES
            || self.max_bytes == 0
            || self.max_bytes > MAX_TREE_BYTES
        {
            return Err(invalid("invalid tree budget"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeEntry {
    pub schema_version: u16,
    pub session_id: String,
    pub id: String,
    pub parent_id: Option<String>,
    pub turn_id: String,
    pub sequence: u64,
    pub branch_id: String,
    pub depth: usize,
    pub role: TurnRole,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeAnnotation {
    pub name: Option<String>,
    pub summary: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeNode {
    pub entry: TreeEntry,
    pub children: usize,
    pub annotation: TreeAnnotation,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreePage {
    pub schema_version: u16,
    pub session_id: String,
    pub enabled: bool,
    pub active_leaf: Option<String>,
    pub branch_id: String,
    pub total: usize,
    pub nodes: Vec<TreeNode>,
    pub next_cursor: Option<usize>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TreeEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub schema_version: u16,
    pub session_id: String,
    pub action: TreeAction,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum TreeAction {
    Migrate {
        source_records: usize,
        source_sha256: String,
        leaf: Option<String>,
        limits: TreeLimits,
    },
    Select {
        previous_leaf: Option<String>,
        leaf: Option<String>,
    },
    Annotate {
        entry_id: String,
        annotation: TreeAnnotation,
    },
}
#[derive(Debug, Clone)]
pub struct SessionTree {
    session_id: String,
    enabled: bool,
    limits: TreeLimits,
    entries: BTreeMap<String, TreeEntry>,
    order: Vec<String>,
    by_turn: BTreeMap<String, String>,
    children: BTreeMap<Option<String>, usize>,
    annotations: BTreeMap<String, TreeAnnotation>,
    active_leaf: Option<String>,
    bytes: usize,
}
fn invalid(message: &str) -> SessionError {
    SessionError::InvalidRecord(message.into())
}
fn check_cancel(cancelled: &dyn Fn() -> bool) -> Result<(), SessionError> {
    if cancelled() {
        Err(invalid("tree operation cancelled"))
    } else {
        Ok(())
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= crate::protocol::MAX_ID_BYTES && !id.chars().any(char::is_control)
}
fn encoded_len<T: Serialize>(value: &T) -> Result<usize, SessionError> {
    Ok(serde_json::to_vec(value)?.len())
}
pub fn entry_id(session_id: &str, sequence: u64) -> String {
    format!(
        "entry-{:x}-{sequence}",
        Sha256::digest(session_id.as_bytes())
    )
}

pub(crate) fn records_digest(records: &[SessionRecord]) -> Result<String, SessionError> {
    let mut digest = Sha256::new();
    for record in records {
        digest.update(serde_json::to_vec(record)?);
        digest.update(b"\n");
    }
    Ok(format!("{:x}", digest.finalize()))
}
impl SessionTree {
    pub fn recover(
        session_id: &str,
        records: &[SessionRecord],
        limits: TreeLimits,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, SessionError> {
        limits.validate()?;
        if !valid_id(session_id) {
            return Err(invalid("invalid tree session identity"));
        }
        let mut tree = Self {
            session_id: session_id.into(),
            enabled: false,
            limits,
            entries: BTreeMap::new(),
            order: vec![],
            by_turn: BTreeMap::new(),
            children: BTreeMap::new(),
            annotations: BTreeMap::new(),
            active_leaf: None,
            bytes: 0,
        };
        for (index, record) in records.iter().enumerate() {
            check_cancel(cancelled)?;
            if record.kind == "turn" {
                let turn: Turn = serde_json::from_value(record.value["turn"].clone())?;
                let expected = tree.plan_turn(&turn, record.sequence)?;
                match record.value.get("tree_entry") {
                    Some(raw) if tree.enabled => {
                        let entry: TreeEntry = serde_json::from_value(raw.clone())?;
                        if entry != expected {
                            return Err(invalid(
                                "tree entry parent/identity/sequence conflicts with journal",
                            ));
                        }
                    }
                    Some(_) => return Err(invalid("tree entry precedes its migration marker")),
                    None if tree.enabled => {
                        return Err(invalid("tree-enabled turn omitted its entry-parent link"));
                    }
                    None => {}
                }
                tree.commit_turn(expected);
            } else if record.kind == "event" && record.value["event"]["type"] == "session_tree" {
                let event: TreeEvent = serde_json::from_value(record.value["event"].clone())?;
                tree.validate_event(&event)?;
                if let TreeAction::Migrate {
                    source_records,
                    source_sha256,
                    ..
                } = &event.action
                    && (*source_records != index
                        || *source_sha256 != records_digest(&records[..index])?)
                {
                    return Err(invalid("tree migration source changed"));
                }
                tree.commit_event(event);
            }
        }
        check_cancel(cancelled)?;
        Ok(tree)
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn limits(&self) -> TreeLimits {
        self.limits
    }
    pub fn active_leaf(&self) -> Option<&str> {
        self.active_leaf.as_deref()
    }
    pub fn branch_id(&self) -> &str {
        self.active_leaf
            .as_ref()
            .and_then(|id| self.entries.get(id))
            .map_or("linear", |entry| entry.branch_id.as_str())
    }
    pub fn entry(&self, id: &str) -> Option<&TreeEntry> {
        self.entries.get(id)
    }
    pub fn entry_for_turn(&self, id: &str) -> Option<&TreeEntry> {
        self.by_turn
            .get(id)
            .and_then(|entry| self.entries.get(entry))
    }
    pub fn annotation(&self, id: &str) -> TreeAnnotation {
        self.annotations.get(id).cloned().unwrap_or_default()
    }
    pub fn ancestry(
        &self,
        leaf: Option<&str>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<TreeEntry>, SessionError> {
        let mut path = vec![];
        let mut current = leaf;
        while let Some(id) = current {
            check_cancel(cancelled)?;
            if !valid_id(id) {
                return Err(invalid("invalid tree leaf identity"));
            }
            let entry = self
                .entries
                .get(id)
                .ok_or_else(|| invalid("tree leaf or parent not found in this session"))?;
            if path.len() >= self.limits.max_depth {
                return Err(invalid("tree ancestry exceeds depth budget"));
            }
            path.push(entry.clone());
            current = entry.parent_id.as_deref();
        }
        path.reverse();
        Ok(path)
    }
    pub fn page(&self, cursor: usize, limit: usize) -> Result<TreePage, SessionError> {
        if cursor > self.order.len() || limit == 0 || limit > MAX_TREE_PAGE {
            return Err(invalid("invalid tree page cursor or limit"));
        }
        let mut page = TreePage {
            schema_version: TREE_VERSION,
            session_id: self.session_id.clone(),
            enabled: self.enabled,
            active_leaf: self.active_leaf.clone(),
            branch_id: self.branch_id().into(),
            total: self.order.len(),
            nodes: vec![],
            next_cursor: None,
        };
        let mut next = cursor;
        for id in self.order.iter().skip(cursor).take(limit) {
            let node = TreeNode {
                entry: self.entries[id].clone(),
                children: self.children.get(&Some(id.clone())).copied().unwrap_or(0),
                annotation: self.annotation(id),
            };
            page.nodes.push(node);
            if encoded_len(&page)? > MAX_TREE_PAGE_BYTES - 64 {
                page.nodes.pop();
                break;
            }
            next += 1;
        }
        if next < self.order.len() {
            page.next_cursor = Some(next);
        }
        Ok(page)
    }
    fn contains_ancestor_turn(&self, turn_id: &str) -> bool {
        let mut current = self.active_leaf();
        for _ in 0..self.limits.max_depth {
            let Some(entry) = current.and_then(|id| self.entries.get(id)) else {
                return false;
            };
            if entry.turn_id == turn_id {
                return true;
            }
            current = entry.parent_id.as_deref();
        }
        false
    }
    pub(crate) fn plan_turn(&self, turn: &Turn, sequence: u64) -> Result<TreeEntry, SessionError> {
        turn.validate().map_err(|e| invalid(&e.to_string()))?;
        if self.entries.len() >= self.limits.max_entries || self.by_turn.contains_key(&turn.id) {
            return Err(invalid(
                "tree entry limit exceeded or duplicate turn identity",
            ));
        }
        // Once enabled, active-turn parents may only refer to this ancestry.
        // They remain correlation IDs, never entry-parent links.
        if self.enabled
            && let Some(parent) = &turn.parent_id
            && !self.contains_ancestor_turn(parent)
        {
            return Err(invalid("turn parent is outside selected tree ancestry"));
        }
        let id = entry_id(&self.session_id, sequence);
        if self.entries.contains_key(&id) {
            return Err(invalid("duplicate tree entry identity"));
        }
        let parent = self
            .active_leaf
            .as_ref()
            .and_then(|id| self.entries.get(id));
        let depth = parent.map_or(1, |entry| entry.depth + 1);
        if depth > self.limits.max_depth {
            return Err(invalid("tree depth budget exceeded"));
        }
        let branch_id = if self.children.get(&self.active_leaf).copied().unwrap_or(0) > 0 {
            format!("branch-{id}")
        } else {
            parent.map_or_else(|| "linear".into(), |entry| entry.branch_id.clone())
        };
        let entry = TreeEntry {
            schema_version: TREE_VERSION,
            session_id: self.session_id.clone(),
            id,
            parent_id: self.active_leaf.clone(),
            turn_id: turn.id.clone(),
            sequence,
            branch_id,
            depth,
            role: turn.role,
        };
        if self.bytes.saturating_add(encoded_len(&entry)?) > self.limits.max_bytes {
            return Err(invalid("tree index byte budget exceeded"));
        }
        Ok(entry)
    }
    pub(crate) fn commit_turn(&mut self, entry: TreeEntry) {
        self.bytes += serde_json::to_vec(&entry)
            .expect("validated tree entry")
            .len();
        *self.children.entry(entry.parent_id.clone()).or_default() += 1;
        self.by_turn.insert(entry.turn_id.clone(), entry.id.clone());
        self.order.push(entry.id.clone());
        self.active_leaf = Some(entry.id.clone());
        self.entries.insert(entry.id.clone(), entry);
    }
    pub(crate) fn migration(&self, records: &[SessionRecord]) -> Result<TreeEvent, SessionError> {
        let event = self.event(TreeAction::Migrate {
            source_records: records.len(),
            source_sha256: records_digest(records)?,
            leaf: self.active_leaf.clone(),
            limits: self.limits,
        });
        self.validate_event(&event)?;
        Ok(event)
    }
    pub(crate) fn selection(&self, leaf: Option<String>) -> Result<TreeEvent, SessionError> {
        let event = self.event(TreeAction::Select {
            previous_leaf: self.active_leaf.clone(),
            leaf,
        });
        self.validate_event(&event)?;
        Ok(event)
    }
    pub(crate) fn annotation_event(
        &self,
        id: String,
        annotation: TreeAnnotation,
    ) -> Result<TreeEvent, SessionError> {
        let event = self.event(TreeAction::Annotate {
            entry_id: id,
            annotation,
        });
        self.validate_event(&event)?;
        Ok(event)
    }
    fn event(&self, action: TreeAction) -> TreeEvent {
        TreeEvent {
            kind: "session_tree".into(),
            schema_version: TREE_VERSION,
            session_id: self.session_id.clone(),
            action,
        }
    }
    fn validate_event(&self, event: &TreeEvent) -> Result<(), SessionError> {
        if event.kind != "session_tree"
            || event.schema_version != TREE_VERSION
            || event.session_id != self.session_id
        {
            return Err(invalid("tree event version or session mismatch"));
        }
        match &event.action {
            TreeAction::Migrate { limits, leaf, .. } => {
                limits.validate()?;
                if self.enabled
                    || leaf != &self.active_leaf
                    || self.entries.len() > limits.max_entries
                    || self.bytes > limits.max_bytes
                    || self
                        .entries
                        .values()
                        .any(|entry| entry.depth > limits.max_depth)
                {
                    return Err(invalid("tree migration conflicts with source or budget"));
                }
            }
            TreeAction::Select {
                previous_leaf,
                leaf,
            } => {
                if !self.enabled || previous_leaf != &self.active_leaf {
                    return Err(invalid("stale tree selection"));
                }
                self.ancestry(leaf.as_deref(), &|| false)?;
            }
            TreeAction::Annotate {
                entry_id,
                annotation,
            } => {
                if !self.enabled || !self.entries.contains_key(entry_id) {
                    return Err(invalid("annotation target not found"));
                }
                if annotation.name.as_ref().is_some_and(|s| {
                    s.is_empty() || s.len() > 128 || s.chars().any(char::is_control)
                }) || annotation
                    .summary
                    .as_ref()
                    .is_some_and(|s| s.trim().is_empty() || s.len() > 4096 || s.contains('\0'))
                {
                    return Err(invalid("invalid tree name or summary"));
                }
                let old = self
                    .annotations
                    .get(entry_id)
                    .map(encoded_len)
                    .transpose()?
                    .unwrap_or(0);
                if self
                    .bytes
                    .saturating_sub(old)
                    .saturating_add(encoded_len(annotation)?)
                    > self.limits.max_bytes
                {
                    return Err(invalid("tree annotation byte budget exceeded"));
                }
            }
        }
        Ok(())
    }
    pub(crate) fn commit_event(&mut self, event: TreeEvent) {
        match event.action {
            TreeAction::Migrate { limits, .. } => {
                self.enabled = true;
                self.limits = limits;
            }
            TreeAction::Select { leaf, .. } => self.active_leaf = leaf,
            TreeAction::Annotate {
                entry_id,
                annotation,
            } => {
                if let Some(old) = self.annotations.insert(entry_id, annotation.clone()) {
                    self.bytes -= serde_json::to_vec(&old).expect("stored annotation").len();
                }
                self.bytes += serde_json::to_vec(&annotation)
                    .expect("validated annotation")
                    .len();
            }
        }
    }
}

/// A fork carries completed historical messages, never unresolved ownership
/// that could be mistaken for a fresh permission to retry a side effect.
pub(crate) fn validate_fork_turns(turns: &[Turn]) -> Result<(), SessionError> {
    let mut seen = std::collections::BTreeSet::new();
    let mut calls = BTreeMap::<(String, String), bool>::new();
    for turn in turns {
        if turn.parent_id.as_ref().is_some_and(|id| !seen.contains(id))
            || !seen.insert(turn.id.clone())
        {
            return Err(invalid(
                "fork ancestry has a missing or duplicate turn parent",
            ));
        }
        let Some(metadata) = &turn.metadata else {
            if turn.role == TurnRole::Tool {
                return Err(invalid("fork tool result omitted correlation"));
            }
            continue;
        };
        if let Some(raw) = metadata.get("tool_calls") {
            if turn.role != TurnRole::Assistant {
                return Err(invalid("fork tool calls require an assistant turn"));
            }
            let parent = turn
                .parent_id
                .as_ref()
                .ok_or_else(|| invalid("fork tool calls omitted active turn"))?;
            for call in raw
                .as_array()
                .ok_or_else(|| invalid("invalid fork tool call list"))?
            {
                let id = call["id"]
                    .as_str()
                    .filter(|id| valid_id(id))
                    .ok_or_else(|| invalid("invalid fork tool call identity"))?;
                if calls.insert((parent.clone(), id.into()), false).is_some() {
                    return Err(invalid("duplicate fork tool call"));
                }
            }
        }
        if turn.role == TurnRole::Tool {
            let parent = turn
                .parent_id
                .as_ref()
                .ok_or_else(|| invalid("fork result omitted active turn"))?;
            let id = metadata["tool_call_id"]
                .as_str()
                .ok_or_else(|| invalid("fork result omitted call identity"))?;
            let done = calls
                .get_mut(&(parent.clone(), id.into()))
                .ok_or_else(|| invalid("fork result has no call"))?;
            if *done
                || !matches!(
                    metadata["outcome"].as_str(),
                    Some(
                        "succeeded"
                            | "failed"
                            | "cancelled"
                            | "denied"
                            | "policy_denied"
                            | "rejected"
                    )
                )
            {
                return Err(invalid("fork cannot inherit an unknown tool outcome"));
            }
            *done = true;
        }
    }
    if calls.values().any(|done| !*done) {
        return Err(invalid("fork cannot inherit an unfinished tool call"));
    }
    Ok(())
}
