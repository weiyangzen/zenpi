#[path = "../src/search.rs"]
#[allow(dead_code)]
mod search;
#[path = "../src/tool_output.rs"]
#[allow(dead_code)]
mod tool_output;
use zenpi::backend;
use zenpi::view_model;

#[path = "../src/dag.rs"]
#[allow(dead_code)]
mod dag;
#[path = "../src/security.rs"]
#[allow(dead_code)]
mod security;
#[path = "../src/tools.rs"]
#[allow(dead_code)]
mod tools;

use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use serde_json::{Map, Value, json};
use tempfile::tempdir;
use tools::{
    SideEffectPolicy, Tool, ToolCall, ToolContext, ToolDefinition, ToolError, ToolErrorCode,
    ToolRegistry, ToolResult, ToolSideEffect,
};

fn call(name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        id: "call-1".into(),
        name: name.into(),
        arguments,
    }
}

fn successful_output(result: ToolResult) -> Value {
    match result {
        ToolResult::Success { output, .. } => output,
        ToolResult::Error { error, .. } => panic!("unexpected tool error: {error:?}"),
    }
}

fn error_code(result: ToolResult) -> ToolErrorCode {
    match result {
        ToolResult::Error { error, .. } => error.code,
        ToolResult::Success { output, .. } => panic!("unexpected tool success: {output}"),
    }
}

#[test]
fn builtins_publish_typed_object_schemas() {
    let registry = ToolRegistry::with_read_only_builtins().unwrap();
    let definitions = registry.definitions();
    assert_eq!(definitions.len(), 5);
    assert_eq!(
        definitions
            .iter()
            .map(|definition| definition.name.as_str())
            .collect::<Vec<_>>(),
        [
            "find",
            "list_directory",
            "read_file",
            "read_tool_output",
            "search_text"
        ]
    );
    assert!(definitions.iter().all(|definition| {
        definition.input_schema["type"] == "object"
            && definition.side_effect == ToolSideEffect::ReadOnly
    }));
    assert_eq!(
        registry
            .definition("read_file")
            .map(|definition| definition.name.as_str()),
        Some("read_file")
    );
}

#[test]
fn read_file_is_bounded_and_workspace_scoped() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("hello.txt"), "hello world").unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let registry = ToolRegistry::with_read_only_builtins().unwrap();

    let output = successful_output(registry.execute(
        &context,
        SideEffectPolicy::default(),
        call("read_file", json!({ "path": "hello.txt", "max_bytes": 5 })),
    ));
    assert_eq!(output["content"], "hello");
    assert_eq!(output["truncated"], true);

    let denied = registry.execute(
        &context,
        SideEffectPolicy::default(),
        call("read_file", json!({ "path": "../outside.txt" })),
    );
    assert_eq!(error_code(denied), ToolErrorCode::PathDenied);
}

#[cfg(unix)]
#[test]
fn read_file_rejects_a_symlink_that_escapes_the_workspace() {
    use std::os::unix::fs::symlink;

    let workspace = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    symlink(
        outside.path().join("secret.txt"),
        workspace.path().join("escape.txt"),
    )
    .unwrap();
    let context = ToolContext::new(workspace.path()).unwrap();
    let registry = ToolRegistry::with_read_only_builtins().unwrap();

    let result = registry.execute(
        &context,
        SideEffectPolicy::read_only(),
        call("read_file", json!({ "path": "escape.txt" })),
    );
    assert_eq!(error_code(result), ToolErrorCode::PathDenied);
}

#[test]
fn list_directory_is_sorted_and_reports_truncation() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("z.txt"), "z").unwrap();
    fs::write(directory.path().join("a.txt"), "a").unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let registry = ToolRegistry::with_read_only_builtins().unwrap();

    let output = successful_output(registry.execute(
        &context,
        SideEffectPolicy::read_only(),
        call("list_directory", json!({ "max_entries": 1 })),
    ));
    assert_eq!(output["entries"][0]["name"], "a.txt");
    assert_eq!(output["truncated"], true);
}

#[cfg(unix)]
#[test]
fn list_directory_describes_symlinks_without_following_them() {
    use std::os::unix::fs::symlink;

    let workspace = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    symlink(
        outside.path().join("secret.txt"),
        workspace.path().join("link.txt"),
    )
    .unwrap();
    let context = ToolContext::new(workspace.path()).unwrap();
    let registry = ToolRegistry::with_read_only_builtins().unwrap();

    let output = successful_output(registry.execute(
        &context,
        SideEffectPolicy::read_only(),
        call("list_directory", json!({})),
    ));
    assert_eq!(output["entries"][0]["kind"], "symlink");
    assert!(output["entries"][0].get("size").is_some_and(Value::is_null));
}

