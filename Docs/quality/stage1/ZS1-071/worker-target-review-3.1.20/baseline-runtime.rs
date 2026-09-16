//! Bounded background execution for interactive hosts.
//!
//! The core [`crate::core::Agent`] deliberately exposes a synchronous
//! backend boundary.  This module is the small orchestration layer that keeps
//! a TUI or headless host responsive while a turn is in flight: requests are
//! admitted through a bounded queue, work runs on a dedicated thread, and a
//! cancellation token is passed to the job implementation.  A job may return
//! an error or observe cancellation at its own boundary; no unsafe thread
//! termination is attempted. During shutdown, a job that does not cooperate
//! is detached after a bounded grace period so terminal hosts are not held
//! open forever. Detaching stops the runtime from accepting that job's result;
//! it cannot roll back side effects or forcibly stop arbitrary Rust code.
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
    time::{Duration, Instant},
};

/// Grace period used by the ordinary shutdown helpers before a
/// non-cooperative job is detached.
pub const DEFAULT_SHUTDOWN_GRACE: Duration = Duration::from_secs(1);

/// Upper bound accepted by configurable shutdown helpers.
pub const MAX_SHUTDOWN_GRACE: Duration = Duration::from_secs(30);

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
    /// Set by a job immediately before it returns a successful result.  A
    /// host can race a late shutdown/cancel request with that final return;
    /// once completion is published, that request must not rewrite a
    /// successful result as `JobOutcome::Cancelled`.
    completed: Arc<AtomicBool>,
}

impl CancellationToken {
    fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            completed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Request cancellation.  Returns `true` when this call changed the
    /// token from active to cancelled and `false` if it was already set.
    pub fn cancel(&self) -> bool {
        // A completion marker wins over a late cancel request.  The job has
        // already crossed its semantic terminal boundary, so changing the
        // token here would make the worker misreport a successful result.
        if self.completed.load(Ordering::Acquire) {
            return false;
        }
        !self.cancelled.swap(true, Ordering::Release)
    }

    /// Check whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire) && !self.completed.load(Ordering::Acquire)
    }

    /// Publish that the job has reached its semantic completion boundary.
    ///
    /// This is intentionally opt-in: generic runtime jobs that do not call
    /// it retain the historical cooperative-cancellation behavior, while
    /// adapters that can distinguish a late cancel from an in-flight cancel
    /// can preserve their successful terminal result.
    pub fn mark_completed(&self) {
        self.completed.store(true, Ordering::Release);
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
    /// The runtime stopped accepting this job's result after cancellation.
    /// A cooperative job has returned before this is emitted. During bounded
    /// shutdown, arbitrary non-cooperative code may instead be detached and
    /// can briefly outlive the runtime; cancellation is not a rollback.
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
    Shutdown { grace: Duration },
}

struct Active<O, E> {
    id: JobId,
    token: CancellationToken,
    done: Receiver<JobExecution<O, E>>,
    join: JoinHandle<()>,
}

#[derive(Debug, Clone, Copy)]
struct ShutdownState {
    deadline: Instant,
}

impl ShutdownState {
    fn new(grace: Duration) -> Self {
        Self {
            deadline: Instant::now() + grace,
        }
    }

    fn remaining(self, poll_interval: Duration) -> Duration {
        self.deadline
            .saturating_duration_since(Instant::now())
            .min(poll_interval)
    }

