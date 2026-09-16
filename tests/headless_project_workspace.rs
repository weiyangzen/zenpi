#![cfg(unix)]
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tempfile::tempdir;
use zenpi::{
    approval::{ApprovalMode, ApprovalPolicy},
    core::Agent,
    project_workspace::ProjectOwnerPool,
    session::SessionStore,
    tools::{SideEffectPolicy, ToolContext, ToolRegistry},
};
fn agent(root: &Path) -> Agent {
    let mut agent = Agent::with_echo(
        SessionStore::open_in_workspace(root.join("initial.jsonl"), root).unwrap(),
    );
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(root).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    agent.set_approval_policy(ApprovalPolicy {
        mode: ApprovalMode::Never,
        ..Default::default()
    });
    agent
}
struct Wire {
    input: UnixStream,
    output: BufReader<UnixStream>,
    join: Option<thread::JoinHandle<()>>,
    records: Vec<Value>,
}
impl Wire {
    fn new(agent: Agent) -> Self {
        let (input, read) = UnixStream::pair().unwrap();
        let (write, output) = UnixStream::pair().unwrap();
        output
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let join =
            thread::spawn(move || zenpi::headless::run_async_streams(agent, read, write).unwrap());
        Self {
            input,
            output: BufReader::new(output),
            join: Some(join),
            records: Vec::new(),
        }
    }
    fn send(&mut self, value: Value) {
        writeln!(self.input, "{value}").unwrap();
    }
    fn next(&mut self) -> Value {
        let mut line = String::new();
        assert!(
            self.output.read_line(&mut line).unwrap() > 0,
            "unexpected EOF: {:?}",
            self.records
        );
        let value = serde_json::from_str::<Value>(&line).unwrap();
        self.records.push(value.clone());
        value
    }
    fn response(&mut self, id: &str) -> Value {
        loop {
            let value = self.next();
            if value["type"] == "response" && value["id"] == id {
                return value;
            }
        }
    }
    fn project(&mut self, id: &str, control: Value) -> Value {
        self.send(json!({"schema_version":2,"type":"project","id":id,"project":control}));
        self.response(id)
    }
    fn finish(mut self) -> Vec<Value> {
        self.send(json!({"schema_version":2,"type":"shutdown","id":"test-stop"}));
        assert_eq!(self.response("test-stop")["success"], true);
        self.input.shutdown(std::net::Shutdown::Write).unwrap();
        self.join.take().unwrap().join().unwrap();
        self.records.clone()
    }
}
impl Drop for Wire {
    fn drop(&mut self) {
        let _ = self.input.shutdown(std::net::Shutdown::Write);
    }
}

// The busy-diff fixture reuses Wire's real asynchronous JSONL host. A separate
// test process supplies cwd A and proxy isolation without mutating test globals.
fn busy_diff_until(wire: &mut Wire, predicate: impl Fn(&Value) -> bool) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .expect("busy-diff JSONL deadline");
        wire.output
            .get_ref()
            .set_read_timeout(Some(remaining))
            .unwrap();
        let value = wire.next();
        println!("busy-diff recv {value}");
        if predicate(&value) {
            return value;
        }
    }
}

fn busy_diff_call(wire: &mut Wire, value: Value) -> Value {
    let id = value["id"].clone();
    println!("busy-diff send {value}");
    wire.send(value);
    busy_diff_until(wire, |v| v["type"] == "response" && v["id"] == id)
}

fn busy_diff_provider(listener: std::net::TcpListener, release: std::sync::mpsc::Receiver<()>) {
    use std::{io::Read, os::fd::AsRawFd};
    for index in 0..3 {
        let mut ready = libc::pollfd {
            fd: listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        assert_eq!(
            unsafe { libc::poll(&mut ready, 1, 10_000) },
            1,
            "provider accept deadline"
        );
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        // Same content-length HTTP/SSE fixture as headless_protocol's live steer
        // test, with a hard body bound and explicit gate instead of a sleep.
        let mut reader = BufReader::new((&mut stream).take(1_048_576));
        let mut length = None;
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                length = Some(value.trim().parse::<usize>().unwrap());
            }
        }
        let length = length.expect("provider content length");
        assert!(length <= 262_144);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        drop(reader);
        let request: Value = serde_json::from_slice(&body).unwrap();
        println!("busy-diff http {index} {request}");
        let partial = format!(
            "data: {{\"id\":\"gate-{index}\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"BUSY_GATE_{index}\"}},\"finish_reason\":null}}]}}\n\n"
        );
        let terminal = "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{partial}", partial.len() + terminal.len()).unwrap();
        stream.flush().unwrap();
        release
            .recv_timeout(Duration::from_secs(10))
            .expect("provider release deadline");
        // A cancellation may close the transport before this final write.
        let result = stream
            .write_all(terminal.as_bytes())
            .and_then(|()| stream.flush());
        println!("busy-diff provider released {index}: {result:?}");
    }
}

