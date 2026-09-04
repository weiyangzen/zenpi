use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use tempfile::tempdir;
use zenpi::backend::{
    AttachmentKind, BackendError, InputAttachment, OpenAiCompatibleBackend, OpenAiWireApi,
    ProviderCapabilities,
};
use zenpi::core::{Agent, TurnInputRequest};
use zenpi::session::SessionStore;

fn respond(mut stream: TcpStream, wire_api: OpenAiWireApi) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    let mut request = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("fixture client closed before sending HTTP headers".into());
        }
        request.extend_from_slice(&chunk[..count]);
        if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
        if request.len() >= 64 * 1024 {
            return Err("fixture request headers are too large".into());
        }
    };
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let expected_path = match wire_api {
        OpenAiWireApi::ChatCompletions => "/v1/chat/completions",
        OpenAiWireApi::Responses => "/v1/responses",
    };
    if !headers.starts_with(&format!("POST {expected_path} HTTP/1.1")) {
        return Err(format!("unexpected request line: {headers}"));
    }
    if !headers
        .to_ascii_lowercase()
        .contains("authorization: bearer test-key")
    {
        return Err("fixture request omitted the bearer token".into());
    }
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.strip_prefix("content-length:")
                .or_else(|| line.strip_prefix("Content-Length:"))
        })
        .and_then(|value| value.trim().parse::<usize>().ok())
        .ok_or_else(|| "fixture request must include content length".to_owned())?;
    while request.len() < header_end + content_length {
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("fixture client closed before sending the complete body".into());
        }
        request.extend_from_slice(&chunk[..count]);
    }
    let body: Value = serde_json::from_slice(&request[header_end..header_end + content_length])
        .map_err(|error| error.to_string())?;
    let expected_stream = matches!(wire_api, OpenAiWireApi::Responses);
    if body.get("model").and_then(Value::as_str) != Some("mock-model")
        || body.get("stream").and_then(Value::as_bool) != Some(expected_stream)
    {
        return Err(format!("unexpected request body: {body}"));
    }

    match wire_api {
        OpenAiWireApi::ChatCompletions => {
            if body.get("messages").and_then(Value::as_array).is_none() {
                return Err(format!("chat request omitted messages: {body}"));
            }
        }
        OpenAiWireApi::Responses => {
            let input = body
                .get("input")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("responses request omitted input: {body}"))?;
            if input
                .first()
                .and_then(|item| item.get("role"))
                .and_then(Value::as_str)
                != Some("user")
            {
                return Err(format!("responses request had unexpected input: {body}"));
            }
            if input
                .first()
                .and_then(|item| item.get("content"))
                .and_then(Value::as_str)
                != Some("hello responses")
            {
                return Err(format!("responses request lost user content: {body}"));
            }
        }
    }
    let payload = match wire_api {
        OpenAiWireApi::ChatCompletions => {
            r#"{"id":"fixture","model":"mock-model","choices":[{"message":{"content":"fixture answer"}}],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}"#
        }
        OpenAiWireApi::Responses => {
            "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_fixture\"}}\0\0\n\nevent: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"responses answer\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_fixture\",\"model\":\"mock-model\",\"usage\":{\"input_tokens\":4,\"output_tokens\":5,\"total_tokens\":9}}}\n\ndata: [DONE]\n"
        }
    };
    let content_type = if expected_stream {
        "text/event-stream"
    } else {
        "application/json"
    };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn openai_compatible_backend_works_against_local_fixture() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || -> Result<(), String> {
        let (stream, _) = listener.accept().map_err(|error| error.to_string())?;
        respond(stream, OpenAiWireApi::ChatCompletions)
    });
    let backend = OpenAiCompatibleBackend::new(
        format!("http://127.0.0.1:{port}/v1"),
        Some("test-key".into()),
        "mock-model",
    )
    .unwrap();
    let dir = tempdir().unwrap();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("fixture.jsonl")).unwrap(),
        Box::new(backend),
    );
    let result = agent
        .process(TurnInputRequest::new("hello fixture"))
        .unwrap();
    let assistant = result.assistant.unwrap();
    assert_eq!(assistant.content, "fixture answer");
    assert_eq!(assistant.metadata.unwrap()["usage"]["total_tokens"], 3);
    server.join().unwrap().unwrap();
}

