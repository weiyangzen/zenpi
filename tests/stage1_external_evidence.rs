#![cfg(unix)]
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};
use tempfile::{TempDir, tempdir};
use zenpi::{
    core::Agent, domain_execution::*, domains::*, governance::*, session::SessionStore, tools::*,
};
struct Fixture {
    root: TempDir,
    workspace: PathBuf,
    agent: Agent,
    store: ExecutionStore,
    handoff: BlueprintHandoff,
    goal: Goal,
    blueprint: Blueprint,
    request: ExternalPreparation,
}
impl Fixture {
    fn new(command: &str) -> Self {
        Self::with_ttl(command, 120000)
    }
    fn with_ttl(command: &str, ttl: u64) -> Self {
        let root = tempdir().unwrap();
        let workspace = root.path().join("work");
        let control = root.path().join("control");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&control).unwrap();
        fs::write(workspace.join("file.txt"), "before\n").unwrap();
        fs::write(workspace.join("unowned.txt"), "keep\n").unwrap();
        let session_path = control.join("session.jsonl");
        let mut session = SessionStore::open_in_workspace(&session_path, &workspace).unwrap();
        let blueprint = Blueprint::new(
            "evidence-plan",
            "1",
            vec![BlueprintItem::new("edit", 10).with_task(
                BlueprintTask::new("Change file.txt to after", vec![command.into()]).unwrap(),
            )],
        )
        .unwrap();
        let goal = Goal::new(
            "evidence-goal",
            &blueprint,
            zenpi::b3::ResourceBudget {
                tokens: 1000,
                wall_clock_ms: 1000,
                attempts: 10,
                disk_bytes: 100000,
            },
            None,
        )
        .unwrap();
        let mut domains = zenpi::domain_store::DomainStore::open(
            zenpi::domain_store::path_for_session(&session_path),
        )
        .unwrap();
        domains.put_blueprint(blueprint.clone()).unwrap();
        domains.put_goal(goal.clone()).unwrap();
        let mut executor =
            BlueprintExecutor::new(ExecutionStore::open(path_for_session(&session_path)).unwrap());
        let mut handoffs = HandoffStore::open(handoff_path_for_session(&session_path)).unwrap();
        let handoff = executor
            .admit_external_handoff(&mut handoffs, &goal, &blueprint)
            .unwrap()
            .request;
        let now = zenpi::session::unix_time_ms();
        let lease = BlueprintLease {
            lease_id: "existing-lease".into(),
            issued_at_ms: now,
            expires_at_ms: now + ttl,
        };
        let policy = BlueprintPolicySpec {
            blueprint_digest: blueprint.digest.clone(),
            goal_digest: "b".repeat(64),
            item_id: "edit".into(),
            allowed_tools: ["write_file".into(), "read_file".into()].into(),
            readable_paths: [".".into()].into(),
            writable_paths: [
                "file.txt".into(),
                format!(".zenpi-results/{}.json", handoff.claim_digest),
            ]
            .into(),
            denied_paths: Default::default(),
            protected_paths: Default::default(),
            allowed_commands: Default::default(),
            denied_commands: Default::default(),
            network_hosts: Default::default(),
            max_actions: 16,
            max_command_timeout_ms: 6000,
            max_command_output_bytes: 8192,
        };
        let context = ToolContext::new(&workspace).unwrap();
        let (gate, _) =
            BlueprintGate::compile(&context, policy.clone(), lease.clone(), now).unwrap();
        let digest = gate.evidence().policy_digest;
        let limits = ResourceLimits::default();
        let mut ledger = WorkerBudgetLedger::restore(&mut session, limits).unwrap();
        ledger
            .open_lease(
                &mut session,
                WorkerLease {
                    lease_id: lease.lease_id.clone(),
                    blueprint_item: "edit".into(),
                    policy_digest: digest.clone(),
                    expires_at_ms: lease.expires_at_ms,
                    limits,
                },
                now,
            )
            .unwrap();
        let request = ExternalPreparation {
            claim_digest: handoff.claim_digest.clone(),
            lease_id: lease.lease_id.clone(),
            policy_digest: digest,
            owned_paths: vec!["file.txt".into()],
            validator_timeout_ms: 6000,
            policy,
            lease,
        };
        fs::write(
            workspace.join("preparation.json"),
            serde_json::to_vec(&request).unwrap(),
        )
        .unwrap();
        let mut agent = Agent::with_echo(session);
        agent.set_tools(
            ToolRegistry::with_all_builtins().unwrap(),
            context,
            SideEffectPolicy::all_builtins(),
        );
        Self {
            root,
            workspace,
            agent,
            store: executor.into_store(),
            handoff,
            goal,
            blueprint,
            request,
        }
    }
    fn prepare(&mut self) -> ExternalContract {
        self.agent
            .external_evidence_control(
                &mut self.store,
                &self.handoff,
                &self.goal,
                &self.blueprint,
                Some(&self.request),
                &|| false,
            )
            .unwrap()
            .contract
            .unwrap()
    }
    fn worker(&self) {
        assert!(
            Command::new("/bin/sh")
                .args(["-c", "printf 'after\n' > file.txt"])
                .current_dir(&self.workspace)
                .status()
                .unwrap()
                .success()
        );
    }
    fn manifest(&self, c: &ExternalContract) -> VerifiableResultManifest {
        let after =
            inspect_external_worktree(&self.workspace, &c.protected_paths, &|| false).unwrap();
        let changes = external_file_diff(&c.baseline, &after);
        use sha2::{Digest, Sha256};
        let diff_sha256 = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&changes).unwrap())
        );
        VerifiableResultManifest {
            schema_version: 2,
            declaration: ExternalResultManifest {
                execution_id: c.execution_id.clone(),
                claim_digest: c.claim_digest.clone(),
                blueprint_digest: self.blueprint.digest.clone(),
                item_id: "edit".into(),
                status: ExecutionStatus::Succeeded,
                acceptance_passed: true,
                acceptance_evidence: vec!["worker declaration only".into()],
            },
            contract_digest: c.digest.clone(),
            lease_id: c.lease_id.clone(),
            policy_digest: c.policy_digest.clone(),
            baseline_revision: c.baseline.revision.clone(),
            output_revision: after.revision,
            diff_sha256,
            changes,
            validator_ids: c.validators.iter().map(|v| v.id.clone()).collect(),
        }
    }
    fn write_manifest(&self, m: &VerifiableResultManifest) -> PathBuf {
        let path = self
            .workspace
            .join(".zenpi-results")
            .join(format!("{}.json", self.handoff.claim_digest));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, serde_json::to_vec(m).unwrap()).unwrap();
        path
    }
    fn import(
        &mut self,
        m: &VerifiableResultManifest,
    ) -> Result<ExecutionStoreChange, ExecutionError> {
        let path = self.write_manifest(m);
        self.store
            .import_external_manifest(path, &self.blueprint.digest)
    }
    fn accept(
        &mut self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ExternalEvidence, zenpi::core::AgentError> {
        self.agent.external_evidence_control(
            &mut self.store,
            &self.handoff,
            &self.goal,
            &self.blueprint,
            None,
            cancelled,
        )
    }
}
const VALIDATOR: &str = "/usr/bin/grep -qx after file.txt && printf host-verified";

