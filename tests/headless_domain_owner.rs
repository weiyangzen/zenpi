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
