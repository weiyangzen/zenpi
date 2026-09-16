# `src/governance.rs` per-file learn report (ZS1-083)

- Source: `src/governance.rs`
- Complete read range: byte `[0,44516)`; 1,132 lines
- Bytes: `44516`
- SHA-256: `4d80ffb9f29bb987e488bad93bc35546a6994065e793bd645fc3d73802930552`

## Resource governance

`ResourceLimits`, `ResourceUsage`, and `ResourceKind` define bounded token, wall time, disk, process, concurrency, and network dimensions. `BudgetLedger` restores the latest durable session event, checks elapsed time at cooperative boundaries, charges with overflow rollback, releases concurrency, persists usage, and supports separately bounded summary-cost reservations. Unknown summary prices fail closed when a monetary cap is configured.

## Worker leases and reservations

`WorkerLease`, `BudgetReservation`, `WorkerOperation`, `BudgetSettlement`, and `BudgetDirective` model the host-owned worker budget boundary. A reservation records only bounded resource upper bounds, policy and gate identities, network host, and opaque credential handles. It is held before side effects; actual settlement is recorded later, and cancellation, exhaustion, lease expiry, and unknown outcomes remain explicit terminal states.

## Durable snapshot lifecycle

`WorkerBudgetLedger` restores and validates versioned snapshots tied to one session and exact limits. Mutations validate candidate snapshots and monotonic transitions before journal-and-replace commits. Restart marks unsettled operations as unknown, while lease renewal, revoke, reservation, settle, cancel, and terminalization enforce identity, expiry, limits, and bounded collection sizes. Cross-process writer exclusion remains the `SessionStore` responsibility.

## Scope and integration boundary

The module supplies governance and accounting invariants to runtime, headless, and worker hosts. It does not spawn workers, execute tools, call providers, render TUI, or own credentials; host layers must perform those side effects only after these durable gates pass.
