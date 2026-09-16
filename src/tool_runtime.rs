//! Bounded execution of one provider tool batch through the existing registry.
//!
//! The caller owns policy/approval decisions, governance reservations, operation
//! journaling, output compaction and result persistence. Prepare runs on that
//! owner in source order and cannot rewrite arguments. Persist returned results
//! in source order, then finish the entire input boundary gate batch. This
//! executor neither writes a second journal nor retries an interrupted call.
use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use crate::tools::{
    SideEffectPolicy, ToolCall, ToolContext, ToolError, ToolErrorCode, ToolExecutionMode,
    ToolFailure, ToolPreview, ToolRegistry, ToolResult,
};

pub const MAX_BATCH_CALLS: usize = 32;
pub const MAX_BATCH_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct ToolBatchOptions {
    /// Effective remaining concurrency granted by the host's governance owner.
    /// Zero is a visible rejection, never an implicit grant of one worker.
    pub max_concurrency: usize,
    pub sequential: bool,
    /// A length-terminated provider message may contain incomplete arguments.
    /// Reject the whole batch before preparation or dispatch in that case.
    pub arguments_truncated: bool,
}

impl Default for ToolBatchOptions {
    fn default() -> Self {
        Self {
            max_concurrency: 1,
            sequential: false,
            arguments_truncated: false,
        }
    }
}

/// Produced only after the host has run skill, worker-binding and approval
/// checks and durably recorded dispatch intent. This is not itself an approval
/// authority. The registry rechecks policy and the immutable gate on dispatch.
#[derive(Debug, Clone)]
pub enum ToolBatchDecision {
    Execute,
    ExecuteApproved(Option<ToolPreview>),
    /// The host registers this call's progress queue before dispatch. Context
    /// cloning preserves the original workspace and immutable policy gate.
    ExecuteWithCapture(crate::tool_output::CommandOutputCapture),
    ExecuteApprovedWithCapture {
        preview: Option<ToolPreview>,
        capture: crate::tool_output::CommandOutputCapture,
    },
    Reject(ToolFailure),
}

#[derive(Debug)]
pub struct ToolBatchOutcome {
    pub mode: ToolExecutionMode,
    /// Exactly one terminal result per source call, in source order.
    pub results: Vec<ToolResult>,
}

