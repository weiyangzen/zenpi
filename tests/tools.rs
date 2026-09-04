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
    assert_eq!(definitions.len(), 3);
    assert_eq!(
        definitions
            .iter()
            .map(|definition| definition.name.as_str())
            .collect::<Vec<_>>(),
        ["list_directory", "read_file", "search_text"]
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
    let created = successful_output(registry.execute(
        &context,
        policy,
        call("write_file", json!({"path":"new.txt","content":"one"})),
    ));
    assert_eq!(created["bytes"], 3);
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
    assert_eq!(
        fs::read_to_string(directory.path().join("new.txt")).unwrap(),
        "two"
    );
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
    let timed_out = registry.execute(
        &context,
        policy,
        call("run_command", json!({"command":"sleep 1","timeout_ms":10})),
    );
    assert_eq!(error_code(timed_out), ToolErrorCode::CommandTimeout);
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
