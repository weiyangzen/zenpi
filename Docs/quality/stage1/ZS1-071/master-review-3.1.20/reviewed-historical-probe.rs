use std::{
    sync::{Mutex, mpsc},
    thread,
    time::Duration,
};
use zenpi::runtime::{
    BackgroundRunner, InputBoundaryGate, JobOutcome, RuntimeConfig, RuntimeEvent,
};

#[test]
fn event_backpressure_delays_cancel_and_transport_admitted_tail_can_have_no_job_event() {
    let (tokens, token_rx) = mpsc::sync_channel(1);
    let runner = BackgroundRunner::spawn(
        move |_: (), token| {
            tokens.send(token.clone()).unwrap();
            while !token.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            Ok::<_, String>(())
        },
        RuntimeConfig {
            command_capacity: 4,
            event_capacity: 1,
            ..Default::default()
        },
    );
    let active = runner.try_submit(()).unwrap();
    let token = token_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    // Accepted occupies the only event slot, so Started blocks the worker.
    runner.try_cancel(active).unwrap();
    runner.try_shutdown_with_grace(Duration::ZERO).unwrap();
    let tail = runner.try_submit(()).unwrap();
    thread::sleep(Duration::from_millis(25));
    assert!(
        !token.is_cancelled(),
        "cancel command is admitted but cannot yet be processed"
    );
    let mut events = Vec::new();
    loop {
        let event = runner.recv_timeout(Duration::from_secs(2)).unwrap();
        let closed = matches!(event, RuntimeEvent::Closed);
        assert!(
            !matches!(&event, RuntimeEvent::Accepted{id,..} | RuntimeEvent::Started{id} | RuntimeEvent::Queued{id,..} | RuntimeEvent::Rejected{id,..} | RuntimeEvent::Completed{id,..} | RuntimeEvent::CancelRequested{id} if *id == tail)
        );
        events.push(format!("{event:?}"));
        if closed {
            break;
        }
    }
    assert!(token.is_cancelled());
    runner.join().unwrap();
    println!(
        "OBS blocked event delivery delays cancellation; tail {} was transport-admitted but got no job event; events={events:?}",
        tail.get()
    );
}

#[test]
fn completion_marker_can_override_a_cancellation_already_observed_by_the_job() {
    let (ready, ready_rx) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let released = Mutex::new(released);
    let runner = BackgroundRunner::spawn(
        move |_: (), token| {
            ready.send(()).unwrap();
            released
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            assert!(token.is_cancelled());
            token.mark_completed();
            assert!(!token.is_cancelled());
            Ok::<_, String>(7)
        },
        RuntimeConfig::default(),
    );
    let id = runner.try_submit(()).unwrap();
    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    runner.try_cancel(id).unwrap();
    loop {
        if matches!(
            runner.recv_timeout(Duration::from_secs(2)).unwrap(),
            RuntimeEvent::CancelRequested { .. }
        ) {
            break;
        }
    }
    release.send(()).unwrap();
    loop {
        if let RuntimeEvent::Completed { outcome, .. } =
            runner.recv_timeout(Duration::from_secs(2)).unwrap()
        {
            assert!(matches!(outcome, JobOutcome::Succeeded(7)));
            break;
        }
    }
    runner.shutdown_and_join().unwrap();
    println!(
        "OBS earlier cancellation=true then mark_completed => cancellation=false, Succeeded(7)"
    );
}

#[test]
fn zero_max_pending_normalizes_to_one_instead_of_disabling_queueing() {
    let runner = BackgroundRunner::spawn(
        |_: u8, token| {
            while !token.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            Ok::<_, String>(())
        },
        RuntimeConfig {
            max_pending: 0,
            poll_interval: Duration::ZERO,
            ..Default::default()
        },
    );
    let _active = runner.try_submit(1).unwrap();
    loop {
        if matches!(
            runner.recv_timeout(Duration::from_secs(2)).unwrap(),
            RuntimeEvent::Started { .. }
        ) {
            break;
        }
    }
    let queued = runner.try_submit(2).unwrap();
    loop {
        if let RuntimeEvent::Queued { id, depth } =
            runner.recv_timeout(Duration::from_secs(2)).unwrap()
        {
            assert_eq!(id, queued);
            assert_eq!(depth, 1);
            break;
        }
    }
    let rejected = runner.try_submit(3).unwrap();
    loop {
        if let RuntimeEvent::Rejected { id, reason } =
            runner.recv_timeout(Duration::from_secs(2)).unwrap()
        {
            assert_eq!(id, rejected);
            assert_eq!(reason, zenpi::runtime::SubmitError::QueueFull);
            break;
        }
    }
    runner.shutdown_and_join().unwrap();
    println!("OBS max_pending=0 still retains one pending job; next job explicitly rejected");
}

#[test]
fn gate_builder_and_drop_do_not_revoke_a_previously_issued_boundary() {
    use zenpi::{
        input_queue::{InputKind, InputQueue, InputQueueLimits},
        protocol::InputQueueAction,
        session::SessionStore,
    };
    let dir = tempfile::tempdir().unwrap();
    let mut session = SessionStore::open(dir.path().join("journal")).unwrap();
    let mut queue = InputQueue::recover(&session, InputQueueLimits::default()).unwrap();
    queue
        .execute(
            &mut session,
            InputQueueAction::Enqueue {
                input_id: "i".into(),
                kind: InputKind::Steer,
                text: "owned queued text".into(),
            },
        )
        .unwrap();
    let gate = InputBoundaryGate::new("model").unwrap();
    let old = gate.boundary(false).unwrap();
    let mut gate = gate.with_context_parent("new-parent").unwrap();
    assert_eq!(old.context_parent(), None);
    assert_eq!(
        gate.boundary(false).unwrap().context_parent(),
        Some("new-parent")
    );
    assert!(
        gate.begin_tool_batch(&["duplicate".into(), "duplicate".into()])
            .is_err()
    );
    drop(gate);
    let applied = queue.apply_boundary(&mut session, &old).unwrap();
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].applied_parent_id, None);
    println!(
        "OBS builder parent change, invalid batch and gate Drop leave earlier boundary usable with original parent=None"
    );
}
