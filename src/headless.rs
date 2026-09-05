//! Strict stdin/stdout JSONL mode.
//!
//! Only this module owns wire I/O.  The core remains synchronous and
//! testable, while composed agents can connect with a pipe and receive one
//! correlated response per input line.

use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File},
    io::{self, BufRead, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    b3::{HandoffRecord, unix_ms_to_rfc3339},
    backend::ProviderEvent,
    core::{Agent, AgentError, AgentEvent, ProcessResult, TurnInputRequest},
    domain_store::{self, DomainStore},
    protocol::{Command, StdioEvent, StdioRequest, StdioResponse, encode_line, parse_line},
    session::{ReconnectJournal, SessionStore, SessionSummary, unix_time_ms},
};

const MAX_REPLAY_EVENTS: usize = 4096;
/// Keep terminal response replay bounded even when a long-lived client uses a
/// unique request ID for every turn.  Event replay and terminal replay have
/// separate budgets because terminal lines can contain a complete answer.
const MAX_REPLAY_TERMINALS: usize = 256;
const MAX_REPLAY_TERMINAL_BYTES: usize = 16 * 1024 * 1024;
/// Steers can arrive faster than the active worker can be cancelled and
/// reissued.  Refuse excess intents instead of retaining an unbounded FIFO.
const MAX_PENDING_STEERS: usize = 128;
/// A provider can produce progress faster than a composed client can drain
/// stdout. Keep each request mailbox bounded and report loss explicitly
/// rather than allowing a long-lived process to grow without limit.
const MAX_ASYNC_PROVIDER_EVENTS: usize = 4096;
const MAX_ASYNC_AGENT_EVENTS: usize = 4096;
/// Per-request aggregate limits for events waiting behind a slow stdout
/// consumer. Counts alone are insufficient because one provider delta or
/// agent error can contain substantially more data than thousands of normal
/// lifecycle markers.
const MAX_ASYNC_PROVIDER_BYTES: usize = 1024 * 1024;
const MAX_ASYNC_AGENT_BYTES: usize = 1024 * 1024;
/// An explicit shutdown is a cancellation request and gets only a short grace
/// period before the runner is stopped. Plain stdin EOF is different: it is
/// the normal completion signal for one-shot pipelines, so admitted work must
/// drain instead of being relabelled as cancelled.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(250);
/// Explicit paths accepted by host-side inspection commands are workspace
/// relative. Keep the same byte bound as the protocol path field even when a
/// caller reaches these helpers directly.
const MAX_WORKSPACE_PATH_BYTES: usize = 4096;
/// A domain listing is a terminal response, not a file transfer. Keep it well
/// below the replay/cache budget so one `/blueprint show` can always be
/// replayed by request ID.
const MAX_DOMAIN_VIEW_BYTES: usize = 512 * 1024;
/// Keep slash-session recovery useful without turning one response into a
/// transcript dump.  The cursor in the response lets callers request the
/// following page with another `/resume <sequence>` command.
const MAX_SLASH_RESUME_RECORDS: usize = 128;
const MAX_SLASH_RESUME_BYTES: usize = 128 * 1024;
const MAX_SLASH_RESUME_EVENT_BYTES: usize = 8 * 1024;
/// A GC response is an audit receipt, not a directory listing. Keep the
/// serialized owner result comfortably below the replay-cache line budget.
const MAX_SESSION_GC_RECEIPT_BYTES: usize = 64 * 1024;
/// Learn evidence is a reference plus a small integrity receipt, not a file
/// transfer. Hashing is bounded so a command cannot turn the owner into an
/// unbounded workspace reader.
const MAX_LEARN_EVIDENCE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
struct TerminalRecord {
    fingerprint: String,
    line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestIdAdmission {
    New,
    Replay,
    InFlightSame,
    Conflict,
    UnknownOutcome,
    ReplayExpired,
    LedgerFull,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ReconnectEntry {
    Owner {
        epoch: u64,
    },
    Reserved {
        id: String,
        fingerprint: String,
    },
    Released {
        id: String,
    },
    Terminal {
        id: String,
        fingerprint: String,
        line: String,
    },
    Event {
        sequence: u64,
        line: String,
    },
    Acknowledged {
        next_sequence: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReplayReport {
    first_available: Option<u64>,
    last_available: Option<u64>,
    replayed: usize,
    gap: bool,
}

#[derive(Default)]
struct ReplayState {
    events: std::collections::VecDeque<(u64, String)>,
    terminals: HashMap<String, TerminalRecord>,
    terminal_order: VecDeque<String>,
    terminal_bytes: usize,
    event_bytes: usize,
    /// Fingerprints reserved by admitted requests that have not reached a
    /// terminal response yet. Keeping this separate from `terminals` makes
    /// in-flight retries deterministic and side-effect free.
    in_flight: HashMap<String, String>,
    /// The first sequence no longer retained after FIFO eviction.
    replay_floor: Option<u64>,
    journal: Option<ReconnectJournal>,
    completed: HashMap<String, String>,
    unknown: HashMap<String, String>,
    owner_epoch: u64,
    next_sequence: u64,
    acknowledged: u64,
}

impl ReplayState {
    fn open(session: &SessionStore) -> Result<Self, HeadlessError> {
        let (journal, entries) = ReconnectJournal::open(session)?;
        let mut state = Self::default();
        for entry in entries {
            match serde_json::from_value::<ReconnectEntry>(entry)? {
                ReconnectEntry::Owner { epoch } => state.owner_epoch = epoch,
                ReconnectEntry::Reserved { id, fingerprint } => {
                    state.in_flight.insert(id, fingerprint);
                }
                ReconnectEntry::Released { id } => {
                    state.in_flight.remove(&id);
                }
                ReconnectEntry::Terminal {
                    id,
                    fingerprint,
                    line,
                } => {
                    state.in_flight.insert(id.clone(), fingerprint);
                    state.cache_terminal(id, line);
                }
                ReconnectEntry::Event { sequence, line } => state.cache_event(sequence, line),
                ReconnectEntry::Acknowledged { next_sequence } => {
                    state.acknowledged = next_sequence
                }
            }
        }
        state.unknown = std::mem::take(&mut state.in_flight);
        state.owner_epoch = state
            .owner_epoch
            .checked_add(1)
            .ok_or_else(|| io::Error::other("reconnect owner epoch exhausted"))?;
        state.journal = Some(journal);
        state.persist(ReconnectEntry::Owner {
            epoch: state.owner_epoch,
        })?;
        Ok(state)
    }

    fn persist(&mut self, entry: ReconnectEntry) -> Result<(), HeadlessError> {
        if let Some(journal) = &mut self.journal {
            journal.append(serde_json::to_value(entry)?)?;
        }
        Ok(())
    }

    fn remember_event(&mut self, sequence: u64, line: String) -> Result<(), HeadlessError> {
        self.persist(ReconnectEntry::Event {
            sequence,
            line: line.clone(),
        })?;
        self.cache_event(sequence, line);
        Ok(())
    }

    fn cache_event(&mut self, sequence: u64, line: String) {
        const MAX_REPLAY_BYTES: usize = 16 * 1024 * 1024;
        self.next_sequence = self.next_sequence.max(sequence.saturating_add(1));
        self.event_bytes = self.event_bytes.saturating_add(line.len());
        self.events.push_back((sequence, line));
        while self.events.len() > MAX_REPLAY_EVENTS || self.event_bytes > MAX_REPLAY_BYTES {
            if let Some((evicted_sequence, evicted)) = self.events.pop_front() {
                self.event_bytes = self.event_bytes.saturating_sub(evicted.len());
                let floor = evicted_sequence.saturating_add(1);
                self.replay_floor = Some(
                    self.replay_floor
                        .map_or(floor, |current| current.max(floor)),
                );
            } else {
                break;
            }
        }
    }

    fn replay_from<W: Write>(
        &self,
        from_sequence: u64,
        output: &mut W,
    ) -> io::Result<ReplayReport> {
        let first_available = self.events.front().map(|(sequence, _)| *sequence);
        let last_available = self.events.back().map(|(sequence, _)| *sequence);
        let mut gap = self.replay_floor.is_some_and(|floor| from_sequence < floor)
            || from_sequence > self.next_sequence;
        let mut replayed = 0;
        let mut expected = from_sequence;
        for (sequence, line) in self
            .events
            .iter()
            .filter(|(sequence, _)| *sequence >= from_sequence)
        {
            gap |= *sequence != expected;
            expected = sequence.saturating_add(1);
            output.write_all(line.as_bytes())?;
            replayed += 1;
        }
        output.flush()?;
        Ok(ReplayReport {
            first_available,
            last_available,
            replayed,
            gap,
        })
    }

    fn admit(
        &mut self,
        request_id: Option<&str>,
        fingerprint: &str,
    ) -> Result<RequestIdAdmission, HeadlessError> {
        let Some(request_id) = request_id else {
            return Ok(RequestIdAdmission::New);
        };
        if let Some(record) = self.terminals.get(request_id) {
            return Ok(if record.fingerprint == fingerprint {
                RequestIdAdmission::Replay
            } else {
                RequestIdAdmission::Conflict
            });
        }
        if let Some(existing) = self.unknown.get(request_id) {
            return Ok(if existing == fingerprint {
                RequestIdAdmission::UnknownOutcome
            } else {
                RequestIdAdmission::Conflict
            });
        }
        if let Some(existing) = self.completed.get(request_id) {
            return Ok(if existing == fingerprint {
                RequestIdAdmission::ReplayExpired
            } else {
                RequestIdAdmission::Conflict
            });
        }
        if let Some(existing) = self.in_flight.get(request_id) {
            // A request that is still running has no terminal record to
            // compare against.  Treat every reuse as an in-flight duplicate
            // and keep the original reservation intact; once it completes,
            // the terminal fingerprint enforces payload conflict semantics.
            let _payload_matches = existing == fingerprint;
            return Ok(RequestIdAdmission::InFlightSame);
        }
        if self.completed.len() + self.unknown.len() + self.in_flight.len() >= 16_384 {
            return Ok(RequestIdAdmission::LedgerFull);
        }
        self.persist(ReconnectEntry::Reserved {
            id: request_id.into(),
            fingerprint: fingerprint.into(),
        })?;
        self.in_flight
            .insert(request_id.to_owned(), fingerprint.to_owned());
        Ok(RequestIdAdmission::New)
    }

    fn cached_line(&self, request_id: &str) -> Option<&str> {
        self.terminals
            .get(request_id)
            .map(|record| record.line.as_str())
    }

    fn release(&mut self, request_id: Option<&str>) -> Result<(), HeadlessError> {
        if let Some(request_id) = request_id {
            self.persist(ReconnectEntry::Released {
                id: request_id.into(),
            })?;
            self.in_flight.remove(request_id);
        }
        Ok(())
    }

    /// Isolate replay and request-ID state when a host switches to a
    /// different session journal. Sequence numbers remain process-global so
    /// reconnecting clients can still detect that the old prefix is gone.
    /// The request currently performing the switch stays reserved until its
    /// new-session terminal response is cached.
    fn reset_for_session(
        &mut self,
        session: &SessionStore,
        request_id: Option<&str>,
        sequence_floor: u64,
    ) -> Result<(), HeadlessError> {
        if self.journal.as_ref().is_some_and(|journal| {
            fs::canonicalize(session.path()).ok().as_ref() == Some(&journal.session_path)
        }) {
            return Ok(());
        }
        let current = request_id.and_then(|id| {
            self.in_flight
                .get(id)
                .cloned()
                .map(|fingerprint| (id.to_owned(), fingerprint))
        });
        let mut next = Self::open(session)?;
        if let Some((id, fingerprint)) = current {
            match next.admit(Some(&id), &fingerprint)? {
                RequestIdAdmission::New => {}
                _ => {
                    return Err(io::Error::other(
                        "session switch request ID conflicts with target journal",
                    )
                    .into());
                }
            }
        }
        next.replay_floor = Some(next.replay_floor.unwrap_or(0).max(sequence_floor));
        *self = next;
        Ok(())
    }

    fn remember_terminal(&mut self, request_id: String, line: String) -> Result<(), HeadlessError> {
        let fingerprint = self
            .in_flight
            .get(&request_id)
            .cloned()
            .or_else(|| self.completed.get(&request_id).cloned())
            .unwrap_or_default();
        self.persist(ReconnectEntry::Terminal {
            id: request_id.clone(),
            fingerprint,
            line: line.clone(),
        })?;
        self.cache_terminal(request_id, line);
        Ok(())
    }

    fn cache_terminal(&mut self, request_id: String, line: String) {
        // A retry of an existing ID replaces its cached line and refreshes
        // its position.  Keeping an explicit FIFO avoids relying on the
        // intentionally unordered `HashMap` iteration order when evicting.
        let fingerprint = self
            .in_flight
            .remove(&request_id)
            .or_else(|| {
                self.terminals
                    .get(&request_id)
                    .map(|record| record.fingerprint.clone())
            })
            .unwrap_or_default();
        if let Some(previous) = self.terminals.remove(&request_id) {
            self.terminal_bytes = self.terminal_bytes.saturating_sub(previous.line.len());
            self.terminal_order.retain(|id| id != &request_id);
        }
        self.terminal_bytes = self.terminal_bytes.saturating_add(line.len());
        self.completed
            .insert(request_id.clone(), fingerprint.clone());
        self.terminals
            .insert(request_id.clone(), TerminalRecord { fingerprint, line });
        self.terminal_order.push_back(request_id);
        while self.terminal_order.len() > MAX_REPLAY_TERMINALS
            || self.terminal_bytes > MAX_REPLAY_TERMINAL_BYTES
        {
            let Some(evicted) = self.terminal_order.pop_front() else {
                break;
            };
            if let Some(record) = self.terminals.remove(&evicted) {
                self.terminal_bytes = self.terminal_bytes.saturating_sub(record.line.len());
            }
        }
    }
}

/// Hash the normalized wire request, excluding the correlation ID itself.
/// IDs are map keys; payload changes under a reused ID must never be treated
/// as an idempotent retry. Alias spellings are normalized because they have
/// identical dispatch semantics.
fn request_fingerprint(request: &StdioRequest) -> Result<String, HeadlessError> {
    let mut normalized = request.clone();
    normalized.id = None;
    if normalized.kind == "slash" {
        normalized.kind = "command".to_owned();
    }
    if normalized.text.is_none() {
        normalized.text = normalized.message.clone();
    }
    normalized.message = None;
    let bytes = serde_json::to_vec(&normalized)?;
    let mut digest = Sha256::new();
    digest.update(b"zenpi-headless-request-v1\0");
    digest.update(bytes);
    Ok(format!("{:x}", digest.finalize()))
}

#[derive(Debug, Error)]
pub enum HeadlessError {
    #[error("headless I/O: {0}")]
    Io(#[from] io::Error),
    #[error("headless encoding: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("headless agent: {0}")]
    Agent(#[from] AgentError),
    #[error("headless persistence: {0}")]
    Session(#[from] crate::session::SessionError),
}

/// Run the headless loop over arbitrary buffered input/output.  This generic
/// form is used by integration tests and embedders; `run_stdio` supplies the
/// process stdin/stdout handles.
pub fn run_headless<R: BufRead, W: Write>(
    agent: &mut Agent,
    mut input: R,
    mut output: W,
) -> Result<(), HeadlessError> {
    let mut replay = ReplayState::open(agent.session())?;
    let mut event_sequence = replay.next_sequence;
    // Keep at most one bounded frame in memory. `BufRead::read_line` grows its
    // destination until LF, which lets an untrusted peer force an arbitrarily
    // large allocation before the protocol limit is checked. The chunked
    // reader below consumes an overlong frame through the caller's buffer but
    // retains no more than `MAX_LINE_BYTES + 1` bytes.
    let mut frame = Vec::with_capacity(crate::protocol::MAX_LINE_BYTES.min(8 * 1024));
    loop {
        match read_frame(&mut input, &mut frame)? {
            Frame::Eof => {
                agent.close();
                return Ok(());
            }
            Frame::TooLong => {
                write_response(
                    &mut output,
                    StdioResponse::error_with_code(
                        None,
                        "invalid",
                        "line_too_long",
                        "input frame exceeds maximum size",
                    ),
                )?;
                continue;
            }
            Frame::Data => {}
        }
        let frame_text = match std::str::from_utf8(&frame) {
            Ok(frame) => frame,
            Err(_) => {
                write_response(
                    &mut output,
                    StdioResponse::error_with_code(
                        None,
                        "invalid",
                        "invalid_utf8",
                        "input frame is not valid UTF-8",
                    ),
                )?;
                continue;
            }
        };
        // The reader excludes LF; parse_line intentionally accepts an
        // optional CR for CRLF clients.
        if frame_text.trim().is_empty() {
            continue;
        }
        let request = match parse_line(frame_text) {
            Ok(request) => request,
            Err(error) => {
                write_response(
                    &mut output,
                    StdioResponse::error_with_code(
                        None,
                        "invalid",
                        error.code(),
                        error.to_string(),
                    ),
                )?;
                continue;
            }
        };
        let id = request.id().map(str::to_owned);
        let command_kind = request.kind.clone();
        let request_version = request.schema_version;
        let fingerprint = request_fingerprint(&request)?;
        // Validate a clone before consulting the replay cache. This prevents
        // a reused ID with an unsupported version or malformed payload from
        // bypassing protocol validation by replaying an old response.
        let parsed_command = request.clone().into_command();
        match replay.admit(id.as_deref(), &fingerprint)? {
            RequestIdAdmission::Replay => {
                if let Some(request_id) = id.as_deref()
                    && let Some(cached) = replay.cached_line(request_id)
                {
                    output.write_all(cached.as_bytes())?;
                    output.flush()?;
                    continue;
                }
            }
            RequestIdAdmission::Conflict => {
                write_response(
                    &mut output,
                    StdioResponse::error_with_code(
                        id.clone(),
                        command_kind,
                        "request_id_conflict",
                        "request ID was already used for a different request",
                    )
                    .for_version(request_version),
                )?;
                continue;
            }
            RequestIdAdmission::InFlightSame => {
                // The synchronous host never has a concurrent operation, but
                // retaining the explicit branch makes the invariant obvious
                // if an embedding caller re-enters the dispatcher.
                write_response(
                    &mut output,
                    StdioResponse::error_with_code(
                        id,
                        command_kind,
                        "duplicate_request_in_flight",
                        "request ID is already in flight",
                    )
                    .for_version(request_version),
                )?;
                continue;
            }
            RequestIdAdmission::New => {}
            admission => {
                write_response(
                    &mut output,
                    admission_error(id, command_kind, admission).for_version(request_version),
                )?;
                continue;
            }
        }
        let command = match parsed_command {
            Ok(command) => command,
            Err(error) => {
                write_cached_response(
                    &mut output,
                    StdioResponse::error_with_code(
                        id,
                        command_kind,
                        error.code(),
                        error.to_string(),
                    )
                    .for_version(request_version),
                    &mut replay,
                )?;
                continue;
            }
        };
        let should_stop = handle_command(
            agent,
            id,
            command,
            &mut output,
            &mut event_sequence,
            &mut replay,
            request_version,
        )?;
        if should_stop {
            return Ok(());
        }
    }
}

/// Result of consuming one LF-delimited frame. The frame bytes themselves are
/// retained in the caller-provided buffer so the loop can reuse its capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    Eof,
    Data,
    TooLong,
}

/// Consume exactly one frame without allocating in proportion to an
/// overlong line. A final unterminated frame at EOF is valid JSONL input and
/// is returned as `Data`; an empty input at EOF returns `Eof`.
fn read_frame<R: BufRead>(input: &mut R, frame: &mut Vec<u8>) -> io::Result<Frame> {
    frame.clear();
    let mut saw_bytes = false;
    let mut too_long = false;
    let limit = crate::protocol::MAX_LINE_BYTES;

    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            if !saw_bytes {
                return Ok(Frame::Eof);
            }
            return Ok(if too_long {
                Frame::TooLong
            } else {
                Frame::Data
            });
        }
        saw_bytes = true;

        let newline = chunk.iter().position(|byte| *byte == b'\n');
        let payload_len = newline.unwrap_or(chunk.len());
        if !too_long {
            // Retain one byte beyond the limit so the boundary check is
            // exact, then discard the rest while consuming the frame.
            let remaining = limit.saturating_add(1).saturating_sub(frame.len());
            let copy_len = payload_len.min(remaining);
            frame.extend_from_slice(&chunk[..copy_len]);
            if frame.len() > limit {
                too_long = true;
                frame.clear();
            }
        }

        let consumed = newline.map_or(chunk.len(), |index| index + 1);
        input.consume(consumed);
        if newline.is_some() {
            return Ok(if too_long {
                Frame::TooLong
            } else {
                Frame::Data
            });
        }
    }
}

/// Process stdin/stdout without printing any diagnostics to stdout.
pub fn run_stdio(agent: &mut Agent) -> Result<(), HeadlessError> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    run_headless(agent, stdin.lock(), stdout.lock())
}

/// Owned stdio entry point used by the binary. Input is read on a dedicated
/// thread while provider/tool work runs through the bounded runtime, so a
/// slow provider cannot stop headless clients from sending status or follow-up
/// commands. The borrowed `run_stdio` above remains the deterministic
/// synchronous embedding API.
pub fn run_stdio_owned(agent: Agent) -> Result<(), HeadlessError> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    run_async_stdio(agent, stdin, stdout)
}

/// Asynchronous transport over caller-owned streams. This is public so PTY
/// and reconnect tests can exercise the production scheduler without taking
/// process-global stdin/stdout.
pub fn run_async_streams<R: io::Read + Send + 'static, W: Write>(
    agent: Agent,
    input: R,
    output: W,
) -> Result<(), HeadlessError> {
    run_async_stdio(agent, input, output)
}

struct AsyncWork {
    id: Option<String>,
    command: &'static str,
    version: u16,
    events: Arc<Mutex<AsyncEventBuffer>>,
    turn_id: Arc<Mutex<Option<String>>>,
}

/// Runtime result shared by normal provider turns and explicit local shell
/// operations. Shell output is kept as JSON so the headless wire can expose
/// the complete supervised outcome without manufacturing a provider turn.
enum AsyncProcessResult {
    Turn(ProcessResult),
    UserShell(serde_json::Value),
}

/// Events produced by one background request are kept with that request.
/// The runtime may start a queued replacement before the host has consumed
/// the previous `Completed` event; storing only a global `Agent` event vector
/// would then correlate the replacement's lifecycle to the wrong request.
struct BufferedEvent<T> {
    event: T,
    serialized_bytes: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DroppedEvents {
    count: u64,
    bytes: u64,
}

impl DroppedEvents {
    fn record(&mut self, bytes: usize) {
        self.count = self.count.saturating_add(1);
        self.bytes = self
            .bytes
            .saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX));
    }

    fn is_empty(self) -> bool {
        self.count == 0
    }
}

#[derive(Default)]
struct AsyncEventBuffer {
    provider: Vec<BufferedEvent<ProviderEvent>>,
    agent: Vec<BufferedEvent<AgentEvent>>,
    provider_bytes: usize,
    agent_bytes: usize,
    provider_dropped: DroppedEvents,
    agent_dropped: DroppedEvents,
}

const EVENT_MAILBOX_POISONED: &str = "headless event mailbox mutex poisoned";

fn event_mailbox_headless_error() -> HeadlessError {
    HeadlessError::Io(io::Error::other(EVENT_MAILBOX_POISONED))
}

fn event_mailbox_agent_error() -> AgentError {
    AgentError::InvalidTurn(EVENT_MAILBOX_POISONED.into())
}

fn event_mailbox_backend_error() -> crate::backend::BackendError {
    crate::backend::BackendError::Transport(EVENT_MAILBOX_POISONED.into())
}

fn serialized_event_len<T: serde::Serialize>(event: &T) -> usize {
    #[derive(Default)]
    struct ByteCounter(usize);

    impl Write for ByteCounter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let mut counter = ByteCounter::default();
    // These two event enums and every nested value use derived serializers
    // that cannot fail with the counting writer. Treat an unexpected error as
    // an over-limit event rather than retaining an unaccounted value.
    serde_json::to_writer(&mut counter, event).map_or(usize::MAX, |()| counter.0)
}

fn is_admission_event(event: &AgentEvent) -> bool {
    matches!(
        event,
        AgentEvent::TurnAccepted { .. } | AgentEvent::TurnRejected { .. }
    )
}

fn buffered_admission_prefix_len(events: &[BufferedEvent<AgentEvent>]) -> Option<usize> {
    // A request normally publishes one marker, but a recovery/reissue path
    // may append more than one lifecycle event before the host polls. Drain
    // through the *last* marker so every admission remains ahead of provider
    // output even when ordinary agent events are interleaved.
    events
        .iter()
        .rposition(|event| is_admission_event(&event.event))
        .map(|last_admission| last_admission.saturating_add(1))
}

fn admission_prefix_len(events: &[AgentEvent]) -> Option<usize> {
    events
        .iter()
        .rposition(is_admission_event)
        .map(|last_admission| last_admission.saturating_add(1))
}

