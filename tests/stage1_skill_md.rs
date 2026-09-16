use std::{cell::Cell, fs, path::Path};
use tempfile::tempdir;
use zenpi::skills::{MAX_SKILL_FILE_BYTES, SkillError, SkillInvocation, SkillScope, SkillSet};

fn markdown(root: &Path, path: &str, name: &str, description: &str, body: &str) {
    let directory = root.join(path);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n{body}"),
    )
    .unwrap();
}

#[test]
fn existing_load_entry_projects_metadata_and_body_requires_invocation() {
    let temp = tempdir().unwrap();
    markdown(
        temp.path(),
        "group/reader",
        "reader",
        "Read & inspect files",
        "BODY_ONLY_SENTINEL",
    );
    let set = SkillSet::load(temp.path(), &temp.path().join("missing")).unwrap();
    assert_eq!(set.metadata().count(), 1);
    assert!(
        set.effective_instructions()
            .contains("Read &amp; inspect files")
    );
    assert!(!set.effective_instructions().contains("BODY_ONLY_SENTINEL"));
    assert_eq!(set.manifests().count(), 0);
    let arguments = "\"two words\" $1 $(touch NEVER) `echo literal`";
    let body = set
        .load_body("reader", SkillInvocation::Explicit, arguments, || false)
        .unwrap();
    assert_eq!(body.body, "BODY_ONLY_SENTINEL");
    assert_eq!(body.arguments, arguments);
    assert_eq!(body.metadata.scope, SkillScope::User);
    assert_eq!(body.metadata.source_hash.len(), 64);
    assert!(!temp.path().join("NEVER").exists());
}

#[test]
fn disabled_model_skill_is_hidden_but_explicitly_loadable() {
    let temp = tempdir().unwrap();
    markdown(
        temp.path(),
        "hidden",
        "hidden",
        "Hidden skill\ndisable-model-invocation: true",
        "secret body",
    );
    let set = SkillSet::load(temp.path(), &temp.path().join("none")).unwrap();
    assert_eq!(set.metadata().count(), 1);
    assert_eq!(set.instructions(), "");
    assert!(
        set.load_body("hidden", SkillInvocation::Model, "", || false)
            .is_err()
    );
    assert_eq!(
        set.load_body("hidden", SkillInvocation::Explicit, "", || false)
            .unwrap()
            .body,
        "secret body"
    );
}

#[test]
fn nested_ignore_rules_negation_and_skill_root_stop_are_effective() {
    let temp = tempdir().unwrap();
    for (dir, name) in [
        ("group/yes", "yes"),
        ("group/no", "no"),
        ("hidden", "hidden"),
        (".private", "private"),
        ("node_modules/pkg", "pkg"),
        ("root", "root"),
        ("root/child", "child"),
    ] {
        markdown(temp.path(), dir, name, "fixture", "body");
    }
    fs::write(
        temp.path().join(".gitignore"),
        "hidden/\ngroup/*\n!group/yes/\n",
    )
    .unwrap();
    fs::write(temp.path().join("group/.ignore"), "!yes/\n").unwrap();
    let set = SkillSet::load(temp.path(), &temp.path().join("none")).unwrap();
    assert_eq!(
        set.metadata().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        ["root", "yes"]
    );
    fs::write(temp.path().join("group/yes/.fdignore"), "SKILL.md\n").unwrap();
    let set = SkillSet::load(temp.path(), &temp.path().join("none")).unwrap();
    assert_eq!(
        set.metadata().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        ["root"]
    );
}

#[test]
fn provenance_conflicts_and_precedence_are_stable_across_restarts() {
    let temp = tempdir().unwrap();
    let user = temp.path().join("user");
    let project = temp.path().join("project");
    let explicit = temp.path().join("explicit");
    markdown(&user, "a", "same", "user", "user body");
    markdown(&user, "z", "same", "later lexical", "loser body");
    markdown(&project, "a", "same", "project", "project body");
    markdown(&explicit, "a", "same", "explicit", "explicit body");
    let paths = [explicit.join("a/SKILL.md")];
    let first = SkillSet::load_with_paths(&user, &project, &paths, || false).unwrap();
    let second = SkillSet::load_with_paths(&user, &project, &paths, || false).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.collisions().len(), 3);
    assert_eq!(first.metadata().next().unwrap().scope, SkillScope::Explicit);
    assert_eq!(
        first
            .load_body("same", SkillInvocation::Explicit, "", || false)
            .unwrap()
            .body,
        "explicit body"
    );
}