#[test]
fn responses_backend_works_against_local_fixture() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || -> Result<(), String> {
        let (stream, _) = listener.accept().map_err(|error| error.to_string())?;
        respond(stream, OpenAiWireApi::Responses)
    });
    let backend = OpenAiCompatibleBackend::new_with_wire_api(
        format!("http://127.0.0.1:{port}/v1"),
        Some("test-key".into()),
        "mock-model",
        OpenAiWireApi::Responses,
    )
    .unwrap();
    let dir = tempdir().unwrap();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("responses.jsonl")).unwrap(),
        Box::new(backend),
    );
    let result = agent
        .process(TurnInputRequest::new("hello responses"))
        .unwrap();
    let assistant = result.assistant.unwrap();
    assert_eq!(assistant.content, "responses answer");
    assert_eq!(assistant.metadata.unwrap()["usage"]["total_tokens"], 9);
    server.join().unwrap().unwrap();
}

#[test]
fn capabilities_are_explicit_per_adapter() {
    let responses = ProviderCapabilities::for_wire_api(OpenAiWireApi::Responses);
    assert!(responses.text && responses.tools && responses.streaming && responses.reasoning);
    let chat = ProviderCapabilities::for_wire_api(OpenAiWireApi::ChatCompletions);
    assert!(chat.text && chat.tools);
    assert!(!chat.streaming);
    assert!(!chat.reasoning);
}

#[test]
fn retry_reuses_a_stable_idempotency_key() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || -> Result<(), String> {
        let mut keys = Vec::new();
        for attempt in 0..2 {
            let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4096];
            loop {
                let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..count]);
                if request.windows(4).any(|part| part == b"\r\n\r\n") {
                    break;
                }
            }
            let headers = String::from_utf8_lossy(&request);
            let key = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("x-idempotency-key"))
                        .map(|(_, value)| value.trim().to_owned())
                })
                .ok_or_else(|| "missing idempotency key".to_owned())?;
            keys.push(key);
            if attempt == 0 {
                write!(
                    stream,
                    "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .map_err(|error| error.to_string())?;
            } else {
                let payload = "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"retried\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"retry-id\"}}\n\n";
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    payload.len(),
                    payload
                )
                .map_err(|error| error.to_string())?;
            }
        }
        if keys.len() != 2 || keys[0] != keys[1] {
            return Err(format!("idempotency key changed across retry: {keys:?}"));
        }
        Ok(())
    });
    let backend = OpenAiCompatibleBackend::new_with_wire_api(
        format!("http://127.0.0.1:{port}"),
        Some("test-key".into()),
        "mock-model",
        OpenAiWireApi::Responses,
    )
    .unwrap()
    .with_max_retries(1)
    .unwrap();
    let dir = tempdir().unwrap();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("retry.jsonl")).unwrap(),
        Box::new(backend),
    );
    assert_eq!(
        agent
            .process(TurnInputRequest::new("retry this"))
            .unwrap()
            .assistant
            .unwrap()
            .content,
        "retried"
    );
    server.join().unwrap().unwrap();
}

#[test]
fn retry_matrix_honors_retry_after_and_keeps_one_idempotency_key() {
    for status in [408_u16, 409, 425, 429, 500, 502, 503] {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || -> Result<(), String> {
            let mut keys = Vec::new();
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
                let request = read_headers(&mut stream)?;
                keys.push(header_value(&request, "x-idempotency-key")?);
                if attempt == 0 {
                    write!(
                        stream,
                        "HTTP/1.1 {status} Retry\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .map_err(|error| error.to_string())?;
                } else {
                    write_sse_completion(&mut stream, "matrix-ok")?;
                }
            }
            if keys[0] != keys[1] {
                return Err("idempotency key changed across retry".into());
            }
            Ok(())
        });
        let backend = retry_backend(port, 1);
        let dir = tempdir().unwrap();
        let mut agent = Agent::new(
            SessionStore::open(dir.path().join(format!("retry-{status}.jsonl"))).unwrap(),
            Box::new(backend),
        );
        assert_eq!(
            agent
                .process(TurnInputRequest::new("retry matrix"))
                .unwrap()
                .assistant
                .unwrap()
                .content,
            "matrix-ok"
        );
        server.join().unwrap().unwrap();
    }
}

#[test]
fn non_retryable_status_is_not_reissued() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || -> Result<(), String> {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let _ = read_headers(&mut stream)?;
        write!(
            stream,
            "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        thread::sleep(Duration::from_millis(250));
        if listener.accept().is_ok() {
            return Err("400 response was retried".into());
        }
        Ok(())
    });
    let backend = retry_backend(port, 2);
    let dir = tempdir().unwrap();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("no-retry.jsonl")).unwrap(),
        Box::new(backend),
    );
    let error = agent
        .process(TurnInputRequest::new("do not retry"))
        .unwrap_err();
    assert!(matches!(
        error,
        zenpi::core::AgentError::Backend(BackendError::HttpStatus { status: 400, .. })
    ));
    server.join().unwrap().unwrap();
}

