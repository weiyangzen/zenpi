use std::io::Cursor;

use serde_json::Value;
use tempfile::tempdir;
use zenpi::{
    b3::ResourceBudget,
    core::{Agent, TurnRole},
    domain_store::{DomainStore, path_for_session},
    domains::{Blueprint, BlueprintItem, Goal, GoalStatus},
    headless::run_headless,
    session::SessionStore,
};

fn records(bytes: &[u8]) -> Vec<Value> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).expect("valid JSONL response"))
        .collect()
}

fn seed_goal(session_path: &std::path::Path) {
    let blueprint =
        Blueprint::new("owner-plan", "1", vec![BlueprintItem::new("build", 100)]).unwrap();
    let goal = Goal::new("owner-goal", &blueprint, ResourceBudget::default(), None).unwrap();
    let mut store = DomainStore::open(path_for_session(session_path)).unwrap();
    store.put_blueprint(blueprint).unwrap();
    store.put_goal(goal).unwrap();
}

#[test]
fn headless_goal_status_is_a_durable_typed_owner_operation() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.jsonl");
    seed_goal(&session_path);
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let input = [
        serde_json::json!({
            "type": "command",
            "id": "show-one",
            "text": "/goal status owner-goal",
        }),
        serde_json::json!({
            "type": "command",
            "id": "start",
            "text": "/goal status owner-goal running",
        }),
        serde_json::json!({
            "type": "command",
            "id": "finish",
            "text": "/goal transition owner-goal done",
        }),
        serde_json::json!({
            "type": "command",
            "id": "illegal",
            "text": "/goal status owner-goal queued",
        }),
        serde_json::json!({"type": "shutdown", "id": "shutdown"}),
    ]
    .into_iter()
    .map(|value| serde_json::to_string(&value).unwrap())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.into_bytes()), &mut output).unwrap();
    let records = records(&output);

    let shown = records
        .iter()
        .find(|record| record["id"] == "show-one")
        .unwrap();
    assert_eq!(shown["success"], true);
    assert_eq!(shown["data"]["action"], "show");
    assert_eq!(shown["data"]["goal"]["status"], "queued");

    let started = records
        .iter()
        .find(|record| record["id"] == "start")
        .unwrap();
    assert_eq!(started["success"], true);
    assert_eq!(started["data"]["action"], "transition");
    assert_eq!(started["data"]["from_status"], "queued");
    assert_eq!(started["data"]["to_status"], "running");
    assert_eq!(started["data"]["change"], "updated");
    assert_eq!(started["data"]["goal"]["status"], "running");

    let finished = records
        .iter()
        .find(|record| record["id"] == "finish")
        .unwrap();
    assert_eq!(finished["success"], true);
    assert_eq!(finished["data"]["to_status"], "done");
    assert_eq!(finished["data"]["goal"]["status"], "done");

    let illegal = records
        .iter()
        .find(|record| record["id"] == "illegal")
        .unwrap();
    assert_eq!(illegal["success"], false);
    assert_eq!(illegal["code"], "goal_invalid_transition");
    assert!(illegal["error"].as_str().unwrap().contains("done"));

    let persisted = DomainStore::open(path_for_session(&session_path)).unwrap();
    assert_eq!(
        persisted.goal("owner-goal").unwrap().status,
        GoalStatus::Done
    );
    assert!(
        agent
            .history()
            .iter()
            .all(|turn| turn.role != TurnRole::User)
    );
}

#[test]
fn headless_goal_create_links_one_persisted_blueprint_without_provider_work() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.jsonl");
    let blueprint =
        Blueprint::new("create-plan", "1", vec![BlueprintItem::new("build", 100)]).unwrap();
    let mut store = DomainStore::open(path_for_session(&session_path)).unwrap();
    store.put_blueprint(blueprint).unwrap();
    drop(store);
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let input = [
        serde_json::json!({"type":"command","id":"create","text":"/goal create created-goal create-plan@1"}),
        serde_json::json!({"type":"command","id":"show","text":"/goal show created-goal"}),
        serde_json::json!({"type":"shutdown","id":"shutdown"}),
    ]
    .into_iter()
    .map(|value| serde_json::to_string(&value).unwrap())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.into_bytes()), &mut output).unwrap();
    let records = records(&output);
    let created = records
        .iter()
        .find(|record| record["id"] == "create")
        .unwrap();
    assert_eq!(created["success"], true);
    assert_eq!(created["data"]["action"], "create");
    assert_eq!(created["data"]["goal"]["status"], "queued");
    let shown = records
        .iter()
        .find(|record| record["id"] == "show")
        .unwrap();
    assert_eq!(shown["data"]["goal"]["id"], "created-goal");
    assert_eq!(shown["data"]["goal"]["blueprint_id"], "create-plan");
}

