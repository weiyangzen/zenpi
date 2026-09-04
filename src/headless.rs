//! Strict stdin/stdout JSONL mode.
//!
//! Only this module owns wire I/O.  The core remains synchronous and
//! testable, while composed agents can connect with a pipe and receive one
//! correlated response per input line.

use std::{
    collections::HashMap,
    io::{self, BufRead, Write},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use serde_json::json;
use thiserror::Error;

use crate::{
    b3::{HandoffRecord, unix_ms_to_rfc3339},
    backend::ProviderEvent,
    core::{Agent, AgentError, ProcessResult, TurnInputRequest},
    protocol::{Command, StdioEvent, StdioResponse, encode_line, parse_line},
    session::unix_time_ms,
};

const MAX_REPLAY_EVENTS: usize = 4096;

#[derive(Default)]
struct ReplayState {
    events: std::collections::VecDeque<(u64, String)>,
    terminals: HashMap<String, String>,
}

impl ReplayState {
    fn remember_event(&mut self, sequence: u64, line: String) {
        if self.events.len() == MAX_REPLAY_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back((sequence, line));
    }

    fn replay_from<W: Write>(&self, from_sequence: u64, output: &mut W) -> io::Result<usize> {
        let mut count = 0;
        for (_, line) in self
            .events
            .iter()
            .filter(|(sequence, _)| *sequence >= from_sequence)
        {
            output.write_all(line.as_bytes())?;
            count += 1;
        }
        output.flush()?;
        Ok(count)
    }
}

#[derive(Debug, Error)]
pub enum HeadlessError {
    #[error("headless I/O: {0}")]
    Io(#[from] io::Error),
    #[error("headless encoding: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("headless agent: {0}")]
    Agent(#[from] AgentError),
}

/// Run the headless loop over arbitrary buffered input/output.  This generic
/// form is used by integration tests and embedders; `run_stdio` supplies the
/// process stdin/stdout handles.
pub fn run_headless<R: BufRead, W: Write>(
    agent: &mut Agent,
    mut input: R,
    mut output: W,
) -> Result<(), HeadlessError> {
    let mut event_sequence = 0_u64;
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
        let command = match request.into_command() {
            Ok(command) => command,
            Err(error) => {
                write_response(
                    &mut output,
                    StdioResponse::error_with_code(
                        id,
                        command_kind,
                        error.code(),
                        error.to_string(),
                    ),
                )?;
                continue;
            }
        };
        let should_stop = handle_command(agent, id, command, &mut output, &mut event_sequence)?;
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
    events: Arc<Mutex<Vec<ProviderEvent>>>,
    turn_id: Arc<Mutex<Option<String>>>,
}

enum AsyncTurn {
    Standard(TurnInputRequest),
    Reissue {
        message: String,
        superseded_turn_id: String,
    },
}

struct AsyncRequest {
    turn: AsyncTurn,
    events: Arc<Mutex<Vec<ProviderEvent>>>,
    started_turn_id: Arc<Mutex<Option<String>>>,
}

type ProviderEventQueue = Arc<Mutex<Vec<ProviderEvent>>>;
type StartedTurn = Arc<Mutex<Option<String>>>;
type AsyncRequestParts = (AsyncRequest, ProviderEventQueue, StartedTurn);

impl AsyncRequest {
    fn new(request: TurnInputRequest) -> AsyncRequestParts {
        let events = Arc::new(Mutex::new(Vec::new()));
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
        let events = Arc::new(Mutex::new(Vec::new()));
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
}

fn drain_provider_events<W: Write>(
    jobs: &mut HashMap<crate::runtime::JobId, AsyncWork>,
    output: &mut W,
    sequence: &mut u64,
    replay: &mut ReplayState,
) -> Result<(), HeadlessError> {
    for work in jobs.values() {
        let Ok(mut events) = work.events.lock() else {
            continue;
        };
        for event in events.drain(..) {
            let value = serde_json::to_value(event)?;
            let turn_id = value
                .get("turn_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let envelope = StdioEvent::new(*sequence, work.id.clone(), turn_id, value);
            let current = *sequence;
            *sequence = sequence.saturating_add(1);
            let line = encode_line(&envelope)?;
            replay.remember_event(current, line.clone());
            output.write_all(line.as_bytes())?;
        }
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
        let request_id = jobs
            .values()
            .find(|work| work.command == "prompt" || work.command == "steer")
            .and_then(|work| work.id.clone());
        let turn_id = Some(request.turn_id.clone());
        let value = serde_json::json!({
            "type": "approval_request",
            "approval": request,
        });
        let envelope = StdioEvent::new(*sequence, request_id, turn_id, value);
        let current = *sequence;
        *sequence = sequence.saturating_add(1);
        let line = encode_line(&envelope)?;
        replay.remember_event(current, line.clone());
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
    let Ok(mut events) = work.events.lock() else {
        return Ok(());
    };
    for event in events.drain(..) {
        let value = serde_json::to_value(event)?;
        let turn_id = value
            .get("turn_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let envelope = StdioEvent::new(*sequence, work.id.clone(), turn_id, value);
        let current = *sequence;
        *sequence = sequence.saturating_add(1);
        let line = encode_line(&envelope)?;
        replay.remember_event(current, line.clone());
        output.write_all(line.as_bytes())?;
    }
    Ok(())
}

enum ReaderMessage {
    Line(String),
    Eof,
    Error(io::Error),
}

fn run_async_stdio<R: io::Read + Send + 'static, W: Write>(
    agent: Agent,
    input: R,
    mut output: W,
) -> Result<(), HeadlessError> {
    let (line_tx, line_rx) = mpsc::sync_channel::<ReaderMessage>(32);
    thread::Builder::new()
        .name("zenpi-stdin-reader".into())
        .spawn(move || {
            let mut reader = io::BufReader::new(input);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        let _ = line_tx.send(ReaderMessage::Eof);
                        break;
                    }
                    Ok(_) => {
                        let frame = line.strip_suffix('\n').unwrap_or(&line).to_owned();
                        if line_tx.send(ReaderMessage::Line(frame)).is_err() {
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
        move |request: AsyncRequest, token| -> Result<ProcessResult, AgentError> {
            if token.is_cancelled() {
                return Err(AgentError::Backend(crate::backend::BackendError::Cancelled));
            }
            let mut agent = worker_state
                .lock()
                .map_err(|_| AgentError::InvalidTurn("agent lock poisoned".into()))?;
            let events = Arc::clone(&request.events);
            let started_turn_id = Arc::clone(&request.started_turn_id);
            let mut sink = |event: ProviderEvent| {
                if let Ok(mut pending) = events.lock() {
                    pending.push(event);
                }
                Ok(())
            };
            let result = match request.turn {
                AsyncTurn::Standard(request) => {
                    let submission = agent.submit(request)?;
                    if let Ok(mut active) = started_turn_id.lock() {
                        *active = submission.turn_id().map(str::to_owned);
                    }
                    let assistant = if submission.accepted() {
                        agent.run_active_turn_cancelable_with_events(
                            || token.is_cancelled(),
                            &mut sink,
                        )?
                    } else {
                        None
                    };
                    ProcessResult {
                        submission,
                        assistant,
                    }
                }
                AsyncTurn::Reissue {
                    message,
                    superseded_turn_id,
                } => {
                    let submission = agent.start_steer_reissue(message, &superseded_turn_id)?;
                    if let Ok(mut active) = started_turn_id.lock() {
                        *active = submission.turn_id().map(str::to_owned);
                    }
                    let assistant = agent.run_active_turn_cancelable_with_events(
                        || token.is_cancelled(),
                        &mut sink,
                    )?;
                    ProcessResult {
                        submission,
                        assistant,
                    }
                }
            };
            if token.is_cancelled() {
                return Err(AgentError::Backend(crate::backend::BackendError::Cancelled));
            }
            Ok(result)
        },
        crate::runtime::RuntimeConfig::default(),
    );
    let mut jobs: HashMap<crate::runtime::JobId, AsyncWork> = HashMap::new();
    let mut request_to_job: HashMap<String, crate::runtime::JobId> = HashMap::new();
    let mut event_sequence = 0_u64;
    let mut replay = ReplayState::default();
    let mut stopping = false;
    let mut shutdown_sent = false;
    loop {
        drain_provider_events(&mut jobs, &mut output, &mut event_sequence, &mut replay)?;
        drain_approval_events(
            approval.as_ref(),
            &jobs,
            &mut output,
            &mut event_sequence,
            &mut replay,
        )?;
        while let Ok(event) = runner.try_next_event() {
            if handle_runtime_event(
                event,
                &mut jobs,
                &mut request_to_job,
                &shared,
                &mut output,
                &mut event_sequence,
                &mut replay,
                stopping,
                shutdown_sent,
            )? {
                return Ok(());
            }
        }
        if stopping {
            drain_provider_events(&mut jobs, &mut output, &mut event_sequence, &mut replay)?;
            if jobs.is_empty() && !shutdown_sent {
                let _ = runner.try_shutdown();
                shutdown_sent = true;
            }
            match runner.recv_timeout(Duration::from_millis(10)) {
                Ok(event) => {
                    if handle_runtime_event(
                        event,
                        &mut jobs,
                        &mut request_to_job,
                        &shared,
                        &mut output,
                        &mut event_sequence,
                        &mut replay,
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
            Ok(ReaderMessage::Line(line)) => process_async_line(
                line,
                &shared,
                approval.as_ref(),
                &runner,
                &mut jobs,
                &mut request_to_job,
                &mut output,
                &mut event_sequence,
                &mut replay,
                &mut stopping,
            )?,
            Ok(ReaderMessage::Eof) => {
                stopping = true;
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
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                stopping = true;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_runtime_event<W: Write>(
    event: crate::runtime::RuntimeEvent<ProcessResult, AgentError>,
    jobs: &mut HashMap<crate::runtime::JobId, AsyncWork>,
    request_to_job: &mut HashMap<String, crate::runtime::JobId>,
    shared: &Arc<Mutex<Agent>>,
    output: &mut W,
    event_sequence: &mut u64,
    replay: &mut ReplayState,
    stopping: bool,
    shutdown_sent: bool,
) -> Result<bool, HeadlessError> {
    use crate::runtime::{JobOutcome, RuntimeEvent};
    match event {
        RuntimeEvent::Completed { id, outcome } => {
            let Some(meta) = jobs.remove(&id) else {
                return Ok(false);
            };
            let event_request_id = meta.id.clone();
            if let Some(request_id) = meta.id.as_ref() {
                request_to_job.remove(request_id);
            }
            // Provider deltas precede the one terminal response, even when
            // the worker and output loop become ready on the same tick.
            drain_provider_events_for_job(&meta, output, event_sequence, replay)?;
            match outcome {
                JobOutcome::Succeeded(result) => {
                    let data = json!({
                        "submission": result.submission,
                        "assistant": result.assistant,
                    });
                    write_cached_response(
                        output,
                        StdioResponse::success(meta.id.clone(), meta.command, Some(data)),
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
                    ),
                    replay,
                )?,
                JobOutcome::Cancelled => write_cached_response(
                    output,
                    StdioResponse::error_with_code(
                        meta.id.clone(),
                        meta.command,
                        "backend_cancelled",
                        "request was cancelled",
                    ),
                    replay,
                )?,
                JobOutcome::Panicked => write_cached_response(
                    output,
                    StdioResponse::error_with_code(
                        meta.id.clone(),
                        meta.command,
                        "runtime_panic",
                        "background request panicked",
                    ),
                    replay,
                )?,
            }
            if let Ok(mut agent) = shared.lock() {
                write_events(&mut agent, output, event_sequence, event_request_id)?;
            }
            Ok(false)
        }
        RuntimeEvent::Closed => Ok(stopping && shutdown_sent && jobs.is_empty()),
        _ => Ok(false),
    }
}

#[allow(clippy::too_many_arguments)]
fn process_async_line<W, F>(
    line: String,
    shared: &Arc<Mutex<Agent>>,
    approval: Option<&crate::approval::ApprovalCoordinator>,
    runner: &crate::runtime::BackgroundRunner<AsyncRequest, ProcessResult, AgentError, F>,
    jobs: &mut HashMap<crate::runtime::JobId, AsyncWork>,
    request_to_job: &mut HashMap<String, crate::runtime::JobId>,
    output: &mut W,
    event_sequence: &mut u64,
    replay: &mut ReplayState,
    stopping: &mut bool,
) -> Result<(), HeadlessError>
where
    W: Write,
    F: Fn(AsyncRequest, crate::runtime::CancellationToken) -> Result<ProcessResult, AgentError>
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
    if let Some(request_id) = id.as_ref()
        && let Some(cached) = replay.terminals.get(request_id)
    {
        output.write_all(cached.as_bytes())?;
        output.flush()?;
        return Ok(());
    }
    let command = match request.into_command() {
        Ok(command) => command,
        Err(error) => {
            write_response(
                output,
                StdioResponse::error_with_code(id, name, error.code(), error.to_string()),
            )?;
            return Ok(());
        }
    };
    match command {
        Command::Prompt {
            text,
            mode,
            expected_turn_id,
            attachments,
        } => {
            if let Some(request_id) = id.as_ref()
                && request_to_job.contains_key(request_id)
            {
                write_response(
                    output,
                    StdioResponse::error_with_code(
                        id,
                        name,
                        "duplicate_request_in_flight",
                        "request ID is already in flight",
                    ),
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
            let job_id = runner.try_submit(request).map_err(|error| {
                HeadlessError::Agent(AgentError::InvalidTurn(error.to_string()))
            })?;
            jobs.insert(
                job_id,
                AsyncWork {
                    id: id.clone(),
                    command: "prompt",
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
            if let Some(request_id) = id.as_ref()
                && request_to_job.contains_key(request_id)
            {
                write_response(
                    output,
                    StdioResponse::error_with_code(
                        id,
                        name,
                        "duplicate_request_in_flight",
                        "request ID is already in flight",
                    ),
                )?;
                return Ok(());
            }
            let active = jobs
                .iter()
                .find(|(_, work)| work.command == "prompt" || work.command == "steer")
                .map(|(job_id, work)| {
                    (
                        *job_id,
                        work.turn_id.lock().ok().and_then(|turn_id| turn_id.clone()),
                    )
                });
            let (request, events, started_turn_id) = if let Some((job_id, active_turn_id)) = active
            {
                if expected_turn_id.as_deref().is_some_and(|expected| {
                    active_turn_id
                        .as_deref()
                        .is_some_and(|active| expected != active)
                }) {
                    write_response(
                        output,
                        StdioResponse::error_with_code(
                            id,
                            name,
                            "expected_turn_mismatch",
                            "expected turn does not match the active request",
                        ),
                    )?;
                    return Ok(());
                }
                let Some(active_turn_id) = active_turn_id else {
                    let request = TurnInputRequest {
                        message: text,
                        expected_turn_id,
                        mode: crate::protocol::TurnMode::Steer,
                        attachments: Vec::new(),
                    };
                    let (request, events, started_turn_id) = AsyncRequest::new(request);
                    let job_id = runner.try_submit(request).map_err(|error| {
                        HeadlessError::Agent(AgentError::InvalidTurn(error.to_string()))
                    })?;
                    jobs.insert(
                        job_id,
                        AsyncWork {
                            id: id.clone(),
                            command: "steer",
                            events,
                            turn_id: started_turn_id,
                        },
                    );
                    if let Some(request_id) = id {
                        request_to_job.insert(request_id, job_id);
                    }
                    return Ok(());
                };
                runner.try_cancel(job_id).map_err(|error| {
                    HeadlessError::Agent(AgentError::InvalidTurn(error.to_string()))
                })?;
                let (request, events, started_turn_id) =
                    AsyncRequest::reissue(text, active_turn_id);
                (request, events, started_turn_id)
            } else {
                let request = TurnInputRequest {
                    message: text,
                    expected_turn_id,
                    mode: crate::protocol::TurnMode::Steer,
                    attachments: Vec::new(),
                };
                let (request, events, started_turn_id) = AsyncRequest::new(request);
                (request, events, started_turn_id)
            };
            let job_id = runner.try_submit(request).map_err(|error| {
                HeadlessError::Agent(AgentError::InvalidTurn(error.to_string()))
            })?;
            jobs.insert(
                job_id,
                AsyncWork {
                    id: id.clone(),
                    command: "steer",
                    events,
                    turn_id: started_turn_id,
                },
            );
            if let Some(request_id) = id.clone() {
                request_to_job.insert(request_id, job_id);
            }
        }
        Command::Cancel { target_id } => {
            let Some(job_id) = request_to_job.get(&target_id).copied() else {
                write_response(
                    output,
                    StdioResponse::error_with_code(
                        id,
                        name,
                        "unknown_request",
                        "target request is not running",
                    ),
                )?;
                return Ok(());
            };
            runner.try_cancel(job_id).map_err(|error| {
                HeadlessError::Agent(AgentError::InvalidTurn(error.to_string()))
            })?;
            write_response(
                output,
                StdioResponse::success(
                    id,
                    name,
                    Some(json!({ "target_id": target_id, "cancel_requested": true })),
                ),
            )?;
        }
        Command::Status => match shared.try_lock() {
            Ok(agent) => write_response(
                output,
                StdioResponse::success(id, name, Some(serde_json::to_value(agent.snapshot())?)),
            )?,
            Err(_) => write_response(
                output,
                StdioResponse::success(id, name, Some(json!({"phase": "running", "busy": true}))),
            )?,
        },
        Command::Shutdown => {
            // Shutdown is an explicit cancellation boundary. Report that the
            // close was accepted; the outer loop then drains every admitted
            // job to a terminal response before the process exits.
            write_response(
                output,
                StdioResponse::success(id, name, Some(json!({"closing": true}))),
            )?;
            *stopping = true;
        }
        Command::Approve {
            approval_id,
            decision,
            remember,
        } => {
            let Some(approval) = approval else {
                write_response(
                    output,
                    StdioResponse::error_with_code(
                        id,
                        name,
                        "approval_unavailable",
                        "no approval coordinator is configured",
                    ),
                )?;
                return Ok(());
            };
            match approval.respond(crate::approval::ApprovalResponse {
                request_id: approval_id.clone(),
                decision,
                remember,
            }) {
                Ok(()) => write_response(
                    output,
                    StdioResponse::success(
                        id,
                        name,
                        Some(json!({"approval_id": approval_id, "accepted": true})),
                    ),
                )?,
                Err(error) => write_response(
                    output,
                    StdioResponse::error_with_code(id, name, "approval_error", error.to_string()),
                )?,
            }
        }
        Command::Resume {
            path: None,
            from_sequence: Some(from_sequence),
        } => {
            let count = replay.replay_from(from_sequence, output)?;
            write_response(
                output,
                StdioResponse::success(
                    id,
                    name,
                    Some(json!({
                        "from_sequence": from_sequence,
                        "replayed": count,
                        "next_sequence": *event_sequence,
                    })),
                )
                .for_version(request_version),
            )?;
        }
        other => match shared.try_lock() {
            Ok(mut agent) => {
                let should_stop = handle_command(&mut agent, id, other, output, event_sequence)?;
                if should_stop {
                    *stopping = true;
                }
            }
            Err(_) => write_response(
                output,
                StdioResponse::error_with_code(
                    id,
                    name,
                    "agent_busy",
                    "command cannot run while a turn owns the session",
                ),
            )?,
        },
    }
    Ok(())
}

fn handle_command<W: Write>(
    agent: &mut Agent,
    id: Option<String>,
    command: Command,
    output: &mut W,
    event_sequence: &mut u64,
) -> Result<bool, HeadlessError> {
    let name = crate::protocol::command_name(&command);
    match command {
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
                    write_response(output, StdioResponse::success(id.clone(), name, Some(data)))?;
                    write_events(agent, output, event_sequence, id.clone())?;
                }
                Err(error) => {
                    write_response(
                        output,
                        StdioResponse::error_with_code(
                            id.clone(),
                            name,
                            error.code(),
                            error.to_string(),
                        ),
                    )?;
                    write_events(agent, output, event_sequence, id.clone())?;
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
                    write_response(output, StdioResponse::success(id.clone(), name, Some(data)))?;
                    write_events(agent, output, event_sequence, id.clone())?;
                }
                Err(error) => {
                    write_response(
                        output,
                        StdioResponse::error_with_code(
                            id.clone(),
                            name,
                            error.code(),
                            error.to_string(),
                        ),
                    )?;
                    write_events(agent, output, event_sequence, id.clone())?;
                }
            }
        }
        Command::Cancel { .. } => write_response(
            output,
            StdioResponse::error_with_code(
                id,
                name,
                "unsupported_sync_command",
                "cancel is available in asynchronous stdio mode",
            ),
        )?,
        Command::Status => {
            write_response(
                output,
                StdioResponse::success(id, name, Some(serde_json::to_value(agent.snapshot())?)),
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
                        write_response(
                            output,
                            StdioResponse::success(
                                id.clone(),
                                name,
                                Some(json!({"accepted": true, "handoff": handoff})),
                            ),
                        )?;
                        write_events(agent, output, event_sequence, id.clone())?;
                    }
                    Err(error) => {
                        write_response(
                            output,
                            StdioResponse::error_with_code(
                                id,
                                name,
                                error.code(),
                                error.to_string(),
                            ),
                        )?;
                    }
                },
                Err(error) => write_response(
                    output,
                    StdioResponse::error_with_code(id, name, "handoff_error", error.to_string()),
                )?,
            }
        }
        Command::Resume {
            path,
            from_sequence: _,
        } => match path {
            Some(path) => match agent.resume_session(path) {
                Ok(()) => write_response(
                    output,
                    StdioResponse::success(id, name, Some(serde_json::to_value(agent.snapshot())?)),
                )?,
                Err(error) => write_response(
                    output,
                    StdioResponse::error_with_code(id, name, error.code(), error.to_string()),
                )?,
            },
            None => write_response(
                output,
                StdioResponse::success(id, name, Some(serde_json::to_value(agent.snapshot())?)),
            )?,
        },
        Command::Approve { .. } => write_response(
            output,
            StdioResponse::error_with_code(
                id,
                name,
                "approval_unavailable",
                "approval responses require the asynchronous headless host",
            ),
        )?,
        Command::Shutdown => {
            agent.close();
            write_response(
                output,
                StdioResponse::success(id, name, Some(json!({"closed": true}))),
            )?;
            return Ok(true);
        }
    }
    Ok(false)
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
    output.write_all(line.as_bytes())?;
    output.flush()?;
    if let Some(request_id) = request_id {
        replay.terminals.insert(request_id, line);
    }
    Ok(())
}

fn write_events<W: Write>(
    agent: &mut Agent,
    output: &mut W,
    sequence: &mut u64,
    request_id: Option<String>,
) -> Result<(), HeadlessError> {
    for event in agent.take_events() {
        let line = if request_id.is_some() {
            let value = serde_json::to_value(&event)?;
            let turn_id = value
                .get("turn_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let envelope = StdioEvent::new(*sequence, request_id.clone(), turn_id, value);
            encode_line(&envelope)?
        } else {
            let value = serde_json::to_value(&event)?;
            let turn_id = value
                .get("turn_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let envelope = StdioEvent::new(*sequence, None, turn_id, value);
            encode_line(&envelope)?
        };
        *sequence = sequence.saturating_add(1);
        output.write_all(line.as_bytes())?;
    }
    output.flush()?;
    Ok(())
}
