#![cfg(unix)]
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    thread,
    time::Duration,
};
use tempfile::tempdir;
use zenpi::{
    approval::{ApprovalMode, ApprovalPolicy},
    core::Agent,
    session::SessionStore,
    tools::{SideEffectPolicy, ToolContext, ToolRegistry},
};

// Native fixture compilation and cold executable setup must not compete with
// the other fixture's real 1000 ms extension initialization deadline.
static EXTENSION_FIXTURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
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

impl Wire {
    fn command(&mut self, id: &str, text: &str) -> Value {
        self.send(json!({"schema_version":2,"type":"command","id":id,"text":text}));
        self.response(id)
    }
    fn prompt(&mut self, id: &str, text: &str) -> Value {
        self.send(json!({"schema_version":2,"type":"prompt","id":id,"text":text}));
        self.response(id)
    }
}

#[test]
fn bare_new_is_the_only_public_syntax_and_is_registered() {
    use zenpi::slash::{SessionAction, SlashCommand, parse};
    assert!(matches!(
        parse("/new").unwrap(),
        Some(SlashCommand::Session {
            action: SessionAction::New
        })
    ));
    assert!(parse("/new destination").is_err());
    assert!(parse("/session new").is_err());
    assert!(zenpi::slash::spec("new").is_some());
}

#[test]
fn old_port_cannot_acquire_the_new_sessions_running_turn() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use zenpi::backend::{Backend, BackendError, Completion, CompletionRequest};
    struct HoldingBackend {
        started: Arc<AtomicBool>,
        release: Arc<AtomicBool>,
    }
    impl Backend for HoldingBackend {
        fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
            self.started.store(true, Ordering::Release);
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while !self.release.load(Ordering::Acquire) {
                assert!(
                    std::time::Instant::now() < deadline,
                    "test did not release provider"
                );
                thread::sleep(Duration::from_millis(1));
            }
            Ok(Completion::text("new owner reply"))
        }
    }
    struct ReleaseOnDrop(Arc<AtomicBool>);
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let root = tempdir().unwrap();
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let _release_on_drop = ReleaseOnDrop(release.clone());
    let initial = Agent::new(
        SessionStore::open_in_workspace(root.path().join("initial.jsonl"), root.path()).unwrap(),
        Box::new(HoldingBackend {
            started: started.clone(),
            release: release.clone(),
        }),
    );
    let old_port = initial.input_port();
    let mut wire = Wire::new(initial);
    let new = wire.command("new-for-port", "/new");
    assert_eq!(new["success"], true, "{new}");
    let new_id = new["data"]["new_session_id"].as_str().unwrap();
    wire.send(
        json!({"schema_version":2,"type":"prompt","id":"running-new","text":"new owner input"}),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !started.load(Ordering::Acquire) {
        assert!(
            std::time::Instant::now() < deadline,
            "new provider never started"
        );
        thread::sleep(Duration::from_millis(1));
    }
    for session_id in [old_port.session_id(), new_id.to_owned()] {
        let rejected = old_port.submit(zenpi::protocol::InputQueueRequest {
            schema_version: 2,
            id: "old-port-request".into(),
            kind: "input_queue".into(),
            session_id,
            input_queue: zenpi::protocol::InputQueueAction::Enqueue {
                input_id: "old-port-input".into(),
                kind: zenpi::input_queue::InputKind::Steer,
                text: "STALE_INPUT_MUST_NOT_ARRIVE".into(),
            },
        });
        assert!(
            rejected.is_err(),
            "revoked old port accepted input during new owner's turn"
        );
    }
    release.store(true, Ordering::Release);
    let response = wire.response("running-new");
    assert_eq!(response["success"], true, "{response}");
    assert_eq!(response["project"]["session_id"], new_id);
    wire.finish();
    let bytes = fs::read_to_string(new["data"]["new_session_path"].as_str().unwrap()).unwrap();
    assert!(!bytes.contains("STALE_INPUT_MUST_NOT_ARRIVE"));
}