#[test]
fn integrated_command_parser_and_async_classifier_agree_for_aliases_and_quotes() {
    use zenpi::slash::{BlueprintAction, SlashCommand};
    for command in ["/blueprint", "/BLUEPRINT", "/bp", "/BP", "/\"blueprint\""] {
        for action in ["accept", "ACCEPT", "\"prepare\""] {
            let text = format!("{command} {action} evidence-plan@1 {}", "a".repeat(64));
            assert!(
                matches!(
                    zenpi::slash::parse(&text),
                    Ok(Some(SlashCommand::Blueprint {
                        action: BlueprintAction::Accept { .. } | BlueprintAction::Prepare { .. }
                    }))
                ),
                "parser rejected {text}"
            );
            assert!(
                zenpi::slash::external_evidence_control(&text),
                "async classifier missed {text}"
            );
        }
    }
    for text in [
        "/blueprint accept",
        "/blueprint import plan@1 file.json",
        "/bp list",
        "blueprint accept plan@1 claim",
    ] {
        assert!(
            !zenpi::slash::external_evidence_control(text),
            "unexpected control {text}"
        );
    }
}

#[test]
fn integrated_accepted_replay_rejects_another_session_owner() {
    let mut f = Fixture::new(VALIDATOR);
    let c = f.prepare();
    f.worker();
    let m = f.manifest(&c);
    f.import(&m).unwrap();
    let accepted = f.accept(&|| false).unwrap();
    assert_eq!(accepted.status, EvidenceStatus::Accepted);
    let before = fs::read(f.store.path()).unwrap();
    let mut other =
        SessionStore::open_in_workspace(f.root.path().join("other.jsonl"), &f.workspace).unwrap();
    let mut ledger = WorkerBudgetLedger::restore(&mut other, ResourceLimits::default()).unwrap();
    let sequence = other.next_sequence();
    let result = f.store.accept_external_candidate(
        &mut other,
        &mut ledger,
        &mut zenpi::tool_output::SessionOutputStore::default(),
        &f.handoff,
        &f.goal,
        &f.blueprint,
        &f.workspace,
        &|| false,
    );
    assert!(result.is_err(), "accepted another session: {result:?}");
    assert_eq!(other.next_sequence(), sequence);
    assert_eq!(fs::read(f.store.path()).unwrap(), before);
    assert_eq!(f.accept(&|| false).unwrap(), accepted);
}

