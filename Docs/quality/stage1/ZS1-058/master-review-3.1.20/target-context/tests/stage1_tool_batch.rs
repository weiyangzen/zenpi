use serde_json::{Map, Value, json};
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};
use tempfile::tempdir;
use zenpi::tool_runtime::{ToolBatchDecision, ToolBatchOptions, execute_tool_batch};
use zenpi::tools::*;

// Exercise the actual Agent -> HTTP/SSE -> registry -> journal path, in addition
// to the executor contracts below. No backend completion is synthesized in Agent.
struct HostRead {
    trace: Arc<Trace>,
    mode: ToolExecutionMode,
    wait_cancel: bool,
}
impl Tool for HostRead {
    fn execution_mode(&self) -> ToolExecutionMode {
        self.mode
    }
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "host_read".into(),
            description: "Read a real fixture with execution timing".into(),
            input_schema: json!({"type":"object"}),
            side_effect: ToolSideEffect::ReadOnly,
        }
    }
    fn invoke(&self, context: &ToolContext, args: &Map<String, Value>) -> Result<Value, ToolError> {
        self.invoke_cancellable(context, args, &|| false)
    }
    fn invoke_cancellable(
        &self,
        context: &ToolContext,
        args: &Map<String, Value>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        self.trace.starts.fetch_add(1, Ordering::SeqCst);
        let active = self.trace.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.trace.peak.fetch_max(active, Ordering::SeqCst);
        struct Active<'a>(&'a AtomicUsize);
        impl Drop for Active<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _active = Active(&self.trace.active);
        let path = args["path"].as_str().unwrap();
        let wait = if self.wait_cancel {
            2000
        } else if path == "first" {
            60
        } else {
            5
        };
        let deadline = Instant::now() + Duration::from_millis(wait);
        while Instant::now() < deadline {
            if cancelled() {
                return Err(ToolError::Cancelled);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if self.wait_cancel {
            return Err(ToolError::CommandTimeout(2000));
        }
        let result = ReadFileTool.invoke(context, args);
        self.trace.finishes.lock().unwrap().push(path.into());
        result
    }
}

fn host_http(
    count: usize,
) -> (
    zenpi::backend::OpenAiCompatibleBackend,
    Arc<Mutex<Vec<Value>>>,
    std::thread::JoinHandle<()>,
) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let captured = Arc::new(Mutex::new(Vec::new()));
    let records = captured.clone();
    let handle = std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
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
                    Err(error) => panic!("HTTP accept: {error}"),
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
                assert_ne!(n, 0, "incomplete HTTP request");
                bytes.extend_from_slice(&chunk[..n]);
                assert!(bytes.len() <= 2 * 1024 * 1024);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = std::str::from_utf8(&bytes[..end]).unwrap();
                    let length: usize = header
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    if bytes.len() >= end + 4 + length {
                        break serde_json::from_slice::<Value>(&bytes[end + 4..end + 4 + length])
                            .unwrap();
                    }
                }
            };
            assert_eq!(request["stream"], true);
            records.lock().unwrap().push(request);
            let delta = if index == 0 {
                json!({"tool_calls":[
                    {"index":0,"id":"host-first","type":"function","function":{"name":"host_read","arguments":"{\"path\":\"first\"}"}},
                    {"index":1,"id":"host-second","type":"function","function":{"name":"host_read","arguments":"{\"path\":\"second\"}"}}
                ]})
            } else {
                json!({"content":"actual continuation"})
            };
            let body = format!(
                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"id":"host-batch","model":"fixture","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
                json!({"id":"host-batch","model":"fixture","choices":[{"index":0,"delta":{},"finish_reason":if index==0 {"tool_calls"} else {"stop"}}]})
            );
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            stream.flush().unwrap();
        }
    });
    let backend = zenpi::backend::OpenAiCompatibleBackend::from_values_with_wire_api(
        url,
        Some("fixture-only".into()),
        "fixture".into(),
        zenpi::backend::OpenAiWireApi::ChatCompletions,
    )
    .unwrap();
    (backend, captured, handle)
}