impl AsyncEventBuffer {
    fn push_provider(&mut self, event: ProviderEvent) {
        let serialized_bytes = serialized_event_len(&event);
        let next_bytes = self.provider_bytes.checked_add(serialized_bytes);
        if self.provider.len() < MAX_ASYNC_PROVIDER_EVENTS
            && next_bytes.is_some_and(|bytes| bytes <= MAX_ASYNC_PROVIDER_BYTES)
        {
            self.provider_bytes = next_bytes.unwrap_or(self.provider_bytes);
            self.provider.push(BufferedEvent {
                event,
                serialized_bytes,
            });
        } else {
            self.provider_dropped.record(serialized_bytes);
        }
    }

    fn push_agent(&mut self, event: AgentEvent) {
        let serialized_bytes = serialized_event_len(&event);
        if is_admission_event(&event) {
            // Admission is a correctness boundary, not best-effort progress.
            // If a flood has consumed the mailbox, evict ordinary lifecycle
            // events until this small, bounded marker fits. Never evict an
            // earlier TurnAccepted/TurnRejected marker.
            if serialized_bytes > MAX_ASYNC_AGENT_BYTES {
                self.agent_dropped.record(serialized_bytes);
                return;
            }
            loop {
                let fits_count = self.agent.len() < MAX_ASYNC_AGENT_EVENTS;
                let fits_bytes = self
                    .agent_bytes
                    .checked_add(serialized_bytes)
                    .is_some_and(|bytes| bytes <= MAX_ASYNC_AGENT_BYTES);
                if fits_count && fits_bytes {
                    self.agent_bytes = self
                        .agent_bytes
                        .checked_add(serialized_bytes)
                        .unwrap_or(self.agent_bytes);
                    self.agent.push(BufferedEvent {
                        event,
                        serialized_bytes,
                    });
                    return;
                }

                let Some(eviction_index) = self
                    .agent
                    .iter()
                    .position(|buffered| !is_admission_event(&buffered.event))
                else {
                    // Valid core admission markers are tiny and bounded. This
                    // branch is only reachable after a pathological sequence
                    // of markers; report the loss rather than violating the
                    // byte/count bound.
                    self.agent_dropped.record(serialized_bytes);
                    return;
                };
                let evicted = self.agent.remove(eviction_index);
                self.agent_bytes = self.agent_bytes.saturating_sub(evicted.serialized_bytes);
                self.agent_dropped.record(evicted.serialized_bytes);
            }
        }

        let next_bytes = self.agent_bytes.checked_add(serialized_bytes);
        if self.agent.len() < MAX_ASYNC_AGENT_EVENTS
            && next_bytes.is_some_and(|bytes| bytes <= MAX_ASYNC_AGENT_BYTES)
        {
            self.agent_bytes = next_bytes.unwrap_or(self.agent_bytes);
            self.agent.push(BufferedEvent {
                event,
                serialized_bytes,
            });
        } else {
            self.agent_dropped.record(serialized_bytes);
        }
    }

    fn take_provider(&mut self) -> (Vec<ProviderEvent>, DroppedEvents) {
        let events = std::mem::take(&mut self.provider)
            .into_iter()
            .map(|buffered| buffered.event)
            .collect();
        self.provider_bytes = 0;
        let dropped = std::mem::take(&mut self.provider_dropped);
        (events, dropped)
    }

    fn take_agent(&mut self) -> (Vec<AgentEvent>, DroppedEvents) {
        let events = std::mem::take(&mut self.agent)
            .into_iter()
            .map(|buffered| buffered.event)
            .collect();
        self.agent_bytes = 0;
        let dropped = std::mem::take(&mut self.agent_dropped);
        (events, dropped)
    }

    /// Admission markers are produced immediately after `Agent::submit`,
    /// before the provider starts. Keep them ahead of streamed provider
    /// events even when the host polls the request before it completes. Agent
    /// construction may also preload recovery errors, so include any prefix
    /// before the first admission marker rather than requiring the marker to
    /// be the first queued event. Drain through the last marker to cover
    /// interleaved recovery/reissue events; any later AgentEvents remain
    /// buffered until the provider trace is drained.
    fn take_admission(&mut self) -> Vec<AgentEvent> {
        let Some(admission_end) = buffered_admission_prefix_len(&self.agent) else {
            return Vec::new();
        };
        let drained = self.agent.drain(..admission_end).collect::<Vec<_>>();
        let drained_bytes = drained.iter().fold(0_usize, |total, buffered| {
            total.saturating_add(buffered.serialized_bytes)
        });
        self.agent_bytes = self.agent_bytes.saturating_sub(drained_bytes);
        drained.into_iter().map(|buffered| buffered.event).collect()
    }
}

/// A steer received while the active worker is still between runtime
/// admission and `Agent::submit`.  The runtime emits `Started` as soon as it
/// launches a job, but the agent turn ID is only known after the job acquires
/// the session lock.  Keeping the request here avoids converting that small
/// race window into a spurious `no_active_turn` response.
struct PendingSteer {
    id: Option<String>,
    version: u16,
    text: String,
    expected_turn_id: Option<String>,
    target_job: crate::runtime::JobId,
    cancel_sent: bool,
    superseded_turn_id: Option<String>,
}

fn enqueue_pending_steer<W: Write>(
    pending_steers: &mut VecDeque<PendingSteer>,
    pending: PendingSteer,
    output: &mut W,
    replay: &mut ReplayState,
) -> Result<(), HeadlessError> {
    if pending_steers.len() >= MAX_PENDING_STEERS {
        // This is a host-side admission failure, not a worker failure.  Keep
        // the JSONL session alive and let the caller retry with backpressure.
        write_runtime_rejection(
            output,
            pending.id,
            "steer",
            crate::runtime::SubmitError::QueueFull,
            pending.version,
            replay,
        )?;
    } else {
        pending_steers.push_back(pending);
    }
    Ok(())
}

/// Collect one bounded resource snapshot for a host command.  This helper is
/// intentionally provider-independent: a malformed or expensive workspace
/// walk must never acquire the agent lock or submit a model turn.
pub fn collect_resource_snapshot(
    path: Option<&str>,
) -> Result<crate::resources::ResourceSnapshot, crate::resources::ResourceError> {
    let root = match path {
        Some(path) => resolve_workspace_path(path)
            .map_err(|_| crate::resources::ResourceError::PathDenied(PathBuf::from(path)))?,
        None => std::env::current_dir()
            .map_err(crate::resources::ResourceError::Io)?
            .canonicalize()
            .map_err(crate::resources::ResourceError::Io)?,
    };
    crate::resources::ResourceCollector::new(root)?.collect()
}

/// Resolve an explicit host inspection path beneath the process workspace.
/// This is intentionally stricter than the public tool path resolver: absolute
/// paths, parent traversal, and symlink components are rejected before any
/// file is opened. The returned path is canonical so a later collector can
/// enforce the same boundary even if the caller supplied `.` components.
fn resolve_workspace_path(raw: &str) -> Result<PathBuf, String> {
    if raw.trim().is_empty()
        || raw.len() > MAX_WORKSPACE_PATH_BYTES
        || raw.chars().any(char::is_control)
    {
        return Err("workspace path is empty, too long, or contains control characters".into());
    }
    let relative = Path::new(raw);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("workspace path must be relative and stay inside the current workspace".into());
    }
    let workspace = std::env::current_dir()
        .map_err(|error| error.to_string())?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let candidate = workspace.join(relative);
    // Reject links in every existing component, including a final link. This
    // avoids silently accepting an alias that happens to point back inside the
    // workspace and makes the no-symlink policy deterministic.
    let mut cursor = workspace.clone();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            continue;
        };
        cursor.push(part);
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("workspace path contains a symbolic link".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.to_string()),
        }
    }
    let canonical = candidate
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !canonical.starts_with(&workspace) {
        return Err("workspace path escapes the current workspace".into());
    }
    Ok(canonical)
}

fn domain_store_for_agent(
    agent: Option<&Agent>,
    read_only: bool,
) -> Result<DomainStore, SlashDispatchError> {
    let Some(agent) = agent else {
        // A busy host must not fall back to the process-global default store:
        // that would both create a file as a read-only side effect and expose
        // records belonging to a different session.
        return Err(SlashDispatchError {
            code: "agent_busy",
            message: "domain store is temporarily unavailable while the agent is busy".into(),
        });
    };
    let path = domain_store::path_for_session(agent.session().path());
    let result = if read_only {
        DomainStore::open_read_only(path)
    } else {
        DomainStore::open(path)
    };
    result.map_err(|error| SlashDispatchError {
        code: "domain_store_error",
        message: error.to_string(),
    })
}

fn serialized_store_summary(store: &DomainStore) -> Result<serde_json::Value, SlashDispatchError> {
    let summary = store.summary().map_err(|error| SlashDispatchError {
        code: "domain_store_error",
        message: error.to_string(),
    })?;
    // Do not expose a user's absolute home/session path in a control-plane
    // response. The store digest and counts still identify the snapshot.
    let mut summary = serde_json::to_value(summary).map_err(|error| SlashDispatchError {
        code: "serialization_error",
        message: error.to_string(),
    })?;
    if let Some(fields) = summary.as_object_mut() {
        fields.insert(
            "path".into(),
            serde_json::Value::String("<domain-store>".into()),
        );
    }
    Ok(summary)
}

fn serialized_domain_store(store: &DomainStore) -> Result<serde_json::Value, SlashDispatchError> {
    let summary = serialized_store_summary(store)?;
    let mut result = json!({
        "store": summary,
        "blueprints": [],
        "goals": [],
        "learns": [],
        "truncated": false,
    });
    let mut used = serde_json::to_vec(&result)
        .map_err(|error| SlashDispatchError {
            code: "serialization_error",
            message: error.to_string(),
        })?
        .len();
    let mut truncated = false;
    append_bounded_domain_records(
        &mut result,
        "blueprints",
        store.blueprints(),
        &mut used,
        &mut truncated,
    )?;
    append_bounded_domain_records(
        &mut result,
        "goals",
        store.goals(),
        &mut used,
        &mut truncated,
    )?;
    append_bounded_domain_records(
        &mut result,
        "learns",
        store.learns(),
        &mut used,
        &mut truncated,
    )?;
    result["truncated"] = serde_json::Value::Bool(truncated);
    let encoded_len = serde_json::to_vec(&result)
        .map_err(|error| SlashDispatchError {
            code: "serialization_error",
            message: error.to_string(),
        })?
        .len();
    if encoded_len > MAX_DOMAIN_VIEW_BYTES {
        // This should only be reachable if JSON structural overhead changes;
        // retain a deterministic summary-only fallback rather than emitting a
        // response that cannot fit the host's bounded replay cache.
        return Ok(json!({
            "store": result["store"].clone(),
            "blueprints": [],
            "goals": [],
            "learns": [],
            "truncated": true,
        }));
    }
    Ok(result)
}

/// Convert a resource snapshot to a protocol-safe value. Workspace inventory
/// is intentionally reported relative to the host rather than leaking the
/// process' absolute checkout path into JSONL logs.
pub fn serialized_resource_snapshot(
    snapshot: crate::resources::ResourceSnapshot,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serde_json::to_value(snapshot)?;
    if let Some(workspace) = value
        .get_mut("workspace")
        .and_then(serde_json::Value::as_object_mut)
    {
        workspace.insert("root".into(), serde_json::Value::String(".".into()));
    }
    Ok(value)
}

fn append_bounded_domain_records<T: serde::Serialize>(
    value: &mut serde_json::Value,
    key: &str,
    records: Vec<&T>,
    used: &mut usize,
    truncated: &mut bool,
) -> Result<(), SlashDispatchError> {
    let Some(array) = value.get_mut(key).and_then(serde_json::Value::as_array_mut) else {
        return Err(SlashDispatchError {
            code: "serialization_error",
            message: format!("domain projection field `{key}` is not an array"),
        });
    };
    for record in records {
        let encoded = serde_json::to_vec(record).map_err(|error| SlashDispatchError {
            code: "serialization_error",
            message: error.to_string(),
        })?;
        // Reserve one byte for a comma/new array element. Over-reserving keeps
        // the final object below the hard cap even as keys are changed later.
        let additional = encoded.len().saturating_add(1);
        if used.saturating_add(additional) > MAX_DOMAIN_VIEW_BYTES {
            *truncated = true;
            break;
        }
        let record = serde_json::from_slice(&encoded).map_err(|error| SlashDispatchError {
            code: "serialization_error",
            message: error.to_string(),
        })?;
        array.push(record);
        *used = used.saturating_add(additional);
    }
    Ok(())
}

/// Read a blueprint file with the same bounded record limit as the domain
/// model. This helper is read-only: it never creates, chmods, or replaces the
/// target. A domain-store JSONL file is recognized separately by
/// [`validate_blueprint_target`].
fn read_bounded_blueprint_bytes(path: &Path) -> Result<Vec<u8>, SlashDispatchError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| SlashDispatchError {
        code: "blueprint_io",
        message: error.to_string(),
    })?;
    if metadata.file_type().is_symlink() {
        return Err(SlashDispatchError {
            code: "blueprint_path_denied",
            message: "blueprint path must not be a symbolic link".into(),
        });
    }
    if !metadata.is_file() {
        return Err(SlashDispatchError {
            code: "blueprint_not_file",
            message: "blueprint path is not a regular file".into(),
        });
    }
    if metadata.len() > crate::domains::MAX_DOMAIN_RECORD_BYTES as u64 {
        return Err(SlashDispatchError {
            code: "blueprint_too_large",
            message: format!(
                "blueprint exceeds {} bytes",
                crate::domains::MAX_DOMAIN_RECORD_BYTES
            ),
        });
    }
    let file = File::open(path).map_err(|error| SlashDispatchError {
        code: "blueprint_io",
        message: error.to_string(),
    })?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((crate::domains::MAX_DOMAIN_RECORD_BYTES as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| SlashDispatchError {
            code: "blueprint_io",
            message: error.to_string(),
        })?;
    if bytes.len() > crate::domains::MAX_DOMAIN_RECORD_BYTES {
        return Err(SlashDispatchError {
            code: "blueprint_too_large",
            message: format!(
                "blueprint exceeds {} bytes",
                crate::domains::MAX_DOMAIN_RECORD_BYTES
            ),
        });
    }
    Ok(bytes)
}

fn decode_blueprint_bytes(bytes: &[u8]) -> Result<crate::domains::Blueprint, SlashDispatchError> {
    let text = std::str::from_utf8(bytes).map_err(|_| SlashDispatchError {
        code: "blueprint_invalid_utf8",
        message: "blueprint is not valid UTF-8".into(),
    })?;
    crate::domains::Blueprint::decode_json(text).map_err(|error| SlashDispatchError {
        code: "blueprint_invalid",
        message: error.to_string(),
    })
}

fn first_json_kind(bytes: &[u8]) -> Option<String> {
    bytes
        .split(|byte| *byte == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .find(|line| !line.iter().all(u8::is_ascii_whitespace))
        .and_then(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .and_then(|value| {
            value
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
}

fn validate_blueprint_target(path: &str) -> Result<serde_json::Value, SlashDispatchError> {
    let path = resolve_workspace_path(path).map_err(|message| SlashDispatchError {
        code: "blueprint_path_denied",
        message,
    })?;
    let bytes = read_bounded_blueprint_bytes(&path)?;
    // Inspect the envelope without opening it first. Calling `DomainStore::open`
    // on every candidate would create missing parents and tighten permissions
    // on an ordinary Blueprint file. Existing domain stores use the explicit
    // read-only loader so validation has no filesystem side effects.
    if first_json_kind(&bytes).as_deref() == Some("domain_store") {
        let store = DomainStore::open_existing(&path).map_err(|error| SlashDispatchError {
            code: "domain_store_error",
            message: error.to_string(),
        })?;
        let summary = serialized_store_summary(&store)?;
        return Ok(json!({
            "kind": "domain_store",
            "valid": true,
            "store": summary,
        }));
    }
    let blueprint = decode_blueprint_bytes(&bytes)?;
    Ok(json!({
        "kind": "blueprint",
        "valid": true,
        "blueprint": blueprint,
    }))
}

/// Read-only domain-store projection shared by the TUI and JSONL hosts. The
/// returned value is already bounded by `DomainStore` validation and can be
/// rendered directly without starting a worker.
pub fn domain_store_view(agent: Option<&Agent>) -> Result<serde_json::Value, String> {
    let store = domain_store_for_agent(agent, true).map_err(|error| error.message)?;
    serialized_domain_store(&store).map_err(|error| error.message)
}

/// Validate a plain Blueprint JSON file or an existing domain-store snapshot
/// for use by non-JSONL hosts (notably the interactive TUI).
pub fn blueprint_validation_view(path: &str) -> Result<serde_json::Value, String> {
    validate_blueprint_target(path).map_err(|error| error.message)
}

/// Return the sessions owned by the configured zenpi home. This is a
/// read-only projection for slash-command hosts; it does not create the
/// sessions directory and redacts absolute filesystem paths before crossing
/// the JSONL boundary.
pub fn list_session_summaries() -> Result<Vec<SessionSummary>, String> {
    let paths = crate::config::ConfigPaths::discover().map_err(|error| error.to_string())?;
    crate::session::list_sessions_with_fallback(&paths.sessions, paths.root.join("session.jsonl"))
        .map_err(|error| error.to_string())
}

/// Serialize a bounded, secret-free session listing for TUI/headless hosts.
pub fn session_list_view() -> Result<serde_json::Value, String> {
    let sessions = list_session_summaries()?;
    let values = sessions
        .iter()
        .map(serialized_session_summary)
        .collect::<Vec<_>>();
    Ok(json!({
        "command": "session",
        "route": "local",
        "accepted": true,
        "action": "list",
        "count": values.len(),
        "sessions": values,
    }))
}

/// Open an existing session for an idle agent. Unlike `Agent::resume_session`
/// this host-facing helper validates the target first, so a typo cannot create
/// a new journal and a symbolic-link target cannot redirect the final open
/// outside the caller's intended path.
pub fn open_session_path(agent: &mut Agent, raw_path: &str) -> Result<serde_json::Value, String> {
    let path = validate_existing_session_path(raw_path)?;
    agent
        .resume_session(path)
        .map_err(|error| error.to_string())?;
    let summary = agent.snapshot().session;
    Ok(json!({
        "command": "session",
        "route": "local",
        "accepted": true,
        "action": "open",
        "session": serialized_session_summary(&summary),
    }))
}

/// Execute a durable session maintenance action without borrowing or
/// changing the active agent.  Fork/export/import deliberately take an
/// explicit source and destination so a typo cannot be interpreted as an
/// implicit active-session operation.  Sources must be clean, ordinary zenpi
/// journals; destinations must not already exist and are never overwritten.
/// All validation happens before a destination is opened or created.
pub fn session_maintenance_view(
    action: &crate::slash::SessionAction,
) -> Result<serde_json::Value, String> {
    use crate::slash::SessionAction;

    let (operation, source_raw, destination_raw) = match action {
        SessionAction::Fork {
            source,
            destination,
        } => ("fork", source.as_str(), destination.as_str()),
        SessionAction::Export {
            source,
            destination,
        } => ("export", source.as_str(), destination.as_str()),
        SessionAction::Import {
            source,
            destination,
        } => ("import", source.as_str(), destination.as_str()),
        _ => {
            return Err("session maintenance requires fork, export, or import".into());
        }
    };
    let source_path = validate_existing_session_path(source_raw)?;
    let source = SessionStore::open_existing(&source_path).map_err(|error| {
        format!("session {operation} source is not a readable journal: {error}")
    })?;
    validate_clean_session_source(&source, operation)?;
    let destination = validate_session_destination_path(destination_raw)?;

    // Keep the source immutable and let each structured SessionStore API own
    // its atomic copy/validation behavior. No shell, external diff, or model
    // call is involved in these local control-plane operations.
    let summary = match operation {
        "fork" => source
            .fork_to(&destination)
            .map(|store| store.summary())
            .map_err(|error| format!("session fork failed: {error}"))?,
        "export" => {
            source
                .export_to(&destination)
                .map_err(|error| format!("session export failed: {error}"))?;
            SessionStore::open_existing(&destination)
                .map_err(|error| format!("session export verification failed: {error}"))?
                .summary()
        }
        "import" => crate::session::import_session(&source_path, &destination)
            .map(|store| store.summary())
            .map_err(|error| format!("session import failed: {error}"))?,
        _ => unreachable!("operation was constrained above"),
    };

    Ok(json!({
        "command": "session",
        "route": "local",
        "accepted": true,
        "action": operation,
        "durable": true,
        "source": redact_session_path(&source_path.display().to_string()),
        "destination": redact_session_path(&destination.display().to_string()),
        "session": serialized_session_summary(&summary),
    }))
}

/// Execute the complete typed session lifecycle owner behind the shared slash
/// dispatcher.  The session module owns locking, identity and confirmation
/// checks; this adapter only resolves bounded host paths and projects a
/// secret-free receipt for TUI/headless callers.
pub fn session_lifecycle_view(
    action: &crate::slash::SessionAction,
    active_session: Option<&Path>,
) -> Result<serde_json::Value, String> {
    use crate::session::{
        SessionLifecycleReceipt as Receipt, SessionLifecycleRequest as Request,
        execute_session_lifecycle,
    };
    use crate::slash::SessionAction;

    let paths = crate::config::ConfigPaths::discover().map_err(|error| error.to_string())?;
    let request = match action {
        SessionAction::List => Request::List {
            directory: paths.sessions,
            include_archived: false,
        },
        SessionAction::Agents => Request::Agents {
            directory: paths.sessions,
        },
        SessionAction::Inspect { path } => Request::Inspect {
            path: validate_existing_session_path(path)?,
        },
        SessionAction::ResumeLast => Request::ResumeLast {
            directory: paths.sessions,
        },
        SessionAction::Fork {
            source,
            destination,
        } => Request::Fork {
            source: validate_existing_session_path(source)?,
            destination: validate_session_destination_path(destination)?,
        },
        SessionAction::Export {
            source,
            destination,
        } => Request::Export {
            source: validate_existing_session_path(source)?,
            destination: validate_session_destination_path(destination)?,
        },
        SessionAction::Import {
            source,
            destination,
        } => Request::Import {
            source: validate_existing_session_path(source)?,
            destination: validate_session_destination_path(destination)?,
        },
        SessionAction::Migrate {
            source,
            destination,
        } => Request::Migrate {
            source: validate_existing_session_path(source)?,
            destination: validate_session_destination_path(destination)?,
        },
        SessionAction::Archive { path } => Request::Archive {
            path: validate_existing_session_path(path)?,
        },
        SessionAction::Unarchive { path } => Request::Unarchive {
            path: validate_existing_session_path(path)?,
        },
        SessionAction::Delete { path, confirmed } => Request::Delete {
            path: validate_existing_session_path(path)?,
            confirmed: *confirmed,
        },
        SessionAction::Queue {
            source,
            recipient,
            request_id,
            payload,
            ttl_ms,
        } => Request::Queue {
            source: validate_existing_session_path(source)?,
            recipient: validate_existing_session_path(recipient)?,
            request_id: request_id.clone(),
            payload: payload.clone(),
            ttl_ms: *ttl_ms,
        },
        SessionAction::Gc { policy } => Request::Gc {
            directory: paths.sessions,
            retain_newest: policy.retain_newest,
            older_than_ms: policy.older_than_seconds.saturating_mul(1_000),
            confirmed: policy.confirm,
        },
        SessionAction::Open { .. } => {
            return Err("session open must use the active session owner".into());
        }
    };
    let receipt = execute_session_lifecycle(request, active_session, unix_time_ms())
        .map_err(|error| error.to_string())?;
    let data = match receipt {
        Receipt::Catalog { sessions } => json!({
            "sessions": sessions.iter().map(serialized_catalog_entry).collect::<Vec<_>>()
        }),
        Receipt::Inspection { inspection } => json!({
            "session": serialized_session_summary(&inspection.summary),
            "turn_count": inspection.turns.len(),
            "event_count": inspection.events.len(),
            "recovery_warnings": inspection.recovery_warnings.len(),
        }),
        Receipt::Session { session } => json!({
            "session": serialized_catalog_entry(&session)
        }),
        Receipt::Deleted { path } => json!({
            "path": redact_session_path(&path.display().to_string())
        }),
        Receipt::Queued { message } => json!({
            "message_id": message.digest,
            "message": message,
            "execution_started": false,
        }),
        Receipt::GarbageCollected {
            removed,
            inspected,
            skipped_unowned,
        } => json!({
            "removed": removed.iter().map(|path| redact_session_path(&path.display().to_string())).collect::<Vec<_>>(),
            "inspected": inspected,
            "skipped_unowned": skipped_unowned,
        }),
    };
    let mut response = json!({
        "command": "session",
        "route": "local",
        "accepted": true,
        "action": session_action_name(action),
        "durable": true,
        "receipt": data,
    });
    // Keep the compatibility shape for callers that expect a top-level
    // catalog/session field while retaining the typed receipt projection.
    if let Some(object) = response.as_object_mut()
        && let Some(receipt_object) = data.as_object()
    {
        for key in ["sessions", "session", "message_id", "message", "removed"] {
            if let Some(value) = receipt_object.get(key) {
                object.insert(key.into(), value.clone());
            }
        }
    }
    Ok(response)
}

fn serialized_catalog_entry(entry: &crate::session::SessionCatalogEntry) -> serde_json::Value {
    let mut value = serialized_session_summary(&entry.summary);
    if let Some(object) = value.as_object_mut() {
        object.insert("archived".into(), json!(entry.archived));
        object.insert("last_activity_ms".into(), json!(entry.last_activity_ms));
    }
    value
}

fn session_action_name(action: &crate::slash::SessionAction) -> &'static str {
    use crate::slash::SessionAction;
    match action {
        SessionAction::List => "list",
        SessionAction::Agents => "agents",
        SessionAction::Inspect { .. } => "inspect",
        SessionAction::Open { .. } => "open",
        SessionAction::ResumeLast => "resume_last",
        SessionAction::Fork { .. } => "fork",
        SessionAction::Export { .. } => "export",
        SessionAction::Import { .. } => "import",
        SessionAction::Migrate { .. } => "migrate",
        SessionAction::Archive { .. } => "archive",
        SessionAction::Unarchive { .. } => "unarchive",
        SessionAction::Delete { .. } => "delete",
        SessionAction::Queue { .. } => "queue",
        SessionAction::Gc { .. } => "gc",
    }
}

/// `/session resume-last` must replace the active agent, not merely open and
/// drop a temporary store. This helper is intentionally separate from the
/// immutable lifecycle receipt adapter above.
pub fn resume_last_session_view(agent: &mut Agent) -> Result<serde_json::Value, String> {
    if agent.phase() != crate::core::AgentPhase::Idle {
        return Err("session resume-last is available only while the agent is idle".into());
    }
    let paths = crate::config::ConfigPaths::discover().map_err(|error| error.to_string())?;
    let store =
        crate::session::resume_last_session(&paths.sessions).map_err(|error| error.to_string())?;
    let path = store.path().to_owned();
    agent
        .resume_session(path)
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "command": "session",
        "route": "local",
        "accepted": true,
        "action": "resume_last",
        "durable": true,
        "session": serialized_session_summary(&agent.snapshot().session),
    }))
}

