//! Multi-agent projection for the Gantt pane.
//!
//! One row per agent: the live master session first, then the DAG worker
//! tree from `.zenpi/dag.json`, then headless workers of this project that
//! are not DAG nodes. Rows join three sources: the DAG node (structure,
//! status, summary, heartbeat, mailbox), the headless process table
//! (liveness) and the worker's session journal (timeline, trajectory).

use crate::dag::{DagNode, DagSnapshot};
use crate::trajectory::{format_duration_ms, format_tokens};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use unicode_width::UnicodeWidthStr;

/// Rows drawn in the pane; the rest are counted, not listed.
pub(super) const MAX_AGENT_ROWS: usize = 64;
/// DAG stores above this size are not parsed on the terminal thread.
pub(super) const MAX_DAG_STORE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgentState {
    /// A live process (or the master) is working.
    Running,
    /// The master's turn is blocked on an approval decision.
    Waiting,
    /// A live process with nothing to do.
    Idle,
    /// A DAG node without a worker yet.
    Open,
    /// A DAG node whose worker process is gone while the node is open.
    Stale,
    /// DAG status `green`.
    Done,
    /// DAG status `red`.
    Failed,
}

impl AgentState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Waiting => "approval",
            Self::Idle => "idle",
            Self::Open => "open",
            Self::Stale => "stale",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Running => "●",
            Self::Waiting => "⏸",
            Self::Idle => "◌",
            Self::Open => "○",
            Self::Stale => "⚠",
            Self::Done => "✓",
            Self::Failed => "✗",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Running => Color::Green,
            Self::Waiting | Self::Stale => Color::Yellow,
            Self::Idle => Color::Gray,
            Self::Open => Color::DarkGray,
            Self::Done => Color::Cyan,
            Self::Failed => Color::Red,
        }
    }

    /// States whose bar extends to "now".
    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Waiting)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentRow {
    pub id: String,
    /// Tree glyphs drawn before the icon, e.g. `│ └ `.
    pub tree: String,
    pub state: AgentState,
    pub summary: Option<String>,
    pub session: Option<PathBuf>,
    pub started_ms: Option<u64>,
    pub updated_ms: Option<u64>,
    pub heartbeat_ms: u64,
    pub unread: usize,
    pub tokens: Option<u64>,
}

/// What the TUI knows about its own session.
#[derive(Debug, Clone)]
pub(super) struct MasterInfo {
    pub state: AgentState,
    pub summary: Option<String>,
    pub session: Option<PathBuf>,
    pub started_ms: Option<u64>,
    pub updated_ms: Option<u64>,
    pub tokens: Option<u64>,
}

/// One headless process that belongs to the active project.
#[derive(Debug, Clone)]
pub(super) struct WorkerProcess {
    pub pid: u32,
    pub busy: bool,
    pub session: Option<String>,
}

/// Cached DAG read plus the projected rows.
#[derive(Debug, Clone, Default)]
pub(super) struct AgentsView {
    pub rows: Vec<AgentRow>,
    pub hidden: usize,
    pub error: Option<String>,
    cache: Option<(PathBuf, (u64, Option<SystemTime>), DagSnapshot)>,
}

impl AgentsView {
    /// Re-read the DAG only when its size or mtime changed, then re-project.
    pub fn refresh(
        &mut self,
        master: MasterInfo,
        dag_path: Option<&Path>,
        workers: &[WorkerProcess],
        now_ms: u64,
    ) {
        self.error = None;
        let snapshot = match dag_path {
            Some(path) => self.load(path),
            None => None,
        };
        let empty = DagSnapshot::default();
        let dag = snapshot.unwrap_or(&empty);
        let dag_dir = dag_path.and_then(Path::parent);
        let mut rows = project(master, dag, dag_dir, workers, now_ms);
        self.hidden = rows.len().saturating_sub(MAX_AGENT_ROWS);
        rows.truncate(MAX_AGENT_ROWS);
        self.rows = rows;
    }

    fn load(&mut self, path: &Path) -> Option<&DagSnapshot> {
        let stamp = match std::fs::metadata(path) {
            Ok(metadata) => (metadata.len(), metadata.modified().ok()),
            Err(_) => {
                self.cache = None;
                return None;
            }
        };
        let fresh = self
            .cache
            .as_ref()
            .is_some_and(|(cached, seen, _)| cached == path && *seen == stamp);
        if !fresh {
            match crate::dag::DagStore::at(path).snapshot(MAX_DAG_STORE_BYTES) {
                Ok(snapshot) => self.cache = Some((path.to_owned(), stamp, snapshot)),
                Err(error) => {
                    self.error = Some(error);
                    self.cache = None;
                }
            }
        }
        self.cache.as_ref().map(|(_, _, snapshot)| snapshot)
    }

