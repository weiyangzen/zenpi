mod support;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Map, Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::tempdir;
use zenpi::{
    backend::{Backend, BackendError, Completion, CompletionRequest, Usage},
    core::{Agent, Turn, TurnInputRequest, TurnRole},
    session::SessionStore,
    tools::{
        SideEffectPolicy, Tool, ToolCall, ToolContext, ToolDefinition, ToolError, ToolRegistry,
    },
    trajectory::{CacheSeverity, RecordKind, RecordStatus, Trajectory},
    tui::{TrajectoryLevel, TuiAction, TuiState},
};

fn turn(id: &str, parent: Option<&str>, role: TurnRole, content: &str, at: u64) -> Turn {
    let mut turn = Turn::new(id, role, content);
    turn.parent_id = parent.map(str::to_owned);
    turn.created_at_ms = at;
    turn
}

fn request(at: u64, instructions: &str, context: usize, shared: Option<usize>) -> Value {
    let mut value = json!({
        "started_ms": at, "first_token_ms": at + 200, "completed_ms": at + 1200,
        "instructions_digest": instructions, "tools_digest": "t1",
        "context_turns": context,
    });
    if let Some(shared) = shared {
        value["shared_turns"] = json!(shared);
        value["previous_context_turns"] = json!(context - 2);
    }
    value
}

fn usage(input: u64, output: u64, cached: u64) -> Value {
    json!({"input_tokens": input, "output_tokens": output, "total_tokens": input + output,
           "cache_read_tokens": cached})
}

/// Two turns: a tool step plus answer, then a turn whose second request
/// reports a cache miss although its prefix was stable.
fn sample_history() -> Vec<Turn> {
    let mut call = turn(
        "assistant-tool-call-1",
        Some("u1"),
        TurnRole::Assistant,
        "",
        1_000,
    );
    call.metadata = Some(json!({
        "tool_calls": [{"id": "c1", "name": "read_file", "arguments": {"path": "config.yaml"}}],
        "usage": usage(3000, 40, 0),
        "request": request(1_000, "i1", 1, None),
    }));
    let mut result = turn(
        "tool-1",
        Some("u1"),
        TurnRole::Tool,
        "{\"text\":\"key: value\"}",
        2_500,
    );
    result.metadata =
        Some(json!({"tool_call_id": "c1", "tool_name": "read_file", "outcome": "succeeded"}));
    let mut answer = turn(
        "assistant-1",
        Some("u1"),
        TurnRole::Assistant,
        "Config read.",
        3_000,
    );
    answer.metadata = Some(json!({
        "usage": usage(3100, 20, 2900),
        "request": request(3_000, "i1", 3, Some(1)),
        "model": "m",
    }));
    let mut second = turn(
        "assistant-2",
        Some("u2"),
        TurnRole::Assistant,
        "Done.",
        9_000,
    );
    second.metadata = Some(json!({
        "usage": usage(3300, 10, 0),
        "request": request(9_000, "i1", 5, Some(3)),
    }));
    vec![
        turn("u1", None, TurnRole::User, "Read the config", 900),
        call,
        result,
        answer,
        turn("u2", None, TurnRole::User, "Summarize it", 8_900),
        second,
    ]
}

#[test]
fn projection_groups_turns_and_flags_cache_misses() {
    let trajectory = Trajectory::from_turns(&sample_history(), None);
    assert_eq!(trajectory.turns.len(), 2);
    let kinds: Vec<_> = trajectory.records.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        [
            RecordKind::User,
            RecordKind::ToolCall,
            RecordKind::ToolResult,
            RecordKind::Assistant,
            RecordKind::User,
            RecordKind::Assistant
        ]
    );
    assert_eq!(trajectory.records[1].summary, "read_file config.yaml");
    assert_eq!(
        trajectory.records[2].tool_name.as_deref(),
        Some("read_file")
    );
    assert_eq!(trajectory.records[2].summary, "read_file key: value");
    let envelope = Trajectory::from_turns(
        &[turn(
            "tool-x",
            None,
            TurnRole::Tool,
            r#"{"call_id":"c","output":{"content":"name: zenpi\nfeature: x","path":"a"},"status":"success","tool":"read_file"}"#,
            1,
        )],
        None,
    );
    assert_eq!(
        envelope.records[0].summary,
        "result success · name: zenpi feature: x"
    );
    let verdicts: Vec<_> = trajectory
        .records
        .iter()
        .filter_map(|record| record.cache.as_ref().map(|verdict| verdict.severity))
        .collect();
    assert_eq!(
        verdicts,
        [CacheSeverity::Ok, CacheSeverity::Ok, CacheSeverity::Miss]
    );
    assert_eq!(trajectory.cache_issues(), vec![5]);
    let stats = &trajectory.stats;
    assert_eq!(stats.requests, 3);
    assert_eq!(stats.cache_read_tokens, 2900);
    assert_eq!(stats.avg_ttft_ms, Some(200));
    assert_eq!(stats.misses, 1);
    assert!(stats.cache_hit_ratio().unwrap() > 0.3);
    assert_eq!(trajectory.search("CONFIG"), vec![0, 1, 3]);
    let summary = trajectory.summary_json();
    assert_eq!(summary["kv_cache"][0]["severity"], "miss");
    assert_eq!(summary["kv_cache"][0]["index"], 6);
}