/// Map the protocol mailbox request onto the shared session mailbox owner.
pub fn mailbox_slash_view(
    agent: &Agent,
    action: &crate::slash::MailboxAction,
) -> Result<serde_json::Value, String> {
    let request = match action {
        crate::slash::MailboxAction::Send {
            recipient_session_id,
            message_id,
            text,
            ttl_ms,
        } => crate::protocol::MailboxRequest::Send {
            recipient_session_id: recipient_session_id.clone(),
            message_id: message_id.clone(),
            text: text.clone(),
            ttl_ms: *ttl_ms,
        },
        crate::slash::MailboxAction::List {
            after_sequence,
            limit,
        } => crate::protocol::MailboxRequest::Receive {
            after_sequence: *after_sequence,
            limit: *limit,
        },
        crate::slash::MailboxAction::Acknowledge { message_id } => {
            crate::protocol::MailboxRequest::Acknowledge {
                message_id: message_id.clone(),
            }
        }
        crate::slash::MailboxAction::Claim { message_id } => {
            crate::protocol::MailboxRequest::Claim {
                message_id: message_id.clone(),
            }
        }
        crate::slash::MailboxAction::Complete {
            message_id,
            outcome,
            result,
        } => crate::protocol::MailboxRequest::Complete {
            message_id: message_id.clone(),
            outcome: *outcome,
            result: result.clone(),
        },
    };
    execute_mailbox(agent.session(), &request)
        .map(|mut value| {
            if let Some(object) = value.as_object_mut() {
                object.insert("command".into(), json!("mailbox"));
                object.insert("route".into(), json!("local"));
                object.insert("accepted".into(), json!(true));
            }
            value
        })
        .map_err(|error| error.to_string())
}

/// Execute the explicitly confirmed session-retention owner for both TUI and
/// headless hosts. The active journal is excluded by identity, and the
/// session module performs the final ownership/symlink checks before removal.
pub fn session_gc_view(
    agent: &Agent,
    policy: &crate::slash::SessionGcPolicy,
) -> Result<serde_json::Value, String> {
    let paths = crate::config::ConfigPaths::discover().map_err(|error| error.to_string())?;
    session_gc_view_at(agent, policy, &paths.sessions)
}

/// Testable/embedded variant of [`session_gc_view`] with an explicit session
/// directory. Production hosts use the configured path above; embedders can
/// keep their own isolated home without mutating process-global environment.
pub fn session_gc_view_at(
    agent: &Agent,
    policy: &crate::slash::SessionGcPolicy,
    directory: &Path,
) -> Result<serde_json::Value, String> {
    if !policy.confirm {
        return Err("session gc requires explicit --yes confirmation".into());
    }
    if policy.retain_newest == 0 && policy.older_than_seconds == 0 {
        return Err("session gc requires a non-zero retention policy".into());
    }
    if policy.retain_newest > crate::slash::MAX_SESSION_GC_RETAIN_NEWEST
        || policy.older_than_seconds > crate::slash::MAX_SESSION_GC_AGE_SECONDS
    {
        return Err("session gc retention policy exceeds the bounded limit".into());
    }
    if agent.phase() != crate::core::AgentPhase::Idle {
        return Err("session gc is available only while the agent is idle".into());
    }
    let report = crate::session::garbage_collect_sessions_with_active(
        directory,
        crate::session::GarbageCollectionPolicy {
            retain_newest: policy.retain_newest,
            older_than_ms: policy.older_than_seconds.saturating_mul(1_000),
        },
        Some(agent.session().path()),
        unix_time_ms(),
    )
    .map_err(|error| error.to_string())?;
    let removed = report
        .removed
        .iter()
        .map(|path| bound_slash_text(&redact_session_path(&path.display().to_string()), 256))
        .collect::<Vec<_>>();
    let value = json!({
        "command": "session",
        "route": "local",
        "accepted": true,
        "action": "gc",
        "durable": true,
        "confirmed": true,
        "retention": {
            "retain_newest": policy.retain_newest,
            "older_than_seconds": policy.older_than_seconds,
        },
        "inspected": report.inspected,
        "skipped_unowned": report.skipped_unowned,
        "removed_count": report.removed.len(),
        "removed": removed,
    });
    let encoded = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    if encoded.len() > MAX_SESSION_GC_RECEIPT_BYTES {
        return Err("session gc receipt exceeds the bounded response limit".into());
    }
    Ok(value)
}

fn validate_clean_session_source(source: &SessionStore, operation: &str) -> Result<(), String> {
    let valid_header = source
        .records()
        .first()
        .is_some_and(|record| record.kind == "session" && record.sequence == 0);
    if !valid_header || !source.recovery_warnings().is_empty() {
        return Err(format!(
            "session {operation} source must be a clean zenpi session journal"
        ));
    }
    Ok(())
}

fn validate_session_destination_path(raw_path: &str) -> Result<PathBuf, String> {
    if raw_path.trim().is_empty()
        || raw_path.len() > MAX_WORKSPACE_PATH_BYTES
        || raw_path.chars().any(char::is_control)
    {
        return Err(
            "session destination is empty, too long, or contains control characters".into(),
        );
    }
    let path = Path::new(raw_path);
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("session destination must not contain parent traversal".into());
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err("session destination must not be a symbolic link".into())
        }
        Ok(_) => Err("session destination already exists".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path.to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// Replay a bounded suffix of the active durable journal for the local slash
/// owner.  Unlike the transport-level `resume` command (which replays an
/// in-memory event queue), this operation survives process restarts and writes
/// a `session_resumed` marker before reporting success.
pub fn resume_session_view(
    agent: &mut Agent,
    requested_sequence: Option<u64>,
) -> Result<serde_json::Value, String> {
    if agent.phase() == crate::core::AgentPhase::Closed {
        return Err("agent is closed".into());
    }
    if agent.phase() == crate::core::AgentPhase::Running {
        return Err("session replay is unavailable while the agent is busy".into());
    }
    let session = agent.session();
    let next_sequence = session.next_sequence();
    if requested_sequence.is_some_and(|sequence| sequence > next_sequence) {
        return Err(format!(
            "resume sequence is ahead of the session (next sequence is {next_sequence})"
        ));
    }
    let first_available = session.records().first().map(|record| record.sequence);
    let last_available = session.records().last().map(|record| record.sequence);
    let recovery_warnings = session
        .recovery_warnings()
        .iter()
        .take(32)
        .map(|warning| {
            json!({
                "line": warning.line,
                "reason": bound_slash_text(&warning.reason, 512),
            })
        })
        .collect::<Vec<_>>();
    let from_sequence = requested_sequence.unwrap_or_else(|| {
        last_available
            .or(first_available)
            .map(|last| last.saturating_sub(31))
            .unwrap_or(next_sequence)
    });
    let replay_gap = first_available.is_some_and(|first| from_sequence < first);
    let mut records = Vec::new();
    let mut retained_bytes = 0_usize;
    let mut truncated = false;
    for record in session
        .records()
        .iter()
        .filter(|record| record.sequence >= from_sequence)
    {
        if records.len() >= MAX_SLASH_RESUME_RECORDS {
            truncated = true;
            break;
        }
        let projected = project_session_record(record);
        let encoded_len = serde_json::to_vec(&projected)
            .map_err(|error| format!("resume projection failed: {error}"))?
            .len();
        if !records.is_empty()
            && retained_bytes.saturating_add(encoded_len) > MAX_SLASH_RESUME_BYTES
        {
            truncated = true;
            break;
        }
        retained_bytes = retained_bytes.saturating_add(encoded_len);
        records.push(projected);
    }
    if !truncated {
        truncated = session.records().iter().any(|record| {
            record.sequence >= from_sequence
                && record.sequence
                    > records
                        .last()
                        .and_then(|value| value.get("sequence"))
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(from_sequence)
        });
    }
    let replay_cursor = records
        .last()
        .and_then(|record| record.get("sequence"))
        .and_then(serde_json::Value::as_u64)
        .map_or(from_sequence, |sequence| sequence.saturating_add(1));
    let replayed = records.len();
    let recovered_operations = session
        .interrupted_operations()
        .into_iter()
        .map(|operation| {
            serde_json::json!({
                "operation_id": operation.operation_id,
                "kind": operation.kind,
                "turn_id": operation.turn_id,
                "retry_requires_confirmation": operation.retry_requires_confirmation,
            })
        })
        .collect::<Vec<_>>();
    // The marker is appended only after projection succeeds. A disk failure
    // therefore returns an error instead of claiming that recovery happened.
    let marker_sequence = agent.session().next_sequence();
    agent
        .session_mut()
        .append_event(json!({
            "type": "session_resumed",
            "operation": "resume",
            "trigger": "manual_slash",
            "requested_sequence": requested_sequence,
            "from_sequence": from_sequence,
            "replayed": replayed,
            "truncated": truncated,
            "replay_cursor": replay_cursor,
        }))
        .map_err(|error| format!("resume marker could not be persisted: {error}"))?;
    let next_sequence = agent.session().next_sequence();
    Ok(json!({
        "command": "resume",
        "route": "local",
        "accepted": true,
        // Keep the original slash response field while making the durable
        // journal interpretation explicit below.
        "sequence": requested_sequence,
        "durable": true,
        "requested_sequence": requested_sequence,
        "from_sequence": from_sequence,
        "first_available_sequence": first_available,
        "last_available_sequence": last_available,
        "replayed": replayed,
        "replay_gap": replay_gap,
        "truncated": truncated,
        "replay_cursor": replay_cursor,
        "next_sequence": next_sequence,
        "marker_sequence": marker_sequence,
        "recovery_warnings": recovery_warnings,
        "recovered_operations": recovered_operations,
        "records": records,
    }))
}

fn project_session_record(record: &crate::session::SessionRecord) -> serde_json::Value {
    match record.kind.as_str() {
        "session" => json!({
            "sequence": record.sequence,
            "kind": "session",
            "session_id": record.value.get("session_id"),
            "version": record.value.get("version"),
        }),
        "turn" => {
            let turn = record.value.get("turn");
            let content = turn
                .and_then(|value| value.get("content"))
                .and_then(serde_json::Value::as_str)
                .map(|value| bound_slash_text(value, 4096));
            let original_len = turn
                .and_then(|value| value.get("content"))
                .and_then(serde_json::Value::as_str)
                .map_or(0, str::len);
            json!({
                "sequence": record.sequence,
                "kind": "turn",
                "id": turn.and_then(|value| value.get("id")),
                "parent_id": turn.and_then(|value| value.get("parent_id")),
                "role": turn.and_then(|value| value.get("role")),
                "content": content,
                "content_truncated": original_len > 4096,
            })
        }
        "event" => {
            let event = record.value.get("event").cloned().unwrap_or_default();
            let encoded_len = serde_json::to_vec(&event).map_or(0, |bytes| bytes.len());
            if encoded_len > MAX_SLASH_RESUME_EVENT_BYTES {
                json!({
                    "sequence": record.sequence,
                    "kind": "event",
                    "event_type": event.get("type").and_then(serde_json::Value::as_str),
                    "truncated": true,
                    "encoded_bytes": encoded_len,
                })
            } else {
                json!({
                    "sequence": record.sequence,
                    "kind": "event",
                    "event": event,
                })
            }
        }
        kind => json!({
            "sequence": record.sequence,
            "kind": kind,
            "redacted": true,
        }),
    }
}

/// Execute deterministic context compaction through the core owner and add a
/// small transport-neutral projection for TUI/headless callers.
pub fn compact_context_view(agent: &mut Agent) -> Result<serde_json::Value, String> {
    let report = agent
        .compact_context()
        .map_err(|error| format!("context compaction failed: {error}"))?;
    let mut value = serde_json::to_value(report)
        .map_err(|error| format!("context compaction response failed: {error}"))?;
    if let Some(object) = value.as_object_mut() {
        object.insert("command".into(), json!("compact"));
        object.insert("route".into(), json!("local"));
        object.insert("accepted".into(), json!(true));
        object.insert("durable".into(), json!(true));
        object.insert(
            "next_sequence".into(),
            json!(agent.session().next_sequence()),
        );
    }
    Ok(value)
}

fn serialized_session_summary(summary: &SessionSummary) -> serde_json::Value {
    json!({
        "path": redact_session_path(&summary.path),
        "session_id": summary.session_id,
        "created_at_ms": summary.created_at_ms,
        "turn_count": summary.turn_count,
        "handoff_count": summary.handoff_count,
        "handoff_record_count": summary.handoff_record_count,
        "runtime_intent_count": summary.runtime_intent_count,
        "event_count": summary.event_count,
        "recovery_warnings": summary.recovery_warnings,
        "next_seq": summary.next_seq,
    })
}

fn serialized_agent_snapshot(agent: &Agent) -> Result<serde_json::Value, serde_json::Error> {
    let mut snapshot = agent.snapshot();
    snapshot.session.path = redact_session_path(&snapshot.session.path);
    serde_json::to_value(snapshot)
}

fn redact_session_path(raw: &str) -> String {
    let path = Path::new(raw);
    if let (Ok(cwd), Ok(canonical)) = (
        std::env::current_dir().and_then(|path| path.canonicalize()),
        path.canonicalize(),
    ) && let Ok(relative) = canonical.strip_prefix(cwd)
    {
        let rendered = relative.display().to_string();
        return if rendered.is_empty() {
            ".".into()
        } else {
            format!("./{rendered}")
        };
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("<session:{name}>"))
        .unwrap_or_else(|| "<session>".into())
}

fn validate_existing_session_path(raw_path: &str) -> Result<PathBuf, String> {
    if raw_path.trim().is_empty()
        || raw_path.len() > MAX_WORKSPACE_PATH_BYTES
        || raw_path.chars().any(char::is_control)
    {
        return Err("session path is empty, too long, or contains control characters".into());
    }
    let path = Path::new(raw_path);
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("session path must not contain parent traversal".into());
    }
    // Check the final component lexically. Parent directories may be platform
    // aliases (for example macOS `/var` -> `/private/var`); the session store
    // itself uses `O_NOFOLLOW` for the final open and rejects a final symlink.
    // Rejecting every ancestor here would make ordinary temporary/session paths
    // unusable on those systems.
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("session path is not a regular file".into());
    }
    path.canonicalize().map_err(|error| error.to_string())
}

/// Read one bounded JSON domain record from a regular, non-symlink file.
/// Domain records are intentionally supplied by path rather than embedded in
/// a slash command: the command frame remains small and the JSON parser can
/// enforce the same 64 KiB record limit as the durable store.
fn read_domain_json<T: DeserializeOwned>(
    path: &Path,
    kind: &'static str,
) -> Result<T, SlashDispatchError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| SlashDispatchError {
        code: "domain_input_io",
        message: error.to_string(),
    })?;
    if metadata.file_type().is_symlink() {
        return Err(SlashDispatchError {
            code: "domain_input_path_denied",
            message: format!("{kind} input path must not be a symbolic link"),
        });
    }
    if !metadata.is_file() {
        return Err(SlashDispatchError {
            code: "domain_input_not_file",
            message: format!("{kind} input path is not a regular file"),
        });
    }
    let limit = crate::domains::MAX_DOMAIN_RECORD_BYTES;
    if metadata.len() > limit as u64 {
        return Err(SlashDispatchError {
            code: "domain_input_too_large",
            message: format!("{kind} input exceeds {limit} bytes"),
        });
    }
    let file = File::open(path).map_err(|error| SlashDispatchError {
        code: "domain_input_io",
        message: error.to_string(),
    })?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| SlashDispatchError {
            code: "domain_input_io",
            message: error.to_string(),
        })?;
    if bytes.len() > limit {
        return Err(SlashDispatchError {
            code: "domain_input_too_large",
            message: format!("{kind} input exceeds {limit} bytes"),
        });
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| SlashDispatchError {
        code: "domain_input_invalid_utf8",
        message: format!("{kind} input is not valid UTF-8"),
    })?;
    serde_json::from_str(text).map_err(|error| SlashDispatchError {
        code: "domain_input_invalid",
        message: format!("{kind} input is invalid JSON: {error}"),
    })
}

fn persist_blueprint_from_path(
    agent: Option<&mut Agent>,
    path: &str,
) -> Result<serde_json::Value, SlashDispatchError> {
    let path = resolve_workspace_path(path).map_err(|message| SlashDispatchError {
        code: "domain_input_path_denied",
        message,
    })?;
    let blueprint: crate::domains::Blueprint = read_domain_json(&path, "blueprint")?;
    blueprint.validate().map_err(|error| SlashDispatchError {
        code: "blueprint_invalid",
        message: error.to_string(),
    })?;
    let Some(agent) = agent else {
        return Err(SlashDispatchError {
            code: "agent_busy",
            message: "blueprint persistence is unavailable while the agent is busy".into(),
        });
    };
    let mut store = domain_store_for_agent(Some(agent), false)?;
    let change = store
        .put_blueprint(blueprint.clone())
        .map_err(|error| SlashDispatchError {
            code: "domain_store_error",
            message: error.to_string(),
        })?;
    let summary = serialized_store_summary(&store)?;
    Ok(json!({
        "command": "blueprint",
        "route": "local",
        "accepted": true,
        "action": "put",
        "change": change,
        "blueprint": blueprint,
        "store": summary,
    }))
}

fn persist_goal_from_path(
    agent: Option<&mut Agent>,
    path: &str,
) -> Result<serde_json::Value, SlashDispatchError> {
    let path = resolve_workspace_path(path).map_err(|message| SlashDispatchError {
        code: "domain_input_path_denied",
        message,
    })?;
    let goal: crate::domains::Goal = read_domain_json(&path, "goal")?;
    goal.validate().map_err(|error| SlashDispatchError {
        code: "goal_invalid",
        message: error.to_string(),
    })?;
    let Some(agent) = agent else {
        return Err(SlashDispatchError {
            code: "agent_busy",
            message: "goal persistence is unavailable while the agent is busy".into(),
        });
    };
    let mut store = domain_store_for_agent(Some(agent), false)?;
    let change = store
        .put_goal(goal.clone())
        .map_err(|error| SlashDispatchError {
            code: "domain_store_error",
            message: error.to_string(),
        })?;
    let summary = serialized_store_summary(&store)?;
    Ok(json!({
        "command": "goal",
        "route": "local",
        "accepted": true,
        "action": "put",
        "change": change,
        "goal": goal,
        "store": summary,
    }))
}

fn persist_learn_from_path(
    agent: Option<&mut Agent>,
    path: &str,
) -> Result<serde_json::Value, SlashDispatchError> {
    let path = resolve_workspace_path(path).map_err(|message| SlashDispatchError {
        code: "domain_input_path_denied",
        message,
    })?;
    let learn: crate::domains::Learn = read_domain_json(&path, "learn")?;
    learn.validate().map_err(|error| SlashDispatchError {
        code: "learn_invalid",
        message: error.to_string(),
    })?;
    let Some(agent) = agent else {
        return Err(SlashDispatchError {
            code: "agent_busy",
            message: "learn persistence is unavailable while the agent is busy".into(),
        });
    };
    let mut store = domain_store_for_agent(Some(agent), false)?;
    let change = store
        .put_learn(learn.clone())
        .map_err(|error| SlashDispatchError {
            code: "domain_store_error",
            message: error.to_string(),
        })?;
    let summary = serialized_store_summary(&store)?;
    Ok(json!({
        "command": "learn",
        "route": "local",
        "accepted": true,
        "action": "put",
        "change": change,
        "learn": learn,
        "store": summary,
    }))
}

/// Add one existing repository-relative artifact to a Learn record. The
/// artifact is inspected only far enough to produce a bounded integrity
/// receipt; its bytes never cross the command boundary or reach a provider.
/// The domain snapshot is probed read-only before opening the mutable owner so
/// missing IDs and invalid paths cannot create a sibling store.
fn add_learn_evidence_from_ref(
    agent: Option<&mut Agent>,
    id: &str,
    raw_reference: &str,
) -> Result<serde_json::Value, SlashDispatchError> {
    validate_domain_id(id, "learn_id")?;
    let path = resolve_workspace_path(raw_reference).map_err(|message| SlashDispatchError {
        code: "learn_evidence_path_denied",
        message,
    })?;
    let receipt = learn_evidence_receipt(&path, raw_reference)?;
    let Some(agent) = agent else {
        return Err(SlashDispatchError {
            code: "agent_busy",
            message: "learn evidence is unavailable while the agent is busy".into(),
        });
    };
    let store_path = domain_store::path_for_session(agent.session().path());
    let existing =
        DomainStore::open_read_only(&store_path).map_err(|error| SlashDispatchError {
            code: "domain_store_error",
            message: error.to_string(),
        })?;
    if existing.learn(id).is_none() {
        return Err(SlashDispatchError {
            code: "learn_not_found",
            message: format!("learn `{id}` is not found"),
        });
    }
    drop(existing);

    let mut store = DomainStore::open(&store_path).map_err(|error| SlashDispatchError {
        code: "domain_store_error",
        message: error.to_string(),
    })?;
    let change = store
        .add_learn_evidence(id, raw_reference.to_owned())
        .map_err(|error| match error {
            domain_store::DomainStoreError::LearnNotFound(id) => SlashDispatchError {
                code: "learn_not_found",
                message: format!("learn `{id}` is not found"),
            },
            other => SlashDispatchError {
                code: "learn_evidence_error",
                message: other.to_string(),
            },
        })?;
    let learn = store.learn(id).ok_or_else(|| SlashDispatchError {
        code: "learn_not_found",
        message: format!("learn `{id}` is not found"),
    })?;
    let summary = serialized_store_summary(&store)?;
    Ok(json!({
        "command": "learn",
        "route": "local",
        "accepted": true,
        "action": "evidence",
        "learn_id": id,
        "change": change,
        "evidence": receipt,
        "learn": learn,
        "store": summary,
    }))
}

/// Return a deterministic, secret-free checkpoint for a Learn record. This
/// operation is intentionally read-only: until an external b3ehive owner is
/// available, `resume` validates the durable checkpoint but does not start a
/// model or worker execution.
fn resume_learn_view(
    agent: Option<&Agent>,
    id: &str,
) -> Result<serde_json::Value, SlashDispatchError> {
    validate_domain_id(id, "learn_id")?;
    let store = domain_store_for_agent(agent, true)?;
    let learn = store.learn(id).ok_or_else(|| SlashDispatchError {
        code: "learn_not_found",
        message: format!("learn `{id}` is not found"),
    })?;
    let summary = serialized_store_summary(&store)?;
    Ok(json!({
        "command": "learn",
        "route": "local",
        "accepted": true,
        "action": "resume",
        "learn_id": id,
        "checkpoint": {
            "validated": true,
            "evidence_count": learn.evidence.len(),
            "execution_state": "untracked",
            "delivery": "external_owner_required",
            "zenpi_started": false,
        },
        "learn": learn,
        "store": summary,
    }))
}