#[test]
fn cancellation_during_retry_backoff_stops_before_a_second_request() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let first_response_sent = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let fixture_flag = std::sync::Arc::clone(&first_response_sent);
    let server = thread::spawn(move || -> Result<(), String> {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let _ = read_headers(&mut stream)?;
        write!(
            stream,
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 5\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .map_err(|error| error.to_string())?;
        fixture_flag.store(true, std::sync::atomic::Ordering::Release);
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        thread::sleep(Duration::from_millis(250));
        if listener.accept().is_ok() {
            return Err("request retried after cancellation".into());
        }
        Ok(())
    });
    let backend = retry_backend(port, 2);
    let dir = tempdir().unwrap();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("cancel-backoff.jsonl")).unwrap(),
        Box::new(backend),
    );
    let started = std::time::Instant::now();
    let error = agent
        .process_with_cancel(TurnInputRequest::new("cancel retry"), || {
            first_response_sent.load(std::sync::atomic::Ordering::Acquire)
                && started.elapsed() >= Duration::from_millis(30)
        })
        .unwrap_err();
    assert!(matches!(
        error,
        zenpi::core::AgentError::Backend(BackendError::Cancelled)
    ));
    assert!(started.elapsed() < Duration::from_secs(1));
    server.join().unwrap().unwrap();
}

#[test]
fn circuit_breaker_opens_and_recovers_after_cooldown() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || -> Result<(), String> {
        for attempt in 0..2 {
            let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
            let _ = read_headers(&mut stream)?;
            if attempt == 0 {
                write!(
                    stream,
                    "HTTP/1.1 503 Down\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .map_err(|error| error.to_string())?;
            } else {
                write_sse_completion(&mut stream, "recovered")?;
            }
        }
        Ok(())
    });
    let backend = retry_backend(port, 0)
        .with_circuit_breaker(1, Duration::from_millis(100))
        .unwrap();
    let dir = tempdir().unwrap();
    let mut agent = Agent::new(
        SessionStore::open(dir.path().join("circuit.jsonl")).unwrap(),
        Box::new(backend),
    );
    assert!(agent.process(TurnInputRequest::new("first")).is_err());
    let second = agent.process(TurnInputRequest::new("second")).unwrap_err();
    assert!(matches!(
        second,
        zenpi::core::AgentError::Backend(BackendError::CircuitOpen { .. })
    ));
    let cooldown_deadline = std::time::Instant::now() + Duration::from_secs(2);
    let recovered = loop {
        match agent.process(TurnInputRequest::new("third")) {
            Ok(result) => break result,
            Err(zenpi::core::AgentError::Backend(BackendError::CircuitOpen { .. }))
                if std::time::Instant::now() < cooldown_deadline =>
            {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("circuit did not recover: {error}"),
        }
    };
    assert_eq!(recovered.assistant.unwrap().content, "recovered");
    server.join().unwrap().unwrap();
}

fn retry_backend(port: u16, retries: u32) -> OpenAiCompatibleBackend {
    OpenAiCompatibleBackend::new_with_wire_api(
        format!("http://127.0.0.1:{port}"),
        Some("test-key".into()),
        "mock-model",
        OpenAiWireApi::Responses,
    )
    .unwrap()
    .with_max_retries(retries)
    .unwrap()
}

fn read_headers(stream: &mut TcpStream) -> Result<String, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("client closed before sending headers".into());
        }
        request.extend_from_slice(&chunk[..count]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&request).into_owned())
}

fn header_value(headers: &str, expected: &str) -> Result<String, String> {
    headers
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case(expected))
                .map(|(_, value)| value.trim().to_owned())
        })
        .ok_or_else(|| format!("missing {expected} header"))
}

fn write_sse_completion(stream: &mut TcpStream, text: &str) -> Result<(), String> {
    let payload = format!(
        "event: response.output_text.delta\ndata: {{\"type\":\"response.output_text.delta\",\"delta\":{}}}\n\nevent: response.completed\ndata: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"retry-id\"}}}}\n\n",
        serde_json::to_string(text).map_err(|error| error.to_string())?
    );
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    )
    .map_err(|error| error.to_string())
}

