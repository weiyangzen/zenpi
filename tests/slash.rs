use tempfile::tempdir;
use zenpi::core::Agent;
use zenpi::session::SessionStore;
use zenpi::slash::{
    BlueprintAction, InputRoute, SlashCommand, SlashError, SlashRoute, complete, help, parse,
    route_input, spec,
};
use zenpi::tui::{SlashDispatchAction, TuiState, dispatch_slash_command};

#[test]
fn core_slash_commands_parse_to_typed_values() {
    assert_eq!(
        parse("  /model gpt-test").unwrap(),
        Some(SlashCommand::Model {
            name: Some("gpt-test".into()),
        })
    );
    assert_eq!(
        parse("/blueprint validate \"Docs/Blueprint v2.md\"").unwrap(),
        Some(SlashCommand::Blueprint {
            action: BlueprintAction::Validate {
                path: Some("Docs/Blueprint v2.md".into()),
            },
        })
    );
    assert_eq!(
        parse("/learn src/core.rs").unwrap(),
        Some(SlashCommand::Learn {
            target: Some("src/core.rs".into()),
        })
    );
    assert!(parse("/goal ship it").unwrap().unwrap().is_first_class());
    assert!(!parse("/status").unwrap().unwrap().is_first_class());
}

#[test]
fn compete_and_loop_are_runtime_routes() {
    for input in ["/compete run-a", "/loop --budget 3"] {
        let command = parse(input).unwrap().unwrap();
        assert_eq!(command.route(), SlashRoute::Runtime);
        assert!(command.is_runtime());
    }
}

#[test]
fn malformed_commands_fail_closed() {
    assert_eq!(parse("/").unwrap_err(), SlashError::Empty);
    assert!(matches!(
        parse("/goal").unwrap_err(),
        SlashError::MissingArgument { .. }
    ));
    assert!(matches!(
        parse("/blueprint run").unwrap_err(),
        SlashError::MissingArgument { .. }
    ));
    assert!(matches!(
        parse("/does-not-exist").unwrap_err(),
        SlashError::UnknownCommand(_)
    ));
    assert_eq!(
        parse("/goal 'unfinished").unwrap_err(),
        SlashError::UnterminatedQuote
    );
}

#[test]
fn history_default_and_completion_are_bounded() {
    assert_eq!(
        parse("/history").unwrap(),
        Some(SlashCommand::History { limit: None })
    );
    assert_eq!(
        parse("/history 20").unwrap(),
        Some(SlashCommand::History { limit: Some(20) })
    );
    assert!(matches!(
        parse("/history 0").unwrap_err(),
        SlashError::InvalidHistoryLimit
    ));
    assert_eq!(complete("/go"), vec!["goal"]);
    assert!(complete("/").contains(&"blueprint"));
    assert_eq!(spec("/BP").map(|entry| entry.name), Some("blueprint"));
    assert!(help(None).unwrap().contains("/goal <instruction>"));
    assert!(help(Some("loop")).unwrap().contains("b3ehive runtime"));
    assert!(help(Some("missing")).is_none());
}

#[test]
fn oversized_slash_input_is_rejected_before_tokenization() {
    let input = format!("/goal {}", "x".repeat(zenpi::slash::MAX_SLASH_INPUT_BYTES));
    assert_eq!(parse(&input).unwrap_err(), SlashError::TooLong);
}

#[test]
fn typed_commands_round_trip_for_journal_events() {
    let command = parse("/blueprint open 'Docs/Blueprint v2.md'")
        .unwrap()
        .unwrap();
    let wire = serde_json::to_string(&command).unwrap();
    let decoded: SlashCommand = serde_json::from_str(&wire).unwrap();
    assert_eq!(decoded, command);
}

#[test]
fn route_input_keeps_slash_commands_out_of_prompt_path() {
    assert_eq!(
        route_input(" ordinary text ").unwrap(),
        InputRoute::Prompt(" ordinary text ".into())
    );
    assert!(matches!(
        route_input("/status").unwrap(),
        InputRoute::Slash(SlashCommand::Status)
    ));
    assert!(route_input("/unknown").is_err());
}

#[test]
fn local_dispatch_updates_view_without_creating_a_turn() {
    let directory = tempdir().unwrap();
    let session = SessionStore::open(directory.path().join("session.jsonl")).unwrap();
    let mut agent = Agent::with_echo(session);
    let mut state = TuiState::default();

    assert_eq!(
        dispatch_slash_command(
            SlashCommand::Model { name: None },
            &mut state,
            Some(&mut agent)
        ),
        SlashDispatchAction::Continue
    );
    assert_eq!(
        dispatch_slash_command(
            SlashCommand::Goal {
                instruction: "keep it bounded".into(),
            },
            &mut state,
            Some(&mut agent),
        ),
        SlashDispatchAction::Continue
    );
    assert_eq!(agent.history().len(), 0);
    assert!(
        state
            .messages()
            .any(|message| { message.text.contains("goal command acknowledged") })
    );
}

#[test]
fn dispatch_cancel_and_exit_are_control_actions() {
    let mut state = TuiState::default();
    assert_eq!(
        dispatch_slash_command(SlashCommand::Cancel, &mut state, None),
        SlashDispatchAction::Interrupt
    );
    assert_eq!(
        dispatch_slash_command(SlashCommand::Exit, &mut state, None),
        SlashDispatchAction::Quit
    );
}
