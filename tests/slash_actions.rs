use std::process::Command;

use tempfile::tempdir;
use zenpi::{
    backend::{AttachmentKind, InputAttachment},
    core::{Agent, TurnInputRequest},
    headless::run_headless,
    session::SessionStore,
    slash_actions::{self, MAX_SLASH_DIFF_BYTES},
    tools::ToolContext,
    tui::{MessageRole, SlashDispatchAction, TuiState, dispatch_slash_command},
};

fn git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("git must be installed for slash action tests");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn diff_owner_reports_tracked_and_untracked_changes_with_bounds() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "zenpi@example.test"]);
    git(root, &["config", "user.name", "zenpi test"]);
    std::fs::write(root.join("tracked.txt"), "before\n").unwrap();
    git(root, &["add", "tracked.txt"]);
    git(root, &["commit", "-qm", "initial"]);
    std::fs::write(root.join("tracked.txt"), "before\nafter\n").unwrap();

    let tracked = slash_actions::diff_value_at(root, Some("tracked.txt")).unwrap();
    assert_eq!(tracked["accepted"], true);
    assert_eq!(tracked["changed"], true);
    assert!(tracked["diff"].as_str().unwrap().contains("+after"));
    assert_eq!(tracked["path"], "tracked.txt");

    std::fs::write(root.join("new.txt"), "new file\n").unwrap();
    let untracked = slash_actions::diff_value_at(root, Some("new.txt")).unwrap();
    assert_eq!(untracked["changed"], true);
    assert!(untracked["diff"].as_str().unwrap().contains("+new file"));
    // The command must not leak the temporary workspace's absolute path.
    assert!(
        !untracked["diff"]
            .as_str()
            .unwrap()
            .contains(root.to_str().unwrap())
    );

    let all = slash_actions::diff_value_at(root, None).unwrap();
    assert!(all["diff"].as_str().unwrap().contains("+after"));
    assert!(all["diff"].as_str().unwrap().contains("+new file"));
    assert!(all["diff"].as_str().unwrap().len() <= MAX_SLASH_DIFF_BYTES);

    let dot = slash_actions::diff_value_at(root, Some(".")).unwrap();
    assert_eq!(dot["path"], ".");
    assert!(dot["diff"].as_str().unwrap().contains("+after"));
}

#[test]
fn diff_owner_rejects_escape_and_symlink_paths() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    std::fs::write(root.join("note.txt"), "safe\n").unwrap();
    assert!(slash_actions::diff_value_at(root, Some("../note.txt")).is_err());
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("note.txt"), root.join("link.txt")).unwrap();
    #[cfg(unix)]
    assert!(slash_actions::diff_value_at(root, Some("link.txt")).is_err());
}

#[test]
fn attach_owner_stages_reference_and_consumes_it_on_next_turn() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    std::fs::write(root.join("note.md"), "private attachment\n").unwrap();
    let attachment = slash_actions::attachment_for_path(root, "note.md").unwrap();
    assert_eq!(attachment.kind, AttachmentKind::File);
    assert_eq!(attachment.mime_type, "text/plain");

    let session = SessionStore::open(root.join("session.jsonl")).unwrap();
    let mut agent = Agent::with_echo(session);
    agent.set_attachment_workspace(ToolContext::new(root).unwrap());
    let result = slash_actions::attach_value_at(&mut agent, root, "note.md").unwrap();
    assert_eq!(result["accepted"], true);
    assert_eq!(agent.pending_attachments().len(), 1);
    assert_eq!(result["path"], "note.md");
    assert!(result["sha256"].as_str().unwrap().len() == 64);

    let submission = agent
        .submit(TurnInputRequest::new("summarize attachment"))
        .unwrap();
    assert!(submission.accepted());
    assert!(agent.pending_attachments().is_empty());
    let metadata = agent.history()[0].metadata.as_ref().unwrap();
    assert_eq!(metadata["attachments"][0]["path"], "note.md");
    assert!(
        !std::fs::read_to_string(agent.session().path())
            .unwrap()
            .contains("private attachment")
    );
}

#[test]
fn staged_attachment_is_carried_into_a_cancel_reissue_turn() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    std::fs::write(root.join("note.txt"), "reissue attachment\n").unwrap();
    let session = SessionStore::open(root.join("session.jsonl")).unwrap();
    let mut agent = Agent::with_echo(session);
    agent.set_attachment_workspace(ToolContext::new(root).unwrap());

    let first = slash_actions::attach_value_at(&mut agent, root, "note.txt").unwrap();
    assert_eq!(first["accepted"], true);
    agent.submit(TurnInputRequest::new("first turn")).unwrap();
    agent.run_active_turn().unwrap();

    slash_actions::attach_value_at(&mut agent, root, "note.txt").unwrap();
    let submission = agent
        .start_steer_reissue("retry with attachment".into(), "superseded-turn")
        .unwrap();
    assert!(submission.accepted());
    assert!(agent.pending_attachments().is_empty());
    let turn = agent.history().last().unwrap();
    assert_eq!(
        turn.metadata.as_ref().unwrap()["attachments"][0]["path"],
        "note.txt"
    );
    assert_eq!(
        turn.metadata.as_ref().unwrap()["steer"]["strategy"],
        "cancel_reissue"
    );
}

