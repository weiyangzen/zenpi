//! Project identities and directory selection, independent of BentoBox panes.
//!
//! A host prepares a candidate with `with_directory`, prepares its session,
//! tools and configuration for `candidate.active().cwd()`, then publishes both
//! together. The owner pool prepares sessions without changing the process cwd.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const PROJECT_WORKSPACE_SCHEMA_VERSION: u16 = 1;
/// Resource caps for retained project identities and their JSON checkpoint.
pub const MAX_PROJECT_TABS: usize = 64;
pub const MAX_PROJECT_PATH_BYTES: usize = 4096;
pub const MAX_PROJECT_WORKSPACE_BYTES: usize = 512 * 1024;

/// Stable across restarts and independent of the displayed directory name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(String);

impl ProjectId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTab {
    id: ProjectId,
    cwd: PathBuf,
}

impl ProjectTab {
    pub fn id(&self) -> &ProjectId {
        &self.id
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Display text only; callers must use terminal cell widths for hit tests.
    /// Identically named directories deliberately have distinct IDs.
    pub fn title(&self) -> &str {
        self.cwd
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_else(|| self.cwd.to_str().expect("validated UTF-8 cwd"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenOutcome {
    Cancelled,
    Opened(ProjectId),
    Selected(ProjectId),
}

#[derive(Debug, Error)]
pub enum ProjectWorkspaceError {
    #[error("select an existing working folder")]
    EmptyPath,
    #[error("working folder must be an existing directory")]
    NotDirectory,
    #[error(
        "working folder must be UTF-8, at most {MAX_PROJECT_PATH_BYTES} bytes, and contain no control characters"
    )]
    InvalidPath,
    #[error("relative selection requires an absolute base directory")]
    RelativeBase,
    #[error("project tab limit reached ({MAX_PROJECT_TABS})")]
    Capacity,
    #[error("project tab was not found")]
    UnknownProject,
    #[error("the last project tab cannot be closed")]
    LastProject,
    #[error("unsupported project workspace schema")]
    Schema,
    #[error("project checkpoint identity or selection is invalid")]
    InvalidCheckpoint,
    #[error("project checkpoint exceeds {MAX_PROJECT_WORKSPACE_BYTES} bytes")]
    CheckpointTooLarge,
    #[error("working folder I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("project checkpoint JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// The checkpoint contains no credentials, transcript, or layout data.
/// Deserialize only via `from_json_bytes`, which validates all identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectWorkspace {
    schema_version: u16,
    tabs: Vec<ProjectTab>,
    active: Option<ProjectId>,
}

impl Default for ProjectWorkspace {
    fn default() -> Self {
        Self {
            schema_version: PROJECT_WORKSPACE_SCHEMA_VERSION,
            tabs: Vec::new(),
            active: None,
        }
    }
}

impl ProjectWorkspace {
    pub fn tabs(&self) -> &[ProjectTab] {
        &self.tabs
    }

    pub fn active(&self) -> Option<&ProjectTab> {
        self.active
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|tab| &tab.id == id))
    }

    /// Prepare a new projection without changing the current selection. A
    /// picker passes `None` on cancellation; there is never a placeholder tab.
    /// Host context preparation may fail after this returns: discard the
    /// candidate in that case and retain the current workspace and owner.
    pub fn with_directory(
        &self,
        selected: Option<&Path>,
        base: &Path,
    ) -> Result<(Self, OpenOutcome), ProjectWorkspaceError> {
        let Some(selected) = selected else {
            return Ok((self.clone(), OpenOutcome::Cancelled));
        };
        let cwd = normalize_directory(selected, base)?;
        let id = directory_id(&cwd);
        let mut candidate = self.clone();
        let outcome = if candidate.tabs.iter().any(|tab| tab.id == id) {
            OpenOutcome::Selected(id.clone())
        } else {
            if candidate.tabs.len() >= MAX_PROJECT_TABS {
                return Err(ProjectWorkspaceError::Capacity);
            }
            candidate.tabs.push(ProjectTab {
                id: id.clone(),
                cwd,
            });
            OpenOutcome::Opened(id.clone())
        };
        candidate.active = Some(id);
        Ok((candidate, outcome))
    }

    /// Selection candidates use IDs, never names or mutable display indices.
    pub fn with_active(&self, id: &ProjectId) -> Result<Self, ProjectWorkspaceError> {
        let tab = self
            .tabs
            .iter()
            .find(|tab| &tab.id == id)
            .ok_or(ProjectWorkspaceError::UnknownProject)?;
        // Recheck availability before the host starts preparing this owner.
        if normalize_directory(&tab.cwd, &tab.cwd)? != tab.cwd {
            return Err(ProjectWorkspaceError::InvalidCheckpoint);
        }
        let mut candidate = self.clone();
        candidate.active = Some(id.clone());
        Ok(candidate)
    }

    pub fn without_project(&self, id: &ProjectId) -> Result<Self, ProjectWorkspaceError> {
        let index = self
            .tabs
            .iter()
            .position(|tab| &tab.id == id)
            .ok_or(ProjectWorkspaceError::UnknownProject)?;
        if self.tabs.len() == 1 {
            return Err(ProjectWorkspaceError::LastProject);
        }
        let mut candidate = self.clone();
        candidate.tabs.remove(index);
        if candidate.active.as_ref() == Some(id) {
            let next = index.min(candidate.tabs.len() - 1);
            candidate.active = Some(candidate.tabs[next].id.clone());
        }
        Ok(candidate)
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, ProjectWorkspaceError> {
        let bytes = serde_json::to_vec(self)?;
        if bytes.len() > MAX_PROJECT_WORKSPACE_BYTES {
            return Err(ProjectWorkspaceError::CheckpointTooLarge);
        }
        Ok(bytes)
    }

    /// Restore all-or-nothing. Missing directories, aliases, duplicate IDs,
    /// invalid selection and unsupported schemas leave the caller unchanged.
    /// Restoring does not create directories, sessions, or configuration.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, ProjectWorkspaceError> {
        if bytes.len() > MAX_PROJECT_WORKSPACE_BYTES {
            return Err(ProjectWorkspaceError::CheckpointTooLarge);
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Checkpoint {
            schema_version: u16,
            tabs: Vec<ProjectTab>,
            active: Option<ProjectId>,
        }
        let saved: Checkpoint = serde_json::from_slice(bytes)?;
        if saved.schema_version != PROJECT_WORKSPACE_SCHEMA_VERSION {
            return Err(ProjectWorkspaceError::Schema);
        }
        if saved.tabs.len() > MAX_PROJECT_TABS {
            return Err(ProjectWorkspaceError::Capacity);
        }
        let mut ids = BTreeSet::new();
        for tab in &saved.tabs {
            if !tab.cwd.is_absolute() {
                return Err(ProjectWorkspaceError::InvalidCheckpoint);
            }
            let canonical = normalize_directory(&tab.cwd, &tab.cwd)?;
            if canonical != tab.cwd || directory_id(&canonical) != tab.id || !ids.insert(&tab.id) {
                return Err(ProjectWorkspaceError::InvalidCheckpoint);
            }
        }
        if saved.tabs.is_empty() != saved.active.is_none()
            || saved.active.as_ref().is_some_and(|id| !ids.contains(id))
        {
            return Err(ProjectWorkspaceError::InvalidCheckpoint);
        }
        Ok(Self {
            schema_version: saved.schema_version,
            tabs: saved.tabs,
            active: saved.active,
        })
    }
}

/// Resolve a picker path against its explicit browsing base. Symlinks and
/// `..` refer to their actual filesystem target; process-wide cwd is untouched.
pub fn normalize_directory(selected: &Path, base: &Path) -> Result<PathBuf, ProjectWorkspaceError> {
    if selected.as_os_str().is_empty() {
        return Err(ProjectWorkspaceError::EmptyPath);
    }
    validate_path_text(selected)?;
    let path = if selected.is_absolute() {
        selected.to_path_buf()
    } else {
        if !base.is_absolute() {
            return Err(ProjectWorkspaceError::RelativeBase);
        }
        base.join(selected)
    };
    let canonical = path.canonicalize()?;
    if !canonical.is_dir() {
        return Err(ProjectWorkspaceError::NotDirectory);
    }
    validate_path_text(&canonical)?;
    Ok(canonical)
}

fn validate_path_text(path: &Path) -> Result<(), ProjectWorkspaceError> {
    let text = path.to_str().ok_or(ProjectWorkspaceError::InvalidPath)?;
    if text.len() > MAX_PROJECT_PATH_BYTES || text.chars().any(char::is_control) {
        return Err(ProjectWorkspaceError::InvalidPath);
    }
    Ok(())
}

fn directory_id(cwd: &Path) -> ProjectId {
    let mut hash = Sha256::new();
    hash.update(b"zenpi-project-workspace-v1\0");
    hash.update(cwd.to_str().expect("validated UTF-8 cwd").as_bytes());
    ProjectId(format!("{:x}", hash.finalize()))
}

/// Stable correlation stamped onto admitted work, independent of selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectContext {
    pub project_id: String,
    pub cwd: String,
    pub session_id: String,
}
impl ProjectContext {
    pub fn from_agent(project_id: &str, agent: &crate::core::Agent) -> Self {
        let cwd = agent
            .attachment_workspace_root()
            .unwrap_or_else(|| Path::new(&agent.session().header().cwd));
        // Contexts are shared between TUI/headless owners and survive a
        // restart. Keep the identity canonical so a relative session/header
        // can never make a later request resolve against process cwd.
        let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
        Self {
            project_id: project_id.to_owned(),
            cwd: cwd.display().to_string(),
            session_id: agent.session().session_id().to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct NewSessionPlan {
    context: ProjectContext,
    old_path: PathBuf,
    workspace: ProjectWorkspace,
    sessions: std::collections::BTreeMap<String, PathBuf>,
    checkpoint: PathBuf,
    previous: Option<Vec<u8>>,
    request_id: Option<String>,
    fingerprint: String,
    version: u16,
}

#[derive(Debug)]
pub(crate) struct NewSessionCommit {
    pub(crate) context: ProjectContext,
    pub(crate) path: PathBuf,
    pub(crate) data: serde_json::Value,
    checkpoint_bytes: Vec<u8>,
}

impl NewSessionPlan {
    pub(crate) fn execute(
        self,
        agent: &mut crate::core::Agent,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<NewSessionCommit, crate::core::AgentError> {
        use crate::core::AgentError;
        if agent.session().session_id() != self.context.session_id
            || agent
                .session()
                .path()
                .canonicalize()
                .map_err(crate::session::SessionError::from)?
                != self
                    .old_path
                    .canonicalize()
                    .map_err(crate::session::SessionError::from)?
        {
            return Err(AgentError::InvalidTurn(
                "new session request belongs to an old owner".into(),
            ));
        }
        let digest = format!(
            "{:x}",
            Sha256::digest(
                format!(
                    "{}:{}:{}",
                    self.context.session_id,
                    self.request_id.as_deref().unwrap_or("tui"),
                    self.fingerprint
                )
                .as_bytes()
            )
        );
        let mut committed = None;
        agent.new_session_with_commit(&digest, cancelled, |replacement, file| {
            let path = replacement.path().to_path_buf();
            let mut context = self.context.clone();
            context.session_id = replacement.session_id().to_owned();
            let data = serde_json::json!({
                "command":"new", "route":"local", "created":true,
                "project_id":context.project_id, "cwd":context.cwd,
                "old_session_id":self.context.session_id, "old_session_path":self.old_path,
                "new_session_id":context.session_id, "new_session_path":path,
            });
            file.append_event(
                replacement,
                serde_json::json!({
                    "type":"session_new_transition", "version":self.version,
                    "request_id":self.request_id, "fingerprint":self.fingerprint,
                    "project":context, "data":data,
                }),
            )?;
            let mut sessions = self.sessions;
            sessions.insert(context.project_id.clone(), path.clone());
            let bytes = serde_json::to_vec(&serde_json::json!({
                "schema_version":1, "workspace":self.workspace, "sessions":sessions
            }))
            .map_err(|e| AgentError::InvalidTurn(e.to_string()))?;
            file.verify()?;
            if cancelled() {
                return Err(crate::backend::BackendError::Cancelled.into());
            }
            write_owner_checkpoint(&self.checkpoint, self.previous.as_deref(), &bytes)
                .map_err(AgentError::InvalidTurn)?;
            committed = Some(NewSessionCommit {
                context,
                path,
                data,
                checkpoint_bytes: bytes,
            });
            Ok(())
        })?;
        Ok(committed.expect("successful new session committed its checkpoint"))
    }
}

/// Actual project runtimes shared by TUI and the owned JSONL host. View labels
/// never enter this map; every key comes from canonical ProjectWorkspace IDs.
pub struct ProjectOwnerPool {
    workspace: ProjectWorkspace,
    owners:
        std::collections::BTreeMap<String, std::sync::Arc<std::sync::Mutex<crate::core::Agent>>>,
    /// Independent arch master-session owners (ZS1-156). They are created
    /// lazily, use a distinct journal, and are never part of the durable
    /// project checkpoint, so the discussion lane cannot adopt their history.
    arch_owners:
        std::collections::BTreeMap<String, std::sync::Arc<std::sync::Mutex<crate::core::Agent>>>,
    sessions: std::collections::BTreeMap<String, PathBuf>,
    overrides: crate::config::ConfigOverrides,
    echo_fixture: bool,
    session_root: PathBuf,
    initial_session: (String, PathBuf),
    contexts: std::collections::BTreeMap<String, ProjectContext>,
    checkpoint: Option<(PathBuf, Option<Vec<u8>>)>,
}
impl ProjectOwnerPool {
    pub fn new(
        agent: std::sync::Arc<std::sync::Mutex<crate::core::Agent>>,
    ) -> Result<Self, String> {
        let owner = agent.lock().map_err(|_| "project owner lock poisoned")?;
        let cwd = owner
            .attachment_workspace_root()
            .unwrap_or_else(|| Path::new(&owner.session().header().cwd));
        let workspace = ProjectWorkspace::default()
            .with_directory(Some(cwd), cwd)
            .map_err(|e| e.to_string())?
            .0;
        let id = workspace
            .active()
            .ok_or("missing initial project")?
            .id()
            .as_str()
            .to_owned();
        let session = owner.session().path().to_path_buf();
        let session = if session.is_absolute() {
            session
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(session)
        };
        Ok(Self {
            workspace,
            owners: [(id.clone(), std::sync::Arc::clone(&agent))].into(),
            arch_owners: std::collections::BTreeMap::new(),
            initial_session: (id.clone(), session.clone()),
            contexts: [(id.clone(), ProjectContext::from_agent(&id, &owner))].into(),
            checkpoint: None,
            sessions: [(id, session.clone())].into(),
            overrides: owner.project_overrides(),
            echo_fixture: owner.backend_name() == "echo",
            session_root: session
                .parent()
                .ok_or("session has no parent")?
                .join("projects"),
        })
    }
    pub fn context(&self, id: &str) -> Option<&ProjectContext> {
        self.contexts.get(id)
    }
    pub fn active_context(&self) -> &ProjectContext {
        self.context(
            self.workspace
                .active()
                .expect("committed project")
                .id()
                .as_str(),
        )
        .expect("prepared context")
    }
    pub fn refresh_owner_context(&mut self, id: &str, agent: &crate::core::Agent) {
        self.contexts
            .insert(id.to_owned(), ProjectContext::from_agent(id, agent));
        let session_path = agent.session().path();
        let session_path = session_path.canonicalize().unwrap_or_else(|_| {
            if session_path.is_absolute() {
                session_path.to_path_buf()
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(session_path))
                    .unwrap_or_else(|_| session_path.to_path_buf())
            }
        });
        self.sessions.insert(id.to_owned(), session_path);
    }
    /// A worker owns only a checkpoint proposal, never another Agent. The
    /// existing compare-and-replace writer rejects concurrent host changes.
    pub(crate) fn new_session_plan(
        &self,
        id: &str,
        request_id: Option<String>,
        fingerprint: String,
        version: u16,
    ) -> Result<NewSessionPlan, String> {
        let (path, previous) = self
            .checkpoint
            .clone()
            .ok_or("new session requires a durable project owner checkpoint")?;
        Ok(NewSessionPlan {
            context: self.contexts.get(id).ok_or("project not found")?.clone(),
            old_path: self
                .sessions
                .get(id)
                .ok_or("project session not found")?
                .clone(),
            workspace: self.workspace.clone(),
            sessions: self.sessions.clone(),
            checkpoint: path,
            previous,
            request_id,
            fingerprint,
            version,
        })
    }

    pub(crate) fn accept_new_session(&mut self, committed: &NewSessionCommit) {
        self.contexts.insert(
            committed.context.project_id.clone(),
            committed.context.clone(),
        );
        self.sessions
            .insert(committed.context.project_id.clone(), committed.path.clone());
        if let Some((_, previous)) = &mut self.checkpoint {
            *previous = Some(committed.checkpoint_bytes.clone());
        }
    }

    /// Commit a fully prepared resume while the caller still holds the old
    /// Agent. Only the infallible Agent replacement may follow this method.
    pub(crate) fn commit_resumed_session(
        &mut self,
        id: &str,
        session: &crate::session::SessionStore,
    ) -> Result<(), String> {
        let mut context = self.contexts.get(id).ok_or("project not found")?.clone();
        let path = session.path().canonicalize().map_err(|e| e.to_string())?;
        validate_path_text(&path).map_err(|e| e.to_string())?;
        let mut sessions = self.sessions.clone();
        sessions.insert(id.to_owned(), path);
        context.session_id = session.session_id().to_owned();
        if let Some((path, previous)) = &mut self.checkpoint {
            let bytes = serde_json::to_vec(&serde_json::json!({
                "schema_version": 1, "workspace": self.workspace, "sessions": sessions
            }))
            .map_err(|e| e.to_string())?;
            write_owner_checkpoint(path, previous.as_deref(), &bytes)?;
            *previous = Some(bytes);
        }
        self.sessions = sessions;
        self.contexts.insert(id.to_owned(), context);
        Ok(())
    }
    /// Enable a session-scoped checkpoint shared by both production hosts.
    /// Strict validation precedes owner preparation; a failed restore leaves
    /// the initial owner usable and returns an explicit error to the host.
    pub fn restore_checkpoint(&mut self) -> Result<bool, String> {
        let path = self
            .session_root
            .parent()
            .ok_or("missing session parent")?
            .join("project-workspace.json");
        let bytes = read_owner_checkpoint(&path)?;
        let Some(bytes) = bytes else {
            self.checkpoint = Some((path, None));
            return Ok(false);
        };
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Saved {
            schema_version: u16,
            workspace: serde_json::Value,
            sessions: std::collections::BTreeMap<String, PathBuf>,
        }
        let saved: Saved = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if saved.schema_version != 1 {
            return Err("unsupported project owner checkpoint".into());
        }
        let workspace = ProjectWorkspace::from_json_bytes(
            &serde_json::to_vec(&saved.workspace).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if saved.sessions.len() != workspace.tabs().len()
            || saved.sessions.iter().any(|(id, path)| {
                !workspace.tabs().iter().any(|tab| tab.id().as_str() == id)
                    || !path.is_absolute()
                    || validate_path_text(path).is_err()
            })
        {
            return Err("invalid project session mapping".into());
        }
        self.publish(workspace, &saved.sessions)?;
        self.checkpoint = Some((path, Some(bytes)));
        Ok(true)
    }
    pub fn workspace(&self) -> &ProjectWorkspace {
        &self.workspace
    }
    pub fn sessions(&self) -> &std::collections::BTreeMap<String, PathBuf> {
        &self.sessions
    }
    pub fn owner(&self, id: &str) -> Option<std::sync::Arc<std::sync::Mutex<crate::core::Agent>>> {
        self.owners.get(id).cloned()
    }

    /// Lazily prepare the independent arch master-session owner for a project
    /// (ZS1-156). It opens `<session dir>/arch.jsonl`, so it has its own
    /// journal, model, and approval coordinator distinct from the discussion
    /// owner.
    pub fn arch_agent(
        &mut self,
        id: &str,
    ) -> Result<std::sync::Arc<std::sync::Mutex<crate::core::Agent>>, String> {
        if let Some(existing) = self.arch_owners.get(id) {
            return Ok(existing.clone());
        }
        let active = self
            .owners
            .get(id)
            .cloned()
            .ok_or("project owner not found")?;
        let (arch_session, cwd, overrides, echo_fixture) = {
            let agent = active.lock().map_err(|_| "project owner lock poisoned")?;
            let session = agent.session().path().to_path_buf();
            let arch_session = session.with_file_name("arch.jsonl");
            let cwd = agent
                .attachment_workspace_root()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(agent.session().header().cwd.clone()));
            (
                arch_session,
                cwd,
                agent.project_overrides(),
                agent.backend_name() == "echo",
            )
        };
        let agent = crate::core::Agent::prepare_project_with_options(
            &arch_session,
            &cwd,
            overrides,
            echo_fixture,
        )
        .map_err(|error| error.to_string())?;
        let handle = std::sync::Arc::new(std::sync::Mutex::new(agent));
        self.arch_owners.insert(id.to_owned(), handle.clone());
        Ok(handle)
    }

    pub fn close_all(&self) {
        for owner in self.owners.values() {
            if let Ok(mut agent) = owner.lock() {
                agent.close();
            }
        }
        for owner in self.arch_owners.values() {
            if let Ok(mut agent) = owner.lock() {
                agent.close();
            }
        }
    }
    pub fn owner_handles(
        &self,
    ) -> Vec<(String, std::sync::Arc<std::sync::Mutex<crate::core::Agent>>)> {
        self.owners
            .iter()
            .map(|(id, owner)| (id.clone(), owner.clone()))
            .collect()
    }
    pub fn active(&self) -> std::sync::Arc<std::sync::Mutex<crate::core::Agent>> {
        self.owner(
            self.workspace
                .active()
                .expect("committed project")
                .id()
                .as_str(),
        )
        .expect("prepared owner")
    }
    /// Prepare configuration, durable session and tool cwd before publishing a
    /// new active ID. Existing running owners need no lock for selection.
    pub fn publish(
        &mut self,
        candidate: ProjectWorkspace,
        saved_sessions: &std::collections::BTreeMap<String, PathBuf>,
    ) -> Result<(), String> {
        let tab = candidate.active().ok_or("no active project")?;
        let id = tab.id().as_str().to_owned();
        let mut next_sessions = std::collections::BTreeMap::new();
        for tab in candidate.tabs() {
            let key = tab.id().as_str();
            let path = saved_sessions
                .get(key)
                .or_else(|| self.sessions.get(key))
                .or_else(|| (key == self.initial_session.0).then_some(&self.initial_session.1))
                .cloned()
                .unwrap_or_else(|| self.session_root.join(key).join("session.jsonl"));
            let path = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map_err(|e| e.to_string())?
                    .join(path)
            };
            validate_path_text(&path).map_err(|e| e.to_string())?;
            next_sessions.insert(key.to_owned(), path);
        }
        // An explicit session resume may change a retained project's journal.
        // Restore its actual Agent instead of keeping the startup handle and
        // publishing only the saved label/session metadata.
        for (key, owner) in &self.owners {
            if next_sessions.get(key) != self.sessions.get(key) {
                let agent = owner
                    .try_lock()
                    .map_err(|_| "Project is running; cancel or wait before replacing it")?;
                if agent.phase() == crate::core::AgentPhase::Running {
                    return Err("Project is running".into());
                }
            }
        }
        let prepared =
            if self.owners.contains_key(&id) && self.sessions.get(&id) == next_sessions.get(&id) {
                None
            } else {
                Some(
                    crate::core::Agent::prepare_project_with_options(
                        next_sessions.get(&id).ok_or("missing project session")?,
                        tab.cwd(),
                        self.overrides.clone(),
                        self.echo_fixture,
                    )
                    .map_err(|e| e.to_string())?,
                )
            };
        if let Some((path, previous)) = &mut self.checkpoint {
            let bytes = serde_json::to_vec(&serde_json::json!({"schema_version": 1, "workspace": candidate, "sessions": next_sessions})).map_err(|e| e.to_string())?;
            write_owner_checkpoint(path, previous.as_deref(), &bytes)?;
            *previous = Some(bytes);
        }
        self.owners.retain(|key, owner| {
            let keep = next_sessions.get(key) == self.sessions.get(key);
            if !keep && let Ok(mut agent) = owner.try_lock() {
                agent.close();
            }
            keep
        });
        self.contexts.retain(|id, _| self.owners.contains_key(id));
        self.arch_owners.retain(|key, owner| {
            let keep = self.owners.contains_key(key);
            if !keep && let Ok(mut agent) = owner.try_lock() {
                agent.close();
            }
            keep
        });
        if let Some(agent) = prepared {
            self.contexts
                .insert(id.clone(), ProjectContext::from_agent(&id, &agent));
            self.owners
                .insert(id, std::sync::Arc::new(std::sync::Mutex::new(agent)));
        }
        self.sessions = next_sessions;
        self.workspace = candidate;
        Ok(())
    }
    pub fn open(&mut self, cwd: Option<&Path>) -> Result<OpenOutcome, String> {
        let base = self.workspace.active().ok_or("no active project")?.cwd();
        let (candidate, outcome) = self
            .workspace
            .with_directory(cwd, base)
            .map_err(|e| e.to_string())?;
        self.publish(candidate, &Default::default())?;
        Ok(outcome)
    }
    pub fn select(&mut self, id: &str) -> Result<(), String> {
        let tab = self
            .workspace
            .tabs()
            .iter()
            .find(|tab| tab.id().as_str() == id)
            .ok_or("project not found")?;
        let candidate = self
            .workspace
            .with_active(tab.id())
            .map_err(|e| e.to_string())?;
        self.publish(candidate, &Default::default())
    }
    pub fn close(&mut self, id: &str) -> Result<(), String> {
        let tab = self
            .workspace
            .tabs()
            .iter()
            .find(|tab| tab.id().as_str() == id)
            .ok_or("project not found")?;
        let candidate = self
            .workspace
            .without_project(tab.id())
            .map_err(|e| e.to_string())?;
        self.publish(candidate, &Default::default())
    }
}

/// Refuse links and oversized input before parsing or allocating its contents.
fn read_owner_checkpoint(path: &Path) -> Result<Option<Vec<u8>>, String> {
    use std::io::Read;
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_PROJECT_WORKSPACE_BYTES as u64
    {
        return Err("invalid or oversized project owner checkpoint".into());
    }
    let mut bytes = Vec::new();
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_PROJECT_WORKSPACE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_PROJECT_WORKSPACE_BYTES {
        return Err("project checkpoint exceeds byte limit".into());
    }
    Ok(Some(bytes))
}
fn write_owner_checkpoint(
    path: &Path,
    previous: Option<&[u8]>,
    bytes: &[u8],
) -> Result<(), String> {
    use std::io::Write;
    if bytes.len() > MAX_PROJECT_WORKSPACE_BYTES {
        return Err("project checkpoint exceeds byte limit".into());
    }
    let lock_path = path.with_extension("lock");
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(&lock_path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("project checkpoint is being updated".into());
        }
    }
    struct Unlock(std::fs::File);
    impl Drop for Unlock {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                use std::os::fd::AsRawFd;
                unsafe {
                    libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
                }
            }
        }
    }
    let _guard = Unlock(lock);
    if read_owner_checkpoint(path)?.as_deref() != previous {
        return Err(
            "project checkpoint changed in another host; reopen the host before modifying projects"
                .into(),
        );
    }
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
    let result = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|e| e.to_string())
}