#[test]
fn search_is_literal_case_configurable_and_bounded() {
    let directory = tempdir().unwrap();
    fs::create_dir(directory.path().join("src")).unwrap();
    fs::write(
        directory.path().join("src/one.txt"),
        "Needle.* literal\nneedle.* second\n",
    )
    .unwrap();
    fs::write(directory.path().join("src/two.txt"), "needle.* third\n").unwrap();
    fs::create_dir(directory.path().join("target")).unwrap();
    fs::write(directory.path().join("target/ignored.txt"), "needle.*\n").unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let registry = ToolRegistry::with_read_only_builtins().unwrap();

    let output = successful_output(registry.execute(
        &context,
        SideEffectPolicy::read_only(),
        call(
            "search_text",
            json!({
                "query": "needle.*",
                "case_sensitive": false,
                "max_matches": 2
            }),
        ),
    ));
    assert_eq!(output["matches"].as_array().unwrap().len(), 2);
    assert_eq!(output["matches"][0]["path"], "src/one.txt");
    assert_eq!(output["truncated"], true);
}

#[derive(Clone)]
struct SideEffectProbe {
    invoked: Arc<AtomicBool>,
    side_effect: ToolSideEffect,
}

impl Tool for SideEffectProbe {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "probe".into(),
            description: "Test whether policy is checked before invocation.".into(),
            input_schema: json!({ "type": "object" }),
            side_effect: self.side_effect,
        }
    }

    fn invoke(&self, _: &ToolContext, _: &Map<String, Value>) -> Result<Value, ToolError> {
        self.invoked.store(true, Ordering::SeqCst);
        Ok(json!({ "ok": true }))
    }
}

#[test]
fn side_effects_are_denied_before_handler_execution() {
    let directory = tempdir().unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let invoked = Arc::new(AtomicBool::new(false));
    let mut registry = ToolRegistry::new();
    registry
        .register(SideEffectProbe {
            invoked: Arc::clone(&invoked),
            side_effect: ToolSideEffect::CommandExecution,
        })
        .unwrap();

    let denied = registry.execute(
        &context,
        SideEffectPolicy::default().with_workspace_writes(true),
        call("probe", json!({})),
    );
    assert_eq!(error_code(denied), ToolErrorCode::PolicyDenied);
    assert!(!invoked.load(Ordering::SeqCst));

    let allowed = registry.execute(
        &context,
        SideEffectPolicy::default().with_command_execution(true),
        call("probe", json!({})),
    );
    assert!(allowed.is_success());
    assert!(invoked.load(Ordering::SeqCst));
}

#[test]
fn malformed_calls_and_unknown_arguments_are_typed_errors() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("file.txt"), "content").unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let registry = ToolRegistry::with_read_only_builtins().unwrap();

    assert_eq!(
        error_code(registry.execute(
            &context,
            SideEffectPolicy::default(),
            call("read_file", json!(["file.txt"])),
        )),
        ToolErrorCode::InvalidArguments
    );
    assert_eq!(
        error_code(registry.execute(
            &context,
            SideEffectPolicy::default(),
            call("read_file", json!({ "path": "file.txt", "surprise": true })),
        )),
        ToolErrorCode::InvalidArguments
    );
    assert_eq!(
        error_code(registry.execute(
            &context,
            SideEffectPolicy::default(),
            call("missing", json!({})),
        )),
        ToolErrorCode::UnknownTool
    );
}

#[test]
fn write_and_edit_tools_are_atomic_and_policy_guarded() {
    let directory = tempdir().unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let denied = registry.execute(
        &context,
        SideEffectPolicy::read_only(),
        call("write_file", json!({"path":"new.txt","content":"one"})),
    );
    assert_eq!(error_code(denied), ToolErrorCode::PolicyDenied);
    let policy = SideEffectPolicy::all_builtins();
    let preview = tools::WriteFileTool::preview(
        &context,
        json!({"path":"new.txt","content":"one"})
            .as_object()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(preview["changed"], true);
    assert!(
        preview["diff"]
            .as_str()
            .unwrap()
            .contains("--- new.txt\n+++ new.txt\n")
    );
    let typed_preview = registry
        .approval_preview(
            &context,
            &call("write_file", json!({"path":"new.txt","content":"one"})),
        )
        .unwrap()
        .unwrap();
    assert!(matches!(
        typed_preview,
        tools::ToolPreview::Diff {
            ref path,
            ref patch,
            changed: true,
            truncated: false,
            before_bytes: 0,
            after_bytes: 3,
            ..
        } if path == "new.txt" && patch.contains("+one\n")
    ));
    let stale_call = call(
        "write_file",
        json!({"path":"stale.txt","content":"approved"}),
    );
    let stale_preview = registry
        .approval_preview(&context, &stale_call)
        .unwrap()
        .unwrap();
    fs::write(directory.path().join("stale.txt"), "changed while waiting").unwrap();
    let stale = registry.execute_approved(&context, policy, stale_call, Some(&stale_preview));
    assert_eq!(error_code(stale), ToolErrorCode::StalePreview);
    assert_eq!(
        fs::read_to_string(directory.path().join("stale.txt")).unwrap(),
        "changed while waiting"
    );
    let created = successful_output(registry.execute(
        &context,
        policy,
        call("write_file", json!({"path":"new.txt","content":"one"})),
    ));
    assert_eq!(created["bytes"], 3);
    assert!(created["diff"].as_str().unwrap().contains("+one\n"));
    assert_eq!(
        fs::read_to_string(directory.path().join("new.txt")).unwrap(),
        "one"
    );
    let edited = successful_output(registry.execute(
        &context,
        policy,
        call(
            "edit_file",
            json!({"path":"new.txt","old":"one","new":"two"}),
        ),
    ));
    assert_eq!(edited["replacements"], 1);
    let edit_diff = edited["diff"].as_str().unwrap();
    assert!(edit_diff.contains("-one\n"));
    assert!(edit_diff.contains("+two\n"));
    assert_eq!(
        fs::read_to_string(directory.path().join("new.txt")).unwrap(),
        "two"
    );
    let edit_preview = registry
        .approval_preview(
            &context,
            &call(
                "edit_file",
                json!({"path":"new.txt","old":"two","new":"three"}),
            ),
        )
        .unwrap()
        .unwrap();
    assert!(matches!(
        edit_preview,
        tools::ToolPreview::Diff {
            ref patch,
            changed: true,
            ..
        } if patch.contains("-two\n") && patch.contains("+three\n")
    ));
}

#[test]
fn write_preview_rejects_binary_existing_sources_without_writing() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("binary.dat");
    fs::write(&path, [0_u8, 159, 255, 1]).unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let call = call(
        "write_file",
        json!({"path":"binary.dat","content":"replacement"}),
    );
    let preview = registry.approval_preview(&context, &call).unwrap_err();
    assert_eq!(preview.code(), ToolErrorCode::InvalidUtf8);
    assert_eq!(fs::read(&path).unwrap(), [0_u8, 159, 255, 1]);
    let direct = registry.execute(&context, SideEffectPolicy::all_builtins(), call);
    assert_eq!(error_code(direct), ToolErrorCode::InvalidUtf8);
    assert_eq!(fs::read(&path).unwrap(), [0_u8, 159, 255, 1]);
}

