use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use tempfile::tempdir;
use zenpi::backend::OpenAiCompatibleBackend;
use zenpi::core::{Agent, TurnInputRequest};
use zenpi::session::SessionStore;

fn respond(mut stream: TcpStream) -> Result<(), String> {
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
    if !headers.starts_with("POST /v1/chat/completions HTTP/1.1") {
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
    if body.get("model").and_then(Value::as_str) != Some("mock-model")
        || body.get("stream").and_then(Value::as_bool) != Some(false)
    {
        return Err(format!("unexpected request body: {body}"));
    }

    let payload = r#"{"id":"fixture","model":"mock-model","choices":[{"message":{"content":"fixture answer"}}],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}"#;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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
        respond(stream)
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
