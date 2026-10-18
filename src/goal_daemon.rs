//! Lightweight detached supervisor for `/goal` continuation.
//!
//! The supervisor never opens the session. It starts one short-lived worker
//! process at a time; each worker owns the session, executes one Blueprint
//! attempt, and exits. Goal status and receipts are the durable checkpoint, so
//! killing and restarting either process resumes from the same attempt.

use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use crate::{
    domain_execution::{ExecutionStore, path_for_session as execution_path_for_session},
    domain_store::{self, DomainStore},
    domains::GoalStatus,
};

#[derive(Debug, Clone, Default)]
pub struct WorkerConfig {
    pub tools: Option<Vec<String>>,
    pub backend: Option<String>,
    pub profile: Option<String>,
    pub model: Option<String>,
}

impl WorkerConfig {
    fn append_args(&self, command: &mut Command) {
        if let Some(tools) = &self.tools {
            let value = if tools.is_empty() {
                "none".to_owned()
            } else {
                tools.join(",")
            };
            command.arg("--tools").arg(value);
        }
        if let Some(backend) = &self.backend {
            command.arg("--backend").arg(backend);
        }
        if let Some(profile) = &self.profile {
            command.arg("--profile").arg(profile);
        }
        if let Some(model) = &self.model {
            command.arg("--model").arg(model);
        }
    }
}

#[derive(Debug, Clone)]
pub struct DaemonPaths {
    pub pid: PathBuf,
    pub status: PathBuf,
    pub log: PathBuf,
    pub stop: PathBuf,
}

impl DaemonPaths {
    pub fn for_session(session: &Path, target: &str) -> Result<Self, String> {
        let parent = session
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let stem = format!("goal-daemon-{}", sanitize_target(target));
        Ok(Self {
            pid: parent.join(format!("{stem}.pid")),
            status: parent.join(format!("{stem}.json")),
            log: parent.join(format!("{stem}.log")),
            stop: parent.join(format!("{stem}.stop")),
        })
    }
}

#[derive(Debug, Clone)]
pub struct GoalState {
    pub goal_id: String,
    pub goal_status: String,
    pub terminal: bool,
    pub receipt_count: usize,
    pub attempts_used: u32,
    pub latest_receipt_status: Option<String>,
    pub latest_receipt_error: Option<String>,
}

pub fn goal_state(session: &Path, target: &str) -> Result<GoalState, String> {
    let (blueprint_id, version) = target
        .split_once('@')
        .map_or((target, None), |(id, version)| (id, Some(version)));
    let domains = DomainStore::open_read_only(domain_store::path_for_session(session))
        .map_err(|error| error.to_string())?;
    let goals = domains
        .goals()
        .into_iter()
        .filter(|goal| {
            goal.blueprint_id == blueprint_id
                && version.is_none_or(|version| goal.blueprint_version == version)
        })
        .collect::<Vec<_>>();
    let goal = match goals.as_slice() {
        [goal] => *goal,
        [] => return Err(format!("no Goal is linked to blueprint `{target}`")),
        _ => {
            return Err(format!(
                "blueprint `{target}` has multiple linked Goals; use an explicit target"
            ));
        }
    };
    let executions = ExecutionStore::open_read_only(execution_path_for_session(session))
        .map_err(|error| error.to_string())?;
    let receipts = executions
        .receipts()
        .iter()
        .filter(|receipt| receipt.goal_id == goal.id)
        .collect::<Vec<_>>();
    let latest = receipts.iter().max_by_key(|receipt| receipt.attempt);
    Ok(GoalState {
        goal_id: goal.id.clone(),
        goal_status: goal.status.as_str().to_owned(),
        terminal: matches!(
            goal.status,
            GoalStatus::Done | GoalStatus::Blocked | GoalStatus::Cancelled
        ),
        receipt_count: receipts.len(),
        attempts_used: receipts.iter().map(|receipt| receipt.attempt).max().unwrap_or(0),
        latest_receipt_status: latest.map(|receipt| receipt.status.as_str().to_owned()),
        latest_receipt_error: latest.and_then(|receipt| receipt.error.clone()),
    })
}