#[test]
fn write_to_directory_fails_without_leaving_atomic_temporary_files() {
    let directory = tempdir().unwrap();
    let target = directory.path().join("destination");
    fs::create_dir(&target).unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let result = registry.execute(
        &context,
        SideEffectPolicy::all_builtins(),
        call("write_file", json!({"path":"destination","content":"new"})),
    );
    assert_eq!(error_code(result), ToolErrorCode::NotAFile);
    assert!(target.is_dir());
    let leftovers = fs::read_dir(directory.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".destination.zenpi-")
        })
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn write_diff_is_empty_when_content_is_unchanged() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("same.txt"), "same\n").unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let output = successful_output(registry.execute(
        &context,
        SideEffectPolicy::all_builtins(),
        call("write_file", json!({"path":"same.txt","content":"same\n"})),
    ));
    assert_eq!(output["diff"], "");
    assert_eq!(output["diff_truncated"], false);
}

#[test]
fn write_diff_represents_final_newline_changes_as_a_hunk() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("newline.txt"), "same").unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let output = successful_output(registry.execute(
        &context,
        SideEffectPolicy::all_builtins(),
        call(
            "write_file",
            json!({"path":"newline.txt","content":"same\n"}),
        ),
    ));
    let diff = output["diff"].as_str().unwrap();
    assert!(diff.contains("-same\n"));
    assert!(diff.contains("+same\n"));
    assert!(diff.contains("\\ No newline at end of file\n"));
}

#[test]
fn write_diff_is_bounded_for_large_replacements() {
    let directory = tempdir().unwrap();
    fs::write(
        directory.path().join("large.txt"),
        "old-line\n".repeat(tools::MAX_WRITE_BYTES / 9),
    )
    .unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    // Keep the request itself below MAX_TOOL_CALL_BYTES while making the
    // before+after diff larger than the response cap.
    let content = "new-line\n".repeat(6_000);
    let output = successful_output(registry.execute(
        &context,
        SideEffectPolicy::all_builtins(),
        call("write_file", json!({"path":"large.txt","content":content})),
    ));
    let diff = output["diff"].as_str().unwrap();
    assert!(diff.len() <= tools::MAX_DIFF_BYTES);
    assert_eq!(output["diff_truncated"], true);
    assert!(diff.contains("[diff truncated]"));
}

