use serde_json::json;
use zenpi::{
    backend::Usage,
    context::{self, ContextBudget, ContextError, SemanticCheckpoint},
    core::{Turn, TurnRole},
    session::SessionStore,
};

fn history() -> Vec<Turn> {
    let mut call = Turn::with_parent("calls", "u1", TurnRole::Assistant, "");
    call.metadata = Some(json!({"tool_calls":[
        {"id":"r","name":"read_file","arguments":{"path":"constraints.txt"}},
        {"id":"w","name":"write_file","arguments":{"path":"output.txt","content":"ok"}}
    ]}));
    let result = |id, name, record| {
        let mut turn = Turn::with_parent(record, "u1", TurnRole::Tool, "ok");
        turn.metadata = Some(json!({"tool_call_id":id,"tool_name":name,"outcome":"succeeded"}));
        turn
    };
    vec![
        Turn::new("s", TurnRole::System, "Preserve the BentoBox layout."),
        Turn::new(
            "u1",
            TurnRole::User,
            "Goal: improve project tabs. Keep Unicode folders.",
        ),
        call,
        result("r", "read_file", "r-result"),
        result("w", "write_file", "w-result"),
        Turn::with_parent("a1", "u1", TurnRole::Assistant, "Files updated."),
        Turn::new("u2", TurnRole::User, "Next: verify restart."),
    ]
}

fn finalize(turns: &[Turn], session: &str) -> SemanticCheckpoint {
    context::prepare_semantic_compaction(turns, session, "linear", 0, &|| false)
        .unwrap()
        .finalize(
            turns,
            "Goal: improve project tabs. Preserve BentoBox and Unicode paths. Files updated; restart verification remains.".into(),
            "fixture".into(), "fixture-model".into(),
            Usage { input_tokens: 100, output_tokens: 20, total_tokens: 120 },
            ContextBudget::default(), &|| false,
        ).unwrap()
}

#[test]
fn whole_turn_cut_keeps_multi_call_pairs_files_and_source_identity() {
    let turns = history();
    let cp = finalize(&turns, "session-one");
    assert_eq!(cp.schema_version, 2);
    assert_eq!(cp.source_end, 6);
    assert_eq!(cp.retained_ids, ["u2"]);
    assert_eq!(cp.read_files, ["constraints.txt"]);
    assert_eq!(cp.modified_files, ["output.txt"]);
    assert!(cp.unresolved_calls.is_empty());
    let output = context::restore_semantic_checkpoint(
        &turns,
        &cp,
        "session-one",
        "linear",
        ContextBudget::default(),
    )
    .unwrap();
    assert_eq!(
        output.iter().map(|t| t.role).collect::<Vec<_>>(),
        [TurnRole::System, TurnRole::System, TurnRole::User]
    );
    assert_eq!(output[0].id, "s");
    assert!(output[1].content.contains("Unicode"));
    assert_eq!(output[2].id, "u2");
}

#[test]
fn changed_source_cut_tail_parent_summary_and_branch_are_rejected() {
    let source = history();
    let cp = finalize(&source, "session-one");
    for field in 0..7 {
        let mut changed = cp.clone();
        match field {
            0 => changed.source_end = 4,
            1 => changed.retained_ids.clear(),
            2 => changed.covered_sha256.clear(),
            3 => changed.summary.push('!'),
            4 => changed.source.tip_id = "other".into(),
            5 => changed.source.branch_id = "other".into(),
            _ => changed.schema_version = 99,
        }
        assert!(
            context::restore_semantic_checkpoint(
                &source,
                &changed,
                "session-one",
                "linear",
                ContextBudget::default()
            )
            .is_err(),
            "field {field}"
        );
    }
    let mut changed = source.clone();
    changed[1].content.push('!');
    assert!(
        context::restore_semantic_checkpoint(
            &changed,
            &cp,
            "session-one",
            "linear",
            ContextBudget::default()
        )
        .is_err()
    );
    changed = source.clone();
    changed[3].parent_id = Some("missing".into());
    assert!(
        context::prepare_semantic_compaction(&changed, "session-one", "linear", 0, &|| false)
            .is_err()
    );
    changed = source.clone();
    changed.remove(3);
    assert!(
        context::prepare_semantic_compaction(&changed, "session-one", "linear", 0, &|| false)
            .is_err()
    );
    assert!(
        context::restore_semantic_checkpoint(
            &source,
            &cp,
            "other-session",
            "linear",
            ContextBudget::default()
        )
        .is_err()
    );
}

#[test]
fn unresolved_tool_results_stay_in_tail_and_failed_assistants_are_not_projected() {
    let mut turns = history();
    let mut call = Turn::with_parent("pending", "u2", TurnRole::Assistant, "");
    call.metadata =
        Some(json!({"tool_calls":[{"id":"r","name":"read_file","arguments":{"path":"todo.txt"}}]}));
    turns.push(call);
    let mut bad = Turn::with_parent(
        "aborted",
        "u2",
        TurnRole::Assistant,
        "never treat as answer",
    );
    bad.metadata = Some(json!({"stop_reason":"aborted"}));
    turns.push(bad);
    let cp = finalize(&turns, "session-one");
    assert_eq!(cp.unresolved_calls.len(), 1);
    assert_eq!(cp.unresolved_calls[0].turn_id, "u2");
    let output = context::restore_semantic_checkpoint(
        &turns,
        &cp,
        "session-one",
        "linear",
        ContextBudget::default(),
    )
    .unwrap();
    assert!(output.iter().any(|t| t.id == "pending"));
    assert!(!output.iter().any(|t| t.id == "aborted"));
    // A later user record cannot cause an unresolved prior call to be summarized away.
    turns.push(Turn::new("u3", TurnRole::User, "Continue"));
    assert_eq!(finalize(&turns, "session-one").source_end, 6);
}

