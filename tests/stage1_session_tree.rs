use serde_json::{Value, json};
use std::{
    cell::Cell,
    io::Write,
    path::Path,
    time::{Duration, Instant},
};
use tempfile::tempdir;
use zenpi::{
    core::{Turn, TurnRole},
    session::SessionStore,
    session_tree::{MAX_TREE_PAGE_BYTES, TreeAnnotation, TreeLimits},
};
fn append(store: &mut SessionStore, id: &str, parent: Option<&str>) {
    let turn = parent.map_or_else(
        || Turn::new(id, TurnRole::User, id),
        |p| Turn::with_parent(id, p, TurnRole::Assistant, id),
    );
    store.append_turn(turn).unwrap();
}
fn id(store: &SessionStore, turn: &str) -> String {
    store
        .tree_snapshot(&|| false)
        .unwrap()
        .entry_for_turn(turn)
        .unwrap()
        .id
        .clone()
}
fn selected(store: &SessionStore) -> Option<String> {
    store
        .tree_snapshot(&|| false)
        .unwrap()
        .active_leaf()
        .map(str::to_owned)
}
fn path_ids(store: &SessionStore, leaf: Option<&str>) -> Vec<String> {
    store
        .tree_ancestry_turns(leaf, &|| false)
        .unwrap()
        .iter()
        .map(|t| t.id.clone())
        .collect()
}
fn fixture(path: &Path) -> SessionStore {
    let mut store = SessionStore::open(path).unwrap();
    append(&mut store, "A", None);
    append(&mut store, "B", Some("A"));
    append(&mut store, "D", Some("A"));
    store
}
#[test]
fn legacy_inspection_is_read_only_and_entry_parent_is_independent_of_active_turn() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut s = fixture(&path);
    let before = std::fs::read(&path).unwrap();
    let tree = s.tree_snapshot(&|| false).unwrap();
    assert!(!tree.enabled());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let b = tree.entry_for_turn("B").unwrap();
    let d = tree.entry_for_turn("D").unwrap();
    assert_eq!(d.parent_id.as_deref(), Some(b.id.as_str()));
    assert_eq!(s.turns()[2].parent_id.as_deref(), Some("A"));
    assert_ne!(d.id, "D");
    assert!(s.enable_tree(Default::default(), &|| false).unwrap());
    assert!(std::fs::read(&path).unwrap().starts_with(&before));
    assert!(!s.enable_tree(Default::default(), &|| false).unwrap());
    drop(s);
    let reopened = SessionStore::open_existing(&path).unwrap();
    assert_eq!(selected(&reopened), Some(d.id.clone()));
}
#[test]
fn a_to_b_and_a_to_c_coexist_and_navigation_is_durable_without_new_turn() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut s = fixture(&path);
    s.enable_tree(Default::default(), &|| false).unwrap();
    let a = id(&s, "A");
    let b = id(&s, "B");
    s.select_tree_leaf(Some(&a), &|| false).unwrap();
    drop(s);
    let mut s = SessionStore::open(&path).unwrap();
    assert_eq!(selected(&s), Some(a.clone()));
    assert_eq!(path_ids(&s, Some(&a)), ["A"]);
    append(&mut s, "C", Some("A"));
    let c = id(&s, "C");
    assert_eq!(path_ids(&s, Some(&c)), ["A", "C"]);
    assert_eq!(path_ids(&s, Some(&b)), ["A", "B"]);
    assert_eq!(s.turns().len(), 4);
    let tree = s.tree_snapshot(&|| false).unwrap();
    assert_ne!(
        tree.entry(&b).unwrap().branch_id,
        tree.entry(&c).unwrap().branch_id
    );
    assert_eq!(tree.page(0, 10).unwrap().nodes[0].children, 2);
    s.select_tree_leaf(Some(&b), &|| false).unwrap();
    s.select_tree_leaf(Some(&c), &|| false).unwrap();
    drop(s);
    assert_eq!(selected(&SessionStore::open(&path).unwrap()), Some(c));
}
#[test]
fn invalid_or_cancelled_controls_leave_the_journal_and_leaf_unchanged() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut s = fixture(&path);
    let before = std::fs::read(&path).unwrap();
    assert!(s.enable_tree(Default::default(), &|| true).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let polls = Cell::new(0);
    assert!(
        s.enable_tree(Default::default(), &|| {
            polls.set(polls.get() + 1);
            polls.get() > 3
        })
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    s.enable_tree(Default::default(), &|| false).unwrap();
    let before = std::fs::read(&path).unwrap();
    let old = selected(&s);
    let a = id(&s, "A");
    for leaf in ["missing".to_owned(), "x".repeat(300), "\n".into()] {
        assert!(s.select_tree_leaf(Some(&leaf), &|| false).is_err());
    }
    let polls = Cell::new(0);
    assert!(
        s.select_tree_leaf(Some(&a), &|| {
            polls.set(polls.get() + 1);
            polls.get() == 2
        })
        .is_err()
    );
    assert!(
        s.annotate_tree_entry(
            &a,
            TreeAnnotation {
                name: Some("bad\nlabel".into()),
                summary: None
            },
            &|| false
        )
        .is_err()
    );
    assert!(
        s.append_event(json!({"type":"session_tree","action":"select"}))
            .is_err()
    );
    assert_eq!(selected(&s), old);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
#[test]
fn entry_depth_and_byte_limits_reject_before_append() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("existing.jsonl");
    let mut s = fixture(&path);
    let before = std::fs::read(&path).unwrap();
    assert!(
        s.enable_tree(
            TreeLimits {
                max_entries: 2,
                ..Default::default()
            },
            &|| false
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    s.enable_tree(
        TreeLimits {
            max_entries: 3,
            ..Default::default()
        },
        &|| false,
    )
    .unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(
        s.append_turn(Turn::new("overflow", TurnRole::User, "no"))
            .is_err()
    );
    assert_eq!(s.turns().len(), 3);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    for (file, limits) in [
        (
            "depth",
            TreeLimits {
                max_depth: 1,
                ..Default::default()
            },
        ),
        (
            "bytes",
            TreeLimits {
                max_bytes: 1,
                ..Default::default()
            },
        ),
    ] {
        let path = dir.path().join(file);
        let mut s = SessionStore::open(&path).unwrap();
        s.enable_tree(limits, &|| false).unwrap();
        if file == "depth" {
            append(&mut s, "A", None);
        }
        let before = std::fs::read(&path).unwrap();
        assert!(s.append_turn(Turn::new("B", TurnRole::User, "no")).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
#[test]
fn names_summaries_and_pagination_are_bounded_and_survive_reopen() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut s = SessionStore::open(&path).unwrap();
    s.enable_tree(Default::default(), &|| false).unwrap();
    for n in 0..40 {
        append(&mut s, &format!("t{n}"), None);
        let entry = id(&s, &format!("t{n}"));
        s.annotate_tree_entry(
            &entry,
            TreeAnnotation {
                name: Some(format!("枝 {n}")),
                summary: Some("summary ".repeat(250)),
            },
            &|| false,
        )
        .unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    drop(s);
    let s = SessionStore::open(&path).unwrap();
    let tree = s.tree_snapshot(&|| false).unwrap();
    let mut cursor = 0;
    let mut nodes = vec![];
    loop {
        let page = tree.page(cursor, 128).unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= MAX_TREE_PAGE_BYTES);
        nodes.extend(page.nodes);
        match page.next_cursor {
            Some(next) => {
                assert!(next > cursor);
                cursor = next;
            }
            None => break,
        }
    }
    assert_eq!(nodes.len(), 40);
    assert_eq!(nodes[0].annotation.name.as_deref(), Some("枝 0"));
    assert!(tree.page(99, 2).is_err());
    assert!(tree.page(0, 0).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
#[test]
fn competing_writers_cannot_publish_stale_links_or_idempotent_selection() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut winner = fixture(&path);
    winner.enable_tree(Default::default(), &|| false).unwrap();
    let mut stale = SessionStore::open(&path).unwrap();
    let a = id(&winner, "A");
    let old = selected(&stale).unwrap();
    winner.select_tree_leaf(Some(&a), &|| false).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(stale.select_tree_leaf(Some(&old), &|| false).is_err());
    assert!(
        stale
            .append_turn(Turn::new("stale", TurnRole::User, "no"))
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(selected(&stale), Some(old));
    assert_eq!(selected(&SessionStore::open(&path).unwrap()), Some(a));
}
#[test]
fn corrupt_parent_cycle_cross_session_branch_or_migration_hash_fail_closed() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.jsonl");
    let mut s = fixture(&source);
    s.enable_tree(Default::default(), &|| false).unwrap();
    append(&mut s, "C", None);
    let original = std::fs::read_to_string(&source).unwrap();
    for case in 0..7 {
        let mut rows: Vec<Value> = original
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        if case == 6 {
            let migration = rows
                .iter_mut()
                .find(|v| v["event"]["type"] == "session_tree")
                .unwrap();
            migration["event"]["action"]["source_sha256"] = json!("wrong");
        } else {
            let entry = rows.last_mut().unwrap().get_mut("tree_entry").unwrap();
            match case {
                0 => entry["parent_id"] = entry["id"].clone(),
                1 => entry["parent_id"] = json!("missing"),
                2 => entry["session_id"] = json!("other-session"),
                3 => entry["branch_id"] = json!("other-branch"),
                4 => entry["depth"] = json!(999),
                _ => entry["schema_version"] = json!(99),
            }
        }
        let path = dir.path().join(format!("bad-{case}.jsonl"));
        let bytes = rows
            .into_iter()
            .map(|v| v.to_string() + "\n")
            .collect::<String>();
        std::fs::write(&path, &bytes).unwrap();
        assert!(SessionStore::open(&path).is_err(), "case {case}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
    }
}
#[test]
fn selected_fork_has_fresh_owner_only_selected_ancestry_and_no_live_controls() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("source.jsonl");
    let mut s = fixture(&path);
    s.append_event(json!({"type":"operation_started","operation_id":"foreign-operation","operation_kind":"provider","turn_id":"A","retry_requires_confirmation":true})).unwrap();
    s.append_event(json!({"type":"mailbox_delivered","owner":"source-only"}))
        .unwrap();
    s.append_event(json!({"type":"input_queue","pending":"do not replay"}))
        .unwrap();
    s.enable_tree(Default::default(), &|| false).unwrap();
    let a = id(&s, "A");
    let b = id(&s, "B");
    s.select_tree_leaf(Some(&a), &|| false).unwrap();
    append(&mut s, "C", Some("A"));
    s.annotate_tree_entry(
        &b,
        TreeAnnotation {
            name: Some("chosen".into()),
            summary: Some("A and B only".into()),
        },
        &|| false,
    )
    .unwrap();
    let before = std::fs::read(&path).unwrap();
    let destination = dir.path().join("fork.jsonl");
    let fork = s
        .fork_at_tree_leaf(Some(&b), &destination, &|| false)
        .unwrap();
    assert_ne!(fork.session_id(), s.session_id());
    assert_eq!(
        fork.turns()
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        ["A", "B"]
    );
    assert!(fork.operation_recovery().is_empty());
    assert!(!fork.events().iter().any(|v| matches!(
        v["type"].as_str(),
        Some("operation_started" | "mailbox_delivered" | "input_queue")
    )));
    assert_eq!(
        fork.tree_snapshot(&|| false)
            .unwrap()
            .annotation(&id(&fork, "B"))
            .name
            .as_deref(),
        Some("chosen")
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(fork.header().cwd, s.header().cwd);
    assert!(
        s.fork_at_tree_leaf(Some(&b), &destination, &|| false)
            .is_err()
    );
}
#[test]
fn unfinished_and_unknown_tool_ancestry_cannot_gain_a_new_owner_via_fork() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut s = SessionStore::open(&path).unwrap();
    s.enable_tree(Default::default(), &|| false).unwrap();
    append(&mut s, "A", None);
    let mut call = Turn::with_parent("call", "A", TurnRole::Assistant, "");
    call.metadata =
        Some(json!({"tool_calls":[{"id":"tool","name":"write_file","arguments":{"path":"file"}}]}));
    s.append_turn(call).unwrap();
    let leaf = selected(&s).unwrap();
    let dest = dir.path().join("fork.jsonl");
    assert!(s.fork_at_tree_leaf(Some(&leaf), &dest, &|| false).is_err());
    assert!(!dest.exists());
    let mut result = Turn::with_parent("result", "A", TurnRole::Tool, "unknown");
    result.metadata = Some(json!({"tool_call_id":"tool","outcome":"unknown_outcome"}));
    s.append_turn(result).unwrap();
    assert!(
        s.fork_at_tree_leaf(selected(&s).as_deref(), &dest, &|| false)
            .is_err()
    );
    assert!(!dest.exists());
}
#[test]
fn cancelled_fork_publishes_nothing_and_does_not_touch_an_existing_destination() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let s = fixture(&path);
    let leaf = id(&s, "B");
    let destination = dir.path().join("fork.jsonl");
    let saw_staged_turn = Cell::new(false);
    assert!(
        s.fork_at_tree_leaf(Some(&leaf), &destination, &|| {
            let staged = std::fs::read_dir(dir.path()).unwrap().any(|entry| {
                let entry = entry.unwrap();
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".tree-fork")
                    && std::fs::read_to_string(entry.path())
                        .unwrap_or_default()
                        .contains("\"kind\":\"turn\"")
            });
            saw_staged_turn.set(saw_staged_turn.get() || staged);
            staged
        })
        .is_err()
    );
    assert!(
        saw_staged_turn.get(),
        "fixture must cancel after an actual staged turn"
    );
    assert!(!destination.exists());
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|e| {
        !e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".tree-fork")
    }));
    std::fs::write(&destination, "user file").unwrap();
    assert!(
        s.fork_at_tree_leaf(Some(&leaf), &destination, &|| false)
            .is_err()
    );
    assert_eq!(std::fs::read_to_string(&destination).unwrap(), "user file");
}
#[test]
fn interrupted_append_keeps_last_complete_leaf_and_later_append_keeps_both_branches() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut s = fixture(&path);
    s.enable_tree(Default::default(), &|| false).unwrap();
    let a = id(&s, "A");
    s.select_tree_leaf(Some(&a), &|| false).unwrap();
    drop(s);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{\"kind\":\"turn\",\"tree_entry\":")
        .unwrap();
    let mut s = SessionStore::open(&path).unwrap();
    assert_eq!(selected(&s), Some(a));
    assert!(!s.recovery_warnings().is_empty());
    append(&mut s, "C", Some("A"));
    let leaf = selected(&s);
    drop(s);
    let reopened = SessionStore::open(&path).unwrap();
    assert_eq!(selected(&reopened), leaf);
    assert_eq!(reopened.turns().len(), 4);
}
#[test]
#[ignore = "launched in independent processes by parent tests"]
fn tree_process_helper() {
    let path = std::env::var("ZENPI_TREE_TEST_PATH").unwrap();
    let mut s = SessionStore::open(&path).unwrap();
    let dir = Path::new(&path).parent().unwrap();
    let mode = std::env::var("ZENPI_TREE_TEST_MODE").unwrap();
    if mode == "select" {
        let leaf = std::env::var("ZENPI_TREE_TEST_LEAF").unwrap();
        s.select_tree_leaf(Some(&leaf), &|| false).unwrap();
        std::process::exit(0);
    }
    std::fs::write(dir.join(format!("ready-{mode}")), b"ready").unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while !dir.join("go").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let result = s.append_turn(Turn::new(&mode, TurnRole::User, &mode));
    std::fs::write(
        dir.join(format!("result-{mode}")),
        json!({"success":result.is_ok(),"error":result.err().map(|e|e.to_string())}).to_string(),
    )
    .unwrap();
}
fn helper(path: &Path, mode: &str) -> std::process::Command {
    let mut c = std::process::Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "tree_process_helper", "--ignored", "--nocapture"])
        .env("ZENPI_TREE_TEST_PATH", path)
        .env("ZENPI_TREE_TEST_MODE", mode);
    c
}
#[test]
fn independent_process_selection_and_competing_appends_preserve_single_owner() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut s = fixture(&path);
    s.enable_tree(Default::default(), &|| false).unwrap();
    let a = id(&s, "A");
    drop(s);
    assert!(
        helper(&path, "select")
            .env("ZENPI_TREE_TEST_LEAF", &a)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(selected(&SessionStore::open(&path).unwrap()), Some(a));
    let mut p1 = helper(&path, "one").spawn().unwrap();
    let mut p2 = helper(&path, "two").spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while !dir.path().join("ready-one").exists() || !dir.path().join("ready-two").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    std::fs::write(dir.path().join("go"), b"go").unwrap();
    assert!(p1.wait().unwrap().success());
    assert!(p2.wait().unwrap().success());
    let results: [Value; 2] = ["one", "two"].map(|name| {
        serde_json::from_slice(&std::fs::read(dir.path().join(format!("result-{name}"))).unwrap())
            .unwrap()
    });
    assert_eq!(results.iter().filter(|v| v["success"] == true).count(), 1);
    let s = SessionStore::open(&path).unwrap();
    assert_eq!(s.turns().len(), 4);
    assert!(s.recovery_warnings().is_empty());
    assert_eq!(path_ids(&s, selected(&s).as_deref()).len(), 2);
}

#[test]
fn fork_publication_race_preserves_the_other_writers_destination() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.jsonl");
    let s = fixture(&source);
    let leaf = id(&s, "B");
    let destination = dir.path().join("fork.jsonl");
    let raced = Cell::new(false);
    let result = s.fork_at_tree_leaf(Some(&leaf), &destination, &|| {
        if !raced.get()
            && std::fs::read_dir(dir.path()).unwrap().any(|entry| {
                let entry = entry.unwrap();
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".tree-fork")
                    && std::fs::read_to_string(entry.path())
                        .unwrap_or_default()
                        .contains("\"kind\":\"turn\"")
            })
        {
            std::fs::write(&destination, "concurrent user's file").unwrap();
            raced.set(true);
        }
        false
    });
    assert!(raced.get());
    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "concurrent user's file"
    );
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".tree-fork")
    }));
}