#[test]
fn prefix_changes_are_classified() {
    let mut history = sample_history();
    // A changed system prompt invalidates the whole cache.
    history[3].metadata.as_mut().unwrap()["request"]["instructions_digest"] = json!("i2");
    let trajectory = Trajectory::from_turns(&history, None);
    let verdict = trajectory.records[3].cache.clone().unwrap();
    assert_eq!(verdict.severity, CacheSeverity::Warn);
    assert!(verdict.message.contains("instructions changed"));

    // A rewritten earlier turn is a warning, but expected right after compaction.
    let mut history = sample_history();
    history[5].metadata.as_mut().unwrap()["request"]["shared_turns"] = json!(1);
    let trajectory = Trajectory::from_turns(&history, None);
    assert_eq!(
        trajectory.records[5].cache.as_ref().unwrap().severity,
        CacheSeverity::Warn
    );
    history.insert(
        5,
        turn("context-1", None, TurnRole::System, "summary", 8_950),
    );
    history[4].parent_id = None;
    let trajectory = Trajectory::from_turns(&history, None);
    let compacted = trajectory.records.last().unwrap().cache.clone().unwrap();
    assert_eq!(compacted.severity, CacheSeverity::Info);
}

#[test]
fn unfinished_turns_are_errors_unless_running() {
    let history = vec![
        turn("u1", None, TurnRole::User, "first", 1),
        turn("assistant-1", Some("u1"), TurnRole::Assistant, "ok", 2),
        turn("u2", None, TurnRole::User, "second", 3),
    ];
    let failed = Trajectory::from_turns(&history, None);
    assert_eq!(failed.records[2].status, RecordStatus::Error);
    let running = Trajectory::from_turns(&history, Some("u2"));
    assert_eq!(running.records[2].status, RecordStatus::InProgress);
    assert!(running.in_progress());
}

struct CachingToolBackend {
    calls: AtomicUsize,
}

impl Backend for CachingToolBackend {
    crate::support::controlled_local_backend!();
    fn complete(&self, _: CompletionRequest<'_>) -> Result<Completion, BackendError> {
        let mut completion = if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            let mut completion = Completion::text("");
            completion.tool_calls = vec![ToolCall {
                id: "call-1".into(),
                name: "constant".into(),
                arguments: json!({}),
            }];
            completion.usage = Some(Usage {
                cache_write_tokens: 2000,
                ..Usage::new(2048, 16, 2064)
            });
            completion
        } else {
            let mut completion = Completion::text("tool result incorporated");
            completion.usage = Some(Usage {
                cache_read_tokens: 2000,
                ..Usage::new(2100, 8, 2108)
            });
            completion
        };
        completion.model = Some("scripted".into());
        Ok(completion)
    }
}

struct ConstantTool;

impl Tool for ConstantTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "constant".into(),
            description: "Return a deterministic value for testing.".into(),
            input_schema: json!({"type": "object", "additionalProperties": false}),
            side_effect: zenpi::tools::ToolSideEffect::ReadOnly,
        }
    }

    fn invoke(&self, _: &ToolContext, _: &Map<String, Value>) -> Result<Value, ToolError> {
        Ok(json!({"value": 42}))
    }
}

#[test]
fn core_persists_per_request_usage_timing_and_prefix_fingerprint() {
    let workspace = tempdir().unwrap();
    let path = workspace.path().join("trajectory.jsonl");
    let mut registry = ToolRegistry::new();
    registry.register(ConstantTool).unwrap();
    let mut agent = Agent::new(
        SessionStore::open(&path).unwrap(),
        Box::new(CachingToolBackend {
            calls: AtomicUsize::new(0),
        }),
    );
    agent.set_tools(
        registry,
        ToolContext::new(workspace.path()).unwrap(),
        SideEffectPolicy::read_only(),
    );
    agent
        .process(TurnInputRequest::new("use the tool"))
        .unwrap();

    let history = SessionStore::open_existing(&path).unwrap().turns().to_vec();
    let call = history
        .iter()
        .find(|turn| turn.id.starts_with("assistant-tool-call"))
        .unwrap();
    let metadata = call.metadata.as_ref().unwrap();
    assert_eq!(metadata["usage"]["cache_write_tokens"], 2000);
    assert_eq!(metadata["request"]["context_turns"], 1);
    assert!(metadata["request"].get("shared_turns").is_none());
    let answer = history.last().unwrap().metadata.as_ref().unwrap();
    assert_eq!(answer["usage"]["cache_read_tokens"], 2000);
    assert_eq!(answer["request"]["context_turns"], 3);
    assert_eq!(answer["request"]["shared_turns"], 1);
    assert_eq!(answer["request"]["previous_context_turns"], 1);
    assert_eq!(
        answer["request"]["instructions_digest"],
        metadata["request"]["instructions_digest"]
    );

    // The request record never reaches the provider, so it must not count
    // toward the context estimate that drives compaction.
    let mut stripped = history.clone();
    for turn in &mut stripped {
        if let Some(metadata) = turn.metadata.as_mut().and_then(Value::as_object_mut) {
            metadata.remove("request");
        }
    }
    assert_eq!(
        zenpi::context::estimate_tokens(&history),
        zenpi::context::estimate_tokens(&stripped)
    );

    let trajectory = Trajectory::from_turns(&history, None);
    assert_eq!(trajectory.stats.requests, 2);
    assert_eq!(trajectory.stats.kv_issues(), 0);
    let last = trajectory.records.last().unwrap();
    assert_eq!(last.cache.as_ref().unwrap().severity, CacheSeverity::Ok);
    assert!(last.cache.as_ref().unwrap().message.contains("95%"));
}

