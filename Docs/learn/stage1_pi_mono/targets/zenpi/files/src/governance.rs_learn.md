# `src/governance.rs` per-file learn report (ZS1-083)

- Source: `src/governance.rs`
- Complete read range: byte `[0,44516)`; 1,256 lines
- Bytes: `44516`
- SHA-256: `4d80ffb9f29bb987e488bad93bc35546a6994065e793bd645fc3d73802930552`

## Resource governance

`ResourceLimits`, `ResourceUsage`, and `ResourceKind` define bounded token, wall time, disk, process, concurrency, and network dimensions. `BudgetLedger` restores the latest durable session event, checks elapsed time at cooperative boundaries, charges with overflow rollback, releases concurrency, persists usage, and supports separately bounded summary-cost reservations. Unknown summary prices fail closed when a monetary cap is configured.

## Worker leases and reservations

`WorkerLease`, `BudgetReservation`, `WorkerOperation`, `BudgetSettlement`, and `BudgetDirective` model the host-owned worker budget boundary. A reservation records operation and lease identities, a `BudgetOrigin` (blueprint worker, agent tool, or user shell, all sharing one ledger so a shell escape cannot obtain a fresh budget), bounded resource upper bounds, policy and gate identities, an optional network host, and opaque credential handles. It is held before side effects and is not refundable; actual settlement is recorded later. `BudgetTerminal` makes cancellation, exhaustion, and lease expiry explicit terminal states, while an operation left unsettled after restart is flagged `unknown_outcome` and blocks new work with `RecoveryRequired` until the host reconciles it.

## Durable snapshot lifecycle

`WorkerBudgetLedger` restores and validates versioned snapshots tied to one session and exact limits, and `restore` enforces monotonic transitions across the journaled history. Each mutation builds a candidate and journals it before replacing live state through `commit`, which re-checks session identity and the bounded snapshot size; the mutators themselves enforce lease and reservation validation, identity, expiry, policy, budget exhaustion, and collection caps (32 leases, 512 operations, 900 KB). Lease renewal extends an active lease by at most 24 hours, and `active_lease`, `require_settled_item`, and `restore_existing` gate host acceptance on a live, fully settled lease. Restart marks unsettled operations as unknown. Cross-process writer exclusion remains the `SessionStore` responsibility.

## Scope and integration boundary

The module supplies governance and accounting invariants to runtime, headless, and worker hosts. It does not spawn workers, execute tools, call providers, render TUI, or own credentials; host layers must perform those side effects only after these durable gates pass.