/// Execute one deterministic, dependency-aware Blueprint item through the
/// local receipt owner.  This operation is deliberately provider/shell-free:
/// it proves durable admission, budget accounting, dependency ordering, and
/// restart-safe evidence without pretending that zenpi is a b3ehive scheduler.
pub fn run_blueprint_next(agent: &mut Agent, target: &str) -> Result<serde_json::Value, String> {
    if agent.phase() != crate::core::AgentPhase::Idle {
        return Err("blueprint execution is available only while the agent is idle".into());
    }
    let target = target.trim();
    if target.is_empty() || target.len() > crate::domains::MAX_ID_BYTES {
        return Err("blueprint run target must be a bounded id or id@version".into());
    }
    let (blueprint_id, version) = target
        .split_once('@')
        .map_or((target, None), |(id, version)| (id, Some(version)));
    if blueprint_id.is_empty()
        || blueprint_id.len() > crate::domains::MAX_ID_BYTES
        || blueprint_id.chars().any(char::is_control)
        || !blueprint_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._:/-".contains(character))
        || version.is_some_and(|value| {
            value.is_empty()
                || value.len() > crate::domains::MAX_VERSION_BYTES
                || value.chars().any(char::is_control)
                || value.chars().any(char::is_whitespace)
        })
    {
        return Err("blueprint run target is invalid".into());
    }
    let path = domain_store::path_for_session(agent.session().path());
    let store = DomainStore::open_read_only(&path).map_err(|error| error.to_string())?;
    let blueprint = if let Some(version) = version {
        store
            .blueprint(blueprint_id, version)
            .cloned()
            .ok_or_else(|| format!("blueprint `{target}` is not found"))?
    } else {
        let versions = store.blueprint_versions(blueprint_id);
        if versions.is_empty() {
            return Err(format!("blueprint `{target}` is not found"));
        }
        if versions.len() > 1 {
            return Err(format!(
                "blueprint `{blueprint_id}` has multiple versions; use id@version"
            ));
        }
        versions[0].clone()
    };
    let linked_goals = store
        .goals()
        .into_iter()
        .filter(|goal| {
            goal.blueprint_id == blueprint.id
                && goal.blueprint_version == blueprint.version
                && goal.blueprint_digest == blueprint.digest
        })
        .cloned()
        .collect::<Vec<_>>();
    let goal = match linked_goals.as_slice() {
        [] => return Err(format!("no Goal is linked to blueprint `{target}`")),
        [goal] => goal.clone(),
        _ => {
            return Err(format!(
                "blueprint `{target}` has multiple linked Goals; run one Goal at a time"
            ));
        }
    };
    drop(store);

    let execution_path = crate::domain_execution::path_for_session(agent.session().path());
    let execution_store = crate::domain_execution::ExecutionStore::open(&execution_path)
        .map_err(|error| error.to_string())?;
    let mut executor = crate::domain_execution::BlueprintExecutor::new(execution_store);
    let outcome = executor
        .run_next(&goal, &blueprint, || false)
        .map_err(|error| error.to_string())?;
    let mut value = match &outcome {
        crate::domain_execution::RunOutcome::Executed {
            receipt,
            resumed_running,
        } => json!({
            "command": "blueprint",
            "route": "local",
            "accepted": true,
            "action": "run",
            "target": target,
            "goal_id": goal.id,
            "status": "recorded",
            "execution_scope": "control_plane_only",
            "external_work_executed": false,
            "resumed_running": resumed_running,
            "receipt": receipt,
        }),
        crate::domain_execution::RunOutcome::Complete => json!({
            "command": "blueprint",
            "route": "local",
            "accepted": true,
            "action": "run",
            "target": target,
            "goal_id": goal.id,
            "status": "complete",
        }),
        crate::domain_execution::RunOutcome::Blocked {
            item_id,
            waiting_on,
        } => json!({
            "command": "blueprint",
            "route": "local",
            "accepted": true,
            "action": "run",
            "target": target,
            "goal_id": goal.id,
            "status": "blocked",
            "item_id": item_id,
            "waiting_on": waiting_on,
        }),
        crate::domain_execution::RunOutcome::Cancelled { item_id, attempt } => json!({
            "command": "blueprint",
            "route": "local",
            "accepted": true,
            "action": "run",
            "target": target,
            "goal_id": goal.id,
            "status": "cancelled",
            "item_id": item_id,
            "attempt": attempt,
        }),
    };
    let goal_status =
        sync_goal_status_after_execution(agent, &goal, &blueprint, executor.store(), &outcome)?;
    if let Some(object) = value.as_object_mut() {
        object.insert("goal_status".into(), json!(goal_status));
        object.insert(
            "receipt_store_generation".into(),
            json!(executor.store().generation()),
        );
        object.insert(
            "receipt_count".into(),
            json!(executor.store().receipts().len()),
        );
    }
    Ok(value)
}

/// Queue one declarative Blueprint task for an external b3ehive owner. This
/// never runs the instruction, shell, provider, or acceptance commands.
pub fn handoff_blueprint_next(
    agent: &mut Agent,
    target: &str,
) -> Result<serde_json::Value, String> {
    if agent.phase() != crate::core::AgentPhase::Idle {
        return Err("blueprint handoff is available only while the agent is idle".into());
    }
    let (blueprint_id, version) = target
        .split_once('@')
        .map_or((target, None), |(id, version)| (id, Some(version)));
    let store = DomainStore::open_read_only(domain_store::path_for_session(agent.session().path()))
        .map_err(|error| error.to_string())?;
    let blueprint = if let Some(version) = version {
        store.blueprint(blueprint_id, version).cloned()
    } else {
        let versions = store.blueprint_versions(blueprint_id);
        (versions.len() == 1).then(|| versions[0].clone())
    }
    .ok_or_else(|| format!("blueprint `{target}` is not found or needs an explicit version"))?;
    let goal = store
        .goals()
        .into_iter()
        .find(|goal| {
            goal.blueprint_id == blueprint.id
                && goal.blueprint_version == blueprint.version
                && goal.blueprint_digest == blueprint.digest
        })
        .cloned()
        .ok_or_else(|| format!("no Goal is linked to blueprint `{target}`"))?;
    let execution = crate::domain_execution::ExecutionStore::open(
        crate::domain_execution::path_for_session(agent.session().path()),
    )
    .map_err(|error| error.to_string())?;
    let mut handoffs = crate::domain_execution::HandoffStore::open(
        crate::domain_execution::handoff_path_for_session(agent.session().path()),
    )
    .map_err(|error| error.to_string())?;
    let owner = crate::domain_execution::BlueprintExecutor::new(execution);
    let outcome = owner
        .handoff_next(&mut handoffs, &goal, &blueprint)
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "command": "blueprint",
        "route": "external_owner",
        "accepted": true,
        "action": "handoff",
        "target": target,
        "handoff": outcome.request,
        "already_queued": outcome.already_queued,
        "execution_scope": "external_owner_required",
        "zenpi_started": false,
        "status": "queued",
    }))
}

/// Reconcile the durable Goal state after a receipt owner call.  Receipts are
/// written first, so a crash between the two snapshots is recoverable: a
/// later `/blueprint run` sees the existing receipt and finishes the Goal
/// transition without repeating the item.
fn sync_goal_status_after_execution(
    agent: &Agent,
    goal: &crate::domains::Goal,
    _blueprint: &crate::domains::Blueprint,
    _execution_store: &crate::domain_execution::ExecutionStore,
    outcome: &crate::domain_execution::RunOutcome,
) -> Result<crate::domains::GoalStatus, String> {
    let should_start = matches!(
        outcome,
        crate::domain_execution::RunOutcome::Executed { .. }
            | crate::domain_execution::RunOutcome::Complete
    ) && goal.status == crate::domains::GoalStatus::Queued;
    let mut store = DomainStore::open(domain_store::path_for_session(agent.session().path()))
        .map_err(|error| error.to_string())?;
    if should_start {
        store
            .transition_goal(&goal.id, crate::domains::GoalStatus::Running)
            .map_err(|error| error.to_string())?;
    }
    let status = store
        .goal(&goal.id)
        .map(|record| record.status)
        .ok_or_else(|| format!("Goal `{}` disappeared during Blueprint execution", goal.id))?;
    // No external-work evidence importer exists yet. The local receipt owner
    // is deliberately bookkeeping-only, so it must never close a user-facing
    // Goal, even when every local receipt says `succeeded`. The unused
    // reconciliation inputs stay in the signature for the future
    // result-manifest owner.
    Ok(status)
}

fn validate_domain_id(value: &str, field: &'static str) -> Result<(), SlashDispatchError> {
    if value.trim().is_empty()
        || value.len() > crate::domains::MAX_ID_BYTES
        || value.chars().any(char::is_control)
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._:/-".contains(character))
    {
        return Err(SlashDispatchError {
            code: "learn_invalid_id",
            message: format!("{field} is not a valid bounded identifier"),
        });
    }
    Ok(())
}

fn learn_evidence_receipt(
    path: &Path,
    requested: &str,
) -> Result<serde_json::Value, SlashDispatchError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| SlashDispatchError {
        code: "learn_evidence_io",
        message: error.to_string(),
    })?;
    let workspace = std::env::current_dir()
        .map_err(|error| SlashDispatchError {
            code: "learn_evidence_io",
            message: error.to_string(),
        })?
        .canonicalize()
        .map_err(|error| SlashDispatchError {
            code: "learn_evidence_io",
            message: error.to_string(),
        })?;
    let relative = path
        .strip_prefix(&workspace)
        .map(|value| value.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| requested.to_owned());
    if metadata.is_dir() {
        return Ok(json!({
            "reference": relative,
            "kind": "directory",
            "bytes": 0,
        }));
    }
    if !metadata.is_file() {
        return Err(SlashDispatchError {
            code: "learn_evidence_not_file",
            message: "learn evidence reference is not a regular file or directory".into(),
        });
    }
    if metadata.len() > MAX_LEARN_EVIDENCE_BYTES {
        return Err(SlashDispatchError {
            code: "learn_evidence_too_large",
            message: format!("learn evidence exceeds {MAX_LEARN_EVIDENCE_BYTES} bytes"),
        });
    }
    let mut file = File::open(path).map_err(|error| SlashDispatchError {
        code: "learn_evidence_io",
        message: error.to_string(),
    })?;
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer).map_err(|error| SlashDispatchError {
            code: "learn_evidence_io",
            message: error.to_string(),
        })?;
        if read == 0 {
            break;
        }
        bytes = bytes.saturating_add(read as u64);
        if bytes > MAX_LEARN_EVIDENCE_BYTES {
            return Err(SlashDispatchError {
                code: "learn_evidence_too_large",
                message: format!("learn evidence exceeds {MAX_LEARN_EVIDENCE_BYTES} bytes"),
            });
        }
        digest.update(&buffer[..read]);
    }
    Ok(json!({
        "reference": relative,
        "kind": "file",
        "bytes": bytes,
        "sha256": format!("{:x}", digest.finalize()),
    }))
}

/// Persist a validated Blueprint JSON document for an interactive host.
/// Errors are returned as text so the TUI can render them without exposing
/// the headless transport's private dispatch type.
pub fn persist_blueprint_path(agent: &mut Agent, path: &str) -> Result<serde_json::Value, String> {
    persist_blueprint_from_path(Some(agent), path).map_err(|error| error.message)
}

/// Persist a validated Goal JSON document for an interactive host.
pub fn persist_goal_path(agent: &mut Agent, path: &str) -> Result<serde_json::Value, String> {
    persist_goal_from_path(Some(agent), path).map_err(|error| error.message)
}

/// Persist a validated Learn JSON document for an interactive host.
pub fn persist_learn_path(agent: &mut Agent, path: &str) -> Result<serde_json::Value, String> {
    persist_learn_from_path(Some(agent), path).map_err(|error| error.message)
}

/// Add a validated repository-relative artifact reference to a Learn record
/// for interactive hosts. The returned receipt is bounded and never contains
/// file contents.
pub fn add_learn_evidence(
    agent: &mut Agent,
    id: &str,
    reference: &str,
) -> Result<serde_json::Value, String> {
    add_learn_evidence_from_ref(Some(agent), id, reference).map_err(|error| error.message)
}

/// Inspect a durable Learn checkpoint for interactive hosts. This does not
/// invoke a provider or external worker.
pub fn resume_learn(agent: &Agent, id: &str) -> Result<serde_json::Value, String> {
    resume_learn_view(Some(agent), id).map_err(|error| error.message)
}

fn request_in_flight(
    id: Option<&str>,
    request_to_job: &HashMap<String, crate::runtime::JobId>,
    pending_steers: &VecDeque<PendingSteer>,
) -> bool {
    let Some(id) = id else {
        return false;
    };
    request_to_job.contains_key(id)
        || pending_steers
            .iter()
            .any(|pending| pending.id.as_deref() == Some(id))
}

fn cancel_pending_steers_for_job<W: Write>(
    pending_steers: &mut VecDeque<PendingSteer>,
    target_job: crate::runtime::JobId,
    output: &mut W,
    replay: &mut ReplayState,
) -> Result<usize, HeadlessError> {
    let mut retained = VecDeque::with_capacity(pending_steers.len());
    let mut cancelled = 0;
    while let Some(pending) = pending_steers.pop_front() {
        if pending.target_job == target_job {
            write_cached_versioned_response(
                output,
                StdioResponse::error_with_code(
                    pending.id,
                    "steer",
                    "request_cancelled",
                    "deferred steer was cancelled before reissue",
                ),
                pending.version,
                replay,
            )?;
            cancelled += 1;
        } else {
            retained.push_back(pending);
        }
    }
    *pending_steers = retained;
    Ok(cancelled)
}

fn cancel_pending_steer_by_id<W: Write>(
    pending_steers: &mut VecDeque<PendingSteer>,
    target_id: &str,
    output: &mut W,
    replay: &mut ReplayState,
) -> Result<bool, HeadlessError> {
    let Some(index) = pending_steers
        .iter()
        .position(|pending| pending.id.as_deref() == Some(target_id))
    else {
        return Ok(false);
    };
    let Some(pending) = pending_steers.remove(index) else {
        return Ok(false);
    };
    write_cached_versioned_response(
        output,
        StdioResponse::error_with_code(
            pending.id,
            "steer",
            "request_cancelled",
            "deferred steer was cancelled before reissue",
        ),
        pending.version,
        replay,
    )?;
    Ok(true)
}

fn runtime_error_details(reason: crate::runtime::SubmitError) -> (&'static str, &'static str) {
    match reason {
        crate::runtime::SubmitError::QueueFull => (
            "runtime_queue_full",
            "runtime rejected the request because its command queue is full",
        ),
        crate::runtime::SubmitError::Closed => (
            "runtime_closed",
            "runtime rejected the request because it is closed",
        ),
    }
}

fn write_runtime_rejection<W: Write>(
    output: &mut W,
    id: Option<String>,
    command: &str,
    reason: crate::runtime::SubmitError,
    version: u16,
    replay: &mut ReplayState,
) -> Result<(), HeadlessError> {
    let (code, message) = runtime_error_details(reason);
    let response =
        StdioResponse::error_with_code(id.clone(), command, code, message).for_version(version);
    if reason == crate::runtime::SubmitError::QueueFull {
        // Queue pressure is retryable. Do not turn a transient admission
        // failure into a cached terminal result for this request ID.
        replay.release(id.as_deref())?;
        write_response(output, response)
    } else {
        write_cached_response(output, response, replay)
    }
}

/// Emit a retryable control-plane failure and release the request ID
/// reservation made before dispatch. A caller may safely retry the same ID
/// after the transient condition clears.
fn write_retryable_response<W: Write>(
    output: &mut W,
    response: StdioResponse,
    replay: &mut ReplayState,
) -> Result<(), HeadlessError> {
    replay.release(response.id.as_deref())?;
    write_response(output, response)
}

fn write_cached_versioned_response<W: Write>(
    output: &mut W,
    response: StdioResponse,
    version: u16,
    replay: &mut ReplayState,
) -> Result<(), HeadlessError> {
    write_cached_response(output, response.for_version(version), replay)
}

enum AsyncTurn {
    Standard(TurnInputRequest),
    UserShell(String),
    Reissue {
        message: String,
        superseded_turn_id: String,
    },
}

struct AsyncRequest {
    turn: AsyncTurn,
    events: Arc<Mutex<AsyncEventBuffer>>,
    started_turn_id: Arc<Mutex<Option<String>>>,
}

type ProviderEventQueue = Arc<Mutex<AsyncEventBuffer>>;
type StartedTurn = Arc<Mutex<Option<String>>>;
type AsyncRequestParts = (AsyncRequest, ProviderEventQueue, StartedTurn);

impl AsyncRequest {
    fn new(request: TurnInputRequest) -> AsyncRequestParts {
        let events = Arc::new(Mutex::new(AsyncEventBuffer::default()));
        let started_turn_id = Arc::new(Mutex::new(None));
        (
            Self {
                turn: AsyncTurn::Standard(request),
                events: Arc::clone(&events),
                started_turn_id: Arc::clone(&started_turn_id),
            },
            events,
            started_turn_id,
        )
    }

    fn reissue(message: String, superseded_turn_id: String) -> AsyncRequestParts {
        let events = Arc::new(Mutex::new(AsyncEventBuffer::default()));
        let started_turn_id = Arc::new(Mutex::new(None));
        (
            Self {
                turn: AsyncTurn::Reissue {
                    message,
                    superseded_turn_id,
                },
                events: Arc::clone(&events),
                started_turn_id: Arc::clone(&started_turn_id),
            },
            events,
            started_turn_id,
        )
    }

    fn user_shell(input: String) -> AsyncRequestParts {
        let events = Arc::new(Mutex::new(AsyncEventBuffer::default()));
        let started_turn_id = Arc::new(Mutex::new(None));
        (
            Self {
                turn: AsyncTurn::UserShell(input),
                events: Arc::clone(&events),
                started_turn_id: Arc::clone(&started_turn_id),
            },
            events,
            started_turn_id,
        )
    }
}

#[derive(Clone, Copy)]
enum EventMailboxStream {
    Provider,
    Agent,
}

impl EventMailboxStream {
    const fn name(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::Agent => "agent",
        }
    }

    const fn max_events(self) -> usize {
        match self {
            Self::Provider => MAX_ASYNC_PROVIDER_EVENTS,
            Self::Agent => MAX_ASYNC_AGENT_EVENTS,
        }
    }

    const fn max_bytes(self) -> usize {
        match self {
            Self::Provider => MAX_ASYNC_PROVIDER_BYTES,
            Self::Agent => MAX_ASYNC_AGENT_BYTES,
        }
    }
}

fn write_dropped_event<W: Write>(
    output: &mut W,
    sequence: &mut u64,
    replay: &mut ReplayState,
    request_id: Option<String>,
    turn_id: Option<String>,
    stream: EventMailboxStream,
    dropped: DroppedEvents,
) -> Result<(), HeadlessError> {
    if dropped.is_empty() {
        return Ok(());
    }
    let value = serde_json::json!({
        "type": "event_dropped",
        "stream": stream.name(),
        "count": dropped.count,
        "bytes": dropped.bytes,
        "truncated": true,
        "limit_events": stream.max_events(),
        "limit_bytes": stream.max_bytes(),
    });
    let envelope = StdioEvent::new(*sequence, request_id, turn_id, value);
    let current = *sequence;
    *sequence = sequence.saturating_add(1);
    let line = encode_line(&envelope)?;
    replay.remember_event(current, line.clone())?;
    output.write_all(line.as_bytes())?;
    Ok(())
}

fn drain_provider_events<W: Write>(
    jobs: &mut HashMap<crate::runtime::JobId, AsyncWork>,
    output: &mut W,
    sequence: &mut u64,
    replay: &mut ReplayState,
) -> Result<(), HeadlessError> {
    for work in jobs.values() {
        // `ProviderEvent` deliberately contains no turn ID.  The async
        // request owns the correlation captured from `TurnSubmission`, so
        // read it from `AsyncWork` rather than probing the event JSON.
        let work_turn_id = work
            .turn_id
            .lock()
            .map_err(|_| event_mailbox_headless_error())?
            .clone();
        // Snapshot the mailbox before doing JSON encoding or writing to the
        // client. A slow stdout consumer must not hold the request lock and
        // stall the provider worker (or a future lifecycle callback).
        let (agent_events, (provider_events, dropped)) = match work.events.lock() {
            Ok(mut events) => {
                // Admission markers must be observed before any provider delta
                // for the same request. `Agent::submit` publishes them before
                // it enters the provider, and the per-request buffer preserves
                // that relationship.
                (events.take_admission(), events.take_provider())
            }
            Err(_) => return Err(event_mailbox_headless_error()),
        };
        write_buffered_events(
            output,
            sequence,
            work.id.clone(),
            agent_events,
            Some(replay),
        )?;
        for event in provider_events {
            let value = serde_json::to_value(event)?;
            let envelope = StdioEvent::new(*sequence, work.id.clone(), work_turn_id.clone(), value);
            let current = *sequence;
            *sequence = sequence.saturating_add(1);
            let line = encode_line(&envelope)?;
            replay.remember_event(current, line.clone())?;
            output.write_all(line.as_bytes())?;
        }
        write_dropped_event(
            output,
            sequence,
            replay,
            work.id.clone(),
            work_turn_id,
            EventMailboxStream::Provider,
            dropped,
        )?;
    }
    output.flush()?;
    Ok(())
}

fn drain_approval_events<W: Write>(
    approval: Option<&crate::approval::ApprovalCoordinator>,
    jobs: &HashMap<crate::runtime::JobId, AsyncWork>,
    output: &mut W,
    sequence: &mut u64,
    replay: &mut ReplayState,
) -> Result<(), HeadlessError> {
    let Some(approval) = approval else {
        return Ok(());
    };
    for request in approval.drain_pending() {
        // ApprovalRequest carries the durable turn ID. Match it to the
        // corresponding AsyncWork rather than relying on HashMap iteration;
        // queued requests may coexist with the active tool turn.
        let mut request_id = None;
        for work in jobs
            .values()
            .filter(|work| work.command == "prompt" || work.command == "steer")
        {
            let work_turn_id = work
                .turn_id
                .lock()
                .map_err(|_| event_mailbox_headless_error())?;
            if work_turn_id.as_deref() == Some(request.turn_id.as_str()) {
                request_id = work.id.clone();
                break;
            }
        }
        let turn_id = Some(request.turn_id.clone());
        let value = serde_json::json!({
            "type": "approval_request",
            "approval": request,
        });
        let envelope = StdioEvent::new(*sequence, request_id, turn_id, value);
        let current = *sequence;
        *sequence = sequence.saturating_add(1);
        let line = encode_line(&envelope)?;
        replay.remember_event(current, line.clone())?;
        output.write_all(line.as_bytes())?;
    }
    output.flush()?;
    Ok(())
}

fn drain_provider_events_for_job<W: Write>(
    work: &AsyncWork,
    output: &mut W,
    sequence: &mut u64,
    replay: &mut ReplayState,
) -> Result<(), HeadlessError> {
    // Raw provider events have no turn_id field; correlation is carried by
    // the request metadata captured when the submission was accepted.
    let work_turn_id = work
        .turn_id
        .lock()
        .map_err(|_| event_mailbox_headless_error())?
        .clone();
    // Detach all buffered data before serializing or writing. Holding this
    // mutex across client I/O would let a slow consumer pause the worker.
    let (admission_events, (provider_events, dropped)) = match work.events.lock() {
        Ok(mut events) => (events.take_admission(), events.take_provider()),
        Err(_) => return Err(event_mailbox_headless_error()),
    };
    write_buffered_events(
        output,
        sequence,
        work.id.clone(),
        admission_events,
        Some(replay),
    )?;
    for event in provider_events {
        let value = serde_json::to_value(event)?;
        let envelope = StdioEvent::new(*sequence, work.id.clone(), work_turn_id.clone(), value);
        let current = *sequence;
        *sequence = sequence.saturating_add(1);
        let line = encode_line(&envelope)?;
        replay.remember_event(current, line.clone())?;
        output.write_all(line.as_bytes())?;
    }
    write_dropped_event(
        output,
        sequence,
        replay,
        work.id.clone(),
        work_turn_id,
        EventMailboxStream::Provider,
        dropped,
    )?;
    Ok(())
}

/// Project one scheduler transition onto the public JSONL event stream.
///
/// Provider and agent events describe work *inside* a turn, while the runtime
/// owns admission, queueing, and cancellation.  Dropping the latter made an
/// async client guess whether a request was queued, running, or had actually
/// observed its cancel command.  Keep the payload deliberately small and
/// replay it through the same bounded sequence ledger as every other event.
fn write_runtime_lifecycle_event<W: Write>(
    output: &mut W,
    sequence: &mut u64,
    replay: &mut ReplayState,
    work: &AsyncWork,
    event: serde_json::Value,
) -> Result<(), HeadlessError> {
    let turn_id = work
        .turn_id
        .lock()
        .map_err(|_| event_mailbox_headless_error())?
        .clone();
    let envelope = StdioEvent::new(*sequence, work.id.clone(), turn_id, event);
    let current = *sequence;
    *sequence = sequence.saturating_add(1);
    let line = encode_line(&envelope)?;
    replay.remember_event(current, line.clone())?;
    output.write_all(line.as_bytes())?;
    output.flush()?;
    Ok(())
}

enum ReaderMessage {
    Line(String),
    TooLong,
    InvalidUtf8,
    Eof,
    Error(io::Error),
}