fn host_agent(
    root: &std::path::Path,
    backend: zenpi::backend::OpenAiCompatibleBackend,
    tool: HostRead,
    concurrency: u64,
) -> zenpi::core::Agent {
    fs::write(root.join("first"), "FIRST_REAL_BYTES").unwrap();
    fs::write(root.join("second"), "SECOND_REAL_BYTES").unwrap();
    let session =
        zenpi::session::SessionStore::open_in_workspace(root.join("session.jsonl"), root).unwrap();
    let mut agent = zenpi::core::Agent::new(session, Box::new(backend));
    let mut registry = ToolRegistry::new();
    registry.register(tool).unwrap();
    agent.set_tools(
        registry,
        ToolContext::new(root).unwrap(),
        SideEffectPolicy::read_only(),
    );
    agent
        .set_resource_limits(zenpi::governance::ResourceLimits {
            max_concurrency: concurrency,
            ..Default::default()
        })
        .unwrap();
    agent
}

#[test]
fn actual_http_agent_runs_reads_in_parallel_and_persists_source_order_across_reopen() {
    let root = tempdir().unwrap();
    let trace = Arc::new(Trace::default());
    let (backend, requests, server) = host_http(2);
    let mut agent = host_agent(
        root.path(),
        backend,
        HostRead {
            trace: trace.clone(),
            mode: ToolExecutionMode::Parallel,
            wait_cancel: false,
        },
        2,
    );
    agent.process_sync("read both files").unwrap();
    server.join().unwrap();
    assert_eq!(trace.peak.load(Ordering::SeqCst), 2);
    assert_eq!(*trace.finishes.lock().unwrap(), ["second", "first"]);
    let requests = requests.lock().unwrap();
    let messages = requests[1]["messages"].as_array().unwrap();
    let results: Vec<_> = messages.iter().filter(|m| m["role"] == "tool").collect();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["tool_call_id"], "host-first");
    assert_eq!(results[1]["tool_call_id"], "host-second");
    assert!(results[0].to_string().contains("FIRST_REAL_BYTES"));
    assert!(results[1].to_string().contains("SECOND_REAL_BYTES"));
    let usage: Vec<_> = agent
        .session()
        .events()
        .iter()
        .filter(|e| e["type"] == "resource_usage")
        .map(|e| e["usage"]["concurrency"].as_u64().unwrap())
        .collect();
    assert_eq!(usage.iter().copied().max(), Some(2));
    assert_eq!(usage.last(), Some(&0));
    drop(agent);
    let reopened =
        zenpi::session::SessionStore::open_existing(root.path().join("session.jsonl")).unwrap();
    let ids: Vec<_> = reopened
        .turns()
        .iter()
        .filter(|t| t.role == zenpi::core::TurnRole::Tool)
        .map(|t| {
            t.metadata.as_ref().unwrap()["tool_call_id"]
                .as_str()
                .unwrap()
        })
        .collect();
    assert_eq!(ids, ["host-first", "host-second"]);
    assert_eq!(trace.starts.load(Ordering::SeqCst), 2);
}

#[test]
fn actual_agent_honors_governance_cap_global_and_per_tool_serial_modes() {
    for (cap, global, mode) in [
        (1, false, ToolExecutionMode::Parallel),
        (8, true, ToolExecutionMode::Parallel),
        (8, false, ToolExecutionMode::Sequential),
    ] {
        let root = tempdir().unwrap();
        let trace = Arc::new(Trace::default());
        let (backend, _, server) = host_http(2);
        let mut agent = host_agent(
            root.path(),
            backend,
            HostRead {
                trace: trace.clone(),
                mode,
                wait_cancel: false,
            },
            cap,
        );
        agent.set_tool_batch_sequential(global).unwrap();
        agent.process_sync("read serially").unwrap();
        server.join().unwrap();
        assert_eq!(trace.peak.load(Ordering::SeqCst), 1);
        assert_eq!(*trace.finishes.lock().unwrap(), ["first", "second"]);
    }
}

