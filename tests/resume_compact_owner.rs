use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use tempfile::tempdir;
use zenpi::{
    backend::{Backend, BackendError, Completion, CompletionRequest},
    context::{ContextBudget, restore_checkpoint},
    core::{Agent, Turn, TurnInputRequest, TurnRole},
    headless::{resume_session_view, run_headless},
    session::SessionStore,
    slash::SlashCommand,
    tui::{MessageRole, SlashDispatchAction, TuiState, dispatch_slash_command},
};

fn json_lines(bytes: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[derive(Clone)]
struct CaptureBackend(Arc<Mutex<Vec<String>>>);

impl Backend for CaptureBackend {
    fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        self.0
            .lock()
            .unwrap()
            .extend(request.turns.iter().map(|turn| turn.id.clone()));
        Ok(Completion::text("captured"))
    }

    fn name(&self) -> &str {
        "capture"
    }
}

#[test]
fn headless_resume_replays_durable_records_and_writes_marker() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let mut session = SessionStore::open(&path).unwrap();
    session
        .append_turn(Turn::new("user-1", TurnRole::User, "remember this"))
        .unwrap();
    session
        .append_event(serde_json::json!({"type":"checkpoint","value":1}))
        .unwrap();
    let mut agent = Agent::with_echo(session);

    let value = resume_session_view(&mut agent, Some(0)).unwrap();
    assert_eq!(value["accepted"], true);
    assert_eq!(value["durable"], true);
    assert!(value["replayed"].as_u64().unwrap() >= 3);
    assert!(
        value["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| { record["kind"] == "turn" && record["content"] == "remember this" })
    );
    assert!(
        agent.session().events().iter().any(|event| {
            event["type"] == "session_resumed" && event["trigger"] == "manual_slash"
        })
    );

    let output = std::fs::read_to_string(&path).unwrap();
    assert!(output.contains("session_resumed"));
    assert!(!output.contains("/private/"));
}

#[test]
fn headless_slash_resume_and_compact_are_real_owner_commands() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let input = concat!(
        "{\"type\":\"command\",\"id\":\"r\",\"text\":\"/resume 0\"}\n",
        "{\"type\":\"command\",\"id\":\"c\",\"text\":\"/compact\"}\n",
        "{\"type\":\"shutdown\",\"id\":\"q\"}\n",
    );
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(input.as_bytes()), &mut output).unwrap();
    let records = json_lines(&output);
    let resume = records.iter().find(|value| value["id"] == "r").unwrap();
    assert_eq!(resume["success"], true);
    assert_eq!(resume["data"]["durable"], true);
    let compact = records.iter().find(|value| value["id"] == "c").unwrap();
    assert_eq!(compact["success"], true);
    assert_eq!(compact["data"]["durable"], true);
    assert!(agent.session().events().iter().any(|event| {
        event["type"] == "context_compaction_skipped" || event["type"] == "context_compacted"
    }));
}

#[test]
fn tui_resume_restores_cleared_transcript_and_compact_reports_status() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let mut session = SessionStore::open(&path).unwrap();
    session
        .append_turn(Turn::new("user-1", TurnRole::User, "restore me"))
        .unwrap();
    let mut agent = Agent::with_echo(session);
    let mut state = TuiState::default();
    state.push_message(MessageRole::User, "visible before clear");
    dispatch_slash_command(SlashCommand::Clear, &mut state, Some(&mut agent));
    assert!(!state.messages().any(|message| message.text == "restore me"));
    assert_eq!(
        dispatch_slash_command(
            SlashCommand::Resume { sequence: None },
            &mut state,
            Some(&mut agent),
        ),
        SlashDispatchAction::Continue
    );
    assert!(state.messages().any(|message| message.text == "restore me"));
    assert!(
        state
            .messages()
            .any(|message| message.role == MessageRole::System
                && message.text.starts_with("resume:"))
    );
    dispatch_slash_command(SlashCommand::Compact, &mut state, Some(&mut agent));
    assert!(state.messages().any(|message| {
        message.role == MessageRole::System && message.text.starts_with("compact:")
    }));
}

#[test]
fn compact_checkpoint_is_durable_and_reconstructs_after_restart() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let mut session = SessionStore::open(&path).unwrap();
    for index in 0..20 {
        session
            .append_turn(Turn::new(
                format!("turn-{index}"),
                if index % 2 == 0 {
                    TurnRole::User
                } else {
                    TurnRole::Assistant
                },
                format!("{index}:{}", "x".repeat(500)),
            ))
            .unwrap();
    }
    let mut agent = Agent::with_echo(session);
    agent.set_context_budget(ContextBudget {
        max_tokens: 1_500,
        reserved_output_tokens: 300,
    });
    let report = agent.compact_context().unwrap();
    assert!(report.compacted);
    let checkpoint = report.checkpoint.clone().unwrap();
    assert!(
        agent
            .session()
            .events()
            .iter()
            .any(|event| event["trigger"] == "manual_slash")
    );
    let restored = restore_checkpoint(agent.history(), &checkpoint).unwrap();
    assert_eq!(restored.len(), report.prepared_turns);

    // A fresh agent sees only the durable journal and still reconstructs the
    // same checkpoint path when its first provider request is prepared.
    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut reopened = Agent::new(
        SessionStore::open(&path).unwrap(),
        Box::new(CaptureBackend(captured.clone())),
    );
    reopened.set_context_budget(ContextBudget {
        max_tokens: 1_500,
        reserved_output_tokens: 300,
    });
    assert!(reopened.session().events().iter().any(|event| {
        event["type"] == "context_compacted" && event["trigger"] == "manual_slash"
    }));
    reopened
        .process(TurnInputRequest::new("after restart"))
        .unwrap();
    let ids = captured.lock().unwrap().clone();
    assert!(ids.iter().any(|id| id.starts_with("context-")));
    assert!(ids.iter().any(|id| id == "turn-19"));
    assert!(!ids.iter().any(|id| id == "turn-0"));
}

#[test]
fn resume_rejects_future_sequence_without_mutating_journal() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&path).unwrap());
    let before = std::fs::read_to_string(&path).unwrap();
    let error = resume_session_view(&mut agent, Some(u64::MAX)).unwrap_err();
    assert!(error.contains("ahead"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn resume_projection_stays_bounded_and_exposes_a_continuation_cursor() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let mut session = SessionStore::open(&path).unwrap();
    for index in 0..200 {
        session
            .append_event(serde_json::json!({
                "type": "large-progress",
                "index": index,
                "payload": "x".repeat(9 * 1024),
            }))
            .unwrap();
    }
    let mut agent = Agent::with_echo(session);
    let value = resume_session_view(&mut agent, Some(0)).unwrap();
    assert_eq!(value["truncated"], true);
    assert!(value["records"].as_array().unwrap().len() <= 128);
    assert!(value["replay_cursor"].as_u64().unwrap() > 0);
    assert!(serde_json::to_vec(&value).unwrap().len() <= 160 * 1024);
}
