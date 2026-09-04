use std::time::Instant;

use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;
use zenpi::{
    layout::{LayoutModel, TabId},
    runtime::{BackgroundRunner, JobOutcome, RuntimeConfig, RuntimeEvent},
    tui::{MessageRole, RenderScheduler, TuiState},
};

fn main() {
    const LAYOUT_BATCHES: usize = 10_000;
    let started = Instant::now();
    for index in 0..LAYOUT_BATCHES {
        let tab = TabId::ALL[index % TabId::ALL.len()];
        for &(width, height) in &[(0, 0), (1, 1), (79, 24), (80, 24), (120, 40), (160, 50)] {
            let snapshot = LayoutModel::new(tab).compute(width, height);
            assert!(snapshot.visible_rects_non_overlapping());
        }
    }
    let layout_iterations = LAYOUT_BATCHES * 6;
    let layout_us = started.elapsed().as_micros();

    let mut state = TuiState::new(256);
    for index in 0..128 {
        state.push_message(
            MessageRole::Assistant,
            format!("line {index}: wide benchmark text"),
        );
    }
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("test terminal");
    let render_iterations = 500;
    let started = Instant::now();
    for index in 0..render_iterations {
        state.append_stream(MessageRole::Assistant, ".");
        terminal
            .draw(|frame| state.render_bentobox(frame, "zenpi benchmark"))
            .expect("benchmark render");
        terminal
            .backend_mut()
            .resize(if index % 2 == 0 { 120 } else { 79 }, 40);
    }
    let render_us = started.elapsed().as_micros();

    let mut scheduler = RenderScheduler::new(std::time::Duration::from_millis(16));
    let now = Instant::now();
    scheduler.rendered(now);
    for _ in 0..10_000 {
        scheduler.request();
    }
    assert!(!scheduler.due(now));
    assert_eq!(scheduler.frame_count(), 1);

    let queue_iterations = 2_000;
    let runner = BackgroundRunner::spawn(
        |request: usize, _| Ok::<usize, String>(request),
        RuntimeConfig {
            command_capacity: 64,
            event_capacity: 128,
            max_pending: 32,
            poll_interval: std::time::Duration::from_millis(1),
        },
    );
    let started = Instant::now();
    for request in 0..queue_iterations {
        let id = runner.try_submit(request).expect("queue submission");
        loop {
            if let RuntimeEvent::Completed {
                id: completed_id,
                outcome: JobOutcome::Succeeded(value),
            } = runner.next_event().expect("runtime event")
            {
                assert_eq!(completed_id, id);
                assert_eq!(value, request);
                break;
            }
        }
    }
    let queue_us = started.elapsed().as_micros();
    runner.shutdown_and_join().expect("runtime shutdown");

    println!(
        "{}",
        json!({
            "iterations": {
                "layout": layout_iterations,
                "render": render_iterations,
                "queue": queue_iterations,
                "coalesced_dirty_requests": 10_000,
            },
            "elapsed_us": {
                "layout": layout_us,
                "render": render_us,
                "queue": queue_us,
            },
            "per_operation_us": {
                "layout": layout_us as f64 / layout_iterations as f64,
                "render": render_us as f64 / render_iterations as f64,
                "queue_round_trip": queue_us as f64 / queue_iterations as f64,
            },
            "scheduler_frames_after_10000_dirty_requests": scheduler.frame_count(),
        })
    );
}
