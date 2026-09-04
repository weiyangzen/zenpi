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