fn key(state: &mut TuiState, code: KeyCode) -> TuiAction {
    state.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn render(state: &mut TuiState, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| state.render_bentobox(frame, "zenpi"))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn alt_t_opens_records_and_walks_to_the_inspector_and_back() {
    let mut state = TuiState::default();
    state.set_input("draft untouched");
    state.set_trajectory_history(sample_history());
    state.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::ALT));
    assert_eq!(state.trajectory_level(), Some(TrajectoryLevel::Records));
    let screen = render(&mut state, 140, 30);
    assert!(screen.contains("Trajectory"), "{screen}");
    assert!(screen.contains("KV ✗ 1"), "{screen}");
    assert!(screen.contains("Turn 2"), "{screen}");
    assert!(screen.contains("read_file config.yaml"), "{screen}");
    assert!(screen.contains("94%"), "{screen}");
    assert!(screen.contains("Σ 3 req"), "{screen}");

    // The cursor follows the tail; Enter opens the last record.
    key(&mut state, KeyCode::Enter);
    assert_eq!(state.trajectory_level(), Some(TrajectoryLevel::Inspector));
    assert!(render(&mut state, 140, 30).contains("Agents › master › Turn 2 › #6"));
    key(&mut state, KeyCode::Tab);
    let metrics = render(&mut state, 140, 30);
    assert!(metrics.contains("Verdict: miss"), "{metrics}");
    assert!(metrics.contains("TTFT 0.20s"), "{metrics}");
    assert!(matches!(
        key(&mut state, KeyCode::Char('y')),
        TuiAction::Copy(text) if text.contains("Verdict: miss")
    ));
    assert!(state.trajectory_open());

    key(&mut state, KeyCode::Esc);
    assert_eq!(state.trajectory_level(), Some(TrajectoryLevel::Records));
    key(&mut state, KeyCode::Esc);
    assert_eq!(state.trajectory_level(), Some(TrajectoryLevel::Agents));
    assert!(render(&mut state, 140, 30).contains("master"));
    key(&mut state, KeyCode::Esc);
    assert!(!state.trajectory_open());
    assert_eq!(state.input(), "draft untouched");
}

#[test]
fn search_and_cache_issue_jumps_move_the_cursor() {
    let mut state = TuiState::default();
    state.set_trajectory_history(sample_history());
    state.open_trajectory_viewer();
    key(&mut state, KeyCode::Home);
    key(&mut state, KeyCode::Char('c'));
    key(&mut state, KeyCode::Enter);
    assert!(render(&mut state, 120, 24).contains("#6"));
    key(&mut state, KeyCode::Esc);

    key(&mut state, KeyCode::Char('/'));
    for character in "summarize".chars() {
        key(&mut state, KeyCode::Char(character));
    }
    assert!(render(&mut state, 120, 24).contains("/summarize▏"));
    key(&mut state, KeyCode::Enter);
    let screen = render(&mut state, 120, 24);
    assert!(screen.contains("/summarize 1/1"), "{screen}");
    assert!(screen.contains("Agents › master › Turn 2 › #5"), "{screen}");

    // Space folds the current turn; its records disappear.
    key(&mut state, KeyCode::Char(' '));
    let folded = render(&mut state, 120, 24);
    assert!(folded.contains("▸ Turn 2"), "{folded}");
    assert!(!folded.contains("Summarize it"), "{folded}");
}

#[test]
fn slash_trajectory_opens_the_viewer_and_live_history_refreshes_it() {
    let mut state = TuiState::default();
    let command = zenpi::slash::parse("/trajectory").unwrap().unwrap();
    zenpi::tui::dispatch_slash_command(command, &mut state, None);
    assert!(state.trajectory_open());
    assert!(render(&mut state, 100, 20).contains("No records yet"));
    key(&mut state, KeyCode::Char('q'));

    state.set_trajectory_history(sample_history()[..1].to_vec());
    state.open_trajectory_viewer();
    assert!(!render(&mut state, 100, 20).contains("Turn 2"));
    state.set_trajectory_history(sample_history());
    assert!(render(&mut state, 100, 20).contains("Turn 2"));
}