#[test]
fn toml_hooks_keep_priority_at_same_scope_and_project_markdown_overrides_user() {
    let temp = tempdir().unwrap();
    let user = temp.path().join("user");
    let project = temp.path().join("project");
    markdown(&user, "md", "same", "md", "md body");
    fs::create_dir_all(user.join("toml")).unwrap();
    fs::write(
        user.join("toml/skill.toml"),
        "name='same'\nversion='1'\ninstructions='toml body'\n[hooks]\nprompt_prefix='preserved'\n",
    )
    .unwrap();
    let set = SkillSet::load(&user, &project).unwrap();
    assert_eq!(set.metadata().count(), 0);
    assert_eq!(set.manifests().count(), 1);
    assert_eq!(set.prepare_prompt("prompt"), "preserved\n\nprompt");
    assert_eq!(set.collisions().len(), 1);
    markdown(&project, "md", "same", "project", "project body");
    let set = SkillSet::load(&user, &project).unwrap();
    assert_eq!(set.manifests().count(), 0);
    assert_eq!(set.metadata().count(), 1);
    assert_eq!(set.prepare_prompt("prompt"), "prompt");
}

#[test]
fn yaml_multiline_quotes_unknown_metadata_bom_and_crlf_are_supported() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("SKILL.md");
    fs::write(&path, "\u{feff}---\r\nname: 'quoted'\r\ndescription: |\r\n  first line\r\n  second line\r\nmetadata:\r\n  labels: [one, two]\r\n---\r\nbody\r\n").unwrap();
    let set = SkillSet::load_with_paths(
        &temp.path().join("none"),
        &temp.path().join("none"),
        &[path],
        || false,
    )
    .unwrap();
    assert_eq!(
        set.metadata().next().unwrap().description,
        "first line\nsecond line\n"
    );
    assert_eq!(
        set.load_body("quoted", SkillInvocation::Model, "", || false)
            .unwrap()
            .body,
        "body\r\n"
    );
}

#[test]
fn malformed_frontmatter_types_duplicate_fields_and_utf8_fail_closed() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("SKILL.md");
    for content in [
        "no frontmatter",
        "---\nname: x\ndescription: ok\n",
        "---\nname: x\n---\nbody",
        "---\nname: invalid_name\ndescription: ok\n---\nbody",
        "---\nname: x\ndescription: []\n---\nbody",
        "---\nname: x\ndescription: true\n---\nbody",
        "---\nname: 123\ndescription: ok\n---\nbody",
        "---\nname: x\ndescription: ok\ndisable-model-invocation: 'true'\n---\nbody",
        "---\nname: x\nname: y\ndescription: ok\n---\nbody",
        "---\nname: x\ndescription: [broken\n---\nbody",
    ] {
        fs::write(&path, content).unwrap();
        assert!(
            SkillSet::load(temp.path(), &temp.path().join("none")).is_err(),
            "{content}"
        );
    }
    fs::write(&path, [255u8, 254, 0]).unwrap();
    assert!(SkillSet::load(temp.path(), &temp.path().join("none")).is_err());
    fs::write(&path, vec![b'x'; MAX_SKILL_FILE_BYTES + 1]).unwrap();
    assert!(SkillSet::load(temp.path(), &temp.path().join("none")).is_err());
}

#[test]
fn stale_body_and_policy_are_refused_until_explicit_reload() {
    let temp = tempdir().unwrap();
    markdown(temp.path(), "reader", "reader", "initial", "old body");
    let set = SkillSet::load(temp.path(), &temp.path().join("none")).unwrap();
    markdown(
        temp.path(),
        "reader",
        "reader",
        "changed\ndisable-model-invocation: true",
        "new body",
    );
    assert!(
        set.load_body("reader", SkillInvocation::Model, "", || false)
            .unwrap_err()
            .to_string()
            .contains("reload")
    );
    let reloaded = SkillSet::load(temp.path(), &temp.path().join("none")).unwrap();
    assert_ne!(
        set.metadata().next().unwrap().source_hash,
        reloaded.metadata().next().unwrap().source_hash
    );
    assert!(
        reloaded
            .load_body("reader", SkillInvocation::Model, "", || false)
            .is_err()
    );
    assert_eq!(
        reloaded
            .load_body("reader", SkillInvocation::Explicit, "", || false)
            .unwrap()
            .body,
        "new body"
    );
}

#[test]
fn cancellation_during_discovery_and_body_read_returns_no_partial_value() {
    let temp = tempdir().unwrap();
    markdown(temp.path(), "a", "a", "first", &"body".repeat(10000));
    markdown(temp.path(), "z", "z", "last", "body");
    let previous = SkillSet::load(temp.path(), &temp.path().join("none")).unwrap();
    let calls = Cell::new(0);
    let result = SkillSet::load_with_paths(temp.path(), &temp.path().join("none"), &[], || {
        calls.set(calls.get() + 1);
        calls.get() >= 8
    });
    assert!(matches!(result, Err(SkillError::Cancelled)));
    assert_eq!(previous.metadata().count(), 2);
    let calls = Cell::new(0);
    assert!(matches!(
        previous.load_body("a", SkillInvocation::Explicit, "", || {
            calls.set(calls.get() + 1);
            calls.get() >= 5
        }),
        Err(SkillError::Cancelled)
    ));
    assert_eq!(
        previous,
        SkillSet::load(temp.path(), &temp.path().join("none")).unwrap()
    );
}

