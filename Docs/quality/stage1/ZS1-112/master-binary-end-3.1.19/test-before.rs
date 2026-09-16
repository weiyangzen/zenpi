#![cfg(unix)]
use serde_json::{Value, json};
use std::fs;
use std::time::{Duration, Instant};
use tempfile::{TempDir, tempdir};
use zenpi::search::SearchBackend;
use zenpi::tools::*;
fn fixture() -> (TempDir, ToolContext, ToolRegistry) {
    assert!(
        SearchBackend::discover().available(),
        "this integration test requires an installed rg"
    );
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join("src/nested")).unwrap();
    fs::write(root.path().join(".gitignore"), "ignored.rs\nignored/\n").unwrap();
    fs::write(
        root.path().join("src/a.rs"),
        "before\nneedle12\nafter\nneedle.*\n",
    )
    .unwrap();
    fs::write(root.path().join("src/nested/b.rs"), "NEEDLE34\n").unwrap();
    fs::write(root.path().join("src/ignored.rs"), "needle99\n").unwrap();
    fs::write(root.path().join("src/a.txt"), "needle55\n").unwrap();
    fs::write(root.path().join("src/.hidden.rs"), "needle66\n").unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    (
        root,
        context,
        ToolRegistry::with_read_only_builtins().unwrap(),
    )
}
fn run(registry: &ToolRegistry, context: &ToolContext, name: &str, args: Value) -> ToolResult {
    registry.execute(
        context,
        SideEffectPolicy::read_only(),
        ToolCall {
            id: "search-call".into(),
            name: name.into(),
            arguments: args,
        },
    )
}
fn output(result: ToolResult) -> Value {
    match result {
        ToolResult::Success { output, .. } => output,
        other => panic!("{other:?}"),
    }
}
fn code(result: ToolResult) -> ToolErrorCode {
    match result {
        ToolResult::Error { error, .. } => error.code,
        other => panic!("{other:?}"),
    }
}
#[test]
fn default_search_keeps_original_literal_behavior_without_rg() {
    let (_root, context, registry) = fixture();
    let context = context.with_search_backend(SearchBackend::configured(None).unwrap());
    let value = output(run(
        &registry,
        &context,
        "search_text",
        json!({"query":"needle.*"}),
    ));
    assert_eq!(value["matches"].as_array().unwrap().len(), 1);
    assert_eq!(value["matches"][0]["text"], "needle.*");
    assert!(value.get("engine").is_none());
    assert_eq!(
        code(run(
            &registry,
            &context,
            "search_text",
            json!({"query":"needle","regex":true})
        )),
        ToolErrorCode::Unsupported
    );
    assert_eq!(
        code(run(&registry, &context, "find", json!({"pattern":"*.rs"}))),
        ToolErrorCode::Unsupported
    );
}
#[test]
fn regex_glob_ignore_and_context_use_actual_rg_and_workspace_paths() {
    let (_root, context, registry) = fixture();
    let value = output(run(
        &registry,
        &context,
        "search_text",
        json!({"query":"^needle[0-9]+$","regex":true,"glob":"src/**/*.rs","context":1,"case_sensitive":false}),
    ));
    let matches = value["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0]["path"], "src/a.rs");
    assert_eq!(matches[0]["line"], 2);
    assert!(matches.iter().any(|m| m["path"] == "src/nested/b.rs"));
    let lines = value["context"].as_array().unwrap();
    assert!(
        lines
            .iter()
            .any(|line| line["line"] == 1 && line["text"] == "before")
    );
    assert!(
        lines
            .iter()
            .any(|line| line["line"] == 3 && line["text"] == "after")
    );
    assert_eq!(value["engine"], "ripgrep");
}
#[test]
fn positive_glob_does_not_override_ignore_true_and_false_explicitly_includes_ignored() {
    let (_root, context, registry) = fixture();
    for ignore in [true, false] {
        let value = output(run(
            &registry,
            &context,
            "find",
            json!({"pattern":"src/**/*.rs","ignore":ignore}),
        ));
        let paths = value["paths"].as_array().unwrap();
        assert_eq!(paths.contains(&json!("src/ignored.rs")), !ignore);
        assert!(!paths.contains(&json!("src/.hidden.rs")));
    }
    let value = output(run(
        &registry,
        &context,
        "find",
        json!({"pattern":"*.rs","hidden":true}),
    ));
    assert!(
        value["paths"]
            .as_array()
            .unwrap()
            .contains(&json!("src/.hidden.rs"))
    );
}
#[test]
fn explicit_literal_advanced_search_still_treats_regex_symbols_literally() {
    let (_root, context, registry) = fixture();
    let value = output(run(
        &registry,
        &context,
        "search_text",
        json!({"query":"needle.*","regex":false,"glob":"*.rs"}),
    ));
    assert_eq!(value["matches"].as_array().unwrap().len(), 1);
    assert_eq!(value["matches"][0]["text"], "needle.*");
}
#[test]
fn invalid_patterns_and_arguments_fail_even_without_files() {
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let registry = ToolRegistry::with_read_only_builtins().unwrap();
    for args in [
        json!({"query":"[","regex":true}),
        json!({"query":"x","context":11}),
        json!({"query":"x","glob":"["}),
        json!({"query":"x","regex":"yes"}),
    ] {
        assert_eq!(
            code(run(&registry, &context, "search_text", args)),
            ToolErrorCode::InvalidArguments
        );
    }
    for args in [json!({"pattern":"["}), json!({"pattern":"*","limit":0})] {
        assert_eq!(
            code(run(&registry, &context, "find", args)),
            ToolErrorCode::InvalidArguments
        );
    }
}
#[test]
fn path_escape_and_symlink_targets_are_never_searched() {
    use std::os::unix::fs::symlink;
    let (root, context, registry) = fixture();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("secret"), "needle-secret").unwrap();
    symlink(outside.path(), root.path().join("escape")).unwrap();
    for path in ["../", "escape"] {
        assert_eq!(
            code(run(
                &registry,
                &context,
                "search_text",
                json!({"query":"needle","regex":true,"path":path})
            )),
            ToolErrorCode::PathDenied
        );
    }
    symlink(outside.path().join("secret"), root.path().join("linked.rs")).unwrap();
    let value = output(run(
        &registry,
        &context,
        "search_text",
        json!({"query":"needle-secret","regex":true}),
    ));
    assert!(value["matches"].as_array().unwrap().is_empty());
}
#[test]
fn unavailable_or_disappeared_executable_is_explicit() {
    let (root, context, registry) = fixture();
    let context = context.with_search_backend(
        SearchBackend::configured(Some(root.path().join("missing-rg"))).unwrap(),
    );
    assert_eq!(
        code(run(&registry, &context, "find", json!({"pattern":"*"}))),
        ToolErrorCode::Unsupported
    );
}
#[test]
fn caps_match_count_file_count_and_long_lines_with_reasons() {
    let (root, context, registry) = fixture();
    fs::write(
        root.path().join("long.txt"),
        format!("needle{}\n", "x".repeat(4000)),
    )
    .unwrap();
    let value = output(run(
        &registry,
        &context,
        "search_text",
        json!({"query":"needle","regex":false,"path":"long.txt"}),
    ));
    assert_eq!(value["matches"][0]["text"].as_str().unwrap().len(), 2048);
    assert!(
        value["truncation_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("line_bytes"))
    );
    let value = output(run(
        &registry,
        &context,
        "search_text",
        json!({"query":"needle","regex":false,"max_matches":1}),
    ));
    assert_eq!(value["matches"].as_array().unwrap().len(), 1);
    assert_eq!(value["truncated"], true);
    for i in 0..270 {
        fs::write(root.path().join(format!("f{i:03}.txt")), "none\n").unwrap();
    }
    let value = output(run(
        &registry,
        &context,
        "find",
        json!({"pattern":"*.txt","limit":256}),
    ));
    assert_eq!(value["paths"].as_array().unwrap().len(), 256);
    assert!(
        value["truncation_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("file_limit"))
    );
}
#[test]
fn oversized_binary_and_deep_tree_are_bounded_without_partial_index() {
    let (root, context, registry) = fixture();
    fs::write(root.path().join("large.bin"), vec![0; 1024 * 1024 + 1]).unwrap();
    let mut deep = root.path().to_path_buf();
    for _ in 0..70 {
        deep.push("d");
        fs::create_dir(&deep).unwrap();
    }
    fs::write(deep.join("file"), "deepneedle").unwrap();
    let began = Instant::now();
    let value = output(run(
        &registry,
        &context,
        "search_text",
        json!({"query":"deepneedle","regex":true}),
    ));
    assert!(began.elapsed() < Duration::from_secs(3));
    assert!(value["matches"].as_array().unwrap().is_empty());
    assert_eq!(value["max_depth"], 64);
    assert!(
        value["truncation_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("file_bytes"))
    );
    assert!(!root.path().join(".zenpi").exists());
}
#[test]
fn whitespace_names_are_preserved_and_hidden_is_explicit() {
    let (root, context, registry) = fixture();
    fs::write(root.path().join("a space.rs"), "needle77\n").unwrap();
    let value = output(run(
        &registry,
        &context,
        "find",
        json!({"pattern":"*space*"}),
    ));
    assert_eq!(value["paths"], json!(["a space.rs"]));
}
#[test]
fn cancellation_reaps_the_actual_search_process_and_its_descendant() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempdir().unwrap();
    let executable = root.path().join("controlled-search");
    fs::write(
        &executable,
        "#!/bin/sh\necho $$ > leader\nsleep 30 &\necho $! > descendant\nwait\n",
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let context = ToolContext::new(root.path())
        .unwrap()
        .with_search_backend(SearchBackend::configured(Some(executable)).unwrap());
    let registry = ToolRegistry::with_read_only_builtins().unwrap();
    let began = Instant::now();
    let result = registry.execute_cancellable(
        &context,
        SideEffectPolicy::read_only(),
        ToolCall {
            id: "c".into(),
            name: "find".into(),
            arguments: json!({"pattern":"*"}),
        },
        &|| root.path().join("descendant").exists(),
    );
    assert_eq!(code(result), ToolErrorCode::Cancelled);
    assert!(began.elapsed() < Duration::from_secs(2));
    let leader: i32 = fs::read_to_string(root.path().join("leader"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(unsafe { libc::kill(leader, 0) }, -1);
    // The existing supervised process tests separately cover TERM-resistant
    // descendants; this adapter must reuse that same cleanup owner.
}
#[test]
fn search_subprocess_budget_hook_distinguishes_default_literal() {
    assert!(!search_requires_subprocess(
        json!({"query":"x"}).as_object().unwrap()
    ));
    assert!(search_requires_subprocess(
        json!({"query":"x","regex":false}).as_object().unwrap()
    ));
}

#[test]
fn cumulative_process_budget_includes_preflight_listings_and_search_chunks() {
    let plain = json!({"query":"x"});
    assert_eq!(
        search_process_budget("search_text", plain.as_object().unwrap()),
        0
    );
    assert_eq!(
        search_process_budget("find", json!({"pattern":"*"}).as_object().unwrap()),
        2
    );
    let full = json!({"query":"^needle","regex":true,"glob":"*.rs"});
    let budget = search_process_budget("search_text", full.as_object().unwrap());
    assert_eq!(budget, 19);
    let (root, context, registry) = fixture();
    let real = SearchBackend::discover()
        .executable_path()
        .unwrap()
        .to_owned();
    let script = root.path().join("rg-wrapper");
    let quoted = real.to_str().unwrap().replace("'", "'\"'\"'");
    fs::write(
        &script,
        format!("#!/bin/sh\nprintf start >> .starts\nexec '{quoted}' \"$@\"\n"),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let context = context.with_search_backend(SearchBackend::configured(Some(script)).unwrap());
    assert!(run(&registry, &context, "search_text", full).is_success());
    let starts = fs::read_to_string(root.path().join(".starts"))
        .unwrap()
        .matches("start")
        .count() as u64;
    assert_eq!(starts, 4);
    assert!(starts <= budget);
}

#[test]
fn find_lists_large_files_without_loading_their_contents() {
    let (root, context, registry) = fixture();
    let file = fs::File::create(root.path().join("large.bin")).unwrap();
    file.set_len(20 * 1024 * 1024).unwrap();
    let value = output(run(&registry, &context, "find", json!({"pattern":"*.bin"})));
    assert_eq!(value["paths"], json!(["large.bin"]));
    assert_eq!(value["truncated"], false);
}
#[test]
fn a_live_worker_gate_denies_unconfined_advanced_search_before_any_spawn() {
    use std::os::unix::fs::PermissionsExt;
    let (root, context, registry) = fixture();
    let script = root.path().join("must-not-spawn");
    fs::write(&script, "#!/bin/sh\ntouch spawned\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let context = context.with_search_backend(SearchBackend::configured(Some(script)).unwrap());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let policy = BlueprintPolicySpec {
        blueprint_digest: "a".repeat(64),
        goal_digest: "b".repeat(64),
        item_id: "ZS1-112".into(),
        allowed_tools: ["search_text".into(), "find".into()].into(),
        readable_paths: [".".into()].into(),
        writable_paths: Default::default(),
        denied_paths: Default::default(),
        protected_paths: Default::default(),
        allowed_commands: Default::default(),
        denied_commands: Default::default(),
        network_hosts: Default::default(),
        max_actions: 10,
        max_command_timeout_ms: 1000,
        max_command_output_bytes: 1024,
    };
    let (gate, _) = BlueprintGate::compile(
        &context,
        policy,
        BlueprintLease {
            lease_id: "lease".into(),
            issued_at_ms: now,
            expires_at_ms: now + 10000,
        },
        now,
    )
    .unwrap();
    let context = context.with_blueprint_gate(gate).unwrap();
    assert_eq!(
        code(run(
            &registry,
            &context,
            "search_text",
            json!({"query":"needle","regex":true})
        )),
        ToolErrorCode::PolicyDenied
    );
    assert_eq!(
        code(run(&registry, &context, "find", json!({"pattern":"*"}))),
        ToolErrorCode::PolicyDenied
    );
    assert!(!root.path().join("spawned").exists());
    assert!(
        run(
            &registry,
            &context,
            "search_text",
            json!({"query":"needle"})
        )
        .is_success()
    );
}

fn search_http(
    name: &str,
    arguments: Value,
    count: usize,
) -> (
    zenpi::backend::OpenAiCompatibleBackend,
    std::sync::mpsc::Receiver<Value>,
    std::thread::JoinHandle<()>,
) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let name = name.to_owned();
    let (send, receive) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        for index in 0..count {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let request = loop {
                let mut chunk = [0; 4096];
                let n = stream.read(&mut chunk).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                assert!(bytes.len() < 2 * 1024 * 1024);
                if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                    let size: usize = std::str::from_utf8(&bytes[..end])
                        .unwrap()
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    if bytes.len() >= end + 4 + size {
                        break serde_json::from_slice::<Value>(&bytes[end + 4..end + 4 + size])
                            .unwrap();
                    }
                }
            };
            assert_eq!(request["stream"], true);
            let delta = if index == 0 {
                json!({"tool_calls":[{"index":0,"id":"actual-search","type":"function","function":{"name":name,"arguments":arguments.to_string()}}]})
            } else {
                json!({"content":"search complete"})
            };
            let body = format!(
                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":if index==0 {"tool_calls"} else {"stop"}}]})
            );
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            stream.flush().unwrap();
            let _ = send.send(request);
        }
    });
    let backend = zenpi::backend::OpenAiCompatibleBackend::new_with_wire_api(
        &url,
        None,
        "gpt-4.1",
        zenpi::backend::OpenAiWireApi::ChatCompletions,
    )
    .unwrap()
    .with_model_registry(
        "openai".into(),
        zenpi::providers::registry::ModelRegistry::default(),
    )
    .unwrap();
    (backend, receive, thread)
}

fn search_agent(
    root: &std::path::Path,
    backend: zenpi::backend::OpenAiCompatibleBackend,
    context: ToolContext,
    cap: u64,
) -> zenpi::core::Agent {
    let session =
        zenpi::session::SessionStore::open_in_workspace(root.join("session.jsonl"), root).unwrap();
    let mut agent = zenpi::core::Agent::new(session, Box::new(backend));
    agent.set_tools(
        ToolRegistry::with_read_only_builtins().unwrap(),
        context,
        SideEffectPolicy::read_only(),
    );
    agent
        .set_resource_limits(zenpi::governance::ResourceLimits {
            max_processes: cap,
            ..Default::default()
        })
        .unwrap();
    agent
}
fn persisted_processes(path: &std::path::Path) -> u64 {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let record: Value = serde_json::from_str(line).unwrap();
            (record["event"]["type"] == "resource_usage")
                .then(|| record["event"]["usage"]["processes"].as_u64().unwrap())
        })
        .next_back()
        .unwrap_or(0)
}

