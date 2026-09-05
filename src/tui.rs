//! Interactive terminal mode.
//!
//! [`TuiState`] is deliberately independent from the agent.  This keeps
//! rendering and input handling testable with Ratatui's `TestBackend`, while
//! the runtime loop can connect any synchronous agent through a small
//! callback.  Ratatui keeps a previous cell buffer and emits only changed
//! cells; resize notifications are coalesced by [`RenderScheduler`] so a
//! resize drag or a burst of stream chunks does not cause a draw per event.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt::Display;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use thiserror::Error;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::domains::GoalStatus;
use crate::layout::{
    Breakpoint, FocusDirection, LayoutModel, LayoutSnapshot, PaneId, PaneRect, TabId, Visibility,
};
use crate::slash::{self, BlueprintAction, InputRoute, LayoutAction, PaneAction, SlashCommand};

/// Bound retained transcript memory even when a provider streams forever.
pub const DEFAULT_MAX_MESSAGES: usize = 2_048;
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;
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
pub const DEFAULT_FRAME_INTERVAL: Duration = Duration::from_millis(16);
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const MIN_LOOP_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
    System,
    Error,
}

impl MessageRole {
    fn label(self) -> &'static str {
        match self {
            Self::User => "you",
            Self::Assistant => "zenpi",
            Self::Tool => "tool",
            Self::System => "info",
            Self::Error => "error",
        }
    }

    fn style(self) -> Style {
        let color = match self {
            Self::User => Color::Cyan,
            Self::Assistant => Color::Green,
            Self::Tool => Color::Magenta,
            Self::System => Color::Blue,
            Self::Error => Color::Red,
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiMessage {
    pub role: MessageRole,
    pub text: String,
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
            turn_count: session.turns().len(),
            warning_count: session.recovery_warnings().len(),
            timeline,
            earlier_records: start,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
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
                        |(status, attempt)| format!("{} attempt {}", status.as_str(), attempt),
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
fn latest_execution_status(
    execution: &crate::domain_execution::ExecutionStore,
    goals: &[&crate::domains::Goal],
    blueprint: &crate::domains::Blueprint,
    item_id: &str,
) -> Option<(crate::domain_execution::ExecutionStatus, u32)> {
    let mut latest = None;
    for goal in goals.iter().filter(|goal| {
        goal.blueprint_id == blueprint.id
            && goal.blueprint_version == blueprint.version
            && goal.blueprint_digest == blueprint.digest
    }) {
        let Some(receipt) = execution.latest_receipt_for(goal, blueprint, item_id) else {
            continue;
        };
        if latest.is_none_or(|(_, attempt)| receipt.attempt > attempt) {
            latest = Some((receipt.status, receipt.attempt));
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiAction {
    None,
    Submit(String),
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

/// All mutable view state, with bounded queues and UTF-8-safe editing.
#[derive(Debug, Clone)]
pub struct TuiState {
    messages: VecDeque<TuiMessage>,
    input: String,
    cursor: usize,
    status: String,
    busy: bool,
    scroll: usize,
    history: VecDeque<String>,
    history_cursor: Option<usize>,
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
    session_snapshot: Option<SessionPaneSnapshot>,
    max_messages: usize,
    max_history: usize,
    spinner_tick: usize,
    last_area: Rect,
    cached_transcript: Option<(u16, Vec<Line<'static>>)>,
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
            messages: VecDeque::new(),
            input: String::new(),
            cursor: 0,
            status: "Ready".into(),
            busy: false,
            scroll: 0,
            history: VecDeque::new(),
            history_cursor: None,
            input_scroll: 0,
            preferred_column: None,
            fold_tool_logs: false,
            streaming_role: None,
            streaming_job_id: None,
            streaming_message_started: false,
            workspace_layout: LayoutModel::new(TabId::Project),
            workspace_layouts: BTreeMap::new(),
            layout_dirty: false,
            layout_resets: BTreeSet::new(),
            resource_snapshot: None,
            resource_status: ResourcePaneStatus::Idle,
            gantt_snapshot: None,
            gantt_status: GanttPaneStatus::Idle,
            session_snapshot: None,
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
    /// This touches at most the last 32 validated envelopes and performs no I/O.
    pub fn refresh_session_snapshot(&mut self, session: &crate::session::SessionStore) {
        let snapshot = SessionPaneSnapshot::from_session(session);
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

    /// Borrow the layout model used by the production workspace renderer.
    ///
    /// Keeping this as a typed model lets hosts inspect or adjust bounded
    /// ratios/capabilities without coupling themselves to Ratatui rectangles.
    pub fn workspace_layout(&self) -> &LayoutModel {
        &self.workspace_layout
    }

    /// Return a bounded snapshot of every tab's user-owned state. The active
    /// tab is first, followed by inactive tabs in stable `TabId` order.
    /// Capabilities are retained in memory for the host but omitted by the
    /// config persistence layer.
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
            (self.last_area.width, self.last_area.height)
        }
    }

    /// Select a workspace tab and invalidate the next frame. Each tab keeps
    /// its own ratios, collapsed panes, and focus in memory; switching does
    /// not mark the preference document dirty by itself.
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
                self.scroll = 0;
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
        }
    }

    pub fn clear_messages(&mut self) {
        if !self.messages.is_empty()
            || self.streaming_role.is_some()
            || self.streaming_job_id.is_some()
        {
            self.messages.clear();
            self.streaming_role = None;
            self.streaming_job_id = None;
            self.streaming_message_started = false;
            self.cached_transcript = None;
            self.scroll = 0;
            self.dirty = true;
        }
    }

    pub fn push_message(&mut self, role: MessageRole, text: impl Into<String>) {
        self.messages.push_back(TuiMessage::new(role, text));
        self.cached_transcript = None;
        while self.messages.len() > self.max_messages {
            self.messages.pop_front();
        }
        self.scroll = 0;
        self.dirty = true;
    }

    /// Merge adjacent stream chunks to avoid one allocation per token.
    pub fn append_stream(&mut self, role: MessageRole, chunk: impl AsRef<str>) {
        self.streaming_job_id = None;
        self.streaming_message_started = false;
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
            self.scroll = 0;
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
        self.streaming_role = None;
        self.streaming_job_id = Some(job_id);
        self.streaming_message_started = false;
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
        if !self.streaming_message_started {
            self.push_message(role, chunk.as_ref().to_owned());
            self.streaming_role = Some(role);
            self.streaming_message_started = true;
            return;
        }
        self.append_stream_content(role, chunk.as_ref());
    }

    /// Replace the provisional streamed message with the final provider
    /// content.  A provider may emit no deltas (for example a compatibility
    /// backend or a stream that was coalesced), in which case this appends one
    /// normal message.  The role marker prevents a later turn from rewriting
    /// an older assistant message.
    pub fn finish_stream(&mut self, role: MessageRole, content: impl Into<String>) {
        self.finish_stream_impl(role, content.into());
        self.streaming_job_id = None;
        self.streaming_message_started = false;
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
                self.scroll = 0;
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
    }

    /// Stop tracking a provisional stream while retaining any text already
    /// shown.  This is used for cancellation, refusal, and provider errors;
    /// dropping the marker ensures the next turn cannot rewrite the partial
    /// message.
    pub fn discard_stream(&mut self) {
        self.streaming_role = None;
        self.streaming_job_id = None;
        self.streaming_message_started = false;
    }

    pub fn set_input(&mut self, input: impl Into<String>) {
        self.input = bound_text(sanitize_input(input.into()));
        self.cursor = self.input.len();
        self.history_cursor = None;
        self.input_scroll = 0;
        self.preferred_column = None;
        self.dirty = true;
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_add(amount.max(1));
        self.dirty = true;
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount.max(1));
        self.dirty = true;
    }

    pub fn handle_event(&mut self, event: Event) -> TuiAction {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key),
            Event::Paste(text) => {
                self.insert_text(&text);
                TuiAction::None
            }
            Event::Resize(_, _) => {
                self.scroll = 0;
                self.dirty = true;
                TuiAction::Redraw
            }
            _ => TuiAction::None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> TuiAction {
        let modifiers = key.modifiers;
        if modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
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
                    self.input.drain(..self.cursor);
                    self.cursor = 0;
                    self.input_scroll = 0;
                    self.preferred_column = None;
                    self.dirty = true;
                    return TuiAction::None;
                }
                KeyCode::Char('w') => {
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
                // Workspace tabs are intentionally keyboard-only for now so
                // the existing prompt workflow remains unchanged.  The tab
                // bar mirrors these stable numbers in the BentoBox view.
                KeyCode::Char(character) if ('1'..='5').contains(&character) => {
                    let index = usize::from(character as u8 - b'1');
                    if let Some(tab) = TabId::ALL.get(index).copied() {
                        self.set_workspace_tab(tab);
                    }
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
                if !modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) =>
            {
                self.insert_text(&character.to_string())
            }
            KeyCode::Backspace => self.delete_previous_char(),
            KeyCode::Delete => self.delete_next_char(),
            KeyCode::Left => {
                self.cursor = previous_boundary(&self.input, self.cursor);
                self.preferred_column = None;
            }
            KeyCode::Right => {
                self.cursor = next_boundary(&self.input, self.cursor);
                self.preferred_column = None;
            }
            // Home/End operate on the current logical line, as users expect
            // in a multiline prompt.  Ctrl-Home/Ctrl-End still address the
            // complete buffer.
            KeyCode::Home => {
                self.cursor = if modifiers.contains(KeyModifiers::CONTROL) {
                    0
                } else {
                    line_start(&self.input, self.cursor)
                };
                self.preferred_column = None;
            }
            KeyCode::End => {
                self.cursor = if modifiers.contains(KeyModifiers::CONTROL) {
                    self.input.len()
                } else {
                    line_end(&self.input, self.cursor)
                };
                self.preferred_column = None;
            }
            KeyCode::Up if self.input.is_empty() => self.history_move(-1),
            KeyCode::Down if self.input.is_empty() => self.history_move(1),
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
                // Use trimming only to decide whether the buffer is blank;
                // preserve the submitted bytes themselves. Leading spaces,
                // indentation, and a deliberate trailing newline are part of
                // a code-oriented prompt and must not be rewritten by the
                // terminal host.
                if !self.input.trim().is_empty() {
                    let text = self.input.clone();
                    self.history.push_back(text.clone());
                    while self.history.len() > self.max_history {
                        self.history.pop_front();
                    }
                    self.input.clear();
                    self.cursor = 0;
                    self.history_cursor = None;
                    self.input_scroll = 0;
                    self.preferred_column = None;
                    self.scroll = 0;
                    self.dirty = true;
                    return TuiAction::Submit(text);
                }
            }
            KeyCode::Esc => {
                if self.input.is_empty() {
                    return TuiAction::Interrupt;
                }
                self.input.clear();
                self.cursor = 0;
                self.history_cursor = None;
                self.input_scroll = 0;
                self.preferred_column = None;
                self.dirty = true;
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
        if self.cursor != self.input.len() {
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
        self.input.replace_range(leading..self.cursor, &replacement);
        self.cursor = leading.saturating_add(replacement.len());
        self.history_cursor = None;
        self.input_scroll = 0;
        self.preferred_column = None;
        self.dirty = true;
        true
    }

    fn insert_text(&mut self, text: &str) {
        let remaining = MAX_MESSAGE_BYTES.saturating_sub(self.input.len());
        let sanitized = sanitize_input_with_limit(text, remaining);
        let text = truncate_bytes(&sanitized, remaining);
        if text.is_empty() {
            return;
        }
        self.input.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.history_cursor = None;
        self.input_scroll = 0;
        self.preferred_column = None;
        self.dirty = true;
    }

    fn delete_previous_char(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = previous_boundary(&self.input, self.cursor);
        self.input.drain(start..self.cursor);
        self.cursor = start;
        self.input_scroll = 0;
        self.preferred_column = None;
        self.dirty = true;
    }

    fn delete_next_char(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        let end = next_boundary(&self.input, self.cursor);
        self.input.drain(self.cursor..end);
        self.input_scroll = 0;
        self.preferred_column = None;
        self.dirty = true;
    }

    fn delete_previous_word(&mut self) {
        let before = &self.input[..self.cursor];
        let trimmed = before.trim_end_matches(char::is_whitespace);
        let start = trimmed
            .rfind(char::is_whitespace)
            .map_or(0, |index| next_boundary(trimmed, index));
        self.input.drain(start..self.cursor);
        self.cursor = start;
        self.input_scroll = 0;
        self.preferred_column = None;
        self.dirty = true;
    }

    fn history_move(&mut self, direction: isize) {
        if self.history.is_empty() {
            return;
        }
        let current = self.history_cursor.unwrap_or(self.history.len());
        let next = if direction < 0 {
            current.saturating_sub(1)
        } else {
            (current + 1).min(self.history.len())
        };
        self.history_cursor = (next < self.history.len()).then_some(next);
        self.input = self
            .history_cursor
            .and_then(|index| self.history.get(index).cloned())
            .unwrap_or_default();
        self.cursor = self.input.len();
        self.input_scroll = 0;
        self.preferred_column = None;
        self.dirty = true;
    }

    /// Move to the adjacent logical line while retaining the requested
    /// display-cell column.  Every offset is derived from UTF-8 boundaries,
    /// so a wide or multibyte glyph can never leave `cursor` mid-codepoint.
    fn move_vertical(&mut self, direction: isize) {
        let current_start = line_start(&self.input, self.cursor);
        let current_end = line_end(&self.input, self.cursor);
        let current_column = UnicodeWidthStr::width(&self.input[current_start..self.cursor]);
        let target_column = self.preferred_column.unwrap_or(current_column);
        let target_start = if direction < 0 {
            if current_start == 0 {
                return;
            }
            let previous_end = current_start.saturating_sub(1);
            line_start(&self.input, previous_end)
        } else {
            if current_end >= self.input.len() {
                return;
            }
            current_end + 1
        };
        let target_end = line_end(&self.input, target_start);
        self.cursor = byte_at_column(&self.input[target_start..target_end], target_column)
            .saturating_add(target_start)
            .min(target_end);
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
            self.scroll = 0;
            self.input_scroll = 0;
        }
        let prompt_width = usize::from(area.width.saturating_sub(2)).max(1);
        let prompt_lines = wrap_plain(&self.input, prompt_width).len();
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
                Span::styled(self.status.as_str(), style),
            ])),
            area,
        );
    }

    fn render_transcript(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let title = if self.fold_tool_logs {
            " Conversation (tools folded) "
        } else {
            " Conversation "
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(title);
        let inner = block.inner(area);
        let width = usize::from(inner.width).max(1);
        if self
            .cached_transcript
            .as_ref()
            .is_none_or(|(cached_width, _)| *cached_width != inner.width)
        {
            let messages = self.transcript_messages();
            let mut lines = transcript_lines(&messages, width);
            if lines.len() > MAX_RENDER_LINES {
                lines.drain(..lines.len() - MAX_RENDER_LINES);
            }
            self.cached_transcript = Some((inner.width, lines));
        }
        let lines = self
            .cached_transcript
            .as_ref()
            .map_or(&[][..], |(_, lines)| lines.as_slice());
        let visible = usize::from(inner.height);
        let max_scroll = lines.len().saturating_sub(visible);
        let offset = self.scroll.min(max_scroll);
        let start = max_scroll.saturating_sub(offset);
        let end = (start + visible).min(lines.len());
        let displayed = if start < end {
            lines[start..end].to_vec()
        } else {
            Vec::new()
        };
        frame.render_widget(Paragraph::new(Text::from(displayed)).block(block), area);
    }

    /// Build the render-only message view.  Folding never mutates the
    /// canonical bounded queue, which means expanding the logs restores the
    /// exact lifecycle entries and ordinary transcript ordering.
    fn transcript_messages(&self) -> VecDeque<TuiMessage> {
        if !self.fold_tool_logs {
            return self.messages.clone();
        }
        let mut visible = VecDeque::new();
        let mut folded = 0usize;
        for message in &self.messages {
            if message.role == MessageRole::Tool {
                folded = folded.saturating_add(1);
            } else {
                visible.push_back(message.clone());
            }
        }
        if folded > 0 {
            let suffix = if folded == 1 { "" } else { "s" };
            visible.push_back(TuiMessage::new(
                MessageRole::System,
                format!("{folded} tool log{suffix} folded; press Ctrl-O to expand"),
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
            .title(" Prompt ");
        let inner = block.inner(area);
        let width = usize::from(inner.width).max(1);
        let lines = wrap_plain(&self.input, width)
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>();
        let visible_height = usize::from(inner.height);
        let (cursor_x, cursor_y) = cursor_position(&self.input, self.cursor, width);
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

    fn render_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let text = truncate_to_width(
            " Enter send  |  Shift+Enter newline  |  Ctrl-C interrupt  |  Ctrl-D quit  |  PgUp/PgDn scroll ",
            usize::from(area.width),
        );
        frame.render_widget(
            Paragraph::new(Span::styled(text, Style::default().fg(Color::DarkGray))),
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
            self.scroll = 0;
            self.input_scroll = 0;
        }
        let prompt_width = usize::from(area.width.saturating_sub(2)).max(1);
        let prompt_lines = wrap_plain(&self.input, prompt_width).len();
        let desired_input_height = prompt_lines.min(MAX_INPUT_LINES).saturating_add(2);
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
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(input_height),
                Constraint::Length(1),
            ])
            .split(area);
        self.render_header(frame, chunks[0], title);
        self.render_workspace_tabs(frame, chunks[1]);
        self.render_workspace(frame, chunks[2]);
        self.render_input(frame, chunks[3]);
        self.render_footer(frame, chunks[4]);
    }

    fn render_workspace_tabs(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let active = self.workspace_layout.tab;
        let mut spans = Vec::with_capacity(TabId::ALL.len());
        for (index, tab) in TabId::ALL.into_iter().enumerate() {
            let style = if tab == active {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            spans.push(Span::styled(
                format!(" {}:{} ", index + 1, tab.as_str()),
                style,
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_workspace(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let adapter = BentoBoxLayoutAdapter::new(&self.workspace_layout, area);
        let canonical = conversation_pane_for_tab(adapter.tab());
        for pane in adapter.visible_panes() {
            if pane.rect.width == 0 || pane.rect.height == 0 {
                continue;
            }
            if pane.id == canonical {
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
        let title = workspace_pane_title(id);
        let content = match id {
            PaneId::Gantt => self.gantt_pane_content(),
            PaneId::Resources => self.resource_pane_content(),
            PaneId::SessionList => self.session_pane_content(),
            PaneId::ReplayControls => self.replay_pane_content(),
            PaneId::EventTimeline => self.session_timeline_content(),
            PaneId::GoalConversation
            | PaneId::ProjectConversation
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
            | PaneId::Browser
            | PaneId::Terminal => "-".into(),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(title);
        frame.render_widget(
            Paragraph::new(content)
                .style(Style::default().fg(Color::DarkGray))
                .block(block)
                .wrap(Wrap { trim: true }),
            area,
        );
    }

    fn session_pane_content(&self) -> String {
        match &self.session_snapshot {
            Some(snapshot) => format!(
                "Active session\n{}\nTurns {}\nRecovery warnings {}\nMailbox: owner required",
                snapshot.session_id, snapshot.turn_count, snapshot.warning_count,
            ),
            None => "Session unavailable\nMailbox: owner required".into(),
        }
    }

    fn replay_pane_content(&self) -> String {
        match &self.session_snapshot {
            Some(snapshot) => format!(
                "Journal next sequence: {}\nTransport ACK: unavailable\nReconnect: owner required",
                snapshot.journal_next_sequence,
            ),
            None => "Journal cursor unavailable\nReconnect: owner required".into(),
        }
    }

    fn session_timeline_content(&self) -> String {
        let Some(snapshot) = &self.session_snapshot else {
            return "Journal unavailable".into();
        };
        let mut content = snapshot.timeline.join("\n");
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
        let process = snapshot
            .process
            .resident_bytes
            .map(format_byte_count)
            .unwrap_or_else(|| "unavailable".into());
        let suffix = match &self.resource_status {
            ResourcePaneStatus::Collecting => "\nrefreshing...".into(),
            ResourcePaneStatus::Failed(error) => format!("\nrefresh failed: {error}"),
            ResourcePaneStatus::Idle | ResourcePaneStatus::Ready => String::new(),
        };
        format!(
            "workspace\nfiles {files}  dirs {directories}\nsize {bytes}  nodes {nodes}\ntruncated {truncated}\nhost\ncpu {cpus}  load {load}\nmem free {memory}\nprocess {process}{suffix}",
            files = workspace.files,
            directories = workspace.directories,
            bytes = format_byte_count(workspace.bytes),
            nodes = workspace.nodes,
            truncated = workspace.truncated,
            cpus = snapshot.cpu.logical_cpus,
            load = snapshot
                .cpu
                .load_one_minute
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| "unavailable".into()),
        )
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
        PaneId::ProjectConversation => "Project",
        PaneId::Resources => "Resources",
        PaneId::GoalConversation => "Goal",
        PaneId::Gantt => "Gantt",
        PaneId::Browser => "Browser",
        PaneId::Terminal => "Terminal",
        PaneId::LearnConversation => "Learn",
        PaneId::LearnResources => "Learn resources",
        PaneId::LearnQueue => "Learn queue",
        PaneId::LearnMapping => "Learn mapping",
        PaneId::Evidence => "Evidence",
        PaneId::ReviewConversation => "Review",
        PaneId::Checks => "Checks",
        PaneId::ApprovalQueue => "Approval queue",
        PaneId::Diff => "Diff",
        PaneId::SessionList => "Sessions",
        PaneId::SessionConversation => "Session",
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
/// `show|list [id]`, `status [id] [state]`, and `transition <id> <state>`.
#[derive(Debug)]
enum TuiGoalOwnerAction {
    ShowAll,
    Show(String),
    Transition(String, GoalStatus),
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
    if !is_show && !is_status && !is_transition {
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
    let usage = || "use `/goal status`, `/goal status <goal-id>`, or `/goal status <goal-id> <queued|running|paused|blocked|cancelled|done>`";

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
    if tokens.len() != 3 {
        return Err(usage());
    }
    let id = valid_id(tokens[1])?;
    let status = GoalStatus::parse_token(tokens[2])
        .ok_or("goal status must be queued, running, paused, blocked, cancelled, or done")?;
    Ok(Some(TuiGoalOwnerAction::Transition(id, status)))
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
        SlashCommand::Help { topic } => match slash::help(topic.as_deref()) {
            Some(help) => state.push_message(MessageRole::System, help),
            None => state.push_message(
                MessageRole::Error,
                "unknown help topic; type /help for available commands",
            ),
        },
        SlashCommand::Model { name } => {
            let host_available = agent.is_some();
            let requested_change = name.is_some();
            let message = if let Some(agent) = agent.as_deref_mut() {
                match name.as_ref() {
                    Some(name) => match agent.set_model(Some(name.clone())) {
                        Ok(()) => format!("model selected: {}", inline_token(name, 160)),
                        Err(error) => format!("model change failed: {error}"),
                    },
                    None => format!(
                        "model: {}",
                        agent
                            .snapshot()
                            .model
                            .as_deref()
                            .unwrap_or("<provider default>")
                    ),
                }
            } else {
                match name.as_ref() {
                    Some(name) => format!(
                        "model change unavailable while the agent is busy: {}",
                        inline_token(name, 160)
                    ),
                    None => "model inspection unavailable while the agent is busy".into(),
                }
            };
            let role = if host_available || !requested_change {
                MessageRole::System
            } else {
                MessageRole::Error
            };
            state.push_message(role, message);
        }
        SlashCommand::Models => match crate::config::model_catalog(None) {
            Ok(entries) => {
                let text = if entries.is_empty() {
                    "models: no configured profiles".to_owned()
                } else {
                    entries
                        .into_iter()
                        .map(|entry| {
                            format!(
                                "{}{} model={} provider={} backend={} {}",
                                if entry.active { "* " } else { "  " },
                                entry.profile.as_deref().unwrap_or("default"),
                                entry.model.as_deref().unwrap_or("<unset>"),
                                entry.provider.as_deref().unwrap_or("<unset>"),
                                entry.backend,
                                if entry.configured {
                                    "ready"
                                } else {
                                    "incomplete"
                                },
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                state.push_message(MessageRole::System, text);
            }
            Err(error) => state.push_message(
                MessageRole::Error,
                format!("model catalogue unavailable: {error}"),
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
            match crate::headless::collect_resource_snapshot(path.as_deref()) {
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
                match crate::headless::domain_store_view(agent.as_deref()) {
                    Ok(data) => state.push_message(
                        MessageRole::System,
                        format!(
                            "goal show:\n{}",
                            bounded_display(
                                &serde_json::to_string(&data)
                                    .unwrap_or_else(|_| { "{\"goals\":[]}".into() })
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
            state.push_message(
                MessageRole::Error,
                format!(
                    "plan command is parsed but not executable in this host: {}",
                    instruction
                        .as_deref()
                        .map(bounded_display)
                        .unwrap_or_else(|| "missing instruction".into())
                ),
            );
        }
        SlashCommand::Session { action } => match action {
            crate::slash::SessionAction::List => match crate::headless::session_list_view() {
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
            },
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
            action @ (crate::slash::SessionAction::Fork { .. }
            | crate::slash::SessionAction::Export { .. }
            | crate::slash::SessionAction::Import { .. }) => {
                match crate::headless::session_maintenance_view(&action) {
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
            match crate::slash_actions::diff_value_for_agent(agent.as_deref(), path.as_deref()) {
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
                        Ok(data) => state.push_message(
                            MessageRole::System,
                            format!(
                                "blueprint {}:\n{}",
                                if matches!(action, BlueprintAction::Show) {
                                    "show"
                                } else {
                                    "status"
                                },
                                bounded_display(
                                    &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                                )
                            ),
                        ),
                        Err(error) => state.push_message(
                            MessageRole::Error,
                            format!("blueprint store unavailable: {error}"),
                        ),
                    }
                }
                BlueprintAction::Validate { path: Some(path) } => {
                    match crate::headless::blueprint_validation_view(&path) {
                        Ok(data) => state.push_message(
                            MessageRole::System,
                            format!(
                                "blueprint validate:\n{}",
                                bounded_display(
                                    &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                                )
                            ),
                        ),
                        Err(error) => state.push_message(
                            MessageRole::Error,
                            format!("blueprint validation failed: {error}"),
                        ),
                    }
                }
                BlueprintAction::Validate { path: None } => {
                    match crate::headless::domain_store_view(agent.as_deref()) {
                        Ok(data) => state.push_message(
                            MessageRole::System,
                            format!(
                                "blueprint validate:\n{}",
                                bounded_display(
                                    &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                                )
                            ),
                        ),
                        Err(error) => state.push_message(
                            MessageRole::Error,
                            format!("blueprint validation failed: {error}"),
                        ),
                    }
                }
                BlueprintAction::Put { path } => match agent.as_deref_mut() {
                    Some(agent) => match crate::headless::persist_blueprint_path(agent, &path) {
                        Ok(data) => state.push_message(
                            MessageRole::System,
                            format!(
                                "blueprint persisted:\n{}",
                                bounded_display(
                                    &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                                )
                            ),
                        ),
                        Err(error) => state.push_message(
                            MessageRole::Error,
                            format!("blueprint persistence failed: {error}"),
                        ),
                    },
                    None => state.push_message(
                        MessageRole::Error,
                        "blueprint persistence is unavailable while the agent is busy",
                    ),
                },
                BlueprintAction::Run { target } => match agent.as_deref_mut() {
                    Some(agent) => match crate::headless::run_blueprint_next(agent, &target) {
                        Ok(data) => state.push_message(
                            MessageRole::System,
                            format!(
                                "blueprint run:\n{}",
                                bounded_display(
                                    &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                                )
                            ),
                        ),
                        Err(error) => state.push_message(
                            MessageRole::Error,
                            format!("blueprint run failed: {error}"),
                        ),
                    },
                    None => state.push_message(
                        MessageRole::Error,
                        "blueprint execution is unavailable while the agent is busy",
                    ),
                },
                BlueprintAction::Handoff { target } => match agent.as_deref_mut() {
                    Some(agent) => match crate::headless::handoff_blueprint_next(agent, &target) {
                        Ok(data) => state.push_message(
                            MessageRole::System,
                            format!(
                                "blueprint handoff:\n{}",
                                bounded_display(
                                    &serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
                                )
                            ),
                        ),
                        Err(error) => state.push_message(
                            MessageRole::Error,
                            format!("blueprint handoff failed: {error}"),
                        ),
                    },
                    None => state.push_message(
                        MessageRole::Error,
                        "blueprint handoff is unavailable while the agent is busy",
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
    }
    if !state.is_busy() {
        state.set_status("Ready");
    }
    SlashDispatchAction::Continue
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

fn reload_state_from_agent(state: &mut TuiState, agent: &crate::core::Agent) {
    state.clear_messages();
    for turn in agent.history() {
        let role = match turn.role {
            crate::core::TurnRole::User => MessageRole::User,
            crate::core::TurnRole::Assistant => MessageRole::Assistant,
            crate::core::TurnRole::Tool => MessageRole::Tool,
            crate::core::TurnRole::System => MessageRole::System,
        };
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
        let turns = agent.history();
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
        BlueprintAction::Open { path } => format!("open {}", bounded_display(path)),
    }
}

fn session_action_label(action: &crate::slash::SessionAction) -> &'static str {
    match action {
        crate::slash::SessionAction::List => "list",
        crate::slash::SessionAction::Open { .. } => "open",
        crate::slash::SessionAction::Fork { .. } => "fork",
        crate::slash::SessionAction::Export { .. } => "export",
        crate::slash::SessionAction::Import { .. } => "import",
        crate::slash::SessionAction::Gc { .. } => "gc",
    }
}

fn bounded_display(value: &str) -> String {
    inline_token(value, 512)
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
        "resources: files={} dirs={} bytes={} truncated={} cpu={} memory={} disk={}",
        snapshot.workspace.files,
        snapshot.workspace.directories,
        snapshot.workspace.bytes,
        snapshot.workspace.truncated,
        signal_status(snapshot.cpu.status),
        signal_status(snapshot.memory.status),
        signal_status(snapshot.disk.status),
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

/// Run the interactive mode for zenpi's shared agent.
pub fn run(agent: &mut crate::core::Agent) -> Result<(), crate::error::ZenpiError> {
    let mut state = TuiState::default();
    for turn in agent.history() {
        let role = match turn.role {
            crate::core::TurnRole::User => MessageRole::User,
            crate::core::TurnRole::Assistant => MessageRole::Assistant,
            crate::core::TurnRole::Tool => MessageRole::Tool,
            crate::core::TurnRole::System => MessageRole::System,
        };
        state.push_message(role, &turn.content);
    }
    let config = TuiConfig::default();
    run_with_state(config, state, |text, state| {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TuiRequestKind {
    Model,
    UserShell,
}

enum TuiJobResult {
    Model(crate::core::ProcessResult),
    UserShell(serde_json::Value),
}

struct TuiRequest {
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
                text,
                kind,
                provider_events: Arc::clone(&provider_events),
            },
            provider_events,
        )
    }
}

const MAX_TUI_PENDING_INPUTS: usize = 16;
const MAX_TUI_PENDING_INPUT_BYTES: usize = 256 * 1024;

/// Shell input must not steer a provider turn, and typing during a shell must
/// not cancel it. Keep this FIFO separate from the runtime's model steer lane.
#[derive(Default)]
struct TuiPendingInputs {
    inputs: VecDeque<(TuiRequestKind, String)>,
    bytes: usize,
}

impl TuiPendingInputs {
    fn should_queue(&self, kind: TuiRequestKind, active: Option<TuiRequestKind>) -> bool {
        (kind == TuiRequestKind::UserShell && active.is_some())
            || active == Some(TuiRequestKind::UserShell)
            || !self.inputs.is_empty()
    }

    fn push(&mut self, kind: TuiRequestKind, text: String) -> Result<(), &'static str> {
        if self.inputs.len() >= MAX_TUI_PENDING_INPUTS
            || self.bytes.saturating_add(text.len()) > MAX_TUI_PENDING_INPUT_BYTES
        {
            return Err("Input queue full; input was not submitted");
        }
        self.bytes += text.len();
        self.inputs.push_back((kind, text));
        Ok(())
    }

    fn pop(&mut self) -> Option<(TuiRequestKind, String)> {
        let input = self.inputs.pop_front()?;
        self.bytes -= input.1.len();
        Some(input)
    }

    fn cancel(&mut self, state: &mut TuiState) {
        let count = self.inputs.len();
        self.inputs.clear();
        self.bytes = 0;
        if count != 0 {
            state.push_message(
                MessageRole::System,
                format!("Cancelled {count} queued input(s)"),
            );
        }
    }

    fn admit(&mut self, kind: TuiRequestKind, text: String, state: &mut TuiState) {
        match self.push(kind, text) {
            Ok(()) => {
                state.push_message(
                    MessageRole::System,
                    format!("Input queued ({})", self.inputs.len()),
                );
            }
            Err(error) => state.push_message(MessageRole::Error, error),
        }
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
#[derive(Debug, Default)]
struct TuiProviderEventBuffer {
    events: VecDeque<crate::backend::ProviderEvent>,
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

    fn take(&mut self) -> (VecDeque<crate::backend::ProviderEvent>, u64) {
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
fn serialized_provider_event_bytes(event: &crate::backend::ProviderEvent) -> usize {
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
        let layouts = crate::layout::TabId::ALL
            .into_iter()
            .map(|tab| {
                preferences
                    .model_for(self.profile.as_deref(), tab)
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        state.restore_workspace_layouts(layouts);
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
        for layout in state.workspace_layout_states() {
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
        Ok(())
    }

    fn disable(&mut self) {
        self.disabled = true;
    }
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

    let shared = Arc::new(Mutex::new(agent));
    let approval = shared
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
            let mut agent = worker_state
                .lock()
                .map_err(|_| AgentError::InvalidTurn("agent lock poisoned".into()))?;
            let result = match request.kind {
                TuiRequestKind::Model => {
                    TuiJobResult::Model(agent.process_with_cancel_and_events(
                        crate::core::TurnInputRequest::new(request.text),
                        || token.is_cancelled(),
                        &mut |event| {
                            if let Ok(mut pending) = request.provider_events.lock() {
                                pending.push(event, MAX_TUI_PROVIDER_EVENTS);
                            }
                            Ok(())
                        },
                    )?)
                }
                TuiRequestKind::UserShell => {
                    let command = request.text;
                    let result =
                        agent.run_user_shell_with_cancel(&command, || token.is_cancelled())?;
                    TuiJobResult::UserShell(result)
                }
            };
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
    let resource_runner = BackgroundRunner::spawn(
        |path: Option<String>, token| -> Result<crate::resources::ResourceSnapshot, String> {
            if token.is_cancelled() {
                return Err("resource refresh cancelled".into());
            }
            let snapshot = crate::headless::collect_resource_snapshot(path.as_deref())
                .map_err(|error| error.to_string())?;
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
    let mut state = TuiState::default();
    if let Ok(agent) = shared.lock() {
        for turn in agent.history() {
            let role = match turn.role {
                crate::core::TurnRole::User => MessageRole::User,
                crate::core::TurnRole::Assistant => MessageRole::Assistant,
                crate::core::TurnRole::Tool => MessageRole::Tool,
                crate::core::TurnRole::System => MessageRole::System,
            };
            state.push_message(role, &turn.content);
        }
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
    match resource_runner.try_submit(None) {
        Ok(_) => state.resource_refresh_started(),
        Err(error) => state.resource_refresh_failed(error.to_string()),
    }
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
    // The runtime worker is spawned before terminal setup so it can own the
    // agent independently of the terminal.  Terminal setup can still fail
    // (for example when stdin is not a TTY), so every early return here must
    // close and join the worker instead of relying on `Drop` to detach it.
    let mut guard = match TerminalGuard::enter() {
        Ok(guard) => guard,
        Err(error) => {
            let _ = runner.shutdown_and_join();
            let _ = resource_runner.shutdown_and_join();
            let _ = gantt_runner.shutdown_and_join();
            return Err(crate::error::ZenpiError::Message(error.to_string()));
        }
    };
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = runner.shutdown_and_join();
            let _ = resource_runner.shutdown_and_join();
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
    let mut active_job = None;
    let mut active_job_kind = None;
    let mut pending_inputs = TuiPendingInputs::default();
    let mut stream_buffers: HashMap<crate::runtime::JobId, Arc<Mutex<TuiProviderEventBuffer>>> =
        HashMap::new();
    let mut pending_approvals = VecDeque::new();
    let loop_result = (|| -> Result<(), crate::error::ZenpiError> {
        'outer: loop {
            if guard.termination_requested() {
                break;
            }
            // The worker holds the agent mutex while provider I/O is in flight;
            // use a non-blocking drain so lifecycle events become visible as soon
            // as that mutex is released without freezing keyboard/render polling.
            drain_agent_tool_events(&shared, &mut state);
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
                    RuntimeEvent::Completed { outcome, .. } => {
                        match outcome {
                            JobOutcome::Succeeded(snapshot) => {
                                state.set_resource_snapshot(snapshot);
                            }
                            JobOutcome::Failed(error) => state.resource_refresh_failed(error),
                            JobOutcome::Cancelled => {
                                state.resource_refresh_failed("resource refresh cancelled")
                            }
                            JobOutcome::Panicked => {
                                state.resource_refresh_failed("resource worker panicked")
                            }
                        }
                        scheduler.request();
                    }
                    RuntimeEvent::Rejected { reason, .. } => {
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
                                state.set_gantt_snapshot(result.snapshot)
                            }
                            JobOutcome::Succeeded(_) => {}
                            JobOutcome::Failed(error)
                                if completed_generation == Some(gantt_tracker.generation) =>
                            {
                                state.gantt_refresh_failed(error)
                            }
                            JobOutcome::Failed(_) => {}
                            JobOutcome::Cancelled => {
                                if completed_generation == Some(gantt_tracker.generation) {
                                    state.gantt_refresh_failed("Gantt refresh cancelled");
                                }
                            }
                            JobOutcome::Panicked => {
                                if completed_generation == Some(gantt_tracker.generation) {
                                    state.gantt_refresh_failed("Gantt worker panicked");
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
                for request in coordinator.drain_pending() {
                    let preview = request
                        .preview
                        .as_ref()
                        .map(|preview| format!("\n\nProposed change:\n{}", preview.display_text()))
                        .unwrap_or_default();
                    let safe_arguments = crate::security::redact_json(&request.arguments, &[]);
                    state.push_message(
                        // Keep the approval prompt visible while tool logs are
                        // folded; hiding it would leave the user with no way to
                        // know what the pending y/n response refers to.
                        MessageRole::System,
                        format!(
                            "Approval required: {} {}\nRequest: {}\nPolicy: {}{preview}\nType y to allow once, n to deny.",
                            request.tool, safe_arguments, request.request_id,
                            request.policy_digest.as_deref().unwrap_or("unbound"),
                        ),
                    );
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
                        active_job = None;
                        active_job_kind = None;
                        state.set_busy(false);
                        drain_agent_tool_events(&shared, &mut state);
                        match outcome {
                            JobOutcome::Succeeded(TuiJobResult::Model(result)) => {
                                if let Some(assistant) = result.assistant {
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
                            JobOutcome::Succeeded(TuiJobResult::UserShell(result)) => {
                                render_user_shell_result(&mut state, &result);
                            }
                            JobOutcome::Failed(error) => {
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
                        scheduler.request();
                    }
                    RuntimeEvent::Rejected { id, reason } => {
                        let _ = stream_buffers.remove(&id);
                        if Some(id) != active_job {
                            continue;
                        }
                        // A rejected active request cannot produce a valid
                        // approval response. Do not let its prompt swallow
                        // the next user turn.
                        pending_approvals.clear();
                        active_job = None;
                        active_job_kind = None;
                        state.discard_stream();
                        state.set_busy(false);
                        state.push_message(MessageRole::Error, reason.to_string());
                        state.set_status("Request rejected");
                        scheduler.request();
                    }
                    RuntimeEvent::CancelRequested { id } if Some(id) == active_job => {
                        // Cancellation can also be requested by the runtime
                        // (rather than directly by the key handler).  Treat
                        // that acknowledgement as the terminal boundary for
                        // any approval prompt belonging to this turn.
                        pending_approvals.clear();
                        state.set_status("Interrupt requested");
                        scheduler.request();
                    }
                    RuntimeEvent::Closed => break,
                    _ => {}
                }
            }
            // Admit exactly one queued input after the previous job has crossed
            // its terminal boundary. FIFO admission prevents a later prompt
            // from overtaking a local shell command and never calls the model
            // for a `!` request.
            if active_job.is_none()
                && let Some((kind, text)) = pending_inputs.pop()
            {
                let display = if kind == TuiRequestKind::UserShell {
                    format!("!{text}")
                } else {
                    text.clone()
                };
                let request_text = if kind == TuiRequestKind::UserShell {
                    format!("!{text}")
                } else {
                    text
                };
                let (request, events) = TuiRequest::with_kind(request_text, kind);
                match runner.try_submit(request) {
                    Ok(id) => {
                        stream_buffers.insert(id, events);
                        if kind == TuiRequestKind::Model {
                            state.begin_stream_for_job(id.get());
                        } else {
                            state.push_message(MessageRole::User, display);
                            state.set_status("Local shell running");
                        }
                        active_job = Some(id);
                        active_job_kind = Some(kind);
                        state.set_busy(true);
                    }
                    Err(error) => {
                        state.push_message(MessageRole::Error, error.to_string());
                        state.set_status("Request rejected");
                    }
                }
            }
            let now = Instant::now();
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
                match state.handle_event(event) {
                    TuiAction::Submit(text) => {
                        // Parse before checking an approval prompt.  A slash
                        // command is control-plane input even while a tool is
                        // waiting for confirmation; it must never be mistaken
                        // for a y/n answer or sent to the provider.
                        match slash::route_input(&text) {
                            Err(error) => {
                                state.push_message(MessageRole::User, &text);
                                state.push_message(MessageRole::Error, error.to_string());
                                state.set_status("Command rejected");
                            }
                            Ok(InputRoute::Slash(command)) => {
                                state.push_message(MessageRole::User, &text);
                                let refresh_gantt = command_refreshes_gantt(&command);
                                if let SlashCommand::Resources { path } = command {
                                    match resource_runner.try_submit(path) {
                                        Ok(_) => {
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
                                    let result = approval
                                        .as_ref()
                                        .ok_or(crate::approval::ApprovalError::UnknownRequest)
                                        .and_then(|coordinator| {
                                            coordinator.respond(crate::approval::ApprovalResponse {
                                                request_id: id.clone(),
                                                decision,
                                                remember,
                                            })
                                        });
                                    match result {
                                        Ok(()) => {
                                            pending_approvals
                                                .retain(|request| request.request_id != id);
                                            state.push_message(
                                                MessageRole::System,
                                                match (decision, remember) {
                                                    (
                                                        crate::approval::ApprovalDecision::Allow,
                                                        true,
                                                    ) => "Tool allowed and remembered",
                                                    (
                                                        crate::approval::ApprovalDecision::Allow,
                                                        false,
                                                    ) => "Tool allowed once",
                                                    _ => "Tool denied",
                                                },
                                            );
                                            state.set_status("Working");
                                        }
                                        Err(error) => {
                                            state.push_message(
                                                MessageRole::Error,
                                                format!("approval failed: {error}"),
                                            );
                                            state.set_status("Approval expired");
                                        }
                                    }
                                    scheduler.request();
                                    continue 'outer;
                                }
                                let action = match shared.try_lock() {
                                    Ok(mut agent) => dispatch_slash_command(
                                        command,
                                        &mut state,
                                        Some(&mut agent),
                                    ),
                                    Err(_) => dispatch_slash_command(command, &mut state, None),
                                };
                                match action {
                                    SlashDispatchAction::Interrupt => {
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
                                    pending_inputs.admit(
                                        TuiRequestKind::UserShell,
                                        command,
                                        &mut state,
                                    );
                                } else {
                                    let (request, events) = TuiRequest::with_kind(
                                        format!("!{command}"),
                                        TuiRequestKind::UserShell,
                                    );
                                    match runner.try_submit(request) {
                                        Ok(id) => {
                                            stream_buffers.insert(id, events);
                                            state.push_message(
                                                MessageRole::User,
                                                format!("!{command}"),
                                            );
                                            state.set_busy(true);
                                            state.set_status("Local shell running");
                                            active_job = Some(id);
                                            active_job_kind = Some(TuiRequestKind::UserShell);
                                        }
                                        Err(error) => {
                                            state.push_message(
                                                MessageRole::Error,
                                                error.to_string(),
                                            );
                                            state.set_status("Local shell rejected");
                                        }
                                    }
                                }
                            }
                            Ok(InputRoute::Prompt(text)) => {
                                if let Some(request) = pending_approvals.pop_front() {
                                    let normalized = text.trim().to_ascii_lowercase();
                                    let decision = match normalized.as_str() {
                                        "y" | "yes" | "allow" => {
                                            crate::approval::ApprovalDecision::Allow
                                        }
                                        "n" | "no" | "deny" => {
                                            crate::approval::ApprovalDecision::Deny
                                        }
                                        _ => {
                                            pending_approvals.push_front(request);
                                            // While a local shell approval is on
                                            // screen, ordinary text is queued
                                            // rather than accidentally consumed
                                            // as a response or steering shell.
                                            if pending_approvals
                                                .front()
                                                .is_some_and(|item| item.tool == "user_shell")
                                            {
                                                pending_inputs.admit(
                                                    TuiRequestKind::Model,
                                                    text,
                                                    &mut state,
                                                );
                                            } else {
                                                state.push_message(
                                                    MessageRole::Error,
                                                    "Type y to allow once or n to deny",
                                                );
                                            }
                                            state.set_status("Approval required");
                                            scheduler.request();
                                            continue 'outer;
                                        }
                                    };
                                    let response_result =
                                        if let Some(coordinator) = approval.as_ref() {
                                            coordinator.respond(crate::approval::ApprovalResponse {
                                                request_id: request.request_id,
                                                decision,
                                                remember: false,
                                            })
                                        } else {
                                            Err(crate::approval::ApprovalError::UnknownRequest)
                                        };
                                    match response_result {
                                        Ok(()) => {
                                            state.push_message(
                                                MessageRole::System,
                                                match decision {
                                                    crate::approval::ApprovalDecision::Allow => {
                                                        "Tool allowed once"
                                                    }
                                                    crate::approval::ApprovalDecision::Deny => {
                                                        "Tool denied"
                                                    }
                                                },
                                            );
                                            state.set_status("Working");
                                        }
                                        Err(error) => {
                                            state.push_message(
                                                MessageRole::Error,
                                                error.to_string(),
                                            );
                                            state.set_status("Approval expired");
                                        }
                                    }
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
                                if let Some(id) = active_job {
                                    // A steer cancels the old turn.  Once cancellation
                                    // is admitted, any approval queued for that turn
                                    // is stale and must not intercept the steer text.
                                    let cancel_result = runner.try_cancel(id);
                                    if !matches!(
                                        cancel_result,
                                        Err(crate::runtime::SubmitError::QueueFull)
                                    ) {
                                        pending_approvals.clear();
                                    }
                                    let (request, events) = TuiRequest::new(text);
                                    match runner.try_submit(request) {
                                        Ok(id) => {
                                            stream_buffers.insert(id, events);
                                            state.discard_stream();
                                            state.begin_stream_for_job(id.get());
                                            active_job = Some(id);
                                            active_job_kind = Some(TuiRequestKind::Model);
                                            state.set_busy(true);
                                            state.set_status("Steering");
                                        }
                                        Err(error) => {
                                            state.push_message(
                                                MessageRole::Error,
                                                error.to_string(),
                                            );
                                            state.set_status("Steer rejected");
                                        }
                                    }
                                } else {
                                    let (request, events) = TuiRequest::new(text);
                                    match runner.try_submit(request) {
                                        Ok(id) => {
                                            stream_buffers.insert(id, events);
                                            state.begin_stream_for_job(id.get());
                                            active_job = Some(id);
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
                                        }
                                    }
                                }
                            }
                        }
                    }
                    TuiAction::Interrupt => {
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
                            }
                            state.set_status("Interrupt requested");
                        } else {
                            state.set_status("Ready");
                        }
                    }
                    TuiAction::Quit => break 'outer,
                    TuiAction::Redraw | TuiAction::None => {}
                }
                if state.layout_dirty() && !layout_persistence.disabled {
                    match layout_persistence.save(&state) {
                        Ok(()) => {
                            state.clear_layout_dirty();
                            state.clear_layout_reset_tabs();
                        }
                        Err(error) => {
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
                scheduler.request();
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
    // Restore the terminal before waiting for provider cancellation. A second
    // OS signal can then use the original disposition without stranding raw
    // mode even when an uncooperative network operation delays the join.
    guard.leave();
    // Terminal I/O can fail while a provider job is still active. Always
    // cancel and join the owned runtime before returning that error; relying
    // on `Drop` would detach a worker when its command queue is full.
    let join_result = runner.shutdown_and_join();
    let resource_join_result = resource_runner.shutdown_and_join();
    let gantt_join_result = gantt_runner.shutdown_and_join();
    if let Ok(mut agent) = shared.lock() {
        agent.close();
    }
    match loop_result {
        Err(error) => {
            let _ = join_result;
            Err(error)
        }
        Ok(()) => join_result
            .and(resource_join_result)
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
    mut state: TuiState,
    mut on_submit: F,
) -> Result<(), TuiError>
where
    F: FnMut(String, &mut TuiState) -> Result<(), E>,
    E: Display,
{
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

    'outer: loop {
        if guard.termination_requested() {
            break;
        }
        let now = Instant::now();
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
    guard.leave();
    Ok(())
}

struct TerminalGuard {
    active: bool,
    #[cfg(unix)]
    signals: Option<terminal_signals::SignalGuard>,
}

impl TerminalGuard {
    fn enter() -> Result<Self, TuiError> {
        #[cfg(unix)]
        let signals = terminal_signals::SignalGuard::install()?;
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableBracketedPaste, Hide) {
            let _ = execute!(stdout, Show, DisableBracketedPaste, LeaveAlternateScreen);
            let _ = disable_raw_mode();
            return Err(TuiError::Io(error));
        }
        Ok(Self {
            active: true,
            #[cfg(unix)]
            signals: Some(signals),
        })
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
            let _ = execute!(stdout, Show, DisableBracketedPaste, LeaveAlternateScreen);
            let _ = disable_raw_mode();
            self.active = false;
        }
        #[cfg(unix)]
        drop(self.signals.take());
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
    state.refresh_session_snapshot(agent.session());
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
    for event in events {
        changed = true;
        match event {
            crate::backend::ProviderEvent::TextDelta { delta } => {
                state.append_stream_for_job(job_id, MessageRole::Assistant, delta);
            }
            crate::backend::ProviderEvent::Refusal { text } => {
                state.append_stream_for_job(job_id, MessageRole::Error, text);
            }
            crate::backend::ProviderEvent::Warning { message } => {
                state.push_message(MessageRole::System, message);
            }
            crate::backend::ProviderEvent::ToolCallDelta { call_id, name, .. } => {
                if let Some(call_id) = call_id {
                    state.tool_call_started(call_id, name.unwrap_or_else(|| "unknown".into()));
                } else {
                    state.set_status("Preparing tool call");
                }
            }
            crate::backend::ProviderEvent::ToolCallDone { call } => {
                state.tool_call_started(call.id, call.name);
            }
            _ => {}
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
    for (index, character) in line.char_indices() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if column.saturating_add(character_width) > target_column {
            return index;
        }
        column = column.saturating_add(character_width);
        if column >= target_column {
            return index + character.len_utf8();
        }
    }
    line.len()
}

fn previous_boundary(text: &str, cursor: usize) -> usize {
    let cursor = clamp_char_boundary(text, cursor);
    text[..cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_boundary(text: &str, cursor: usize) -> usize {
    let cursor = clamp_char_boundary(text, cursor);
    text[cursor..]
        .chars()
        .next()
        .map_or(cursor, |character| cursor + character.len_utf8())
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
        for character in logical.chars() {
            let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
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
            line.push(character);
            used = used.saturating_add(character_width);
        }
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn transcript_lines(messages: &VecDeque<TuiMessage>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut result = Vec::new();
    for message in messages {
        let prefix = format!("{}: ", message.role.label());
        // Provider prose gets the small Markdown renderer; user prompts,
        // tool output, and errors stay literal so metadata or diff markers do
        // not acquire surprising presentation semantics.
        let rendered = match message.role {
            MessageRole::Assistant | MessageRole::System => {
                crate::render::render_markdown_prefixed(
                    &prefix,
                    &message.text,
                    width,
                    message.role.style(),
                )
            }
            MessageRole::User | MessageRole::Tool | MessageRole::Error => {
                crate::render::render_plain_prefixed(
                    &prefix,
                    &message.text,
                    width,
                    message.role.style(),
                )
            }
        };
        for line in rendered {
            result.push(line);
            if result.len() >= MAX_RENDER_LINES.saturating_mul(2) {
                return result;
            }
        }
    }
    result
}

fn cursor_position(text: &str, cursor: usize, width: usize) -> (u16, u16) {
    let width = width.max(1);
    let cursor = clamp_char_boundary(text, cursor);
    let mut x = 0usize;
    let mut y = 0usize;
    for character in text[..cursor.min(text.len())].chars() {
        if character == '\n' {
            x = 0;
            y = y.saturating_add(1);
            continue;
        }
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
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
            queue.pop(),
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
    }
}
