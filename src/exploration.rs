//! Exploration batches for long-running Lean goals.
//!
//! A plan holds ordered batches. Every batch contains goal ids that are already
//! durable in the session domain store. The supervisor runs one worker attempt
//! at a time. When a batch has no runnable goal, the next batch runs. When all
//! batches are exhausted it creates a fresh replan batch from the last failed
//! blueprint and continues. It does not stop merely because a batch failed.

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    b3::ResourceBudget,
    domain_store::{self, DomainStore},
    domains::{Blueprint, Goal, GoalStatus},
    goal_daemon::{self, WorkerConfig},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationPlan {
    pub id: String,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub batches: Vec<ExplorationBatch>,
    #[serde(default = "default_stalls")]
    pub max_stalls_per_batch: u32,
    #[serde(default = "default_rounds")]
    pub max_rounds: u32,
    #[serde(default = "default_wall")]
    pub max_wall_ms: u64,
    #[serde(default)]
    pub auto_replan: bool,
    #[serde(default)]
    pub strategies: Vec<String>,
    #[serde(default = "default_budget")]
    pub direction_budget: ResourceBudget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationBatch {
    pub name: String,
    #[serde(default)]
    pub goal_ids: Vec<String>,
}

fn default_stalls() -> u32 {
    2
}

fn default_rounds() -> u32 {
    8
}

fn default_wall() -> u64 {
    6 * 60 * 60 * 1_000
}

fn default_budget() -> ResourceBudget {
    ResourceBudget {
        tokens: 500_000,
        wall_clock_ms: 600_000,
        attempts: 2,
        disk_bytes: 10_000_000,
    }
}

pub fn read_plan(path: &Path) -> Result<ExplorationPlan, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let plan: ExplorationPlan =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid exploration plan: {error}"))?;
    validate_plan(&plan)?;
    Ok(plan)
}

fn validate_plan(plan: &ExplorationPlan) -> Result<(), String> {
    if plan.id.trim().is_empty()
        || plan.id.len() > crate::domains::MAX_ID_BYTES
        || plan.id.chars().any(char::is_control)
    {
        return Err("exploration plan id is not a bounded identifier".into());
    }
    if plan.batches.len() > 256 {
        return Err("exploration plan has too many batches".into());
    }
    for batch in &plan.batches {
        if batch.goal_ids.len() > 256 {
            return Err("exploration batch has too many goals".into());
        }
        for goal_id in &batch.goal_ids {
            if goal_id.trim().is_empty()
                || goal_id.len() > crate::domains::MAX_ID_BYTES
                || goal_id.chars().any(char::is_control)
            {
                return Err("exploration batch goal id is invalid".into());
            }
        }
    }
    if plan.direction_budget.attempts == 0 {
        return Err("exploration direction budget must allow at least one attempt".into());
    }
    Ok(())
}

