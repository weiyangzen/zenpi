use tempfile::tempdir;
use zenpi::approval::ApprovalMode;
use zenpi::core::{Agent, Turn, TurnRole};
use zenpi::session::SessionStore;
use zenpi::slash::{
    ApproveDecision, BlueprintAction, InputRoute, SessionAction, SlashCommand, SlashError,
    SlashRoute, complete, help, parse, route_input, spec,
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
    assert_eq!(parse("/models").unwrap(), Some(SlashCommand::Models));
    assert_eq!(parse("/doctor").unwrap(), Some(SlashCommand::Doctor));
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
    assert_eq!(
        parse("/yolo").unwrap(),
        Some(SlashCommand::Yolo { enabled: true })
    );
    assert_eq!(
        parse("/yolo off").unwrap(),
        Some(SlashCommand::Yolo { enabled: false })
    );
    assert_eq!(
        parse("/approval never").unwrap(),
        Some(SlashCommand::Approval {
            mode: "never".into()
        })
    );
    assert!(parse("/approval invalid").is_err());
    assert_eq!(
        parse("/project open api").unwrap(),
        Some(SlashCommand::Project {
            action: zenpi::slash::ProjectAction::Open { name: "api".into() }
        })
    );
    assert_eq!(
        parse("/review").unwrap(),
        Some(SlashCommand::Diff { path: None })
    );
    assert_eq!(
        parse("/review src/lib.rs").unwrap(),
        Some(SlashCommand::Diff {
            path: Some("src/lib.rs".into())
        })
    );
}

#[test]
fn every_command_spec_has_help_and_completion_entry() {
    for command in zenpi::slash::COMMAND_SPECS {
        assert!(
            spec(command.name).is_some(),
            "missing spec for {}",
            command.name
        );
        assert!(
            help(Some(command.name)).is_some(),
            "missing help for {}",
            command.name
        );
        assert!(!command.name.is_empty());
    }
}

