use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use zenpi::runtime::{BackgroundRunner, JobOutcome, RuntimeConfig, RuntimeEvent, SubmitError};
use zenpi::{core::Agent, session::SessionStore};

fn wait_event<O, E, F, I, A>(
    runner: &BackgroundRunner<I, O, E, F>,
    mut predicate: A,
) -> RuntimeEvent<O, E>
where
    I: Send + 'static,
    O: Send + 'static,
    E: Send + 'static,
    F: Fn(I, zenpi::runtime::CancellationToken) -> Result<O, E> + Send + Sync + 'static,
    A: FnMut(&RuntimeEvent<O, E>) -> bool,
{
    loop {
        let event = runner.next_event().expect("runtime event channel closed");
        if predicate(&event) {
            return event;
        }
    }
}

#[test]
fn work_runs_off_thread_and_emits_a_terminal_result() {
    let runner = BackgroundRunner::spawn(
        |request: String, _token| {
            thread::sleep(Duration::from_millis(20));
            Ok::<_, String>(request.len())
        },
        RuntimeConfig::default(),
    );
    let started_at = Instant::now();
    let id = runner.try_submit("hello".into()).unwrap();
    assert!(started_at.elapsed() < Duration::from_millis(10));
    assert!(matches!(
        wait_event(
            &runner,
            |event| matches!(event, RuntimeEvent::Started { id: actual } if *actual == id)
        ),
        RuntimeEvent::Started { .. }
    ));
    let event = wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Completed { id: actual, .. } if *actual == id),
    );
    assert!(matches!(
        event,
        RuntimeEvent::Completed {
            outcome: JobOutcome::Succeeded(5),
            ..
        }
    ));
    runner.try_shutdown().unwrap();
    assert!(matches!(runner.next_event().unwrap(), RuntimeEvent::Closed));
    runner.join().unwrap();
}

#[test]
fn agent_adapter_keeps_session_state_across_background_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let agent = Arc::new(Mutex::new(Agent::with_echo(
        SessionStore::open(directory.path().join("runtime.jsonl")).unwrap(),
    )));
    let worker_agent = Arc::clone(&agent);
    let runner = BackgroundRunner::spawn(
        move |request: String, _token| {
            let mut agent = worker_agent.lock().expect("agent mutex poisoned");
            agent
                .process_sync(request)
                .map(|result| {
                    result
                        .assistant
                        .expect("echo backend always returns an assistant")
                        .content
                })
                .map_err(|error| error.to_string())
        },
        RuntimeConfig::default(),
    );
    let first = runner.try_submit("first".into()).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Started { id } if *id == first),
    );
    assert!(matches!(
        wait_event(&runner, |event| matches!(event, RuntimeEvent::Completed { id, .. } if *id == first)),
        RuntimeEvent::Completed {
            outcome: JobOutcome::Succeeded(ref text),
            ..
        } if text == "first"
    ));
    let second = runner.try_submit("second".into()).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Started { id } if *id == second),
    );
    assert!(matches!(
        wait_event(&runner, |event| matches!(event, RuntimeEvent::Completed { id, .. } if *id == second)),
        RuntimeEvent::Completed {
            outcome: JobOutcome::Succeeded(ref text),
            ..
        } if text == "second"
    ));
    assert_eq!(agent.lock().unwrap().history().len(), 4);
    runner.try_shutdown().unwrap();
    assert!(matches!(runner.next_event().unwrap(), RuntimeEvent::Closed));
    runner.join().unwrap();
}