#[test]
fn actual_agent_cancellation_joins_parallel_reads_and_releases_durable_reservation() {
    let root = tempdir().unwrap();
    let trace = Arc::new(Trace::default());
    let (backend, requests, server) = host_http(1);
    let mut agent = host_agent(
        root.path(),
        backend,
        HostRead {
            trace: trace.clone(),
            mode: ToolExecutionMode::Parallel,
            wait_cancel: true,
        },
        2,
    );
    agent
        .submit(zenpi::core::TurnInputRequest::new("cancel during reads"))
        .unwrap();
    assert!(
        agent
            .run_active_turn_cancelable(|| trace.active.load(Ordering::SeqCst) == 2)
            .is_err()
    );
    server.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert_eq!(trace.starts.load(Ordering::SeqCst), 2);
    assert_eq!(trace.active.load(Ordering::SeqCst), 0);
    let terminal: Vec<_> = agent
        .session()
        .events()
        .iter()
        .filter(|e| e["type"] == "tool_execution_finished")
        .collect();
    assert_eq!(terminal.len(), 2);
    assert!(terminal.iter().all(|e| e["outcome"] == "cancelled"));
    let usage = agent
        .session()
        .events()
        .iter()
        .rev()
        .find(|e| e["type"] == "resource_usage")
        .unwrap();
    assert_eq!(usage["usage"]["concurrency"], 0);
}

