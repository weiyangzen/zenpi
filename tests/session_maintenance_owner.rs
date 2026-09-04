use std::{fs, io::Cursor, path::Path, thread, time::Duration};

use serde_json::Value;
use tempfile::tempdir;
use zenpi::{
    core::{Agent, Turn, TurnRole},
    headless::{run_headless, session_gc_view_at, session_maintenance_view},
    session::SessionStore,
    slash::{SessionAction, SessionGcPolicy, SlashCommand, SlashError, parse},
    tui::{MessageRole, SlashDispatchAction, TuiState, dispatch_slash_command},
};

fn seed_session(path: &Path) -> Vec<u8> {
    let mut session = SessionStore::open(path).unwrap();
    session
        .append_turn(Turn::new("source-user", TurnRole::User, "durable source"))
        .unwrap();
    drop(session);
    fs::read(path).unwrap()
}

fn response_lines(output: &[u8]) -> Vec<Value> {
    String::from_utf8(output.to_vec())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn session_maintenance_parser_requires_explicit_source_and_destination() {
    for (verb, expected) in [
        (
            "fork",
            SessionAction::Fork {
                source: "source journal.jsonl".into(),
                destination: "fork journal.jsonl".into(),
            },
        ),
        (
            "export",
            SessionAction::Export {
                source: "source journal.jsonl".into(),
                destination: "export journal.jsonl".into(),
            },
        ),
        (
            "import",
            SessionAction::Import {
                source: "source journal.jsonl".into(),
                destination: "import journal.jsonl".into(),
            },
        ),
    ] {
        let input = format!(
            "/session {verb} 'source journal.jsonl' '{} journal.jsonl'",
            verb
        );
        assert_eq!(
            parse(&input).unwrap(),
            Some(SlashCommand::Session { action: expected })
        );
        assert!(matches!(
            parse(&format!("/session {verb} source.jsonl")).unwrap_err(),
            SlashError::MissingSessionPath { .. }
        ));
        assert!(matches!(
            parse(&format!(
                "/session {verb} source.jsonl destination.jsonl extra"
            ))
            .unwrap_err(),
            SlashError::UnexpectedSessionArgument { .. }
        ));
    }

    // Existing navigation syntax remains unchanged alongside the typed pair.
    assert_eq!(
        parse("/session open source.jsonl").unwrap(),
        Some(SlashCommand::Session {
            action: SessionAction::Open {
                path: "source.jsonl".into(),
            },
        })
    );
    assert_eq!(
        parse("/session list").unwrap(),
        Some(SlashCommand::Session {
            action: SessionAction::List,
        })
    );
}

#[test]
fn fork_export_and_import_preserve_the_source_journal() {
    let directory = tempdir().unwrap();
    let source_path = directory.path().join("source.jsonl");
    let source_bytes = seed_session(&source_path);
    let source = SessionStore::open_existing(&source_path).unwrap();
    let source_id = source.session_id().to_owned();
    let source_turns = source.turns().to_vec();
    drop(source);

    let fork_path = directory.path().join("fork.jsonl");
    let export_path = directory.path().join("export.jsonl");
    let import_path = directory.path().join("import.jsonl");
    for action in [
        SessionAction::Fork {
            source: source_path.display().to_string(),
            destination: fork_path.display().to_string(),
        },
        SessionAction::Export {
            source: source_path.display().to_string(),
            destination: export_path.display().to_string(),
        },
        SessionAction::Import {
            source: source_path.display().to_string(),
            destination: import_path.display().to_string(),
        },
    ] {
        let value = session_maintenance_view(&action).unwrap();
        assert_eq!(value["accepted"], true);
        assert_eq!(value["durable"], true);
        assert_eq!(fs::read(&source_path).unwrap(), source_bytes);
    }

    let fork = SessionStore::open_existing(&fork_path).unwrap();
    assert_ne!(fork.session_id(), source_id);
    assert_eq!(fork.turns(), source_turns);

    for copied_path in [&export_path, &import_path] {
        assert_eq!(fs::read(copied_path).unwrap(), source_bytes);
        let copied = SessionStore::open_existing(copied_path).unwrap();
        assert_eq!(copied.session_id(), source_id);
        assert_eq!(copied.turns(), source_turns);
    }
}

#[test]
fn session_maintenance_rejects_overwrite_traversal_and_invalid_sources() {
    let directory = tempdir().unwrap();
    let source_path = directory.path().join("source.jsonl");
    let source_bytes = seed_session(&source_path);

    for (index, action) in ["fork", "export", "import"].into_iter().enumerate() {
        let destination = directory.path().join(format!("existing-{index}.jsonl"));
        fs::write(&destination, b"do not overwrite").unwrap();
        let action = match action {
            "fork" => SessionAction::Fork {
                source: source_path.display().to_string(),
                destination: destination.display().to_string(),
            },
            "export" => SessionAction::Export {
                source: source_path.display().to_string(),
                destination: destination.display().to_string(),
            },
            "import" => SessionAction::Import {
                source: source_path.display().to_string(),
                destination: destination.display().to_string(),
            },
            _ => unreachable!(),
        };
        let error = session_maintenance_view(&action).unwrap_err();
        assert!(error.contains("already exists"));
        assert_eq!(fs::read(&destination).unwrap(), b"do not overwrite");
    }

    let traversal_destination = directory.path().join("nested/../escaped.jsonl");
    let error = session_maintenance_view(&SessionAction::Export {
        source: source_path.display().to_string(),
        destination: traversal_destination.display().to_string(),
    })
    .unwrap_err();
    assert!(error.contains("parent traversal"));
    assert!(!directory.path().join("escaped.jsonl").exists());

    let ordinary_source = directory.path().join("ordinary.txt");
    fs::write(&ordinary_source, b"not a zenpi journal").unwrap();
    let invalid_destination = directory.path().join("from-invalid.jsonl");
    assert!(
        session_maintenance_view(&SessionAction::Import {
            source: ordinary_source.display().to_string(),
            destination: invalid_destination.display().to_string(),
        })
        .is_err()
    );
    assert!(!invalid_destination.exists());
    assert_eq!(fs::read(&ordinary_source).unwrap(), b"not a zenpi journal");
    assert_eq!(fs::read(&source_path).unwrap(), source_bytes);

    #[cfg(unix)]
    {
        let target = directory.path().join("symlink-target.txt");
        let destination = directory.path().join("destination-link.jsonl");
        fs::write(&target, b"link target stays intact").unwrap();
        std::os::unix::fs::symlink(&target, &destination).unwrap();
        assert!(
            session_maintenance_view(&SessionAction::Export {
                source: source_path.display().to_string(),
                destination: destination.display().to_string(),
            })
            .unwrap_err()
            .contains("symbolic link")
        );
        assert_eq!(fs::read(&target).unwrap(), b"link target stays intact");
    }
}

#[test]
fn headless_and_tui_dispatch_execute_typed_session_maintenance_locally() {
    let directory = tempdir().unwrap();
    let source_path = directory.path().join("source.jsonl");
    let source_bytes = seed_session(&source_path);

    let active_path = directory.path().join("active.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&active_path).unwrap());
    let exported_path = directory.path().join("headless-export.jsonl");
    let requests = [
        serde_json::json!({
            "type": "command",
            "id": "export",
            "text": format!(
                "/session export {} {}",
                source_path.display(),
                exported_path.display()
            ),
        }),
        serde_json::json!({"type": "shutdown", "id": "shutdown"}),
    ]
    .into_iter()
    .map(|request| serde_json::to_string(&request).unwrap())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let mut output = Vec::new();
    run_headless(&mut agent, Cursor::new(requests.into_bytes()), &mut output).unwrap();
    let response = response_lines(&output)
        .into_iter()
        .find(|value| value["id"] == "export")
        .unwrap();
    assert_eq!(response["success"], true);
    assert_eq!(response["data"]["action"], "export");
    assert_eq!(response["data"]["route"], "local");
    assert_eq!(fs::read(&exported_path).unwrap(), source_bytes);

    let tui_active_path = directory.path().join("tui-active.jsonl");
    let mut tui_agent = Agent::with_echo(SessionStore::open(&tui_active_path).unwrap());
    let tui_fork_path = directory.path().join("tui-fork.jsonl");
    let mut state = TuiState::default();
    let result = dispatch_slash_command(
        SlashCommand::Session {
            action: SessionAction::Fork {
                source: source_path.display().to_string(),
                destination: tui_fork_path.display().to_string(),
            },
        },
        &mut state,
        Some(&mut tui_agent),
    );
    assert_eq!(result, SlashDispatchAction::Continue);
    assert!(tui_fork_path.exists());
    assert_eq!(tui_agent.session().path(), tui_active_path);
    assert!(state.messages().any(|message| {
        message.role == MessageRole::System && message.text.starts_with("session fork:")
    }));
    assert_eq!(fs::read(&source_path).unwrap(), source_bytes);
}

#[test]
fn gc_owner_is_real_bounded_and_never_removes_active_or_foreign_files() {
    let directory = tempdir().unwrap();
    let sessions = directory.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let active_path = directory.path().join("active.jsonl");
    let active = Agent::with_echo(SessionStore::open(&active_path).unwrap());
    let old_path = sessions.join("old.jsonl");
    SessionStore::open(&old_path).unwrap();
    thread::sleep(Duration::from_millis(10));
    let newest_path = sessions.join("newest.jsonl");
    SessionStore::open(&newest_path).unwrap();
    let foreign_path = sessions.join("foreign.jsonl");
    fs::write(&foreign_path, b"{\"kind\":\"not-a-session\"}\n").unwrap();

    let policy = SessionGcPolicy {
        retain_newest: 1,
        older_than_seconds: 0,
        confirm: true,
    };
    let value = session_gc_view_at(&active, &policy, &sessions).unwrap();
    assert_eq!(value["accepted"], true);
    assert_eq!(value["action"], "gc");
    assert_eq!(value["confirmed"], true);
    assert_eq!(value["removed_count"], 1);
    assert!(value["skipped_unowned"].as_u64().unwrap() >= 1);
    assert!(!old_path.exists());
    assert!(newest_path.exists());
    assert!(foreign_path.exists());

    // A direct owner invocation with the active journal inside the managed
    // directory fails before deleting any other candidate.
    let active_in_dir = sessions.join("active.jsonl");
    let active_agent = Agent::with_echo(SessionStore::open(&active_in_dir).unwrap());
    let other = sessions.join("other.jsonl");
    SessionStore::open(&other).unwrap();
    let error = session_gc_view_at(&active_agent, &policy, &sessions).unwrap_err();
    assert!(error.contains("active session"));
    assert!(active_in_dir.exists());
    assert!(other.exists());
}