    pub fn counts(&self) -> BTreeMap<AgentState, usize> {
        let mut counts = BTreeMap::new();
        for row in &self.rows {
            *counts.entry(row.state).or_insert(0) += 1;
        }
        counts
    }
}

/// Join the master, the DAG tree and the process table into ordered rows.
pub(super) fn project(
    master: MasterInfo,
    dag: &DagSnapshot,
    dag_dir: Option<&Path>,
    workers: &[WorkerProcess],
    now_ms: u64,
) -> Vec<AgentRow> {
    let mut rows = vec![AgentRow {
        id: "master".into(),
        tree: String::new(),
        state: master.state,
        summary: master.summary,
        session: master.session.clone(),
        started_ms: master.started_ms,
        updated_ms: master
            .updated_ms
            .map(|at| if master.state.is_active() { now_ms } else { at }),
        heartbeat_ms: 0,
        unread: 0,
        tokens: master.tokens,
    }];
    let by_id: BTreeMap<&str, &DagNode> = dag
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut claimed_pids = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let roots: Vec<&DagNode> = dag
        .nodes
        .iter()
        .filter(|node| {
            node.parent
                .as_deref()
                .is_none_or(|parent| !by_id.contains_key(parent))
        })
        .collect();
    // Walk depth-first so each subtree stays contiguous under its parent.
    let mut stack: Vec<(&DagNode, Vec<bool>)> = roots
        .iter()
        .rev()
        .enumerate()
        .map(|(index, node)| (*node, vec![index == 0]))
        .collect();
    while let Some((node, lasts)) = stack.pop() {
        if !visited.insert(node.id.as_str()) {
            continue;
        }
        rows.push(node_row(
            node,
            &lasts,
            dag,
            dag_dir,
            workers,
            &mut claimed_pids,
        ));
        let children: Vec<&DagNode> = node
            .children
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .filter(|child| !visited.contains(child.id.as_str()))
            .collect();
        for (index, child) in children.iter().enumerate().rev() {
            let mut child_lasts = lasts.clone();
            child_lasts.push(index + 1 == children.len());
            stack.push((child, child_lasts));
        }
    }
    let master_session = master.session.as_deref();
    for worker in workers {
        let session = worker.session.as_deref().map(Path::new);
        if claimed_pids.contains(&worker.pid) || (session.is_some() && session == master_session) {
            continue;
        }
        let id = session
            .and_then(|path| path.file_stem())
            .and_then(|stem| stem.to_str())
            .map_or_else(|| format!("pid {}", worker.pid), str::to_owned);
        let (started_ms, updated_ms) = session.map_or((None, None), journal_times);
        rows.push(AgentRow {
            id,
            tree: String::new(),
            state: if worker.busy {
                AgentState::Running
            } else {
                AgentState::Idle
            },
            summary: Some(format!("headless pid {}", worker.pid)),
            session: session.map(Path::to_path_buf),
            started_ms,
            updated_ms: if worker.busy {
                Some(now_ms)
            } else {
                updated_ms
            },
            heartbeat_ms: 0,
            unread: 0,
            tokens: None,
        });
    }
    rows
}

fn node_row(
    node: &DagNode,
    lasts: &[bool],
    dag: &DagSnapshot,
    dag_dir: Option<&Path>,
    workers: &[WorkerProcess],
    claimed_pids: &mut BTreeSet<u32>,
) -> AgentRow {
    let journal = dag_dir
        .map(|dir| dir.join("workers").join(format!("{}.jsonl", node.id)))
        .filter(|path| path.is_file());
    let pid = node
        .worker
        .as_deref()
        .and_then(|worker| worker.strip_prefix("pid:"))
        .and_then(|pid| pid.parse::<u32>().ok());
    let process = workers.iter().find(|worker| {
        Some(worker.pid) == pid
            || worker.session.as_deref().is_some_and(|session| {
                Path::new(session).file_stem().and_then(|s| s.to_str()) == Some(node.id.as_str())
            })
    });
    if let Some(process) = process {
        claimed_pids.insert(process.pid);
    }
    let session = journal.or_else(|| {
        process
            .and_then(|process| process.session.as_deref())
            .map(PathBuf::from)
    });
    let state = match (node.status.as_str(), process) {
        ("green", _) => AgentState::Done,
        ("red", _) => AgentState::Failed,
        (_, Some(process)) if process.busy => AgentState::Running,
        (_, Some(_)) => AgentState::Idle,
        _ if node.worker.is_some() => AgentState::Stale,
        _ => AgentState::Open,
    };
    let mailbox = dag.mailbox.get(&node.id).copied().unwrap_or_default();
    let (journal_start, journal_end) = session.as_deref().map_or((None, None), journal_times);
    let updated = [
        node.updated_ms,
        node.worker_heartbeat_ms,
        mailbox.last_ms,
        journal_end.unwrap_or(0),
    ]
    .into_iter()
    .max()
    .filter(|at| *at > 0);
    let mut tree = String::new();
    for (level, last) in lasts.iter().enumerate().skip(1) {
        let is_self = level + 1 == lasts.len();
        tree.push_str(match (is_self, *last) {
            (true, true) => "└ ",
            (true, false) => "├ ",
            (false, true) => "  ",
            (false, false) => "│ ",
        });
    }
    AgentRow {
        id: node.id.clone(),
        tree,
        state,
        summary: node.summary.clone(),
        session,
        started_ms: journal_start.or(updated),
        updated_ms: updated,
        heartbeat_ms: node.worker_heartbeat_ms,
        unread: mailbox.unread,
        tokens: None,
    }
}