#[test]
fn responses_multimodal_payload_is_bounded_and_journal_keeps_references_only() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || -> Result<(), String> {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let body = read_request_body(&mut stream)?;
        let content = body["input"]
            .as_array()
            .and_then(|items| items.last())
            .and_then(|item| item["content"].as_array())
            .ok_or_else(|| format!("missing multimodal content: {body}"))?;
        if content
            .iter()
            .filter(|part| part["type"] == "input_image")
            .count()
            != 1
            || content
                .iter()
                .filter(|part| part["type"] == "input_file")
                .count()
                != 1
        {
            return Err(format!("unexpected multimodal content: {content:?}"));
        }
        if content
            .iter()
            .find(|part| part["type"] == "input_image")
            .and_then(|part| part["image_url"].as_str())
            .is_none_or(|url| !url.starts_with("data:image/png;base64,"))
        {
            return Err("image was not encoded as bounded data URL".into());
        }
        if content
            .iter()
            .find(|part| part["type"] == "input_file")
            .and_then(|part| part["file_data"].as_str())
            .is_none_or(|data| !data.starts_with("data:text/plain;base64,"))
        {
            return Err("file was not encoded as bounded data URL".into());
        }
        write_sse_completion(&mut stream, "vision-ok")
    });

    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("pixel.png"), b"not-a-real-png-but-bounded").unwrap();
    std::fs::write(dir.path().join("note.txt"), b"private attachment bytes").unwrap();
    let backend = OpenAiCompatibleBackend::new_with_wire_api(
        format!("http://127.0.0.1:{port}"),
        Some("test-key".into()),
        "mock-model",
        OpenAiWireApi::Responses,
    )
    .unwrap();
    let journal = dir.path().join("multimodal.jsonl");
    let mut agent = Agent::new(SessionStore::open(&journal).unwrap(), Box::new(backend));
    agent.set_attachment_workspace(zenpi::tools::ToolContext::new(dir.path()).unwrap());
    let attachments = vec![
        InputAttachment {
            kind: AttachmentKind::Image,
            mime_type: "image/png".into(),
            path: Some("pixel.png".into()),
            url: None,
            file_id: None,
        },
        InputAttachment {
            kind: AttachmentKind::File,
            mime_type: "text/plain".into(),
            path: Some("note.txt".into()),
            url: None,
            file_id: None,
        },
    ];
    assert_eq!(
        agent
            .process(TurnInputRequest::new("inspect attachments").with_attachments(attachments))
            .unwrap()
            .assistant
            .unwrap()
            .content,
        "vision-ok"
    );
    server.join().unwrap().unwrap();
    let journal_text = std::fs::read_to_string(journal).unwrap();
    assert!(journal_text.contains("pixel.png") && journal_text.contains("sha256"));
    assert!(!journal_text.contains("private attachment bytes"));
    assert!(!journal_text.contains("base64"));
}

#[test]
fn attachment_path_escape_fails_before_provider_side_effects() {
    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), dir.path().join("escape"))
        .unwrap();
    let backend = OpenAiCompatibleBackend::new_with_wire_api(
        "http://127.0.0.1:1",
        None,
        "mock-model",
        OpenAiWireApi::Responses,
    )
    .unwrap();
    let journal = dir.path().join("rejected.jsonl");
    let mut agent = Agent::new(SessionStore::open(&journal).unwrap(), Box::new(backend));
    agent.set_attachment_workspace(zenpi::tools::ToolContext::new(dir.path()).unwrap());
    #[cfg(unix)]
    {
        let error = agent
            .submit(
                TurnInputRequest::new("escape").with_attachments(vec![InputAttachment {
                    kind: AttachmentKind::File,
                    mime_type: "text/plain".into(),
                    path: Some("escape".into()),
                    url: None,
                    file_id: None,
                }]),
            )
            .unwrap_err();
        assert!(error.to_string().contains("denied"));
    }
    assert_eq!(agent.history().len(), 0);
}

fn read_request_body(stream: &mut TcpStream) -> Result<Value, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    let mut request = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("client closed before complete request".into());
        }
        request.extend_from_slice(&chunk[..count]);
        if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let content_length = header_value(&headers, "content-length")?
        .parse::<usize>()
        .map_err(|error| error.to_string())?;
    while request.len() < header_end + content_length {
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("client closed before request body".into());
        }
        request.extend_from_slice(&chunk[..count]);
    }
    serde_json::from_slice(&request[header_end..header_end + content_length])
        .map_err(|error| error.to_string())
}
