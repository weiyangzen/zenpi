//! Interactive terminal mode.
//!
//! [`TuiState`] is deliberately independent from the agent.  This keeps
//! rendering and input handling testable with Ratatui's `TestBackend`, while
//! the runtime loop can connect any synchronous agent through a small
//! callback.  Ratatui keeps a previous cell buffer and emits only changed
//! cells; resize notifications are coalesced by [`RenderScheduler`] so a
//! resize drag or a burst of stream chunks does not cause a draw per event.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt::Display;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::domains::GoalStatus;
use crate::layout::{
    Breakpoint, FocusDirection, LayoutModel, LayoutSnapshot, PaneId, PaneRect, TabId, Visibility,
    arch_prompt_group, conversation_prompt_group,
};
use crate::slash::{self, BlueprintAction, InputRoute, LayoutAction, PaneAction, SlashCommand};
use crate::tool_runtime::{
    MasterSessionCommand, MasterSessionInputError, classify_master_session_input,
};

/// Bound retained transcript memory even when a provider streams forever.
pub const DEFAULT_MAX_MESSAGES: usize = 2_048;
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;
/// Project tabs are a durable UI checkpoint, not a transcript archive.
/// Keeping this bounded prevents a long-lived TUI from turning startup into
/// an unbounded JSON allocation.
pub const MAX_PROJECT_CHECKPOINT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_RENDER_LINES: usize = 8_192;
/// The Gantt pane is a compact projection, not another copy of the domain
/// store. Bound both retained text and rows so even the largest valid store is
/// cheap to clone and render on every frame.
pub const MAX_GANTT_PANE_BYTES: usize = 32 * 1024;
pub const MAX_GANTT_PANE_ROWS: usize = 96;
/// Keep provider events waiting for the renderer bounded by both count and
/// their serialized UTF-8 size.  A stream can contain a small number of very
/// large deltas, so a count-only mailbox is not a sufficient memory bound.
pub const MAX_TUI_PROVIDER_EVENTS: usize = 4_096;
pub const MAX_TUI_PROVIDER_BYTES: usize = 1024 * 1024;
pub const MAX_SESSION_PANE_RECORDS: usize = 32;
/// Maximum number of visual rows reserved for the editable prompt.  Longer
/// prompts remain editable; the input viewport scrolls to keep the cursor
/// visible instead of growing without bound and starving the transcript.
pub const MAX_INPUT_LINES: usize = 8;
/// Rows reserved for the resident discussion prompt inside the top-left
/// conversation group (ZS1-147). Keeping it fixed leaves the rest of the
/// column for the transcript.
pub const PROMPT_PANE_ROWS: u16 = 4;
/// Rows reserved for the resident arch prompt inside the lower-left arch group
/// (ZS1-148). The arch console is single-line oriented, so it needs fewer rows
/// than the discussion prompt while still sharing the left-column width.
pub const ARCH_PROMPT_PANE_ROWS: u16 = 3;
pub const DEFAULT_FRAME_INTERVAL: Duration = Duration::from_millis(16);
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// The compact htop/nvidia-smi style resource monitor refreshes on a 5 second
/// cadence. Collection still runs on the single-slot worker so it never
/// competes with interactive input.
pub const RESOURCE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const MIN_LOOP_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    Reasoning,
    Tool,
    System,
    Error,
}

impl MessageRole {
    fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Reasoning => "reasoning",
            Self::Tool => "tool",
            Self::System => "system",
            Self::Error => "error",
        }
    }
    fn style(self) -> Style {
        let color = match self {
            Self::User => Color::Cyan,
            Self::Assistant => Color::Green,
            Self::Reasoning => Color::DarkGray,
            Self::Tool => Color::Magenta,
            Self::System => Color::Blue,
            Self::Error => Color::Red,
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuiMessage {
    pub role: MessageRole,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<crate::view_model::ViewBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
}

/// Named per-project display styles accepted by `/project style`.
pub const PROJECT_STYLES: [&str; 7] = [
    "cyan", "green", "yellow", "magenta", "blue", "red", "white",
];

/// Layer-2 sub-tab kind within one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubTabKind {
    /// Reuses the layer-1 project workspace (default).
    Main,
    /// A dedicated `git worktree` for this project.
    Worktree,
    /// Explicit "work in the current place" choice (no new worktree).
    InPlace,
}

/// One layer-2 tab inside a project. Default reuses the layer-1 information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubTab {
    pub name: String,
    pub root: String,
    pub kind: SubTabKind,
    /// Default harness concurrency (parallel workers) for this workspace.
    #[serde(default = "default_subtab_concurrency")]
    pub concurrency: u16,
}

fn default_subtab_concurrency() -> u16 {
    1
}

impl SubTab {
    /// Every layer-2 open lands in an isolated workspace/worktree. The
    /// default `Main` tab intentionally reuses the layer-1 project root so it
    /// stays a zero-cost view of that project; each explicitly opened tab
    /// owns its own root instead of sharing the current workspace.
    pub fn is_isolated(&self) -> bool {
        !matches!(self.kind, SubTabKind::Main)
    }

    /// Canonical root for this tab's isolated workspace.
    pub fn workspace_root(&self) -> std::path::PathBuf {
        std::path::PathBuf::from(&self.root)
    }
}

/// Derive a layer-2 workspace root that cannot collide with the layer-1 root
/// or with any sibling tab, even when two tabs share the same display name.
fn isolated_subtab_root(
    base: &std::path::Path,
    tabs: &[SubTab],
    name: &str,
) -> std::path::PathBuf {
    let parent = base.join(".zenpi-workspaces");
    let mut candidate = parent.join(name);
    let mut suffix = 1u32;
    while candidate.as_path() == base || tabs.iter().any(|tab| tab.workspace_root() == candidate) {
        candidate = parent.join(format!("{name}-{suffix}"));
        suffix += 1;
    }
    candidate
}

/// A clickable region in the layer-2 tab row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubTabHit {
    Select(usize),
    AddWorktree,
    AddInPlace,
    ConcurrencyUp(usize),
    ConcurrencyDown(usize),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectTabMetadata {
    pub cwd: String,
    pub session_path: Option<String>,
    #[serde(default)]
    pub approval_mode: crate::approval::ApprovalMode,
    /// Optional per-project display style (a small named colour palette).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Folder source: `local:<path>` or `ssh:<spec>` for a remote project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ProjectSessionCursor {
    session_id: String,
    next_sequence: u64,
}

impl ProjectTabMetadata {
    pub fn from_session(session: &crate::session::SessionStore) -> Self {
        Self {
            cwd: session.header().cwd.clone(),
            session_path: Some(session.path().display().to_string()),
            approval_mode: crate::approval::ApprovalMode::Always,
            style: None,
            source: None,
        }
    }
}

impl Default for ProjectTabMetadata {
    fn default() -> Self {
        Self {
            cwd: String::new(),
            session_path: None,
            approval_mode: crate::approval::ApprovalMode::Always,
            style: None,
            source: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResourcePaneStatus {
    Idle,
    Collecting,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GanttPaneStatus {
    Idle,
    Refreshing,
    Ready,
    Failed(String),
}

/// Read-only journal evidence. A journal sequence is not a transport ACK or
/// a mailbox delivery receipt; those require their own durable owners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPaneSnapshot {
    session_id: String,
    journal_next_sequence: u64,
    active_leaf: Option<String>,
    turn_count: usize,
    warning_count: usize,
    timeline: Vec<String>,
    earlier_records: usize,
}

impl SessionPaneSnapshot {
    pub fn from_session(session: &crate::session::SessionStore) -> Self {
        let records = session.records();
        let start = records.len().saturating_sub(MAX_SESSION_PANE_RECORDS);
        let timeline = records[start..]
            .iter()
            .map(|record| {
                // Project envelope identity only. Never expose arbitrary event
                // payloads, shell output, or credentials in this side pane.
                let label = match record.kind.as_str() {
                    "event" => record
                        .value
                        .get("event")
                        .and_then(|value| value.get("type"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("event"),
                    kind => kind,
                };
                format!("{} {}", record.sequence, inline_token(label, 64))
            })
            .collect();
        Self {
            session_id: inline_token(session.session_id(), 128),
            journal_next_sequence: session.next_sequence(),
            active_leaf: session.active_tree_leaf().map(str::to_owned),
            turn_count: session.turns().len(),
            warning_count: session.recovery_warnings().len(),
            timeline,
            earlier_records: start,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn active_leaf(&self) -> Option<&str> {
        self.active_leaf.as_deref()
    }

    pub const fn journal_next_sequence(&self) -> u64 {
        self.journal_next_sequence
    }

    pub fn timeline(&self) -> &[String] {
        &self.timeline
    }

    pub const fn earlier_records(&self) -> usize {
        self.earlier_records
    }
}

/// Immutable, bounded projection consumed by the Gantt pane. Domain JSONL is
/// opened and validated off the terminal thread; rendering only reads this
/// already formatted snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GanttPaneSnapshot {
    content: String,
    blueprint_count: usize,
    goal_count: usize,
    truncated: bool,
}

#[derive(Debug, Clone)]
struct GanttRefreshRequest {
    session_path: std::path::PathBuf,
    generation: u64,
}

#[derive(Debug)]
struct GanttRefreshResult {
    snapshot: GanttPaneSnapshot,
    generation: u64,
}

#[derive(Debug, Clone, Default)]
struct GanttRefreshTracker {
    session_path: std::path::PathBuf,
    generation: u64,
}

impl GanttRefreshTracker {
    fn new(session_path: std::path::PathBuf) -> Self {
        Self {
            session_path,
            generation: 0,
        }
    }

    fn request(&self) -> GanttRefreshRequest {
        GanttRefreshRequest {
            session_path: self.session_path.clone(),
            generation: self.generation,
        }
    }

    /// Return true only when the active journal changed. Failed session-open
    /// commands pass the same path and therefore leave the current projection
    /// untouched.
    fn switch_session(&mut self, session_path: std::path::PathBuf) -> bool {
        if self.session_path == session_path {
            return false;
        }
        self.session_path = session_path;
        self.generation = self.generation.saturating_add(1);
        true
    }

    const fn accepts(&self, result: &GanttRefreshResult) -> bool {
        result.generation == self.generation
    }
}

impl GanttPaneSnapshot {
    pub fn content(&self) -> &str {
        &self.content
    }

    pub const fn blueprint_count(&self) -> usize {
        self.blueprint_count
    }

    pub const fn goal_count(&self) -> usize {
        self.goal_count
    }

    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

/// Read and validate the domain store associated with `session_path`, then
/// build the small projection displayed by the production Gantt pane. This is
/// public for host adapters and focused mock-terminal tests; callers should run
/// it outside their render/input thread because opening the snapshot performs
/// bounded filesystem I/O.
pub fn collect_gantt_snapshot(
    session_path: impl AsRef<std::path::Path>,
) -> Result<GanttPaneSnapshot, String> {
    let session_path = session_path.as_ref();
    let path = crate::domain_store::path_for_session(session_path);
    let store = crate::domain_store::DomainStore::open_read_only(path)
        .map_err(|error| format!("domain snapshot unavailable: {error}"))?;
    // The receipt store is deliberately read-only here. A Gantt refresh must
    // never create execution state merely because a user opened the pane;
    // `open_read_only` represents a missing store as an empty projection.
    let execution_path = crate::domain_execution::path_for_session(session_path);
    let execution = crate::domain_execution::ExecutionStore::open_read_only(execution_path)
        .map_err(|error| format!("execution snapshot unavailable: {error}"))?;
    Ok(project_gantt_store(&store, &execution))
}

fn project_gantt_store(
    store: &crate::domain_store::DomainStore,
    execution: &crate::domain_execution::ExecutionStore,
) -> GanttPaneSnapshot {
    use std::fmt::Write as _;

    let blueprints = store.blueprints();
    let goals = store.goals();
    let mut content = String::new();
    let mut rows = 0usize;
    let mut truncated = false;
    let _ = writeln!(
        content,
        "Blueprints {}  Goals {}  gen {}",
        blueprints.len(),
        goals.len(),
        store.generation()
    );
    rows = rows.saturating_add(1);

    if blueprints.is_empty() && goals.is_empty() {
        let _ = write!(content, "No persisted Blueprint or Goal");
        rows = rows.saturating_add(1);
    }

    for goal in &goals {
        let line = format!(
            "[{}] {} -> {}@{}\n",
            goal.status.as_str(),
            inline_token(&goal.id, crate::domains::MAX_ID_BYTES),
            inline_token(&goal.blueprint_id, crate::domains::MAX_ID_BYTES),
            inline_token(&goal.blueprint_version, crate::domains::MAX_VERSION_BYTES),
        );
        if !append_gantt_line(&mut content, &line, &mut rows) {
            truncated = true;
            break;
        }
    }

    if !truncated {
        for blueprint in &blueprints {
            let total_loc = blueprint.items.iter().fold(0_u64, |total, item| {
                total.saturating_add(u64::from(item.estimated_loc))
            });
            let linked_goals = goals
                .iter()
                .filter(|goal| {
                    goal.blueprint_id == blueprint.id && goal.blueprint_version == blueprint.version
                })
                .count();
            let header = format!(
                "{}@{}  items {}  est {} LOC  goals {}\n",
                inline_token(&blueprint.id, crate::domains::MAX_ID_BYTES),
                inline_token(&blueprint.version, crate::domains::MAX_VERSION_BYTES),
                blueprint.items.len(),
                total_loc,
                linked_goals,
            );
            if !append_gantt_line(&mut content, &header, &mut rows) {
                truncated = true;
                break;
            }
            for item in &blueprint.items {
                let dependency = if item.depends_on.is_empty() {
                    "ready".to_owned()
                } else {
                    format!("after {}", item.depends_on.len())
                };
                let execution_status =
                    latest_execution_status(execution, &goals, blueprint, &item.id).map_or_else(
                        || "pending".to_owned(),
                        |receipt| {
                            let mut value =
                                format!("{} attempt {}", receipt.status.as_str(), receipt.attempt);
                            if receipt.external_work_executed {
                                value.push_str(" external");
                            }
                            if let Some(error) = receipt.error.as_deref() {
                                value.push_str(" error ");
                                value.push_str(&inline_token(error, 96));
                            }
                            value
                        },
                    );
                let line = format!(
                    "  - {}  {}  {} LOC  status {}\n",
                    inline_token(&item.id, crate::domains::MAX_ID_BYTES),
                    dependency,
                    item.estimated_loc,
                    execution_status,
                );
                if !append_gantt_line(&mut content, &line, &mut rows) {
                    truncated = true;
                    break;
                }
            }
            if truncated {
                break;
            }
        }
    }

    if truncated {
        append_gantt_marker(&mut content);
    }
    while content.ends_with('\n') {
        content.pop();
    }
    GanttPaneSnapshot {
        content,
        blueprint_count: blueprints.len(),
        goal_count: goals.len(),
        truncated,
    }
}

/// Return the newest receipt for an item across goals linked to this exact
/// Blueprint version/digest. Receipts are goal-scoped, so unrelated goals or
/// stale Blueprint versions must never make an item look complete. When more
/// than one linked goal has the same attempt number, the stable goal order
/// from the domain store wins; the compact pane intentionally shows one
/// bounded status rather than duplicating rows for every goal.
fn latest_execution_status<'a>(
    execution: &'a crate::domain_execution::ExecutionStore,
    goals: &[&crate::domains::Goal],
    blueprint: &crate::domains::Blueprint,
    item_id: &str,
) -> Option<&'a crate::domain_execution::ExecutionReceipt> {
    let mut latest: Option<&'a crate::domain_execution::ExecutionReceipt> = None;
    for goal in goals.iter().filter(|goal| {
        goal.blueprint_id == blueprint.id
            && goal.blueprint_version == blueprint.version
            && goal.blueprint_digest == blueprint.digest
    }) {
        let Some(receipt) = execution.latest_receipt_for(goal, blueprint, item_id) else {
            continue;
        };
        if latest.is_none_or(|previous| receipt.attempt > previous.attempt) {
            latest = Some(receipt);
        }
    }
    latest
}

fn append_gantt_line(content: &mut String, line: &str, rows: &mut usize) -> bool {
    // Reserve the final logical row for a visible truncation marker.
    if *rows >= MAX_GANTT_PANE_ROWS.saturating_sub(1)
        || content.len().saturating_add(line.len()) > MAX_GANTT_PANE_BYTES
    {
        return false;
    }
    content.push_str(line);
    *rows = rows.saturating_add(1);
    true
}

fn append_gantt_marker(content: &mut String) {
    const MARKER: &str = "... Gantt projection truncated";
    let separator_bytes = usize::from(!content.ends_with('\n'));
    let keep = MAX_GANTT_PANE_BYTES.saturating_sub(separator_bytes + MARKER.len());
    if content.len() > keep {
        content.truncate(truncate_bytes(content, keep).len());
    }
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(MARKER);
}

impl TuiMessage {
    pub fn new(role: MessageRole, text: impl Into<String>) -> Self {
        Self {
            role,
            text: bound_text(text.into()),
            blocks: Vec::new(),
            block_id: None,
        }
    }
}

/// The two left-column prompt hot zones that can own keyboard focus (ZS1-148).
///
/// `Discussion` is the resident top-left Conversation prompt (ZS1-147).
/// `Arch` is the lower-left arch console that belongs to the master session and
/// can execute bash/steering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LeftPrompt {
    #[default]
    Discussion,
    Arch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiAction {
    None,
    Submit(String),
    /// A submission from the lower-left Arch master-session console (ZS1-148).
    /// The typed command is already classified so any host can execute it
    /// without re-parsing the operator's text.
    SubmitArch(MasterSessionCommand),
    RespondApproval {
        project: String,
        request_id: String,
        turn_id: String,
        call_id: String,
        allow: bool,
        remember: bool,
    },
    OpenSession(String),
    InspectResource {
        command: String,
        source_hash: String,
    },
    Copy(String),
    OpenExternalEditor,
    Quit,
    Interrupt,
    Redraw,
}

/// Lifecycle state shown for a provider-requested tool call.
///
/// The TUI deliberately keeps this as a small value enum rather than carrying
/// provider-specific payloads.  That makes status updates cheap and lets the
/// same view work for headless-driven embedders that forward only normalized
/// [`crate::core::AgentEvent`] values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRunStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl ToolRunStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "ok",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCompletionQuery {
    root: std::path::PathBuf,
    input: String,
    prefix: String,
    path: String,
    raw_selection: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCompletion {
    input: String,
    source: String,
    directory: bool,
}

/// A capped scan cannot establish that unvisited files are absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCompletionScan {
    Complete,
    EntryLimit,
    TimeLimit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCompletionResult {
    pub entries: Vec<FileCompletion>,
    pub scan: FileCompletionScan,
    pub display_limited: bool,
}

impl FileCompletionResult {
    fn notice(&self) -> Option<(&'static str, &'static str)> {
        let status = match (self.scan, self.display_limited) {
            (FileCompletionScan::EntryLimit, true) => {
                "Partial results · scan and display limits reached"
            }
            (FileCompletionScan::TimeLimit, true) => {
                "Partial results · time and display limits reached"
            }
            (FileCompletionScan::EntryLimit, false) => {
                "Partial results · directory scan limit reached"
            }
            (FileCompletionScan::TimeLimit, false) => "Partial results · search time limit reached",
            (FileCompletionScan::Complete, true) => "Partial results · 128-match display limit",
            (FileCompletionScan::Complete, false) if self.entries.is_empty() => {
                return Some((
                    "No matching files in this directory",
                    "Check the path or filename prefix.",
                ));
            }
            (FileCompletionScan::Complete, false) => return None,
        };
        Some((
            status,
            if self.entries.is_empty() {
                "No matches in scanned entries. Narrow the path or filename prefix."
            } else {
                "Narrow the path or filename prefix."
            },
        ))
    }
}

fn quote_input_path(path: &str) -> String {
    format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Return explicit @ tokens only at word boundaries; skill arguments remain literal.
fn prompt_file_references(text: &str) -> Result<Vec<(usize, usize, String, bool)>, String> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let c = text[i..].chars().next().unwrap();
        if c != '@'
            || (i > 0
                && !text[..i]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace))
        {
            i += c.len_utf8();
            continue;
        }
        let start = i;
        i += 1;
        let quoted = text[i..].starts_with('"');
        if quoted {
            i += 1;
        }
        let mut path = String::new();
        let mut closed = !quoted;
        while i < text.len() {
            let c = text[i..].chars().next().unwrap();
            if quoted && c == '"' {
                i += 1;
                closed = true;
                break;
            }
            if !quoted && c.is_whitespace() {
                break;
            }
            if quoted && c == '\\' {
                i += 1;
                if i >= text.len() {
                    break;
                }
                let next = text[i..].chars().next().unwrap();
                path.push(next);
                i += next.len_utf8();
            } else {
                path.push(c);
                i += c.len_utf8();
            }
            if path.len() > 4096 {
                return Err("File reference exceeds 4096 bytes".into());
            }
        }
        result.push((start, i, path, closed));
        if result.len() > crate::backend::MAX_ATTACHMENTS_PER_TURN {
            return Err("Too many file references".into());
        }
    }
    Ok(result)
}

/// Directory discovery is bounded and runs in a cancellable UI worker, never the input loop.
pub fn complete_file_query(
    query: &FileCompletionQuery,
    cancelled: impl Fn() -> bool,
) -> Result<FileCompletionResult, String> {
    complete_file_query_with_clock(query, cancelled, |start| start.elapsed())
}

fn complete_file_query_with_clock(
    query: &FileCompletionQuery,
    cancelled: impl Fn() -> bool,
    elapsed: impl Fn(Instant) -> Duration,
) -> Result<FileCompletionResult, String> {
    use std::path::{Component, Path};
    if query.path.len() > 4096 || query.path.chars().any(char::is_control) {
        return Err("Invalid completion path".into());
    }
    let relative = Path::new(&query.path);
    if relative.is_absolute()
        || relative.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("File completion stays inside the selected project".into());
    }
    let (parent, needle) = query
        .path
        .rsplit_once('/')
        .map_or(("", query.path.as_str()), |(p, n)| (p, n));
    let mut directory = query.root.clone();
    for component in Path::new(parent).components() {
        if let Component::Normal(part) = component {
            directory.push(part);
            let meta = std::fs::symlink_metadata(&directory).map_err(|e| e.to_string())?;
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err("Completion directory must be a real project directory".into());
            }
        }
    }
    if cancelled() {
        return Err("File completion cancelled".into());
    }
    let start = Instant::now();
    let mut entries = Vec::new();
    let mut scanned = 0;
    let mut scan = FileCompletionScan::Complete;
    for entry in std::fs::read_dir(&directory)
        .map_err(|e| e.to_string())?
        .take(2048)
    {
        if cancelled() {
            return Err("File completion cancelled".into());
        }
        if elapsed(start) > Duration::from_millis(150) {
            scan = FileCompletionScan::TimeLimit;
            break;
        }
        scanned += 1;
        let entry = entry.map_err(|e| e.to_string())?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if !name.starts_with(needle) || name.chars().any(char::is_control) {
            continue;
        }
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() || !(kind.is_file() || kind.is_dir()) {
            continue;
        }
        let path = format!(
            "{}{}{}{}",
            parent,
            if parent.is_empty() { "" } else { "/" },
            name,
            if kind.is_dir() { "/" } else { "" }
        );
        let token = if query.raw_selection {
            path.clone()
        } else {
            quote_input_path(&path)
        };
        entries.push(FileCompletion {
            input: format!("{}{}", query.prefix, token),
            source: query.root.join(&path).display().to_string(),
            directory: kind.is_dir(),
        });
    }
    if cancelled() {
        return Err("File completion cancelled".into());
    }
    // No extra read past the cap: at exactly2048 we cannot prove exhaustion.
    if scan == FileCompletionScan::Complete && scanned == 2048 {
        scan = FileCompletionScan::EntryLimit;
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.input.cmp(&b.input))
    });
    let display_limited = entries.len() > 128;
    entries.truncate(128);
    Ok(FileCompletionResult {
        entries,
        scan,
        display_limited,
    })
}

/// Immutable metadata copied from the current project's resource owner.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandResource {
    command: String,
    description: String,
    source: String,
    hash: String,
}

const MAX_CLIPBOARD_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
struct TranscriptRow {
    line: Line<'static>,
    position: Option<(u64, usize)>,
}

#[derive(Debug, Clone)]
struct TranscriptUx {
    reasoning_folded: bool,
    reasoning_job: Option<u64>,
    scroll_anchor: Option<usize>,
    anchored_position: Option<(u64, usize)>,
    message_offset: u64,
    last_start: usize,
    visible_rows: usize,
    model: Option<String>,
    response_model: Option<String>,
    usage: Option<crate::backend::Usage>,
    history_tokens: Option<u64>,
    history_revision: Option<(String, u64, bool)>,
    context_limit: Option<u64>,
    elapsed: Duration,
    started: Option<Instant>,
    // Canonical approvals supplement coordinator-backed ApprovalView requests.
    // Both travel with this project's draft; status text and modal focus do not.
    canonical_approvals: Vec<String>,
    approval_tracking_saturated: bool,
    activity_ended: bool,
    copy_notice: Option<(String, Instant)>,
}
impl Default for TranscriptUx {
    fn default() -> Self {
        Self {
            reasoning_folded: true,
            reasoning_job: None,
            scroll_anchor: None,
            anchored_position: None,
            message_offset: 0,
            last_start: 0,
            visible_rows: 0,
            model: None,
            response_model: None,
            usage: None,
            history_tokens: None,
            history_revision: None,
            context_limit: None,
            elapsed: Duration::ZERO,
            started: None,
            canonical_approvals: Vec::new(),
            approval_tracking_saturated: false,
            activity_ended: false,
            copy_notice: None,
        }
    }
}
impl TranscriptUx {
    fn pause(&mut self) {
        if let Some(start) = self.started.take() {
            self.elapsed += start.elapsed();
        }
    }
    fn seconds(&self) -> u64 {
        (self.elapsed + self.started.map(|t| t.elapsed()).unwrap_or_default()).as_secs()
    }
}
#[derive(Debug, Clone)]
struct CopyEntry {
    label: String,
    text: String,
}
#[derive(Debug, Clone)]
struct TranscriptBrowser {
    entries: Vec<CopyEntry>,
    selected: usize,
    scroll: u16,
    area: Rect,
}
impl TranscriptBrowser {
    fn new(messages: &VecDeque<TuiMessage>) -> Self {
        let mut entries = Vec::new();
        let mut retained = 0usize;
        for message in messages.iter().rev().take(64) {
            if retained.saturating_add(message.text.len()) > MAX_TUI_PROVIDER_BYTES {
                break;
            }
            retained += message.text.len();
            entries.push(CopyEntry {
                label: format!(
                    "{} · {}",
                    message.role.label(),
                    inline_token(&message.text, 60)
                ),
                text: message.text.clone(),
            });
            for (i, block) in message.blocks.iter().enumerate().take(32) {
                let text = view_block_text(block);
                if retained.saturating_add(text.len()) > MAX_TUI_PROVIDER_BYTES {
                    break;
                }
                retained += text.len();
                entries.push(CopyEntry {
                    label: format!("{} block {}", message.role.label(), i + 1),
                    text,
                });
            }
            if message.role == MessageRole::Assistant {
                for block in crate::render::parse_markdown(&message.text) {
                    if let crate::render::MarkdownBlock::Code { language, text } = block {
                        if retained.saturating_add(text.len()) > MAX_TUI_PROVIDER_BYTES {
                            break;
                        }
                        retained += text.len();
                        entries.push(CopyEntry {
                            label: format!("code · {}", language.unwrap_or_default()),
                            text,
                        });
                    }
                    if entries.len() >= 128 {
                        break;
                    }
                }
            }
            if entries.len() >= 128 {
                entries.truncate(128);
                break;
            }
        }
        Self {
            entries,
            selected: 0,
            scroll: 0,
            area: Rect::default(),
        }
    }
    fn key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        match key.code {
            KeyCode::Esc => Some(TuiAction::Redraw),
            KeyCode::Enter => self
                .entries
                .get(self.selected)
                .map(|e| TuiAction::Copy(e.text.clone())),
            KeyCode::Up | KeyCode::BackTab => {
                self.selected = self.selected.saturating_sub(1);
                self.scroll = 0;
                None
            }
            KeyCode::Down | KeyCode::Tab => {
                self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1));
                self.scroll = 0;
                None
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(8);
                None
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(8);
                None
            }
            KeyCode::Home => {
                self.scroll = 0;
                None
            }
            _ => None,
        }
    }
    fn render(&mut self, frame: &mut Frame<'_>) {
        let all = frame.area();
        let width = all.width.saturating_sub(4);
        let height = all.height.saturating_sub(2);
        self.area = Rect::new(
            all.x + (all.width - width) / 2,
            all.y + (all.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, self.area);
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .title(" Inspect / copy transcript blocks "),
            self.area,
        );
        if width < 6 || height < 6 {
            return;
        }
        let inner = Rect::new(self.area.x + 1, self.area.y + 1, width - 2, height - 2);
        let label = self
            .entries
            .get(self.selected)
            .map(|e| format!("{}/{} {}", self.selected + 1, self.entries.len(), e.label))
            .unwrap_or_else(|| "No messages yet".into());
        frame.render_widget(
            Paragraph::new(label),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        frame.render_widget(
            Paragraph::new("[Prev] [Next] [Copy] [Close] · Up/Down select · PgUp/PgDn read"),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
        let text = self
            .entries
            .get(self.selected)
            .map(|e| e.text.as_str())
            .unwrap_or_default();
        let lines = crate::render::render_plain_prefixed(
            "",
            text,
            usize::from(inner.width),
            Style::default(),
        );
        self.scroll = self.scroll.min(
            lines
                .len()
                .saturating_sub(usize::from(inner.height - 2))
                .min(u16::MAX as usize) as u16,
        );
        frame.render_widget(
            Paragraph::new(lines).scroll((self.scroll, 0)),
            Rect::new(inner.x, inner.y + 2, inner.width, inner.height - 2),
        );
    }
}

#[derive(Debug, Clone)]
pub enum ProjectIntent {
    Open(std::path::PathBuf),
    Select(usize),
    Close(String),
}

/// Project-scoped view of real coordinator requests; never an approval owner.
#[derive(Debug, Clone, Default)]
struct ApprovalView {
    requests: Vec<crate::approval::ApprovalRequest>,
    focused: bool,
    index: usize,
    scroll: usize,
    allow: bool,
    remember: bool,
}

impl TuiState {
    fn same_approval_request(
        left: &crate::approval::ApprovalRequest,
        right: &crate::approval::ApprovalRequest,
    ) -> bool {
        left.request_id == right.request_id
            && left.turn_id == right.turn_id
            && left.call_id == right.call_id
    }

    pub fn present_approval(&mut self, request: crate::approval::ApprovalRequest) {
        if let Err(error) = request.validate() {
            self.push_message(MessageRole::Error, format!("Invalid approval: {error}"));
            return;
        }
        let can_focus = self.directory_picker.is_none() && self.transcript_browser.is_none();
        let project = self.active_project().to_owned();
        let view = self.approval_views.entry(project).or_default();
        if view
            .requests
            .iter()
            .any(|r| Self::same_approval_request(r, &request))
        {
            return;
        }
        if view.requests.len() < 128 {
            let first = view.requests.is_empty();
            view.requests.push(request);
            if first {
                view.focused = can_focus;
                view.index = 0;
                view.scroll = 0;
                view.allow = false;
                view.remember = false;
            }
            self.dirty = true;
            self.sync_activity_timer();
        } else {
            // Keep the existing request cap without timing an unseen approval
            // as active work. Only a lifecycle clear can disambiguate overflow.
            self.transcript_ux.approval_tracking_saturated = true;
            self.sync_activity_timer();
        }
    }
    pub fn approval_count(&self) -> usize {
        self.approval_views
            .get(self.active_project())
            .map_or(0, |v| v.requests.len())
    }
    fn project_approval_count(&self, index: usize) -> usize {
        self.approval_views
            .get(&self.project_tabs[index])
            .map_or(0, |view| view.requests.len())
    }
    fn background_approval_hint(&self) -> Option<String> {
        let pending: Vec<_> = (0..self.project_tabs.len())
            .filter(|index| *index != self.active_project)
            .filter_map(|index| {
                let count = self.project_approval_count(index);
                (count > 0).then_some((index, count))
            })
            .collect();
        if pending.is_empty() {
            return None;
        }
        let mut names = pending
            .iter()
            .take(3)
            .map(|(index, count)| {
                format!(
                    "{} ({count})",
                    // Duplicate directory titles include parent + short ID;
                    // keep that existing disambiguation in the notice too.
                    truncate_to_width(&self.project_label(*index), 28)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        if pending.len() > 3 {
            names.push_str(&format!(" +{} projects", pending.len() - 3));
        }
        Some(format!(
            " Approval waiting: {names} · switch tab, Alt-A review"
        ))
    }
    pub fn retire_approval(&mut self, id: &str) {
        let project = self.active_project().to_owned();
        if let Some(view) = self.approval_views.get_mut(&project) {
            let previous = view.requests.len();
            view.requests.retain(|r| r.request_id != id);
            if view.requests.len() == previous {
                return;
            }
            view.index = view.index.min(view.requests.len().saturating_sub(1));
            view.allow = false;
            view.remember = false;
            view.scroll = 0;
            if view.requests.is_empty() {
                view.focused = false;
            }
            self.dirty = true;
            // A real coordinator retirement also retires its canonical mirror.
            self.transcript_ux
                .canonical_approvals
                .retain(|pending| pending != id);
            self.sync_activity_timer();
        }
    }

    fn retire_approval_request(&mut self, request: &crate::approval::ApprovalRequest) {
        let project = self.active_project().to_owned();
        if let Some(view) = self.approval_views.get_mut(&project) {
            let previous = view.requests.len();
            view.requests
                .retain(|pending| !Self::same_approval_request(pending, request));
            if view.requests.len() == previous {
                return;
            }
            view.index = view.index.min(view.requests.len().saturating_sub(1));
            view.allow = false;
            view.remember = false;
            view.scroll = 0;
            if view.requests.is_empty() {
                view.focused = false;
            }
            self.dirty = true;
            self.transcript_ux
                .canonical_approvals
                .retain(|pending| pending != &request.request_id);
            self.sync_activity_timer();
        }
    }

    fn sync_activity_timer(&mut self) {
        let waiting = self.approval_count() > 0
            || !self.transcript_ux.canonical_approvals.is_empty()
            || self.transcript_ux.approval_tracking_saturated;
        if !self.busy || self.transcript_ux.activity_ended || waiting {
            self.transcript_ux.pause();
        } else if self.transcript_ux.started.is_none() {
            self.transcript_ux.started = Some(Instant::now());
        }
    }

    // Clear waiting on cancellation admission, terminal, or job replacement.
    // A cancellation request clears waiting, but work may still be winding down.
    fn clear_activity_approvals(&mut self) {
        self.approval_views
            .remove(&self.project_tabs[self.active_project]);
        self.transcript_ux.canonical_approvals.clear();
        self.transcript_ux.approval_tracking_saturated = false;
        self.sync_activity_timer();
        self.dirty = true;
    }

    fn end_activity_timer(&mut self) {
        self.transcript_ux.activity_ended = true;
        self.clear_activity_approvals();
    }

    fn approval_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        if self.directory_picker.is_some() {
            return None;
        }
        let project = self.active_project().to_owned();
        let view = self.approval_views.get_mut(&project)?;
        if view.requests.is_empty() {
            return None;
        }
        if key.modifiers == KeyModifiers::ALT && key.code == KeyCode::Char('a') {
            view.focused = true;
            self.dirty = true;
            return Some(TuiAction::Redraw);
        }
        if !view.focused {
            return None;
        }
        self.dirty = true;
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
            return Some(TuiAction::Interrupt);
        }
        if key.modifiers != KeyModifiers::NONE {
            return Some(TuiAction::None);
        }
        match key.code {
            KeyCode::Esc => {
                view.focused = false;
                self.set_status("Approval pending · Alt-A opens review · draft kept");
            }
            KeyCode::Char('/') => {
                view.focused = false;
                return None;
            }
            KeyCode::Char('y') => {
                view.allow = true;
                view.remember = false;
            }
            KeyCode::Char('n') => {
                view.allow = false;
                view.remember = false;
            }
            KeyCode::Char('r')
                if view.requests[view.index].origin
                    != crate::tools::ToolOrigin::BlueprintWorker =>
            {
                view.remember = !view.remember;
            }
            KeyCode::Tab | KeyCode::Right => {
                view.index = (view.index + 1) % view.requests.len();
                view.scroll = 0;
                view.allow = false;
                view.remember = false;
            }
            KeyCode::BackTab | KeyCode::Left => {
                view.index = (view.index + view.requests.len() - 1) % view.requests.len();
                view.scroll = 0;
                view.allow = false;
                view.remember = false;
            }
            KeyCode::Up => view.scroll = view.scroll.saturating_sub(1),
            KeyCode::Down => view.scroll = view.scroll.saturating_add(1),
            KeyCode::PageUp => view.scroll = view.scroll.saturating_sub(10),
            KeyCode::PageDown => view.scroll = view.scroll.saturating_add(10),
            KeyCode::Home => view.scroll = 0,
            KeyCode::End => view.scroll = usize::MAX,
            KeyCode::Enter => {
                let r = &view.requests[view.index];
                return Some(TuiAction::RespondApproval {
                    project,
                    request_id: r.request_id.clone(),
                    turn_id: r.turn_id.clone(),
                    call_id: r.call_id.clone(),
                    allow: view.allow,
                    remember: view.remember,
                });
            }
            _ => {}
        }
        Some(TuiAction::Redraw)
    }
    fn render_approval(&mut self, frame: &mut Frame<'_>) {
        let project = self.active_project().to_owned();
        let Some(view) = self.approval_views.get_mut(&project) else {
            return;
        };
        if view.requests.is_empty() {
            return;
        }
        let area = frame.area();
        if !view.focused {
            // The footer owns the pending hint and its priority relative to
            // save failures; drawing it here would overwrite existing text.
            return;
        }
        if area.width < 4 || area.height < 5 {
            return;
        }
        let modal = Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2);
        frame.render_widget(Clear, modal);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " Approval required {}/{} · {} ",
                view.index + 1,
                view.requests.len(),
                if view.allow { "ALLOW" } else { "DENY" }
            ))
            .border_style(Style::default().fg(Color::Yellow));
        let inner = block.inner(modal);
        frame.render_widget(block, modal);
        let r = &view.requests[view.index];
        let mut text = format!(
            "Project: {project}\nRequest: {}\nTurn: {} · Call: {}\nTool: {} · {:?}\nPolicy: {} · Lease: {}\n",
            r.request_id,
            r.turn_id,
            r.call_id,
            r.tool,
            r.side_effect,
            r.policy_digest.as_deref().unwrap_or("unbound"),
            r.lease_id.as_deref().unwrap_or("none"),
        );
        let owner_truncated = if let Some(crate::tools::ToolPreview::Diff {
            path,
            before_bytes,
            after_bytes,
            truncated,
            patch,
            ..
        }) = &r.preview
        {
            // The owner diff already contains the content changes. Repeating
            // full write/edit arguments delays the actual approval evidence.
            text.push_str(&format!(
                "Path: {path}\nSize: {before_bytes} -> {after_bytes} bytes\n\nProposed change:\n"
            ));
            text.push_str(patch);
            *truncated
        } else {
            text.push_str(&format!(
                "Arguments: {}\n",
                crate::security::redact_json(&r.arguments, &[])
            ));
            false
        };
        if owner_truncated {
            text.insert_str(
                0,
                "Preview truncated by owner; inspect source before allowing.\n\n",
            );
        }
        if text.len() > crate::render::MAX_MARKDOWN_BYTES {
            text.insert_str(
                0,
                "Display truncated; inspect full arguments before allowing.\n\n",
            );
        }
        let height = inner.height.saturating_sub(2);
        let window = crate::render::render_plain_window(
            &text,
            usize::from(inner.width),
            view.scroll,
            usize::from(height),
        );
        view.scroll = window.offset;
        let incomplete = owner_truncated || window.input_truncated;
        frame.render_widget(
            Paragraph::new(window.lines),
            Rect::new(inner.x, inner.y, inner.width, height),
        );
        let remember = if r.origin == crate::tools::ToolOrigin::BlueprintWorker {
            "Worker: once only".to_owned()
        } else {
            format!("r remember: {}", view.remember)
        };
        let decision_hint = if incomplete {
            "INCOMPLETE preview · inspect source · n deny".to_owned()
        } else {
            format!("y allow · n deny · Enter confirm · {remember}")
        };
        frame.render_widget(
            Paragraph::new(format!(
                "{decision_hint}\nTab next · PgUp/PgDn scroll · Esc draft · Ctrl-C cancel"
            ))
            .style(Style::default().fg(Color::Yellow)),
            Rect::new(
                inner.x,
                inner.y + height,
                inner.width,
                inner.height - height,
            ),
        );
    }
}

/// Exact host correlation; coordinator remains the atomic decision authority.
pub fn respond_tui_approval(
    state: &mut TuiState,
    coordinator: &crate::approval::ApprovalCoordinator,
    owner_project: Option<&str>,
    identity: (&str, &str, &str, &str),
    choice: (bool, bool),
) -> Result<(), crate::approval::ApprovalError> {
    use crate::approval::{ApprovalDecision, ApprovalError, ApprovalResponse};
    let (project, request_id, turn_id, call_id) = identity;
    let (allow, remember) = choice;
    if owner_project != Some(project) || state.active_project() != project {
        return Err(ApprovalError::Invalid(
            "Approval belongs to another project".into(),
        ));
    }
    let valid = state.approval_views.get(project).is_some_and(|view| {
        view.requests
            .iter()
            .any(|r| r.request_id == request_id && r.turn_id == turn_id && r.call_id == call_id)
    });
    if !valid || !coordinator.is_pending(request_id) {
        return Err(ApprovalError::UnknownRequest);
    }
    coordinator.respond(ApprovalResponse {
        request_id: request_id.into(),
        decision: if allow {
            ApprovalDecision::Allow
        } else {
            ApprovalDecision::Deny
        },
        remember,
    })?;
    state.retire_approval(request_id);
    state.push_message(
        MessageRole::System,
        format!(
            "Approval decision submitted: {}{}",
            if allow { "allow" } else { "deny" },
            if remember { " · remember" } else { " once" }
        ),
    );
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ModelMenu {
    entries: Vec<crate::providers::registry::ModelDescriptor>,
    active: Option<crate::providers::registry::ModelDescriptor>,
    reasoning_supported: bool,
    effort: Option<String>,
}

const LARGE_PASTE_CHARS: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PasteFold {
    id: u64,
    start_byte: usize,
    end_byte: usize,
    char_count: usize,
}

#[derive(Debug, Clone)]
struct PasteMetadata {
    folds: Vec<PasteFold>,
    next_id: u64,
}

impl Default for PasteMetadata {
    fn default() -> Self {
        Self {
            folds: Vec::new(),
            next_id: 1,
        }
    }
}

impl PasteMetadata {
    fn after_edit(&mut self, input: &str, start: usize, end: usize, inserted: usize) {
        if self.folds.is_empty() {
            return;
        }
        let boundaries = input
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .chain(std::iter::once(input.len()))
            .collect::<Vec<_>>();
        self.folds.retain_mut(|fold| {
            if (start < fold.end_byte && end > fold.start_byte)
                || (start == end && start > fold.start_byte && start < fold.end_byte)
            {
                return false;
            }
            if fold.start_byte >= end {
                fold.start_byte = fold.start_byte + inserted - (end - start);
                fold.end_byte = fold.end_byte + inserted - (end - start);
            }
            boundaries.binary_search(&fold.start_byte).is_ok()
                && boundaries.binary_search(&fold.end_byte).is_ok()
        });
    }

    fn restore(input: &str, draft: &serde_json::Value) -> Option<Self> {
        let folds: Vec<PasteFold> = match draft.get("paste_folds") {
            Some(value) => serde_json::from_value(value.clone()).ok()?,
            None => Vec::new(),
        };
        let next_id = match draft.get("next_paste_id") {
            Some(value) => value.as_u64()?,
            None if folds.is_empty() => 1,
            None => return None,
        };
        if next_id == 0 || folds.len() > MAX_MESSAGE_BYTES / (LARGE_PASTE_CHARS + 1) {
            return None;
        }
        let boundaries = input
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .chain(std::iter::once(input.len()))
            .collect::<Vec<_>>();
        let mut ids = BTreeSet::new();
        let mut previous = 0;
        for fold in &folds {
            if fold.id == 0
                || fold.id >= next_id
                || !ids.insert(fold.id)
                || fold.start_byte < previous
                || fold.start_byte >= fold.end_byte
                || boundaries.binary_search(&fold.start_byte).is_err()
                || boundaries.binary_search(&fold.end_byte).is_err()
                || fold.char_count <= LARGE_PASTE_CHARS
                || input.get(fold.start_byte..fold.end_byte)?.chars().count() != fold.char_count
            {
                return None;
            }
            previous = fold.end_byte;
        }
        Some(Self { folds, next_id })
    }

    fn visible_cursor(&self, cursor: usize) -> usize {
        self.folds
            .iter()
            .find(|fold| cursor > fold.start_byte && cursor < fold.end_byte)
            .map_or(cursor, |fold| fold.end_byte)
    }
}

#[derive(Debug, Clone)]
struct InputDraft {
    text: String,
    cursor: usize,
    paste: PasteMetadata,
}

// A failed submission already brings its canonical text back from the host.
// Retain only bounded editing metadata, never another copy of the payload.
#[derive(Debug, Clone)]
struct SubmittedPaste {
    digest: [u8; 32],
    cursor: usize,
    paste: PasteMetadata,
}

#[derive(Debug, Clone)]
struct SubmittedInput {
    text: String,
    paste: Option<SubmittedPaste>,
}

struct InputProjection {
    text: String,
    spans: Vec<(usize, usize, usize, usize)>,
}

impl InputProjection {
    fn new(input: &str, folds: &[PasteFold]) -> Self {
        let mut text = String::new();
        let mut spans = Vec::with_capacity(folds.len());
        let mut previous = 0;
        for fold in folds {
            text.push_str(&input[previous..fold.start_byte]);
            let start = text.len();
            text.push_str(&format!(
                "[Pasted Content {} chars] #{}",
                fold.char_count, fold.id
            ));
            spans.push((fold.start_byte, fold.end_byte, start, text.len()));
            previous = fold.end_byte;
        }
        text.push_str(&input[previous..]);
        Self { text, spans }
    }

    fn display_cursor(&self, cursor: usize) -> usize {
        let mut canonical = 0;
        let mut display = 0;
        for &(start, end, shown_start, shown_end) in &self.spans {
            if cursor <= start {
                return display + cursor - canonical;
            }
            if cursor < end {
                return shown_start;
            }
            canonical = end;
            display = shown_end;
        }
        display + cursor - canonical
    }

    fn canonical_cursor(&self, cursor: usize, forward: bool) -> usize {
        let mut canonical = 0;
        let mut display = 0;
        for &(start, end, shown_start, shown_end) in &self.spans {
            if cursor <= shown_start {
                return canonical + cursor - display;
            }
            if cursor < shown_end {
                return if forward { end } else { start };
            }
            canonical = end;
            display = shown_end;
        }
        canonical + cursor - display
    }
}

fn input_digest(text: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes()).into()
}

#[test]
fn rejected_paste_restores_metadata_without_overriding_new_terminal_input() {
    let mut state = TuiState::default();
    let payload = "界".repeat(1001);
    state.handle_event(Event::Paste(payload.clone()));
    state.cursor = 0;
    let folds = state.paste.folds.clone();
    assert!(matches!(
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        TuiAction::Submit(_)
    ));
    state.restore_rejected_input(&payload);
    assert_eq!(state.input, payload);
    assert_eq!(state.cursor, 0);
    assert_eq!(state.paste.folds, folds);
    state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let now = Instant::now();
    state.handle_event_at(
        Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        now,
    );
    state.restore_rejected_input(&payload);
    assert!(state.input.is_empty());
    assert!(state.paste.folds.is_empty());
    assert!(state.submitted_pastes.is_empty());
    state.flush_ordinary_paste(now + Duration::from_millis(70));
    assert_eq!(state.input, "x");
}

#[test]
fn normalized_scheduled_shell_rejection_restores_the_exact_original_fold() {
    let mut state = TuiState::default();
    let mut pending = TuiPendingInputs::default();
    for _ in 0..MAX_TUI_PENDING_INPUTS {
        pending.admit_prompt("pending".into(), &mut state);
    }
    let original = format!("  !printf kept # {}\n", "x".repeat(1001));
    state.handle_event(Event::Paste(original.clone()));
    let folds = state.paste.folds.clone();
    state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    pending.admit_original(
        TuiRequestKind::UserShell,
        original.trim().strip_prefix('!').unwrap().to_owned(),
        original.clone(),
        &mut state,
    );
    assert_eq!(state.input, original);
    assert_eq!(state.paste.folds, folds);
}

fn grapheme_boundary(text: &str, offset: usize) -> bool {
    offset == text.len()
        || text
            .grapheme_indices(true)
            .any(|(index, _)| index == offset)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DraftEpoch {
    generation: u64,
    revision: Option<u64>,
}

/// Opaque, process-local identity for one explicit editor operation; no text payload.
#[derive(Debug, Clone)]
pub struct ExternalEditToken {
    id: u64,
    project: String,
    metadata: Option<ProjectTabMetadata>,
    session_id: Option<String>,
    epoch: DraftEpoch,
}

#[derive(Debug, Clone, Default)]
struct ProjectDraft {
    draft_epoch: DraftEpoch,
    kill_buffer: String,
    paste: PasteMetadata,
    submitted_pastes: VecDeque<SubmittedPaste>,
    queued_inputs: Vec<crate::input_queue::QueuedInput>,
    history_draft: Option<InputDraft>,
    history_search: Option<HistorySearch>,
    history_cursor: Option<usize>,
    model_menu: ModelMenu,
    command_resources: Vec<CommandResource>,
    ux: TranscriptUx,
    input: String,
    cursor: usize,
    history: VecDeque<String>,
    scroll: usize,
    input_scroll: usize,
    pane_scroll: BTreeMap<PaneId, u16>,
    presets: BTreeMap<TabId, LayoutModel>,
    busy: bool,
    status: String,
    streaming_role: Option<MessageRole>,
    streaming_job_id: Option<u64>,
    streaming_message_started: bool,
    streaming_block: Option<crate::view_model::StreamingBlock>,
    terminal_snapshot: String,
    session_snapshot: Option<SessionPaneSnapshot>,
    session_browser: Vec<crate::session::SessionSummary>,
    session_browser_cursor: usize,
    session_browser_refresh: SessionBrowserRefresh,
    goal_messages: VecDeque<TuiMessage>,
    persona: String,
    resource_snapshot: Option<crate::resources::ResourceSnapshot>,
    resource_status: Option<ResourcePaneStatus>,
    gantt_snapshot: Option<GanttPaneSnapshot>,
    gantt_status: Option<GanttPaneStatus>,
}

#[derive(Debug, Clone, Default)]
struct SessionBrowserRefresh {
    owner: Option<(std::path::PathBuf, String)>,
    directory: Option<std::path::PathBuf>,
    query: Option<String>,
    last_checked: Option<Instant>,
}

// The interactive host owns one in-flight scan and one replaceable pending
// request. Neither channel grows with keystrokes, ticks or project switches.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionBrowserScope {
    project: String,
    path: std::path::PathBuf,
    session_id: String,
    directory: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionBrowserKey {
    scope: SessionBrowserScope,
    query: Option<String>,
    generation: u64,
}

struct SessionBrowserRequest {
    key: SessionBrowserKey,
    original: Option<SubmittedInput>,
}

struct SessionBrowserProjection {
    rows: Result<Vec<crate::session::SessionSummary>, String>,
    receipt: Option<Result<String, String>>,
}

// Local inspection owns no Agent lock. One worker and one admitted operation
// bound both concurrency and retained receipts, independently of model work.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LocalDiffScope {
    project: String,
    cwd: std::path::PathBuf,
    session_path: Option<String>,
    session_id: Option<String>,
    generation: u64,
}

impl LocalDiffScope {
    fn capture(state: &TuiState, project: &str) -> Result<Self, String> {
        let metadata = state
            .project_metadata(project)
            .ok_or("diff requires a selected project workspace")?;
        let cwd = std::path::PathBuf::from(&metadata.cwd);
        if !cwd.is_absolute() {
            return Err("diff requires an absolute selected project workspace".into());
        }
        let epoch = if project == state.active_project() {
            state.draft_epoch
        } else {
            state
                .project_drafts
                .get(project)
                .ok_or("diff project closed")?
                .draft_epoch
        };
        Ok(Self {
            project: project.to_owned(),
            cwd,
            session_path: metadata.session_path.clone(),
            session_id: state
                .project_session_cursor(project)
                .map(|(id, _)| id.to_owned()),
            generation: epoch.generation,
        })
    }
    fn current(&self, state: &TuiState) -> bool {
        Self::capture(state, &self.project).as_ref() == Ok(self)
    }
    fn show(&self, state: &mut TuiState, role: MessageRole, text: String) {
        if !self.current(state) {
            return;
        }
        if self.project == state.active_project() {
            state.push_message(role, text);
        } else {
            let messages = state
                .project_transcripts
                .entry(self.project.clone())
                .or_default();
            messages.push_back(TuiMessage::new(role, text));
            while messages.len() > state.max_messages {
                messages.pop_front();
                if let Some(draft) = state.project_drafts.get_mut(&self.project) {
                    draft.ux.message_offset = draft.ux.message_offset.saturating_add(1);
                }
            }
            state.project_checkpoint_dirty = true;
            state.dirty = true;
        }
    }
}

struct LocalDiffRequest {
    scope: LocalDiffScope,
    original: SubmittedInput,
    epoch: DraftEpoch,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}
struct LocalDiffWork {
    cwd: std::path::PathBuf,
    path: Option<String>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}
struct LocalDiffHost {
    commands: Option<std::sync::mpsc::SyncSender<LocalDiffWork>>,
    results: Option<std::sync::mpsc::Receiver<Result<serde_json::Value, String>>>,
    thread: Option<std::thread::JoinHandle<()>>,
    active: Option<LocalDiffRequest>,
}
impl LocalDiffHost {
    fn new() -> io::Result<Self> {
        Self::with_reader(|work| {
            crate::slash_actions::diff_value_at_with_cancel(&work.cwd, work.path.as_deref(), || {
                work.cancelled.load(std::sync::atomic::Ordering::Acquire)
            })
            .map_err(|e| e.to_string())
        })
    }
    fn with_reader(
        mut read: impl FnMut(LocalDiffWork) -> Result<serde_json::Value, String> + Send + 'static,
    ) -> io::Result<Self> {
        let (commands, requests) = std::sync::mpsc::sync_channel(1);
        let (results, replies) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("zenpi-local-diff".into())
            .spawn(move || {
                while let Ok(work) = requests.recv() {
                    if results.send(read(work)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            commands: Some(commands),
            results: Some(replies),
            thread: Some(thread),
            active: None,
        })
    }
    fn start(&mut self, state: &mut TuiState, path: Option<String>, text: String) {
        let original = state.bind_submitted_input(text);
        let admission = LocalDiffScope::capture(state, state.active_project()).and_then(|scope| {
            if self.active.is_some() {
                return Err("local diff already running; cancel it or wait".into());
            }
            if scope.session_path.is_none() || scope.session_id.is_none() {
                return Err("local diff requires a selected session owner".into());
            }
            let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let work = LocalDiffWork {
                cwd: scope.cwd.clone(),
                path,
                cancelled: Arc::clone(&cancelled),
            };
            self.commands
                .as_ref()
                .ok_or("local diff worker closed")?
                .try_send(work)
                .map_err(|_| "local diff worker unavailable".to_owned())?;
            Ok((scope, cancelled))
        });
        match admission {
            Ok((scope, cancelled)) => {
                self.active = Some(LocalDiffRequest {
                    scope,
                    cancelled,
                    epoch: state.draft_epoch,
                    original,
                });
                state.push_message(
                    MessageRole::System,
                    "Local diff running · Ctrl-C or /cancel stops this inspection",
                );
            }
            Err(error) => {
                state.push_message(MessageRole::Error, error);
                state.restore_submitted_input(original);
            }
        }
    }
    fn cancel_current(&mut self, state: &mut TuiState) -> bool {
        if let Some(request) = &self.active
            && request.scope.project == state.active_project()
            && request.scope.current(state)
        {
            request
                .cancelled
                .store(true, std::sync::atomic::Ordering::Release);
            state.push_message(MessageRole::System, "Local diff cancellation requested");
            return true;
        }
        false
    }
    fn poll(&mut self, state: &mut TuiState) -> bool {
        if let Some(request) = &self.active
            && !request.scope.current(state)
        {
            request
                .cancelled
                .store(true, std::sync::atomic::Ordering::Release);
        }
        let result = match self.results.as_ref().map(|r| r.try_recv()) {
            Some(Ok(result)) => result,
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) if self.active.is_some() => {
                Err("local diff worker closed".into())
            }
            _ => return false,
        };
        let Some(request) = self.active.take() else {
            return false;
        };
        if !request.scope.current(state) {
            return false;
        }
        if request.cancelled.load(std::sync::atomic::Ordering::Acquire) {
            request
                .scope
                .show(state, MessageRole::System, "Local diff cancelled".into());
            return true;
        }
        match result {
            Ok(value) => request
                .scope
                .show(state, MessageRole::System, format_diff_view(&value)),
            Err(error) => {
                request
                    .scope
                    .show(state, MessageRole::Error, format!("diff failed: {error}"));
                // A receipt may restore only its unchanged draft, in its own
                // project/session. New typing (even subsequently erased) wins.
                if request.scope.project == state.active_project() {
                    if state.draft_epoch == request.epoch {
                        state.restore_submitted_input(request.original);
                    }
                } else if let Some(draft) = state.project_drafts.get_mut(&request.scope.project)
                    && draft.draft_epoch == request.epoch
                    && draft.input.is_empty()
                {
                    draft.input = request.original.text;
                    draft.cursor = draft.input.len();
                    if let Some(snapshot) = request.original.paste {
                        draft.paste = snapshot.paste;
                        draft.cursor = snapshot.cursor.min(draft.input.len());
                    }
                    draft.draft_epoch.revision =
                        draft.draft_epoch.revision.and_then(|n| n.checked_add(1));
                }
            }
        }
        true
    }
}
impl Drop for LocalDiffHost {
    fn drop(&mut self) {
        if let Some(request) = &self.active {
            request
                .cancelled
                .store(true, std::sync::atomic::Ordering::Release);
        }
        self.commands.take();
        self.results.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct SessionBrowserHost {
    commands: Option<std::sync::mpsc::SyncSender<(SessionBrowserKey, bool)>>,
    results: Option<std::sync::mpsc::Receiver<(SessionBrowserKey, SessionBrowserProjection)>>,
    thread: Option<std::thread::JoinHandle<()>>,
    active: Option<SessionBrowserRequest>,
    pending: Option<SessionBrowserRequest>,
    desired: Option<SessionBrowserKey>,
    generation: u64,
    closed: bool,
}

fn scan_session_browser(key: &SessionBrowserKey, explicit: bool) -> SessionBrowserProjection {
    let rows = match &key.query {
        Some(query) => crate::session::search_sessions(&key.scope.directory, query, 32)
            .map(|hits| hits.into_iter().map(|hit| hit.summary).collect()),
        None => crate::session::list_sessions(&key.scope.directory),
    }
    .map(|mut rows: Vec<_>| {
        rows.truncate(32);
        rows
    })
    .map_err(|error| bounded_display(&error.to_string()));
    // Preserve the existing command scopes: the pane scans the active owner
    // directory, whereas the list receipt uses the configured global catalog.
    // Both reads now run here, never on the input/render thread. Search keeps
    // its existing headless receipt (including snippets) in the owner directory.
    let receipt = explicit.then(|| {
        let data = match &key.query {
            Some(query) => crate::headless::session_search_view_in(&key.scope.directory, query),
            None => crate::headless::session_lifecycle_view(
                &crate::slash::SessionAction::List,
                Some(&key.scope.path),
            ),
        };
        data.map(|data| bounded_display(&data.to_string()))
            .map_err(|error| bounded_display(&error))
    });
    SessionBrowserProjection { rows, receipt }
}

impl SessionBrowserHost {
    fn new() -> io::Result<Self> {
        Self::with_scanner(scan_session_browser)
    }

    fn with_scanner(
        mut scan: impl FnMut(&SessionBrowserKey, bool) -> SessionBrowserProjection + Send + 'static,
    ) -> io::Result<Self> {
        let (commands, requests) = std::sync::mpsc::sync_channel(1);
        let (results, replies) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("zenpi-session-browser".into())
            .spawn(move || {
                while let Ok((key, explicit)) = requests.recv() {
                    let projection = scan(&key, explicit);
                    if results.send((key, projection)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            commands: Some(commands),
            results: Some(replies),
            thread: Some(thread),
            active: None,
            pending: None,
            desired: None,
            generation: 0,
            closed: false,
        })
    }

    fn scope(state: &TuiState) -> Option<SessionBrowserScope> {
        let refresh = &state.session_browser_refresh;
        let (path, session_id) = refresh.owner.as_ref()?;
        Some(SessionBrowserScope {
            project: state.active_project().to_owned(),
            path: path.clone(),
            session_id: session_id.clone(),
            directory: refresh.directory.clone()?,
        })
    }

    fn enqueue(
        &mut self,
        state: &mut TuiState,
        query: Option<String>,
        original: Option<SubmittedInput>,
    ) {
        let Some(scope) = Self::scope(state) else {
            if let Some(original) = original {
                state.push_message(
                    MessageRole::Error,
                    "session browser requires an active session owner",
                );
                state.restore_submitted_input(original);
            }
            return;
        };
        let Some(generation) = self.generation.checked_add(1).filter(|_| !self.closed) else {
            if let Some(original) = original {
                state.push_message(MessageRole::Error, "session browser worker unavailable");
                state.restore_submitted_input(original);
            }
            return;
        };
        self.generation = generation;
        let key = SessionBrowserKey {
            scope,
            query,
            generation,
        };
        self.desired = Some(key.clone());
        // A newer intent supersedes the pending intent, including its input
        // receipt. It must never restore an older command over a newer draft.
        self.pending = Some(SessionBrowserRequest { key, original });
    }

    fn route(&mut self, command: &SlashCommand, state: &mut TuiState, text: String) -> bool {
        let query = match command {
            SlashCommand::Session {
                action: crate::slash::SessionAction::List,
            } => None,
            SlashCommand::Session {
                action: crate::slash::SessionAction::Search { query },
            } => Some(query.clone()),
            _ => return false,
        };
        self.command(state, query, text);
        true
    }

    fn command(&mut self, state: &mut TuiState, query: Option<String>, text: String) {
        if text.len() > MAX_MESSAGE_BYTES {
            state.push_message(
                MessageRole::Error,
                "session browser command exceeds input receipt limit",
            );
            state.restore_rejected_input(text);
            return;
        }
        let original = state.bind_submitted_input(text);
        if query
            .as_ref()
            .is_some_and(|q| q.trim().is_empty() || q.len() > 256)
        {
            state.push_message(
                MessageRole::Error,
                "session search query or limit is invalid",
            );
            state.restore_submitted_input(original);
            return;
        }
        if query.is_none() {
            state.session_browser_refresh.query = None;
        }
        self.enqueue(state, query, Some(original));
    }

    fn poll(&mut self, state: &mut TuiState) -> bool {
        let scope = Self::scope(state);
        if self.desired.as_ref().map(|key| &key.scope) != scope.as_ref() {
            self.desired = None;
            self.pending = None;
            if scope.is_some() {
                self.enqueue(state, state.session_browser_refresh.query.clone(), None);
            }
        }
        let mut changed = false;
        match self.results.as_ref().map(|results| results.try_recv()) {
            Some(Ok((key, projection))) => {
                if self
                    .active
                    .as_ref()
                    .is_some_and(|request| request.key == key)
                {
                    let request = self.active.take().expect("matched active request");
                    // Check the complete key, including query and generation,
                    // and the live project/session before projecting any part.
                    if self.desired.as_ref() == Some(&key) && scope.as_ref() == Some(&key.scope) {
                        state.session_browser_refresh.last_checked = Some(Instant::now());
                        if let Ok(rows) = projection.rows {
                            let selected = state.selected_session_browser_path();
                            state.session_browser_cursor = if request.original.is_some()
                                && key.query.is_some()
                            {
                                0
                            } else {
                                selected
                                    .and_then(|path| rows.iter().position(|row| row.path == path))
                                    .unwrap_or_else(|| {
                                        state
                                            .session_browser_cursor
                                            .min(rows.len().saturating_sub(1))
                                    })
                            };
                            state.session_browser = rows;
                            state.session_browser_refresh.query = key.query.clone();
                            if request.original.is_some() && key.query.is_some() {
                                state.set_workspace_tab(TabId::Session);
                                state.focus_workspace_pane(PaneId::SessionList);
                            }
                        }
                        if let Some(receipt) = projection.receipt {
                            let label = if key.query.is_some() {
                                "session search"
                            } else {
                                "sessions"
                            };
                            match receipt {
                                Ok(text) => state
                                    .push_message(MessageRole::System, format!("{label}:\n{text}")),
                                Err(error) => {
                                    let failure = if key.query.is_some() {
                                        "session search"
                                    } else {
                                        "session listing"
                                    };
                                    state.push_message(
                                        MessageRole::Error,
                                        format!("{failure} failed: {error}"),
                                    );
                                    if let Some(original) = request.original {
                                        state.restore_submitted_input(original);
                                    }
                                }
                            }
                        }
                        state.dirty = true;
                        changed = true;
                    }
                }
            }
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) if !self.closed => {
                self.closed = true;
                let active = self.active.take();
                if let Some(request) = self.pending.take().or(active)
                    && self.desired.as_ref() == Some(&request.key)
                    && scope.as_ref() == Some(&request.key.scope)
                    && let Some(original) = request.original
                {
                    state.restore_submitted_input(original);
                }
                state.push_message(MessageRole::Error, "session browser worker closed");
                changed = true;
            }
            _ => {}
        }
        if !self.closed
            && self.active.is_none()
            && self.pending.is_none()
            && state
                .session_browser_refresh
                .last_checked
                .is_none_or(|last| last.elapsed() >= Duration::from_millis(500))
        {
            self.enqueue(state, state.session_browser_refresh.query.clone(), None);
        }
        if !self.closed
            && self.active.is_none()
            && let Some(request) = self.pending.take()
        {
            let job = (request.key.clone(), request.original.is_some());
            match self.commands.as_ref().expect("open worker").try_send(job) {
                Ok(()) => self.active = Some(request),
                Err(std::sync::mpsc::TrySendError::Full(_)) => self.pending = Some(request),
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    self.closed = true;
                    if let Some(original) = request.original {
                        state.restore_submitted_input(original);
                    }
                    state.push_message(MessageRole::Error, "session browser worker closed");
                    changed = true;
                }
            }
        }
        changed
    }
}

impl Drop for SessionBrowserHost {
    fn drop(&mut self) {
        // Disconnect both bounded channels before joining, so an unread reply
        // cannot deadlock shutdown. The terminal is restored before this drop.
        // An in-progress filesystem call remains non-cancellable; it may delay
        // process exit, but no input/render iteration waits for that scan.
        self.commands.take();
        self.results.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Debug, Clone)]
struct HistorySearch {
    query: String,
    before: Option<usize>,
    matched: Option<usize>,
}

// Ordinary key streams need a short classification window on terminals that
// do not frame paste events. Unicode/IME starts render immediately; ASCII starts
// wait at most one short interval. Both paths suppress Enter during a burst.
#[cfg(not(windows))]
const ORDINARY_PASTE_INTERVAL: Duration = Duration::from_millis(8);
#[cfg(windows)]
const ORDINARY_PASTE_INTERVAL: Duration = Duration::from_millis(30);
#[cfg(not(windows))]
const ORDINARY_PASTE_ACTIVE_IDLE: Duration = Duration::from_millis(8);
#[cfg(windows)]
const ORDINARY_PASTE_ACTIVE_IDLE: Duration = Duration::from_millis(60);
const ORDINARY_PASTE_ENTER_WINDOW: Duration = Duration::from_millis(120);

#[derive(Debug, Clone, Default)]
struct OrdinaryPasteBurst {
    buffer: String,
    last_input: Option<Instant>,
    suppress_until: Option<Instant>,
    active: bool,
    rejected: bool,
}

/// All mutable view state, with bounded queues and UTF-8-safe editing.
#[derive(Debug, Clone)]
pub struct TuiState {
    approval_views: BTreeMap<String, ApprovalView>,
    queued_inputs: Vec<crate::input_queue::QueuedInput>,
    scheduled_inputs: Vec<TuiScheduledInput>,
    file_completion: Option<(FileCompletionQuery, FileCompletionResult)>,
    command_resources: Vec<CommandResource>,
    transcript_ux: TranscriptUx,
    transcript_browser: Option<TranscriptBrowser>,
    follow_hit: Option<Rect>,
    project_workspace: Option<crate::project_workspace::ProjectWorkspace>,
    directory_picker: Option<crate::directory_picker::DirectoryPicker>,
    pending_project: Option<ProjectIntent>,
    project_drafts: BTreeMap<String, ProjectDraft>,
    messages: VecDeque<TuiMessage>,
    /// Goal has an explicit transcript lane rather than borrowing the active
    /// conversation's scroll state. Hosts may populate it as Goal execution
    /// events arrive.
    goal_messages: VecDeque<TuiMessage>,
    input: String,
    // One ephemeral editing buffer per project; never serialized or ticket-bound.
    kill_buffer: String,
    draft_epoch: DraftEpoch,
    next_draft_generation: Option<u64>,
    next_external_edit: Option<u64>,
    external_edit_active: Option<u64>,
    ordinary_paste: OrdinaryPasteBurst,
    paste: PasteMetadata,
    submitted_pastes: VecDeque<SubmittedPaste>,
    cursor: usize,
    status: String,
    busy: bool,
    scroll: usize,
    history: VecDeque<String>,
    history_cursor: Option<usize>,
    history_draft: Option<InputDraft>,
    history_search: Option<HistorySearch>,
    /// Visual row offset for the multiline prompt viewport.
    input_scroll: usize,
    /// Display-cell column retained while moving vertically through lines.
    preferred_column: Option<usize>,
    /// Whether verbose tool lifecycle entries are replaced by one summary
    /// line in the transcript.  Tool entries remain in the bounded message
    /// queue, so toggling this flag never loses history.
    fold_tool_logs: bool,
    /// Role of the message currently being assembled from provider deltas.
    /// Keeping this marker separate from the transcript lets the terminal
    /// replace the provisional streamed text with the authoritative final
    /// assistant turn instead of appending the same answer a second time.
    streaming_role: Option<MessageRole>,
    /// Runtime job that owns the provisional stream. Provider events are
    /// buffered per job in the async host; this second guard prevents a late
    /// event/finalizer from an interrupted job rewriting the replacement turn.
    streaming_job_id: Option<u64>,
    streaming_message_started: bool,
    /// Canonical bounded block state for the active streamed assistant text.
    /// The transcript remains the renderer-facing projection of this block.
    streaming_block: Option<crate::view_model::StreamingBlock>,
    /// The active lightweight workspace layout.  The legacy `render` method
    /// intentionally remains available for embedders; the production async
    /// loop opts into [`Self::render_bentobox`] below.
    workspace_layout: LayoutModel,
    /// Inactive tab models are retained independently so switching tabs does
    /// not leak one tab's ratios/focus into another. The active model remains
    /// in `workspace_layout` for compatibility with existing render helpers.
    workspace_layouts: BTreeMap<TabId, LayoutModel>,
    /// Set only when user-owned layout state changes. Hosts use this bit to
    /// persist layout changes without writing a file on every rendered frame.
    layout_dirty: bool,
    /// Tabs whose saved state should be removed rather than replaced with a
    /// serialized copy of the built-in preset. This preserves reset semantics
    /// across future preset/schema migrations.
    layout_resets: BTreeSet<TabId>,
    /// Latest completed workspace/host snapshot shown by the Resources pane.
    /// Collection is owned by the production host's bounded background runner;
    /// rendering only reads this small value and therefore never walks disk.
    resource_snapshot: Option<crate::resources::ResourceSnapshot>,
    resource_status: ResourcePaneStatus,
    /// Last valid Blueprint/Goal projection. Collection and store validation
    /// happen on a dedicated bounded worker in the production host.
    gantt_snapshot: Option<GanttPaneSnapshot>,
    gantt_status: GanttPaneStatus,
    /// Last bounded, redacted local shell result for the optional Terminal
    /// pane. The pane never owns a child process; execution remains in the
    /// host runner.
    terminal_snapshot: String,
    session_snapshot: Option<SessionPaneSnapshot>,
    session_browser: Vec<crate::session::SessionSummary>,
    session_browser_cursor: usize,
    session_browser_refresh: SessionBrowserRefresh,
    async_session_browser: bool,
    /// Stable project identity for the Wave-style top-level workspace tab.
    /// Feature projections (goal/learn/review/session) live inside it.
    project_name: String,
    /// Wave-style top-level project tabs. Feature tabs remain projections
    /// within the selected project.
    project_tabs: Vec<String>,
    active_project: usize,
    project_transcripts: BTreeMap<String, VecDeque<TuiMessage>>,
    project_layouts: BTreeMap<String, LayoutModel>,
    project_metadata: BTreeMap<String, ProjectTabMetadata>,
    project_subtabs: BTreeMap<String, Vec<SubTab>>,
    active_subtab: BTreeMap<String, usize>,
    subtab_hits: Vec<(Rect, SubTabHit)>,
    project_session_cursors: BTreeMap<String, ProjectSessionCursor>,
    project_checkpoint_dirty: bool,
    checkpoint_error: Option<String>,
    persona: String,
    palette_index: usize,
    palette_dismissed: bool,
    model_menu: ModelMenu,
    /// Per-zone model selections for the discussion and arch regions (ZS1-152).
    /// The actual parallel work count still comes from the active project's
    /// layer-2 workspace.
    zone_models: crate::view_model::ZoneModels,
    workspace_area: Rect,
    /// Inline editable Goal for the top-left conversation group (ZS1-147).
    goal_edit: Option<crate::view_model::GoalEdit>,
    /// Last committed Goal label shown beside the discussion conversation.
    goal_text: String,
    /// Committed Goal handed to the domain owner on the next host tick.
    goal_edit_intent: Option<String>,
    /// Rectangle of the docked discussion prompt while it is rendered in the
    /// left column. Used to anchor the command palette above the docked input.
    docked_prompt_rect: Option<Rect>,
    /// Which left-column prompt owns keyboard focus (ZS1-148). The discussion
    /// prompt is the default so existing single-prompt behavior is unchanged.
    left_prompt: LeftPrompt,
    /// Resident transcript for the lower-left Arch master-session console
    /// (ZS1-148). It is intentionally separate from the discussion transcript:
    /// arch records operator bash/steer submissions and their bounded results.
    arch_messages: VecDeque<TuiMessage>,
    /// Draft buffer for the arch console. Edited only while `left_prompt` is
    /// [`LeftPrompt::Arch`].
    arch_input: String,
    arch_cursor: usize,
    /// Rectangle of the docked arch prompt while it is rendered in the left
    /// column, so a mouse press can move focus into the arch console.
    arch_prompt_rect: Option<Rect>,
    /// Single-concurrency guard for the master session. A bash command cannot
    /// start while a master turn is already active; a steering instruction
    /// joins the active turn instead of forking a second worker.
    master_busy: bool,
    /// Last classified arch submission, drained by the host that owns the
    /// master session. The view layer never executes a command itself.
    arch_intent: Option<MasterSessionCommand>,
    /// Whether the current frame renders the prompt inside the left column.
    dock_prompt: bool,
    palette_area: Rect,
    palette_start: usize,
    palette_rows: usize,
    project_hits: Vec<(Rect, usize)>,
    /// Layer-1 tab currently armed by a left-button press so a subsequent
    /// drag can reorder it. Cleared on release; never persisted.
    dragging_project_tab: Option<usize>,
    /// Layer-2 sub-tab currently armed by a left-button press for drag
    /// reordering. Cleared on release; never persisted.
    dragging_subtab: Option<usize>,
    dragging_split: Option<(
        crate::layout::Column,
        crate::layout::Column,
        u16,
        crate::layout::ColumnRatios,
    )>,
    dragging_row: Option<(PaneId, PaneId, u16, u16, u16, u16)>,
    pane_scroll: BTreeMap<PaneId, u16>,
    max_messages: usize,
    max_history: usize,
    spinner_tick: usize,
    last_area: Rect,
    cached_transcript: Option<(u16, Vec<TranscriptRow>)>,
    dirty: bool,
}

impl Default for TuiState {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_MESSAGES)
    }
}

impl TuiState {
    pub fn new(max_messages: usize) -> Self {
        Self {
            approval_views: BTreeMap::new(),
            queued_inputs: Vec::new(),
            scheduled_inputs: Vec::new(),
            file_completion: None,
            command_resources: Vec::new(),
            transcript_ux: TranscriptUx::default(),
            transcript_browser: None,
            follow_hit: None,
            project_workspace: None,
            directory_picker: None,
            pending_project: None,
            project_drafts: BTreeMap::new(),
            messages: VecDeque::new(),
            goal_messages: VecDeque::new(),
            input: String::new(),
            kill_buffer: String::new(),
            draft_epoch: DraftEpoch {
                generation: 1,
                revision: Some(0),
            },
            next_draft_generation: Some(2),
            next_external_edit: Some(1),
            external_edit_active: None,
            ordinary_paste: OrdinaryPasteBurst::default(),
            paste: PasteMetadata::default(),
            submitted_pastes: VecDeque::new(),
            cursor: 0,
            status: "Ready".into(),
            busy: false,
            scroll: 0,
            history: VecDeque::new(),
            history_cursor: None,
            history_draft: None,
            history_search: None,
            input_scroll: 0,
            preferred_column: None,
            fold_tool_logs: false,
            streaming_role: None,
            streaming_job_id: None,
            streaming_message_started: false,
            streaming_block: None,
            workspace_layout: LayoutModel::new(TabId::Project),
            workspace_layouts: BTreeMap::new(),
            layout_dirty: false,
            layout_resets: BTreeSet::new(),
            resource_snapshot: None,
            resource_status: ResourcePaneStatus::Idle,
            gantt_snapshot: None,
            gantt_status: GanttPaneStatus::Idle,
            terminal_snapshot: "No local shell result".into(),
            session_snapshot: None,
            session_browser: Vec::new(),
            session_browser_cursor: 0,
            session_browser_refresh: SessionBrowserRefresh::default(),
            async_session_browser: false,
            project_name: "default".into(),
            project_tabs: vec!["default".into()],
            active_project: 0,
            project_transcripts: BTreeMap::new(),
            project_layouts: BTreeMap::new(),
            project_metadata: BTreeMap::new(),
            project_subtabs: BTreeMap::new(),
            active_subtab: BTreeMap::new(),
            subtab_hits: Vec::new(),
            project_session_cursors: BTreeMap::new(),
            project_checkpoint_dirty: false,
            checkpoint_error: None,
            persona: "INTJ".into(),
            palette_index: 0,
            palette_dismissed: false,
            model_menu: ModelMenu::default(),
            zone_models: crate::view_model::ZoneModels::default(),
            workspace_area: Rect::default(),
            goal_edit: None,
            goal_text: String::new(),
            goal_edit_intent: None,
            docked_prompt_rect: None,
            left_prompt: LeftPrompt::Discussion,
            arch_messages: VecDeque::new(),
            arch_input: String::new(),
            arch_cursor: 0,
            arch_prompt_rect: None,
            master_busy: false,
            arch_intent: None,
            dock_prompt: false,
            palette_area: Rect::default(),
            palette_start: 0,
            palette_rows: 0,
            project_hits: Vec::new(),
            dragging_project_tab: None,
            dragging_subtab: None,
            dragging_split: None,
            dragging_row: None,
            pane_scroll: BTreeMap::new(),
            max_messages: max_messages.max(1),
            max_history: 100,
            spinner_tick: 0,
            last_area: Rect::default(),
            cached_transcript: None,
            dirty: true,
        }
    }

    pub fn messages(&self) -> impl Iterator<Item = &TuiMessage> {
        self.messages.iter()
    }

    pub fn session_snapshot(&self) -> Option<&SessionPaneSnapshot> {
        self.session_snapshot.as_ref()
    }

    /// Refresh only from the current session owner, never from provider text.
    /// The pane projects validated envelopes. The interactive host only copies
    /// browser ownership here; its worker performs directory I/O. Synchronous
    /// embedders retain the legacy browser helper and 500 ms scan throttle.
    pub fn refresh_session_snapshot(&mut self, session: &crate::session::SessionStore) {
        // Goal execution messages belong to the old owner/session. Clear the
        // lane before projecting the replacement journal.
        if self
            .session_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.session_id != session.session_id())
        {
            self.clear_goal_messages();
        }
        self.persona = session
            .events()
            .iter()
            .rev()
            .find(|event| event["type"] == "persona_selected")
            .and_then(|event| event["persona"].as_str())
            .and_then(crate::persona::normalize)
            .unwrap_or("INTJ")
            .into();
        self.cached_transcript = None;
        let snapshot = SessionPaneSnapshot::from_session(session);
        self.refresh_session_browser(session);
        self.set_project_session_cursor(
            self.active_project().to_owned(),
            session.session_id().to_owned(),
            session.next_sequence(),
        );
        if self.session_snapshot.as_ref() != Some(&snapshot) {
            self.session_snapshot = Some(snapshot);
            self.dirty = true;
        }
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Return the currently selected BentoBox workspace tab.
    pub fn workspace_tab(&self) -> TabId {
        self.workspace_layout.tab
    }

    /// Set the top-level project label without changing any feature pane.
    pub fn set_project_name(&mut self, name: impl Into<String>) {
        let name = name.into();
        if !name.trim().is_empty() && name != self.project_name {
            // Project identity is the top-level tab identity. Keep the
            // legacy setter from creating a label that is absent from the
            // project strip.
            let _ = self.rename_project_tab(&self.project_name.clone(), name);
        }
    }

    pub fn project_tabs(&self) -> &[String] {
        &self.project_tabs
    }

    pub fn project_index(&self, name: &str) -> Option<usize> {
        self.project_tabs.iter().position(|item| item == name)
    }

    pub fn active_project(&self) -> &str {
        &self.project_tabs[self.active_project]
    }

    /// Create a project tab and select it. Each project owns its session and
    /// workspace in the host; this view layer only tracks stable tab labels.
    pub fn open_project_tab(&mut self, name: impl Into<String>) -> bool {
        let name = name.into();
        if name.trim().is_empty()
            || self.project_tabs.contains(&name)
            || self.project_tabs.len() >= crate::project_workspace::MAX_PROJECT_TABS
        {
            return false;
        }
        self.touch_editor_draft();
        // A new project opens on the current layer: insert it right after the
        // active project instead of appending to the far right.
        let insert_at = (self.active_project + 1).min(self.project_tabs.len());
        self.project_tabs.insert(insert_at, name);
        self.select_project_tab(insert_at)
    }

    pub fn select_project_tab(&mut self, index: usize) -> bool {
        if index >= self.project_tabs.len() {
            return false;
        }
        if index == self.active_project {
            return true;
        }
        self.finish_ordinary_paste();
        let old = self.active_project().to_owned();
        self.project_transcripts
            .insert(old.clone(), std::mem::take(&mut self.messages));
        self.project_layouts
            .insert(old.clone(), self.workspace_layout.clone());
        self.project_drafts.insert(
            old,
            ProjectDraft {
                draft_epoch: self.draft_epoch,
                kill_buffer: std::mem::take(&mut self.kill_buffer),
                paste: std::mem::take(&mut self.paste),
                submitted_pastes: std::mem::take(&mut self.submitted_pastes),
                queued_inputs: std::mem::take(&mut self.queued_inputs),
                history_draft: self.history_draft.take(),
                history_search: self.history_search.take(),
                history_cursor: self.history_cursor.take(),
                model_menu: std::mem::take(&mut self.model_menu),
                command_resources: std::mem::take(&mut self.command_resources),
                ux: std::mem::take(&mut self.transcript_ux),
                input: std::mem::take(&mut self.input),
                cursor: self.cursor,
                history: std::mem::take(&mut self.history),
                scroll: self.scroll,
                input_scroll: self.input_scroll,
                pane_scroll: std::mem::take(&mut self.pane_scroll),
                presets: std::mem::take(&mut self.workspace_layouts),
                busy: self.busy,
                status: self.status.clone(),
                streaming_role: self.streaming_role,
                streaming_job_id: self.streaming_job_id,
                streaming_message_started: self.streaming_message_started,
                streaming_block: self.streaming_block.take(),
                terminal_snapshot: std::mem::take(&mut self.terminal_snapshot),
                session_snapshot: self.session_snapshot.take(),
                session_browser: std::mem::take(&mut self.session_browser),
                session_browser_cursor: self.session_browser_cursor,
                session_browser_refresh: std::mem::take(&mut self.session_browser_refresh),
                goal_messages: std::mem::take(&mut self.goal_messages),
                persona: self.persona.clone(),
                resource_snapshot: self.resource_snapshot.take(),
                resource_status: Some(self.resource_status.clone()),
                gantt_snapshot: self.gantt_snapshot.take(),
                gantt_status: Some(self.gantt_status.clone()),
            },
        );
        self.active_project = index;
        self.project_name = self.project_tabs[index].clone();
        self.messages = self
            .project_transcripts
            .remove(&self.project_name)
            .unwrap_or_default();
        self.workspace_layout = self
            .project_layouts
            .remove(&self.project_name)
            .unwrap_or_else(|| LayoutModel::new(TabId::Project));
        let draft = self
            .project_drafts
            .remove(&self.project_name)
            .unwrap_or_default();
        self.file_completion = None;
        self.queued_inputs = draft.queued_inputs;
        self.history_draft = draft.history_draft;
        self.history_search = draft.history_search;
        self.history_cursor = draft.history_cursor;
        self.model_menu = draft.model_menu;
        self.command_resources = draft.command_resources;
        self.transcript_ux = draft.ux;
        self.transcript_browser = None;
        self.draft_epoch = self.activate_draft_epoch(draft.draft_epoch);
        self.input = draft.input;
        self.kill_buffer = draft.kill_buffer;
        self.paste = draft.paste;
        self.submitted_pastes = draft.submitted_pastes;
        self.cursor = draft.cursor;
        self.history = draft.history;
        self.scroll = draft.scroll;
        self.input_scroll = draft.input_scroll;
        self.pane_scroll = draft.pane_scroll;
        self.workspace_layouts = draft.presets;
        self.busy = draft.busy;
        self.status = if draft.status.is_empty() {
            "Ready".into()
        } else {
            draft.status
        };
        self.streaming_role = draft.streaming_role;
        self.streaming_job_id = draft.streaming_job_id;
        self.streaming_message_started = draft.streaming_message_started;
        self.streaming_block = draft.streaming_block;
        self.terminal_snapshot = draft.terminal_snapshot;
        self.session_snapshot = draft.session_snapshot;
        self.session_browser = draft.session_browser;
        self.session_browser_cursor = draft.session_browser_cursor;
        self.session_browser_refresh = draft.session_browser_refresh;
        self.goal_messages = draft.goal_messages;
        self.persona = if draft.persona.is_empty() {
            "INTJ".into()
        } else {
            draft.persona
        };
        self.resource_snapshot = draft.resource_snapshot;
        self.resource_status = draft.resource_status.unwrap_or(ResourcePaneStatus::Idle);
        self.gantt_snapshot = draft.gantt_snapshot;
        self.gantt_status = draft.gantt_status.unwrap_or(GanttPaneStatus::Idle);
        self.preferred_column = None;
        self.cached_transcript = None;
        self.palette_dismissed = false;
        self.project_metadata
            .entry(self.project_name.clone())
            .or_default();
        self.dirty = true;
        self.project_checkpoint_dirty = true;
        true
    }

    /// Move a project tab to a new zero-based position, preserving the active
    /// project by name.
    pub fn move_project_tab(&mut self, name: &str, target: usize) -> bool {
        let Some(index) = self.project_index(name) else {
            return false;
        };
        let len = self.project_tabs.len();
        if len < 2 {
            return false;
        }
        let target = target.min(len - 1);
        if target == index {
            return true;
        }
        let active_name = self.project_tabs[self.active_project].clone();
        let tab = self.project_tabs.remove(index);
        self.project_tabs.insert(target, tab);
        self.active_project = self
            .project_tabs
            .iter()
            .position(|item| item == &active_name)
            .unwrap_or(0);
        self.project_name = self.project_tabs[self.active_project].clone();
        true
    }

    /// Set a project tab's display style from the small named palette.
    pub fn style_project_tab(&mut self, name: &str, style: &str) -> bool {
        let style = style.to_ascii_lowercase();
        if !PROJECT_STYLES.contains(&style.as_str()) {
            return false;
        }
        let Some(index) = self.project_index(name) else {
            return false;
        };
        let key = self.project_tabs[index].clone();
        self.project_metadata.entry(key).or_default().style = Some(style);
        true
    }

    fn project_style_color(&self, index: usize) -> Option<Color> {
        let name = self.project_tabs.get(index)?;
        match self.project_metadata.get(name)?.style.as_deref()? {
            "cyan" => Some(Color::Cyan),
            "green" => Some(Color::Green),
            "yellow" => Some(Color::Yellow),
            "magenta" => Some(Color::Magenta),
            "blue" => Some(Color::Blue),
            "red" => Some(Color::Red),
            "white" => Some(Color::White),
            _ => None,
        }
    }

    fn ensure_subtabs(&mut self) {
        let project = self.active_project().to_owned();
        if !self.project_subtabs.contains_key(&project) {
            let root = self
                .project_metadata
                .get(&project)
                .map(|metadata| metadata.cwd.clone())
                .filter(|cwd| !cwd.is_empty())
                .unwrap_or_else(|| self.project_cwd().display().to_string());
            self.project_subtabs.insert(
                project.clone(),
                vec![SubTab {
                    name: project.clone(),
                    root,
                    kind: SubTabKind::Main,
                    concurrency: 1,
                }],
            );
            self.active_subtab.insert(project, 0);
        }
    }

    /// Layer-2 tabs of the active project (default reuses the layer-1 data).
    pub fn subtabs(&mut self) -> Vec<SubTab> {
        self.ensure_subtabs();
        self.project_subtabs
            .get(self.active_project())
            .cloned()
            .unwrap_or_default()
    }

    pub fn active_subtab(&mut self) -> usize {
        self.ensure_subtabs();
        *self.active_subtab.get(self.active_project()).unwrap_or(&0)
    }

    /// Root of the active layer-2 workspace. The default `Main` tab resolves
    /// to the layer-1 project folder; every other tab resolves to its own
    /// isolated root. Layer-1 and layer-2 state are never aliased implicitly.
    pub fn active_subtab_root(&mut self) -> String {
        let index = self.active_subtab();
        self.subtabs()
            .get(index)
            .map(|tab| tab.root.clone())
            .unwrap_or_else(|| self.project_cwd().display().to_string())
    }

    pub fn subtab_select(&mut self, index: usize) -> bool {
        self.ensure_subtabs();
        let project = self.active_project().to_owned();
        let len = self
            .project_subtabs
            .get(&project)
            .map(Vec::len)
            .unwrap_or(0);
        if index < len {
            self.active_subtab.insert(project, index);
            true
        } else {
            false
        }
    }

    /// Add a layer-2 tab that works without creating a git worktree. It still
    /// receives an isolated workspace root under the project so two layer-2
    /// opens never share the layer-1 working folder.
    pub fn subtab_add_in_place(&mut self, name: Option<String>) -> bool {
        self.ensure_subtabs();
        let project = self.active_project().to_owned();
        let base = self.project_cwd();
        let tabs = self.project_subtabs.entry(project.clone()).or_default();
        if tabs.len() >= crate::project_workspace::MAX_PROJECT_SUBTABS {
            return false;
        }
        let name = name.unwrap_or_else(|| format!("in-place-{}", tabs.len()));
        let root = isolated_subtab_root(&base, tabs, &name);
        tabs.push(SubTab {
            name,
            root: root.display().to_string(),
            kind: SubTabKind::InPlace,
            concurrency: 1,
        });
        let index = tabs.len() - 1;
        self.active_subtab.insert(project, index);
        true
    }

    /// Create a fresh `git worktree` and open it as a layer-2 tab.
    pub fn subtab_add_worktree(&mut self, name: Option<String>) -> Result<String, String> {
        self.ensure_subtabs();
        let project = self.active_project().to_owned();
        let base = self.project_cwd();
        let tabs = self.project_subtabs.entry(project.clone()).or_default();
        if tabs.len() >= crate::project_workspace::MAX_PROJECT_SUBTABS {
            return Err("too many sub-tabs".into());
        }
        let name = name.unwrap_or_else(|| format!("wt-{}", tabs.len()));
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        {
            return Err("invalid worktree name".into());
        }
        let path = base.join(".zenpi-worktrees").join(&name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        crate::project_workspace::add_worktree(&base, &path, &name)?;
        tabs.push(SubTab {
            name: name.clone(),
            root: path.display().to_string(),
            kind: SubTabKind::Worktree,
            concurrency: 1,
        });
        let index = tabs.len() - 1;
        self.active_subtab.insert(project, index);
        Ok(name)
    }

    pub fn subtab_close(&mut self, index: usize) -> bool {
        self.ensure_subtabs();
        let project = self.active_project().to_owned();
        let cwd = self.project_cwd();
        let Some(tabs) = self.project_subtabs.get_mut(&project) else {
            return false;
        };
        if index == 0 || index >= tabs.len() {
            return false;
        }
        let removed = tabs.remove(index);
        if removed.kind == SubTabKind::Worktree {
            let _ = crate::project_workspace::remove_worktree(
                &cwd,
                std::path::Path::new(&removed.root),
            );
        }
        let active = self.active_subtab.entry(project).or_insert(0);
        if *active >= tabs.len() {
            *active = tabs.len().saturating_sub(1);
        } else if *active > index {
            *active -= 1;
        }
        true
    }

    pub fn subtab_move(&mut self, index: usize, target: usize) -> bool {
        self.ensure_subtabs();
        let project = self.active_project().to_owned();
        let active_name = {
            let active = *self.active_subtab.get(&project).unwrap_or(&0);
            self.project_subtabs
                .get(&project)
                .and_then(|tabs| tabs.get(active))
                .map(|tab| tab.name.clone())
        };
        let Some(tabs) = self.project_subtabs.get_mut(&project) else {
            return false;
        };
        if index == 0 || index >= tabs.len() || tabs.len() < 3 {
            return false;
        }
        let target = target.min(tabs.len() - 1).max(1);
        if target == index {
            return true;
        }
        let item = tabs.remove(index);
        tabs.insert(target, item);
        if let Some(name) = active_name
            && let Some(position) = tabs.iter().position(|tab| tab.name == name)
        {
            self.active_subtab.insert(project, position);
        }
        true
    }

    /// Move the active layer-1 project tab by one position (wraps).
    pub fn move_active_project(&mut self, delta: isize) -> bool {
        let len = self.project_tabs.len();
        if len < 2 {
            return false;
        }
        let target = (self.active_project as isize + delta).rem_euclid(len as isize) as usize;
        let name = self.active_project().to_owned();
        self.move_project_tab(&name, target)
    }

    /// Move the active layer-2 sub-tab by one position (the Main tab stays at 0).
    pub fn move_active_subtab(&mut self, delta: isize) -> bool {
        self.ensure_subtabs();
        let project = self.active_project().to_owned();
        let len = self
            .project_subtabs
            .get(&project)
            .map(Vec::len)
            .unwrap_or(0);
        if len < 3 {
            return false;
        }
        let current = *self.active_subtab.get(&project).unwrap_or(&0);
        if current == 0 {
            return false;
        }
        let target = (current as isize + delta).clamp(1, len as isize - 1) as usize;
        if target == current {
            return false;
        }
        self.subtab_move(current, target)
    }

    /// Rename one layer-2 tab (worktree or otherwise).
    pub fn subtab_rename(&mut self, index: usize, name: &str) -> bool {
        self.ensure_subtabs();
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let project = self.active_project().to_owned();
        let Some(tabs) = self.project_subtabs.get_mut(&project) else {
            return false;
        };
        if index >= tabs.len() {
            return false;
        }
        tabs[index].name = name.to_owned();
        true
    }

    /// Adjust the default harness concurrency for one layer-2 tab.
    pub fn subtab_concurrency(&mut self, index: usize, delta: isize) -> bool {
        self.ensure_subtabs();
        let project = self.active_project().to_owned();
        let Some(tabs) = self.project_subtabs.get_mut(&project) else {
            return false;
        };
        if index >= tabs.len() {
            return false;
        }
        let next = (tabs[index].concurrency as isize + delta).clamp(1, 64) as u16;
        if next == tabs[index].concurrency {
            return false;
        }
        tabs[index].concurrency = next;
        true
    }

    pub fn close_project_tab(&mut self, name: &str) -> bool {
        if self.project_tabs.len() <= 1 {
            return false;
        }
        let Some(index) = self.project_index(name) else {
            return false;
        };
        if index == self.active_project {
            self.select_project_tab(if index + 1 < self.project_tabs.len() {
                index + 1
            } else {
                index - 1
            });
        }
        self.project_tabs.remove(index);
        if index < self.active_project {
            self.active_project -= 1;
        }
        self.project_name = self.project_tabs[self.active_project].clone();
        self.project_transcripts.remove(name);
        self.project_layouts.remove(name);
        self.project_metadata.remove(name);
        self.project_session_cursors.remove(name);
        self.project_drafts.remove(name);
        self.approval_views.remove(name);
        self.scheduled_inputs.retain(|entry| entry.project != name);
        self.cached_transcript = None;
        self.dirty = true;
        self.project_checkpoint_dirty = true;
        true
    }

    fn project_cwd(&self) -> std::path::PathBuf {
        self.project_metadata(self.active_project())
            .map(|m| m.cwd.clone().into())
            .unwrap_or_else(|| ".".into())
    }
    pub fn directory_picker_open(&self) -> bool {
        self.directory_picker.is_some()
    }
    pub fn take_project_intent(&mut self) -> Option<ProjectIntent> {
        self.pending_project.take()
    }
    pub fn open_directory_picker(&mut self) {
        if let Some(view) = self
            .approval_views
            .get_mut(&self.project_tabs[self.active_project])
        {
            view.focused = false;
        }
        self.cancel_history_preview();
        let base = self
            .project_metadata(self.active_project())
            .map(|m| std::path::PathBuf::from(&m.cwd))
            .filter(|p| p.is_absolute() && p.is_dir())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| "/".into());
        self.directory_picker = Some(crate::directory_picker::DirectoryPicker::new(&base));
        self.dirty = true;
    }
    fn picker_result(&mut self, result: Option<Option<std::path::PathBuf>>) {
        if let Some(path) = result {
            self.directory_picker = None;
            self.pending_project = path.map(ProjectIntent::Open);
        }
        self.dirty = true;
    }
    fn request_project_select(&mut self, index: usize) {
        if index < self.project_tabs.len() && index != self.active_project {
            self.touch_editor_draft();
        }
        if self.project_workspace.is_some() {
            self.pending_project = Some(ProjectIntent::Select(index));
        } else {
            self.select_project_tab(index);
        }
    }
    pub fn project_label(&self, index: usize) -> String {
        let key = &self.project_tabs[index];
        self.project_workspace
            .as_ref()
            .and_then(|w| w.tabs().iter().find(|t| t.id().as_str() == key))
            .map(|tab| {
                let duplicate = self
                    .project_workspace
                    .as_ref()
                    .unwrap()
                    .tabs()
                    .iter()
                    .filter(|t| t.title() == tab.title())
                    .count()
                    > 1;
                if duplicate {
                    format!(
                        "{} ·{} #{}",
                        truncate_to_width(tab.title(), 12),
                        truncate_to_width(
                            &tab.cwd()
                                .parent()
                                .unwrap_or(tab.cwd())
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy(),
                            6
                        ),
                        &tab.id().as_str()[..6]
                    )
                } else {
                    tab.title().to_owned()
                }
            })
            .unwrap_or_else(|| key.clone())
    }

    /// Migrate legacy display-name keys to canonical directory IDs, preserving
    /// their view data. Invalid legacy placeholders are not restored as projects.
    pub fn initialize_project_workspace(
        &mut self,
        agent: &crate::core::Agent,
    ) -> Result<(), String> {
        let cwd = agent
            .attachment_workspace_root()
            .unwrap_or_else(|| std::path::Path::new(&agent.session().header().cwd));
        let mut workspace = crate::project_workspace::ProjectWorkspace::default();
        let old_active = self.active_project().to_owned();
        let mut selected = None;
        for old in self.project_tabs.clone() {
            let path = self
                .project_metadata
                .get(&old)
                .map(|m| std::path::PathBuf::from(&m.cwd))
                .filter(|p| p.is_absolute());
            let Some(path) = path else {
                continue;
            };
            if let Ok((candidate, _)) = workspace.with_directory(Some(&path), cwd) {
                let id = candidate.active().unwrap().id().as_str().to_owned();
                if old == old_active {
                    selected = Some(candidate.active().unwrap().id().clone());
                }
                if old != id {
                    self.rename_project_tab(&old, id.clone());
                }
                workspace = candidate;
            }
        }
        if workspace.tabs().is_empty() {
            workspace = workspace
                .with_directory(Some(cwd), cwd)
                .map_err(|e| e.to_string())?
                .0;
            let id = workspace.active().unwrap().id().as_str().to_owned();
            let previous_project = self.active_project().to_owned();
            self.rename_project_tab(&previous_project, id);
            self.set_active_project_metadata(ProjectTabMetadata::from_session(agent.session()));
        } else if let Some(id) = selected {
            workspace = workspace.with_active(&id).map_err(|e| e.to_string())?;
        }
        self.project_tabs
            .retain(|id| workspace.tabs().iter().any(|t| t.id().as_str() == id));
        self.active_project = self
            .project_tabs
            .iter()
            .position(|id| Some(id.as_str()) == workspace.active().map(|t| t.id().as_str()))
            .unwrap_or(0);
        self.project_name = self.project_tabs[self.active_project].clone();
        self.project_workspace = Some(workspace);
        self.project_checkpoint_dirty = true;
        self.cached_transcript = None;
        Ok(())
    }

    pub fn project_tab_count(&self) -> usize {
        self.project_tabs.len()
    }

    pub fn active_project_index(&self) -> usize {
        self.active_project
    }

    /// Return a project tab and its metadata without exposing mutable state.
    pub fn project_tab_context(&self, index: usize) -> Option<(&str, Option<&ProjectTabMetadata>)> {
        let name = self.project_tabs.get(index)?;
        Some((name.as_str(), self.project_metadata.get(name)))
    }

    pub fn next_project_tab(&mut self, backwards: bool) -> bool {
        if self.project_tabs.len() < 2 {
            return false;
        }
        let next = if backwards {
            (self.active_project + self.project_tabs.len() - 1) % self.project_tabs.len()
        } else {
            (self.active_project + 1) % self.project_tabs.len()
        };
        self.touch_editor_draft();
        self.select_project_tab(next)
    }

    /// Expose the project strip in display order for renderers/hosts.
    pub fn project_tab_labels(&self) -> Vec<String> {
        self.project_tabs.clone()
    }

    /// Switch project and return the metadata the host must bind next.
    pub fn project_switch_context(&mut self, index: usize) -> Option<ProjectTabMetadata> {
        if index < self.project_tabs.len() && index != self.active_project {
            self.touch_editor_draft();
        }
        if !self.select_project_tab(index) {
            return None;
        }
        Some(
            self.project_metadata(self.active_project())
                .cloned()
                .unwrap_or_default(),
        )
    }

    /// Rename a project tab without changing its identity or stored view.
    pub fn rename_project_tab(&mut self, old: &str, new: impl Into<String>) -> bool {
        let new = new.into();
        if new.trim().is_empty() || self.project_tabs.iter().any(|item| item == &new) {
            return false;
        }
        let Some(index) = self.project_tabs.iter().position(|item| item == old) else {
            return false;
        };
        if index == self.active_project {
            self.touch_editor_draft();
        } else if let Some(draft) = self.project_drafts.get_mut(old) {
            draft.draft_epoch.revision = draft.draft_epoch.revision.and_then(|n| n.checked_add(1));
        }
        self.project_tabs[index] = new.clone();
        for entry in &mut self.scheduled_inputs {
            if entry.project == old {
                entry.project = new.clone();
            }
        }
        if let Some(value) = self.project_transcripts.remove(old) {
            self.project_transcripts.insert(new.clone(), value);
        }
        if let Some(value) = self.project_layouts.remove(old) {
            self.project_layouts.insert(new.clone(), value);
        }
        if let Some(value) = self.project_metadata.remove(old) {
            self.project_metadata.insert(new.clone(), value);
        }
        if let Some(value) = self.project_session_cursors.remove(old) {
            self.project_session_cursors.insert(new.clone(), value);
        }
        if let Some(view) = self.approval_views.remove(old) {
            self.approval_views.insert(new.clone(), view);
        }
        if let Some(draft) = self.project_drafts.remove(old) {
            self.project_drafts.insert(new.clone(), draft);
        }
        if self.active_project == index {
            self.project_name = new;
        }
        self.project_checkpoint_dirty = true;
        self.dirty = true;
        true
    }

    /// Persist the project-tab strip as a small JSON projection. The host can
    /// store this beside its session and restore it on the next launch.
    pub fn project_tabs_json(&self) -> serde_json::Value {
        let projects = self
            .project_tabs
            .iter()
            .map(|name| {
                let messages = if name == self.active_project() {
                    self.messages.clone()
                } else {
                    self.project_transcripts
                        .get(name)
                        .cloned()
                        .unwrap_or_default()
                };
                let layout = if name == self.active_project() {
                    Some(self.workspace_layout.clone())
                } else {
                    self.project_layouts.get(name).cloned()
                };
                let draft = if name == self.active_project() {
                    let paste = self.stable_paste();
                    serde_json::json!({"input":self.stable_input().0,"cursor":self.stable_input().1,
                        "paste_folds":paste.folds,"next_paste_id":paste.next_id,
                        "scroll":self.scroll,"history":self.history,"presets":self.workspace_layouts,
                        "reasoning_folded":self.transcript_ux.reasoning_folded})
                } else {
                    self.project_drafts.get(name).map(|draft| {
                        let (input,cursor,paste) = draft.history_draft.as_ref()
                            .map(|value| (value.text.as_str(),value.cursor,&value.paste))
                            .unwrap_or((&draft.input,draft.cursor,&draft.paste));
                        serde_json::json!({"input":input,"cursor":cursor,"paste_folds":paste.folds,
                            "next_paste_id":paste.next_id,"scroll":draft.scroll,"history":draft.history,
                            "presets":draft.presets,"reasoning_folded":draft.ux.reasoning_folded})
                    }).unwrap_or_default()
                };
                serde_json::json!({
                    "draft": draft,
                    "name": name,
                    "metadata": self.project_metadata.get(name),
                    "layout": layout,
                    "messages": messages,
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "schema_version": 3,
            "scheduled_inputs": self.scheduled_inputs,
            "workspace": self.project_workspace,
            "projects": self.project_tabs,
            "active": self.active_project,
            "project_name": self.project_name,
            "metadata": self.project_metadata,
            "session_cursors": self.project_session_cursors,
            "project_state": projects,
        })
    }

    /// Persist project identity independently from the pane-layout preference
    /// file. Hosts can checkpoint this projection beside their session index.
    pub fn project_checkpoint(&self) -> serde_json::Value {
        self.project_tabs_json()
    }

    pub fn project_checkpoint_dirty(&self) -> bool {
        self.project_checkpoint_dirty
    }
    pub fn clear_project_checkpoint_dirty(&mut self) {
        self.project_checkpoint_dirty = false;
    }

    fn checkpoint_result(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.clear_project_checkpoint_dirty();
                if self.checkpoint_error.take().is_some() {
                    self.dirty = true;
                }
            }
            Err(error) => {
                let error = inline_token(&error, 160);
                if self.checkpoint_error.as_ref() != Some(&error) {
                    self.checkpoint_error = Some(error);
                    self.dirty = true;
                }
            }
        }
    }

    /// Human-readable navigation contract used by status/help surfaces.
    pub fn project_navigation_model() -> &'static str {
        "top-level tabs are projects; Goal, Learn, Review, and Session are panes inside the active project workspace"
    }

    pub fn restore_project_tabs(&mut self, value: &serde_json::Value) -> bool {
        let Ok(encoded) = serde_json::to_vec(value) else {
            return false;
        };
        if encoded.len() > MAX_PROJECT_CHECKPOINT_BYTES {
            return false;
        }
        let workspace = match value.get("workspace").filter(|v| !v.is_null()) {
            Some(v) => match serde_json::to_vec(v)
                .ok()
                .and_then(|b| crate::project_workspace::ProjectWorkspace::from_json_bytes(&b).ok())
            {
                Some(w) => Some(w),
                None => return false,
            },
            None => None,
        };
        let Some(projects) = value.get("projects").and_then(|v| v.as_array()) else {
            return false;
        };
        // Keep the persisted strip deterministic even if an older host wrote
        // duplicate or blank project names.
        let mut names = Vec::new();
        for name in projects.iter().filter_map(|v| v.as_str()) {
            let name = name.trim();
            if name.is_empty() || names.iter().any(|item: &String| item == name) {
                continue;
            }
            names.push(name.to_owned());
        }
        if names.is_empty() || names.len() > crate::project_workspace::MAX_PROJECT_TABS {
            return false;
        }
        if let Some(w) = workspace.as_ref()
            && (names.len() != w.tabs().len()
                || names
                    .iter()
                    .any(|id| !w.tabs().iter().any(|t| t.id().as_str() == id)))
        {
            return false;
        }
        if let Some(w) = workspace.as_ref() {
            let active = value.get("active").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            if names.get(active).map(String::as_str) != w.active().map(|t| t.id().as_str()) {
                return false;
            }
            for tab in w.tabs() {
                if value["metadata"][tab.id().as_str()]["cwd"].as_str() != tab.cwd().to_str() {
                    return false;
                }
            }
        }
        self.touch_editor_draft();
        self.project_workspace = workspace;
        let active = value.get("active").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        self.scheduled_inputs = value
            .get("scheduled_inputs")
            .and_then(|v| serde_json::from_value::<Vec<TuiScheduledInput>>(v.clone()).ok())
            .unwrap_or_default()
            .into_iter()
            .take(MAX_TUI_PENDING_INPUTS)
            .filter(|entry| {
                entry.text.len() <= MAX_TUI_PENDING_INPUT_BYTES
                    && entry.id.len() <= 128
                    && names.contains(&entry.project)
            })
            .map(|mut entry| {
                entry.interrupted = true;
                entry
            })
            .collect();
        let mut retained = 0usize;
        self.scheduled_inputs.retain(|entry| {
            retained = retained.saturating_add(entry.text.len());
            retained <= MAX_TUI_PENDING_INPUT_BYTES
        });
        self.project_tabs = names;
        self.active_project = active.min(self.project_tabs.len() - 1);
        self.project_name = self.project_tabs[self.active_project].clone();
        self.project_transcripts.clear();
        self.project_layouts.clear();
        self.project_metadata.clear();
        self.project_session_cursors.clear();
        if let Some(metadata) = value.get("metadata") {
            self.project_metadata = serde_json::from_value(metadata.clone()).unwrap_or_default();
        }
        if let Some(cursors) = value.get("session_cursors") {
            self.project_session_cursors =
                serde_json::from_value(cursors.clone()).unwrap_or_default();
            self.project_session_cursors.retain(|name, cursor| {
                self.project_tabs.iter().any(|item| item == name)
                    && !cursor.session_id.is_empty()
                    && cursor.session_id.len() <= 128
            });
        }
        // A restored strip starts with explicit empty runtime slots. The host
        // may subsequently hydrate transcripts/layouts from its session store.
        for name in &self.project_tabs {
            self.project_transcripts.entry(name.clone()).or_default();
            self.project_layouts
                .entry(name.clone())
                .or_insert_with(|| self.workspace_layout.clone());
            self.project_metadata.entry(name.clone()).or_default();
        }
        // New checkpoints carry isolated per-project runtime state. Older
        // checkpoints remain readable and fall back to empty slots.
        let mut invalid_paste_metadata = false;
        if let Some(states) = value.get("project_state").and_then(|v| v.as_array()) {
            for state in states {
                let Some(name) = state.get("name").and_then(|v| v.as_str()) else {
                    continue;
                };
                if !self.project_tabs.iter().any(|item| item == name) {
                    continue;
                }
                if let Some(d) = state.get("draft") {
                    let input = d["input"].as_str().unwrap_or_default();
                    if input.len() <= MAX_MESSAGE_BYTES {
                        let cursor = (d["cursor"].as_u64().unwrap_or(0) as usize).min(input.len());
                        {
                            let invalid_cursor = !grapheme_boundary(input, cursor);
                            let cursor = if cursor == input.len() {
                                cursor
                            } else {
                                input
                                    .grapheme_indices(true)
                                    .map(|(offset, _)| offset)
                                    .take_while(|offset| *offset <= cursor)
                                    .last()
                                    .unwrap_or(0)
                            };
                            let paste = (!invalid_cursor)
                                .then(|| PasteMetadata::restore(input, d))
                                .flatten()
                                .unwrap_or_else(|| {
                                    invalid_paste_metadata = true;
                                    PasteMetadata::default()
                                });
                            let cursor = paste.visible_cursor(cursor);
                            self.project_drafts.insert(
                                name.to_owned(),
                                ProjectDraft {
                                    paste,
                                    ux: TranscriptUx {
                                        reasoning_folded: d["reasoning_folded"]
                                            .as_bool()
                                            .unwrap_or(true),
                                        ..TranscriptUx::default()
                                    },
                                    input: input.to_owned(),
                                    cursor,
                                    scroll: d["scroll"]
                                        .as_u64()
                                        .unwrap_or(0)
                                        .min(MAX_RENDER_LINES as u64)
                                        as usize,
                                    history: serde_json::from_value::<VecDeque<String>>(
                                        d["history"].clone(),
                                    )
                                    .unwrap_or_default()
                                    .into_iter()
                                    .take(self.max_history)
                                    .filter(|h| h.len() <= MAX_MESSAGE_BYTES)
                                    .collect(),
                                    presets: serde_json::from_value(d["presets"].clone())
                                        .unwrap_or_default(),
                                    ..ProjectDraft::default()
                                },
                            );
                        }
                    }
                }
                if let Some(layout) = state.get("layout")
                    && let Ok(layout) = serde_json::from_value::<LayoutModel>(layout.clone())
                    && (layout.tab == TabId::Project
                        || (value["schema_version"] == 3 && self.project_workspace.is_some()))
                {
                    self.project_layouts.insert(name.to_owned(), layout);
                }
                if let Some(messages) = state.get("messages")
                    && let Ok(messages) =
                        serde_json::from_value::<VecDeque<TuiMessage>>(messages.clone())
                    && messages.len() <= self.max_messages
                    && messages
                        .iter()
                        .all(|message| message.text.len() <= MAX_MESSAGE_BYTES)
                {
                    self.project_transcripts.insert(name.to_owned(), messages);
                }
                if self.project_workspace.is_none()
                    && let Some(metadata) = state.get("metadata")
                    && let Ok(metadata) =
                        serde_json::from_value::<ProjectTabMetadata>(metadata.clone())
                {
                    self.project_metadata.insert(name.to_owned(), metadata);
                }
            }
        }
        self.messages = self
            .project_transcripts
            .get(&self.project_name)
            .cloned()
            .unwrap_or_default();
        if let Some(layout) = self.project_layouts.get(&self.project_name).cloned() {
            self.workspace_layout = layout;
        }
        if let Some(draft) = self.project_drafts.remove(&self.project_name) {
            self.transcript_ux = draft.ux;
            self.draft_epoch = self.activate_draft_epoch(draft.draft_epoch);
            self.input = draft.input;
            self.paste = draft.paste;
            self.submitted_pastes.clear();
            self.cursor = draft.cursor;
            self.scroll = draft.scroll;
            self.history = draft.history;
            self.workspace_layouts = draft.presets;
        }
        if invalid_paste_metadata {
            self.push_message(
                MessageRole::System,
                "Invalid paste metadata; restored complete saved text without folding",
            );
            self.set_status("Paste metadata recovered; full draft retained");
        }
        self.cached_transcript = None;
        self.dirty = true;
        true
    }

    pub fn restore_project_checkpoint(&mut self, value: &serde_json::Value) -> bool {
        self.restore_project_tabs(value)
    }

    /// Return the currently selected project's complete in-memory view. This
    /// is the boundary a host uses when swapping project tabs atomically.
    pub fn project_view(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.active_project(),
            "metadata": self.project_metadata(self.active_project()),
            "transcript_messages": self.messages.len(),
            "feature_projection": self.workspace_layout.tab.as_str(),
        })
    }

    /// Atomically switch the visible project and return its host metadata.
    /// The host can use the returned projection to bind the Agent/session.
    pub fn select_project_view(&mut self, index: usize) -> Option<serde_json::Value> {
        if !self.select_project_tab(index) {
            return None;
        }
        Some(self.project_view())
    }

    /// Return a stable snapshot suitable for a host-side project switch.
    pub fn project_tab_snapshot(&self, name: &str) -> Option<serde_json::Value> {
        self.project_tabs.iter().any(|item| item == name).then(|| {
            serde_json::json!({
                "name": name,
                "metadata": self.project_metadata(name),
                "has_transcript": self.project_transcripts.get(name).is_some_and(|m| !m.is_empty()),
                "has_layout": self.project_layouts.contains_key(name),
            })
        })
    }

    pub fn project_name(&self) -> &str {
        &self.project_name
    }

    pub fn project_metadata(&self, name: &str) -> Option<&ProjectTabMetadata> {
        self.project_metadata.get(name)
    }

    /// Attach host-owned cwd/session identity to a project tab. The TUI does
    /// not open sessions or change directories; it stores the projection so
    /// the host can restore the correct project context on selection.
    pub fn set_project_metadata(&mut self, name: impl Into<String>, metadata: ProjectTabMetadata) {
        let name = name.into();
        if self.project_tabs.iter().any(|item| item == &name)
            && self.project_metadata.get(&name) != Some(&metadata)
        {
            if name == self.active_project() {
                self.touch_editor_draft();
            } else if let Some(draft) = self.project_drafts.get_mut(&name) {
                draft.draft_epoch.revision =
                    draft.draft_epoch.revision.and_then(|n| n.checked_add(1));
            }
            self.project_metadata.insert(name, metadata);
            self.project_checkpoint_dirty = true;
            self.dirty = true;
        }
    }

    pub fn set_active_project_metadata(&mut self, metadata: ProjectTabMetadata) {
        let name = self.active_project().to_owned();
        self.set_project_metadata(name, metadata);
    }

    /// Bind durable journal identity to the selected project. This is an
    /// evidence cursor, not a transport acknowledgement or permission grant.
    pub fn set_project_session_cursor(
        &mut self,
        name: impl Into<String>,
        session_id: impl Into<String>,
        next_sequence: u64,
    ) {
        let name = name.into();
        let session_id = session_id.into();
        if self.project_tabs.iter().any(|item| item == &name)
            && !session_id.is_empty()
            && session_id.len() <= 128
        {
            if self
                .project_session_cursors
                .get(&name)
                .is_none_or(|old| old.session_id != session_id)
            {
                if name == self.active_project() {
                    self.touch_editor_draft();
                } else if let Some(draft) = self.project_drafts.get_mut(&name) {
                    draft.draft_epoch.revision =
                        draft.draft_epoch.revision.and_then(|n| n.checked_add(1));
                }
            }
            let cursor = ProjectSessionCursor {
                session_id,
                next_sequence,
            };
            if self.project_session_cursors.get(&name) != Some(&cursor) {
                self.project_session_cursors.insert(name, cursor);
                self.project_checkpoint_dirty = true;
            }
        }
    }

    pub fn project_session_cursor(&self, name: &str) -> Option<(&str, u64)> {
        self.project_session_cursors
            .get(name)
            .map(|cursor| (cursor.session_id.as_str(), cursor.next_sequence))
    }

    /// Borrow the layout model used by the production workspace renderer.
    ///
    /// Keeping this as a typed model lets hosts inspect or adjust bounded
    /// ratios/capabilities without coupling themselves to Ratatui rectangles.
    pub fn workspace_layout(&self) -> &LayoutModel {
        &self.workspace_layout
    }

    /// The project tab is the only top-level workspace scope. Feature
    /// presets may exist for migration, but they are never project identity.
    pub fn active_project_workspace(&self) -> &LayoutModel {
        &self.workspace_layout
    }

    /// Return a bounded snapshot of every persisted pane preset. Project tabs
    /// still own the visible workspace; legacy feature presets remain stored
    /// only for migration and compatibility.
    pub fn workspace_layout_states(&self) -> Vec<LayoutModel> {
        let mut states = Vec::with_capacity(TabId::ALL.len());
        states.push(self.workspace_layout.clone());
        states.extend(self.workspace_layouts.values().cloned());
        states
    }

    /// Restore saved tab models before entering the production event loop.
    /// Unknown tabs are impossible through the typed API; duplicate entries
    /// deterministically use the last one. Host capabilities are applied later
    /// because they are not portable user preferences.
    pub fn restore_workspace_layouts<I>(&mut self, layouts: I)
    where
        I: IntoIterator<Item = LayoutModel>,
    {
        let capabilities = self.workspace_layout.capabilities;
        self.workspace_layouts.clear();
        self.workspace_layout = LayoutModel::new(TabId::Project);
        self.workspace_layout.set_capabilities(capabilities);
        for mut layout in layouts {
            layout.set_capabilities(capabilities);
            if layout.tab == TabId::Project {
                self.workspace_layout = layout;
            } else {
                self.workspace_layouts.insert(layout.tab, layout);
            }
        }
        self.layout_dirty = false;
        self.layout_resets.clear();
        self.dirty = true;
    }

    /// Whether a user-owned layout mutation should be persisted by a host.
    pub fn layout_dirty(&self) -> bool {
        self.layout_dirty
    }

    /// Clear the persistence bit after a successful save. Rendering dirtiness
    /// is intentionally independent and is not affected by this operation.
    pub fn clear_layout_dirty(&mut self) {
        self.layout_dirty = false;
    }

    /// Return tabs whose persisted state should be removed on the next save.
    pub fn layout_reset_tabs(&self) -> Vec<TabId> {
        self.layout_resets.iter().copied().collect()
    }

    /// Clear reset intents after a successful persistence checkpoint.
    pub fn clear_layout_reset_tabs(&mut self) {
        self.layout_resets.clear();
    }

    /// Mark a bounded resource refresh as admitted by the host. The actual
    /// filesystem walk runs outside the terminal thread in production.
    pub fn resource_refresh_started(&mut self) {
        self.resource_status = ResourcePaneStatus::Collecting;
        self.dirty = true;
    }

    /// Publish a completed resource snapshot for the Resources pane.
    pub fn set_resource_snapshot(&mut self, snapshot: crate::resources::ResourceSnapshot) {
        self.resource_snapshot = Some(snapshot);
        self.resource_status = ResourcePaneStatus::Ready;
        self.dirty = true;
    }

    /// Publish a bounded collection failure while retaining the last good
    /// snapshot, when one exists.
    pub fn resource_refresh_failed(&mut self, error: impl AsRef<str>) {
        self.resource_status = ResourcePaneStatus::Failed(inline_token(error.as_ref(), 256));
        self.dirty = true;
    }

    /// Borrow the last completed snapshot. Embedders can use this to confirm
    /// that a refresh reached the UI without coupling to rendered text.
    pub fn resource_snapshot(&self) -> Option<&crate::resources::ResourceSnapshot> {
        self.resource_snapshot.as_ref()
    }

    /// Publish the active conversation's context usage so the resource monitor
    /// can show the opencode-style `context` column. Either side may be absent
    /// independently; the pane then reports `context unavailable`.
    pub fn set_context_usage(&mut self, used_tokens: Option<u64>, limit_tokens: Option<u64>) {
        self.transcript_ux.history_tokens = used_tokens;
        self.transcript_ux.context_limit = limit_tokens;
        self.dirty = true;
    }

    /// Mark a Gantt refresh as admitted by the production host.
    pub fn gantt_refresh_started(&mut self) {
        self.gantt_status = GanttPaneStatus::Refreshing;
        self.dirty = true;
    }

    /// Drop a projection owned by a session which is no longer active. A
    /// session switch must not leave the prior session's domain data visible
    /// while its replacement snapshot is loading.
    pub fn clear_gantt_snapshot(&mut self) {
        self.gantt_snapshot = None;
        self.gantt_status = GanttPaneStatus::Idle;
        self.dirty = true;
    }

    /// Publish one completed, bounded domain projection.
    pub fn set_gantt_snapshot(&mut self, snapshot: GanttPaneSnapshot) {
        self.gantt_snapshot = Some(snapshot);
        self.gantt_status = GanttPaneStatus::Ready;
        self.dirty = true;
    }

    /// Keep the last valid Gantt snapshot visible when a later refresh fails.
    pub fn gantt_refresh_failed(&mut self, error: impl AsRef<str>) {
        self.gantt_status = GanttPaneStatus::Failed(inline_token(error.as_ref(), 256));
        self.dirty = true;
    }

    pub fn gantt_snapshot(&self) -> Option<&GanttPaneSnapshot> {
        self.gantt_snapshot.as_ref()
    }

    /// Return the currently focused BentoBox pane.
    pub fn focused_workspace_pane(&self) -> Option<PaneId> {
        self.workspace_layout.focused_pane()
    }

    /// Move focus to the next pane in the active workspace.  Before the first
    /// frame is drawn there is no measured viewport, so use the wide preset as
    /// a conservative default; a subsequent resize/render applies the real
    /// breakpoint without losing the selected pane.
    pub fn focus_next_workspace_pane(&mut self) -> Option<PaneId> {
        let (width, height) = self.workspace_viewport();
        let focused = self.workspace_layout.focus_next(width, height);
        if focused.is_some() {
            self.layout_dirty = true;
            self.dirty = true;
        }
        focused
    }

    /// Move focus to the previous pane in the active workspace.
    pub fn focus_previous_workspace_pane(&mut self) -> Option<PaneId> {
        let (width, height) = self.workspace_viewport();
        let focused = self.workspace_layout.focus_previous(width, height);
        if focused.is_some() {
            self.layout_dirty = true;
            self.dirty = true;
        }
        focused
    }

    /// Move focus geometrically in the active workspace.
    pub fn focus_workspace_direction(&mut self, direction: FocusDirection) -> Option<PaneId> {
        let (width, height) = self.workspace_viewport();
        let focused = self
            .workspace_layout
            .focus_direction(direction, width, height);
        if focused.is_some() {
            self.layout_dirty = true;
            self.dirty = true;
        }
        focused
    }

    /// Nudge the focused column split by one bounded keyboard step.
    pub fn adjust_workspace_split(&mut self, direction: FocusDirection) -> bool {
        let (width, height) = self.workspace_viewport();
        let changed = self
            .workspace_layout
            .adjust_focused_split(direction, width, height);
        if changed {
            self.layout_dirty = true;
            self.dirty = true;
        }
        changed
    }

    /// Reset ratios, collapsed panes, and focus to the active tab preset.
    pub fn reset_workspace_layout(&mut self) {
        let tab = self.workspace_layout.tab;
        self.workspace_layout.reset_layout();
        self.layout_resets.insert(tab);
        self.layout_dirty = true;
        self.dirty = true;
    }

    fn workspace_viewport(&self) -> (u16, u16) {
        if self.last_area.width == 0 && self.last_area.height == 0 {
            // A wide fallback keeps all required panes keyboard reachable
            // before the first draw.  It is never used after a real frame.
            (160, 40)
        } else {
            // Navigation must use the same measured content area as the
            // renderer, after the tabs, header, prompt and footer are laid out.
            (self.workspace_area.width, self.workspace_area.height)
        }
    }

    /// Select an internal pane preset for compatibility. This does not alter
    /// the top-level project tab identity.
    pub fn set_workspace_tab(&mut self, tab: TabId) {
        if self.workspace_layout.tab != tab {
            let capabilities = self.workspace_layout.capabilities;
            let current = std::mem::replace(
                &mut self.workspace_layout,
                LayoutModel::new(tab).with_capabilities(capabilities),
            );
            self.workspace_layouts.insert(current.tab, current);
            self.workspace_layout = self
                .workspace_layouts
                .remove(&tab)
                .unwrap_or_else(|| LayoutModel::new(tab).with_capabilities(capabilities));
            self.workspace_layout.set_capabilities(capabilities);
            self.project_checkpoint_dirty = true;
            self.dirty = true;
        }
    }

    /// Select a pane in the active BentoBox tab. A pane that was collapsed is
    /// expanded before focus is assigned; unavailable optional adapters are
    /// rejected without mutating layout state.
    pub fn focus_workspace_pane(&mut self, pane: PaneId) -> bool {
        let valid = self
            .workspace_layout
            .preset()
            .panes
            .into_iter()
            .find(|spec| spec.id == pane)
            .is_some_and(|spec| {
                !spec.optional || pane_available(pane, self.workspace_layout.capabilities)
            });
        if !valid {
            return false;
        }
        let was_focused = self.workspace_layout.focused == Some(pane);
        let was_collapsed = self.workspace_layout.collapsed.remove(&pane);
        let changed = !was_focused || was_collapsed;
        self.workspace_layout.focused = Some(pane);
        if changed {
            self.layout_dirty = true;
            self.dirty = true;
        }
        true
    }

    /// Set the collapsed state of one pane in the active tab. The pane must be
    /// part of the tab preset; unknown panes and unavailable optional panes
    /// are rejected so a slash command cannot poison persisted preferences.
    pub fn set_workspace_pane_collapsed(&mut self, pane: PaneId, collapsed: bool) -> bool {
        let valid = self
            .workspace_layout
            .preset()
            .panes
            .into_iter()
            .find(|spec| spec.id == pane)
            .is_some_and(|spec| {
                !spec.optional || pane_available(pane, self.workspace_layout.capabilities)
            });
        if !valid {
            return false;
        }
        let was_collapsed = self.workspace_layout.collapsed.contains(&pane);
        if was_collapsed == collapsed {
            return true;
        }
        self.workspace_layout.set_collapsed(pane, collapsed);
        if collapsed && self.workspace_layout.focused == Some(pane) {
            let (width, height) = self.workspace_viewport();
            let _ = self.workspace_layout.focus_next(width, height);
            if self
                .workspace_layout
                .focused
                .is_some_and(|focused| focused == pane)
            {
                self.workspace_layout.focused = None;
            }
        }
        self.layout_dirty = true;
        self.dirty = true;
        true
    }

    /// Toggle one pane's collapsed state and return its new state. `None`
    /// means the pane is not present/available in the active preset.
    pub fn toggle_workspace_pane(&mut self, pane: PaneId) -> Option<bool> {
        let valid = self
            .workspace_layout
            .preset()
            .panes
            .into_iter()
            .find(|spec| spec.id == pane)
            .is_some_and(|spec| {
                !spec.optional || pane_available(pane, self.workspace_layout.capabilities)
            });
        if !valid {
            return None;
        }
        let collapsed = !self.workspace_layout.collapsed.contains(&pane);
        self.set_workspace_pane_collapsed(pane, collapsed);
        Some(collapsed)
    }

    /// Request an explicit layout persistence checkpoint. Production TUI
    /// persistence runs at the next event-loop checkpoint; this method keeps
    /// `/layout save` deterministic without writing from the command parser.
    pub fn request_layout_save(&mut self) {
        self.layout_dirty = true;
        self.dirty = true;
    }

    /// Return a small JSON-safe layout projection for slash responses and
    /// diagnostics. Geometry remains computed from the terminal-independent
    /// model and is bounded to the current viewport.
    pub fn workspace_layout_summary(&self) -> serde_json::Value {
        let (width, height) = self.workspace_viewport();
        let snapshot = self.workspace_layout.compute(width, height);
        serde_json::json!({
            "tab": self.workspace_layout.tab,
            "ratios": self.workspace_layout.ratios,
            "focused": self.workspace_layout.focused,
            "collapsed": self.workspace_layout.collapsed,
            "breakpoint": snapshot.breakpoint,
            "visible_panes": snapshot.visible_panes().map(|pane| pane.id).collect::<Vec<_>>(),
            "layout_dirty": self.layout_dirty,
        })
    }

    /// Set optional pane capabilities for a host that has an external adapter.
    /// Browser and PTY panes remain disabled by default in the binary.
    pub fn set_workspace_capabilities(&mut self, capabilities: crate::layout::PaneCapabilities) {
        self.workspace_layout.set_capabilities(capabilities);
        for layout in self.workspace_layouts.values_mut() {
            layout.set_capabilities(capabilities);
        }
        self.dirty = true;
    }

    /// Return whether tool lifecycle entries are currently collapsed.
    pub fn tool_logs_folded(&self) -> bool {
        self.fold_tool_logs
    }

    /// Set the tool-log folding state and invalidate the transcript cache.
    pub fn set_tool_logs_folded(&mut self, folded: bool) {
        if self.fold_tool_logs != folded {
            self.fold_tool_logs = folded;
            self.cached_transcript = None;
            self.scroll = 0;
            self.dirty = true;
        }
    }

    /// Toggle tool-log folding and return the new state.
    pub fn toggle_tool_logs(&mut self) -> bool {
        let folded = !self.fold_tool_logs;
        self.set_tool_logs_folded(folded);
        folded
    }

    /// Add or update a normalized tool lifecycle entry.
    ///
    /// A call ID is used as a stable key, allowing the later `ToolResult`
    /// event to update the original line instead of appending duplicate
    /// messages.  Input is sanitized and bounded because provider data is
    /// displayed in a terminal context.
    pub fn tool_call_started(&mut self, call_id: impl AsRef<str>, name: impl AsRef<str>) {
        self.upsert_tool_log(call_id.as_ref(), name.as_ref(), ToolRunStatus::Running);
    }

    /// Mark a tool call as completed, failed, or cancelled.
    pub fn tool_call_finished(&mut self, call_id: impl AsRef<str>, status: ToolRunStatus) {
        let call_id = inline_token(call_id.as_ref(), 96);
        let key = tool_log_key(&call_id);
        let position = self.messages.iter().rposition(|message| {
            message.role == MessageRole::Tool && tool_log_matches(&message.text, &key)
        });
        let name = position
            .and_then(|index| self.messages.get(index))
            .and_then(|message| tool_log_name(&message.text, &key))
            .unwrap_or_else(|| "unknown".into());
        self.upsert_tool_log(&call_id, &name, status);
    }

    /// Resolve any entries that were still running when a turn was stopped.
    /// This prevents a cancelled or failed request from leaving a misleading
    /// permanent "running" marker in the transcript.
    pub fn finish_running_tools(&mut self, status: ToolRunStatus) {
        let terminal = match status {
            ToolRunStatus::Running => crate::view_model::ToolStatus::Running,
            ToolRunStatus::Succeeded => crate::view_model::ToolStatus::Succeeded,
            ToolRunStatus::Failed => crate::view_model::ToolStatus::Failed,
            ToolRunStatus::Cancelled => crate::view_model::ToolStatus::Cancelled,
        };
        for message in &mut self.messages {
            for block in &mut message.blocks {
                if let crate::view_model::ViewBlock::ToolStatus { status, .. } = block
                    && *status == crate::view_model::ToolStatus::Running
                {
                    *status = terminal;
                    self.project_checkpoint_dirty = true;
                    self.dirty = true;
                    self.cached_transcript = None;
                }
            }
        }
        let running = self
            .messages
            .iter()
            .filter(|message| {
                message.role == MessageRole::Tool && message.text.ends_with(" [running]")
            })
            .map(|message| message.text.clone())
            .collect::<Vec<_>>();
        for message in running {
            if let Some((key, rest)) = message.split_once(' ') {
                let name = rest
                    .strip_suffix(" [running]")
                    .unwrap_or("unknown")
                    .to_owned();
                if let Some(call_id) = key
                    .strip_prefix("[tool:")
                    .and_then(|value| value.strip_suffix(']'))
                {
                    self.upsert_tool_log(call_id, &name, status);
                }
            }
        }
    }

    fn upsert_tool_log(&mut self, call_id: &str, name: &str, status: ToolRunStatus) {
        let call_id = inline_token(call_id, 96);
        let name = inline_token(name, 160);
        let key = tool_log_key(&call_id);
        let text = format!("{key} {name} [{}]", status.label());
        if let Some(index) = self.messages.iter().rposition(|message| {
            message.role == MessageRole::Tool && tool_log_matches(&message.text, &key)
        }) {
            if let Some(message) = self.messages.get_mut(index)
                && message.text != text
            {
                message.text = bound_text(text);
                self.cached_transcript = None;
                self.project_checkpoint_dirty = true;
                self.dirty = true;
            }
            return;
        }
        self.push_message(MessageRole::Tool, text);
    }

    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    pub fn take_dirty(&mut self) -> bool {
        let dirty = self.dirty;
        self.dirty = false;
        dirty
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        let status = bound_text(status.into());
        if self.status != status {
            self.status = status;
            self.dirty = true;
        }
    }

    pub fn set_busy(&mut self, busy: bool) {
        if self.busy != busy {
            self.busy = busy;
            if busy {
                self.transcript_ux.elapsed = Duration::ZERO;
                self.transcript_ux.activity_ended = false;
                self.sync_activity_timer();
            } else {
                self.end_activity_timer();
            }
            if !busy && self.status == "Working" {
                self.status = "Ready".into();
            }
            self.dirty = true;
        }
    }

    pub fn tick(&mut self) {
        if self.busy {
            self.spinner_tick = self.spinner_tick.wrapping_add(1);
            self.dirty = true;
        }
    }

    pub fn set_max_history(&mut self, limit: usize) {
        self.max_history = limit;
        while self.history.len() > limit {
            self.history.pop_front();
            self.project_checkpoint_dirty = true;
        }
    }

    pub fn clear_messages(&mut self) {
        if !self.messages.is_empty()
            || self.streaming_role.is_some()
            || self.streaming_job_id.is_some()
        {
            self.messages.clear();
            self.transcript_ux.scroll_anchor = None;
            self.transcript_ux.anchored_position = None;
            self.transcript_ux.message_offset = 0;
            self.transcript_ux.reasoning_job = None;
            self.streaming_role = None;
            self.streaming_job_id = None;
            self.streaming_message_started = false;
            self.streaming_block = None;
            self.cached_transcript = None;
            self.scroll = 0;
            self.project_checkpoint_dirty = true;
            self.dirty = true;
        }
    }

    pub fn push_message(&mut self, role: MessageRole, text: impl Into<String>) {
        self.messages.push_back(TuiMessage::new(role, text));
        self.cached_transcript = None;
        while self.messages.len() > self.max_messages {
            self.messages.pop_front();
            self.transcript_ux.message_offset = self.transcript_ux.message_offset.saturating_add(1);
        }
        self.project_checkpoint_dirty = true;
        self.dirty = true;
    }

    pub fn push_message_block(
        &mut self,
        role: MessageRole,
        text: impl Into<String>,
        block: crate::view_model::ViewBlock,
    ) {
        let mut message = TuiMessage::new(role, text);
        if block.validate().is_ok() {
            message.blocks.push(block);
        }
        self.messages.push_back(message);
        self.cached_transcript = None;
        while self.messages.len() > self.max_messages {
            self.messages.pop_front();
            self.transcript_ux.message_offset = self.transcript_ux.message_offset.saturating_add(1);
        }
        self.project_checkpoint_dirty = true;
        self.dirty = true;
    }

    /// Append an event to the Goal hot-zone transcript. Goal events are kept
    /// separate from ordinary conversation messages so a future owner can
    /// stream execution progress without contaminating the chat lane.
    pub fn push_goal_message(&mut self, role: MessageRole, text: impl Into<String>) {
        self.goal_messages.push_back(TuiMessage::new(role, text));
        while self.goal_messages.len() > self.max_messages {
            self.goal_messages.pop_front();
        }
        self.dirty = true;
    }

    /// Number of messages currently projected in the Goal hot zone.
    pub fn goal_message_count(&self) -> usize {
        self.goal_messages.len()
    }

    /// Current inline Goal label shown in the top-left conversation group.
    pub fn goal_text(&self) -> &str {
        &self.goal_text
    }

    /// Whether the inline Goal editor currently owns keyboard input.
    pub fn goal_edit_active(&self) -> bool {
        self.goal_edit.is_some()
    }

    /// Current editor contents while the inline Goal editor is open.
    pub fn goal_edit_buffer(&self) -> Option<&str> {
        self.goal_edit
            .as_ref()
            .map(crate::view_model::GoalEdit::text)
    }

    /// Enter the inline Goal editor seeded with the committed Goal.  Returns
    /// `false` only if the retained Goal is itself out of bounds.
    pub fn begin_goal_edit(&mut self) -> bool {
        match crate::view_model::GoalEdit::begin(self.goal_text.clone()) {
            Ok(edit) => {
                self.goal_edit = Some(edit);
                self.dirty = true;
                true
            }
            Err(error) => {
                self.set_status(format!("Goal not editable: {error}"));
                false
            }
        }
    }

    /// Abandon the inline Goal editor without changing the committed Goal.
    pub fn cancel_goal_edit(&mut self) -> bool {
        let active = self.goal_edit.take().is_some();
        if active {
            self.dirty = true;
        }
        active
    }

    /// Commit the inline Goal editor.  On success the bounded Goal is retained
    /// on the state and queued for the domain owner via
    /// [`Self::take_goal_edit_intent`].  An empty Goal is rejected and the
    /// editor stays open so the user can correct it.
    pub fn commit_goal_edit(&mut self) -> Option<String> {
        let edit = self.goal_edit.as_ref()?;
        let committed = match edit.commit() {
            Ok(text) => text,
            Err(error) => {
                self.set_status(format!("Goal not saved: {error}"));
                return None;
            }
        };
        self.goal_edit = None;
        self.goal_text = committed.clone();
        self.goal_edit_intent = Some(committed.clone());
        self.push_message(MessageRole::System, format!("Goal updated: {committed}"));
        self.dirty = true;
        Some(committed)
    }

    /// Drain the last committed Goal for the host to persist through the
    /// domain/external owner. The view layer never writes storage itself.
    pub fn take_goal_edit_intent(&mut self) -> Option<String> {
        self.goal_edit_intent.take()
    }

    /// Which left-column prompt currently owns keyboard focus (ZS1-148).
    pub fn left_prompt(&self) -> LeftPrompt {
        self.left_prompt
    }

    /// Focus a left-column prompt without mutating either draft.
    pub fn set_left_prompt(&mut self, prompt: LeftPrompt) -> bool {
        if self.left_prompt == prompt {
            return false;
        }
        self.left_prompt = prompt;
        self.dirty = true;
        true
    }

    /// Toggle between the top-left discussion prompt and the lower-left arch
    /// console. Bound to Alt-M in the production key map.
    pub fn toggle_left_prompt(&mut self) -> LeftPrompt {
        let next = match self.left_prompt {
            LeftPrompt::Discussion => LeftPrompt::Arch,
            LeftPrompt::Arch => LeftPrompt::Discussion,
        };
        self.left_prompt = next;
        self.dirty = true;
        next
    }

    /// The model-selecting zone owned by the focused left prompt (ZS1-152).
    pub fn focused_zone(&self) -> crate::view_model::Zone {
        match self.left_prompt {
            LeftPrompt::Discussion => crate::view_model::Zone::Discussion,
            LeftPrompt::Arch => crate::view_model::Zone::Arch,
        }
    }

    /// Durable per-zone model overrides held by this view (ZS1-152).
    pub fn zone_models(&self) -> &crate::view_model::ZoneModels {
        &self.zone_models
    }

    /// Stored model for one zone, if it owns an explicit selection. Workers
    /// never store a model.
    pub fn zone_model(&self, zone: crate::view_model::Zone) -> Option<&str> {
        self.zone_models.get(zone)
    }

    /// Effective model for one zone: an explicit discussion/arch override wins,
    /// otherwise the global model is used. Workers always use the global model.
    pub fn effective_zone_model<'a>(
        &'a self,
        zone: crate::view_model::Zone,
        global_model: Option<&'a str>,
    ) -> Option<&'a str> {
        self.zone_models.effective(zone, global_model)
    }

    /// Set or clear the model selection for one zone. The worker pool is
    /// rejected: its model is the project/global model. An invalid identity
    /// changes nothing.
    pub fn set_zone_model(
        &mut self,
        zone: crate::view_model::Zone,
        model: Option<String>,
    ) -> Result<(), crate::view_model::ViewModelError> {
        let mut next = self.zone_models.clone();
        next.set(zone, model)?;
        self.zone_models = next;
        self.dirty = true;
        Ok(())
    }

    /// Replace every zone override from a durable snapshot (restart/checkpoint
    /// restore). Validation is atomic: an invalid snapshot changes nothing.
    pub fn restore_zone_models(
        &mut self,
        models: crate::view_model::ZoneModels,
    ) -> Result<bool, crate::view_model::ViewModelError> {
        models.validate()?;
        let changed = self.zone_models != models;
        self.zone_models = models;
        if changed {
            self.dirty = true;
        }
        Ok(changed)
    }

    /// Concurrency quota for one zone. Discussion and arch are single
    /// concurrency; the worker pool runs at the active project's layer-2 count.
    pub fn zone_concurrency(&self, zone: crate::view_model::Zone) -> u32 {
        zone.concurrency(self.worker_concurrency())
    }

    /// Project-defined background worker count for the active layer-2
    /// workspace. Falls back to one when no explicit workspace is selected.
    pub fn worker_concurrency(&self) -> u32 {
        let project = self.active_project();
        let index = self.active_subtab.get(project).copied().unwrap_or(0);
        self.project_subtabs
            .get(project)
            .and_then(|tabs| tabs.get(index))
            .map(|tab| u32::from(tab.concurrency))
            .unwrap_or(1)
    }

    /// Arch console transcript, oldest first.
    pub fn arch_messages(&self) -> impl Iterator<Item = &TuiMessage> {
        self.arch_messages.iter()
    }

    pub fn arch_message_count(&self) -> usize {
        self.arch_messages.len()
    }

    /// Append one bounded row to the arch console transcript. This records
    /// operator intent and host results; it is not a model transcript.
    pub fn push_arch_message(&mut self, role: MessageRole, text: impl Into<String>) {
        let text = bound_text(text.into());
        if text.is_empty() {
            return;
        }
        self.arch_messages.push_back(TuiMessage::new(role, text));
        while self.arch_messages.len() > self.max_messages {
            self.arch_messages.pop_front();
        }
        self.pane_scroll.remove(&PaneId::Arch);
        self.dirty = true;
    }

    pub fn arch_input(&self) -> &str {
        &self.arch_input
    }

    /// Replace the arch draft (used by hosts restoring a checkpoint and by
    /// tests). The value is bounded by the master-session budget.
    pub fn set_arch_input(&mut self, text: impl Into<String>) {
        let mut text = text.into();
        if text.len() > crate::tool_runtime::MAX_MASTER_SESSION_INPUT_BYTES {
            let mut boundary = crate::tool_runtime::MAX_MASTER_SESSION_INPUT_BYTES;
            while !text.is_char_boundary(boundary) {
                boundary -= 1;
            }
            text.truncate(boundary);
        }
        self.arch_cursor = text.len();
        self.arch_input = text;
        self.dirty = true;
    }

    /// Whether the master session still has an active turn.
    pub fn master_busy(&self) -> bool {
        self.master_busy
    }

    /// Set the master-session concurrency flag. Hosts use this when a job that
    /// was not started through [`Self::submit_arch_prompt`] reaches a terminal
    /// boundary, so the arch console can never stay permanently busy.
    pub fn set_master_busy(&mut self, busy: bool) {
        self.master_busy = busy;
    }

    /// Mark the master turn finished and record its bounded outcome in the arch
    /// transcript. Called by the owner host after a bash/steer turn terminates.
    pub fn complete_master_turn(&mut self, role: MessageRole, text: impl Into<String>) {
        self.master_busy = false;
        self.push_arch_message(role, text);
    }

    /// Drain the last classified arch submission. The view layer only
    /// classifies; the host owns execution.
    pub fn take_arch_intent(&mut self) -> Option<MasterSessionCommand> {
        self.arch_intent.take()
    }

    /// Classify and submit the current arch draft (ZS1-148).
    ///
    /// The master session is single-concurrency: a `!command` bash submission is
    /// rejected while a master turn is active, because the shell owner requires
    /// an idle session. A steering instruction is allowed during an active turn
    /// and joins it rather than forking a second worker. The draft is consumed
    /// atomically: a rejected submission keeps it intact.
    pub fn submit_arch_prompt(&mut self) -> Result<TuiAction, MasterSessionInputError> {
        let command = classify_master_session_input(&self.arch_input)?;
        if command.is_bash() && self.master_busy {
            return Err(MasterSessionInputError::Busy);
        }
        let display = command.display();
        if !display.trim().is_empty() {
            self.push_arch_message(MessageRole::User, display);
        }
        self.arch_input.clear();
        self.arch_cursor = 0;
        self.master_busy = true;
        self.arch_intent = Some(command.clone());
        self.dirty = true;
        Ok(TuiAction::SubmitArch(command))
    }

    fn goal_edit_key(&mut self, key: KeyEvent) -> TuiAction {
        match key.code {
            KeyCode::Esc => {
                self.cancel_goal_edit();
                TuiAction::Redraw
            }
            KeyCode::Enter => {
                self.commit_goal_edit();
                TuiAction::Redraw
            }
            KeyCode::Backspace => {
                if let Some(edit) = self.goal_edit.as_mut() {
                    edit.pop_char();
                }
                self.dirty = true;
                TuiAction::Redraw
            }
            KeyCode::Char(character)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(edit) = self.goal_edit.as_mut() {
                    let _ = edit.push_char(character);
                }
                self.dirty = true;
                TuiAction::Redraw
            }
            _ => TuiAction::None,
        }
    }

    /// Whether the arch console should consume this key. Only editing keys are
    /// captured; Ctrl/Alt chords (interrupt, tab switching, pane focus) keep
    /// their existing meaning so arch focus never traps the terminal.
    fn arch_prompt_captures(&self, key: KeyEvent) -> bool {
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return false;
        }
        matches!(
            key.code,
            KeyCode::Char(_)
                | KeyCode::Backspace
                | KeyCode::Delete
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::Enter
                | KeyCode::Esc
        )
    }

    /// Bounded editor for the lower-left arch console (ZS1-148). Enter submits
    /// (Shift-Enter inserts a newline), Esc returns focus to the discussion
    /// prompt while keeping the draft, and every edit stays within the
    /// master-session byte budget.
    fn arch_prompt_key(&mut self, key: KeyEvent) -> TuiAction {
        let max = crate::tool_runtime::MAX_MASTER_SESSION_INPUT_BYTES;
        match key.code {
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                if self.arch_input.len() < max {
                    let at = self.arch_cursor.min(self.arch_input.len());
                    self.arch_input.insert(at, '\n');
                    self.arch_cursor = at + 1;
                }
            }
            KeyCode::Enter => match self.submit_arch_prompt() {
                Ok(action) => return action,
                Err(error) => {
                    self.set_status(format!("Arch console: {error}"));
                }
            },
            KeyCode::Esc => {
                self.left_prompt = LeftPrompt::Discussion;
                self.set_status("Arch console unfocused · Alt-M to return");
            }
            KeyCode::Char(character) => {
                let width = character.len_utf8();
                if self.arch_input.len().saturating_add(width) <= max {
                    let at = self.arch_cursor.min(self.arch_input.len());
                    if self.arch_input.is_char_boundary(at) {
                        self.arch_input.insert(at, character);
                        self.arch_cursor = at + width;
                    }
                }
            }
            KeyCode::Backspace => {
                if self.arch_cursor > 0 {
                    let mut at = self.arch_cursor.min(self.arch_input.len());
                    while at > 0 && !self.arch_input.is_char_boundary(at) {
                        at -= 1;
                    }
                    if at > 0 {
                        let previous = self.arch_input[..at]
                            .chars()
                            .next_back()
                            .map_or(0, char::len_utf8);
                        self.arch_input.replace_range(at - previous..at, "");
                        self.arch_cursor = at - previous;
                    }
                }
            }
            KeyCode::Delete => {
                let at = self.arch_cursor.min(self.arch_input.len());
                if at < self.arch_input.len() && self.arch_input.is_char_boundary(at) {
                    let next = self.arch_input[at..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8);
                    self.arch_input.replace_range(at..at + next, "");
                }
            }
            KeyCode::Left => {
                let mut at = self.arch_cursor.min(self.arch_input.len());
                while at > 0 && !self.arch_input.is_char_boundary(at) {
                    at -= 1;
                }
                if at > 0 {
                    at -= self.arch_input[..at]
                        .chars()
                        .next_back()
                        .map_or(0, char::len_utf8);
                }
                self.arch_cursor = at;
            }
            KeyCode::Right => {
                let mut at = self.arch_cursor.min(self.arch_input.len());
                while at < self.arch_input.len() && !self.arch_input.is_char_boundary(at) {
                    at += 1;
                }
                if at < self.arch_input.len() {
                    at += self.arch_input[at..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8);
                }
                self.arch_cursor = at;
            }
            KeyCode::Home => self.arch_cursor = 0,
            KeyCode::End => self.arch_cursor = self.arch_input.len(),
            _ => {}
        }
        self.dirty = true;
        TuiAction::Redraw
    }

    /// Replace the Goal lane when its owner changes (for example after a
    /// session switch). This prevents stale execution notices from crossing
    /// session boundaries while preserving the ordinary conversation.
    pub fn clear_goal_messages(&mut self) {
        if !self.goal_messages.is_empty() {
            self.goal_messages.clear();
            self.pane_scroll.remove(&PaneId::GoalConversation);
            self.dirty = true;
        }
    }

    /// Merge adjacent stream chunks to avoid one allocation per token.
    pub fn append_stream(&mut self, role: MessageRole, chunk: impl AsRef<str>) {
        self.streaming_job_id = None;
        self.streaming_message_started = false;
        self.streaming_block = None;
        self.append_stream_content(role, chunk.as_ref());
    }

    fn append_stream_content(&mut self, role: MessageRole, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        if let Some(last) = self.messages.back_mut()
            && last.role == role
            && last.text.len().saturating_add(chunk.len()) <= MAX_MESSAGE_BYTES
        {
            last.text.push_str(chunk);
            self.streaming_role = Some(role);
            self.cached_transcript = None;
            self.project_checkpoint_dirty = true;
            self.dirty = true;
            return;
        }
        self.push_message(role, chunk.to_owned());
        self.streaming_role = Some(role);
    }

    /// Start a stream owned by one runtime job. The first delta always starts
    /// a fresh transcript entry, even if the previous message has the same
    /// role; this prevents a replacement turn from being merged into a prior
    /// answer while an interrupted provider is still winding down.
    pub fn begin_stream_for_job(&mut self, job_id: u64) {
        if self.streaming_job_id != Some(job_id) {
            if self.streaming_job_id.is_some() {
                self.clear_activity_approvals();
            }
            self.transcript_ux.canonical_approvals.clear();
            self.transcript_ux.activity_ended = false;
            self.transcript_ux.elapsed = Duration::ZERO;
            self.transcript_ux.started = None;
            self.sync_activity_timer();
        }
        self.transcript_ux.reasoning_job = None;
        self.streaming_role = None;
        self.streaming_job_id = Some(job_id);
        self.streaming_message_started = false;
        self.streaming_block =
            crate::view_model::StreamingBlock::new(format!("job-{job_id}:assistant")).ok();
    }

    /// Append a provider delta only when it belongs to the currently active
    /// job. A stale job event is intentionally ignored rather than rendered
    /// into the replacement turn.
    pub fn append_stream_for_job(
        &mut self,
        job_id: u64,
        role: MessageRole,
        chunk: impl AsRef<str>,
    ) {
        if self.streaming_job_id != Some(job_id) || chunk.as_ref().is_empty() {
            return;
        }
        if let Some(block) = self.streaming_block.as_mut()
            && block.append(chunk.as_ref()).is_err()
        {
            return;
        }
        if !self.streaming_message_started {
            self.push_message(role, chunk.as_ref().to_owned());
            self.streaming_role = Some(role);
            self.streaming_message_started = true;
            return;
        }
        if let Some(message) = self.messages.iter_mut().rev().find(|m| m.role == role) {
            message.text.push_str(chunk.as_ref());
            self.cached_transcript = None;
            self.project_checkpoint_dirty = true;
            self.dirty = true;
            self.streaming_role = Some(role);
        } else {
            self.push_message(role, chunk.as_ref().to_owned());
            self.streaming_role = Some(role);
        }
    }

    /// Replace the provisional streamed message with the final provider
    /// content.  A provider may emit no deltas (for example a compatibility
    /// backend or a stream that was coalesced), in which case this appends one
    /// normal message.  The role marker prevents a later turn from rewriting
    /// an older assistant message.
    pub fn finish_stream(&mut self, role: MessageRole, content: impl Into<String>) {
        let content = content.into();
        if let Some(block) = self.streaming_block.as_mut() {
            let _ = block.complete(Some(&content));
        }
        self.finish_stream_impl(role, content);
        self.streaming_job_id = None;
        self.streaming_message_started = false;
        self.streaming_block = None;
    }

    fn finish_stream_impl(&mut self, role: MessageRole, content: String) {
        let content = bound_text(content);
        if self.streaming_role == Some(role)
            && let Some(index) = self
                .messages
                .iter()
                .rposition(|message| message.role == role)
            && let Some(message) = self.messages.get_mut(index)
        {
            if message.text != content {
                message.text = content;
                self.cached_transcript = None;
                self.project_checkpoint_dirty = true;
                self.dirty = true;
            }
            self.streaming_role = None;
            return;
        }
        self.streaming_role = None;
        self.push_message(role, content);
    }

    /// Finalize a stream only if the supplied runtime job still owns the
    /// provisional entry. A stale completion is discarded without changing
    /// the visible replacement answer.
    pub fn finish_stream_for_job(
        &mut self,
        job_id: u64,
        role: MessageRole,
        content: impl Into<String>,
    ) {
        if self.streaming_job_id != Some(job_id) {
            return;
        }
        let content = content.into();
        if self.streaming_message_started {
            self.finish_stream_impl(role, content);
        } else {
            self.streaming_role = None;
            self.push_message(role, content);
        }
        self.streaming_job_id = None;
        self.streaming_message_started = false;
        self.streaming_block = None;
    }

    /// Commit the current provisional stream when the canonical turn terminal
    /// event arrives. Block/TextDone events do not own turn completion because
    /// one turn may contain multiple blocks or tool iterations.
    pub fn complete_stream_for_job(&mut self, job_id: u64, role: MessageRole) {
        if self.streaming_job_id != Some(job_id) {
            return;
        }
        let content = self
            .messages
            .iter()
            .rev()
            .find(|message| message.role == role)
            .map(|message| message.text.clone())
            .unwrap_or_default();
        self.finish_stream_for_job(job_id, role, content);
    }

    /// Stop tracking a provisional stream while retaining any text already
    /// shown.  This is used for cancellation, refusal, and provider errors;
    /// dropping the marker ensures the next turn cannot rewrite the partial
    /// message.
    pub fn discard_stream(&mut self) {
        self.streaming_role = None;
        self.streaming_job_id = None;
        self.streaming_message_started = false;
        self.streaming_block = None;
    }

    /// Consume one canonical view-model event. Legacy provider/agent
    /// adapters may still feed the transcript, but this entry point keeps
    /// TUI behavior aligned with headless `ViewEvent` envelopes.
    pub fn apply_view_event_for_job(&mut self, job_id: u64, event: &crate::view_model::ViewEvent) {
        use crate::view_model::ViewEventKind;
        if self.streaming_job_id != Some(job_id) {
            return;
        }
        if matches!(
            event.event,
            ViewEventKind::TurnCompleted { .. }
                | ViewEventKind::TurnFailed { .. }
                | ViewEventKind::TurnCancelled { .. }
                | ViewEventKind::Closed
        ) {
            self.end_activity_timer();
        }
        match &event.event {
            ViewEventKind::ReasoningDelta { delta } => self.append_reasoning_for_job(job_id, delta),
            ViewEventKind::Usage {
                input_tokens,
                output_tokens,
                total_tokens,
            } => {
                if self.streaming_job_id == Some(job_id) {
                    self.transcript_ux.usage = Some(crate::backend::Usage {
                        input_tokens: *input_tokens,
                        output_tokens: *output_tokens,
                        total_tokens: *total_tokens,
                    });
                    self.dirty = true;
                }
            }
            ViewEventKind::TextDelta { delta } => {
                self.append_stream_for_job(job_id, MessageRole::Assistant, delta);
            }
            ViewEventKind::Block {
                block: crate::view_model::ViewBlock::Paragraph { text },
            } => {
                if !self.streaming_message_started {
                    self.append_stream_for_job(job_id, MessageRole::Assistant, text);
                }
            }
            ViewEventKind::Block {
                block: crate::view_model::ViewBlock::Error { message, .. },
            }
            | ViewEventKind::TurnFailed { message, .. } => {
                self.finish_stream_for_job(job_id, MessageRole::Error, message.clone());
            }
            ViewEventKind::Block {
                block:
                    block @ crate::view_model::ViewBlock::ToolStatus {
                        output: Some(output),
                        ..
                    },
            } => {
                if self.streaming_job_id != Some(job_id) {
                    return;
                }
                let Some(block_id) = event.block_id.as_ref() else {
                    return;
                };
                let key = format!("job-{job_id}:{block_id}");
                if let Some(message) = self
                    .messages
                    .iter_mut()
                    .find(|message| message.block_id.as_ref() == Some(&key))
                {
                    message.text = bound_text(output.clone());
                    message.blocks = vec![block.clone()];
                    self.cached_transcript = None;
                    self.project_checkpoint_dirty = true;
                    self.dirty = true;
                } else {
                    self.push_message_block(MessageRole::Tool, output.clone(), block.clone());
                    if let Some(message) = self.messages.back_mut() {
                        message.block_id = Some(key);
                    }
                }
            }
            ViewEventKind::ToolStarted { call_id, name } => {
                if self.streaming_job_id != Some(job_id) {
                    return;
                }
                self.tool_call_started(call_id, name);
                self.set_status(format!("Running {}", inline_token(name, 80)));
            }
            ViewEventKind::ToolCallDelta { call_id, name, .. } => {
                if let Some(call_id) = call_id {
                    self.tool_call_started(call_id, name.as_deref().unwrap_or("unknown"));
                } else {
                    self.set_status("Preparing tool call");
                }
            }
            ViewEventKind::ToolCallReady { call_id, name, .. } => {
                self.tool_call_started(call_id, name);
            }
            ViewEventKind::ToolFinished {
                call_id, status, ..
            } => {
                if self.streaming_job_id != Some(job_id) {
                    return;
                }
                let prefix = format!("job-{job_id}:");
                for message in &mut self.messages {
                    if message
                        .block_id
                        .as_ref()
                        .is_some_and(|id| id.starts_with(&prefix))
                    {
                        for block in &mut message.blocks {
                            if let crate::view_model::ViewBlock::ToolStatus {
                                call_id: id,
                                status: state,
                                ..
                            } = block
                                && id == call_id
                            {
                                *state = *status;
                                self.project_checkpoint_dirty = true;
                                self.cached_transcript = None;
                                self.dirty = true;
                            }
                        }
                    }
                }
                let status = match status {
                    crate::view_model::ToolStatus::Succeeded => ToolRunStatus::Succeeded,
                    crate::view_model::ToolStatus::Failed => ToolRunStatus::Failed,
                    crate::view_model::ToolStatus::Cancelled => ToolRunStatus::Cancelled,
                    crate::view_model::ToolStatus::Queued
                    | crate::view_model::ToolStatus::Running => ToolRunStatus::Running,
                };
                self.tool_call_finished(call_id, status);
            }
            ViewEventKind::ApprovalRequired {
                approval_id,
                tool,
                arguments,
            } => {
                if event.validate().is_err() || self.transcript_ux.activity_ended {
                    return;
                }
                let pending = &mut self.transcript_ux.canonical_approvals;
                if !pending.contains(approval_id) {
                    if pending.len() < 128 {
                        pending.push(approval_id.clone());
                    } else {
                        // Do not resume early when an untracked request remains.
                        // Saturation is cleared by cancellation/end/new job only.
                        self.transcript_ux.approval_tracking_saturated = true;
                    }
                }
                self.sync_activity_timer();
                self.push_message_block(
                    MessageRole::System,
                    format!("Approval required: {tool}\nRequest: {approval_id}"),
                    crate::view_model::ViewBlock::Approval {
                        approval_id: approval_id.clone(),
                        tool: tool.clone(),
                        arguments: arguments.clone(),
                        state: crate::view_model::ApprovalState::Pending,
                    },
                );
                self.set_status("Approval required");
            }
            ViewEventKind::ApprovalResolved { approval_id, state } => {
                if event.validate().is_err() || self.transcript_ux.activity_ended {
                    return;
                }
                if *state != crate::view_model::ApprovalState::Pending {
                    let pending = &mut self.transcript_ux.canonical_approvals;
                    let previous = pending.len();
                    pending.retain(|id| id != approval_id);
                    if pending.len() != previous {
                        self.sync_activity_timer();
                    }
                }
                self.push_message(
                    MessageRole::System,
                    format!("Approval {approval_id}: {state:?}"),
                );
            }
            ViewEventKind::Dropped { stream, count } => {
                self.push_message(
                    MessageRole::Error,
                    format!(
                        "{} stream truncated: {count} event(s) dropped",
                        stream_name(*stream)
                    ),
                );
            }
            ViewEventKind::TurnCancelled { .. } | ViewEventKind::Closed => self.discard_stream(),
            ViewEventKind::TurnCompleted { .. } => {
                self.complete_stream_for_job(job_id, MessageRole::Assistant);
            }
            _ => {}
        }
    }

    pub fn append_reasoning_for_job(&mut self, job: u64, delta: &str) {
        if self.streaming_job_id != Some(job) || delta.is_empty() {
            return;
        }
        if self.transcript_ux.reasoning_job == Some(job) {
            if let Some(message) = self
                .messages
                .iter_mut()
                .rev()
                .find(|m| m.role == MessageRole::Reasoning)
            {
                let remaining = MAX_MESSAGE_BYTES.saturating_sub(message.text.len());
                message.text.push_str(truncate_bytes(delta, remaining));
            }
        } else {
            self.push_message(MessageRole::Reasoning, delta.to_owned());
            self.transcript_ux.reasoning_job = Some(job);
        }
        self.cached_transcript = None;
        self.project_checkpoint_dirty = true;
        self.dirty = true;
    }
    pub fn toggle_reasoning(&mut self) -> bool {
        self.transcript_ux.reasoning_folded = !self.transcript_ux.reasoning_folded;
        self.cached_transcript = None;
        self.project_checkpoint_dirty = true;
        self.dirty = true;
        self.transcript_ux.reasoning_folded
    }
    pub fn follow_latest(&mut self) {
        self.transcript_ux.scroll_anchor = None;
        self.transcript_ux.anchored_position = None;
        self.scroll = 0;
        self.project_checkpoint_dirty = true;
        self.dirty = true;
    }
    pub fn open_transcript_browser(&mut self) {
        self.transcript_browser = Some(TranscriptBrowser::new(&self.messages));
        self.dirty = true;
    }
    pub fn copy_latest_answer(&self) -> Option<String> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::Assistant)
            .map(|m| m.text.clone())
    }
    fn refresh_transcript_status(&mut self, agent: &crate::core::Agent) {
        self.update_resource_catalog(&agent.resource_snapshot());
        let _ = self.update_model_catalog(agent);
        self.transcript_ux.model = agent.snapshot().model;
        let revision = (
            agent.session().session_id().to_owned(),
            agent.session().next_sequence(),
            self.busy,
        );
        if self.transcript_ux.history_revision.as_ref() == Some(&revision) {
            return;
        }
        let history = agent.selected_history().unwrap_or_default();
        self.transcript_ux.history_revision = Some(revision);
        if !self.busy {
            self.transcript_ux.response_model = history
                .iter()
                .rev()
                .filter_map(|t| t.metadata.as_ref())
                .find_map(|m| m.get("model").and_then(|v| v.as_str()))
                .map(str::to_owned);
        }
        self.transcript_ux.history_tokens =
            Some(crate::context::estimate_tokens(&history).input_tokens);
        self.transcript_ux.context_limit = Some(agent.context_budget().max_tokens);
        if !self.busy {
            self.transcript_ux.usage = history
                .iter()
                .rev()
                .filter_map(|t| t.metadata.as_ref())
                .find_map(|m| m.get("usage"))
                .and_then(|v| serde_json::from_value(v.clone()).ok());
        }
    }

    pub fn update_input_queue(&mut self, reply: &crate::input_queue::InputQueueReply) {
        use crate::input_queue::InputQueueReply;
        let inputs = match reply {
            InputQueueReply::Input { input, .. } => {
                if self.queued_inputs.iter().any(|existing| existing == input) {
                    return;
                }
                let mut inputs = self.queued_inputs.clone();
                if let Some(existing) = inputs.iter_mut().find(|item| item.id == input.id) {
                    *existing = input.clone();
                } else if inputs.len() < 32 {
                    inputs.push(input.clone());
                }
                inputs
            }
            InputQueueReply::Page { inputs: page, .. } => {
                if self.queued_inputs.iter().eq(page.iter().take(32)) {
                    return;
                }
                page.iter().take(32).cloned().collect()
            }
            InputQueueReply::Modes { .. } => return,
        };
        if self.queued_inputs == inputs {
            return;
        }
        let choices = self.slash_choices();
        let selected = choices.get(self.palette_index);
        self.queued_inputs = inputs;
        let updated = self.slash_choices();
        if updated != choices {
            self.palette_index = selected
                .and_then(|choice| updated.iter().position(|value| value == choice))
                .unwrap_or_else(|| self.palette_index.min(updated.len().saturating_sub(1)));
            // Changed choices must be painted before Tab can accept them.
            self.palette_rows = 0;
        }
        self.dirty = true;
    }

    pub fn update_model_catalog(
        &mut self,
        agent: &crate::core::Agent,
    ) -> Result<(), crate::core::AgentError> {
        let status = agent.model_status()?;
        let mut menu = ModelMenu {
            entries: serde_json::from_value(status["catalog"].clone()).unwrap_or_default(),
            active: serde_json::from_value(status["active"].clone()).ok(),
            reasoning_supported: status["capabilities"]["reasoning"].as_bool() == Some(true),
            effort: agent.reasoning_effort().map(str::to_owned),
        };
        // An explicitly configured unknown model has conservative metadata but
        // may not appear among named catalogue entries. Preserve that identity.
        if let Some(active) = &menu.active
            && !menu.entries.iter().any(|m| m.id == active.id)
        {
            menu.entries.push(active.clone());
        }
        if self.model_menu != menu {
            let choices = self.slash_choices();
            let selected = choices.get(self.palette_index);
            self.model_menu = menu;
            let updated = self.slash_choices();
            if updated != choices {
                self.palette_index = selected
                    .and_then(|choice| updated.iter().position(|value| value == choice))
                    .unwrap_or_else(|| self.palette_index.min(updated.len().saturating_sub(1)));
                self.palette_rows = 0;
            }
            self.dirty = true;
        }
        Ok(())
    }

    fn reasoning_choices(&self) -> Vec<String> {
        let Some(active) = &self.model_menu.active else {
            return Vec::new();
        };
        let mut choices = vec!["default".to_owned()];
        if self.model_menu.reasoning_supported {
            choices.extend(active.reasoning_levels.iter().filter_map(|level| {
                serde_json::to_value(level)
                    .ok()?
                    .as_str()
                    .map(str::to_owned)
            }));
        }
        choices
    }

    fn model_choice_detail(&self, choice: &str) -> Option<(String, String)> {
        if let Some(id) = choice.strip_prefix("/model ") {
            let model = self.model_menu.entries.iter().find(|m| m.id == id)?;
            let levels = model
                .reasoning_levels
                .iter()
                .filter_map(|v| serde_json::to_value(v).ok()?.as_str().map(str::to_owned))
                .collect::<Vec<_>>()
                .join(",");
            return Some((
                format!(
                    "{} · context {} · output {} · reasoning {}",
                    model.provider,
                    model.context_window,
                    model.max_output_tokens,
                    if levels.is_empty() {
                        "unavailable"
                    } else {
                        &levels
                    }
                ),
                format!(
                    "{} · {}",
                    if self.model_menu.active.as_ref().is_some_and(|m| m.id == id) {
                        "Selected"
                    } else {
                        "Registry"
                    },
                    serde_json::to_string(&model.sources).unwrap_or_default()
                ),
            ));
        }
        if choice.starts_with("/reasoning") {
            return Some((
                format!(
                    "Reasoning: {} · {}",
                    self.model_menu
                        .effort
                        .as_deref()
                        .unwrap_or("provider default"),
                    if self.model_menu.reasoning_supported {
                        "advertised levels only"
                    } else {
                        "explicit levels unavailable"
                    }
                ),
                "default omits the request field; none is an explicit model level".into(),
            ));
        }
        None
    }

    fn take_submitted_paste(&mut self, text: &str) -> Option<SubmittedPaste> {
        let digest = input_digest(text);
        self.submitted_pastes
            .iter()
            // This is only the newest, not-yet-routed Submit. Accepted work
            // carries metadata on its actual scheduled ID, job or ticket.
            .rposition(|item| item.digest == digest)
            .and_then(|index| self.submitted_pastes.remove(index))
    }

    fn apply_rejected_input(&mut self, text: String, snapshot: Option<SubmittedPaste>) {
        self.set_input(text);
        if let Some(snapshot) = snapshot {
            self.paste.next_id = self.paste.next_id.max(snapshot.paste.next_id);
            self.paste.folds = snapshot.paste.folds;
            self.cursor = snapshot.cursor.min(self.input.len());
        }
    }

    fn set_rejected_input(&mut self, text: impl Into<String>) {
        let text = text.into();
        let snapshot = self.take_submitted_paste(&text);
        self.apply_rejected_input(text, snapshot);
    }

    fn restore_rejected_input(&mut self, text: impl Into<String>) {
        let text = text.into();
        let submitted = self.bind_submitted_input(text);
        self.restore_submitted_input(submitted);
    }

    fn bind_submitted_input(&mut self, text: String) -> SubmittedInput {
        let paste = self.take_submitted_paste(&text);
        SubmittedInput { text, paste }
    }

    fn restore_submitted_input(&mut self, submitted: SubmittedInput) {
        // A held character is already a newer draft. Retire the failed
        // snapshot without changing that draft or its classification window.
        if self.input.is_empty()
            && self.ordinary_paste.buffer.is_empty()
            && !self.ordinary_paste.rejected
        {
            self.apply_rejected_input(submitted.text, submitted.paste);
        }
    }

    fn touch_editor_draft(&mut self) {
        self.draft_epoch.revision = self.draft_epoch.revision.and_then(|n| n.checked_add(1));
    }

    fn activate_draft_epoch(&mut self, epoch: DraftEpoch) -> DraftEpoch {
        if epoch.generation != 0 {
            return epoch;
        }
        let generation = self.next_draft_generation;
        self.next_draft_generation = generation.and_then(|n| n.checked_add(1));
        DraftEpoch {
            generation: generation.unwrap_or(0),
            revision: generation.map(|_| 0),
        }
    }

    /// Capture a metadata-only editor ticket. The canonical draft stays in its owner.
    pub fn begin_external_edit(&mut self) -> Result<ExternalEditToken, String> {
        self.finish_ordinary_paste();
        if self.external_edit_active.is_some() {
            return Err("An external editor already owns this terminal".into());
        }
        if self.pending_project.is_some()
            || self.directory_picker.is_some()
            || self.history_search.is_some()
            || self.transcript_browser.is_some()
            || !self.slash_choices().is_empty()
            || self
                .approval_views
                .get(self.active_project())
                .is_some_and(|v| v.focused && !v.requests.is_empty())
        {
            return Err(
                "Close the active picker or pending project change before external editing".into(),
            );
        }
        if self.draft_epoch.revision.is_none() || self.draft_epoch.generation == 0 {
            return Err(
                "Draft edit identifiers exhausted; restart the TUI before external editing".into(),
            );
        }
        let id = self
            .next_external_edit
            .ok_or("External edit identifiers exhausted")?;
        self.next_external_edit = id.checked_add(1);
        self.external_edit_active = Some(id);
        Ok(ExternalEditToken {
            id,
            project: self.active_project().to_owned(),
            metadata: self.project_metadata(self.active_project()).cloned(),
            session_id: self
                .project_session_cursor(self.active_project())
                .map(|(id, _)| id.to_owned()),
            epoch: self.draft_epoch,
        })
    }

    /// Apply only this still-current operation; failure never restores an old snapshot.
    pub fn finish_external_edit(
        &mut self,
        token: ExternalEditToken,
        result: Result<String, String>,
    ) -> Result<bool, String> {
        if self.external_edit_active != Some(token.id) {
            return Err("Stale external editor result ignored; current draft retained".into());
        }
        self.external_edit_active = None;
        let text = result?;
        if self.active_project() != token.project
            || self.draft_epoch != token.epoch
            || self.project_metadata(self.active_project()) != token.metadata.as_ref()
            || self
                .project_session_cursor(self.active_project())
                .map(|(id, _)| id)
                != token.session_id.as_deref()
            || self.pending_project.is_some()
        {
            return Err("External edit not applied: project, session or draft changed; current draft retained".into());
        }
        if text.len() > MAX_MESSAGE_BYTES {
            return Err("External edit exceeds 256 KiB; draft unchanged".into());
        }
        let text = sanitize_input_with_limit(&text, MAX_MESSAGE_BYTES + 1);
        if text.len() > MAX_MESSAGE_BYTES {
            return Err("External edit exceeds 256 KiB; draft unchanged".into());
        }
        if text == self.input {
            return Ok(false);
        }
        let paste = PasteMetadata {
            folds: Vec::new(),
            next_id: self.paste.next_id,
        };
        if !self.input_fits_checkpoint(&text, text.len(), &paste) {
            return Err(
                "External edit exceeds 4 MiB project checkpoint budget; draft unchanged".into(),
            );
        }
        self.set_input(text);
        self.palette_dismissed = true;
        self.edited_input();
        Ok(true)
    }

    pub fn set_input(&mut self, input: impl Into<String>) {
        self.touch_editor_draft();
        self.ordinary_paste = OrdinaryPasteBurst::default();
        self.history_search = None;
        self.history_draft = None;
        self.palette_dismissed = false;
        self.palette_index = 0;
        self.palette_rows = 0;
        self.input = bound_text(sanitize_input(input.into()));
        self.paste.folds.clear();
        self.cursor = self.input.len();
        self.history_cursor = None;
        self.input_scroll = 0;
        self.preferred_column = None;
        self.project_checkpoint_dirty = true;
        self.dirty = true;
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.transcript_ux.anchored_position = None;
        self.transcript_ux.scroll_anchor = Some(
            self.transcript_ux
                .scroll_anchor
                .unwrap_or(self.transcript_ux.last_start)
                .saturating_sub(amount.max(1)),
        );
        self.scroll = self.scroll.saturating_add(amount.max(1));
        self.project_checkpoint_dirty = true;
        self.dirty = true;
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.transcript_ux.anchored_position = None;
        if let Some(anchor) = self.transcript_ux.scroll_anchor.as_mut() {
            *anchor = anchor.saturating_add(amount.max(1));
        }
        self.scroll = self.scroll.saturating_sub(amount.max(1));
        if self.scroll == 0 {
            self.transcript_ux.scroll_anchor = None;
        }
        self.project_checkpoint_dirty = true;
        self.dirty = true;
    }

    pub fn file_completion_query(&self) -> Option<FileCompletionQuery> {
        if self.palette_dismissed
            || self.cursor != self.input.len()
            || self.directory_picker.is_some()
            || self.transcript_browser.is_some()
        {
            return None;
        }
        let mut masked = self.input.clone();
        for fold in &self.paste.folds {
            masked.replace_range(
                fold.start_byte..fold.end_byte,
                &" ".repeat(fold.end_byte - fold.start_byte),
            );
        }
        let input = &masked;
        for command in ["/attach ", "/diff ", "/review ", "/reload "] {
            if let Some(path) = input.strip_prefix(command) {
                if !self.paste.folds.is_empty() {
                    return None;
                }
                let raw_selection = command == "/reload ";
                let path = if raw_selection {
                    path.to_owned()
                } else {
                    path.trim_matches('"')
                        .replace("\\\"", "\"")
                        .replace("\\\\", "\\")
                };
                return Some(FileCompletionQuery {
                    root: self.project_cwd(),
                    input: self.input.clone(),
                    prefix: command.into(),
                    path,
                    raw_selection,
                });
            }
        }
        if !input.starts_with('/') {
            let (start, end, path, closed) = prompt_file_references(input).ok()?.pop()?;
            if self
                .paste
                .folds
                .iter()
                .any(|fold| start < fold.end_byte && end > fold.start_byte)
            {
                return None;
            }
            if end != input.len() {
                return None;
            }
            if input.ends_with(char::is_whitespace) && closed {
                return None;
            }
            return Some(FileCompletionQuery {
                root: self.project_cwd(),
                input: self.input.clone(),
                prefix: self.input[..start + 1].into(),
                path,
                raw_selection: false,
            });
        }
        None
    }
    pub fn apply_file_completion(
        &mut self,
        query: FileCompletionQuery,
        result: FileCompletionResult,
    ) -> bool {
        if self.file_completion_query().as_ref() != Some(&query) {
            return false;
        }
        self.file_completion = Some((query, result));
        self.palette_index = 0;
        self.palette_rows = 0;
        self.dirty = true;
        true
    }
    fn current_file_completion(&self) -> Option<&FileCompletionResult> {
        self.file_completion.as_ref().and_then(|(query, result)| {
            (self.file_completion_query().as_ref() == Some(query)).then_some(result)
        })
    }
    pub fn command_available(&self, command: &str) -> bool {
        if !self.busy || (!command.starts_with('/') && slash::spec(command).is_none()) {
            return true;
        }
        let name = command
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("");
        if self
            .command_resources
            .iter()
            .any(|r| r.command.trim_start_matches('/') == name)
        {
            return true;
        }
        matches!(
            name,
            "help"
                | "?"
                | "status"
                | "history"
                | "clear"
                | "cancel"
                | "approve"
                | "exit"
                | "quit"
                | "q"
                | "project"
                | "layout"
                | "pane"
                | "resources"
                | "skills"
                | "templates"
                | "reload"
                | "input"
                | "scheduled"
                | "compact"
                | "diff"
                | "review"
        )
    }

    pub fn update_resource_catalog(&mut self, snapshot: &crate::resource_loader::ResourceSnapshot) {
        let resources = snapshot
            .skills
            .metadata()
            .map(|m| CommandResource {
                command: format!("/skill:{}", m.name),
                description: inline_token(&m.description, 128),
                source: menu_source(&m.source.display().to_string(), 512),
                hash: m.source_hash.clone(),
            })
            .chain(
                snapshot
                    .templates
                    .templates()
                    .filter(|m| {
                        slash::spec(&m.name).is_none()
                            && !matches!(m.name.as_str(), "reasoning" | "scheduled" | "input")
                    })
                    .map(|m| CommandResource {
                        command: format!("/{}", m.name),
                        description: inline_token(&m.description, 128),
                        source: menu_source(&m.source.display().to_string(), 512),
                        hash: m.source_hash.clone(),
                    }),
            )
            .take(2048)
            .collect::<Vec<_>>();
        if self.command_resources != resources {
            self.command_resources = resources;
            self.palette_index = 0;
            self.dirty = true;
        }
    }

    /// Inspect one selected owner's metadata without loading its body or copying
    /// all full paths into the abbreviated command cache.
    pub fn inspect_resource_source(
        &mut self,
        snapshot: &crate::resource_loader::ResourceSnapshot,
        command: &str,
        expected_hash: &str,
    ) -> Result<(), String> {
        let source = if let Some(name) = command.strip_prefix("/skill:") {
            snapshot
                .skills
                .metadata()
                .find(|m| m.name == name)
                .map(|m| (&m.source, &m.source_hash))
        } else {
            snapshot
                .templates
                .templates()
                .find(|m| command.strip_prefix('/') == Some(m.name.as_str()))
                .map(|m| (&m.source, &m.source_hash))
        };
        let Some((path, hash)) = source else {
            self.update_resource_catalog(snapshot);
            return Err("Resource no longer exists; choose from the refreshed menu".into());
        };
        if hash != expected_hash {
            self.update_resource_catalog(snapshot);
            return Err("Resource changed; menu refreshed, press F2 again".into());
        }
        let path = serde_json::to_string(path).map_err(|e| e.to_string())?;
        let text = format!(
            "Resource source: {command}\nGeneration: {}\nSource: {path}\nSHA256: {hash}\n",
            snapshot.generation
        );
        if text.len() > MAX_CLIPBOARD_BYTES {
            return Err("Resource source exceeds the 64 KiB inspector budget".into());
        }
        self.transcript_browser = Some(TranscriptBrowser {
            entries: vec![CopyEntry {
                label: format!("Resource source · {command}"),
                text,
            }],
            selected: 0,
            scroll: 0,
            area: Rect::default(),
        });
        self.dirty = true;
        Ok(())
    }

    /// Cached names allow classification while the provider owns the Agent lock.
    /// Admission still resolves/hash-checks the actual resource through Agent.
    pub fn route_catalog_input(&self, text: &str) -> Result<slash::InputRoute, slash::SlashError> {
        if slash::resource_input_candidate(text) {
            let command = text.split_whitespace().next().unwrap_or("");
            if self.command_resources.iter().any(|r| r.command == command) {
                return Ok(slash::InputRoute::Prompt(text.to_owned()));
            }
        }
        slash::route_input(text)
    }

    pub fn slash_choices(&self) -> Vec<String> {
        if let Some(result) = self.current_file_completion() {
            return result.entries.iter().map(|e| e.input.clone()).collect();
        }
        if !self.paste.folds.is_empty()
            || self.palette_dismissed
            || self.cursor != self.input.len()
            || !self.input.starts_with('/')
            || self.input.contains('\n')
        {
            return Vec::new();
        }
        let input = self.input.as_str();
        if input.ends_with(' ') && input.trim().contains(' ') {
            return Vec::new();
        }
        let Some((command, query)) = input.split_once(' ') else {
            let prefix = input.to_ascii_lowercase();
            return slash::COMMAND_SPECS
                .iter()
                .flat_map(|spec| std::iter::once(spec.name).chain(spec.aliases.iter().copied()))
                .map(|name| format!("/{name}"))
                .chain(
                    [
                        "/skills",
                        "/templates",
                        "/reload",
                        "/reasoning",
                        "/scheduled",
                        "/input",
                    ]
                    .map(str::to_owned),
                )
                .chain(self.command_resources.iter().map(|r| r.command.clone()))
                .filter(|name| name.to_ascii_lowercase().starts_with(&prefix))
                .take(2048)
                .collect();
        };
        let values: Vec<String> = match command {
            "/persona" | "/personas" => crate::persona::TYPES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "/model" => self
                .model_menu
                .entries
                .iter()
                .map(|m| m.id.clone())
                .collect(),
            "/reasoning" => self.reasoning_choices(),
            "/input" => {
                let mut options = ["list", "steer", "follow-up", "edit", "cancel", "mode"]
                    .map(str::to_owned)
                    .to_vec();
                options.extend(
                    self.queued_inputs
                        .iter()
                        .filter(|input| input.status == crate::input_queue::InputStatus::Received)
                        .flat_map(|input| {
                            [
                                format!("edit {} {} {}", input.id, input.revision, input.text),
                                format!("cancel {}", input.id),
                            ]
                        })
                        .filter(|option| option.len() + 7 <= MAX_MESSAGE_BYTES),
                );
                options.extend(["steer", "follow-up"].into_iter().flat_map(|kind| {
                    ["one-at-a-time", "all"].map(|mode| format!("mode {kind} {mode}"))
                }));
                options
            }
            "/scheduled" => {
                let mut values = vec!["list".to_owned()];
                for entry in self
                    .scheduled_inputs
                    .iter()
                    .filter(|entry| entry.project == self.active_project())
                {
                    values.push(format!("cancel {}", entry.id));
                    if entry.interrupted {
                        values.push(format!("retry {}", entry.id));
                    }
                    let edit = format!("edit {} {} {}", entry.id, entry.revision, entry.text);
                    if edit.len() + 12 <= MAX_MESSAGE_BYTES {
                        values.push(edit);
                    }
                }
                values
            }
            "/goal" => [
                "show",
                "list",
                "status",
                "run",
                "resume",
                "cancel",
                "transition",
                "put",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            "/yolo" => ["on", "off"].iter().map(|s| s.to_string()).collect(),
            "/approval" | "/approvals" => ["ask", "always", "never"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "/learn" => ["show", "put", "evidence", "resume"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "/review" => Vec::new(),
            "/project" => {
                let mut values = vec![
                    "list".to_string(),
                    "open".to_string(),
                    "select".to_string(),
                    "close".to_string(),
                ];
                if query.eq_ignore_ascii_case("open")
                    || query.eq_ignore_ascii_case("select")
                    || query.eq_ignore_ascii_case("close")
                {
                    values = self.project_tabs.clone();
                }
                values
            }
            "/session" => ["list", "resume-last", "open", "fork", "inspect", "archive"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            _ => Vec::new(),
        };
        values
            .into_iter()
            .filter(|value| {
                value
                    .to_ascii_lowercase()
                    .starts_with(&query.to_ascii_lowercase())
            })
            .map(|value| format!("{command} {value}"))
            .collect()
    }

    fn choose_slash(&mut self, choices: &[String]) {
        if let Some(choice) = choices.get(self.palette_index % choices.len()) {
            if let Some(entry) = self
                .current_file_completion()
                .and_then(|result| result.entries.iter().find(|e| e.input == *choice))
                .cloned()
            {
                self.set_completed_input(if entry.directory {
                    choice.clone()
                } else {
                    format!("{choice} ")
                });
                self.file_completion = None;
                self.palette_dismissed = !entry.directory;
                return;
            }
            let second_level = choice.contains(' ');
            self.set_completed_input(format!("{choice} "));
            self.palette_index = 0;
            self.palette_dismissed = second_level;
        }
    }

    fn set_completed_input(&mut self, text: String) {
        let prefix = self
            .input
            .char_indices()
            .zip(text.chars())
            .take_while(|((_, old), new)| old == new)
            .map(|((index, ch), _)| index + ch.len_utf8())
            .last()
            .unwrap_or(0);
        let prefix = if grapheme_boundary(&self.input, prefix) && grapheme_boundary(&text, prefix) {
            prefix
        } else {
            previous_boundary(&self.input, prefix)
        };
        if self.replace_input_range(prefix, self.input.len(), &text[prefix..]) {
            self.ordinary_paste = OrdinaryPasteBurst::default();
            self.history_search = None;
            self.palette_dismissed = false;
            self.palette_index = 0;
            self.palette_rows = 0;
        }
    }

    fn render_slash_choices(&mut self, frame: &mut Frame<'_>, prompt: Rect) {
        self.palette_area = Rect::default();
        self.palette_rows = 0;
        let choices = self.slash_choices();
        let notice = self
            .current_file_completion()
            .and_then(FileCompletionResult::notice);
        let available = prompt.y.saturating_sub(frame.area().y);
        if choices.is_empty() {
            if let Some((status, guidance)) = notice
                && available >= 4
            {
                let area = Rect::new(prompt.x, prompt.y - 4, prompt.width.min(112), 4);
                self.palette_area = area;
                let width = usize::from(area.width.saturating_sub(2));
                let lines = [status, guidance].map(|text| Line::from(menu_prefix(text, width)));
                frame.render_widget(Clear, area);
                frame.render_widget(
                    Paragraph::new(lines.to_vec())
                        .block(Block::default().borders(Borders::ALL).title(" Files ")),
                    area,
                );
            }
            return;
        }
        // Preserve the existing minimum space for one selectable candidate.
        // Explanations use spare rows; the title still marks partial results.
        let notice_rows = if notice.is_some() {
            available.saturating_sub(5).min(2)
        } else {
            0
        };
        let footer = 4 + notice_rows;
        let height = (choices.len().min(8) as u16 + footer).min(available);
        if height < footer + 1 {
            return;
        }
        let area = Rect::new(prompt.x, prompt.y - height, prompt.width.min(112), height);
        self.palette_area = area;
        let selected = self.palette_index.min(choices.len() - 1);
        self.palette_rows = usize::from(height - footer).min(choices.len());
        self.palette_start = selected.saturating_sub(self.palette_rows.saturating_sub(1));
        let mut lines = choices
            .iter()
            .enumerate()
            .skip(self.palette_start)
            .take(self.palette_rows)
            .map(|(i, choice)| {
                let enabled = self.command_available(choice);
                let shown = if !self.paste.folds.is_empty()
                    && self.paste.folds.iter().all(|fold| {
                        choice.get(fold.start_byte..fold.end_byte)
                            == self.input.get(fold.start_byte..fold.end_byte)
                    }) {
                    InputProjection::new(choice, &self.paste.folds).text
                } else {
                    choice.clone()
                };
                let text = format!("{}{}", shown, if enabled { "" } else { " · idle only" });
                Line::from(Span::styled(
                    inline_token(&text, usize::from(area.width.saturating_sub(2))),
                    if i == selected {
                        Style::default().fg(Color::Black).bg(Color::Cyan)
                    } else if enabled {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ))
            })
            .collect::<Vec<_>>();
        let choice = &choices[selected];
        let source_width = usize::from(area.width.saturating_sub(2));
        let (detail, source) = if let Some(detail) = self.model_choice_detail(choice) {
            detail
        } else if let Some(resource) = self.command_resources.iter().find(|r| r.command == *choice)
        {
            (resource.description.clone(), {
                let hash = resource.hash.get(..12).unwrap_or(&resource.hash);
                let suffix = format!(" · {hash}");
                let basename = resource
                    .source
                    .rsplit('/')
                    .next()
                    .unwrap_or(&resource.source);
                let reserve = UnicodeWidthStr::width(suffix.as_str());
                if source_width > UnicodeWidthStr::width(basename) + reserve {
                    format!(
                        "{}{}",
                        menu_source(&resource.source, source_width - reserve),
                        suffix
                    )
                } else {
                    menu_source(&resource.source, source_width)
                }
            })
        } else if let Some(file) = self
            .current_file_completion()
            .and_then(|result| result.entries.iter().find(|f| f.input == *choice))
        {
            (
                if file.directory {
                    "Directory · Enter/Tab browse"
                } else {
                    "File reference · validated again at submission"
                }
                .into(),
                menu_source(&file.source, source_width),
            )
        } else {
            (
                slash::spec(choice.split_whitespace().next().unwrap_or(choice))
                    .map(|s| s.summary)
                    .unwrap_or("Project resource owner")
                    .to_owned(),
                if self.busy && matches!(choice.as_str(), "/reload" | "/skills" | "/templates") {
                    "Queued after current work"
                } else {
                    "Up/Down select · Tab/Enter complete · Esc dismiss"
                }
                .into(),
            )
        };
        for text in [detail, source] {
            lines.push(Line::from(Span::styled(
                menu_prefix(&text, source_width),
                Style::default().fg(Color::DarkGray),
            )));
        }
        if let Some((status, guidance)) = notice {
            for text in [status, guidance]
                .into_iter()
                .take(usize::from(notice_rows))
            {
                lines.push(Line::from(Span::styled(
                    menu_prefix(text, source_width),
                    Style::default().fg(Color::Yellow),
                )));
            }
        }
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(format!(
                " Commands / files {}/{}{}{} ",
                selected + 1,
                choices.len(),
                if notice.is_some() { " · partial" } else { "" },
                if self.command_resources.iter().any(|r| r.command == *choice) {
                    " · F2 source"
                } else {
                    ""
                }
            ))),
            area,
        );
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> TuiAction {
        if let Some(picker) = self.directory_picker.as_mut() {
            let result = picker.mouse(mouse);
            self.picker_result(result);
            return TuiAction::Redraw;
        }

        if let Some(browser) = self.transcript_browser.as_mut() {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                && mouse.row == browser.area.y + 2
                && mouse.column > browser.area.x
            {
                let x = mouse.column - browser.area.x - 1;
                let code = if x < 6 {
                    Some(KeyCode::Up)
                } else if x < 13 {
                    Some(KeyCode::Down)
                } else if x < 20 {
                    Some(KeyCode::Enter)
                } else if x < 28 {
                    Some(KeyCode::Esc)
                } else {
                    None
                };
                if let Some(code) = code {
                    let action = browser.key(KeyEvent::new(code, KeyModifiers::NONE));
                    if let Some(action) = action {
                        self.transcript_browser = None;
                        self.dirty = true;
                        return action;
                    }
                }
            }
            if let Some(browser) = self.transcript_browser.as_mut() {
                match mouse.kind {
                    MouseEventKind::ScrollUp => browser.scroll = browser.scroll.saturating_sub(3),
                    MouseEventKind::ScrollDown => browser.scroll = browser.scroll.saturating_add(3),
                    _ => {}
                }
            }
            self.dirty = true;
            return TuiAction::Redraw;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && self
                .follow_hit
                .is_some_and(|r| r.contains(Position::new(mouse.column, mouse.row)))
        {
            self.follow_latest();
            return TuiAction::Redraw;
        }
        let position = Position::new(mouse.column, mouse.row);
        if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
            self.dragging_split = None;
            self.dragging_row = None;
            self.dragging_project_tab = None;
            self.dragging_subtab = None;
            return TuiAction::Redraw;
        }
        // Arm a tab drag on press. The move is applied on `Drag` so the strip
        // reorders live under the cursor; release simply clears the arm.
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some((_, index)) = self
                .project_hits
                .iter()
                .find(|(rect, _)| rect.contains(position))
                && *index < self.project_tabs.len()
            {
                self.dragging_project_tab = Some(*index);
                self.dragging_subtab = None;
            } else if let Some((_, SubTabHit::Select(index))) = self
                .subtab_hits
                .iter()
                .find(|(rect, _)| rect.contains(position))
            {
                self.dragging_subtab = Some(*index);
                self.dragging_project_tab = None;
            }
        }
        if matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left)) {
            if let Some(source) = self.dragging_project_tab {
                let target = self
                    .project_hits
                    .iter()
                    .find(|(rect, _)| rect.contains(position))
                    .map(|(_, index)| *index)
                    .filter(|index| *index < self.project_tabs.len() && *index != source);
                if let Some(target) = target {
                    let name = self.project_tabs[source].clone();
                    if self.move_project_tab(&name, target) {
                        self.dragging_project_tab = Some(target);
                    }
                }
                return TuiAction::Redraw;
            }
            if let Some(source) = self.dragging_subtab {
                let target = self
                    .subtab_hits
                    .iter()
                    .find(|(rect, _)| rect.contains(position))
                    .and_then(|(_, hit)| match hit {
                        SubTabHit::Select(index) if *index != source => Some(*index),
                        _ => None,
                    });
                if let Some(target) = target
                    && self.subtab_move(source, target)
                {
                    self.dragging_subtab = Some(target.max(1));
                }
                return TuiAction::Redraw;
            }
        }
        // Match the familiar terminal/browser tab gesture without changing
        // the BentoBox layout: middle-clicking a project tab closes only the
        // tab projection; its session journal remains on disk and can be
        // reopened through the picker/session browser.
        if mouse.kind == MouseEventKind::Down(MouseButton::Middle)
            && let Some((_, index)) = self
                .project_hits
                .iter()
                .find(|(rect, _)| rect.contains(position))
            && *index < self.project_tabs.len()
        {
            let name = self.project_tabs[*index].clone();
            let active_draft = *index == self.active_project && !self.input.trim().is_empty();
            let retained_draft = self
                .project_drafts
                .get(&name)
                .is_some_and(|draft| !draft.input.trim().is_empty());
            if active_draft || retained_draft {
                self.set_status("Draft kept · clear the prompt before closing this project");
                return TuiAction::Redraw;
            }
            if self.close_project_tab(&name) {
                return TuiAction::Redraw;
            }
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            if self.palette_area.contains(position) {
                let choices = self.slash_choices();
                if !choices.is_empty()
                    && mouse.row > self.palette_area.y
                    && mouse.row < self.palette_area.bottom() - 1
                {
                    let row = usize::from(mouse.row - self.palette_area.y - 1);
                    if row >= self.palette_rows {
                        return TuiAction::Redraw;
                    }
                    self.palette_index = self.palette_start + row;
                    self.choose_slash(&choices);
                }
                return TuiAction::Redraw;
            }
            if let Some((_, index)) = self
                .project_hits
                .iter()
                .find(|(rect, _)| rect.contains(position))
            {
                if *index == usize::MAX - 2 {
                    let name = self.active_project().to_owned();
                    self.close_project_tab(&name);
                } else if *index == self.project_tabs.len() {
                    self.open_directory_picker();
                } else {
                    self.request_project_select(*index);
                }
                return TuiAction::Redraw;
            }
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some((_, hit)) = self
                .subtab_hits
                .iter()
                .find(|(rect, _)| rect.contains(position))
        {
            match *hit {
                SubTabHit::Select(index) => {
                    self.subtab_select(index);
                }
                SubTabHit::AddWorktree => match self.subtab_add_worktree(None) {
                    Ok(name) => self.set_status(format!("worktree sub-tab: {name}")),
                    Err(error) => self.set_status(format!("worktree add failed: {error}")),
                },
                SubTabHit::AddInPlace => {
                    self.subtab_add_in_place(None);
                }
                SubTabHit::ConcurrencyUp(index) => {
                    self.subtab_concurrency(index, 1);
                }
                SubTabHit::ConcurrencyDown(index) => {
                    self.subtab_concurrency(index, -1);
                }
            }
            return TuiAction::Redraw;
        }
        let adapter = BentoBoxLayoutAdapter::new(&self.workspace_layout, self.workspace_area);
        let panes: Vec<_> = adapter.visible_panes().collect();
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            // A press in either left-column prompt moves keyboard focus there
            // (ZS1-148). The prompt rectangles are recorded by the frame that
            // was actually drawn, so a collapsed group never steals focus.
            if self
                .arch_prompt_rect
                .is_some_and(|rect| rect.contains(position))
            {
                self.set_left_prompt(LeftPrompt::Arch);
            } else if self
                .docked_prompt_rect
                .is_some_and(|rect| rect.contains(position))
            {
                self.set_left_prompt(LeftPrompt::Discussion);
            }
            self.dragging_row = panes.iter().find_map(|top| {
                panes
                    .iter()
                    .find(|bottom| {
                        bottom.id != top.id
                            && bottom.rect.x == top.rect.x
                            && bottom.rect.y == top.rect.bottom()
                            && mouse.row.abs_diff(bottom.rect.y) <= 2
                            && mouse.column > top.rect.x
                            && mouse.column < top.rect.right().saturating_sub(1)
                    })
                    .map(|bottom| {
                        let preset = self.workspace_layout.preset();
                        let weight = |id| {
                            self.workspace_layout
                                .row_weights
                                .get(&id)
                                .copied()
                                .unwrap_or_else(|| {
                                    preset
                                        .panes
                                        .iter()
                                        .find(|spec| spec.id == id)
                                        .unwrap()
                                        .row_weight
                                })
                        };
                        let column = preset
                            .panes
                            .iter()
                            .find(|spec| spec.id == top.id)
                            .unwrap()
                            .column;
                        let specs: Vec<_> = preset
                            .panes
                            .iter()
                            .filter(|spec| {
                                spec.column == column && panes.iter().any(|pane| pane.id == spec.id)
                            })
                            .collect();
                        let total: u16 = specs.iter().map(|spec| weight(spec.id)).sum();
                        let minimum: u16 = specs.iter().map(|spec| spec.min_size.height).sum();
                        (
                            top.id,
                            bottom.id,
                            mouse.row,
                            weight(top.id),
                            weight(bottom.id),
                            self.workspace_area
                                .height
                                .saturating_sub(minimum)
                                .max(1)
                                .saturating_mul(1000)
                                / total.max(1),
                        )
                    })
            });
            let preset = self.workspace_layout.preset();
            self.dragging_split = panes.iter().find_map(|pane| {
                let adjacent = panes
                    .iter()
                    .find(|other| other.rect.x == pane.rect.right())?;
                let left = preset.panes.iter().find(|spec| spec.id == pane.id)?.column;
                let right = preset
                    .panes
                    .iter()
                    .find(|spec| spec.id == adjacent.id)?
                    .column;
                (left != right
                    // Forgiving two-cell grip, matching herdr's coarse-mouse
                    // pane handle behavior.
                    && mouse.column.abs_diff(pane.rect.right()) <= 2
                    && mouse.row >= pane.rect.y
                    && mouse.row < pane.rect.bottom())
                .then_some((
                    left,
                    right,
                    mouse.column,
                    self.workspace_layout.ratios.bounded(),
                ))
            });
            if let Some(pane) = panes.iter().find(|pane| pane.rect.contains(position)) {
                self.focus_workspace_pane(pane.id);
                if pane.id == PaneId::SessionList
                    && mouse.column > pane.rect.x
                    && mouse.column < pane.rect.right().saturating_sub(1)
                    && mouse.row > pane.rect.y
                    && mouse.row < pane.rect.bottom().saturating_sub(1)
                {
                    let row = self
                        .session_browser_scroll(pane.rect)
                        .saturating_add(usize::from(mouse.row.saturating_sub(pane.rect.y + 1)));
                    if row < self.session_browser.len().min(32) {
                        self.session_browser_cursor = row;
                        self.dirty = true;
                        return TuiAction::Redraw;
                    }
                }
            }
        }
        if let Some((top, bottom, origin, top_weight, bottom_weight, scale)) = self.dragging_row
            && matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left))
        {
            // Transfer only this pair's weight; unrelated rows retain their share.
            let delta = (i32::from(mouse.row) - i32::from(origin)) * 1000 / i32::from(scale.max(1));
            let total = top_weight + bottom_weight;
            let split = (i32::from(top_weight) + delta).clamp(
                i32::from(total.saturating_sub(1000).max(1)),
                i32::from((total - 1).min(1000)),
            ) as u16;
            self.workspace_layout.row_weights.insert(top, split);
            self.workspace_layout
                .row_weights
                .insert(bottom, total - split);
            self.layout_dirty = true;
            self.cached_transcript = None;
        } else if let Some((left, right, origin, initial)) = self.dragging_split
            && matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left))
            && self.workspace_area.width > 0
        {
            let delta = (i32::from(mouse.column) - i32::from(origin)) * 100
                / i32::from(self.workspace_area.width);
            let total = initial.get(left) + initial.get(right);
            let value =
                (i32::from(initial.get(left)) + delta).clamp(5, i32::from(total - 5)) as u16;
            let mut ratios = initial;
            ratios.set(left, value);
            ratios.set(right, total - value);
            self.workspace_layout.set_ratios(ratios);
            self.layout_dirty = true;
            self.cached_transcript = None;
        }
        if let Some(pane) = panes.iter().find(|pane| pane.rect.contains(position))
            && matches!(
                mouse.kind,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
            )
        {
            self.focus_workspace_pane(pane.id);
            if pane.id == PaneId::SessionList {
                self.move_session_browser_cursor(if mouse.kind == MouseEventKind::ScrollUp {
                    -3
                } else {
                    3
                });
            } else if pane.id == conversation_pane_for_tab(self.workspace_layout.tab) {
                if mouse.kind == MouseEventKind::ScrollUp {
                    self.scroll_up(3);
                } else {
                    self.scroll_down(3);
                }
            } else {
                let scroll = self.pane_scroll.entry(pane.id).or_default();
                *scroll = if mouse.kind == MouseEventKind::ScrollUp {
                    scroll.saturating_sub(3)
                } else {
                    scroll.saturating_add(3)
                };
            }
        }
        self.dirty = true;
        TuiAction::Redraw
    }

    /// Flush due terminal text even while idle (spinner ticks are insufficient).
    /// Hosts also use the remaining delay to wake before the normal poll period.
    pub fn flush_ordinary_paste(&mut self, now: Instant) -> bool {
        let timeout = self.ordinary_paste_timeout();
        if self
            .ordinary_paste
            .last_input
            .is_some_and(|last| now.saturating_duration_since(last) > timeout)
        {
            self.flush_ordinary_paste_buffer()
        } else {
            false
        }
    }

    fn ordinary_paste_timeout(&self) -> Duration {
        if self.ordinary_paste.active {
            ORDINARY_PASTE_ACTIVE_IDLE
        } else {
            ORDINARY_PASTE_INTERVAL
        }
    }

    fn ordinary_paste_wait(&self, now: Instant, normal: Duration) -> Duration {
        if self.ordinary_paste.buffer.is_empty() && !self.ordinary_paste.rejected {
            return normal;
        }
        self.ordinary_paste.last_input.map_or(normal, |last| {
            normal.min(
                (self.ordinary_paste_timeout() + Duration::from_millis(1))
                    .saturating_sub(now.saturating_duration_since(last)),
            )
        })
    }

    fn flush_ordinary_paste_buffer(&mut self) -> bool {
        let text = std::mem::take(&mut self.ordinary_paste.buffer);
        let active = std::mem::take(&mut self.ordinary_paste.active);
        let rejected = std::mem::take(&mut self.ordinary_paste.rejected);
        if rejected {
            self.set_status(format!(
                "Input exceeds {} bytes; draft unchanged",
                MAX_MESSAGE_BYTES
            ));
        } else if !text.is_empty() {
            if active {
                self.insert_paste(&text);
            } else {
                self.insert_text(&text);
            }
            self.palette_dismissed = active;
            self.palette_index = 0;
            self.palette_rows = 0;
            self.project_checkpoint_dirty = true;
        }
        !text.is_empty() || rejected
    }

    fn finish_ordinary_paste(&mut self) {
        self.flush_ordinary_paste_buffer();
        self.ordinary_paste = OrdinaryPasteBurst::default();
    }

    fn buffer_ordinary_char(&mut self, ch: char, now: Instant, active: bool) {
        self.ordinary_paste.last_input = Some(now);
        self.ordinary_paste.active |= active;
        if self.ordinary_paste.active {
            self.ordinary_paste.suppress_until = Some(now + ORDINARY_PASTE_ENTER_WINDOW);
        }
        if self.ordinary_paste.rejected {
            return;
        }
        if self.input.len() + self.ordinary_paste.buffer.len() + ch.len_utf8() > MAX_MESSAGE_BYTES {
            self.ordinary_paste.buffer.clear();
            self.ordinary_paste.rejected = true;
        } else {
            self.ordinary_paste.buffer.push(ch);
        }
    }

    pub fn handle_event(&mut self, event: Event) -> TuiAction {
        self.handle_event_at(event, Instant::now())
    }

    /// Timestamped terminal boundary, also used for deterministic timing tests.
    /// Direct handle_key remains the already-classified editor/modal dispatcher.
    pub fn handle_event_at(&mut self, event: Event, now: Instant) -> TuiAction {
        self.flush_ordinary_paste(now);
        let composing = self.directory_picker.is_none()
            && self.transcript_browser.is_none()
            && self.history_search.is_none()
            && !self
                .approval_views
                .get(self.active_project())
                .is_some_and(|view| view.focused && !view.requests.is_empty());
        if let Event::Key(key) = &event {
            if key.kind == KeyEventKind::Release {
                return TuiAction::None;
            }
            if composing
                && !key
                    .modifiers
                    .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL | KeyModifiers::SUPER)
            {
                let fast = self.ordinary_paste.last_input.is_some_and(|last| {
                    now.saturating_duration_since(last) <= ORDINARY_PASTE_INTERVAL
                });
                if let KeyCode::Char(ch) = key.code {
                    if ch.is_ascii() || fast || self.ordinary_paste.active {
                        self.buffer_ordinary_char(ch, now, fast);
                    } else {
                        // Do not hold an isolated IME commit. UTF-8 stays in the
                        // normal grapheme-aware editor; following fast keys can
                        // still activate buffering without byte-index retro-grabs.
                        self.insert_text(&ch.to_string());
                        self.palette_dismissed = false;
                        self.palette_index = 0;
                        self.palette_rows = 0;
                        self.ordinary_paste.last_input = Some(now);
                        self.project_checkpoint_dirty = true;
                    }
                    return TuiAction::None;
                }
                if key.code == KeyCode::Enter
                    && key.modifiers.is_empty()
                    && (self.ordinary_paste.active
                        || (fast && !self.ordinary_paste.buffer.is_empty())
                        || self
                            .ordinary_paste
                            .suppress_until
                            .is_some_and(|until| now <= until))
                {
                    self.buffer_ordinary_char('\n', now, true);
                    return TuiAction::None;
                }
            }
        }
        if matches!(event, Event::Key(_) | Event::Mouse(_) | Event::Paste(_)) {
            self.finish_ordinary_paste();
        }
        // A deliberate completion key reopens choices after literal paste.
        // Buffered characters themselves must never activate that menu.
        if matches!(&event, Event::Key(key) if key.code == KeyCode::Tab && key.modifiers.is_empty())
        {
            self.palette_dismissed = false;
        }
        if matches!(event, Event::Key(_) | Event::Paste(_)) && self.directory_picker.is_none() {
            self.project_checkpoint_dirty = true;
        }
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key),
            Event::Mouse(mouse) => {
                let project = self.active_project().to_owned();
                if mouse.row > 0
                    && let Some(view) = self.approval_views.get_mut(&project)
                    && view.focused
                    && !view.requests.is_empty()
                {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => view.scroll = view.scroll.saturating_sub(3),
                        MouseEventKind::ScrollDown => view.scroll = view.scroll.saturating_add(3),
                        _ => {}
                    }
                    self.dirty = true;
                    TuiAction::Redraw
                } else {
                    self.handle_mouse(mouse)
                }
            }
            Event::Paste(text) => {
                let project = self.active_project().to_owned();
                if let Some(view) = self.approval_views.get_mut(&project) {
                    view.focused = false;
                }
                if self.transcript_browser.is_some() {
                    return TuiAction::None;
                }
                if let Some(picker) = self.directory_picker.as_mut() {
                    picker.paste(&text);
                    self.dirty = true;
                    return TuiAction::Redraw;
                }
                if self.history_search.is_some() {
                    self.history_search_text(&text);
                } else {
                    self.insert_paste(&text);
                    // Pasted slash text remains literal until a deliberate later key.
                    self.palette_dismissed = true;
                }
                TuiAction::None
            }
            Event::Resize(_, _) => {
                self.dirty = true;
                TuiAction::Redraw
            }
            _ => TuiAction::None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> TuiAction {
        if key.kind == KeyEventKind::Release {
            return TuiAction::None;
        }
        self.touch_editor_draft();
        self.project_checkpoint_dirty = true;
        // The inline Goal editor is modal within the conversation group so
        // Enter saves and Esc cancels instead of reaching the prompt.
        if self.goal_edit.is_some() {
            return self.goal_edit_key(key);
        }
        // The arch console is the focused left-column prompt (ZS1-148). It only
        // captures editing keys; every control chord still reaches its usual
        // handler so focus can always be moved away.
        if self.left_prompt == LeftPrompt::Arch && self.arch_prompt_captures(key) {
            return self.arch_prompt_key(key);
        }
        if let Some(action) = self.approval_key(key) {
            return action;
        }
        if self.history_search.is_some() {
            return self.history_search_key(key);
        }
        if let Some(picker) = self.directory_picker.as_mut() {
            let result = picker.key(key);
            self.picker_result(result);
            return TuiAction::Redraw;
        }

        if self.transcript_browser.is_some()
            && key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.transcript_browser = None;
            return if self.busy {
                TuiAction::Interrupt
            } else {
                TuiAction::Redraw
            };
        }
        if let Some(browser) = self.transcript_browser.as_mut() {
            let action = browser.key(key);
            self.dirty = true;
            if let Some(action) = action {
                self.transcript_browser = None;
                return action;
            }
            return TuiAction::Redraw;
        }
        if key.modifiers == KeyModifiers::ALT
            && key.code == KeyCode::Enter
            && let Some(index) = self.adjacent_paste()
        {
            self.cursor = self.paste.folds.remove(index).start_byte;
            self.edited_input();
            self.palette_dismissed = false;
            return TuiAction::Redraw;
        }
        if key.modifiers == KeyModifiers::ALT {
            match key.code {
                KeyCode::Left => {
                    self.move_word(false);
                    return TuiAction::Redraw;
                }
                KeyCode::Right => {
                    self.move_word(true);
                    return TuiAction::Redraw;
                }
                KeyCode::Up => {
                    self.history_move(-1);
                    return TuiAction::Redraw;
                }
                KeyCode::Down => {
                    self.history_move(1);
                    return TuiAction::Redraw;
                }
                _ => {}
            }
        }
        if key.modifiers == KeyModifiers::ALT {
            match key.code {
                KeyCode::Char('r') => {
                    self.toggle_reasoning();
                    return TuiAction::Redraw;
                }
                KeyCode::Char('b') => {
                    self.open_transcript_browser();
                    return TuiAction::Redraw;
                }
                KeyCode::Char('c') => {
                    return self
                        .copy_latest_answer()
                        .map(TuiAction::Copy)
                        .unwrap_or_else(|| {
                            self.set_status("No assistant answer to copy");
                            TuiAction::Redraw
                        });
                }
                KeyCode::Char(',') => {
                    self.move_active_subtab(-1);
                    return TuiAction::Redraw;
                }
                KeyCode::Char('.') => {
                    self.move_active_subtab(1);
                    return TuiAction::Redraw;
                }
                KeyCode::Char('n') => {
                    match self.subtab_add_worktree(None) {
                        Ok(name) => self.set_status(format!("worktree sub-tab: {name}")),
                        Err(error) => self.set_status(format!("worktree add failed: {error}")),
                    }
                    return TuiAction::Redraw;
                }
                KeyCode::Char('i') => {
                    self.subtab_add_in_place(None);
                    return TuiAction::Redraw;
                }
                KeyCode::Char('w') => {
                    let index = self.active_subtab();
                    self.subtab_close(index);
                    return TuiAction::Redraw;
                }
                KeyCode::Char('g') => {
                    self.begin_goal_edit();
                    return TuiAction::Redraw;
                }
                KeyCode::Char('m') => {
                    // ZS1-148: switch the left-column prompt focus between the
                    // resident discussion group and the arch master console.
                    match self.toggle_left_prompt() {
                        LeftPrompt::Arch => self.set_status(
                            "Arch console focused · !cmd runs bash · text steers · Esc unfocuses",
                        ),
                        LeftPrompt::Discussion => {
                            self.set_status("Discussion prompt focused · Alt-M for arch console")
                        }
                    }
                    return TuiAction::Redraw;
                }
                _ => {}
            }
        }
        if key.code == KeyCode::End && self.input.is_empty() {
            self.follow_latest();
            return TuiAction::Redraw;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('t') {
            self.open_directory_picker();
            return TuiAction::Redraw;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('g') {
            return if self.external_editor_shortcut_available() {
                TuiAction::OpenExternalEditor
            } else {
                TuiAction::None
            };
        }
        if matches!(
            key.code,
            KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete
        ) {
            self.palette_dismissed = false;
            self.palette_index = 0;
            self.palette_rows = 0;
        }
        let choices = self.slash_choices();
        if !choices.is_empty() && key.modifiers.is_empty() {
            match key.code {
                KeyCode::F(2) => {
                    if let Some(resource) = choices
                        .get(self.palette_index % choices.len())
                        .and_then(|choice| {
                            self.command_resources.iter().find(|r| r.command == *choice)
                        })
                    {
                        return TuiAction::InspectResource {
                            command: resource.command.clone(),
                            source_hash: resource.hash.clone(),
                        };
                    }
                }
                // An ambiguous completion selects a highlighted row only once
                // that menu has been drawn. Before then, preserve the common
                // prefix completion below instead of guessing an unseen row.
                KeyCode::Tab if choices.len() == 1 || self.palette_rows > 0 => {
                    self.choose_slash(&choices);
                    return TuiAction::Redraw;
                }
                KeyCode::Down => {
                    self.palette_index = (self.palette_index + 1) % choices.len();
                    return TuiAction::Redraw;
                }
                KeyCode::Up => {
                    self.palette_index = (self.palette_index + choices.len() - 1) % choices.len();
                    return TuiAction::Redraw;
                }
                KeyCode::Esc => {
                    self.palette_dismissed = true;
                    return TuiAction::Redraw;
                }
                KeyCode::Enter => {
                    let exact = choices
                        .get(self.palette_index % choices.len())
                        .is_some_and(|choice| choice.eq_ignore_ascii_case(&self.input));
                    let submenu = matches!(
                        self.input.as_str(),
                        "/persona"
                            | "/personas"
                            | "/model"
                            | "/goal"
                            | "/learn"
                            | "/session"
                            | "/approval"
                            | "/approvals"
                    );
                    if !exact || submenu {
                        self.choose_slash(&choices);
                        return TuiAction::Redraw;
                    }
                }
                _ => {}
            }
        }
        let modifiers = key.modifiers;
        if modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                // Layer-1 tab order: Ctrl-B/F move the active project tab.
                KeyCode::Char('b') => {
                    self.move_active_project(-1);
                    return TuiAction::Redraw;
                }
                KeyCode::Char('f') => {
                    self.move_active_project(1);
                    return TuiAction::Redraw;
                }
                // Ctrl-C is an interrupt while a provider turn is active;
                // quitting in that state would discard a usable session
                // instead of returning the user to an idle prompt. Ctrl-D
                // remains the explicit empty-prompt quit binding.
                KeyCode::Char('c') if self.input.is_empty() && self.busy => {
                    return TuiAction::Interrupt;
                }
                KeyCode::Char('c') | KeyCode::Char('d') if self.input.is_empty() => {
                    return TuiAction::Quit;
                }
                KeyCode::Char('c') => return TuiAction::Interrupt,
                KeyCode::Char('u') => {
                    let start = self.visible_line_boundary(false);
                    let start = if start == self.cursor && start > 0 {
                        start - 1
                    } else {
                        start
                    };
                    self.kill_input_range(start, self.cursor);
                    self.input_scroll = 0;
                    self.preferred_column = None;
                    self.dirty = true;
                    return TuiAction::None;
                }
                KeyCode::Char('y') => {
                    self.yank_input();
                    return TuiAction::Redraw;
                }
                KeyCode::Char('r') => {
                    self.begin_history_search();
                    return TuiAction::Redraw;
                }
                KeyCode::Char('p') => {
                    self.history_move(-1);
                    return TuiAction::Redraw;
                }
                KeyCode::Char('n') => {
                    self.history_move(1);
                    return TuiAction::Redraw;
                }
                KeyCode::Char('a') => {
                    self.cursor = self.visible_line_boundary(false);
                    self.preferred_column = None;
                    self.dirty = true;
                    return TuiAction::None;
                }
                KeyCode::Char('e') => {
                    self.cursor = self.visible_line_boundary(true);
                    self.preferred_column = None;
                    self.dirty = true;
                    return TuiAction::None;
                }
                KeyCode::Left if !self.input.is_empty() => {
                    self.move_word(false);
                    return TuiAction::None;
                }
                KeyCode::Right if !self.input.is_empty() => {
                    self.move_word(true);
                    return TuiAction::None;
                }
                KeyCode::Delete => {
                    let end = self.word_boundary(true);
                    self.kill_input_range(self.cursor, end);
                    return TuiAction::Redraw;
                }
                KeyCode::Char('k') => {
                    let end = self.visible_line_boundary(true);
                    let end = if end == self.cursor && end < self.input.len() {
                        end + 1
                    } else {
                        end
                    };
                    self.kill_input_range(self.cursor, end);
                    return TuiAction::Redraw;
                }
                KeyCode::Char('w') if self.input.is_empty() && self.project_tabs.len() > 1 => {
                    self.pending_project =
                        Some(ProjectIntent::Close(self.active_project().to_owned()));
                    return TuiAction::Redraw;
                }
                KeyCode::Char('w') | KeyCode::Backspace => {
                    self.delete_previous_word();
                    return TuiAction::None;
                }
                KeyCode::Char('l') => return TuiAction::Redraw,
                KeyCode::Char('o') => {
                    self.toggle_tool_logs();
                    return TuiAction::Redraw;
                }
                // Workspace controls are kept on modified keys so ordinary
                // arrow navigation remains dedicated to multiline prompt
                // editing. Ctrl-Arrow moves pane focus; Ctrl-Shift-Arrow
                // adjusts the selected split by one bounded step.
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
                    if self.input.is_empty() =>
                {
                    let direction = match key.code {
                        KeyCode::Left => FocusDirection::Left,
                        KeyCode::Right => FocusDirection::Right,
                        KeyCode::Up => FocusDirection::Up,
                        KeyCode::Down => FocusDirection::Down,
                        _ => unreachable!(),
                    };
                    if modifiers.contains(KeyModifiers::SHIFT) && direction.is_horizontal() {
                        self.adjust_workspace_split(direction);
                    } else {
                        self.focus_workspace_direction(direction);
                    }
                    return TuiAction::Redraw;
                }
                KeyCode::Char('0') if self.input.is_empty() => {
                    self.reset_workspace_layout();
                    return TuiAction::Redraw;
                }
                // Workspace navigation follows herdr's tab-cycle model; the
                // visible names remain the source of truth, while Ctrl-Tab
                // provides a discoverable non-numeric shortcut.
                KeyCode::Tab if self.input.is_empty() => {
                    let current = self.active_project;
                    let next = if modifiers.contains(KeyModifiers::SHIFT) {
                        (current + self.project_tabs.len() - 1) % self.project_tabs.len()
                    } else {
                        (current + 1) % self.project_tabs.len()
                    };
                    self.request_project_select(next);
                    return TuiAction::Redraw;
                }
                // Keep Ctrl+1..5 as a compatibility shortcut. The visible
                // tab bar is the primary navigation and is mouse-addressable;
                // numeric keys are never interpreted while drafting text.
                KeyCode::Char(character) if ('1'..='9').contains(&character) => {
                    let index = usize::from(character as u8 - b'1');
                    self.request_project_select(index);
                    return TuiAction::Redraw;
                }
                // Ctrl-J is the portable terminal spelling of a newline.
                // Treat Ctrl-Enter the same way because a few terminals send
                // that pair as `KeyCode::Enter` rather than `Char('j')`.
                KeyCode::Char('j') | KeyCode::Enter => {
                    self.insert_text("\n");
                    return TuiAction::None;
                }
                _ => {}
            }
        }
        match key.code {
            // With an empty prompt, Tab cycles BentoBox focus; while typing
            // it remains available to the terminal's normal input handling.
            KeyCode::Tab if self.input.is_empty() => {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    self.focus_previous_workspace_pane();
                } else {
                    self.focus_next_workspace_pane();
                }
                return TuiAction::Redraw;
            }
            // Most terminals report Shift-Tab as BackTab rather than Tab with
            // a SHIFT modifier. Accept both spellings so reverse pane focus
            // works outside synthetic key tests.
            KeyCode::BackTab if self.input.is_empty() => {
                self.focus_previous_workspace_pane();
                return TuiAction::Redraw;
            }
            // Complete a slash command only while the command name is the
            // whole trailing token. Ordinary prompt text keeps Tab inert,
            // and a cursor in the middle of a draft never moves later bytes.
            KeyCode::Tab if modifiers.is_empty() => {
                if self.complete_slash_input() {
                    return TuiAction::Redraw;
                }
            }
            // An unbound control chord must never type its printable key name
            // into the prompt. For example, Ctrl-D on a non-empty draft used
            // to append `d`, and Ctrl-A appended `a` instead of being inert.
            KeyCode::Char(character)
                if !modifiers.intersects(
                    KeyModifiers::ALT | KeyModifiers::CONTROL | KeyModifiers::SUPER,
                ) =>
            {
                self.insert_text(&character.to_string())
            }
            KeyCode::Backspace => self.delete_previous_char(),
            KeyCode::Delete => self.delete_next_char(),
            KeyCode::Left => {
                self.cursor = self.adjacent_cursor(false);
                self.preferred_column = None;
            }
            KeyCode::Right => {
                self.cursor = self.adjacent_cursor(true);
                self.preferred_column = None;
            }
            // Home/End operate on the current logical line, as users expect
            // in a multiline prompt.  Ctrl-Home/Ctrl-End still address the
            // complete buffer.
            KeyCode::Home => {
                self.cursor = if modifiers.contains(KeyModifiers::CONTROL) {
                    0
                } else {
                    self.visible_line_boundary(false)
                };
                self.preferred_column = None;
            }
            KeyCode::End => {
                self.cursor = if modifiers.contains(KeyModifiers::CONTROL) {
                    self.input.len()
                } else {
                    self.visible_line_boundary(true)
                };
                self.preferred_column = None;
            }
            KeyCode::Up
                if self.input.is_empty()
                    && self.workspace_layout.focused == Some(PaneId::SessionList) =>
            {
                self.move_session_browser_cursor(-1);
                return TuiAction::Redraw;
            }
            KeyCode::Down
                if self.input.is_empty()
                    && self.workspace_layout.focused == Some(PaneId::SessionList) =>
            {
                self.move_session_browser_cursor(1);
                return TuiAction::Redraw;
            }
            KeyCode::PageUp
                if self.input.is_empty()
                    && self.workspace_layout.focused == Some(PaneId::SessionList) =>
            {
                self.move_session_browser_cursor(-8);
                return TuiAction::Redraw;
            }
            KeyCode::PageDown
                if self.input.is_empty()
                    && self.workspace_layout.focused == Some(PaneId::SessionList) =>
            {
                self.move_session_browser_cursor(8);
                return TuiAction::Redraw;
            }
            KeyCode::Up if self.input.is_empty() || self.history_cursor.is_some() => {
                self.history_move(-1)
            }
            KeyCode::Down if self.input.is_empty() || self.history_cursor.is_some() => {
                self.history_move(1)
            }
            KeyCode::Up => self.move_vertical(-1),
            KeyCode::Down => self.move_vertical(1),
            KeyCode::PageUp => self.scroll_up(8),
            KeyCode::PageDown => self.scroll_down(8),
            KeyCode::Enter => {
                // A shifted Enter is an insertion operation, never a submit.
                // This is handled after the Ctrl branch so terminals that
                // report Ctrl-Shift-Enter remain newline-safe as well.
                if modifiers.contains(KeyModifiers::SHIFT) {
                    self.insert_text("\n");
                    return TuiAction::None;
                }
                if self.input.is_empty()
                    && self.workspace_layout.focused == Some(PaneId::SessionList)
                {
                    if let Some(path) = self.selected_session_browser_path() {
                        return TuiAction::OpenSession(path.to_owned());
                    }
                    return TuiAction::Redraw;
                }
                // Use trimming only to decide whether the buffer is blank;
                // preserve the submitted bytes themselves. Leading spaces,
                // indentation, and a deliberate trailing newline are part of
                // a code-oriented prompt and must not be rewritten by the
                // terminal host.
                if !self.input.trim().is_empty() {
                    let text = self.input.clone();
                    if !self.paste.folds.is_empty() {
                        self.submitted_pastes.push_back(SubmittedPaste {
                            digest: input_digest(&text),
                            cursor: self.cursor,
                            paste: self.paste.clone(),
                        });
                        while self.submitted_pastes.len()
                            > MAX_TUI_PENDING_INPUTS
                                + crate::input_queue::InputQueueLimits::default().max_pending
                                + 1
                        {
                            self.submitted_pastes.pop_front();
                        }
                    }
                    self.paste.folds.clear();
                    self.history_draft = None;
                    self.history.push_back(text.clone());
                    while self.history.len() > self.max_history {
                        self.history.pop_front();
                    }
                    self.input.clear();
                    self.cursor = 0;
                    self.history_cursor = None;
                    self.input_scroll = 0;
                    self.preferred_column = None;
                    self.follow_latest();
                    self.dirty = true;
                    return TuiAction::Submit(text);
                }
            }
            KeyCode::Esc => {
                if self.history_draft.is_some() {
                    self.cancel_history_preview();
                } else if self.input.is_empty() {
                    return TuiAction::Interrupt;
                } else {
                    self.palette_dismissed = true;
                    self.set_status(
                        "Draft kept · Ctrl-U deletes to line start · Ctrl-R searches history",
                    );
                }
            }
            _ => {}
        }
        self.dirty = true;
        TuiAction::None
    }

    /// Complete the slash command name at the end of the input buffer.
    /// Completion is deliberately conservative: it never rewrites a
    /// multiline prompt, guesses an argument, or acts on a cursor in the
    /// middle of a draft. The shared slash catalogue remains authoritative.
    fn complete_slash_input(&mut self) -> bool {
        if !self.paste.folds.is_empty() || self.cursor != self.input.len() {
            return false;
        }
        let trimmed = self.input.trim_start_matches(char::is_whitespace);
        let leading = self.input.len().saturating_sub(trimmed.len());
        if self.input[..leading].contains(['\n', '\r']) {
            return false;
        }
        if !trimmed.starts_with('/')
            || trimmed.len() <= 1
            || trimmed[1..].chars().any(char::is_whitespace)
        {
            return false;
        }
        let candidates = slash::complete(trimmed);
        if candidates.is_empty() {
            return false;
        }
        let body = &trimmed[1..];
        let common = slash_common_prefix(&candidates);
        let replacement = if candidates.len() == 1 && common.len() >= body.len() {
            format!("/{} ", candidates[0])
        } else if common.len() > body.len() {
            format!("/{common}")
        } else if let Some(exact) = candidates
            .iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(body))
        {
            format!("/{exact} ")
        } else {
            return false;
        };
        let replaced_len = self.cursor.saturating_sub(leading);
        let resulting_len = self
            .input
            .len()
            .saturating_sub(replaced_len)
            .saturating_add(replacement.len());
        if resulting_len > MAX_MESSAGE_BYTES {
            return false;
        }
        self.replace_input_range(leading, self.cursor, &replacement);
        self.history_cursor = None;
        self.input_scroll = 0;
        self.preferred_column = None;
        self.dirty = true;
        true
    }

    fn projection(&self) -> InputProjection {
        InputProjection::new(&self.input, &self.paste.folds)
    }

    fn adjacent_paste(&self) -> Option<usize> {
        self.paste
            .folds
            .iter()
            .position(|fold| fold.end_byte == self.cursor)
            .or_else(|| {
                self.paste
                    .folds
                    .iter()
                    .position(|fold| fold.start_byte == self.cursor)
            })
    }

    fn adjacent_cursor(&self, forward: bool) -> usize {
        if forward {
            self.paste
                .folds
                .iter()
                .find(|fold| fold.start_byte == self.cursor)
                .map_or_else(
                    || next_boundary(&self.input, self.cursor),
                    |fold| fold.end_byte,
                )
        } else {
            self.paste
                .folds
                .iter()
                .find(|fold| fold.end_byte == self.cursor)
                .map_or_else(
                    || previous_boundary(&self.input, self.cursor),
                    |fold| fold.start_byte,
                )
        }
    }

    fn visible_line_boundary(&self, end: bool) -> usize {
        let view = self.projection();
        let cursor = view.display_cursor(self.cursor);
        view.canonical_cursor(
            if end {
                line_end(&view.text, cursor)
            } else {
                line_start(&view.text, cursor)
            },
            end,
        )
    }

    fn edited_input(&mut self) {
        self.touch_editor_draft();
        self.history_draft = None;
        self.history_cursor = None;
        self.preferred_column = None;
        self.input_scroll = 0;
        self.project_checkpoint_dirty = true;
        self.dirty = true;
    }

    // Every range replacement acts on canonical bytes. Intersected folds are
    // deleted atomically; insertion cannot leave a cursor inside hidden text.
    fn expanded_input_range(&self, mut start: usize, mut end: usize) -> (usize, usize) {
        if start != end {
            for fold in &self.paste.folds {
                if start < fold.end_byte && end > fold.start_byte {
                    start = start.min(fold.start_byte);
                    end = end.max(fold.end_byte);
                }
            }
        }
        (start, end)
    }

    fn kill_input_range(&mut self, start: usize, end: usize) {
        let (start, end) = self.expanded_input_range(start, end);
        if start == end {
            return;
        }
        let Some(removed) = self.input.get(start..end) else {
            return;
        };
        if removed.len() > MAX_MESSAGE_BYTES {
            self.set_status("Kill exceeds input budget; draft unchanged");
            return;
        }
        let removed = removed.to_owned();
        if self.replace_input_range(start, end, "") {
            self.kill_buffer = removed;
        }
    }

    fn yank_input(&mut self) {
        if self.kill_buffer.is_empty() {
            return;
        }
        if self.input.len() + self.kill_buffer.len() > MAX_MESSAGE_BYTES {
            self.set_status("Yank exceeds 256 KiB input budget; draft unchanged");
            return;
        }
        let mut proposed = self.input.clone();
        proposed.insert_str(self.cursor, &self.kill_buffer);
        let mut paste = self.paste.clone();
        paste.after_edit(&proposed, self.cursor, self.cursor, self.kill_buffer.len());
        let mut cursor = self.cursor + self.kill_buffer.len();
        if !grapheme_boundary(&proposed, cursor) {
            cursor = next_boundary(&proposed, cursor);
        }
        if !self.input_fits_checkpoint(&proposed, cursor, &paste) {
            self.set_status("Yank exceeds 4 MiB project checkpoint budget; draft unchanged");
            return;
        }
        self.input = proposed;
        self.paste = paste;
        self.cursor = cursor;
        self.edited_input();
    }

    fn replace_input_range(&mut self, start: usize, end: usize, text: &str) -> bool {
        let (start, end) = self.expanded_input_range(start, end);
        if self.input.len() - (end - start) + text.len() > MAX_MESSAGE_BYTES {
            self.set_status(format!(
                "Input exceeds {} bytes; draft unchanged",
                MAX_MESSAGE_BYTES
            ));
            return false;
        }
        self.input.replace_range(start..end, text);
        self.paste.after_edit(&self.input, start, end, text.len());
        self.cursor = start + text.len();
        if !grapheme_boundary(&self.input, self.cursor) {
            self.cursor = next_boundary(&self.input, self.cursor);
        }
        self.edited_input();
        true
    }

    fn insert_text(&mut self, text: &str) {
        let text = sanitize_input_with_limit(text, MAX_MESSAGE_BYTES.saturating_add(4));
        if !text.is_empty() {
            self.replace_input_range(self.cursor, self.cursor, &text);
        }
    }

    fn insert_paste(&mut self, text: &str) {
        let text = sanitize_input_with_limit(text, MAX_MESSAGE_BYTES.saturating_add(4));
        let count = text.chars().count();
        if count <= LARGE_PASTE_CHARS {
            self.insert_text(&text);
            return;
        }
        if self.input.len() + text.len() > MAX_MESSAGE_BYTES {
            self.set_status(format!(
                "Input exceeds {} bytes; draft unchanged",
                MAX_MESSAGE_BYTES
            ));
            return;
        }
        let start = self.cursor;
        let end = start + text.len();
        let mut proposed = self.input.clone();
        proposed.insert_str(start, &text);
        let mut paste = self.paste.clone();
        paste.after_edit(&proposed, start, start, text.len());
        if grapheme_boundary(&proposed, start) && grapheme_boundary(&proposed, end) {
            let Some(next_id) = paste.next_id.checked_add(1) else {
                self.set_status("Paste identifiers exhausted; draft unchanged");
                return;
            };
            paste.folds.push(PasteFold {
                id: paste.next_id,
                start_byte: start,
                end_byte: end,
                char_count: count,
            });
            paste.next_id = next_id;
            paste.folds.sort_by_key(|fold| fold.start_byte);
        }
        if !self.input_fits_checkpoint(&proposed, end, &paste) {
            self.set_status("Paste exceeds 4 MiB project checkpoint budget; draft unchanged");
            return;
        }
        self.input = proposed;
        self.paste = paste;
        self.cursor = end;
        if !grapheme_boundary(&self.input, self.cursor) {
            self.cursor = next_boundary(&self.input, self.cursor);
        }
        self.edited_input();
    }

    fn input_fits_checkpoint(&self, proposed: &str, cursor: usize, paste: &PasteMetadata) -> bool {
        let mut checkpoint = self.project_checkpoint();
        if let Some(states) = checkpoint["project_state"].as_array_mut()
            && let Some(project) = states
                .iter_mut()
                .find(|row| row["name"] == self.active_project())
        {
            let draft = &mut project["draft"];
            draft["input"] = serde_json::json!(proposed);
            draft["cursor"] = serde_json::json!(cursor);
            draft["paste_folds"] = serde_json::json!(paste.folds);
            draft["next_paste_id"] = serde_json::json!(paste.next_id);
        }
        serde_json::to_vec_pretty(&checkpoint)
            .is_ok_and(|bytes| bytes.len() <= MAX_PROJECT_CHECKPOINT_BYTES)
    }

    fn delete_previous_char(&mut self) {
        if self.cursor > 0 {
            self.replace_input_range(self.adjacent_cursor(false), self.cursor, "");
        }
    }

    fn delete_next_char(&mut self) {
        if self.cursor < self.input.len() {
            self.replace_input_range(self.cursor, self.adjacent_cursor(true), "");
        }
    }

    fn delete_previous_word(&mut self) {
        self.kill_input_range(self.word_boundary(false), self.cursor);
    }

    fn stable_paste(&self) -> &PasteMetadata {
        self.history_draft
            .as_ref()
            .map_or(&self.paste, |draft| &draft.paste)
    }

    fn stable_input(&self) -> (&str, usize) {
        self.history_draft
            .as_ref()
            .map(|draft| (draft.text.as_str(), draft.cursor))
            .unwrap_or((&self.input, self.cursor))
    }

    fn cancel_history_preview(&mut self) {
        self.touch_editor_draft();
        if let Some(draft) = self.history_draft.take() {
            self.input = draft.text;
            self.cursor = draft.cursor;
            self.paste.next_id = self.paste.next_id.max(draft.paste.next_id);
            self.paste.folds = draft.paste.folds;
        }
        self.history_search = None;
        self.history_cursor = None;
        self.palette_dismissed = true;
        self.dirty = true;
    }

    fn history_move(&mut self, direction: isize) {
        self.touch_editor_draft();
        if self.history.is_empty() || (direction > 0 && self.history_cursor.is_none()) {
            return;
        }
        if self.history_draft.is_none() {
            self.history_draft = Some(InputDraft {
                text: self.input.clone(),
                cursor: self.cursor,
                paste: self.paste.clone(),
            });
        }
        let current = self.history_cursor.unwrap_or(self.history.len());
        let next = if direction < 0 {
            current.saturating_sub(1)
        } else {
            (current + 1).min(self.history.len())
        };
        if next == self.history.len() {
            self.cancel_history_preview();
        } else {
            self.history_cursor = Some(next);
            self.input = self.history[next].clone();
            self.paste.folds.clear();
            self.cursor = self.input.len();
        }
        self.palette_dismissed = true;
        self.input_scroll = 0;
        self.preferred_column = None;
        self.dirty = true;
    }

    fn begin_history_search(&mut self) {
        if self.history_draft.is_none() {
            self.history_draft = Some(InputDraft {
                text: self.input.clone(),
                cursor: self.cursor,
                paste: self.paste.clone(),
            });
        }
        self.history_search = Some(HistorySearch {
            query: String::new(),
            before: None,
            matched: None,
        });
        self.search_history();
    }

    fn search_history(&mut self) {
        self.touch_editor_draft();
        let search = self.history_search.as_mut().unwrap();
        let before = search.before.unwrap_or(self.history.len());
        search.matched = self
            .history
            .iter()
            .enumerate()
            .take(before)
            .rev()
            .find(|(_, text)| text.contains(&search.query))
            .map(|(index, _)| index);
        if let Some(index) = search.matched {
            self.input = self.history[index].clone();
            self.paste.folds.clear();
            self.cursor = self.input.len();
        } else if let Some(draft) = &self.history_draft {
            self.input = draft.text.clone();
            self.cursor = draft.cursor;
            self.paste.folds = draft.paste.folds.clone();
            self.paste.next_id = self.paste.next_id.max(draft.paste.next_id);
        }
        self.palette_dismissed = true;
        self.input_scroll = 0;
        self.dirty = true;
    }

    fn history_search_text(&mut self, text: &str) {
        let search = self.history_search.as_mut().unwrap();
        let text = sanitize_input_with_limit(text, 1025).replace('\n', " ");
        if search.query.len() + text.len() > 1024 {
            self.set_status("History query exceeds1024 bytes; query unchanged");
            return;
        }
        search.query.push_str(&text);
        search.before = None;
        self.search_history();
    }

    fn history_search_key(&mut self, key: KeyEvent) -> TuiAction {
        match key.code {
            KeyCode::Esc | KeyCode::Char('c')
                if key.code == KeyCode::Esc || key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.cancel_history_preview()
            }
            KeyCode::Enter => {
                if self
                    .history_search
                    .as_ref()
                    .is_some_and(|s| s.matched.is_some())
                {
                    self.history_search = None;
                    self.history_draft = None;
                    self.history_cursor = None;
                } else {
                    self.cancel_history_preview();
                }
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let search = self.history_search.as_mut().unwrap();
                search.before = search.matched;
                self.search_history();
            }
            KeyCode::Backspace => {
                let search = self.history_search.as_mut().unwrap();
                let boundary = previous_boundary(&search.query, search.query.len());
                search.query.truncate(boundary);
                search.before = None;
                self.search_history();
            }
            KeyCode::Char(c)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                self.history_search_text(&c.to_string())
            }
            _ => {}
        }
        self.dirty = true;
        TuiAction::Redraw
    }

    fn word_boundary(&self, forward: bool) -> usize {
        let view = self.projection();
        let cursor = view.display_cursor(self.cursor);
        let boundary = |offset| view.canonical_cursor(offset, forward);
        // Traverse once even for a full256KiB single-word draft; repeatedly
        // finding the previous grapheme would make word movement quadratic.
        if forward {
            let mut in_space = false;
            for (offset, grapheme) in view
                .text
                .grapheme_indices(true)
                .filter(|(offset, _)| *offset >= cursor)
            {
                let space = grapheme.chars().next().is_some_and(char::is_whitespace);
                if in_space && !space {
                    return boundary(offset);
                }
                if space {
                    in_space = true;
                }
            }
            self.input.len()
        } else {
            let mut in_word = false;
            for (offset, grapheme) in view.text[..cursor].grapheme_indices(true).rev() {
                let space = grapheme.chars().next().is_some_and(char::is_whitespace);
                if in_word && space {
                    return boundary(offset + grapheme.len());
                }
                if !space {
                    in_word = true;
                }
            }
            0
        }
    }
    fn move_word(&mut self, forward: bool) {
        self.touch_editor_draft();
        self.cursor = self.word_boundary(forward);
        self.preferred_column = None;
        self.dirty = true;
    }

    /// Move to the adjacent logical line while retaining the requested
    /// display-cell column.  Every offset is derived from UTF-8 boundaries,
    /// so a wide or multibyte glyph can never leave `cursor` mid-codepoint.
    fn move_vertical(&mut self, direction: isize) {
        self.touch_editor_draft();
        let view = self.projection();
        let cursor = view.display_cursor(self.cursor);
        let current_start = line_start(&view.text, cursor);
        let current_end = line_end(&view.text, cursor);
        let current_column = UnicodeWidthStr::width(&view.text[current_start..cursor]);
        let target_column = self.preferred_column.unwrap_or(current_column);
        let target_start = if direction < 0 {
            if current_start == 0 {
                return;
            }
            let previous_end = current_start.saturating_sub(1);
            line_start(&view.text, previous_end)
        } else {
            if current_end >= view.text.len() {
                return;
            }
            current_end + 1
        };
        let target_end = line_end(&view.text, target_start);
        let target = byte_at_column(&view.text[target_start..target_end], target_column)
            .saturating_add(target_start)
            .min(target_end);
        self.cursor = view.canonical_cursor(target, direction > 0);
        self.preferred_column = Some(target_column);
        self.input_scroll = 0;
        self.dirty = true;
    }

    /// Render a complete frame.  No minimum dimensions are assumed: zero and
    /// one-cell areas are valid during terminal resize transitions.
    pub fn render(&mut self, frame: &mut Frame<'_>, title: &str) {
        let area = frame.area();
        if area != self.last_area {
            self.last_area = area;
            self.cached_transcript = None;
            self.input_scroll = 0;
        }
        let prompt_width = usize::from(area.width.saturating_sub(2)).max(1);
        let prompt_lines = wrap_plain(&self.projection().text, prompt_width).len();
        let desired_input_height = prompt_lines.min(MAX_INPUT_LINES).saturating_add(2);
        // Keep header/footer visible whenever there is room, and let the
        // transcript yield rows to the prompt first.  Tiny resize states can
        // legitimately collapse the prompt to zero rows.
        let input_height = if area.height > 3 {
            u16::try_from(desired_input_height)
                .unwrap_or(u16::MAX)
                .min(area.height.saturating_sub(3))
        } else {
            0
        };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(input_height),
                Constraint::Length(1),
            ])
            .split(area);
        self.render_header(frame, chunks[0], title);
        self.render_transcript(frame, chunks[1]);
        self.render_input(frame, chunks[2]);
        self.render_footer(frame, chunks[3]);
        if let Some(browser) = self.transcript_browser.as_mut() {
            browser.render(frame);
        }
        self.render_approval(frame);
    }

    fn render_header(&self, frame: &mut Frame<'_>, area: Rect, title: &str) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let marker = if self.busy {
            ['|', '/', '-', '\\'][self.spinner_tick % 4]
        } else {
            ' '
        };
        let style = if self.busy {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {title} "),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {marker} "), style),
                Span::styled(
                    format!(
                        "{}{}",
                        self.status,
                        if self.busy {
                            format!(" ({}s · Ctrl-C)", self.transcript_ux.seconds())
                        } else {
                            String::new()
                        }
                    ),
                    style,
                ),
                Span::styled(
                    self.transcript_ux
                        .copy_notice
                        .as_ref()
                        .filter(|(_, t)| t.elapsed() < Duration::from_secs(5))
                        .map(|(text, _)| format!(" | {text}"))
                        .unwrap_or_default(),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    self.transcript_ux
                        .model
                        .as_ref()
                        .map(|m| {
                            let actual = self
                                .transcript_ux
                                .response_model
                                .as_ref()
                                .filter(|actual| *actual != m)
                                .map(|actual| format!(" (last {})", inline_token(actual, 40)))
                                .unwrap_or_default();
                            format!(" | model {}{}", inline_token(m, 64), actual)
                        })
                        .unwrap_or_default(),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    self.transcript_ux
                        .usage
                        .map(|u| format!(" | in {} out {}", u.input_tokens, u.output_tokens))
                        .unwrap_or_else(|| " | tokens —".into()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    self.transcript_ux
                        .history_tokens
                        .zip(self.transcript_ux.context_limit)
                        .map(|(used, budget)| format!(" | history ~{used}/{budget}"))
                        .unwrap_or_default(),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    {
                        let n = self.project_drafts.values().filter(|d| d.busy).count();
                        if n > 0 {
                            format!(" | {n} background")
                        } else {
                            String::new()
                        }
                    },
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    self.project_metadata(self.active_project())
                        .filter(|m| !m.cwd.is_empty())
                        .map(|m| format!(" | {}", m.cwd))
                        .unwrap_or_default(),
                    Style::default().fg(Color::DarkGray),
                ),
            ])),
            area,
        );
    }

    fn render_transcript(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.render_transcript_pane(frame, area, false);
    }

    fn render_transcript_pane(&mut self, frame: &mut Frame<'_>, area: Rect, goal: bool) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let title = if goal {
            " Goal "
        } else if self.fold_tool_logs {
            " Conversation (tools folded) "
        } else {
            " Conversation "
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(
                if self.workspace_layout.focused
                    == Some(if goal {
                        PaneId::GoalConversation
                    } else {
                        conversation_pane_for_tab(self.workspace_layout.tab)
                    })
                {
                    crate::persona::color(&self.persona)
                } else {
                    Color::DarkGray
                },
            ))
            .title(title);
        let inner = block.inner(area);
        let width = usize::from(inner.width).max(1);
        if goal {
            let mut lines = transcript_lines(&self.goal_messages, width, &self.persona);
            lines.truncate(MAX_RENDER_LINES);
            frame.render_widget(
                Paragraph::new(lines).block(block).scroll((
                    *self
                        .pane_scroll
                        .get(&PaneId::GoalConversation)
                        .unwrap_or(&0),
                    0,
                )),
                area,
            );
            return;
        }
        if self
            .cached_transcript
            .as_ref()
            .is_none_or(|(cached_width, _)| *cached_width != inner.width)
        {
            let messages = self.transcript_messages();
            let lines = transcript_window(
                messages.iter().map(|(id, message)| (*id, message)),
                width,
                &self.persona,
            );
            self.cached_transcript = Some((inner.width, lines));
        }
        let lines = self
            .cached_transcript
            .as_ref()
            .map_or(&[][..], |(_, lines)| lines.as_slice());
        let visible = usize::from(inner.height);
        let max_scroll = lines.len().saturating_sub(visible);
        let start = if let Some(anchor) = self.transcript_ux.scroll_anchor {
            self.transcript_ux
                .anchored_position
                .and_then(|position| {
                    lines
                        .iter()
                        .position(|row| row.position == Some(position))
                        .or_else(|| {
                            lines
                                .iter()
                                .find_map(|row| row.position)
                                .filter(|first| position < *first)
                                .map(|_| 0)
                        })
                })
                .unwrap_or(anchor)
                .min(max_scroll)
        } else {
            max_scroll.saturating_sub(self.scroll.min(max_scroll))
        };
        let scroll = max_scroll.saturating_sub(start);
        if self.scroll != scroll {
            self.scroll = scroll;
            self.project_checkpoint_dirty = true;
        }
        self.transcript_ux.last_start = start;
        self.transcript_ux.visible_rows = visible;
        if self.scroll == 0 {
            self.transcript_ux.scroll_anchor = None;
            self.transcript_ux.anchored_position = None;
        } else {
            self.transcript_ux.scroll_anchor = Some(start);
            self.transcript_ux.anchored_position = lines.get(start).and_then(|row| row.position);
        }
        let end = (start + visible).min(lines.len());
        let displayed = if start < end {
            lines[start..end]
                .iter()
                .map(|row| row.line.clone())
                .collect()
        } else {
            Vec::new()
        };
        frame.render_widget(Paragraph::new(Text::from(displayed)).block(block), area);
        self.follow_hit = None;
        if self.scroll > 0 && area.width >= 20 {
            let label = format!(" ↓ Latest +{} ", self.scroll);
            let width = (UnicodeWidthStr::width(label.as_str()) as u16).min(area.width - 2);
            let rect = Rect::new(area.right() - width - 1, area.y, width, 1);
            frame.render_widget(
                Paragraph::new(label).style(Style::default().fg(Color::Yellow)),
                rect,
            );
            self.follow_hit = Some(rect);
        }
    }

    /// Build the render-only message view.  Folding never mutates the
    /// canonical bounded queue, which means expanding the logs restores the
    /// exact lifecycle entries and ordinary transcript ordering.
    fn transcript_messages(&self) -> VecDeque<(u64, TuiMessage)> {
        let mut visible = VecDeque::new();
        let mut folded = 0usize;
        for (index, message) in self.messages.iter().enumerate() {
            let id = self
                .transcript_ux
                .message_offset
                .saturating_add(index as u64);
            if message.role == MessageRole::Reasoning && self.transcript_ux.reasoning_folded {
                visible.push_back((
                    id,
                    TuiMessage::new(
                        MessageRole::Reasoning,
                        format!("{} bytes · collapsed · Alt-R expand", message.text.len()),
                    ),
                ));
            } else if message.role == MessageRole::Tool && self.fold_tool_logs {
                folded = folded.saturating_add(1);
            } else {
                visible.push_back((id, message.clone()));
            }
        }
        if folded > 0 {
            let suffix = if folded == 1 { "" } else { "s" };
            visible.push_back((
                self.transcript_ux
                    .message_offset
                    .saturating_add(self.messages.len() as u64),
                TuiMessage::new(
                    MessageRole::System,
                    format!("{folded} tool log{suffix} folded; press Ctrl-O to expand"),
                ),
            ));
        }
        visible
    }

    fn render_input(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if self.busy {
                Color::Yellow
            } else {
                Color::Cyan
            }))
            .title(if let Some(search) = &self.history_search {
                format!(
                    " History search: {} · {} · Ctrl-R older · Enter select · Esc restore ",
                    inline_token(&search.query, 40),
                    if search.matched.is_some() {
                        "match"
                    } else {
                        "no match"
                    }
                )
            } else if self.docked_prompt_rect == Some(area) {
                if self.goal_text.is_empty() {
                    " Prompt · Goal: — · Alt-G edit ".to_owned()
                } else {
                    format!(
                        " Prompt · Goal: {} · Alt-G edit ",
                        inline_token(&self.goal_text, 48)
                    )
                }
            } else if self.paste.folds.is_empty() && self.input.starts_with('/') {
                " Prompt  • command palette active ".to_owned()
            } else {
                " Prompt · Ctrl-R history · Ctrl-J newline ".to_owned()
            });
        let inner = block.inner(area);
        let width = usize::from(inner.width).max(1);
        let projection = self.projection();
        let lines = wrap_plain(&projection.text, width)
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>();
        let visible_height = usize::from(inner.height);
        let (cursor_x, cursor_y) = cursor_position(
            &projection.text,
            projection.display_cursor(self.cursor),
            width,
        );
        let max_scroll = lines.len().saturating_sub(visible_height);
        if visible_height > 0 {
            let mut scroll = self.input_scroll.min(max_scroll);
            let cursor_y = usize::from(cursor_y);
            if cursor_y < scroll {
                scroll = cursor_y;
            } else if cursor_y >= scroll.saturating_add(visible_height) {
                scroll = cursor_y.saturating_add(1).saturating_sub(visible_height);
            }
            self.input_scroll = scroll.min(max_scroll);
        } else {
            self.input_scroll = 0;
        }
        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .block(block)
                .wrap(Wrap { trim: false })
                .scroll((u16::try_from(self.input_scroll).unwrap_or(u16::MAX), 0)),
            area,
        );
        let cursor_y = usize::from(cursor_y).saturating_sub(self.input_scroll);
        let x = inner
            .x
            .saturating_add(cursor_x.min(inner.width.saturating_sub(1)));
        let y = inner.y.saturating_add(
            u16::try_from(cursor_y)
                .unwrap_or(u16::MAX)
                .min(inner.height.saturating_sub(1)),
        );
        frame.set_cursor_position(Position::new(x, y));
    }

    /// Inline Goal editor in the top-left conversation group. Enter commits
    /// through the bounded [`crate::view_model::GoalEdit`]; Esc cancels.
    fn render_goal_editor(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let text = self
            .goal_edit
            .as_ref()
            .map(|edit| edit.text().to_owned())
            .unwrap_or_default();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Goal · Enter save · Esc cancel ");
        let inner = block.inner(area);
        let width = usize::from(inner.width).max(1);
        let lines = wrap_plain(&text, width)
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
        if inner.width > 0 && inner.height > 0 {
            let (cursor_x, cursor_y) = cursor_position(&text, text.len(), width);
            let x = inner
                .x
                .saturating_add(cursor_x.min(inner.width.saturating_sub(1)));
            let y = inner
                .y
                .saturating_add(cursor_y.min(inner.height.saturating_sub(1)));
            frame.set_cursor_position(Position::new(x, y));
        }
    }

    /// True when the arch pane can render its grouped transcript + prompt. The
    /// group is offered at the same Standard/Wide breakpoints as the discussion
    /// group; narrower viewports keep the ordinary pane and bottom strip.
    fn arch_group_active(&self, area: Rect) -> bool {
        matches!(
            Breakpoint::for_size(area.width, area.height),
            Breakpoint::Standard | Breakpoint::Wide
        )
    }

    /// Bounded arch transcript. Arch is a conversation lane owned by the master
    /// session, so it shows operator submissions and host results but never
    /// mutates the discussion transcript.
    fn render_arch_transcript(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let focused = self.left_prompt == LeftPrompt::Arch;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if focused {
                Color::Cyan
            } else {
                Color::DarkGray
            }))
            .title(" Arch · master session ");
        let inner_height = usize::from(block.inner(area).height);
        let mut lines: Vec<Line> = self
            .arch_messages
            .iter()
            .rev()
            .take(inner_height.max(1))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|message| {
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", message.role.label()),
                        Style::default().fg(match message.role {
                            MessageRole::User => Color::Cyan,
                            MessageRole::Error => Color::Red,
                            _ => Color::Gray,
                        }),
                    ),
                    Span::raw(message.text.clone()),
                ])
            })
            .collect();
        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "arch console idle · !cmd bash · text steer",
                Style::default().fg(Color::DarkGray),
            )));
        }
        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .block(block)
                .wrap(Wrap { trim: true }),
            area,
        );
    }

    /// Resident arch prompt rendered inside the left column directly beneath the
    /// arch transcript. It keeps the pane's exact x/width, so arch + Prompt are
    /// one group.
    fn render_arch_input(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let focused = self.left_prompt == LeftPrompt::Arch;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if focused { Color::Cyan } else { Color::Magenta }))
            .title(if focused {
                " Arch prompt · !cmd bash · text steer · Enter send "
            } else {
                " Arch prompt · Alt-M focus "
            });
        let inner = block.inner(area);
        let width = usize::from(inner.width).max(1);
        let lines = wrap_plain(&self.arch_input, width)
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
        if focused && inner.width > 0 && inner.height > 0 {
            let (cursor_x, cursor_y) = cursor_position(&self.arch_input, self.arch_cursor, width);
            let x = inner
                .x
                .saturating_add(cursor_x.min(inner.width.saturating_sub(1)));
            let y = inner
                .y
                .saturating_add(cursor_y.min(inner.height.saturating_sub(1)));
            frame.set_cursor_position(Position::new(x, y));
        }
    }

    fn external_editor_shortcut_available(&self) -> bool {
        self.directory_picker.is_none()
            && self.transcript_browser.is_none()
            && self.history_search.is_none()
            && !self
                .approval_views
                .get(self.active_project())
                .is_some_and(|view| view.focused && !view.requests.is_empty())
            && self.slash_choices().is_empty()
    }

    fn render_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let unsaved = self
            .checkpoint_error
            .as_ref()
            .map(|error| format!(" Draft not saved · {error}"));
        let pending_approval = self
            .approval_views
            .get(self.active_project())
            .filter(|view| !view.focused && !view.requests.is_empty())
            .map(|view| format!(" {} approvals pending · Alt-A review", view.requests.len()));
        let background_approval = self.background_approval_hint();
        let project_status = if let Some(path) = self.status.strip_prefix("Project ready · ") {
            let prefix = " Project ready · ";
            let budget = usize::from(area.width).saturating_sub(UnicodeWidthStr::width(prefix));
            Some(format!("{prefix}{}", menu_source(path, budget)))
        } else if self.status == "Project closed" {
            Some(" Project closed".to_owned())
        } else {
            None
        };
        let text = truncate_to_width(
            if let Some(warning) = unsaved.as_deref() {
                warning
            } else if let Some(hint) = pending_approval.as_deref() {
                hint
            } else if let Some(hint) = background_approval.as_deref() {
                hint
            } else if let Some(status) = project_status.as_deref() {
                status
            } else if self.adjacent_paste().is_some() {
                " Alt-Enter expand paste · Left/Right move · Backspace/Delete remove · Enter send "
            } else if area.width < 100 {
                " Enter send · Ctrl-T folder · Ctrl-Tab project · Ctrl-W close · Ctrl-B/F tab order · Alt-,/. subtab order · Alt-N/I/W subtab · Tab panes · Ctrl-J newline · Ctrl-R history · Ctrl-C stop · /help input "
            } else if self.external_editor_shortcut_available() {
                " Enter send · Ctrl-T folder · Ctrl-Tab project · Ctrl-W close · Ctrl-B/F tab order · Alt-,/. subtab order · Alt-N/I/W subtab · Tab panes · Ctrl-G editor · Ctrl-U line kill · Ctrl-Y yank · Ctrl-C stop · Alt-R reasoning · Alt-C copy · Alt-B blocks · /help input "
            } else {
                " Enter send · Ctrl-T folder · Ctrl-Tab project · Ctrl-W close · Ctrl-B/F tab order · Alt-,/. subtab order · Alt-N/I/W subtab · Tab panes · Ctrl-U line kill · Ctrl-Y yank · Ctrl-C stop · Alt-R reasoning · Alt-C copy · Alt-B blocks · /help input "
            },
            usize::from(area.width),
        );
        frame.render_widget(
            Paragraph::new(Span::styled(
                text,
                Style::default().fg(
                    if unsaved.is_none()
                        && (pending_approval.is_some() || background_approval.is_some())
                    {
                        Color::Yellow
                    } else {
                        Color::DarkGray
                    },
                ),
            )),
            area,
        );
    }

    /// Render the production workspace view using the bounded [`LayoutModel`].
    ///
    /// This is deliberately a small adapter rather than a second UI toolkit:
    /// the model owns breakpoint/visibility decisions, while Ratatui only
    /// receives clipped rectangles and renders the existing transcript and
    /// prompt.  Browser and PTY panes are not enabled by default, so their
    /// adapters cannot accidentally start a child process or network view.
    pub fn render_bentobox(&mut self, frame: &mut Frame<'_>, title: &str) {
        let area = frame.area();
        if area.width == 0 || area.height == 0 {
            return;
        }
        if area != self.last_area {
            self.last_area = area;
            self.cached_transcript = None;
            self.input_scroll = 0;
        }
        let prompt_width = usize::from(area.width.saturating_sub(2)).max(1);
        let prompt_lines = wrap_plain(&self.projection().text, prompt_width).len();
        let desired_input_height = prompt_lines.min(MAX_INPUT_LINES).saturating_add(2);
        // Conversation + Prompt are one resident group. At a roomy viewport on
        // the project workspace the prompt is rendered inside the top-left
        // conversation pane, so it is exactly as wide as the left column.
        // Active overlays (palette, completion, history search, pickers) and
        // narrow/compact viewports keep the full-width bottom strip so menus
        // and long lines retain room.
        let overlay_active = !self.slash_choices().is_empty()
            || self.current_file_completion().is_some()
            || self.history_search.is_some()
            || self.directory_picker.is_some()
            || self.transcript_browser.is_some();
        let group_prompt = !overlay_active
            && self.workspace_layout.tab == TabId::Project
            && matches!(
                Breakpoint::for_size(area.width, area.height),
                Breakpoint::Standard | Breakpoint::Wide
            );
        let input_height = if area.height > 4 {
            u16::try_from(desired_input_height)
                .unwrap_or(u16::MAX)
                .min(area.height.saturating_sub(4))
        } else {
            0
        };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(input_height),
                Constraint::Length(1),
            ])
            .split(area);
        self.render_workspace_tabs(frame, chunks[0]);
        self.render_header(frame, chunks[1], title);
        self.docked_prompt_rect = None;
        self.dock_prompt = group_prompt;
        self.render_workspace(frame, chunks[2]);
        // When the group could not render the prompt (for example the
        // conversation pane is collapsed) fall back to the bottom strip so the
        // prompt is never unreachable.
        if self.docked_prompt_rect.is_none() {
            if self.goal_edit.is_some() {
                self.render_goal_editor(frame, chunks[3]);
            } else {
                self.render_input(frame, chunks[3]);
            }
        }
        self.render_footer(frame, chunks[4]);
        let prompt_anchor = self.docked_prompt_rect.unwrap_or(chunks[3]);
        self.render_slash_choices(frame, prompt_anchor);
        if let Some(picker) = self.directory_picker.as_mut() {
            picker.render(frame);
        }
        if let Some(browser) = self.transcript_browser.as_mut() {
            browser.render(frame);
        }
        self.render_approval(frame);
    }

    fn render_workspace_tabs(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.project_hits.clear();
        if area.width == 0 || area.height == 0 {
            return;
        }
        let limit = area.right();
        let prefix = if area.width > 40 {
            truncate_to_width(" zenpi | projects: ", usize::from(limit - area.x))
        } else {
            String::new()
        };
        frame.render_widget(
            Paragraph::new(prefix.clone()),
            Rect::new(area.x, area.y, limit - area.x, 1),
        );
        let mut x = area.x + UnicodeWidthStr::width(prefix.as_str()) as u16;
        // Left-aligned controls: `[-]` closes the active workspace, `[+]`
        // opens a new project on the current layer. Horizontal reordering is
        // done by dragging a tab, not by `[<] [>]` buttons.
        if area.width >= 16 {
            let minus = Rect::new(x, area.y, 4, 1);
            frame.render_widget(
                Paragraph::new(" [-]").style(Style::default().fg(Color::Cyan)),
                minus,
            );
            self.project_hits.push((minus, usize::MAX - 2));
            x += 4;
            let plus = Rect::new(x, area.y, 4, 1);
            frame.render_widget(
                Paragraph::new(" [+]").style(Style::default().fg(Color::Green)),
                plus,
            );
            self.project_hits.push((plus, self.project_tabs.len()));
            x += 4;
        }
        for index in 0..self.project_tabs.len() {
            if x >= limit {
                break;
            }
            let label = self.project_label(index);
            let approvals = self.project_approval_count(index);
            let attention = if approvals > 0 {
                format!("!{approvals} ")
            } else {
                String::new()
            };
            let text = truncate_to_width(
                &format!(
                    "{}{}{}{} ",
                    if index == self.active_project {
                        "["
                    } else {
                        " "
                    },
                    attention,
                    label,
                    if index == self.active_project {
                        "]"
                    } else {
                        " "
                    }
                ),
                usize::from(limit - x).min(32),
            );
            let width = UnicodeWidthStr::width(text.as_str()) as u16;
            if width == 0 {
                break;
            }
            let rect = Rect::new(x, area.y, width, 1);
            frame.render_widget(
                Paragraph::new(text).style(Style::default().fg(if approvals > 0 {
                    Color::Yellow
                } else if let Some(style) = self.project_style_color(index) {
                    style
                } else if index == self.active_project {
                    Color::Cyan
                } else {
                    Color::White
                })),
                rect,
            );
            self.project_hits.push((rect, index));
            x += width;
        }
        if area.height >= 2 {
            self.render_subtabs(frame, Rect::new(area.x, area.y + 1, area.width, 1));
        }
    }

    /// Layer-2 row: the active project's sub-tabs plus the two add choices
    /// (`[~]` work in the current place, `[+]` create a new worktree).
    fn render_subtabs(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.subtab_hits.clear();
        if area.width == 0 || area.height == 0 {
            return;
        }
        let tabs = self.subtabs();
        let active = self.active_subtab();
        let active_concurrency = tabs.get(active).map(|tab| tab.concurrency).unwrap_or(1);
        let controls = format!(" ↑ {active_concurrency} ↓ [~] [+] ");
        let controls_width = UnicodeWidthStr::width(controls.as_str()) as u16;
        let limit = area.right().saturating_sub(controls_width);
        let prefix = "  └ ";
        frame.render_widget(
            Paragraph::new(prefix).style(Style::default().fg(Color::DarkGray)),
            Rect::new(area.x, area.y, UnicodeWidthStr::width(prefix) as u16, 1),
        );
        let mut x = area.x + UnicodeWidthStr::width(prefix) as u16;
        for (index, tab) in tabs.iter().enumerate() {
            if x >= limit {
                break;
            }
            let text = truncate_to_width(
                &format!(
                    "{}{}{} ",
                    if index == active { "[" } else { " " },
                    tab.name,
                    if index == active { "]" } else { " " }
                ),
                usize::from(limit.saturating_sub(x)).min(28),
            );
            let width = UnicodeWidthStr::width(text.as_str()) as u16;
            if width == 0 {
                break;
            }
            let rect = Rect::new(x, area.y, width, 1);
            frame.render_widget(
                Paragraph::new(text).style(Style::default().fg(if index == active {
                    Color::Cyan
                } else {
                    Color::Gray
                })),
                rect,
            );
            self.subtab_hits.push((rect, SubTabHit::Select(index)));
            x += width;
        }
        let mut cx = limit;
        let up = Rect::new(cx, area.y, 3, 1);
        frame.render_widget(
            Paragraph::new(" ↑").style(Style::default().fg(Color::Cyan)),
            up,
        );
        self.subtab_hits
            .push((up, SubTabHit::ConcurrencyUp(active)));
        cx += 3;
        let number = format!(" {active_concurrency} ");
        let number_width = UnicodeWidthStr::width(number.as_str()) as u16;
        let rect = Rect::new(cx, area.y, number_width, 1);
        frame.render_widget(
            Paragraph::new(number).style(Style::default().fg(Color::White)),
            rect,
        );
        cx += number_width;
        let down = Rect::new(cx, area.y, 3, 1);
        frame.render_widget(
            Paragraph::new(" ↓").style(Style::default().fg(Color::Cyan)),
            down,
        );
        self.subtab_hits
            .push((down, SubTabHit::ConcurrencyDown(active)));
        cx += 3;
        let in_place = Rect::new(cx, area.y, 4, 1);
        frame.render_widget(
            Paragraph::new(" [~]").style(Style::default().fg(Color::Magenta)),
            in_place,
        );
        self.subtab_hits.push((in_place, SubTabHit::AddInPlace));
        cx += 4;
        let worktree = Rect::new(cx, area.y, 4, 1);
        frame.render_widget(
            Paragraph::new(" [+]").style(Style::default().fg(Color::Green)),
            worktree,
        );
        self.subtab_hits.push((worktree, SubTabHit::AddWorktree));
    }

    fn render_workspace(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.workspace_area = area;
        if area.width == 0 || area.height == 0 {
            return;
        }
        let adapter = BentoBoxLayoutAdapter::new(&self.workspace_layout, area);
        let canonical = conversation_pane_for_tab(adapter.tab());
        self.arch_prompt_rect = None;
        for pane in adapter.visible_panes() {
            if pane.rect.width == 0 || pane.rect.height == 0 {
                continue;
            }
            if pane.id == PaneId::GoalConversation {
                self.render_transcript_pane(frame, pane.rect, true);
            } else if pane.id == PaneId::Arch && self.arch_group_active(area) {
                // Arch + Prompt are one left-column group (ZS1-148): the arch
                // transcript keeps the upper rows and the master-session
                // console prompt is rendered directly beneath it with the exact
                // same x/width, never spilling into the center column.
                let source =
                    PaneRect::new(pane.rect.x, pane.rect.y, pane.rect.width, pane.rect.height);
                let (conversation, prompt) = arch_prompt_group(source, ARCH_PROMPT_PANE_ROWS);
                let conversation = Rect::new(
                    conversation.x,
                    conversation.y,
                    conversation.width,
                    conversation.height,
                );
                let prompt = Rect::new(prompt.x, prompt.y, prompt.width, prompt.height);
                self.render_arch_transcript(frame, conversation);
                self.arch_prompt_rect = Some(prompt);
                self.render_arch_input(frame, prompt);
            } else if pane.id == canonical
                && pane.id == PaneId::ProjectConversation
                && self.dock_prompt
            {
                // Conversation + Prompt are one left-column group: the
                // transcript keeps the upper rows and the resident prompt (or
                // the inline Goal editor) is rendered directly beneath it with
                // the exact same x/width.
                let source =
                    PaneRect::new(pane.rect.x, pane.rect.y, pane.rect.width, pane.rect.height);
                let (transcript, prompt) = conversation_prompt_group(source, PROMPT_PANE_ROWS);
                let transcript = Rect::new(
                    transcript.x,
                    transcript.y,
                    transcript.width,
                    transcript.height,
                );
                let prompt = Rect::new(prompt.x, prompt.y, prompt.width, prompt.height);
                self.render_transcript(frame, transcript);
                self.docked_prompt_rect = Some(prompt);
                if self.goal_edit.is_some() {
                    self.render_goal_editor(frame, prompt);
                } else {
                    self.render_input(frame, prompt);
                }
            } else if pane.id == canonical
                || matches!(
                    pane.id,
                    PaneId::ProjectConversation
                        | PaneId::LearnConversation
                        | PaneId::ReviewConversation
                        | PaneId::SessionConversation
                )
            {
                // Every workspace keeps its ordinary conversation lane
                // visible alongside Goal; tab switching changes context, not
                // the existence of either hot zone.
                self.render_transcript(frame, pane.rect);
            } else {
                self.render_workspace_pane(frame, pane.id, pane.rect);
            }
        }
    }

    fn render_workspace_pane(&self, frame: &mut Frame<'_>, id: PaneId, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        // Keep the split affordance visible without adding a second toolbar:
        // the centered grip is a stable mouse target at every breakpoint.
        let title = format!(" {}  ⋮ ", workspace_pane_title(id));
        let content = match id {
            PaneId::Gantt => self.gantt_pane_content(),
            PaneId::Arch => self.arch_pane_content(),
            PaneId::Execution => self.execution_terminal_content(),
            PaneId::Resources => self.resource_pane_content(),
            PaneId::SessionList => self.session_browser_content(),
            PaneId::ReplayControls => self.replay_pane_content(),
            PaneId::Terminal => self.terminal_snapshot.clone(),
            PaneId::EventTimeline => self.session_timeline_content(),
            // Goal is rendered as a transcript in `render_workspace`; this
            // branch is retained for compact-layout fallback paths.
            PaneId::GoalConversation => "-".into(),
            PaneId::ProjectConversation
            | PaneId::LearnConversation
            | PaneId::ReviewConversation
            | PaneId::SessionConversation => "-".into(),
            PaneId::LearnResources
            | PaneId::LearnQueue
            | PaneId::LearnMapping
            | PaneId::Evidence
            | PaneId::Checks
            | PaneId::ApprovalQueue
            | PaneId::Diff
            | PaneId::Browser => "-".into(),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(
                Style::default().fg(if self.workspace_layout.focused == Some(id) {
                    Color::Cyan
                } else {
                    Color::DarkGray
                }),
            )
            .title(title);
        let scroll = if id == PaneId::SessionList {
            self.session_browser_scroll(area)
        } else {
            usize::from(*self.pane_scroll.get(&id).unwrap_or(&0))
        };
        let text = match id {
            PaneId::Gantt => style_status_marks(&content),
            PaneId::Resources => style_resource_pane(&content),
            _ => ratatui::text::Text::from(content),
        };
        let paragraph = Paragraph::new(text)
            .style(Style::default().fg(Color::DarkGray))
            .block(block)
            .scroll((scroll.min(usize::from(u16::MAX)) as u16, 0));
        // Session selection and mouse hits address items, so every item must
        // occupy one row even when the pane is too narrow for its full label.
        let paragraph = if id == PaneId::SessionList {
            paragraph
        } else {
            paragraph.wrap(Wrap { trim: true })
        };
        frame.render_widget(paragraph, area);
    }

    fn session_browser_scroll(&self, area: Rect) -> usize {
        let visible = usize::from(area.height.saturating_sub(2)).max(1);
        self.session_browser_cursor
            .saturating_sub(visible.saturating_sub(1))
            .max(usize::from(
                *self.pane_scroll.get(&PaneId::SessionList).unwrap_or(&0),
            ))
    }

    fn session_browser_content(&self) -> String {
        if self.session_browser.is_empty() {
            return "No sessions".into();
        }
        self.session_browser
            .iter()
            .take(32)
            .enumerate()
            .map(|(index, summary)| {
                format!(
                    "{}{}  turns={} events={} next={}",
                    if index == self.session_browser_cursor {
                        "> "
                    } else {
                        "  "
                    },
                    summary.session_id,
                    summary.turn_count,
                    summary.event_count,
                    summary.next_seq
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn refresh_session_browser(&mut self, session: &crate::session::SessionStore) {
        if self.async_session_browser {
            // Snapshot refresh is called under the agent mutex on the host.
            // Only copy ownership here; the browser worker performs all scans.
            let directory = session
                .path()
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_owned();
            if self.session_browser_refresh.directory.as_ref() != Some(&directory) {
                self.session_browser_refresh = SessionBrowserRefresh {
                    directory: Some(directory),
                    ..SessionBrowserRefresh::default()
                };
                self.session_browser.clear();
                self.session_browser_cursor = 0;
                self.dirty = true;
            }
            self.session_browser_refresh.owner =
                Some((session.path().to_owned(), session.session_id().to_owned()));
            return;
        }
        let directory = session
            .path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let directory_changed =
            self.session_browser_refresh.directory.as_deref() != Some(directory);
        let owner_changed = self
            .session_snapshot
            .as_ref()
            .is_none_or(|snapshot| snapshot.session_id != session.session_id());
        if directory_changed {
            self.session_browser_refresh = SessionBrowserRefresh {
                directory: Some(directory.to_owned()),
                ..SessionBrowserRefresh::default()
            };
            self.session_browser.clear();
            self.session_browser_cursor = 0;
            self.dirty = true;
        } else if !owner_changed
            && self
                .session_browser_refresh
                .last_checked
                .is_some_and(|last| last.elapsed() < Duration::from_millis(500))
        {
            return;
        }
        self.session_browser_refresh.last_checked = Some(Instant::now());
        let result = match &self.session_browser_refresh.query {
            Some(query) => crate::session::search_sessions(directory, query, 32)
                .map(|hits| hits.into_iter().map(|hit| hit.summary).collect()),
            None => crate::session::list_sessions(directory),
        };
        // A transient scan failure does not erase the user's current selection.
        // The host still validates the selected journal when opening it.
        let Ok(mut summaries) = result else {
            return;
        };
        summaries.truncate(32);
        if summaries == self.session_browser {
            return;
        }
        let selected = self.selected_session_browser_path();
        self.session_browser_cursor = selected
            .and_then(|path| summaries.iter().position(|summary| summary.path == path))
            .unwrap_or_else(|| {
                self.session_browser_cursor
                    .min(summaries.len().saturating_sub(1))
            });
        self.session_browser = summaries;
        self.dirty = true;
    }

    pub fn search_session_browser(
        &mut self,
        session: &crate::session::SessionStore,
        query: &str,
    ) -> Result<usize, crate::session::SessionError> {
        let directory = session
            .path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let hits = crate::session::search_sessions(directory, query, 32)?;
        self.session_browser = hits.iter().map(|hit| hit.summary.clone()).collect();
        self.session_browser_cursor = 0;
        self.session_browser_refresh = SessionBrowserRefresh {
            directory: Some(directory.to_owned()),
            query: Some(query.to_owned()),
            last_checked: Some(Instant::now()),
            ..SessionBrowserRefresh::default()
        };
        self.dirty = true;
        Ok(hits.len())
    }

    pub fn session_browser_cursor(&self) -> usize {
        self.session_browser_cursor
    }

    pub fn move_session_browser_cursor(&mut self, delta: isize) {
        if self.session_browser.is_empty() {
            self.session_browser_cursor = 0;
            return;
        }
        let max = self.session_browser.len().min(32).saturating_sub(1) as isize;
        self.session_browser_cursor =
            (self.session_browser_cursor as isize + delta).clamp(0, max) as usize;
        self.dirty = true;
    }

    /// Return the repository path selected in the bounded session browser.
    /// The host owns opening and recovery; this accessor only exposes the
    /// already-loaded, validated summary path.
    pub fn selected_session_browser_path(&self) -> Option<&str> {
        self.session_browser
            .get(self.session_browser_cursor)
            .map(|summary| summary.path.as_str())
    }

    fn replay_pane_content(&self) -> String {
        match &self.session_snapshot {
            Some(snapshot) => format!(
                "Journal next sequence: {}\nTransport ACK: unavailable\nReconnect: owner required\nMailbox: owner required",
                snapshot.journal_next_sequence,
            ),
            None => "Journal cursor unavailable\nReconnect: owner required".into(),
        }
    }

    fn session_timeline_content(&self) -> String {
        let Some(snapshot) = &self.session_snapshot else {
            return "Journal unavailable".into();
        };
        let mut content = format!(
            "Active leaf: {}\n{}",
            snapshot.active_leaf.as_deref().unwrap_or("root"),
            snapshot.timeline.join("\n")
        );
        if snapshot.earlier_records > 0 {
            content.push_str(&format!(
                "\n{} earlier records omitted",
                snapshot.earlier_records
            ));
        }
        content
    }

    fn resource_pane_content(&self) -> String {
        let Some(snapshot) = self.resource_snapshot.as_ref() else {
            return match &self.resource_status {
                ResourcePaneStatus::Idle => "Waiting for workspace snapshot".into(),
                ResourcePaneStatus::Collecting => "Collecting workspace snapshot...".into(),
                ResourcePaneStatus::Failed(error) => format!("Snapshot failed\n{error}"),
                ResourcePaneStatus::Ready => "Workspace snapshot unavailable".into(),
            };
        };
        let workspace = &snapshot.workspace;
        let memory = snapshot
            .memory
            .available_bytes
            .map(format_byte_count)
            .unwrap_or_else(|| "unavailable".into());
        let memory = match snapshot.memory.total_bytes.map(format_byte_count) {
            Some(total) => format!("{memory} / {total}"),
            None => memory,
        };
        let process = snapshot
            .process
            .resident_bytes
            .map(format_byte_count)
            .unwrap_or_else(|| "unavailable".into());
        let load = snapshot
            .cpu
            .load_one_minute
            .map(|value| format!("{value:.2}"))
            .unwrap_or_else(|| "unavailable".into());
        let mut lines = vec![
            format!("cpu {} cores  load {load}", snapshot.cpu.logical_cpus),
            format!("mem free {memory}"),
            format_gpu_signal(&snapshot.gpu),
            format_network_signal(&snapshot.network),
            format_process_summary(&snapshot.processes),
        ];
        for row in &snapshot.processes.rows {
            lines.push(format!(
                "  {:<10} {:>4}  {}",
                row.class.label(),
                row.count,
                format_byte_count(row.resident_bytes)
            ));
        }
        let context = self
            .transcript_ux
            .history_tokens
            .zip(self.transcript_ux.context_limit)
            .map(|(used, limit)| format!("context history {used}/{limit} tokens"))
            .unwrap_or_else(|| "context unavailable".into());
        lines.push(context);
        lines.push(format!(
            "lsp {} servers  mcp {} servers",
            snapshot
                .processes
                .count_for(crate::resources::ProcessClass::Lsp),
            snapshot
                .processes
                .count_for(crate::resources::ProcessClass::Mcp)
        ));
        lines.push("workspace".into());
        lines.push(format!(
            "files {}  dirs {}",
            workspace.files, workspace.directories
        ));
        lines.push(format!(
            "size {}  nodes {}",
            format_byte_count(workspace.bytes),
            workspace.nodes
        ));
        lines.push(format!("truncated {}", workspace.truncated));
        lines.push(format!("self {process}"));
        match &self.resource_status {
            ResourcePaneStatus::Collecting => lines.push("refreshing...".into()),
            ResourcePaneStatus::Failed(error) => lines.push(format!("refresh failed: {error}")),
            ResourcePaneStatus::Idle | ResourcePaneStatus::Ready => {}
        }
        lines.join("\n")
    }

    fn gantt_pane_content(&self) -> String {
        let Some(snapshot) = self.gantt_snapshot.as_ref() else {
            return match &self.gantt_status {
                GanttPaneStatus::Idle => "Waiting for Blueprint/Goal snapshot".into(),
                GanttPaneStatus::Refreshing => "Loading Blueprint/Goal snapshot...".into(),
                GanttPaneStatus::Failed(error) => format!("Gantt refresh failed\n{error}"),
                GanttPaneStatus::Ready => "Blueprint/Goal snapshot unavailable".into(),
            };
        };
        let content = match &self.gantt_status {
            GanttPaneStatus::Refreshing => format!("{}\nrefreshing...", snapshot.content),
            GanttPaneStatus::Failed(error) => {
                format!("{}\nrefresh failed: {error}", snapshot.content)
            }
            GanttPaneStatus::Idle | GanttPaneStatus::Ready => snapshot.content.clone(),
        };
        bound_gantt_pane_content(content)
    }

    /// Architecture projection: the active BentoBox tab and its pane/column
    /// structure, so the layout itself is inspectable without leaving the TUI.
    fn arch_pane_content(&self) -> String {
        let preset = self.workspace_layout.preset();
        let mut lines = vec![format!("tab {}", preset.tab)];
        for spec in &preset.panes {
            lines.push(format!(
                "  {:<22} {:?} row{} optional={}",
                spec.id.as_str(),
                spec.column,
                spec.row,
                PaneId::is_optional(spec.id)
            ));
        }
        bound_gantt_pane_content(lines.join("\n"))
    }

    /// Execution projection: the external owner plus the Blueprint/Goal
    /// snapshot the host already holds.
    fn execution_pane_content(&self) -> String {
        let mut lines = vec![
            "owner: external (b3ehive); zenpi persists intents only".to_owned(),
            self.gantt_pane_content(),
        ];
        bound_gantt_pane_content(lines.join("\n"))
    }

    /// The Execution pane is an embedded terminal: it mirrors the live local
    /// PTY snapshot so unix-fluent users can drive commands in place.
    fn execution_terminal_content(&self) -> String {
        if self.terminal_snapshot.trim().is_empty() {
            "embedded terminal (PTY): run a local `!command` to attach output here".to_owned()
        } else {
            self.terminal_snapshot.clone()
        }
    }
}

/// Colour the blueprint three-state marks with a soft, low-glare red/yellow/
/// green so status is readable at a glance without harsh terminal colours.
fn style_status_marks(content: &str) -> ratatui::text::Text<'static> {
    use ratatui::text::Span;
    const MARKS: [(&str, Color); 3] = [
        ("[ ]", Color::Rgb(178, 102, 102)),
        ("[_]", Color::Rgb(176, 148, 74)),
        ("[x]", Color::Rgb(96, 158, 110)),
    ];
    let mut lines = Vec::new();
    for raw in content.lines() {
        let mut spans: Vec<Span> = Vec::new();
        let mut rest = raw;
        loop {
            let mut best: Option<(usize, &str, Color)> = None;
            for (mark, color) in MARKS {
                if let Some(position) = rest.find(mark)
                    && best.is_none_or(|(current, _, _)| position < current)
                {
                    best = Some((position, mark, color));
                }
            }
            match best {
                Some((position, mark, color)) => {
                    if position > 0 {
                        spans.push(Span::raw(rest[..position].to_owned()));
                    }
                    spans.push(Span::styled(
                        mark.to_owned(),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ));
                    rest = &rest[position + mark.len()..];
                }
                None => {
                    if !rest.is_empty() {
                        spans.push(Span::raw(rest.to_owned()));
                    }
                    break;
                }
            }
        }
        lines.push(ratatui::text::Line::from(spans));
    }
    ratatui::text::Text::from(lines)
}

/// Colour the compact resource monitor so CPU, memory, GPU and network read
/// like a small `htop`/`nvidia-smi` board, while merged process classes keep a
/// stable per-class hue.
fn style_resource_pane(content: &str) -> ratatui::text::Text<'static> {
    use ratatui::text::{Line, Span};
    let mut lines = Vec::new();
    for raw in content.lines() {
        let (color, bold) = resource_line_style(raw.trim_start());
        let style = if bold {
            Style::default().fg(color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color)
        };
        lines.push(Line::from(Span::styled(raw.to_owned(), style)));
    }
    ratatui::text::Text::from(lines)
}

fn resource_line_style(trimmed: &str) -> (Color, bool) {
    if trimmed.starts_with("cpu") {
        return (Color::Cyan, true);
    }
    if trimmed.starts_with("mem") {
        return (Color::Green, true);
    }
    if trimmed.starts_with("gpu") {
        return (Color::Magenta, true);
    }
    if trimmed.starts_with("net") {
        return (Color::Blue, true);
    }
    if trimmed.starts_with("processes") {
        return (Color::Yellow, true);
    }
    if trimmed.starts_with("context") {
        return (Color::LightBlue, false);
    }
    match trimmed.split_whitespace().next().unwrap_or_default() {
        "zenpi" => (Color::Green, false),
        "opencode" => (Color::Cyan, false),
        "agent" => (Color::Blue, false),
        "lsp" => (Color::LightMagenta, false),
        "mcp" => (Color::LightCyan, false),
        "node" => (Color::Yellow, false),
        "rust" => (Color::LightRed, false),
        "shell" => (Color::LightGreen, false),
        "git" => (Color::LightBlue, false),
        "search" => (Color::Magenta, false),
        "refresh" | "Snapshot" => (Color::Red, true),
        _ => (Color::DarkGray, false),
    }
}

fn bound_gantt_pane_content(content: String) -> String {
    const MARKER: &str = "... Gantt pane content truncated";
    let too_many_rows = content.lines().count() > MAX_GANTT_PANE_ROWS;
    if !too_many_rows && content.len() <= MAX_GANTT_PANE_BYTES {
        return content;
    }

    let mut bounded = String::new();
    for line in content.lines().take(MAX_GANTT_PANE_ROWS.saturating_sub(1)) {
        let suffix_len = usize::from(!bounded.is_empty()) + 1 + MARKER.len();
        let available = MAX_GANTT_PANE_BYTES.saturating_sub(bounded.len() + suffix_len);
        if available == 0 {
            break;
        }
        if !bounded.is_empty() {
            bounded.push('\n');
        }
        bounded.push_str(truncate_bytes(line, available));
        if line.len() > available {
            break;
        }
    }
    if !bounded.is_empty() {
        bounded.push('\n');
    }
    bounded.push_str(MARKER);
    bounded
}

/// Ratatui-facing rectangle for one pane in a computed BentoBox snapshot.
/// The rectangle is clipped to the adapter area, so transient zero/one-cell
/// resize events can never make a widget draw outside the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneFrame {
    pub id: PaneId,
    pub rect: Rect,
    pub visibility: Visibility,
}

/// Reusable bridge from the terminal-independent [`LayoutModel`] to Ratatui.
/// Consumers can inspect the snapshot for diagnostics or use `pane`/
/// `visible_panes` to render their own widgets without duplicating breakpoint
/// and capability logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BentoBoxLayoutAdapter {
    area: Rect,
    snapshot: LayoutSnapshot,
    panes: Vec<PaneFrame>,
}

impl BentoBoxLayoutAdapter {
    pub fn new(model: &LayoutModel, area: Rect) -> Self {
        let snapshot = model.compute(area.width, area.height);
        let panes = snapshot
            .panes
            .iter()
            .map(|pane| PaneFrame {
                id: pane.id,
                rect: translate_pane_rect(area, pane.rect),
                visibility: pane.visibility,
            })
            .collect();
        Self {
            area,
            snapshot,
            panes,
        }
    }

    pub fn area(&self) -> Rect {
        self.area
    }

    pub fn tab(&self) -> TabId {
        self.snapshot.tab
    }

    pub fn breakpoint(&self) -> Breakpoint {
        self.snapshot.breakpoint
    }

    pub fn snapshot(&self) -> &LayoutSnapshot {
        &self.snapshot
    }

    pub fn pane(&self, id: PaneId) -> Option<PaneFrame> {
        self.panes.iter().find(|pane| pane.id == id).copied()
    }

    pub fn visible_panes(&self) -> impl Iterator<Item = PaneFrame> + '_ {
        self.panes
            .iter()
            .copied()
            .filter(|pane| pane.visibility == Visibility::Visible)
    }
}

fn translate_pane_rect(area: Rect, pane: PaneRect) -> Rect {
    let x_offset = pane.x.min(area.width);
    let y_offset = pane.y.min(area.height);
    let x = area.x.saturating_add(x_offset);
    let y = area.y.saturating_add(y_offset);
    let width = pane.width.min(area.width.saturating_sub(x_offset));
    let height = pane.height.min(area.height.saturating_sub(y_offset));
    Rect::new(x, y, width, height)
}

fn conversation_pane_for_tab(tab: TabId) -> PaneId {
    match tab {
        TabId::Project => PaneId::ProjectConversation,
        TabId::Goal => PaneId::GoalConversation,
        TabId::Learn => PaneId::LearnConversation,
        TabId::Review => PaneId::ReviewConversation,
        TabId::Session => PaneId::SessionConversation,
    }
}

fn pane_available(pane: PaneId, capabilities: crate::layout::PaneCapabilities) -> bool {
    match pane {
        PaneId::Browser => capabilities.browser,
        PaneId::Terminal => capabilities.terminal,
        _ => true,
    }
}

fn workspace_pane_title(id: PaneId) -> &'static str {
    match id {
        PaneId::ProjectConversation => "Conversation",
        PaneId::Resources => "Resources",
        PaneId::GoalConversation => "Goal",
        PaneId::Gantt => "Gantt",
        PaneId::Arch => "Arch",
        PaneId::Execution => "Execution",
        PaneId::Browser => "Browser",
        PaneId::Terminal => "Terminal",
        PaneId::LearnConversation => "Conversation",
        PaneId::LearnResources => "Learn resources",
        PaneId::LearnQueue => "Learn queue",
        PaneId::LearnMapping => "Learn mapping",
        PaneId::Evidence => "Evidence",
        PaneId::ReviewConversation => "Conversation",
        PaneId::Checks => "Checks",
        PaneId::ApprovalQueue => "Approval queue",
        PaneId::Diff => "Diff",
        PaneId::SessionList => "Sessions",
        PaneId::SessionConversation => "Conversation",
        PaneId::ReplayControls => "Replay",
        PaneId::EventTimeline => "Events",
    }
}

/// Coalesces dirty notifications and caps output at one frame per interval.
#[derive(Debug, Clone)]
pub struct RenderScheduler {
    interval: Duration,
    dirty: bool,
    last_render: Option<Instant>,
    frames: u64,
}

impl RenderScheduler {
    pub fn new(interval: Duration) -> Self {
        Self {
            // A zero interval would make the event loop spin when a caller
            // builds a config programmatically.  Keep the scheduler bounded
            // even when no CLI validation has run.
            interval: interval.max(MIN_LOOP_INTERVAL),
            dirty: true,
            last_render: None,
            frames: 0,
        }
    }

    pub fn request(&mut self) {
        self.dirty = true;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn due(&self, now: Instant) -> bool {
        self.dirty
            && self
                .last_render
                .is_none_or(|last| now.saturating_duration_since(last) >= self.interval)
    }

    pub fn rendered(&mut self, now: Instant) {
        self.dirty = false;
        self.last_render = Some(now);
        self.frames = self.frames.saturating_add(1);
    }

    pub fn frame_count(&self) -> u64 {
        self.frames
    }
}

#[derive(Debug, Clone)]
pub struct TuiConfig {
    pub title: String,
    pub max_messages: usize,
    pub poll_interval: Duration,
    pub frame_interval: Duration,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            title: "zenpi".into(),
            max_messages: DEFAULT_MAX_MESSAGES,
            poll_interval: DEFAULT_POLL_INTERVAL,
            frame_interval: DEFAULT_FRAME_INTERVAL,
        }
    }
}

#[derive(Debug, Error)]
pub enum TuiError {
    #[error("terminal I/O failed: {0}")]
    Io(#[from] io::Error),
}

/// The control result of a parsed slash command.
///
/// This is deliberately separate from [`TuiAction`].  Input editing remains
/// a pure view concern, while the host decides whether an admitted command
/// should interrupt a worker or leave the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashDispatchAction {
    Continue,
    Interrupt,
    Quit,
}

/// Interpret the existing `/goal <instruction>` payload for the TUI owner.
/// The slash grammar intentionally keeps Goal as a compact instruction so
/// natural-language submissions remain possible; these bounded forms mirror
/// the headless owner without adding a second parser variant:
/// `show|list [id]`, `status [id] [state]`, `run|resume|cancel <id>`, and
/// `transition <id> <state>`.
#[derive(Debug)]
enum TuiGoalOwnerAction {
    ShowAll,
    Show(String),
    Transition(String, GoalStatus),
    Create(String, String),
    Run(String),
    Resume(String),
}

fn parse_tui_goal_owner_action(
    instruction: &str,
) -> Result<Option<TuiGoalOwnerAction>, &'static str> {
    let tokens = instruction.split_whitespace().collect::<Vec<_>>();
    let Some(action) = tokens.first() else {
        return Ok(None);
    };
    let is_show = action.eq_ignore_ascii_case("show") || action.eq_ignore_ascii_case("list");
    let is_status = action.eq_ignore_ascii_case("status");
    let is_transition =
        action.eq_ignore_ascii_case("transition") || action.eq_ignore_ascii_case("set-status");
    let is_cancel = action.eq_ignore_ascii_case("cancel");
    let is_resume = action.eq_ignore_ascii_case("resume");
    let is_run = action.eq_ignore_ascii_case("run");
    let is_create = action.eq_ignore_ascii_case("create");
    if !is_show && !is_status && !is_transition && !is_cancel && !is_resume && !is_run && !is_create
    {
        return Ok(None);
    }
    let valid_id = |raw: &str| {
        if raw.is_empty()
            || raw.len() > crate::domains::MAX_ID_BYTES
            || raw.chars().any(char::is_control)
            || !raw
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._:/-".contains(character))
        {
            return Err("goal id is not a valid bounded identifier");
        }
        Ok(raw.to_owned())
    };
    let usage = || "use `/goal status [<goal-id>]`, `/goal run|resume|cancel <goal-id>`, or `/goal transition <goal-id> <state>`";

    if is_create {
        if tokens.len() != 3 {
            return Err("use `/goal create <goal-id> <blueprint-id[@version]>`");
        }
        let id = valid_id(tokens[1])?;
        let target = tokens[2];
        if target.is_empty()
            || target.len() > crate::domains::MAX_ID_BYTES + 1 + crate::domains::MAX_VERSION_BYTES
            || target.chars().any(char::is_control)
        {
            return Err("blueprint target is not bounded");
        }
        return Ok(Some(TuiGoalOwnerAction::Create(id, target.to_owned())));
    }

    if is_show {
        return match tokens.len() {
            1 => Ok(Some(TuiGoalOwnerAction::ShowAll)),
            2 => Ok(Some(TuiGoalOwnerAction::Show(valid_id(tokens[1])?))),
            _ => Err(usage()),
        };
    }
    if is_status {
        return match tokens.len() {
            1 => Ok(Some(TuiGoalOwnerAction::ShowAll)),
            2 => Ok(Some(TuiGoalOwnerAction::Show(valid_id(tokens[1])?))),
            3 => {
                let id = valid_id(tokens[1])?;
                let status = GoalStatus::parse_token(tokens[2]).ok_or(
                    "goal status must be queued, running, paused, blocked, cancelled, or done",
                )?;
                Ok(Some(TuiGoalOwnerAction::Transition(id, status)))
            }
            _ => Err(usage()),
        };
    }
    if is_cancel || is_resume || is_run {
        if tokens.len() != 2 {
            return Err(usage());
        }
        let id = valid_id(tokens[1])?;
        let status = if is_cancel {
            GoalStatus::Cancelled
        } else {
            GoalStatus::Running
        };
        return Ok(Some(if is_run {
            TuiGoalOwnerAction::Run(id)
        } else if is_resume {
            TuiGoalOwnerAction::Resume(id)
        } else {
            TuiGoalOwnerAction::Transition(id, status)
        }));
    }
    if tokens.len() != 3 {
        return Err(usage());
    }
    let id = valid_id(tokens[1])?;
    let status = GoalStatus::parse_token(tokens[2])
        .ok_or("goal status must be queued, running, paused, blocked, cancelled, or done")?;
    Ok(Some(TuiGoalOwnerAction::Transition(id, status)))
}

fn format_tree_projection(value: &serde_json::Value) -> String {
    let page = value.get("tree").unwrap_or(value);
    let mut lines = vec![format!(
        "Tree · {} entries",
        page["total"].as_u64().unwrap_or(0)
    )];
    if let Some(nodes) = page["nodes"].as_array() {
        for node in nodes {
            lines.push(format!(
                "{} · {}",
                node["entry"]["role"].as_str().unwrap_or("entry"),
                node["annotation"]["name"].as_str().unwrap_or("")
            ));
            lines.push(node["entry"]["id"].as_str().unwrap_or("").to_owned());
        }
    }
    if let Some(cursor) = page["next_cursor"].as_u64() {
        lines.push(format!("Next page: /tree list {cursor}"));
    }
    if let Some(path) = value["fork"]["path"].as_str() {
        lines.push(format!("Fork saved: {path}"));
    }
    lines.push("Active leaf:".into());
    lines.push(
        value
            .get("active_leaf")
            .unwrap_or(&page["active_leaf"])
            .as_str()
            .unwrap_or("root")
            .into(),
    );
    lines.join("\n")
}

fn format_external_evidence_projection(value: &serde_json::Value) -> String {
    let evidence = &value["evidence"];
    let status = evidence["status"].as_str().unwrap_or("candidate");
    let accepted = value["master_accepted"] == true;
    let mut text = format!(
        "External work: {status}\nMaster acceptance: {}",
        if accepted { "accepted" } else { "not accepted" }
    );
    if let Some(contract) = evidence.get("contract").filter(|v| v.is_object()) {
        text.push_str(&format!(
            "\nClaim: {}\nBaseline: {}",
            contract["claim_digest"].as_str().unwrap_or(""),
            contract["baseline"]["revision"].as_str().unwrap_or("")
        ));
        if status == "prepared" {
            text.push_str(&format!(
                "\nResult manifest: .zenpi-results/{}.json",
                contract["claim_digest"].as_str().unwrap_or("")
            ));
        }
    }
    if let Some(observations) = evidence["observations"].as_array() {
        for observation in observations {
            text.push_str(&format!(
                "\nValidator {}: exit {}, reaped {}",
                observation["validator_id"].as_str().unwrap_or(""),
                observation["exit_code"],
                observation["child_reaped"]
            ));
        }
    }
    if let Some(reason) = evidence["reason"].as_str() {
        text.push_str(&format!("\n{}", bounded_display(reason)));
    }
    text
}

fn format_output_projection(value: &serde_json::Value) -> String {
    if let Some(text) = value["text"].as_str() {
        // Preserve literal artifact text, including star-only redaction and
        // embedded Markdown fences, in the conversation renderer.
        let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
        let fence = "`".repeat((longest + 1).max(3));
        format!(
            "Output {} · {} bytes {}..{}{}\n\n{fence}text\n{}\n{fence}",
            value["artifact_id"].as_str().unwrap_or(""),
            value["stream"].as_str().unwrap_or(""),
            value["raw_start"],
            value["raw_end"],
            if value["complete"] == true {
                ""
            } else {
                " (incomplete)"
            },
            text
        )
    } else {
        format!("Output cleanup removed {} artifact(s)", value["removed"])
    }
}

/// Shared typed branch controls. Browse/select never dispatch a provider job.
pub fn dispatch_tree_input(
    state: &mut TuiState,
    agent: Option<&mut crate::core::Agent>,
    input: &str,
) -> bool {
    if crate::slash::tree_control(input).is_none() {
        return false;
    }
    let result = if state.busy {
        Err("tree control requires an idle project".into())
    } else if let Some(agent) = agent {
        crate::slash_actions::tree_control_request(
            agent.session().session_id(),
            "tui-tree-control",
            input,
        )
        .and_then(|request| {
            agent.tree_request(request.expect("recognized tree control"), &|| false)
        })
        .map_err(|e| e.to_string())
        .map(|value| {
            if matches!(
                crate::slash::tree_control(input),
                Some(Ok(crate::protocol::TreeAction::Select { .. }))
            ) {
                reload_state_from_agent(state, agent);
            }
            state.refresh_session_snapshot(agent.session());
            state.push_message(MessageRole::System, format_tree_projection(&value));
        })
    } else {
        Err("tree control requires an idle owner".into())
    };
    if let Err(error) = result {
        state.push_message(MessageRole::Error, error);
        state.set_rejected_input(input);
    }
    true
}

/// Handle the terminal reasoning picker through the actual Agent setter.
/// Returns false for other commands; failures preserve the submitted input.
pub fn dispatch_reasoning_input(
    state: &mut TuiState,
    agent: Option<&mut crate::core::Agent>,
    input: &str,
) -> bool {
    let mut args = input.split_whitespace();
    if args.next() != Some("/reasoning") {
        return false;
    }
    let effort = args.next();
    let result = if args.next().is_some() {
        Err("Usage: /reasoning [default|advertised level]".to_owned())
    } else if state.busy {
        Err("reasoning selection requires an idle project".to_owned())
    } else if let Some(agent) = agent {
        if let Some(effort) = effort {
            agent
                .set_reasoning_effort((effort != "default").then(|| effort.to_owned()))
                .map_err(|e| e.to_string())
                .map(|()| {
                    state.refresh_transcript_status(agent);
                    state.push_message(
                        MessageRole::System,
                        format!(
                            "reasoning selected: {}",
                            agent.reasoning_effort().unwrap_or("provider default")
                        ),
                    );
                })
        } else {
            state
                .update_model_catalog(agent)
                .map_err(|e| e.to_string())
                .and_then(|()| {
                    if state.reasoning_choices().is_empty() {
                        Err(
                            "Reasoning selection unavailable: this backend has no model registry"
                                .to_owned(),
                        )
                    } else {
                        state.set_input("/reasoning ");
                        Ok(())
                    }
                })
        }
    } else {
        Err("reasoning selection requires an idle project".to_owned())
    };
    if let Err(error) = result {
        state.push_message(MessageRole::Error, error);
        state.set_rejected_input(input);
    }
    true
}

/// Execute the local, transport-independent part of a slash command.
///
/// The function is intentionally side-effect-light: it updates the visible
/// transcript and, when an agent is supplied, reads or changes only local
/// agent state.  `/compete` and `/loop` are rendered as explicit pending
/// runtime-adapter errors, not success acknowledgements or model prompts.
/// `Cancel` returns [`SlashDispatchAction::Interrupt`] so the owning runtime
/// can issue its typed cancellation request.
pub fn dispatch_slash_command(
    command: SlashCommand,
    state: &mut TuiState,
    mut agent: Option<&mut crate::core::Agent>,
) -> SlashDispatchAction {
    match command {
        SlashCommand::Project { action } => {
            use crate::slash::ProjectAction;
            let (ok, message) = match action {
                ProjectAction::List => (
                    true,
                    format!(
                        "projects: {} (active: {})\n{}",
                        state.project_tabs().join(", "),
                        state.active_project(),
                        TuiState::project_navigation_model()
                    ),
                ),
                ProjectAction::Open { name } => {
                    // A remote folder source is read-only: probe it, record the
                    // source on the tab, and never fabricate a local session.
                    if let Ok(crate::folder_source::FolderSource::Remote { spec }) =
                        crate::folder_source::FolderSource::resolve(&name)
                    {
                        let ok = state.open_project_tab(name.clone());
                        if ok {
                            if let Some(index) = state.project_index(&name) {
                                let key = state.project_tabs[index].clone();
                                state
                                    .project_metadata
                                    .entry(key)
                                    .or_default()
                                    .source = Some(format!("ssh:{}", spec.display()));
                            }
                            match spec.probe(&spec.path) {
                                Ok(entries) => state.push_message(
                                    MessageRole::System,
                                    format!(
                                        "remote {} ({} entries): {}",
                                        spec.display(),
                                        entries.len(),
                                        entries.join("  ")
                                    ),
                                ),
                                Err(error) => state.push_message(
                                    MessageRole::Error,
                                    format!("remote probe failed: {error}"),
                                ),
                            }
                        }
                        state.push_message(
                            MessageRole::System,
                            if ok {
                                format!("remote project: {}", spec.display())
                            } else {
                                format!("project already exists: {name}")
                            },
                        );
                        return SlashDispatchAction::Continue;
                    }
                    let ok = state.open_project_tab(name.clone());
                    if ok && let Some(agent) = agent.as_deref_mut() {
                        let mut slug = name
                            .chars()
                            .map(|c| {
                                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                                    c
                                } else {
                                    '-'
                                }
                            })
                            .collect::<String>();
                        if slug.is_empty() {
                            slug = "project".into();
                        }
                        let path = agent
                            .session()
                            .path()
                            .parent()
                            .unwrap_or_else(|| std::path::Path::new("."))
                            .join(format!("{slug}.jsonl"));
                        match crate::session::SessionStore::open(&path).and_then(|_| {
                            agent.resume_session(path.clone()).map_err(|e| {
                                crate::session::SessionError::InvalidRecord(e.to_string())
                            })
                        }) {
                            Ok(()) => state.set_active_project_metadata(
                                ProjectTabMetadata::from_session(agent.session()),
                            ),
                            Err(error) => state.push_message(
                                MessageRole::Error,
                                format!("project session create failed: {error}"),
                            ),
                        }
                        bind_agent_to_active_project(state, agent);
                    }
                    (
                        ok,
                        if ok {
                            format!("project opened: {name}")
                        } else {
                            format!("project already exists or has an invalid name: {name}")
                        },
                    )
                }
                ProjectAction::Select { name } => {
                    let index = state.project_tabs().iter().position(|item| item == &name);
                    let ok = index.is_some_and(|i| state.select_project_tab(i));
                    if ok && let Some(agent) = agent.as_deref_mut() {
                        bind_agent_to_active_project(state, agent);
                    }
                    (
                        ok,
                        if ok {
                            format!("project selected: {name}")
                        } else {
                            format!("project not found: {name}")
                        },
                    )
                }
                ProjectAction::Close { name } => {
                    if name == state.active_project()
                        && agent
                            .as_ref()
                            .is_some_and(|owner| owner.phase() == crate::core::AgentPhase::Running)
                    {
                        (
                            false,
                            "Project has running work; wait or cancel it before closing."
                                .to_owned(),
                        )
                    } else {
                        let was_active = state.active_project() == name;
                        let ok = state.close_project_tab(&name);
                        if ok
                            && was_active
                            && let Some(agent) = agent.as_deref_mut()
                        {
                            bind_agent_to_active_project(state, agent);
                        }
                        (
                            ok,
                            if ok {
                                format!("project closed: {name}")
                            } else {
                                format!("cannot close project: {name}")
                            },
                        )
                    }
                }
                ProjectAction::Move { name, index } => {
                    let ok = state.move_project_tab(&name, index);
                    (
                        ok,
                        if ok {
                            format!("project moved: {name} -> {index}")
                        } else {
                            format!("cannot move project: {name}")
                        },
                    )
                }
                ProjectAction::Rename { old, new } => {
                    let ok = !new.trim().is_empty()
                        && !state.project_tabs().contains(&new)
                        && state.rename_project_tab(&old, new.clone());
                    (
                        ok,
                        if ok {
                            format!("project renamed: {old} -> {new}")
                        } else {
                            format!("cannot rename project: {old}")
                        },
                    )
                }
                ProjectAction::Style { name, style } => {
                    let ok = state.style_project_tab(&name, &style);
                    (
                        ok,
                        if ok {
                            format!("project style: {name} = {style}")
                        } else {
                            format!("unknown project or style: {name}/{style}")
                        },
                    )
                }
            };
            state.push_message(
                if ok {
                    MessageRole::System
                } else {
                    MessageRole::Error
                },
                message,
            );
        }
        SlashCommand::Yolo { enabled } => {
            if let Some(agent) = agent.as_deref_mut() {
                let mut policy = agent.configured_approval_policy().unwrap_or_default();
                policy.mode = if enabled {
                    crate::approval::ApprovalMode::Never
                } else {
                    crate::approval::ApprovalMode::ReadOnly
                };
                agent.set_approval_policy(policy);
                state.push_message(
                    MessageRole::System,
                    format!("yolo {}", if enabled { "on" } else { "off" }),
                );
            } else {
                state.push_message(MessageRole::Error, "yolo requires an idle owner");
            }
        }
        SlashCommand::Approval { mode } => {
            let mode = mode.to_ascii_lowercase();
            if !matches!(mode.as_str(), "ask" | "always" | "never") {
                state.push_message(
                    MessageRole::Error,
                    "approval mode must be ask, always, or never",
                );
            } else {
                if let Some(agent) = agent.as_deref_mut() {
                    let mut policy = agent.configured_approval_policy().unwrap_or_default();
                    policy.mode = match mode.as_str() {
                        "ask" => crate::approval::ApprovalMode::ReadOnly,
                        "always" => crate::approval::ApprovalMode::Always,
                        _ => crate::approval::ApprovalMode::Never,
                    };
                    agent.set_approval_policy(policy);
                    state.push_message(MessageRole::System, format!("approval mode: {mode}"));
                } else {
                    state.push_message(MessageRole::Error, "approval requires an idle owner");
                }
            }
        }
        SlashCommand::Help { topic } => match if topic.as_deref() == Some("input") {
            Some("Input: Ctrl-G External editor · save and close to return. Enter sends; Ctrl-J/Shift-Enter inserts newline. Left/Right move; Ctrl/Alt-Left/Right move words; Ctrl-W/Ctrl-Backspace deletes previous word; Ctrl-Delete deletes next word; Ctrl-A/E line start/end; Ctrl-K deletes to line end; Ctrl-U deletes to logical line start (at line start, deletes previous newline). Ctrl-Y restores the last nonempty Ctrl-U/K/W or word deletion as plain text; one buffer per project, process-only, at most 256 KiB, never the system clipboard. Repeated yank preserves the buffer; input/checkpoint limits reject atomically. Ctrl-P/N or Alt-Up/Down browse history and return to the saved draft; Ctrl-R searches, Ctrl-R again selects older matches, Enter accepts without sending, Esc restores. Paste inserts literal text, never executes keys; limits reject without replacing the draft. Esc keeps drafted input. /input list views runtime inputs; /input edit ID REVISION TEXT changes an accepted queued input; /input cancel ID withdraws it. Future jobs: /scheduled list, /scheduled edit ID REVISION TEXT, /scheduled cancel ID. Restarted scheduled jobs are interrupted/unconfirmed; inspect the session before /scheduled retry ID.".into())
        } else if topic.as_deref() == Some("reasoning") {
            Some("/reasoning [default|advertised level] - select current model reasoning effort; default omits the field. Idle project required.".into())
        } else {
            slash::help(topic.as_deref()).map(|text| if topic.is_none() { format!("{text}\n/reasoning [default|advertised level]         Select actual model reasoning effort") } else { text })
        } {
            Some(help) => state.push_message(MessageRole::System, help),
            None => state.push_message(
                MessageRole::Error,
                "unknown help topic; type /help for available commands",
            ),
        },
        SlashCommand::Persona { name } => {
            if let Some(agent) = agent.as_deref_mut() {
                if let Some(name) = name
                    && let Err(error) = agent.set_persona(&name)
                {
                    state.push_message(MessageRole::Error, error.to_string());
                    return SlashDispatchAction::Continue;
                }
                state.persona = agent.persona().into();
                state.cached_transcript = None;
                state.set_status(format!("Persona {}", state.persona));
            } else {
                state.push_message(MessageRole::Error, "persona requires an idle owner");
            }
        }
        SlashCommand::Model { name } => {
            if let Some(name) = name {
                // `/model <name>` targets the focused left zone (ZS1-152). The
                // discussion zone is the main conversation and therefore also
                // updates the global model; the arch zone is a separate
                // master-session region and must not disturb the discussion
                // agent's active model.
                let zone = state.focused_zone();
                if zone == crate::view_model::Zone::Arch {
                    match state.set_zone_model(zone, Some(name.clone())) {
                        Ok(()) => state.push_message(
                            MessageRole::System,
                            format!("arch model selected: {}", inline_token(&name, 160)),
                        ),
                        Err(error) => state.push_message(
                            MessageRole::Error,
                            format!("arch model change failed: {error}"),
                        ),
                    }
                    if let Some(agent) = agent.as_deref_mut()
                        && let Err(error) = agent.set_zone_model(zone, Some(name))
                    {
                        state.push_message(
                            MessageRole::Error,
                            format!("arch model change failed: {error}"),
                        );
                    }
                } else if let Some(agent) = agent.as_deref_mut() {
                    match agent.set_model(Some(name.clone())) {
                        Ok(()) => {
                            let _ = state.set_zone_model(zone, Some(name.clone()));
                            state.refresh_transcript_status(agent);
                            // Keep the command palette's model/reasoning catalog
                            // bound to the owner we just changed.  Model
                            // selection is an idle-owner mutation; leaving the
                            // previous catalog active makes `/reasoning ` show
                            // no choices (or choices for the old model) until
                            // the next explicit `/model` refresh.
                            if let Err(error) = state.update_model_catalog(agent) {
                                state.push_message(
                                    MessageRole::Error,
                                    format!("model catalog refresh failed: {error}"),
                                );
                            }
                            state.push_message(
                                MessageRole::System,
                                format!("model selected: {}", inline_token(&name, 160)),
                            );
                        }
                        Err(error) => state.push_message(
                            MessageRole::Error,
                            format!("model change failed: {error}"),
                        ),
                    }
                } else {
                    // No owner is bound yet: still record the discussion zone
                    // preference so a later host can adopt it.
                    match state.set_zone_model(zone, Some(name.clone())) {
                        Ok(()) => state.push_message(
                            MessageRole::System,
                            format!("model selected: {}", inline_token(&name, 160)),
                        ),
                        Err(error) => state.push_message(
                            MessageRole::Error,
                            format!("model change failed: {error}"),
                        ),
                    }
                }
            } else if let Some(agent) = agent.as_deref_mut() {
                match state.update_model_catalog(agent) {
                    Ok(()) if !state.model_menu.entries.is_empty() => state.set_input("/model "),
                    Ok(()) => state.push_message(
                        MessageRole::Error,
                        "Model selection unavailable: this backend has no model registry",
                    ),
                    Err(error) => state.push_message(MessageRole::Error, error.to_string()),
                }
            } else {
                state.push_message(
                    MessageRole::Error,
                    "model selection requires an idle project",
                );
            }
        }
        SlashCommand::Models => match agent.as_deref().map(crate::slash_actions::models_value) {
            Some(Ok(value)) => state.push_message(
                MessageRole::System,
                format!(
                    "models:\n{}",
                    bounded_display(&serde_json::to_string_pretty(&value).unwrap_or_default())
                ),
            ),
            Some(Err(error)) => state.push_message(MessageRole::Error, error.to_string()),
            None => state.push_message(
                MessageRole::Error,
                "model catalogue requires an idle project",
            ),
        },
        SlashCommand::Doctor => match crate::config::doctor_value(None) {
            Ok(value) => state.push_message(
                MessageRole::System,
                format!(
                    "doctor:\n{}",
                    bounded_display(&serde_json::to_string(&value).unwrap_or_else(|_| "{}".into()))
                ),
            ),
            Err(error) => state.push_message(MessageRole::Error, format!("doctor failed: {error}")),
        },
        SlashCommand::Status => {
            let message = if let Some(agent) = agent.as_deref_mut() {
                format_agent_status(agent)
            } else {
                "status: agent host is busy; no model request was submitted".into()
            };
            state.push_message(MessageRole::System, message);
        }
        SlashCommand::History { limit } => {
            let message = format_history(agent.as_deref(), state, limit);
            state.push_message(MessageRole::System, message);
        }
        SlashCommand::Resources { path } => {
            match crate::headless::collect_resource_snapshot_at(
                path.as_deref(),
                &state.project_cwd(),
            ) {
                Ok(snapshot) => {
                    let summary = format_resource_summary(&snapshot);
                    state.set_resource_snapshot(snapshot);
                    state.push_message(MessageRole::System, summary);
                }
                Err(error) => {
                    state.resource_refresh_failed(error.to_string());
                    state.push_message(
                        MessageRole::Error,
                        format!("resource collection failed: {error}"),
                    );
                }
            }
        }
        SlashCommand::Layout { action } => {
            dispatch_layout_command(state, action);
        }
        SlashCommand::Pane { action } => {
            dispatch_pane_command(state, action);
        }
        SlashCommand::Clear => {
            state.clear_messages();
            state.set_status("Ready");
        }
        SlashCommand::Cancel => {
            state.set_status("Interrupt requested");
            return SlashDispatchAction::Interrupt;
        }
        SlashCommand::Exit => return SlashDispatchAction::Quit,
        SlashCommand::Goal { instruction } => match parse_tui_goal_owner_action(&instruction) {
            Ok(Some(TuiGoalOwnerAction::ShowAll)) => {
                state.push_goal_message(MessageRole::System, "Goal store refreshed");
                match crate::headless::domain_store_view(agent.as_deref()) {
                    Ok(data) => state.push_message(
                        MessageRole::System,
                        format!(
                            "goal show:\n{}",
                            bounded_display(
                                &serde_json::to_string(&data)
                                    .unwrap_or_else(|_| "{\"goals\":[]}".into())
                            )
                        ),
                    ),
                    Err(error) => state.push_message(
                        MessageRole::Error,
                        format!("goal store unavailable: {error}"),
                    ),
                }
            }
            Ok(Some(TuiGoalOwnerAction::Show(id))) => {
                state.push_goal_message(MessageRole::System, format!("Goal selected: {id}"));
                match crate::headless::domain_store_view(agent.as_deref()) {
                    Ok(data) => {
                        let goal = data
                            .get("goals")
                            .and_then(serde_json::Value::as_array)
                            .and_then(|goals| {
                                goals.iter().find(|goal| {
                                    goal.get("id").and_then(serde_json::Value::as_str)
                                        == Some(id.as_str())
                                })
                            });
                        match goal {
                            Some(goal) => state.push_message(
                                MessageRole::System,
                                format!(
                                    "goal show:\n{}",
                                    bounded_display(
                                        &serde_json::to_string(goal)
                                            .unwrap_or_else(|_| "{}".into())
                                    )
                                ),
                            ),
                            None => state.push_message(
                                MessageRole::Error,
                                format!("goal `{id}` is not found"),
                            ),
                        }
                    }
                    Err(error) => state.push_message(
                        MessageRole::Error,
                        format!("goal store unavailable: {error}"),
                    ),
                }
            }
            Ok(Some(TuiGoalOwnerAction::Transition(id, status))) => match agent.as_deref_mut() {
                Some(agent) => match crate::headless::transition_goal_status(agent, &id, status) {
                    Ok(data) => state.push_message(
                        MessageRole::System,
                        format!(
                            "goal status:\n{}",
                            bounded_display(
                                &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                            )
                        ),
                    ),
                    Err(error) => state
                        .push_message(MessageRole::Error, format!("goal status failed: {error}")),
                },
                None => state.push_message(
                    MessageRole::Error,
                    "goal status is unavailable while the agent is busy",
                ),
            },
            Ok(Some(TuiGoalOwnerAction::Create(id, target))) => match agent.as_deref_mut() {
                Some(agent) => {
                    match crate::headless::create_goal_from_blueprint_for_tui(agent, &id, &target) {
                        Ok(data) => state.push_message(
                            MessageRole::System,
                            format!(
                                "goal create:\n{}",
                                bounded_display(
                                    &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                                )
                            ),
                        ),
                        Err(error) => state.push_message(
                            MessageRole::Error,
                            format!("goal create failed: {error}"),
                        ),
                    }
                }
                None => state.push_message(
                    MessageRole::Error,
                    "goal creation is unavailable while the agent is busy",
                ),
            },
            Ok(Some(TuiGoalOwnerAction::Run(id))) => match agent.as_deref_mut() {
                Some(agent) => {
                    let target = crate::headless::domain_store_view(Some(agent))
                        .ok()
                        .and_then(|data| {
                            data.get("goals")
                                .and_then(serde_json::Value::as_array)
                                .and_then(|goals| {
                                    goals.iter().find(|goal| {
                                        goal.get("id").and_then(serde_json::Value::as_str)
                                            == Some(id.as_str())
                                    })
                                })
                                .and_then(|goal| {
                                    Some(format!(
                                        "{}@{}",
                                        goal.get("blueprint_id")?.as_str()?,
                                        goal.get("blueprint_version")?.as_str()?
                                    ))
                                })
                        });
                    match target {
                        Some(target) => {
                            match crate::headless::run_goal_steps_for_tui(agent, &target, 1) {
                                Ok(data) => state.push_message(
                                    MessageRole::System,
                                    format!(
                                        "goal execution:\n{}",
                                        bounded_display(
                                            &serde_json::to_string(&data)
                                                .unwrap_or_else(|_| "{}".into())
                                        )
                                    ),
                                ),
                                Err(error) => state.push_message(
                                    MessageRole::Error,
                                    format!("goal execution failed: {error}"),
                                ),
                            }
                        }
                        None => state
                            .push_message(MessageRole::Error, format!("goal `{id}` is not found")),
                    }
                }
                None => state.push_message(
                    MessageRole::Error,
                    "goal execution is unavailable while the agent is busy",
                ),
            },
            Ok(Some(TuiGoalOwnerAction::Resume(id))) => match agent.as_deref_mut() {
                Some(agent) => {
                    match crate::headless::transition_goal_status(agent, &id, GoalStatus::Running) {
                        Ok(data) => state.push_message(
                            MessageRole::System,
                            format!(
                                "goal status:\n{}",
                                bounded_display(
                                    &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                                )
                            ),
                        ),
                        Err(error) => state.push_message(
                            MessageRole::Error,
                            format!("goal status failed: {error}"),
                        ),
                    }
                }
                None => state.push_message(
                    MessageRole::Error,
                    "goal status is unavailable while the agent is busy",
                ),
            },
            Ok(None) => state.push_message(
                MessageRole::Error,
                format!(
                    "goal creation/execution requires an external b3ehive owner: {}",
                    bounded_display(&instruction)
                ),
            ),
            Err(error) => state.push_message(MessageRole::Error, error),
        },
        SlashCommand::GoalPut { path } => {
            let Some(agent) = agent.as_deref_mut() else {
                state.push_message(
                    MessageRole::Error,
                    "goal persistence is unavailable while the agent is busy",
                );
                return SlashDispatchAction::Continue;
            };
            match crate::headless::persist_goal_path(agent, &path) {
                Ok(data) => state.push_message(
                    MessageRole::System,
                    format!(
                        "goal persisted:\n{}",
                        bounded_display(
                            &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                        )
                    ),
                ),
                Err(error) => state.push_message(
                    MessageRole::Error,
                    format!("goal persistence failed: {error}"),
                ),
            }
        }
        SlashCommand::Plan { instruction } => {
            let Some(agent) = agent.as_deref_mut() else {
                state.push_message(
                    MessageRole::Error,
                    "plan creation is unavailable while the agent is busy",
                );
                return SlashDispatchAction::Continue;
            };
            let Some(instruction) = instruction else {
                state.push_message(
                    MessageRole::Error,
                    "use `/plan <blueprint-id> :: <step 1>; <step 2>`",
                );
                return SlashDispatchAction::Continue;
            };
            match crate::headless::create_blueprint_from_plan_for_tui(agent, &instruction) {
                Ok(data) => state.push_message(
                    MessageRole::System,
                    format!(
                        "plan created:\n{}",
                        bounded_display(
                            &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                        )
                    ),
                ),
                Err(error) => {
                    state.push_message(MessageRole::Error, format!("plan creation failed: {error}"))
                }
            }
        }
        SlashCommand::Session { action } => match action {
            crate::slash::SessionAction::New => state.push_message(
                MessageRole::Error,
                "new session requires the asynchronous durable project owner",
            ),
            crate::slash::SessionAction::List => {
                if let Some(owner) = agent.as_deref() {
                    state.session_browser_refresh.query = None;
                    state.session_browser_refresh.last_checked = None;
                    state.refresh_session_browser(owner.session());
                }
                match crate::headless::session_lifecycle_view(
                    &crate::slash::SessionAction::List,
                    agent.as_deref().map(|value| value.session().path()),
                ) {
                    Ok(data) => state.push_message(
                        MessageRole::System,
                        format!(
                            "sessions:\n{}",
                            bounded_display(
                                &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                            )
                        ),
                    ),
                    Err(error) => state.push_message(
                        MessageRole::Error,
                        format!("session listing failed: {error}"),
                    ),
                }
            }
            crate::slash::SessionAction::Search { query } => {
                if let Some(owner) = agent.as_deref()
                    && state
                        .search_session_browser(owner.session(), &query)
                        .is_ok()
                {
                    state.set_workspace_tab(TabId::Session);
                    state.focus_workspace_pane(PaneId::SessionList);
                }
                let result = agent
                    .as_deref()
                    .and_then(|value| value.session().path().parent())
                    .ok_or_else(|| "session search requires an active session owner".to_owned())
                    .and_then(|directory| {
                        crate::headless::session_search_view_in(directory, &query)
                    });
                match result {
                    Ok(data) => state.push_message(
                        MessageRole::System,
                        format!(
                            "session search:\n{}",
                            bounded_display(
                                &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                            )
                        ),
                    ),
                    Err(error) => state.push_message(
                        MessageRole::Error,
                        format!("session search failed: {error}"),
                    ),
                }
            }
            crate::slash::SessionAction::Open { path } => match agent.as_deref_mut() {
                Some(agent) => match crate::headless::open_session_path(agent, &path) {
                    Ok(data) => {
                        // Opening a session changes the conversation owner;
                        // discard the old visible transcript and render the
                        // recovered turns from the replacement journal.
                        reload_state_from_agent(state, agent);
                        state.push_message(
                            MessageRole::System,
                            format!(
                                "session opened:\n{}",
                                bounded_display(
                                    &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                                )
                            ),
                        );
                    }
                    Err(error) => state
                        .push_message(MessageRole::Error, format!("session open failed: {error}")),
                },
                None => state.push_message(
                    MessageRole::Error,
                    "session cannot be opened while the agent is busy",
                ),
            },
            crate::slash::SessionAction::ResumeLast => match agent.as_deref_mut() {
                Some(agent) => match crate::headless::resume_last_session_view(agent) {
                    Ok(data) => {
                        reload_state_from_agent(state, agent);
                        state.push_message(
                            MessageRole::System,
                            format!(
                                "session resumed:\n{}",
                                bounded_display(
                                    &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                                )
                            ),
                        );
                    }
                    Err(error) => state.push_message(
                        MessageRole::Error,
                        format!("session resume-last failed: {error}"),
                    ),
                },
                None => state.push_message(
                    MessageRole::Error,
                    "session resume-last requires the session owner",
                ),
            },
            action @ (crate::slash::SessionAction::Agents
            | crate::slash::SessionAction::Inspect { .. }
            | crate::slash::SessionAction::Fork { .. }
            | crate::slash::SessionAction::Export { .. }
            | crate::slash::SessionAction::Import { .. }
            | crate::slash::SessionAction::Migrate { .. }
            | crate::slash::SessionAction::Archive { .. }
            | crate::slash::SessionAction::Unarchive { .. }
            | crate::slash::SessionAction::Delete { .. }
            | crate::slash::SessionAction::RetireMailbox { .. }
            | crate::slash::SessionAction::Queue { .. }) => {
                let active_path = agent.as_deref().map(|value| value.session().path());
                match crate::headless::session_lifecycle_view(&action, active_path) {
                    Ok(data) => state.push_message(
                        MessageRole::System,
                        format!(
                            "session {}:\n{}",
                            session_action_label(&action),
                            bounded_display(
                                &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                            )
                        ),
                    ),
                    Err(error) => state.push_message(
                        MessageRole::Error,
                        format!("session {} failed: {error}", session_action_label(&action)),
                    ),
                }
            }
            crate::slash::SessionAction::Gc { policy } => match agent.as_deref() {
                Some(agent) => match crate::headless::session_gc_view(agent, &policy) {
                    Ok(data) => state.push_message(
                        MessageRole::System,
                        format!(
                            "session gc:\n{}",
                            bounded_display(
                                &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                            )
                        ),
                    ),
                    Err(error) => state
                        .push_message(MessageRole::Error, format!("session gc failed: {error}")),
                },
                None => state.push_message(
                    MessageRole::Error,
                    "session gc is unavailable while the agent is busy",
                ),
            },
        },
        SlashCommand::Recovery { action } => match agent.as_deref_mut() {
            Some(agent) => match crate::headless::recovery_view(agent, &action) {
                Ok(data) => {
                    state.refresh_session_snapshot(agent.session());
                    state.push_message(
                        MessageRole::System,
                        format!("recovery:\n{}", bounded_display(&data.to_string()),),
                    );
                }
                Err(error) => state.push_message(MessageRole::Error, error),
            },
            None => state.push_message(
                MessageRole::Error,
                "recovery requires the idle session owner",
            ),
        },
        SlashCommand::Mailbox { action } => match agent.as_deref_mut() {
            Some(agent) => match crate::headless::mailbox_slash_view(agent, &action) {
                Ok(data) => state.push_message(
                    MessageRole::System,
                    format!(
                        "{}\n{}",
                        mailbox_result_heading(&data),
                        bounded_display(
                            &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                        )
                    ),
                ),
                Err(error) => {
                    state.push_message(MessageRole::Error, format!("mailbox failed: {error}"))
                }
            },
            None => state.push_message(
                MessageRole::Error,
                "mailbox command requires the session owner",
            ),
        },
        SlashCommand::Resume { sequence } => {
            let Some(agent) = agent.as_deref_mut() else {
                state.push_message(
                    MessageRole::Error,
                    "session replay is unavailable while the agent is busy",
                );
                return SlashDispatchAction::Continue;
            };
            match crate::headless::resume_session_view(agent, sequence) {
                Ok(data) => {
                    // A no-argument resume is the recovery action users reach
                    // for after `/clear`; restore the bounded durable turns
                    // before showing the replay cursor/marker details.
                    if sequence.is_none() {
                        reload_state_from_agent(state, agent);
                    }
                    state.refresh_session_snapshot(agent.session());
                    state.push_message(
                        MessageRole::System,
                        format!(
                            "resume:\n{}",
                            bounded_display(
                                &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                            )
                        ),
                    );
                }
                Err(error) => {
                    state.push_message(MessageRole::Error, format!("resume failed: {error}"))
                }
            }
        }
        SlashCommand::Compact => {
            let Some(agent) = agent.as_deref_mut() else {
                state.push_message(
                    MessageRole::Error,
                    "context compaction is unavailable while the agent is busy",
                );
                return SlashDispatchAction::Continue;
            };
            match crate::headless::compact_context_view(agent) {
                Ok(data) => state.push_message(
                    MessageRole::System,
                    format!(
                        "compact:\n{}",
                        bounded_display(
                            &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                        )
                    ),
                ),
                Err(error) => {
                    state.push_message(MessageRole::Error, format!("compact failed: {error}"))
                }
            }
        }
        SlashCommand::Diff { path } => {
            // Synchronous embedders may supply the workspace owner before
            // creating tab metadata. A selected tab always takes precedence;
            // an absent owner never means the process working directory.
            let workspace = if state.project_metadata(state.active_project()).is_none() {
                agent
                    .as_deref()
                    .and_then(|owner| owner.attachment_workspace_root())
                    .map(std::path::Path::to_path_buf)
                    .ok_or_else(|| "diff requires a selected project workspace".to_owned())
            } else {
                LocalDiffScope::capture(state, state.active_project()).map(|scope| scope.cwd)
            };
            match workspace.and_then(|root| {
                crate::slash_actions::diff_value_at(&root, path.as_deref())
                    .map_err(|e| e.to_string())
            }) {
                Ok(data) => state.push_message(MessageRole::System, format_diff_view(&data)),
                Err(error) => {
                    state.push_message(MessageRole::Error, format!("diff failed: {error}"))
                }
            }
        }
        SlashCommand::Attach { path } => match agent.as_deref_mut() {
            Some(agent) => match crate::slash_actions::attach_value(agent, &path) {
                Ok(data) => state.push_message(
                    MessageRole::System,
                    format!(
                        "attachment staged:\n{}",
                        bounded_display(
                            &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                        )
                    ),
                ),
                Err(error) => {
                    state.push_message(MessageRole::Error, format!("attachment failed: {error}"))
                }
            },
            None => state.push_message(
                MessageRole::Error,
                "attachment staging is unavailable while the agent is busy",
            ),
        },
        SlashCommand::Approve { id, decision } => match agent.as_deref_mut() {
            Some(agent) => match crate::headless::respond_to_slash_approval(agent, &id, decision) {
                Ok(data) => state.push_message(
                    MessageRole::System,
                    format!(
                        "approval resolved:\n{}",
                        bounded_display(
                            &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                        )
                    ),
                ),
                Err(error) => {
                    state.push_message(MessageRole::Error, format!("approval failed: {error}"))
                }
            },
            None => state.push_message(
                MessageRole::Error,
                "approval response is unavailable while the agent is busy",
            ),
        },
        SlashCommand::Blueprint { action } => {
            use crate::slash::BlueprintAction;
            let action_display = blueprint_action_display(&action);
            match action {
                BlueprintAction::Show | BlueprintAction::Status => {
                    match crate::headless::domain_store_view(agent.as_deref()) {
                        Ok(data) => {
                            state.push_goal_message(MessageRole::System, "Blueprint status loaded");
                            state.push_message(
                                MessageRole::System,
                                format!(
                                    "blueprint {}:\n{}",
                                    if matches!(action, BlueprintAction::Show) {
                                        "show"
                                    } else {
                                        "status"
                                    },
                                    bounded_display(
                                        &serde_json::to_string(&data)
                                            .unwrap_or_else(|_| "{}".into())
                                    )
                                ),
                            )
                        }
                        Err(error) => {
                            state.push_goal_message(
                                MessageRole::Error,
                                format!("Blueprint status failed: {error}"),
                            );
                            state.push_message(
                                MessageRole::Error,
                                format!("blueprint store unavailable: {error}"),
                            )
                        }
                    }
                }
                BlueprintAction::Validate { path: Some(path) } => {
                    match crate::headless::blueprint_validation_view_at(&path, &state.project_cwd())
                    {
                        Ok(data) => {
                            state.push_goal_message(
                                MessageRole::System,
                                "Blueprint validation completed",
                            );
                            state.push_message(
                                MessageRole::System,
                                format!(
                                    "blueprint validate:\n{}",
                                    bounded_display(
                                        &serde_json::to_string(&data)
                                            .unwrap_or_else(|_| "{}".into())
                                    )
                                ),
                            )
                        }
                        Err(error) => {
                            state.push_goal_message(
                                MessageRole::Error,
                                format!("Blueprint validation failed: {error}"),
                            );
                            state.push_message(
                                MessageRole::Error,
                                format!("blueprint validation failed: {error}"),
                            )
                        }
                    }
                }
                BlueprintAction::Validate { path: None } => {
                    match crate::headless::domain_store_view(agent.as_deref()) {
                        Ok(data) => {
                            state.push_goal_message(
                                MessageRole::System,
                                "Blueprint validation completed",
                            );
                            state.push_message(
                                MessageRole::System,
                                format!(
                                    "blueprint validate:\n{}",
                                    bounded_display(
                                        &serde_json::to_string(&data)
                                            .unwrap_or_else(|_| "{}".into())
                                    )
                                ),
                            )
                        }
                        Err(error) => {
                            state.push_goal_message(
                                MessageRole::Error,
                                format!("Blueprint validation failed: {error}"),
                            );
                            state.push_message(
                                MessageRole::Error,
                                format!("blueprint validation failed: {error}"),
                            )
                        }
                    }
                }
                BlueprintAction::Put { path } => match agent.as_deref_mut() {
                    Some(agent) => match crate::headless::persist_blueprint_path(agent, &path) {
                        Ok(data) => {
                            state.push_goal_message(MessageRole::System, "Blueprint persisted");
                            state.push_message(
                                MessageRole::System,
                                format!(
                                    "blueprint persisted:\n{}",
                                    bounded_display(
                                        &serde_json::to_string(&data)
                                            .unwrap_or_else(|_| "{}".into())
                                    )
                                ),
                            )
                        }
                        Err(error) => {
                            state.push_goal_message(
                                MessageRole::Error,
                                format!("Blueprint persistence failed: {error}"),
                            );
                            state.push_message(
                                MessageRole::Error,
                                format!("blueprint persistence failed: {error}"),
                            )
                        }
                    },
                    None => state.push_message(
                        MessageRole::Error,
                        "blueprint persistence is unavailable while the agent is busy",
                    ),
                },
                BlueprintAction::Run { target } => match agent.as_deref_mut() {
                    Some(agent) => {
                        state.push_goal_message(
                            MessageRole::System,
                            "Blueprint item execution started",
                        );
                        match crate::headless::run_blueprint_next(agent, &target) {
                            Ok(data) => {
                                state.push_goal_message(
                                    MessageRole::System,
                                    "Blueprint item execution receipt recorded",
                                );
                                state.push_message(
                                    MessageRole::System,
                                    format!(
                                        "blueprint run:\n{}",
                                        bounded_display(
                                            &serde_json::to_string(&data)
                                                .unwrap_or_else(|_| "{}".into())
                                        )
                                    ),
                                )
                            }
                            Err(error) => {
                                state.push_goal_message(
                                    MessageRole::Error,
                                    format!("Blueprint execution failed: {error}"),
                                );
                                state.push_message(
                                    MessageRole::Error,
                                    format!("blueprint run failed: {error}"),
                                )
                            }
                        }
                    }
                    None => state.push_message(
                        MessageRole::Error,
                        "blueprint execution is unavailable while the agent is busy",
                    ),
                },
                BlueprintAction::Handoff { target } => match agent.as_deref_mut() {
                    Some(agent) => match crate::headless::handoff_blueprint_next(agent, &target) {
                        Ok(data) => {
                            state.push_goal_message(
                                MessageRole::System,
                                "Blueprint handoff recorded",
                            );
                            state.push_message(
                                MessageRole::System,
                                format!(
                                    "blueprint handoff:\n{}",
                                    bounded_display(
                                        &serde_json::to_string(&data)
                                            .unwrap_or_else(|_| "{}".into())
                                    )
                                ),
                            )
                        }
                        Err(error) => {
                            state.push_goal_message(
                                MessageRole::Error,
                                format!("Blueprint handoff failed: {error}"),
                            );
                            state.push_message(
                                MessageRole::Error,
                                format!("blueprint handoff failed: {error}"),
                            )
                        }
                    },
                    None => state.push_message(
                        MessageRole::Error,
                        "blueprint handoff is unavailable while the agent is busy",
                    ),
                },
                BlueprintAction::Handoffs => match agent.as_deref() {
                    Some(agent) => match crate::headless::blueprint_handoffs_view(agent) {
                        Ok(data) => state.push_message(
                            MessageRole::System,
                            format!(
                                "blueprint handoffs:\n{}",
                                bounded_display(&data.to_string())
                            ),
                        ),
                        Err(error) => state.push_message(
                            MessageRole::Error,
                            format!("blueprint handoffs failed: {error}"),
                        ),
                    },
                    None => state.push_message(
                        MessageRole::Error,
                        "blueprint handoffs are unavailable while the agent is busy",
                    ),
                },
                action @ (BlueprintAction::Prepare { .. } | BlueprintAction::Accept { .. }) => {
                    match agent.as_deref_mut() {
                        Some(agent) => {
                            match crate::headless::external_evidence_action(agent, &action, &|| {
                                false
                            }) {
                                Ok(value) => state.push_message(
                                    MessageRole::System,
                                    format_external_evidence_projection(&value),
                                ),
                                Err(error) => state.push_message(MessageRole::Error, error),
                            }
                        }
                        None => state.push_message(
                            MessageRole::Error,
                            "external verification requires an idle owner",
                        ),
                    }
                }
                BlueprintAction::Import { target, path } => match agent.as_deref_mut() {
                    Some(agent) => {
                        match crate::headless::import_blueprint_manifest(agent, &target, &path) {
                            Ok(value) => {
                                state.push_goal_message(
                                    MessageRole::System,
                                    "Blueprint manifest imported",
                                );
                                state.push_goal_message(
                                    MessageRole::Assistant,
                                    format_external_evidence_projection(&value),
                                );
                            }
                            Err(error) => state.push_goal_message(
                                MessageRole::Error,
                                format!("blueprint manifest import failed: {error}"),
                            ),
                        }
                    }
                    None => state.push_goal_message(
                        MessageRole::Error,
                        "blueprint manifest import is unavailable while the agent is busy",
                    ),
                },
                BlueprintAction::Open { path: target } => {
                    state.push_message(
                        MessageRole::Error,
                        format!(
                            "blueprint {} is not executable in this host: {}",
                            action_display,
                            bounded_display(&target)
                        ),
                    );
                }
            }
        }
        SlashCommand::Learn { target } => {
            if target
                .as_deref()
                .is_none_or(|value| value.trim().is_empty() || value.eq_ignore_ascii_case("show"))
                || target
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case("status"))
            {
                match crate::headless::domain_store_view(agent.as_deref()) {
                    Ok(data) => state.push_message(
                        MessageRole::System,
                        format!(
                            "learn show:\n{}",
                            bounded_display(
                                &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                            )
                        ),
                    ),
                    Err(error) => state.push_message(
                        MessageRole::Error,
                        format!("learn store unavailable: {error}"),
                    ),
                }
            } else {
                state.push_message(
                    MessageRole::Error,
                    format!(
                        "learn execution requires an external owner: {}",
                        target
                            .as_deref()
                            .map(bounded_display)
                            .unwrap_or_else(|| "current target".into())
                    ),
                );
            }
        }
        SlashCommand::LearnPut { path } => match agent {
            Some(agent) => match crate::headless::persist_learn_path(agent, &path) {
                Ok(data) => state.push_message(
                    MessageRole::System,
                    format!(
                        "learn persisted:\n{}",
                        bounded_display(
                            &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                        )
                    ),
                ),
                Err(error) => state.push_message(
                    MessageRole::Error,
                    format!("learn persistence failed: {error}"),
                ),
            },
            None => state.push_message(
                MessageRole::Error,
                "learn persistence is unavailable while the agent is busy",
            ),
        },
        SlashCommand::LearnEvidence { id, reference } => match agent {
            Some(agent) => match crate::headless::add_learn_evidence(agent, &id, &reference) {
                Ok(data) => state.push_message(
                    MessageRole::System,
                    format!(
                        "learn evidence added:\n{}",
                        bounded_display(
                            &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                        )
                    ),
                ),
                Err(error) => state.push_message(
                    MessageRole::Error,
                    format!("learn evidence failed: {error}"),
                ),
            },
            None => state.push_message(
                MessageRole::Error,
                "learn evidence is unavailable while the agent is busy",
            ),
        },
        SlashCommand::LearnImport { id, path } => match agent.as_deref_mut() {
            Some(agent) => {
                match crate::headless::import_learn_manifest_for_tui(agent, &id, &path) {
                    Ok(data) => state.push_message(
                        MessageRole::System,
                        format!(
                            "learn manifest imported:\n{}",
                            bounded_display(
                                &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                            )
                        ),
                    ),
                    Err(error) => state.push_message(
                        MessageRole::Error,
                        format!("learn manifest import failed: {error}"),
                    ),
                }
            }
            None => state.push_message(
                MessageRole::Error,
                "learn manifest import is unavailable while the agent is busy",
            ),
        },
        SlashCommand::LearnResume { id } => match agent.as_deref() {
            Some(agent) => match crate::headless::resume_learn(agent, &id) {
                Ok(data) => state.push_message(
                    MessageRole::System,
                    format!(
                        "learn resume checkpoint:\n{}",
                        bounded_display(
                            &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                        )
                    ),
                ),
                Err(error) => {
                    state.push_message(MessageRole::Error, format!("learn resume failed: {error}"))
                }
            },
            None => state.push_message(
                MessageRole::Error,
                "learn resume is unavailable while the agent is busy",
            ),
        },
        SlashCommand::Compete { args } => {
            dispatch_runtime_intent(state, agent, crate::b3::RuntimeIntentKind::Compete, &args);
        }
        SlashCommand::Loop { args } => {
            dispatch_runtime_intent(state, agent, crate::b3::RuntimeIntentKind::Loop, &args);
        }
        SlashCommand::Sync { requirement } => {
            let workspace = agent
                .as_deref()
                .map(|agent| std::path::PathBuf::from(agent.session().header().cwd.clone()));
            let Some(workspace) = workspace else {
                state.push_message(
                    MessageRole::Error,
                    "sync cannot run while the agent is busy".to_owned(),
                );
                return SlashDispatchAction::Continue;
            };
            match crate::sync::sync_requirement(&workspace, &requirement) {
                Ok(receipt) => {
                    state.push_message(
                        MessageRole::System,
                        format!(
                            "sync: {} -> {} (duplicate={}, queued={})",
                            receipt.item_id, receipt.blueprint, receipt.duplicate, receipt.queued
                        ),
                    );
                    if !receipt.duplicate {
                        let args = vec![
                            "start".to_owned(),
                            receipt.item_id.clone(),
                            requirement.clone(),
                        ];
                        dispatch_runtime_intent(
                            state,
                            agent,
                            crate::b3::RuntimeIntentKind::Loop,
                            &args,
                        );
                    }
                }
                Err(error) => state
                    .push_message(MessageRole::Error, format!("sync failed: {error}")),
            }
        }
        SlashCommand::Execute { args } => {
            dispatch_runtime_intent(state, agent, crate::b3::RuntimeIntentKind::Execute, &args);
        }
        SlashCommand::Explore { args } => {
            dispatch_runtime_intent(state, agent, crate::b3::RuntimeIntentKind::Explore, &args);
        }
        SlashCommand::Worktree { action } => {
            use crate::slash::WorktreeAction;
            let message = match action {
                WorktreeAction::List => {
                    let active = state.active_subtab();
                    state
                        .subtabs()
                        .iter()
                        .enumerate()
                        .map(|(index, tab)| {
                            let kind = match tab.kind {
                                SubTabKind::Main => "main",
                                SubTabKind::Worktree => "worktree",
                                SubTabKind::InPlace => "in-place",
                            };
                            format!(
                                "{}{} [{kind}] {}",
                                if index == active { "*" } else { " " },
                                tab.name,
                                tab.root
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
                WorktreeAction::Add { in_place, name } => {
                    if in_place {
                        if state.subtab_add_in_place(name) {
                            "layer-2 tab added (in place)".to_owned()
                        } else {
                            "cannot add layer-2 tab".to_owned()
                        }
                    } else {
                        match state.subtab_add_worktree(name) {
                            Ok(name) => format!("worktree sub-tab added: {name}"),
                            Err(error) => format!("worktree add failed: {error}"),
                        }
                    }
                }
                WorktreeAction::Select { index } => {
                    if state.subtab_select(index) {
                        format!("sub-tab selected: {index}")
                    } else {
                        format!("no such sub-tab: {index}")
                    }
                }
                WorktreeAction::Close { index } => {
                    if state.subtab_close(index) {
                        format!("sub-tab closed: {index}")
                    } else {
                        format!("cannot close sub-tab: {index}")
                    }
                }
                WorktreeAction::Move { index, target } => {
                    if state.subtab_move(index, target) {
                        format!("sub-tab moved: {index} -> {target}")
                    } else {
                        format!("cannot move sub-tab: {index}")
                    }
                }
                WorktreeAction::Rename { index, name } => {
                    if state.subtab_rename(index, &name) {
                        format!("sub-tab renamed: {index} -> {name}")
                    } else {
                        format!("cannot rename sub-tab: {index}")
                    }
                }
                WorktreeAction::Concurrency { index, delta } => {
                    if state.subtab_concurrency(index, delta as isize) {
                        format!("sub-tab concurrency updated: {index} {delta:+}")
                    } else {
                        format!("cannot update concurrency: {index}")
                    }
                }
            };
            state.push_message(MessageRole::System, message);
        }
    }
    if !state.is_busy() {
        state.set_status("Ready");
    }
    SlashDispatchAction::Continue
}

/// Project runtime pool. Each Agent owns its backend, skills, approval queue,
/// session and tool root. Preparing another project never locks a running one.
pub struct ProjectRuntimeHost {
    pool: crate::project_workspace::ProjectOwnerPool,
}
impl ProjectRuntimeHost {
    pub fn new(
        state: &mut TuiState,
        agent: Arc<Mutex<crate::core::Agent>>,
    ) -> Result<Self, String> {
        let owner = agent.lock().map_err(|_| "agent lock poisoned")?;
        state.initialize_project_workspace(&owner)?;
        let cwd = owner
            .attachment_workspace_root()
            .unwrap_or_else(|| std::path::Path::new(&owner.session().header().cwd))
            .to_path_buf();
        let actual = crate::project_workspace::ProjectWorkspace::default()
            .with_directory(Some(&cwd), &cwd)
            .map_err(|e| e.to_string())?
            .0;
        let actual_id = actual.active().unwrap().id().as_str().to_owned();
        let wanted = state.active_project().to_owned();
        let metadata = ProjectTabMetadata::from_session(owner.session());
        drop(owner);
        let mut host = Self {
            pool: crate::project_workspace::ProjectOwnerPool::new(Arc::clone(&agent))?,
        };
        if !state.project_tabs.contains(&actual_id) {
            let workspace = state
                .project_workspace
                .as_ref()
                .unwrap()
                .with_directory(Some(&cwd), &cwd)
                .map_err(|e| e.to_string())?
                .0;
            state.project_workspace = Some(workspace);
            state.open_project_tab(actual_id.clone());
        }
        state.project_metadata.insert(actual_id.clone(), metadata);
        if wanted != actual_id {
            let index = state.project_index(&wanted).unwrap();
            if let Err(error) = host.apply(state, ProjectIntent::Select(index)) {
                state.select_project_tab(state.project_index(&actual_id).unwrap());
                let w = state.project_workspace.as_ref().unwrap();
                let id = w
                    .tabs()
                    .iter()
                    .find(|t| t.id().as_str() == actual_id)
                    .unwrap()
                    .id();
                state.project_workspace = Some(w.with_active(id).map_err(|e| e.to_string())?);
                state.push_message(
                    MessageRole::Error,
                    format!("Saved project could not be restored: {error}"),
                );
            }
        }
        let saved = state
            .project_metadata
            .iter()
            .filter_map(|(id, meta)| {
                meta.session_path
                    .as_ref()
                    .map(|path| (id.clone(), std::path::PathBuf::from(path)))
            })
            .collect();
        host.pool.publish(
            state
                .project_workspace
                .clone()
                .ok_or("missing project workspace")?,
            &saved,
        )?;
        Ok(host)
    }
    /// Production hosts share only project identities and session paths; the
    /// existing UI checkpoint remains responsible for drafts and pane state.
    pub fn restore_shared_workspace(&mut self, state: &mut TuiState) -> Result<(), String> {
        if self.pool.restore_checkpoint()? {
            let previous_session_paths: BTreeMap<_, _> = state
                .project_metadata
                .iter()
                .filter_map(|(id, metadata)| {
                    metadata
                        .session_path
                        .as_ref()
                        .map(|path| (id.clone(), std::path::PathBuf::from(path)))
                })
                .collect();
            let workspace = self.pool.workspace().clone();
            let wanted = workspace
                .active()
                .ok_or("no saved active project")?
                .id()
                .as_str()
                .to_owned();
            for tab in workspace.tabs() {
                let id = tab.id().as_str().to_owned();
                if state.project_index(&id).is_none() {
                    state.open_project_tab(id.clone());
                }
                let metadata = state.project_metadata.entry(id.clone()).or_default();
                metadata.cwd = tab.cwd().display().to_string();
                metadata.session_path = self
                    .pool
                    .sessions()
                    .get(&id)
                    .map(|path| path.display().to_string());
            }
            for id in state.project_tabs.clone() {
                if !workspace.tabs().iter().any(|tab| tab.id().as_str() == id) {
                    state.close_project_tab(&id);
                }
            }
            state.select_project_tab(
                state
                    .project_index(&wanted)
                    .ok_or("missing restored project")?,
            );
            state.project_workspace = Some(workspace);
            if let Ok(agent) = self.pool.active().lock() {
                let changed_session = state
                    .project_session_cursor(state.active_project())
                    .map_or_else(
                        || {
                            previous_session_paths
                                .get(state.active_project())
                                .is_some_and(|path| path.as_path() != agent.session().path())
                        },
                        |(id, _)| id != agent.session().session_id(),
                    );
                state
                    .set_active_project_metadata(ProjectTabMetadata::from_session(agent.session()));
                state.refresh_session_snapshot(agent.session());
                state.refresh_transcript_status(&agent);
                if changed_session || state.messages.is_empty() {
                    reload_state_from_agent(state, &agent);
                }
            }
        } else {
            self.pool
                .publish(self.pool.workspace().clone(), &Default::default())?;
        }
        state.project_checkpoint_dirty = true;
        state.cached_transcript = None;
        Ok(())
    }
    pub fn active(&self, state: &TuiState) -> Arc<Mutex<crate::core::Agent>> {
        self.pool
            .owner(state.active_project())
            .expect("committed project owner")
    }
    /// Resume through the shared durable owner before changing any view state.
    /// The same path serves slash actions and the session-list control.
    pub fn resume_session(
        &mut self,
        state: &mut TuiState,
        action: &crate::slash::SessionAction,
    ) -> Result<serde_json::Value, String> {
        if state.is_busy() {
            return Err("session cannot be opened while the project is busy".into());
        }
        let id = state.active_project().to_owned();
        let owner = self.pool.owner(&id).ok_or("project not found")?;
        let mut agent = owner
            .try_lock()
            .map_err(|_| "session cannot be opened while the agent is busy")?;
        let commit = |session: &crate::session::SessionStore| {
            self.pool
                .commit_resumed_session(&id, session)
                .map_err(crate::core::AgentError::InvalidTurn)
        };
        let data = match action {
            crate::slash::SessionAction::Open { path } => {
                crate::headless::open_session_path_with_commit(&mut agent, path, commit)?
            }
            crate::slash::SessionAction::ResumeLast => {
                crate::headless::resume_last_session_view_with_commit(&mut agent, commit)?
            }
            _ => return Err("expected session open or resume-last".into()),
        };
        let approval_mode = state
            .project_metadata(&id)
            .map(|meta| meta.approval_mode)
            .unwrap_or_default();
        let mut metadata = ProjectTabMetadata::from_session(agent.session());
        metadata.approval_mode = approval_mode;
        state.set_active_project_metadata(metadata);
        reload_state_from_agent(state, &agent);
        state.refresh_transcript_status(&agent);
        Ok(data)
    }

    pub fn apply(&mut self, state: &mut TuiState, intent: ProjectIntent) -> Result<(), String> {
        let current = state
            .project_workspace
            .as_ref()
            .ok_or("project host is not initialized")?;
        let base = current.active().ok_or("no active project")?.cwd();
        let workspace = match &intent {
            ProjectIntent::Open(path) => {
                current
                    .with_directory(Some(path), base)
                    .map_err(|e| e.to_string())?
                    .0
            }
            ProjectIntent::Select(index) => {
                let key = state.project_tabs.get(*index).ok_or("project not found")?;
                let tab = current
                    .tabs()
                    .iter()
                    .find(|t| t.id().as_str() == key)
                    .ok_or("project not found")?;
                current.with_active(tab.id()).map_err(|e| e.to_string())?
            }
            ProjectIntent::Close(key) => {
                if let Some(owner) = self.pool.owner(key) {
                    let owner=owner.try_lock().map_err(|_|"Project is running; switch freely, but wait or cancel before closing it.")?;
                    if owner.phase() == crate::core::AgentPhase::Running {
                        return Err("Project is running".into());
                    }
                }
                let tab = current
                    .tabs()
                    .iter()
                    .find(|t| t.id().as_str() == key)
                    .ok_or("project not found")?;
                current
                    .without_project(tab.id())
                    .map_err(|e| e.to_string())?
            }
        };
        let tab = workspace.active().ok_or("no active project")?;
        let id = tab.id().as_str().to_owned();
        // Confirm the shortest-path project action in the footer so a picker
        // selection is immediately visible even when the tab strip is narrow.
        let project_status = match &intent {
            ProjectIntent::Close(_) => "Project closed".to_owned(),
            ProjectIntent::Open(_) | ProjectIntent::Select(_) => {
                format!("Project ready · {}", tab.cwd().display())
            }
        };
        let saved_sessions = state
            .project_metadata
            .iter()
            .filter_map(|(key, metadata)| {
                metadata
                    .session_path
                    .as_ref()
                    .map(|path| (key.clone(), std::path::PathBuf::from(path)))
            })
            .collect();
        self.pool.publish(workspace.clone(), &saved_sessions)?;
        if let Ok(agent) = self.pool.active().try_lock() {
            state.project_metadata.insert(
                id.clone(),
                ProjectTabMetadata::from_session(agent.session()),
            );
        }
        if let Some(index) = state.project_index(&id) {
            state.select_project_tab(index);
        } else {
            state.open_project_tab(id.clone());
        }
        if let ProjectIntent::Close(key) = intent {
            state.close_project_tab(&key);
        }
        state.project_workspace = Some(workspace);
        state.project_checkpoint_dirty = true;
        state.cached_transcript = None;
        state.set_status(project_status);
        // A running existing owner needs no lock to switch its saved view.
        if let Ok(agent) = self.active(state).try_lock() {
            let changed_session = state
                .project_session_cursor(state.active_project())
                .map_or_else(
                    || {
                        saved_sessions
                            .get(state.active_project())
                            .is_some_and(|path| path.as_path() != agent.session().path())
                    },
                    |(id, _)| id != agent.session().session_id(),
                );
            state.set_active_project_metadata(ProjectTabMetadata::from_session(agent.session()));
            state.refresh_session_snapshot(agent.session());
            state.refresh_transcript_status(&agent);
            if changed_session || state.messages.is_empty() {
                reload_state_from_agent(state, &agent);
            }
        }
        Ok(())
    }
}

fn mailbox_result_heading(data: &serde_json::Value) -> &'static str {
    let has_reply = data
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|messages| {
            messages.iter().any(|entry| {
                entry
                    .get("message")
                    .and_then(|message| message.get("payload"))
                    .and_then(|payload| payload.get("in_reply_to"))
                    .is_some()
            })
        });
    if has_reply {
        "mailbox: correlated result received"
    } else {
        "mailbox:"
    }
}

/// Bind the shared Agent to the project currently selected by the TUI. This
/// is deliberately kept at the host boundary: project tabs own session and
/// cwd, while the workspace renderer remains agent-agnostic.
fn bind_agent_to_active_project(state: &mut TuiState, agent: &mut crate::core::Agent) {
    let metadata = state
        .project_metadata(state.active_project())
        .cloned()
        .unwrap_or_default();
    if let Some(path) = metadata.session_path.as_deref()
        && let Err(error) = agent.resume_session(path.to_owned())
    {
        state.push_message(
            MessageRole::Error,
            format!("project session switch failed: {error}"),
        );
    }
    if !metadata.cwd.trim().is_empty() {
        match crate::tools::ToolContext::new(&metadata.cwd) {
            Ok(context) => agent.set_attachment_workspace(context),
            Err(error) => state.push_message(
                MessageRole::Error,
                format!("project workspace switch failed: {error}"),
            ),
        }
    }
    let _ = agent.set_approval_policy(crate::approval::ApprovalPolicy {
        mode: metadata.approval_mode,
        ..crate::approval::ApprovalPolicy::default()
    });
    state.refresh_session_snapshot(agent.session());
}

fn dispatch_runtime_intent(
    state: &mut TuiState,
    agent: Option<&mut crate::core::Agent>,
    kind: crate::b3::RuntimeIntentKind,
    args: &[String],
) {
    let Some(agent) = agent else {
        state.push_message(
            MessageRole::Error,
            format!(
                "{} intent cannot be persisted while the agent is busy",
                kind.as_str()
            ),
        );
        return;
    };
    match crate::runtime_intent::runtime_intent_value(agent, kind, args) {
        Ok(data) => state.push_message(
            MessageRole::System,
            format!(
                "{} runtime intent: delivery=journal_only, stored={}, zenpi_started=false, external_state=untracked, intent_id={}",
                kind.as_str(),
                data.get("persisted")
                    .or_else(|| data.get("records_durable"))
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                data.pointer("/intent/intent_id")
                    .or_else(|| data.get("latest_intent_id"))
                    .and_then(serde_json::Value::as_str)
                    .map(|value| inline_token(value, 160))
                    .unwrap_or_else(|| "<status>".into()),
            ),
        ),
        Err(error) => state.push_message(
            MessageRole::Error,
            format!("{} runtime intent failed: {error}", kind.as_str()),
        ),
    }
}

fn dispatch_layout_command(state: &mut TuiState, action: LayoutAction) {
    match action {
        LayoutAction::Show { tab } => {
            let summary = layout_summary_for_tab(state, tab);
            state.push_message(
                MessageRole::System,
                format!(
                    "layout show:\n{}",
                    bounded_display(
                        &serde_json::to_string(&summary).unwrap_or_else(|_| "{}".into())
                    )
                ),
            );
        }
        LayoutAction::Preset { tab } => {
            let target = tab.unwrap_or_else(|| state.workspace_tab());
            state.set_workspace_tab(target);
            state.reset_workspace_layout();
            state.push_message(
                MessageRole::System,
                format!("layout preset applied: {target}"),
            );
        }
        LayoutAction::Reset { tab } => {
            let target = tab.unwrap_or_else(|| state.workspace_tab());
            state.set_workspace_tab(target);
            state.reset_workspace_layout();
            state.push_message(MessageRole::System, format!("layout reset: {target}"));
        }
        LayoutAction::Save => {
            state.request_layout_save();
            state.push_message(
                MessageRole::System,
                "layout save requested; preferences will be written at the next checkpoint",
            );
        }
    }
}

fn dispatch_pane_command(state: &mut TuiState, action: PaneAction) {
    match action {
        PaneAction::Show => {
            let summary = state.workspace_layout_summary();
            state.push_message(
                MessageRole::System,
                format!(
                    "pane state:\n{}",
                    bounded_display(
                        &serde_json::to_string(&summary).unwrap_or_else(|_| "{}".into())
                    )
                ),
            );
        }
        PaneAction::Focus { pane } => {
            if state.focus_workspace_pane(pane) {
                state.push_message(MessageRole::System, format!("pane focused: {pane}"));
            } else {
                state.push_message(
                    MessageRole::Error,
                    format!("pane unavailable in the active tab: {pane}"),
                );
            }
        }
        PaneAction::Collapse { pane } => {
            if state.set_workspace_pane_collapsed(pane, true) {
                state.push_message(MessageRole::System, format!("pane collapsed: {pane}"));
            } else {
                state.push_message(
                    MessageRole::Error,
                    format!("pane unavailable in the active tab: {pane}"),
                );
            }
        }
        PaneAction::Expand { pane } => {
            if state.set_workspace_pane_collapsed(pane, false) {
                state.push_message(MessageRole::System, format!("pane expanded: {pane}"));
            } else {
                state.push_message(
                    MessageRole::Error,
                    format!("pane unavailable in the active tab: {pane}"),
                );
            }
        }
        PaneAction::Toggle { pane } => match state.toggle_workspace_pane(pane) {
            Some(collapsed) => state.push_message(
                MessageRole::System,
                format!(
                    "pane {}: {pane}",
                    if collapsed { "collapsed" } else { "expanded" }
                ),
            ),
            None => state.push_message(
                MessageRole::Error,
                format!("pane unavailable in the active tab: {pane}"),
            ),
        },
    }
}

fn layout_summary_for_tab(state: &TuiState, tab: Option<TabId>) -> serde_json::Value {
    let target = tab.unwrap_or_else(|| state.workspace_tab());
    if target == state.workspace_tab() {
        return state.workspace_layout_summary();
    }
    let model = state
        .workspace_layout_states()
        .into_iter()
        .find(|layout| layout.tab == target)
        .unwrap_or_else(|| LayoutModel::new(target));
    let (width, height) = state.workspace_viewport();
    let snapshot = model.compute(width, height);
    serde_json::json!({
        "tab": model.tab,
        "ratios": model.ratios,
        "focused": model.focused,
        "collapsed": model.collapsed,
        "breakpoint": snapshot.breakpoint,
        "visible_panes": snapshot.visible_panes().map(|pane| pane.id).collect::<Vec<_>>(),
        "layout_dirty": state.layout_dirty,
    })
}

fn format_agent_status(agent: &crate::core::Agent) -> String {
    let snapshot = agent.snapshot();
    format!(
        "phase={:?} backend={} model={} active_turn={} session_turns={}",
        snapshot.phase,
        inline_token(&snapshot.backend, 96),
        snapshot
            .model
            .as_deref()
            .map(|value| inline_token(value, 160))
            .unwrap_or_else(|| "<provider default>".into()),
        snapshot
            .active_turn_id
            .as_deref()
            .map(|value| inline_token(value, 96))
            .unwrap_or_else(|| "<none>".into()),
        snapshot.session.turn_count,
    )
}

fn turn_reasoning(turn: &crate::core::Turn) -> Vec<&str> {
    turn.metadata
        .as_ref()
        .and_then(|m| m.get("annotations"))
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten()
        .filter(|a| a.get("type").and_then(|v| v.as_str()) == Some("reasoning"))
        .filter_map(|a| a.get("text").and_then(|v| v.as_str()))
        .collect()
}
fn restore_turn_reasoning(state: &mut TuiState, turn: &crate::core::Turn) {
    for text in turn_reasoning(turn) {
        state.push_message(MessageRole::Reasoning, text);
    }
}
fn finish_turn_reasoning(state: &mut TuiState, job: u64, turn: &crate::core::Turn) {
    let text = turn_reasoning(turn).join("\n");
    if text.is_empty() {
        return;
    }
    if state.transcript_ux.reasoning_job == Some(job) {
        if let Some(message) = state
            .messages
            .iter_mut()
            .rev()
            .find(|m| m.role == MessageRole::Reasoning)
        {
            message.text = bound_text(text);
            state.cached_transcript = None;
            state.dirty = true;
        }
    } else {
        state.push_message(MessageRole::Reasoning, text);
        state.transcript_ux.reasoning_job = Some(job);
    }
}

fn reload_state_from_agent(state: &mut TuiState, agent: &crate::core::Agent) {
    let turns = match agent.selected_history() {
        Ok(turns) => turns,
        Err(error) => {
            state.push_message(MessageRole::Error, error.to_string());
            return;
        }
    };
    state.clear_messages();
    for turn in &turns {
        let role = match turn.role {
            crate::core::TurnRole::User => MessageRole::User,
            crate::core::TurnRole::Assistant => MessageRole::Assistant,
            crate::core::TurnRole::Tool => MessageRole::Tool,
            crate::core::TurnRole::System => MessageRole::System,
        };
        restore_turn_reasoning(state, turn);
        state.push_message(role, &turn.content);
    }
    state.refresh_session_snapshot(agent.session());
}

fn format_history(
    agent: Option<&crate::core::Agent>,
    state: &TuiState,
    limit: Option<usize>,
) -> String {
    let limit = limit.unwrap_or(20).clamp(1, 128);
    if let Some(agent) = agent {
        let turns = match agent.selected_history() {
            Ok(turns) => turns,
            Err(error) => return format!("history unavailable: {error}"),
        };
        if turns.is_empty() {
            return "history: empty".into();
        }
        let start = turns.len().saturating_sub(limit);
        return turns[start..]
            .iter()
            .map(|turn| {
                format!(
                    "{} {}: {}",
                    inline_token(&turn.id, 96),
                    format!("{:?}", turn.role).to_ascii_lowercase(),
                    bounded_display(&turn.content)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    let messages = state.messages.iter().rev().take(limit).collect::<Vec<_>>();
    if messages.is_empty() {
        "history: empty".into()
    } else {
        messages
            .into_iter()
            .rev()
            .map(|message| {
                format!(
                    "{}: {}",
                    message.role.label(),
                    bounded_display(&message.text)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn blueprint_action_display(action: &BlueprintAction) -> String {
    match action {
        BlueprintAction::Show => "show".into(),
        BlueprintAction::Status => "status".into(),
        BlueprintAction::Validate { path } => format!(
            "validate {}",
            path.as_deref()
                .map(bounded_display)
                .unwrap_or_else(|| "configured blueprint".into())
        ),
        BlueprintAction::Put { path } => format!("put {}", bounded_display(path)),
        BlueprintAction::Run { target } => format!("run {}", bounded_display(target)),
        BlueprintAction::Handoff { target } => format!("handoff {}", bounded_display(target)),
        BlueprintAction::Handoffs => "handoffs".into(),
        BlueprintAction::Import { target, path } => format!(
            "import {} {}",
            bounded_display(target),
            bounded_display(path)
        ),
        BlueprintAction::Prepare { target, path } => format!(
            "prepare {} {}",
            bounded_display(target),
            bounded_display(path)
        ),
        BlueprintAction::Accept { target, claim } => format!(
            "accept {} {}",
            bounded_display(target),
            bounded_display(claim)
        ),
        BlueprintAction::Open { path } => format!("open {}", bounded_display(path)),
    }
}

fn session_action_label(action: &crate::slash::SessionAction) -> &'static str {
    match action {
        crate::slash::SessionAction::New => "new",
        crate::slash::SessionAction::List => "list",
        crate::slash::SessionAction::Search { .. } => "search",
        crate::slash::SessionAction::Agents => "agents",
        crate::slash::SessionAction::Inspect { .. } => "inspect",
        crate::slash::SessionAction::Open { .. } => "open",
        crate::slash::SessionAction::ResumeLast => "resume-last",
        crate::slash::SessionAction::Fork { .. } => "fork",
        crate::slash::SessionAction::Export { .. } => "export",
        crate::slash::SessionAction::Import { .. } => "import",
        crate::slash::SessionAction::Migrate { .. } => "migrate",
        crate::slash::SessionAction::Archive { .. } => "archive",
        crate::slash::SessionAction::Unarchive { .. } => "unarchive",
        crate::slash::SessionAction::Delete { .. } => "delete",
        crate::slash::SessionAction::RetireMailbox { .. } => "retire-mailbox",
        crate::slash::SessionAction::Queue { .. } => "queue",
        crate::slash::SessionAction::Gc { .. } => "gc",
    }
}

fn bounded_display(value: &str) -> String {
    inline_token(value, 512)
}

fn format_resource_view(value: &serde_json::Value) -> String {
    if !value["resources"].is_object() {
        return bounded_display(&serde_json::to_string_pretty(value).unwrap_or_default());
    }
    // Put the bounded owner catalogue's source index before descriptions. A
    // large description must not consume the message budget before paths from
    // later entries are visible. The owner already returns at most16 per kind.
    let mut text = String::from("Resource sources (full paths):\n");
    for kind in ["skills", "templates"] {
        if let Some(entries) = value["resources"][kind].as_array() {
            for entry in entries.iter().take(16) {
                if let Some(source) = entry["source"].as_str() {
                    text.push_str(&format!(
                        "{kind}: {} · {}\n",
                        serde_json::to_string(source).unwrap_or_default(),
                        entry["source_hash"].as_str().unwrap_or("")
                    ));
                }
            }
        }
    }
    text.push('\n');
    text.push_str(&serde_json::to_string_pretty(value).unwrap_or_default());
    if text.len() > MAX_MESSAGE_BYTES {
        const NOTICE: &str = "\n[resource detail truncated; source index above]";
        text.truncate(truncate_bytes(&text, MAX_MESSAGE_BYTES - NOTICE.len()).len());
        text.push_str(NOTICE);
    }
    text
}

#[test]
fn resource_source_index_precedes_oversize_unicode_descriptions() {
    let source = format!("/{}/工作 e\u{301}/SKILL.md", "long space/".repeat(70));
    let value = serde_json::json!({"resources":{"skills":[
        {"source":"/first/SKILL.md","description":"界".repeat(MAX_MESSAGE_BYTES)},
        {"source":source,"source_hash":"abc"}
    ]}});
    let mut state = TuiState::default();
    state.push_message(MessageRole::System, format_resource_view(&value));
    let message = state.messages().last().unwrap();
    assert!(message.text.len() <= MAX_MESSAGE_BYTES);
    assert!(message.text.contains(&source));
    assert!(
        message
            .text
            .ends_with("[resource detail truncated; source index above]")
    );
    assert!(!message.text.contains('\u{fffd}'));
    let controls = format_resource_view(&serde_json::json!({"resources":{"skills":[
        {"source":"/control\u{1b}[31m\n/SKILL.md"}
    ]}}));
    assert!(!controls.contains('\u{1b}'));
    assert!(controls.contains("\\u001b"));
}

/// Render a diff owner response without collapsing its hunks into the short
/// one-line diagnostic bound used by ordinary slash-command messages.
fn format_diff_view(value: &serde_json::Value) -> String {
    let path = value
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(".");
    let changed = value
        .get("changed")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let truncated = value
        .get("truncated")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let status = value
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let diff = value
        .get("diff")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let mut rendered = format!(
        "diff {} changed={} truncated={}",
        bounded_display(path),
        changed,
        truncated
    );
    if !status.is_empty() {
        rendered.push_str("\nstatus: ");
        rendered.push_str(status);
    }
    if !diff.is_empty() {
        rendered.push('\n');
        rendered.push_str(diff);
    }
    rendered
}

fn signal_status(status: crate::resources::SignalStatus) -> &'static str {
    match status {
        crate::resources::SignalStatus::Available => "available",
        crate::resources::SignalStatus::Unavailable => "unavailable",
    }
}

fn format_resource_summary(snapshot: &crate::resources::ResourceSnapshot) -> String {
    format!(
        "resources: files={} dirs={} bytes={} truncated={} cpu={} memory={} disk={} gpu={} network={} processes={} lsp={} mcp={}",
        snapshot.workspace.files,
        snapshot.workspace.directories,
        snapshot.workspace.bytes,
        snapshot.workspace.truncated,
        signal_status(snapshot.cpu.status),
        signal_status(snapshot.memory.status),
        signal_status(snapshot.disk.status),
        signal_status(snapshot.gpu.status),
        signal_status(snapshot.network.status),
        snapshot.processes.total,
        snapshot
            .processes
            .count_for(crate::resources::ProcessClass::Lsp),
        snapshot
            .processes
            .count_for(crate::resources::ProcessClass::Mcp),
    )
}

fn format_byte_count(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn format_gpu_signal(gpu: &crate::resources::GpuSignal) -> String {
    if gpu.devices.is_empty() {
        return "gpu unavailable".into();
    }
    let devices: Vec<String> = gpu
        .devices
        .iter()
        .map(|device| {
            let utilization = device
                .utilization_percent
                .map(|value| format!("{value:.0}%"))
                .unwrap_or_else(|| "util --".into());
            let memory = match (device.memory_used_bytes, device.memory_total_bytes) {
                (Some(used), Some(total)) => {
                    format!("{}/{}", format_byte_count(used), format_byte_count(total))
                }
                _ => "mem --".into(),
            };
            format!("{} {utilization} {memory}", device.name)
        })
        .collect();
    let mut line = format!("gpu {}", devices.join("; "));
    if gpu.truncated {
        line.push_str(" ...");
    }
    line
}

fn format_network_signal(network: &crate::resources::NetworkSignal) -> String {
    match (network.received_bytes, network.transmitted_bytes) {
        (Some(received), Some(transmitted)) => format!(
            "net rx {}  tx {}",
            format_byte_count(received),
            format_byte_count(transmitted)
        ),
        _ => "net unavailable".into(),
    }
}

fn format_process_summary(processes: &crate::resources::ProcessSummary) -> String {
    if processes.total == 0 {
        return "processes unavailable".into();
    }
    let mut line = format!(
        "processes {} total  {}",
        processes.total,
        format_byte_count(processes.resident_bytes)
    );
    if processes.truncated {
        line.push_str("  truncated");
    }
    line
}

/// Run the interactive mode for zenpi's shared agent.
pub fn run(agent: &mut crate::core::Agent) -> Result<(), crate::error::ZenpiError> {
    let mut state = TuiState::default();
    reload_state_from_agent(&mut state, agent);
    let config = TuiConfig::default();
    run_with_state_controls(config, state, true, |text, state| {
        if crate::slash::input_queue_control(&text).is_some() {
            let original = state.bind_submitted_input(text.clone());
            let result = crate::slash_actions::input_queue_control_request(
                agent.session().session_id(),
                "tui-input-control",
                &text,
            )
            .and_then(|request| {
                agent.input_queue_request(request.expect("recognized input control"))
            })
            .map_err(|error| error.to_string());
            TuiInputControls::show_result(state, result, original);
            return Ok(());
        }
        if crate::slash::output_control(&text).is_some() {
            match crate::slash_actions::output_control_request(
                agent.session().session_id(),
                "tui-output-control",
                &text,
            )
            .and_then(|r| {
                agent
                    .tool_output_control(r.expect("recognized control"), &|| false)
                    .map_err(|e| crate::core::AgentError::InvalidTurn(e.to_string()))
            }) {
                Ok(value) => {
                    state.push_message(MessageRole::System, format_output_projection(&value))
                }
                Err(error) => state.push_message(MessageRole::Error, error.to_string()),
            }
            return Ok(());
        }
        if dispatch_tree_input(state, Some(agent), &text) {
            return Ok(());
        }
        let result = agent.process_sync(text);
        for event in agent.take_events() {
            apply_agent_tool_event(state, event);
        }
        match result {
            Ok(result) => {
                if let Some(assistant) = result.assistant {
                    state.push_message(MessageRole::Assistant, assistant.content);
                }
                Ok::<(), crate::core::AgentError>(())
            }
            Err(error) => Err(error),
        }
    })
    .map_err(|error| crate::error::ZenpiError::Message(error.to_string()))
}

/// The UI owns only outstanding control tickets. The Agent owns the durable queue.
#[derive(Default)]
pub struct TuiInputControls {
    ports: HashMap<String, crate::input_queue::InputPort>,
    pending: Vec<TuiInputControl>,
    serial: u64,
}
struct TuiInputControl {
    project: String,
    original: SubmittedInput,
    ticket: crate::input_queue::InputTicket,
}
impl TuiInputControls {
    pub fn remember_owner(&mut self, project: &str, agent: &crate::core::Agent) {
        self.ports.insert(project.to_owned(), agent.input_port());
    }
    fn next_id(&mut self) -> String {
        self.serial = self.serial.saturating_add(1);
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("tui-{time}-{}", self.serial)
    }
    fn has_pending(&self, project: &str) -> bool {
        self.pending.iter().any(|p| p.project == project)
    }
    pub fn submit(
        &mut self,
        state: &mut TuiState,
        owner: &Arc<Mutex<crate::core::Agent>>,
        action: crate::protocol::InputQueueAction,
        original: String,
    ) {
        let original = state.bind_submitted_input(original);
        if let crate::protocol::InputQueueAction::Enqueue { text, .. }
        | crate::protocol::InputQueueAction::Edit { text, .. } = &action
        {
            if !text.starts_with('/')
                && let Err(error) = prompt_file_references(text)
            {
                Self::show_result(state, Err(error), original);
                return;
            }
            if !text.starts_with('/')
                && prompt_file_references(text).is_ok_and(|references| !references.is_empty())
            {
                Self::show_result(state, Err("File references require a scheduled or idle prompt; in-turn queue inputs are text-only".into()), original);
                return;
            }
            if text.starts_with('/')
                && state.command_resources.iter().any(|resource| {
                    text.split_whitespace().next() == Some(resource.command.as_str())
                })
            {
                Self::show_result(
                    state,
                    Err("Skill/template invocation requires a scheduled or idle prompt".into()),
                    original,
                );
                return;
            }
        }
        let project = state.active_project().to_owned();
        let id = self.next_id();
        let result = match owner.try_lock() {
            Ok(mut agent) => {
                self.remember_owner(&project, &agent);
                let request = crate::protocol::InputQueueRequest {
                    schema_version: crate::protocol::PROTOCOL_VERSION,
                    id,
                    kind: "input_queue".into(),
                    session_id: agent.session().session_id().to_owned(),
                    input_queue: action,
                };
                Some(
                    agent
                        .input_queue_request(request)
                        .map_err(|e| e.to_string()),
                )
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                if self.pending.len() >= 32 {
                    Some(Err(
                        "Input control capacity exceeded; draft unchanged".into()
                    ))
                } else if let Some(port) = self.ports.get(&project) {
                    let request = crate::protocol::InputQueueRequest {
                        schema_version: crate::protocol::PROTOCOL_VERSION,
                        id,
                        kind: "input_queue".into(),
                        session_id: port.session_id(),
                        input_queue: action,
                    };
                    match port.submit(request) {
                        Ok(ticket) => {
                            self.pending.push(TuiInputControl {
                                project,
                                original: original.clone(),
                                ticket,
                            });
                            state.push_message(
                                MessageRole::System,
                                "Input update pending; awaiting durable owner receipt",
                            );
                            None
                        }
                        Err(error) => Some(Err(error)),
                    }
                } else {
                    Some(Err(
                        "Current project input owner unavailable; draft unchanged".into(),
                    ))
                }
            }
            Err(_) => Some(Err("Input owner lock poisoned".into())),
        };
        if let Some(result) = result {
            Self::show_result(state, result, original);
        }
    }
    fn show_result(
        state: &mut TuiState,
        result: Result<crate::input_queue::InputQueueReply, String>,
        original: SubmittedInput,
    ) {
        match result {
            Ok(reply) => {
                // The receipt owns this command's metadata; success drops it.
                state.update_input_queue(&reply);
                let line = |input: &crate::input_queue::QueuedInput| {
                    let status = match input.status {
                        crate::input_queue::InputStatus::Received => "received",
                        crate::input_queue::InputStatus::Applied => "applied",
                        crate::input_queue::InputStatus::Cancelled => "cancelled",
                    };
                    format!(
                        "{} · {} · rev {} · {:?}\n{}",
                        input.id,
                        status,
                        input.revision,
                        input.kind,
                        inline_token(&input.text, 120)
                    )
                };
                let description = match &reply {
                    crate::input_queue::InputQueueReply::Input { input, duplicate } => format!(
                        "{}{}",
                        line(input),
                        if *duplicate {
                            "\nDuplicate receipt; no second enqueue"
                        } else {
                            ""
                        }
                    ),
                    crate::input_queue::InputQueueReply::Page {
                        inputs,
                        next_sequence,
                    } => {
                        let mut text = if inputs.is_empty() {
                            "Queue empty".to_owned()
                        } else {
                            inputs.iter().map(line).collect::<Vec<_>>().join("\n")
                        };
                        if let Some(cursor) = next_sequence {
                            text.push_str(&format!("\nNext: /input list {cursor}"));
                        }
                        text
                    }
                    crate::input_queue::InputQueueReply::Modes { steer, follow_up } => {
                        format!("Modes: steer {steer:?}; follow-up {follow_up:?}")
                    }
                };
                state.push_message(
                    MessageRole::System,
                    format!("Input owner confirmed:\n{description}"),
                );
            }
            Err(error) => {
                state.push_message(
                    MessageRole::Error,
                    format!("Input update rejected: {error}"),
                );
                state.restore_submitted_input(original);
            }
        }
    }
    pub fn poll(&mut self, state: &mut TuiState) -> bool {
        self.ports
            .retain(|project, _| state.project_index(project).is_some());
        let visible = state.active_project().to_owned();
        let mut changed = false;
        let mut index = 0;
        while index < self.pending.len() {
            let result = match self.pending[index].ticket.try_recv() {
                Ok(result) => result,
                Err(std::sync::mpsc::TryRecvError::Empty) => { index += 1; continue; }
                Err(_) => Err("Input owner disconnected before confirmation; inspect /input list before retry".into()),
            };
            let pending = self.pending.remove(index);
            if let Some(project) = state.project_index(&pending.project) {
                state.select_project_tab(project);
                Self::show_result(state, result, pending.original);
                changed = true;
            }
        }
        if let Some(index) = state.project_index(&visible) {
            state.select_project_tab(index);
        }
        changed
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TuiRequestKind {
    Model,
    UserShell,
    ResourceControl,
}

enum TuiJobResult {
    Model(crate::core::ProcessResult),
    UserShell(serde_json::Value),
    ResourceControl(serde_json::Value),
}

struct TuiRequest {
    owner: Option<Arc<Mutex<crate::core::Agent>>>,
    text: String,
    kind: TuiRequestKind,
    provider_events: Arc<Mutex<TuiProviderEventBuffer>>,
}

impl TuiRequest {
    fn new(text: String) -> (Self, Arc<Mutex<TuiProviderEventBuffer>>) {
        Self::with_kind(text, TuiRequestKind::Model)
    }

    fn with_kind(text: String, kind: TuiRequestKind) -> (Self, Arc<Mutex<TuiProviderEventBuffer>>) {
        let provider_events = Arc::new(Mutex::new(TuiProviderEventBuffer::default()));
        (
            Self {
                owner: None,
                text,
                kind,
                provider_events: Arc::clone(&provider_events),
            },
            provider_events,
        )
    }
}

type NewSessionResult = Result<crate::project_workspace::NewSessionCommit, crate::core::AgentError>;
type NewSessionResultCell = Arc<Mutex<Option<NewSessionResult>>>;
type NewSessionRequest = (
    Arc<Mutex<crate::core::Agent>>,
    crate::project_workspace::NewSessionPlan,
    NewSessionResultCell,
);
type NewSessionWorker = crate::runtime::BackgroundRunner<
    NewSessionRequest,
    (),
    (),
    fn(NewSessionRequest, crate::runtime::CancellationToken) -> Result<(), ()>,
>;

struct NewSessionJob {
    id: crate::runtime::JobId,
    project: String,
    owner: Arc<Mutex<crate::core::Agent>>,
    submitted: SubmittedInput,
    result: NewSessionResultCell,
    detached: bool,
}

/// A local session transition can prepare while another project's provider
/// runs. Both workers still use the existing project Agent and its mutex.
struct NewSessionControl {
    worker: NewSessionWorker,
    active: Option<NewSessionJob>,
}

impl NewSessionControl {
    fn run(
        (owner, plan, result_cell): NewSessionRequest,
        token: crate::runtime::CancellationToken,
    ) -> Result<(), ()> {
        // The UI briefly reads the same Agent to refresh its view. Give that
        // reader a bounded opportunity to release it without blocking input.
        let deadline = Instant::now() + Duration::from_millis(100);
        let mut result = loop {
            if token.is_cancelled() {
                break Err(crate::core::AgentError::Backend(
                    crate::backend::BackendError::Cancelled,
                ));
            }
            match owner.try_lock() {
                Ok(mut agent) => break plan.execute(&mut agent, &|| token.is_cancelled()),
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    break Err(crate::core::AgentError::InvalidTurn(
                        "new session owner lock poisoned".into(),
                    ));
                }
                Err(std::sync::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    break Err(crate::core::AgentError::InvalidTurn(
                        "new session owner is busy".into(),
                    ));
                }
            }
        };
        if result.is_ok()
            || matches!(
                &result,
                Err(crate::core::AgentError::Session(
                    crate::session::SessionError::CandidateRetained(_)
                ))
            )
        {
            token.mark_completed();
        }
        if token.is_cancelled() {
            // Extension/tool preparation reports its own cancellation error.
            // Preserve the runtime's cancellation semantics in the result
            // cell too, including when delivery follows a shutdown detach.
            result = Err(crate::core::AgentError::Backend(
                crate::backend::BackendError::Cancelled,
            ));
        }
        // Runtime shutdown may detach after its grace period. Preserve the
        // actual result independently so Cancelled/Closed cannot invent a
        // rollback or permit another operation on a still-running owner.
        *result_cell
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(result);
        Ok(())
    }

    fn new() -> Self {
        Self {
            worker: crate::runtime::BackgroundRunner::spawn(
                Self::run
                    as fn(NewSessionRequest, crate::runtime::CancellationToken) -> Result<(), ()>,
                crate::runtime::RuntimeConfig {
                    max_pending: 0,
                    ..Default::default()
                },
            ),
            active: None,
        }
    }

    fn owns(&self, project: &str) -> bool {
        self.active
            .as_ref()
            .is_some_and(|job| job.project == project)
    }

    fn start(
        &mut self,
        state: &mut TuiState,
        owner: Arc<Mutex<crate::core::Agent>>,
        plan: crate::project_workspace::NewSessionPlan,
        text: &str,
    ) -> Result<(), String> {
        if self.active.is_some() {
            return Err("another new session is being prepared; retry when it finishes".into());
        }
        let result = Arc::new(Mutex::new(None));
        let id = self
            .worker
            .try_submit((Arc::clone(&owner), plan, Arc::clone(&result)))
            .map_err(|e| e.to_string())?;
        self.active = Some(NewSessionJob {
            id,
            project: state.active_project().to_owned(),
            owner,
            submitted: state.bind_submitted_input(text.to_owned()),
            result,
            detached: false,
        });
        state.set_busy(true);
        state.set_status("Preparing new session");
        Ok(())
    }

    fn cancel_current(&self, state: &mut TuiState) -> bool {
        let Some(job) = self
            .active
            .as_ref()
            .filter(|job| job.project == state.active_project())
        else {
            return false;
        };
        match self.worker.try_cancel(job.id) {
            Ok(()) => state.set_status("Interrupt requested"),
            Err(error) => state.push_message(
                MessageRole::Error,
                format!("Cancel was not queued: {error}"),
            ),
        }
        true
    }

    fn apply_event(
        &mut self,
        event: crate::runtime::RuntimeEvent<(), ()>,
        state: &mut TuiState,
        host: &mut ProjectRuntimeHost,
        inputs: &mut TuiInputControls,
    ) -> bool {
        use crate::runtime::{JobOutcome, RuntimeEvent};
        let (id, runtime_outcome, rejection) = match event {
            RuntimeEvent::Completed { id, outcome } => (id, outcome, None),
            RuntimeEvent::Rejected { id, reason } => {
                (id, JobOutcome::Failed(()), Some(reason.to_string()))
            }
            _ => return false,
        };
        let Some(job) = self.active.as_mut().filter(|job| job.id == id) else {
            return false;
        };
        let result = job
            .result
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if result.is_none() && matches!(runtime_outcome, JobOutcome::Cancelled) {
            job.detached = true;
            let visible = state.active_project().to_owned();
            if let Some(index) = state.project_index(&job.project) {
                state.select_project_tab(index);
                state.set_busy(true);
                state.push_message(MessageRole::Error, "New session shutdown is unfinished; restore this project from its durable checkpoint before retrying");
                state.set_status("New session recovery pending");
            }
            if let Some(index) = state.project_index(&visible) {
                state.select_project_tab(index);
            }
            return true;
        }
        let outcome = match result {
            Some(Ok(committed)) => JobOutcome::Succeeded(committed),
            Some(Err(crate::core::AgentError::Backend(
                crate::backend::BackendError::Cancelled,
            ))) => JobOutcome::Cancelled,
            Some(Err(error)) => JobOutcome::Failed(error),
            None if matches!(runtime_outcome, JobOutcome::Panicked) => JobOutcome::Panicked,
            None => JobOutcome::Failed(crate::core::AgentError::InvalidTurn(
                rejection
                    .unwrap_or_else(|| "new session worker returned without its result".into()),
            )),
        };
        let job = self.active.take().expect("matching new session job");
        let visible = state.active_project().to_owned();
        if let JobOutcome::Succeeded(committed) = &outcome {
            host.pool.accept_new_session(committed);
        }
        if let Some(index) = state.project_index(&job.project) {
            state.select_project_tab(index);
            state.set_busy(false);
            drain_agent_tool_events(&job.owner, state);
            match outcome {
                JobOutcome::Succeeded(_) => {
                    if let Ok(agent) = job.owner.try_lock() {
                        let mode = state
                            .project_metadata(&job.project)
                            .map(|m| m.approval_mode)
                            .unwrap_or_default();
                        let mut metadata = ProjectTabMetadata::from_session(agent.session());
                        metadata.approval_mode = mode;
                        state.set_active_project_metadata(metadata);
                        reload_state_from_agent(state, &agent);
                        state.refresh_transcript_status(&agent);
                        inputs.remember_owner(&job.project, &agent);
                    }
                    state.set_status("New session ready");
                }
                JobOutcome::Failed(error) => {
                    state.restore_submitted_input(job.submitted);
                    state.push_message(MessageRole::Error, error.to_string());
                    state.set_status("Request failed");
                }
                JobOutcome::Cancelled => {
                    state.restore_submitted_input(job.submitted);
                    state.set_status("Interrupted");
                }
                JobOutcome::Panicked => {
                    state.restore_submitted_input(job.submitted);
                    state.push_message(MessageRole::Error, "New session worker panicked; reopen the project to recover its durable owner");
                    state.set_status("Request failed");
                }
            }
        }
        if let Some(index) = state.project_index(&visible) {
            state.select_project_tab(index);
        }
        true
    }

    fn poll(
        &mut self,
        state: &mut TuiState,
        host: &mut ProjectRuntimeHost,
        inputs: &mut TuiInputControls,
    ) -> bool {
        let mut changed = false;
        while let Ok(event) = self.worker.try_next_event() {
            changed |= self.apply_event(event, state, host, inputs);
        }
        let settled = self
            .active
            .as_ref()
            .filter(|job| {
                job.detached
                    && job
                        .result
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .is_some()
            })
            .map(|job| job.id);
        if let Some(id) = settled {
            changed |= self.apply_event(
                crate::runtime::RuntimeEvent::Completed {
                    id,
                    outcome: crate::runtime::JobOutcome::Succeeded(()),
                },
                state,
                host,
                inputs,
            );
        }
        changed
    }

    fn shutdown(
        mut self,
        state: &mut TuiState,
        host: &mut ProjectRuntimeHost,
        inputs: &mut TuiInputControls,
    ) -> std::thread::Result<()> {
        let mut requested = false;
        loop {
            if !requested {
                match self.worker.try_shutdown() {
                    Ok(()) => requested = true,
                    Err(crate::runtime::SubmitError::Closed) => break,
                    Err(crate::runtime::SubmitError::QueueFull) => {}
                }
            }
            match self.worker.recv_timeout(Duration::from_millis(10)) {
                Ok(crate::runtime::RuntimeEvent::Closed) => break,
                Ok(event) => {
                    self.apply_event(event, state, host, inputs);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        self.poll(state, host, inputs);
        self.worker.join()
    }
}

const MAX_TUI_PENDING_INPUTS: usize = 16;
const MAX_TUI_PENDING_INPUT_BYTES: usize = 256 * 1024;

fn queued_request_text(kind: TuiRequestKind, text: String) -> String {
    // Old checkpoints and explicit /scheduled edits may contain only the shell
    // command. New submissions keep the exact original input as the FIFO owner.
    if kind == TuiRequestKind::UserShell && !text.trim_start().starts_with('!') {
        format!("!{text}")
    } else {
        text
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TuiScheduledInput {
    id: String,
    revision: u64,
    project: String,
    kind: TuiRequestKind,
    text: String,
    #[serde(default)]
    interrupted: bool,
}

/// Existing FIFO for future jobs, distinct from the Agent's in-turn input queue.
#[derive(Default)]
pub struct TuiPendingInputs {
    inputs: VecDeque<(TuiRequestKind, String)>,
    projects: VecDeque<String>,
    identities: VecDeque<(String, u64)>,
    paste_metadata: VecDeque<Option<SubmittedPaste>>,
    serial: u64,
    bytes: usize,
}
impl TuiPendingInputs {
    fn should_queue(&self, kind: TuiRequestKind, active: Option<TuiRequestKind>) -> bool {
        (kind != TuiRequestKind::Model && active.is_some())
            || active.is_some_and(|kind| kind != TuiRequestKind::Model)
            || !self.inputs.is_empty()
    }
    fn push(&mut self, kind: TuiRequestKind, text: String) -> Result<(), &'static str> {
        if text.trim().is_empty()
            || self.inputs.len() >= MAX_TUI_PENDING_INPUTS
            || self.bytes.saturating_add(text.len()) > MAX_TUI_PENDING_INPUT_BYTES
        {
            return Err("Input queue full or empty input; input was not submitted");
        }
        self.serial = self.serial.saturating_add(1);
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        self.identities
            .push_back((format!("scheduled-{time}-{}", self.serial), 0));
        self.bytes += text.len();
        self.inputs.push_back((kind, text));
        self.paste_metadata.push_back(None);
        self.projects.push_back(String::new());
        Ok(())
    }
    fn pop(&mut self) -> Option<(TuiRequestKind, String, Option<SubmittedPaste>)> {
        let input = self.inputs.pop_front()?;
        self.projects.pop_front();
        self.identities.pop_front();
        self.bytes -= input.1.len();
        Some((input.0, input.1, self.paste_metadata.pop_front().unwrap()))
    }
    fn sync_projection(&self, state: &mut TuiState) {
        state
            .scheduled_inputs
            .retain(|entry| entry.interrupted && state.project_tabs.contains(&entry.project));
        for ((kind, text), (project, (id, revision))) in self
            .inputs
            .iter()
            .zip(self.projects.iter().zip(&self.identities))
        {
            state.scheduled_inputs.push(TuiScheduledInput {
                id: id.clone(),
                revision: *revision,
                project: project.clone(),
                kind: *kind,
                text: text.clone(),
                interrupted: false,
            });
        }
        state.palette_rows = 0;
        state.palette_index = 0;
        state.dirty = true;
        state.project_checkpoint_dirty = true;
    }
    fn remove(&mut self, index: usize) {
        if let Some((_, text)) = self.inputs.remove(index) {
            self.bytes -= text.len();
        }
        self.projects.remove(index);
        self.identities.remove(index);
        self.paste_metadata.remove(index);
    }
    fn cancel(&mut self, state: &mut TuiState) {
        let mut count = 0;
        for index in (0..self.inputs.len()).rev() {
            if self.projects[index].is_empty() || self.projects[index] == state.active_project() {
                self.remove(index);
                count += 1;
            }
        }
        self.sync_projection(state);
        if count != 0 {
            state.push_message(
                MessageRole::System,
                format!("Cancelled {count} queued input(s)"),
            );
        }
    }
    pub fn admit_prompt(&mut self, text: String, state: &mut TuiState) {
        self.admit(TuiRequestKind::Model, text, state);
    }
    fn admit(&mut self, kind: TuiRequestKind, text: String, state: &mut TuiState) {
        let rejected = text.clone();
        self.admit_original(kind, text, rejected, state);
    }

    fn admit_original(
        &mut self,
        kind: TuiRequestKind,
        text: String,
        rejected: String,
        state: &mut TuiState,
    ) {
        let paste = state.take_submitted_paste(&rejected);
        // Keep one canonical payload in the existing bounded FIFO. Discarding
        // whitespace here loses both the rejected text and its fold identity
        // when the accepted job later executes, fails, is edited or cancelled.
        let text = if kind == TuiRequestKind::UserShell {
            rejected.clone()
        } else {
            text
        };
        let total_bytes = state.scheduled_inputs.iter().fold(0usize, |bytes, entry| {
            bytes.saturating_add(entry.text.len())
        });
        let result = if state.scheduled_inputs.len() >= MAX_TUI_PENDING_INPUTS {
            Err("Scheduled queue full; resolve interrupted entries first")
        } else if total_bytes.saturating_add(text.len()) > MAX_TUI_PENDING_INPUT_BYTES {
            Err("Scheduled queue byte limit exceeded; resolve interrupted entries first")
        } else {
            self.push(kind, text)
        };
        match result {
            Ok(()) => {
                *self.paste_metadata.back_mut().unwrap() = paste;
                *self.projects.back_mut().unwrap() = state.active_project().to_owned();
                self.sync_projection(state);
                let (id, revision) = self.identities.back().unwrap();
                state.push_message(MessageRole::System, format!("Input queued ({}) · scheduled {id} revision {revision} · project {} · /scheduled list", self.inputs.len(), state.active_project()));
            }
            Err(error) => {
                state.push_message(MessageRole::Error, error);
                state.restore_submitted_input(SubmittedInput {
                    text: rejected,
                    paste,
                });
            }
        }
    }
    /// Manage only the existing unstarted FIFO; accepted Agent inputs use /input.
    pub fn scheduled_command(&mut self, state: &mut TuiState, input: &str) -> bool {
        let (name, rest) = input.split_once(char::is_whitespace).unwrap_or((input, ""));
        if name != "/scheduled" {
            return false;
        }
        let rest = rest.trim_start();
        let (verb, rest) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let result: Result<(), String> = (|| {
            if verb.is_empty() || verb == "list" {
                if !rest.trim().is_empty() {
                    return Err("Use /scheduled list".into());
                }
                let entries = state
                    .scheduled_inputs
                    .iter()
                    .filter(|entry| entry.project == state.active_project())
                    .map(|entry| {
                        let mut value = serde_json::to_value(entry).unwrap_or_default();
                        value["status"] = serde_json::json!(if entry.interrupted {
                            "interrupted_unconfirmed"
                        } else {
                            "scheduled"
                        });
                        value
                    })
                    .collect::<Vec<_>>();
                state.push_message(MessageRole::System, format!("Scheduled jobs (interrupted=true means execution unconfirmed; inspect session before retry):\n{}", bounded_display(&serde_json::to_string_pretty(&entries).unwrap_or_default())));
                return Ok(());
            }
            let (id, tail) = rest
                .trim_start()
                .split_once(char::is_whitespace)
                .unwrap_or((rest.trim(), ""));
            let entry = state
                .scheduled_inputs
                .iter()
                .find(|entry| entry.id == id)
                .cloned()
                .ok_or("Scheduled job missing or already started")?;
            if entry.project != state.active_project() {
                return Err("Scheduled job belongs to another project".into());
            }
            let index = self
                .identities
                .iter()
                .position(|(candidate, _)| candidate == id);
            match verb {
                "cancel" if tail.trim().is_empty() => {
                    if let Some(index) = index {
                        self.remove(index);
                    }
                    state.scheduled_inputs.retain(|entry| entry.id != id);
                    self.sync_projection(state);
                    state.push_message(
                        MessageRole::System,
                        format!("Scheduled job cancelled: {id}"),
                    );
                }
                "edit" => {
                    let (revision, text) = tail
                        .trim_start()
                        .split_once(char::is_whitespace)
                        .ok_or("Use /scheduled edit ID REVISION TEXT")?;
                    let revision: u64 = revision
                        .parse()
                        .map_err(|_| "Revision must be an integer")?;
                    if revision != entry.revision {
                        return Err("Scheduled job revision is stale".into());
                    }
                    if text.trim().is_empty() || text.len() > MAX_TUI_PENDING_INPUT_BYTES {
                        return Err("Scheduled text is empty or exceeds256KiB".into());
                    }
                    if entry.kind == TuiRequestKind::ResourceControl
                        && crate::slash::resource_control(text).is_none()
                        && text.trim() != "/compact"
                    {
                        return Err(
                            "Scheduled resource control must remain /skills, /templates, /reload or /compact"
                                .into(),
                        );
                    }
                    let total_bytes = state
                        .scheduled_inputs
                        .iter()
                        .fold(0usize, |bytes, row| bytes.saturating_add(row.text.len()));
                    if total_bytes
                        .saturating_sub(entry.text.len())
                        .saturating_add(text.len())
                        > MAX_TUI_PENDING_INPUT_BYTES
                    {
                        return Err("Scheduled queue byte limit exceeded".into());
                    }
                    let revision = revision
                        .checked_add(1)
                        .ok_or("Scheduled revision exhausted")?;
                    if let Some(index) = index {
                        let bytes = self.bytes - self.inputs[index].1.len() + text.len();
                        if bytes > MAX_TUI_PENDING_INPUT_BYTES {
                            return Err("Scheduled queue byte limit exceeded".into());
                        }
                        self.bytes = bytes;
                        self.paste_metadata[index] = None;
                        self.inputs[index].1 = text.to_owned();
                        self.identities[index].1 = revision;
                    } else if entry.interrupted {
                        let total = state
                            .scheduled_inputs
                            .iter()
                            .map(|entry| entry.text.len())
                            .sum::<usize>()
                            - entry.text.len()
                            + text.len();
                        if total > MAX_TUI_PENDING_INPUT_BYTES {
                            return Err("Scheduled recovery byte limit exceeded".into());
                        }
                        let entry = state
                            .scheduled_inputs
                            .iter_mut()
                            .find(|entry| entry.id == id)
                            .unwrap();
                        entry.text = text.to_owned();
                        entry.revision = revision;
                    } else {
                        return Err("Scheduled job already started".into());
                    }
                    self.sync_projection(state);
                    state.push_message(
                        MessageRole::System,
                        format!("Scheduled job edited: {id} revision {revision}"),
                    );
                }
                "retry" if tail.trim().is_empty() && entry.interrupted => {
                    self.push(entry.kind, entry.text)?;
                    *self.projects.back_mut().unwrap() = entry.project;
                    state.scheduled_inputs.retain(|entry| entry.id != id);
                    self.sync_projection(state);
                    state.push_message(
                        MessageRole::System,
                        "Interrupted scheduled job explicitly queued for retry",
                    );
                }
                _ => {
                    return Err(
                        "Use /scheduled list|edit ID REVISION TEXT|cancel ID|retry ID".into(),
                    );
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            state.push_message(MessageRole::Error, error);
            state.set_rejected_input(input);
        } else {
            state.take_submitted_paste(input);
        }
        true
    }
}

fn render_user_shell_result(state: &mut TuiState, result: &serde_json::Value) {
    if let Some(help) = result.get("help").and_then(serde_json::Value::as_str) {
        state.push_message(MessageRole::System, help);
        state.set_status("Ready");
        return;
    }
    let cancelled = result.get("cancelled").and_then(serde_json::Value::as_bool) == Some(true);
    let timed_out = result.get("timed_out").and_then(serde_json::Value::as_bool) == Some(true);
    let exit_code = result.get("exit_code").and_then(serde_json::Value::as_i64);
    let signal = result.get("signal").and_then(serde_json::Value::as_i64);
    let status = if cancelled {
        "Local shell cancelled".to_owned()
    } else if timed_out {
        "Local shell timed out".to_owned()
    } else if let Some(signal) = signal {
        format!("Local shell terminated by signal {signal}")
    } else {
        format!(
            "Local shell exit {}",
            exit_code.map_or_else(|| "unknown".into(), |code| code.to_string())
        )
    };
    let mut output = status.clone();
    for key in ["stdout", "stderr"] {
        if let Some(text) = result.get(key).and_then(serde_json::Value::as_str)
            && !text.is_empty()
        {
            output.push_str(&format!(
                "\n{key}:\n{}",
                crate::security::redact_text(text, &[])
            ));
        }
    }
    if result.get("truncated").and_then(serde_json::Value::as_bool) == Some(true)
        || result
            .get("stdout_truncated")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        || result
            .get("stderr_truncated")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    {
        output.push_str("\nLocal shell output truncated");
    }
    state.terminal_snapshot = crate::security::redact_text(&output, &[]);
    state.terminal_snapshot = bound_text(state.terminal_snapshot.clone());
    state.push_message(
        if cancelled || timed_out || signal.is_some() || exit_code != Some(0) {
            MessageRole::Error
        } else {
            MessageRole::System
        },
        output,
    );
    state.set_status(status);
}

/// Provider deltas are produced on the worker thread while the terminal loop
/// may be busy rendering or waiting for input. Keep that mailbox bounded, but
/// retain a loss counter so backpressure never turns into a silent transcript
/// gap. The counter is drained together with the queue under one lock, which
/// gives the host a consistent snapshot for each warning it renders.
#[derive(Debug)]
enum TuiStreamEvent {
    Provider(crate::backend::ProviderEvent),
    Agent(crate::core::AgentEvent),
}

#[derive(Debug, Default)]
struct TuiProviderEventBuffer {
    events: VecDeque<TuiStreamEvent>,
    bytes: usize,
    dropped: u64,
}

impl TuiProviderEventBuffer {
    /// Enqueue an event using the production byte budget. The count argument
    /// remains explicit because focused tests and embedders may use a smaller
    /// mailbox while preserving the same aggregate-byte invariant.
    fn push(&mut self, event: crate::backend::ProviderEvent, limit: usize) {
        self.push_with_limits(event, limit, MAX_TUI_PROVIDER_BYTES);
    }

    fn push_with_limits(
        &mut self,
        event: crate::backend::ProviderEvent,
        max_events: usize,
        max_bytes: usize,
    ) {
        let event_bytes = serialized_provider_event_bytes(&event);
        self.push_stream(
            TuiStreamEvent::Provider(event),
            event_bytes,
            max_events,
            max_bytes,
        );
    }

    fn push_agent(&mut self, event: crate::core::AgentEvent) {
        let event_bytes = serialized_provider_event_bytes(&event);
        self.push_stream(
            TuiStreamEvent::Agent(event),
            event_bytes,
            MAX_TUI_PROVIDER_EVENTS,
            MAX_TUI_PROVIDER_BYTES,
        );
    }

    fn push_stream(
        &mut self,
        event: TuiStreamEvent,
        event_bytes: usize,
        max_events: usize,
        max_bytes: usize,
    ) {
        let priority = |event: &TuiStreamEvent| {
            matches!(
                event,
                TuiStreamEvent::Agent(
                    crate::core::AgentEvent::ToolCall { .. }
                        | crate::core::AgentEvent::ToolResult { .. }
                )
            )
        };
        if priority(&event) && event_bytes <= max_bytes && max_events > 0 {
            while self.events.len() >= max_events
                || self.bytes.saturating_add(event_bytes) > max_bytes
            {
                let Some(index) = self.events.iter().position(|event| !priority(event)) else {
                    break;
                };
                let evicted = self.events.remove(index).expect("located event");
                let bytes = match evicted {
                    TuiStreamEvent::Agent(event) => serialized_provider_event_bytes(&event),
                    TuiStreamEvent::Provider(event) => serialized_provider_event_bytes(&event),
                };
                self.bytes = self.bytes.saturating_sub(bytes);
                self.dropped = self.dropped.saturating_add(1);
            }
        }
        let within_count = max_events > 0 && self.events.len() < max_events;
        let within_bytes = self
            .bytes
            .checked_add(event_bytes)
            .is_some_and(|bytes| bytes <= max_bytes);
        if within_count && within_bytes {
            self.bytes = self.bytes.saturating_add(event_bytes);
            self.events.push_back(event);
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    fn take(&mut self) -> (VecDeque<TuiStreamEvent>, u64) {
        self.bytes = 0;
        (
            std::mem::take(&mut self.events),
            std::mem::take(&mut self.dropped),
        )
    }
}

/// Count the encoded event without allocating a second copy of its payload.
/// JSON serialization is also the unit used by the headless mailbox, so a
/// multi-byte UTF-8 delta and escaped control characters consume the same
/// deterministic budget in both transports. ProviderEvent's derived
/// serializer is infallible for its current fields; an unexpected failure is
/// treated as an over-limit event rather than retaining unaccounted data.
fn serialized_provider_event_bytes(event: &impl Serialize) -> usize {
    struct ByteCounter(usize);

    impl io::Write for ByteCounter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let mut counter = ByteCounter(0);
    serde_json::to_writer(&mut counter, event).map_or(usize::MAX, |()| counter.0)
}

/// Optional bridge between the production TUI and the bounded config-owned
/// layout snapshot. Discovery and restore are best-effort: a malformed or
/// stale preference file must not prevent the agent from starting with safe
/// built-in presets. Once a write fails, persistence is disabled for this
/// process so a broken path cannot spam the transcript on every keypress.
#[derive(Debug, Default)]
struct TuiLayoutPersistence {
    paths: Option<crate::config::ConfigPaths>,
    profile: Option<String>,
    disabled: bool,
}

impl TuiLayoutPersistence {
    fn discover(requested_profile: Option<String>) -> Self {
        let Ok(paths) = crate::config::ConfigPaths::discover() else {
            return Self::default();
        };
        Self {
            profile: requested_profile.or_else(|| active_layout_profile(&paths)),
            paths: Some(paths),
            disabled: false,
        }
    }

    fn restore(&self, state: &mut TuiState) -> Result<(), String> {
        let Some(paths) = self.paths.as_ref() else {
            return Ok(());
        };
        let preferences =
            crate::config::load_layout_preferences(paths).map_err(|error| error.to_string())?;
        let layouts = [crate::layout::TabId::Project]
            .into_iter()
            .map(|tab| {
                preferences
                    .model_for(self.profile.as_deref(), tab)
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        state.restore_workspace_layouts(layouts);
        let project_path = paths.project_tabs_path();
        if project_path.try_exists().map_err(|e| e.to_string())? {
            use std::io::Read;
            let mut options = std::fs::OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW);
            }
            let file = options.open(&project_path).map_err(|e| e.to_string())?;
            if !file.metadata().map_err(|e| e.to_string())?.is_file() {
                return Err("project checkpoint must be a regular file".into());
            }
            let mut bytes = Vec::new();
            file.take((MAX_PROJECT_CHECKPOINT_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > MAX_PROJECT_CHECKPOINT_BYTES {
                return Err("project checkpoint too large".into());
            }
            let value = serde_json::from_slice(&bytes)
                .map_err(|e| format!("invalid project checkpoint: {e}"))?;
            if !state.restore_project_checkpoint(&value) {
                return Err("invalid project checkpoint".into());
            }
        }
        Ok(())
    }

    fn save(&mut self, state: &TuiState) -> Result<(), String> {
        if self.disabled {
            return Ok(());
        }
        let Some(paths) = self.paths.as_ref() else {
            return Ok(());
        };
        let mut preferences =
            crate::config::load_layout_preferences(paths).map_err(|error| error.to_string())?;
        let mut changed = false;
        let reset_tabs = state.layout_reset_tabs();
        for tab in reset_tabs.iter().copied() {
            changed |= preferences
                .reset_tab(self.profile.as_deref(), tab)
                .map_err(|error| error.to_string())?;
        }
        for layout in [state.active_project_workspace().clone()] {
            if reset_tabs.contains(&layout.tab) {
                continue;
            }
            changed |= preferences
                .set_model(self.profile.as_deref(), &layout)
                .map_err(|error| error.to_string())?;
        }
        if changed {
            crate::config::save_layout_preferences(paths, &preferences)
                .map_err(|error| error.to_string())?;
        }
        let project_path = paths.project_tabs_path();
        let bytes = serde_json::to_vec_pretty(&state.project_checkpoint())
            .map_err(|error| error.to_string())?;
        atomic_project_checkpoint_write(&project_path, &bytes)?;
        Ok(())
    }

    fn save_project_checkpoint(&self, state: &TuiState) -> Result<(), String> {
        let Some(paths) = self.paths.as_ref() else {
            return Ok(());
        };
        let bytes = serde_json::to_vec_pretty(&state.project_checkpoint())
            .map_err(|error| error.to_string())?;
        atomic_project_checkpoint_write(&paths.project_tabs_path(), &bytes)
    }

    fn disable(&mut self) {
        self.disabled = true;
    }
}

fn atomic_project_checkpoint_write(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_PROJECT_CHECKPOINT_BYTES {
        return Err(format!(
            "project checkpoint exceeds {} bytes",
            MAX_PROJECT_CHECKPOINT_BYTES
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    crate::config::atomic_write_if_changed(path, bytes)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn active_layout_profile(paths: &crate::config::ConfigPaths) -> Option<String> {
    let candidate = std::env::var("ZENPI_PROFILE").ok().or_else(|| {
        crate::config::load_config(paths)
            .ok()
            .and_then(|config| config.default_profile)
    })?;
    let valid = !candidate.is_empty()
        && candidate.len() <= 128
        && candidate
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'));
    valid.then_some(candidate)
}

fn command_refreshes_gantt(command: &SlashCommand) -> bool {
    matches!(
        command,
        SlashCommand::Goal { .. } | SlashCommand::GoalPut { .. } | SlashCommand::Blueprint { .. }
    )
}

/// Run the production TUI with provider work on the bounded runtime worker.
/// The older callback-based `run_with_state` remains available for embedders
/// and deterministic tests; the binary uses this owned form so a worker can
/// safely hold the agent for the duration of one request.
pub fn run_async(agent: crate::core::Agent) -> Result<(), crate::error::ZenpiError> {
    run_async_with_profile(agent, None)
}

/// Run the production TUI while keeping the provider/profile selected by the
/// command line aligned with the profile that owns persisted BentoBox state.
/// The optional value is already validated by the CLI; embedders may omit it
/// and use the configured default profile.
pub fn run_async_with_profile(
    agent: crate::core::Agent,
    profile: Option<String>,
) -> Result<(), crate::error::ZenpiError> {
    use crate::core::AgentError;
    use crate::runtime::{BackgroundRunner, JobOutcome, RuntimeConfig, RuntimeEvent};

    let mut local_diff_host = LocalDiffHost::new()?;
    let mut session_browser_host = SessionBrowserHost::new()?;
    let mut editor = ExternalEditorHost::capture_startup();
    let mut shared = Arc::new(Mutex::new(agent));
    let mut approval = shared
        .lock()
        .ok()
        .and_then(|agent| agent.approval_coordinator());
    let worker_state = Arc::clone(&shared);
    // Keep provider events bounded independently of the transcript. A slow
    // terminal must not turn an unbounded stream into unbounded memory.
    let runner = BackgroundRunner::spawn(
        move |request: TuiRequest, token| -> Result<TuiJobResult, AgentError> {
            if token.is_cancelled() {
                return Err(AgentError::Backend(crate::backend::BackendError::Cancelled));
            }
            let mut agent = request
                .owner
                .as_ref()
                .unwrap_or(&worker_state)
                .lock()
                .map_err(|_| AgentError::InvalidTurn("agent lock poisoned".into()))?;
            let tool_events = Arc::clone(&request.provider_events);
            let tool_sink = Arc::new(move |event| {
                if let Ok(mut pending) = tool_events.lock() {
                    pending.push_agent(event);
                }
            });
            let result = agent.with_live_tool_events(
                tool_sink,
                |agent| -> Result<TuiJobResult, AgentError> {
                    Ok(match request.kind {
                        TuiRequestKind::Model => {
                            let mut attachments = Vec::new();
                            if !request.text.starts_with('/') {
                                for (_, _, path, closed) in prompt_file_references(&request.text)
                                    .map_err(AgentError::InvalidTurn)?
                                {
                                    if !closed || path.is_empty() {
                                        return Err(AgentError::InvalidTurn(
                                            "Incomplete file reference".into(),
                                        ));
                                    }
                                    attachments.push(
                                        crate::slash_actions::attachment_for_path(
                                            agent.attachment_workspace_root().ok_or_else(|| {
                                                AgentError::InvalidTurn(
                                                    "File reference requires project cwd".into(),
                                                )
                                            })?,
                                            &path,
                                        )
                                        .map_err(|e| AgentError::InvalidTurn(e.to_string()))?,
                                    );
                                }
                            }
                            TuiJobResult::Model(
                                agent.process_with_cancel_and_events(
                                    crate::core::TurnInputRequest::new(request.text)
                                        .with_attachments(attachments),
                                    || token.is_cancelled(),
                                    &mut |event| {
                                        if let Ok(mut pending) = request.provider_events.lock() {
                                            pending.push(event, MAX_TUI_PROVIDER_EVENTS);
                                        }
                                        Ok(())
                                    },
                                )?,
                            )
                        }
                        TuiRequestKind::ResourceControl => {
                            let value = if let Some(value) =
                                crate::headless::external_evidence_input(
                                    agent,
                                    &request.text,
                                    &|| token.is_cancelled(),
                                )? {
                                value
                            } else {
                                crate::slash_actions::resource_control_value(
                                    agent,
                                    &request.text,
                                    || token.is_cancelled(),
                                )?
                                .ok_or_else(|| {
                                    AgentError::InvalidTurn("unknown resource control".into())
                                })?
                            };
                            TuiJobResult::ResourceControl(value)
                        }
                        TuiRequestKind::UserShell => {
                            let command = request.text;
                            let result = agent
                                .run_user_shell_with_cancel(&command, || token.is_cancelled())?;
                            TuiJobResult::UserShell(result)
                        }
                    })
                },
            )?;
            // The core has crossed its durable completion boundary when it
            // returns a successful ProcessResult. Mark it before returning to
            // the runtime so a late shutdown/cancel cannot report a persisted
            // assistant answer as an interruption.
            token.mark_completed();
            Ok(result)
        },
        RuntimeConfig::default(),
    );
    // Workspace accounting can touch thousands of directory entries, so it
    // has a separate single-slot worker. This keeps startup and `/resources`
    // refreshes off the terminal thread without adding an async runtime.
    let completion_runner = BackgroundRunner::spawn(
        |query: FileCompletionQuery,
         token|
         -> Result<(FileCompletionQuery, FileCompletionResult), String> {
            let entries = complete_file_query(&query, || token.is_cancelled())?;
            token.mark_completed();
            Ok((query, entries))
        },
        RuntimeConfig::default(),
    );
    let resource_runner = BackgroundRunner::spawn(
        |(root, path): (std::path::PathBuf, Option<String>),
         token|
         -> Result<crate::resources::ResourceSnapshot, String> {
            if token.is_cancelled() {
                return Err("resource refresh cancelled".into());
            }
            let root = root.canonicalize().map_err(|e| e.to_string())?;
            let selected = path
                .as_deref()
                .map(|p| root.join(p))
                .unwrap_or_else(|| root.clone())
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if !selected.starts_with(&root) {
                return Err("Resource path escapes the project".into());
            }
            let snapshot = crate::resources::ResourceCollector::new(selected)
                .and_then(|collector| collector.collect())
                .map_err(|e| e.to_string())?;
            if token.is_cancelled() {
                return Err("resource refresh cancelled".into());
            }
            token.mark_completed();
            Ok(snapshot)
        },
        RuntimeConfig {
            command_capacity: 1,
            event_capacity: 8,
            max_pending: 1,
            poll_interval: Duration::from_millis(10),
        },
    );
    // The Gantt projection owns a separate single-slot worker. Domain snapshot
    // validation is bounded but still performs filesystem I/O, so it must not
    // run on the terminal thread or wait for the provider-owned agent mutex.
    let gantt_session_path = shared
        .lock()
        .map_err(|_| crate::error::ZenpiError::Message("agent lock poisoned".into()))?
        .session()
        .path()
        .to_path_buf();
    let mut gantt_tracker = GanttRefreshTracker::new(gantt_session_path);
    let gantt_runner = BackgroundRunner::spawn(
        |request: GanttRefreshRequest, token| -> Result<GanttRefreshResult, String> {
            if token.is_cancelled() {
                return Err("Gantt refresh cancelled".into());
            }
            let snapshot = collect_gantt_snapshot(request.session_path)?;
            if token.is_cancelled() {
                return Err("Gantt refresh cancelled".into());
            }
            token.mark_completed();
            Ok(GanttRefreshResult {
                snapshot,
                generation: request.generation,
            })
        },
        RuntimeConfig {
            command_capacity: 1,
            event_capacity: 8,
            max_pending: 1,
            poll_interval: Duration::from_millis(10),
        },
    );
    let mut state = TuiState {
        async_session_browser: true,
        ..TuiState::default()
    };
    if let Ok(agent) = shared.lock() {
        state.set_active_project_metadata(ProjectTabMetadata::from_session(agent.session()));
        state.persona = agent.persona().into();
        let _ = state.update_model_catalog(&agent);
        reload_state_from_agent(&mut state, &agent);
        state.refresh_session_snapshot(agent.session());
    }
    let mut layout_persistence = TuiLayoutPersistence::discover(profile);
    if let Err(error) = layout_persistence.restore(&mut state) {
        // Keep startup usable with a safe preset, but make a corrupt or stale
        // preference visible instead of silently discarding the user's file.
        state.push_message(
            MessageRole::Error,
            format!("layout preferences ignored; using presets: {error}"),
        );
        layout_persistence.disable();
    }
    let mut project_host = ProjectRuntimeHost::new(&mut state, Arc::clone(&shared))
        .map_err(crate::error::ZenpiError::Message)?;
    if let Err(error) = project_host.restore_shared_workspace(&mut state) {
        state.push_message(
            MessageRole::Error,
            format!("Shared project workspace could not be restored: {error}"),
        );
    }
    shared = project_host.active(&state);
    if let Ok(agent) = shared.lock() {
        gantt_tracker.switch_session(agent.session().path().to_path_buf());
    }
    let mut active_resource_job = match resource_runner.try_submit((state.project_cwd(), None)) {
        Ok(id) => {
            state.resource_refresh_started();
            Some(id)
        }
        Err(error) => {
            state.resource_refresh_failed(error.to_string());
            None
        }
    };
    let mut active_resource_project = state.active_project().to_owned();
    let mut last_resource_refresh = Instant::now();
    let mut active_gantt_generation = Some(gantt_tracker.generation);
    let mut active_gantt_job = match gantt_runner.try_submit(gantt_tracker.request()) {
        Ok(id) => {
            state.gantt_refresh_started();
            Some(id)
        }
        Err(error) => {
            active_gantt_generation = None;
            state.gantt_refresh_failed(error.to_string());
            None
        }
    };
    let mut gantt_refresh_pending = false;
    let config = TuiConfig::default();
    let mut new_sessions = NewSessionControl::new();
    // The runtime worker is spawned before terminal setup so it can own the
    // agent independently of the terminal.  Terminal setup can still fail
    // (for example when stdin is not a TTY), so every early return here must
    // close and join the worker instead of relying on `Drop` to detach it.
    let mut guard = match TerminalGuard::enter() {
        Ok(guard) => guard,
        Err(error) => {
            let _ = runner.shutdown_and_join();
            let _ = new_sessions.worker.shutdown_and_join();
            let _ = resource_runner.shutdown_and_join();
            let _ = completion_runner.shutdown_and_join();
            let _ = gantt_runner.shutdown_and_join();
            return Err(crate::error::ZenpiError::Message(error.to_string()));
        }
    };
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = runner.shutdown_and_join();
            let _ = new_sessions.worker.shutdown_and_join();
            let _ = resource_runner.shutdown_and_join();
            let _ = completion_runner.shutdown_and_join();
            let _ = gantt_runner.shutdown_and_join();
            guard.leave();
            return Err(crate::error::ZenpiError::Message(error.to_string()));
        }
    };
    let frame_interval = config.frame_interval.max(MIN_LOOP_INTERVAL);
    let poll_interval = config.poll_interval.max(MIN_LOOP_INTERVAL);
    let mut scheduler = RenderScheduler::new(frame_interval);
    let mut resize_pending = false;
    let mut last_tick = Instant::now();
    let mut last_file_query: Option<FileCompletionQuery> = None;
    let mut active_completion = None;
    let mut active_job = None;
    let mut active_job_kind = None;
    let mut active_job_project: Option<String> = None;
    let mut active_job_owner: Option<Arc<Mutex<crate::core::Agent>>> = None;
    let mut pending_inputs = TuiPendingInputs::default();
    let mut input_controls = TuiInputControls::default();
    if let Ok(agent) = shared.try_lock() {
        input_controls.remember_owner(state.active_project(), &agent);
    }
    let mut last_checkpoint = Instant::now() - Duration::from_millis(250);
    let mut submitted_inputs: HashMap<crate::runtime::JobId, SubmittedInput> = HashMap::new();
    let mut stream_buffers: HashMap<crate::runtime::JobId, Arc<Mutex<TuiProviderEventBuffer>>> =
        HashMap::new();
    let mut pending_approvals = VecDeque::new();
    let loop_result = (|| -> Result<(), crate::error::ZenpiError> {
        'outer: loop {
            if guard.termination_requested() {
                break;
            }
            if new_sessions.poll(&mut state, &mut project_host, &mut input_controls) {
                if let Some(path) = state
                    .project_metadata(state.active_project())
                    .and_then(|meta| meta.session_path.clone())
                    && gantt_tracker.switch_session(path.into())
                {
                    state.clear_gantt_snapshot();
                    gantt_refresh_pending = true;
                }
                scheduler.request();
            }
            if input_controls.poll(&mut state) {
                scheduler.request();
            }
            let desired = state.file_completion_query();
            if desired != last_file_query {
                if let Some(id) = active_completion.take() {
                    let _ = completion_runner.try_cancel(id);
                }
                state.file_completion = None;
                last_file_query = desired.clone();
                if let Some(query) = desired {
                    match completion_runner.try_submit(query) {
                        Ok(id) => active_completion = Some(id),
                        Err(_) => {
                            last_file_query = None;
                        }
                    }
                }
            }
            while let Ok(event) = completion_runner.try_next_event() {
                if let RuntimeEvent::Completed { id, outcome } = event
                    && active_completion == Some(id)
                {
                    active_completion = None;
                    match outcome {
                        JobOutcome::Succeeded((query, entries)) => {
                            state.apply_file_completion(query, entries);
                        }
                        JobOutcome::Failed(error) => {
                            if state.file_completion_query() == last_file_query {
                                state.set_status(format!("File completion: {error}"));
                            }
                        }
                        _ => {}
                    }
                    scheduler.request();
                }
            }
            if let Some(intent) = state.take_project_intent() {
                if let ProjectIntent::Close(key) = &intent
                    && (active_job_project.as_ref() == Some(key)
                        || new_sessions.owns(key)
                        || pending_inputs.projects.contains(key)
                        || input_controls.has_pending(key))
                {
                    state.push_message(
                        MessageRole::Error,
                        "Project has running or queued work; wait or cancel it before closing.",
                    );
                    continue;
                }
                match project_host.apply(&mut state, intent) {
                    Ok(()) => {
                        shared = project_host.active(&state);
                        state.resource_snapshot = None;
                        last_resource_refresh = Instant::now() - RESOURCE_REFRESH_INTERVAL;
                        let path = state
                            .project_metadata(state.active_project())
                            .and_then(|m| m.session_path.clone());
                        if let Some(path) = path
                            && gantt_tracker.switch_session(path.into())
                        {
                            state.clear_gantt_snapshot();
                            gantt_refresh_pending = true;
                        }
                    }
                    Err(error) => state
                        .push_message(MessageRole::Error, format!("Project open failed: {error}")),
                }
                scheduler.request();
            }
            // The worker holds the agent mutex while provider I/O is in flight;
            // use a non-blocking drain so lifecycle events become visible as soon
            // as that mutex is released without freezing keyboard/render polling.
            drain_agent_tool_events(&shared, &mut state);
            if local_diff_host.poll(&mut state) {
                scheduler.request();
            }
            if session_browser_host.poll(&mut state) {
                scheduler.request();
            }
            if active_gantt_job.is_none() && gantt_refresh_pending {
                match gantt_runner.try_submit(gantt_tracker.request()) {
                    Ok(id) => {
                        active_gantt_job = Some(id);
                        active_gantt_generation = Some(gantt_tracker.generation);
                        gantt_refresh_pending = false;
                        state.gantt_refresh_started();
                    }
                    Err(crate::runtime::SubmitError::QueueFull) => {}
                    Err(crate::runtime::SubmitError::Closed) => {
                        gantt_refresh_pending = false;
                        state.gantt_refresh_failed("Gantt worker closed");
                    }
                }
            }
            while let Ok(event) = resource_runner.try_next_event() {
                match event {
                    RuntimeEvent::Completed { id, outcome } => {
                        if Some(id) != active_resource_job {
                            continue;
                        }
                        active_resource_job = None;
                        if active_resource_project != state.active_project() {
                            continue;
                        }
                        match outcome {
                            JobOutcome::Succeeded(snapshot)
                                if active_resource_project == state.active_project() =>
                            {
                                state.set_resource_snapshot(snapshot);
                            }
                            JobOutcome::Succeeded(_) => {}
                            JobOutcome::Failed(error) => state.resource_refresh_failed(error),
                            JobOutcome::Cancelled => {
                                state.resource_refresh_failed("resource refresh cancelled")
                            }
                            JobOutcome::Panicked => {
                                state.resource_refresh_failed("resource worker panicked")
                            }
                        }
                        last_resource_refresh = Instant::now();
                        scheduler.request();
                    }
                    RuntimeEvent::Rejected { id, reason } => {
                        if Some(id) != active_resource_job {
                            continue;
                        }
                        active_resource_job = None;
                        if active_resource_project != state.active_project() {
                            continue;
                        }
                        last_resource_refresh = Instant::now();
                        state.resource_refresh_failed(reason.to_string());
                        scheduler.request();
                    }
                    RuntimeEvent::Closed => {
                        state.resource_refresh_failed("resource worker closed");
                        scheduler.request();
                    }
                    _ => {}
                }
            }
            let now = Instant::now();
            if active_resource_job.is_none()
                && now.saturating_duration_since(last_resource_refresh) >= RESOURCE_REFRESH_INTERVAL
            {
                match resource_runner.try_submit((state.project_cwd(), None)) {
                    Ok(id) => {
                        active_resource_job = Some(id);
                        active_resource_project = state.active_project().to_owned();
                        last_resource_refresh = now;
                        state.resource_refresh_started();
                        scheduler.request();
                    }
                    Err(crate::runtime::SubmitError::QueueFull) => {}
                    Err(crate::runtime::SubmitError::Closed) => {
                        state.resource_refresh_failed("resource worker closed");
                        last_resource_refresh = now;
                        scheduler.request();
                    }
                }
            }
            while let Ok(event) = gantt_runner.try_next_event() {
                match event {
                    RuntimeEvent::Completed { id, outcome } => {
                        if Some(id) != active_gantt_job {
                            continue;
                        }
                        active_gantt_job = None;
                        let completed_generation = active_gantt_generation.take();
                        match outcome {
                            JobOutcome::Succeeded(result)
                                if completed_generation == Some(result.generation)
                                    && gantt_tracker.accepts(&result) =>
                            {
                                state.set_gantt_snapshot(result.snapshot);
                                state.push_goal_message(
                                    MessageRole::System,
                                    "Blueprint/Goal snapshot refreshed",
                                );
                            }
                            JobOutcome::Succeeded(_) => {}
                            JobOutcome::Failed(error)
                                if completed_generation == Some(gantt_tracker.generation) =>
                            {
                                state.gantt_refresh_failed(&error);
                                state.push_goal_message(
                                    MessageRole::Error,
                                    format!("Goal refresh failed: {error}"),
                                );
                            }
                            JobOutcome::Failed(_) => {}
                            JobOutcome::Cancelled => {
                                if completed_generation == Some(gantt_tracker.generation) {
                                    state.gantt_refresh_failed("Gantt refresh cancelled");
                                    state.push_goal_message(
                                        MessageRole::System,
                                        "Goal refresh cancelled",
                                    );
                                }
                            }
                            JobOutcome::Panicked => {
                                if completed_generation == Some(gantt_tracker.generation) {
                                    state.gantt_refresh_failed("Gantt worker panicked");
                                    state.push_goal_message(
                                        MessageRole::Error,
                                        "Goal refresh worker panicked",
                                    );
                                }
                            }
                        }
                        scheduler.request();
                    }
                    RuntimeEvent::Rejected { id, reason } => {
                        if Some(id) == active_gantt_job {
                            active_gantt_job = None;
                            active_gantt_generation = None;
                            if reason == crate::runtime::SubmitError::QueueFull {
                                gantt_refresh_pending = true;
                                state.gantt_refresh_started();
                            } else {
                                state.gantt_refresh_failed(reason.to_string());
                            }
                            scheduler.request();
                        }
                    }
                    RuntimeEvent::Closed => {
                        active_gantt_job = None;
                        active_gantt_generation = None;
                        gantt_refresh_pending = false;
                        state.gantt_refresh_failed("Gantt worker closed");
                        scheduler.request();
                    }
                    _ => {}
                }
            }
            let visible_project = state.active_project().to_owned();
            if let Some(index) = active_job_project
                .as_ref()
                .and_then(|key| state.project_index(key))
            {
                state.select_project_tab(index);
            }
            if let Some(id) = active_job {
                // Drop buffers belonging to superseded jobs before draining
                // the active one. This keeps a late cancelled stream from
                // interleaving with the replacement turn.
                stream_buffers.retain(|job_id, _| *job_id == id);
                if let Some(events) = stream_buffers.get(&id)
                    && drain_tui_provider_events(&mut state, id.get(), events)
                {
                    scheduler.request();
                }
            }
            if let Some(coordinator) = approval.as_ref() {
                let stale: Vec<_> = pending_approvals
                    .iter()
                    .filter(|r: &&crate::approval::ApprovalRequest| {
                        !coordinator.is_pending(&r.request_id)
                    })
                    .cloned()
                    .collect();
                for request in stale {
                    state.retire_approval_request(&request);
                    pending_approvals.retain(|r| !TuiState::same_approval_request(r, &request));
                }
                for request in coordinator.drain_pending() {
                    state.present_approval(request.clone());
                    let preview = request.preview.as_ref().map(|preview| {
                        let text = format!("\n\nProposed change:\n{}", preview.display_text());
                        let block = match preview {
                            crate::tools::ToolPreview::Diff { path, patch, .. } => {
                                crate::view_model::ViewBlock::Diff {
                                    path: Some(path.clone()),
                                    patch: patch.clone(),
                                }
                            }
                        };
                        (text, block)
                    });
                    let safe_arguments = crate::security::redact_json(&request.arguments, &[]);
                    let suffix = preview
                        .as_ref()
                        .map(|(text, _)| text.as_str())
                        .unwrap_or("");
                    let prompt = format!(
                        "Approval required: {} {}\nRequest: {}\nPolicy: {}{}\nAlt-A reviews this request; y/n selects, Enter confirms.",
                        request.tool,
                        safe_arguments,
                        request.request_id,
                        request.policy_digest.as_deref().unwrap_or("unbound"),
                        suffix,
                    );
                    if let Some((_, block)) = preview {
                        state.push_message_block(MessageRole::System, prompt, block);
                    } else {
                        state.push_message(
                            // Keep the approval prompt visible while tool logs are
                            // folded; hiding it would leave the user with no way to
                            // know what the pending y/n response refers to.
                            MessageRole::System,
                            prompt,
                        );
                    }
                    // The coordinator itself is bounded by one request per tool
                    // turn; keep a defensive host bound for malformed providers.
                    if pending_approvals.len() < 128 {
                        pending_approvals.push_back(request);
                    }
                    state.set_status("Approval required");
                    scheduler.request();
                }
            }
            while let Ok(event) = runner.try_next_event() {
                match event {
                    RuntimeEvent::Completed { id, outcome } => {
                        let rejected_input = submitted_inputs.remove(&id);
                        let events = stream_buffers.remove(&id);
                        if Some(id) != active_job {
                            // A superseded stream may still publish its
                            // terminal event after the replacement was
                            // admitted. Its buffered deltas are intentionally
                            // discarded and must not alter the active turn.
                            continue;
                        }
                        // A final provider delta can race the runtime
                        // completion notification. Drain this job's buffer
                        // once more before replacing its provisional line.
                        if let Some(events) = events
                            && drain_tui_provider_events(&mut state, id.get(), &events)
                        {
                            scheduler.request();
                        }
                        // Approval prompts belong to the turn that just
                        // reached a terminal state. Drop any prompt that
                        // raced with completion/cancellation; otherwise the
                        // next normal user message would be consumed as a
                        // stale y/n response.
                        pending_approvals.clear();
                        state.clear_activity_approvals();
                        active_job = None;
                        active_job_kind = None;
                        state.set_busy(false);
                        drain_agent_tool_events(
                            active_job_owner.as_ref().unwrap_or(&shared),
                            &mut state,
                        );
                        match outcome {
                            JobOutcome::Succeeded(TuiJobResult::Model(result)) => {
                                if let Some(assistant) = result.assistant {
                                    finish_turn_reasoning(&mut state, id.get(), &assistant);
                                    state.finish_stream_for_job(
                                        id.get(),
                                        MessageRole::Assistant,
                                        assistant.content,
                                    );
                                } else {
                                    state.discard_stream();
                                }
                                state.set_status("Ready");
                            }
                            JobOutcome::Succeeded(TuiJobResult::ResourceControl(result))
                                if result["command"] == "blueprint" =>
                            {
                                state.push_message(
                                    MessageRole::System,
                                    format_external_evidence_projection(&result),
                                );
                                state.set_status("Ready");
                            }
                            JobOutcome::Succeeded(TuiJobResult::ResourceControl(result))
                                if result["command"] == "tool_output" =>
                            {
                                state.push_message(
                                    MessageRole::System,
                                    format_output_projection(&result),
                                );
                                state.set_status("Ready");
                            }
                            JobOutcome::Succeeded(TuiJobResult::ResourceControl(result))
                                if result["command"] == "tree" =>
                            {
                                if result["tree_selection"] == true
                                    && let Ok(agent) =
                                        active_job_owner.as_ref().unwrap_or(&shared).try_lock()
                                {
                                    reload_state_from_agent(&mut state, &agent);
                                }
                                state.push_message(
                                    MessageRole::System,
                                    format_tree_projection(&result),
                                );
                                state.set_status("Ready");
                            }
                            JobOutcome::Succeeded(TuiJobResult::ResourceControl(result)) => {
                                state.push_message(
                                    MessageRole::System,
                                    // Resource metadata is inspectable content, including
                                    // full source paths. push_message retains the existing
                                    // message byte bound; preserve line breaks for scrolling.
                                    format_resource_view(&result),
                                );
                                state.set_status(if result["command"] == "compact" {
                                    if result["compacted"] == true {
                                        "Context compacted"
                                    } else {
                                        "Context within budget"
                                    }
                                } else {
                                    "Resources ready"
                                });
                            }
                            JobOutcome::Succeeded(TuiJobResult::UserShell(result)) => {
                                state.discard_stream();
                                render_user_shell_result(&mut state, &result);
                                // ZS1-148: record the bounded result in the arch
                                // master-session lane and release its single
                                // concurrency slot.
                                if state.master_busy() {
                                    let summary = result
                                        .get("exit_code")
                                        .and_then(serde_json::Value::as_i64)
                                        .map(|code| format!("arch bash finished (exit {code})"))
                                        .unwrap_or_else(|| "arch bash finished".to_owned());
                                    state.complete_master_turn(MessageRole::System, summary);
                                }
                            }
                            JobOutcome::Failed(error) => {
                                if let Some(text) = rejected_input {
                                    state.restore_submitted_input(text);
                                }
                                state.discard_stream();
                                state.finish_running_tools(ToolRunStatus::Failed);
                                state.push_message(MessageRole::Error, error.to_string());
                                state.set_status("Request failed");
                            }
                            JobOutcome::Cancelled => {
                                state.discard_stream();
                                state.finish_running_tools(ToolRunStatus::Cancelled);
                                state.set_status("Interrupted");
                            }
                            JobOutcome::Panicked => {
                                state.discard_stream();
                                state.finish_running_tools(ToolRunStatus::Failed);
                                state.push_message(MessageRole::Error, "background job panicked");
                                state.set_status("Request failed");
                            }
                        }
                        // Any terminal job boundary releases the arch master
                        // session's single concurrency slot (ZS1-148). The arm
                        // above may already have recorded a richer arch result.
                        if state.master_busy() {
                            state.set_master_busy(false);
                        }
                        scheduler.request();
                    }
                    RuntimeEvent::Rejected { id, reason } => {
                        let rejected_input = submitted_inputs.remove(&id);
                        let _ = stream_buffers.remove(&id);
                        if Some(id) != active_job {
                            continue;
                        }
                        // A rejected active request cannot produce a valid
                        // approval response. Do not let its prompt swallow
                        // the next user turn.
                        pending_approvals.clear();
                        state.clear_activity_approvals();
                        active_job = None;
                        active_job_kind = None;
                        state.discard_stream();
                        state.set_busy(false);
                        if state.master_busy() {
                            state.set_master_busy(false);
                        }
                        state.push_message(MessageRole::Error, reason.to_string());
                        state.set_status("Request rejected");
                        if let Some(text) = rejected_input {
                            state.restore_submitted_input(text);
                        }
                        scheduler.request();
                    }
                    RuntimeEvent::CancelRequested { id } if Some(id) == active_job => {
                        // Cancellation can also be requested by the runtime
                        // (rather than directly by the key handler).  Treat
                        // that acknowledgement as the terminal boundary for
                        // any approval prompt belonging to this turn.
                        pending_approvals.clear();
                        state.clear_activity_approvals();
                        state.set_status("Interrupt requested");
                        scheduler.request();
                    }
                    RuntimeEvent::Closed => break,
                    _ => {}
                }
            }
            if let Some(index) = state.project_index(&visible_project) {
                state.select_project_tab(index);
            }
            if active_job.is_none() {
                active_job_project = None;
                active_job_owner = None;
            }
            // Admit exactly one queued input after the previous job has crossed
            // its terminal boundary. FIFO admission prevents a later prompt
            // from overtaking a local shell command and never calls the model
            // for a `!` request.
            let queued_project = pending_inputs.projects.front().cloned();
            if active_job.is_none()
                && !queued_project
                    .as_deref()
                    .is_some_and(|project| new_sessions.owns(project))
                && let Some((kind, text, paste)) = pending_inputs.pop()
            {
                pending_inputs.sync_projection(&mut state);
                if let Some(index) = queued_project
                    .as_ref()
                    .and_then(|key| state.project_index(key))
                {
                    state.select_project_tab(index);
                    shared = project_host.active(&state);
                }
                let request_text = queued_request_text(kind, text);
                let display = request_text.clone();
                let (request, events) = TuiRequest::with_kind(request_text, kind);
                let mut request = request;
                request.owner = Some(Arc::clone(&shared));
                if let Ok(agent) = shared.try_lock() {
                    input_controls.remember_owner(state.active_project(), &agent);
                }
                approval = shared
                    .try_lock()
                    .ok()
                    .and_then(|agent| agent.approval_coordinator())
                    .or(approval.clone());
                let submitted_text = SubmittedInput {
                    text: request.text.clone(),
                    paste,
                };
                match runner.try_submit(request) {
                    Ok(id) => {
                        submitted_inputs.insert(id, submitted_text);
                        stream_buffers.insert(id, events);
                        if kind == TuiRequestKind::Model {
                            state.begin_stream_for_job(id.get());
                        } else {
                            if kind == TuiRequestKind::UserShell {
                                state.begin_stream_for_job(id.get());
                            }
                            state.push_message(MessageRole::User, display);
                            state.set_status(if kind == TuiRequestKind::ResourceControl {
                                "Loading resources"
                            } else {
                                "Local shell running"
                            });
                        }
                        active_job = Some(id);
                        active_job_project = Some(state.active_project().to_owned());
                        active_job_owner = Some(Arc::clone(&shared));
                        active_job_kind = Some(kind);
                        state.set_busy(true);
                    }
                    Err(error) => {
                        state.push_message(MessageRole::Error, error.to_string());
                        state.set_status("Request rejected");
                        state.restore_submitted_input(submitted_text);
                    }
                }
            }
            if let Some(index) = state.project_index(&visible_project) {
                state.select_project_tab(index);
                shared = project_host.active(&state);
            }
            if last_checkpoint.elapsed() >= Duration::from_millis(250) {
                if state.layout_dirty() && !layout_persistence.disabled {
                    match layout_persistence.save(&state) {
                        Ok(()) => {
                            state.clear_layout_dirty();
                            state.clear_layout_reset_tabs();
                        }
                        Err(error) => {
                            state.checkpoint_result(Err(error.clone()));
                            state.push_message(
                                MessageRole::Error,
                                format!(
                                    "layout preferences could not be saved; persistence disabled: {error}"
                                ),
                            );
                            layout_persistence.disable();
                            state.clear_layout_dirty();
                            state.clear_layout_reset_tabs();
                        }
                    }
                }
                if state.project_checkpoint_dirty() && !layout_persistence.disabled {
                    let result = layout_persistence.save_project_checkpoint(&state);
                    state.checkpoint_result(result);
                }
                scheduler.request();
                last_checkpoint = Instant::now();
            }
            if editor
                .poll(&mut state, &mut guard, &mut terminal, Some(&shared))
                .map_err(|error| crate::error::ZenpiError::Message(error.to_string()))?
            {
                scheduler.request();
                resize_pending = false;
            }
            if editor.active() {
                std::thread::sleep(poll_interval.min(Duration::from_millis(20)));
                continue;
            }
            let now = Instant::now();
            state.flush_ordinary_paste(now);
            if state.is_busy() && now.saturating_duration_since(last_tick) >= poll_interval {
                state.tick();
                scheduler.request();
                last_tick = now;
            }
            if state.take_dirty() {
                scheduler.request();
            }
            if scheduler.due(now) {
                if resize_pending {
                    terminal
                        .autoresize()
                        .map_err(|error| crate::error::ZenpiError::Message(error.to_string()))?;
                    resize_pending = false;
                }
                terminal
                    .draw(|frame| state.render_bentobox(frame, &config.title))
                    .map_err(|error| crate::error::ZenpiError::Message(error.to_string()))?;
                scheduler.rendered(Instant::now());
            }
            let wait = if scheduler.is_dirty() {
                frame_interval.min(poll_interval)
            } else {
                poll_interval
            };
            let wait = state.ordinary_paste_wait(Instant::now(), wait);
            if !event::poll(wait)
                .map_err(|error| crate::error::ZenpiError::Message(error.to_string()))?
            {
                continue;
            }
            let mut processed = 0usize;
            loop {
                let event = event::read()
                    .map_err(|error| crate::error::ZenpiError::Message(error.to_string()))?;
                if matches!(event, Event::Resize(_, _)) {
                    resize_pending = true;
                }
                // ZS1-148: an arch master-session submission is normalized onto
                // the standard submit route. `display()` keeps the leading `!`
                // for bash so the shared parser selects the user-shell owner,
                // while steering text reaches the existing Steer/Prompt path.
                let action = match state.handle_event(event) {
                    TuiAction::SubmitArch(command) => TuiAction::Submit(command.display()),
                    other => other,
                };
                match action {
                    TuiAction::RespondApproval {
                        project,
                        request_id,
                        turn_id,
                        call_id,
                        allow,
                        remember,
                    } => {
                        let result = approval
                            .as_ref()
                            .ok_or(crate::approval::ApprovalError::UnknownRequest)
                            .and_then(|owner| {
                                respond_tui_approval(
                                    &mut state,
                                    owner,
                                    active_job_project.as_deref(),
                                    (&project, &request_id, &turn_id, &call_id),
                                    (allow, remember),
                                )
                            });
                        match result {
                            Ok(()) => pending_approvals.retain(|r| r.request_id != request_id),
                            Err(error) => {
                                // A failed response is not a decision. The coordinator
                                // reconciliation above retires requests only after
                                // they are no longer pending.
                                state.push_message(
                                    MessageRole::Error,
                                    format!("Approval unavailable: {error}"),
                                );
                            }
                        }
                        scheduler.request();
                    }
                    TuiAction::InspectResource {
                        command,
                        source_hash,
                    } => {
                        let result = match shared.try_lock() {
                            Ok(agent) => state.inspect_resource_source(
                                &agent.resource_snapshot(),
                                &command,
                                &source_hash,
                            ),
                            Err(_) => {
                                Err("Resource owner busy; press F2 again when work finishes".into())
                            }
                        };
                        if let Err(error) = result {
                            state.set_status(error);
                        }
                        scheduler.request();
                    }
                    TuiAction::OpenExternalEditor => {
                        editor
                            .start(&mut state, &mut guard, &mut terminal, Some(&shared))
                            .map_err(|error| {
                                crate::error::ZenpiError::Message(error.to_string())
                            })?;
                        scheduler.request();
                        continue 'outer;
                    }
                    TuiAction::Copy(text) => {
                        state.transcript_ux.copy_notice = Some((
                            match write_terminal_clipboard(&text) {
                                Ok(()) => format!("Clipboard sent {} bytes", text.len()),
                                Err(error) => format!("Copy failed: {error}"),
                            },
                            Instant::now(),
                        ));
                        state.dirty = true;
                        scheduler.request();
                    }
                    TuiAction::OpenSession(path) => {
                        let action = crate::slash::SessionAction::Open { path };
                        match project_host.resume_session(&mut state, &action) {
                            Ok(data) => {
                                state.push_message(
                                    MessageRole::System,
                                    format!(
                                        "session opened:\n{}",
                                        bounded_display(&data.to_string())
                                    ),
                                );
                                if let Some(path) = state
                                    .project_metadata(state.active_project())
                                    .and_then(|meta| meta.session_path.as_ref())
                                    && gantt_tracker.switch_session(path.into())
                                {
                                    state.clear_gantt_snapshot();
                                    gantt_refresh_pending = true;
                                    state.gantt_refresh_started();
                                }
                            }
                            Err(error) => state.push_message(
                                MessageRole::Error,
                                format!("session open failed: {error}"),
                            ),
                        }
                    }
                    TuiAction::Submit(text) => {
                        if new_sessions.owns(state.active_project())
                            && !text.trim_start().starts_with('/')
                        {
                            state.restore_rejected_input(&text);
                            state.push_message(MessageRole::Error, "Wait for this project's new session to finish before submitting input");
                            scheduler.request();
                            continue 'outer;
                        }
                        // Parse before checking an approval prompt.  A slash
                        // command is control-plane input even while a tool is
                        // waiting for confirmation; it must never be mistaken
                        // for a y/n answer or sent to the provider.
                        if let Some(action) = slash::input_queue_control(&text) {
                            state.push_message(MessageRole::User, &text);
                            match action {
                                Ok(action) => {
                                    input_controls.submit(&mut state, &shared, action, text)
                                }
                                Err(error) => {
                                    state.push_message(MessageRole::Error, error);
                                    state.set_rejected_input(text);
                                }
                            }
                            scheduler.request();
                            continue 'outer;
                        }
                        if let Some(action) = slash::output_control(&text) {
                            if !state.busy
                                && active_job.is_none()
                                && pending_inputs.inputs.is_empty()
                                && action.is_ok()
                            {
                                pending_inputs.admit(
                                    TuiRequestKind::ResourceControl,
                                    text,
                                    &mut state,
                                );
                            } else {
                                state.push_message(
                                    MessageRole::Error,
                                    action.err().unwrap_or_else(|| {
                                        "output control requires an idle project".into()
                                    }),
                                );
                                state.set_rejected_input(text);
                            }
                            scheduler.request();
                            continue 'outer;
                        }
                        if let Some(action) = slash::tree_control(&text) {
                            if !state.busy
                                && active_job.is_none()
                                && pending_inputs.inputs.is_empty()
                                && matches!(&action, Ok(action) if !matches!(action, crate::protocol::TreeAction::List { .. }))
                            {
                                pending_inputs.admit(
                                    TuiRequestKind::ResourceControl,
                                    text,
                                    &mut state,
                                );
                            } else {
                                state.push_message(MessageRole::User, &text);
                                if pending_inputs.inputs.is_empty() {
                                    match shared.try_lock() {
                                        Ok(mut agent) => {
                                            dispatch_tree_input(
                                                &mut state,
                                                Some(&mut agent),
                                                &text,
                                            );
                                        }
                                        Err(_) => {
                                            dispatch_tree_input(&mut state, None, &text);
                                        }
                                    }
                                } else {
                                    dispatch_tree_input(&mut state, None, &text);
                                }
                            }
                            scheduler.request();
                            continue 'outer;
                        }
                        if pending_inputs.scheduled_command(&mut state, &text) {
                            scheduler.request();
                            continue 'outer;
                        }
                        if text.split_whitespace().next() == Some("/reasoning") {
                            state.push_message(MessageRole::User, &text);
                            match shared.try_lock() {
                                Ok(mut agent) => {
                                    dispatch_reasoning_input(&mut state, Some(&mut agent), &text);
                                }
                                Err(_) => {
                                    dispatch_reasoning_input(&mut state, None, &text);
                                }
                            }
                            scheduler.request();
                            continue 'outer;
                        }
                        if slash::resource_control(&text).is_some()
                            || slash::external_evidence_control(&text)
                            || text.trim() == "/compact"
                        {
                            pending_inputs.admit(TuiRequestKind::ResourceControl, text, &mut state);
                            scheduler.request();
                            continue 'outer;
                        }
                        match state.route_catalog_input(&text) {
                            Err(error) => {
                                state.push_message(MessageRole::User, &text);
                                state.push_message(MessageRole::Error, error.to_string());
                                state.set_status("Command rejected");
                                state.set_rejected_input(text);
                            }
                            Ok(InputRoute::Slash(command)) => {
                                // Local diff owns the active project until it
                                // completes or is cancelled.  It is a
                                // separate worker from the provider job, so
                                // `state.busy` alone cannot express this
                                // admission boundary.  Rejecting here keeps
                                // commands from silently waiting behind a
                                // diff and gives the same immediate feedback
                                // as other busy-owner commands.
                                if local_diff_host.active.is_some()
                                    && !matches!(command, SlashCommand::Cancel)
                                {
                                    state.set_status("Command requires an idle project");
                                    state.set_rejected_input(text);
                                    scheduler.request();
                                    continue 'outer;
                                }
                                // Keep /diff discoverable in the busy palette,
                                // but do not start a second local inspection
                                // while the project still owns provider work.
                                if matches!(command, SlashCommand::Diff { .. })
                                    && (state.busy || active_job.is_some())
                                {
                                    state.set_status("Command requires an idle project");
                                    state.set_rejected_input(text);
                                    scheduler.request();
                                    continue 'outer;
                                }
                                if !state.command_available(command.name()) {
                                    state.set_status("Command requires an idle project");
                                    state.set_rejected_input(text);
                                    scheduler.request();
                                    continue 'outer;
                                }
                                if matches!(
                                    &command,
                                    SlashCommand::Session {
                                        action: crate::slash::SessionAction::New
                                    }
                                ) {
                                    let project = state.active_project().to_owned();
                                    let busy = (active_job.is_some()
                                        && active_job_project.as_deref() == Some(project.as_str()))
                                        || input_controls.has_pending(&project)
                                        || pending_inputs.projects.contains(&project)
                                        || state
                                            .scheduled_inputs
                                            .iter()
                                            .any(|entry| entry.project == project)
                                        || state.approval_views.contains_key(&project)
                                        || (active_resource_job.is_some()
                                            && active_resource_project == project)
                                        || editor.active();
                                    let fingerprint = format!(
                                        "tui-{}",
                                        std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_nanos()
                                    );
                                    let proposal = if busy {
                                        Err("new session requires settled requests, approvals and scheduled work".to_owned())
                                    } else {
                                        project_host.pool.new_session_plan(
                                            &project,
                                            None,
                                            fingerprint,
                                            1,
                                        )
                                    };
                                    match proposal {
                                        Err(error) => {
                                            state.push_message(MessageRole::Error, error);
                                            state.restore_rejected_input(&text);
                                        }
                                        Ok(plan) => {
                                            if let Err(error) = new_sessions.start(
                                                &mut state,
                                                Arc::clone(&shared),
                                                plan,
                                                &text,
                                            ) {
                                                state.restore_rejected_input(&text);
                                                state.push_message(MessageRole::Error, error);
                                            }
                                        }
                                    }
                                    scheduler.request();
                                    continue 'outer;
                                }
                                state.push_message(MessageRole::User, &text);
                                if let SlashCommand::Project { action } = &command {
                                    use crate::slash::ProjectAction;
                                    let intent = match action {
                                        ProjectAction::Open { name } => {
                                            Some(ProjectIntent::Open(name.into()))
                                        }
                                        ProjectAction::Select { name } => state
                                            .project_index(name)
                                            .or_else(|| {
                                                (0..state.project_tabs.len())
                                                    .find(|i| state.project_label(*i) == *name)
                                            })
                                            .map(ProjectIntent::Select),
                                        ProjectAction::Close { name } => state
                                            .project_index(name)
                                            .or_else(|| {
                                                (0..state.project_tabs.len())
                                                    .find(|i| state.project_label(*i) == *name)
                                            })
                                            .map(|i| {
                                                ProjectIntent::Close(state.project_tabs[i].clone())
                                            }),
                                        ProjectAction::List => {
                                            state.push_message(
                                                MessageRole::System,
                                                (0..state.project_tabs.len())
                                                    .map(|i| {
                                                        format!(
                                                            "{} {}",
                                                            state.project_tabs[i],
                                                            state.project_label(i)
                                                        )
                                                    })
                                                    .collect::<Vec<_>>()
                                                    .join("\n"),
                                            );
                                            None
                                        }
                                        ProjectAction::Move { name, index } => {
                                            let ok = state.move_project_tab(name, *index);
                                            state.push_message(
                                                MessageRole::System,
                                                if ok {
                                                    format!("project moved: {name} -> {index}")
                                                } else {
                                                    format!("cannot move project: {name}")
                                                },
                                            );
                                            None
                                        }
                                        ProjectAction::Rename { old, new } => {
                                            let ok = !new.trim().is_empty()
                                                && !state.project_tabs.contains(new)
                                                && state.rename_project_tab(old, new.clone());
                                            state.push_message(
                                                MessageRole::System,
                                                if ok {
                                                    format!("project renamed: {old} -> {new}")
                                                } else {
                                                    format!("cannot rename project: {old}")
                                                },
                                            );
                                            None
                                        }
                                        ProjectAction::Style { name, style } => {
                                            let ok = state.style_project_tab(name, style);
                                            state.push_message(
                                                MessageRole::System,
                                                if ok {
                                                    format!("project style: {name} = {style}")
                                                } else {
                                                    format!("unknown project or style: {name}/{style}")
                                                },
                                            );
                                            None
                                        }
                                    };
                                    if intent.is_none()
                                        && !matches!(
                                            action,
                                            ProjectAction::List
                                                | ProjectAction::Move { .. }
                                                | ProjectAction::Rename { .. }
                                                | ProjectAction::Style { .. }
                                        )
                                    {
                                        state.push_message(MessageRole::Error, "Project not found");
                                    }
                                    state.pending_project = intent;
                                    scheduler.request();
                                    break;
                                }
                                let refresh_gantt = command_refreshes_gantt(&command);
                                if let SlashCommand::Resources { path } = command {
                                    match resource_runner.try_submit((state.project_cwd(), path)) {
                                        Ok(id) => {
                                            active_resource_job = Some(id);
                                            active_resource_project =
                                                state.active_project().to_owned();
                                            last_resource_refresh = Instant::now();
                                            state.resource_refresh_started();
                                            state.push_message(
                                                MessageRole::System,
                                                "resource refresh started",
                                            );
                                        }
                                        Err(error) => {
                                            state.resource_refresh_failed(error.to_string());
                                            state.push_message(
                                                MessageRole::Error,
                                                format!("resource refresh rejected: {error}"),
                                            );
                                        }
                                    }
                                    scheduler.request();
                                    break;
                                }
                                if let SlashCommand::Approve { id, decision } = command {
                                    if active_job_project.as_deref() != Some(state.active_project())
                                    {
                                        state.push_message(MessageRole::Error,"Approval belongs to another project; switch to its tab to respond.");
                                        scheduler.request();
                                        break;
                                    }
                                    let (decision, remember) = match decision {
                                        crate::slash::ApproveDecision::Once => {
                                            (crate::approval::ApprovalDecision::Allow, false)
                                        }
                                        crate::slash::ApproveDecision::Always => {
                                            (crate::approval::ApprovalDecision::Allow, true)
                                        }
                                        crate::slash::ApproveDecision::Deny => {
                                            (crate::approval::ApprovalDecision::Deny, false)
                                        }
                                    };
                                    let request = pending_approvals
                                        .iter()
                                        .find(|request| request.request_id == id)
                                        .cloned();
                                    let project = state.active_project().to_owned();
                                    let result = request
                                        .as_ref()
                                        .ok_or(crate::approval::ApprovalError::UnknownRequest)
                                        .and_then(|request| {
                                            let coordinator = approval.as_ref().ok_or(
                                                crate::approval::ApprovalError::UnknownRequest,
                                            )?;
                                            respond_tui_approval(
                                                &mut state,
                                                coordinator,
                                                active_job_project.as_deref(),
                                                (&project, &id, &request.turn_id, &request.call_id),
                                                (
                                                    decision
                                                        == crate::approval::ApprovalDecision::Allow,
                                                    remember,
                                                ),
                                            )
                                        });
                                    match result {
                                        Ok(()) => pending_approvals
                                            .retain(|request| request.request_id != id),
                                        Err(error) => {
                                            state.push_message(
                                                MessageRole::Error,
                                                format!("approval failed: {error}"),
                                            );
                                            state.set_status("Approval unavailable");
                                        }
                                    }
                                    scheduler.request();
                                    continue 'outer;
                                }
                                if let SlashCommand::Diff { path } = command {
                                    local_diff_host.start(&mut state, path, text);
                                    scheduler.request();
                                    continue 'outer;
                                }
                                if session_browser_host.route(&command, &mut state, text.clone()) {
                                    scheduler.request();
                                    continue 'outer;
                                }
                                let action = if let SlashCommand::Session {
                                    action:
                                        session_action @ (crate::slash::SessionAction::Open { .. }
                                        | crate::slash::SessionAction::ResumeLast),
                                } = &command
                                {
                                    match project_host.resume_session(&mut state, session_action) {
                                        Ok(data) => state.push_message(
                                            MessageRole::System,
                                            format!(
                                                "session opened:\n{}",
                                                bounded_display(&data.to_string())
                                            ),
                                        ),
                                        Err(error) => state.push_message(
                                            MessageRole::Error,
                                            format!("session open failed: {error}"),
                                        ),
                                    }
                                    SlashDispatchAction::Continue
                                } else {
                                    match shared.try_lock() {
                                        Ok(mut agent) => dispatch_slash_command(
                                            command,
                                            &mut state,
                                            Some(&mut agent),
                                        ),
                                        Err(_) => dispatch_slash_command(command, &mut state, None),
                                    }
                                };
                                if state
                                    .messages
                                    .back()
                                    .is_some_and(|m| m.role == MessageRole::Error)
                                {
                                    state.restore_rejected_input(&text);
                                } else {
                                    state.take_submitted_paste(&text);
                                }
                                if let Ok(agent) = shared.try_lock() {
                                    state.refresh_transcript_status(&agent);
                                }
                                match action {
                                    SlashDispatchAction::Interrupt => {
                                        if local_diff_host.cancel_current(&mut state) {
                                            scheduler.request();
                                            continue 'outer;
                                        }
                                        if new_sessions.cancel_current(&mut state) {
                                            scheduler.request();
                                            continue 'outer;
                                        }
                                        if active_job.is_some()
                                            && active_job_project.as_deref()
                                                != Some(state.active_project())
                                        {
                                            state.push_message(
                                                MessageRole::System,
                                                "Switch to the running project to interrupt it.",
                                            );
                                            continue 'outer;
                                        }
                                        pending_inputs.cancel(&mut state);
                                        if let Some(id) = active_job {
                                            let cancel_result = runner.try_cancel(id);
                                            if !matches!(
                                                cancel_result,
                                                Err(crate::runtime::SubmitError::QueueFull)
                                            ) {
                                                if let Some(approval) = approval.as_ref() {
                                                    approval.emergency_cancel();
                                                }
                                                pending_approvals.clear();
                                                state.clear_activity_approvals();
                                            }
                                            state.set_status("Interrupt requested");
                                        } else {
                                            state.set_status("Ready");
                                        }
                                    }
                                    SlashDispatchAction::Quit => break 'outer,
                                    SlashDispatchAction::Continue => {}
                                }
                                // `/session open` may replace the active
                                // journal. Reconcile after dispatch so a
                                // rejected switch retains its prior pane, but
                                // a successful switch clears that pane before
                                // the replacement store is loaded.
                                if let Ok(agent) = shared.try_lock() {
                                    let active_path = agent.session().path().to_path_buf();
                                    drop(agent);
                                    if gantt_tracker.switch_session(active_path) {
                                        state.clear_gantt_snapshot();
                                        gantt_refresh_pending = true;
                                        state.gantt_refresh_started();
                                    }
                                }
                                if refresh_gantt {
                                    gantt_refresh_pending = true;
                                    state.gantt_refresh_started();
                                }
                            }
                            Ok(InputRoute::UserShell(command)) => {
                                // The parser preserves the leading marker for
                                // auditability; the worker receives it again
                                // when this normalized command is submitted.
                                let command = command
                                    .strip_prefix('!')
                                    .unwrap_or(command.as_str())
                                    .to_owned();
                                if pending_inputs
                                    .should_queue(TuiRequestKind::UserShell, active_job_kind)
                                {
                                    pending_inputs.admit_original(
                                        TuiRequestKind::UserShell,
                                        command,
                                        text.clone(),
                                        &mut state,
                                    );
                                } else {
                                    let (request, events) = TuiRequest::with_kind(
                                        format!("!{command}"),
                                        TuiRequestKind::UserShell,
                                    );
                                    let mut request = request;
                                    request.owner = Some(Arc::clone(&shared));
                                    if let Ok(agent) = shared.try_lock() {
                                        input_controls
                                            .remember_owner(state.active_project(), &agent);
                                    }
                                    approval = shared
                                        .try_lock()
                                        .ok()
                                        .and_then(|agent| agent.approval_coordinator())
                                        .or(approval.clone());
                                    let submitted_text = state.bind_submitted_input(text.clone());
                                    match runner.try_submit(request) {
                                        Ok(id) => {
                                            submitted_inputs.insert(id, submitted_text);
                                            stream_buffers.insert(id, events);
                                            state.begin_stream_for_job(id.get());
                                            state.push_message(
                                                MessageRole::User,
                                                format!("!{command}"),
                                            );
                                            state.set_busy(true);
                                            state.set_status("Local shell running");
                                            active_job = Some(id);
                                            active_job_project =
                                                Some(state.active_project().to_owned());
                                            active_job_owner = Some(Arc::clone(&shared));
                                            active_job_kind = Some(TuiRequestKind::UserShell);
                                        }
                                        Err(error) => {
                                            state.push_message(
                                                MessageRole::Error,
                                                error.to_string(),
                                            );
                                            state.set_status("Local shell rejected");
                                            state.restore_submitted_input(submitted_text);
                                        }
                                    }
                                }
                            }
                            Ok(InputRoute::Prompt(text)) => {
                                if active_job.is_some()
                                    && active_job_project.as_deref() != Some(state.active_project())
                                {
                                    pending_inputs.admit(TuiRequestKind::Model, text, &mut state);
                                    scheduler.request();
                                    continue 'outer;
                                }
                                if pending_inputs
                                    .should_queue(TuiRequestKind::Model, active_job_kind)
                                {
                                    pending_inputs.admit(TuiRequestKind::Model, text, &mut state);
                                    scheduler.request();
                                    continue 'outer;
                                }
                                state.push_message(MessageRole::User, &text);
                                if active_job.is_some() {
                                    // Resources and files need normal owner admission as a future
                                    // job; text-only inputs can join the current Agent boundary.
                                    if text.starts_with('/')
                                        || prompt_file_references(&text)
                                            .map_or(true, |references| !references.is_empty())
                                    {
                                        pending_inputs.admit(
                                            TuiRequestKind::Model,
                                            text,
                                            &mut state,
                                        );
                                    } else {
                                        let action = crate::protocol::InputQueueAction::Enqueue {
                                            input_id: input_controls.next_id(),
                                            kind: crate::input_queue::InputKind::Steer,
                                            text: text.clone(),
                                        };
                                        input_controls.submit(&mut state, &shared, action, text);
                                    }
                                } else {
                                    let (request, events) = TuiRequest::new(text);
                                    let mut request = request;
                                    request.owner = Some(Arc::clone(&shared));
                                    if let Ok(agent) = shared.try_lock() {
                                        input_controls
                                            .remember_owner(state.active_project(), &agent);
                                    }
                                    approval = shared
                                        .try_lock()
                                        .ok()
                                        .and_then(|agent| agent.approval_coordinator())
                                        .or(approval.clone());
                                    let submitted_text =
                                        state.bind_submitted_input(request.text.clone());
                                    match runner.try_submit(request) {
                                        Ok(id) => {
                                            submitted_inputs.insert(id, submitted_text);
                                            stream_buffers.insert(id, events);
                                            state.begin_stream_for_job(id.get());
                                            active_job = Some(id);
                                            active_job_project =
                                                Some(state.active_project().to_owned());
                                            active_job_owner = Some(Arc::clone(&shared));
                                            active_job_kind = Some(TuiRequestKind::Model);
                                            state.set_busy(true);
                                            state.set_status("Working");
                                        }
                                        Err(error) => {
                                            state.push_message(
                                                MessageRole::Error,
                                                error.to_string(),
                                            );
                                            state.set_status("Request rejected");
                                            state.restore_submitted_input(submitted_text);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    TuiAction::Interrupt => {
                        if local_diff_host.cancel_current(&mut state) {
                            scheduler.request();
                            continue 'outer;
                        }
                        if new_sessions.cancel_current(&mut state) {
                            scheduler.request();
                            continue 'outer;
                        }
                        if active_job.is_some()
                            && active_job_project.as_deref() != Some(state.active_project())
                        {
                            state.push_message(MessageRole::System,"The running request belongs to another project; switch to that tab to interrupt.");
                            continue 'outer;
                        }
                        pending_inputs.cancel(&mut state);
                        if let Some(id) = active_job {
                            let cancel_result = runner.try_cancel(id);
                            if !matches!(cancel_result, Err(crate::runtime::SubmitError::QueueFull))
                            {
                                // The cancel command is admitted (or the
                                // worker is already closed), so no approval
                                // from this turn can be answered safely.
                                if let Some(approval) = approval.as_ref() {
                                    approval.emergency_cancel();
                                }
                                pending_approvals.clear();
                                state.clear_activity_approvals();
                            }
                            state.set_status("Interrupt requested");
                        } else {
                            state.set_status("Ready");
                        }
                    }
                    TuiAction::Quit => break 'outer,
                    // ZS1-148: arch submissions are normalized to `Submit`
                    // before this match, so this arm is a defensive no-op.
                    TuiAction::SubmitArch(_) => {}
                    TuiAction::Redraw | TuiAction::None => {}
                }
                processed += 1;
                if processed >= 256
                    || !event::poll(Duration::ZERO)
                        .map_err(|error| crate::error::ZenpiError::Message(error.to_string()))?
                {
                    break;
                }
            }
        }
        Ok(())
    })();
    editor.shutdown(&mut state);
    state.finish_ordinary_paste();
    // Save any final layout mutation when the user quits before the next
    // event-loop checkpoint. The operation is bounded and atomic; a failure
    // must not mask the primary terminal/runtime result.
    if state.layout_dirty()
        && !layout_persistence.disabled
        && layout_persistence.save(&state).is_ok()
    {
        state.clear_layout_dirty();
        state.clear_layout_reset_tabs();
    }
    if state.project_checkpoint_dirty() && !layout_persistence.disabled {
        let result = layout_persistence.save_project_checkpoint(&state);
        state.checkpoint_result(result);
    }
    // Restore the terminal before waiting for provider cancellation. A second
    // OS signal can then use the original disposition without stranding raw
    // mode even when an uncooperative network operation delays the join.
    guard.leave();
    drop(local_diff_host);
    drop(session_browser_host);
    if let Some(error) = &state.checkpoint_error {
        eprintln!("Draft not saved: {error}");
    }
    // Terminal I/O can fail while a provider job is still active. Always
    // cancel and join the owned runtime before returning that error; relying
    // on `Drop` would detach a worker when its command queue is full.
    let join_result = runner.shutdown_and_join();
    let new_session_join_result =
        new_sessions.shutdown(&mut state, &mut project_host, &mut input_controls);
    if state.project_checkpoint_dirty() && !layout_persistence.disabled {
        let result = layout_persistence.save_project_checkpoint(&state);
        state.checkpoint_result(result);
    }
    let resource_join_result = resource_runner.shutdown_and_join();
    let completion_join_result = completion_runner.shutdown_and_join();
    let gantt_join_result = gantt_runner.shutdown_and_join();
    project_host.pool.close_all();
    match loop_result {
        Err(error) => {
            let _ = join_result;
            Err(error)
        }
        Ok(()) => join_result
            .and(new_session_join_result)
            .and(resource_join_result)
            .and(completion_join_result)
            .and(gantt_join_result)
            .map_err(|_| crate::error::ZenpiError::Message("runtime worker panicked".into())),
    }
}

/// Run a TUI with a synchronous submit callback.  Callback errors become
/// visible transcript entries and do not strand the terminal in raw mode.
pub fn run_interactive<F, E>(config: TuiConfig, on_submit: F) -> Result<(), TuiError>
where
    F: FnMut(String, &mut TuiState) -> Result<(), E>,
    E: Display,
{
    let state = TuiState::new(config.max_messages);
    run_with_state(config, state, on_submit)
}

/// Same loop with caller-provided state, used to restore a session transcript
/// before entering the alternate screen.
pub fn run_with_state<F, E>(
    config: TuiConfig,
    state: TuiState,
    on_submit: F,
) -> Result<(), TuiError>
where
    F: FnMut(String, &mut TuiState) -> Result<(), E>,
    E: Display,
{
    run_with_state_controls(config, state, false, on_submit)
}

// Only the Agent-backed synchronous host may route queue commands through its
// callback. Generic prompt callbacks must never receive control-plane input.
fn run_with_state_controls<F, E>(
    config: TuiConfig,
    mut state: TuiState,
    input_controls: bool,
    mut on_submit: F,
) -> Result<(), TuiError>
where
    F: FnMut(String, &mut TuiState) -> Result<(), E>,
    E: Display,
{
    let mut editor = ExternalEditorHost::capture_startup();
    let mut guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    // Public config fields can be constructed directly, so normalize both
    // intervals at the loop boundary instead of relying on CLI defaults.
    let frame_interval = config.frame_interval.max(MIN_LOOP_INTERVAL);
    let poll_interval = config.poll_interval.max(MIN_LOOP_INTERVAL);
    let mut scheduler = RenderScheduler::new(frame_interval);
    let mut resize_pending = false;
    let mut last_tick = Instant::now();

    let loop_result = (|| -> Result<(), TuiError> {
        'outer: loop {
            if guard.termination_requested() {
                break;
            }
            if editor.poll(&mut state, &mut guard, &mut terminal, None)? {
                scheduler.request();
                resize_pending = false;
            }
            if editor.active() {
                std::thread::sleep(poll_interval.min(Duration::from_millis(20)));
                continue;
            }
            let now = Instant::now();
            state.flush_ordinary_paste(now);
            if state.is_busy() && now.saturating_duration_since(last_tick) >= poll_interval {
                state.tick();
                scheduler.request();
                last_tick = now;
            }
            if state.take_dirty() {
                scheduler.request();
            }
            if scheduler.due(now) {
                if resize_pending {
                    terminal.autoresize()?;
                    resize_pending = false;
                }
                terminal.draw(|frame| state.render(frame, &config.title))?;
                scheduler.rendered(Instant::now());
            }

            let wait = if scheduler.is_dirty() {
                frame_interval
                    .saturating_sub(Instant::now().saturating_duration_since(now))
                    .min(poll_interval)
            } else {
                poll_interval
            };
            let wait = state.ordinary_paste_wait(Instant::now(), wait);
            if !event::poll(wait)? {
                continue;
            }
            let mut processed = 0usize;
            loop {
                let event = event::read()?;
                if matches!(event, Event::Resize(_, _)) {
                    resize_pending = true;
                }
                match state.handle_event(event) {
                    TuiAction::OpenExternalEditor => {
                        editor.start(&mut state, &mut guard, &mut terminal, None)?;
                        scheduler.request();
                        continue 'outer;
                    }
                    TuiAction::Copy(text) => {
                        state.transcript_ux.copy_notice = Some((
                            match write_terminal_clipboard(&text) {
                                Ok(()) => format!("Clipboard sent {} bytes", text.len()),
                                Err(error) => format!("Copy failed: {error}"),
                            },
                            Instant::now(),
                        ));
                        state.dirty = true;
                    }
                    TuiAction::RespondApproval { .. } => {
                        state.push_message(
                            MessageRole::Error,
                            "Approval requires the production owner host",
                        );
                    }
                    TuiAction::InspectResource { .. } => {
                        state.set_status(
                            "Resource source inspection requires the production owner host",
                        );
                    }
                    TuiAction::OpenSession(_) => {
                        state.push_message(
                        MessageRole::Error,
                        "session cannot be opened in the synchronous TUI host; use the production TUI",
                    );
                        state.set_status("Session open unavailable");
                    }
                    TuiAction::Submit(text)
                        if input_controls && slash::input_queue_control(&text).is_some() =>
                    {
                        state.push_message(MessageRole::User, &text);
                        if let Err(error) = on_submit(text, &mut state) {
                            state.push_message(MessageRole::Error, error.to_string());
                            state.set_status("Input control failed");
                        }
                    }
                    // ZS1-148: the arch console belongs to the master session.
                    // The synchronous host has no shell owner, but it can still
                    // forward a steering instruction through `on_submit` and it
                    // must report a bash rejection instead of faking success.
                    TuiAction::SubmitArch(command) => match command {
                        MasterSessionCommand::Bash(_) => {
                            state.complete_master_turn(
                                MessageRole::Error,
                                "local shell unavailable in the synchronous TUI host",
                            );
                            state.set_status("Arch bash unavailable");
                        }
                        MasterSessionCommand::Steer(prompt) => {
                            state.set_busy(true);
                            state.set_status("Working");
                            let result = on_submit(prompt.clone(), &mut state);
                            match result {
                                Ok(()) => state.complete_master_turn(
                                    MessageRole::System,
                                    format!("arch steer applied: {prompt}"),
                                ),
                                Err(error) => state
                                    .complete_master_turn(MessageRole::Error, error.to_string()),
                            }
                            state.set_busy(false);
                        }
                    },
                    TuiAction::Submit(text) => match slash::route_input(&text) {
                        Err(error) => {
                            state.push_message(MessageRole::User, &text);
                            state.push_message(MessageRole::Error, error.to_string());
                            state.set_status("Command rejected");
                        }
                        Ok(InputRoute::Slash(command)) => {
                            state.push_message(MessageRole::User, &text);
                            match dispatch_slash_command(command, &mut state, None) {
                                SlashDispatchAction::Quit => break 'outer,
                                SlashDispatchAction::Interrupt => {
                                    state.set_busy(false);
                                    state.set_status("Interrupted");
                                }
                                SlashDispatchAction::Continue => {}
                            }
                        }
                        Ok(InputRoute::UserShell(_command)) => {
                            state.push_message(
                            MessageRole::Error,
                            "local shell is unavailable in the synchronous TUI host; use the production TUI",
                        );
                            state.set_status("Local shell unavailable");
                        }
                        Ok(InputRoute::Prompt(prompt)) => {
                            state.push_message(MessageRole::User, &prompt);
                            state.set_busy(true);
                            state.set_status("Working");
                            if let Err(error) = on_submit(prompt, &mut state) {
                                state.push_message(MessageRole::Error, error.to_string());
                                state.set_status("Request failed");
                            }
                            state.set_busy(false);
                        }
                    },
                    TuiAction::Quit => break 'outer,
                    TuiAction::Interrupt => {
                        state.set_busy(false);
                        state.set_status("Interrupted");
                    }
                    TuiAction::Redraw | TuiAction::None => {}
                }
                scheduler.request();
                processed += 1;
                if processed >= 256 || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        Ok(())
    })();
    editor.shutdown(&mut state);
    guard.leave();
    loop_result
}

struct ExternalEditorHost {
    #[cfg(unix)]
    command: Result<crate::config::EditorCommand, String>,
    #[cfg(unix)]
    current: Option<RunningExternalEditor>,
}
#[cfg(unix)]
struct RunningExternalEditor {
    token: ExternalEditToken,
    owner: Option<Arc<Mutex<crate::core::Agent>>>,
    session: crate::external_editor::EditorSession,
}
impl ExternalEditorHost {
    fn capture_startup() -> Self {
        Self {
            #[cfg(unix)]
            command: crate::config::resolve_editor_command(&std::env::vars_os().collect()),
            #[cfg(unix)]
            current: None,
        }
    }
    fn active(&self) -> bool {
        #[cfg(unix)]
        {
            self.current.is_some()
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
    fn start(
        &mut self,
        state: &mut TuiState,
        guard: &mut TerminalGuard,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        owner: Option<&Arc<Mutex<crate::core::Agent>>>,
    ) -> Result<(), TuiError> {
        #[cfg(not(unix))]
        {
            let _ = (guard, terminal, owner);
            state.set_status("External editor is unavailable on this platform");
        }
        #[cfg(unix)]
        {
            if guard.original_terminal.is_none() {
                state.set_status("External editor requires the foreground terminal on stdin");
                return Ok(());
            }
            let token = match state.begin_external_edit() {
                Ok(token) => token,
                Err(error) => {
                    state.set_status(error);
                    return Ok(());
                }
            };
            let prepared = self
                .command
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|command| {
                    crate::external_editor::PreparedEditor::new(
                        state.input(),
                        command,
                        &state.project_cwd(),
                    )
                });
            let prepared = match prepared {
                Ok(prepared) => prepared,
                Err(error) => {
                    let error = state.finish_external_edit(token, Err(error)).unwrap_err();
                    state.set_status(error);
                    return Ok(());
                }
            };
            if let Err(error) = guard.suspend_editor(event::reset_event_reader) {
                let error = prepared.reject(format!("Editor terminal suspension failed: {error}"));
                let _ = state.finish_external_edit(token, Err(error.clone()));
                return Err(io::Error::other(error).into());
            }
            match crate::external_editor::EditorSession::start(prepared) {
                Ok(session) => {
                    self.current = Some(RunningExternalEditor {
                        token,
                        session,
                        owner: owner.map(Arc::clone),
                    });
                }
                Err(error) => {
                    let error = state.finish_external_edit(token, Err(error)).unwrap_err();
                    state.set_status(error);
                    guard.resume_editor(event::reset_event_reader, terminal)?;
                }
            }
        }
        Ok(())
    }
    fn poll(
        &mut self,
        state: &mut TuiState,
        guard: &mut TerminalGuard,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        owner: Option<&Arc<Mutex<crate::core::Agent>>>,
    ) -> Result<bool, TuiError> {
        #[cfg(not(unix))]
        {
            let _ = (state, guard, terminal, owner);
        }
        #[cfg(unix)]
        {
            let Some(result) = self.current.as_mut().and_then(|edit| edit.session.poll()) else {
                return Ok(false);
            };
            let edit = self.current.take().unwrap();
            guard.resume_editor(event::reset_event_reader, terminal)?;
            if edit.session.fatal_failure() {
                let message = result
                    .err()
                    .unwrap_or_else(|| "Editor cleanup failed".into());
                let _ = state.finish_external_edit(edit.token, Err(message.clone()));
                return Err(io::Error::other(message).into());
            }
            let same_owner = match (edit.owner.as_ref(), owner) {
                (Some(original), Some(current)) => Arc::ptr_eq(original, current),
                (None, None) => true,
                _ => false,
            };
            let result = if same_owner {
                result
            } else {
                Err(
                    "External edit not applied: session owner changed; current draft retained"
                        .into(),
                )
            };
            match state.finish_external_edit(edit.token, result) {
                Ok(true) => state.set_status("External edit applied · Enter explicitly to send"),
                Ok(false) => state.set_status("External edit unchanged · draft preserved"),
                Err(error) => state.set_status(error),
            }
            return Ok(true);
        }
        #[allow(unreachable_code)]
        Ok(false)
    }
    fn shutdown(&mut self, state: &mut TuiState) {
        #[cfg(unix)]
        if let Some(mut edit) = self.current.take() {
            let error = edit
                .session
                .shutdown()
                .err()
                .unwrap_or_else(|| "External edit cancelled".into());
            let _ = state.finish_external_edit(edit.token, Err(error.clone()));
            eprintln!("{error}");
        }
        #[cfg(not(unix))]
        let _ = state;
    }
}

struct TerminalGuard {
    active: bool,
    #[cfg(unix)]
    original_terminal: Option<crate::external_editor::TerminalState>,
    #[cfg(unix)]
    signals: Option<terminal_signals::SignalGuard>,
}

impl TerminalGuard {
    fn enter() -> Result<Self, TuiError> {
        #[cfg(unix)]
        let signals = terminal_signals::SignalGuard::install()?;
        #[cfg(unix)]
        // A redirected stdin can still use crossterm's /dev/tty fallback.
        // Only the explicit editor request requires actual foreground stdin.
        let original_terminal = crate::external_editor::TerminalState::capture().ok();
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(
            stdout,
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableMouseCapture,
            Hide
        ) {
            let _ = execute!(
                stdout,
                Show,
                DisableMouseCapture,
                DisableBracketedPaste,
                LeaveAlternateScreen
            );
            let _ = disable_raw_mode();
            return Err(TuiError::Io(error));
        }
        Ok(Self {
            active: true,
            #[cfg(unix)]
            original_terminal,
            #[cfg(unix)]
            signals: Some(signals),
        })
    }

    // The caller has exclusive event ownership. The reset callback must clear
    // both decoded events and partial parser bytes; tcflush alone is insufficient.
    #[cfg(unix)]
    fn suspend_editor(&mut self, reset_reader: fn() -> io::Result<()>) -> Result<(), TuiError> {
        let original = self
            .original_terminal
            .as_ref()
            .ok_or_else(|| io::Error::other("External editor requires foreground stdin"))?;
        reset_reader()?;
        original.flush()?;
        self.active = false;
        let screen = execute!(
            io::stdout(),
            Show,
            DisableMouseCapture,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let raw = disable_raw_mode();
        let original = original.restore(true);
        screen.and(raw).and(original)?;
        Ok(())
    }

    #[cfg(unix)]
    fn resume_editor(
        &mut self,
        reset_reader: fn() -> io::Result<()>,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<(), TuiError> {
        let original = self
            .original_terminal
            .as_ref()
            .ok_or_else(|| io::Error::other("External editor requires foreground stdin"))?;
        original.reclaim()?;
        reset_reader()?;
        original.flush()?;
        original.restore(false)?;
        enable_raw_mode()?;
        // Mark active before writes so a partial output failure still runs leave.
        self.active = true;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableMouseCapture,
            Hide
        )?;
        // Fullscreen resize invalidates both buffers without a cursor-position
        // query (clear() queries the terminal and can consume handoff input).
        terminal.resize(terminal.size()?.into())?;
        Ok(())
    }

    fn termination_requested(&self) -> bool {
        #[cfg(unix)]
        {
            self.signals
                .as_ref()
                .is_some_and(terminal_signals::SignalGuard::requested)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    fn leave(&mut self) {
        if self.active {
            let mut stdout = io::stdout();
            let _ = execute!(
                stdout,
                Show,
                DisableMouseCapture,
                DisableBracketedPaste,
                LeaveAlternateScreen
            );
            let _ = disable_raw_mode();
            self.active = false;
        }
        #[cfg(unix)]
        {
            if let Some(original) = &self.original_terminal {
                let _ = original.reclaim();
                let _ = original.restore(false);
            }
            drop(self.signals.take());
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.leave();
    }
}

#[cfg(unix)]
mod terminal_signals {
    use std::io;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    static OWNED: AtomicBool = AtomicBool::new(false);
    static SIGNAL: AtomicI32 = AtomicI32::new(0);

    // The handler performs no allocation, locks, I/O, or terminal work. The
    // normal bounded poll loop observes this flag and runs owned cleanup.
    extern "C" fn request_stop(signal: libc::c_int) {
        SIGNAL.store(signal, Ordering::Relaxed);
    }

    pub(super) struct SignalGuard {
        previous: Vec<(libc::c_int, libc::sigaction)>,
    }

    impl SignalGuard {
        pub(super) fn install() -> io::Result<Self> {
            if OWNED
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "a TUI already owns terminal signal handling",
                ));
            }
            SIGNAL.store(0, Ordering::Relaxed);
            let mut guard = Self {
                previous: Vec::with_capacity(4),
            };
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
                // Both structures are fully initialized before the OS reads
                // them; libc writes the previous disposition on success.
                let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
                let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
                action.sa_sigaction = request_stop as *const () as usize;
                action.sa_flags = libc::SA_RESTART;
                unsafe {
                    libc::sigemptyset(&mut action.sa_mask);
                }
                if unsafe { libc::sigaction(signal, &action, &mut previous) } != 0 {
                    return Err(io::Error::last_os_error());
                }
                guard.previous.push((signal, previous));
            }
            Ok(guard)
        }

        pub(super) fn requested(&self) -> bool {
            SIGNAL.load(Ordering::Relaxed) != 0
        }
    }

    impl Drop for SignalGuard {
        fn drop(&mut self) {
            for (signal, previous) in self.previous.iter().rev() {
                // Restore only dispositions installed by this scoped owner.
                unsafe {
                    libc::sigaction(*signal, previous, std::ptr::null_mut());
                }
            }
            OWNED.store(false, Ordering::Release);
        }
    }
}

/// Project agent lifecycle events into the compact terminal transcript.  The
/// production loop receives provider deltas through a separate queue (so it
/// never locks the agent while network I/O is in progress), then drains these
/// durable tool events once the worker releases the agent mutex.
fn drain_agent_tool_events(shared: &Arc<Mutex<crate::core::Agent>>, state: &mut TuiState) {
    let Ok(mut agent) = shared.try_lock() else {
        return;
    };
    for event in agent.take_events() {
        apply_agent_tool_event(state, event);
    }
    let request = crate::protocol::InputQueueRequest {
        schema_version: crate::protocol::PROTOCOL_VERSION,
        id: "tui-refresh-inputs".into(),
        kind: "input_queue".into(),
        session_id: agent.session().session_id().to_owned(),
        input_queue: crate::protocol::InputQueueAction::List {
            after_sequence: None,
            limit: 32,
        },
    };
    if let Ok(reply) = agent.input_queue_request(request) {
        state.update_input_queue(&reply);
    }
    state.refresh_session_snapshot(agent.session());
    state.refresh_transcript_status(&agent);
}

fn apply_agent_tool_event(state: &mut TuiState, event: crate::core::AgentEvent) {
    match event {
        crate::core::AgentEvent::ToolCall { call_id, tool, .. } => {
            state.tool_call_started(call_id, tool)
        }
        crate::core::AgentEvent::ToolResult {
            call_id, success, ..
        } => state.tool_call_finished(
            call_id,
            if success {
                ToolRunStatus::Succeeded
            } else {
                ToolRunStatus::Failed
            },
        ),
        _ => {}
    }
}

fn drain_tui_provider_events(
    state: &mut TuiState,
    job_id: u64,
    events: &Arc<Mutex<TuiProviderEventBuffer>>,
) -> bool {
    let Ok(mut events) = events.lock() else {
        return false;
    };
    let (events, dropped) = events.take();
    let mut changed = false;
    let mut view_sequence = 0_u64;
    for event in events {
        changed = true;
        let event = match event {
            TuiStreamEvent::Agent(event) => {
                if let Ok(view) =
                    crate::view_model::ViewEvent::from_agent_event(view_sequence, None, &event)
                {
                    view_sequence = view_sequence.saturating_add(1);
                    state.apply_view_event_for_job(job_id, &view);
                }
                continue;
            }
            TuiStreamEvent::Provider(event) => event,
        };
        if state.streaming_job_id == Some(job_id)
            && let crate::backend::ProviderEvent::ResponseCreated {
                model: Some(model), ..
            }
            | crate::backend::ProviderEvent::Completed {
                model: Some(model), ..
            } = &event
        {
            state.transcript_ux.response_model = Some(model.clone());
        }
        // One core turn can make several provider requests around tool calls.
        // A provider completion is not the owning runtime's terminal event.
        if matches!(event, crate::backend::ProviderEvent::Completed { .. }) {
            continue;
        }
        if let Ok(view) = crate::view_model::ViewEvent::from_provider_event(
            view_sequence,
            None,
            Some(&format!("job-{job_id}")),
            None,
            &event,
        ) {
            view_sequence = view_sequence.saturating_add(1);
            state.apply_view_event_for_job(job_id, &view);
        }
        if let crate::backend::ProviderEvent::Warning { message } = event {
            state.push_message(MessageRole::System, message);
        }
    }
    if dropped > 0 {
        // A bounded mailbox may have to discard deltas when a terminal is
        // slower than the provider. Never hide that loss from the operator;
        // the final ProcessResult remains authoritative for the answer, while
        // this transcript entry explains why live rendering may be incomplete.
        state.push_message(
            MessageRole::Error,
            format!(
                "Provider stream truncated: {dropped} event(s) dropped; final response remains authoritative"
            ),
        );
        state.set_status("Provider stream truncated");
        changed = true;
    }
    changed
}

/// Encode an explicit user copy request. OSC52 payload is base64, so source
/// control bytes cannot become terminal commands. No clipboard is touched by rendering.
pub fn terminal_clipboard_sequence(text: &str) -> Result<String, String> {
    use base64::Engine;
    if text.len() > MAX_CLIPBOARD_BYTES {
        return Err("Selection exceeds 64 KiB; choose a smaller block".into());
    }
    Ok(format!(
        "\x1b]52;c;{}\x07",
        base64::engine::general_purpose::STANDARD.encode(text.as_bytes())
    ))
}
fn write_terminal_clipboard(text: &str) -> Result<(), String> {
    use std::io::Write;
    let sequence = terminal_clipboard_sequence(text)?;
    let mut out = io::stdout().lock();
    out.write_all(sequence.as_bytes())
        .and_then(|_| out.flush())
        .map_err(|e| e.to_string())
}

fn bound_text(text: String) -> String {
    truncate_bytes(&text, MAX_MESSAGE_BYTES).to_owned()
}

fn slash_common_prefix(candidates: &[&'static str]) -> String {
    let Some(first) = candidates.first() else {
        return String::new();
    };
    let mut prefix = first.to_string();
    for candidate in candidates.iter().skip(1) {
        let shared = prefix
            .chars()
            .zip(candidate.chars())
            .take_while(|(left, right)| left.eq_ignore_ascii_case(right))
            .count();
        prefix = prefix.chars().take(shared).collect();
        if prefix.is_empty() {
            break;
        }
    }
    prefix
}

/// Keep pasted prompt text from emitting terminal control sequences. Newlines
/// remain available for multiline editing, tabs become deterministic spaces,
/// and all other control characters are rendered as a visible marker.
fn sanitize_input(text: String) -> String {
    sanitize_input_with_limit(&text, MAX_MESSAGE_BYTES)
}

fn sanitize_input_with_limit(text: &str, max_bytes: usize) -> String {
    if !text.chars().any(char::is_control) {
        return truncate_bytes(text, max_bytes).to_owned();
    }
    let mut result = String::with_capacity(text.len().min(max_bytes));
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        let replacement = match character {
            '\n' => "\n",
            '\r' if characters.peek() == Some(&'\n') => "",
            '\r' => "\n",
            '\t' => "    ",
            character if character.is_control() => "?",
            character => {
                let mut buffer = [0_u8; 4];
                let encoded = character.encode_utf8(&mut buffer);
                if result.len().saturating_add(encoded.len()) > max_bytes {
                    break;
                }
                result.push_str(encoded);
                continue;
            }
        };
        if result.len().saturating_add(replacement.len()) > max_bytes {
            break;
        }
        result.push_str(replacement);
    }
    result
}

/// Keep provider-controlled identifiers and tool names on one terminal line.
/// Control characters are removed rather than emitted, so a malformed model
/// response cannot move the cursor or spoof transcript rows.
fn inline_token(text: &str, max_chars: usize) -> String {
    let mut result = String::new();
    let mut count = 0usize;
    for character in text.chars() {
        if count >= max_chars {
            break;
        }
        if !character.is_control() {
            result.push(character);
            count = count.saturating_add(1);
        }
    }
    if result.is_empty() {
        "unknown".into()
    } else {
        result
    }
}

fn tool_log_key(call_id: &str) -> String {
    format!("[tool:{call_id}]")
}

fn tool_log_matches(text: &str, key: &str) -> bool {
    text.strip_prefix(key)
        .is_some_and(|rest| rest.starts_with(' '))
}

fn tool_log_name(text: &str, key: &str) -> Option<String> {
    let rest = text.strip_prefix(key)?.trim_start();
    let name = rest.rsplit_once(" [")?.0;
    (!name.is_empty()).then(|| name.to_owned())
}

fn truncate_bytes(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn clamp_char_boundary(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn line_start(text: &str, cursor: usize) -> usize {
    let cursor = clamp_char_boundary(text, cursor);
    text[..cursor].rfind('\n').map_or(0, |index| index + 1)
}

fn line_end(text: &str, cursor: usize) -> usize {
    let cursor = clamp_char_boundary(text, cursor);
    text[cursor..]
        .find('\n')
        .map_or(text.len(), |offset| cursor + offset)
}

/// Return a UTF-8 boundary in `line` at (or immediately before) a display
/// column.  A wide glyph is never split: a target column that lands inside
/// it resolves to the glyph's leading boundary.
fn byte_at_column(line: &str, target_column: usize) -> usize {
    if target_column == 0 {
        return 0;
    }
    let mut column = 0usize;
    for (index, grapheme) in line.grapheme_indices(true) {
        let width = UnicodeWidthStr::width(grapheme);
        if column.saturating_add(width) > target_column {
            return index;
        }
        column = column.saturating_add(width);
        if column >= target_column {
            return index + grapheme.len();
        }
    }
    line.len()
}

fn previous_boundary(text: &str, cursor: usize) -> usize {
    text.grapheme_indices(true)
        .map(|(index, _)| index)
        .take_while(|index| *index < cursor)
        .last()
        .unwrap_or(0)
}

fn next_boundary(text: &str, cursor: usize) -> usize {
    text.grapheme_indices(true)
        .map(|(index, _)| index)
        .find(|index| *index > cursor)
        .unwrap_or(text.len())
}

// Menu paths are projections of the resource owner's unchanged full source.
// Keep the filename and trailing directories visible, even when a workspace
// path is longer than the cached metadata or the available terminal cells.
fn menu_source(text: &str, width: usize) -> String {
    let text = inline_token(text, usize::MAX);
    if UnicodeWidthStr::width(text.as_str()) <= width {
        return text;
    }
    if width == 0 {
        return String::new();
    }
    let available = width - 1;
    let basename = text.rsplit('/').next().unwrap_or(&text);
    let minimum_tail = UnicodeWidthStr::width(basename).min(available);
    let prefix = menu_prefix(&text, (available / 4).min(available - minimum_tail));
    let tail_budget = available - UnicodeWidthStr::width(prefix.as_str());
    let mut used = 0;
    let mut start = text.len();
    for (offset, grapheme) in text.grapheme_indices(true).rev() {
        let cells = UnicodeWidthStr::width(grapheme);
        if used + cells > tail_budget {
            break;
        }
        used += cells;
        start = offset;
    }
    format!("{prefix}…{}", &text[start..])
}

fn menu_prefix(text: &str, width: usize) -> String {
    let text = inline_token(text, usize::MAX);
    let mut result = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let cells = UnicodeWidthStr::width(grapheme);
        if used + cells > width {
            break;
        }
        used += cells;
        result.push_str(grapheme);
    }
    result
}

fn truncate_to_width(text: &str, width: usize) -> String {
    let mut result = String::new();
    let mut used = 0usize;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used.saturating_add(character_width) > width {
            break;
        }
        used += character_width;
        result.push(character);
    }
    result
}

fn wrap_plain(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for logical in text.split('\n') {
        if logical.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        let mut used = 0usize;
        for character in logical.graphemes(true) {
            let character_width = UnicodeWidthStr::width(character);
            // A terminal cell cannot contain a glyph wider than the current
            // viewport (this occurs transiently while resizing to one column).
            // Keep the line width bounded and leave a visible marker instead
            // of allowing an over-wide first glyph to spill into the border.
            if character_width > width {
                if used > 0 {
                    lines.push(std::mem::take(&mut line));
                }
                line.push('?');
                used = 1;
                continue;
            }
            if !line.is_empty() && used.saturating_add(character_width) > width {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            line.push_str(character);
            used = used.saturating_add(character_width);
        }
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn transcript_lines(
    messages: &VecDeque<TuiMessage>,
    width: usize,
    persona: &str,
) -> Vec<Line<'static>> {
    transcript_window(
        messages
            .iter()
            .enumerate()
            .map(|(id, message)| (id as u64, message)),
        width,
        persona,
    )
    .into_iter()
    .map(|row| row.line)
    .collect()
}

/// Build a recent display window without rendering the entire oldest prefix.
/// Source positions identify rows even when the retained window moves forward.
fn transcript_window<'a>(
    messages: impl DoubleEndedIterator<Item = (u64, &'a TuiMessage)>,
    width: usize,
    persona: &str,
) -> Vec<TranscriptRow> {
    let width = width.max(1);
    let max_lines = MAX_RENDER_LINES
        .min(crate::render::MAX_MARKDOWN_CELLS / width)
        .max(1);
    let mut result = VecDeque::new();
    let mut messages = messages.rev().peekable();
    let mut omitted = false;
    while let Some((id, message)) = messages.next() {
        let prefix = if message.role == MessageRole::Reasoning {
            "◇ Reasoning: "
        } else {
            "● "
        };
        let style = if message.role == MessageRole::Assistant {
            Style::default().fg(crate::persona::color(persona))
        } else {
            message.role.style()
        };
        // Provider prose gets the small Markdown renderer; user prompts,
        // tool output, and errors stay literal so metadata or diff markers do
        // not acquire surprising presentation semantics.
        // Live output text is already the complete literal projection of its
        // canonical block; retain that block for inspection without printing
        // the same snapshot twice in the conversation pane.
        let block_text = if message.blocks.is_empty() || message.block_id.is_some() {
            None
        } else {
            Some(
                message
                    .blocks
                    .iter()
                    .map(view_block_text)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        };
        let display_text = block_text
            .as_deref()
            .map(|blocks| format!("{}\n{}", message.text, blocks))
            .unwrap_or_else(|| message.text.clone());
        let rendered = match message.role {
            MessageRole::Assistant | MessageRole::System => {
                crate::render::render_markdown_tail_prefixed_with_metadata(
                    prefix,
                    &display_text,
                    width,
                    style,
                )
            }
            MessageRole::User | MessageRole::Tool | MessageRole::Error | MessageRole::Reasoning => {
                crate::render::render_plain_tail_prefixed_with_metadata(
                    prefix,
                    &display_text,
                    width,
                    style,
                )
            }
        };
        let remaining = max_lines.saturating_sub(result.len());
        let skipped = rendered.lines.len().saturating_sub(remaining);
        let omitted_visual_lines = rendered.omitted_visual_lines;
        omitted |= omitted_visual_lines > 0;
        for (line_index, line) in rendered.lines.into_iter().enumerate().rev().take(remaining) {
            result.push_front(TranscriptRow {
                line,
                // Keep the identity in the message's full visual layout as
                // its retained tail advances during streaming at this width.
                position: Some((id, omitted_visual_lines + line_index)),
            });
        }
        if result.len() == max_lines {
            omitted |= skipped > 0 || messages.peek().is_some();
            break;
        }
    }
    if omitted {
        result.pop_front();
        result.push_front(TranscriptRow {
            line: Line::from(Span::styled(
                truncate_to_width("[Earlier rows omitted from this view]", width),
                Style::default().fg(Color::DarkGray),
            )),
            position: None,
        });
    }
    result.into_iter().collect()
}

fn view_block_text(block: &crate::view_model::ViewBlock) -> String {
    match block {
        crate::view_model::ViewBlock::Diff { path, patch } => {
            format!(
                "Proposed change:\n[diff {}]\n{}",
                path.as_deref().unwrap_or("."),
                patch
            )
        }
        crate::view_model::ViewBlock::Code { language, text } => format!(
            "[code{}]\n{}",
            language
                .as_deref()
                .map(|value| format!(" {value}"))
                .unwrap_or_default(),
            text
        ),
        crate::view_model::ViewBlock::Error { code, message, .. } => format!(
            "[error{}] {}",
            code.as_deref()
                .map(|value| format!(" {value}"))
                .unwrap_or_default(),
            message
        ),
        crate::view_model::ViewBlock::Paragraph { text }
        | crate::view_model::ViewBlock::PlainText { text }
        | crate::view_model::ViewBlock::Quote { text }
        | crate::view_model::ViewBlock::Heading { text, .. } => text.clone(),
        crate::view_model::ViewBlock::List { items, .. } => items.join("\n"),
        crate::view_model::ViewBlock::ToolStatus {
            call_id,
            name,
            status,
            output,
        } => format!(
            "[tool {} {} {:?}]{}",
            name,
            call_id,
            status,
            output
                .as_deref()
                .map(|value| format!("\n{value}"))
                .unwrap_or_default()
        ),
        crate::view_model::ViewBlock::Approval {
            approval_id,
            tool,
            state,
            ..
        } => format!("[approval {tool} {approval_id} {:?}]", state),
        crate::view_model::ViewBlock::Rule => "────────".into(),
    }
}

fn stream_name(stream: crate::view_model::ViewStream) -> &'static str {
    match stream {
        crate::view_model::ViewStream::Provider => "Provider",
        crate::view_model::ViewStream::Agent => "Agent",
        crate::view_model::ViewStream::Terminal => "Terminal",
        crate::view_model::ViewStream::Control => "Control",
    }
}

fn cursor_position(text: &str, cursor: usize, width: usize) -> (u16, u16) {
    let width = width.max(1);
    let cursor = clamp_char_boundary(text, cursor);
    let mut x = 0usize;
    let mut y = 0usize;
    for character in text[..cursor.min(text.len())].graphemes(true) {
        if character == "\n" {
            x = 0;
            y = y.saturating_add(1);
            continue;
        }
        let character_width = UnicodeWidthStr::width(character);
        // Match `wrap_plain`: represent a glyph wider than the viewport with
        // one cell so the cursor follows the rendered text rather than
        // drifting past the right edge during a narrow resize.
        let character_width = if character_width > width {
            1
        } else {
            character_width
        };
        if x > 0 && x.saturating_add(character_width) > width {
            x = 0;
            y = y.saturating_add(1);
        }
        x = x.saturating_add(character_width);
    }
    (
        u16::try_from(x).unwrap_or(u16::MAX),
        u16::try_from(y).unwrap_or(u16::MAX),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn file_completion_scan_limits_and_cancel_are_deterministic() {
        use std::cell::Cell;
        let root = tempfile::tempdir().unwrap();
        for index in 0..2049 {
            std::fs::write(root.path().join(format!("file-{index:04}")), "").unwrap();
        }
        let mut query = FileCompletionQuery {
            root: root.path().into(),
            input: "/attach file-".into(),
            prefix: "/attach ".into(),
            path: "file-".into(),
            raw_selection: false,
        };
        let polls = Cell::new(0);
        let capped = complete_file_query_with_clock(
            &query,
            || false,
            |_| {
                polls.set(polls.get() + 1);
                Duration::ZERO
            },
        )
        .unwrap();
        assert_eq!(polls.get(), 2048, "never scan beyond the existing cap");
        assert_eq!(capped.scan, FileCompletionScan::EntryLimit);
        assert!(capped.display_limited);
        assert_eq!(capped.entries.len(), 128);
        std::fs::remove_file(root.path().join("file-2048")).unwrap();
        let exact_cap =
            complete_file_query_with_clock(&query, || false, |_| Duration::ZERO).unwrap();
        assert_eq!(
            exact_cap.scan,
            FileCompletionScan::EntryLimit,
            "at exactly the cap, exhaustion has not been established"
        );
        query.path = "not-scanned-match".into();
        let empty = complete_file_query_with_clock(&query, || false, |_| Duration::ZERO).unwrap();
        assert_eq!(empty.scan, FileCompletionScan::EntryLimit);
        assert!(!empty.display_limited);
        assert!(empty.entries.is_empty());
        assert!(
            empty
                .notice()
                .unwrap()
                .1
                .contains("No matches in scanned entries")
        );
        assert!(
            !empty
                .notice()
                .unwrap()
                .0
                .contains("No matching files in this directory")
        );

        query.path = "file-".into();
        let ticks = Cell::new(0);
        let timed = complete_file_query_with_clock(
            &query,
            || false,
            |_| {
                ticks.set(ticks.get() + 1);
                Duration::from_millis(if ticks.get() == 1 { 150 } else { 151 })
            },
        )
        .unwrap();
        assert_eq!(timed.scan, FileCompletionScan::TimeLimit);
        assert_eq!(timed.entries.len(), 1, "150ms stays allowed;151ms stops");
        let timed_empty =
            complete_file_query_with_clock(&query, || false, |_| Duration::from_millis(151))
                .unwrap();
        assert!(timed_empty.entries.is_empty());
        assert_eq!(timed_empty.scan, FileCompletionScan::TimeLimit);
        assert!(complete_file_query_with_clock(&query, || true, |_| Duration::ZERO).is_err());
        let cancels = Cell::new(0);
        let late = complete_file_query_with_clock(
            &query,
            || {
                cancels.set(cancels.get() + 1);
                cancels.get() > 2049
            },
            |_| Duration::ZERO,
        );
        assert_eq!(cancels.get(), 2050);
        assert_eq!(late.unwrap_err(), "File completion cancelled");
    }

    #[test]
    fn file_completion_exhaustion_and_display_limit_are_separate() {
        let root = tempfile::tempdir().unwrap();
        let query = FileCompletionQuery {
            root: root.path().into(),
            input: "@".into(),
            prefix: "@".into(),
            path: String::new(),
            raw_selection: false,
        };
        let empty = complete_file_query_with_clock(&query, || false, |_| Duration::ZERO).unwrap();
        assert_eq!(empty.scan, FileCompletionScan::Complete);
        assert!(!empty.display_limited);
        assert_eq!(
            empty.notice().unwrap().0,
            "No matching files in this directory"
        );
        for index in 0..128 {
            std::fs::write(root.path().join(format!("file-{index:03}")), "").unwrap();
        }
        let exact = complete_file_query_with_clock(&query, || false, |_| Duration::ZERO).unwrap();
        assert_eq!(exact.scan, FileCompletionScan::Complete);
        assert_eq!(exact.entries.len(), 128);
        assert!(!exact.display_limited);
        assert!(exact.notice().is_none());
        std::fs::write(root.path().join("file-128"), "").unwrap();
        let clipped = complete_file_query_with_clock(&query, || false, |_| Duration::ZERO).unwrap();
        assert_eq!(clipped.scan, FileCompletionScan::Complete);
        assert!(clipped.display_limited);
        assert_eq!(clipped.entries.len(), 128);
        assert!(clipped.notice().unwrap().0.contains("display limit"));
    }

    fn approval_timer_request(id: &str) -> crate::approval::ApprovalRequest {
        crate::approval::ApprovalRequest {
            request_id: id.into(),
            turn_id: "timer-turn".into(),
            call_id: format!("call-{id}"),
            tool: "write_file".into(),
            side_effect: crate::tools::ToolSideEffect::WorkspaceWrite,
            arguments: serde_json::json!({"path":"unused.txt"}),
            preview: None,
            origin: crate::tools::ToolOrigin::AgentTool,
            policy_digest: None,
            lease_id: None,
        }
    }

    #[test]
    fn approval_timer_pending_request_pauses_without_status_text() {
        let mut s = TuiState::default();
        s.set_busy(true);
        s.transcript_ux.started = Some(Instant::now() - Duration::from_secs(5));
        s.present_approval(approval_timer_request("one"));
        assert!(s.transcript_ux.started.is_none());
        assert!(s.transcript_ux.elapsed >= Duration::from_secs(5));
    }

    #[test]
    fn approval_timer_pending_survives_status_and_hidden_modal() {
        let mut s = TuiState::default();
        s.set_busy(true);
        s.present_approval(approval_timer_request("one"));
        s.set_status("Approval required");
        s.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        let elapsed = s.transcript_ux.elapsed;
        s.set_status("Draft kept");
        assert!(s.transcript_ux.started.is_none());
        assert_eq!(s.transcript_ux.elapsed, elapsed);
        assert_eq!(s.approval_count(), 1);
    }

    #[test]
    fn approval_timer_last_retire_resumes_without_tool_or_text_event() {
        let mut s = TuiState::default();
        s.set_busy(true);
        s.present_approval(approval_timer_request("one"));
        s.set_status("Approval required");
        let elapsed = s.transcript_ux.elapsed;
        s.retire_approval("one");
        assert!(s.transcript_ux.started.is_some());
        assert_eq!(s.transcript_ux.elapsed, elapsed);
    }

    #[test]
    fn approval_timer_approval_before_busy_stays_paused() {
        let mut s = TuiState::default();
        s.present_approval(approval_timer_request("one"));
        s.set_busy(true);
        assert!(s.transcript_ux.started.is_none());
    }

    #[test]
    fn approval_timer_status_words_alone_cannot_pause() {
        let mut s = TuiState::default();
        s.set_busy(true);
        let started = s.transcript_ux.started;
        s.set_status("Approval history copied");
        assert_eq!(s.transcript_ux.started, started);
    }

    #[test]
    fn approval_timer_unknown_and_partial_retire_control() {
        let mut s = TuiState::default();
        s.set_busy(true);
        s.present_approval(approval_timer_request("one"));
        s.present_approval(approval_timer_request("two"));
        s.set_status("Approval required");
        s.retire_approval("unknown");
        s.retire_approval("one");
        assert!(s.transcript_ux.started.is_none());
        assert_eq!(s.approval_count(), 1);
    }

    fn approval_timer_event(s: &mut TuiState, job: u64, kind: crate::view_model::ViewEventKind) {
        let event = crate::view_model::ViewEvent::with_context(
            0,
            None,
            Some("timer-turn".into()),
            None,
            kind,
        )
        .unwrap();
        s.apply_view_event_for_job(job, &event);
    }

    fn approval_timer_required(id: &str) -> crate::view_model::ViewEventKind {
        crate::view_model::ViewEventKind::ApprovalRequired {
            approval_id: id.into(),
            tool: "write_file".into(),
            arguments: "{}".into(),
        }
    }

    fn approval_timer_resolved(id: &str) -> crate::view_model::ViewEventKind {
        crate::view_model::ViewEventKind::ApprovalResolved {
            approval_id: id.into(),
            state: crate::view_model::ApprovalState::Allowed,
        }
    }

    #[test]
    fn approval_timer_canonical_multiple_unknown_pending_and_real_mirror() {
        let mut s = TuiState::default();
        s.begin_stream_for_job(1);
        s.set_busy(true);
        approval_timer_event(&mut s, 1, approval_timer_required("one"));
        approval_timer_event(&mut s, 1, approval_timer_required("one"));
        approval_timer_event(&mut s, 1, approval_timer_required("two"));
        assert_eq!(s.transcript_ux.canonical_approvals.len(), 2);
        assert!(s.transcript_ux.started.is_none());
        approval_timer_event(&mut s, 1, approval_timer_resolved("unknown"));
        approval_timer_event(
            &mut s,
            1,
            crate::view_model::ViewEventKind::ApprovalResolved {
                approval_id: "one".into(),
                state: crate::view_model::ApprovalState::Pending,
            },
        );
        assert_eq!(s.transcript_ux.canonical_approvals.len(), 2);
        approval_timer_event(&mut s, 1, approval_timer_resolved("one"));
        assert!(s.transcript_ux.started.is_none());
        s.present_approval(approval_timer_request("two"));
        approval_timer_event(&mut s, 1, approval_timer_resolved("two"));
        assert!(
            s.transcript_ux.started.is_none(),
            "canonical resolution cannot retire coordinator request"
        );
        s.retire_approval("two");
        assert!(s.transcript_ux.started.is_some());
        s.present_approval(approval_timer_request("three"));
        approval_timer_event(&mut s, 1, approval_timer_required("three"));
        s.retire_approval("three");
        assert!(
            s.transcript_ux.started.is_some(),
            "real retirement clears its mirror without another event"
        );
        approval_timer_event(&mut s, 1, approval_timer_required("four"));
        approval_timer_event(&mut s, 1, approval_timer_resolved("four"));
        assert!(
            s.transcript_ux.started.is_some(),
            "canonical-only last resolution resumes"
        );
    }

    #[test]
    fn approval_timer_stale_jobs_cannot_pause_resume_or_cancel_replacement() {
        let mut s = TuiState::default();
        s.begin_stream_for_job(1);
        s.set_busy(true);
        s.present_approval(approval_timer_request("old"));
        approval_timer_event(&mut s, 1, approval_timer_required("old"));
        s.begin_stream_for_job(2);
        assert_eq!(s.approval_count(), 0);
        assert!(s.transcript_ux.started.is_some());
        assert_eq!(s.transcript_ux.elapsed, Duration::ZERO);
        let started = s.transcript_ux.started;
        approval_timer_event(&mut s, 1, approval_timer_required("late"));
        approval_timer_event(
            &mut s,
            1,
            crate::view_model::ViewEventKind::TurnCancelled { reason: None },
        );
        assert_eq!(s.transcript_ux.started, started);
        assert_eq!(s.streaming_job_id, Some(2));
        approval_timer_event(&mut s, 2, approval_timer_required("current"));
        approval_timer_event(&mut s, 1, approval_timer_resolved("current"));
        assert!(s.transcript_ux.started.is_none());
        assert_eq!(s.transcript_ux.canonical_approvals, ["current"]);
    }

    #[test]
    fn approval_timer_terminal_events_stop_and_next_turn_resets() {
        use crate::view_model::ViewEventKind as V;
        for terminal in [
            V::TurnCancelled { reason: None },
            V::TurnCompleted {
                response_id: None,
                model: None,
            },
            V::TurnFailed {
                code: None,
                message: "fixture".into(),
                retryable: false,
            },
            V::Closed,
        ] {
            let mut s = TuiState::default();
            s.begin_stream_for_job(1);
            s.set_busy(true);
            s.transcript_ux.started = Some(Instant::now() - Duration::from_secs(7));
            s.present_approval(approval_timer_request("one"));
            approval_timer_event(&mut s, 1, approval_timer_required("one"));
            let elapsed = s.transcript_ux.elapsed;
            approval_timer_event(&mut s, 1, terminal);
            assert!(s.transcript_ux.started.is_none());
            assert_eq!(s.approval_count(), 0);
            assert!(s.transcript_ux.canonical_approvals.is_empty());
            s.retire_approval("one");
            s.set_status("Working");
            assert_eq!(s.transcript_ux.elapsed, elapsed);
            assert!(s.transcript_ux.started.is_none());
            s.set_busy(false);
            s.begin_stream_for_job(2);
            s.set_busy(true);
            assert!(s.transcript_ux.started.is_some());
            assert_eq!(s.transcript_ux.elapsed, Duration::ZERO);
        }
    }

    #[test]
    fn approval_timer_admitted_cancel_clears_wait_but_only_terminal_stops_work() {
        let mut s = TuiState::default();
        s.begin_stream_for_job(1);
        s.set_busy(true);
        s.present_approval(approval_timer_request("one"));
        approval_timer_event(&mut s, 1, approval_timer_required("one"));
        s.set_status("Interrupt requested");
        assert!(
            s.transcript_ux.started.is_none(),
            "text alone also covers a rejected/queue-full request"
        );
        s.clear_activity_approvals();
        assert_eq!(s.approval_count(), 0);
        assert!(
            s.transcript_ux.started.is_some(),
            "admission is not provider completion"
        );
        s.set_busy(false);
        assert!(s.transcript_ux.started.is_none());
        s.retire_approval("one");
        assert!(s.transcript_ux.started.is_none());
    }

    #[test]
    fn approval_timer_projects_keep_independent_running_and_waiting_intervals() {
        let mut s = TuiState::default();
        s.begin_stream_for_job(1);
        s.set_busy(true);
        s.transcript_ux.started = Some(Instant::now() - Duration::from_secs(9));
        s.present_approval(approval_timer_request("one"));
        approval_timer_event(&mut s, 1, approval_timer_required("one"));
        let first_elapsed = s.transcript_ux.elapsed;
        s.open_project_tab("second");
        s.begin_stream_for_job(2);
        s.set_busy(true);
        let second_started = s.transcript_ux.started;
        s.retire_approval("one");
        assert_eq!(s.transcript_ux.started, second_started);
        s.select_project_tab(0);
        assert_eq!(s.transcript_ux.elapsed, first_elapsed);
        assert!(s.transcript_ux.started.is_none());
        s.retire_approval("one");
        assert!(s.transcript_ux.started.is_some());
        let first_started = s.transcript_ux.started;
        s.select_project_tab(1);
        assert_eq!(
            s.transcript_ux.started, second_started,
            "hiding a running project must not restart its interval"
        );
        s.set_busy(false);
        s.select_project_tab(0);
        assert_eq!(s.transcript_ux.started, first_started);
        assert_eq!(s.transcript_ux.elapsed, first_elapsed);
    }

    #[test]
    fn approval_timer_bounded_tracking_never_resumes_with_untracked_waits() {
        for canonical in [false, true] {
            let mut s = TuiState::default();
            s.begin_stream_for_job(1);
            s.set_busy(true);
            for i in 0..129 {
                let id = format!("pending-{i}");
                if canonical {
                    approval_timer_event(&mut s, 1, approval_timer_required(&id));
                } else {
                    s.present_approval(approval_timer_request(&id));
                }
            }
            assert_eq!(
                if canonical {
                    s.transcript_ux.canonical_approvals.len()
                } else {
                    s.approval_count()
                },
                128
            );
            assert!(s.transcript_ux.approval_tracking_saturated);
            for i in 0..129 {
                let id = format!("pending-{i}");
                if canonical {
                    approval_timer_event(&mut s, 1, approval_timer_resolved(&id));
                } else {
                    s.retire_approval(&id);
                }
            }
            assert!(s.transcript_ux.started.is_none());
            s.set_busy(false);
            s.begin_stream_for_job(2);
            s.set_busy(true);
            assert!(!s.transcript_ux.approval_tracking_saturated);
            assert!(s.transcript_ux.started.is_some());
        }
    }

    #[test]
    fn approval_timer_real_coordinator_response_checks_identity_before_resuming() {
        use crate::approval::{ApprovalCoordinator, ApprovalDecision, ApprovalPolicy};
        let owner = ApprovalCoordinator::default();
        let worker_owner = owner.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            worker_owner.request_response(
                approval_timer_request("real"),
                &mut ApprovalPolicy::default(),
                &|| {
                    let _ = ready_tx.try_send(());
                    false
                },
            )
        });
        // The cancellation predicate runs after pending registration. This is
        // a channel handshake, not a sleep intended to cross a timer second.
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut s = TuiState::default();
        s.set_busy(true);
        s.present_approval(owner.drain_pending().pop().unwrap());
        assert!(s.transcript_ux.started.is_none());
        let project = s.active_project().to_owned();
        let wrong = respond_tui_approval(
            &mut s,
            &owner,
            Some(&project),
            (&project, "real", "timer-turn", "wrong-call"),
            (true, false),
        );
        assert!(wrong.is_err());
        assert!(s.transcript_ux.started.is_none());
        respond_tui_approval(
            &mut s,
            &owner,
            Some(&project),
            (&project, "real", "timer-turn", "call-real"),
            (false, false),
        )
        .unwrap();
        assert!(s.transcript_ux.started.is_some());
        assert_eq!(
            worker.join().unwrap().unwrap().decision,
            ApprovalDecision::Deny
        );
        let started = s.transcript_ux.started;
        assert!(
            respond_tui_approval(
                &mut s,
                &owner,
                Some(&project),
                (&project, "real", "timer-turn", "call-real"),
                (true, false)
            )
            .is_err()
        );
        assert_eq!(s.transcript_ux.started, started);
    }

    #[test]
    fn approval_timer_hidden_footer_replaces_shortcut_text_after_resize() {
        let mut state = TuiState::default();
        state.set_input("keep this draft");
        state.present_approval(approval_timer_request("footer"));
        state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        for width in [180, 40, 80, 180] {
            let mut terminal =
                Terminal::new(ratatui::backend::TestBackend::new(width, 40)).unwrap();
            terminal
                .draw(|frame| state.render_bentobox(frame, "zenpi"))
                .unwrap();
            let footer: String = (0..width)
                .map(|x| terminal.backend().buffer()[(x, 39)].symbol())
                .collect();
            assert_eq!(
                footer.trim(),
                "1 approvals pending · Alt-A review",
                "width={width}"
            );
            assert_eq!(state.input(), "keep this draft");
            assert_eq!(state.approval_count(), 1);
        }
    }

    #[test]
    fn approval_timer_hidden_footer_preserves_unsaved_draft_warning() {
        let mut state = TuiState::default();
        state.present_approval(approval_timer_request("footer"));
        state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        state.checkpoint_result(Err("disk full".into()));
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(180, 40)).unwrap();
        terminal
            .draw(|frame| state.render_bentobox(frame, "zenpi"))
            .unwrap();
        let footer: String = (0..180)
            .map(|x| terminal.backend().buffer()[(x, 39)].symbol())
            .collect();
        assert_eq!(footer.trim(), "Draft not saved · disk full");
        assert_eq!(state.approval_count(), 1);
    }

    #[test]
    fn footer_exposes_project_tab_shortcuts_when_idle() {
        let mut state = TuiState::default();
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(180, 40)).unwrap();
        terminal
            .draw(|frame| state.render_bentobox(frame, "zenpi"))
            .unwrap();
        let footer: String = (0..180)
            .map(|x| terminal.backend().buffer()[(x, 39)].symbol())
            .collect();
        assert!(footer.contains("Ctrl-Tab project"));
        assert!(footer.contains("Ctrl-W close"));
        assert!(footer.contains("Ctrl-T folder"));
    }

    #[test]
    fn normal_unicode_typing_reopens_file_completion_after_escape() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("工作.txt"), "file").unwrap();
        let session = crate::session::SessionStore::open_in_workspace(
            root.path().join("session.jsonl"),
            root.path(),
        )
        .unwrap();
        let mut state = TuiState::default();
        state.set_active_project_metadata(ProjectTabMetadata::from_session(&session));
        state.set_input("inspect @");
        state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(state.file_completion_query().is_none());
        state.handle_event_at(
            Event::Key(KeyEvent::new(KeyCode::Char('工'), KeyModifiers::NONE)),
            Instant::now(),
        );
        assert_eq!(state.input(), "inspect @工");
        let query = state
            .file_completion_query()
            .expect("new typing reopens completion");
        let entries = complete_file_query(&query, || false).unwrap();
        assert!(state.apply_file_completion(query, entries));
        assert!(
            state
                .slash_choices()
                .iter()
                .any(|choice| choice.contains("工作.txt"))
        );
    }

    #[test]
    fn pending_terminal_input_survives_async_submission_rejection() {
        let now = Instant::now();
        for input in ["x", "AB"] {
            let mut state = TuiState::default();
            for character in input.chars() {
                state.handle_event_at(
                    Event::Key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)),
                    now,
                );
            }
            assert!(
                state.input().is_empty(),
                "the terminal character is still held"
            );
            state.restore_rejected_input("previous rejected submission");
            state.flush_ordinary_paste(now + Duration::from_millis(70));
            assert_eq!(state.input(), input);
            assert!(state.project_checkpoint().to_string().contains(input));
            if input == "AB" {
                assert!(matches!(
                    state.handle_event_at(
                        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
                        now + Duration::from_millis(90)
                    ),
                    TuiAction::None
                ));
                state.flush_ordinary_paste(now + Duration::from_millis(160));
                assert_eq!(state.input(), "AB\n");
            }
        }
        let mut empty = TuiState::default();
        empty.restore_rejected_input("recover this submission");
        assert_eq!(empty.input(), "recover this submission");
        empty.set_input("newer visible draft");
        empty.restore_rejected_input("old submission");
        assert_eq!(empty.input(), "newer visible draft");
    }

    #[test]
    fn gantt_refresh_tracker_rejects_a_result_from_the_previous_session() {
        let mut tracker = GanttRefreshTracker::new("session-a.jsonl".into());
        let stale = GanttRefreshResult {
            snapshot: GanttPaneSnapshot {
                content: "plan-a".into(),
                blueprint_count: 1,
                goal_count: 0,
                truncated: false,
            },
            generation: tracker.generation,
        };
        assert!(tracker.accepts(&stale));
        assert!(!tracker.switch_session("session-a.jsonl".into()));
        assert!(tracker.switch_session("session-b.jsonl".into()));
        assert!(!tracker.accepts(&stale));
        assert_eq!(
            tracker.request().session_path,
            PathBuf::from("session-b.jsonl")
        );
    }

    #[test]
    fn clearing_gantt_snapshot_hides_the_previous_session_immediately() {
        let mut state = TuiState::default();
        state.set_gantt_snapshot(GanttPaneSnapshot {
            content: "plan-a".into(),
            blueprint_count: 1,
            goal_count: 0,
            truncated: false,
        });
        state.clear_gantt_snapshot();
        state.gantt_refresh_started();
        assert!(state.gantt_snapshot().is_none());
        assert_eq!(
            state.gantt_pane_content(),
            "Loading Blueprint/Goal snapshot..."
        );
    }

    #[test]
    fn gantt_status_suffix_cannot_exceed_pane_row_or_byte_limits() {
        let mut state = TuiState::default();
        state.set_gantt_snapshot(GanttPaneSnapshot {
            content: (0..MAX_GANTT_PANE_ROWS)
                .map(|index| format!("row-{index}-{}", "x".repeat(400)))
                .collect::<Vec<_>>()
                .join("\n"),
            blueprint_count: 1,
            goal_count: 1,
            truncated: true,
        });
        state.gantt_refresh_started();
        let content = state.gantt_pane_content();
        assert!(content.len() <= MAX_GANTT_PANE_BYTES);
        assert!(content.lines().count() <= MAX_GANTT_PANE_ROWS);
        assert!(content.contains("Gantt pane content truncated"));
    }

    #[test]
    fn provider_stream_overflow_is_visible_in_transcript() {
        let events = Arc::new(Mutex::new(TuiProviderEventBuffer::default()));
        {
            let mut pending = events.lock().expect("provider event buffer lock");
            pending.push(
                crate::backend::ProviderEvent::TextDelta {
                    delta: "kept".into(),
                },
                1,
            );
            pending.push(
                crate::backend::ProviderEvent::TextDelta {
                    delta: "dropped".into(),
                },
                1,
            );
        }

        let mut state = TuiState::default();
        state.begin_stream_for_job(7);
        assert!(drain_tui_provider_events(&mut state, 7, &events));

        let messages = state
            .messages()
            .map(|message| (message.role, message.text.clone()))
            .collect::<Vec<_>>();
        assert!(messages.contains(&(MessageRole::Assistant, "kept".into())));
        assert!(messages.iter().any(|(role, text)| {
            *role == MessageRole::Error && text.contains("Provider stream truncated: 1")
        }));
        assert_eq!(state.status(), "Provider stream truncated");

        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 8))
            .expect("test terminal");
        terminal
            .draw(|frame| state.render(frame, "zenpi"))
            .expect("render overflow warning");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Provider stream truncated: 1"));
        assert!(!drain_tui_provider_events(&mut state, 7, &events));
    }

    #[test]
    fn canonical_view_events_project_into_tui_transcript_and_tool_rows() {
        let mut state = TuiState::default();
        state.begin_stream_for_job(9);
        let delta = crate::view_model::ViewEvent::with_context(
            0,
            Some("request".into()),
            Some("turn".into()),
            Some("turn:assistant".into()),
            crate::view_model::ViewEventKind::TextDelta {
                delta: "hello".into(),
            },
        )
        .unwrap();
        state.apply_view_event_for_job(9, &delta);
        let tool = crate::view_model::ViewEvent::with_context(
            1,
            Some("request".into()),
            Some("turn".into()),
            None,
            crate::view_model::ViewEventKind::ToolStarted {
                call_id: "call".into(),
                name: "read_file".into(),
            },
        )
        .unwrap();
        state.apply_view_event_for_job(9, &tool);
        assert!(state.messages().any(|message| message.text == "hello"));
        assert!(state.messages().any(|message| {
            message.role == MessageRole::Tool && message.text.contains("read_file")
        }));
        let block = crate::view_model::ViewEvent::with_context(
            2,
            Some("request".into()),
            Some("turn".into()),
            Some("turn:assistant".into()),
            crate::view_model::ViewEventKind::Block {
                block: crate::view_model::ViewBlock::Paragraph {
                    text: "complete block".into(),
                },
            },
        )
        .unwrap();
        state.apply_view_event_for_job(9, &block);
        assert!(state.streaming_job_id == Some(9));
        let completed = crate::view_model::ViewEvent::with_context(
            3,
            Some("request".into()),
            Some("turn".into()),
            None,
            crate::view_model::ViewEventKind::TurnCompleted {
                response_id: None,
                model: None,
            },
        )
        .unwrap();
        state.apply_view_event_for_job(9, &completed);
        assert!(state.streaming_job_id.is_none());
    }

    #[test]
    fn canonical_approval_and_drop_events_are_visible() {
        let mut state = TuiState::default();
        state.begin_stream_for_job(1);
        let approval = crate::view_model::ViewEvent::with_context(
            0,
            None,
            Some("turn-approval".into()),
            None,
            crate::view_model::ViewEventKind::ApprovalRequired {
                approval_id: "approval-1".into(),
                tool: "write_file".into(),
                arguments: "{}".into(),
            },
        )
        .unwrap();
        state.apply_view_event_for_job(1, &approval);
        let dropped = crate::view_model::ViewEvent::new(
            1,
            crate::view_model::ViewEventKind::Dropped {
                stream: crate::view_model::ViewStream::Provider,
                count: 2,
            },
        )
        .unwrap();
        state.apply_view_event_for_job(1, &dropped);
        assert!(state.messages().any(|message| {
            message.text.contains("Approval required")
                && message.blocks.iter().any(|block| {
                    matches!(
                        block,
                        crate::view_model::ViewBlock::Approval { approval_id, .. }
                            if approval_id == "approval-1"
                    )
                })
        }));
        assert!(
            state
                .messages()
                .any(|message| message.text.contains("Provider stream truncated: 2"))
        );
    }

    #[test]
    fn live_tool_snapshots_replace_per_stream_and_reject_late_jobs() {
        use crate::core::AgentEvent;
        use crate::tool_output::{OutputProgress, OutputSnapshot, OutputStream};
        let events = Arc::new(Mutex::new(TuiProviderEventBuffer::default()));
        let mut state = TuiState::default();
        state.begin_stream_for_job(71);
        events.lock().unwrap().push_agent(AgentEvent::ToolCall {
            turn_id: "turn-71".into(),
            call_id: "call".into(),
            tool: "run_command".into(),
        });
        let progress = |stream, text: &str| AgentEvent::ToolProgress {
            turn_id: "turn-71".into(),
            progress: OutputProgress {
                call_id: "call".into(),
                stream,
                snapshot: OutputSnapshot {
                    text: text.into(),
                    display_start: 0,
                    display_end: text.len() as u64,
                    bytes_observed: text.len() as u64,
                    truncated: false,
                    lossy: false,
                },
                dropped_updates: 0,
            },
        };
        for text in ["first", "second", "latest"] {
            state.clear_project_checkpoint_dirty();
            events
                .lock()
                .unwrap()
                .push_agent(progress(OutputStream::Stdout, text));
            drain_tui_provider_events(&mut state, 71, &events);
            assert!(
                state.project_checkpoint_dirty(),
                "tool snapshot must persist without keys"
            );
            assert!(state.project_checkpoint().to_string().contains(text));
        }
        events
            .lock()
            .unwrap()
            .push_agent(progress(OutputStream::Stderr, "error stream"));
        drain_tui_provider_events(&mut state, 71, &events);
        let blocks: Vec<_> = state.messages().filter(|m| m.block_id.is_some()).collect();
        assert_eq!(blocks.len(), 2);
        assert!(blocks.iter().any(|m| m.text.contains("latest")));
        assert!(
            !blocks
                .iter()
                .any(|m| m.text.contains("first") || m.text.contains("second"))
        );
        assert!(blocks.iter().any(|m| m.text.contains("error stream")));
        let rendered = transcript_lines(&state.messages, 120, "zenpi")
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(rendered.matches("latest").count(), 1);
        assert_eq!(rendered.matches("error stream").count(), 1);
        events.lock().unwrap().push_agent(AgentEvent::ToolResult {
            turn_id: "turn-71".into(),
            call_id: "call".into(),
            success: true,
        });
        drain_tui_provider_events(&mut state, 71, &events);
        assert!(state.messages().flat_map(|m| &m.blocks).all(|b| !matches!(
            b,
            crate::view_model::ViewBlock::ToolStatus {
                status: crate::view_model::ToolStatus::Running,
                ..
            }
        )));
        state.begin_stream_for_job(72);
        state.clear_project_checkpoint_dirty();
        let before: Vec<_> = state.messages().cloned().collect();
        events
            .lock()
            .unwrap()
            .push_agent(progress(OutputStream::Stdout, "late must not appear"));
        events.lock().unwrap().push_agent(AgentEvent::ToolCall {
            turn_id: "turn-71".into(),
            call_id: "late".into(),
            tool: "run_command".into(),
        });
        drain_tui_provider_events(&mut state, 71, &events);
        assert_eq!(state.messages().cloned().collect::<Vec<_>>(), before);
        assert!(
            !state.project_checkpoint_dirty(),
            "late output must not schedule a save"
        );
    }

    #[test]
    fn provider_event_buffer_enforces_bytes_and_resets_accounting() {
        let first = crate::backend::ProviderEvent::TextDelta {
            delta: "a".repeat(32),
        };
        let second = crate::backend::ProviderEvent::TextDelta {
            delta: "b".repeat(32),
        };
        let first_bytes = serialized_provider_event_bytes(&first);
        let second_bytes = serialized_provider_event_bytes(&second);
        let mut buffer = TuiProviderEventBuffer::default();
        buffer.push_with_limits(first, 8, first_bytes + second_bytes - 1);
        buffer.push_with_limits(second, 8, first_bytes + second_bytes - 1);
        assert_eq!(buffer.events.len(), 1);
        assert_eq!(buffer.bytes, first_bytes);
        assert_eq!(buffer.dropped, 1);
        let (events, dropped) = buffer.take();
        assert_eq!(events.len(), 1);
        assert_eq!(dropped, 1);
        assert_eq!(buffer.bytes, 0);

        let oversized = crate::backend::ProviderEvent::TextDelta {
            delta: "x".repeat(MAX_TUI_PROVIDER_BYTES),
        };
        buffer.push(oversized, MAX_TUI_PROVIDER_EVENTS);
        assert!(buffer.events.is_empty());
        assert_eq!(buffer.dropped, 1);
    }

    #[test]
    fn pending_shell_fifo_is_bounded_and_preserves_kind() {
        let mut queue = TuiPendingInputs::default();
        assert!(!queue.should_queue(TuiRequestKind::UserShell, None));
        queue
            .push(TuiRequestKind::UserShell, "echo one".into())
            .expect("first shell input");
        assert!(queue.should_queue(TuiRequestKind::Model, Some(TuiRequestKind::UserShell)));
        assert_eq!(
            queue.pop().map(|(kind, text, _)| (kind, text)),
            Some((TuiRequestKind::UserShell, "echo one".into()))
        );
        for index in 0..MAX_TUI_PENDING_INPUTS {
            queue
                .push(TuiRequestKind::Model, format!("prompt-{index}"))
                .expect("bounded queue slot");
        }
        assert!(
            queue
                .push(TuiRequestKind::UserShell, "overflow".into())
                .is_err()
        );
    }

    #[test]
    fn user_shell_result_renders_nonzero_status_and_redacted_output() {
        let mut state = TuiState::default();
        let result = serde_json::json!({
            "origin": "user_shell",
            "stdout": "api_key=secret-value",
            "stderr": "permission denied",
            "exit_code": 1,
            "cancelled": false,
            "timed_out": false,
        });
        render_user_shell_result(&mut state, &result);
        assert_eq!(state.status(), "Local shell exit 1");
        assert!(state.messages().any(|message| {
            message.text.contains("Local shell exit 1") && !message.text.contains("secret-value")
        }));
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 8))
            .expect("test terminal");
        terminal
            .draw(|frame| state.render_workspace_pane(frame, PaneId::Terminal, frame.area()))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Local shell exit 1"));
        assert!(!rendered.contains("secret-value"));
    }

    #[test]
    fn terminal_snapshot_bound_is_utf8_safe() {
        let mut state = TuiState::default();
        render_user_shell_result(
            &mut state,
            &serde_json::json!({
                "stdout": "界".repeat(MAX_MESSAGE_BYTES),
                "exit_code": 0,
                "cancelled": false,
                "timed_out": false
            }),
        );
        assert!(state.terminal_snapshot.len() <= MAX_MESSAGE_BYTES);
        assert!(std::str::from_utf8(state.terminal_snapshot.as_bytes()).is_ok());
    }

    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn session_browser_cursor_is_bounded_and_resets_on_search() {
        let mut state = TuiState::default();
        state.session_browser = (0..40)
            .map(|index| crate::session::SessionSummary {
                path: format!("session-{index}"),
                session_id: format!("id-{index}"),
                created_at_ms: 0,
                turn_count: 0,
                handoff_count: 0,
                handoff_record_count: 0,
                runtime_intent_count: 0,
                event_count: 0,
                recovery_warnings: 0,
                next_seq: 0,
            })
            .collect();
        state.move_session_browser_cursor(100);
        assert_eq!(state.session_browser_cursor(), 31);
        assert_eq!(state.selected_session_browser_path(), Some("session-31"));
        state.move_session_browser_cursor(-100);
        assert_eq!(state.session_browser_cursor(), 0);
        assert!(state.session_browser_content().starts_with("> "));
        state
            .workspace_layout
            .set_focused(Some(PaneId::SessionList));
        assert_eq!(
            state.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
            TuiAction::Redraw
        );
        assert_eq!(state.session_browser_cursor(), 8);
        assert_eq!(
            state.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)),
            TuiAction::Redraw
        );
        assert_eq!(state.session_browser_cursor(), 0);
        assert_eq!(
            state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            TuiAction::OpenSession("session-0".into())
        );
    }

    #[test]
    fn session_browser_periodic_refresh_keeps_path_and_project_search_ownership() {
        use crate::{
            core::{Turn, TurnRole},
            session::SessionStore,
        };
        let root = tempfile::tempdir().unwrap();
        let owner = SessionStore::open(root.path().join("owner.jsonl")).unwrap();
        for name in ["b", "c"] {
            let mut store = SessionStore::open(root.path().join(format!("{name}.jsonl"))).unwrap();
            store
                .append_turn(Turn::new("u", TurnRole::User, "needle"))
                .unwrap();
        }
        let mut state = TuiState::default();
        state.refresh_session_snapshot(&owner);
        state.search_session_browser(&owner, "needle").unwrap();
        state.move_session_browser_cursor(1);
        let selected = state.selected_session_browser_path().unwrap().to_owned();
        let mut inserted = SessionStore::open(root.path().join("a.jsonl")).unwrap();
        inserted
            .append_turn(Turn::new("u", TurnRole::User, "needle"))
            .unwrap();
        // A tick within the scan interval does not enumerate the directory again.
        state.refresh_session_snapshot(&owner);
        assert_eq!(state.session_browser.len(), 2);
        state.session_browser_refresh.last_checked = None;
        state.refresh_session_snapshot(&owner);
        assert_eq!(state.session_browser.len(), 3);
        assert_eq!(
            state.selected_session_browser_path(),
            Some(selected.as_str())
        );
        assert_eq!(state.session_browser_cursor(), 2);

        assert!(state.open_project_tab("other"));
        let other = tempfile::tempdir().unwrap();
        let other_owner = SessionStore::open(other.path().join("other.jsonl")).unwrap();
        state.refresh_session_snapshot(&other_owner);
        assert!(state.session_browser_refresh.query.is_none());
        assert_eq!(state.session_browser.len(), 1);
        assert!(state.select_project_tab(0));
        state.session_browser_refresh.last_checked = None;
        state.refresh_session_snapshot(&owner);
        assert_eq!(
            state.session_browser_refresh.query.as_deref(),
            Some("needle")
        );
        assert_eq!(
            state.selected_session_browser_path(),
            Some(selected.as_str())
        );

        // Deleting the selected file clamps to a surviving row without an invalid index.
        std::fs::remove_file(selected).unwrap();
        state.session_browser_refresh.last_checked = None;
        state.refresh_session_snapshot(&owner);
        assert_eq!(state.session_browser.len(), 2);
        assert_eq!(state.session_browser_cursor(), 1);
        state.search_session_browser(&owner, "absent").unwrap();
        state.session_browser_refresh.last_checked = None;
        state.refresh_session_snapshot(&owner);
        assert!(state.session_browser.is_empty());
        assert_eq!(state.selected_session_browser_path(), None);
        assert!(state.search_session_browser(&owner, "").is_err());
        assert_eq!(
            state.session_browser_refresh.query.as_deref(),
            Some("absent")
        );
        let mut agent = crate::core::Agent::with_echo(owner);
        dispatch_slash_command(
            SlashCommand::Session {
                action: crate::slash::SessionAction::List,
            },
            &mut state,
            Some(&mut agent),
        );
        assert!(state.session_browser_refresh.query.is_none());
        assert_eq!(state.session_browser.len(), 3);
    }

    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn session_browser_mouse_click_maps_rows_and_stays_bounded() {
        let mut state = TuiState::default();
        state.session_browser = (0..40)
            .map(|index| crate::session::SessionSummary {
                path: format!("session-{index}"),
                session_id: format!("id-{index}"),
                created_at_ms: 0,
                turn_count: 0,
                handoff_count: 0,
                handoff_record_count: 0,
                runtime_intent_count: 0,
                event_count: 0,
                recovery_warnings: 0,
                next_seq: 0,
            })
            .collect();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
        state.set_workspace_tab(TabId::Session);
        terminal
            .draw(|frame| state.render_bentobox(frame, "zenpi"))
            .unwrap();
        let pane = BentoBoxLayoutAdapter::new(&state.workspace_layout, state.workspace_area)
            .pane(PaneId::SessionList)
            .expect("session pane in project preset")
            .rect;
        state.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: pane.x + 2,
            row: pane.y + 4,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(state.session_browser_cursor(), 3);
        state.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: pane.x + 2,
            row: pane.bottom().saturating_sub(2),
            modifiers: KeyModifiers::NONE,
        });
        assert!(state.session_browser_cursor() < 32);
        assert_eq!(state.focused_workspace_pane(), Some(PaneId::SessionList));
    }
}

#[test]
fn accepted_shell_queue_owns_original_bytes_through_pop_cancel_and_edit() {
    let original = format!("  !printf kept # {}\n", "x".repeat(1001));
    let mut state = TuiState::default();
    let mut pending = TuiPendingInputs::default();
    let submit = |state: &mut TuiState, pending: &mut TuiPendingInputs| {
        state.handle_event(Event::Paste(original.clone()));
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        pending.admit_original(
            TuiRequestKind::UserShell,
            original.trim().strip_prefix('!').unwrap().into(),
            original.clone(),
            state,
        );
        assert_eq!(pending.inputs.back().unwrap().1, original);
        assert_eq!(pending.bytes, original.len());
        assert_eq!(state.scheduled_inputs.last().unwrap().text, original);
    };
    submit(&mut state, &mut pending);
    pending.cancel(&mut state);
    assert!(state.submitted_pastes.is_empty());
    submit(&mut state, &mut pending);
    let id = pending.identities.front().unwrap().0.clone();
    let edited = "  !printf edited\n";
    assert!(pending.scheduled_command(&mut state, &format!("/scheduled edit {id} 0 {edited}")));
    assert!(state.submitted_pastes.is_empty());
    let (kind, text, paste) = pending.pop().unwrap();
    assert!(paste.is_none());
    assert_eq!(queued_request_text(kind, text), edited);
    pending.sync_projection(&mut state);
    submit(&mut state, &mut pending);
    let (kind, text, paste) = pending.pop().unwrap();
    let text = queued_request_text(kind, text);
    assert_eq!(text, original);
    let snapshot = paste.unwrap();
    assert!(state.submitted_pastes.is_empty());
    state.apply_rejected_input(text, Some(snapshot));
    assert_eq!(state.input, original);
    assert_eq!(state.paste.folds.len(), 1);
    assert_eq!(
        queued_request_text(TuiRequestKind::UserShell, "echo legacy".into()),
        "!echo legacy"
    );
}

#[test]
fn accepted_shell_queue_budget_counts_original_whitespace_and_marker() {
    let mut state = TuiState::default();
    let mut pending = TuiPendingInputs::default();
    pending.admit_prompt("x".repeat(MAX_TUI_PENDING_INPUT_BYTES - 3), &mut state);
    pending.admit_original(
        TuiRequestKind::UserShell,
        "x".into(),
        "  !x\n".into(),
        &mut state,
    );
    assert_eq!(pending.inputs.len(), 1);
    assert_eq!(state.input, "  !x\n");
}

#[test]
fn identical_scheduled_payloads_keep_metadata_on_their_actual_queue_identity() {
    for kind in [TuiRequestKind::Model, TuiRequestKind::UserShell] {
        for edit_second in [false, true] {
            let original = format!("  !echo same # {}\n", "x".repeat(1001));
            let mut state = TuiState::default();
            let mut pending = TuiPendingInputs::default();
            state.handle_event(Event::Paste(original.clone()));
            state.cursor = 0;
            let first = state.paste.folds.clone();
            state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            pending.admit(kind, original.clone(), &mut state);
            state.handle_event(Event::Paste(original.clone()));
            let second = state.paste.folds.clone();
            state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            pending.admit(kind, original.clone(), &mut state);
            assert_ne!(first, second);
            assert!(state.submitted_pastes.is_empty());
            let first_id = pending.identities[0].0.clone();
            let second_id = pending.identities[1].0.clone();
            let command = if edit_second {
                format!("/scheduled edit {second_id} 0 echo edited")
            } else {
                format!("/scheduled cancel {second_id}")
            };
            pending.scheduled_command(&mut state, &command);
            assert_eq!(pending.identities[0].0, first_id);
            let (_, text, paste) = pending.pop().unwrap();
            state.restore_submitted_input(SubmittedInput { text, paste });
            assert_eq!(state.input, original);
            assert_eq!(state.cursor, 0);
            assert_eq!(state.paste.folds, first);
            if edit_second {
                let (_, text, paste) = pending.pop().unwrap();
                assert_eq!(text, "echo edited");
                assert!(paste.is_none());
            }
            assert!(pending.paste_metadata.is_empty());
        }
    }
}

#[test]
fn identical_async_control_receipts_restore_their_bound_snapshot_and_keep_newer_draft() {
    let original = "界".repeat(1001);
    let mut state = TuiState::default();
    state.handle_event(Event::Paste(original.clone()));
    state.cursor = 0;
    let first = state.paste.folds.clone();
    state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let first_receipt = state.bind_submitted_input(original.clone());
    state.handle_event(Event::Paste(original.clone()));
    state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let second_receipt = state.bind_submitted_input(original.clone());
    assert!(state.submitted_pastes.is_empty());
    TuiInputControls::show_result(
        &mut state,
        Err("first owner receipt rejected".into()),
        first_receipt,
    );
    assert_eq!(state.input, original);
    assert_eq!(state.cursor, 0);
    assert_eq!(state.paste.folds, first);
    state.set_input("newer draft");
    TuiInputControls::show_result(
        &mut state,
        Err("second owner receipt rejected".into()),
        second_receipt,
    );
    assert_eq!(state.input, "newer draft");
    assert!(state.paste.folds.is_empty());
}

#[cfg(test)]
mod async_session_browser_tests {
    use super::*;
    use crate::{
        core::{Agent, Turn, TurnRole},
        session::SessionStore,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };

    fn state_for(owner: &SessionStore) -> TuiState {
        let mut state = TuiState {
            async_session_browser: true,
            ..TuiState::default()
        };
        state.refresh_session_snapshot(owner);
        state
    }

    fn turn_at(root: &std::path::Path, name: &str, text: &str) -> SessionStore {
        let mut store = SessionStore::open(root.join(format!("{name}.jsonl"))).unwrap();
        store
            .append_turn(Turn::new("u", TurnRole::User, text))
            .unwrap();
        store
    }

    fn settle(host: &mut SessionBrowserHost, state: &mut TuiState) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            host.poll(state);
            if host.active.is_none() && host.pending.is_none() {
                break;
            }
            assert!(Instant::now() < deadline, "browser did not settle");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn search(host: &mut SessionBrowserHost, state: &mut TuiState, query: &str) {
        assert!(host.route(
            &SlashCommand::Session {
                action: crate::slash::SessionAction::Search {
                    query: query.into()
                }
            },
            state,
            format!("/session search {query}"),
        ));
    }

    #[test]
    fn session_browser_barrier_keeps_real_drain_keys_resize_and_render_reachable() {
        let root = tempfile::tempdir().unwrap();
        let owner = turn_at(root.path(), "owner", "needle");
        let mut state = state_for(&owner);
        let agent = Arc::new(Mutex::new(Agent::with_echo(owner)));
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let caller = std::thread::current().id();
        let mut host = SessionBrowserHost::with_scanner(move |key, explicit| {
            assert_ne!(caller, std::thread::current().id());
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            scan_session_browser(key, explicit)
        })
        .unwrap();
        host.poll(&mut state);
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let start = Instant::now();
        // The same drain and projection methods used by run_async execute while
        // the worker is provably inside a scan and cannot produce a reply.
        for _ in 0..20 {
            drain_agent_tool_events(&agent, &mut state);
            host.poll(&mut state);
        }
        state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        state.finish_ordinary_paste();
        assert_eq!(state.input(), "x");
        state.handle_event(Event::Resize(100, 30));
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| state.render_bentobox(frame, "zenpi"))
            .unwrap();
        assert!(
            state.session_browser.is_empty(),
            "no reply exists before release"
        );
        assert!(host.active.is_some());
        assert!(host.pending.is_none());
        eprintln!(
            "ASYNC barrier-held drain20+key+resize+render_us={}",
            start.elapsed().as_micros()
        );
        release_tx.send(()).unwrap();
        settle(&mut host, &mut state);
        assert_eq!(state.session_browser.len(), 1);
    }

    #[test]
    fn session_browser_one_active_one_latest_rejects_same_query_old_generation() {
        let root = tempfile::tempdir().unwrap();
        let owner = turn_at(root.path(), "owner", "needle");
        let mut state = state_for(&owner);
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let count = Arc::new(AtomicUsize::new(0));
        let worker_count = Arc::clone(&count);
        let mut host = SessionBrowserHost::with_scanner(move |key, _| {
            let index = worker_count.fetch_add(1, Ordering::SeqCst);
            entered_tx.send(key.clone()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            let mut projection = scan_session_browser(key, false);
            projection.receipt = Some(Ok(format!("receipt-{index}")));
            projection
        })
        .unwrap();
        search(&mut host, &mut state, "needle");
        host.poll(&mut state);
        let old = entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        for i in 0..100 {
            search(&mut host, &mut state, &format!("query-{i}"));
            host.poll(&mut state);
        }
        search(&mut host, &mut state, "needle");
        let latest = host.pending.as_ref().unwrap().key.clone();
        assert_eq!(old.query, latest.query);
        assert!(latest.generation > old.generation);
        assert_eq!(host.active.as_ref().unwrap().key, old);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        release_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while host.active.as_ref().is_some_and(|r| r.key == old) {
            host.poll(&mut state);
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            entered_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            latest
        );
        assert!(state.session_browser.is_empty());
        assert!(!state.messages().any(|m| m.text.contains("receipt-0")));
        release_tx.send(()).unwrap();
        settle(&mut host, &mut state);
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert!(state.messages().any(|m| m.text.contains("receipt-1")));
        assert_eq!(state.session_browser.len(), 1);
    }

    #[test]
    fn session_browser_stale_scope_rejects_project_path_session_and_query() {
        for change in ["project", "path", "session", "query"] {
            let root = tempfile::tempdir().unwrap();
            let owner = turn_at(root.path(), "owner", "needle");
            let mut state = state_for(&owner);
            let (entered_tx, entered_rx) = mpsc::sync_channel(1);
            let (release_tx, release_rx) = mpsc::sync_channel(1);
            let mut first = true;
            let mut host = SessionBrowserHost::with_scanner(move |key, _| {
                if first {
                    first = false;
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                    let mut projection = scan_session_browser(key, false);
                    projection.receipt = Some(Ok("stale receipt".into()));
                    projection
                } else {
                    SessionBrowserProjection {
                        rows: Ok(vec![]),
                        receipt: None,
                    }
                }
            })
            .unwrap();
            host.poll(&mut state);
            entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            match change {
                "project" => {
                    assert!(state.open_project_tab("other"));
                    state.refresh_session_snapshot(&owner);
                }
                "path" => {
                    state.session_browser_refresh.owner.as_mut().unwrap().0 =
                        root.path().join("other.jsonl")
                }
                "session" => {
                    state.session_browser_refresh.owner.as_mut().unwrap().1 =
                        "new-session-id".into()
                }
                "query" => search(&mut host, &mut state, "absent"),
                _ => unreachable!(),
            }
            host.poll(&mut state);
            release_tx.send(()).unwrap();
            settle(&mut host, &mut state);
            assert!(state.session_browser.is_empty(), "{change}");
            assert!(
                !state.messages().any(|m| m.text.contains("stale receipt")),
                "{change}"
            );
        }
    }

    #[test]
    fn session_browser_async_selection_queries_empty_and_failed_refresh() {
        let root = tempfile::tempdir().unwrap();
        let owner = SessionStore::open(root.path().join("owner.jsonl")).unwrap();
        turn_at(root.path(), "b", "needle");
        turn_at(root.path(), "c", "needle");
        let mut state = state_for(&owner);
        let mut host = SessionBrowserHost::new().unwrap();
        search(&mut host, &mut state, "needle");
        settle(&mut host, &mut state);
        assert_eq!(state.session_browser.len(), 2);
        state.move_session_browser_cursor(1);
        let selected = state.selected_session_browser_path().unwrap().to_owned();
        turn_at(root.path(), "a", "needle");
        state.session_browser_refresh.last_checked = None;
        settle(&mut host, &mut state);
        assert_eq!(state.session_browser.len(), 3);
        assert_eq!(
            state.selected_session_browser_path(),
            Some(selected.as_str())
        );
        assert_eq!(state.session_browser_cursor(), 2);
        assert!(state.open_project_tab("other"));
        let other = tempfile::tempdir().unwrap();
        let other_owner = SessionStore::open(other.path().join("other.jsonl")).unwrap();
        state.refresh_session_snapshot(&other_owner);
        settle(&mut host, &mut state);
        assert_eq!(state.session_browser.len(), 1);
        assert!(state.session_browser_refresh.query.is_none());
        assert!(state.select_project_tab(0));
        state.refresh_session_snapshot(&owner);
        settle(&mut host, &mut state);
        assert_eq!(
            state.session_browser_refresh.query.as_deref(),
            Some("needle")
        );
        assert_eq!(
            state.selected_session_browser_path(),
            Some(selected.as_str())
        );
        std::fs::remove_file(selected).unwrap();
        state.session_browser_refresh.last_checked = None;
        settle(&mut host, &mut state);
        assert_eq!(state.session_browser_cursor(), 1);
        let old_rows = state.session_browser.clone();
        // An ordinary directory carrying the journal extension is rejected;
        // this is a real scan failure without a FIFO or special-file probe.
        std::fs::create_dir(root.path().join("bad.jsonl")).unwrap();
        search(&mut host, &mut state, "other-query");
        state.set_input("newer draft");
        settle(&mut host, &mut state);
        assert_eq!(state.session_browser, old_rows);
        assert_eq!(state.session_browser_cursor(), 1);
        assert_eq!(
            state.session_browser_refresh.query.as_deref(),
            Some("needle")
        );
        assert_eq!(state.input(), "newer draft");
        std::fs::remove_dir(root.path().join("bad.jsonl")).unwrap();
        search(&mut host, &mut state, "absent");
        settle(&mut host, &mut state);
        assert!(state.session_browser.is_empty());
        assert_eq!(state.selected_session_browser_path(), None);
        search(&mut host, &mut state, "");
        assert_eq!(
            state.session_browser_refresh.query.as_deref(),
            Some("absent")
        );
        assert!(host.active.is_none());
        state.session_browser_refresh.last_checked = None;
        settle(&mut host, &mut state);
        assert!(state.session_browser.is_empty());
    }

    #[test]
    fn session_browser_list_keeps_global_receipt_separate_and_clears_local_filter() {
        let local = tempfile::tempdir().unwrap();
        let global = tempfile::tempdir().unwrap();
        let owner = turn_at(local.path(), "local-owner", "local needle");
        let global_owner = turn_at(global.path(), "global-owner", "global needle");
        let global_id = global_owner.session_id().to_owned();
        let mut state = state_for(&owner);
        state.session_browser_refresh.query = Some("absent".into());
        // Replace only configuration discovery for this hermetic two-directory
        // boundary test. The host still receives independent pane/receipt data.
        let global_path = global.path().to_owned();
        let mut host = SessionBrowserHost::with_scanner(move |key, explicit| {
            let mut projection = scan_session_browser(key, false);
            if explicit {
                projection.receipt = Some(
                    crate::session::list_sessions(&global_path)
                        .map(|rows| {
                            rows.iter()
                                .map(|row| row.session_id.clone())
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .map_err(|error| error.to_string()),
                );
            }
            projection
        })
        .unwrap();
        assert!(host.route(
            &SlashCommand::Session {
                action: crate::slash::SessionAction::List
            },
            &mut state,
            "/session list".into()
        ));
        assert!(state.session_browser_refresh.query.is_none());
        settle(&mut host, &mut state);
        assert_eq!(state.session_browser.len(), 1);
        assert_eq!(state.session_browser[0].session_id, owner.session_id());
        assert!(state.messages().any(|m| m.text.contains(&global_id)));
        assert!(
            !state
                .messages()
                .any(|m| m.text.contains(owner.session_id()))
        );
    }

    #[test]
    fn session_browser_closed_worker_restores_latest_receipt_without_overwriting_new_draft() {
        let root = tempfile::tempdir().unwrap();
        let owner = turn_at(root.path(), "owner", "needle");
        let mut state = state_for(&owner);
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let mut host = SessionBrowserHost::with_scanner(move |_, _| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            panic!("injected scanner panic");
        })
        .unwrap();
        search(&mut host, &mut state, "first");
        host.poll(&mut state);
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        search(&mut host, &mut state, "latest");
        release_tx.send(()).unwrap();
        settle(&mut host, &mut state);
        assert!(host.closed);
        assert_eq!(state.input(), "/session search latest");
        state.set_input("newer draft");
        search(&mut host, &mut state, "rejected");
        assert_eq!(state.input(), "newer draft");
        assert!(host.active.is_none() && host.pending.is_none());
    }
}

#[cfg(test)]
mod local_diff_host_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };

    fn state() -> TuiState {
        let mut state = TuiState::default();
        state.set_active_project_metadata(ProjectTabMetadata {
            cwd: "/tmp/selected-B".into(),
            session_path: Some("/tmp/B.jsonl".into()),
            approval_mode: Default::default(),
            ..Default::default()
        });
        state.set_project_session_cursor(state.active_project().to_owned(), "session-B", 1);
        state.set_busy(true);
        state
    }
    fn value() -> serde_json::Value {
        serde_json::json!({"path":"note", "diff":"+B_ONLY", "status":"?? note"})
    }
    fn settle(host: &mut LocalDiffHost, state: &mut TuiState) {
        let until = Instant::now() + Duration::from_secs(2);
        while host.active.is_some() && Instant::now() < until {
            host.poll(state);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(host.active.is_none());
    }
    #[test]
    fn late_result_stays_in_original_project_and_preserves_both_drafts_layouts() {
        let mut state = state();
        let b = state.active_project().to_owned();
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let mut host = LocalDiffHost::with_reader(move |work| {
            assert_eq!(work.cwd, std::path::Path::new("/tmp/selected-B"));
            entered.send(()).unwrap();
            gate.recv().unwrap();
            Ok(value())
        })
        .unwrap();
        host.start(&mut state, Some("note".into()), "/review note".into());
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        state.set_input("new B draft");
        let layout_b = state.active_project_workspace().clone();
        state.open_project_tab("A");
        state.set_active_project_metadata(ProjectTabMetadata {
            cwd: "/tmp/A".into(),
            session_path: Some("/tmp/A.jsonl".into()),
            approval_mode: Default::default(),
            ..Default::default()
        });
        state.set_input("new A draft");
        let layout_a = state.active_project_workspace().clone();
        release.send(()).unwrap();
        settle(&mut host, &mut state);
        assert_eq!(state.active_project(), "A");
        assert_eq!(state.input(), "new A draft");
        assert_eq!(*state.active_project_workspace(), layout_a);
        assert!(!state.messages().any(|m| m.text.contains("B_ONLY")));
        state.select_project_tab(state.project_index(&b).unwrap());
        assert!(state.messages().any(|m| m.text.contains("B_ONLY")));
        assert_eq!(state.input(), "new B draft");
        assert_eq!(*state.active_project_workspace(), layout_b);
    }
    #[test]
    fn session_replacement_discards_a_completed_old_receipt() {
        let mut state = state();
        let mut host = LocalDiffHost::with_reader(|_| Ok(value())).unwrap();
        host.start(&mut state, None, "/diff".into());
        state.set_project_session_cursor(state.active_project().to_owned(), "replacement", 1);
        settle(&mut host, &mut state);
        assert!(!state.messages().any(|m| m.text.contains("B_ONLY")));
    }
    #[test]
    fn cancellation_blocks_late_success_and_single_slot_rejects_second_command() {
        let mut state = state();
        let (release, gate) = mpsc::channel();
        let mut host = LocalDiffHost::with_reader(move |_| {
            gate.recv().unwrap();
            Ok(value())
        })
        .unwrap();
        host.start(&mut state, None, "/diff".into());
        host.start(&mut state, None, "/review".into());
        assert_eq!(state.input(), "/review");
        assert!(
            state
                .messages()
                .last()
                .unwrap()
                .text
                .contains("already running")
        );
        assert!(host.cancel_current(&mut state));
        release.send(()).unwrap();
        settle(&mut host, &mut state);
        assert!(!state.messages().any(|m| m.text.contains("B_ONLY")));
        assert_eq!(state.input(), "/review");
    }
    #[test]
    fn worker_shutdown_signals_cancel_and_joins_without_detachment() {
        let done = Arc::new(AtomicBool::new(false));
        let finished = Arc::clone(&done);
        let (entered, started) = mpsc::channel();
        let mut host = LocalDiffHost::with_reader(move |work| {
            entered.send(()).unwrap();
            while !work.cancelled.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(2));
            }
            finished.store(true, Ordering::Release);
            Err("cancelled".into())
        })
        .unwrap();
        host.start(&mut state(), None, "/diff".into());
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(host);
        assert!(done.load(Ordering::Acquire));
    }
    #[test]
    fn middle_click_closes_project_tab_without_touching_session_owner() {
        let mut state = TuiState::default();
        assert!(state.open_project_tab("other"));
        state.select_project_tab(1);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
        terminal
            .draw(|frame| state.render_bentobox(frame, "zenpi"))
            .unwrap();
        let (_, index) = state
            .project_hits
            .iter()
            .find(|(_, index)| *index == 1)
            .copied()
            .expect("second project tab hit target");
        let rect = state
            .project_hits
            .iter()
            .find(|(_, candidate)| *candidate == index)
            .map(|(rect, _)| *rect)
            .unwrap();
        let before = state.project_tabs().to_vec();
        state.set_input("keep this draft");
        state.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Middle),
            column: rect.x + 1,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(state.project_tabs(), before.as_slice());
        assert_eq!(state.input(), "keep this draft");
        state.set_input("");
        state.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Middle),
            column: rect.x + 1,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(before, vec!["default".to_owned(), "other".to_owned()]);
        assert_eq!(state.project_tabs(), &["default".to_owned()]);
        assert!(!state.project_metadata("other").is_some());
    }
    #[test]
    fn ctrl_w_requests_project_close_only_for_an_empty_prompt() {
        let mut state = TuiState::default();
        assert!(state.open_project_tab("other"));
        assert_eq!(
            state.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL,)),
            TuiAction::Redraw
        );
        assert!(matches!(
            state.take_project_intent(),
            Some(ProjectIntent::Close(ref name)) if name == "other"
        ));
        state.set_input("draft text");
        assert_eq!(
            state.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL,)),
            TuiAction::None
        );
        assert_eq!(state.input(), "draft ");
        assert!(state.take_project_intent().is_none());
    }
    #[test]
    fn middle_click_keeps_a_non_active_project_draft() {
        let mut state = TuiState::default();
        state.set_input("background draft");
        assert!(state.open_project_tab("other"));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
        terminal
            .draw(|frame| state.render_bentobox(frame, "zenpi"))
            .unwrap();
        let rect = state
            .project_hits
            .iter()
            .find(|(_, index)| *index == 0)
            .map(|(rect, _)| *rect)
            .unwrap();
        state.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Middle),
            column: rect.x + 1,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            state.project_tabs(),
            &["default".to_owned(), "other".to_owned()]
        );
        assert!(state.status().contains("Draft kept"));
    }
    #[test]
    fn missing_context_never_falls_back_and_failure_keeps_newer_draft() {
        let mut missing = TuiState::default();
        dispatch_slash_command(SlashCommand::Diff { path: None }, &mut missing, None);
        assert_eq!(missing.messages().last().unwrap().role, MessageRole::Error);
        let mut state = state();
        let (release, gate) = mpsc::channel();
        let mut host = LocalDiffHost::with_reader(move |_| {
            gate.recv().unwrap();
            Err("denied".into())
        })
        .unwrap();
        host.start(&mut state, None, "/diff".into());
        state.set_input("new draft");
        release.send(()).unwrap();
        settle(&mut host, &mut state);
        assert_eq!(state.input(), "new draft");
        assert_eq!(state.messages().last().unwrap().role, MessageRole::Error);
    }
}