#[test]
fn headless_plan_creates_bounded_sequential_blueprint_without_provider_work() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let input = [
        serde_json::json!({"type":"command","id":"plan","text":"/plan release-plan :: inspect; test; publish"}),
        serde_json::json!({"type":"command","id":"show","text":"/blueprint show"}),
        serde_json::json!({"type":"shutdown","id":"shutdown"}),
    ].into_iter().map(|value| serde_json::to_string(&value).unwrap()).collect::<Vec<_>>().join("\n") + "\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.into_bytes()), &mut output).unwrap();
    let records = records(&output);
    let plan = records
        .iter()
        .find(|record| record["id"] == "plan")
        .unwrap();
    assert_eq!(plan["success"], true);
    assert_eq!(plan["data"]["action"], "create");
    let blueprint = plan["data"]["blueprint"].clone();
    assert_eq!(blueprint["items"].as_array().unwrap().len(), 3);
    assert_eq!(blueprint["items"][0]["task"]["instruction"], "inspect");
    assert_eq!(
        blueprint["items"][0]["task"]["acceptance_commands"][0],
        "external-owner:acceptance-required"
    );
    assert_eq!(blueprint["items"][1]["depends_on"][0], "step-1");
    assert_eq!(blueprint["items"][2]["depends_on"][0], "step-2");
}

#[test]
fn missing_or_malformed_goal_status_fails_without_creating_a_store() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.jsonl");
    let domain_path = path_for_session(&session_path);
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let input = [
        serde_json::json!({
            "type": "command",
            "id": "missing",
            "text": "/goal status absent running",
        }),
        serde_json::json!({
            "type": "command",
            "id": "bad-status",
            "text": "/goal status absent exploding",
        }),
        serde_json::json!({
            "type": "command",
            "id": "bad-shape",
            "text": "/goal transition absent",
        }),
        serde_json::json!({"type": "shutdown", "id": "shutdown"}),
    ]
    .into_iter()
    .map(|value| serde_json::to_string(&value).unwrap())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.into_bytes()), &mut output).unwrap();
    let records = records(&output);

    let missing = records
        .iter()
        .find(|record| record["id"] == "missing")
        .unwrap();
    assert_eq!(missing["success"], false);
    assert_eq!(missing["code"], "goal_not_found");
    let bad_status = records
        .iter()
        .find(|record| record["id"] == "bad-status")
        .unwrap();
    assert_eq!(bad_status["success"], false);
    assert_eq!(bad_status["code"], "goal_invalid_status");
    let bad_shape = records
        .iter()
        .find(|record| record["id"] == "bad-shape")
        .unwrap();
    assert_eq!(bad_shape["success"], false);
    assert_eq!(bad_shape["code"], "goal_status_usage");
    assert!(!domain_path.exists());
}

#[test]
fn headless_goal_run_resume_and_cancel_aliases_are_durable() {
    let dir = tempdir().unwrap();
    let session_path = dir.path().join("session.jsonl");
    seed_goal(&session_path);
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let input = [
        serde_json::json!({"type":"command","id":"run","text":"/goal run owner-goal"}),
        serde_json::json!({"type":"command","id":"pause","text":"/goal status owner-goal paused"}),
        serde_json::json!({"type":"command","id":"resume","text":"/goal resume owner-goal"}),
        serde_json::json!({"type":"command","id":"cancel","text":"/goal cancel owner-goal"}),
        serde_json::json!({"type":"shutdown","id":"shutdown"}),
    ]
    .into_iter()
    .map(|value| serde_json::to_string(&value).unwrap())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.into_bytes()), &mut output).unwrap();
    let records = records(&output);
    for id in ["run", "pause", "resume", "cancel"] {
        assert_eq!(
            records.iter().find(|record| record["id"] == id).unwrap()["success"],
            true,
            "{id}: {:?}",
            records.iter().find(|record| record["id"] == id).unwrap()
        );
    }
    assert_eq!(
        DomainStore::open(path_for_session(&session_path))
            .unwrap()
            .goal("owner-goal")
            .unwrap()
            .status,
        GoalStatus::Cancelled
    );
}