#[test]
fn edit_rejects_a_source_larger_than_the_write_budget() {
    let directory = tempdir().unwrap();
    let original = format!("{}needle\n", "x".repeat(tools::MAX_WRITE_BYTES));
    let path = directory.path().join("oversized.txt");
    fs::write(&path, &original).unwrap();
    assert!(original.len() > tools::MAX_WRITE_BYTES);

    let context = ToolContext::new(directory.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let result = registry.execute(
        &context,
        SideEffectPolicy::all_builtins(),
        call(
            "edit_file",
            json!({"path":"oversized.txt","old":"needle","new":"changed"}),
        ),
    );

    assert_eq!(error_code(result), ToolErrorCode::LimitExceeded);
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn command_tool_is_bounded_and_scrubs_environment() {
    let directory = tempdir().unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let policy = SideEffectPolicy::read_only().with_command_execution(true);
    let output = successful_output(registry.execute(
        &context,
        policy,
        call("run_command", json!({"command":"printf ok"})),
    ));
    assert_eq!(output["stdout"], "ok");
    assert_eq!(output["timeout_ms"], 30_000);
    let timed_out = registry.execute(
        &context,
        policy,
        call("run_command", json!({"command":"sleep 1","timeout_ms":10})),
    );
    assert_eq!(error_code(timed_out), ToolErrorCode::CommandTimeout);
}

#[cfg(unix)]
#[test]
fn command_timeout_accepts_long_validations_but_remains_bounded() {
    let directory = tempdir().unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let tool = tools::RunCommandTool;
    let output = tool
        .invoke(
            &context,
            json!({"command":"printf ok", "timeout_ms":600_000})
                .as_object()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(output["stdout"], "ok");
    assert_eq!(output["timeout_ms"], 600_000);
    let schema = tool.definition().input_schema;
    assert_eq!(schema["properties"]["timeout_ms"]["maximum"], 600_000);
    for timeout_ms in [0, 600_001] {
        let error = tool
            .invoke(
                &context,
                json!({"command":"touch must-not-run", "timeout_ms":timeout_ms})
                    .as_object()
                    .unwrap(),
            )
            .unwrap_err();
        assert_eq!(error.code(), ToolErrorCode::InvalidArguments);
    }
    assert!(!directory.path().join("must-not-run").exists());
}

#[cfg(unix)]
#[test]
#[ignore = "runs a real 121-second command to cross the former timeout ceiling"]
fn command_can_run_beyond_two_minutes() {
    let directory = tempdir().unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let began = std::time::Instant::now();
    let output = tools::RunCommandTool
        .invoke(
            &context,
            json!({"command":"sleep 121; printf completed", "timeout_ms":150_000})
                .as_object()
                .unwrap(),
        )
        .unwrap();
    assert!(began.elapsed() >= std::time::Duration::from_secs(121));
    assert_eq!(output["stdout"], "completed");
    assert_eq!(output["exit_code"], 0);
    assert_eq!(output["timed_out"], false);
    assert_eq!(output["child_reaped"], true);
}

#[cfg(unix)]
#[test]
fn timed_out_command_reaps_its_descendant_process_group() {
    let directory = tempdir().unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let marker = directory.path().join("descendant-ran");
    let arguments = json!({
        "command": "(sleep 0.3; printf leaked > descendant-ran) & wait",
        "timeout_ms": 20
    });
    let error = tools::RunCommandTool::invoke_with_cancel(
        &context,
        arguments.as_object().unwrap(),
        &|| false,
    )
    .unwrap_err();
    assert_eq!(error.code(), ToolErrorCode::CommandTimeout);
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(!marker.exists(), "descendant survived command timeout");
}

#[derive(Clone, Copy)]
struct LargeOutputTool;

impl Tool for LargeOutputTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "large_output".into(),
            description: "Return a large deterministic payload.".into(),
            input_schema: json!({"type":"object","additionalProperties":false}),
            side_effect: ToolSideEffect::ReadOnly,
        }
    }

    fn invoke(&self, _: &ToolContext, _: &Map<String, Value>) -> Result<Value, ToolError> {
        Ok(json!({"text": "x".repeat(tools::MAX_INLINE_TOOL_RESULT_BYTES + 1024)}))
    }
}

