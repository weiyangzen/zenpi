#![cfg(unix)]

use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use tempfile::tempdir;
use zenpi::{core::Agent, session::SessionStore};

const BINARY: &str = env!("CARGO_BIN_EXE_zenpi");

// These tests each spawn a full production host binary and a loopback provider
// fixture. Running them concurrently oversubscribes the runner and makes the
// provider-readiness deadlines flaky; serialize them so the assertions measure
// the host behaviour rather than machine contention.
static HOST_TEST_SERIAL: Mutex<()> = Mutex::new(());

fn serial_guard() -> std::sync::MutexGuard<'static, ()> {
    HOST_TEST_SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone)]
struct Captured {
    path: String,
    body: Value,
}

enum Mode {
    Text(&'static str),
    Hold,
}

struct Fixture {
    url: String,
    requests: Arc<Mutex<Vec<Captured>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Fixture {
    fn start(mode: Mode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_requests = requests.clone();
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // The listener is non-blocking; force the accepted
                        // connection back to blocking so a read that races the
                        // client's first packet cannot drop the request.
                        stream.set_nonblocking(false).unwrap();
                        let Some((path, body)) = read_request(&mut stream) else {
                            continue;
                        };
                        worker_requests.lock().unwrap().push(Captured {
                            path,
                            body: body.clone(),
                        });
                        match &mode {
                            Mode::Text(text) => respond_chat(&mut stream, &body, text),
                            Mode::Hold => {
                                stream
                                    .set_read_timeout(Some(Duration::from_millis(50)))
                                    .unwrap();
                                let mut byte = [0u8; 1];
                                loop {
                                    match stream.read(&mut byte) {
                                        Ok(0) | Err(_) => break,
                                        Ok(_) => {}
                                    }
                                    if worker_stop.load(Ordering::Acquire) {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url,
            requests,
            stop,
            worker: Some(worker),
        }
    }

    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    fn captured(&self) -> Vec<Captured> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<(String, Value)> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut bytes = Vec::new();
    let end = loop {
        let mut chunk = [0u8; 4096];
        let count = stream.read(&mut chunk).ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
        if bytes.len() > 4 * 1024 * 1024 {
            return None;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let path = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default()
        .to_owned();
    let length: usize = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|value| value.trim().parse().unwrap())
        })
        .unwrap_or(0);
    while bytes.len() < end + length {
        let mut chunk = [0u8; 4096];
        let count = stream.read(&mut chunk).ok()?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let body = serde_json::from_slice(&bytes[end..(end + length).min(bytes.len())]).ok()?;
    Some((path, body))
}

fn respond_chat(stream: &mut TcpStream, request: &Value, text: &str) {
    let (content_type, body) = if request["stream"] == true {
        let first = json!({"id":"fixture","model":"fixture","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]});
        let second = json!({"id":"fixture","model":"fixture","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
        (
            "text/event-stream",
            format!("data: {first}\n\ndata: {second}\n\ndata: [DONE]\n\n"),
        )
    } else {
        (
            "application/json",
            json!({"id":"fixture","model":"fixture","choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}).to_string(),
        )
    };
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

fn clean_env(home: &Path, url: &str) -> Vec<(String, String)> {
    let mut environment: Vec<(String, String)> = std::env::vars()
        .filter(|(key, _)| {
            !key.starts_with("ZENPI_")
                && !key.starts_with("OPENAI_")
                && !key.starts_with("ANTHROPIC_")
                && !key.starts_with("GEMINI_")
        })
        .collect();
    environment.extend([
        ("ZENPI_HOME".into(), home.display().to_string()),
        ("ZENPI_BACKEND".into(), "openai".into()),
        ("ZENPI_MODEL".into(), "gpt-4.1".into()),
        ("ZENPI_BASE_URL".into(), url.into()),
        ("ZENPI_API_KEY".into(), "stage1-host-fixture".into()),
        ("ZENPI_WIRE_API".into(), "chat".into()),
        ("ZENPI_MAX_RETRIES".into(), "0".into()),
        ("NO_PROXY".into(), "127.0.0.1,localhost".into()),
        ("no_proxy".into(), "127.0.0.1,localhost".into()),
    ]);
    environment
}

struct Host {
    child: Child,
    input: Option<ChildStdin>,
    receiver: mpsc::Receiver<Value>,
    non_json: Arc<Mutex<Vec<String>>>,
    stderr: Arc<Mutex<String>>,
    reader: Option<thread::JoinHandle<()>>,
    stderr_reader: Option<thread::JoinHandle<()>>,
    seen: Vec<Value>,
}

impl Host {
    fn spawn(workspace: &Path, home: &Path, url: &str, session: &Path) -> Self {
        let mut child = Command::new(BINARY)
            .arg("--headless")
            .arg("--session")
            .arg(session)
            .current_dir(workspace)
            .env_clear()
            .envs(clean_env(home, url))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (send, receiver) = mpsc::channel();
        let non_json = Arc::new(Mutex::new(Vec::new()));
        let reader_marker = non_json.clone();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                match serde_json::from_str::<Value>(&line) {
                    Ok(value) => {
                        if send.send(value).is_err() {
                            break;
                        }
                    }
                    Err(_) => reader_marker.lock().unwrap().push(line),
                }
            }
        });
        let stderr_buffer = Arc::new(Mutex::new(String::new()));
        let stderr_marker = stderr_buffer.clone();
        let stderr_reader = thread::spawn(move || {
            let mut text = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut text);
            *stderr_marker.lock().unwrap() = text;
        });
        Self {
            child,
            input: Some(input),
            receiver,
            non_json,
            stderr: stderr_buffer,
            reader: Some(reader),
            stderr_reader: Some(stderr_reader),
            seen: Vec::new(),
        }
    }

    fn send(&mut self, value: Value) {
        self.send_bytes(format!("{value}\n").as_bytes());
    }

    fn send_bytes(&mut self, bytes: &[u8]) {
        if let Some(input) = self.input.as_mut() {
            let _ = input.write_all(bytes);
            let _ = input.flush();
        }
    }

    fn wait(&mut self, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                panic!(
                    "timed out waiting for host output; stderr={}",
                    self.stderr.lock().unwrap()
                );
            }
            match self.receiver.recv_timeout(remaining) {
                Ok(value) => {
                    self.seen.push(value.clone());
                    if predicate(&value) {
                        return value;
                    }
                }
                Err(error) => panic!(
                    "host output ended unexpectedly ({error}); stderr={}",
                    self.stderr.lock().unwrap()
                ),
            }
        }
    }

    fn response(&mut self, id: &str) -> Value {
        self.wait(|value| value["type"] == "response" && value["id"] == id)
    }

    fn has_failure(&self) -> bool {
        self.seen
            .iter()
            .any(|value| value["success"] == false || value["error"].is_string())
    }

    fn finish(mut self) -> i32 {
        self.input.take();
        let status = wait_for_exit(&mut self.child, Duration::from_secs(20));
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
        assert!(
            !status.killed,
            "host had to be killed; stderr={}",
            self.stderr.lock().unwrap()
        );
        status.code.unwrap_or(-1)
    }

    fn finish_with_kill(mut self) -> i32 {
        self.input.take();
        if self.child.try_wait().unwrap().is_none() {
            let _ = self.child.kill();
        }
        let status = self.child.wait().unwrap();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
        status.code().unwrap_or(-1)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Exit {
    code: Option<i32>,
    killed: bool,
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> Exit {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return Exit {
                code: status.code(),
                killed: false,
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Exit {
                code: None,
                killed: true,
            };
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn production_headless_entry_streams_real_http_into_machine_readable_stdout() {
    let _serial = serial_guard();
    let fixture = Fixture::start(Mode::Text("HOST_ENTRY_OK"));
    let home = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let sessions = workspace.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let session = sessions.join("initial.jsonl");
    let mut host = Host::spawn(workspace.path(), home.path(), &fixture.url, &session);

    host.send(
        json!({"schema_version":2,"type":"prompt","id":"host-prompt","text":"hello from host"}),
    );
    let response = host.response("host-prompt");
    assert_eq!(response["success"], true, "{response}");
    assert_eq!(response["data"]["assistant"]["content"], "HOST_ENTRY_OK");

    host.send(json!({"schema_version":2,"type":"shutdown","id":"host-stop"}));
    let shutdown = host.response("host-stop");
    assert_eq!(shutdown["success"], true, "{shutdown}");

    let non_json = host.non_json.clone();
    let code = host.finish();
    assert_eq!(code, 0, "headless host must exit cleanly");
    assert!(
        non_json.lock().unwrap().is_empty(),
        "stdout carried non-protocol bytes: {:?}",
        non_json.lock().unwrap()
    );

    let captured = fixture.captured();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].path, "/v1/chat/completions");
    assert_eq!(captured[0].body["model"], "gpt-4.1");
    assert!(
        captured[0].body.to_string().contains("hello from host"),
        "real HTTP request omitted the prompt: {}",
        captured[0].body
    );

    let journal = fs::read_to_string(&session).unwrap();
    assert!(
        journal.contains("HOST_ENTRY_OK"),
        "journal missing answer: {journal}"
    );
    for line in journal.lines() {
        serde_json::from_str::<Value>(line).unwrap();
    }
}

#[test]
fn malformed_duplicate_and_unsupported_frames_fail_visibly_without_stopping_the_host() {
    let _serial = serial_guard();
    use zenpi::protocol::MAX_LINE_BYTES;

    let fixture = Fixture::start(Mode::Text("RECOVERED"));
    let home = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let sessions = workspace.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let session = sessions.join("bounds.jsonl");
    let mut host = Host::spawn(workspace.path(), home.path(), &fixture.url, &session);

    host.send_bytes(b"{not json}\n");
    host.send_bytes(&[b'x'; MAX_LINE_BYTES + 1]);
    host.send_bytes(b"\n");
    host.send_bytes(b"\xff\xfe\n");
    host.send(json!({"schema_version":3,"type":"status","id":"unsupported"}));
    host.send(json!({"schema_version":2,"type":"nonsense","id":"unknown-kind"}));
    host.send(json!({"schema_version":2,"type":"status","id":"after-failures"}));
    host.send(json!({"schema_version":2,"type":"prompt","id":"host-prompt","text":"recover"}));
    let recovered = host.response("host-prompt");
    assert_eq!(recovered["success"], true, "{recovered}");
    assert_eq!(recovered["data"]["assistant"]["content"], "RECOVERED");
    host.send(json!({"schema_version":2,"type":"shutdown","id":"host-stop"}));
    assert_eq!(host.response("host-stop")["success"], true);

    let non_json = host.non_json.clone();
    let failed = host.has_failure();
    let code = host.finish();
    assert_eq!(code, 0);
    assert!(failed, "malformed frames must be reported as failures");
    assert!(
        non_json.lock().unwrap().is_empty(),
        "malformed input leaked raw bytes to stdout"
    );
    assert_eq!(
        fixture.count(),
        1,
        "only the recovered prompt may reach the provider"
    );
}

#[test]
fn host_shutdown_cancels_an_in_flight_provider_request_and_reaps_the_connection() {
    let _serial = serial_guard();
    let fixture = Fixture::start(Mode::Hold);
    let home = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let sessions = workspace.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let session = sessions.join("cancel.jsonl");
    let mut host = Host::spawn(workspace.path(), home.path(), &fixture.url, &session);

    host.send(json!({"schema_version":2,"type":"prompt","id":"host-prompt","text":"hold"}));
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.count() == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fixture.count(), 1, "provider request never arrived");

    host.send(json!({"schema_version":2,"type":"shutdown","id":"host-stop"}));
    let cancelled = host.response("host-prompt");
    assert_eq!(cancelled["success"], false, "{cancelled}");
    assert_eq!(host.response("host-stop")["success"], true);
    assert_eq!(host.finish(), 0);
}

#[test]
fn independent_restart_restores_the_durable_input_queue_without_provider_reissue() {
    let _serial = serial_guard();
    let fixture = Fixture::start(Mode::Text("FIRST"));
    let home = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let sessions = workspace.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let session = sessions.join("restart.jsonl");

    {
        let mut host = Host::spawn(workspace.path(), home.path(), &fixture.url, &session);
        host.send(json!({"schema_version":2,"type":"prompt","id":"host-prompt","text":"persist"}));
        assert_eq!(host.response("host-prompt")["success"], true);
        host.send(json!({"schema_version":2,"type":"command","id":"queue","text":"/input follow-up f1 queued-after-restart"}));
        let queued = host.response("queue");
        assert_eq!(queued["success"], true, "{queued}");
        host.send(json!({"schema_version":2,"type":"shutdown","id":"host-stop"}));
        assert_eq!(host.response("host-stop")["success"], true);
        assert_eq!(host.finish(), 0);
    }

    {
        let mut host = Host::spawn(workspace.path(), home.path(), &fixture.url, &session);
        host.send(json!({"schema_version":2,"type":"command","id":"list","text":"/input list"}));
        let listed = host.response("list");
        assert_eq!(listed["success"], true, "{listed}");
        assert!(
            listed.to_string().contains("queued-after-restart"),
            "restart lost the durable queued input: {listed}"
        );
        host.send(json!({"schema_version":2,"type":"shutdown","id":"host-stop"}));
        assert_eq!(host.response("host-stop")["success"], true);
        assert_eq!(host.finish(), 0);
    }

    assert_eq!(
        fixture.count(),
        1,
        "restart must not reissue the provider request"
    );
}

#[test]
fn read_only_session_directory_fails_closed_without_mutation() {
    let _serial = serial_guard();
    use std::os::unix::fs::PermissionsExt;

    let home = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let locked = workspace.path().join("locked");
    fs::create_dir_all(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
    let session = locked.join("session.jsonl");
    if fs::write(&session, b"privilege probe").is_ok() {
        fs::remove_file(&session).ok();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        eprintln!("skipping read-only assertion: the runner can write to a 0o500 directory");
        return;
    }

    let mut host = Host::spawn(
        workspace.path(),
        home.path(),
        "http://127.0.0.1:9/v1",
        &session,
    );
    host.send(json!({"schema_version":2,"type":"prompt","id":"host-prompt","text":"no write"}));
    let code = host.finish_with_kill();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();

    assert_ne!(
        code, 0,
        "host must fail closed when it cannot own the session"
    );
    assert!(
        !session.exists(),
        "host created a session despite the denied directory"
    );
}

#[test]
fn forged_external_manifest_is_rejected_by_the_host_importer() {
    let _serial = serial_guard();
    let directory = tempdir().unwrap();
    let mut agent =
        Agent::with_echo(SessionStore::open(directory.path().join("fake.jsonl")).unwrap());
    let before = agent.session().events().len();
    let forged = directory.path().join("forged.json");
    fs::write(
        &forged,
        json!({"schema_version":2,"declaration":{"acceptance_passed":true,"status":"succeeded"}})
            .to_string(),
    )
    .unwrap();

    assert!(
        zenpi::headless::import_blueprint_manifest(
            &mut agent,
            "missing-plan@1",
            forged.to_str().unwrap()
        )
        .is_err()
    );
    assert!(
        zenpi::headless::import_blueprint_manifest(&mut agent, "missing-plan@1", "absent.json")
            .is_err()
    );
    assert_eq!(agent.session().events().len(), before);
}

fn parse_rss(stderr: &str) -> Option<f64> {
    if cfg!(target_os = "macos") {
        stderr.lines().find_map(|line| {
            line.contains("maximum resident set size").then(|| {
                line.split_whitespace()
                    .filter_map(|field| field.parse::<f64>().ok())
                    .next_back()
            })?
        })
    } else {
        stderr.lines().find_map(|line| {
            line.contains("Maximum resident set size (kbytes)")
                .then(|| {
                    line.split_whitespace()
                        .filter_map(|field| field.parse::<f64>().ok())
                        .next_back()
                        .map(|value| value * 1024.0)
                })?
        })
    }
}

fn measure_cold_start(binary: &Path, home: &Path, session: &Path) -> (f64, f64) {
    let mut command = Command::new("/usr/bin/time");
    if cfg!(target_os = "macos") {
        command.arg("-l");
    } else {
        command.arg("-v");
    }
    command
        .arg(binary)
        .arg("--headless")
        .arg("--session")
        .arg(session)
        .env_clear()
        .envs([
            ("ZENPI_HOME", home.display().to_string()),
            ("ZENPI_BACKEND", "openai".to_owned()),
            ("ZENPI_BASE_URL", "http://127.0.0.1:1".to_owned()),
            ("ZENPI_MODEL", "benchmark-no-network".to_owned()),
            ("ZENPI_API_KEY", "benchmark-placeholder".to_owned()),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"type\":\"shutdown\",\"id\":\"budget\"}\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "cold start failed: {stderr}");
    (elapsed_ms, parse_rss(&stderr).unwrap_or(0.0))
}

#[test]
fn release_artifact_startup_size_and_rss_stay_within_lightweight_budget() {
    let _serial = serial_guard();
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let release = std::env::var_os("ZENPI_RELEASE_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("target/release/zenpi"));
    if !release.is_file() {
        eprintln!(
            "skipping lightweight budget probe: {} is not built (run cargo build --release --locked)",
            release.display()
        );
        return;
    }
    let size = fs::metadata(&release).unwrap().len();
    assert!(
        size <= 8 * 1024 * 1024,
        "release binary is {size} bytes, above the 8 MiB budget"
    );
    let home = tempdir().unwrap();
    let mut best_ms = f64::MAX;
    let mut worst_rss = 0.0_f64;
    for index in 0..3 {
        let session = home.path().join(format!("budget-{index}.jsonl"));
        let (elapsed_ms, rss) = measure_cold_start(&release, home.path(), &session);
        best_ms = best_ms.min(elapsed_ms);
        worst_rss = worst_rss.max(rss);
    }
    assert!(
        best_ms <= 1000.0,
        "cold start {best_ms:.1} ms exceeds the 1000 ms budget"
    );
    assert!(
        worst_rss <= 96.0 * 1024.0 * 1024.0,
        "peak RSS {worst_rss:.0} bytes exceeds the 96 MiB budget"
    );
}