#[test]
fn pending_follow_up_is_fifo_and_cancellation_is_cooperative() {
    let (first_started_tx, first_started_rx) = mpsc::sync_channel(1);
    let runner = BackgroundRunner::spawn(
        move |request: u8, token| {
            if request == 1 {
                first_started_tx.send(()).unwrap();
                while !token.is_cancelled() {
                    thread::sleep(Duration::from_millis(2));
                }
                Err::<u8, String>("stopped".into())
            } else {
                Ok(request + 10)
            }
        },
        RuntimeConfig::default(),
    );
    let first = runner.try_submit(1).unwrap();
    assert!(matches!(
        wait_event(
            &runner,
            |event| matches!(event, RuntimeEvent::Started { id } if *id == first)
        ),
        RuntimeEvent::Started { .. }
    ));
    first_started_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    let second = runner.try_submit(2).unwrap();
    assert!(matches!(
        wait_event(
            &runner,
            |event| matches!(event, RuntimeEvent::Queued { id, .. } if *id == second)
        ),
        RuntimeEvent::Queued { depth: 1, .. }
    ));
    runner.try_cancel(first).unwrap();
    assert!(matches!(
        wait_event(
            &runner,
            |event| matches!(event, RuntimeEvent::CancelRequested { id } if *id == first)
        ),
        RuntimeEvent::CancelRequested { .. }
    ));
    assert!(matches!(
        wait_event(
            &runner,
            |event| matches!(event, RuntimeEvent::Completed { id, .. } if *id == first)
        ),
        RuntimeEvent::Completed {
            outcome: JobOutcome::Cancelled,
            ..
        }
    ));
    assert!(matches!(
        wait_event(
            &runner,
            |event| matches!(event, RuntimeEvent::Started { id } if *id == second)
        ),
        RuntimeEvent::Started { .. }
    ));
    assert!(matches!(
        wait_event(
            &runner,
            |event| matches!(event, RuntimeEvent::Completed { id, .. } if *id == second)
        ),
        RuntimeEvent::Completed {
            outcome: JobOutcome::Succeeded(12),
            ..
        }
    ));
    runner.try_shutdown().unwrap();
    assert!(matches!(runner.next_event().unwrap(), RuntimeEvent::Closed));
    runner.join().unwrap();
}

#[test]
fn pending_queue_is_bounded_and_rejection_is_explicit() {
    let runner = BackgroundRunner::spawn(
        |_request: u8, token| {
            while !token.is_cancelled() {
                thread::sleep(Duration::from_millis(2));
            }
            Ok::<_, String>(())
        },
        RuntimeConfig {
            max_pending: 1,
            ..RuntimeConfig::default()
        },
    );
    let first = runner.try_submit(1).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Started { id } if *id == first),
    );
    let second = runner.try_submit(2).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Queued { id, .. } if *id == second),
    );
    let third = runner.try_submit(3).unwrap();
    assert!(matches!(
        wait_event(
            &runner,
            |event| matches!(event, RuntimeEvent::Rejected { id, .. } if *id == third)
        ),
        RuntimeEvent::Rejected {
            reason: SubmitError::QueueFull,
            ..
        }
    ));
    runner.try_cancel(first).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Completed { id, .. } if *id == first),
    );
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Started { id } if *id == second),
    );
    runner.try_cancel(second).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Completed { id, .. } if *id == second),
    );
    runner.try_shutdown().unwrap();
    assert!(matches!(runner.next_event().unwrap(), RuntimeEvent::Closed));
    runner.join().unwrap();
}

#[test]
fn shutdown_cancels_active_job_and_closes_after_terminal_result() {
    let runner = BackgroundRunner::spawn(
        |_request: (), token| {
            while !token.is_cancelled() {
                thread::sleep(Duration::from_millis(2));
            }
            Ok::<_, String>(())
        },
        RuntimeConfig::default(),
    );
    let id = runner.try_submit(()).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Started { id: actual } if *actual == id),
    );
    runner.try_shutdown().unwrap();
    assert!(matches!(
        wait_event(
            &runner,
            |event| matches!(event, RuntimeEvent::CancelRequested { id: actual } if *actual == id)
        ),
        RuntimeEvent::CancelRequested { .. }
    ));
    assert!(matches!(
        wait_event(
            &runner,
            |event| matches!(event, RuntimeEvent::Completed { id: actual, .. } if *actual == id)
        ),
        RuntimeEvent::Completed {
            outcome: JobOutcome::Cancelled,
            ..
        }
    ));
    assert!(matches!(runner.next_event().unwrap(), RuntimeEvent::Closed));
    runner.join().unwrap();
}