pub fn write_plan(path: &Path, plan: &ExplorationPlan) -> Result<(), String> {
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(plan).map_err(|error| error.to_string())?;
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

pub fn run_exploration(
    session: &Path,
    workspace: &Path,
    plan_path: &Path,
    config: &WorkerConfig,
    poll: Duration,
    max_errors: usize,
) -> Result<(), String> {
    let session = goal_daemon::absolute_path(session)?;
    let plan_path = goal_daemon::absolute_path(plan_path)?;
    let mut plan = read_plan(&plan_path)?;
    if let Some(expected) = plan.session.as_deref()
        && goal_daemon::absolute_path(Path::new(expected))? != session
    {
        return Err("exploration plan belongs to a different session".into());
    }
    let workspace = goal_daemon::session_cwd(&session).unwrap_or_else(|| workspace.to_path_buf());
    let paths = goal_daemon::DaemonPaths::for_session(&session, &format!("explore-{}", plan.id))?;
    let _guard = goal_daemon::acquire_pid(&paths)?;
    let _ = fs::remove_file(&paths.stop);
    let started = Instant::now();
    let mut round = 0u32;
    let mut history: Vec<Value> = Vec::new();
    let mut errors: BTreeMap<String, usize> = BTreeMap::new();
    let mut last_failed: Option<String> = None;
    let mut last_batch: Option<String> = None;

    loop {
        if paths.stop.exists() {
            let _ = fs::remove_file(&paths.stop);
            write_status(
                &paths,
                &session,
                &plan,
                "stopped",
                round,
                last_batch.as_deref(),
                &history,
                None,
            )?;
            return Ok(());
        }
        if plan.max_wall_ms > 0 && started.elapsed() >= Duration::from_millis(plan.max_wall_ms) {
            write_status(
                &paths,
                &session,
                &plan,
                "budget_exhausted",
                round,
                last_batch.as_deref(),
                &history,
                Some("wall budget exhausted"),
            )?;
            return Ok(());
        }

        plan = read_plan(&plan_path)?;
        let mut pending = None;
        let mut achieved = false;
        for batch in &plan.batches {
            for goal_id in &batch.goal_ids {
                let state = goal_daemon::goal_state_by_id(&session, goal_id)?;
                if state.goal_status == "done" {
                    achieved = true;
                    break;
                }
                if !state.terminal {
                    pending = Some(goal_id.clone());
                    last_batch = Some(batch.name.clone());
                    break;
                }
                last_failed = Some(goal_id.clone());
            }
            if achieved || pending.is_some() {
                break;
            }
        }
        if achieved {
            write_status(
                &paths,
                &session,
                &plan,
                "done",
                round,
                last_batch.as_deref(),
                &history,
                None,
            )?;
            return Ok(());
        }

        if let Some(goal_id) = pending {
            let before = goal_daemon::goal_state_by_id(&session, &goal_id)?;
            write_status(
                &paths,
                &session,
                &plan,
                "running",
                round,
                last_batch.as_deref(),
                &history,
                None,
            )?;
            let exit = run_worker(&session, &workspace, &paths.log, &goal_id, config);
            let after = goal_daemon::goal_state_by_id(&session, &goal_id)?;
            let progress = after.receipt_count > before.receipt_count;
            let exit_code = exit.as_ref().ok().and_then(|status| status.code());
            history.push(json!({
                "round": round,
                "batch": last_batch,
                "goal_id": goal_id,
                "goal_status": after.goal_status,
                "receipt_count": after.receipt_count,
                "worker_exit": exit_code,
                "error": after.latest_receipt_error,
                "at_ms": goal_daemon::now_ms(),
            }));
            if history.len() > 64 {
                history.remove(0);
            }
            if after.terminal {
                last_failed = Some(goal_id.clone());
                errors.remove(&goal_id);
                continue;
            }
            if progress {
                errors.remove(&goal_id);
            } else {
                let count = errors.entry(goal_id.clone()).or_insert(0);
                *count += 1;
                if *count > max_errors {
                    block_goal(&session, &goal_id)?;
                    last_failed = Some(goal_id.clone());
                    errors.remove(&goal_id);
                }
            }
            std::thread::sleep(poll);
            continue;
        }

        // Every batch is terminal. Replan instead of stopping, unless the
        // plan disables replanning or its round budget is exhausted.
        if plan.auto_replan && round < plan.max_rounds {
            round += 1;
            let source = last_failed
                .clone()
                .or_else(|| {
                    plan.batches
                        .last()
                        .and_then(|batch| batch.goal_ids.last().cloned())
                })
                .ok_or_else(|| "exploration plan has no goals to replan from".to_owned())?;
            write_status(
                &paths,
                &session,
                &plan,
                "replanning",
                round,
                last_batch.as_deref(),
                &history,
                None,
            )?;
            make_replan_batch(&session, &plan_path, &plan, round, &source)?;
            continue;
        }

        write_status(
            &paths,
            &session,
            &plan,
            "awaiting_batch",
            round,
            last_batch.as_deref(),
            &history,
            Some("no runnable batch; waiting for a new batch, stop, or wall budget"),
        )?;
        std::thread::sleep(poll);
    }
}

fn run_worker(
    session: &Path,
    workspace: &Path,
    log_path: &Path,
    goal_id: &str,
    config: &WorkerConfig,
) -> Result<std::process::ExitStatus, String> {
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|error| error.to_string())?;
    crate::security::restrict_private_file(log_path).map_err(|error| error.to_string())?;
    let log_err = log.try_clone().map_err(|error| error.to_string())?;
    let mut command = Command::new(goal_daemon::current_exe()?);
    command
        .arg("--goal-worker-id")
        .arg(goal_id)
        .arg("--session")
        .arg(session)
        .arg("--mode")
        .arg("headless")
        .arg("--auto");
    config.append_args(&mut command);
    command
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .status()
        .map_err(|error| error.to_string())
}

