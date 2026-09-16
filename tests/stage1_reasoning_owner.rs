use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    thread,
    time::Duration,
};
use tempfile::tempdir;
use zenpi::{
    backend::{OpenAiCompatibleBackend, OpenAiWireApi},
    core::{Agent, TurnInputRequest},
    providers::registry::ModelRegistry,
    session::SessionStore,
};

fn backend(url: &str, effort: Option<&str>, wire: OpenAiWireApi) -> OpenAiCompatibleBackend {
    OpenAiCompatibleBackend::new_with_settings(
        url,
        None,
        "gpt-5.2",
        wire,
        effort.map(str::to_owned),
        None,
    )
    .unwrap()
    .with_model_registry("openai".into(), ModelRegistry::default())
    .unwrap()
}

fn read_request(stream: &mut TcpStream) -> Value {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut bytes = Vec::new();
    let end = loop {
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break end + 4;
        }
        assert!(bytes.len() < 65536);
    };
    let len: usize = String::from_utf8_lossy(&bytes[..end])
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(str::to_owned)
        })
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(len < 2 * 1024 * 1024);
    while bytes.len() < end + len {
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&chunk[..count]);
    }
    serde_json::from_slice(&bytes[end..end + len]).unwrap()
}
fn server(count: usize) -> (String, mpsc::Receiver<Value>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (send, recv) = mpsc::channel();
    let handle = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        for _ in 0..count {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            let request = read_request(&mut stream);
            let body = format!(
                "data: {}\n\ndata: {}\n\n",
                json!({"type":"response.output_text.delta","delta":"ok"}),
                json!({"type":"response.completed","response":{"id":"r","model":request["model"],"status":"completed"}})
            );
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            stream.flush().unwrap();
            send.send(request).unwrap();
        }
    });
    (url, recv, handle)
}

#[test]
fn real_requests_use_new_effort_and_restart_preserves_model_effort_and_default_reset() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let (url, requests, server) = server(3);
    let mut agent = Agent::new(
        SessionStore::open(&path).unwrap(),
        Box::new(backend(&url, None, OpenAiWireApi::Responses)),
    );
    agent.set_reasoning_effort(Some("high".into())).unwrap();
    assert_eq!(agent.reasoning_effort(), Some("high"));
    assert_eq!(agent.model_status().unwrap()["reasoning_effort"], "high");
    agent.process(TurnInputRequest::new("first")).unwrap();
    assert_eq!(
        requests.recv_timeout(Duration::from_secs(5)).unwrap()["reasoning"]["effort"],
        "high"
    );
    agent.set_reasoning_effort(Some("low".into())).unwrap();
    drop(agent);
    let mut agent = Agent::new(
        SessionStore::open(&path).unwrap(),
        Box::new(backend(&url, Some("high"), OpenAiWireApi::Responses)),
    );
    agent.restore_model_selection().unwrap();
    assert_eq!(agent.reasoning_effort(), Some("low"));
    agent.process(TurnInputRequest::new("second")).unwrap();
    assert_eq!(
        requests.recv_timeout(Duration::from_secs(5)).unwrap()["reasoning"]["effort"],
        "low"
    );
    agent.set_reasoning_effort(None).unwrap();
    agent.set_model(Some("gpt-4.1".into())).unwrap();
    drop(agent);
    // Restoring saved None must not validate against stale factory 'high'.
    let mut agent = Agent::new(
        SessionStore::open(&path).unwrap(),
        Box::new(backend(&url, Some("high"), OpenAiWireApi::Responses)),
    );
    agent.restore_model_selection().unwrap();
    assert_eq!(agent.reasoning_effort(), None);
    assert_eq!(agent.model_status().unwrap()["active"]["id"], "gpt-4.1");
    agent.process(TurnInputRequest::new("third")).unwrap();
    let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(request["model"], "gpt-4.1");
    assert!(request.get("reasoning").is_none());
    server.join().unwrap();
}