pub fn run_supervisor(
    session: &Path,
    workspace: &Path,
    target: &str,
    config: &WorkerConfig,
    poll: Duration,
    max_errors: usize,
) -> Result<(), String> {
    let session = absolute_path(session)?;
    let paths = DaemonPaths::for_session(&session, target)?;
    let workspace = session_cwd(&session).unwrap_or_else(|| workspace.to_path_buf());
    let _guard = acquire_pid(&paths)?;
    let mut errors = 0usize;
    let mut state = goal_state(&session, target)?;
    write_status(
        &paths,
        json!({
            "target": target,
            "session": session,
            "pid": std::process::id(),
            "state": "starting",
            "goal_id": state.goal_id,
            "goal_status": state.goal_status,
            "receipt_count": state.receipt_count,
            "attempts_used": state.attempts_used,
            "latest_receipt_status": state.latest_receipt_status,
            "updated_at_ms": now_ms(),
        }),
    )?;

    loop {
        if paths.stop.exists() {
            let _ = fs::remove_file(&paths.stop);
            state = goal_state(&session, target)?;
            write_status(
                &paths,
                status_value(target, &session, &state, "stopped", None, None),
            )?;
            return Ok(());
        }
        state = goal_state(&session, target)?;
        let last_receipts = state.receipt_count;
        if state.terminal {
            write_status(
                &paths,
                status_value(target, &session, &state, &state.goal_status, None, None),
            )?;
            return Ok(());
        }
        write_status(
            &paths,
            status_value(target, &session, &state, "running", None, None),
        )?;

        let log_start = fs::metadata(&paths.log).map(|meta| meta.len()).unwrap_or(0);
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&paths.log)
            .map_err(|error| error.to_string())?;
        crate::security::restrict_private_file(&paths.log).map_err(|error| error.to_string())?;
        let log_err = log.try_clone().map_err(|error| error.to_string())?;
        let mut command = Command::new(current_exe()?);
        command
            .arg("--goal-worker")
            .arg(target)
            .arg("--session")
            .arg(&session)
            .arg("--mode")
            .arg("headless")
            .arg("--auto");
        config.append_args(&mut command);
        command
            .current_dir(&workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err));
        let exit = command.status();
        let after = goal_state(&session, target)?;
        let progress = after.receipt_count > last_receipts;
        let locked = appended_log_contains(&paths.log, log_start, "locked");
        if after.terminal {
            write_status(
                &paths,
                status_value(
                    target,
                    &session,
                    &after,
                    &after.goal_status,
                    exit.as_ref().ok().and_then(|status| status.code()),
                    None,
                ),
            )?;
            return Ok(());
        }
        if locked {
            errors = 0;
            write_status(
                &paths,
                status_value(
                    target,
                    &session,
                    &after,
                    "waiting_for_session",
                    exit.as_ref().ok().and_then(|status| status.code()),
                    Some("session is owned by another process"),
                ),
            )?;
        } else if progress {
            errors = 0;
            write_status(
                &paths,
                status_value(
                    target,
                    &session,
                    &after,
                    "running",
                    exit.as_ref().ok().and_then(|status| status.code()),
                    None,
                ),
            )?;
        } else {
            errors = errors.saturating_add(1);
            if errors > max_errors {
                let error = exit
                    .as_ref()
                    .err()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "worker made no durable progress".to_owned());
                write_status(
                    &paths,
                    status_value(
                        target,
                        &session,
                        &after,
                        "error",
                        exit.as_ref().ok().and_then(|status| status.code()),
                        Some(&error),
                    ),
                )?;
                return Err(error);
            }
            write_status(
                &paths,
                status_value(
                    target,
                    &session,
                    &after,
                    "retrying",
                    exit.as_ref().ok().and_then(|status| status.code()),
                    Some("worker made no durable progress"),
                ),
            )?;
        }
        std::thread::sleep(poll);
    }
}

