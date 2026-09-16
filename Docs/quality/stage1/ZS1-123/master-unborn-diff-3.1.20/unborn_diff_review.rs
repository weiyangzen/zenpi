#![cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::{fs, path::Path, process::Command};
use zenpi::slash_actions::{MAX_SLASH_DIFF_BYTES, MAX_STATUS_BYTES, diff_value_at};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .env_remove("GIT_GLOB_PATHSPECS")
        .env_remove("GIT_ICASE_PATHSPECS")
        .arg("--literal-pathspecs")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn repository(committed: bool, format: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    git(root, &["init", "-q", &format!("--object-format={format}")]);
    git(root, &["config", "user.name", "unborn fixture"]);
    git(root, &["config", "user.email", "unborn@example.test"]);
    if committed {
        git(root, &["commit", "--allow-empty", "-qm", "empty baseline"]);
    }
    d
}
fn put(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn populate(root: &Path, scenario: &str) -> Vec<String> {
    let names = [
        "scope[ab]/space file",
        "scope[ab]/quote\"name",
        "scope[ab]/中文 文件",
        r"scope[ab]/note\file",
        "scope[ab]/note/file",
        "scope[ab]/square[ab]",
        "scope[ab]/:(glob)*",
    ];
    let mut explicit = Vec::new();
    for (i, name) in names.iter().enumerate() {
        put(root, name, &format!("STAGED_{i:02}\n"));
        if scenario == "unstaged-only" {
            git(root, &["add", "--intent-to-add", "--", name]);
        } else {
            git(root, &["add", "--", name]);
        }
        match scenario {
            "mixed-files" => {
                let other = format!("{name}.mixed");
                put(root, &other, &format!("INDEX_INTERMEDIATE_{i:02}\n"));
                git(root, &["add", "--", &other]);
                put(root, &other, &format!("FINAL_WORKTREE_{i:02}\n"));
                explicit.push(other);
            }
            "same-file" | "unstaged-only" => put(root, name, &format!("FINAL_WORKTREE_{i:02}\n")),
            "stage-delete" => fs::remove_file(root.join(name)).unwrap(),
            "stage-rename" => {
                let renamed = format!("{name}.renamed");
                git(root, &["mv", "--", name, &renamed]);
                put(root, &renamed, &format!("FINAL_RENAME_{i:02}\n"));
                explicit.push(renamed);
            }
            "worktree-rename" => {
                let renamed = format!("{name}.moved");
                fs::rename(root.join(name), root.join(&renamed)).unwrap();
                explicit.push(renamed);
            }
            "staged-only" => (),
            _ => panic!("unknown scenario"),
        }
        // A staged rename removes its old path entirely from Git status.
        if scenario != "stage-rename" {
            explicit.push(name.to_string());
        }
    }
    put(root, "scopea/neighbor", "OUTSIDE_SCOPE\n");
    if scenario == "unstaged-only" {
        git(root, &["add", "--intent-to-add", "--", "scopea/neighbor"]);
    } else {
        git(root, &["add", "--", "scopea/neighbor"]);
    }
    if !matches!(
        scenario,
        "stage-delete" | "stage-rename" | "worktree-rename"
    ) {
        assert_ne!(
            fs::metadata(root.join(names[3])).unwrap().ino(),
            fs::metadata(root.join(names[4])).unwrap().ino()
        );
    }
    explicit
}

#[test]
fn unborn_matches_empty_commit_worktree_view_in_all_scopes() {
    let mut mismatches = Vec::new();
    let mut comparisons = 0;
    for format in ["sha1", "sha256"] {
        for scenario in [
            "staged-only",
            "unstaged-only",
            "mixed-files",
            "same-file",
            "stage-delete",
            "stage-rename",
            "worktree-rename",
        ] {
            let unborn = repository(false, format);
            let baseline = repository(true, format);
            let explicit = populate(unborn.path(), scenario);
            assert_eq!(explicit, populate(baseline.path(), scenario));
            for quote in ["true", "false"] {
                git(unborn.path(), &["config", "core.quotePath", quote]);
                git(baseline.path(), &["config", "core.quotePath", quote]);
                let mut scopes = vec![None, Some("."), Some("scope[ab]"), Some("scopea")];
                scopes.extend(explicit.iter().map(|s| Some(s.as_str())));
                for scope in scopes {
                    let actual = diff_value_at(unborn.path(), scope).unwrap();
                    let expected = diff_value_at(baseline.path(), scope).unwrap();
                    comparisons += 1;
                    assert_eq!(
                        actual["status"], expected["status"],
                        "status {format}/{scenario}/{scope:?}"
                    );
                    if actual["diff"] != expected["diff"]
                        || actual["changed"] != expected["changed"]
                    {
                        println!(
                            "MISMATCH {format}/{scenario}/{quote}/{scope:?}\nactual={actual}\nexpected={expected}"
                        );
                        mismatches.push(format!("{format}/{scenario}/{quote}/{scope:?}"));
                    }
                    if scope == Some("scope[ab]") {
                        assert!(!actual["diff"].as_str().unwrap().contains("OUTSIDE_SCOPE"));
                    }
                    assert!(actual["diff"].as_str().unwrap().len() <= MAX_SLASH_DIFF_BYTES);
                    assert!(actual["status"].as_str().unwrap().len() <= MAX_STATUS_BYTES);
                }
            }
        }
    }
    println!(
        "empty_baseline_comparisons={comparisons} mismatches={}",
        mismatches.len()
    );
    assert!(
        mismatches.is_empty(),
        "unborn baseline mismatch: {mismatches:?}"
    );
}

#[test]
fn empty_untracked_files_are_real_changes_without_aggregate_errors() {
    for format in ["sha1", "sha256"] {
        for committed in [false, true] {
            let d = repository(committed, format);
            let root = d.path();
            put(root, "new dir/deep/empty\" 中文", "");
            put(root, r"new dir/deep/empty\file", "");
            put(root, "new dir/deep/content", "NONEMPTY_POSITIVE\n");
            for name in ["new dir/deep/empty\" 中文", r"new dir/deep/empty\file"] {
                let output = Command::new("git")
                    .current_dir(root)
                    .args([
                        "diff",
                        "--no-index",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--no-color",
                        "--",
                        "/dev/null",
                        &format!("./{name}"),
                    ])
                    .output()
                    .unwrap();
                println!(
                    "empty no-index format={format} committed={committed} status={:?} stdout={:?}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stdout)
                );
                assert_eq!(output.status.code(), Some(1));
                assert!(String::from_utf8_lossy(&output.stdout).contains("new file mode"));
            }
            for scope in [
                None,
                Some("."),
                Some("new dir"),
                Some("new dir/deep/empty\" 中文"),
                Some(r"new dir/deep/empty\file"),
            ] {
                let value = diff_value_at(root, scope).unwrap();
                assert_eq!(value["changed"], true);
                assert_eq!(value["truncated"], false);
                let diff = value["diff"].as_str().unwrap();
                assert!(diff.contains("new file mode"));
                assert_eq!(
                    diff.matches("diff --git ").count(),
                    if scope.is_some_and(|s| s.contains("empty")) {
                        1
                    } else {
                        3
                    }
                );
            }
            for i in 0..40 {
                put(root, &format!("count/empty {i:02}"), "");
            }
            let value = diff_value_at(root, Some("count")).unwrap();
            assert_eq!(
                value["diff"]
                    .as_str()
                    .unwrap()
                    .matches("new file mode")
                    .count(),
                32
            );
        }
    }
}

#[test]
fn unborn_mixed_view_respects_final_output_and_status_caps() {
    let d = repository(false, "sha256");
    let root = d.path();
    put(root, "a-staged", "STAGED_POSITIVE\n");
    git(root, &["add", "--", "a-staged"]);
    put(root, "z-mixed", "INDEX_ONLY_INTERMEDIATE\n");
    git(root, &["add", "--", "z-mixed"]);
    put(root, "z-mixed", &"CURRENT_WORKTREE_LINE\n".repeat(8000));
    let value = diff_value_at(root, None).unwrap();
    let diff = value["diff"].as_str().unwrap();
    assert!(diff.contains("+STAGED_POSITIVE"));
    assert!(diff.contains("+CURRENT_WORKTREE_LINE"));
    assert!(!diff.contains("INDEX_ONLY_INTERMEDIATE"));
    assert_eq!(value["truncated"], true);
    assert!(diff.len() <= MAX_SLASH_DIFF_BYTES && diff.ends_with("[diff truncated]"));
    let many = repository(false, "sha1");
    for i in 0..180 {
        put(
            many.path(),
            &format!("new dir/file {i:03} {}", "q".repeat(220)),
            "ordinary\n",
        );
    }
    let value = diff_value_at(many.path(), None).unwrap();
    let status = value["status"].as_str().unwrap();
    assert!(status.len() <= MAX_STATUS_BYTES && status.ends_with("[diff truncated]"));
    assert_eq!(
        value["diff"]
            .as_str()
            .unwrap()
            .matches("diff --git ")
            .count(),
        32
    );
}

#[test]
fn git_failures_are_not_presented_as_successful_empty_reviews() {
    let outside = tempfile::tempdir().unwrap();
    assert!(diff_value_at(outside.path(), None).is_err());
    let d = repository(false, "sha1");
    put(d.path(), "staged", "content\n");
    git(d.path(), &["add", "--", "staged"]);
    fs::write(d.path().join(".git/index"), "invalid index bytes").unwrap();
    assert!(diff_value_at(d.path(), None).is_err());
}

#[test]
fn inherited_git_modes_preserve_unborn_empty_tree_discovery() {
    for variable in ["GIT_GLOB_PATHSPECS", "GIT_ICASE_PATHSPECS"] {
        let output = Command::new(std::env::current_exe().unwrap())
            .env(variable, "1")
            .args([
                "--exact",
                "empty_untracked_files_are_real_changes_without_aggregate_errors",
                "--nocapture",
            ])
            .output()
            .unwrap();
        println!(
            "{variable} child={}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
}