#[test]
fn invalid_level_or_wire_does_not_change_effective_state_or_journal() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut agent = Agent::new(
        SessionStore::open(&path).unwrap(),
        Box::new(backend(
            "http://127.0.0.1:9",
            Some("high"),
            OpenAiWireApi::Responses,
        )),
    );
    let state = agent.model_status().unwrap();
    let bytes = fs::read(&path).unwrap();
    for effort in ["max", "ultra", "HIGH", "", "low\n"] {
        assert!(agent.set_reasoning_effort(Some(effort.into())).is_err());
        assert_eq!(state, agent.model_status().unwrap());
        assert_eq!(bytes, fs::read(&path).unwrap());
    }
    let other = dir.path().join("chat.jsonl");
    let mut chat = Agent::new(
        SessionStore::open(&other).unwrap(),
        Box::new(backend(
            "http://127.0.0.1:9",
            None,
            OpenAiWireApi::ChatCompletions,
        )),
    );
    let before = fs::read(&other).unwrap();
    assert!(chat.set_reasoning_effort(Some("high".into())).is_err());
    assert_eq!(before, fs::read(&other).unwrap());
    assert_eq!(chat.reasoning_effort(), None);
    agent.set_reasoning_effort(Some("none".into())).unwrap();
    assert_eq!(agent.reasoning_effort(), Some("none"));
}

#[test]
fn failed_durable_append_keeps_provider_effort_unchanged() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("readonly.jsonl");
    drop(SessionStore::open(&path).unwrap());
    let readonly = SessionStore::open_existing(&path).unwrap();
    let mut agent = Agent::new(
        readonly,
        Box::new(backend(
            "http://127.0.0.1:9",
            Some("high"),
            OpenAiWireApi::Responses,
        )),
    );
    let old = agent.model_status().unwrap();
    let bytes = fs::read(&path).unwrap();
    let saved = dir.path().join("saved.jsonl");
    fs::rename(&path, &saved).unwrap();
    fs::create_dir(&path).unwrap();
    let records = agent.session().records().len();
    assert!(agent.set_reasoning_effort(Some("low".into())).is_err());
    assert_eq!(records, agent.session().records().len());
    assert_eq!(old, agent.model_status().unwrap());
    assert_eq!(bytes, fs::read(&saved).unwrap());
}

#[test]
fn active_or_closed_owner_rejects_changes_before_validation_or_journaling() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("busy.jsonl");
    let mut agent = Agent::new(
        SessionStore::open(&path).unwrap(),
        Box::new(backend(
            "http://127.0.0.1:9",
            Some("high"),
            OpenAiWireApi::Responses,
        )),
    );
    agent.submit(TurnInputRequest::new("pending")).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(agent.set_reasoning_effort(Some("low".into())).is_err());
    assert_eq!(agent.reasoning_effort(), Some("high"));
    assert_eq!(before, fs::read(&path).unwrap());
    let other = dir.path().join("closed.jsonl");
    let mut closed = Agent::new(
        SessionStore::open(&other).unwrap(),
        Box::new(backend(
            "http://127.0.0.1:9",
            Some("high"),
            OpenAiWireApi::Responses,
        )),
    );
    closed.try_close().unwrap();
    let before = fs::read(&other).unwrap();
    assert!(closed.set_reasoning_effort(None).is_err());
    assert_eq!(before, fs::read(&other).unwrap());
}

#[test]
fn invalid_saved_effort_fails_resume_before_replacing_any_current_owner_state() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("current.jsonl");
    let target = dir.path().join("invalid.jsonl");
    let mut agent = Agent::new(
        SessionStore::open(&path).unwrap(),
        Box::new(backend(
            "http://127.0.0.1:9",
            Some("high"),
            OpenAiWireApi::Responses,
        )),
    );
    let descriptor = ModelRegistry::default()
        .resolve("openai", "gpt-5.2")
        .unwrap();
    let mut invalid = SessionStore::open(&target).unwrap();
    invalid.append_event(json!({"type":"model_selected","model":"gpt-5.2","descriptor":descriptor,"digest":descriptor.digest(),"reasoning_effort":"ultra"})).unwrap();
    drop(invalid);
    let old = agent.model_status().unwrap();
    let session = agent.session().session_id().to_owned();
    let bytes = fs::read(&path).unwrap();
    assert!(agent.resume_session(&target).is_err());
    assert_eq!(session, agent.session().session_id());
    assert_eq!(old, agent.model_status().unwrap());
    assert_eq!(bytes, fs::read(&path).unwrap());
}