#[test]
fn slash_help_explains_goal_boundary_and_posture_controls() {
    let goal_help = help(Some("goal")).unwrap();
    assert!(goal_help.contains("separate host primitive"));
    assert!(help(Some("yolo")).unwrap().contains("/yolo"));
    assert!(help(Some("approval")).unwrap().contains("ask|always|never"));
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
fn recovery_commands_require_explicit_confirmation() {
    use zenpi::slash::RecoveryAction;
    assert_eq!(
        parse("/recovery").unwrap(),
        Some(SlashCommand::Recovery {
            action: RecoveryAction::Inspect,
        })
    );
    assert_eq!(
        parse("/recovery retry operation-1 --yes").unwrap(),
        Some(SlashCommand::Recovery {
            action: RecoveryAction::Retry {
                operation_id: "operation-1".into()
            },
        })
    );
    assert!(spec("recovery").is_some());
    for invalid in [
        "/recovery retry operation-1",
        "/recovery abandon operation-1",
        "/recovery retry operation-1 --yes extra",
        "/recovery abandon '' --yes",
    ] {
        assert!(parse(invalid).is_err(), "{invalid}");
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
    assert!(matches!(
        parse("/models extra").unwrap_err(),
        SlashError::UnexpectedArgument { command: "models" }
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
    assert!(help(Some("doctor")).unwrap().contains("redacted"));
    assert!(complete("/mo").contains(&"models"));
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
fn route_input_classifies_explicit_user_shell_before_provider() {
    assert_eq!(
        route_input("  !echo 'hello' | wc -c  \n").unwrap(),
        InputRoute::UserShell("!echo 'hello' | wc -c".into())
    );
    assert_eq!(route_input("!").unwrap(), InputRoute::UserShell("!".into()));
    assert!(matches!(
        route_input("!!echo hidden"),
        Err(SlashError::UnsupportedUserShellExtension)
    ));
    assert!(matches!(
        route_input("!echo \u{0}"),
        Err(SlashError::UserShellControlCharacter)
    ));
    assert!(matches!(
        route_input(&format!(
            "!{}",
            "x".repeat(zenpi::slash::MAX_USER_SHELL_INPUT_BYTES)
        )),
        Err(SlashError::UserShellTooLong)
    ));
    // Shell-looking ordinary prompt text remains a prompt when it does not
    // begin with the explicit bang escape.
    assert_eq!(
        route_input("please run !echo hi").unwrap(),
        InputRoute::Prompt("please run !echo hi".into())
    );
}

#[test]
fn common_workflow_commands_have_typed_arguments_and_metadata() {
    assert_eq!(
        parse("/plan sketch a safe migration").unwrap(),
        Some(SlashCommand::Plan {
            instruction: Some("sketch a safe migration".into()),
        })
    );
    assert_eq!(
        parse("/blueprint put target/blueprint.json").unwrap(),
        Some(SlashCommand::Blueprint {
            action: BlueprintAction::Put {
                path: "target/blueprint.json".into(),
            },
        })
    );
    assert_eq!(
        parse("/goal put target/goal.json").unwrap(),
        Some(SlashCommand::GoalPut {
            path: "target/goal.json".into(),
        })
    );
    assert_eq!(
        parse("/learn put target/learn.json").unwrap(),
        Some(SlashCommand::LearnPut {
            path: "target/learn.json".into(),
        })
    );
    assert_eq!(
        parse("/session fork source.jsonl fork.jsonl").unwrap(),
        Some(SlashCommand::Session {
            action: SessionAction::Fork {
                source: "source.jsonl".into(),
                destination: "fork.jsonl".into(),
            },
        })
    );
    assert_eq!(
        parse("/session export source.jsonl 'tmp/session.jsonl'").unwrap(),
        Some(SlashCommand::Session {
            action: SessionAction::Export {
                source: "source.jsonl".into(),
                destination: "tmp/session.jsonl".into(),
            },
        })
    );
    assert_eq!(
        parse("/resume 42").unwrap(),
        Some(SlashCommand::Resume { sequence: Some(42) })
    );
    assert_eq!(parse("/compact").unwrap(), Some(SlashCommand::Compact));
    assert_eq!(
        parse("/session retire-mailbox inactive.jsonl --yes").unwrap(),
        Some(SlashCommand::Session {
            action: SessionAction::RetireMailbox {
                path: "inactive.jsonl".into(),
                confirmed: true,
            },
        })
    );
    assert_eq!(
        parse("/diff src/main.rs").unwrap(),
        Some(SlashCommand::Diff {
            path: Some("src/main.rs".into()),
        })
    );
    assert_eq!(
        parse("/attach assets/input.png").unwrap(),
        Some(SlashCommand::Attach {
            path: "assets/input.png".into(),
        })
    );
    assert_eq!(
        parse("/approve req-1 always").unwrap(),
        Some(SlashCommand::Approve {
            id: "req-1".into(),
            decision: ApproveDecision::Always,
        })
    );
    assert_eq!(
        parse("/approve req-1 once").unwrap(),
        Some(SlashCommand::Approve {
            id: "req-1".into(),
            decision: ApproveDecision::Once,
        })
    );
    assert_eq!(
        parse("/approve req-1 deny").unwrap(),
        Some(SlashCommand::Approve {
            id: "req-1".into(),
            decision: ApproveDecision::Deny,
        })
    );
    for command in [
        "plan", "session", "resume", "compact", "diff", "attach", "approve",
    ] {
        assert!(spec(command).is_some(), "missing /{command} metadata");
    }
}

#[test]
fn common_workflow_commands_fail_closed_on_invalid_arguments() {
    for input in [
        "/session retire-mailbox",
        "/session retire-mailbox inactive.jsonl",
        "/session retire-mailbox inactive.jsonl --yes extra",
    ] {
        assert!(
            parse(input).is_err(),
            "unexpected retirement acceptance: {input}"
        );
    }
    assert!(matches!(
        parse("/resume nope").unwrap_err(),
        SlashError::InvalidResumeSequence
    ));
    assert!(matches!(
        parse("/session open").unwrap_err(),
        SlashError::MissingSessionPath { .. }
    ));
    assert!(matches!(
        parse("/session what").unwrap_err(),
        SlashError::UnknownSessionAction { .. }
    ));
    assert!(matches!(
        parse("/attach").unwrap_err(),
        SlashError::MissingArgument { .. }
    ));
    assert!(matches!(
        parse("/approve req-1 maybe").unwrap_err(),
        SlashError::InvalidApproveDecision
    ));
    assert!(matches!(
        parse("/approve req-1 once extra").unwrap_err(),
        SlashError::UnexpectedArgument { .. }
    ));
    assert!(matches!(
        parse("/blueprint put").unwrap_err(),
        SlashError::MissingArgument { .. }
    ));
    assert!(matches!(
        parse("/goal put target/goal.json extra").unwrap_err(),
        SlashError::UnexpectedArgument { .. }
    ));
    assert!(matches!(
        parse("/learn put").unwrap_err(),
        SlashError::MissingArgument { .. }
    ));
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
    assert!(state.messages().any(|message| {
        message
            .text
            .contains("goal creation/execution requires an external b3ehive owner")
    }));
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

#[test]
fn session_open_replaces_tui_transcript_from_existing_journal() {
    let directory = tempdir().unwrap();
    let source_path = directory.path().join("source.jsonl");
    let mut source = SessionStore::open(&source_path).unwrap();
    source
        .append_turn(Turn::new("source-user", TurnRole::User, "remember this"))
        .unwrap();
    source
        .append_turn(Turn::with_parent(
            "source-assistant",
            "source-user",
            TurnRole::Assistant,
            "loaded from session",
        ))
        .unwrap();

    let active = SessionStore::open(directory.path().join("active.jsonl")).unwrap();
    let mut agent = Agent::with_echo(active);
    let mut state = TuiState::default();
    state.push_message(zenpi::tui::MessageRole::User, "old transcript");

    let action = dispatch_slash_command(
        SlashCommand::Session {
            action: SessionAction::Open {
                path: source_path.display().to_string(),
            },
        },
        &mut state,
        Some(&mut agent),
    );

    assert_eq!(action, SlashDispatchAction::Continue);
    assert_eq!(agent.session().path(), source_path.canonicalize().unwrap());
    assert!(
        agent
            .history()
            .iter()
            .any(|turn| turn.content == "loaded from session")
    );
    assert!(
        !state
            .messages()
            .any(|message| message.text == "old transcript")
    );
    assert!(
        state
            .messages()
            .any(|message| message.text.contains("loaded from session"))
    );
}

#[test]
fn session_open_rejects_missing_target_without_creating_a_journal() {
    let directory = tempdir().unwrap();
    let missing = directory.path().join("missing.jsonl");
    let active = SessionStore::open(directory.path().join("active.jsonl")).unwrap();
    let mut agent = Agent::with_echo(active);
    let mut state = TuiState::default();

    dispatch_slash_command(
        SlashCommand::Session {
            action: SessionAction::Open {
                path: missing.display().to_string(),
            },
        },
        &mut state,
        Some(&mut agent),
    );

    assert!(!missing.exists());
    assert!(
        state
            .messages()
            .any(|message| message.text.contains("session open failed"))
    );
    assert_eq!(
        agent.session().path(),
        directory.path().join("active.jsonl")
    );
}

#[test]
fn approval_slash_commands_mutate_configured_agent_policy() {
    let directory = tempdir().unwrap();
    let active = SessionStore::open(directory.path().join("active.jsonl")).unwrap();
    let mut agent = Agent::with_echo(active);
    agent.set_attachment_workspace(
        zenpi::tools::ToolContext::new(directory.path().to_path_buf()).unwrap(),
    );
    let mut state = TuiState::default();
    dispatch_slash_command(
        SlashCommand::Yolo { enabled: true },
        &mut state,
        Some(&mut agent),
    );
    assert_eq!(agent.approval_policy().unwrap().mode, ApprovalMode::Never);
    dispatch_slash_command(
        SlashCommand::Approval {
            mode: "always".into(),
        },
        &mut state,
        Some(&mut agent),
    );
    assert_eq!(agent.approval_policy().unwrap().mode, ApprovalMode::Always);
}
