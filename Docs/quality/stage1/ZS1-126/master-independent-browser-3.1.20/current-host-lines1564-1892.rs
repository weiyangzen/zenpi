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
