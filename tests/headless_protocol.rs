use serde_json::Value;
use std::io::Cursor;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tempfile::tempdir;
use zenpi::{
    approval::ApprovalDecision, core::Agent, headless::run_headless, protocol::parse_line,
    session::SessionStore,
};

fn json_lines(bytes: &[u8]) -> Vec<Value> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn malformed_frame_isolated_from_all_commands() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("wire.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let input = b"{bad}\n{\"type\":\"prompt\",\"id\":\"p\",\"text\":\"a\\u2028b\",\"mode\":\"start_if_idle\"}\n{\"type\":\"status\",\"id\":\"s\"}\n{\"type\":\"handoff\",\"id\":\"h\",\"to\":\"worker\",\"summary\":\"ready\",\"artifacts\":[\"Docs/plan.md\"]}\n{\"type\":\"shutdown\",\"id\":\"q\"}\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input), &mut output).unwrap();
    let records = json_lines(&output);
    assert!(records.iter().any(|v| v["success"] == false));
    for id in ["p", "s", "h", "q"] {
        assert!(
            records
                .iter()
                .any(|v| v["id"] == id && v["success"] == true)
        );
    }
    let journal = json_lines(&std::fs::read(&path).unwrap());
    assert!(journal.iter().any(|v| v["kind"] == "handoff_record"));
    let seq: Vec<u64> = journal.iter().map(|v| v["seq"].as_u64().unwrap()).collect();
    assert!(seq.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn unsupported_version_and_missing_id_are_rejected() {
    assert!(
        parse_line(r#"{"schema_version":3,"type":"status","id":"x"}"#)
            .unwrap()
            .into_command()
            .is_err()
    );
    assert!(
        parse_line(r#"{"type":"status"}"#)
            .unwrap()
            .into_command()
            .is_err()
    );
}

#[test]
fn approval_command_is_typed_and_bounded() {
    let command = parse_line(
        r#"{"type":"approve","id":"a","approval_id":"approval-call-1","decision":"allow","remember":true}"#,
    )
    .unwrap()
    .into_command()
    .unwrap();
    assert!(matches!(
        command,
        zenpi::protocol::Command::Approve {
            approval_id,
            decision: ApprovalDecision::Allow,
            remember: true,
        } if approval_id == "approval-call-1"
    ));
}

#[test]
fn sequential_steer_and_eof_are_safe_terminal_paths() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("steer-eof.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let input = b"{\"type\":\"prompt\",\"id\":\"p\",\"text\":\"hello\",\"mode\":\"start_if_idle\"}\n{\"type\":\"steer\",\"id\":\"s\",\"text\":\"follow-up\"}\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input), &mut output).unwrap();
    let records = json_lines(&output);
    assert!(
        records
            .iter()
            .any(|value| value["id"] == "p" && value["success"] == true)
    );
    assert!(
        records
            .iter()
            .any(|value| value["id"] == "s" && value["success"] == true)
    );
    assert_eq!(agent.phase(), zenpi::core::AgentPhase::Closed);
}

#[test]
fn protocol_v2_accepts_resume_sequence_and_v1_remains_supported() {
    let v2 = parse_line(r#"{"schema_version":2,"type":"resume","id":"r2","from_sequence":7}"#)
        .unwrap()
        .into_command()
        .unwrap();
    assert!(matches!(
        v2,
        zenpi::protocol::Command::Resume {
            path: None,
            from_sequence: Some(7)
        }
    ));
    let v1 = parse_line(r#"{"schema_version":1,"type":"status","id":"r1"}"#)
        .unwrap()
        .into_command();
    assert!(v1.is_ok());
    let response =
        zenpi::protocol::StdioResponse::success(Some("r2".into()), "status", None).for_version(2);
    assert_eq!(response.schema_version, 2);
}

#[test]
fn in_flight_duplicate_request_id_is_suppressed_before_side_effects() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || -> Result<(), String> {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let body = read_http_body(&mut stream)?;
        if body.to_string().contains("duplicate prompt") {
            return Err("duplicate prompt reached provider".into());
        }
        thread::sleep(Duration::from_millis(100));
        let payload = "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"one answer\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"once\"}}\n\n";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        )
        .map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        thread::sleep(Duration::from_millis(100));
        if listener.accept().is_ok() {
            return Err("duplicate request opened a second provider connection".into());
        }
        Ok(())
    });
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("duplicate.jsonl");
    let backend = zenpi::backend::OpenAiCompatibleBackend::new_with_wire_api(
        format!("http://127.0.0.1:{port}"),
        None,
        "mock-model",
        zenpi::backend::OpenAiWireApi::Responses,
    )
    .unwrap();
    let agent = Agent::new(
        SessionStore::open(&session_path).unwrap(),
        Box::new(backend),
    );
    let (mut writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let output = SharedWriter::default();
    let captured = output.clone();
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(agent, reader, output).unwrap();
    });
    writeln!(
        writer,
        "{}",
        serde_json::json!({"schema_version":2,"type":"prompt","id":"same","text":"original prompt"})
    )
    .unwrap();
    writeln!(writer, "{}", serde_json::json!({"schema_version":2,"type":"prompt","id":"same","text":"duplicate prompt"})).unwrap();
    wait_for_output(&captured, "one answer");
    writeln!(
        writer,
        "{}",
        serde_json::json!({"schema_version":2,"type":"shutdown","id":"stop"})
    )
    .unwrap();
    drop(writer);
    host.join().unwrap();
    server.join().unwrap().unwrap();
    let records = json_lines(&captured.0.lock().unwrap());
    assert!(records.iter().any(|record| {
        record["id"] == "same" && record["code"] == "duplicate_request_in_flight"
    }));
    let session = SessionStore::open(session_path).unwrap();
    assert_eq!(
        session
            .turns()
            .iter()
            .filter(|turn| turn.role == zenpi::core::TurnRole::User)
            .count(),
        1
    );
}

#[derive(Clone, Default)]
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn live_steer_cancels_and_reissues_without_losing_or_duplicating_input() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || -> Result<(), String> {
        let (mut first, _) = listener.accept().map_err(|error| error.to_string())?;
        read_http_body(&mut first)?;
        let first_payload = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-1\"}}\n\n"
        );
        write!(
            first,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{first_payload}",
            first_payload.len()
        )
        .map_err(|error| error.to_string())?;
        first.flush().map_err(|error| error.to_string())?;

        let (mut second, _) = listener.accept().map_err(|error| error.to_string())?;
        let body = read_http_body(&mut second)?;
        let occurrences = body
            .to_string()
            .matches("use the corrected request")
            .count();
        if occurrences != 1 {
            return Err(format!(
                "steer appeared {occurrences} times in reissue body: {body}"
            ));
        }
        let payload = "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"corrected answer\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-2\"}}\n\n";
        write!(
            second,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    });

    let dir = tempdir().unwrap();
    let path = dir.path().join("live-steer.jsonl");
    let backend = zenpi::backend::OpenAiCompatibleBackend::new_with_wire_api(
        format!("http://127.0.0.1:{port}"),
        None,
        "mock-model",
        zenpi::backend::OpenAiWireApi::Responses,
    )
    .unwrap();
    let agent = Agent::new(SessionStore::open(&path).unwrap(), Box::new(backend));
    let (mut input_writer, input_reader) = std::os::unix::net::UnixStream::pair().unwrap();
    let output = SharedWriter::default();
    let captured = output.clone();
    let host = thread::spawn(move || {
        zenpi::headless::run_async_streams(agent, input_reader, output).unwrap();
    });
    writeln!(
        input_writer,
        "{}",
        serde_json::json!({"schema_version":2,"type":"prompt","id":"p1","text":"original request"})
    )
    .unwrap();
    wait_for_output(&captured, "partial");
    writeln!(
        input_writer,
        "{}",
        serde_json::json!({"schema_version":2,"type":"steer","id":"s1","text":"use the corrected request"})
    )
    .unwrap();
    wait_for_output(&captured, "corrected answer");
    writeln!(
        input_writer,
        "{}",
        serde_json::json!({"schema_version":2,"type":"shutdown","id":"q1"})
    )
    .unwrap();
    drop(input_writer);
    host.join().unwrap();
    server.join().unwrap().unwrap();

    let output = captured.0.lock().unwrap().clone();
    let records = json_lines(&output);
    assert_eq!(
        records
            .iter()
            .filter(|record| record["id"] == "p1" && record["type"] == "response")
            .count(),
        1
    );
    assert!(
        records
            .iter()
            .any(|record| { record["id"] == "s1" && record["success"] == true })
    );
    let session = SessionStore::open(path).unwrap();
    assert_eq!(
        session
            .turns()
            .iter()
            .filter(|turn| turn.content == "use the corrected request")
            .count(),
        1
    );
    assert_eq!(
        session
            .turns()
            .iter()
            .filter(|turn| {
                turn.role == zenpi::core::TurnRole::Assistant && turn.content == "corrected answer"
            })
            .count(),
        1
    );
}

fn wait_for_output(output: &SharedWriter, needle: &str) {
    for _ in 0..300 {
        if String::from_utf8_lossy(&output.0.lock().unwrap()).contains(needle) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for {needle}; output={}",
        String::from_utf8_lossy(&output.0.lock().unwrap())
    );
}

fn read_http_body(stream: &mut TcpStream) -> Result<Value, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("request ended before headers".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        })
        .ok_or_else(|| "missing content length".to_owned())?;
    while bytes.len() < header_end + length {
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("request ended before body".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    serde_json::from_slice(&bytes[header_end..header_end + length])
        .map_err(|error| error.to_string())
}