pub fn spawn_detached(
    session: &Path,
    workspace: &Path,
    target: &str,
    config: &WorkerConfig,
    poll: Duration,
    max_errors: usize,
) -> Result<Value, String> {
    let session = absolute_path(session)?;
    let paths = DaemonPaths::for_session(&session, target)?;
    if let Some(pid) = read_live_pid(&paths.pid) {
        return Err(format!("goal daemon is already running (pid {pid})"));
    }
    let _ = fs::remove_file(&paths.stop);
    let _ = fs::remove_file(&paths.status);
    let workspace = session_cwd(&session).unwrap_or_else(|| workspace.to_path_buf());
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log)
        .map_err(|error| error.to_string())?;
    crate::security::restrict_private_file(&paths.log).map_err(|error| error.to_string())?;
    let stderr = log.try_clone().map_err(|error| error.to_string())?;
    let mut command = Command::new(current_exe()?);
    command
        .arg("--goal-daemon")
        .arg(target)
        .arg("--session")
        .arg(&session)
        .arg("--auto")
        .arg("--goal-poll-ms")
        .arg(poll.as_millis().to_string())
        .arg("--goal-max-errors")
        .arg(max_errors.to_string());
    config.append_args(&mut command);
    command
        .current_dir(&workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command.spawn().map_err(|error| error.to_string())?;
    Ok(json!({
        "target": target,
        "session": session,
        "pid": child.id(),
        "log": paths.log,
        "status": paths.status,
        "state": "starting",
        "message": "goal daemon started; it waits for the current session owner to exit",
    }))
}

pub fn daemon_status(session: &Path, target: &str) -> Result<Value, String> {
    let session = absolute_path(session)?;
    let paths = DaemonPaths::for_session(&session, target)?;
    let mut value = match fs::read(&paths.status) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
            .map_err(|error| format!("goal daemon status is invalid: {error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({
            "target": target,
            "session": session,
            "state": "not_found",
        }),
        Err(error) => return Err(error.to_string()),
    };
    let alive = read_live_pid(&paths.pid).is_some();
    if let Some(object) = value.as_object_mut() {
        object.insert("pid_alive".into(), json!(alive));
        object.insert("log".into(), json!(paths.log));
        object.insert("status_path".into(), json!(paths.status));
        object.insert("stop_path".into(), json!(paths.stop));
    }
    Ok(value)
}

pub fn request_stop(session: &Path, target: &str) -> Result<Value, String> {
    let session = absolute_path(session)?;
    let paths = DaemonPaths::for_session(&session, target)?;
    if read_live_pid(&paths.pid).is_none() {
        return Ok(json!({
            "target": target,
            "session": session,
            "state": "not_running",
            "pid_alive": false,
        }));
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.stop)
        .map_err(|error| error.to_string())?;
    writeln!(file, "{}", now_ms()).map_err(|error| error.to_string())?;
    crate::security::restrict_private_file(&paths.stop).map_err(|error| error.to_string())?;
    Ok(json!({
        "target": target,
        "session": session,
        "state": "stop_requested",
        "stop_path": paths.stop,
        "message": "the supervisor stops after the current worker exits",
    }))
}

fn status_value(
    target: &str,
    session: &Path,
    state: &GoalState,
    state_name: &str,
    worker_exit: Option<i32>,
    error: Option<&str>,
) -> Value {
    json!({
        "target": target,
        "session": session,
        "pid": std::process::id(),
        "state": state_name,
        "goal_id": state.goal_id,
        "goal_status": state.goal_status,
        "receipt_count": state.receipt_count,
        "attempts_used": state.attempts_used,
        "latest_receipt_status": state.latest_receipt_status,
        "latest_receipt_error": state.latest_receipt_error,
        "worker_exit": worker_exit,
        "last_error": error,
        "updated_at_ms": now_ms(),
    })
}

fn write_status(paths: &DaemonPaths, value: Value) -> Result<(), String> {
    let temporary = paths.status.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    crate::security::restrict_private_file(&temporary).map_err(|error| error.to_string())?;
    fs::rename(&temporary, &paths.status).map_err(|error| error.to_string())
}

struct PidGuard {
    path: PathBuf,
    pid: u32,
}

impl Drop for PidGuard {
    fn drop(&mut self) {
        if let Ok(contents) = fs::read_to_string(&self.path)
            && contents.trim() == self.pid.to_string()
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn acquire_pid(paths: &DaemonPaths) -> Result<PidGuard, String> {
    if let Some(pid) = read_live_pid(&paths.pid) {
        return Err(format!("goal daemon is already running (pid {pid})"));
    }
    let _ = fs::remove_file(&paths.pid);
    let pid = std::process::id();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&paths.pid)
        .map_err(|error| error.to_string())?;
    writeln!(file, "{pid}").map_err(|error| error.to_string())?;
    crate::security::restrict_private_file(&paths.pid).map_err(|error| error.to_string())?;
    Ok(PidGuard {
        path: paths.pid.clone(),
        pid,
    })
}

fn read_live_pid(path: &Path) -> Option<u32> {
    let pid = fs::read_to_string(path).ok()?.trim().parse::<u32>().ok()?;
    process_alive(pid).then_some(pid)
}

fn process_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|error| error.to_string())
}