#[test]
fn large_tool_results_become_private_retrievable_artifacts() {
    let directory = tempdir().unwrap();
    let context = ToolContext::new(directory.path()).unwrap();
    let mut registry = ToolRegistry::new();
    registry.register(LargeOutputTool).unwrap();
    let output = successful_output(registry.execute_compact(
        &context,
        SideEffectPolicy::read_only(),
        call("large_output", json!({})),
    ));
    assert_eq!(output["compacted"], true);
    let artifact = output["artifact"].as_str().unwrap();
    assert!(artifact.starts_with(".zenpi/artifacts/"));
    let path = directory.path().join(artifact);
    assert!(path.is_file());
    assert_eq!(
        output["bytes"].as_u64().unwrap(),
        fs::metadata(&path).unwrap().len()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

fn worker_policy() -> tools::BlueprintPolicySpec {
    tools::BlueprintPolicySpec {
        blueprint_digest: "a".repeat(64),
        goal_digest: "b".repeat(64),
        item_id: "CF-404".into(),
        allowed_tools: [
            "read_file",
            "list_directory",
            "search_text",
            "write_file",
            "edit_file",
            "run_command",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        readable_paths: [".".into()].into(),
        writable_paths: ["src".into()].into(),
        denied_paths: ["private".into()].into(),
        protected_paths: ["policy.json".into()].into(),
        allowed_commands: ["echo safe", "true"].map(str::to_owned).into(),
        denied_commands: Default::default(),
        network_hosts: Default::default(),
        max_actions: 100,
        max_command_timeout_ms: 1000,
        max_command_output_bytes: 1024,
    }
}

fn worker_context(
    root: &std::path::Path,
    spec: tools::BlueprintPolicySpec,
) -> (ToolContext, tools::BlueprintRevocation) {
    let context = ToolContext::new(root).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let lease = tools::BlueprintLease {
        lease_id: "lease-1".into(),
        issued_at_ms: now,
        expires_at_ms: now + 10_000,
    };
    let (gate, revoke) = tools::BlueprintGate::compile(&context, spec, lease, now).unwrap();
    (context.with_blueprint_gate(gate).unwrap(), revoke)
}

#[cfg(unix)]
#[test]
fn longer_host_ceiling_does_not_override_worker_timeout_budget() {
    let root = tempdir().unwrap();
    for budget in [1_000, 600_000] {
        let mut spec = worker_policy();
        spec.max_command_timeout_ms = budget;
        let (context, _) = worker_context(root.path(), spec);
        let result = tools::RunCommandTool.invoke(
            &context,
            json!({"command":"echo safe", "timeout_ms":600_000})
                .as_object()
                .unwrap(),
        );
        if budget == 1_000 {
            assert_eq!(result.unwrap_err().code(), ToolErrorCode::PolicyDenied);
        } else {
            assert_eq!(result.unwrap()["stdout"], "safe\n");
        }
    }
}

#[test]
fn worker_gate_is_immutable_digest_bound_and_deny_over_allow() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("private")).unwrap();
    fs::write(root.path().join("private/secret.txt"), "hidden").unwrap();
    fs::write(root.path().join(".env"), "SECRET=hidden").unwrap();
    let mut spec = worker_policy();
    let (context, _) = worker_context(root.path(), spec.clone());
    spec.denied_paths.clear();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    for path in ["private/secret.txt", "./private/secret.txt", ".env"] {
        let result = registry.execute(
            &context,
            SideEffectPolicy::all_builtins(),
            call("read_file", json!({"path": path})),
        );
        assert_eq!(error_code(result), ToolErrorCode::PolicyDenied);
    }
    for path in ["outside.txt", "src/.env", "src/Blueprint.md", "policy.json"] {
        assert_eq!(
            error_code(registry.execute(
                &context,
                SideEffectPolicy::all_builtins(),
                call("write_file", json!({"path": path,"content":"denied"}))
            )),
            ToolErrorCode::PolicyDenied
        );
        assert!(!root.path().join(path).exists());
    }
    let result = successful_output(registry.execute(
        &context,
        SideEffectPolicy::all_builtins(),
        call("write_file", json!({"path":"src/ok.txt","content":"ok"})),
    ));
    assert_eq!(
        fs::read_to_string(root.path().join("src/ok.txt")).unwrap(),
        "ok"
    );
    let evidence = context.policy_evidence().unwrap();
    assert_eq!(evidence.policy_digest.len(), 64);
    assert_eq!(
        result["policy_evidence"]["policy_digest"],
        evidence.policy_digest
    );
    assert_eq!(result["policy_evidence"]["lease_id"], "lease-1");
    assert_eq!(result["policy_evidence"]["origin"], "blueprint_worker");
}

#[test]
fn worker_gate_rejects_unbound_origin_cross_origin_and_revocation() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("ok.txt"), "ok").unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let unbound = ToolContext::new(root.path())
        .unwrap()
        .with_origin(tools::ToolOrigin::BlueprintWorker);
    assert_eq!(
        error_code(registry.execute(
            &unbound,
            SideEffectPolicy::all_builtins(),
            call("read_file", json!({"path":"ok.txt"}))
        )),
        ToolErrorCode::PolicyDenied
    );
    let (worker, revoke) = worker_context(root.path(), worker_policy());
    for origin in [tools::ToolOrigin::AgentTool, tools::ToolOrigin::UserShell] {
        assert_eq!(
            error_code(registry.execute(
                &worker.clone().with_origin(origin),
                SideEffectPolicy::all_builtins(),
                call("read_file", json!({"path":"ok.txt"}))
            )),
            ToolErrorCode::PolicyDenied
        );
    }
    revoke.revoke();
    assert_eq!(
        error_code(registry.execute(
            &worker,
            SideEffectPolicy::all_builtins(),
            call("read_file", json!({"path":"ok.txt"}))
        )),
        ToolErrorCode::PolicyDenied
    );
}

#[test]
fn worker_preflight_rejects_unknown_effects_network_and_general_shell() {
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let lease = tools::BlueprintLease {
        lease_id: "lease".into(),
        issued_at_ms: now,
        expires_at_ms: now + 1000,
    };
    let mut unknown = worker_policy();
    unknown.allowed_tools.insert("external_agent".into());
    let mut network = worker_policy();
    network.network_hosts.insert("example.com".into());
    for spec in [unknown, network] {
        assert!(tools::BlueprintGate::compile(&context, spec, lease.clone(), now).is_err());
    }
    for timeout_ms in [0, 600_001] {
        let mut spec = worker_policy();
        spec.max_command_timeout_ms = timeout_ms;
        assert!(tools::BlueprintGate::compile(&context, spec, lease.clone(), now).is_err());
    }
    for command in [
        "rm -rf src",
        "curl example.com",
        "codex",
        "sh -c true",
        "echo $(touch evil)",
        "echo safe > out",
        "echo safe; true",
        "echo safe\ntrue",
    ] {
        let mut spec = worker_policy();
        spec.allowed_commands.insert(command.into());
        assert!(
            tools::BlueprintGate::compile(&context, spec, lease.clone(), now).is_err(),
            "admitted {command}"
        );
    }
    let mut expired = lease.clone();
    expired.expires_at_ms = now;
    assert!(tools::BlueprintGate::compile(&context, worker_policy(), expired, now).is_err());
    let (gate, _) = tools::BlueprintGate::compile(&context, worker_policy(), lease, now).unwrap();
    let other = tempdir().unwrap();
    assert!(
        ToolContext::new(other.path())
            .unwrap()
            .with_blueprint_gate(gate)
            .is_err()
    );
}