#[test]
fn busy_diff_uses_selected_project_through_new_replay_switch_and_cancel() {
    use std::{
        os::unix::process::CommandExt,
        process::{Command, Stdio},
        sync::mpsc,
    };
    const CHILD_ROOT: &str = "ZENPI_BUSY_DIFF_TEST_CHILD";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        busy_diff_selected_child(Path::new(&root));
        return;
    }
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("A")).unwrap();
    fs::create_dir(root.path().join("B")).unwrap();
    fs::create_dir(root.path().join("config")).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "busy_diff_uses_selected_project_through_new_replay_switch_and_cancel",
            "--nocapture",
        ])
        .current_dir(root.path().join("A"))
        .env(CHILD_ROOT, root.path())
        .env("ZENPI_HOME", root.path().join("config"))
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(Stdio::null())
        .process_group(0)
        .stdout(fs::File::create(root.path().join("stdout.log")).unwrap())
        .stderr(fs::File::create(root.path().join("stderr.log")).unwrap());
    // Names only: inherited proxy/auth/config values are never printed.
    for (name, _) in std::env::vars_os() {
        let key = name.to_string_lossy();
        if (key.starts_with("ZENPI_") && key != CHILD_ROOT && key != "ZENPI_HOME")
            || key.starts_with("OPENAI_")
            || [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
            ]
            .contains(&key.as_ref())
        {
            command.env_remove(name);
        }
    }
    let mut child = command.spawn().unwrap();
    let pid = child.id();
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let waiter = thread::spawn(move || {
        let _ = done_tx.send(child.wait());
    });
    let status = match done_rx.recv_timeout(Duration::from_secs(45)) {
        Ok(status) => status.unwrap(),
        Err(error) => {
            let killed = unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
            let reaped = done_rx.recv_timeout(Duration::from_secs(3));
            let evidence = root.keep();
            panic!(
                "busy-diff child deadline: {error}; kill={killed}, reap={reaped:?}; evidence={}",
                evidence.display()
            );
        }
    };
    waiter.join().unwrap();
    let stdout = fs::read_to_string(root.path().join("stdout.log")).unwrap();
    let stderr = fs::read_to_string(root.path().join("stderr.log")).unwrap();
    println!("busy-diff child pid={pid}, reaped=true, status={status}\n{stdout}\n{stderr}");
    if !status.success() {
        let evidence = root.keep();
        panic!(
            "busy-diff child failed; retained evidence={}",
            evidence.display()
        );
    }
    assert!(stdout.contains("busy-diff cleanup host joined; provider joined"));
}

