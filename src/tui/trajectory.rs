//! Full-screen trajectory viewer (Alt-T, `/trajectory`).
//!
//! Three levels, navigated with Enter/Esc or `1`/`2`/`3`:
//! the agent overview (one row per agent with a shared timeline bar), the
//! turn-grouped record list of one agent, and an inspector for one record
//! with Content / Metrics / Raw sections. The data comes from
//! [`crate::trajectory::Trajectory`]; this module only navigates and draws.

use super::agents::AgentState;
use super::{TuiAction, truncate_to_width, wrap_plain};
use crate::trajectory::{
    CacheSeverity, RecordKind, RecordStatus, Trajectory, TrajectoryRecord, format_duration_ms,
    format_offset_ms, format_tokens,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::collections::BTreeSet;
use std::path::PathBuf;
use unicode_width::UnicodeWidthStr;

/// Worker journals loaded when the viewer opens; the rest load on Enter.
const EAGER_WORKER_LOADS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrajectoryLevel {
    Agents,
    Records,
    Inspector,
}

/// One agent the viewer can show: the in-process master or a worker journal.
/// The first source is always the master.
#[derive(Debug, Clone)]
pub(super) struct AgentSource {
    pub name: String,
    /// DAG tree glyphs shown before the name, as in the Gantt pane.
    pub tree: String,
    pub state: AgentState,
    pub session: Option<PathBuf>,
    pub trajectory: Option<Trajectory>,
}

pub(super) enum ViewerKey {
    Stay,
    Close,
    Action(TuiAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectorTab {
    Content,
    Metrics,
    Raw,
}

impl InspectorTab {
    const ALL: [Self; 3] = [Self::Content, Self::Metrics, Self::Raw];

    fn label(self) -> &'static str {
        match self {
            Self::Content => "Content",
            Self::Metrics => "Metrics",
            Self::Raw => "Raw",
        }
    }

    fn step(self, forward: bool) -> Self {
        let index = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        let next = if forward { index + 1 } else { index + 2 };
        Self::ALL[next % Self::ALL.len()]
    }
}

#[derive(Debug, Clone)]
struct AgentEntry {
    source: AgentSource,
    load_error: Option<String>,
}

impl AgentEntry {
    fn ensure_loaded(&mut self) {
        if self.source.trajectory.is_some() || self.load_error.is_some() {
            return;
        }
        // DAG coordination nodes have no worker journal; that is not an error.
        let Some(path) = self.source.session.as_ref() else {
            return;
        };
        match crate::session::inspect_session(path) {
            Ok(inspection) => {
                let running = self.source.state.is_active();
                let root = running
                    .then(|| crate::trajectory::open_root(&inspection.turns))
                    .flatten();
                self.source.trajectory = Some(Trajectory::from_turns(&inspection.turns, root));
            }
            Err(error) => self.load_error = Some(error.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Turn(usize),
    Record(usize),
}

#[derive(Debug, Clone, Default)]
struct Search {
    query: String,
    editing: bool,
    matches: Vec<usize>,
    current: usize,
}

#[derive(Debug, Clone)]
pub(super) struct TrajectoryViewer {
    agents: Vec<AgentEntry>,
    agent: usize,
    level: TrajectoryLevel,
    /// Cursor into the record level's visible rows.
    cursor: usize,
    scroll: usize,
    collapsed: BTreeSet<usize>,
    /// Record position shown by the inspector.
    record: usize,
    tab: InspectorTab,
    inspector_scroll: usize,
    search: Option<Search>,
    help: bool,
    notice: Option<String>,
    /// Body height of the last draw, used for page steps.
    page: usize,
    master_revision: u64,
}

impl TrajectoryViewer {
    pub(super) fn new(sources: Vec<AgentSource>, master_revision: u64) -> Self {
        let mut agents: Vec<_> = sources
            .into_iter()
            .map(|source| AgentEntry {
                source,
                load_error: None,
            })
            .collect();
        for agent in agents.iter_mut().take(EAGER_WORKER_LOADS + 1) {
            agent.ensure_loaded();
        }
        Self {
            agents,
            agent: 0,
            level: TrajectoryLevel::Agents,
            cursor: 0,
            scroll: 0,
            collapsed: BTreeSet::new(),
            record: 0,
            tab: InspectorTab::Content,
            inspector_scroll: 0,
            search: None,
            help: false,
            notice: None,
            page: 10,
            master_revision,
        }
    }

    /// Select the agent at `index` (clamped), loading its journal if needed.
    pub(super) fn select_agent(mut self, index: usize) -> Self {
        self.agent = index.min(self.agents.len().saturating_sub(1));
        if let Some(agent) = self.agents.get_mut(self.agent) {
            agent.ensure_loaded();
        }
        self
    }

    /// Open straight into the selected agent's records, following the tail.
    pub(super) fn open_records(mut self) -> Self {
        if !self.agents.is_empty() {
            self.enter_records();
        }
        self
    }

    pub(super) fn level(&self) -> TrajectoryLevel {
        self.level
    }

    pub(super) fn master_revision(&self) -> u64 {
        self.master_revision
    }

    /// Replace the master's trajectory after the live session advanced. The
    /// record cursor keeps following the tail when it was already there.
    pub(super) fn sync_master(&mut self, revision: u64, trajectory: Trajectory) {
        self.master_revision = revision;
        let Some(master) = self.agents.first_mut() else {
            return;
        };
        master.source.state = match (trajectory.in_progress(), master.source.state) {
            (true, AgentState::Waiting) => AgentState::Waiting,
            (true, _) => AgentState::Running,
            (false, _) => AgentState::Idle,
        };
        let following = self.agent == 0 && self.cursor + 1 >= self.rows().len();
        self.agents[0].source.trajectory = Some(trajectory);
        if self.agent != 0 {
            return;
        }
        let rows = self.rows().len();
        if following && self.level == TrajectoryLevel::Records {
            self.cursor = rows.saturating_sub(1);
        }
        self.cursor = self.cursor.min(rows.saturating_sub(1));
        let records = self.trajectory().map_or(0, Trajectory::len);
        self.record = self.record.min(records.saturating_sub(1));
        self.refresh_search();
    }

    fn trajectory(&self) -> Option<&Trajectory> {
        self.agents.get(self.agent)?.source.trajectory.as_ref()
    }

    fn rows(&self) -> Vec<Row> {
        let Some(trajectory) = self.trajectory() else {
            return Vec::new();
        };
        let mut rows = Vec::with_capacity(trajectory.len() + trajectory.turns.len());
        for (index, group) in trajectory.turns.iter().enumerate() {
            rows.push(Row::Turn(index));
            if !self.collapsed.contains(&group.number) {
                rows.extend(group.records.iter().map(|position| Row::Record(*position)));
            }
        }
        rows
    }

    fn enter_records(&mut self) {
        if let Some(agent) = self.agents.get_mut(self.agent) {
            agent.ensure_loaded();
        }
        self.level = TrajectoryLevel::Records;
        self.collapsed.clear();
        self.search = None;
        self.cursor = self.rows().len().saturating_sub(1);
    }

    fn enter_inspector(&mut self) {
        let rows = self.rows();
        match rows.get(self.cursor) {
            Some(Row::Record(position)) => {
                self.record = *position;
                self.level = TrajectoryLevel::Inspector;
                self.inspector_scroll = 0;
            }
            Some(Row::Turn(index)) => self.toggle_fold(*index),
            None => {}
        }
    }

    fn toggle_fold(&mut self, group_index: usize) {
        let Some(number) = self
            .trajectory()
            .and_then(|trajectory| trajectory.turns.get(group_index))
            .map(|group| group.number)
        else {
            return;
        };
        if !self.collapsed.remove(&number) {
            self.collapsed.insert(number);
        }
        let rows = self.rows();
        self.cursor = rows
            .iter()
            .position(|row| *row == Row::Turn(group_index))
            .unwrap_or(0);
    }

    fn cursor_group(&self) -> Option<usize> {
        let trajectory = self.trajectory()?;
        match self.rows().get(self.cursor)? {
            Row::Turn(index) => Some(*index),
            Row::Record(position) => {
                let number = trajectory.records.get(*position)?.turn;
                trajectory
                    .turns
                    .iter()
                    .position(|group| group.number == number)
            }
        }
    }

    /// Move the record cursor onto `position`, unfolding its turn if needed.
    fn reveal(&mut self, position: usize) {
        let Some(number) = self
            .trajectory()
            .and_then(|trajectory| trajectory.records.get(position))
            .map(|record| record.turn)
        else {
            return;
        };
        self.collapsed.remove(&number);
        if let Some(row) = self
            .rows()
            .iter()
            .position(|row| *row == Row::Record(position))
        {
            self.cursor = row;
        }
        if self.level == TrajectoryLevel::Inspector {
            self.record = position;
            self.inspector_scroll = 0;
        }
    }

    fn refresh_search(&mut self) {
        let Some(query) = self.search.as_ref().map(|search| search.query.clone()) else {
            return;
        };
        let matches = self
            .trajectory()
            .map(|trajectory| trajectory.search(&query))
            .unwrap_or_default();
        if let Some(search) = self.search.as_mut() {
            search.current = search.current.min(matches.len().saturating_sub(1));
            search.matches = matches;
        }
    }

    fn jump_match(&mut self, forward: bool) {
        let current = self.current_record();
        let Some(search) = self.search.as_mut() else {
            self.notice = Some("no search: press / to search".into());
            return;
        };
        if search.matches.is_empty() {
            self.notice = Some(format!("no match for \"{}\"", search.query));
            return;
        }
        let count = search.matches.len();
        search.current = match current.and_then(|c| search.matches.iter().position(|m| *m == c)) {
            Some(at) if forward => (at + 1) % count,
            Some(at) => (at + count - 1) % count,
            None if forward => search
                .matches
                .iter()
                .position(|m| current.is_none_or(|c| *m > c))
                .unwrap_or(0),
            None => search
                .matches
                .iter()
                .rposition(|m| current.is_none_or(|c| *m < c))
                .unwrap_or(count - 1),
        };
        let target = search.matches[search.current];
        self.reveal(target);
    }

    fn jump_cache_issue(&mut self) {
        let issues = self
            .trajectory()
            .map(Trajectory::cache_issues)
            .unwrap_or_default();
        let current = self.current_record();
        let Some(target) = issues
            .iter()
            .copied()
            .find(|position| current.is_none_or(|c| *position > c))
            .or_else(|| issues.first().copied())
        else {
            self.notice = Some("no KV-cache issues in this trajectory".into());
            return;
        };
        self.reveal(target);
    }

    fn current_record(&self) -> Option<usize> {
        match self.level {
            TrajectoryLevel::Inspector => Some(self.record),
            _ => match self.rows().get(self.cursor) {
                Some(Row::Record(position)) => Some(*position),
                _ => None,
            },
        }
    }

    pub(super) fn key(&mut self, key: KeyEvent) -> ViewerKey {
        self.notice = None;
        if self.search.as_ref().is_some_and(|search| search.editing) {
            return self.search_key(key);
        }
        if self.help {
            self.help = false;
            return ViewerKey::Stay;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if control => return ViewerKey::Close,
            KeyCode::Char('q') => return ViewerKey::Close,
            KeyCode::Esc => {
                match self.level {
                    TrajectoryLevel::Agents => return ViewerKey::Close,
                    TrajectoryLevel::Records if self.search.is_some() => self.search = None,
                    TrajectoryLevel::Records => self.level = TrajectoryLevel::Agents,
                    TrajectoryLevel::Inspector => {
                        self.level = TrajectoryLevel::Records;
                        self.reveal(self.record);
                    }
                }
                return ViewerKey::Stay;
            }
            KeyCode::Char('?') => {
                self.help = true;
                return ViewerKey::Stay;
            }
            KeyCode::Char('1') | KeyCode::Char('a') => {
                self.level = TrajectoryLevel::Agents;
                return ViewerKey::Stay;
            }
            KeyCode::Char('2') if self.level != TrajectoryLevel::Records => {
                if self.level == TrajectoryLevel::Inspector {
                    self.level = TrajectoryLevel::Records;
                    self.reveal(self.record);
                } else {
                    self.enter_records();
                }
                return ViewerKey::Stay;
            }
            KeyCode::Char('3') if self.level == TrajectoryLevel::Records => {
                self.enter_inspector();
                return ViewerKey::Stay;
            }
            KeyCode::Char('R') => {
                if let Some(agent) = self.agents.get_mut(self.agent)
                    && agent.source.session.is_some()
                    && self.agent != 0
                {
                    agent.source.trajectory = None;
                    agent.load_error = None;
                    agent.ensure_loaded();
                    self.notice = Some(format!("reloaded {}", agent.source.name));
                }
                return ViewerKey::Stay;
            }
            _ => {}
        }
        match self.level {
            TrajectoryLevel::Agents => self.agents_key(key),
            TrajectoryLevel::Records => self.records_key(key),
            TrajectoryLevel::Inspector => return self.inspector_key(key),
        }
        ViewerKey::Stay
    }

    fn search_key(&mut self, key: KeyEvent) -> ViewerKey {
        let Some(search) = self.search.as_mut() else {
            return ViewerKey::Stay;
        };
        match key.code {
            KeyCode::Esc => self.search = None,
            KeyCode::Enter => {
                search.editing = false;
                if search.query.trim().is_empty() {
                    self.search = None;
                } else {
                    self.refresh_search();
                    self.jump_match(true);
                }
            }
            KeyCode::Backspace => {
                search.query.pop();
            }
            KeyCode::Char(character)
                if !key.modifiers.contains(KeyModifiers::CONTROL) && search.query.len() < 256 =>
            {
                search.query.push(character);
            }
            _ => {}
        }
        ViewerKey::Stay
    }

    fn agents_key(&mut self, key: KeyEvent) {
        let count = self.agents.len();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.agent = self.agent.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.agent = (self.agent + 1).min(count.saturating_sub(1));
            }
            KeyCode::Home | KeyCode::Char('g') => self.agent = 0,
            KeyCode::End | KeyCode::Char('G') => self.agent = count.saturating_sub(1),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') if count > 0 => {
                self.enter_records();
            }
            _ => {}
        }
    }

    fn records_key(&mut self, key: KeyEvent) {
        let last = self.rows().len().saturating_sub(1);
        let page = self.page.max(1);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.cursor = (self.cursor + 1).min(last),
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(page),
            KeyCode::PageDown => self.cursor = (self.cursor + page).min(last),
            KeyCode::Home | KeyCode::Char('g') => self.cursor = 0,
            KeyCode::End | KeyCode::Char('G') => self.cursor = last,
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => self.enter_inspector(),
            KeyCode::Left | KeyCode::Char('h') => self.level = TrajectoryLevel::Agents,
            KeyCode::Char(' ') => {
                if let Some(group) = self.cursor_group() {
                    self.toggle_fold(group);
                }
            }
            KeyCode::Char('/') => {
                self.search = Some(Search {
                    editing: true,
                    ..Search::default()
                });
            }
            KeyCode::Char('n') => self.jump_match(true),
            KeyCode::Char('N') => self.jump_match(false),
            KeyCode::Char('c') => self.jump_cache_issue(),
            _ => {}
        }
    }

    fn inspector_key(&mut self, key: KeyEvent) -> ViewerKey {
        let records = self.trajectory().map_or(0, Trajectory::len);
        let page = self.page.max(1);
        match key.code {
            KeyCode::Tab => self.tab = self.tab.step(true),
            KeyCode::BackTab => self.tab = self.tab.step(false),
            KeyCode::Up | KeyCode::Char('k') => {
                self.inspector_scroll = self.inspector_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => self.inspector_scroll += 1,
            KeyCode::PageUp => self.inspector_scroll = self.inspector_scroll.saturating_sub(page),
            KeyCode::PageDown => self.inspector_scroll += page,
            KeyCode::Home | KeyCode::Char('g') => self.inspector_scroll = 0,
            KeyCode::Left | KeyCode::Char('h') if self.record > 0 => {
                self.record -= 1;
                self.inspector_scroll = 0;
            }
            KeyCode::Right | KeyCode::Char('l') if self.record + 1 < records => {
                self.record += 1;
                self.inspector_scroll = 0;
            }
            KeyCode::Char('n') => self.jump_match(true),
            KeyCode::Char('N') => self.jump_match(false),
            KeyCode::Char('c') => self.jump_cache_issue(),
            KeyCode::Char('y') => {
                let text = self.inspector_text(usize::MAX).join("\n");
                self.notice = Some(format!("copied {} section", self.tab.label()));
                return ViewerKey::Action(TuiAction::Copy(text));
            }
            _ => {}
        }
        ViewerKey::Stay
    }

    pub(super) fn mouse(&mut self, mouse: MouseEvent) {
        let down = match mouse.kind {
            MouseEventKind::ScrollDown => true,
            MouseEventKind::ScrollUp => false,
            _ => return,
        };
        let code = if down { KeyCode::Down } else { KeyCode::Up };
        for _ in 0..3 {
            self.key(KeyEvent::new(code, KeyModifiers::NONE));
        }
    }

    fn breadcrumb(&self) -> String {
        let mut parts = vec!["Agents".to_owned()];
        if self.level != TrajectoryLevel::Agents
            && let Some(agent) = self.agents.get(self.agent)
        {
            parts.push(agent.source.name.clone());
            let record = self
                .current_record()
                .and_then(|position| self.trajectory()?.records.get(position));
            if let Some(record) = record {
                parts.push(format!("Turn {}", record.turn));
                parts.push(format!("#{}", record.index));
            } else if let Some(group) = self
                .cursor_group()
                .and_then(|index| self.trajectory()?.turns.get(index))
            {
                parts.push(format!("Turn {}", group.number));
            }
        }
        parts.join(" › ")
    }

    fn kv_badge(&self) -> (String, Color) {
        let trajectory = match self.level {
            TrajectoryLevel::Agents => None,
            _ => self.trajectory(),
        };
        let stats = match trajectory {
            Some(trajectory) => trajectory.stats.clone(),
            None => {
                let mut stats = crate::trajectory::TrajectoryStats::default();
                for trajectory in self
                    .agents
                    .iter()
                    .filter_map(|a| a.source.trajectory.as_ref())
                {
                    stats.requests += trajectory.stats.requests;
                    stats.warnings += trajectory.stats.warnings;
                    stats.misses += trajectory.stats.misses;
                }
                stats
            }
        };
        match (stats.requests, stats.kv_issues()) {
            (0, _) => ("KV n/a".into(), Color::DarkGray),
            (_, 0) => ("KV ✓".into(), Color::Green),
            (_, issues) if stats.misses > 0 => (format!("KV ✗ {issues}"), Color::Red),
            (_, issues) => (format!("KV ⚠ {issues}"), Color::Yellow),
        }
    }

    pub(super) fn render(&mut self, frame: &mut Frame<'_>) {
        // Drawn as a zoom over the workspace, inset like the Resources zoom.
        let all = frame.area();
        let area = if all.width > 40 && all.height > 12 {
            Rect::new(all.x + 2, all.y + 1, all.width - 4, all.height - 2)
        } else {
            all
        };
        frame.render_widget(Clear, area);
        let (badge, badge_color) = self.kv_badge();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Line::from(vec![
                Span::styled(
                    " Trajectory ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("[{}] ", self.breadcrumb())),
            ]))
            .title(
                Line::from(Span::styled(
                    format!(" {badge} "),
                    Style::default()
                        .fg(badge_color)
                        .add_modifier(Modifier::BOLD),
                ))
                .right_aligned(),
            );
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width < 20 || inner.height < 5 {
            return;
        }
        let footer_height = if self.level == TrajectoryLevel::Agents {
            1
        } else {
            2
        };
        let body = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(footer_height),
        );
        self.page = usize::from(body.height).saturating_sub(2).max(1);
        if self.help {
            frame.render_widget(Paragraph::new(help_lines()), body);
        } else {
            match self.level {
                TrajectoryLevel::Agents => self.render_agents(frame, body),
                TrajectoryLevel::Records => self.render_records(frame, body),
                TrajectoryLevel::Inspector => self.render_inspector(frame, body),
            }
        }
        let footer = Rect::new(
            inner.x,
            body.y + body.height,
            inner.width,
            footer_height.min(inner.height),
        );
        self.render_footer(frame, footer);
    }

    fn render_agents(&self, frame: &mut Frame<'_>, area: Rect) {
        let width = usize::from(area.width);
        let columns = format!(
            "  {:<16} {:<10} {:>5} {:>5} {:>8} {:>6} {:>5} {:>9}  ",
            "Agent", "Status", "Turns", "Reqs", "Tokens", "Cache", "KV", "Duration"
        );
        let bar_width = width.saturating_sub(columns.width()).max(4);
        let span = self.agents.iter().filter_map(|agent| {
            let trajectory = agent.source.trajectory.as_ref()?;
            Some((trajectory.started_ms()?, trajectory.ended_ms()?))
        });
        let (origin, end) = span.fold((u64::MAX, 0), |(lo, hi), (start, stop)| {
            (lo.min(start), hi.max(stop))
        });
        let mut lines = vec![
            Line::styled(
                format!("{columns}Timeline"),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Line::styled("─".repeat(width), Style::default().fg(Color::DarkGray)),
        ];
        for (index, agent) in self.agents.iter().enumerate() {
            let selected = index == self.agent;
            let marker = if selected { "> " } else { "  " };
            let name =
                truncate_to_width(&format!("{}{}", agent.source.tree, agent.source.name), 16);
            let name_pad = 16usize.saturating_sub(name.width());
            let state = agent.source.state;
            let status_style = Style::default().fg(state.color());
            let mut spans = vec![
                Span::raw(format!("{marker}{name}{} ", " ".repeat(name_pad))),
                Span::styled(
                    format!("{} {:<8} ", state.icon(), state.label()),
                    status_style,
                ),
            ];
            match (&agent.source.trajectory, &agent.load_error) {
                (Some(trajectory), _) => {
                    let stats = &trajectory.stats;
                    let cache = stats
                        .cache_hit_ratio()
                        .map_or("n/a".into(), |ratio| format!("{:.0}%", ratio * 100.0));
                    let (kv, kv_color) = match (stats.requests, stats.kv_issues()) {
                        (0, _) => ("-".to_owned(), Color::DarkGray),
                        (_, 0) => ("✓".to_owned(), Color::Green),
                        (_, n) if stats.misses > 0 => (format!("✗{n}"), Color::Red),
                        (_, n) => (format!("⚠{n}"), Color::Yellow),
                    };
                    let duration = match (trajectory.started_ms(), trajectory.ended_ms()) {
                        (Some(start), Some(stop)) => format_duration_ms(stop - start),
                        _ => "-".into(),
                    };
                    spans.push(Span::raw(format!(
                        "{:>5} {:>5} {:>8} {:>6} ",
                        trajectory.turns.len(),
                        stats.requests,
                        format_tokens(stats.total_tokens()),
                        cache
                    )));
                    spans.push(Span::styled(
                        format!("{kv:>5}"),
                        Style::default().fg(kv_color),
                    ));
                    spans.push(Span::raw(format!(" {duration:>9}  ")));
                    spans.push(Span::styled(
                        timeline_bar(trajectory, origin, end, bar_width),
                        status_style.add_modifier(Modifier::BOLD),
                    ));
                }
                (None, Some(error)) => spans.push(Span::styled(
                    truncate_to_width(&format!("unavailable: {error}"), width.saturating_sub(28)),
                    Style::default().fg(Color::Red),
                )),
                (None, None) if agent.source.session.is_none() => spans.push(Span::styled(
                    "no worker journal · coordination node",
                    Style::default().fg(Color::DarkGray),
                )),
                (None, None) => spans.push(Span::styled(
                    "not loaded · Enter to load",
                    Style::default().fg(Color::DarkGray),
                )),
            }
            let mut line = Line::from(spans);
            if selected {
                line = line.style(Style::default().add_modifier(Modifier::REVERSED));
            }
            lines.push(line);
        }
        if self.agents.is_empty() {
            lines.push(Line::styled(
                "  No agent sessions yet. Send a prompt, then reopen with Alt-T.",
                Style::default().fg(Color::DarkGray),
            ));
        } else if origin < end {
            lines.push(Line::raw(""));
            let axis = format!(
                "{:>pad$}00:00:00 {} {}",
                "",
                "─".repeat(bar_width.saturating_sub(18)),
                format_offset_ms(end, origin),
                pad = columns.width()
            );
            lines.push(Line::styled(axis, Style::default().fg(Color::DarkGray)));
        }
        // Keep the two header rows and scroll the agent rows under them.
        let room = usize::from(area.height).saturating_sub(2).max(1);
        let skip = (self.agent + 1).saturating_sub(room);
        let mut lines = lines.into_iter();
        let mut visible: Vec<_> = lines.by_ref().take(2).collect();
        visible.extend(lines.skip(skip));
        frame.render_widget(Paragraph::new(visible), area);
    }

    fn render_records(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let width = usize::from(area.width);
        let height = usize::from(area.height);
        let Some(trajectory) = self.trajectory() else {
            let message = self
                .agents
                .get(self.agent)
                .and_then(|agent| agent.load_error.clone())
                .unwrap_or_else(|| "no trajectory loaded".into());
            frame.render_widget(
                Paragraph::new(Line::styled(message, Style::default().fg(Color::Red))),
                area,
            );
            return;
        };
        if trajectory.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    "No records yet. Send a prompt to start a trajectory.",
                    Style::default().fg(Color::DarkGray),
                )),
                area,
            );
            return;
        }
        let rows = self.rows();
        self.cursor = self.cursor.min(rows.len().saturating_sub(1));
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + height {
            self.scroll = self.cursor + 1 - height;
        }
        self.scroll = self.scroll.min(rows.len().saturating_sub(height));
        let Some(trajectory) = self.trajectory() else {
            return;
        };
        let origin = trajectory.started_ms().unwrap_or(0);
        let matches: BTreeSet<usize> = self
            .search
            .as_ref()
            .map(|search| search.matches.iter().copied().collect())
            .unwrap_or_default();
        let lines: Vec<Line> = rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(height)
            .map(|(index, row)| {
                let selected = index == self.cursor;
                let line = match row {
                    Row::Turn(group) => {
                        let group = &trajectory.turns[*group];
                        let folded = self.collapsed.contains(&group.number);
                        let mut text = format!(
                            "{} Turn {} · {} records · {} · +{}",
                            if folded { "▸" } else { "▾" },
                            group.number,
                            group.records.len(),
                            format_duration_ms(group.duration_ms()),
                            format_offset_ms(group.started_ms, origin)
                        );
                        if group.in_progress {
                            text.push_str(" · in progress");
                        }
                        Line::styled(
                            truncate_to_width(&text, width),
                            Style::default().add_modifier(Modifier::BOLD),
                        )
                    }
                    Row::Record(position) => record_line(
                        &trajectory.records[*position],
                        origin,
                        width,
                        matches.contains(position),
                    ),
                };
                if selected {
                    line.style(Style::default().add_modifier(Modifier::REVERSED))
                } else {
                    line
                }
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_inspector(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let width = usize::from(area.width);
        let mut tabs = vec![Span::raw(" ")];
        for tab in InspectorTab::ALL {
            let style = if tab == self.tab {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            tabs.push(Span::styled(format!(" {} ", tab.label()), style));
            tabs.push(Span::raw(" "));
        }
        let lines = self.inspector_text(width.saturating_sub(1));
        let height = usize::from(area.height).saturating_sub(2);
        self.inspector_scroll = self
            .inspector_scroll
            .min(lines.len().saturating_sub(height));
        let mut body = vec![
            Line::from(tabs),
            Line::styled("─".repeat(width), Style::default().fg(Color::DarkGray)),
        ];
        body.extend(
            lines
                .into_iter()
                .skip(self.inspector_scroll)
                .take(height)
                .map(styled_inspector_line),
        );
        frame.render_widget(Paragraph::new(body), area);
    }

    /// The selected record's current section as wrapped plain lines.
    fn inspector_text(&self, width: usize) -> Vec<String> {
        let Some(trajectory) = self.trajectory() else {
            return Vec::new();
        };
        let Some(record) = trajectory.records.get(self.record) else {
            return vec!["no record selected".into()];
        };
        let origin = trajectory.started_ms().unwrap_or(0);
        let raw = match self.tab {
            InspectorTab::Content => content_section(record),
            InspectorTab::Metrics => metrics_section(record, origin),
            InspectorTab::Raw => record
                .metadata
                .as_ref()
                .and_then(|metadata| serde_json::to_string_pretty(metadata).ok())
                .unwrap_or_else(|| "(no metadata)".into()),
        };
        if width == usize::MAX {
            return raw.lines().map(str::to_owned).collect();
        }
        wrap_plain(&raw, width.max(8))
    }

    fn render_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        let width = usize::from(area.width);
        let hint_style = Style::default().fg(Color::DarkGray);
        let mut lines = Vec::new();
        if self.level != TrajectoryLevel::Agents
            && let Some(trajectory) = self.trajectory()
        {
            lines.push(Line::styled(
                truncate_to_width(&stats_line(trajectory, width), width),
                Style::default().fg(Color::Cyan),
            ));
        }
        let hint = if let Some(search) = self.search.as_ref().filter(|search| search.editing) {
            Line::from(vec![
                Span::styled(
                    format!("/{}▏", search.query),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled("  Enter search · Esc cancel", hint_style),
            ])
        } else if let Some(notice) = &self.notice {
            Line::styled(notice.clone(), Style::default().fg(Color::Yellow))
        } else {
            let keys = match self.level {
                TrajectoryLevel::Agents => {
                    "↑↓ select · Enter trajectory · R reload · ? help · Esc close"
                }
                TrajectoryLevel::Records => {
                    "↑↓ move · Enter inspect · Space fold · / search · n/N match · c next KV issue · a agents · Esc back"
                }
                TrajectoryLevel::Inspector => {
                    "Tab section · ←→ record · ↑↓ scroll · c next KV issue · y copy · Esc back"
                }
            };
            let mut spans = Vec::new();
            if let Some(search) = self.search.as_ref() {
                let position = if search.matches.is_empty() {
                    0
                } else {
                    search.current + 1
                };
                spans.push(Span::styled(
                    format!("/{} {}/{} · ", search.query, position, search.matches.len()),
                    Style::default().fg(Color::Yellow),
                ));
            }
            spans.push(Span::styled(keys, hint_style));
            Line::from(spans)
        };
        lines.push(hint);
        let skip = lines.len().saturating_sub(usize::from(area.height));
        frame.render_widget(
            Paragraph::new(lines.into_iter().skip(skip).collect::<Vec<_>>()),
            area,
        );
    }
}

fn kind_style(kind: RecordKind) -> Style {
    let color = match kind {
        RecordKind::User | RecordKind::Steer => Color::Cyan,
        RecordKind::Assistant => Color::Green,
        RecordKind::ToolCall => Color::Magenta,
        RecordKind::ToolResult => Color::LightMagenta,
        RecordKind::System => Color::Blue,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn cache_mark(record: &TrajectoryRecord) -> Option<(&'static str, Color)> {
    match record.cache.as_ref()?.severity {
        CacheSeverity::Ok => None,
        CacheSeverity::Info => Some(("ℹ", Color::Blue)),
        CacheSeverity::Warn => Some(("⚠", Color::Yellow)),
        CacheSeverity::Miss => Some(("✗", Color::Red)),
    }
}

fn record_line(
    record: &TrajectoryRecord,
    origin: u64,
    width: usize,
    matched: bool,
) -> Line<'static> {
    let prefix = format!(
        "  #{:<4} +{} {:<9} ",
        record.index,
        format_offset_ms(record.timestamp_ms, origin),
        record.kind.label()
    );
    let metrics = match record.usage {
        Some(usage) => {
            let (input, output) = (
                format_tokens(usage.input_tokens),
                format_tokens(usage.output_tokens),
            );
            // Narrow terminals keep room for the summary.
            let mut text = if width >= 90 {
                format!("{input} in · {output} out")
            } else {
                format!("{input}/{output}")
            };
            if let Some(ratio) = record.cache_ratio() {
                text.push_str(&format!(" ⚡{:.0}%", ratio * 100.0));
            }
            text
        }
        None if record.kind.is_request() => "no usage".into(),
        None => format!("~{}", format_tokens(record.estimated_tokens())),
    };
    let mark = cache_mark(record);
    let status = match record.status {
        RecordStatus::Error => Some(("✗ ", Color::Red)),
        RecordStatus::InProgress => Some(("… ", Color::Yellow)),
        RecordStatus::Success => None,
    };
    // Prefix, status glyph, two-column gap, metrics and the KV mark.
    let status_width = if status.is_some() { 2 } else { 0 };
    let fixed = prefix.width() + status_width + 2 + metrics.width() + 2;
    let summary_width = width.saturating_sub(fixed);
    let summary = truncate_to_width(&record.summary, summary_width);
    let padding = summary_width.saturating_sub(summary.width());
    let summary_style = if matched {
        Style::default().fg(Color::Black).bg(Color::Yellow)
    } else if record.status == RecordStatus::Error {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    };
    let (number, rest) = prefix.split_at(prefix.find(record.kind.label()).unwrap_or(0));
    let mut spans = vec![
        Span::styled(number.to_owned(), Style::default().fg(Color::DarkGray)),
        Span::styled(rest.to_owned(), kind_style(record.kind)),
    ];
    if let Some((symbol, color)) = status {
        spans.push(Span::styled(symbol, Style::default().fg(color)));
    }
    spans.push(Span::styled(summary, summary_style));
    spans.push(Span::raw(" ".repeat(padding + 2)));
    spans.push(Span::styled(metrics, Style::default().fg(Color::DarkGray)));
    match mark {
        Some((symbol, color)) => spans.push(Span::styled(
            format!(" {symbol}"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        None => spans.push(Span::raw("  ")),
    }
    Line::from(spans)
}

fn timeline_bar(trajectory: &Trajectory, origin: u64, end: u64, width: usize) -> String {
    let (Some(start), Some(stop)) = (trajectory.started_ms(), trajectory.ended_ms()) else {
        return "·".repeat(width);
    };
    let span = end.saturating_sub(origin).max(1) as f64;
    let cell = |at: u64| ((at.saturating_sub(origin)) as f64 / span * width as f64) as usize;
    let from = cell(start).min(width.saturating_sub(1));
    let to = cell(stop).clamp(from + 1, width);
    (0..width)
        .map(|index| {
            if (from..to).contains(&index) {
                '█'
            } else {
                '░'
            }
        })
        .collect()
}

fn stats_line(trajectory: &Trajectory, width: usize) -> String {
    let stats = &trajectory.stats;
    let kv = match (stats.requests, stats.kv_issues()) {
        (0, _) => "KV n/a".to_owned(),
        (_, 0) => "KV ✓".to_owned(),
        (_, issues) => format!("KV {issues} issue(s)"),
    };
    let ttft = stats.avg_ttft_ms.map_or("-".into(), format_duration_ms);
    if width < 110 {
        let cache = stats
            .cache_hit_ratio()
            .map_or("n/a".into(), |ratio| format!("{:.0}%", ratio * 100.0));
        return format!(
            "Σ {} req · {}/{} · cache {cache} · TTFT {ttft} · {kv}",
            stats.requests,
            format_tokens(stats.input_tokens),
            format_tokens(stats.output_tokens),
        );
    }
    let cache = match stats.cache_hit_ratio() {
        Some(ratio) => format!(
            "cache read {} write {} ({:.0}% of input)",
            format_tokens(stats.cache_read_tokens),
            format_tokens(stats.cache_write_tokens),
            ratio * 100.0
        ),
        None => "cache n/a".into(),
    };
    format!(
        "Σ {} req · in {} · out {} · reasoning {} · {cache} · avg TTFT {ttft} · {kv}",
        stats.requests,
        format_tokens(stats.input_tokens),
        format_tokens(stats.output_tokens),
        format_tokens(stats.reasoning_tokens),
    )
}

fn content_section(record: &TrajectoryRecord) -> String {
    let mut text = format!(
        "Type: {} · #{} · turn {}",
        record.kind.label(),
        record.index,
        record.turn
    );
    if let Some(tool) = &record.tool_name {
        text.push_str(&format!(" · tool {tool}"));
    }
    if let Some(model) = &record.model {
        text.push_str(&format!(" · model {model}"));
    }
    text.push_str(match record.status {
        RecordStatus::Success => " · ok",
        RecordStatus::Error => " · error",
        RecordStatus::InProgress => " · in progress",
    });
    if let Some(error) = &record.error {
        text.push_str(&format!("\nError: {error}"));
    }
    if let Some(calls) = record
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("tool_calls"))
        .and_then(serde_json::Value::as_array)
    {
        text.push_str("\n\n── Tool calls ──");
        for call in calls {
            text.push_str(&format!(
                "\n{} {}",
                call["name"].as_str().unwrap_or("tool"),
                call["arguments"]
            ));
        }
    }
    let body = serde_json::from_str::<serde_json::Value>(&record.detail)
        .ok()
        .filter(|value| value.is_object() || value.is_array())
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| record.detail.clone());
    if !body.trim().is_empty() {
        text.push_str("\n\n── Content ──\n");
        text.push_str(&body);
    }
    text
}

fn metrics_section(record: &TrajectoryRecord, origin: u64) -> String {
    let mut lines = vec![format!(
        "Recorded: {} (+{})",
        crate::b3::unix_ms_to_rfc3339(record.timestamp_ms),
        format_offset_ms(record.timestamp_ms, origin)
    )];
    match record.usage {
        Some(usage) => {
            lines.push(format!(
                "Tokens:   input {}  output {}  reasoning {}  total {}",
                usage.input_tokens, usage.output_tokens, usage.reasoning_tokens, usage.total_tokens
            ));
            let ratio = record.cache_ratio().map_or("not reported".into(), |ratio| {
                format!("{:.1}% hit", ratio * 100.0)
            });
            lines.push(format!(
                "Cache:    read {}  write {}  uncached {}  ({ratio})",
                usage.cache_read_tokens,
                usage.cache_write_tokens,
                usage.input_tokens.saturating_sub(usage.cache_read_tokens)
            ));
        }
        None if record.kind.is_request() => {
            lines.push("Tokens:   provider reported no usage".into())
        }
        None => lines.push(format!(
            "Tokens:   ~{} (estimated from content)",
            record.estimated_tokens()
        )),
    }
    if let Some(request) = &record.request {
        let ttft = request.ttft_ms().map_or("-".into(), format_duration_ms);
        let throughput = record
            .usage
            .and_then(|usage| request.throughput(usage.output_tokens))
            .map_or("-".into(), |rate| format!("{rate:.0} tok/s"));
        lines.push(format!(
            "Timing:   TTFT {ttft}  total {}  decode {throughput}",
            format_duration_ms(request.duration_ms())
        ));
        lines.push(String::new());
        lines.push("── Prompt prefix ──".into());
        lines.push(format!(
            "Instructions digest  {}",
            request.instructions_digest
        ));
        lines.push(format!("Tools digest         {}", request.tools_digest));
        let shared = match (request.shared_turns, request.previous_context_turns) {
            (Some(shared), Some(previous)) => {
                format!("{shared} of previous {previous} turns shared")
            }
            _ => "no previous request in this process".into(),
        };
        lines.push(format!(
            "Context turns        {} ({shared})",
            request.context_turns
        ));
    } else if record.kind.is_request() {
        lines.push("Timing:   not recorded (journal predates request metadata)".into());
    }
    if let Some(verdict) = &record.cache {
        lines.push(String::new());
        lines.push("── KV cache ──".into());
        let label = match verdict.severity {
            CacheSeverity::Ok => "ok",
            CacheSeverity::Info => "info",
            CacheSeverity::Warn => "warning",
            CacheSeverity::Miss => "miss",
        };
        lines.push(format!("Verdict: {label} — {}", verdict.message));
        if let Some(hint) = verdict.hint {
            lines.push(format!("Hint:    {hint}"));
        }
    }
    if let Some(error) = &record.error {
        lines.push(String::new());
        lines.push(format!("Error:   {error}"));
    }
    lines.join("\n")
}

fn styled_inspector_line(text: String) -> Line<'static> {
    let style = if text.starts_with("──") {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else if text.starts_with("Verdict: warning") {
        Style::default().fg(Color::Yellow)
    } else if text.starts_with("Verdict: miss") || text.starts_with("Error") {
        Style::default().fg(Color::Red)
    } else if text.starts_with("Hint:") {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
    };
    Line::styled(text, style)
}

fn help_lines() -> Vec<Line<'static>> {
    let rows = [
        ("Levels", ""),
        ("  1 / a", "agent overview"),
        ("  2", "trajectory records of the selected agent"),
        ("  3 / Enter", "inspect the selected record"),
        ("  Esc", "back one level (closes from the overview)"),
        ("  q / Ctrl-C", "close the viewer"),
        ("Records", ""),
        ("  ↑↓ j k, PgUp PgDn", "move"),
        ("  g / G, Home / End", "first / last row"),
        ("  Space", "fold or unfold the current turn"),
        ("  / then n / N", "search records, next / previous match"),
        ("  c", "jump to the next KV-cache warning or miss"),
        ("Inspector", ""),
        ("  Tab / Shift-Tab", "Content · Metrics · Raw"),
        ("  ← →", "previous / next record"),
        ("  y", "copy the current section"),
        ("Agents", ""),
        ("  R", "reload the selected worker journal"),
        ("KV marks", ""),
        (
            "  ⚡n%",
            "share of the prompt served from the provider cache",
        ),
        (
            "  ⚠",
            "instructions, tools or earlier turns changed: prefix cache lost",
        ),
        (
            "  ✗",
            "prefix stable but the provider reported a cache miss",
        ),
        ("  ℹ", "prefix rewritten by compaction (expected)"),
        ("", "press any key to return"),
    ];
    rows.iter()
        .map(|(key, text)| {
            if text.is_empty() {
                Line::styled(*key, Style::default().add_modifier(Modifier::BOLD))
            } else {
                Line::from(vec![
                    Span::styled(format!("{key:<24}"), Style::default().fg(Color::Cyan)),
                    Span::raw(*text),
                ])
            }
        })
        .collect()
}
