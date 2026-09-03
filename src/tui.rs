//! Interactive terminal mode.
//!
//! [`TuiState`] is deliberately independent from the agent.  This keeps
//! rendering and input handling testable with Ratatui's `TestBackend`, while
//! the runtime loop can connect any synchronous agent through a small
//! callback.  Ratatui keeps a previous cell buffer and emits only changed
//! cells; resize notifications are coalesced by [`RenderScheduler`] so a
//! resize drag or a burst of stream chunks does not cause a draw per event.

use std::collections::VecDeque;
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

/// Bound retained transcript memory even when a provider streams forever.
pub const DEFAULT_MAX_MESSAGES: usize = 2_048;
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;
pub const MAX_RENDER_LINES: usize = 8_192;
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
        if !self.messages.is_empty() {
            self.messages.clear();
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
        let chunk = chunk.as_ref();
        if chunk.is_empty() {
            return;
        }
        if let Some(last) = self.messages.back_mut()
            && last.role == role
            && last.text.len().saturating_add(chunk.len()) <= MAX_MESSAGE_BYTES
        {
            last.text.push_str(chunk);
            self.cached_transcript = None;
            self.scroll = 0;
            self.dirty = true;
            return;
        }
        self.push_message(role, chunk.to_owned());
    }

    pub fn set_input(&mut self, input: impl Into<String>) {
        self.input = bound_text(input.into());
        self.cursor = self.input.len();
        self.history_cursor = None;
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
                KeyCode::Char('c') | KeyCode::Char('d') if self.input.is_empty() => {
                    return TuiAction::Quit;
                }
                KeyCode::Char('c') => return TuiAction::Interrupt,
                KeyCode::Char('u') => {
                    self.input.drain(..self.cursor);
                    self.cursor = 0;
                    self.dirty = true;
                    return TuiAction::None;
                }
                KeyCode::Char('w') => {
                    self.delete_previous_word();
                    return TuiAction::None;
                }
                KeyCode::Char('l') => return TuiAction::Redraw,
                _ => {}
            }
        }
        match key.code {
            KeyCode::Char(character) if !modifiers.contains(KeyModifiers::ALT) => {
                self.insert_text(&character.to_string())
            }
            KeyCode::Backspace => self.delete_previous_char(),
            KeyCode::Delete => self.delete_next_char(),
            KeyCode::Left => self.cursor = previous_boundary(&self.input, self.cursor),
            KeyCode::Right => self.cursor = next_boundary(&self.input, self.cursor),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.input.len(),
            KeyCode::Up if self.input.is_empty() => self.history_move(-1),
            KeyCode::Down if self.input.is_empty() => self.history_move(1),
            KeyCode::PageUp => self.scroll_up(8),
            KeyCode::PageDown => self.scroll_down(8),
            KeyCode::Enter => {
                let text = self.input.trim().to_owned();
                if !text.is_empty() {
                    self.history.push_back(text.clone());
                    while self.history.len() > self.max_history {
                        self.history.pop_front();
                    }
                    self.input.clear();
                    self.cursor = 0;
                    self.history_cursor = None;
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
                self.dirty = true;
            }
            _ => {}
        }
        self.dirty = true;
        TuiAction::None
    }

    fn insert_text(&mut self, text: &str) {
        let remaining = MAX_MESSAGE_BYTES.saturating_sub(self.input.len());
        let text = truncate_bytes(text, remaining);
        if text.is_empty() {
            return;
        }
        self.input.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.history_cursor = None;
        self.dirty = true;
    }

    fn delete_previous_char(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = previous_boundary(&self.input, self.cursor);
        self.input.drain(start..self.cursor);
        self.cursor = start;
        self.dirty = true;
    }

    fn delete_next_char(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        let end = next_boundary(&self.input, self.cursor);
        self.input.drain(self.cursor..end);
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
        }
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(3),
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
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Conversation ");
        let inner = block.inner(area);
        let width = usize::from(inner.width).max(1);
        if self
            .cached_transcript
            .as_ref()
            .is_none_or(|(cached_width, _)| *cached_width != inner.width)
        {
            let mut lines = transcript_lines(&self.messages, width);
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

    fn render_input(&self, frame: &mut Frame<'_>, area: Rect) {
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
        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
        let (cursor_x, cursor_y) = cursor_position(&self.input, self.cursor, width);
        let x = inner
            .x
            .saturating_add(cursor_x.min(inner.width.saturating_sub(1)));
        let y = inner
            .y
            .saturating_add(cursor_y.min(inner.height.saturating_sub(1)));
        frame.set_cursor_position(Position::new(x, y));
    }

    fn render_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let text = truncate_to_width(
            " Enter send  |  Ctrl-C quit  |  PgUp/PgDn scroll ",
            usize::from(area.width),
        );
        frame.render_widget(
            Paragraph::new(Span::styled(text, Style::default().fg(Color::DarkGray))),
            area,
        );
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

/// Run the production TUI with provider work on the bounded runtime worker.
/// The older callback-based `run_with_state` remains available for embedders
/// and deterministic tests; the binary uses this owned form so a worker can
/// safely hold the agent for the duration of one request.
pub fn run_async(agent: crate::core::Agent) -> Result<(), crate::error::ZenpiError> {
    use crate::core::{AgentError, ProcessResult};
    use crate::runtime::{BackgroundRunner, JobOutcome, RuntimeConfig, RuntimeEvent};

    let shared = Arc::new(Mutex::new(agent));
    let worker_state = Arc::clone(&shared);
    let runner = BackgroundRunner::spawn(
        move |text: String, token| -> Result<ProcessResult, AgentError> {
            if token.is_cancelled() {
                return Err(AgentError::Backend(crate::backend::BackendError::Cancelled));
            }
            let mut agent = worker_state
                .lock()
                .map_err(|_| AgentError::InvalidTurn("agent lock poisoned".into()))?;
            let result = agent
                .process_with_cancel(crate::core::TurnInputRequest::new(text), || {
                    token.is_cancelled()
                })?;
            if token.is_cancelled() {
                return Err(AgentError::Backend(crate::backend::BackendError::Cancelled));
            }
            Ok(result)
        },
        RuntimeConfig::default(),
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
    }
    let config = TuiConfig::default();
    let mut guard = TerminalGuard::enter()
        .map_err(|error| crate::error::ZenpiError::Message(error.to_string()))?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)
        .map_err(|error| crate::error::ZenpiError::Message(error.to_string()))?;
    let frame_interval = config.frame_interval.max(MIN_LOOP_INTERVAL);
    let poll_interval = config.poll_interval.max(MIN_LOOP_INTERVAL);
    let mut scheduler = RenderScheduler::new(frame_interval);
    let mut resize_pending = false;
    let mut last_tick = Instant::now();
    let mut active_job = None;
    'outer: loop {
        while let Ok(event) = runner.try_next_event() {
            match event {
                RuntimeEvent::Completed { id, outcome } if Some(id) == active_job => {
                    active_job = None;
                    state.set_busy(false);
                    match outcome {
                        JobOutcome::Succeeded(result) => {
                            if let Some(assistant) = result.assistant {
                                state.push_message(MessageRole::Assistant, assistant.content);
                            }
                            state.set_status("Ready");
                        }
                        JobOutcome::Failed(error) => {
                            state.push_message(MessageRole::Error, error.to_string());
                            state.set_status("Request failed");
                        }
                        JobOutcome::Cancelled => state.set_status("Interrupted"),
                        JobOutcome::Panicked => {
                            state.push_message(MessageRole::Error, "background job panicked");
                            state.set_status("Request failed");
                        }
                    }
                    scheduler.request();
                }
                RuntimeEvent::Rejected { id, reason } if Some(id) == active_job => {
                    active_job = None;
                    state.set_busy(false);
                    state.push_message(MessageRole::Error, reason.to_string());
                    state.set_status("Request rejected");
                    scheduler.request();
                }
                RuntimeEvent::Closed => break,
                _ => {}
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
                .draw(|frame| state.render(frame, &config.title))
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
                    state.push_message(MessageRole::User, &text);
                    if active_job.is_some() {
                        state.push_message(MessageRole::Error, "a request is already running");
                        state.set_status("Busy");
                    } else {
                        match runner.try_submit(text) {
                            Ok(id) => {
                                active_job = Some(id);
                                state.set_busy(true);
                                state.set_status("Working");
                            }
                            Err(error) => {
                                state.push_message(MessageRole::Error, error.to_string());
                                state.set_status("Request rejected");
                            }
                        }
                    }
                }
                TuiAction::Interrupt => {
                    if let Some(id) = active_job {
                        let _ = runner.try_cancel(id);
                        state.set_status("Interrupt requested");
                    } else {
                        state.set_status("Ready");
                    }
                }
                TuiAction::Quit => break 'outer,
                TuiAction::Redraw | TuiAction::None => {}
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
    let _ = runner.try_shutdown();
    drop(runner);
    guard.leave();
    Ok(())
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
                TuiAction::Submit(text) => {
                    state.push_message(MessageRole::User, &text);
                    state.set_busy(true);
                    state.set_status("Working");
                    if let Err(error) = on_submit(text, &mut state) {
                        state.push_message(MessageRole::Error, error.to_string());
                        state.set_status("Request failed");
                    }
                    state.set_busy(false);
                }
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
}

impl TerminalGuard {
    fn enter() -> Result<Self, TuiError> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableBracketedPaste, Hide) {
            let _ = disable_raw_mode();
            return Err(TuiError::Io(error));
        }
        Ok(Self { active: true })
    }

    fn leave(&mut self) {
        if self.active {
            let mut stdout = io::stdout();
            let _ = execute!(stdout, Show, DisableBracketedPaste, LeaveAlternateScreen);
            let _ = disable_raw_mode();
            self.active = false;
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.leave();
    }
}

fn bound_text(text: String) -> String {
    truncate_bytes(&text, MAX_MESSAGE_BYTES).to_owned()
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
        let full_prefix = format!("{}: ", message.role.label());
        // Reserve one cell for message content before truncating the role
        // prefix.  Without this bound a one- or two-column viewport emitted
        // lines wider than the area and Ratatui had to repair the overflow.
        let prefix = truncate_to_width(&full_prefix, width.saturating_sub(1));
        let prefix_width = UnicodeWidthStr::width(prefix.as_str());
        let body_width = width.saturating_sub(prefix_width).max(1);
        let continuation = " ".repeat(prefix_width);
        for (logical_index, logical) in message.text.split('\n').enumerate() {
            for (wrapped_index, body) in wrap_plain(logical, body_width).into_iter().enumerate() {
                let left = if logical_index == 0 && wrapped_index == 0 {
                    prefix.clone()
                } else {
                    continuation.clone()
                };
                result.push(Line::from(vec![
                    Span::styled(left, message.role.style()),
                    Span::raw(body),
                ]));
                if result.len() >= MAX_RENDER_LINES.saturating_mul(2) {
                    return result;
                }
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