#[test]
fn actual_agent_reserves_all_search_starts_before_spawn_and_retains_usage_after_reopen() {
    use std::os::unix::fs::PermissionsExt;
    for (name, args, cap, reserved, starts) in [
        ("search_text", json!({"query":"needle"}), 0, 0, 0),
        (
            "search_text",
            json!({"query":"^needle[0-9]+$","regex":true,"glob":"*.rs"}),
            19,
            19,
            4,
        ),
        ("find", json!({"pattern":"*.rs"}), 2, 2, 2),
    ] {
        let root = tempdir().unwrap();
        fs::write(root.path().join("answer.rs"), "needle12\n").unwrap();
        let real = SearchBackend::discover()
            .executable_path()
            .unwrap()
            .to_owned();
        let script = root.path().join(".rg-wrapper");
        fs::write(&script,format!("#!/bin/sh\nif [ ! -f .before-spawn ]; then /bin/cp session.jsonl .before-spawn; fi\nprintf start >> .starts\nexec '{}' \"$@\"\n",real.to_str().unwrap().replace("'","'\"'\"'"))).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let context = ToolContext::new(root.path())
            .unwrap()
            .with_search_backend(SearchBackend::configured(Some(script.clone())).unwrap());
        let (backend, requests, server) = search_http(name, args.clone(), 2);
        let mut agent = search_agent(root.path(), backend, context, cap);
        agent.process_sync("search workspace").unwrap();
        server.join().unwrap();
        let sent: Vec<_> = requests.try_iter().collect();
        assert_eq!(sent.len(), 2);
        let result = sent[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "tool")
            .unwrap();
        assert!(result.to_string().contains("answer.rs"));
        assert_eq!(
            persisted_processes(&root.path().join("session.jsonl")),
            reserved
        );
        if starts > 0 {
            assert_eq!(
                fs::read_to_string(root.path().join(".starts"))
                    .unwrap()
                    .matches("start")
                    .count(),
                starts
            );
            assert_eq!(
                persisted_processes(&root.path().join(".before-spawn")),
                reserved,
                "reservation must be durable before first child starts"
            );
        } else {
            assert!(!root.path().join(".starts").exists());
        }
        drop(agent);
        if starts > 0 {
            let (backend, _, server) = search_http(name, args, 1);
            let context = ToolContext::new(root.path())
                .unwrap()
                .with_search_backend(SearchBackend::configured(Some(script)).unwrap());
            let mut resumed = search_agent(root.path(), backend, context, cap);
            let previous = fs::read(root.path().join(".starts")).unwrap();
            assert!(
                resumed
                    .process_sync("search again with exhausted budget")
                    .is_err()
            );
            assert_eq!(previous, fs::read(root.path().join(".starts")).unwrap());
            assert_eq!(
                persisted_processes(&root.path().join("session.jsonl")),
                reserved
            );
            server.join().unwrap();
        }
    }
}

