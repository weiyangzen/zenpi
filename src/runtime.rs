//! Bounded background execution for interactive hosts.
//!
//! The core [`crate::core::Agent`] deliberately exposes a synchronous
//! backend boundary.  This module is the small orchestration layer that keeps
//! a TUI or headless host responsive while a turn is in flight: requests are
//! admitted through a bounded queue, work runs on a dedicated thread, and a
//! cancellation token is passed to the job implementation.  A job may return
//! an error or observe cancellation at its own boundary; no unsafe thread
//! termination is attempted.
//!
//! The runner is generic so it can be wired to `Agent` without coupling the
//! channel protocol to the UI.  An adapter normally captures an `Arc` of the
//! backend/session coordinator and maps each request to one turn.  Requests
//! submitted while a job is running become pending follow-ups and are
//! started in FIFO order after the active job reaches a terminal result.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// Monotonically increasing identifier assigned to one submitted job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(u64);

impl JobId {
    /// Return the numeric representation for logging or wire protocols.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Cooperative cancellation shared by the host and the background job.
///
/// Calling [`CancellationToken::cancel`] is lock-free and idempotent.  A
/// backend must check [`CancellationToken::is_cancelled`] at suitable points
/// (for example between streamed chunks or before a retry) because Rust does
/// not provide a safe way to kill an arbitrary thread.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Request cancellation.  Returns `true` when this call changed the
    /// token from active to cancelled and `false` if it was already set.
    pub fn cancel(&self) -> bool {
        !self.cancelled.swap(true, Ordering::Release)
    }

    /// Check whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Runtime limits and polling cadence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeConfig {
    /// Number of commands retained by the host-facing channel.
    pub command_capacity: usize,
    /// Number of events retained for a host that is briefly busy rendering.
    pub event_capacity: usize,
    /// Maximum follow-ups retained behind the active job.
    pub max_pending: usize,
    /// How frequently the worker checks its command channel while a job runs.
    pub poll_interval: Duration,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            command_capacity: 32,
            event_capacity: 128,
            max_pending: 32,
            poll_interval: Duration::from_millis(10),
        }
    }
}

impl RuntimeConfig {
    fn normalized(self) -> Self {
        Self {
            command_capacity: self.command_capacity.max(1),
            event_capacity: self.event_capacity.max(1),
            max_pending: self.max_pending.max(1),
            poll_interval: self.poll_interval.max(Duration::from_millis(1)),
        }
    }
}

/// Why a request could not be admitted to the bounded command channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitError {
    /// The host must retry later; no request was admitted.
    QueueFull,
    /// The worker has exited or is shutting down.
    Closed,
}

impl std::fmt::Display for SubmitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::QueueFull => "runtime command queue is full",
            Self::Closed => "runtime worker is closed",
        })
    }
}

impl std::error::Error for SubmitError {}

/// Terminal outcome for one background job.
#[derive(Debug)]
pub enum JobOutcome<O, E> {
    Succeeded(O),
    Failed(E),
    Cancelled,
    /// The job panicked.  The worker remains alive and can drain queued
    /// follow-ups instead of silently leaving a request in Running forever.
    Panicked,
}

/// Events emitted in order by the worker.  `Completed` is the terminal event
/// for a job; `Closed` is the terminal event for the whole runner.
#[derive(Debug)]
pub enum RuntimeEvent<O, E> {
    /// The worker admitted a request.  `queued` is true for a follow-up that
    /// arrived while another job was active.
    Accepted { id: JobId, queued: bool },
    /// A queued request became the active job.
    Started { id: JobId },
    /// A request was retained behind the active job.
    Queued { id: JobId, depth: usize },
    /// Cancellation was requested for the active job.
    CancelRequested { id: JobId },
    /// The active job reached its terminal result.
    Completed {
        id: JobId,
        outcome: JobOutcome<O, E>,
    },
    /// The worker rejected a request after the command itself was admitted.
    Rejected { id: JobId, reason: SubmitError },
    /// The runner has finished shutting down and will emit no more events.
    Closed,
}

enum Command<I> {
    Submit { id: JobId, request: I },
    Cancel { id: JobId },
    Shutdown,
}

struct Active<O, E> {
    id: JobId,
    token: CancellationToken,
    done: Receiver<JobExecution<O, E>>,
    join: JoinHandle<()>,
}

enum JobExecution<O, E> {
    Completed(Result<O, E>),
    Panicked,
}

/// A background runner with bounded command and event channels.
pub struct BackgroundRunner<I, O, E, F>
where
    I: Send + 'static,
    O: Send + 'static,
    E: Send + 'static,
    F: Fn(I, CancellationToken) -> Result<O, E> + Send + Sync + 'static,
{
    command_tx: SyncSender<Command<I>>,
    event_rx: Receiver<RuntimeEvent<O, E>>,
    next_id: AtomicU64,
    join: Option<JoinHandle<()>>,
    _job: Arc<F>,
}

