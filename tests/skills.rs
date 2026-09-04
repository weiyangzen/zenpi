use std::fs;

use tempfile::tempdir;
use zenpi::skills::SkillSet;

fn write_skill(root: &std::path::Path, directory: &str, name: &str, instructions: &str) {
    let path = root.join(directory);
    fs::create_dir_all(&path).unwrap();
    fs::write(
        path.join("skill.toml"),
        format!(
            "name = \"{name}\"\nversion = \"1.0.0\"\ninstructions = \"{instructions}\"\ntools = [\"read_file\"]\n[hooks]\nprompt_prefix = \"prefix-{instructions}\"\ncontext_prefix = \"context-{instructions}\"\ntool_allowlist = [\"read_file\"]\nsession_close = \"close-{instructions}\"\n"
        ),
    )
    .unwrap();
}

#[test]
fn project_skills_override_user_skills_and_hooks_are_ordered() {
    let dir = tempdir().unwrap();
    let user = dir.path().join("user");
    let project = dir.path().join("project");
    write_skill(&user, "shared", "shared", "user");
    write_skill(&project, "shared", "shared", "project");
    let set = SkillSet::load(&user, &project).unwrap();
    assert_eq!(set.manifests().count(), 1);
    assert!(set.instructions().contains("project"));
    assert!(!set.instructions().contains("user"));
    assert_eq!(set.prepare_prompt("hello"), "prefix-project\n\nhello");
    let effective = set.effective_instructions();
    assert!(effective.contains("prefix-project"));
    assert!(effective.contains("context-project"));
    assert!(set.tool_allowed("read_file"));
    assert!(!set.tool_allowed("run_command"));
    assert_eq!(set.session_close_outputs(), vec!["close-project"]);
}

#[test]
fn session_close_hooks_are_durable_and_ordered_without_auth_access() {
    let dir = tempdir().unwrap();
    let user = dir.path().join("user");
    let project = dir.path().join("project");
    write_skill(&user, "a", "a", "first");
    write_skill(&user, "b", "b", "second");
    let skills = SkillSet::load(&user, &project).unwrap();
    let session_path = dir.path().join("session.jsonl");
    let mut agent =
        zenpi::core::Agent::with_echo(zenpi::session::SessionStore::open(&session_path).unwrap());
    agent.set_skills(skills);
    agent.try_close().unwrap();
    let session = zenpi::session::SessionStore::open(session_path).unwrap();
    let close = session
        .events()
        .iter()
        .filter(|event| event["type"] == "skill_session_close")
        .collect::<Vec<_>>();
    assert_eq!(close.len(), 2);
    assert_eq!(close[0]["order"], 0);
    assert_eq!(close[0]["output"], "close-first");
    assert_eq!(close[1]["order"], 1);
    assert_eq!(close[1]["output"], "close-second");
    let journal = fs::read_to_string(session.path()).unwrap();
    assert!(!journal.contains("OPENAI_API_KEY"));
}

#[test]
fn invalid_optional_hook_is_rejected_before_it_can_corrupt_a_session() {
    let dir = tempdir().unwrap();
    let user = dir.path().join("user");
    let project = dir.path().join("project");
    let invalid = user.join("invalid");
    fs::create_dir_all(&invalid).unwrap();
    fs::write(
        invalid.join("skill.toml"),
        format!(
            "name = \"invalid\"\nversion = \"1\"\ninstructions = \"ok\"\n[hooks]\nprompt_prefix = \"{}\"\n",
            "x".repeat(16 * 1024 + 1)
        ),
    )
    .unwrap();
    assert!(SkillSet::load(&user, &project).is_err());
    assert!(!dir.path().join("session.jsonl").exists());
}

#[test]
fn malformed_or_symlinked_skill_fails_closed() {
    let dir = tempdir().unwrap();
    let user = dir.path().join("user");
    let project = dir.path().join("project");
    let broken = user.join("broken");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join("skill.toml"), "name = \"../bad\"\n").unwrap();
    assert!(SkillSet::load(&user, &project).is_err());
}
