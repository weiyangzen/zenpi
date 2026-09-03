//! Strict stdin/stdout JSONL mode.
//!
//! Only this module owns wire I/O.  The core remains synchronous and
//! testable, while composed agents can connect with a pipe and receive one
//! correlated response per input line.

use std::io::{self, BufRead, Write};

use serde_json::json;
use thiserror::Error;

use crate::{
    b3::{HandoffRecord, unix_ms_to_rfc3339},
    core::{Agent, AgentError, TurnInputRequest},
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