impl<I, O, E, F> std::fmt::Debug for BackgroundRunner<I, O, E, F>
where
    I: Send + 'static,
    O: Send + 'static,
    E: Send + 'static,
    F: Fn(I, CancellationToken) -> Result<O, E> + Send + Sync + 'static,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BackgroundRunner")
            .field("next_id", &self.next_id.load(Ordering::Relaxed))
            .field("running", &self.join.is_some())
            .finish()
    }
}

impl<I, O, E, F> BackgroundRunner<I, O, E, F>
where
    I: Send + 'static,
    O: Send + 'static,
    E: Send + 'static,
    F: Fn(I, CancellationToken) -> Result<O, E> + Send + Sync + 'static,
{
    /// Spawn a worker and return its nonblocking host handle.
    pub fn spawn(job: F, config: RuntimeConfig) -> Self {
        let config = config.normalized();
        let (command_tx, command_rx) = mpsc::sync_channel(config.command_capacity);
        let (event_tx, event_rx) = mpsc::sync_channel(config.event_capacity);
        let job = Arc::new(job);
        let worker_job = Arc::clone(&job);
        let join = thread::Builder::new()
            .name("zenpi-runtime".into())
            .spawn(move || worker_loop(command_rx, event_tx, worker_job, config))
            .expect("failed to spawn zenpi runtime worker");
        Self {
            command_tx,
            event_rx,
            next_id: AtomicU64::new(1),
            join: Some(join),
            _job: job,
        }
    }

    /// Try to admit one request without blocking the caller.
    pub fn try_submit(&self, request: I) -> Result<JobId, SubmitError> {
        let id = JobId(self.next_id.fetch_add(1, Ordering::Relaxed));
        match self.command_tx.try_send(Command::Submit { id, request }) {
            Ok(()) => Ok(id),
            Err(TrySendError::Full(_)) => Err(SubmitError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(SubmitError::Closed),
        }
    }

    /// Try to request cancellation of one active job without blocking.
    pub fn try_cancel(&self, id: JobId) -> Result<(), SubmitError> {
        match self.command_tx.try_send(Command::Cancel { id }) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(SubmitError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(SubmitError::Closed),
        }
    }

    /// Request orderly shutdown without blocking.  The active job receives a
    /// cancellation request and the worker emits `Closed` after it returns.
    pub fn try_shutdown(&self) -> Result<(), SubmitError> {
        match self.command_tx.try_send(Command::Shutdown) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(SubmitError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(SubmitError::Closed),
        }
    }

    /// Wait for the next worker event.
    pub fn next_event(&self) -> Result<RuntimeEvent<O, E>, mpsc::RecvError> {
        self.event_rx.recv()
    }

    /// Wait for the next event up to `timeout`, allowing a UI loop to poll
    /// terminal input and runtime events in the same cadence.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<RuntimeEvent<O, E>, RecvTimeoutError> {
        self.event_rx.recv_timeout(timeout)
    }

    /// Poll for the next worker event without blocking.
    pub fn try_next_event(&self) -> Result<RuntimeEvent<O, E>, TryRecvError> {
        self.event_rx.try_recv()
    }

    /// Join the worker after the host has drained the `Closed` event.
    pub fn join(mut self) -> thread::Result<()> {
        self.join
            .take()
            .expect("runtime worker already joined")
            .join()
    }

    /// Orderly close for owners that cannot conveniently drain the event
    /// stream themselves. The worker owns and joins every job thread before
    /// this returns.
    pub fn shutdown_and_join(mut self) -> thread::Result<()> {
        let _ = self.command_tx.send(Command::Shutdown);
        while !matches!(self.event_rx.recv(), Ok(RuntimeEvent::Closed) | Err(_)) {}
        self.join
            .take()
            .expect("runtime worker already joined")
            .join()
    }
}

impl<I, O, E, F> Drop for BackgroundRunner<I, O, E, F>
where
    I: Send + 'static,
    O: Send + 'static,
    E: Send + 'static,
    F: Fn(I, CancellationToken) -> Result<O, E> + Send + Sync + 'static,
{
    fn drop(&mut self) {
        // Drop cannot block because the caller may hold resources required by
        // its job closure. Production owners use `shutdown_and_join`; this
        // fallback still requests cancellation rather than silently leaking
        // more work.
        let _ = self.command_tx.try_send(Command::Shutdown);
        let _ = self.join.take();
    }
}