#[test]
fn worker_search_listing_and_compaction_do_not_leak_prohibited_paths() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("ok.txt"), "needle visible").unwrap();
    fs::write(root.path().join(".env"), "needle hidden").unwrap();
    fs::create_dir(root.path().join("private")).unwrap();
    fs::write(root.path().join("private/secret"), "needle hidden").unwrap();
    let (context, _) = worker_context(root.path(), worker_policy());
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let output = successful_output(registry.execute(
        &context,
        SideEffectPolicy::all_builtins(),
        call("list_directory", json!({})),
    ));
    assert_eq!(output["entries"].as_array().unwrap().len(), 1);
    let output = successful_output(registry.execute(
        &context,
        SideEffectPolicy::all_builtins(),
        call("search_text", json!({"query":"needle"})),
    ));
    assert_eq!(output["matches"].as_array().unwrap().len(), 1);
    assert!(!output.to_string().contains("hidden"));
    fs::write(root.path().join("large.txt"), "a".repeat(100_000)).unwrap();
    let output = successful_output(registry.execute_compact(
        &context,
        SideEffectPolicy::all_builtins(),
        call("read_file", json!({"path":"large.txt"})),
    ));
    assert_eq!(output["truncated"], true);
    assert!(output["artifact"].is_null());
    assert!(!root.path().join(".zenpi").exists());
}

#[test]
fn worker_action_budget_is_shared_across_context_clones_and_direct_invocation() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("ok.txt"), "ok").unwrap();
    let mut spec = worker_policy();
    spec.max_actions = 1;
    let (context, _) = worker_context(root.path(), spec);
    let arguments = json!({"path":"ok.txt"});
    tools::ReadFileTool
        .invoke(&context, arguments.as_object().unwrap())
        .unwrap();
    let error = tools::ReadFileTool
        .invoke(&context.clone(), arguments.as_object().unwrap())
        .unwrap_err();
    assert_eq!(error.code(), ToolErrorCode::PolicyDenied);
    assert!(error.to_string().contains("action_budget_exhausted"));
}

#[cfg(unix)]
#[test]
fn worker_gate_refuses_symlinks_and_hardlinks_even_inside_workspace() {
    use std::os::unix::fs::symlink;
    let root = tempdir().unwrap();
    fs::write(root.path().join("secret.key"), "secret").unwrap();
    symlink("secret.key", root.path().join("alias")).unwrap();
    fs::hard_link(
        root.path().join("secret.key"),
        root.path().join("hardalias"),
    )
    .unwrap();
    let (context, _) = worker_context(root.path(), worker_policy());
    for path in ["alias", "hardalias"] {
        let error = tools::ReadFileTool
            .invoke(&context, json!({"path":path}).as_object().unwrap())
            .unwrap_err();
        assert_eq!(error.code(), ToolErrorCode::PolicyDenied);
    }
}

#[cfg(unix)]
#[test]
fn worker_commands_are_declared_direct_argv_with_no_shell_expansion() {
    let root = tempdir().unwrap();
    let mut spec = worker_policy();
    spec.denied_commands.insert("true".into());
    let (context, _) = worker_context(root.path(), spec);
    let output = tools::RunCommandTool::invoke_with_cancel(
        &context,
        json!({"command":"echo safe"}).as_object().unwrap(),
        &|| false,
    )
    .unwrap();
    assert_eq!(output["stdout"], "safe\n");
    assert_eq!(output["origin"], "blueprint_worker");
    for command in ["true", "echo other", "echo safe; touch marker"] {
        let error = tools::RunCommandTool::invoke_with_cancel(
            &context,
            json!({"command":command}).as_object().unwrap(),
            &|| false,
        )
        .unwrap_err();
        assert_eq!(error.code(), ToolErrorCode::PolicyDenied);
    }
    assert!(!root.path().join("marker").exists());
}

#[cfg(unix)]
#[test]
fn precancelled_command_does_not_spawn_and_user_shell_keeps_exit_evidence() {
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let error = tools::RunCommandTool::invoke_with_cancel(
        &context,
        json!({"command":"printf ran > marker", "timeout_ms":600_000})
            .as_object()
            .unwrap(),
        &|| true,
    )
    .unwrap_err();
    assert_eq!(error.code(), ToolErrorCode::Cancelled);
    assert!(!root.path().join("marker").exists());
    let context = context.with_origin(tools::ToolOrigin::UserShell);
    let output = tools::RunCommandTool::invoke_user_shell_with_cancel(
        &context,
        json!({"command":"printf out; printf err >&2; exit 7"})
            .as_object()
            .unwrap(),
        &|| false,
    )
    .unwrap();
    assert_eq!(output["origin"], "user_shell");
    assert_eq!(output["exit_code"], 7);
    assert_eq!(output["stdout"], "out");
    assert_eq!(output["stderr"], "err");
    assert_eq!(output["child_reaped"], true);
    assert_eq!(output["process_group_terminated"], true);
}

