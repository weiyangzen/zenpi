# b3ehive Occam Audit

Status: read-only design audit. Evidence was inspected on 2026-09-03 in
`/Users/mac/Github/b3ehive` at revision
`76cb266a79ef1555a7c974af0b7340333dbd39e8`.
The corresponding source repository is [`weiyangzen/b3ehive`](https://github.com/weiyangzen/b3ehive).

## What b3ehive provides

The execution skill defines immutable claim identity, isolated task roots,
checksum-valid worker handoffs, a Master-only acceptance transition, and a
same-name Gantt projection. The looper skill adds bounded resource envelopes,
leases, side-effect gates, evidence records, route/estimate decisions, and
multi-grain feedback. These observations come from the repository's
`execution-cron-builder/SKILL.md`, `looper-cron-builder/SKILL.md`, and their
bridge-contract references; no remote service was contacted.

## Selected first-class records

| b3 concept | zenpi owner | Why it earns its bytes |
|---|---|---|
| Handoff and result manifest | `src/b3.rs` plus session integration | Lets one standalone process hand a bounded, verifiable summary to another agent through stdio or a file, without a broker |
| Resource budget, envelope, and lease | `src/b3.rs` | Carries host-supplied limits and expiry so an embedding controller can account for a cheap agent; the records never allocate workers |
| Side-effect gate | `src/b3.rs` | Makes publish, protected-path, network/spend, and destructive decisions explicit and fail-closed |
| Evidence record | `src/b3.rs` | Preserves changed paths, commands, validation result, and Master decision in a compact handoff |
| Route decision and estimator policy | `src/b3.rs` | Carries a host's chosen route and bounded estimate without embedding provider or scheduler logic |
| Looper log | `src/b3.rs` | Carries feedback about a route/instrument for a parent controller; it is not an automatic self-modification loop |

Handoff and manifest records are versioned and digest-protected. The compact
accounting/evidence records are bounded, serializable, and validated by one
owner; their host envelope supplies lifecycle/version context. `ParentLeaseRef`
is accepted only as an external attribution reference;
zenpi never creates or hides a nested agent. Handoff artifacts are references,
not arbitrary file content.

## Rejected surfaces

zenpi deliberately does not embed the b3 scheduler/cron daemon, persistent
worker pool, tmux controller, proposal competition, optimization engine, ROI
loop, dashboard, plugin marketplace, remote queue, or an additional RPC/server
protocol. Those surfaces would duplicate the host's execution responsibility,
increase resident CPU/memory, and create a third product mode. They remain
available to an embedding b3ehive host through the records above.

## Acceptance boundaries

The Rust bridge must reject empty or invalid identifiers, line breaks, oversized
text/lists, unsafe artifact paths, digest tampering, expired leases, and
over-budget accounting without mutating prior state. Artifact references must
remain repository-relative; it must prove round-trip
serialization and explicit allow/deny decisions. Evidence command fields are
labels only and are never executed. A malformed handoff is a typed error on
stderr or a typed headless response; it is never silently repaired. The
Blueprint's handoff and b3 tests are the executable evidence for these decisions.

## Design conclusion

The smallest useful integration is data, not orchestration: session records and
handoffs make zenpi composable; inert budget/evidence records make it legible to
b3ehive; the host retains scheduling and admission authority. This split keeps a
single-agent process small and avoids two competing sources of truth.

Related contract: [`../Zenpi_Execution_Spec.md`](../Zenpi_Execution_Spec.md).