#[test]
fn shutdown_emits_active_terminal_before_queued_cancellation() {
    let (first_started_tx, first_started_rx) = mpsc::sync_channel(1);
    let runner = BackgroundRunner::spawn(
        move |request: u8, token| {
            if request == 1 {
                first_started_tx.send(()).unwrap();
                while !token.is_cancelled() {
                    thread::yield_now();
                }
            }
            // The queued request must never be started once shutdown has
            // begun; both requests receive explicit terminal events instead.
            Ok::<_, String>(request)
        },
        RuntimeConfig::default(),
    );
    let active = runner.try_submit(1).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Started { id } if *id == active),
    );
    first_started_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap();

    let queued = runner.try_submit(2).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Queued { id, .. } if *id == queued),
    );
    runner.try_shutdown().unwrap();

    let mut events = Vec::new();
    loop {
        let event = runner.next_event().unwrap();
        let closed = matches!(event, RuntimeEvent::Closed);
        events.push(event);
        if closed {
            break;
        }
    }

    let active_completed = events.iter().position(|event| {
        matches!(
            event,
            RuntimeEvent::Completed { id, outcome: JobOutcome::Cancelled } if *id == active
        )
    });
    let queued_cancel_requested = events
        .iter()
        .position(|event| matches!(event, RuntimeEvent::CancelRequested { id } if *id == queued));
    let queued_completed = events.iter().position(|event| {
        matches!(
            event,
            RuntimeEvent::Completed { id, outcome: JobOutcome::Cancelled } if *id == queued
        )
    });
    assert!(
        active_completed.is_some(),
        "active request did not receive a cancelled terminal event: {events:?}"
    );
    assert!(
        queued_cancel_requested.is_some(),
        "queued request did not receive a cancellation event: {events:?}"
    );
    assert!(
        queued_completed.is_some(),
        "queued request did not receive a cancelled terminal event: {events:?}"
    );
    assert!(
        active_completed.unwrap() < queued_cancel_requested.unwrap(),
        "queued cancellation preceded active terminal: {events:?}"
    );
    assert!(
        queued_cancel_requested.unwrap() < queued_completed.unwrap(),
        "queued terminal did not follow its cancellation request: {events:?}"
    );
    runner.join().unwrap();
}

#[test]
fn late_cancellation_after_completion_marker_preserves_success() {
    let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_completed = Arc::clone(&completed);
    let worker_release = Arc::clone(&release);
    let runner = BackgroundRunner::spawn(
        move |_request: (), token| {
            // Model an adapter that has committed its successful result but
            // has not yet returned through the runtime's done channel.
            token.mark_completed();
            worker_completed.store(true, std::sync::atomic::Ordering::Release);
            while !worker_release.load(std::sync::atomic::Ordering::Acquire) {
                thread::yield_now();
            }
            Ok::<_, String>(7_u8)
        },
        RuntimeConfig::default(),
    );
    let id = runner.try_submit(()).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Started { id: actual } if *actual == id),
    );
    while !completed.load(std::sync::atomic::Ordering::Acquire) {
        thread::yield_now();
    }
    runner.try_cancel(id).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::CancelRequested { id: actual } if *actual == id),
    );
    release.store(true, std::sync::atomic::Ordering::Release);
    assert!(matches!(
        wait_event(
            &runner,
            |event| matches!(event, RuntimeEvent::Completed { id: actual, .. } if *actual == id)
        ),
        RuntimeEvent::Completed {
            outcome: JobOutcome::Succeeded(7),
            ..
        }
    ));
    runner.try_shutdown().unwrap();
    assert!(matches!(runner.next_event().unwrap(), RuntimeEvent::Closed));
    runner.join().unwrap();
}

#[test]
fn panicking_job_still_reaches_a_terminal_event_and_runner_survives() {
    let runner = BackgroundRunner::spawn(
        |_request: u8, _token| -> Result<u8, String> { panic!("fixture panic") },
        RuntimeConfig::default(),
    );
    let first = runner.try_submit(1).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Started { id } if *id == first),
    );
    assert!(matches!(
        wait_event(
            &runner,
            |event| matches!(event, RuntimeEvent::Completed { id, .. } if *id == first)
        ),
        RuntimeEvent::Completed {
            outcome: JobOutcome::Panicked,
            ..
        }
    ));
    let second = runner.try_submit(2).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Started { id } if *id == second),
    );
    assert!(matches!(
        wait_event(
            &runner,
            |event| matches!(event, RuntimeEvent::Completed { id, .. } if *id == second)
        ),
        RuntimeEvent::Completed {
            outcome: JobOutcome::Panicked,
            ..
        }
    ));
    runner.try_shutdown().unwrap();
    assert!(matches!(runner.next_event().unwrap(), RuntimeEvent::Closed));
    runner.join().unwrap();
}

