use std::{
    cell::Cell,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tempfile::tempdir;
use zenpi::{
    prompt_templates::{
        MAX_TEMPLATE_OUTPUT_BYTES, PromptTemplates, TemplateError, parse_command_args,
        substitute_args,
    },
    resource_loader::{
        ResourceError, ResourceKind, ResourceLoader, ResourcePaths, TextResourcePath,
    },
    skills::{SkillInvocation, SkillScope},
};

fn put(root: &Path, path: &str, content: &str) -> PathBuf {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();
    path
}

fn config(root: &Path) -> ResourcePaths {
    ResourcePaths {
        user_skills: root.join("user-skills"),
        project_skills: root.join("project-skills"),
        skill_paths: vec![],
        user_templates: root.join("user-prompts"),
        project_templates: root.join("project-prompts"),
        template_paths: vec![],
        text_resources: vec![TextResourcePath {
            name: "system".into(),
            path: root.join("SYSTEM.md"),
        }],
    }
}

fn fixtures(root: &Path, version: &str) {
    put(
        root,
        "user-skills/research/SKILL.md",
        &format!("---\nname: research\ndescription: research {version}\n---\nbody {version}"),
    );
    put(
        root,
        "user-prompts/review.md",
        &format!(
            "---\ndescription: review {version}\nargument-hint: '<target>'\n---\nreview {version}: $1 / $ARGUMENTS"
        ),
    );
    put(root, "SYSTEM.md", &format!("system {version}"));
}

#[test]
fn substitution_matches_source_positional_defaults_slices_and_literals() {
    let cases: &[(&str, &[&str], &str)] = &[
        ("$1 $2 $3", &["a", "b"], "a b "),
        ("$@ = $ARGUMENTS", &["a", "b"], "a b = a b"),
        (
            "$1: $ARGUMENTS",
            &["$2", "$(touch marker)"],
            "$2: $2 $(touch marker)",
        ),
        ("${1:-seven steps}", &[], "seven steps"),
        ("${2:-$1}", &["a"], "$1"),
        ("${1:-fallback}", &[""], "fallback"),
        (
            "${@:-all default} ${ARGUMENTS:-other}",
            &[],
            "all default other",
        ),
        ("${1:-fallback}", &["$ARGUMENTS"], "$ARGUMENTS"),
        ("${@:2}", &["a", "b", "c"], "b c"),
        ("${@:2:1}", &["a", "b", "c"], "b"),
        ("${@:0}", &["a", "b"], "a b"),
        ("${@:1:0}", &["a", "b"], ""),
        ("${@:99}", &["a", "b"], ""),
        ("${@:2:99}", &["a", "b", "c"], "b c"),
        ("${@:1}", &["${@:2}", "literal"], "${@:2} literal"),
        ("$0 $99999999999999999999999999", &["a"], " "),
        ("${99999999999999999999999999:-fallback}", &[], "fallback"),
        ("${@:99999999999999999999999999}", &["a"], ""),
        ("$1.5 pre$ARGUMENTS", &["a"], "a.5 prea"),
        (
            "$A $$ $ $ARGS $arguments",
            &["a"],
            "$A $$ $ $ARGS $arguments",
        ),
        ("\\$100", &[], "\\"),
        ("$1$2", &["日本語", "🎉"], "日本語🎉"),
        ("$@", &["a", "", "c"], "a  c"),
        ("$1", &["line1\nline2\tend"], "line1\nline2\tend"),
    ];
    for (template, arguments, expected) in cases {
        let args = arguments
            .iter()
            .map(|arg| arg.to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            substitute_args(template, &args, || false).unwrap(),
            *expected,
            "{template}"
        );
    }
    let args = (1..=15).map(|n| n.to_string()).collect::<Vec<_>>();
    assert_eq!(
        substitute_args("$10 $12 $15", &args, || false).unwrap(),
        "10 12 15"
    );
}

#[test]
fn source_quote_grammar_is_distinct_from_control_slash_tokenizer() {
    for (input, expected) in [
        ("a b c", vec!["a", "b", "c"]),
        ("\"first arg\" second", vec!["first arg", "second"]),
        ("'single' \"double\"", vec!["single", "double"]),
        ("\"\" \" \"", vec![" "]),
        ("one\n\n\ttwo", vec!["one", "two"]),
        ("\"line1\nline2\"", vec!["line1\nline2"]),
        ("unclosed 'quoted tail", vec!["unclosed", "quoted tail"]),
        ("path\\ with\\ spaces", vec!["path\\", "with\\", "spaces"]),
        (
            "$1 `echo hello` $(touch x)",
            vec!["$1", "`echo", "hello`", "$(touch", "x)"],
        ),
        ("", vec![]),
    ] {
        assert_eq!(parse_command_args(input).unwrap(), expected);
    }
    assert!(zenpi::slash::parse("/goal 'unclosed").is_err());
    assert_eq!(
        parse_command_args("日本語 'two words'").unwrap(),
        ["日本語", "two words"]
    );
}

#[test]
fn catalogue_discovery_frontmatter_source_and_nonrecursive_expansion() {
    let temp = tempdir().unwrap();
    put(
        temp.path(),
        "prompts/review.md",
        "\u{feff}---\r\ndescription: |\r\n  first line\r\n  second line\r\nargument-hint: '<target>'\r\ncustom: [inert, metadata]\r\n---\r\n $1 / ${2:-default} / $ARGUMENTS \r\n",
    );
    put(temp.path(), "prompts/plain.md", "日本語 first line\nbody");
    put(
        temp.path(),
        "prompts/nested/hidden.md",
        "must not be discovered",
    );
    let set = PromptTemplates::load(
        &temp.path().join("prompts"),
        &temp.path().join("none"),
        &[],
        || false,
    )
    .unwrap();
    assert_eq!(set.templates().count(), 2);
    let template = set.get("review").unwrap();
    assert_eq!(template.description, "first line\nsecond line\n");
    assert_eq!(template.argument_hint.as_deref(), Some("<target>"));
    assert_eq!(template.scope, SkillScope::User);
    assert_eq!(template.source_hash.len(), 64);
    let expanded = set.expand("/review\n'$2'", || false).unwrap().unwrap();
    assert_eq!(expanded.content, "$2 / default / $2");
    assert_eq!(expanded.source, template.source);
    assert_eq!(expanded.source_hash, template.source_hash);
    for unmatched in ["text /review", "/unknown", "/", " /review", "/Review"] {
        assert!(set.expand(unmatched, || false).unwrap().is_none());
    }
    assert!(!temp.path().join("marker").exists());
}

#[test]
fn precedence_is_queryable_and_same_priority_duplicates_are_errors() {
    let temp = tempdir().unwrap();
    put(temp.path(), "user/review.md", "user");
    put(temp.path(), "project/review.md", "project");
    let explicit = put(temp.path(), "explicit/review.md", "explicit");
    let other = put(temp.path(), "other/review.md", "other");
    let set = PromptTemplates::load(
        &temp.path().join("user"),
        &temp.path().join("project"),
        &[explicit.clone(), explicit.clone()],
        || false,
    )
    .unwrap();
    assert_eq!(set.get("review").unwrap().content, "explicit");
    assert_eq!(set.collisions().len(), 2);
    assert_eq!(set.collisions()[1].winner, explicit.canonicalize().unwrap());
    assert!(matches!(
        PromptTemplates::load(
            &temp.path().join("user"),
            &temp.path().join("project"),
            &[explicit, other],
            || false
        ),
        Err(TemplateError::Duplicate { .. })
    ));
}

#[test]
fn failed_reload_preserves_entire_previous_snapshot_and_path_selection() {
    let temp = tempdir().unwrap();
    fixtures(temp.path(), "v1");
    let mut loader = ResourceLoader::new(config(temp.path())).unwrap();
    let first = loader.reload(|| false).unwrap();
    let selected_paths = loader.paths().clone();
    assert_eq!(first.generation, 1);
    fixtures(temp.path(), "v2");
    let missing = temp.path().join("missing.md");
    let mut bad = loader.paths().clone();
    bad.text_resources[0].path = missing;
    assert!(loader.reload_with_paths(bad, || false).is_err());
    assert!(Arc::ptr_eq(&first, &loader.snapshot()));
    assert_eq!(loader.paths(), &selected_paths);
    assert_eq!(first.text_resources["system"].content, "system v1");
    assert_eq!(
        first.skills.metadata().next().unwrap().description,
        "research v1"
    );
    assert_eq!(
        first
            .templates
            .expand("/review target", || false)
            .unwrap()
            .unwrap()
            .content,
        "review v1: target / target"
    );
    let next = loader.reload(|| false).unwrap();
    assert_eq!(next.generation, 2);
    assert_eq!(next.text_resources["system"].content, "system v2");
    assert_ne!(
        first.text_resources["system"].source_hash,
        next.text_resources["system"].source_hash
    );
}

#[test]
fn admitted_turn_arc_keeps_old_templates_text_and_resolved_skill_body_after_reload() {
    let temp = tempdir().unwrap();
    fixtures(temp.path(), "v1");
    let mut loader = ResourceLoader::new(config(temp.path())).unwrap();
    loader.reload(|| false).unwrap();
    let admitted = loader.snapshot();
    let selected_skill = admitted
        .skills
        .load_body("research", SkillInvocation::Model, "literal $1", || false)
        .unwrap();
    fixtures(temp.path(), "v2");
    loader.reload(|| false).unwrap();
    let next_turn = loader.snapshot();
    assert_eq!(admitted.generation, 1);
    assert_eq!(selected_skill.body, "body v1");
    assert_eq!(selected_skill.arguments, "literal $1");
    assert_eq!(admitted.text_resources["system"].content, "system v1");
    assert_eq!(
        admitted
            .templates
            .expand("/review a", || false)
            .unwrap()
            .unwrap()
            .content,
        "review v1: a / a"
    );
    assert_eq!(next_turn.generation, 2);
    assert_eq!(
        next_turn
            .templates
            .expand("/review a", || false)
            .unwrap()
            .unwrap()
            .content,
        "review v2: a / a"
    );
    // Unselected old-generation skill bodies do not silently read new policy.
    assert!(
        admitted
            .skills
            .load_body("research", SkillInvocation::Model, "", || false)
            .is_err()
    );
}

#[test]
fn cancellation_at_every_candidate_checkpoint_keeps_old_arc() {
    let temp = tempdir().unwrap();
    fixtures(temp.path(), "v1");
    let mut loader = ResourceLoader::new(config(temp.path())).unwrap();
    let first = loader.reload(|| false).unwrap();
    fixtures(temp.path(), "v2");
    let calls = Cell::new(0usize);
    let mut probe = ResourceLoader::new(config(temp.path())).unwrap();
    probe
        .reload(|| {
            calls.set(calls.get() + 1);
            false
        })
        .unwrap();
    assert!(calls.get() > 20);
    for cancel_at in 1..=calls.get() {
        let position = Cell::new(0usize);
        let result = loader.reload(|| {
            position.set(position.get() + 1);
            position.get() >= cancel_at
        });
        assert!(
            matches!(result, Err(ResourceError::Cancelled)),
            "checkpoint {cancel_at}"
        );
        assert!(
            Arc::ptr_eq(&first, &loader.snapshot()),
            "checkpoint {cancel_at}"
        );
    }
    assert_eq!(loader.reload(|| false).unwrap().generation, 2);
}

#[test]
fn bad_yaml_duplicate_templates_and_duplicate_text_resources_do_not_publish() {
    let temp = tempdir().unwrap();
    fixtures(temp.path(), "v1");
    let mut loader = ResourceLoader::new(config(temp.path())).unwrap();
    let first = loader.reload(|| false).unwrap();
    for invalid in [
        "---\ndescription: [broken\n---\nbody",
        "---\ndescription: true\n---\nbody",
        "---\ndescription: a\ndescription: b\n---\nbody",
        "---\ndescription: ok\n",
        " ",
    ] {
        put(temp.path(), "user-prompts/review.md", invalid);
        assert!(loader.reload(|| false).is_err(), "{invalid}");
        assert!(Arc::ptr_eq(&first, &loader.snapshot()));
    }
    fixtures(temp.path(), "v2");
    let a = put(temp.path(), "a/review.md", "a");
    let b = put(temp.path(), "b/review.md", "b");
    let mut bad = loader.paths().clone();
    bad.template_paths = vec![a, b];
    assert!(matches!(
        loader.reload_with_paths(bad, || false),
        Err(ResourceError::Template(TemplateError::Duplicate { .. }))
    ));
    assert!(Arc::ptr_eq(&first, &loader.snapshot()));
    let mut duplicate = loader.paths().clone();
    duplicate
        .text_resources
        .push(duplicate.text_resources[0].clone());
    assert!(loader.reload_with_paths(duplicate, || false).is_err());
    assert!(Arc::ptr_eq(&first, &loader.snapshot()));
}

#[test]
fn selection_roundtrip_and_restart_load_current_files_without_memory_cache() {
    let temp = tempdir().unwrap();
    fixtures(temp.path(), "v1");
    let mut first = ResourceLoader::new(config(temp.path())).unwrap();
    let before = first.reload(|| false).unwrap();
    let saved = serde_json::to_vec(first.paths()).unwrap();
    let mut restarted = ResourceLoader::new(serde_json::from_slice(&saved).unwrap()).unwrap();
    assert_eq!(*restarted.reload(|| false).unwrap(), *before);
    fixtures(temp.path(), "v2");
    let mut restarted = ResourceLoader::new(serde_json::from_slice(&saved).unwrap()).unwrap();
    let snapshot = restarted.reload(|| false).unwrap();
    assert_eq!(snapshot.generation, 1); // Runtime counter starts fresh, not a durable acceptance cursor.
    assert_eq!(snapshot.text_resources["system"].content, "system v2");
    assert_ne!(
        snapshot.templates.get("review").unwrap().source_hash,
        before.templates.get("review").unwrap().source_hash
    );
}

#[test]
fn unified_collision_query_and_legacy_hooks_survive_resource_reload() {
    let temp = tempdir().unwrap();
    fixtures(temp.path(), "v1");
    put(
        temp.path(),
        "project-skills/research/SKILL.md",
        "---\nname: research\ndescription: project\n---\nproject body",
    );
    put(temp.path(), "project-prompts/review.md", "project $1");
    put(
        temp.path(),
        "user-skills/legacy/skill.toml",
        "name='legacy'\nversion='1'\ninstructions='legacy body'\n[hooks]\nprompt_prefix='prefix'\nsession_close='close'\n",
    );
    let mut loader = ResourceLoader::new(config(temp.path())).unwrap();
    let snapshot = loader.reload(|| false).unwrap();
    assert_eq!(snapshot.skills.prepare_prompt("prompt"), "prefix\n\nprompt");
    assert_eq!(snapshot.skills.session_close_outputs(), ["close"]);
    let collisions = snapshot.collisions();
    assert_eq!(collisions.len(), 2);
    assert_eq!(collisions[0].kind, ResourceKind::Skill);
    assert_eq!(collisions[1].kind, ResourceKind::Template);
    assert!(
        collisions
            .iter()
            .all(|entry| entry.winner.to_string_lossy().contains("project"))
    );
}

#[test]
fn expansion_limits_and_cancellation_never_return_partial_prompt() {
    let args = vec!["x".repeat(32768)];
    assert!(substitute_args(&"$1".repeat(33), &args, || false).is_err());
    assert!(substitute_args("$1", &["bad\0".into()], || false).is_err());
    let args = vec!["x".repeat(1024)];
    assert_eq!(
        substitute_args(&"$1".repeat(1024), &args, || false)
            .unwrap()
            .len(),
        MAX_TEMPLATE_OUTPUT_BYTES
    );
    let calls = Cell::new(0);
    assert!(matches!(
        substitute_args(&"$1".repeat(100), &args, || {
            calls.set(calls.get() + 1);
            calls.get() > 10
        }),
        Err(TemplateError::Cancelled)
    ));
}

#[test]
fn bad_utf8_oversize_and_symlinks_preserve_previous_snapshot() {
    let temp = tempdir().unwrap();
    fixtures(temp.path(), "v1");
    let mut loader = ResourceLoader::new(config(temp.path())).unwrap();
    let first = loader.reload(|| false).unwrap();
    fs::write(temp.path().join("user-prompts/review.md"), [255u8]).unwrap();
    assert!(loader.reload(|| false).is_err());
    fs::write(
        temp.path().join("user-prompts/review.md"),
        vec![b'x'; 128 * 1024 + 1],
    )
    .unwrap();
    assert!(loader.reload(|| false).is_err());
    assert!(Arc::ptr_eq(&first, &loader.snapshot()));
    #[cfg(unix)]
    {
        fs::remove_file(temp.path().join("user-prompts/review.md")).unwrap();
        std::os::unix::fs::symlink(
            temp.path().join("SYSTEM.md"),
            temp.path().join("user-prompts/review.md"),
        )
        .unwrap();
        assert!(loader.reload(|| false).is_err());
        assert!(Arc::ptr_eq(&first, &loader.snapshot()));
    }
}
