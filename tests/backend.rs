use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use tempfile::tempdir;
use zenpi::backend::{OpenAiCompatibleBackend, OpenAiWireApi, ProviderCapabilities};
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
