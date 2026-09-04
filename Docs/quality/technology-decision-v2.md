# Technology Decision Gates (v2)

The measured baseline is recorded in
[`runtime-budget-v2.md`](runtime-budget-v2.md). The current release passes its
dependency, binary, startup/RSS, queue, render, layout, and coalescing gates, so
none of the following additions is justified by the measured evidence.

| Candidate | Decision now | Re-open only when | Migration boundary |
|---|---|---|---|
| Tokio + reqwest | DEFER | A required test needs socket-granularity cancellation, nonblocking Chat streaming, or concurrent tools that bounded threads cannot satisfy | Replace provider I/O and runtime ownership together; never wrap blocking `ureq` in Tokio |
| clap | DEFER | The typed hand parser/completion metadata can no longer express or test the accepted command grammar without duplication | Replace process argument parsing as one owner; slash grammar remains transport-neutral |
| SQLite | DEFER | A repeatable session-history workload breaches an approved query/memory budget or multi-process indexing becomes required | Add an optional index while JSONL remains the append-only recovery source |
| Second TUI/web stack | REJECT for v2 | A separately approved GUI blueprint consumes the stable core/headless protocol | Separate workspace/crate; no GUI dependency in the TUI/headless binary |

These are decision gates, not permanent bans. A future proposal must attach
before/after `bench_runtime.py` JSON receipts, the failing product acceptance
test, binary/dependency delta, rollback plan, and ownership migration. A faster
microbenchmark alone is insufficient. Conversely, a current gate failure must
first be investigated as a regression; it does not automatically authorize a
new framework or database.

No candidate dependency was added for this benchmark. The probe is a normal
Cargo example compiled from repository code, and the Python runner uses only
the standard library plus platform `time(1)` on Darwin/Linux.
