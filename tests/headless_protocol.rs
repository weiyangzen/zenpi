use serde_json::Value;
use std::io::Cursor;
use tempfile::tempdir;
use zenpi::{core::Agent, headless::run_headless, protocol::parse_line, session::SessionStore};

fn json_lines(bytes: &[u8]) -> Vec<Value> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn malformed_frame_isolated_from_all_commands() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("wire.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let input = b"{bad}\n{\"type\":\"prompt\",\"id\":\"p\",\"text\":\"a\\u2028b\",\"mode\":\"start_if_idle\"}\n{\"type\":\"status\",\"id\":\"s\"}\n{\"type\":\"handoff\",\"id\":\"h\",\"to\":\"worker\",\"summary\":\"ready\",\"artifacts\":[\"Docs/plan.md\"]}\n{\"type\":\"shutdown\",\"id\":\"q\"}\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input), &mut output).unwrap();
    let records = json_lines(&output);
    assert!(records.iter().any(|v| v["success"] == false));
    for id in ["p", "s", "h", "q"] {
        assert!(
            records
                .iter()
                .any(|v| v["id"] == id && v["success"] == true)
        );
    }
    let journal = json_lines(&std::fs::read(&path).unwrap());
    assert!(journal.iter().any(|v| v["kind"] == "handoff_record"));
    let seq: Vec<u64> = journal.iter().map(|v| v["seq"].as_u64().unwrap()).collect();
    assert!(seq.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn unsupported_version_and_missing_id_are_rejected() {
    assert!(
        parse_line(r#"{"schema_version":2,"type":"status","id":"x"}"#)
            .unwrap()
            .into_command()
            .is_err()
    );
    assert!(
        parse_line(r#"{"type":"status"}"#)
            .unwrap()
            .into_command()
            .is_err()
    );
}

#[test]
fn sequential_steer_and_eof_are_safe_terminal_paths() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("steer-eof.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let input = b"{\"type\":\"prompt\",\"id\":\"p\",\"text\":\"hello\",\"mode\":\"start_if_idle\"}\n{\"type\":\"steer\",\"id\":\"s\",\"text\":\"follow-up\"}\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input), &mut output).unwrap();
    let records = json_lines(&output);
    assert!(
        records
            .iter()
            .any(|value| value["id"] == "p" && value["success"] == true)
    );
    assert!(
        records
            .iter()
            .any(|value| value["id"] == "s" && value["success"] == true)
    );
    assert_eq!(agent.phase(), zenpi::core::AgentPhase::Closed);
}