/// Execute and join every admitted call. `cancelled` and `prepare` need not be
/// Send/Sync: only the owner calls them. Parallel handlers receive a latched
/// cancellation predicate, polled by the owner at most every 5 ms while waiting.
/// Cooperative handlers must poll that predicate; legacy handlers are joined
/// even if they cannot respond promptly. Nothing is detached on cancellation.
///
/// A single sequential/unknown/mutating tool serializes the WHOLE batch,
/// including its read-only neighbours. Invalid batch identities and truncated
/// arguments prevent all effects. Individual policy failures remain correlated
/// results. Host preparation errors can be returned as Reject; the host should
/// latch cancellation too if persistence failed and later calls must not start.
pub fn execute_tool_batch(
    registry: &ToolRegistry,
    context: &ToolContext,
    policy: SideEffectPolicy,
    calls: &[ToolCall],
    options: ToolBatchOptions,
    cancelled: &dyn Fn() -> bool,
    prepare: &mut dyn FnMut(&ToolCall) -> ToolBatchDecision,
) -> Result<ToolBatchOutcome, ToolError> {
    if options.max_concurrency == 0 || options.max_concurrency > MAX_BATCH_CALLS {
        return Err(ToolError::LimitExceeded(
            "batch concurrency must be between 1 and 32".into(),
        ));
    }
    if calls.len() > MAX_BATCH_CALLS
        || serde_json::to_vec(calls).map_err(ToolError::Json)?.len() > MAX_BATCH_BYTES
    {
        return Err(ToolError::LimitExceeded(
            "tool batch exceeds call/byte budget".into(),
        ));
    }
    let mode = if options.sequential
        || options.max_concurrency == 1
        || calls
            .iter()
            .any(|call| registry.execution_mode(&call.name) == ToolExecutionMode::Sequential)
    {
        ToolExecutionMode::Sequential
    } else {
        ToolExecutionMode::Parallel
    };
    let mut ids = BTreeSet::new();
    let invalid = options.arguments_truncated
        || calls
            .iter()
            .any(|call| crate::tools::validate_call(call).is_err() || !ids.insert(&call.id));
    if invalid {
        return Ok(ToolBatchOutcome {
            mode,
            results: calls
                .iter()
                .map(|call| {
                    failure(
                        call,
                        ToolErrorCode::InvalidCall,
                        "invalid, duplicate or truncated tool batch; no calls executed",
                    )
                })
                .collect(),
        });
    }
    if mode == ToolExecutionMode::Sequential {
        let mut stopped = false;
        let mut results = Vec::with_capacity(calls.len());
        for call in calls {
            stopped |= cancelled();
            let result = if stopped {
                cancellation(call)
            } else {
                let decision = prepare(call);
                // Approval can finish after the cancellation epoch changed.
                stopped |= cancelled();
                if stopped {
                    cancellation(call)
                } else {
                    dispatch(registry, context, policy, call, &decision, cancelled)
                }
            };
            stopped |= matches!(&result, ToolResult::Error { error, .. } if error.code == ToolErrorCode::Cancelled);
            // A possibly-started side effect with no trustworthy outcome
            // blocks the rest of the batch. The host must persist/reconcile
            // this uncertainty; cancellation is not evidence of rollback.
            if registry.definition(&call.name).is_some_and(|definition| {
                definition.side_effect != crate::tools::ToolSideEffect::ReadOnly
            }) && matches!(&result, ToolResult::Error { error, .. } if matches!(error.code,
                    ToolErrorCode::Io | ToolErrorCode::Internal | ToolErrorCode::CommandTimeout | ToolErrorCode::Cancelled))
            {
                stopped = true;
            }
            results.push(result);
        }
        return Ok(ToolBatchOutcome { mode, results });
    }

    let stop = AtomicBool::new(false);
    let decisions: Vec<_> = calls
        .iter()
        .map(|call| {
            if cancelled() {
                stop.store(true, Ordering::SeqCst);
            }
            if stop.load(Ordering::SeqCst) {
                ToolBatchDecision::Reject(ToolFailure {
                    code: ToolErrorCode::Cancelled,
                    message: "batch cancelled before preparation".into(),
                })
            } else {
                prepare(call)
            }
        })
        .collect();
    if cancelled() {
        stop.store(true, Ordering::SeqCst);
    }
    let next = AtomicUsize::new(0);
    let mut results: Vec<Option<ToolResult>> = vec![None; calls.len()];
    std::thread::scope(|scope| {
        // On owner unwind, request cancellation BEFORE scope joins workers.
        struct StopOnDrop<'a>(&'a AtomicBool);
        impl Drop for StopOnDrop<'_> {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let _guard = StopOnDrop(&stop);
        let (sender, receiver) = mpsc::channel();
        let mut handles = Vec::new();
        for worker in 0..options.max_concurrency.min(calls.len()) {
            let sender = sender.clone();
            let stop = &stop;
            let next = &next;
            let decisions = &decisions;
            let spawned = std::thread::Builder::new().name(format!("zenpi-tool-{worker}"))
                .spawn_scoped(scope, move || loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(call) = calls.get(index) else { break; };
                    let result = if stop.load(Ordering::SeqCst) { cancellation(call) }
                        else { dispatch(registry, context, policy, call, &decisions[index],
                            &|| stop.load(Ordering::SeqCst)) };
                    if matches!(&result, ToolResult::Error { error, .. } if error.code == ToolErrorCode::Cancelled) {
                        stop.store(true, Ordering::SeqCst);
                    }
                    if sender.send((index, result)).is_err() { break; }
                });
            match spawned {
                Ok(handle) => handles.push(handle),
                Err(_) => {
                    stop.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
        drop(sender);
        let mut received = 0;
        while received < calls.len() {
            if cancelled() {
                stop.store(true, Ordering::SeqCst);
            }
            match receiver.recv_timeout(Duration::from_millis(5)) {
                Ok((index, result)) => {
                    results[index] = Some(result);
                    received += 1;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        for handle in handles {
            if handle.join().is_err() {
                stop.store(true, Ordering::SeqCst);
            }
        }
    });
    Ok(ToolBatchOutcome {
        mode,
        results: results
            .into_iter()
            .zip(calls)
            .map(|(result, call)| {
                result.unwrap_or_else(|| {
                    failure(
                        call,
                        ToolErrorCode::Internal,
                        "batch worker unavailable; inspect operation journal before retry",
                    )
                })
            })
            .collect(),
    })
}

fn dispatch(
    registry: &ToolRegistry,
    context: &ToolContext,
    policy: SideEffectPolicy,
    call: &ToolCall,
    decision: &ToolBatchDecision,
    cancelled: &dyn Fn() -> bool,
) -> ToolResult {
    catch_unwind(AssertUnwindSafe(|| {
        if cancelled() {
            return cancellation(call);
        }
        match decision {
            ToolBatchDecision::ExecuteWithCapture(capture) => {
                match context.clone().with_output_capture(capture.clone()) {
                    Ok(context) => dispatch(
                        registry,
                        &context,
                        policy,
                        call,
                        &ToolBatchDecision::Execute,
                        cancelled,
                    ),
                    Err(error) => failure(
                        call,
                        error.code(),
                        &crate::security::redact_text(&error.to_string(), &[]),
                    ),
                }
            }
            ToolBatchDecision::ExecuteApprovedWithCapture { preview, capture } => {
                match context.clone().with_output_capture(capture.clone()) {
                    Ok(context) => dispatch(
                        registry,
                        &context,
                        policy,
                        call,
                        &ToolBatchDecision::ExecuteApproved(preview.clone()),
                        cancelled,
                    ),
                    Err(error) => failure(
                        call,
                        error.code(),
                        &crate::security::redact_text(&error.to_string(), &[]),
                    ),
                }
            }
            ToolBatchDecision::Reject(error) => ToolResult::Error {
                call_id: call.id.clone(),
                tool: call.name.clone(),
                error: ToolFailure {
                    code: error.code,
                    message: crate::security::redact_text(&error.message, &[]),
                },
            },
            ToolBatchDecision::ExecuteApproved(expected) => {
                match registry.approval_preview(context, call) {
                    Ok(actual) if &actual == expected => {}
                    Ok(_) => {
                        return failure(
                            call,
                            ToolErrorCode::StalePreview,
                            "workspace changed after approval; review the new diff and retry",
                        );
                    }
                    Err(error) => {
                        return failure(
                            call,
                            error.code(),
                            &crate::security::redact_text(&error.to_string(), &[]),
                        );
                    }
                }
                registry.execute_cancellable(context, policy, call.clone(), cancelled)
            }
            ToolBatchDecision::Execute => {
                registry.execute_cancellable(context, policy, call.clone(), cancelled)
            }
        }
    }))
    .unwrap_or_else(|_| {
        failure(
            call,
            ToolErrorCode::Internal,
            "tool handler panicked; inspect operation journal before retry",
        )
    })
}

fn cancellation(call: &ToolCall) -> ToolResult {
    failure(
        call,
        ToolErrorCode::Cancelled,
        "batch cancelled; pending call was not executed",
    )
}

fn failure(call: &ToolCall, code: ToolErrorCode, message: &str) -> ToolResult {
    ToolResult::Error {
        call_id: call.id.clone(),
        tool: call.name.clone(),
        error: ToolFailure {
            code,
            message: message.into(),
        },
    }
}
