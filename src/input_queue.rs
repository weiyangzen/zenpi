//! Durable, session-owned steering and follow-up inputs.
//!
//! Mutations append one event through the existing SessionStore writer before
//! publishing a projection. This module never opens a second journal/writer or
//! launches work. Hosts must build model context from `inputs_for_turn` as well
//! as ordinary turns; the applied event is the durable input commit, so a crash
//! between applying and requesting a provider never loses or reapplies an ID.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{protocol::InputQueueAction, runtime::InputBoundary, session::SessionStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Steer,
    FollowUp,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueueMode {
    #[default]
    #[serde(rename = "one-at-a-time", alias = "one_at_a_time")]
    OneAtATime,
    #[serde(rename = "all")]
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputStatus {
    Received,
    Applied,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueuedInput {
    pub id: String,
    pub kind: InputKind,
    pub text: String,
    pub sequence: u64,
    pub revision: u64,
    pub status: InputStatus,
    pub applied_turn_id: Option<String>,
    #[serde(default)]
    pub applied_parent_id: Option<String>,
    /// Original enqueue fingerprint survives explicit edits and terminal state.
    pub submitted_sha256: String,
}

#[derive(Debug, Clone, Copy)]
pub struct InputQueueLimits {
    pub max_pending: usize,
    pub max_records: usize,
    pub max_retained_bytes: usize,
}

impl Default for InputQueueLimits {
    fn default() -> Self {
        Self {
            max_pending: 32,
            max_records: 4096,
            max_retained_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Error)]
pub enum InputQueueError {
    #[error("invalid input queue request: {0}")]
    Invalid(String),
    #[error("input queue resource limit exceeded")]
    Capacity,
    #[error("input ID already exists with a different original payload")]
    Conflict,
    #[error("unknown input ID")]
    NotFound,
    #[error("input is already applied or cancelled")]
    Terminal,
    #[error("input edit revision is stale")]
    StaleRevision,
    #[error("input queue projection is stale; reload from the session writer")]
    StaleWriter,
    #[error("input queue belongs to another session")]
    WrongSession,
    #[error("input queue journal is invalid: {0}")]
    Recovery(String),
    #[error(transparent)]
    Session(#[from] crate::session::SessionError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Change {
    Enqueued {
        input: QueuedInput,
    },
    Edited {
        id: String,
        expected_revision: u64,
        text: String,
    },
    Cancelled {
        id: String,
    },
    Configured {
        kind: InputKind,
        mode: QueueMode,
    },
    Applied {
        model_turn_id: String,
        #[serde(default)]
        parent_id: Option<String>,
        ids: Vec<String>,
        kind: InputKind,
        would_stop: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueEvent {
    #[serde(rename = "type")]
    event_type: String,
    schema_version: u16,
    session_id: String,
    sequence: u64,
    change: Change,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum InputQueueReply {
    Input {
        input: QueuedInput,
        duplicate: bool,
    },
    Page {
        inputs: Vec<QueuedInput>,
        next_sequence: Option<u64>,
    },
    Modes {
        steer: QueueMode,
        follow_up: QueueMode,
    },
}

#[derive(Debug, Clone)]
pub struct InputQueue {
    session_id: String,
    limits: InputQueueLimits,
    inputs: BTreeMap<String, QueuedInput>,
    applied_turns: BTreeSet<String>,
    next_sequence: u64,
    steer_mode: QueueMode,
    follow_up_mode: QueueMode,
}

impl InputQueue {
    pub fn recover(
        session: &SessionStore,
        limits: InputQueueLimits,
    ) -> Result<Self, InputQueueError> {
        if limits.max_pending == 0
            || limits.max_records < limits.max_pending
            || limits.max_records > 4096
            || limits.max_retained_bytes == 0
            || limits.max_retained_bytes > 4 * 1024 * 1024
        {
            return Err(InputQueueError::Invalid(
                "limits outside bounded queue envelope".into(),
            ));
        }
        let mut queue = Self {
            session_id: session.summary().session_id,
            limits,
            inputs: BTreeMap::new(),
            applied_turns: BTreeSet::new(),
            next_sequence: 0,
            steer_mode: QueueMode::default(),
            follow_up_mode: QueueMode::default(),
        };
        for value in session
            .events()
            .iter()
            .filter(|v| v["type"] == "input_queue")
        {
            let event: QueueEvent = serde_json::from_value(value.clone())?;
            queue.replay(event)?;
        }
        Ok(queue)
    }

    pub fn get(&self, id: &str) -> Option<&QueuedInput> {
        self.inputs.get(id)
    }

    pub fn has_pending(&self) -> bool {
        self.inputs
            .values()
            .any(|i| i.status == InputStatus::Received)
    }

    pub fn modes(&self) -> (QueueMode, QueueMode) {
        (self.steer_mode, self.follow_up_mode)
    }

    /// Dispatch a transport request only into its explicitly addressed session.
    pub fn execute_request(
        &mut self,
        session: &mut SessionStore,
        request: crate::protocol::InputQueueRequest,
    ) -> Result<InputQueueReply, InputQueueError> {
        request
            .validate()
            .map_err(|e| InputQueueError::Invalid(e.to_string()))?;
        if request.session_id != self.session_id {
            return Err(InputQueueError::WrongSession);
        }
        self.execute(session, request.input_queue)
    }

    /// Reconstruct already committed input context for a provider turn after restart.
    /// Reading never changes status and never launches a provider or tool.
    pub fn inputs_for_turn(&self, model_turn_id: &str) -> Vec<QueuedInput> {
        self.ordered()
            .into_iter()
            .filter(|input| input.applied_turn_id.as_deref() == Some(model_turn_id))
            .cloned()
            .collect()
    }

    pub fn execute(
        &mut self,
        session: &mut SessionStore,
        action: InputQueueAction,
    ) -> Result<InputQueueReply, InputQueueError> {
        action
            .validate()
            .map_err(|error| InputQueueError::Invalid(error.to_string()))?;
        self.ensure_writer(session)?;
        let (change, id) = match action {
            InputQueueAction::Enqueue {
                input_id,
                kind,
                text,
            } => {
                if let Some(existing) = self.inputs.get(&input_id) {
                    if existing.kind != kind || existing.submitted_sha256 != text_hash(&text) {
                        return Err(InputQueueError::Conflict);
                    }
                    return Ok(InputQueueReply::Input {
                        input: existing.clone(),
                        duplicate: true,
                    });
                }
                let input = QueuedInput {
                    id: input_id.clone(),
                    kind,
                    submitted_sha256: text_hash(&text),
                    text,
                    sequence: self.next_sequence,
                    revision: 0,
                    status: InputStatus::Received,
                    applied_turn_id: None,
                    applied_parent_id: None,
                };
                (Change::Enqueued { input }, Some(input_id))
            }
            InputQueueAction::Edit {
                input_id,
                expected_revision,
                text,
            } => (
                Change::Edited {
                    id: input_id.clone(),
                    expected_revision,
                    text,
                },
                Some(input_id),
            ),
            InputQueueAction::Cancel { input_id } => {
                if let Some(input) = self.inputs.get(&input_id)
                    && input.status == InputStatus::Cancelled
                {
                    return Ok(InputQueueReply::Input {
                        input: input.clone(),
                        duplicate: true,
                    });
                }
                (
                    Change::Cancelled {
                        id: input_id.clone(),
                    },
                    Some(input_id),
                )
            }
            InputQueueAction::Configure { kind, mode } => (Change::Configured { kind, mode }, None),
            InputQueueAction::List {
                after_sequence,
                limit,
            } => {
                let mut bytes = 0;
                let mut inputs = Vec::new();
                let mut next_sequence = None;
                for input in self
                    .ordered()
                    .into_iter()
                    .filter(|i| after_sequence.is_none_or(|n| i.sequence > n))
                {
                    if inputs.len() == usize::from(limit)
                        || bytes + input.text.len() + 512 > 512 * 1024
                    {
                        next_sequence = inputs.last().map(|i: &QueuedInput| i.sequence);
                        break;
                    }
                    bytes += input.text.len() + 512;
                    inputs.push(input.clone());
                }
                return Ok(InputQueueReply::Page {
                    inputs,
                    next_sequence,
                });
            }
        };
        self.persist(session, change)?;
        Ok(match id {
            Some(id) => InputQueueReply::Input {
                input: self.inputs[&id].clone(),
                duplicate: false,
            },
            None => InputQueueReply::Modes {
                steer: self.steer_mode,
                follow_up: self.follow_up_mode,
            },
        })
    }

    /// Consume once for a distinct next-model-turn boundary. A previous empty
    /// poll may be retried after preparation; a nonempty poll freezes the batch
    /// even when more input arrives during preparation. Follow-ups are eligible
    /// only at WouldStop and steering always takes priority there.
    pub fn apply_boundary(
        &mut self,
        session: &mut SessionStore,
        boundary: &InputBoundary,
    ) -> Result<Vec<QueuedInput>, InputQueueError> {
        self.ensure_writer(session)?;
        if !boundary.is_current() {
            return Err(InputQueueError::Invalid(
                "stale/cancelled input boundary".into(),
            ));
        }
        if self.applied_turns.contains(boundary.model_turn_id()) {
            return Ok(Vec::new());
        }
        let has_steer = self
            .inputs
            .values()
            .any(|i| i.status == InputStatus::Received && i.kind == InputKind::Steer);
        let kind = if has_steer {
            InputKind::Steer
        } else if boundary.is_would_stop() {
            InputKind::FollowUp
        } else {
            return Ok(Vec::new());
        };
        let mode = if kind == InputKind::Steer {
            self.steer_mode
        } else {
            self.follow_up_mode
        };
        let ids: Vec<String> = self
            .ordered()
            .into_iter()
            .filter(|i| i.kind == kind && i.status == InputStatus::Received)
            .take(if mode == QueueMode::All {
                self.limits.max_pending
            } else {
                1
            })
            .map(|i| i.id.clone())
            .collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.persist(
            session,
            Change::Applied {
                model_turn_id: boundary.model_turn_id().into(),
                parent_id: boundary.context_parent().map(str::to_owned),
                ids: ids.clone(),
                kind,
                would_stop: boundary.is_would_stop(),
            },
        )?;
        Ok(ids.iter().map(|id| self.inputs[id].clone()).collect())
    }

    /// Repair the idempotent conversational projection of the queue commit.
    /// Must complete before admitting another user turn or calling a provider.
    /// Helper-only historical events without a parent remain queue-only.
    pub fn repair_projections(&self, session: &mut SessionStore) -> Result<(), InputQueueError> {
        self.ensure_writer(session)?;
        for input in self.ordered() {
            let Some(parent) = &input.applied_parent_id else {
                continue;
            };
            let Some(parent_turn) = session.turns().iter().find(|t| &t.id == parent) else {
                return Err(InputQueueError::Recovery(
                    "applied input parent is missing".into(),
                ));
            };
            if parent_turn.role != crate::core::TurnRole::User {
                return Err(InputQueueError::Recovery(
                    "applied input parent is not a user turn".into(),
                ));
            }
            // Length-delimited serialization avoids concatenation ambiguity. The
            // reserved namespace plus full digest cannot alias a caller's input ID.
            let key = serde_json::to_vec(&(self.session_id.as_str(), input.id.as_str()))?;
            let mut turn = crate::core::Turn::with_parent(
                format!("queued-input-{:x}", Sha256::digest(key)),
                parent,
                crate::core::TurnRole::User,
                &input.text,
            );
            // Stable across recovery; the parent timestamp is already durable.
            turn.created_at_ms = parent_turn.created_at_ms;
            turn.metadata = Some(serde_json::json!({"input_queue": {
                "input_id": input.id, "sequence": input.sequence,
                "revision": input.revision, "model_turn_id": input.applied_turn_id,
                "kind": input.kind,
            }}));
            // append_turn checks *all* content on a duplicate, including role,
            // parent and provenance, so a forged/user-owned ID fails closed.
            session.append_turn(turn)?;
        }
        Ok(())
    }

    fn ordered(&self) -> Vec<&QueuedInput> {
        let mut inputs: Vec<_> = self.inputs.values().collect();
        inputs.sort_by_key(|input| input.sequence);
        inputs
    }

    fn ensure_writer(&self, session: &SessionStore) -> Result<(), InputQueueError> {
        if session.summary().session_id != self.session_id {
            return Err(InputQueueError::WrongSession);
        }
        let latest = session
            .events()
            .iter()
            .rev()
            .find(|v| v["type"] == "input_queue");
        let next = match latest {
            Some(value) => value["sequence"]
                .as_u64()
                .and_then(|n| n.checked_add(1))
                .ok_or_else(|| InputQueueError::Recovery("invalid last queue sequence".into()))?,
            None => 0,
        };
        if next != self.next_sequence {
            return Err(InputQueueError::StaleWriter);
        }
        Ok(())
    }

    fn persist(
        &mut self,
        session: &mut SessionStore,
        change: Change,
    ) -> Result<(), InputQueueError> {
        let event = QueueEvent {
            event_type: "input_queue".into(),
            schema_version: 1,
            session_id: self.session_id.clone(),
            sequence: self.next_sequence,
            change,
        };
        // Bounded candidate projection keeps invalid transitions and failed
        // fsync/append from altering the visible state. SessionStore is the sole writer.
        let mut next = self.clone();
        next.replay(event.clone())?;
        session.append_event(serde_json::to_value(event)?)?;
        *self = next;
        Ok(())
    }

    fn replay(&mut self, event: QueueEvent) -> Result<(), InputQueueError> {
        if event.event_type != "input_queue"
            || event.schema_version != 1
            || event.session_id != self.session_id
            || event.sequence != self.next_sequence
        {
            return Err(InputQueueError::Recovery(
                "schema/session/sequence mismatch".into(),
            ));
        }
        let next = self
            .next_sequence
            .checked_add(1)
            .ok_or(InputQueueError::Capacity)?;
        match event.change {
            Change::Enqueued { input } => {
                InputQueueAction::Enqueue {
                    input_id: input.id.clone(),
                    kind: input.kind,
                    text: input.text.clone(),
                }
                .validate()
                .map_err(|e| InputQueueError::Recovery(e.to_string()))?;
                if self.inputs.contains_key(&input.id) {
                    return Err(InputQueueError::Conflict);
                }
                if input.sequence != self.next_sequence
                    || input.revision != 0
                    || input.status != InputStatus::Received
                    || input.applied_turn_id.is_some()
                    || input.applied_parent_id.is_some()
                    || input.submitted_sha256 != text_hash(&input.text)
                {
                    return Err(InputQueueError::Recovery("invalid received input".into()));
                }
                if self.inputs.len() >= self.limits.max_records
                    || self
                        .inputs
                        .values()
                        .filter(|i| i.status == InputStatus::Received)
                        .count()
                        >= self.limits.max_pending
                {
                    return Err(InputQueueError::Capacity);
                }
                self.inputs.insert(input.id.clone(), input);
            }
            Change::Edited {
                id,
                expected_revision,
                text,
            } => {
                InputQueueAction::Edit {
                    input_id: id.clone(),
                    expected_revision,
                    text: text.clone(),
                }
                .validate()
                .map_err(|e| InputQueueError::Recovery(e.to_string()))?;
                let input = self.inputs.get_mut(&id).ok_or(InputQueueError::NotFound)?;
                if input.status != InputStatus::Received {
                    return Err(InputQueueError::Terminal);
                }
                if input.revision != expected_revision {
                    return Err(InputQueueError::StaleRevision);
                }
                input.revision = input
                    .revision
                    .checked_add(1)
                    .ok_or(InputQueueError::Capacity)?;
                input.text = text;
            }
            Change::Cancelled { id } => {
                let input = self.inputs.get_mut(&id).ok_or(InputQueueError::NotFound)?;
                if input.status != InputStatus::Received {
                    return Err(InputQueueError::Terminal);
                }
                input.status = InputStatus::Cancelled;
            }
            Change::Configured { kind, mode } => {
                if kind == InputKind::Steer {
                    self.steer_mode = mode;
                } else {
                    self.follow_up_mode = mode;
                }
            }
            Change::Applied {
                model_turn_id,
                parent_id,
                ids,
                kind,
                would_stop,
            } => {
                if let Some(parent) = &parent_id {
                    crate::protocol::validate_queue_id(parent)
                        .map_err(|e| InputQueueError::Recovery(e.to_string()))?;
                }
                if kind == InputKind::FollowUp
                    && (!would_stop
                        || self.inputs.values().any(|i| {
                            i.kind == InputKind::Steer && i.status == InputStatus::Received
                        }))
                {
                    return Err(InputQueueError::Recovery(
                        "follow-up applied outside stop boundary or before steering".into(),
                    ));
                }
                crate::protocol::validate_queue_id(&model_turn_id)
                    .map_err(|e| InputQueueError::Recovery(e.to_string()))?;
                if ids.is_empty() || !self.applied_turns.insert(model_turn_id.clone()) {
                    return Err(InputQueueError::Recovery(
                        "empty/repeated applied model turn".into(),
                    ));
                }
                let mode = if kind == InputKind::Steer {
                    self.steer_mode
                } else {
                    self.follow_up_mode
                };
                let expected: Vec<_> = self
                    .ordered()
                    .into_iter()
                    .filter(|i| i.kind == kind && i.status == InputStatus::Received)
                    .take(if mode == QueueMode::All {
                        self.limits.max_pending
                    } else {
                        1
                    })
                    .map(|i| i.id.clone())
                    .collect();
                if ids != expected {
                    return Err(InputQueueError::Recovery(
                        "applied IDs violate lane/order/mode".into(),
                    ));
                }
                for id in ids {
                    let input = self.inputs.get_mut(&id).ok_or(InputQueueError::NotFound)?;
                    input.status = InputStatus::Applied;
                    input.applied_turn_id = Some(model_turn_id.clone());
                    input.applied_parent_id = parent_id.clone();
                }
            }
        }
        if self
            .inputs
            .values()
            .map(|i| i.text.len() + i.id.len())
            .sum::<usize>()
            > self.limits.max_retained_bytes
        {
            return Err(InputQueueError::Capacity);
        }
        self.next_sequence = next;
        Ok(())
    }
}

fn text_hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// Bounded cross-thread mailbox. Only the existing Agent owner services it;
/// this handle neither writes a journal nor creates another runtime job.
#[derive(Debug, Clone, Default)]
pub struct InputPort(std::sync::Arc<std::sync::Mutex<PortState>>);
#[derive(Debug, Default)]
struct PortState {
    session_id: String,
    preparing: bool,
    turn_id: Option<String>,
    pending: std::collections::VecDeque<PendingInput>,
}
#[derive(Debug)]
struct PendingInput {
    request: crate::protocol::InputQueueRequest,
    turn_id: String,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    reply: std::sync::mpsc::SyncSender<Result<InputQueueReply, String>>,
}
#[derive(Debug)]
pub struct InputTicket {
    receiver: std::sync::mpsc::Receiver<Result<InputQueueReply, String>>,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl InputTicket {
    pub fn try_recv(
        &self,
    ) -> Result<Result<InputQueueReply, String>, std::sync::mpsc::TryRecvError> {
        self.receiver.try_recv()
    }
    /// Cancels a not-yet-serviced ticket, never the model/tool job. Once receipt
    /// is durable use the explicit Cancel input action instead.
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
    }
}
impl Drop for InputTicket {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl InputPort {
    pub fn new(session_id: &str) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(PortState {
            session_id: session_id.into(),
            ..Default::default()
        })))
    }
    /// Observable preparation state for host progress; it grants no boundary.
    pub fn is_preparing(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).preparing
    }
    pub(crate) fn preparation(&self) -> InputPreparation {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).preparing = true;
        InputPreparation(self.clone())
    }
    pub fn session_id(&self) -> String {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .session_id
            .clone()
    }
    pub fn submit(
        &self,
        request: crate::protocol::InputQueueRequest,
    ) -> Result<InputTicket, String> {
        request.validate().map_err(|e| e.to_string())?;
        let mut state = self.0.lock().map_err(|_| "input port poisoned")?;
        let turn_id = state
            .turn_id
            .clone()
            .ok_or("input owner is idle; execute through the owner")?;
        if state.pending.len() >= 32 {
            return Err("input ticket capacity exceeded".into());
        }
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        state.pending.push_back(PendingInput {
            request,
            turn_id,
            cancelled: cancelled.clone(),
            reply,
        });
        Ok(InputTicket {
            receiver,
            cancelled,
        })
    }
    pub(crate) fn set_scope(&self, turn_id: Option<&str>) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.turn_id.as_deref() == turn_id {
            return;
        }
        for pending in state.pending.drain(..) {
            let _ = pending.reply.send(Err("input turn scope expired".into()));
        }
        state.turn_id = turn_id.map(str::to_owned);
        state.preparing = false;
    }
    pub(crate) fn service(
        &self,
        session: &mut SessionStore,
        turn_id: &str,
        cancelled: bool,
    ) -> Result<(), InputQueueError> {
        let pending = {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::take(&mut state.pending)
        };
        if pending.is_empty() {
            return Ok(());
        }
        let mut queue = match InputQueue::recover(session, InputQueueLimits::default()) {
            Ok(queue) => queue,
            Err(error) => {
                for item in pending {
                    let _ = item.reply.send(Err(error.to_string()));
                }
                return Err(error);
            }
        };
        for item in pending {
            if cancelled
                || item.turn_id != turn_id
                || item.cancelled.load(std::sync::atomic::Ordering::Acquire)
            {
                let _ = item
                    .reply
                    .send(Err("input ticket cancelled or turn scope expired".into()));
                continue;
            }
            let result = queue.execute_request(session, item.request);
            let fatal = matches!(
                &result,
                Err(InputQueueError::Session(_)
                    | InputQueueError::Recovery(_)
                    | InputQueueError::StaleWriter)
            );
            let _ = item.reply.send(
                result
                    .as_ref()
                    .map(Clone::clone)
                    .map_err(ToString::to_string),
            );
            if fatal {
                result?;
            }
        }
        Ok(())
    }
}

/// Services tickets on the existing owner during cancellation polling. Split
/// borrowing lets a provider/tool run while SessionStore remains single-writer.
pub(crate) struct InputPump<'a> {
    session: std::cell::RefCell<&'a mut SessionStore>,
    port: InputPort,
    turn_id: &'a str,
    failure: std::cell::RefCell<Option<String>>,
}
impl<'a> InputPump<'a> {
    pub fn new(session: &'a mut SessionStore, port: InputPort, turn_id: &'a str) -> Self {
        Self {
            session: std::cell::RefCell::new(session),
            port,
            turn_id,
            failure: Default::default(),
        }
    }
    pub fn poll(&self, cancelled: bool) -> bool {
        if self.failure.borrow().is_some() {
            return true;
        }
        if let Err(error) =
            self.port
                .service(&mut self.session.borrow_mut(), self.turn_id, cancelled)
        {
            *self.failure.borrow_mut() = Some(error.to_string());
            return true;
        }
        cancelled
    }
    pub fn finish(self) -> Result<(), crate::core::AgentError> {
        match self.failure.into_inner() {
            Some(error) => Err(crate::core::AgentError::Recovery(error)),
            None => Ok(()),
        }
    }
}

pub(crate) struct InputPreparation(InputPort);
impl Drop for InputPreparation {
    fn drop(&mut self) {
        self.0.0.lock().unwrap_or_else(|e| e.into_inner()).preparing = false;
    }
}