#[test]
fn integrated_accepted_replay_rejects_another_workspace() {
    let mut f = Fixture::new(VALIDATOR);
    let c = f.prepare();
    f.worker();
    let m = f.manifest(&c);
    f.import(&m).unwrap();
    let accepted = f.accept(&|| false).unwrap();
    let before = fs::read(f.store.path()).unwrap();
    let other = f.root.path().join("other-workspace");
    fs::create_dir(&other).unwrap();
    let mut ledger = WorkerBudgetLedger::restore_existing(f.agent.session_mut()).unwrap();
    let sequence = f.agent.session().next_sequence();
    let result = f.store.accept_external_candidate(
        f.agent.session_mut(),
        &mut ledger,
        &mut zenpi::tool_output::SessionOutputStore::default(),
        &f.handoff,
        &f.goal,
        &f.blueprint,
        &other,
        &|| false,
    );
    assert!(result.is_err(), "accepted another workspace: {result:?}");
    assert_eq!(f.agent.session().next_sequence(), sequence);
    assert_eq!(fs::read(f.store.path()).unwrap(), before);
    assert_eq!(f.accept(&|| false).unwrap(), accepted);
}

#[test]
fn actual_worker_delta_candidate_host_validator_acceptance_and_restart() {
    let mut f = Fixture::new(VALIDATOR);
    let contract = f.prepare();
    f.worker();
    let m = f.manifest(&contract);
    f.import(&m).unwrap();
    assert_eq!(
        f.store
            .external_evidence(&contract.execution_id)
            .unwrap()
            .status,
        EvidenceStatus::Candidate
    );
    assert!(!f.store.is_master_accepted(&f.store.receipts()[0]));
    assert_eq!(f.import(&m).unwrap(), ExecutionStoreChange::Unchanged);
    let accepted = f.accept(&|| false).unwrap();
    assert_eq!(accepted.status, EvidenceStatus::Accepted);
    assert_eq!(accepted.observations.len(), 1);
    let observed = &accepted.observations[0];
    assert_eq!(observed.argv, vec!["/bin/sh", "-c", VALIDATOR]);
    assert_eq!(observed.exit_code, Some(0));
    assert!(observed.child_reaped && observed.output.iter().all(|r| r.complete));
    assert!(f.store.is_master_accepted(&f.store.receipts()[0]));
    let before = fs::read(f.store.path()).unwrap();
    assert_eq!(f.accept(&|| false).unwrap(), accepted);
    assert_eq!(fs::read(f.store.path()).unwrap(), before);
    let session_path = f.agent.session().path().to_owned();
    let execution_path = f.store.path().to_owned();
    f.agent.try_close().unwrap();
    drop(f.agent);
    let session = SessionStore::open_existing_writable(session_path).unwrap();
    let mut agent = Agent::with_echo(session);
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(&f.workspace).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    let mut store = ExecutionStore::open(execution_path).unwrap();
    assert_eq!(
        agent
            .external_evidence_control(&mut store, &f.handoff, &f.goal, &f.blueprint, None, &|| {
                false
            })
            .unwrap(),
        accepted
    );
    assert_eq!(
        agent
            .session()
            .events()
            .iter()
            .filter(|e| e["type"] == "external_validator_observed")
            .count(),
        1
    );
}
#[test]
fn wrong_hash_revision_path_baseline_policy_validator_and_claim_are_not_accepted() {
    for kind in [
        "hash",
        "revision",
        "path",
        "baseline",
        "policy",
        "missing-validator",
        "claim",
    ] {
        let mut f = Fixture::new(VALIDATOR);
        let c = f.prepare();
        f.worker();
        let mut m = f.manifest(&c);
        match kind {
            "hash" => m.changes[0].after.as_mut().unwrap().sha256 = "a".repeat(64),
            "revision" => m.output_revision = "a".repeat(64),
            "path" => m.changes[0].path = "../escape".into(),
            "baseline" => m.baseline_revision = "a".repeat(64),
            "policy" => m.policy_digest = "a".repeat(64),
            "missing-validator" => m.validator_ids.clear(),
            "claim" => m.declaration.claim_digest = "a".repeat(64),
            _ => unreachable!(),
        }
        if kind == "hash" {
            use sha2::{Digest, Sha256};
            m.diff_sha256 = format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&m.changes).unwrap())
            );
        }
        let imported = f.import(&m);
        if imported.is_ok() {
            assert!(f.accept(&|| false).is_err(), "{kind}");
        }
        assert!(
            !f.store.is_master_accepted(&f.store.receipts()[0]),
            "{kind}"
        );
        assert!(
            !f.agent
                .session()
                .events()
                .iter()
                .any(|e| e["type"] == "external_validator_observed")
        );
    }
}
#[test]
fn forged_exit_and_commands_are_not_manifest_authority() {
    let mut f = Fixture::new(VALIDATOR);
    let c = f.prepare();
    f.worker();
    let m = f.manifest(&c);
    let path = f.write_manifest(&m);
    for key in ["exit_code", "argv", "host_observations", "accepted"] {
        let mut value = serde_json::to_value(&m).unwrap();
        value[key] = json!(0);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            f.store
                .import_external_manifest(&path, &f.blueprint.digest)
                .is_err()
        );
    }
    assert!(!f.store.is_master_accepted(&f.store.receipts()[0]));
}
#[test]
fn actual_unowned_change_and_nonzero_validator_cannot_promote_candidate() {
    let mut f = Fixture::new(VALIDATOR);
    let c = f.prepare();
    f.worker();
    fs::write(f.workspace.join("unowned.txt"), "forbidden\n").unwrap();
    let m = f.manifest(&c);
    assert!(f.import(&m).is_err());
    let mut f = Fixture::new("printf host-failure >&2; exit 7");
    let c = f.prepare();
    f.worker();
    let m = f.manifest(&c);
    f.import(&m).unwrap();
    let rejected = f.accept(&|| false).unwrap();
    assert_eq!(rejected.status, EvidenceStatus::Rejected);
    assert_eq!(rejected.observations[0].exit_code, Some(7));
    assert!(!f.store.is_master_accepted(&f.store.receipts()[0]));
}
#[test]
fn revoked_lease_duplicate_claim_and_control_plane_declarations_are_not_completion() {
    let mut f = Fixture::new(VALIDATOR);
    let c = f.prepare();
    f.worker();
    let m = f.manifest(&c);
    f.import(&m).unwrap();
    f.agent
        .cancel_blueprint_worker(&c.lease_id, "stale-lease", zenpi::session::unix_time_ms())
        .unwrap();
    assert!(f.accept(&|| false).is_err());
    let mut changed = m.clone();
    changed.declaration.acceptance_evidence = vec!["new payload".into()];
    assert!(f.import(&changed).is_err());
    let mut f = Fixture::new(VALIDATOR);
    let c = f.prepare();
    let declaration = ExternalResultManifest {
        execution_id: c.execution_id.clone(),
        claim_digest: c.claim_digest.clone(),
        blueprint_digest: f.blueprint.digest.clone(),
        item_id: "edit".into(),
        status: ExecutionStatus::Succeeded,
        acceptance_passed: true,
        acceptance_evidence: vec!["control_plane_only: passed".into()],
    };
    let path = f.workspace.join("legacy.json");
    fs::write(&path, serde_json::to_vec(&declaration).unwrap()).unwrap();
    f.store
        .import_external_manifest(path, &f.blueprint.digest)
        .unwrap();
    assert!(f.accept(&|| false).is_err());
    assert!(!f.store.is_master_accepted(&f.store.receipts()[0]));
    assert!(
        f.store
            .attach_external_manifest(&c.execution_id, "a".repeat(64))
            .is_err()
    );
}
#[test]
fn actual_validator_cancel_reaps_and_restart_cannot_accept_or_replay() {
    let mut f = Fixture::new("printf started; sleep 10");
    let c = f.prepare();
    f.worker();
    let m = f.manifest(&c);
    f.import(&m).unwrap();
    let started = Instant::now();
    let cancelled = f
        .accept(&|| started.elapsed() > Duration::from_millis(300))
        .unwrap();
    assert_eq!(cancelled.status, EvidenceStatus::Cancelled);
    assert!(cancelled.observations[0].cancelled && cancelled.observations[0].child_reaped);
    assert!(!f.store.is_master_accepted(&f.store.receipts()[0]));
    let reopened = ExecutionStore::open(f.store.path()).unwrap();
    assert_eq!(
        reopened.external_evidence(&c.execution_id).unwrap(),
        &cancelled
    );
    assert!(f.accept(&|| false).is_err());
    assert_eq!(
        f.agent
            .session()
            .events()
            .iter()
            .filter(|e| e["type"] == "external_validator_observed")
            .count(),
        1
    );
}
#[test]
fn symlink_and_hardlink_products_are_rejected_without_reading_outside() {
    let mut f = Fixture::new(VALIDATOR);
    let c = f.prepare();
    let outside = f.root.path().join("outside");
    fs::write(&outside, "after\n").unwrap();
    fs::remove_file(f.workspace.join("file.txt")).unwrap();
    std::os::unix::fs::symlink(&outside, f.workspace.join("file.txt")).unwrap();
    assert!(inspect_external_worktree(&f.workspace, &c.protected_paths, &|| false).is_err());
    fs::remove_file(f.workspace.join("file.txt")).unwrap();
    fs::hard_link(&outside, f.workspace.join("file.txt")).unwrap();
    assert!(inspect_external_worktree(&f.workspace, &c.protected_paths, &|| false).is_err());
}