fn worker_loop<I, O, E, F>(
    command_rx: Receiver<Command<I>>,
    event_tx: SyncSender<RuntimeEvent<O, E>>,
    job: Arc<F>,
    config: RuntimeConfig,
) where
    I: Send + 'static,
    O: Send + 'static,
    E: Send + 'static,
    F: Fn(I, CancellationToken) -> Result<O, E> + Send + Sync + 'static,
{
    let mut active: Option<Active<O, E>> = None;
    let mut pending = VecDeque::new();
    let mut stopping = false;

    loop {
        if let Some(done) = active.as_ref().and_then(|item| item.done.try_recv().ok()) {
            let item = active.take().expect("active job disappeared");
            let _ = item.join.join();
            let outcome = if item.token.is_cancelled() {
                JobOutcome::Cancelled
            } else {
                match done {
                    JobExecution::Completed(Ok(output)) => JobOutcome::Succeeded(output),
                    JobExecution::Completed(Err(error)) => JobOutcome::Failed(error),
                    JobExecution::Panicked => JobOutcome::Panicked,
                }
            };
            if !emit(
                &event_tx,
                RuntimeEvent::Completed {
                    id: item.id,
                    outcome,
                },
            ) {
                return;
            }
            if stopping {
                let _ = emit(&event_tx, RuntimeEvent::Closed);
                return;
            }
            if let Some((id, request)) = pending.pop_front()
                && !start_job(&event_tx, &job, &mut active, id, request)
            {
                return;
            }
            continue;
        }

        let command = if active.is_some() {
            match command_rx.recv_timeout(config.poll_interval) {
                Ok(command) => command,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => Command::Shutdown,
            }
        } else {
            match command_rx.recv() {
                Ok(command) => command,
                Err(_) => Command::Shutdown,
            }
        };

        match command {
            Command::Submit { id, request } if active.is_none() && !stopping => {
                if !emit(&event_tx, RuntimeEvent::Accepted { id, queued: false }) {
                    return;
                }
                if !start_job(&event_tx, &job, &mut active, id, request) {
                    return;
                }
            }
            Command::Submit { id, request } if !stopping => {
                if pending.len() >= config.max_pending {
                    if !emit(
                        &event_tx,
                        RuntimeEvent::Rejected {
                            id,
                            reason: SubmitError::QueueFull,
                        },
                    ) {
                        return;
                    }
                } else {
                    pending.push_back((id, request));
                    if !emit(&event_tx, RuntimeEvent::Accepted { id, queued: true })
                        || !emit(
                            &event_tx,
                            RuntimeEvent::Queued {
                                id,
                                depth: pending.len(),
                            },
                        )
                    {
                        return;
                    }
                }
            }
            Command::Submit { id, .. } => {
                if !emit(
                    &event_tx,
                    RuntimeEvent::Rejected {
                        id,
                        reason: SubmitError::Closed,
                    },
                ) {
                    return;
                }
            }
            Command::Cancel { id } => {
                if active.as_ref().is_some_and(|item| item.id == id) {
                    let item = active.as_ref().expect("active job disappeared");
                    item.token.cancel();
                    if !emit(&event_tx, RuntimeEvent::CancelRequested { id }) {
                        return;
                    }
                } else if let Some(index) =
                    pending.iter().position(|(pending_id, _)| *pending_id == id)
                {
                    // A queued follow-up has no worker thread yet, but it
                    // still gets a terminal event so hosts never have to
                    // guess whether the request was dropped.
                    pending.remove(index);
                    if !emit(&event_tx, RuntimeEvent::CancelRequested { id })
                        || !emit(
                            &event_tx,
                            RuntimeEvent::Completed {
                                id,
                                outcome: JobOutcome::Cancelled,
                            },
                        )
                    {
                        return;
                    }
                }
            }
            Command::Shutdown => {
                stopping = true;
                while let Some((id, _)) = pending.pop_front() {
                    if !emit(&event_tx, RuntimeEvent::CancelRequested { id })
                        || !emit(
                            &event_tx,
                            RuntimeEvent::Completed {
                                id,
                                outcome: JobOutcome::Cancelled,
                            },
                        )
                    {
                        return;
                    }
                }
                if let Some(item) = active.as_ref() {
                    item.token.cancel();
                    if !emit(&event_tx, RuntimeEvent::CancelRequested { id: item.id }) {
                        return;
                    }
                } else {
                    let _ = emit(&event_tx, RuntimeEvent::Closed);
                    return;
                }
            }
        }
    }
}

fn start_job<I, O, E, F>(
    event_tx: &SyncSender<RuntimeEvent<O, E>>,
    job: &Arc<F>,
    active: &mut Option<Active<O, E>>,
    id: JobId,
    request: I,
) -> bool
where
    I: Send + 'static,
    O: Send + 'static,
    E: Send + 'static,
    F: Fn(I, CancellationToken) -> Result<O, E> + Send + Sync + 'static,
{
    let token = CancellationToken::new();
    let child_token = token.clone();
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let job = Arc::clone(job);
    let join = match thread::Builder::new()
        .name(format!("zenpi-job-{}", id.get()))
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                job(request, child_token)
            }));
            let execution = match result {
                Ok(result) => JobExecution::Completed(result),
                Err(_) => JobExecution::Panicked,
            };
            let _ = done_tx.send(execution);
        }) {
        Ok(join) => join,
        Err(_) => {
            return emit(
                event_tx,
                RuntimeEvent::Rejected {
                    id,
                    reason: SubmitError::Closed,
                },
            );
        }
    };
    *active = Some(Active {
        id,
        token,
        done: done_rx,
        join,
    });
    emit(event_tx, RuntimeEvent::Started { id })
}

fn emit<O, E>(event_tx: &SyncSender<RuntimeEvent<O, E>>, event: RuntimeEvent<O, E>) -> bool {
    event_tx.send(event).is_ok()
}