/// Journal creation and last-write times, in Unix milliseconds.
fn journal_times(path: &Path) -> (Option<u64>, Option<u64>) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return (None, None);
    };
    let millis = |time: std::io::Result<SystemTime>| {
        time.ok()
            .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
    };
    (millis(metadata.created()), millis(metadata.modified()))
}

/// The Agents section of the Gantt pane, `width` columns wide.
pub(super) fn section_lines(
    view: &AgentsView,
    selected: Option<usize>,
    width: usize,
    now_ms: u64,
) -> Vec<Line<'static>> {
    let mut header = vec![Span::styled(
        "Agents",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )];
    for (state, count) in view.counts() {
        header.push(Span::raw(" · "));
        header.push(Span::styled(
            format!("{count} {}", state.label()),
            Style::default().fg(state.color()),
        ));
    }
    let mut lines = vec![Line::from(header)];
    let origin = view
        .rows
        .iter()
        .filter_map(|row| row.started_ms.or(row.updated_ms))
        .min()
        .unwrap_or(now_ms);
    // The window runs to "now" only while someone is working; an idle view
    // ends at the last activity so old runs keep their resolution.
    let horizon = if view.rows.iter().any(|row| row.state.is_active()) {
        now_ms
    } else {
        view.rows
            .iter()
            .filter_map(|row| row.updated_ms)
            .max()
            .unwrap_or(now_ms)
            .min(now_ms)
    };
    let name_width = view
        .rows
        .iter()
        .map(|row| row.tree.width() + row.id.width())
        .max()
        .unwrap_or(6)
        .clamp(6, 24);
    let bar_width = (width / 5).clamp(6, 24);
    for (index, row) in view.rows.iter().enumerate() {
        let chosen = selected == Some(index);
        let id = super::truncate_to_width(&row.id, name_width.saturating_sub(row.tree.width()));
        let name_pad = name_width.saturating_sub(row.tree.width() + id.width());
        let age = match (row.state.is_active(), row.started_ms, row.updated_ms) {
            (true, Some(start), _) => format_duration_ms(now_ms.saturating_sub(start)),
            (false, _, Some(at)) => {
                format!("{} ago", format_duration_ms(now_ms.saturating_sub(at)))
            }
            _ => "-".into(),
        };
        let mut detail = String::new();
        if row.state == AgentState::Stale {
            detail.push_str("worker exited · ");
        }
        if row.unread > 0 {
            detail.push_str(&format!("✉{} ", row.unread));
        }
        if let Some(tokens) = row.tokens.filter(|tokens| *tokens > 0) {
            detail.push_str(&format!("{} tok ", format_tokens(tokens)));
        }
        if let Some(summary) = &row.summary {
            detail.push_str(summary);
        }
        let style = Style::default().fg(row.state.color());
        let fixed = 2 + name_width + 2 + 2 + 9 + bar_width + 2 + 9 + 1;
        let mut spans = vec![
            Span::styled(
                if chosen { "› " } else { "  " },
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(row.tree.clone(), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{} ", row.state.icon()), style),
            Span::styled(
                format!("{id}{} ", " ".repeat(name_pad)),
                Style::default().fg(Color::White),
            ),
            Span::styled(format!("{:<9}", row.state.label()), style),
            Span::styled(format!("▕{}▏", bar(row, origin, horizon, bar_width)), style),
            Span::styled(format!("{age:>9} "), Style::default().fg(Color::DarkGray)),
            Span::styled(
                super::truncate_to_width(&detail, width.saturating_sub(fixed)),
                Style::default().fg(Color::Gray),
            ),
        ];
        if chosen {
            for span in &mut spans {
                span.style = span.style.add_modifier(Modifier::REVERSED);
            }
        }
        lines.push(Line::from(spans));
    }
    if view.hidden > 0 {
        lines.push(Line::styled(
            format!("  … {} more agents", view.hidden),
            Style::default().fg(Color::DarkGray),
        ));
    }
    if view.rows.len() == 1 {
        lines.push(Line::styled(
            "  no DAG workers in this project yet",
            Style::default().fg(Color::DarkGray),
        ));
    }
    if let Some(error) = &view.error {
        lines.push(Line::styled(
            format!("  DAG unavailable: {error}"),
            Style::default().fg(Color::Red),
        ));
    }
    lines
}

/// One row's activity between `origin` and `horizon`; active rows reach
/// the horizon, which is "now" whenever any row is active.
fn bar(row: &AgentRow, origin: u64, horizon: u64, width: usize) -> String {
    let span = horizon.saturating_sub(origin).max(1) as f64;
    let cell = |at: u64| (at.saturating_sub(origin) as f64 / span * width as f64) as usize;
    let Some(start) = row.started_ms.or(row.updated_ms) else {
        return " ".repeat(width);
    };
    let end = if row.state.is_active() {
        horizon
    } else {
        row.updated_ms.unwrap_or(start)
    };
    let from = cell(start).min(width.saturating_sub(1));
    let to = cell(end).clamp(from + 1, width);
    let fill = if row.state.is_active() { '█' } else { '▓' };
    (0..width)
        .map(|index| {
            if (from..to).contains(&index) {
                fill
            } else {
                '·'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::MailboxCounts;

    fn node(
        id: &str,
        parent: Option<&str>,
        children: &[&str],
        status: &str,
        worker: Option<&str>,
    ) -> DagNode {
        DagNode {
            id: id.into(),
            parent: parent.map(str::to_owned),
            children: children.iter().map(|c| (*c).to_owned()).collect(),
            status: status.into(),
            worker: worker.map(str::to_owned),
            summary: None,
            updated_ms: 1_000,
            worker_heartbeat_ms: 0,
        }
    }

    fn master() -> MasterInfo {
        MasterInfo {
            state: AgentState::Idle,
            summary: None,
            session: None,
            started_ms: Some(500),
            updated_ms: Some(900),
            tokens: None,
        }
    }

    #[test]
    fn rows_follow_the_tree_and_join_liveness() {
        let dag = DagSnapshot {
            nodes: vec![
                node("plan", None, &["api", "ui"], "open", Some("pid:10")),
                node("api", Some("plan"), &[], "green", Some("pid:11")),
                node("ui", Some("plan"), &["ui-test"], "open", Some("pid:12")),
                node("ui-test", Some("ui"), &[], "open", None),
            ],
            mailbox: BTreeMap::from([(
                "ui".to_owned(),
                MailboxCounts {
                    unread: 2,
                    claimed: 0,
                    last_ms: 2_000,
                },
            )]),
        };
        let workers = [
            WorkerProcess {
                pid: 10,
                busy: true,
                session: None,
            },
            WorkerProcess {
                pid: 99,
                busy: false,
                session: Some("/tmp/other.jsonl".into()),
            },
        ];
        let rows = project(master(), &dag, None, &workers, 5_000);
        let view: Vec<_> = rows
            .iter()
            .map(|row| (row.tree.as_str(), row.id.as_str(), row.state))
            .collect();
        assert_eq!(
            view,
            [
                ("", "master", AgentState::Idle),
                ("", "plan", AgentState::Running),
                ("├ ", "api", AgentState::Done),
                ("└ ", "ui", AgentState::Stale),
                ("  └ ", "ui-test", AgentState::Open),
                ("", "other", AgentState::Idle),
            ]
        );
        assert_eq!(rows[3].unread, 2);
        assert_eq!(rows[3].updated_ms, Some(2_000));
    }

    #[test]
    fn idle_bars_keep_resolution_long_after_the_run() {
        let row = |id: &str, start: u64, end: u64| AgentRow {
            id: id.into(),
            tree: String::new(),
            state: AgentState::Done,
            summary: None,
            session: None,
            started_ms: Some(start),
            updated_ms: Some(end),
            heartbeat_ms: 0,
            unread: 0,
            tokens: None,
        };
        let view = AgentsView {
            rows: vec![row("a", 0, 50_000), row("b", 50_000, 100_000)],
            ..AgentsView::default()
        };
        // An hour later the two halves still split the bar.
        let lines = section_lines(&view, None, 120, 3_700_000);
        let text: Vec<String> = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect();
        assert!(
            text[1].contains(&format!("▕{}{}▏", "▓".repeat(12), "·".repeat(12))),
            "{}",
            text[1]
        );
        assert!(
            text[2].contains(&format!("▕{}{}▏", "·".repeat(12), "▓".repeat(12))),
            "{}",
            text[2]
        );
    }
}
