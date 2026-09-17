//! ZS1-143 runtime acceptance: drive the production binary in headless mode
//! (the same host the TUI drives) through three use cases and assert the
//! slash-control surface, the runtime-intent handoff, and the BentoBox panes.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

struct Host {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<Value>,
    reader: Option<thread::JoinHandle<()>>,
}

impl Host {
    fn start(root: &Path) -> Self {
        std::fs::create_dir_all(root.join("user")).unwrap();
        std::fs::write(
            root.join("user/config.toml"),
            "backend='openai'\nprovider='openai'\nmodel='accept-model'\nbase_url='http://127.0.0.1:1/v1'\nwire_api='chat'\nrequires_openai_auth=false\n",
        )
        .unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_zenpi"))
            .args(["--headless", "--session", "cli.jsonl"])
            .current_dir(root)
            .env("ZENPI_HOME", root.join("user"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    let _ = send.send(value);
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
            reader: Some(reader),
        }
    }

    fn command(&mut self, id: &str, text: &str) -> Value {
        self.stdin
            .write_all(format!("{{\"schema_version\":2,\"type\":\"command\",\"id\":{id:?},\"text\":{text:?}}}\n").as_bytes())
            .unwrap();
        self.stdin.flush().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while std::time::Instant::now() < deadline {
            if let Ok(value) = self.lines.recv_timeout(Duration::from_millis(200))
                && value["type"] == "response"
                && value["id"] == id
            {
                return value;
            }
        }
        panic!("no response for {text}");
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.stdin.write_all(b"{\"schema_version\":2,\"type\":\"shutdown\",\"id\":\"stop\"}\n");
        let _ = self.stdin.flush();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn temp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn use_case_one_blueprint_plan_and_execute() {
    let dir = temp();
    let mut host = Host::start(dir.path());
    // /blueprint owner is browsable and local (no provider turn).
    assert_eq!(host.command("b", "/blueprint list")["success"], true);
    // /plan explains its syntax instead of pretending to run.
    let plan = host.command("p", "/plan");
    assert_eq!(plan["success"], false);
    assert_eq!(plan["code"], "plan_syntax");
    // /execute persists an inert intent for the external owner.
    let execute = host.command("e", "/execute start BP-1");
    assert_eq!(execute["success"], true, "{execute}");
    assert_eq!(execute["data"]["zenpi_started"], false);
    assert_eq!(execute["data"]["delivery"], "journal_only");
}

#[test]
fn use_case_two_learn_is_local_and_read_only() {
    let dir = temp();
    let mut host = Host::start(dir.path());
    let learn = host.command("l", "/learn");
    assert_eq!(learn["success"], true, "{learn}");
    assert_eq!(learn["data"]["command"], "learn");
}

#[test]
fn use_case_three_explore_and_addloop_are_inert_intents() {
    let dir = temp();
    let mut host = Host::start(dir.path());
    let explore = host.command("x", "/explore why sse stalls");
    assert_eq!(explore["success"], true, "{explore}");
    assert_eq!(explore["data"]["zenpi_started"], false);
    assert_eq!(explore["data"]["delivery"], "journal_only");
    let addloop = host.command("a", "/addloop start audit");
    assert_eq!(addloop["success"], true, "{addloop}");
    assert_eq!(addloop["data"]["zenpi_started"], false);
    let status = host.command("s", "/loop status");
    assert_eq!(status["success"], true, "{status}");
    assert!(status["data"]["stored_count"].as_u64().unwrap_or(0) >= 1);
}

#[test]
fn panes_arch_execution_and_bentobox_are_addressable() {
    let dir = temp();
    let mut host = Host::start(dir.path());
    // Layer-2 worktree sub-tabs default to one Main tab reusing the layer-1 data.
    let list = host.command("w", "/worktree list");
    assert_eq!(list["success"], true, "{list}");
    // BentoBox panes (including the new arch/execution) are addressable.
    assert_eq!(host.command("pa", "/pane arch")["success"], true);
    assert_eq!(host.command("pe", "/pane execution")["success"], true);
    assert_eq!(host.command("lg", "/layout")["success"], true);
    let _ = json!({});
}