fn absolute_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|error| error.to_string())
}

fn session_cwd(session: &Path) -> Option<PathBuf> {
    let file = fs::File::open(session).ok()?;
    let mut line = String::new();
    BufReader::new(file).read_line(&mut line).ok()?;
    let value: Value = serde_json::from_str(&line).ok()?;
    value
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
}

fn appended_log_contains(path: &Path, offset: u64, needle: &str) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    use std::io::{Read, Seek, SeekFrom};
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return false;
    }
    let mut bytes = Vec::new();
    let _ = file.take(256 * 1024).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).contains(needle)
}

fn sanitize_target(target: &str) -> String {
    target
        .chars()
        .take(120)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_is_sanitized_into_bounded_filenames() {
        assert_eq!(sanitize_target("pilot4/unit@1"), "pilot4_unit_1");
        assert!(sanitize_target(&"x".repeat(500)).len() <= 120);
    }

    #[test]
    fn daemon_paths_share_one_session_directory() {
        let paths = DaemonPaths::for_session(Path::new("/tmp/session.jsonl"), "plan@1").unwrap();
        assert_eq!(paths.pid, PathBuf::from("/tmp/goal-daemon-plan_1.pid"));
        assert_eq!(paths.status, PathBuf::from("/tmp/goal-daemon-plan_1.json"));
        assert_eq!(paths.log, PathBuf::from("/tmp/goal-daemon-plan_1.log"));
        assert_eq!(paths.stop, PathBuf::from("/tmp/goal-daemon-plan_1.stop"));
    }

    #[test]
    fn goal_state_reads_durable_receipts_and_last_error() {
        use crate::{
            b3::ResourceBudget,
            domain_execution::{ExecutionReceipt, ExecutionStatus, deterministic_cost},
            domains::{Blueprint, BlueprintItem, Goal},
        };

        let directory = tempfile::tempdir().unwrap();
        let session = directory.path().join("session.jsonl");
        let blueprint = Blueprint::new(
            "daemon-plan",
            "1",
            vec![BlueprintItem::new("work", 2).with_task(crate::domains::BlueprintTask {
                instruction: "do the work".into(),
                acceptance_commands: vec!["true".into()],
            })],
        )
        .unwrap();
        let goal = Goal::new(
            "daemon-goal",
            &blueprint,
            ResourceBudget {
                tokens: 100,
                wall_clock_ms: 10,
                attempts: 2,
                disk_bytes: 2_000,
            },
            None,
        )
        .unwrap();
        let mut domains = DomainStore::open(domain_store::path_for_session(&session)).unwrap();
        domains.put_blueprint(blueprint.clone()).unwrap();
        domains.put_goal(goal.clone()).unwrap();
        let item = &blueprint.items[0];
        let mut execution =
            ExecutionStore::open(execution_path_for_session(&session)).unwrap();
        execution
            .upsert_receipt(ExecutionReceipt {
                execution_id: "daemon-execution".into(),
                goal_id: goal.id.clone(),
                blueprint_id: blueprint.id.clone(),
                blueprint_version: blueprint.version.clone(),
                blueprint_digest: blueprint.digest.clone(),
                item_id: item.id.clone(),
                attempt: 1,
                status: ExecutionStatus::Failed,
                cost: deterministic_cost(item),
                evidence: "local_goal_runner local_task_executed=true".into(),
                external_work_executed: false,
                manifest_checksum: None,
                error: Some("acceptance failed".into()),
            })
            .unwrap();

        let state = goal_state(&session, "daemon-plan@1").unwrap();
        assert_eq!(state.goal_id, "daemon-goal");
        assert_eq!(state.goal_status, "queued");
        assert!(!state.terminal);
        assert_eq!(state.receipt_count, 1);
        assert_eq!(state.attempts_used, 1);
        assert_eq!(state.latest_receipt_status.as_deref(), Some("failed"));
        assert_eq!(state.latest_receipt_error.as_deref(), Some("acceptance failed"));
    }
}