fn call(id: &str, name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments,
    }
}
fn options(n: usize) -> ToolBatchOptions {
    ToolBatchOptions {
        max_concurrency: n,
        ..Default::default()
    }
}
fn code(result: &ToolResult) -> ToolErrorCode {
    match result {
        ToolResult::Error { error, .. } => error.code,
        _ => panic!("expected error: {result:?}"),
    }
}
fn output(result: &ToolResult) -> &Value {
    match result {
        ToolResult::Success { output, .. } => output,
        _ => panic!("expected success: {result:?}"),
    }
}
#[derive(Default)]
struct Trace {
    active: AtomicUsize,
    peak: AtomicUsize,
    starts: AtomicUsize,
    finishes: Mutex<Vec<String>>,
}
struct Probe {
    name: &'static str,
    mode: ToolExecutionMode,
    trace: Arc<Trace>,
    pair: Option<Arc<Barrier>>,
    reverse: bool,
    wait_cancel: bool,
}
impl Tool for Probe {
    fn execution_mode(&self) -> ToolExecutionMode {
        self.mode
    }
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.into(),
            description: "Instrumented real workspace read".into(),
            input_schema: json!({"type":"object"}),
            side_effect: ToolSideEffect::ReadOnly,
        }
    }
    fn invoke(&self, context: &ToolContext, args: &Map<String, Value>) -> Result<Value, ToolError> {
        self.invoke_cancellable(context, args, &|| false)
    }
    fn invoke_cancellable(
        &self,
        context: &ToolContext,
        args: &Map<String, Value>,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Value, ToolError> {
        self.trace.starts.fetch_add(1, Ordering::SeqCst);
        let active = self.trace.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.trace.peak.fetch_max(active, Ordering::SeqCst);
        struct Exit<'a>(&'a AtomicUsize);
        impl Drop for Exit<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _exit = Exit(&self.trace.active);
        if let Some(pair) = &self.pair {
            pair.wait();
        }
        if self.wait_cancel {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !cancel() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            return if cancel() {
                Err(ToolError::Cancelled)
            } else {
                Err(ToolError::CommandTimeout(3000))
            };
        }
        let path = args["path"].as_str().unwrap();
        if self.reverse && path == "first" {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !self
                .trace
                .finishes
                .lock()
                .unwrap()
                .contains(&"second".to_owned())
            {
                assert!(Instant::now() < deadline, "second read did not finish");
                std::thread::yield_now();
            }
        }
        let result = ReadFileTool.invoke(context, args);
        self.trace.finishes.lock().unwrap().push(path.into());
        result
    }
}
fn probe(name: &'static str, trace: &Arc<Trace>, mode: ToolExecutionMode) -> Probe {
    Probe {
        name,
        mode,
        trace: trace.clone(),
        pair: None,
        reverse: false,
        wait_cancel: false,
    }
}
#[test]
fn real_reads_overlap_with_bounded_concurrency_and_return_in_source_order() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("first"), "alpha").unwrap();
    fs::write(root.path().join("second"), "beta").unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let trace = Arc::new(Trace::default());
    let mut p = probe("probe", &trace, ToolExecutionMode::Parallel);
    p.pair = Some(Arc::new(Barrier::new(2)));
    p.reverse = true;
    let mut registry = ToolRegistry::new();
    registry.register(p).unwrap();
    let calls = vec![
        call("a", "probe", json!({"path":"first"})),
        call("b", "probe", json!({"path":"second"})),
        call("c", "probe", json!({"path":"first"})),
        call("d", "probe", json!({"path":"second"})),
    ];
    let owner = std::thread::current().id();
    let mut prepared = Vec::new();
    let result = execute_tool_batch(
        &registry,
        &context,
        SideEffectPolicy::read_only(),
        &calls,
        options(2),
        &|| false,
        &mut |c| {
            assert_eq!(std::thread::current().id(), owner);
            prepared.push(c.id.clone());
            ToolBatchDecision::Execute
        },
    )
    .unwrap();
    assert_eq!(prepared, vec!["a", "b", "c", "d"]);
    assert_eq!(result.mode, ToolExecutionMode::Parallel);
    assert_eq!(trace.peak.load(Ordering::SeqCst), 2);
    assert_eq!(trace.active.load(Ordering::SeqCst), 0);
    assert_eq!(trace.finishes.lock().unwrap()[0], "second");
    for (i, r) in result.results.iter().enumerate() {
        match r {
            ToolResult::Success { call_id, .. } => assert_eq!(call_id, &calls[i].id),
            _ => panic!("{r:?}"),
        };
        assert_eq!(
            output(r)["content"],
            if i % 2 == 0 { "alpha" } else { "beta" }
        );
    }
}
#[test]
fn one_tool_or_global_sequential_declaration_serializes_entire_batch() {
    for global in [false, true] {
        let root = tempdir().unwrap();
        fs::write(root.path().join("first"), "a").unwrap();
        let context = ToolContext::new(root.path()).unwrap();
        let trace = Arc::new(Trace::default());
        let mut registry = ToolRegistry::new();
        registry
            .register(probe("parallel", &trace, ToolExecutionMode::Parallel))
            .unwrap();
        registry
            .register(probe(
                "barrier",
                &trace,
                if global {
                    ToolExecutionMode::Parallel
                } else {
                    ToolExecutionMode::Sequential
                },
            ))
            .unwrap();
        let calls = ["parallel", "barrier", "parallel"]
            .into_iter()
            .enumerate()
            .map(|(i, n)| call(&format!("c{i}"), n, json!({"path":"first"})))
            .collect::<Vec<_>>();
        let mut opt = options(3);
        opt.sequential = global;
        let mut prep_seen = Vec::new();
        let outcome = execute_tool_batch(
            &registry,
            &context,
            SideEffectPolicy::read_only(),
            &calls,
            opt,
            &|| false,
            &mut |_| {
                prep_seen.push(trace.starts.load(Ordering::SeqCst));
                ToolBatchDecision::Execute
            },
        )
        .unwrap();
        assert_eq!(outcome.mode, ToolExecutionMode::Sequential);
        assert_eq!(trace.peak.load(Ordering::SeqCst), 1);
        assert_eq!(prep_seen, vec![0, 1, 2]);
        assert!(outcome.results.iter().all(ToolResult::is_success));
    }
}
#[test]
fn write_between_reads_observes_source_order_and_forms_whole_batch_barrier() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file"), "before").unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let calls = [
        call("a", "read_file", json!({"path":"file"})),
        call("b", "write_file", json!({"path":"file","content":"after"})),
        call("c", "read_file", json!({"path":"file"})),
    ];
    let outcome = execute_tool_batch(
        &registry,
        &context,
        SideEffectPolicy::all_builtins(),
        &calls,
        options(3),
        &|| false,
        &mut |c| {
            ToolBatchDecision::ExecuteApproved(registry.approval_preview(&context, c).unwrap())
        },
    )
    .unwrap();
    assert_eq!(outcome.mode, ToolExecutionMode::Sequential);
    assert_eq!(output(&outcome.results[0])["content"], "before");
    assert_eq!(output(&outcome.results[2])["content"], "after");
}
#[test]
fn malformed_duplicate_and_length_terminated_batches_have_zero_dispatch() {
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    for case in 0..4 {
        let mut calls = vec![
            call("a", "write_file", json!({"path":"file","content":"bad"})),
            call("b", "read_file", json!({"path":"file"})),
        ];
        let mut opt = options(2);
        match case {
            0 => calls[1].id = "a".into(),
            1 => calls[1].id = "bad id".into(),
            2 => calls[1].arguments = json!("truncated"),
            _ => opt.arguments_truncated = true,
        };
        let mut prepared = 0;
        let outcome = execute_tool_batch(
            &registry,
            &context,
            SideEffectPolicy::all_builtins(),
            &calls,
            opt,
            &|| false,
            &mut |_| {
                prepared += 1;
                ToolBatchDecision::Execute
            },
        )
        .unwrap();
        assert_eq!(prepared, 0);
        assert!(!root.path().join("file").exists());
        assert!(
            outcome
                .results
                .iter()
                .all(|r| code(r) == ToolErrorCode::InvalidCall)
        );
    }
}
#[test]
fn approval_denial_and_side_effect_policy_both_prevent_writes() {
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let calls = [call(
        "a",
        "write_file",
        json!({"path":"file","content":"bad"}),
    )];
    for reject in [true, false] {
        let result = execute_tool_batch(
            &registry,
            &context,
            if reject {
                SideEffectPolicy::all_builtins()
            } else {
                SideEffectPolicy::read_only()
            },
            &calls,
            options(2),
            &|| false,
            &mut |_| {
                if reject {
                    ToolBatchDecision::Reject(ToolFailure {
                        code: ToolErrorCode::PolicyDenied,
                        message: "host approval denied".into(),
                    })
                } else {
                    ToolBatchDecision::Execute
                }
            },
        )
        .unwrap();
        assert_eq!(code(&result.results[0]), ToolErrorCode::PolicyDenied);
        assert!(!root.path().join("file").exists());
    }
}
#[test]
fn approved_preview_is_rechecked_at_dispatch() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file"), "before").unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let calls = [call(
        "a",
        "write_file",
        json!({"path":"file","content":"after"}),
    )];
    let preview = registry.approval_preview(&context, &calls[0]).unwrap();
    fs::write(root.path().join("file"), "concurrent").unwrap();
    let outcome = execute_tool_batch(
        &registry,
        &context,
        SideEffectPolicy::all_builtins(),
        &calls,
        options(2),
        &|| false,
        &mut |_| ToolBatchDecision::ExecuteApproved(preview.clone()),
    )
    .unwrap();
    assert_eq!(code(&outcome.results[0]), ToolErrorCode::StalePreview);
    assert_eq!(
        fs::read_to_string(root.path().join("file")).unwrap(),
        "concurrent"
    );
}
#[test]
fn cancellation_stops_queued_calls_and_joins_running_parallel_handlers() {
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let trace = Arc::new(Trace::default());
    let mut registry = ToolRegistry::new();
    let mut p = probe("waiting", &trace, ToolExecutionMode::Parallel);
    p.wait_cancel = true;
    registry.register(p).unwrap();
    let calls = (0..8)
        .map(|i| call(&format!("c{i}"), "waiting", json!({})))
        .collect::<Vec<_>>();
    let outcome = execute_tool_batch(
        &registry,
        &context,
        SideEffectPolicy::read_only(),
        &calls,
        options(2),
        &|| trace.active.load(Ordering::SeqCst) == 2,
        &mut |_| ToolBatchDecision::Execute,
    )
    .unwrap();
    assert_eq!(trace.starts.load(Ordering::SeqCst), 2);
    assert_eq!(trace.active.load(Ordering::SeqCst), 0);
    assert!(
        outcome
            .results
            .iter()
            .all(|r| code(r) == ToolErrorCode::Cancelled)
    );
}
#[test]
fn cancellation_during_preparation_prevents_all_parallel_dispatch() {
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let trace = Arc::new(Trace::default());
    let mut registry = ToolRegistry::new();
    registry
        .register(probe("read", &trace, ToolExecutionMode::Parallel))
        .unwrap();
    let stop = AtomicBool::new(false);
    let calls = [call("a", "read", json!({})), call("b", "read", json!({}))];
    let outcome = execute_tool_batch(
        &registry,
        &context,
        SideEffectPolicy::read_only(),
        &calls,
        options(2),
        &|| stop.load(Ordering::SeqCst),
        &mut |_| {
            stop.store(true, Ordering::SeqCst);
            ToolBatchDecision::Execute
        },
    )
    .unwrap();
    assert_eq!(trace.starts.load(Ordering::SeqCst), 0);
    assert!(
        outcome
            .results
            .iter()
            .all(|r| code(r) == ToolErrorCode::Cancelled)
    );
}
#[test]
fn cancellation_after_last_approval_prevents_dispatch() {
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let stop = AtomicBool::new(false);
    let calls = [call(
        "a",
        "write_file",
        json!({"path":"file","content":"bad"}),
    )];
    let outcome = execute_tool_batch(
        &registry,
        &context,
        SideEffectPolicy::all_builtins(),
        &calls,
        options(2),
        &|| stop.load(Ordering::SeqCst),
        &mut |_| {
            stop.store(true, Ordering::SeqCst);
            ToolBatchDecision::Execute
        },
    )
    .unwrap();
    assert_eq!(code(&outcome.results[0]), ToolErrorCode::Cancelled);
    assert!(!root.path().join("file").exists());
}
#[test]
fn host_budget_rejection_is_visible_and_never_runs_preparation() {
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let registry = ToolRegistry::new();
    for n in [0, 33] {
        assert!(
            execute_tool_batch(
                &registry,
                &context,
                SideEffectPolicy::read_only(),
                &[],
                options(n),
                &|| false,
                &mut |_| panic!("prepare")
            )
            .is_err()
        );
    }
    let calls = (0..33)
        .map(|i| call(&format!("c{i}"), "unknown", json!({})))
        .collect::<Vec<_>>();
    assert!(
        execute_tool_batch(
            &registry,
            &context,
            SideEffectPolicy::read_only(),
            &calls,
            options(2),
            &|| false,
            &mut |_| panic!("prepare")
        )
        .is_err()
    );
}
#[test]
fn parallel_builtins_recheck_worker_gate_and_default_extensions_are_sequential() {
    struct Legacy;
    impl Tool for Legacy {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "legacy".into(),
                description: "legacy".into(),
                input_schema: json!({"type":"object"}),
                side_effect: ToolSideEffect::ReadOnly,
            }
        }
        fn invoke(&self, _: &ToolContext, _: &Map<String, Value>) -> Result<Value, ToolError> {
            panic!("not called")
        }
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("file"), "secret").unwrap();
    let context = ToolContext::new(root.path())
        .unwrap()
        .with_origin(ToolOrigin::BlueprintWorker);
    let mut registry = ToolRegistry::with_all_builtins().unwrap();
    registry.register(Legacy).unwrap();
    assert_eq!(
        registry.execution_mode("legacy"),
        ToolExecutionMode::Sequential
    );
    assert_eq!(
        registry.execution_mode("unknown"),
        ToolExecutionMode::Sequential
    );
    for name in ["read_file", "list_directory", "search_text"] {
        assert_eq!(registry.execution_mode(name), ToolExecutionMode::Parallel);
    }
    let calls = [
        call("a", "read_file", json!({"path":"file"})),
        call("b", "list_directory", json!({})),
    ];
    let outcome = execute_tool_batch(
        &registry,
        &context,
        SideEffectPolicy::all_builtins(),
        &calls,
        options(2),
        &|| false,
        &mut |_| ToolBatchDecision::Execute,
    )
    .unwrap();
    assert!(
        outcome
            .results
            .iter()
            .all(|r| code(r) == ToolErrorCode::PolicyDenied)
    );
}
#[cfg(unix)]
#[test]
fn cancelling_real_command_kills_and_reaps_its_process_group_before_return() {
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let calls = [
        call(
            "command",
            "run_command",
            json!({"command":"echo $$ > leader; sleep 20 & echo $! > descendant; wait","timeout_ms":30000}),
        ),
        call(
            "pending",
            "write_file",
            json!({"path":"should-not-exist","content":"bad"}),
        ),
    ];
    let began = Instant::now();
    let outcome = execute_tool_batch(
        &registry,
        &context,
        SideEffectPolicy::all_builtins(),
        &calls,
        options(3),
        &|| root.path().join("descendant").exists(),
        &mut |_| ToolBatchDecision::Execute,
    )
    .unwrap();
    assert!(began.elapsed() < Duration::from_secs(5));
    assert!(
        outcome
            .results
            .iter()
            .all(|r| code(r) == ToolErrorCode::Cancelled)
    );
    assert!(!root.path().join("should-not-exist").exists());
    let leader: i32 = fs::read_to_string(root.path().join("leader"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // SAFETY: signal 0 observes existence only; it does not signal another process.
    assert_eq!(unsafe { libc::kill(leader, 0) }, -1);
    let mut status = 0;
    // SAFETY: waitpid only checks the already-reaped child PID.
    assert_eq!(
        unsafe { libc::waitpid(leader, &mut status, libc::WNOHANG) },
        -1
    );
}

#[test]
fn cancellation_in_the_final_parallel_prepare_is_seen_before_workers_spawn() {
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let trace = Arc::new(Trace::default());
    let mut registry = ToolRegistry::new();
    registry
        .register(probe("read", &trace, ToolExecutionMode::Parallel))
        .unwrap();
    let stop = AtomicBool::new(false);
    let calls = [call("only", "read", json!({}))];
    let outcome = execute_tool_batch(
        &registry,
        &context,
        SideEffectPolicy::read_only(),
        &calls,
        options(2),
        &|| stop.load(Ordering::SeqCst),
        &mut |_| {
            stop.store(true, Ordering::SeqCst);
            ToolBatchDecision::Execute
        },
    )
    .unwrap();
    assert_eq!(trace.starts.load(Ordering::SeqCst), 0);
    assert_eq!(code(&outcome.results[0]), ToolErrorCode::Cancelled);
}

#[test]
fn a_panicking_handler_keeps_call_identity_and_other_results() {
    struct Panics;
    impl Tool for Panics {
        fn execution_mode(&self) -> ToolExecutionMode {
            ToolExecutionMode::Parallel
        }
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "panics".into(),
                description: "failure probe".into(),
                input_schema: json!({"type":"object"}),
                side_effect: ToolSideEffect::ReadOnly,
            }
        }
        fn invoke(&self, _: &ToolContext, _: &Map<String, Value>) -> Result<Value, ToolError> {
            panic!("intentional test panic")
        }
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("file"), "retained").unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let mut registry = ToolRegistry::with_read_only_builtins().unwrap();
    registry.register(Panics).unwrap();
    let calls = [
        call("panic-id", "panics", json!({})),
        call("read-id", "read_file", json!({"path":"file"})),
    ];
    let outcome = execute_tool_batch(
        &registry,
        &context,
        SideEffectPolicy::read_only(),
        &calls,
        options(2),
        &|| false,
        &mut |_| ToolBatchDecision::Execute,
    )
    .unwrap();
    assert_eq!(code(&outcome.results[0]), ToolErrorCode::Internal);
    assert!(matches!(&outcome.results[0],ToolResult::Error{call_id,..} if call_id=="panic-id"));
    assert_eq!(output(&outcome.results[1])["content"], "retained");
}