#[test]
fn relative_resources_remain_under_skill_and_symlinks_are_refused() {
    let temp = tempdir().unwrap();
    markdown(
        temp.path(),
        "reader",
        "reader",
        "reader",
        "read references/file.txt",
    );
    fs::create_dir_all(temp.path().join("reader/references")).unwrap();
    fs::write(temp.path().join("reader/references/file.txt"), "fixture").unwrap();
    let set = SkillSet::load(temp.path(), &temp.path().join("none")).unwrap();
    let body = set
        .load_body("reader", SkillInvocation::Explicit, "", || false)
        .unwrap();
    assert!(
        body.resolve_resource("references/file.txt")
            .unwrap()
            .ends_with("reader/references/file.txt")
    );
    for denied in ["../outside", "/etc/passwd", "", "refs/../../outside"] {
        assert!(body.resolve_resource(denied).is_err());
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(temp.path(), temp.path().join("reader/link")).unwrap();
        assert!(body.resolve_resource("link/reader/SKILL.md").is_err());
        fs::remove_file(temp.path().join("reader/SKILL.md")).unwrap();
        std::os::unix::fs::symlink(
            temp.path().join("reader/references/file.txt"),
            temp.path().join("reader/SKILL.md"),
        )
        .unwrap();
        assert!(
            set.load_body("reader", SkillInvocation::Explicit, "", || false)
                .is_err()
        );
        assert!(SkillSet::load(temp.path(), &temp.path().join("none")).is_err());
    }
}

#[test]
#[ignore = "host fixture: set ZENPI_SKILL_FIXTURE_ROOT to the actual local skill repository"]
fn actual_learn_and_execution_skills_load_without_evaluating_instructions() {
    let root = std::env::var_os("ZENPI_SKILL_FIXTURE_ROOT").expect("host skill root required");
    let root = Path::new(&root);
    let missing = root.join("__stage1_missing__");
    let paths = [
        root.join("learn-cron-builder/SKILL.md"),
        root.join("execution-cron-builder/SKILL.md"),
    ];
    let set = SkillSet::load_with_paths(&missing, &missing, &paths, || false).unwrap();
    assert_eq!(set.metadata().count(), 2);
    for name in ["learn-cron-builder", "execution-cron-builder"] {
        let body = set
            .load_body(name, SkillInvocation::Explicit, "literal $1", || false)
            .unwrap();
        assert!(body.body.len() > 1000);
        assert!(!set.instructions().contains(&body.body));
        assert_eq!(body.arguments, "literal $1");
    }
}

