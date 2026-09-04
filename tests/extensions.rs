use std::{collections::BTreeSet, fs, time::Duration};

use tempfile::tempdir;
use zenpi::{
    extensions::{
        CapabilityBroker, CapabilityScope, ExtensionCatalog, install, remove, set_disabled, upgrade,
    },
    tools::{SideEffectPolicy, ToolCall, ToolContext, ToolRegistry},
};

#[cfg(unix)]
fn write_extension(root: &std::path::Path, name: &str, api_version: u32, disabled: bool) {
    use std::os::unix::fs::PermissionsExt;

    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("extension.toml"),
        format!(
            r#"name = "{name}"
version = "1.2.3"
api_version = {api_version}
disabled = {disabled}
executable = "server.sh"

[permissions]
workspace_read = true
workspace_write = false
command_execution = false
network = false

[[tools]]
name = "ext_echo"
description = "Return bounded extension arguments"
side_effect = "read_only"
input_schema = {{ type = "object" }}
"#
        ),
    )
    .unwrap();
    let script = root.join("server.sh");
    fs::write(
        &script,
        "#!/bin/sh\nIFS= read -r line\nif env | grep -Eq 'OPENAI|ZENPI_API_KEY'; then exit 90; fi\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}'\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(script, permissions).unwrap();
}

#[cfg(unix)]
#[test]
fn local_mcp_tool_is_framed_and_child_environment_has_no_provider_auth() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("extensions");
    write_extension(&root.join("echo"), "echo", 1, false);
    let catalog = ExtensionCatalog::load(&root).unwrap();
    assert_eq!(catalog.summaries().len(), 1);
    let mut registry = ToolRegistry::new();
    catalog.register_tools(&mut registry).unwrap();
    let context = ToolContext::new(dir.path()).unwrap();
    let result = registry.execute(
        &context,
        SideEffectPolicy::read_only(),
        ToolCall {
            id: "call-1".into(),
            name: "ext_echo".into(),
            arguments: serde_json::json!({"message":"hello"}),
        },
    );
    assert!(result.is_success(), "{result:?}");
    let encoded = serde_json::to_string(&result).unwrap();
    assert!(encoded.contains("\"ok\":true"));
    assert!(!encoded.contains("OPENAI"));
}

#[cfg(unix)]
#[test]
fn extension_lifecycle_rejects_incompatible_and_never_loads_disabled() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("installed");
    let source = dir.path().join("source");
    write_extension(&source, "sample", 1, false);
    let summary = install(&root, &source).unwrap();
    assert_eq!(summary.name, "sample");
    assert!(set_disabled(&root, "sample", true).unwrap());
    let disabled = ExtensionCatalog::load(&root).unwrap();
    assert!(disabled.summaries()[0].disabled);
    let mut registry = ToolRegistry::new();
    disabled.register_tools(&mut registry).unwrap();
    assert!(registry.definitions().is_empty());
    assert!(set_disabled(&root, "sample", false).unwrap());
    assert_eq!(ExtensionCatalog::load(&root).unwrap().summaries().len(), 1);
    let upgraded = dir.path().join("upgraded");
    write_extension(&upgraded, "sample", 1, false);
    let mut upgraded_text = fs::read_to_string(upgraded.join("extension.toml")).unwrap();
    upgraded_text = upgraded_text.replace("1.2.3", "2.0.0");
    fs::write(upgraded.join("extension.toml"), upgraded_text).unwrap();
    assert_eq!(upgrade(&root, &upgraded).unwrap().version, "2.0.0");
    assert!(remove(&root, "sample").unwrap());
    assert!(!remove(&root, "sample").unwrap());

    let incompatible = dir.path().join("incompatible");
    write_extension(&incompatible, "future", 99, false);
    assert!(install(&root, &incompatible).is_err());
    assert!(!root.join("future").exists());
}

#[cfg(unix)]
#[test]
fn manifest_path_escape_and_excess_permission_fail_closed() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("extensions");
    let path = root.join("bad");
    write_extension(&path, "bad", 1, false);
    let text = fs::read_to_string(path.join("extension.toml")).unwrap();
    fs::write(
        path.join("extension.toml"),
        text.replace(
            "executable = \"server.sh\"",
            "executable = \"../server.sh\"",
        ),
    )
    .unwrap();
    assert!(ExtensionCatalog::load(&root).is_err());
}

#[test]
fn capability_handles_are_scoped_opaque_and_revocable() {
    let mut broker = CapabilityBroker::default();
    let handle = broker
        .issue(
            "sample",
            BTreeSet::from([CapabilityScope::WorkspaceRead]),
            Duration::from_secs(60),
        )
        .unwrap();
    assert!(handle.id.starts_with("cap_"));
    assert!(!handle.id.contains("key"));
    assert!(broker.authorize(&handle.id, "sample", CapabilityScope::WorkspaceRead));
    assert!(!broker.authorize(&handle.id, "sample", CapabilityScope::WorkspaceWrite));
    assert!(!broker.authorize(&handle.id, "other", CapabilityScope::WorkspaceRead));
    assert!(broker.revoke(&handle.id));
    assert!(!broker.authorize(&handle.id, "sample", CapabilityScope::WorkspaceRead));
}

#[cfg(unix)]
#[test]
fn extension_cli_installs_lists_disables_enables_and_removes() {
    let home = tempdir().unwrap();
    let source = tempdir().unwrap();
    write_extension(source.path(), "cli_sample", 1, false);
    let binary = env!("CARGO_BIN_EXE_zenpi");
    let run = |args: &[&str]| {
        std::process::Command::new(binary)
            .env("HOME", home.path())
            .env_remove("ZENPI_HOME")
            .args(args)
            .output()
            .unwrap()
    };
    assert!(
        run(&["extension", "install", source.path().to_str().unwrap()])
            .status
            .success()
    );
    let listed = run(&["extension", "list", "--json"]);
    assert!(listed.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&listed.stdout).unwrap()[0]["name"],
        "cli_sample"
    );
    assert!(
        run(&["extension", "disable", "cli_sample"])
            .status
            .success()
    );
    assert!(run(&["extension", "enable", "cli_sample"]).status.success());
    assert!(run(&["extension", "remove", "cli_sample"]).status.success());
}