#[test]
fn failed_writes_are_not_reported_as_modified_and_unknown_prefix_is_not_compacted() {
    let mut turns = history();
    turns[4].metadata.as_mut().unwrap()["outcome"] = json!("denied");
    assert!(finalize(&turns, "session-one").modified_files.is_empty());
    turns[4].metadata.as_mut().unwrap()["outcome"] = json!("unknown_outcome");
    assert!(
        context::prepare_semantic_compaction(&turns, "session-one", "linear", 0, &|| false)
            .is_err()
    );
}

#[test]
fn cancellation_empty_summary_and_budget_failure_leave_existing_checkpoint_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    let mut session = SessionStore::open(&path).unwrap();
    for turn in history() {
        session.append_turn(turn).unwrap();
    }
    let cp = finalize(session.turns(), session.session_id());
    assert!(
        session
            .append_semantic_checkpoint(&cp, ContextBudget::default(), &|| false)
            .unwrap()
    );
    let before = std::fs::read(&path).unwrap();
    assert!(
        session
            .append_semantic_checkpoint(&cp, ContextBudget::default(), &|| true)
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(
        !session
            .append_semantic_checkpoint(&cp, ContextBudget::default(), &|| false)
            .unwrap()
    );
    assert!(matches!(
        context::prepare_semantic_compaction(
            session.turns(),
            session.session_id(),
            "linear",
            0,
            &|| true
        ),
        Err(ContextError::Cancelled)
    ));
    let plan = context::prepare_semantic_compaction(
        session.turns(),
        session.session_id(),
        "linear",
        0,
        &|| false,
    )
    .unwrap();
    assert!(
        plan.finalize(
            session.turns(),
            "".into(),
            "fixture".into(),
            "model".into(),
            Usage::default(),
            ContextBudget::default(),
            &|| false
        )
        .is_err()
    );
    assert!(
        context::restore_semantic_checkpoint(
            session.turns(),
            &cp,
            session.session_id(),
            "linear",
            ContextBudget {
                max_tokens: 10,
                reserved_output_tokens: 9
            }
        )
        .is_err()
    );
    assert_eq!(
        session
            .latest_semantic_checkpoint(ContextBudget::default())
            .unwrap(),
        Some(cp)
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn stale_preparation_and_duplicate_result_rejected() {
    let mut turns = history();
    let plan = context::prepare_semantic_compaction(&turns, "session-one", "linear", 0, &|| false)
        .unwrap();
    turns.push(Turn::with_parent(
        "a2",
        "u2",
        TurnRole::Assistant,
        "new data",
    ));
    assert!(
        plan.finalize(
            &turns,
            "summary".into(),
            "fixture".into(),
            "model".into(),
            Usage::default(),
            ContextBudget::default(),
            &|| false
        )
        .is_err()
    );
    let mut duplicate = turns[3].clone();
    duplicate.id = "another-result".into();
    turns.push(duplicate);
    assert!(
        context::prepare_semantic_compaction(&turns, "session-one", "linear", 0, &|| false)
            .is_err()
    );
}

#[test]
fn durable_checkpoint_survives_a_new_process_and_retains_appended_turns() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    let mut session = SessionStore::open(&path).unwrap();
    for turn in history() {
        session.append_turn(turn).unwrap();
    }
    let cp = finalize(session.turns(), session.session_id());
    session
        .append_semantic_checkpoint(&cp, ContextBudget::default(), &|| false)
        .unwrap();
    session
        .append_turn(Turn::with_parent(
            "a2",
            "u2",
            TurnRole::Assistant,
            "restart checked",
        ))
        .unwrap();
    drop(session);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "checkpoint_recovery_child"])
        .env("ZENPI_CHECKPOINT_FIXTURE", &path)
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
}

#[test]
#[ignore = "invoked in a fresh process by durable_checkpoint_survives_a_new_process_and_retains_appended_turns"]
fn checkpoint_recovery_child() {
    let session =
        SessionStore::open_existing(std::env::var_os("ZENPI_CHECKPOINT_FIXTURE").unwrap()).unwrap();
    let cp = session
        .latest_semantic_checkpoint(ContextBudget::default())
        .unwrap()
        .unwrap();
    let projected = context::restore_semantic_checkpoint(
        session.turns(),
        &cp,
        session.session_id(),
        "linear",
        ContextBudget::default(),
    )
    .unwrap();
    assert!(projected.iter().any(|t| t.content.contains("Unicode")));
    assert_eq!(projected.last().unwrap().id, "a2");
}

#[test]
fn legacy_digest_checkpoint_remains_readable_without_becoming_semantic() {
    let turns: Vec<_> = (0..20)
        .map(|i| {
            Turn::new(
                format!("t{i}"),
                if i % 2 == 0 {
                    TurnRole::User
                } else {
                    TurnRole::Assistant
                },
                "x".repeat(500),
            )
        })
        .collect();
    let budget = ContextBudget {
        max_tokens: 1500,
        reserved_output_tokens: 300,
    };
    let old = context::prepare_context(&turns, budget, &|| false)
        .unwrap()
        .checkpoint
        .unwrap();
    let json = serde_json::to_value(&old).unwrap();
    assert!(serde_json::from_value::<SemanticCheckpoint>(json).is_err());
    assert!(context::restore_checkpoint(&turns, &old).is_ok());
}