mod model_tools {
    use serde_json::{Value, json};
    use std::{
        collections::BTreeSet,
        fs,
        io::{BufRead, BufReader, Read, Write},
        net::{TcpListener, TcpStream},
        path::Path,
        process::{Child, ChildStdin, Command, Stdio},
        sync::{Arc, Mutex},
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    use tempfile::tempdir;
    use zenpi::{
        backend::{Backend, BackendError, Completion, CompletionRequest},
        core::{Agent, TurnInputRequest},
        session::SessionStore,
        skills::{ModelSkillTools, SkillInvocation, SkillSet},
        tools::{
            BlueprintGate, BlueprintLease, BlueprintPolicySpec, SideEffectPolicy, Tool, ToolCall,
            ToolContext, ToolDefinition, ToolError, ToolErrorCode, ToolExecutionMode, ToolRegistry,
            ToolResult, ToolSideEffect,
        },
    };

    fn fixture(root: &Path, body: &str) -> SkillSet {
        fs::create_dir_all(root.join("reader/references")).unwrap();
        fs::write(
            root.join("reader/SKILL.md"),
            format!("---\nname: reader\ndescription: read fixture\n---\n{body}"),
        )
        .unwrap();
        fs::write(
            root.join("reader/references/source.md"),
            "RESOURCE_SENTINEL",
        )
        .unwrap();
        fs::create_dir_all(root.join("manual")).unwrap();
        fs::write(root.join("manual/SKILL.md"),"---\nname: manual\ndescription: explicit only\ndisable-model-invocation: true\n---\nMANUAL_SENTINEL").unwrap();
        SkillSet::load(root, &root.join("none")).unwrap()
    }
    fn call(name: &str, args: Value) -> ToolCall {
        ToolCall {
            id: "call".into(),
            name: name.into(),
            arguments: args,
        }
    }
    fn output(result: ToolResult) -> Value {
        match result {
            ToolResult::Success { output, .. } => output,
            other => panic!("{other:?}"),
        }
    }
    fn invoke(
        registry: &ToolRegistry,
        context: &ToolContext,
        name: &str,
        args: Value,
    ) -> ToolResult {
        registry.execute(context, SideEffectPolicy::read_only(), call(name, args))
    }

    #[test]
    fn real_registered_tools_lazy_load_then_read_relative_resource_and_scope_expires() {
        let dir = tempdir().unwrap();
        let skills = fixture(dir.path(), "BODY_SENTINEL");
        let shared = ModelSkillTools::default();
        let mut registry = ToolRegistry::new();
        shared.register(&mut registry).unwrap();
        let context = ToolContext::new(dir.path()).unwrap();
        assert_eq!(
            registry.execution_mode("load_skill"),
            ToolExecutionMode::Sequential
        );
        assert!(!invoke(&registry, &context, "load_skill", json!({"name":"reader"})).is_success());
        let scope = shared.begin_turn(skills.clone(), BTreeSet::new()).unwrap();
        assert!(
            !invoke(
                &registry,
                &context,
                "read_skill_resource",
                json!({"skill":"reader","path":"references/source.md"})
            )
            .is_success()
        );
        let result = output(invoke(
            &registry,
            &context,
            "load_skill",
            json!({"name":"reader","arguments":"$(touch never) `literal`"}),
        ));
        assert_eq!(result["body"], "BODY_SENTINEL");
        assert_eq!(result["arguments"], "$(touch never) `literal`");
        assert!(!dir.path().join("never").exists());
        assert!(
            result["skill"]["source_hash"]
                .as_str()
                .is_some_and(|hash| hash.len() == 64)
        );
        let resource = output(invoke(
            &registry,
            &context,
            "read_skill_resource",
            json!({"skill":"reader","path":"./references/source.md"}),
        ));
        assert_eq!(resource["resource"]["content"], "RESOURCE_SENTINEL");
        assert!(!invoke(&registry, &context, "load_skill", json!({"name":"manual"})).is_success());
        assert_eq!(
            skills
                .load_body("manual", SkillInvocation::Explicit, "", || false)
                .unwrap()
                .body,
            "MANUAL_SENTINEL"
        );
        drop(scope);
        assert!(shared.loaded_metadata("reader").unwrap().is_none());
        assert!(
            !invoke(
                &registry,
                &context,
                "read_skill_resource",
                json!({"skill":"reader","path":"references/source.md"})
            )
            .is_success()
        );
        let _next = shared.begin_turn(skills, BTreeSet::new()).unwrap();
        assert!(
            !invoke(
                &registry,
                &context,
                "read_skill_resource",
                json!({"skill":"reader","path":"references/source.md"})
            )
            .is_success()
        );
    }

    #[test]
    fn model_resource_paths_hashes_limits_disabled_and_cancellation_are_authoritative() {
        let dir = tempdir().unwrap();
        let skills = fixture(dir.path(), "BODY_SENTINEL");
        let shared = ModelSkillTools::default();
        let mut registry = ToolRegistry::new();
        shared.register(&mut registry).unwrap();
        let context = ToolContext::new(dir.path()).unwrap();
        let scope = shared.begin_turn(skills.clone(), BTreeSet::new()).unwrap();
        assert!(invoke(&registry, &context, "load_skill", json!({"name":"reader"})).is_success());
        fs::write(
            dir.path().join("reader/references/large.md"),
            "x".repeat(65537),
        )
        .unwrap();
        fs::write(dir.path().join("reader/references/binary"), [255, 0, 1]).unwrap();
        fs::write(dir.path().join("reader/references/nul"), "a\0b").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("manual"), dir.path().join("reader/link"))
            .unwrap();
        for path in [
            "../manual/SKILL.md",
            "/etc/passwd",
            "references/large.md",
            "references/binary",
            "references/nul",
            "SKILL.md",
            "link/SKILL.md",
            "link/anything",
        ] {
            assert!(
                !invoke(
                    &registry,
                    &context,
                    "read_skill_resource",
                    json!({"skill":"reader","path":path})
                )
                .is_success(),
                "{path}"
            );
        }
        let result = registry.execute_cancellable(
            &context,
            SideEffectPolicy::read_only(),
            call(
                "read_skill_resource",
                json!({"skill":"reader","path":"references/source.md"}),
            ),
            &|| true,
        );
        assert!(
            matches!(result,ToolResult::Error{error,..} if error.code==ToolErrorCode::Cancelled)
        );
        fs::write(
            dir.path().join("reader/SKILL.md"),
            "---\nname: reader\ndescription: changed\n---\nnew",
        )
        .unwrap();
        assert!(
            !invoke(
                &registry,
                &context,
                "read_skill_resource",
                json!({"skill":"reader","path":"references/source.md"})
            )
            .is_success()
        );
        drop(scope);
        let _next = shared
            .begin_turn(skills, BTreeSet::from(["reader".into()]))
            .unwrap();
        assert!(!invoke(&registry, &context, "load_skill", json!({"name":"reader"})).is_success());
    }