#[test]
fn blueprint_revoked_after_preparation_is_denied_inside_parallel_registry_dispatch() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file"), "retained").unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let policy = BlueprintPolicySpec {
        blueprint_digest: "a".repeat(64),
        goal_digest: "b".repeat(64),
        item_id: "ZS1-102".into(),
        allowed_tools: ["read_file".into()].into(),
        readable_paths: [".".into()].into(),
        writable_paths: Default::default(),
        denied_paths: Default::default(),
        protected_paths: Default::default(),
        allowed_commands: Default::default(),
        denied_commands: Default::default(),
        network_hosts: Default::default(),
        max_actions: 2,
        max_command_timeout_ms: 1000,
        max_command_output_bytes: 1024,
    };
    let (gate, revoke) = BlueprintGate::compile(
        &context,
        policy,
        BlueprintLease {
            lease_id: "batch-lease".into(),
            issued_at_ms: now,
            expires_at_ms: now + 10000,
        },
        now,
    )
    .unwrap();
    let context = context.with_blueprint_gate(gate).unwrap();
    let registry = ToolRegistry::with_read_only_builtins().unwrap();
    let calls = [
        call("a", "read_file", json!({"path":"file"})),
        call("b", "read_file", json!({"path":"file"})),
    ];
    let outcome = execute_tool_batch(
        &registry,
        &context,
        SideEffectPolicy::read_only(),
        &calls,
        options(2),
        &|| false,
        &mut |_| {
            revoke.revoke();
            ToolBatchDecision::Execute
        },
    )
    .unwrap();
    assert!(
        outcome
            .results
            .iter()
            .all(|r| code(r) == ToolErrorCode::PolicyDenied)
    );
}

#[cfg(unix)]
#[test]
fn unknown_command_timeout_blocks_later_side_effects_without_retry() {
    let root = tempdir().unwrap();
    let context = ToolContext::new(root.path()).unwrap();
    let registry = ToolRegistry::with_all_builtins().unwrap();
    let calls = [
        call(
            "timeout",
            "run_command",
            json!({"command":"sleep 20","timeout_ms":10}),
        ),
        call(
            "pending",
            "write_file",
            json!({"path":"should-not-exist","content":"bad"}),
        ),
    ];
    let mut prepared = Vec::new();
    let outcome = execute_tool_batch(
        &registry,
        &context,
        SideEffectPolicy::all_builtins(),
        &calls,
        options(3),
        &|| false,
        &mut |c| {
            prepared.push(c.id.clone());
            ToolBatchDecision::Execute
        },
    )
    .unwrap();
    assert_eq!(prepared, vec!["timeout"]);
    assert_eq!(code(&outcome.results[0]), ToolErrorCode::CommandTimeout);
    assert_eq!(code(&outcome.results[1]), ToolErrorCode::Cancelled);
    assert!(!root.path().join("should-not-exist").exists());
}