#[test]
fn new_keeps_project_and_preferences_but_has_empty_history_and_durable_replay() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("项目 space");
    fs::create_dir(&root).unwrap();
    let mut initial = agent(&root);
    initial.set_persona("ESFP").unwrap();
    let old_port = initial.input_port();
    let old_id = initial.session().session_id().to_owned();
    let mut wire = Wire::new(initial);
    assert_eq!(
        wire.prompt("seed", "old conversation body")["success"],
        true
    );
    assert_eq!(
        wire.project("tabs-before", json!({"action":"list"}))["data"]["workspace"]["tabs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let old_bytes = fs::read(root.join("initial.jsonl")).unwrap();
    let new = wire.command("create", "/new");
    assert_eq!(new["success"], true, "{new}");
    assert_eq!(new["data"]["old_session_id"], old_id);
    assert_eq!(new["data"]["project_id"], new["project"]["project_id"]);
    assert_eq!(
        new["data"]["cwd"],
        root.canonicalize().unwrap().display().to_string()
    );
    let new_id = new["data"]["new_session_id"].as_str().unwrap();
    assert_ne!(old_id, new_id);
    assert_eq!(old_port.session_id(), old_id);
    let new_path = Path::new(new["data"]["new_session_path"].as_str().unwrap());
    let session = SessionStore::open_existing(new_path).unwrap();
    assert!(session.turns().is_empty());
    assert!(
        session
            .events()
            .iter()
            .any(|e| e["type"] == "persona_selected" && e["persona"] == "ESFP")
    );
    assert!(
        fs::read(root.join("initial.jsonl"))
            .unwrap()
            .starts_with(&old_bytes)
    );
    let files = fs::read_dir(&root).unwrap().count();
    assert_eq!(wire.command("create", "/new"), new);
    assert_eq!(
        wire.command("create", "/status")["code"],
        "request_id_conflict"
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), files);
    assert_eq!(
        wire.prompt("next", "fresh conversation body")["success"],
        true
    );
    assert_eq!(
        SessionStore::open_existing(new_path).unwrap().turns().len(),
        2
    );
    wire.finish();
    let mut restart = Wire::new(agent(&root));
    assert_eq!(restart.command("create", "/new"), new);
    assert_eq!(
        restart.command("create", "/status")["code"],
        "request_id_conflict"
    );
    let old = restart.command(
        "open-old",
        &format!(
            "/session open {}",
            serde_json::to_string(&root.join("initial.jsonl")).unwrap()
        ),
    );
    assert_eq!(old["success"], true, "{old}");
    assert_eq!(old["project"]["session_id"], old_id);
    assert_eq!(
        restart.command(
            "open-new",
            &format!("/session open {}", serde_json::to_string(new_path).unwrap())
        )["project"]["session_id"],
        new_id
    );
    restart.finish();
}

#[test]
fn running_request_rejects_new_without_cancelling_old_work_then_allows_retry() {
    let root = tempdir().unwrap();
    let mut wire = Wire::new(agent(root.path()));
    wire.send(json!({"schema_version":2,"type":"user_shell","id":"running","text":"!sleep 0.3; printf settled > settled.txt"}));
    loop {
        let row = wire.next();
        if row["request_id"] == "running" && row["event"]["type"] == "request_started" {
            break;
        }
    }
    let rejected = wire.command("busy-new", "/new");
    assert_eq!(rejected["code"], "agent_busy", "{rejected}");
    assert_eq!(wire.response("running")["success"], true);
    assert_eq!(
        fs::read_to_string(root.path().join("settled.txt")).unwrap(),
        "settled"
    );
    assert_eq!(wire.command("settled-new", "/new")["success"], true);
    wire.finish();
}