#[test]
#[ignore = "independent process fixture"]
fn external_evidence_process_helper() {
    let root = PathBuf::from(std::env::var("ZENPI_EVIDENCE_TEST_ROOT").unwrap());
    let workspace = root.join("work");
    let session_path = root.join("control/session.jsonl");
    let session = SessionStore::open_existing_writable(&session_path).unwrap();
    let mut agent = Agent::with_echo(session);
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(&workspace).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    let domains = zenpi::domain_store::DomainStore::open_read_only(
        zenpi::domain_store::path_for_session(&session_path),
    )
    .unwrap();
    let goal = domains.goal("evidence-goal").unwrap();
    let blueprint = domains.blueprint("evidence-plan", "1").unwrap();
    let handoffs = HandoffStore::open_read_only(handoff_path_for_session(&session_path)).unwrap();
    let handoff = &handoffs.requests()[0];
    let mut store = ExecutionStore::open(path_for_session(&session_path)).unwrap();
    let crash = std::env::var("ZENPI_EVIDENCE_TEST_CRASH").is_ok();
    let result =
        agent.external_evidence_control(&mut store, handoff, goal, blueprint, None, &|| {
            if crash && root.join("validator-marker").exists() {
                std::process::exit(17);
            }
            false
        });
    if crash {
        panic!("fixture did not stop at the actual validator boundary: {result:?}");
    }
    assert_eq!(result.unwrap().status, EvidenceStatus::Accepted);
    assert_eq!(
        agent
            .session()
            .events()
            .iter()
            .filter(|e| e["type"] == "external_validator_observed")
            .count(),
        1
    );
}
#[test]
fn independent_restart_acceptance_does_not_reexecute_validators() {
    let mut f = Fixture::new(VALIDATOR);
    let c = f.prepare();
    f.worker();
    let m = f.manifest(&c);
    f.import(&m).unwrap();
    assert_eq!(
        f.accept(&|| false).unwrap().status,
        EvidenceStatus::Accepted
    );
    f.agent.try_close().unwrap();
    drop(f.agent);
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "external_evidence_process_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("ZENPI_EVIDENCE_TEST_ROOT", f.root.path())
        .env_remove("ZENPI_EVIDENCE_TEST_CRASH")
        .status()
        .unwrap();
    assert!(status.success());
}
#[test]
fn abrupt_host_exit_leaves_unknown_validation_for_reconcile_without_worker_replay() {
    let mut f = Fixture::new("touch ../validator-marker; printf validator-finished");
    let c = f.prepare();
    f.worker();
    let m = f.manifest(&c);
    f.import(&m).unwrap();
    f.agent.try_close().unwrap();
    let session_path = f.agent.session().path().to_owned();
    drop(f.agent);
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "external_evidence_process_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("ZENPI_EVIDENCE_TEST_ROOT", f.root.path())
        .env("ZENPI_EVIDENCE_TEST_CRASH", "1")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(17));
    assert!(f.root.path().join("validator-marker").exists());
    let mut store = ExecutionStore::open(f.store.path()).unwrap();
    assert_eq!(
        store.external_evidence(&c.execution_id).unwrap().status,
        EvidenceStatus::Validating
    );
    assert!(!store.is_master_accepted(&store.receipts()[0]));
    let session = SessionStore::open_existing_writable(&session_path).unwrap();
    let mut agent = Agent::with_echo(session);
    agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(&f.workspace).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    assert!(
        agent
            .external_evidence_control(&mut store, &f.handoff, &f.goal, &f.blueprint, None, &|| {
                false
            })
            .is_err()
    );
    assert!(!agent.operation_recovery().is_empty());
    assert_eq!(
        agent
            .session()
            .events()
            .iter()
            .filter(|e| e["type"] == "tool_execution_started")
            .count(),
        1
    );
}
#[test]
fn borrowed_headless_and_tui_share_candidate_and_host_acceptance() {
    use std::io::Cursor;
    let mut f = Fixture::new(VALIDATOR);
    let input=json!({"schema_version":2,"type":"command","id":"prepare","text":"/blueprint prepare evidence-plan@1 preparation.json"}).to_string()+"\n";
    let mut output = Vec::new();
    zenpi::headless::run_headless(&mut f.agent, Cursor::new(input), &mut output).unwrap();
    let records: Vec<serde_json::Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let prepared = records
        .iter()
        .find(|r| r["id"] == "prepare" && r["type"] == "response")
        .unwrap();
    assert_eq!(prepared["success"], true, "{prepared}");
    assert_eq!(prepared["data"]["master_accepted"], false);
    let session_path = f.agent.session().path().to_owned();
    drop(f.agent);
    f.agent = Agent::with_echo(SessionStore::open_existing_writable(&session_path).unwrap());
    f.agent.set_tools(
        ToolRegistry::with_all_builtins().unwrap(),
        ToolContext::new(&f.workspace).unwrap(),
        SideEffectPolicy::all_builtins(),
    );
    f.store.reload().unwrap();
    let c = f
        .store
        .external_evidence(&f.store.receipts()[0].execution_id)
        .unwrap()
        .contract
        .clone()
        .unwrap();
    f.worker();
    let m = f.manifest(&c);
    f.write_manifest(&m);
    let imported = zenpi::headless::import_blueprint_manifest(
        &mut f.agent,
        "evidence-plan@1",
        &format!(".zenpi-results/{}.json", c.claim_digest),
    )
    .unwrap();
    assert_eq!(imported["status"], "candidate");
    assert_eq!(imported["master_accepted"], false);
    let mut state = zenpi::tui::TuiState::default();
    zenpi::tui::dispatch_slash_command(
        zenpi::slash::SlashCommand::Blueprint {
            action: zenpi::slash::BlueprintAction::Accept {
                target: "evidence-plan@1".into(),
                claim: c.claim_digest.clone(),
            },
        },
        &mut state,
        Some(&mut f.agent),
    );
    f.store.reload().unwrap();
    assert!(f.store.is_master_accepted(&f.store.receipts()[0]));
    assert!(
        state
            .messages()
            .any(|m| m.text.contains("Master acceptance: accepted"))
    );
    let domains = zenpi::domain_store::DomainStore::open_read_only(
        zenpi::domain_store::path_for_session(f.agent.session().path()),
    )
    .unwrap();
    assert_eq!(
        domains.goal("evidence-goal").unwrap().status,
        GoalStatus::Done
    );
}