fn block_goal(session: &Path, goal_id: &str) -> Result<(), String> {
    let mut store = DomainStore::open(domain_store::path_for_session(session))
        .map_err(|error| error.to_string())?;
    store
        .transition_goal(goal_id, GoalStatus::Blocked)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn make_replan_batch(
    session: &Path,
    plan_path: &Path,
    plan: &ExplorationPlan,
    round: u32,
    source_goal_id: &str,
) -> Result<(), String> {
    let mut store = DomainStore::open(domain_store::path_for_session(session))
        .map_err(|error| error.to_string())?;
    let goal = store
        .goal(source_goal_id)
        .cloned()
        .ok_or_else(|| format!("source goal `{source_goal_id}` is not found"))?;
    let blueprint = store
        .blueprint(&goal.blueprint_id, &goal.blueprint_version)
        .cloned()
        .ok_or_else(|| format!("source blueprint `{}` is not found", goal.blueprint_id))?;
    let strategy = plan
        .strategies
        .get((round.saturating_sub(1) as usize) % plan.strategies.len().max(1))
        .cloned()
        .unwrap_or_else(|| "split the task into smaller independently checkable lemmas".into());
    let previous = goal_daemon::goal_state_by_id(session, source_goal_id)?
        .latest_receipt_error
        .unwrap_or_else(|| "the previous direction did not pass acceptance".into());
    let previous = previous.chars().take(1_000).collect::<String>();
    let mut items = blueprint.items.clone();
    for item in &mut items {
        if let Some(task) = &mut item.task {
            task.instruction = format!(
                "Replan round {round}. Strategy: {strategy}. The previous direction failed: {previous}. Do not repeat the same proof step. {}",
                task.instruction
            );
        }
    }
    let new_blueprint_id = format!("{}-round-{round}", plan.id);
    let new_goal_id = format!("{new_blueprint_id}-goal");
    if store.blueprint(&new_blueprint_id, "1").is_none() {
        let blueprint = Blueprint::new(new_blueprint_id.clone(), "1", items)
            .map_err(|error| error.to_string())?;
        store
            .put_blueprint(blueprint)
            .map_err(|error| error.to_string())?;
    }
    let blueprint = store
        .blueprint(&new_blueprint_id, "1")
        .cloned()
        .ok_or_else(|| "replan blueprint disappeared".to_owned())?;
    if store.goal(&new_goal_id).is_none() {
        let goal = Goal::new(
            new_goal_id.clone(),
            &blueprint,
            plan.direction_budget,
            None,
        )
        .map_err(|error| error.to_string())?;
        store.put_goal(goal).map_err(|error| error.to_string())?;
    }
    let mut updated = plan.clone();
    updated.batches.push(ExplorationBatch {
        name: format!("replan-{round}"),
        goal_ids: vec![new_goal_id],
    });
    write_plan(plan_path, &updated)?;
    Ok(())
}

fn write_status(
    paths: &goal_daemon::DaemonPaths,
    session: &Path,
    plan: &ExplorationPlan,
    state: &str,
    round: u32,
    batch: Option<&str>,
    history: &[Value],
    error: Option<&str>,
) -> Result<(), String> {
    goal_daemon::write_status(
        paths,
        json!({
            "plan": plan.id,
            "session": session,
            "pid": std::process::id(),
            "state": state,
            "round": round,
            "batch": batch,
            "batches": plan.batches.len(),
            "auto_replan": plan.auto_replan,
            "last_error": error,
            "history": history,
            "updated_at_ms": goal_daemon::now_ms(),
        }),
    )
}

pub fn spawn_detached(
    session: &Path,
    workspace: &Path,
    plan_path: &Path,
    config: &WorkerConfig,
    poll: Duration,
    max_errors: usize,
) -> Result<Value, String> {
    let session = goal_daemon::absolute_path(session)?;
    let plan_path = goal_daemon::absolute_path(plan_path)?;
    let plan = read_plan(&plan_path)?;
    if let Some(expected) = plan.session.as_deref()
        && goal_daemon::absolute_path(Path::new(expected))? != session
    {
        return Err("exploration plan belongs to a different session".into());
    }
    let paths = goal_daemon::DaemonPaths::for_session(&session, &format!("explore-{}", plan.id))?;
    if let Some(pid) = goal_daemon::read_live_pid(&paths.pid) {
        return Err(format!("exploration daemon is already running (pid {pid})"));
    }
    let _ = fs::remove_file(&paths.stop);
    let _ = fs::remove_file(&paths.status);
    let workspace = goal_daemon::session_cwd(&session).unwrap_or_else(|| workspace.to_path_buf());
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log)
        .map_err(|error| error.to_string())?;
    crate::security::restrict_private_file(&paths.log).map_err(|error| error.to_string())?;
    let stderr = log.try_clone().map_err(|error| error.to_string())?;
    let mut command = Command::new(goal_daemon::current_exe()?);
    command
        .arg("--goal-explore")
        .arg(&plan_path)
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
        "plan": plan.id,
        "session": session,
        "pid": child.id(),
        "log": paths.log,
        "status": paths.status,
        "state": "starting",
        "message": "exploration daemon started; failed batches rotate into replan batches",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_round_trips_and_requires_attempts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plan.json");
        let plan = ExplorationPlan {
            id: "explore".into(),
            session: None,
            batches: vec![ExplorationBatch {
                name: "one".into(),
                goal_ids: vec!["goal-a".into()],
            }],
            max_stalls_per_batch: 2,
            max_rounds: 2,
            max_wall_ms: 1_000,
            auto_replan: true,
            strategies: vec!["switch".into()],
            direction_budget: default_budget(),
        };
        write_plan(&path, &plan).unwrap();
        assert_eq!(read_plan(&path).unwrap().batches[0].goal_ids[0], "goal-a");
        let mut invalid = plan.clone();
        invalid.direction_budget.attempts = 0;
        let bytes = serde_json::to_vec(&invalid).unwrap();
        fs::write(&path, bytes).unwrap();
        assert!(read_plan(&path).is_err());
    }
}
