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
    core::{Agent, AgentError, ProcessResult, TurnInputRequest},
    protocol::{Command, StdioResponse, encode_line, parse_line},
    session::unix_time_ms,
};

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
        let should_stop = handle_command(agent, id, command, &mut output)?;
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

struct AsyncWork {
    id: Option<String>,
    command: &'static str,
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
    let worker_state = Arc::clone(&shared);
    let runner = crate::runtime::BackgroundRunner::spawn(
        move |request: TurnInputRequest, token| -> Result<ProcessResult, AgentError> {
            if token.is_cancelled() {
                return Err(AgentError::Backend(crate::backend::BackendError::Cancelled));
            }
            let mut agent = worker_state
                .lock()
                .map_err(|_| AgentError::InvalidTurn("agent lock poisoned".into()))?;
            let result = agent.process(request)?;
            if token.is_cancelled() {
                return Err(AgentError::Backend(crate::backend::BackendError::Cancelled));
            }
            Ok(result)
        },
        crate::runtime::RuntimeConfig::default(),
    );
    let mut jobs: HashMap<crate::runtime::JobId, AsyncWork> = HashMap::new();
    let mut stopping = false;
    let mut shutdown_sent = false;
    loop {
        while let Ok(event) = runner.try_next_event() {
            if handle_runtime_event(
                event,
                &mut jobs,
                &shared,
                &mut output,
                stopping,
                shutdown_sent,
            )? {
                return Ok(());
            }
        }
        if stopping {
            if jobs.is_empty() && !shutdown_sent {
                let _ = runner.try_shutdown();
                shutdown_sent = true;
            }
            match runner.recv_timeout(Duration::from_millis(10)) {
                Ok(event) => {
                    if handle_runtime_event(
                        event,
                        &mut jobs,
                        &shared,
                        &mut output,
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
                &runner,
                &mut jobs,
                &mut output,
                &mut stopping,
            )?,
            Ok(ReaderMessage::Eof) => {
                stopping = true;
            }
            Ok(ReaderMessage::Error(error)) => {
                write_response(
                    &mut output,
                    StdioResponse::error_with_code(None, "stdio", "io_error", error.to_string()),
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

fn handle_runtime_event<W: Write>(
    event: crate::runtime::RuntimeEvent<ProcessResult, AgentError>,
    jobs: &mut HashMap<crate::runtime::JobId, AsyncWork>,
    shared: &Arc<Mutex<Agent>>,
    output: &mut W,
    stopping: bool,
    shutdown_sent: bool,
) -> Result<bool, HeadlessError> {
    use crate::runtime::{JobOutcome, RuntimeEvent};
    match event {
        RuntimeEvent::Completed { id, outcome } => {
            let Some(meta) = jobs.remove(&id) else {
                return Ok(false);
            };
            match outcome {
                JobOutcome::Succeeded(result) => {
                    let data = json!({
                        "submission": result.submission,
                        "assistant": result.assistant,
                    });
                    write_response(
                        output,
                        StdioResponse::success(meta.id, meta.command, Some(data)),
                    )?;
                }
                JobOutcome::Failed(error) => write_response(
                    output,
                    StdioResponse::error_with_code(
                        meta.id,
                        meta.command,
                        error.code(),
                        error.to_string(),
                    ),
                )?,
                JobOutcome::Cancelled => write_response(
                    output,
                    StdioResponse::error_with_code(
                        meta.id,
                        meta.command,
                        "backend_cancelled",
                        "request was cancelled",
                    ),
                )?,
                JobOutcome::Panicked => write_response(
                    output,
                    StdioResponse::error_with_code(
                        meta.id,
                        meta.command,
                        "runtime_panic",
                        "background request panicked",
                    ),
                )?,
            }
            if let Ok(mut agent) = shared.lock() {
                write_events(&mut agent, output)?;
            }
            Ok(false)
        }
        RuntimeEvent::Closed => Ok(stopping && shutdown_sent && jobs.is_empty()),
        _ => Ok(false),
    }
}

fn process_async_line<W, F>(
    line: String,
    shared: &Arc<Mutex<Agent>>,
    runner: &crate::runtime::BackgroundRunner<TurnInputRequest, ProcessResult, AgentError, F>,
    jobs: &mut HashMap<crate::runtime::JobId, AsyncWork>,
    output: &mut W,
    stopping: &mut bool,
) -> Result<(), HeadlessError>
where
    W: Write,
    F: Fn(TurnInputRequest, crate::runtime::CancellationToken) -> Result<ProcessResult, AgentError>
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
        } => {
            let request = TurnInputRequest {
                message: text,
                expected_turn_id,
                mode,
            };
            let job_id = runner.try_submit(request).map_err(|error| {
                HeadlessError::Agent(AgentError::InvalidTurn(error.to_string()))
            })?;
            jobs.insert(
                job_id,
                AsyncWork {
                    id,
                    command: "prompt",
                },
            );
        }
        Command::Steer {
            text,
            expected_turn_id,
        } => {
            let request = TurnInputRequest {
                message: text,
                expected_turn_id,
                mode: crate::protocol::TurnMode::Steer,
            };
            let job_id = runner.try_submit(request).map_err(|error| {
                HeadlessError::Agent(AgentError::InvalidTurn(error.to_string()))
            })?;
            jobs.insert(
                job_id,
                AsyncWork {
                    id,
                    command: "steer",
                },
            );
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
            write_response(
                output,
                StdioResponse::success(id, name, Some(json!({"closed": true}))),
            )?;
            *stopping = true;
        }
        other => match shared.try_lock() {
            Ok(mut agent) => {
                let should_stop = handle_command(&mut agent, id, other, output)?;
                if should_stop {
                    *stopping = true;
                }
            }
            Err(_) => write_response(
                output,
                StdioResponse::success(id, name, Some(json!({"phase": "running", "busy": true}))),
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
) -> Result<bool, HeadlessError> {
    let name = crate::protocol::command_name(&command);
    match command {
        Command::Prompt {
            text,
            mode,
            expected_turn_id,
        } => {
            let result = agent.process(TurnInputRequest {
                message: text,
                expected_turn_id,
                mode,
            });
            match result {
                Ok(result) => {
                    let data = json!({
                        "submission": result.submission,
                        "assistant": result.assistant,
                    });
                    write_response(output, StdioResponse::success(id, name, Some(data)))?;
                    write_events(agent, output)?;
                }
                Err(error) => {
                    write_response(
                        output,
                        StdioResponse::error_with_code(id, name, error.code(), error.to_string()),
                    )?;
                    write_events(agent, output)?;
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
            });
            match result {
                Ok(result) => {
                    let data = json!({
                        "submission": result.submission,
                        "assistant": result.assistant,
                    });
                    write_response(output, StdioResponse::success(id, name, Some(data)))?;
                    write_events(agent, output)?;
                }
                Err(error) => {
                    write_response(
                        output,
                        StdioResponse::error_with_code(id, name, error.code(), error.to_string()),
                    )?;
                    write_events(agent, output)?;
                }
            }
        }
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
                                id,
                                name,
                                Some(json!({"accepted": true, "handoff": handoff})),
                            ),
                        )?;
                        write_events(agent, output)?;
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
        Command::Resume { path } => match path {
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

fn write_events<W: Write>(agent: &mut Agent, output: &mut W) -> Result<(), HeadlessError> {
    for event in agent.take_events() {
        let line = encode_line(&event)?;
        output.write_all(line.as_bytes())?;
    }
    output.flush()?;
    Ok(())
}