#[cfg(unix)]
#[test]
fn successful_shell_cannot_leave_background_process_or_hold_pipe_forever() {
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    for command in [
        "(sleep 0.5; printf leaked > marker) & exit 0",
        "(sleep 0.5; printf leaked > marker) >/dev/null 2>&1 & exit 0",
    ] {
        let start = std::time::Instant::now();
        tools::RunCommandTool::invoke_with_cancel(
            &context,
            json!({"command":command,"timeout_ms":100})
                .as_object()
                .unwrap(),
            &|| false,
        )
        .unwrap();
        assert!(start.elapsed() < std::time::Duration::from_millis(400));
        std::thread::sleep(std::time::Duration::from_millis(550));
        assert!(!root.path().join("marker").exists());
    }
}

#[cfg(unix)]
#[test]
fn term_resistant_descendants_die_after_timeout_and_host_cancel() {
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    for host_cancel in [false, true] {
        let start = std::time::Instant::now();
        let command = "(trap '' TERM; sleep 0.3; printf leaked > marker) & wait";
        let error = tools::RunCommandTool::invoke_with_cancel(
            &context,
            json!({"command":command,"timeout_ms":if host_cancel {600_000} else {30}})
                .as_object()
                .unwrap(),
            &|| host_cancel && start.elapsed() > std::time::Duration::from_millis(20),
        )
        .unwrap_err();
        assert_eq!(
            error.code(),
            if host_cancel {
                ToolErrorCode::Cancelled
            } else {
                ToolErrorCode::CommandTimeout
            }
        );
        assert!(start.elapsed() < std::time::Duration::from_millis(400));
        std::thread::sleep(std::time::Duration::from_millis(350));
        assert!(!root.path().join("marker").exists());
    }
}

#[cfg(unix)]
#[test]
#[ignore = "fresh process for deterministic temporary-name collision fixtures"]
fn reliability_collision_child() {
    use std::os::unix::fs::symlink;
    let root = tempdir().unwrap();
    let target = root.path().join("file.txt");
    fs::write(&target, "original").unwrap();
    let temporary = root
        .path()
        .join(format!(".file.txt.zenpi-{}-1", std::process::id()));
    let kind = std::env::var("ZENPI_TEST_COLLISION_KIND").unwrap();
    let other = root.path().join("other.txt");
    fs::write(&other, "other owner").unwrap();
    match kind.as_str() {
        "file" => fs::write(&temporary, "unowned temporary").unwrap(),
        "symlink" => symlink(&other, &temporary).unwrap(),
        "directory" => fs::create_dir(&temporary).unwrap(),
        _ => panic!("invalid fixture"),
    }
    let original_metadata = fs::symlink_metadata(&temporary).unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let result = registry.execute(
        &context,
        SideEffectPolicy::all_builtins(),
        call(
            "write_file",
            json!({"path":"file.txt","content":"replacement"}),
        ),
    );
    assert_eq!(error_code(result), ToolErrorCode::Io);
    assert_eq!(fs::read(&target).unwrap(), b"original");
    assert_eq!(fs::read(&other).unwrap(), b"other owner");
    let metadata = fs::symlink_metadata(&temporary).expect("unowned collision path must survive");
    assert_eq!(metadata.file_type(), original_metadata.file_type());
    match kind.as_str() {
        "file" => assert_eq!(fs::read(&temporary).unwrap(), b"unowned temporary"),
        "symlink" => assert_eq!(fs::read_link(&temporary).unwrap(), other),
        "directory" => assert_eq!(fs::read_dir(&temporary).unwrap().count(), 0),
        _ => unreachable!(),
    }
    // A subsequent, unique temporary still replaces successfully; the collision stays intact.
    assert!(
        registry
            .execute(
                &context,
                SideEffectPolicy::all_builtins(),
                call("write_file", json!({"path":"file.txt","content":"retry"}))
            )
            .is_success()
    );
    assert_eq!(fs::read(&target).unwrap(), b"retry");
    assert_eq!(
        fs::symlink_metadata(&temporary).unwrap().file_type(),
        original_metadata.file_type()
    );
}

#[cfg(unix)]
#[test]
fn reliability_create_new_collision_never_removes_or_overwrites_another_path() {
    let mut failures = Vec::new();
    for kind in ["file", "symlink", "directory"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "reliability_collision_child",
                "--ignored",
                "--nocapture",
            ])
            .env("ZENPI_TEST_COLLISION_KIND", kind)
            .output()
            .unwrap();
        println!(
            "collision={kind}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if !output.status.success() {
            failures.push(kind);
        }
    }
    assert!(
        failures.is_empty(),
        "failed collision fixtures: {failures:?}"
    );
}