    fn expired(self) -> bool {
        Instant::now() >= self.deadline
    }
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
    /// Set by the worker on every exit path, including an early exit caused
    /// by a dropped event receiver.  Keeping this bit outside the event
    /// channel lets an owner safely join after it already consumed `Closed`
    /// (or after the worker failed before it could emit that event).
    closed: Arc<AtomicBool>,
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
        let closed = Arc::new(AtomicBool::new(false));
        let worker_closed = Arc::clone(&closed);
        let job = Arc::new(job);
        let worker_job = Arc::clone(&job);
        let join = thread::Builder::new()
            .name("zenpi-runtime".into())
            .spawn(move || {
                // The guard covers every return and panic in `worker_loop`,
                // so shutdown helpers never wait forever for a consumed or
                // unavailable `Closed` event.
                let _closed_guard = WorkerClosedGuard(worker_closed);
                worker_loop(command_rx, event_tx, worker_job, config)
            })
            .expect("failed to spawn zenpi runtime worker");
        Self {
            command_tx,
            event_rx,
            next_id: AtomicU64::new(1),
            join: Some(join),
            closed,
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
    /// cancellation request. The worker waits up to
    /// [`DEFAULT_SHUTDOWN_GRACE`] before detaching a non-cooperative job and
    /// emitting its terminal event followed by `Closed`.
    pub fn try_shutdown(&self) -> Result<(), SubmitError> {
        self.try_shutdown_with_grace(DEFAULT_SHUTDOWN_GRACE)
    }

    /// Request orderly shutdown with a caller-selected grace period.
    ///
    /// The request itself is nonblocking. `grace` is clamped to
    /// [`MAX_SHUTDOWN_GRACE`]; a zero duration requests immediate detach after
    /// the cancellation token is set. This bound applies to the runtime's
    /// worker ownership, not to arbitrary side effects already in progress in
    /// detached job code.
    pub fn try_shutdown_with_grace(&self, grace: Duration) -> Result<(), SubmitError> {
        match self.command_tx.try_send(Command::Shutdown {
            grace: normalize_shutdown_grace(grace),
        }) {
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
    /// stream themselves. Cooperative jobs are joined before this returns;
    /// non-cooperative jobs are detached after the default grace period.
    pub fn shutdown_and_join(self) -> thread::Result<()> {
        self.shutdown_and_join_with_grace(DEFAULT_SHUTDOWN_GRACE)
    }

    /// Orderly close with a bounded grace period for the active job.
    ///
    /// The runtime worker and every cooperative job thread are joined. A job
    /// that does not return within `grace` is detached: this call then returns
    /// after the runtime worker has emitted terminal state and exited, while
    /// the detached job may briefly continue executing. The duration is
    /// clamped to [`MAX_SHUTDOWN_GRACE`].
    pub fn shutdown_and_join_with_grace(mut self, grace: Duration) -> thread::Result<()> {
        let grace = normalize_shutdown_grace(grace);
        // `send` can deadlock here: a full command queue can only be drained
        // by a worker that may itself be blocked sending into a full event
        // queue.  Keep both channels moving while retrying the shutdown
        // command.  The worker's closed bit also handles the case where the
        // caller already consumed `Closed` before invoking this helper.
        while !self.closed.load(Ordering::Acquire) {
            match self.command_tx.try_send(Command::Shutdown { grace }) {
                Ok(()) => break,
                Err(TrySendError::Disconnected(_)) => break,
                Err(TrySendError::Full(_)) => {
                    match self.event_rx.recv_timeout(Duration::from_millis(10)) {
                        Ok(RuntimeEvent::Closed) => break,
                        Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            }
        }
        while !self.closed.load(Ordering::Acquire) {
            match self.event_rx.recv_timeout(Duration::from_millis(10)) {
                Ok(RuntimeEvent::Closed) => break,
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
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
        let _ = self.command_tx.try_send(Command::Shutdown {
            grace: Duration::ZERO,
        });
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
    let mut stopping: Option<ShutdownState> = None;

    loop {
        match active.as_ref().map(|item| item.done.try_recv()) {
            Some(Ok(done)) => {
                let item = active.take().expect("active job disappeared");
                let _ = item.join.join();
                let outcome = classify_execution(&item.token, done);
                if !finish_active(
                    &event_tx,
                    item.id,
                    outcome,
                    &mut pending,
                    stopping.is_some(),
                ) {
                    return;
                }
                if stopping.is_some() {
                    return;
                }
                if let Some((id, request)) = pending.pop_front()
                    && !start_job(&event_tx, &job, &mut active, id, request)
                {
                    return;
                }
                continue;
            }
            // A job thread exiting without delivering its result is still a
            // terminal condition. Treat it as a panic rather than polling a
            // permanently disconnected `done` receiver forever.
            Some(Err(TryRecvError::Disconnected)) => {
                let item = active.take().expect("active job disappeared");
                let _ = item.join.join();
                if !finish_active(
                    &event_tx,
                    item.id,
                    JobOutcome::Panicked,
                    &mut pending,
                    stopping.is_some(),
                ) {
                    return;
                }
                if stopping.is_some() {
                    return;
                }
                if let Some((id, request)) = pending.pop_front()
                    && !start_job(&event_tx, &job, &mut active, id, request)
                {
                    return;
                }
                continue;
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }

        if stopping.is_some_and(ShutdownState::expired) && active.is_some() {
            // Dropping JoinHandle detaches only the job thread. Its one-shot
            // done channel is disconnected when the receiver below is
            // dropped, so a late result can never re-enter this runtime.
            let item = active.take().expect("active job disappeared");
            let id = item.id;
            drop(item);
            if !finish_active(&event_tx, id, JobOutcome::Cancelled, &mut pending, true) {
                return;
            }
            return;
        }

        let command = if active.is_some() {
            let wait = stopping
                .map(|state| state.remaining(config.poll_interval))
                .unwrap_or(config.poll_interval);
            match command_rx.recv_timeout(wait) {
                Ok(command) => command,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => Command::Shutdown {
                    grace: DEFAULT_SHUTDOWN_GRACE,
                },
            }
        } else {
            match command_rx.recv() {
                Ok(command) => command,
                Err(_) => Command::Shutdown {
                    grace: DEFAULT_SHUTDOWN_GRACE,
                },
            }
        };

        match command {
            Command::Submit { id, request } if active.is_none() && stopping.is_none() => {
                if !emit(&event_tx, RuntimeEvent::Accepted { id, queued: false }) {
                    return;
                }
                if !start_job(&event_tx, &job, &mut active, id, request) {
                    return;
                }
            }
            Command::Submit { id, request } if stopping.is_none() => {
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
            Command::Shutdown { grace } => {
                let candidate = ShutdownState::new(grace);
                stopping = Some(match stopping {
                    // Repeated shutdown requests may shorten, but never
                    // extend, the owner's already established deadline.
                    Some(existing) if existing.deadline <= candidate.deadline => existing,
                    _ => candidate,
                });
                if let Some(item) = active.as_ref() {
                    item.token.cancel();
                    if !emit(&event_tx, RuntimeEvent::CancelRequested { id: item.id }) {
                        return;
                    }
                } else {
                    cancel_pending(&event_tx, &mut pending);
                    let _ = emit(&event_tx, RuntimeEvent::Closed);
                    return;
                }
            }
        }
    }
}

fn classify_execution<O, E>(
    token: &CancellationToken,
    done: JobExecution<O, E>,
) -> JobOutcome<O, E> {
    if token.is_cancelled() {
        JobOutcome::Cancelled
    } else {
        match done {
            JobExecution::Completed(Ok(output)) => JobOutcome::Succeeded(output),
            JobExecution::Completed(Err(error)) => JobOutcome::Failed(error),
            JobExecution::Panicked => JobOutcome::Panicked,
        }
    }
}

fn finish_active<I, O, E>(
    event_tx: &SyncSender<RuntimeEvent<O, E>>,
    id: JobId,
    outcome: JobOutcome<O, E>,
    pending: &mut VecDeque<(JobId, I)>,
    stopping: bool,
) -> bool {
    if !emit(event_tx, RuntimeEvent::Completed { id, outcome }) {
        return false;
    }
    if stopping {
        // Preserve submission order during shutdown. The active job owns the
        // oldest request, so its terminal event precedes queued cancellation.
        if !cancel_pending(event_tx, pending) {
            return false;
        }
        return emit(event_tx, RuntimeEvent::Closed);
    }
    true
}

fn cancel_pending<I, O, E>(
    event_tx: &SyncSender<RuntimeEvent<O, E>>,
    pending: &mut VecDeque<(JobId, I)>,
) -> bool {
    while let Some((id, _)) = pending.pop_front() {
        if !emit(event_tx, RuntimeEvent::CancelRequested { id })
            || !emit(
                event_tx,
                RuntimeEvent::Completed {
                    id,
                    outcome: JobOutcome::Cancelled,
                },
            )
        {
            return false;
        }
    }
    true
}

fn normalize_shutdown_grace(grace: Duration) -> Duration {
    grace.min(MAX_SHUTDOWN_GRACE)
}

/// RAII marker for the worker lifecycle.  This is intentionally separate
/// from the event stream: event delivery is bounded and may be interrupted
/// by a host that exits early, while joining must still have an authoritative
/// completion signal.
struct WorkerClosedGuard(Arc<AtomicBool>);

impl Drop for WorkerClosedGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
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