#[test]
fn failed_owner_checkpoint_preserves_old_session_and_removes_candidate() {
    let root = tempdir().unwrap();
    let initial = agent(root.path());
    let old_id = initial.session().session_id().to_owned();
    let mut wire = Wire::new(initial);
    assert_eq!(wire.prompt("before", "old request")["success"], true);
    // A real filesystem replacement defeats the checkpoint CAS; the host must
    // not overwrite it or leave an unpublished candidate behind.
    let checkpoint = root.path().join("project-workspace.json");
    fs::write(&checkpoint, b"foreign checkpoint").unwrap();
    let rejected = wire.command("failed-new", "/new");
    assert_eq!(rejected["success"], false, "{rejected}");
    assert_eq!(fs::read(&checkpoint).unwrap(), b"foreign checkpoint");
    assert!(!fs::read_dir(root.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("new-")
    }));
    assert_eq!(
        wire.prompt("after", "owner still usable")["project"]["session_id"],
        old_id
    );
    wire.finish();
}

fn gate_extension(root: &Path, initial: &mut Agent) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let extensions = root.join("extensions");
    let gate = extensions.join("gate");
    fs::create_dir_all(&gate).unwrap();
    fs::write(gate.join("extension.toml"), "name='gate'\nversion='1.0.0'\napi_version=2\nexecutable='plugin'\nhook_timeout_ms=1000\nhooks=['session_start']\n").unwrap();
    fs::write(gate.join("plugin.c"), r#"
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <unistd.h>
int main(void) {
    char line[65536];
    if (!fgets(line, sizeof(line), stdin)) return 2;
    int initialize = strstr(line, "\"method\":\"initialize\"") != NULL;
    char *id = strstr(line, "\"id\":");
    char *cap = strstr(line, "\"capability\":{");
    if (!id || !cap) return 3;
    unsigned long request = strtoul(id + 5, NULL, 10);
    cap += 13;
    char *end = strchr(cap, '}');
    if (!end) return 4;
    end[1] = 0;
    if (initialize && access("block", F_OK) == 0) {
        FILE *marker = fopen("entered", "w");
        if (!marker) return 5;
        fputs("actual initialize process", marker); fclose(marker);
        while (access("release", F_OK) != 0) usleep(2000);
    }
    printf("{\"jsonrpc\":\"2.0\",\"id\":%lu,\"capability\":%s,\"result\":%s}\n", request, cap,
        initialize ? "{\"api_version\":2,\"hooks\":[\"session_start\"],\"tools\":[]}" : "{\"action\":\"continue\"}");
    return 0;
}
"#).unwrap();
    assert!(
        std::process::Command::new("cc")
            .args(["-O0", "-o"])
            .arg(gate.join("plugin"))
            .arg(gate.join("plugin.c"))
            .status()
            .unwrap()
            .success()
    );
    fs::set_permissions(gate.join("plugin"), fs::Permissions::from_mode(0o700)).unwrap();
    // Establish the executable fixture before timing the real host's
    // cancellable initialize phase. The cold executable launch is diagnostic,
    // not a successful extension/cancellation or startup-budget observation.
    let cold = std::time::Instant::now();
    let probe = std::process::Command::new(gate.join("plugin"))
        .current_dir(&gate)
        .stdin(std::process::Stdio::null())
        .status()
        .unwrap();
    eprintln!(
        "new_fixture_cold_executable_ms={}",
        cold.elapsed().as_millis()
    );
    assert_eq!(probe.code(), Some(2));
    initial.configure_extensions(&extensions, || false).unwrap();
    fs::write(gate.join("block"), "").unwrap();
    gate
}
fn wait_file(path: &Path) {
    let start = std::time::Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "gate never reached: {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn actual_extension_preparation_jsonl_cancel_keeps_owner_and_old_port() {
    let _fixture_guard = EXTENSION_FIXTURE_LOCK.lock().unwrap();
    let root = tempdir().unwrap();
    let mut initial = agent(root.path());
    let old_id = initial.session().session_id().to_owned();
    let gate = gate_extension(root.path(), &mut initial);
    let mut wire = Wire::new(initial);
    wire.send(json!({"schema_version":2,"type":"command","id":"new-cancel","text":"/new"}));
    wait_file(&gate.join("entered"));
    wire.send(json!({"schema_version":2,"type":"cancel","id":"cancel","target_id":"new-cancel"}));
    assert_eq!(wire.response("cancel")["success"], true);
    let response = wire.response("new-cancel");
    assert_eq!(response["code"], "backend_cancelled", "{response}");
    assert_eq!(response["project"]["session_id"], old_id);
    assert!(!fs::read_dir(root.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("new-")
    }));
    assert_eq!(
        wire.prompt("old-next", "still old")["project"]["session_id"],
        old_id
    );
    fs::write(gate.join("release"), "").unwrap();
    assert_eq!(wire.command("retry", "/new")["success"], true);
    wire.finish();
}