#[test]
fn headless_layout_and_pane_owner_persist_project_state_and_fail_closed() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let previous = std::env::var_os("ZENPI_HOME");
    unsafe { std::env::set_var("ZENPI_HOME", &home) };
    let session_path = dir.path().join("session.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    let input = [
        serde_json::json!({"type":"command","id":"pane","text":"/pane collapse resources"}),
        serde_json::json!({"type":"command","id":"layout","text":"/layout show"}),
        serde_json::json!({"type":"command","id":"bad","text":"/pane focus terminal"}),
        serde_json::json!({"type":"shutdown","id":"shutdown"}),
    ]
    .into_iter()
    .map(|value| serde_json::to_string(&value).unwrap())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.into_bytes()), &mut output).unwrap();
    let records = records(&output);
    assert_eq!(
        records
            .iter()
            .find(|record| record["id"] == "pane")
            .unwrap()["success"],
        true
    );
    assert_eq!(
        records
            .iter()
            .find(|record| record["id"] == "layout")
            .unwrap()["data"]["accepted"],
        true
    );
    let bad = records.iter().find(|record| record["id"] == "bad").unwrap();
    assert_eq!(bad["success"], false);
    assert_eq!(bad["code"], "pane_unavailable");
    assert!(home.join("layout.json").is_file());
    assert!(
        std::fs::read_to_string(home.join("layout.json"))
            .unwrap()
            .contains("resources")
    );
    if let Some(value) = previous {
        unsafe { std::env::set_var("ZENPI_HOME", value) };
    } else {
        unsafe { std::env::remove_var("ZENPI_HOME") };
    }
}

#[test]
fn goal_create_respects_explicit_version_and_does_not_mutate_on_rejection() {
    for (target, expected_version) in [
        ("single@missing", None),
        ("single@x@y", None),
        ("single", Some("1")),
        ("multi", None),
        ("multi@2", Some("2")),
        ("label@x@y", Some("x@y")),
    ] {
        let dir = tempdir().unwrap();
        let session_path = dir.path().join("session.jsonl");
        let domain_path = path_for_session(&session_path);
        assert!(
            domain_path.starts_with(dir.path()),
            "fixture store escaped tempdir"
        );
        let mut store = DomainStore::open(&domain_path).unwrap();
        for (id, version) in [
            ("single", "1"),
            ("multi", "1"),
            ("multi", "2"),
            ("label", "x@y"),
        ] {
            store
                .put_blueprint(
                    Blueprint::new(id, version, vec![BlueprintItem::new("inspect", 1)]).unwrap(),
                )
                .unwrap();
        }
        drop(store);
        let before = std::fs::read(&domain_path).unwrap();
        let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
        let input = [
            serde_json::json!({"type":"command","id":"create","text":format!("/goal create selected {target}")}),
            serde_json::json!({"type":"shutdown","id":"stop"}),
        ].into_iter().map(|value| serde_json::to_string(&value).unwrap()).collect::<Vec<_>>().join("\n") + "\n";
        let mut output = Vec::new();
        run_headless(&mut agent, Cursor::new(input.into_bytes()), &mut output).unwrap();
        let wire = records(&output);
        let response = wire
            .iter()
            .find(|row| row["id"] == "create" && row["type"] == "response")
            .unwrap();
        let restored = DomainStore::open_read_only(&domain_path).unwrap();
        if let Some(version) = expected_version {
            assert_eq!(response["success"], true, "{target}: {response}");
            let goal = restored.goal("selected").expect("goal must be durable");
            let blueprint_id = target.split('@').next().unwrap();
            let blueprint = restored.blueprint(blueprint_id, version).unwrap();
            assert_eq!(goal.blueprint_id, blueprint_id);
            assert_eq!(goal.blueprint_version, version);
            assert_eq!(goal.blueprint_digest, blueprint.digest);
            assert_eq!(goal.status, GoalStatus::Queued);
            assert_eq!(response["data"]["zenpi_started"], false);
        } else {
            assert_eq!(response["success"], false, "{target}: {response}");
            assert_eq!(response["code"], "blueprint_not_found");
            assert!(restored.goal("selected").is_none());
            assert_eq!(
                std::fs::read(&domain_path).unwrap(),
                before,
                "{target} mutated domain store"
            );
        }
        assert!(
            agent
                .history()
                .iter()
                .all(|turn| turn.role != TurnRole::User && turn.role != TurnRole::Assistant)
        );
    }
}