#[test]
fn attachment_constructor_rejects_oversized_files() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    std::fs::write(
        root.join("large.bin"),
        vec![0_u8; zenpi::backend::MAX_ATTACHMENT_BYTES + 1],
    )
    .unwrap();
    assert!(slash_actions::attachment_for_path(root, "large.bin").is_err());

    // Ensure the validator still accepts a provider-shaped reference when it
    // is not routed through the workspace slash action.
    let remote = InputAttachment {
        kind: AttachmentKind::Image,
        mime_type: "image/png".into(),
        path: None,
        url: Some("https://example.test/image.png".into()),
        file_id: None,
    };
    remote.validate().unwrap();
}

#[test]
fn headless_slash_owner_actions_are_usable_before_a_prompt() {
    let directory = tempdir().unwrap();
    let session_path = directory.path().join("session.jsonl");
    let mut agent = Agent::with_echo(SessionStore::open(&session_path).unwrap());
    agent.set_attachment_workspace(ToolContext::new(std::env::current_dir().unwrap()).unwrap());
    let input = concat!(
        "{\"type\":\"command\",\"id\":\"d\",\"text\":\"/diff README.md\"}\n",
        "{\"type\":\"command\",\"id\":\"a\",\"text\":\"/attach README.md\"}\n",
        "{\"type\":\"prompt\",\"id\":\"p\",\"text\":\"summarize\"}\n",
        "{\"type\":\"shutdown\",\"id\":\"q\"}\n",
    );
    let mut output = Vec::new();
    run_headless(
        &mut agent,
        std::io::Cursor::new(input.as_bytes()),
        &mut output,
    )
    .unwrap();
    let records = String::from_utf8(output).unwrap();
    let responses = records
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(responses.iter().any(|value| {
        value["id"] == "d" && value["success"] == true && value["data"]["accepted"] == true
    }));
    assert!(responses.iter().any(|value| {
        value["id"] == "a" && value["success"] == true && value["data"]["pending"] == 1
    }));
    assert!(
        responses
            .iter()
            .any(|value| { value["id"] == "p" && value["success"] == true })
    );
    let journal = std::fs::read_to_string(&session_path).unwrap();
    assert!(journal.contains("README.md"));
    assert!(!journal.contains("zenpi is a small Rust agent runtime"));
}

#[test]
fn diff_owner_marks_process_output_truncated_instead_of_failing() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "zenpi@example.test"]);
    git(root, &["config", "user.name", "zenpi test"]);
    std::fs::write(root.join("large.txt"), "seed\n").unwrap();
    git(root, &["add", "large.txt"]);
    git(root, &["commit", "-qm", "initial"]);
    let content = format!("{}\n", "x".repeat(MAX_SLASH_DIFF_BYTES * 2));
    std::fs::write(root.join("large.txt"), content).unwrap();

    let value = slash_actions::diff_value_at(root, Some("large.txt")).unwrap();
    assert_eq!(value["changed"], true);
    assert_eq!(value["truncated"], true);
    assert!(value["diff"].as_str().unwrap().len() <= MAX_SLASH_DIFF_BYTES);
}

#[test]
fn diff_owner_supports_a_new_repository_without_head() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    git(root, &["init", "-q"]);
    std::fs::write(root.join("first.txt"), "initial\n").unwrap();
    git(root, &["add", "first.txt"]);

    let value = slash_actions::diff_value_at(root, Some("first.txt")).unwrap();
    assert_eq!(value["changed"], true);
    assert!(value["diff"].as_str().unwrap().contains("+initial"));
}

#[test]
fn tui_diff_owner_keeps_hunks_as_multiline_transcript_text() {
    let directory = tempdir().unwrap();
    let session = SessionStore::open(directory.path().join("session.jsonl")).unwrap();
    let mut agent = Agent::with_echo(session);
    agent.set_attachment_workspace(ToolContext::new(std::env::current_dir().unwrap()).unwrap());
    let mut state = TuiState::default();
    assert_eq!(
        dispatch_slash_command(
            zenpi::slash::SlashCommand::Diff {
                path: Some("README.md".into()),
            },
            &mut state,
            Some(&mut agent),
        ),
        SlashDispatchAction::Continue
    );
    let message = state
        .messages()
        .find(|message| message.role == MessageRole::System)
        .expect("diff transcript message");
    assert!(message.text.starts_with("diff README.md changed="));
    assert!(message.text.contains("status:") || message.text.contains("truncated="));
}