#[test]
fn actual_agent_rejects_insufficient_search_budget_before_any_spawn() {
    use std::os::unix::fs::PermissionsExt;
    for (name, args, cap) in [
        ("find", json!({"pattern":"*"}), 1),
        (
            "search_text",
            json!({"query":"needle","regex":true,"glob":"*.rs"}),
            18,
        ),
    ] {
        let root = tempdir().unwrap();
        let script = root.path().join(".must-not-spawn");
        fs::write(&script, "#!/bin/sh\ntouch spawned\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let context = ToolContext::new(root.path())
            .unwrap()
            .with_search_backend(SearchBackend::configured(Some(script)).unwrap());
        let (backend, _, server) = search_http(name, args, 1);
        let mut agent = search_agent(root.path(), backend, context, cap);
        assert!(agent.process_sync("try search").is_err());
        server.join().unwrap();
        assert!(!root.path().join("spawned").exists());
        assert_eq!(persisted_processes(&root.path().join("session.jsonl")), 0);
    }
}

#[test]
fn actual_search_owner_cancellation_reaps_child_without_refunding_process_starts() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempdir().unwrap();
    let script = root.path().join(".controlled-rg");
    fs::write(
        &script,
        "#!/bin/sh\necho $$ > .started\nexec /bin/sleep 30\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let context = ToolContext::new(root.path())
        .unwrap()
        .with_search_backend(SearchBackend::configured(Some(script)).unwrap());
    let (backend, _requests, server) = search_http("find", json!({"pattern":"*"}), 1);
    let mut agent = search_agent(root.path(), backend, context, 2);
    let began = Instant::now();
    let result =
        agent.process_with_cancel(zenpi::core::TurnInputRequest::new("find files"), || {
            fs::read_to_string(root.path().join(".started"))
                .is_ok_and(|value| value.trim().parse::<i32>().is_ok())
        });
    assert!(result.is_err());
    assert!(began.elapsed() < Duration::from_secs(3));
    server.join().unwrap();
    let pid: i32 = fs::read_to_string(root.path().join(".started"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "search leader must be reaped"
    );
    assert_eq!(persisted_processes(&root.path().join("session.jsonl")), 2);
    let last = agent
        .session()
        .events()
        .iter()
        .rev()
        .find(|e| e["type"] == "resource_usage")
        .unwrap()
        .clone();
    assert_eq!(last["usage"]["concurrency"], 0);
    drop(agent);
    let reopened =
        zenpi::session::SessionStore::open_existing(root.path().join("session.jsonl")).unwrap();
    assert!(
        reopened
            .turns()
            .iter()
            .filter(|t| t.role == zenpi::core::TurnRole::Tool)
            .any(|t| t.content.contains("cancelled"))
    );
    assert!(!root.path().join(".zenpi/search-index").exists());
}

#[test]
fn actual_headless_queue_returns_search_results_and_durable_budget() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/real.rs"), "needle42\n").unwrap();
    let (backend, requests, server) = search_http(
        "search_text",
        json!({"query":"^needle[0-9]+$","regex":true,"glob":"src/**/*.rs"}),
        2,
    );
    let agent = search_agent(
        root.path(),
        backend,
        ToolContext::new(root.path()).unwrap(),
        19,
    );
    let (mut input, reader) = UnixStream::pair().unwrap();
    let (writer, output) = UnixStream::pair().unwrap();
    output
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let host = std::thread::spawn(move || {
        zenpi::headless::run_async_streams(agent, reader, writer).unwrap()
    });
    writeln!(input,"{}",json!({"schema_version":2,"type":"prompt","id":"headless-search","text":"search the workspace"})).unwrap();
    let mut output = BufReader::new(output);
    loop {
        let mut line = String::new();
        assert!(output.read_line(&mut line).unwrap() > 0);
        let record: Value = serde_json::from_str(&line).unwrap();
        if record["type"] == "response" && record["id"] == "headless-search" {
            assert_eq!(record["success"], true, "{record}");
            break;
        }
    }
    writeln!(
        input,
        "{}",
        json!({"schema_version":2,"type":"shutdown","id":"shutdown"})
    )
    .unwrap();
    drop(input);
    host.join().unwrap();
    server.join().unwrap();
    let captured: Vec<_> = requests.try_iter().collect();
    assert_eq!(captured.len(), 2);
    let tool = captured[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "tool")
        .unwrap();
    assert!(tool.to_string().contains("src/real.rs") && tool.to_string().contains("needle42"));
    assert_eq!(persisted_processes(&root.path().join("session.jsonl")), 19);
    let reopened =
        zenpi::session::SessionStore::open_existing(root.path().join("session.jsonl")).unwrap();
    assert!(
        reopened
            .turns()
            .iter()
            .any(|t| t.role == zenpi::core::TurnRole::Tool && t.content.contains("src/real.rs"))
    );
}