    #[test]
    fn cancellation_inside_body_read_never_admits_the_skill() {
        let dir = tempdir().unwrap();
        let skills = fixture(dir.path(), &"B".repeat(60000));
        let shared = ModelSkillTools::default();
        let mut registry = ToolRegistry::new();
        shared.register(&mut registry).unwrap();
        let context = ToolContext::new(dir.path()).unwrap();
        let _scope = shared.begin_turn(skills, BTreeSet::new()).unwrap();
        let polls = std::cell::Cell::new(0);
        let result = registry.execute_cancellable(
            &context,
            SideEffectPolicy::read_only(),
            call("load_skill", json!({"name":"reader"})),
            &|| {
                polls.set(polls.get() + 1);
                polls.get() > 5
            },
        );
        assert!(
            matches!(result,ToolResult::Error{error,..} if error.code==ToolErrorCode::Cancelled)
        );
        assert!(shared.loaded_metadata("reader").unwrap().is_none());
    }

    struct NamedTool(&'static str);
    impl Tool for NamedTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: self.0.into(),
                description: "fixture".into(),
                input_schema: json!({"type":"object"}),
                side_effect: ToolSideEffect::ReadOnly,
            }
        }
        fn invoke(
            &self,
            _: &ToolContext,
            _: &serde_json::Map<String, Value>,
        ) -> Result<Value, ToolError> {
            Ok(json!({}))
        }
    }
    struct NamesBackend(Arc<Mutex<Vec<String>>>);
    impl Backend for NamesBackend {
        fn complete(&self, request: CompletionRequest<'_>) -> Result<Completion, BackendError> {
            *self.0.lock().unwrap() = request.tools.iter().map(|tool| tool.name.clone()).collect();
            Ok(Completion::text("ok"))
        }
    }

    #[test]
    fn duplicate_tool_registration_preserves_registry_and_existing_agent_runtime() {
        let dir = tempdir().unwrap();
        let shared = ModelSkillTools::default();
        let mut registry = ToolRegistry::new();
        registry.register(NamedTool("read_skill_resource")).unwrap();
        assert!(shared.register(&mut registry).is_err());
        assert!(registry.definition("load_skill").is_none());
        let names = Arc::new(Mutex::new(Vec::new()));
        let mut agent = Agent::new(
            SessionStore::open(dir.path().join("s")).unwrap(),
            Box::new(NamesBackend(names.clone())),
        );
        let mut old = ToolRegistry::new();
        old.register(NamedTool("original")).unwrap();
        agent.set_tools(
            old,
            ToolContext::new(dir.path()).unwrap(),
            SideEffectPolicy::read_only(),
        );
        assert!(
            agent
                .set_tools_with_resources(
                    registry,
                    ToolContext::new(dir.path()).unwrap(),
                    SideEffectPolicy::read_only()
                )
                .is_err()
        );
        agent.process(TurnInputRequest::new("inspect")).unwrap();
        assert_eq!(*names.lock().unwrap(), ["original"]);
    }

    #[test]
    fn existing_blueprint_grants_cannot_authorize_cross_root_skill_tools() {
        let dir = tempdir().unwrap();
        let skills = fixture(dir.path(), "SECRET");
        let shared = ModelSkillTools::default();
        let mut registry = ToolRegistry::new();
        shared.register(&mut registry).unwrap();
        let context = ToolContext::new(dir.path()).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let policy = BlueprintPolicySpec {
            blueprint_digest: "a".repeat(64),
            goal_digest: "b".repeat(64),
            item_id: "ZS1-113".into(),
            allowed_tools: BTreeSet::from(["read_file".into()]),
            readable_paths: BTreeSet::from([".".into()]),
            writable_paths: BTreeSet::new(),
            denied_paths: BTreeSet::new(),
            protected_paths: BTreeSet::new(),
            allowed_commands: BTreeSet::new(),
            denied_commands: BTreeSet::new(),
            network_hosts: BTreeSet::new(),
            max_actions: 10,
            max_command_timeout_ms: 1000,
            max_command_output_bytes: 1024,
        };
        let (gate, _) = BlueprintGate::compile(
            &context,
            policy,
            BlueprintLease {
                lease_id: "fixture".into(),
                issued_at_ms: now,
                expires_at_ms: now + 10000,
            },
            now,
        )
        .unwrap();
        let gated = context.with_blueprint_gate(gate).unwrap();
        let _scope = shared.begin_turn(skills, BTreeSet::new()).unwrap();
        let result = invoke(&registry, &gated, "load_skill", json!({"name":"reader"}));
        assert!(
            matches!(result,ToolResult::Error{error,..} if error.code==ToolErrorCode::PolicyDenied)
        );
        assert!(shared.loaded_metadata("reader").unwrap().is_none());
    }

    fn read_request(stream: &mut TcpStream) -> Value {
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut bytes = Vec::new();
        let end = loop {
            let mut chunk = [0; 4096];
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                break end + 4;
            }
            assert!(bytes.len() < 65536);
        };
        let len: usize = String::from_utf8_lossy(&bytes[..end])
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(str::to_owned)
            })
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(len < 2 * 1024 * 1024);
        while bytes.len() < end + len {
            let mut chunk = [0; 4096];
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
        }
        serde_json::from_slice(&bytes[end..end + len]).unwrap()
    }
    fn tool_reply(name: &str, args: Value) -> Value {
        json!({"role":"assistant","content":null,"tool_calls":[{"id":format!("call-{name}"),"type":"function","function":{"name":name,"arguments":args.to_string()}}]})
    }
    fn final_reply() -> Value {
        json!({"role":"assistant","content":"ok"})
    }
    fn server(replies: Vec<Value>) -> (String, Arc<Mutex<Vec<Value>>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let capture = requests.clone();
        let handle = thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            for (index, mut reply) in replies.into_iter().enumerate() {
                let deadline = std::time::Instant::now() + Duration::from_secs(12);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && std::time::Instant::now() < deadline =>
                        {
                            thread::sleep(Duration::from_millis(5))
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                let request = read_request(&mut stream);
                if let Some(calls) = reply.get_mut("tool_calls").and_then(Value::as_array_mut) {
                    for (offset, call) in calls.iter_mut().enumerate() {
                        call["id"] = json!(format!("call-{index}-{offset}"));
                    }
                }
                let body=json!({"id":format!("r-{index}"),"model":request["model"],"choices":[{"index":0,"finish_reason":if reply.get("tool_calls").is_some(){"tool_calls"}else{"stop"},"message":reply}]}).to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                stream.flush().unwrap();
                capture.lock().unwrap().push(request);
            }
        });
        (url, requests, handle)
    }
    fn last_tool(request: &Value) -> Value {
        let content = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|message| message["role"] == "tool")
            .unwrap()["content"]
            .as_str()
            .unwrap();
        serde_json::from_str(content).unwrap()
    }
    struct Cli {
        child: Child,
        input: ChildStdin,
        lines: std::sync::mpsc::Receiver<Value>,
        reader: Option<thread::JoinHandle<()>>,
    }
    impl Cli {
        fn start(root: &Path, url: &str) -> Self {
            fs::create_dir_all(root.join("user")).unwrap();
            fs::write(root.join("user/config.toml"),format!("backend='openai'\nprovider='openai'\nmodel='gpt-4.1'\nbase_url='{url}'\nwire_api='chat'\nrequires_openai_auth=false\n")).unwrap();
            let mut command = Command::new(env!("CARGO_BIN_EXE_zenpi"));
            command
                .args(["--headless", "--session", "cli.jsonl"])
                .current_dir(root)
                .env("ZENPI_HOME", root.join("user"));
            for key in [
                "ZENPI_MODEL",
                "OPENAI_MODEL",
                "ZENPI_PROFILE",
                "ZENPI_BASE_URL",
                "OPENAI_BASE_URL",
                "ZENPI_WIRE_API",
                "ZENPI_BACKEND",
                "ZENPI_PROVIDER",
                "ZENPI_MODEL_REASONING_EFFORT",
                "ZENPI_MODEL_VERBOSITY",
                "OPENAI_API_KEY",
            ] {
                command.env_remove(key);
            }
            let mut child = command
                .env("ZENPI_API_KEY", "fixture-key")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            let input = child.stdin.take().unwrap();
            let stdout = child.stdout.take().unwrap();
            let (send, lines) = std::sync::mpsc::channel();
            let reader = thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let value = serde_json::from_str(&line.unwrap()).unwrap();
                    if send.send(value).is_err() {
                        break;
                    }
                }
            });
            Self {
                child,
                input,
                lines,
                reader: Some(reader),
            }
        }
        fn send(&mut self, id: &str, text: &str) -> Value {
            writeln!(self.input,"{}",json!({"type":if text.starts_with('/') {"command"}else{"prompt"},"id":id,"text":text})).unwrap();
            self.input.flush().unwrap();
            loop {
                let value = self.lines.recv_timeout(Duration::from_secs(15)).unwrap();
                if value["id"] == id && value.get("success").is_some() {
                    return value;
                }
            }
        }
    }
    impl Drop for Cli {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
        }
    }

    #[test]
    fn actual_production_model_loads_body_and_relative_resource_but_cannot_load_disabled_skill() {
        let dir = tempdir().unwrap();
        fixture(&dir.path().join(".zenpi/skills"), "MODEL_BODY_SENTINEL");
        let (url, requests, server) = server(vec![
            tool_reply("load_skill", json!({"name":"reader"})),
            tool_reply(
                "read_skill_resource",
                json!({"skill":"reader","path":"references/source.md"}),
            ),
            tool_reply("load_skill", json!({"name":"manual"})),
            final_reply(),
        ]);
        let mut cli = Cli::start(dir.path(), &url);
        let reply = cli.send(
            "prompt",
            "Use the available reader skill and its reference.",
        );
        assert_eq!(reply["success"], true, "{reply}");
        server.join().unwrap();
        let requests = requests.lock().unwrap();
        assert!(
            requests[0]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["function"]["name"] == "load_skill")
        );
        assert!(!requests[0].to_string().contains("MODEL_BODY_SENTINEL"));
        assert_eq!(
            last_tool(&requests[1])["output"]["body"],
            "MODEL_BODY_SENTINEL"
        );
        assert_eq!(
            last_tool(&requests[2])["output"]["resource"]["content"],
            "RESOURCE_SENTINEL"
        );
        assert_eq!(last_tool(&requests[3])["status"], "error");
        assert!(!requests[3].to_string().contains("MANUAL_SENTINEL"));
        let journal = fs::read_to_string(dir.path().join("cli.jsonl")).unwrap();
        assert!(journal.contains("source_hash") && journal.contains("MODEL_BODY_SENTINEL"));
        assert!(!journal.contains("MANUAL_SENTINEL"));
        let store = SessionStore::open_existing(dir.path().join("cli.jsonl")).unwrap();
        assert!(store.turns().iter().any(|turn| {
            turn.metadata.as_ref().is_some_and(|metadata| {
                metadata["resource"]["invocation"] == "model"
                    && metadata["resource"]["name"] == "reader"
            })
        }));
    }

    #[test]
    fn actual_production_restart_requires_reload_for_changed_model_loaded_skill() {
        let dir = tempdir().unwrap();
        let skill_root = dir.path().join(".zenpi/skills");
        fixture(&skill_root, "BODY_V1");
        let (url, _, handle) = server(vec![
            tool_reply("load_skill", json!({"name":"reader"})),
            final_reply(),
        ]);
        let mut cli = Cli::start(dir.path(), &url);
        assert_eq!(cli.send("first", "Use reader")["success"], true);
        handle.join().unwrap();
        drop(cli);
        fixture(&skill_root, "BODY_V2");
        let (url, requests, handle) = server(vec![
            tool_reply("load_skill", json!({"name":"reader"})),
            final_reply(),
            tool_reply("load_skill", json!({"name":"reader"})),
            final_reply(),
        ]);
        let mut cli = Cli::start(dir.path(), &url);
        assert_eq!(cli.send("stale", "Use reader again")["success"], true);
        assert_eq!(cli.send("reload", "/reload")["success"], true);
        assert_eq!(cli.send("fresh", "Use reader now")["success"], true);
        handle.join().unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(last_tool(&requests[1])["status"], "error");
        assert!(
            last_tool(&requests[1])
                .to_string()
                .contains("reload required")
        );
        assert_eq!(last_tool(&requests[3])["output"]["body"], "BODY_V2");
    }

    #[test]
    fn admitted_model_skill_generation_stays_selected_across_reload_until_next_turn() {
        let dir = tempdir().unwrap();
        fixture(&dir.path().join("a"), "GENERATION_A");
        fixture(&dir.path().join("b"), "GENERATION_B");
        let (url, requests, handle) = server(vec![
            tool_reply("load_skill", json!({"name":"reader"})),
            final_reply(),
            tool_reply("load_skill", json!({"name":"reader"})),
            final_reply(),
        ]);
        let backend = zenpi::backend::OpenAiCompatibleBackend::new(&url, None, "gpt-4.1")
            .unwrap()
            .with_model_registry(
                "openai".into(),
                zenpi::providers::registry::ModelRegistry::default(),
            )
            .unwrap();
        let mut agent = Agent::new(
            SessionStore::open(dir.path().join("s")).unwrap(),
            Box::new(backend),
        );
        agent
            .set_tools_with_resources(
                ToolRegistry::with_all_builtins().unwrap(),
                ToolContext::new(dir.path()).unwrap(),
                SideEffectPolicy::read_only(),
            )
            .unwrap();
        let paths = |name: &str| zenpi::resource_loader::ResourcePaths {
            user_skills: dir.path().join(name),
            project_skills: dir.path().join("empty"),
            skill_paths: vec![],
            user_templates: dir.path().join("empty-prompts"),
            project_templates: dir.path().join("empty-prompts"),
            template_paths: vec![],
            text_resources: vec![],
        };
        agent.configure_resources(paths("a"), || false).unwrap();
        agent
            .submit(TurnInputRequest::new("read the skill"))
            .unwrap();
        agent.configure_resources(paths("b"), || false).unwrap();
        agent.run_active_turn().unwrap();
        agent.process(TurnInputRequest::new("read again")).unwrap();
        handle.join().unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(last_tool(&requests[1])["output"]["body"], "GENERATION_A");
        assert_eq!(last_tool(&requests[3])["output"]["body"], "GENERATION_B");
    }

    #[test]
    fn same_batch_model_load_then_relative_read_uses_sequential_execution() {
        let dir = tempdir().unwrap();
        fixture(&dir.path().join(".zenpi/skills"), "BATCH_BODY");
        let mut both = tool_reply("load_skill", json!({"name":"reader"}));
        both["tool_calls"].as_array_mut().unwrap().extend(
            tool_reply(
                "read_skill_resource",
                json!({"skill":"reader","path":"references/source.md"}),
            )["tool_calls"]
                .as_array()
                .unwrap()
                .clone(),
        );
        let (url, requests, handle) = server(vec![both, final_reply()]);
        let mut cli = Cli::start(dir.path(), &url);
        let reply = cli.send("batch", "read skill and reference");
        assert_eq!(reply["success"], true, "{reply}");
        handle.join().unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(
            last_tool(&requests[1])["output"]["resource"]["content"],
            "RESOURCE_SENTINEL"
        );
    }

    #[test]
    fn per_turn_loaded_skill_count_is_bounded_and_guard_reset_restores_capacity() {
        let dir = tempdir().unwrap();
        for index in 0..33 {
            let path = dir.path().join(format!("skill-{index}"));
            fs::create_dir_all(&path).unwrap();
            fs::write(
                path.join("SKILL.md"),
                format!("---\nname: skill-{index}\ndescription: fixture\n---\nbody"),
            )
            .unwrap();
        }
        let skills = SkillSet::load(dir.path(), &dir.path().join("none")).unwrap();
        let shared = ModelSkillTools::default();
        let mut registry = ToolRegistry::new();
        shared.register(&mut registry).unwrap();
        let context = ToolContext::new(dir.path()).unwrap();
        let scope = shared.begin_turn(skills.clone(), BTreeSet::new()).unwrap();
        for index in 0..32 {
            assert!(
                invoke(
                    &registry,
                    &context,
                    "load_skill",
                    json!({"name":format!("skill-{index}")})
                )
                .is_success()
            );
        }
        assert!(
            matches!(invoke(&registry,&context,"load_skill",json!({"name":"skill-32"})),ToolResult::Error{error,..} if error.code==ToolErrorCode::LimitExceeded)
        );
        drop(scope);
        let _next = shared.begin_turn(skills, BTreeSet::new()).unwrap();
        assert!(
            invoke(
                &registry,
                &context,
                "load_skill",
                json!({"name":"skill-32"})
            )
            .is_success()
        );
    }

    #[test]
    #[ignore = "host fixture: set ZENPI_SKILL_FIXTURE_ROOT to actual installed skills"]
    fn actual_installed_learn_execution_are_model_loaded_with_real_relative_resource() {
        let root = std::path::PathBuf::from(
            std::env::var_os("ZENPI_SKILL_FIXTURE_ROOT").expect("actual skill root"),
        );
        let dir = tempdir().unwrap();
        let missing = dir.path().join("missing");
        let paths = vec![
            root.join("learn-cron-builder/SKILL.md"),
            root.join("execution-cron-builder/SKILL.md"),
        ];
        let skills = SkillSet::load_with_paths(&missing, &missing, &paths, || false).unwrap();
        let learn = skills
            .load_body("learn-cron-builder", SkillInvocation::Model, "", || false)
            .unwrap();
        let execution = skills
            .load_body("execution-cron-builder", SkillInvocation::Model, "", || {
                false
            })
            .unwrap();
        let reference = learn
            .read_resource("references/coverage-contract.md", || false)
            .unwrap();
        let selection = zenpi::resource_loader::ResourcePaths {
            user_skills: missing.clone(),
            project_skills: missing.clone(),
            skill_paths: paths,
            user_templates: missing.clone(),
            project_templates: missing,
            template_paths: vec![],
            text_resources: vec![],
        };
        fs::write(
            dir.path().join("selection.json"),
            serde_json::to_vec(&selection).unwrap(),
        )
        .unwrap();
        let (url, requests, handle) = server(vec![
            tool_reply("load_skill", json!({"name":"learn-cron-builder"})),
            tool_reply(
                "read_skill_resource",
                json!({"skill":"learn-cron-builder","path":"references/coverage-contract.md"}),
            ),
            tool_reply("load_skill", json!({"name":"execution-cron-builder"})),
            final_reply(),
        ]);
        let mut cli = Cli::start(dir.path(), &url);
        assert_eq!(
            cli.send("reload", "/reload selection.json")["success"],
            true
        );
        let reply = cli.send(
            "prompt",
            "Read the learn and execution skill instructions and their coverage reference.",
        );
        assert_eq!(reply["success"], true, "{reply}");
        handle.join().unwrap();
        let requests = requests.lock().unwrap();
        assert!(
            !requests[0]["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains(&learn.body)
        );
        assert!(
            !requests[0]["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains(&execution.body)
        );
        assert_eq!(last_tool(&requests[1])["output"]["body"], learn.body);
        assert_eq!(
            last_tool(&requests[2])["output"]["resource"]["content"],
            reference.content
        );
        assert_eq!(last_tool(&requests[3])["output"]["body"], execution.body);
    }
}