/// Maximum number of layer-2 sub-tabs retained per project.
pub const MAX_PROJECT_SUBTABS: usize = 32;
const MAX_WORKTREE_OUTPUT_BYTES: usize = 256 * 1024;

/// One git worktree discovered for a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: std::path::PathBuf,
    pub branch: Option<String>,
    pub detached: bool,
}

fn run_git(project: &Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    if output.stdout.len() > MAX_WORKTREE_OUTPUT_BYTES {
        return Err("git output exceeded the bound".to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Read-only `git worktree list` for one project.
pub fn list_worktrees(project: &Path) -> Result<Vec<WorktreeEntry>, String> {
    let text = run_git(project, &["worktree", "list", "--porcelain"])?;
    let mut entries = Vec::new();
    let mut path: Option<std::path::PathBuf> = None;
    let mut detached = false;
    let mut branch: Option<String> = None;
    let mut flush = |path: &mut Option<std::path::PathBuf>,
                     detached: &mut bool,
                     branch: &mut Option<String>,
                     entries: &mut Vec<WorktreeEntry>| {
        if let Some(path) = path.take() {
            entries.push(WorktreeEntry {
                path,
                branch: branch.take(),
                detached: std::mem::take(detached),
            });
        }
    };
    for line in text.lines().chain(std::iter::once("")) {
        if let Some(value) = line.strip_prefix("worktree ") {
            flush(&mut path, &mut detached, &mut branch, &mut entries);
            path = Some(std::path::PathBuf::from(value));
        } else if line.strip_prefix("branch ").is_some() {
            branch = line
                .strip_prefix("branch ")
                .map(|value| value.trim_start_matches("refs/heads/").to_owned());
        } else if line == "detached" {
            detached = true;
        }
    }
    flush(&mut path, &mut detached, &mut branch, &mut entries);
    Ok(entries)
}

/// Create a new worktree at `path` on a fresh branch `branch`.
pub fn add_worktree(project: &Path, path: &Path, branch: &str) -> Result<(), String> {
    let path = path.to_str().ok_or("worktree path is not utf-8")?;
    run_git(project, &["worktree", "add", "-b", branch, path]).map(|_| ())
}

/// Remove a worktree (forced, as execution workers may leave it dirty).
pub fn remove_worktree(project: &Path, path: &Path) -> Result<(), String> {
    let path = path.to_str().ok_or("worktree path is not utf-8")?;
    run_git(project, &["worktree", "remove", "--force", path]).map(|_| ())
}