fn run_async_stdio<R: io::Read + Send + 'static, W: Write>(
    agent: Agent,
    input: R,
    mut output: W,
) -> Result<(), HeadlessError> {
    let mut replay = ReplayState::open(agent.session())?;
    let mut event_sequence = replay.next_sequence;
    let (line_tx, line_rx) = mpsc::sync_channel::<ReaderMessage>(32);
    thread::Builder::new()
        .name("zenpi-stdin-reader".into())
        .spawn(move || {
            let mut reader = io::BufReader::new(input);
            let mut frame = Vec::with_capacity(crate::protocol::MAX_LINE_BYTES.min(8 * 1024));
            loop {
                match read_frame(&mut reader, &mut frame) {
                    Ok(Frame::Eof) => {
                        let _ = line_tx.send(ReaderMessage::Eof);
                        break;
                    }
                    Ok(Frame::TooLong) => {
                        if line_tx.send(ReaderMessage::TooLong).is_err() {
                            break;
                        }
                    }
                    Ok(Frame::Data) => {
                        let message = match std::str::from_utf8(&frame) {
                            Ok(value) => ReaderMessage::Line(value.to_owned()),
                            Err(_) => ReaderMessage::InvalidUtf8,
                        };
                        if line_tx.send(message).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = line_tx.send(ReaderMessage::Error(error));
                        break;
                    }
                }
            }
        })
        .map_err(|error| HeadlessError::Io(io::Error::other(error)))?;

    let shared = Arc::new(Mutex::new(agent));
    let approval = shared
        .lock()
        .ok()
        .and_then(|agent| agent.approval_coordinator());
    let worker_state = Arc::clone(&shared);
    let runner = crate::runtime::BackgroundRunner::spawn(
        move |request: AsyncRequest, token| -> Result<AsyncProcessResult, AgentError> {
            if token.is_cancelled() {
                return Err(AgentError::Backend(crate::backend::BackendError::Cancelled));
            }
            let mut agent = worker_state
                .lock()
                .map_err(|_| AgentError::InvalidTurn("agent lock poisoned".into()))?;
            let events = Arc::clone(&request.events);
            let started_turn_id = Arc::clone(&request.started_turn_id);
            let mut sink = |event: ProviderEvent| {
                let mut pending = events.lock().map_err(|_| event_mailbox_backend_error())?;
                pending.push_provider(event);
                Ok(())
            };
            // Keep the agent lock until its events are copied into this
            // request's buffer.  A queued replacement may start before the
            // host consumes the previous runtime completion; draining the
            // global agent queue later would mis-correlate those events.
            let result = (|| -> Result<AsyncProcessResult, AgentError> {
                match request.turn {
                    AsyncTurn::Standard(request) => {
                        let submission = agent.submit(request)?;
                        let mut active = started_turn_id
                            .lock()
                            .map_err(|_| event_mailbox_agent_error())?;
                        *active = submission.turn_id().map(str::to_owned);
                        drop(active);
                        {
                            let mut pending =
                                events.lock().map_err(|_| event_mailbox_agent_error())?;
                            for event in agent.take_events() {
                                pending.push_agent(event);
                            }
                        }
                        let assistant = if submission.accepted() {
                            agent.run_active_turn_cancelable_with_events(
                                || token.is_cancelled(),
                                &mut sink,
                            )?
                        } else {
                            None
                        };
                        Ok(AsyncProcessResult::Turn(ProcessResult {
                            submission,
                            assistant,
                        }))
                    }
                    AsyncTurn::Reissue {
                        message,
                        superseded_turn_id,
                    } => {
                        let submission = agent.start_steer_reissue(message, &superseded_turn_id)?;
                        let mut active = started_turn_id
                            .lock()
                            .map_err(|_| event_mailbox_agent_error())?;
                        *active = submission.turn_id().map(str::to_owned);
                        drop(active);
                        {
                            let mut pending =
                                events.lock().map_err(|_| event_mailbox_agent_error())?;
                            for event in agent.take_events() {
                                pending.push_agent(event);
                            }
                        }
                        let assistant = agent.run_active_turn_cancelable_with_events(
                            || token.is_cancelled(),
                            &mut sink,
                        )?;
                        Ok(AsyncProcessResult::Turn(ProcessResult {
                            submission,
                            assistant,
                        }))
                    }
                    AsyncTurn::UserShell(input) => {
                        let result =
                            agent.run_user_shell_with_cancel(&input, || token.is_cancelled())?;
                        Ok(AsyncProcessResult::UserShell(result))
                    }
                }
            })();
            {
                let mut pending = events.lock().map_err(|_| event_mailbox_agent_error())?;
                for event in agent.take_events() {
                    pending.push_agent(event);
                }
            }
            let result = result?;
            // `Agent::run_active_turn_cancelable_with_events` has already
            // crossed its durable completion boundary when it returns a
            // successful ProcessResult. Publish that boundary before handing
            // control back to the runtime: a late steer/shutdown may race
            // this tiny gap, but must not relabel a persisted answer as a
            // cancellation (or leave the journal and wire result divergent).
            token.mark_completed();
            Ok(result)
        },
        crate::runtime::RuntimeConfig::default(),
    );
    let mut jobs: HashMap<crate::runtime::JobId, AsyncWork> = HashMap::new();
    let mut request_to_job: HashMap<String, crate::runtime::JobId> = HashMap::new();
    let mut pending_steers = VecDeque::new();
    let mut stopping = false;
    let mut explicit_shutdown = false;
    let mut stopping_since: Option<Instant> = None;
    let mut shutdown_sent = false;
    let mut pending_shutdown: Option<(Option<String>, String, u16)> = None;
    let loop_result = (|| -> Result<(), HeadlessError> {
        loop {
            drain_provider_events(&mut jobs, &mut output, &mut event_sequence, &mut replay)?;
            drain_approval_events(
                approval.as_ref(),
                &jobs,
                &mut output,
                &mut event_sequence,
                &mut replay,
            )?;
            // EOF is the normal one-shot drain boundary, but there is no
            // client left to answer a side-effect approval. Fail closed and
            // wake the worker instead of hanging the pipeline forever. The
            // core turns the denial into a durable tool result and may still
            // let the provider return a useful final explanation.
            if stopping
                && !explicit_shutdown
                && let Some(approval) = approval.as_ref()
            {
                approval.cancel_all();
            }
            while let Ok(event) = runner.try_next_event() {
                if handle_runtime_event(
                    event,
                    &mut jobs,
                    &mut request_to_job,
                    &mut output,
                    &mut event_sequence,
                    &mut replay,
                    &mut pending_steers,
                    &mut pending_shutdown,
                    stopping,
                    shutdown_sent,
                )? {
                    return Ok(());
                }
            }
            // A shutdown request is a stop boundary for *new* input, but a
            // steer already admitted before that boundary still owns a
            // terminal response. Give it a bounded window to complete its
            // cancel/reissue promotion; once the grace period expires,
            // `promote_pending_steers` reports an explicit runtime_closed
            // error instead of silently dropping it.
            let steer_grace_elapsed = explicit_shutdown
                && stopping_since.is_some_and(|started| started.elapsed() >= SHUTDOWN_GRACE);
            promote_pending_steers(
                &runner,
                &mut jobs,
                &mut request_to_job,
                &mut pending_steers,
                &mut output,
                &mut replay,
                stopping && steer_grace_elapsed,
            )?;
            if stopping {
                drain_provider_events(&mut jobs, &mut output, &mut event_sequence, &mut replay)?;
                // Shutdown is the cancellation boundary.  Sending it only
                // after `jobs` became empty would strand an active provider
                // forever: jobs can become empty only after the worker has
                // observed shutdown and emitted their terminal events.
                let grace_elapsed = explicit_shutdown
                    && stopping_since.is_some_and(|started| started.elapsed() >= SHUTDOWN_GRACE);
                if !shutdown_sent
                    && ((jobs.is_empty() && pending_steers.is_empty()) || grace_elapsed)
                {
                    match runner.try_shutdown() {
                        Ok(()) | Err(crate::runtime::SubmitError::Closed) => {
                            shutdown_sent = true;
                        }
                        Err(crate::runtime::SubmitError::QueueFull) => {
                            // Retry after draining one event below; this also
                            // keeps a saturated bounded command/event pair
                            // from deadlocking shutdown.
                        }
                    }
                }
                match runner.recv_timeout(Duration::from_millis(10)) {
                    Ok(event) => {
                        if handle_runtime_event(
                            event,
                            &mut jobs,
                            &mut request_to_job,
                            &mut output,
                            &mut event_sequence,
                            &mut replay,
                            &mut pending_steers,
                            &mut pending_shutdown,
                            true,
                            shutdown_sent,
                        )? {
                            return Ok(());
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
                continue;
            }
            match line_rx.recv_timeout(Duration::from_millis(10)) {
                Ok(ReaderMessage::Line(line)) => {
                    process_async_line(
                        line,
                        &shared,
                        approval.as_ref(),
                        &runner,
                        &mut jobs,
                        &mut request_to_job,
                        &mut pending_steers,
                        &mut pending_shutdown,
                        &mut output,
                        &mut event_sequence,
                        &mut replay,
                        &mut stopping,
                    )?;
                    if stopping {
                        explicit_shutdown = pending_shutdown.is_some();
                        stopping_since.get_or_insert_with(Instant::now);
                    }
                    let steer_grace_elapsed = explicit_shutdown
                        && stopping_since
                            .is_some_and(|started| started.elapsed() >= SHUTDOWN_GRACE);
                    promote_pending_steers(
                        &runner,
                        &mut jobs,
                        &mut request_to_job,
                        &mut pending_steers,
                        &mut output,
                        &mut replay,
                        stopping && steer_grace_elapsed,
                    )?;
                }
                Ok(ReaderMessage::TooLong) => {
                    write_response(
                        &mut output,
                        StdioResponse::error_with_code(
                            None,
                            "invalid",
                            "line_too_long",
                            "input frame exceeds maximum size",
                        ),
                    )?;
                }
                Ok(ReaderMessage::InvalidUtf8) => {
                    write_response(
                        &mut output,
                        StdioResponse::error_with_code(
                            None,
                            "invalid",
                            "invalid_utf8",
                            "input frame is not valid UTF-8",
                        ),
                    )?;
                }
                Ok(ReaderMessage::Eof) => {
                    stopping = true;
                    stopping_since.get_or_insert_with(Instant::now);
                }
                Ok(ReaderMessage::Error(error)) => {
                    let (code, message) = if error.kind() == io::ErrorKind::InvalidData {
                        ("invalid_utf8", "input frame is not valid UTF-8".to_owned())
                    } else {
                        ("io_error", error.to_string())
                    };
                    write_response(
                        &mut output,
                        StdioResponse::error_with_code(None, "stdio", code, message),
                    )?;
                    stopping = true;
                    stopping_since.get_or_insert_with(Instant::now);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    stopping = true;
                    stopping_since.get_or_insert_with(Instant::now);
                }
            }
        }
    })();
    // Every exit path, including a write/read error, must join the runtime.
    // `shutdown_and_join` is deliberately able to drain a saturated event
    // queue and to handle a previously consumed `Closed` event.
    let join_result = runner.shutdown_and_join();
    // The owned runner holds the agent only while a job is executing. Once it
    // has joined, close the shared agent explicitly so session-close hooks and
    // final journal markers are not lost when stdin reaches EOF or shutdown.
    if let Ok(mut agent) = shared.lock() {
        agent.close();
    }
    match loop_result {
        Err(error) => {
            let _ = join_result;
            Err(error)
        }
        Ok(()) => {
            join_result.map_err(|_| HeadlessError::Io(io::Error::other("runtime worker panicked")))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_runtime_event<W: Write>(
    event: crate::runtime::RuntimeEvent<AsyncProcessResult, AgentError>,
    jobs: &mut HashMap<crate::runtime::JobId, AsyncWork>,
    request_to_job: &mut HashMap<String, crate::runtime::JobId>,
    output: &mut W,
    event_sequence: &mut u64,
    replay: &mut ReplayState,
    pending_steers: &mut VecDeque<PendingSteer>,
    pending_shutdown: &mut Option<(Option<String>, String, u16)>,
    stopping: bool,
    shutdown_sent: bool,
) -> Result<bool, HeadlessError> {
    use crate::runtime::{JobOutcome, RuntimeEvent};
    match event {
        RuntimeEvent::Accepted { id, queued } => {
            let Some(work) = jobs.get(&id) else {
                return Ok(false);
            };
            write_runtime_lifecycle_event(
                output,
                event_sequence,
                replay,
                work,
                json!({
                    "type": "request_accepted",
                    "runtime_job_id": id.get(),
                    "queued": queued,
                }),
            )?;
            Ok(false)
        }
        RuntimeEvent::Started { id } => {
            let Some(work) = jobs.get(&id) else {
                return Ok(false);
            };
            write_runtime_lifecycle_event(
                output,
                event_sequence,
                replay,
                work,
                json!({
                    "type": "request_started",
                    "runtime_job_id": id.get(),
                }),
            )?;
            Ok(false)
        }
        RuntimeEvent::Queued { id, depth } => {
            let Some(work) = jobs.get(&id) else {
                return Ok(false);
            };
            write_runtime_lifecycle_event(
                output,
                event_sequence,
                replay,
                work,
                json!({
                    "type": "request_queued",
                    "runtime_job_id": id.get(),
                    "depth": depth,
                }),
            )?;
            Ok(false)
        }
        RuntimeEvent::CancelRequested { id } => {
            let Some(work) = jobs.get(&id) else {
                return Ok(false);
            };
            write_runtime_lifecycle_event(
                output,
                event_sequence,
                replay,
                work,
                json!({
                    "type": "cancel_requested",
                    "runtime_job_id": id.get(),
                }),
            )?;
            Ok(false)
        }
        RuntimeEvent::Completed { id, outcome } => {
            let Some(meta) = jobs.get(&id) else {
                return Ok(false);
            };
            // Preserve the turn ID for a steer that was received before the
            // worker could publish it.  The completed job is removed below,
            // so this is the last reliable place to capture the correlation.
            let completed_turn_id = meta
                .turn_id
                .lock()
                .map_err(|_| event_mailbox_headless_error())?
                .clone();
            for pending in pending_steers.iter_mut() {
                if pending.target_job == id && pending.superseded_turn_id.is_none() {
                    pending.superseded_turn_id = completed_turn_id.clone();
                }
            }
            let Some(meta) = jobs.remove(&id) else {
                // The host may have discarded a job after observing a
                // terminal event; never turn that benign race into a panic.
                return Ok(false);
            };
            let event_request_id = meta.id.clone();
            if let Some(request_id) = meta.id.as_ref() {
                request_to_job.remove(request_id);
            }
            // Snapshot all lifecycle events before writing the terminal
            // response.  A client must be able to treat that response as a
            // strict completion boundary: once it is observed, no progress
            // event for the same request may still be waiting behind it.
            let (agent_events, agent_dropped) = meta
                .events
                .lock()
                .map_err(|_| event_mailbox_headless_error())
                .map(|mut events| events.take_agent())?;
            let mut agent_events = agent_events;
            let admission_end = admission_prefix_len(&agent_events).unwrap_or(0);
            let admission_events = agent_events.drain(..admission_end).collect::<Vec<_>>();
            // Provider deltas and lifecycle markers precede the one terminal
            // response, even when the worker and output loop become ready on
            // the same tick.  The two buffers are drained here (rather than
            // from the global Agent) so queued jobs cannot be mis-correlated.
            write_buffered_events(
                output,
                event_sequence,
                event_request_id.clone(),
                admission_events,
                Some(replay),
            )?;
            drain_provider_events_for_job(&meta, output, event_sequence, replay)?;
            write_buffered_events(
                output,
                event_sequence,
                event_request_id.clone(),
                agent_events,
                Some(replay),
            )?;
            write_dropped_event(
                output,
                event_sequence,
                replay,
                event_request_id,
                completed_turn_id,
                EventMailboxStream::Agent,
                agent_dropped,
            )?;
            match outcome {
                JobOutcome::Succeeded(result) => {
                    let data = match result {
                        AsyncProcessResult::Turn(result) => json!({
                            "submission": result.submission,
                            "assistant": result.assistant,
                        }),
                        AsyncProcessResult::UserShell(result) => result,
                    };
                    write_cached_response(
                        output,
                        StdioResponse::success(meta.id.clone(), meta.command, Some(data))
                            .for_version(meta.version),
                        replay,
                    )?;
                }
                JobOutcome::Failed(error) => write_cached_response(
                    output,
                    StdioResponse::error_with_code(
                        meta.id.clone(),
                        meta.command,
                        error.code(),
                        error.to_string(),
                    )
                    .for_version(meta.version),
                    replay,
                )?,
                JobOutcome::Cancelled => write_cached_response(
                    output,
                    StdioResponse::error_with_code(
                        meta.id.clone(),
                        meta.command,
                        "backend_cancelled",
                        "request was cancelled",
                    )
                    .for_version(meta.version),
                    replay,
                )?,
                JobOutcome::Panicked => write_cached_response(
                    output,
                    StdioResponse::error_with_code(
                        meta.id.clone(),
                        meta.command,
                        "runtime_panic",
                        "background request panicked",
                    )
                    .for_version(meta.version),
                    replay,
                )?,
            }
            Ok(false)
        }
        RuntimeEvent::Rejected { id, reason } => {
            let Some(meta) = jobs.remove(&id) else {
                return Ok(false);
            };
            if let Some(request_id) = meta.id.as_ref() {
                request_to_job.remove(request_id);
            }
            let (code, message) = match reason {
                crate::runtime::SubmitError::QueueFull => (
                    "runtime_queue_full",
                    "runtime rejected the request because its pending queue is full",
                ),
                crate::runtime::SubmitError::Closed => (
                    "runtime_closed",
                    "runtime rejected the request while shutting down",
                ),
            };
            let response =
                StdioResponse::error_with_code(meta.id.clone(), meta.command, code, message)
                    .for_version(meta.version);
            if reason == crate::runtime::SubmitError::QueueFull {
                replay.release(meta.id.as_deref())?;
                write_response(output, response)?;
            } else {
                write_cached_response(output, response, replay)?;
            }
            Ok(false)
        }
        RuntimeEvent::Closed => {
            if let Some((id, command, version)) = pending_shutdown.take() {
                write_cached_versioned_response(
                    output,
                    StdioResponse::success(
                        id,
                        command,
                        Some(json!({"closing": true, "drained": true})),
                    ),
                    version,
                    replay,
                )?;
            }
            Ok(stopping && shutdown_sent && jobs.is_empty())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn process_async_line<W, F>(
    line: String,
    shared: &Arc<Mutex<Agent>>,
    approval: Option<&crate::approval::ApprovalCoordinator>,
    runner: &crate::runtime::BackgroundRunner<AsyncRequest, AsyncProcessResult, AgentError, F>,
    jobs: &mut HashMap<crate::runtime::JobId, AsyncWork>,
    request_to_job: &mut HashMap<String, crate::runtime::JobId>,
    pending_steers: &mut VecDeque<PendingSteer>,
    pending_shutdown: &mut Option<(Option<String>, String, u16)>,
    output: &mut W,
    event_sequence: &mut u64,
    replay: &mut ReplayState,
    stopping: &mut bool,
) -> Result<(), HeadlessError>
where
    W: Write,
    F: Fn(
            AsyncRequest,
            crate::runtime::CancellationToken,
        ) -> Result<AsyncProcessResult, AgentError>
        + Send
        + Sync
        + 'static,
{
    if line.trim().is_empty() {
        return Ok(());
    }
    let request = match parse_line(line.trim_end_matches('\r')) {
        Ok(request) => request,
        Err(error) => {
            write_response(
                output,
                StdioResponse::error_with_code(None, "invalid", error.code(), error.to_string()),
            )?;
            return Ok(());
        }
    };
    let id = request.id().map(str::to_owned);
    let name = request.kind.clone();
    let request_version = request.schema_version;
    let fingerprint = request_fingerprint(&request)?;
    // Validate before consulting the cache. A reused ID must not let an
    // unsupported schema or malformed payload silently replay an old result.
    let parsed_command = request.clone().into_command();
    match replay.admit(id.as_deref(), &fingerprint)? {
        RequestIdAdmission::Replay => {
            if let Some(request_id) = id.as_deref()
                && let Some(cached) = replay.cached_line(request_id)
            {
                output.write_all(cached.as_bytes())?;
                output.flush()?;
                return Ok(());
            }
        }
        RequestIdAdmission::Conflict => {
            write_response(
                output,
                StdioResponse::error_with_code(
                    id,
                    name,
                    "request_id_conflict",
                    "request ID was already used for a different request",
                )
                .for_version(request_version),
            )?;
            return Ok(());
        }
        RequestIdAdmission::InFlightSame => {
            write_response(
                output,
                StdioResponse::error_with_code(
                    id,
                    name,
                    "duplicate_request_in_flight",
                    "request ID is already in flight",
                )
                .for_version(request_version),
            )?;
            return Ok(());
        }
        RequestIdAdmission::New => {}
        admission => {
            write_response(
                output,
                admission_error(id, name, admission).for_version(request_version),
            )?;
            return Ok(());
        }
    }
    let command = match parsed_command {
        Ok(command) => command,
        Err(error) => {
            write_cached_response(
                output,
                StdioResponse::error_with_code(id, name, error.code(), error.to_string())
                    .for_version(request_version),
                replay,
            )?;
            return Ok(());
        }
    };
    match command {
        Command::Slash { input } => {
            let parsed = match crate::slash::route_input(&input) {
                Ok(crate::slash::InputRoute::Slash(command)) => command,
                Ok(crate::slash::InputRoute::Prompt(_)) => {
                    write_cached_response(
                        output,
                        StdioResponse::error_with_code(
                            id,
                            name,
                            "invalid_command",
                            "command input must begin with `/`",
                        )
                        .for_version(request_version),
                        replay,
                    )?;
                    return Ok(());
                }
                Ok(crate::slash::InputRoute::UserShell(input)) => {
                    let (request, events, started_turn_id) = AsyncRequest::user_shell(input);
                    let job_id = match runner.try_submit(request) {
                        Ok(job_id) => job_id,
                        Err(error) => {
                            write_runtime_rejection(
                                output,
                                id,
                                name.as_str(),
                                error,
                                request_version,
                                replay,
                            )?;
                            return Ok(());
                        }
                    };
                    jobs.insert(
                        job_id,
                        AsyncWork {
                            id: id.clone(),
                            command: "user_shell",
                            version: request_version,
                            events,
                            turn_id: started_turn_id,
                        },
                    );
                    if let Some(request_id) = id {
                        request_to_job.insert(request_id, job_id);
                    }
                    return Ok(());
                }
                Err(error) => {
                    let special = unsupported_session_maintenance_parse_error(&error);
                    let code = special
                        .as_ref()
                        .map_or("command_error", |_| "unsupported_command");
                    let message = special.unwrap_or_else(|| error.to_string());
                    write_cached_response(
                        output,
                        StdioResponse::error_with_code(id, name, code, message)
                            .for_version(request_version),
                        replay,
                    )?;
                    return Ok(());
                }
            };
            let command_name = parsed.name();
            if let crate::slash::SlashCommand::Approve {
                id: approval_id,
                decision,
            } = parsed
            {
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
                let Some(approval) = approval else {
                    write_cached_versioned_response(
                        output,
                        StdioResponse::error_with_code(
                            id,
                            command_name,
                            "approval_unavailable",
                            "no tool registry or approval coordinator is configured",
                        ),
                        request_version,
                        replay,
                    )?;
                    return Ok(());
                };
                match approval.respond_with_request(crate::approval::ApprovalResponse {
                    request_id: approval_id.clone(),
                    decision,
                    remember,
                }) {
                    Ok(request) => write_cached_versioned_response(
                        output,
                        StdioResponse::success(
                            id,
                            command_name,
                            Some(json!({
                                "command": "approve",
                                "route": "local",
                                "accepted": true,
                                "approval_id": approval_id,
                                "policy_digest": request.policy_digest,
                                "lease_id": request.lease_id,
                                "decision": decision,
                                "remember": remember,
                            })),
                        ),
                        request_version,
                        replay,
                    )?,
                    Err(error) => write_cached_versioned_response(
                        output,
                        StdioResponse::error_with_code(
                            id,
                            command_name,
                            "approval_error",
                            error.to_string(),
                        ),
                        request_version,
                        replay,
                    )?,
                }
                return Ok(());
            }
            let session_open = matches!(
                &parsed,
                crate::slash::SlashCommand::Session {
                    action: crate::slash::SessionAction::Open { .. }
                }
            );
            if session_open && (!jobs.is_empty() || !pending_steers.is_empty()) {
                write_retryable_response(
                    output,
                    StdioResponse::error_with_code(
                        id,
                        command_name,
                        "agent_busy",
                        "session cannot be opened while requests are queued or running",
                    )
                    .for_version(request_version),
                    replay,
                )?;
                return Ok(());
            }
            let durable_fingerprint = slash_runtime_fingerprint(&parsed)?;
            let runtime_source = id.as_deref().and_then(|request_id| {
                durable_fingerprint.as_deref().map(|fingerprint| {
                    crate::runtime_intent::RuntimeIntentSource {
                        request_id,
                        fingerprint,
                    }
                })
            });
            let execution = match shared.try_lock() {
                Ok(mut agent) => execute_headless_slash(Some(&mut agent), parsed, runtime_source),
                Err(_) => execute_headless_slash(None, parsed, runtime_source),
            };
            match execution {
                Ok(SlashExecution::Response(data)) => {
                    let response =
                        slash_execution_response(id.clone(), command_name, data, request_version);
                    if session_open && response.success {
                        request_to_job.clear();
                        let agent = shared.lock().map_err(|_| event_mailbox_headless_error())?;
                        replay.reset_for_session(
                            agent.session(),
                            id.as_deref(),
                            *event_sequence,
                        )?;
                        *event_sequence = (*event_sequence).max(replay.next_sequence);
                    }
                    write_cached_response(output, response, replay)?;
                }
                Ok(SlashExecution::Cancel) => {
                    let target = jobs
                        .iter()
                        // `jobs` also retains queued follow-ups. Job IDs are
                        // monotonic, so the smallest live prompt/steer is the
                        // runtime's active request; HashMap iteration order
                        // must never decide which request `/cancel` stops.
                        .filter(|(_, work)| work.command == "prompt" || work.command == "steer")
                        .min_by_key(|(job_id, _)| job_id.get())
                        .map(|(job_id, _)| *job_id);
                    if let Some(target) = target {
                        match runner.try_cancel(target) {
                            Ok(()) => {
                                if let Some(approval) = approval {
                                    approval.emergency_cancel();
                                }
                                let _ = cancel_pending_steers_for_job(
                                    pending_steers,
                                    target,
                                    output,
                                    replay,
                                )?;
                                write_cached_response(
                                    output,
                                    StdioResponse::success(
                                        id,
                                        command_name,
                                        Some(json!({"cancel_requested": true})),
                                    )
                                    .for_version(request_version),
                                    replay,
                                )?
                            }
                            Err(error) => {
                                let (code, message) = runtime_error_details(error);
                                write_retryable_response(
                                    output,
                                    StdioResponse::error_with_code(id, command_name, code, message)
                                        .for_version(request_version),
                                    replay,
                                )?
                            }
                        }
                    } else {
                        write_cached_versioned_response(
                            output,
                            StdioResponse::error_with_code(
                                id,
                                command_name,
                                "unknown_request",
                                "no active request to cancel",
                            ),
                            request_version,
                            replay,
                        )?;
                    }
                }
                Ok(SlashExecution::Exit) => {
                    // The shutdown acknowledgement is emitted after the
                    // outer loop has drained admitted work. Emitting it here
                    // would let a client close the pipe before the prompt's
                    // terminal response arrives.
                    pending_shutdown.replace((id, command_name.to_owned(), request_version));
                    *stopping = true;
                }
                Err(error) => {
                    let response =
                        StdioResponse::error_with_code(id, command_name, error.code, error.message)
                            .for_version(request_version);
                    if error.code == "agent_busy" {
                        write_retryable_response(output, response, replay)?;
                    } else {
                        write_cached_response(output, response, replay)?;
                    }
                }
            }
        }
        Command::Resources { path } => match collect_resource_snapshot(path.as_deref()) {
            Ok(snapshot) => write_cached_versioned_response(
                output,
                StdioResponse::success(id, name, Some(serialized_resource_snapshot(snapshot)?)),
                request_version,
                replay,
            )?,
            Err(error) => write_cached_versioned_response(
                output,
                StdioResponse::error_with_code(id, name, "resource_error", error.to_string()),
                request_version,
                replay,
            )?,
        },
        Command::Prompt {
            text,
            mode,
            expected_turn_id,
            attachments,
        } => {
            if request_in_flight(id.as_deref(), request_to_job, pending_steers) {
                write_response(
                    output,
                    StdioResponse::error_with_code(
                        id,
                        name,
                        "duplicate_request_in_flight",
                        "request ID is already in flight",
                    )
                    .for_version(request_version),
                )?;
                return Ok(());
            }
            let request = TurnInputRequest {
                message: text,
                expected_turn_id,
                mode,
                attachments,
            };
            let (request, events, started_turn_id) = AsyncRequest::new(request);
            let job_id = match runner.try_submit(request) {
                Ok(job_id) => job_id,
                Err(error) => {
                    write_runtime_rejection(
                        output,
                        id,
                        name.as_str(),
                        error,
                        request_version,
                        replay,
                    )?;
                    return Ok(());
                }
            };
            jobs.insert(
                job_id,
                AsyncWork {
                    id: id.clone(),
                    command: "prompt",
                    version: request_version,
                    events,
                    turn_id: started_turn_id,
                },
            );
            if let Some(request_id) = id.clone() {
                request_to_job.insert(request_id, job_id);
            }
        }
        Command::Steer {
            text,
            expected_turn_id,
        } => {
            if request_in_flight(id.as_deref(), request_to_job, pending_steers) {
                write_response(
                    output,
                    StdioResponse::error_with_code(
                        id,
                        name,
                        "duplicate_request_in_flight",
                        "request ID is already in flight",
                    )
                    .for_version(request_version),
                )?;
                return Ok(());
            }
            let active = if let Some((job_id, work)) = jobs
                .iter()
                .filter(|(_, work)| work.command == "prompt" || work.command == "steer")
                .min_by_key(|(job_id, _)| job_id.get())
            {
                let active_turn_id = work
                    .turn_id
                    .lock()
                    .map_err(|_| event_mailbox_headless_error())?
                    .clone();
                Some((*job_id, active_turn_id))
            } else {
                None
            };
            if let Some((job_id, active_turn_id)) = active {
                if expected_turn_id.as_deref().is_some_and(|expected| {
                    active_turn_id
                        .as_deref()
                        .is_some_and(|active| expected != active)
                }) {
                    write_retryable_response(
                        output,
                        StdioResponse::error_with_code(
                            id,
                            name,
                            "expected_turn_mismatch",
                            "expected turn does not match the active request",
                        )
                        .for_version(request_version),
                        replay,
                    )?;
                    return Ok(());
                }
                // Defer cancellation and reissue until the original worker
                // has published its turn ID.  This closes the admission
                // race between `RuntimeEvent::Started` and `Agent::submit`.
                enqueue_pending_steer(
                    pending_steers,
                    PendingSteer {
                        id,
                        version: request_version,
                        text,
                        expected_turn_id,
                        target_job: job_id,
                        cancel_sent: false,
                        superseded_turn_id: active_turn_id,
                    },
                    output,
                    replay,
                )?;
                return Ok(());
            }
            {
                let request = TurnInputRequest {
                    message: text,
                    expected_turn_id,
                    mode: crate::protocol::TurnMode::Steer,
                    attachments: Vec::new(),
                };
                let (request, events, started_turn_id) = AsyncRequest::new(request);
                let job_id = match runner.try_submit(request) {
                    Ok(job_id) => job_id,
                    Err(error) => {
                        write_runtime_rejection(
                            output,
                            id,
                            name.as_str(),
                            error,
                            request_version,
                            replay,
                        )?;
                        return Ok(());
                    }
                };
                jobs.insert(
                    job_id,
                    AsyncWork {
                        id: id.clone(),
                        command: "steer",
                        version: request_version,
                        events,
                        turn_id: started_turn_id,
                    },
                );
                if let Some(request_id) = id {
                    request_to_job.insert(request_id, job_id);
                }
            }
        }
        Command::Cancel { target_id } => {
            if cancel_pending_steer_by_id(pending_steers, &target_id, output, replay)? {
                write_cached_versioned_response(
                    output,
                    StdioResponse::success(
                        id,
                        name,
                        Some(json!({
                            "target_id": target_id,
                            "cancel_requested": true,
                            "deferred": true,
                        })),
                    ),
                    request_version,
                    replay,
                )?;
                return Ok(());
            }
            let Some(job_id) = request_to_job.get(&target_id).copied() else {
                write_cached_versioned_response(
                    output,
                    StdioResponse::error_with_code(
                        id,
                        name,
                        "unknown_request",
                        "target request is not running",
                    ),
                    request_version,
                    replay,
                )?;
                return Ok(());
            };
            match runner.try_cancel(job_id) {
                Ok(()) => {
                    let _ = cancel_pending_steers_for_job(pending_steers, job_id, output, replay)?;
                    write_cached_versioned_response(
                        output,
                        StdioResponse::success(
                            id.clone(),
                            name,
                            Some(json!({ "target_id": target_id, "cancel_requested": true })),
                        ),
                        request_version,
                        replay,
                    )?
                }
                Err(error) => {
                    write_runtime_rejection(
                        output,
                        id,
                        name.as_str(),
                        error,
                        request_version,
                        replay,
                    )?;
                }
            }
        }
        Command::Checkpoint(ref request) => match shared.try_lock() {
            Ok(agent) => write_cached_versioned_response(
                output,
                checkpoint_response(&agent, request, id, name, replay)?,
                request_version,
                replay,
            )?,
            Err(_) => write_retryable_response(
                output,
                StdioResponse::error_with_code(
                    id,
                    name,
                    "agent_busy",
                    "checkpoint inspection requires the session owner",
                )
                .for_version(request_version),
                replay,
            )?,
        },
        Command::Mailbox(ref request) => match shared.try_lock() {
            Ok(agent) => write_cached_versioned_response(
                output,
                mailbox_response(&agent, request, id, name),
                request_version,
                replay,
            )?,
            Err(_) => write_retryable_response(
                output,
                StdioResponse::error_with_code(
                    id,
                    name,
                    "agent_busy",
                    "mailbox command requires the session owner",
                )
                .for_version(request_version),
                replay,
            )?,
        },
        Command::UserShell(_) => {
            let input = match command {
                Command::UserShell(request) => request.input,
                _ => unreachable!(),
            };
            if request_in_flight(id.as_deref(), request_to_job, pending_steers) {
                write_response(
                    output,
                    StdioResponse::error_with_code(
                        id,
                        name,
                        "duplicate_request_in_flight",
                        "request ID is already in flight",
                    )
                    .for_version(request_version),
                )?;
                return Ok(());
            }
            let (request, events, started_turn_id) = AsyncRequest::user_shell(input);
            let job_id = match runner.try_submit(request) {
                Ok(job_id) => job_id,
                Err(error) => {
                    write_runtime_rejection(
                        output,
                        id,
                        name.as_str(),
                        error,
                        request_version,
                        replay,
                    )?;
                    return Ok(());
                }
            };
            jobs.insert(
                job_id,
                AsyncWork {
                    id: id.clone(),
                    command: "user_shell",
                    version: request_version,
                    events,
                    turn_id: started_turn_id,
                },
            );
            if let Some(request_id) = id {
                request_to_job.insert(request_id, job_id);
            }
        }
        Command::Status => match shared.try_lock() {
            Ok(agent) => write_cached_versioned_response(
                output,
                StdioResponse::success(id, name, Some(serialized_agent_snapshot(&agent)?)),
                request_version,
                replay,
            )?,
            Err(_) => write_cached_versioned_response(
                output,
                StdioResponse::success(id, name, Some(json!({"phase": "running", "busy": true}))),
                request_version,
                replay,
            )?,
        },
        Command::Shutdown => {
            if let Some(approval) = approval {
                approval.emergency_cancel();
            }
            // Shutdown is an explicit cancellation boundary. Report that the
            // close was accepted; the outer loop then drains every admitted
            // job to a terminal response before the process exits.
            // Keep the acknowledgement until `RuntimeEvent::Closed` so a
            // client can safely treat it as proof that all prior requests
            // have reached their terminal response.
            pending_shutdown.replace((id, name, request_version));
            *stopping = true;
        }
        Command::Approve {
            approval_id,
            decision,
            remember,
        } => {
            let Some(approval) = approval else {
                write_cached_versioned_response(
                    output,
                    StdioResponse::error_with_code(
                        id,
                        name,
                        "approval_unavailable",
                        "no approval coordinator is configured",
                    ),
                    request_version,
                    replay,
                )?;
                return Ok(());
            };
            match approval.respond_with_request(crate::approval::ApprovalResponse {
                request_id: approval_id.clone(),
                decision,
                remember,
            }) {
                Ok(request) => write_cached_versioned_response(
                    output,
                    StdioResponse::success(
                        id,
                        name,
                        Some(
                            json!({"approval_id": approval_id, "accepted": true, "policy_digest": request.policy_digest, "lease_id": request.lease_id}),
                        ),
                    ),
                    request_version,
                    replay,
                )?,
                Err(error) => write_cached_versioned_response(
                    output,
                    StdioResponse::error_with_code(id, name, "approval_error", error.to_string()),
                    request_version,
                    replay,
                )?,
            }
        }
        Command::Resume {
            path: None,
            from_sequence: Some(from_sequence),
        } => {
            let report = replay.replay_from(from_sequence, output)?;
            write_response(
                output,
                StdioResponse::success(
                    id.clone(),
                    name,
                    Some(json!({
                        "from_sequence": from_sequence,
                        "replayed": report.replayed,
                        "replay_gap": report.gap,
                        "first_available_sequence": report.first_available,
                        "last_available_sequence": report.last_available,
                        "next_sequence": *event_sequence,
                    })),
                )
                .for_version(request_version),
            )?;
            // Resume is a read operation whose observable result includes
            // the event suffix written above. Do not cache only its summary
            // line: an identical retry must be able to request the suffix
            // again rather than receiving a misleading response-only replay.
            replay.release(id.as_deref())?;
        }
        Command::Resume {
            path: Some(path),
            from_sequence: None,
        } => {
            if !jobs.is_empty() || !pending_steers.is_empty() {
                write_retryable_response(
                    output,
                    StdioResponse::error_with_code(
                        id,
                        name,
                        "agent_busy",
                        "session cannot be switched while a turn is running",
                    )
                    .for_version(request_version),
                    replay,
                )?;
                return Ok(());
            }
            let Some(mut agent) = shared.try_lock().ok() else {
                write_retryable_response(
                    output,
                    StdioResponse::error_with_code(
                        id,
                        name,
                        "agent_busy",
                        "session cannot be switched while the agent is busy",
                    )
                    .for_version(request_version),
                    replay,
                )?;
                return Ok(());
            };
            let path = match validate_existing_session_path(&path) {
                Ok(path) => path,
                Err(message) => {
                    write_cached_versioned_response(
                        output,
                        StdioResponse::error_with_code(id, name, "session_path_denied", message)
                            .for_version(request_version),
                        request_version,
                        replay,
                    )?;
                    return Ok(());
                }
            };
            match agent.resume_session(path) {
                Ok(()) => {
                    agent.take_events();
                    request_to_job.clear();
                    replay.reset_for_session(agent.session(), id.as_deref(), *event_sequence)?;
                    *event_sequence = (*event_sequence).max(replay.next_sequence);
                    write_cached_versioned_response(
                        output,
                        StdioResponse::success(id, name, Some(serialized_agent_snapshot(&agent)?)),
                        request_version,
                        replay,
                    )?;
                }
                Err(error) => write_cached_versioned_response(
                    output,
                    StdioResponse::error_with_code(id, name, error.code(), error.to_string()),
                    request_version,
                    replay,
                )?,
            }
        }
        other => match shared.try_lock() {
            Ok(mut agent) => {
                let should_stop = handle_command(
                    &mut agent,
                    id,
                    other,
                    output,
                    event_sequence,
                    replay,
                    request_version,
                )?;
                if should_stop {
                    *stopping = true;
                }
            }
            Err(_) => write_retryable_response(
                output,
                StdioResponse::error_with_code(
                    id,
                    name,
                    "agent_busy",
                    "command cannot run while a turn owns the session",
                )
                .for_version(request_version),
                replay,
            )?,
        },
    }
    Ok(())
}

/// Turn deferred steer intents into a cancel/reissue pair once the original
/// job has a durable turn ID.  Cancellation and the replacement submission
/// use the same FIFO runtime channel, so the worker cannot start the reissue
/// before it has observed the cancellation request.  If either channel is
/// temporarily full the intent stays in this queue instead of being dropped
/// or surfacing a misleading `no_active_turn` error.
#[allow(clippy::too_many_arguments)]
fn promote_pending_steers<W, F>(
    runner: &crate::runtime::BackgroundRunner<AsyncRequest, AsyncProcessResult, AgentError, F>,
    jobs: &mut HashMap<crate::runtime::JobId, AsyncWork>,
    request_to_job: &mut HashMap<String, crate::runtime::JobId>,
    pending_steers: &mut VecDeque<PendingSteer>,
    output: &mut W,
    replay: &mut ReplayState,
    stopping: bool,
) -> Result<(), HeadlessError>
where
    W: Write,
    F: Fn(
            AsyncRequest,
            crate::runtime::CancellationToken,
        ) -> Result<AsyncProcessResult, AgentError>
        + Send
        + Sync
        + 'static,
{
    // Process only the requests present at entry.  A failed `try_submit` is
    // requeued for the next scheduler tick and must not spin indefinitely in
    // one call when the command channel remains full.
    let attempts = pending_steers.len();
    for _ in 0..attempts {
        let Some(mut pending) = pending_steers.pop_front() else {
            break;
        };
        if stopping {
            write_cached_versioned_response(
                output,
                StdioResponse::error_with_code(
                    pending.id,
                    "steer",
                    "runtime_closed",
                    "steer was discarded while shutting down",
                ),
                pending.version,
                replay,
            )?;
            continue;
        }

        let active_turn_id = jobs
            .get(&pending.target_job)
            .map(|work| {
                work.turn_id
                    .lock()
                    .map(|turn_id| turn_id.clone())
                    .map_err(|_| event_mailbox_headless_error())
            })
            .transpose()?
            .flatten();
        if let Some(turn_id) = active_turn_id {
            if pending
                .expected_turn_id
                .as_deref()
                .is_some_and(|expected| expected != turn_id)
            {
                write_cached_versioned_response(
                    output,
                    StdioResponse::error_with_code(
                        pending.id,
                        "steer",
                        "expected_turn_mismatch",
                        "expected turn does not match the active request",
                    ),
                    pending.version,
                    replay,
                )?;
                continue;
            }
            pending.superseded_turn_id = Some(turn_id);
        }

        if jobs.contains_key(&pending.target_job) {
            // Do not cancel before the original admission publishes its
            // turn ID.  Cancelling in this narrow window can make the worker
            // return before `Agent::submit`, which would leave no durable
            // parent for a reissue.  The next scheduler tick retries after
            // the ID becomes visible.
            if pending.superseded_turn_id.is_none() {
                enqueue_pending_steer(pending_steers, pending, output, replay)?;
                continue;
            }
            if !pending.cancel_sent {
                match runner.try_cancel(pending.target_job) {
                    Ok(()) => pending.cancel_sent = true,
                    Err(crate::runtime::SubmitError::QueueFull) => {
                        enqueue_pending_steer(pending_steers, pending, output, replay)?;
                        continue;
                    }
                    Err(crate::runtime::SubmitError::Closed) => {
                        write_cached_versioned_response(
                            output,
                            StdioResponse::error_with_code(
                                pending.id,
                                "steer",
                                "runtime_closed",
                                "runtime closed before steer cancellation was admitted",
                            ),
                            pending.version,
                            replay,
                        )?;
                        continue;
                    }
                }
            }
        }

        if let Some(superseded_turn_id) = pending.superseded_turn_id.clone() {
            let (request, events, started_turn_id) =
                AsyncRequest::reissue(pending.text.clone(), superseded_turn_id);
            match runner.try_submit(request) {
                Ok(job_id) => {
                    jobs.insert(
                        job_id,
                        AsyncWork {
                            id: pending.id.clone(),
                            command: "steer",
                            version: pending.version,
                            events,
                            turn_id: started_turn_id,
                        },
                    );
                    if let Some(request_id) = pending.id {
                        request_to_job.insert(request_id, job_id);
                    }
                }
                Err(crate::runtime::SubmitError::QueueFull) => {
                    // Keep the captured ID: the cancellation may already have
                    // been consumed by the worker while the command queue was
                    // full for the replacement request.
                    enqueue_pending_steer(pending_steers, pending, output, replay)?;
                }
                Err(crate::runtime::SubmitError::Closed) => {
                    write_cached_versioned_response(
                        output,
                        StdioResponse::error_with_code(
                            pending.id,
                            "steer",
                            "runtime_closed",
                            "runtime closed before steer reissue was admitted",
                        ),
                        pending.version,
                        replay,
                    )?;
                }
            }
        } else {
            // If the original completed before its turn ID became visible,
            // retain explicit steer semantics. A steer must never silently
            // become a new prompt after the active turn has gone idle; the
            // agent will return a typed `no_active_turn` submission instead.
            let request = TurnInputRequest {
                message: pending.text.clone(),
                expected_turn_id: pending.expected_turn_id.clone(),
                mode: crate::protocol::TurnMode::Steer,
                attachments: Vec::new(),
            };
            let (request, events, started_turn_id) = AsyncRequest::new(request);
            match runner.try_submit(request) {
                Ok(job_id) => {
                    jobs.insert(
                        job_id,
                        AsyncWork {
                            id: pending.id.clone(),
                            command: "steer",
                            version: pending.version,
                            events,
                            turn_id: started_turn_id,
                        },
                    );
                    if let Some(request_id) = pending.id {
                        request_to_job.insert(request_id, job_id);
                    }
                }
                Err(crate::runtime::SubmitError::QueueFull) => {
                    enqueue_pending_steer(pending_steers, pending, output, replay)?;
                }
                Err(crate::runtime::SubmitError::Closed) => {
                    write_cached_versioned_response(
                        output,
                        StdioResponse::error_with_code(
                            pending.id,
                            "steer",
                            "runtime_closed",
                            "runtime closed before steer was admitted",
                        ),
                        pending.version,
                        replay,
                    )?;
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
enum SlashExecution {
    Response(serde_json::Value),
    Cancel,
    Exit,
}

/// Turn a slash dispatcher projection into a terminal wire response. A
/// parsed-but-unowned command carries `accepted:false` in its local
/// projection; that is an operation failure, not a successful command. Keep
/// the action and owner message in the typed error while deliberately omitting
/// arbitrary arguments (which may contain filesystem paths or secrets).
fn slash_execution_response(
    id: Option<String>,
    command: impl Into<String>,
    data: serde_json::Value,
    version: u16,
) -> StdioResponse {
    let command = command.into();
    if data.get("accepted").and_then(serde_json::Value::as_bool) == Some(false) {
        let action = data
            .get("action")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("request");
        let message = data
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("command is not executable in this host");
        return StdioResponse::error_with_code(
            id,
            command.clone(),
            "unsupported_command",
            format!("{command} {action}: {message}"),
        )
        .for_version(version);
    }
    StdioResponse::success(id, command, Some(data)).for_version(version)
}

/// Preserve the control-plane classification used by older clients when a
/// session maintenance command is syntactically recognized but lacks the
/// explicit source/destination pair.  The parser remains strict (so a host
/// cannot accidentally mutate the active session), while the transport
/// reports this as an unowned/unsupported maintenance operation rather than
/// leaking a path-bearing parser diagnostic.
fn unsupported_session_maintenance_parse_error(error: &crate::slash::SlashError) -> Option<String> {
    match error {
        crate::slash::SlashError::MissingSessionPath { action }
            if matches!(*action, "fork" | "export" | "import") =>
        {
            Some(format!("session {action} requires source and destination"))
        }
        _ => None,
    }
}

#[derive(Debug)]
struct SlashDispatchError {
    code: &'static str,
    message: String,
}

/// The small owner grammar accepted inside the existing `/goal <instruction>`
/// slash variant.  Keeping this interpretation here avoids widening the wire
/// protocol while still giving headless clients a typed, durable status
/// operation.  Ordinary goal prose continues to fall through to the explicit
/// "external owner" refusal below.
#[derive(Debug)]
enum GoalOwnerAction {
    ShowAll,
    Show {
        id: String,
    },
    Transition {
        id: String,
        status: crate::domains::GoalStatus,
    },
}

fn parse_goal_owner_action(
    instruction: &str,
) -> Result<Option<GoalOwnerAction>, SlashDispatchError> {
    let tokens = instruction.split_whitespace().collect::<Vec<_>>();
    let Some(action) = tokens.first() else {
        return Ok(None);
    };
    if !action.eq_ignore_ascii_case("show")
        && !action.eq_ignore_ascii_case("list")
        && !action.eq_ignore_ascii_case("status")
        && !action.eq_ignore_ascii_case("transition")
        && !action.eq_ignore_ascii_case("set-status")
    {
        return Ok(None);
    }

    let invalid_usage = || {
        SlashDispatchError {
        code: "goal_status_usage",
        message: "use `/goal status`, `/goal status <goal-id>`, or `/goal status <goal-id> <queued|running|paused|blocked|cancelled|done>`".into(),
    }
    };
    let validate_id = |raw: &str| {
        if raw.is_empty()
            || raw.len() > crate::domains::MAX_ID_BYTES
            || raw.chars().any(char::is_control)
            || !raw
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._:/-".contains(character))
        {
            return Err(SlashDispatchError {
                code: "goal_invalid_id",
                message: "goal id is not a valid bounded identifier".into(),
            });
        }
        Ok(raw.to_owned())
    };

    if action.eq_ignore_ascii_case("show") || action.eq_ignore_ascii_case("list") {
        return match tokens.len() {
            1 => Ok(Some(GoalOwnerAction::ShowAll)),
            2 => Ok(Some(GoalOwnerAction::Show {
                id: validate_id(tokens[1])?,
            })),
            _ => Err(invalid_usage()),
        };
    }
    if action.eq_ignore_ascii_case("status") {
        return match tokens.len() {
            1 => Ok(Some(GoalOwnerAction::ShowAll)),
            2 => Ok(Some(GoalOwnerAction::Show {
                id: validate_id(tokens[1])?,
            })),
            3 => {
                let id = validate_id(tokens[1])?;
                let status = crate::domains::GoalStatus::parse_token(tokens[2]).ok_or_else(|| {
                    SlashDispatchError {
                        code: "goal_invalid_status",
                        message: "goal status must be queued, running, paused, blocked, cancelled, or done".into(),
                    }
                })?;
                Ok(Some(GoalOwnerAction::Transition { id, status }))
            }
            _ => Err(invalid_usage()),
        };
    }

    if tokens.len() != 3 {
        return Err(invalid_usage());
    }
    let id = validate_id(tokens[1])?;
    let status =
        crate::domains::GoalStatus::parse_token(tokens[2]).ok_or_else(|| SlashDispatchError {
            code: "goal_invalid_status",
            message: "goal status must be queued, running, paused, blocked, cancelled, or done"
                .into(),
        })?;
    Ok(Some(GoalOwnerAction::Transition { id, status }))
}

fn map_goal_store_error(
    error: domain_store::DomainStoreError,
    operation: &'static str,
) -> SlashDispatchError {
    match error {
        domain_store::DomainStoreError::GoalNotFound(id) => SlashDispatchError {
            code: "goal_not_found",
            message: format!("goal `{id}` is not found"),
        },
        domain_store::DomainStoreError::InvalidGoalTransition { id, from, to } => {
            SlashDispatchError {
                code: "goal_invalid_transition",
                message: format!(
                    "goal `{id}` cannot transition from {} to {}",
                    from.as_str(),
                    to.as_str()
                ),
            }
        }
        // Do not forward path-bearing store diagnostics across the control
        // plane.  The caller receives a stable operation code instead.
        _ => SlashDispatchError {
            code: "domain_store_error",
            message: format!("{operation} failed"),
        },
    }
}

fn goal_status_response(
    agent: &mut Agent,
    id: &str,
    next_status: crate::domains::GoalStatus,
) -> Result<serde_json::Value, SlashDispatchError> {
    let path = domain_store::path_for_session(agent.session().path());
    // Inspect first so a failed transition for a missing goal does not create
    // a new sibling domain snapshot as a read-side effect.
    let existing = DomainStore::open_read_only(&path)
        .map_err(|error| map_goal_store_error(error, "goal status"))?;
    if existing.goal(id).is_none() {
        return Err(SlashDispatchError {
            code: "goal_not_found",
            message: format!("goal `{id}` is not found"),
        });
    }
    drop(existing);

    let mut store =
        DomainStore::open(&path).map_err(|error| map_goal_store_error(error, "goal status"))?;
    // Re-read after opening the mutable owner.  This keeps the response
    // truthful if another process changed the bounded snapshot between the
    // side-effect-free probe and the atomic update.
    let from_status = store
        .goal(id)
        .map(|goal| goal.status)
        .ok_or_else(|| SlashDispatchError {
            code: "goal_not_found",
            message: format!("goal `{id}` is not found"),
        })?;
    let change = store
        .transition_goal(id, next_status)
        .map_err(|error| map_goal_store_error(error, "goal status"))?;
    let goal = store.goal(id).ok_or_else(|| SlashDispatchError {
        code: "goal_not_found",
        message: format!("goal `{id}` is not found"),
    })?;
    let summary = serialized_store_summary(&store)?;
    Ok(json!({
        "command": "goal",
        "route": "local",
        "accepted": true,
        "action": "transition",
        "goal_id": id,
        "from_status": from_status,
        "to_status": next_status,
        "status": next_status,
        "change": change,
        "goal": goal,
        "store": summary,
    }))
}

/// Transition one persisted Goal through its domain state machine.  This
/// public adapter is intentionally host-neutral so the TUI can adopt the same
/// owner operation without duplicating persistence or validation rules.
pub fn transition_goal_status(
    agent: &mut Agent,
    id: &str,
    next_status: crate::domains::GoalStatus,
) -> Result<serde_json::Value, String> {
    goal_status_response(agent, id, next_status).map_err(|error| error.message)
}

/// Resolve a slash approval through the agent-owned coordinator.  This adapter
/// is shared with the TUI so both transports use the same durable, fail-closed
/// owner rather than acknowledging a parsed command without taking action.
pub fn respond_to_slash_approval(
    agent: &mut Agent,
    id: &str,
    decision: crate::slash::ApproveDecision,
) -> Result<serde_json::Value, String> {
    let (decision, remember) = match decision {
        crate::slash::ApproveDecision::Once => (crate::approval::ApprovalDecision::Allow, false),
        crate::slash::ApproveDecision::Always => (crate::approval::ApprovalDecision::Allow, true),
        crate::slash::ApproveDecision::Deny => (crate::approval::ApprovalDecision::Deny, false),
    };
    let request = agent
        .respond_to_approval(crate::approval::ApprovalResponse {
            request_id: id.to_owned(),
            decision,
            remember,
        })
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "command": "approve",
        "route": "local",
        "accepted": true,
        "approval_id": request.request_id,
        "turn_id": request.turn_id,
        "call_id": request.call_id,
        "tool": request.tool,
        "side_effect": request.side_effect,
        "policy_digest": request.policy_digest,
        "lease_id": request.lease_id,
        "decision": decision,
        "remember": remember,
    }))
}

/// Execute the transport-independent portion of a slash command.  Commands
/// that inspect mutable agent state require a lock; view/runtime acknowledgements
/// deliberately do not, so they remain usable while a provider turn is busy.
fn execute_headless_slash(
    agent: Option<&mut Agent>,
    command: crate::slash::SlashCommand,
    runtime_source: Option<crate::runtime_intent::RuntimeIntentSource<'_>>,
) -> Result<SlashExecution, SlashDispatchError> {
    use crate::slash::{BlueprintAction, SlashCommand};

    match command {
        SlashCommand::Help { topic } => {
            let text = crate::slash::help(topic.as_deref()).ok_or_else(|| SlashDispatchError {
                code: "unknown_help_topic",
                message: "unknown help topic; use /help for available commands".into(),
            })?;
            Ok(SlashExecution::Response(json!({
                "command": "help",
                "text": text,
            })))
        }
        SlashCommand::Model { name } => {
            let Some(agent) = agent else {
                return Err(SlashDispatchError {
                    code: "agent_busy",
                    message: "model command cannot run while the agent is busy".into(),
                });
            };
            if let Some(name) = name {
                agent
                    .set_model(Some(name.clone()))
                    .map_err(|error| SlashDispatchError {
                        code: "model_error",
                        message: error.to_string(),
                    })?;
            }
            Ok(SlashExecution::Response(json!({
                "command": "model",
                "model": agent.snapshot().model,
            })))
        }
        SlashCommand::Models => {
            let entries =
                crate::config::model_catalog(None).map_err(|error| SlashDispatchError {
                    code: "model_catalog_error",
                    message: error.to_string(),
                })?;
            Ok(SlashExecution::Response(json!({
                "command": "models",
                "route": "local",
                "accepted": true,
                "models": entries,
            })))
        }
        SlashCommand::Doctor => {
            let value = crate::config::doctor_value(None).map_err(|error| SlashDispatchError {
                code: "doctor_error",
                message: error.to_string(),
            })?;
            Ok(SlashExecution::Response(value))
        }
        SlashCommand::Status => {
            let Some(agent) = agent else {
                return Err(SlashDispatchError {
                    code: "agent_busy",
                    message: "status snapshot is temporarily unavailable".into(),
                });
            };
            Ok(SlashExecution::Response(
                serialized_agent_snapshot(agent).map_err(|error| SlashDispatchError {
                    code: "serialization_error",
                    message: error.to_string(),
                })?,
            ))
        }
        SlashCommand::History { limit } => {
            let Some(agent) = agent else {
                return Err(SlashDispatchError {
                    code: "agent_busy",
                    message: "history is temporarily unavailable while the agent is busy".into(),
                });
            };
            let limit = limit.unwrap_or(20).clamp(1, 128);
            let turns = agent.history();
            let start = turns.len().saturating_sub(limit);
            let entries = turns[start..]
                .iter()
                .map(|turn| {
                    json!({
                        "id": turn.id,
                        "role": turn.role,
                        "content": bound_slash_text(&turn.content, 4096),
                    })
                })
                .collect::<Vec<_>>();
            Ok(SlashExecution::Response(json!({
                "command": "history",
                "entries": entries,
            })))
        }
        SlashCommand::Resources { path } => match collect_resource_snapshot(path.as_deref()) {
            Ok(snapshot) => Ok(SlashExecution::Response(json!({
                "command": "resources",
                "accepted": true,
                "snapshot": serialized_resource_snapshot(snapshot).map_err(|error| {
                    SlashDispatchError {
                        code: "serialization_error",
                        message: error.to_string(),
                    }
                })?,
            }))),
            Err(error) => Err(SlashDispatchError {
                code: "resource_error",
                message: error.to_string(),
            }),
        },
        SlashCommand::Layout { action } => Ok(SlashExecution::Response(json!({
            "command": "layout",
            "route": "local",
            "accepted": false,
            "action": action,
            "message": "layout mutations require the interactive TUI owner",
        }))),
        SlashCommand::Pane { action } => Ok(SlashExecution::Response(json!({
            "command": "pane",
            "route": "local",
            "accepted": false,
            "action": action,
            "message": "pane focus and visibility require the interactive TUI owner",
        }))),
        SlashCommand::Clear => Ok(SlashExecution::Response(json!({
            "command": "clear",
            "cleared": true,
            "durable_session": true,
        }))),
        SlashCommand::Cancel => Ok(SlashExecution::Cancel),
        SlashCommand::Exit => Ok(SlashExecution::Exit),
        SlashCommand::Goal { instruction } => {
            match parse_goal_owner_action(&instruction)? {
                Some(GoalOwnerAction::ShowAll) => {
                    let store = domain_store_for_agent(agent.as_deref(), true)?;
                    let data = serialized_domain_store(&store)?;
                    return Ok(SlashExecution::Response(json!({
                        "command": "goal",
                        "route": "local",
                        "accepted": true,
                        "action": "show",
                        "store": data["store"].clone(),
                        "goals": data["goals"].clone(),
                    })));
                }
                Some(GoalOwnerAction::Show { id }) => {
                    let store = domain_store_for_agent(agent.as_deref(), true)?;
                    let goal = store.goal(&id).ok_or_else(|| SlashDispatchError {
                        code: "goal_not_found",
                        message: format!("goal `{id}` is not found"),
                    })?;
                    let summary = serialized_store_summary(&store)?;
                    return Ok(SlashExecution::Response(json!({
                        "command": "goal",
                        "route": "local",
                        "accepted": true,
                        "action": "show",
                        "goal_id": id,
                        "store": summary,
                        "goal": goal,
                    })));
                }
                Some(GoalOwnerAction::Transition { id, status }) => {
                    let Some(agent) = agent else {
                        return Err(SlashDispatchError {
                            code: "agent_busy",
                            message:
                                "goal status is temporarily unavailable while the agent is busy"
                                    .into(),
                        });
                    };
                    return Ok(SlashExecution::Response(goal_status_response(
                        agent, &id, status,
                    )?));
                }
                None => {}
            }
            Ok(SlashExecution::Response(json!({
                "command": "goal",
                "route": "local",
                "accepted": false,
                "instruction": bound_slash_text(&instruction, 4096),
                "message": "goal creation/execution requires an external b3ehive owner",
            })))
        }
        SlashCommand::GoalPut { path } => Ok(SlashExecution::Response(persist_goal_from_path(
            agent, &path,
        )?)),
        SlashCommand::Plan { instruction } => Ok(SlashExecution::Response(json!({
            "command": "plan",
            "route": "local",
            "accepted": false,
            "instruction": instruction,
            "message": "plan command is parsed but its blueprint owner is not configured",
        }))),
        SlashCommand::Session { action } => match action {
            crate::slash::SessionAction::List => session_list_view()
                .map(SlashExecution::Response)
                .map_err(|message| SlashDispatchError {
                    code: "session_list_failed",
                    message,
                }),
            crate::slash::SessionAction::Open { path } => {
                let Some(agent) = agent else {
                    return Err(SlashDispatchError {
                        code: "agent_busy",
                        message: "session cannot be opened while the agent is busy".into(),
                    });
                };
                open_session_path(agent, &path)
                    .map(SlashExecution::Response)
                    .map_err(|message| SlashDispatchError {
                        code: "session_open_failed",
                        message,
                    })
            }
            crate::slash::SessionAction::ResumeLast => {
                let Some(agent) = agent else {
                    return Err(SlashDispatchError {
                        code: "agent_busy",
                        message: "session resume-last requires the session owner".into(),
                    });
                };
                resume_last_session_view(agent)
                    .map(SlashExecution::Response)
                    .map_err(|message| SlashDispatchError {
                        code: "session_resume_failed",
                        message,
                    })
            }
            action @ (crate::slash::SessionAction::Agents
            | crate::slash::SessionAction::Inspect { .. }
            | crate::slash::SessionAction::Fork { .. }
            | crate::slash::SessionAction::Export { .. }
            | crate::slash::SessionAction::Import { .. }
            | crate::slash::SessionAction::Migrate { .. }
            | crate::slash::SessionAction::Archive { .. }
            | crate::slash::SessionAction::Unarchive { .. }
            | crate::slash::SessionAction::Delete { .. }
            | crate::slash::SessionAction::Queue { .. }) => {
                let active_path = agent.as_deref().map(|value| value.session().path());
                session_lifecycle_view(&action, active_path)
                    .map(SlashExecution::Response)
                    .map_err(|message| SlashDispatchError {
                        code: "session_lifecycle_failed",
                        message,
                    })
            }
            crate::slash::SessionAction::Gc { policy } => {
                let Some(agent) = agent else {
                    return Err(SlashDispatchError {
                        code: "agent_busy",
                        message: "session gc is unavailable while the agent is busy".into(),
                    });
                };
                session_gc_view(agent, &policy)
                    .map(SlashExecution::Response)
                    .map_err(|message| SlashDispatchError {
                        code: "session_gc_failed",
                        message,
                    })
            }
        },
        SlashCommand::Mailbox { action } => {
            let Some(agent) = agent else {
                return Err(SlashDispatchError {
                    code: "agent_busy",
                    message: "mailbox command requires the session owner".into(),
                });
            };
            mailbox_slash_view(agent, &action)
                .map(SlashExecution::Response)
                .map_err(|message| SlashDispatchError {
                    code: "mailbox_failed",
                    message,
                })
        }
        SlashCommand::Resume { sequence } => {
            let Some(agent) = agent else {
                return Err(SlashDispatchError {
                    code: "agent_busy",
                    message: "session replay is temporarily unavailable while the agent is busy"
                        .into(),
                });
            };
            resume_session_view(agent, sequence)
                .map(SlashExecution::Response)
                .map_err(|message| SlashDispatchError {
                    code: "resume_error",
                    message,
                })
        }
        SlashCommand::Compact => {
            let Some(agent) = agent else {
                return Err(SlashDispatchError {
                    code: "agent_busy",
                    message:
                        "context compaction is temporarily unavailable while the agent is busy"
                            .into(),
                });
            };
            compact_context_view(agent)
                .map(SlashExecution::Response)
                .map_err(|message| SlashDispatchError {
                    code: "compact_error",
                    message,
                })
        }
        SlashCommand::Diff { path } => {
            crate::slash_actions::diff_value_for_agent(agent.as_deref(), path.as_deref())
                .map(|mut data| {
                    if let Some(object) = data.as_object_mut() {
                        object.insert("route".into(), json!("local"));
                        object.insert("accepted".into(), json!(true));
                    }
                    SlashExecution::Response(data)
                })
                .map_err(|error| SlashDispatchError {
                    code: "diff_error",
                    message: error.to_string(),
                })
        }
        SlashCommand::Attach { path } => {
            let Some(agent) = agent else {
                return Err(SlashDispatchError {
                    code: "agent_busy",
                    message: "attachment staging is unavailable while the agent is busy".into(),
                });
            };
            crate::slash_actions::attach_value(agent, &path)
                .map(SlashExecution::Response)
                .map_err(|error| SlashDispatchError {
                    code: "attachment_error",
                    message: error.to_string(),
                })
        }
        SlashCommand::Approve { id, decision } => {
            let Some(agent) = agent else {
                return Err(SlashDispatchError {
                    code: "agent_busy",
                    message: "approval response is temporarily unavailable while the agent is busy"
                        .into(),
                });
            };
            respond_to_slash_approval(agent, &id, decision)
                .map(SlashExecution::Response)
                .map_err(|message| SlashDispatchError {
                    code: "approval_error",
                    message,
                })
        }
        SlashCommand::Blueprint { action } => {
            let action_name = match &action {
                BlueprintAction::Show => "show",
                BlueprintAction::Status => "status",
                BlueprintAction::Validate { .. } => "validate",
                BlueprintAction::Put { .. } => "put",
                BlueprintAction::Run { .. } => "run",
                BlueprintAction::Handoff { .. } => "handoff",
                BlueprintAction::Open { .. } => "open",
            };
            match action {
                BlueprintAction::Show => {
                    let store = domain_store_for_agent(agent.as_deref(), true)?;
                    let data = serialized_domain_store(&store)?;
                    Ok(SlashExecution::Response(json!({
                        "command": "blueprint",
                        "route": "local",
                        "accepted": true,
                        "action": action_name,
                        "store": data["store"].clone(),
                        "blueprints": data["blueprints"].clone(),
                    })))
                }
                BlueprintAction::Status => {
                    let store = domain_store_for_agent(agent.as_deref(), true)?;
                    let summary = serialized_store_summary(&store)?;
                    Ok(SlashExecution::Response(json!({
                        "command": "blueprint",
                        "route": "local",
                        "accepted": true,
                        "action": action_name,
                        "store": summary,
                    })))
                }
                BlueprintAction::Validate { path: Some(path) } => {
                    let result = validate_blueprint_target(&path)?;
                    Ok(SlashExecution::Response(json!({
                        "command": "blueprint",
                        "route": "local",
                        "accepted": true,
                        "action": action_name,
                        "result": result,
                    })))
                }
                BlueprintAction::Validate { path: None } => {
                    let store = domain_store_for_agent(agent.as_deref(), true)?;
                    store.validate().map_err(|error| SlashDispatchError {
                        code: "domain_store_error",
                        message: error.to_string(),
                    })?;
                    let summary = serialized_store_summary(&store)?;
                    Ok(SlashExecution::Response(json!({
                        "command": "blueprint",
                        "route": "local",
                        "accepted": true,
                        "action": action_name,
                        "valid": true,
                        "store": summary,
                    })))
                }
                BlueprintAction::Put { path } => Ok(SlashExecution::Response(
                    persist_blueprint_from_path(agent, &path)?,
                )),
                BlueprintAction::Run { target } => {
                    let Some(agent) = agent else {
                        return Err(SlashDispatchError {
                            code: "agent_busy",
                            message: "blueprint execution is unavailable while the agent is busy"
                                .into(),
                        });
                    };
                    run_blueprint_next(agent, &target)
                        .map(SlashExecution::Response)
                        .map_err(|message| SlashDispatchError {
                            code: "blueprint_execution_error",
                            message,
                        })
                }
                BlueprintAction::Handoff { target } => {
                    let Some(agent) = agent else {
                        return Err(SlashDispatchError {
                            code: "agent_busy",
                            message: "blueprint handoff is unavailable while the agent is busy"
                                .into(),
                        });
                    };
                    handoff_blueprint_next(agent, &target)
                        .map(SlashExecution::Response)
                        .map_err(|message| SlashDispatchError {
                            code: "blueprint_handoff_error",
                            message,
                        })
                }
                BlueprintAction::Open { path: target } => Ok(SlashExecution::Response(json!({
                    "command": "blueprint",
                    "route": "local",
                    "accepted": false,
                    "action": action_name,
                    "target": target,
                    "message": "blueprint execution or interactive opening requires a host adapter",
                }))),
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
                let store = domain_store_for_agent(agent.as_deref(), true)?;
                let data = serialized_domain_store(&store)?;
                return Ok(SlashExecution::Response(json!({
                    "command": "learn",
                    "route": "local",
                    "accepted": true,
                    "action": "show",
                    "store": data["store"].clone(),
                    "learns": data["learns"].clone(),
                })));
            }
            Ok(SlashExecution::Response(json!({
                "command": "learn",
                "route": "local",
                "accepted": false,
                "target": target,
                "message": "learn execution requires an external owner; use /learn show or /learn put <json-path>",
            })))
        }
        SlashCommand::LearnPut { path } => Ok(SlashExecution::Response(persist_learn_from_path(
            agent, &path,
        )?)),
        SlashCommand::LearnEvidence { id, reference } => Ok(SlashExecution::Response(
            add_learn_evidence_from_ref(agent, &id, &reference)?,
        )),
        SlashCommand::LearnResume { id } => Ok(SlashExecution::Response(resume_learn_view(
            agent.as_deref(),
            &id,
        )?)),
        SlashCommand::Compete { args } => {
            let Some(agent) = agent else {
                return Err(SlashDispatchError {
                    code: "agent_busy",
                    message: "compete intent cannot be persisted while the agent is busy".into(),
                });
            };
            crate::runtime_intent::runtime_intent_value_with_source(
                agent,
                crate::b3::RuntimeIntentKind::Compete,
                &args,
                runtime_source,
            )
            .map(SlashExecution::Response)
            .map_err(|error| SlashDispatchError {
                code: error.code(),
                message: error.to_string(),
            })
        }
        SlashCommand::Loop { args } => {
            let Some(agent) = agent else {
                return Err(SlashDispatchError {
                    code: "agent_busy",
                    message: "loop intent cannot be persisted while the agent is busy".into(),
                });
            };
            crate::runtime_intent::runtime_intent_value_with_source(
                agent,
                crate::b3::RuntimeIntentKind::Loop,
                &args,
                runtime_source,
            )
            .map(SlashExecution::Response)
            .map_err(|error| SlashDispatchError {
                code: error.code(),
                message: error.to_string(),
            })
        }
    }
}

/// Fingerprint only the durable runtime intent semantics. Unlike the terminal
/// replay fingerprint, this digest survives aliases and request-envelope
/// version changes across process restarts.
fn slash_runtime_fingerprint(
    command: &crate::slash::SlashCommand,
) -> Result<Option<String>, HeadlessError> {
    use sha2::{Digest, Sha256};

    let value = match command {
        crate::slash::SlashCommand::Compete { args } => {
            Some(json!({"kind": "compete", "args": args}))
        }
        crate::slash::SlashCommand::Loop { args } => Some(json!({"kind": "loop", "args": args})),
        _ => None,
    };
    let Some(value) = value else {
        return Ok(None);
    };
    Ok(Some(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&value)?)
    )))
}

fn bound_slash_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn handle_command<W: Write>(
    agent: &mut Agent,
    id: Option<String>,
    command: Command,
    output: &mut W,
    event_sequence: &mut u64,
    replay: &mut ReplayState,
    request_version: u16,
) -> Result<bool, HeadlessError> {
    let name = crate::protocol::command_name(&command);
    match command {
        Command::Slash { input } => match crate::slash::route_input(&input) {
            Err(error) => {
                let special = unsupported_session_maintenance_parse_error(&error);
                let code = special
                    .as_ref()
                    .map_or("slash_invalid", |_| "unsupported_command");
                let message = special.unwrap_or_else(|| error.to_string());
                write_cached_versioned_response(
                    output,
                    StdioResponse::error_with_code(id, name, code, message),
                    request_version,
                    replay,
                )?
            }
            Ok(crate::slash::InputRoute::Prompt(_)) => write_cached_versioned_response(
                output,
                StdioResponse::error_with_code(
                    id,
                    name,
                    "not_slash_command",
                    "command input must begin with `/`",
                ),
                request_version,
                replay,
            )?,
            Ok(crate::slash::InputRoute::UserShell(input)) => {
                match agent.run_user_shell_with_cancel(&input, || false) {
                    Ok(data) => {
                        write_events(agent, output, event_sequence, id.clone(), replay)?;
                        write_cached_versioned_response(
                            output,
                            StdioResponse::success(id, name, Some(data)),
                            request_version,
                            replay,
                        )?
                    }
                    Err(error) => {
                        write_events(agent, output, event_sequence, id.clone(), replay)?;
                        write_cached_versioned_response(
                            output,
                            StdioResponse::error_with_code(
                                id,
                                name,
                                error.code(),
                                error.to_string(),
                            ),
                            request_version,
                            replay,
                        )?;
                    }
                }
            }
            Ok(crate::slash::InputRoute::Slash(command)) => {
                let command_name = command.name();
                let session_open = matches!(
                    &command,
                    crate::slash::SlashCommand::Session {
                        action: crate::slash::SessionAction::Open { .. }
                    }
                );
                let durable_fingerprint = slash_runtime_fingerprint(&command)?;
                let runtime_source = id.as_deref().and_then(|request_id| {
                    durable_fingerprint.as_deref().map(|fingerprint| {
                        crate::runtime_intent::RuntimeIntentSource {
                            request_id,
                            fingerprint,
                        }
                    })
                });
                match execute_headless_slash(Some(agent), command, runtime_source) {
                    Ok(SlashExecution::Response(data)) => {
                        let response = slash_execution_response(
                            id.clone(),
                            command_name,
                            data,
                            request_version,
                        );
                        if session_open && response.success {
                            replay.reset_for_session(
                                agent.session(),
                                id.as_deref(),
                                *event_sequence,
                            )?;
                            *event_sequence = (*event_sequence).max(replay.next_sequence);
                        }
                        write_cached_response(output, response, replay)?
                    }
                    Ok(SlashExecution::Cancel) => write_cached_versioned_response(
                        output,
                        StdioResponse::error_with_code(
                            id,
                            command_name,
                            "unsupported_sync_command",
                            "cancel is available in asynchronous headless mode",
                        ),
                        request_version,
                        replay,
                    )?,
                    Ok(SlashExecution::Exit) => {
                        agent.close();
                        write_cached_versioned_response(
                            output,
                            StdioResponse::success(
                                id,
                                command_name,
                                Some(json!({"closing": true})),
                            ),
                            request_version,
                            replay,
                        )?;
                        return Ok(true);
                    }
                    Err(error) => write_cached_versioned_response(
                        output,
                        StdioResponse::error_with_code(id, command_name, error.code, error.message),
                        request_version,
                        replay,
                    )?,
                }
            }
        },
        Command::Resources { path } => match collect_resource_snapshot(path.as_deref()) {
            Ok(snapshot) => write_cached_versioned_response(
                output,
                StdioResponse::success(id, name, Some(serialized_resource_snapshot(snapshot)?)),
                request_version,
                replay,
            )?,
            Err(error) => write_cached_versioned_response(
                output,
                StdioResponse::error_with_code(id, name, "resource_error", error.to_string()),
                request_version,
                replay,
            )?,
        },
        Command::Prompt {
            text,
            mode,
            expected_turn_id,
            attachments,
        } => {
            let result = agent.process(TurnInputRequest {
                message: text,
                expected_turn_id,
                mode,
                attachments,
            });
            match result {
                Ok(result) => {
                    let data = json!({
                        "submission": result.submission,
                        "assistant": result.assistant,
                    });
                    // A turn's terminal response is the wire completion
                    // boundary. Flush every lifecycle/provider event first.
                    write_events(agent, output, event_sequence, id.clone(), replay)?;
                    write_cached_versioned_response(
                        output,
                        StdioResponse::success(id.clone(), name, Some(data)),
                        request_version,
                        replay,
                    )?;
                }
                Err(error) => {
                    // Failed admission/provider calls can still leave a
                    // TurnRejected or Error marker; never emit it after the
                    // terminal error response.
                    write_events(agent, output, event_sequence, id.clone(), replay)?;
                    write_cached_versioned_response(
                        output,
                        StdioResponse::error_with_code(
                            id.clone(),
                            name,
                            error.code(),
                            error.to_string(),
                        ),
                        request_version,
                        replay,
                    )?;
                }
            }
        }
        Command::Steer {
            text,
            expected_turn_id,
        } => {
            let result = agent.process(TurnInputRequest {
                message: text,
                expected_turn_id,
                mode: crate::protocol::TurnMode::Steer,
                attachments: Vec::new(),
            });
            match result {
                Ok(result) => {
                    let data = json!({
                        "submission": result.submission,
                        "assistant": result.assistant,
                    });
                    write_events(agent, output, event_sequence, id.clone(), replay)?;
                    write_cached_versioned_response(
                        output,
                        StdioResponse::success(id.clone(), name, Some(data)),
                        request_version,
                        replay,
                    )?;
                }
                Err(error) => {
                    write_events(agent, output, event_sequence, id.clone(), replay)?;
                    write_cached_versioned_response(
                        output,
                        StdioResponse::error_with_code(
                            id.clone(),
                            name,
                            error.code(),
                            error.to_string(),
                        ),
                        request_version,
                        replay,
                    )?;
                }
            }
        }
        Command::Cancel { .. } => write_cached_versioned_response(
            output,
            StdioResponse::error_with_code(
                id,
                name,
                "unsupported_sync_command",
                "cancel is available in asynchronous stdio mode",
            ),
            request_version,
            replay,
        )?,
        Command::Checkpoint(ref request) => {
            let response = checkpoint_response(agent, request, id, name, replay)?;
            write_cached_versioned_response(output, response, request_version, replay)?;
        }
        Command::Mailbox(ref request) => {
            write_cached_versioned_response(
                output,
                mailbox_response(agent, request, id, name),
                request_version,
                replay,
            )?;
        }
        Command::UserShell(_) => {
            let input = match command {
                Command::UserShell(request) => request.input,
                _ => unreachable!(),
            };
            match agent.run_user_shell_with_cancel(&input, || false) {
                Ok(data) => write_cached_versioned_response(
                    output,
                    StdioResponse::success(id, name, Some(data)),
                    request_version,
                    replay,
                )?,
                Err(error) => {
                    write_events(agent, output, event_sequence, id.clone(), replay)?;
                    write_cached_versioned_response(
                        output,
                        StdioResponse::error_with_code(id, name, error.code(), error.to_string()),
                        request_version,
                        replay,
                    )?;
                }
            }
        }
        Command::Status => {
            write_cached_versioned_response(
                output,
                StdioResponse::success(id, name, Some(serialized_agent_snapshot(agent)?)),
                request_version,
                replay,
            )?;
        }
        Command::Handoff {
            to,
            summary,
            artifacts,
        } => {
            let from = std::env::var("ZENPI_AGENT_ID").unwrap_or_else(|_| "zenpi".into());
            let now = unix_time_ms();
            let recipient = to.unwrap_or_else(|| "broadcast".into());
            let claim_id = id.clone().unwrap_or_else(|| format!("claim-{now}"));
            let handoff = HandoffRecord::new(
                from,
                recipient,
                claim_id,
                summary,
                artifacts,
                agent.session().session_id().to_owned(),
                unix_ms_to_rfc3339(now),
            );
            match handoff {
                Ok(handoff) => match agent.append_handoff_record(handoff.clone()) {
                    Ok(()) => {
                        // The handoff lifecycle marker belongs to this
                        // operation and must precede its acknowledgement.
                        write_events(agent, output, event_sequence, id.clone(), replay)?;
                        write_cached_versioned_response(
                            output,
                            StdioResponse::success(
                                id.clone(),
                                name,
                                Some(json!({"accepted": true, "handoff": handoff})),
                            ),
                            request_version,
                            replay,
                        )?;
                    }
                    Err(error) => {
                        // Drain any marker produced before a persistence
                        // failure, then expose the terminal error.
                        write_events(agent, output, event_sequence, id.clone(), replay)?;
                        write_cached_versioned_response(
                            output,
                            StdioResponse::error_with_code(
                                id,
                                name,
                                error.code(),
                                error.to_string(),
                            ),
                            request_version,
                            replay,
                        )?;
                    }
                },
                Err(error) => write_cached_versioned_response(
                    output,
                    StdioResponse::error_with_code(id, name, "handoff_error", error.to_string()),
                    request_version,
                    replay,
                )?,
            }
        }
        Command::Resume {
            path,
            from_sequence,
        } => match (path, from_sequence) {
            (Some(path), None) => {
                let path = match validate_existing_session_path(&path) {
                    Ok(path) => path,
                    Err(message) => {
                        write_cached_versioned_response(
                            output,
                            StdioResponse::error_with_code(
                                id,
                                name,
                                "session_path_denied",
                                message,
                            )
                            .for_version(request_version),
                            request_version,
                            replay,
                        )?;
                        return Ok(false);
                    }
                };
                match agent.resume_session(path) {
                    Ok(()) => {
                        // A session switch invalidates the old transport replay
                        // namespace. Keep sequence monotonic while dropping old
                        // events and terminal IDs before exposing the new
                        // snapshot.
                        agent.take_events();
                        replay.reset_for_session(
                            agent.session(),
                            id.as_deref(),
                            *event_sequence,
                        )?;
                        *event_sequence = (*event_sequence).max(replay.next_sequence);
                        write_cached_versioned_response(
                            output,
                            StdioResponse::success(
                                id,
                                name,
                                Some(serialized_agent_snapshot(agent)?),
                            ),
                            request_version,
                            replay,
                        )?;
                    }
                    Err(error) => write_cached_versioned_response(
                        output,
                        StdioResponse::error_with_code(id, name, error.code(), error.to_string()),
                        request_version,
                        replay,
                    )?,
                }
            }
            (None, Some(from_sequence)) => {
                let report = replay.replay_from(from_sequence, output)?;
                write_response(
                    output,
                    StdioResponse::success(
                        id.clone(),
                        name,
                        Some(json!({
                            "from_sequence": from_sequence,
                            "replayed": report.replayed,
                            "replay_gap": report.gap,
                            "first_available_sequence": report.first_available,
                            "last_available_sequence": report.last_available,
                            "next_sequence": *event_sequence,
                        })),
                    )
                    .for_version(request_version),
                )?;
                replay.release(id.as_deref())?;
            }
            (None, None) => write_cached_versioned_response(
                output,
                StdioResponse::success(id, name, Some(serialized_agent_snapshot(agent)?)),
                request_version,
                replay,
            )?,
            (Some(_), Some(_)) => unreachable!("protocol rejects path plus from_sequence"),
        },
        Command::Approve { .. } => write_cached_versioned_response(
            output,
            StdioResponse::error_with_code(
                id,
                name,
                "approval_unavailable",
                "approval responses require the asynchronous headless host",
            ),
            request_version,
            replay,
        )?,
        Command::Shutdown => {
            agent.close();
            write_cached_versioned_response(
                output,
                StdioResponse::success(id, name, Some(json!({"closed": true}))),
                request_version,
                replay,
            )?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// The checkpoint cursor names the transcript journal. Transport replay has
/// its own namespace and is reported separately by the headless owner.
pub fn inspect_checkpoint(agent: &Agent) -> crate::protocol::CheckpointInspection {
    crate::protocol::CheckpointInspection {
        cursor: crate::protocol::CheckpointCursor {
            session_id: agent.session().session_id().to_owned(),
            next_sequence: agent.session().next_sequence(),
        },
        cursor_scope: crate::protocol::CheckpointCursorScope::SessionJournal,
        reconnect_supported: true,
        acknowledgement_supported: true,
    }
}

fn checkpoint_response(
    agent: &Agent,
    request: &crate::protocol::CheckpointRequest,
    id: Option<String>,
    name: impl Into<String>,
    replay: &mut ReplayState,
) -> Result<StdioResponse, HeadlessError> {
    use crate::protocol::CheckpointRequest;
    let name = name.into();
    let cursor = match request {
        CheckpointRequest::Inspect => None,
        CheckpointRequest::Acknowledge { cursor } | CheckpointRequest::Replay { cursor, .. } => {
            Some(cursor)
        }
    };
    if let Some(cursor) = cursor
        && (cursor.session_id != agent.session().session_id()
            || cursor.next_sequence > agent.session().next_sequence())
    {
        return Ok(StdioResponse::error_with_code(
            id,
            name,
            "checkpoint_cursor_invalid",
            "cursor belongs to another session or is ahead of the durable journal",
        ));
    }
    let data = match request {
        CheckpointRequest::Inspect => {
            let mut value = serde_json::to_value(inspect_checkpoint(agent))?;
            value["acknowledged_next_sequence"] = json!(replay.acknowledged);
            value["transport"] = json!({
                "cursor_scope": "outbound_events", "session_id": agent.session().session_id(),
                "next_sequence": replay.next_sequence, "owner_epoch": replay.owner_epoch,
                "first_available_sequence": replay.events.front().map(|(sequence, _)| sequence),
                "unknown_requests": replay.unknown.len(),
            });
            value
        }
        CheckpointRequest::Acknowledge { cursor } => {
            let acknowledged = replay.acknowledged.max(cursor.next_sequence);
            replay.persist(ReconnectEntry::Acknowledged {
                next_sequence: acknowledged,
            })?;
            replay.acknowledged = acknowledged;
            json!({ "cursor_scope": "session_journal", "session_id": cursor.session_id, "acknowledged_next_sequence": acknowledged })
        }
        CheckpointRequest::Replay { cursor, limit } => {
            let mut records = Vec::new();
            let mut bytes = 0;
            let mut next = cursor.next_sequence;
            let mut gap = false;
            for record in agent
                .session()
                .records()
                .iter()
                .filter(|record| record.sequence >= cursor.next_sequence)
            {
                if records.len() >= usize::from(*limit) {
                    break;
                }
                let size = serde_json::to_vec(record)?.len();
                if bytes + size > MAX_SLASH_RESUME_BYTES {
                    if records.is_empty() {
                        next = record.sequence.saturating_add(1);
                        gap = true;
                    }
                    break;
                }
                gap |= record.sequence != next;
                next = record.sequence.saturating_add(1);
                bytes += size;
                records.push(record);
            }
            gap |= next < agent.session().next_sequence() && records.is_empty();
            json!({
                "cursor_scope": "session_journal", "cursor": { "session_id": cursor.session_id, "next_sequence": next },
                "records": records, "replay_gap": gap, "has_more": next < agent.session().next_sequence(),
            })
        }
    };
    Ok(StdioResponse::success(id, name, Some(data)))
}

fn mailbox_response(
    agent: &Agent,
    request: &crate::protocol::MailboxRequest,
    id: Option<String>,
    name: impl Into<String>,
) -> StdioResponse {
    let name = name.into();
    match execute_mailbox(agent.session(), request) {
        Ok(data) => StdioResponse::success(id, name, Some(data)),
        Err(error) => StdioResponse::error_with_code(id, name, "mailbox_error", error.to_string()),
    }
}

fn execute_mailbox(
    session: &SessionStore,
    request: &crate::protocol::MailboxRequest,
) -> Result<serde_json::Value, crate::session::SessionError> {
    use crate::protocol::{MailboxOutcome, MailboxRequest};
    use crate::session::{MailboxAction, SessionError, SessionMailbox};
    let now = unix_time_ms();
    if let MailboxRequest::Send {
        recipient_session_id,
        message_id,
        text,
        ttl_ms,
    } = request
    {
        // Only clean journals registered beside the active session are
        // addressable. Neither a wire path nor a sender identity is trusted.
        let directory = session.path().parent().unwrap_or_else(|| Path::new("."));
        let matches: Vec<_> = crate::session::list_sessions(directory)?
            .into_iter()
            .filter(|summary| &summary.session_id == recipient_session_id)
            .collect();
        if matches.len() != 1 {
            return Err(SessionError::InvalidRecord(
                "recipient is missing or ambiguous in this session directory".into(),
            ));
        }
        let mailbox = SessionMailbox::open(&matches[0].path)?;
        let message = mailbox.enqueue(session, message_id, json!({"text": text}), *ttl_ms, now)?;
        return Ok(
            json!({"message_id": message.digest, "message": message, "execution_started": false}),
        );
    }
    let mailbox = SessionMailbox::open(session.path())?;
    if let MailboxRequest::Receive {
        after_sequence,
        limit,
    } = request
    {
        let page = mailbox.list(session, *after_sequence, usize::from(*limit).min(64), now)?;
        let messages: Vec<_> = page
            .messages
            .iter()
            .map(|message| json!({ "message_id": message.digest, "message": message }))
            .collect();
        return Ok(json!({"messages": messages, "next_sequence": page.next_sequence}));
    }
    let message_id = match request {
        MailboxRequest::Acknowledge { message_id }
        | MailboxRequest::Claim { message_id }
        | MailboxRequest::Complete { message_id, .. } => message_id,
        _ => unreachable!("send/receive already handled"),
    };
    let message = mailbox.find_message(session, message_id, now)?;
    let action = match request {
        MailboxRequest::Acknowledge { .. } => MailboxAction::Acknowledge,
        MailboxRequest::Claim { .. } => MailboxAction::Claim {
            token: format!("headless-{}", message.digest),
        },
        MailboxRequest::Complete {
            outcome, result, ..
        } => {
            let token = message.claim_token.clone().ok_or_else(|| {
                SessionError::InvalidRecord("message must be claimed before completion".into())
            })?;
            let result = json!({"outcome": outcome, "result": result});
            match outcome {
                MailboxOutcome::Succeeded => MailboxAction::Complete { token, result },
                MailboxOutcome::Failed | MailboxOutcome::Abandoned => MailboxAction::Fail {
                    token,
                    error: result,
                },
            }
        }
        _ => unreachable!("send/receive already handled"),
    };
    let updated = mailbox.update(
        session,
        &message.sender_session_id,
        &message.request_id,
        action,
        now,
    )?;
    Ok(json!({"message_id": updated.digest, "message": updated, "execution_started": false}))
}

fn admission_error(
    id: Option<String>,
    command: String,
    admission: RequestIdAdmission,
) -> StdioResponse {
    let (code, message) = match admission {
        RequestIdAdmission::UnknownOutcome => (
            "unknown_outcome",
            "previous owner stopped without a durable result; execution was not retried; inspect effects and explicitly retry with a new ID or abandon",
        ),
        RequestIdAdmission::ReplayExpired => (
            "request_replay_expired",
            "request completed but its response left the bounded replay window; execution was not repeated",
        ),
        RequestIdAdmission::LedgerFull => (
            "request_ledger_full",
            "durable request ledger is full; execution was not admitted; start a new session",
        ),
        _ => unreachable!("normal admission is handled by the dispatcher"),
    };
    StdioResponse::error_with_code(id, command, code, message)
}

fn write_response<W: Write>(output: &mut W, response: StdioResponse) -> Result<(), HeadlessError> {
    let line = encode_line(&response)?;
    output.write_all(line.as_bytes())?;
    output.flush()?;
    Ok(())
}

fn write_cached_response<W: Write>(
    output: &mut W,
    response: StdioResponse,
    replay: &mut ReplayState,
) -> Result<(), HeadlessError> {
    let request_id = response.id.clone();
    let line = encode_line(&response)?;
    if let Some(request_id) = request_id {
        replay.remember_terminal(request_id, line.clone())?;
    }
    output.write_all(line.as_bytes())?;
    output.flush()?;
    Ok(())
}

fn write_events<W: Write>(
    agent: &mut Agent,
    output: &mut W,
    sequence: &mut u64,
    request_id: Option<String>,
    replay: &mut ReplayState,
) -> Result<(), HeadlessError> {
    write_buffered_events(
        output,
        sequence,
        request_id,
        agent.take_events(),
        Some(replay),
    )
}

fn write_buffered_events<W: Write>(
    output: &mut W,
    sequence: &mut u64,
    request_id: Option<String>,
    events: impl IntoIterator<Item = AgentEvent>,
    replay: Option<&mut ReplayState>,
) -> Result<(), HeadlessError> {
    let mut replay = replay;
    for event in events {
        let value = serde_json::to_value(&event)?;
        let turn_id = value
            .get("turn_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let envelope = StdioEvent::new(*sequence, request_id.clone(), turn_id, value);
        let line = encode_line(&envelope)?;
        let current = *sequence;
        *sequence = sequence.saturating_add(1);
        // Lifecycle events are replayable just like provider deltas. This
        // keeps resume suffixes complete after a reconnect.
        if let Some(replay) = replay.as_deref_mut() {
            replay.remember_event(current, line.clone())?;
        }
        output.write_all(line.as_bytes())?;
    }
    output.flush()?;
    Ok(())
}

#[cfg(test)]
mod async_event_buffer_tests {
    use super::*;

    #[test]
    fn oversized_provider_event_is_counted_but_never_retained() {
        let event = ProviderEvent::TextDelta {
            delta: "x".repeat(MAX_ASYNC_PROVIDER_BYTES),
        };
        let expected_bytes = serialized_event_len(&event);
        assert!(expected_bytes > MAX_ASYNC_PROVIDER_BYTES);

        let mut buffer = AsyncEventBuffer::default();
        buffer.push_provider(event);
        assert!(buffer.provider.is_empty());
        assert_eq!(buffer.provider_bytes, 0);

        let (events, dropped) = buffer.take_provider();
        assert!(events.is_empty());
        assert_eq!(
            dropped,
            DroppedEvents {
                count: 1,
                bytes: expected_bytes as u64,
            }
        );
        assert_eq!(buffer.provider_bytes, 0);
        assert_eq!(buffer.provider_dropped, DroppedEvents::default());
    }

    #[test]
    fn provider_byte_budget_and_take_reset_all_accounting() {
        let first = ProviderEvent::TextDelta {
            delta: "a".repeat(MAX_ASYNC_PROVIDER_BYTES / 2),
        };
        let second = ProviderEvent::TextDelta {
            delta: "b".repeat(MAX_ASYNC_PROVIDER_BYTES / 2),
        };
        let first_bytes = serialized_event_len(&first);
        let second_bytes = serialized_event_len(&second);
        assert!(first_bytes.saturating_add(second_bytes) > MAX_ASYNC_PROVIDER_BYTES);

        let mut buffer = AsyncEventBuffer::default();
        buffer.push_provider(first);
        buffer.push_provider(second);
        assert_eq!(buffer.provider.len(), 1);
        assert_eq!(buffer.provider_bytes, first_bytes);

        let (events, dropped) = buffer.take_provider();
        assert_eq!(events.len(), 1);
        assert_eq!(dropped.count, 1);
        assert_eq!(dropped.bytes, second_bytes as u64);
        assert!(buffer.provider.is_empty());
        assert_eq!(buffer.provider_bytes, 0);
        assert_eq!(buffer.provider_dropped, DroppedEvents::default());

        buffer.push_provider(ProviderEvent::Completed {
            response_id: None,
            model: None,
        });
        assert_eq!(buffer.take_provider().0.len(), 1);
    }

    #[test]
    fn agent_admission_drain_and_oversize_drop_keep_exact_bytes() {
        let recovery = AgentEvent::Error {
            message: "recoverable".into(),
        };
        let accepted = AgentEvent::TurnAccepted {
            turn_id: "turn-byte-accounting".into(),
            mode: crate::protocol::TurnMode::StartIfIdle,
        };
        let recovery_bytes = serialized_event_len(&recovery);
        let accepted_bytes = serialized_event_len(&accepted);
        let mut buffer = AsyncEventBuffer::default();
        buffer.push_agent(recovery);
        buffer.push_agent(accepted);
        assert_eq!(buffer.agent_bytes, recovery_bytes + accepted_bytes);

        let admission = buffer.take_admission();
        assert_eq!(admission.len(), 2);
        assert!(buffer.agent.is_empty());
        assert_eq!(buffer.agent_bytes, 0);

        let oversized = AgentEvent::Error {
            message: "z".repeat(MAX_ASYNC_AGENT_BYTES),
        };
        let oversized_bytes = serialized_event_len(&oversized);
        assert!(oversized_bytes > MAX_ASYNC_AGENT_BYTES);
        buffer.push_agent(oversized);
        assert!(buffer.agent.is_empty());
        assert_eq!(buffer.agent_bytes, 0);

        let (events, dropped) = buffer.take_agent();
        assert!(events.is_empty());
        assert_eq!(dropped.count, 1);
        assert_eq!(dropped.bytes, oversized_bytes as u64);
        assert_eq!(buffer.agent_dropped, DroppedEvents::default());
        assert_eq!(buffer.agent_bytes, 0);
    }

    #[test]
    fn admission_marker_survives_full_count_mailbox() {
        let filler = AgentEvent::Error {
            message: "filler".into(),
        };
        let accepted = AgentEvent::TurnAccepted {
            turn_id: "turn-priority-count".into(),
            mode: crate::protocol::TurnMode::StartIfIdle,
        };
        let mut buffer = AsyncEventBuffer::default();
        for _ in 0..MAX_ASYNC_AGENT_EVENTS {
            buffer.push_agent(filler.clone());
        }
        assert_eq!(buffer.agent.len(), MAX_ASYNC_AGENT_EVENTS);

        buffer.push_agent(accepted.clone());

        assert_eq!(buffer.agent.len(), MAX_ASYNC_AGENT_EVENTS);
        assert!(buffer.agent.iter().any(|event| event.event == accepted));
        assert_eq!(buffer.agent_dropped.count, 1);
        let admission = buffer.take_admission();
        assert!(admission.iter().any(|event| event == &accepted));
    }

    #[test]
    fn admission_marker_survives_full_byte_mailbox() {
        let accepted = AgentEvent::TurnAccepted {
            turn_id: "turn-priority-bytes".into(),
            mode: crate::protocol::TurnMode::StartIfIdle,
        };
        let accepted_bytes = serialized_event_len(&accepted);
        // Leave the filler just below the aggregate limit, but not enough
        // room for the marker. The priority path must evict that ordinary
        // event and retain the admission boundary.
        let filler = AgentEvent::Error {
            message: "x".repeat(
                MAX_ASYNC_AGENT_BYTES
                    .saturating_sub(accepted_bytes)
                    .saturating_add(1),
            ),
        };
        let filler_bytes = serialized_event_len(&filler);
        assert!(filler_bytes <= MAX_ASYNC_AGENT_BYTES);
        assert!(filler_bytes.saturating_add(accepted_bytes) > MAX_ASYNC_AGENT_BYTES);

        let mut buffer = AsyncEventBuffer::default();
        buffer.push_agent(filler);
        assert_eq!(buffer.agent.len(), 1);
        buffer.push_agent(accepted.clone());

        assert_eq!(buffer.agent.len(), 1);
        assert!(buffer.agent.iter().any(|event| event.event == accepted));
        assert_eq!(buffer.agent_dropped.count, 1);
        assert_eq!(buffer.agent_dropped.bytes, filler_bytes as u64);
        assert!(
            buffer
                .take_admission()
                .iter()
                .any(|event| event == &accepted)
        );
    }

    #[test]
    fn interleaved_admission_markers_are_drained_before_provider_events() {
        let first = AgentEvent::TurnAccepted {
            turn_id: "turn-first".into(),
            mode: crate::protocol::TurnMode::StartIfIdle,
        };
        let second = AgentEvent::TurnRejected {
            reason: crate::core::NotSubmittedReason::NotIdle,
        };
        let middle = AgentEvent::Error {
            message: "middle".into(),
        };
        let mut buffer = AsyncEventBuffer::default();
        buffer.push_agent(first.clone());
        buffer.push_agent(middle.clone());
        buffer.push_agent(second.clone());

        let drained = buffer.take_admission();
        assert_eq!(drained, vec![first, middle, second]);
        assert!(buffer.agent.is_empty());
        assert_eq!(buffer.agent_bytes, 0);
    }
}