#[cfg(unix)]
#[test]
fn reliability_write_and_edit_preserve_permissions_and_replace_the_inode() {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    for mode in [0o700, 0o751, 0o640] {
        for tool in ["write_file", "edit_file"] {
            let path = root.path().join("file.txt");
            fs::write(&path, "original\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            let mut old_reader = fs::File::open(&path).unwrap();
            let old_inode = old_reader.metadata().unwrap().ino();
            let args = if tool == "write_file" {
                json!({"path":"file.txt","content":"replacement\n"})
            } else {
                json!({"path":"file.txt","old":"original","new":"replacement"})
            };
            let request = call(tool, args);
            let preview = registry
                .approval_preview(&context, &request)
                .unwrap()
                .unwrap();
            successful_output(registry.execute_approved(
                &context,
                SideEffectPolicy::all_builtins(),
                request,
                Some(&preview),
            ));
            assert_eq!(fs::read(&path).unwrap(), b"replacement\n");
            let metadata = fs::metadata(&path).unwrap();
            assert_ne!(
                metadata.ino(),
                old_inode,
                "must rename a new inode, not truncate the old one"
            );
            let mut original = String::new();
            old_reader.read_to_string(&mut original).unwrap();
            assert_eq!(
                original, "original\n",
                "existing readers retain the complete old file"
            );
            assert_eq!(
                metadata.permissions().mode() & 0o777,
                mode,
                "{tool} must preserve rwx bits"
            );
            assert_eq!(
                fs::read_dir(root.path()).unwrap().count(),
                1,
                "no temporary leak"
            );
        }
    }
}

#[test]
fn reliability_line_ending_diff_is_exact_and_can_be_applied() {
    let cases = [
        ("one\r\ntwo\r\n", "one\ntwo\n"),
        ("one\ntwo\n", "one\r\ntwo\r\n"),
        ("head\r\nlast", "head\nlast"),
        ("first\r\nsecond\nlast\r", "first\nsecond\nlast"),
        ("first\r\nlast", "first\nlast\n"),
    ];
    for (before, after) in cases {
        let root = tempdir().unwrap();
        let apply_root = tempdir().unwrap();
        fs::write(root.path().join("file.txt"), before).unwrap();
        fs::write(apply_root.path().join("file.txt"), before).unwrap();
        let context = ToolContext::new(root.path()).unwrap();
        let registry = ToolRegistry::with_all_builtins().unwrap();
        for tool in ["write_file", "edit_file"] {
            fs::write(root.path().join("file.txt"), before).unwrap();
            fs::write(apply_root.path().join("file.txt"), before).unwrap();
            let args = if tool == "write_file" {
                json!({"path":"file.txt","content":after})
            } else {
                json!({"path":"file.txt","old":before,"new":after})
            };
            let request = call(tool, args);
            let preview = registry
                .approval_preview(&context, &request)
                .unwrap()
                .unwrap();
            let tools::ToolPreview::Diff {
                patch,
                changed,
                truncated,
                ..
            } = &preview;
            println!("{tool} before={before:?} after={after:?} patch={patch:?}");
            assert!(*changed);
            assert!(!truncated);
            assert!(!patch.is_empty());
            assert!(
                patch.contains('\r'),
                "raw CR bytes must remain in the exact unified diff"
            );
            fs::write(apply_root.path().join("change.patch"), patch).unwrap();
            let applied = std::process::Command::new("git")
                .current_dir(apply_root.path())
                .args(["apply", "-p0", "--whitespace=nowarn", "change.patch"])
                .output()
                .unwrap();
            assert!(
                applied.status.success(),
                "{}",
                String::from_utf8_lossy(&applied.stderr)
            );
            assert_eq!(
                fs::read(apply_root.path().join("file.txt")).unwrap(),
                after.as_bytes()
            );
            let output = successful_output(registry.execute_approved(
                &context,
                SideEffectPolicy::all_builtins(),
                request,
                Some(&preview),
            ));
            assert_eq!(output["diff"], *patch);
            assert_eq!(
                fs::read(root.path().join("file.txt")).unwrap(),
                after.as_bytes()
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn reliability_permissions_do_not_add_execute_or_propagate_set_id_bits() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let path = root.path().join("file.txt");
    successful_output(registry.execute(
        &context,
        SideEffectPolicy::all_builtins(),
        call("write_file", json!({"path":"file.txt","content":"first"})),
    ));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o7111,
        0
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o4751)).unwrap();
    successful_output(registry.execute(
        &context,
        SideEffectPolicy::all_builtins(),
        call(
            "write_file",
            json!({"path":"file.txt","content":"replacement"}),
        ),
    ));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
        0o751
    );
}

#[test]
fn web_search_env_and_result_formatting_are_bounded() {
    use zenpi::tools::{format_web_search_results, parse_exa_env};

    assert_eq!(
        parse_exa_env("export EXA_API_KEY=abc123\n").as_deref(),
        Some("abc123")
    );
    assert_eq!(
        parse_exa_env("# c\nEXA_API_KEY=\"quoted\"\n").as_deref(),
        Some("quoted")
    );
    assert_eq!(parse_exa_env("OTHER=1\n"), None);
    assert_eq!(parse_exa_env("EXA_API_KEY=\n"), None);
    assert_eq!(parse_exa_env("EXA_API_KEY=bad\nkey\n"), Some("bad".into()));

    let raw = serde_json::json!({
        "results": [
            {"title": "A", "url": "https://a.example", "text": "x".repeat(2000)},
            {"title": "B", "url": "https://b.example", "text": "short"},
            {"title": "", "url": "", "text": ""}
        ]
    });
    let bounded = format_web_search_results(&raw, 5);
    assert_eq!(bounded["count"], 2);
    assert_eq!(bounded["results"][0]["title"], "A");
    assert!(
        bounded["results"][0]["snippet"].as_str().unwrap().len() <= 600,
        "snippet stays bounded"
    );
}