#[test]
fn replaced_candidate_regular_link_and_directory_are_retained_without_switching() {
    let _fixture_guard = EXTENSION_FIXTURE_LOCK.lock().unwrap();
    use std::os::unix::fs::symlink;
    for kind in ["regular", "link", "directory"] {
        let root = tempdir().unwrap();
        let mut initial = agent(root.path());
        let old_id = initial.session().session_id().to_owned();
        let gate = gate_extension(root.path(), &mut initial);
        let mut wire = Wire::new(initial);
        wire.send(json!({"schema_version":2,"type":"command","id":"replace","text":"/new"}));
        wait_file(&gate.join("entered"));
        let path = fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.file_name().unwrap().to_string_lossy().starts_with("new-"))
            .unwrap();
        fs::remove_file(&path).unwrap();
        let foreign = root.path().join("foreign");
        fs::write(&foreign, "FOREIGN BODY").unwrap();
        match kind {
            "regular" => fs::write(&path, "FOREIGN BODY").unwrap(),
            "link" => symlink(&foreign, &path).unwrap(),
            _ => fs::create_dir(&path).unwrap(),
        }
        fs::write(gate.join("release"), "").unwrap();
        let response = wire.response("replace");
        assert_eq!(response["success"], false, "{kind}: {response}");
        assert!(
            response["error"].as_str().unwrap().contains("retained"),
            "{response}"
        );
        assert_eq!(fs::read_to_string(&foreign).unwrap(), "FOREIGN BODY");
        assert!(fs::symlink_metadata(&path).is_ok());
        assert_eq!(
            wire.prompt("still-old", "owner survives foreign candidate")["project"]["session_id"],
            old_id
        );
        wire.finish();
    }
}

#[test]
fn staged_attachment_is_not_dropped_or_moved_by_new() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("attachment.txt"), "attachment payload").unwrap();
    let mut initial = agent(root.path());
    let old_id = initial.session().session_id().to_owned();
    initial
        .stage_attachment(zenpi::backend::InputAttachment {
            kind: zenpi::backend::AttachmentKind::File,
            mime_type: "text/plain".into(),
            path: Some("attachment.txt".into()),
            url: None,
            file_id: None,
        })
        .unwrap();
    let mut wire = Wire::new(initial);
    let response = wire.command("attached", "/new");
    assert_eq!(response["success"], false, "{response}");
    assert_eq!(response["project"]["session_id"], old_id);
    assert!(response["error"].as_str().unwrap().contains("attachments"));
    wire.finish();
}