#[test]
fn expired_lease_precancelled_prepare_and_missing_delta_fail_without_acceptance() {
    let mut f = Fixture::with_ttl(VALIDATOR, 1000);
    let c = f.prepare();
    f.worker();
    let m = f.manifest(&c);
    f.import(&m).unwrap();
    let wait = f
        .request
        .lease
        .expires_at_ms
        .saturating_sub(zenpi::session::unix_time_ms());
    std::thread::sleep(Duration::from_millis(wait + 5));
    assert!(f.accept(&|| false).is_err());
    assert!(
        !f.agent
            .session()
            .events()
            .iter()
            .any(|e| e["type"] == "external_validator_observed")
    );
    let mut f = Fixture::new(VALIDATOR);
    let before = fs::read(f.store.path()).unwrap();
    assert!(
        f.agent
            .external_evidence_control(
                &mut f.store,
                &f.handoff,
                &f.goal,
                &f.blueprint,
                Some(&f.request),
                &|| true
            )
            .is_err()
    );
    assert_eq!(fs::read(f.store.path()).unwrap(), before);
    let c = f.prepare();
    let m = f.manifest(&c);
    assert!(m.changes.is_empty());
    assert!(f.import(&m).is_err());
    assert!(!f.store.is_master_accepted(&f.store.receipts()[0]));
}

#[test]
#[ignore = "production host fixture export"]
fn export_external_evidence_fixture() {
    let command = std::env::var("ZENPI_EVIDENCE_VALIDATOR").unwrap_or_else(|_| VALIDATOR.into());
    let mut f = Fixture::with_ttl(&command, 300000);
    f.agent.try_close().unwrap();
    let info = json!({"root":f.root.path(),"workspace":f.workspace,"session":f.agent.session().path(),"claim":f.handoff.claim_digest,"target":"evidence-plan@1"});
    let destination = std::env::var("ZENPI_EVIDENCE_EXPORT").unwrap();
    fs::write(destination, serde_json::to_vec(&info).unwrap()).unwrap();
    let _ = f.root.keep();
}