fn busy_diff_selected_child(root: &Path) {
    use std::{net::TcpListener, process::Command, sync::mpsc};
    let a = root.join("A").canonicalize().unwrap();
    let b = root.join("B").canonicalize().unwrap();
    assert_eq!(std::env::current_dir().unwrap(), a);
    for (path, tag) in [(&a, "A"), (&b, "B")] {
        for args in [
            vec!["init", "--quiet"],
            vec!["add", "selected.txt"],
            vec![
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@invalid.test",
                "commit",
                "--quiet",
                "-m",
                "baseline",
            ],
        ] {
            if args[0] == "add" {
                fs::write(path.join("selected.txt"), "base\n").unwrap();
            }
            let out = Command::new("git")
                .args([
                    "-c",
                    "core.hooksPath=/dev/null",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(&args)
                .current_dir(path)
                .output()
                .unwrap();
            println!("busy-diff git {} {args:?}: {}", path.display(), out.status);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        fs::write(path.join("selected.txt"), format!("UNIQUE_DIFF_{tag}\n")).unwrap();
    }
    std::os::unix::fs::symlink(a.join("selected.txt"), b.join("escape-link")).unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    fs::write(root.join("config/config.toml"), format!("backend=\"openai\"\nprovider=\"openai\"\nmodel=\"gpt-4.1\"\nbase_url=\"{endpoint}\"\nwire_api=\"chat\"\nrequires_openai_auth=false\nmax_retries=0\n")).unwrap();
    let (release, gate) = mpsc::sync_channel(0);
    let provider = thread::spawn(move || busy_diff_provider(listener, gate));
    let initial = Agent::prepare_project_with_options(
        &root.join("initial.jsonl"),
        &a,
        Default::default(),
        false,
    )
    .unwrap();
    let mut wire = Wire::new(initial);
    let ca = busy_diff_call(
        &mut wire,
        json!({"schema_version":2,"type":"project","id":"a","project":{"action":"list"}}),
    )["project"]
        .clone();
    let mut cb = busy_diff_call(
        &mut wire,
        json!({"schema_version":2,"type":"project","id":"b","project":{"action":"open","cwd":b}}),
    )["project"]
        .clone();
    assert_eq!(ca["cwd"], a.to_str().unwrap());
    assert_eq!(cb["cwd"], b.to_str().unwrap());
    assert_ne!(ca["project_id"], cb["project_id"]);
    let mut diffs = Vec::new();
    for index in 0..3 {
        let prompt = format!("busy-{index}");
        wire.send(json!({"schema_version":2,"type":"prompt","id":prompt,"project_id":cb["project_id"],"text":format!("gate {index}")}));
        let delta = busy_diff_until(&mut wire, |v| {
            v["event"]["delta"] == format!("BUSY_GATE_{index}")
        });
        assert_eq!(delta["project"], cb);
        let request = json!({"schema_version":2,"type":"command","id":format!("diff-{index}"),"project_id":cb["project_id"],"text":"/diff selected.txt"});
        let response = busy_diff_call(&mut wire, request.clone());
        diffs.push(response.clone());
        // No release send has occurred, so the provider cannot emit its final
        // frame; the response and content are checked at that exact boundary.
        assert!(
            !wire
                .records
                .iter()
                .any(|v| v["type"] == "response" && v["id"] == prompt)
        );
        assert_eq!(response["project"], cb);
        println!("busy-diff gate held {index}; local response={response}");
        if index == 0 {
            for (id, path) in [
                ("deny-parent", "../A/selected.txt"),
                ("deny-link", "escape-link"),
            ] {
                let denied = busy_diff_call(
                    &mut wire,
                    json!({"schema_version":2,"type":"command","id":id,"text":format!("/diff {path}")}),
                );
                assert_eq!(denied["success"], false, "{denied}");
                assert_eq!(denied["project"], cb);
            }
            let foreign = busy_diff_call(
                &mut wire,
                json!({"schema_version":2,"type":"command","id":"foreign-diff","project_id":ca["project_id"],"text":"/diff selected.txt"}),
            );
            assert_eq!(foreign["code"], "project_mismatch");
        }
        if index == 1 {
            assert_eq!(
                busy_diff_call(
                    &mut wire,
                    json!({"schema_version":2,"type":"project","id":"switch-a","project":{"action":"select","id":ca["project_id"]}})
                )["project"],
                ca
            );
            let own_a = busy_diff_call(
                &mut wire,
                json!({"schema_version":2,"type":"command","id":"diff-a","text":"/diff selected.txt"}),
            );
            assert_eq!(own_a["project"], ca);
            assert!(
                own_a["data"]["diff"]
                    .as_str()
                    .unwrap()
                    .contains("UNIQUE_DIFF_A")
            );
            assert_eq!(
                busy_diff_call(&mut wire, request),
                response,
                "cached B response changed under A"
            );
            let foreign = busy_diff_call(
                &mut wire,
                json!({"schema_version":2,"type":"cancel","id":"foreign-cancel","target_id":prompt}),
            );
            assert_eq!(foreign["code"], "project_mismatch");
            assert_eq!(
                busy_diff_call(
                    &mut wire,
                    json!({"schema_version":2,"type":"project","id":"switch-b","project":{"action":"select","id":cb["project_id"]}})
                )["project"],
                cb
            );
            let back = busy_diff_call(
                &mut wire,
                json!({"schema_version":2,"type":"command","id":"diff-back","text":"/diff selected.txt"}),
            );
            assert_eq!(back["project"], cb);
            diffs.push(back);
        }
        if index == 2 {
            assert_eq!(
                busy_diff_call(
                    &mut wire,
                    json!({"schema_version":2,"type":"project","id":"close-busy","project":{"action":"close","id":cb["project_id"]}})
                )["code"],
                "project_error"
            );
            let cancel = busy_diff_call(
                &mut wire,
                json!({"schema_version":2,"type":"cancel","id":"cancel-b","target_id":prompt}),
            );
            assert_eq!(cancel["success"], true);
            assert_eq!(cancel["project"], cb);
        }
        release.send(()).unwrap();
        let terminal = busy_diff_until(&mut wire, |v| v["type"] == "response" && v["id"] == prompt);
        assert_eq!(terminal["project"], cb);
        for record in wire
            .records
            .iter()
            .filter(|v| v["request_id"] == prompt || v["id"] == prompt)
        {
            assert_eq!(
                record["project"], cb,
                "late B record changed owner: {record}"
            );
        }
        if index == 2 {
            assert_eq!(terminal["code"], "backend_cancelled");
        } else {
            assert_eq!(terminal["success"], true);
        }
        if index == 0 {
            let new = busy_diff_call(
                &mut wire,
                json!({"schema_version":2,"type":"command","id":"new-b","text":"/new"}),
            );
            assert_eq!(new["success"], true);
            assert_eq!(new["project"]["project_id"], cb["project_id"]);
            assert_ne!(new["project"]["session_id"], cb["session_id"]);
            cb = new["project"].clone();
        }
    }
    let records = wire.finish();
    provider.join().unwrap();
    assert_eq!(
        records
            .iter()
            .find(|v| v["type"] == "response" && v["id"] == "test-stop")
            .unwrap()["project"],
        cb
    );
    assert!(
        SessionStore::open_existing(root.join("initial.jsonl"))
            .unwrap()
            .turns()
            .is_empty()
    );
    fs::write(
        root.join("records.json"),
        serde_json::to_vec_pretty(&records).unwrap(),
    )
    .unwrap();
    println!("busy-diff cleanup host joined; provider joined");
    assert_eq!(std::env::current_dir().unwrap(), a);
    let wrong: Vec<_> = diffs
        .iter()
        .filter(|v| {
            v["success"] != true
                || v["data"]["diff"].as_str().is_none_or(|text| {
                    !text.contains("UNIQUE_DIFF_B") || text.contains("UNIQUE_DIFF_A")
                })
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "busy /diff must contain only selected B changes while provider gate is held: {wrong:?}"
    );
}
#[test]
fn project_controls_are_real_idempotent_and_restore_across_independent_hosts() {
    let root = tempdir().unwrap();
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    let mut wire = Wire::new(agent(root.path()));
    let initial = wire.project("initial", json!({"action":"list"}));
    let a = initial["project"]["project_id"].clone();
    let opened = wire.project("open", json!({"action":"open","cwd":other}));
    assert_eq!(opened["success"], true);
    let b = opened["project"]["project_id"].clone();
    assert_ne!(a, b);
    assert_eq!(
        opened["project"]["cwd"],
        other.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(
        wire.project("open", json!({"action":"open","cwd":other})),
        opened
    );
    assert_eq!(
        wire.project("open", json!({"action":"list"}))["code"],
        "request_id_conflict"
    );
    wire.send(json!({"schema_version":2,"type":"prompt","id":"in-b","text":"persist only in B"}));
    let result = wire.response("in-b");
    assert_eq!(result["project"]["project_id"], b);
    assert_eq!(result["success"], true);
    wire.finish();
    assert!(
        SessionStore::open_existing(root.path().join("initial.jsonl"))
            .unwrap()
            .turns()
            .is_empty()
    );
    let mut restarted = Wire::new(agent(root.path()));
    let restored = restarted.project("restored", json!({"action":"list"}));
    assert_eq!(restored["project"]["project_id"], b);
    assert_eq!(
        restored["data"]["workspace"]["tabs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    restarted.send(json!({"schema_version":2,"type":"status","id":"history"}));
    assert_eq!(
        restarted.response("history")["data"]["session"]["turn_count"],
        2
    );
    assert_eq!(
        restarted.project("back", json!({"action":"select","id":a}))["project"]["project_id"],
        a
    );
    assert_eq!(
        restarted.project("close", json!({"action":"close","id":b}))["success"],
        true
    );
    assert_eq!(
        restarted.project("last", json!({"action":"close","id":a}))["code"],
        "project_error"
    );
    restarted.finish();
}
#[test]
fn busy_switch_keeps_queued_owners_events_approval_and_tools_in_their_project() {
    let root = tempdir().unwrap();
    let other = root.path().join("same-name");
    fs::create_dir(&other).unwrap();
    let process_cwd = std::env::current_dir().unwrap();
    let mut wire = Wire::new(agent(root.path()));
    let a = wire.project("a", json!({"action":"list"}))["project"]["project_id"].clone();
    wire.send(json!({"schema_version":2,"type":"user_shell","id":"slow-a","text":"!sleep 0.4; printf original > marker.txt"}));
    loop {
        let v = wire.next();
        if v["request_id"] == "slow-a" && v["event"]["type"] == "request_started" {
            break;
        }
    }
    let b = wire.project("open-b", json!({"action":"open","cwd":other}))["project"]["project_id"]
        .clone();
    wire.send(
        json!({"schema_version":2,"type":"cancel","id":"foreign-cancel","target_id":"slow-a"}),
    );
    assert_eq!(wire.response("foreign-cancel")["code"], "project_mismatch");
    wire.send(json!({"schema_version":2,"type":"user_shell","id":"queued-b","text":"!printf second > marker.txt"}));
    assert_eq!(
        wire.project("close-b", json!({"action":"close","id":b}))["code"],
        "project_error"
    );
    let approval = loop {
        let v = wire.next();
        if v["event"]["type"] == "approval_request" {
            break v;
        }
    };
    assert_eq!(approval["project"]["project_id"], b);
    assert!(!other.join("marker.txt").exists());
    wire.send(json!({"schema_version":2,"type":"approve","id":"approve-b","approval_id":approval["event"]["approval"]["request_id"],"decision":"allow"}));
    assert_eq!(wire.response("approve-b")["success"], true);
    assert_eq!(wire.response("queued-b")["success"], true);
    let records = wire.finish();
    for v in records
        .iter()
        .filter(|v| v["request_id"] == "slow-a" || v["id"] == "slow-a")
    {
        assert_eq!(v["project"]["project_id"], a, "{v}");
    }
    for v in records
        .iter()
        .filter(|v| v["request_id"] == "queued-b" || v["id"] == "queued-b")
    {
        assert_eq!(v["project"]["project_id"], b, "{v}");
    }
    assert_eq!(
        fs::read_to_string(root.path().join("marker.txt")).unwrap(),
        "original"
    );
    assert_eq!(
        fs::read_to_string(other.join("marker.txt")).unwrap(),
        "second"
    );
    assert_eq!(std::env::current_dir().unwrap(), process_cwd);
}
#[test]
fn cancelled_invalid_and_mismatched_requests_leave_selection_and_journal_unchanged() {
    let root = tempdir().unwrap();
    let bad = root.path().join("bad");
    fs::create_dir_all(bad.join(".zenpi")).unwrap();
    fs::write(bad.join(".zenpi/config.toml"), "model = [ broken").unwrap();
    let mut wire = Wire::new(agent(root.path()));
    let before = wire.project("list", json!({"action":"list"}));
    let cancelled = wire.project("cancel", json!({"action":"open"}));
    assert_eq!(cancelled["data"]["cancelled"], true);
    assert_eq!(cancelled["data"]["workspace"], before["data"]["workspace"]);
    for (id, control) in [
        (
            "missing",
            json!({"action":"open","cwd":root.path().join("missing")}),
        ),
        ("invalid", json!({"action":"open","cwd":bad})),
        ("unknown", json!({"action":"select","id":"unknown"})),
    ] {
        assert_eq!(wire.project(id, control)["code"], "project_error");
    }
    wire.send(json!({"schema_version":2,"type":"prompt","id":"foreign","project_id":"wrong","text":"must not run"}));
    assert_eq!(wire.response("foreign")["code"], "project_mismatch");
    assert_eq!(
        wire.project("after", json!({"action":"list"}))["data"]["workspace"],
        before["data"]["workspace"]
    );
    wire.finish();
    assert!(!root.path().join("projects").exists());
    assert!(
        SessionStore::open_existing(root.path().join("initial.jsonl"))
            .unwrap()
            .turns()
            .is_empty()
    );
}
#[test]
fn shared_checkpoint_rejects_concurrent_stale_writer_and_corrupt_input_atomically() {
    let root = tempdir().unwrap();
    let one = root.path().join("one");
    let two = root.path().join("two");
    fs::create_dir(&one).unwrap();
    fs::create_dir(&two).unwrap();
    let mut first = ProjectOwnerPool::new(Arc::new(Mutex::new(agent(root.path())))).unwrap();
    let mut second = ProjectOwnerPool::new(Arc::new(Mutex::new(agent(root.path())))).unwrap();
    assert!(!first.restore_checkpoint().unwrap());
    assert!(!second.restore_checkpoint().unwrap());
    let before = second.workspace().clone();
    first.open(Some(&one)).unwrap();
    assert!(
        second
            .open(Some(&two))
            .unwrap_err()
            .contains("another host")
    );
    assert_eq!(second.workspace(), &before);
    let checkpoint = root.path().join("project-workspace.json");
    fs::write(
        &checkpoint,
        vec![b' '; zenpi::project_workspace::MAX_PROJECT_WORKSPACE_BYTES + 1],
    )
    .unwrap();
    assert!(second.restore_checkpoint().is_err());
    assert_eq!(second.workspace(), &before);
    fs::remove_file(&checkpoint).unwrap();
    std::os::unix::fs::symlink(root.path().join("initial.jsonl"), &checkpoint).unwrap();
    assert!(second.restore_checkpoint().is_err());
    assert_eq!(second.workspace(), &before);
}
#[test]
fn tui_and_jsonl_share_the_checkpoint_and_actual_session_owner() {
    use zenpi::tui::{ProjectIntent, ProjectRuntimeHost, TuiState};
    let root = tempdir().unwrap();
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    let mut state = TuiState::default();
    let mut host =
        ProjectRuntimeHost::new(&mut state, Arc::new(Mutex::new(agent(root.path())))).unwrap();
    host.restore_shared_workspace(&mut state).unwrap();
    host.apply(&mut state, ProjectIntent::Open(other.clone()))
        .unwrap();
    let id = state.active_project().to_owned();
    let session_id = host
        .active(&state)
        .lock()
        .unwrap()
        .session()
        .session_id()
        .to_owned();
    drop(host);
    let mut wire = Wire::new(agent(root.path()));
    let value = wire.project("same-owner", json!({"action":"list"}));
    assert_eq!(value["project"]["project_id"], id);
    assert_eq!(value["project"]["session_id"], session_id);
    wire.finish();
    let mut state = TuiState::default();
    let mut host =
        ProjectRuntimeHost::new(&mut state, Arc::new(Mutex::new(agent(root.path())))).unwrap();
    host.restore_shared_workspace(&mut state).unwrap();
    assert_eq!(state.active_project(), id);
    assert_eq!(
        host.active(&state)
            .lock()
            .unwrap()
            .attachment_workspace_root()
            .unwrap(),
        other.canonicalize().unwrap()
    );
}
#[test]
fn project_protocol_is_strict_versioned_bounded_and_owned() {
    let queue = json!({"schema_version":2,"type":"input_queue","id":"guarded-queue","project_id":"selected","session_id":"session","input_queue":{"action":"list","after_sequence":null,"limit":32}});
    assert!(
        zenpi::protocol::parse_line(&queue.to_string())
            .and_then(|r| r.into_command())
            .is_ok()
    );
    let mut invalid_queue = queue.clone();
    invalid_queue["extra"] = json!(true);
    assert!(zenpi::protocol::parse_line(&invalid_queue.to_string()).is_err());

    for value in [
        json!({"type":"project","id":"legacy","project":{"action":"list"}}),
        json!({"schema_version":2,"type":"project","project":{"action":"list"}}),
        json!({"schema_version":2,"type":"project","id":"bad","project":{"action":"open","cwd":""}}),
        json!({"schema_version":2,"type":"project","id":"bad","project":{"action":"open","cwd":"a".repeat(4097)}}),
        json!({"schema_version":2,"type":"project","id":"bad","project":{"action":"list","extra":true}}),
    ] {
        assert!(
            zenpi::protocol::parse_line(&value.to_string())
                .and_then(|r| r.into_command())
                .is_err(),
            "{value}"
        );
    }
    let root = tempdir().unwrap();
    let mut agent = agent(root.path());
    let mut output = Vec::new();
    zenpi::headless::run_headless(&mut agent,std::io::Cursor::new("{\"schema_version\":2,\"type\":\"project\",\"id\":\"list\",\"project\":{\"action\":\"list\"}}\n"),&mut output).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&output).unwrap()["code"],
        "requires_owned_host"
    );
}

#[test]
fn project_envelope_preserves_strict_identity_and_action_parsing() {
    for raw in [
        r#"{"schema_version":2,"type":"project","id":"a","id":"b","project":{"action":"list"}}"#,
        r#"{"schema_version":2,"type":"project","id":"a","project":{"action":"select","id":"one","id":"two"}}"#,
        r#"{"schema_version":2,"type":"project","id":"a","unexpected":true,"project":{"action":"list"}}"#,
        r#"{"schema_version":2,"type":"input_queue","id":"a","project_id":"one","project_id":"two","session_id":"s","input_queue":{"action":"list","limit":32}}"#,
    ] {
        assert!(
            zenpi::protocol::parse_line(raw)
                .and_then(|r| r.into_command())
                .is_err(),
            "accepted {raw}"
        );
    }
    let guarded_tree = r#"{"schema_version":2,"type":"tree","id":"tree","project_id":"selected","session_id":"s","tree":{"action":"enable"}}"#;
    assert!(
        zenpi::protocol::parse_line(guarded_tree)
            .and_then(|r| r.into_command())
            .is_ok()
    );
}

#[test]
fn typed_tree_mutation_runs_in_the_selected_project_owner() {
    let root = tempdir().unwrap();
    let other = root.path().join("branch-project");
    fs::create_dir(&other).unwrap();
    let mut wire = Wire::new(agent(root.path()));
    let opened = wire.project("open-branch", json!({"action":"open","cwd":other}));
    assert_eq!(opened["success"], true);
    let context = &opened["project"];
    wire.send(
        json!({"schema_version":2,"type":"tree","id":"enable-selected-tree",
        "project_id":context["project_id"],"session_id":context["session_id"],
        "tree":{"action":"enable"}}),
    );
    let response = wire.response("enable-selected-tree");
    assert_eq!(response["success"], true, "{response}");
    assert_eq!(response["project"], *context);
    wire.finish();
    let initial = fs::read_to_string(root.path().join("initial.jsonl")).unwrap();
    assert!(!initial.contains("\"type\":\"session_tree\""));
    let saved: Value =
        serde_json::from_slice(&fs::read(root.path().join("project-workspace.json")).unwrap())
            .unwrap();
    let journal = saved["sessions"][context["project_id"].as_str().unwrap()]
        .as_str()
        .unwrap();
    assert!(
        fs::read_to_string(journal)
            .unwrap()
            .contains("\"type\":\"session_tree\"")
    );
}

#[test]
fn project_capacity_and_checkpoint_permission_failure_preserve_the_previous_owner() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempdir().unwrap();
    let mut pool = ProjectOwnerPool::new(Arc::new(Mutex::new(agent(root.path())))).unwrap();
    pool.restore_checkpoint().unwrap();
    let initial = pool.active_context().project_id.clone();
    let one = root.path().join("one");
    fs::create_dir(&one).unwrap();
    pool.open(Some(&one)).unwrap();
    let before = pool.workspace().clone();
    let checkpoint = root.path().join("project-workspace.json");
    let bytes = fs::read(&checkpoint).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let result = pool.select(&initial);
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err());
    assert_eq!(pool.workspace(), &before);
    assert_eq!(fs::read(&checkpoint).unwrap(), bytes);
    let mut candidate = pool.workspace().clone();
    for i in 2..zenpi::project_workspace::MAX_PROJECT_TABS {
        let path = root.path().join(format!("p-{i}"));
        fs::create_dir(&path).unwrap();
        candidate = candidate
            .with_directory(Some(&path), root.path())
            .unwrap()
            .0;
    }
    pool.publish(candidate, &Default::default()).unwrap();
    assert_eq!(pool.workspace().tabs().len(), 64);
    let extra = root.path().join("overflow");
    fs::create_dir(&extra).unwrap();
    let before = pool.workspace().clone();
    assert!(pool.open(Some(&extra)).unwrap_err().contains("limit"));
    assert_eq!(pool.workspace(), &before);
}
#[test]
fn selected_project_bounds_inspection_resume_and_reopening_the_initial_tab() {
    let root = tempdir().unwrap();
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    fs::write(root.path().join("only-original.txt"), "original").unwrap();
    fs::write(other.join("only-other.txt"), "other").unwrap();
    let foreign = SessionStore::open_in_workspace(other.join("foreign.jsonl"), &other).unwrap();
    drop(foreign);
    let mut wire = Wire::new(agent(root.path()));
    let a = wire.project("a", json!({"action":"list"}))["project"]["project_id"].clone();
    wire.send(json!({"schema_version":2,"type":"resume","id":"foreign-resume","path":other.join("foreign.jsonl")}));
    assert_eq!(wire.response("foreign-resume")["success"], false);
    let b =
        wire.project("open", json!({"action":"open","cwd":other}))["project"]["project_id"].clone();
    wire.send(json!({"schema_version":2,"type":"resources","id":"escape","path":".."}));
    assert_eq!(wire.response("escape")["success"], false);
    wire.send(json!({"schema_version":2,"type":"resources","id":"own","path":"."}));
    let snapshot = wire.response("own");
    assert_eq!(snapshot["success"], true);
    assert_eq!(snapshot["project"]["project_id"], b);
    assert_eq!(
        wire.project("close-a", json!({"action":"close","id":a}))["success"],
        true
    );
    let reopened = wire.project("reopen-a", json!({"action":"open","cwd":root.path()}));
    assert_eq!(reopened["success"], true);
    assert_eq!(reopened["project"]["project_id"], a);
    wire.finish();
    let mut restarted = Wire::new(agent(root.path()));
    assert_eq!(
        restarted.project("restored-a", json!({"action":"list"}))["project"]["project_id"],
        a
    );
    restarted.finish();
}

#[test]
fn resumed_journal_restores_as_the_real_agent_in_the_shared_project_pool() {
    let root = tempdir().unwrap();
    let alternate = root.path().join("alternate.jsonl");
    let mut second =
        Agent::with_echo(SessionStore::open_in_workspace(&alternate, root.path()).unwrap());
    second
        .process(zenpi::core::TurnInputRequest::new("alternate history"))
        .unwrap();
    drop(second);
    let mut wire = Wire::new(agent(root.path()));
    wire.send(json!({"schema_version":2,"type":"resume","id":"resume-alternate","path":alternate}));
    let resumed = wire.response("resume-alternate");
    assert_eq!(resumed["success"], true);
    let session = resumed["project"]["session_id"].clone();
    wire.finish();
    let mut wire = Wire::new(agent(root.path()));
    let restored = wire.project("restored-alternate", json!({"action":"list"}));
    assert_eq!(restored["project"]["session_id"], session);
    wire.send(json!({"schema_version":2,"type":"status","id":"alternate-history"}));
    assert_eq!(
        wire.response("alternate-history")["data"]["session"]["turn_count"],
        2
    );
    wire.finish();
}

#[test]
fn rejected_resume_checkpoint_keeps_actual_owner_and_host_usable() {
    use std::os::unix::fs::PermissionsExt;
    for slash in [false, true] {
        let root = tempdir().unwrap();
        let alternate = root.path().join("alternate.jsonl");
        let mut target =
            Agent::with_echo(SessionStore::open_in_workspace(&alternate, root.path()).unwrap());
        target.set_persona("ISTJ").unwrap();
        let target_id = target.session().session_id().to_owned();
        drop(target);
        let mut initial = agent(root.path());
        initial.set_persona("ESFP").unwrap();
        let mut wire = Wire::new(initial);
        let old = wire.project("old", json!({"action":"list"}))["project"].clone();
        assert_eq!(
            wire.project("save", json!({"action":"select", "id":old["project_id"]}))["success"],
            true
        );
        let checkpoint = root.path().join("project-workspace.json");
        let before = fs::read(&checkpoint).unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let request = if slash {
            json!({"schema_version":2,"type":"command","id":"resume-fail","text":format!("/session open {}", alternate.display())})
        } else {
            json!({"schema_version":2,"type":"resume","id":"resume-fail","path":alternate})
        };
        wire.send(request.clone());
        let response = wire.response("resume-fail");
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(response["success"], false, "{response}");
        assert_eq!(
            wire.project("after", json!({"action":"list"}))["project"],
            old
        );
        wire.send(
            json!({"schema_version":2,"type":"command","id":"persona-after","text":"/persona"}),
        );
        assert!(wire.response("persona-after").to_string().contains("ESFP"));
        assert_eq!(fs::read(&checkpoint).unwrap(), before);
        let mut retry = request;
        retry["id"] = json!("resume-retry");
        wire.send(retry);
        let response = wire.response("resume-retry");
        assert_eq!(response["success"], true, "{response}");
        assert_eq!(response["project"]["session_id"], target_id);
        wire.send(
            json!({"schema_version":2,"type":"command","id":"persona-resumed","text":"/persona"}),
        );
        assert!(
            wire.response("persona-resumed")
                .to_string()
                .contains("ISTJ")
        );
        wire.finish();
        let mut restart = Wire::new(agent(root.path()));
        assert_eq!(
            restart.project("restored", json!({"action":"list"}))["project"]["session_id"],
            target_id
        );
        restart.finish();
    }
}