#[test]
fn exclusive_names_skip_existing_files_links_directories_and_stop_at_limit() {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::{PermissionsExt, symlink};
    for limit in [false, true] {
        let root = tempdir().unwrap();
        let initial = agent(root.path());
        let old_id = initial.session().session_id().to_owned();
        let mut normalized =
            zenpi::protocol::parse_line(r#"{"schema_version":2,"type":"command","text":"/new"}"#)
                .unwrap();
        normalized.id = None;
        let mut hasher = Sha256::new();
        hasher.update(b"zenpi-headless-request-v1\0");
        hasher.update(serde_json::to_vec(&normalized).unwrap());
        let fingerprint = format!("{:x}", hasher.finalize());
        let digest = format!(
            "{:x}",
            Sha256::digest(format!("{old_id}:collision:{fingerprint}").as_bytes())
        );
        let name = |i| root.path().join(format!("new-{digest}-{i}.jsonl"));
        fs::write(name(0), "FOREIGN").unwrap();
        symlink(name(0), name(1)).unwrap();
        fs::create_dir(name(2)).unwrap();
        if limit {
            for i in 3..16 {
                fs::write(name(i), "FOREIGN").unwrap();
            }
        }
        let mut wire = Wire::new(initial);
        let result = wire.command("collision", "/new");
        assert_eq!(result["success"], !limit, "{result}");
        if !limit {
            assert_eq!(
                result["data"]["new_session_path"],
                name(3).canonicalize().unwrap().display().to_string()
            );
            assert_eq!(
                fs::metadata(name(3)).unwrap().permissions().mode() & 0o777,
                0o600
            );
        } else {
            assert!(
                result["error"]
                    .as_str()
                    .unwrap()
                    .contains("collision limit")
            );
        }
        assert_eq!(fs::read_to_string(name(0)).unwrap(), "FOREIGN");
        assert!(
            fs::symlink_metadata(name(1))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(name(2).is_dir());
        assert_eq!(wire.prompt("next", "still usable")["success"], true);
        wire.finish();
    }
}

#[test]
fn actual_directory_permission_failure_keeps_old_owner_usable() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempdir().unwrap();
    let mut wire = Wire::new(agent(root.path()));
    let before = wire.prompt("before", "old prompt");
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let failed = wire.command("permission", "/new");
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        failed["success"], false,
        "must execute as an ordinary user: {failed}"
    );
    assert_eq!(
        wire.prompt("after", "old still usable")["project"]["session_id"],
        before["project"]["session_id"]
    );
    wire.finish();
}

#[test]
fn durable_pending_input_and_unknown_operation_are_not_erased() {
    for unknown in [false, true] {
        let root = tempdir().unwrap();
        let mut store =
            SessionStore::open_in_workspace(root.path().join("initial.jsonl"), root.path())
                .unwrap();
        if unknown {
            store
                .begin_operation_with_key(
                    &zenpi::session::InterruptedOperation {
                        operation_id: "uncertain".into(),
                        kind: zenpi::session::OperationKind::Tool,
                        turn_id: "old-turn".into(),
                        retry_requires_confirmation: true,
                    },
                    "uncertain-key",
                )
                .unwrap();
        }
        drop(store);
        let mut initial = agent(root.path());
        if !unknown {
            initial
                .input_queue_request(zenpi::protocol::InputQueueRequest {
                    schema_version: 2,
                    id: "queued".into(),
                    kind: "input_queue".into(),
                    session_id: initial.session().session_id().to_owned(),
                    input_queue: zenpi::protocol::InputQueueAction::Enqueue {
                        input_id: "old-followup".into(),
                        kind: zenpi::input_queue::InputKind::FollowUp,
                        text: "preserve queued input".into(),
                    },
                })
                .unwrap();
        }
        let old = initial.session().session_id().to_owned();
        let bytes = fs::read(root.path().join("initial.jsonl")).unwrap();
        let mut wire = Wire::new(initial);
        let rejected = wire.command("unsettled", "/new");
        assert_eq!(rejected["success"], false, "{rejected}");
        assert_eq!(rejected["project"]["session_id"], old);
        assert!(
            fs::read(root.path().join("initial.jsonl"))
                .unwrap()
                .starts_with(&bytes)
        );
        wire.finish();
    }
}
