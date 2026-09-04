use std::fs;

use tempfile::tempdir;
use zenpi::skills::SkillSet;

fn write_skill(root: &std::path::Path, directory: &str, name: &str, instructions: &str) {
    let path = root.join(directory);
    fs::create_dir_all(&path).unwrap();
    fs::write(
        path.join("skill.toml"),
        format!(
            "name = \"{name}\"\nversion = \"1.0.0\"\ninstructions = \"{instructions}\"\ntools = [\"read_file\"]\n[hooks]\nprompt_prefix = \"prefix-{instructions}\"\n"
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