#[test]
fn owned_shutdown_joins_the_active_job_before_returning() {
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_finished = Arc::clone(&finished);
    let runner = BackgroundRunner::spawn(
        move |_request: (), token| {
            while !token.is_cancelled() {
                thread::yield_now();
            }
            worker_finished.store(true, std::sync::atomic::Ordering::Release);
            Ok::<_, String>(())
        },
        RuntimeConfig::default(),
    );
    let id = runner.try_submit(()).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Started { id: actual } if *actual == id),
    );
    runner.shutdown_and_join().unwrap();
    assert!(finished.load(std::sync::atomic::Ordering::Acquire));
}

#[test]
fn shutdown_and_join_is_safe_after_closed_event_was_consumed() {
    let runner = BackgroundRunner::spawn(
        |_request: (), _token| Ok::<_, String>(()),
        RuntimeConfig::default(),
    );
    runner.try_shutdown().unwrap();
    assert!(matches!(runner.next_event().unwrap(), RuntimeEvent::Closed));

    // The host is allowed to consume `Closed` while polling its event loop
    // and call the owned cleanup helper afterwards.  The lifecycle marker
    // prevents the helper from waiting for an event that is already gone.
    runner.shutdown_and_join().unwrap();
}

#[test]
fn shutdown_and_join_drains_saturated_command_and_event_queues() {
    let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_started = Arc::clone(&started);
    let runner = BackgroundRunner::spawn(
        move |_request: u8, token| {
            worker_started.store(true, std::sync::atomic::Ordering::Release);
            while !token.is_cancelled() {
                thread::yield_now();
            }
            Ok::<_, String>(())
        },
        RuntimeConfig {
            command_capacity: 1,
            event_capacity: 1,
            max_pending: 4,
            ..RuntimeConfig::default()
        },
    );
    runner.try_submit(1).unwrap();
    for _ in 0..100 {
        if started.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(started.load(std::sync::atomic::Ordering::Acquire));
    // Leave the event queue full and the command queue occupied. Cleanup is
    // run on another thread so this regression test can assert a bound.
    let _ = runner.try_submit(2);
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        runner.shutdown_and_join().unwrap();
        done_tx.send(()).unwrap();
    });
    done_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("saturated runtime did not shut down");
}

#[test]
fn bounded_shutdown_detaches_a_noncooperative_job() {
    let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_started = Arc::clone(&started);
    let worker_release = Arc::clone(&release);
    let runner = BackgroundRunner::spawn(
        move |_request: (), _token| {
            worker_started.store(true, std::sync::atomic::Ordering::Release);
            // Deliberately ignore cancellation. The owner releases this
            // fixture after shutdown has detached it so no test thread is
            // left running beyond the test process.
            while !worker_release.load(std::sync::atomic::Ordering::Acquire) {
                thread::yield_now();
            }
            Ok::<_, String>(())
        },
        RuntimeConfig::default(),
    );
    let id = runner.try_submit(()).unwrap();
    wait_event(
        &runner,
        |event| matches!(event, RuntimeEvent::Started { id: actual } if *actual == id),
    );
    while !started.load(std::sync::atomic::Ordering::Acquire) {
        thread::yield_now();
    }

    let began = Instant::now();
    runner
        .shutdown_and_join_with_grace(Duration::from_millis(25))
        .unwrap();
    assert!(
        began.elapsed() < Duration::from_millis(500),
        "bounded shutdown waited for a non-cooperative job: {:?}",
        began.elapsed()
    );

    // Let the detached fixture finish before the test exits. The runtime has
    // already dropped its result receiver, so this late result is ignored.
    release.store(true, std::sync::atomic::Ordering::Release);
}
