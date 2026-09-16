# Account Scheduler Plugin Research

Date: 2026-09-13. Status: research-only; no dependency or runtime acceptance.
Local baseline: `be15ebd`. Public repository heads were resolved through GitHub's
API. Selected pinned sources were inspected; this is not an upstream code audit.
Product contracts belong in the separate discussion draft, without external
project names as normative dependencies.

## 1. Local Extension Boundary

`src/extensions.rs` currently defines executable tool extensions. Manifest API v1
requires 1..64 tools, an executable, and declared permissions. Invocation starts
a child process and handles its output and completion. These permission fields
are not evidence of an OS filesystem/network sandbox.

Reuse catalogue/path-validation conventions, not the model-tool invocation lane.
A scheduler plugin must not register a dummy model tool to satisfy this schema.
`src/governance.rs`, `src/runtime.rs`, and `src/session.rs` remain the quota,
execution, and session owners. `Cargo.toml` contains no Wasm runtime today.

## 2. Candidate Algorithm References

### Pingora: Candidate Selection Boundary

Head: `4487f7b2ab50f159e4a2cf4f6a6b813f61bb6e19`.
[Selection source](https://github.com/cloudflare/pingora/blob/4487f7b2ab50f159e4a2cf4f6a6b813f61bb6e19/pingora-load-balancing/src/selection/mod.rs).

The Rust API separates selector construction from key-based candidate iteration.
Aliases expose weighted random, round-robin, and consistent Ketama hashing.
Bounded unique iteration prevents repeated candidates from causing endless search.
The inspected module still marks least-connection selection as TODO; do not claim
it supplies that algorithm. This is an interface/affinity reference, not an account
quota manager or a binary plugin ABI. Existing durable session bindings should
take precedence over recomputing a hash. Importing an entire proxy is unnecessary.

### LiteLLM: Usage And In-Flight Signals

Head: `30f33a949b8a2bb890a2baee18e2ab7ab015a4f7`.
[Usage-based implementation](https://github.com/BerriAI/litellm/blob/30f33a949b8a2bb890a2baee18e2ab7ab015a4f7/litellm/router_strategy/lowest_tpm_rpm_v2.py),
[least-busy source](https://github.com/BerriAI/litellm/blob/main/litellm/router_strategy/least_busy.py).

The inspected usage implementation compares deployment TPM and RPM, checks usage
before calls, and uses shared cache operations. Some cache-error paths allow a
call to continue; this must not become local hard-budget behavior. Least-busy uses
in-flight counts and supports shared-cache observations with local fallback.
Adapt the separation of load and usage signals. Do not transplant Python/Redis
infrastructure, deployment IDs as billing identities, or soft-observation fallback
as hard quota authority. Multiple API keys may share one actual billing scope.

### llm-d: Cache-Aware Plugin Composition

Head of architecture repository: `965a5080b354187c9f466fbabe4777954348b3f7`.
[Scheduling architecture](https://github.com/llm-d/llm-d/blob/965a5080b354187c9f466fbabe4777954348b3f7/docs/architecture/core/router/epp/scheduling.md),
[router repository](https://github.com/llm-d/llm-d-router).

The architecture separates filters, weighted scorers, and pickers. Documented
signals include session affinity, prefix matching, queue depth, and latency.
Precise cache locality requires serving-engine events; approximate history is a
different signal. Adopt explicit signal quality and a cache-versus-load tradeoff.
Do not assume commercial account endpoints expose engine KV state. Kubernetes,
prefill/decode disaggregation, and multiple scheduler profiles are unnecessary
for this local account selector. One public selection interface is sufficient.

### Envoy: Least Request And Power Of Two Choices

Head: `93f68227c9f666f884cece45c27e0e0f8032111a`.
[Algorithm source](https://github.com/envoyproxy/envoy/blob/93f68227c9f666f884cece45c27e0e0f8032111a/source/extensions/load_balancing_policies/least_request/least_request_lb.cc),
[official algorithm documentation](https://www.envoyproxy.io/docs/envoy/latest/intro/arch_overview/upstream/load_balancing/load_balancers).

Source exposes full-scan and N-choice selection, and weight adjustment using
active request load. The documentation distinguishes equal-weight P2C from its
unequal-weight scheduling path; these are not one interchangeable algorithm.
For a small local account pool, normalized full-scan load is the simpler baseline.
P2C is a future large-pool alternative, not proof that whole-path scheduling is
O(1): candidate filtering and quota admission still cost work. Request counts do
not measure heterogeneous token work or enforce subscription concurrency.

## 3. Installable Plugin Execution

Candidate: [wasmi](https://github.com/wasmi-labs/wasmi), observed head
`2970aa871cc1001b57b267ccecdcd1e42306199e`.
[Config API](https://docs.rs/wasmi/latest/wasmi/struct.Config.html),
[memory limits API](https://docs.rs/wasmi/latest/wasmi/struct.StoreLimitsBuilder.html).

The inspected API pages identify version 2.0.0. Fuel accounting can trap execution;
it is disabled by default. Module parsing/compilation limits and start-function
control are exposed. Linear-memory limits apply per memory, so a host must also
bound memory count, tables, stack, and instances. Fuel is not a wall-clock SLA.
These APIs make a no-import, bounded interpreter a plausible candidate. This is
not a verified dependency choice: Rust 1.88 compatibility, available release,
security review, binary size, RSS, and latency remain implementation gates.

## 4. Design Decision And Non-Adoption

Recommendation: one strategy contract, built-in trusted Rust implementations, and
an optional installable Wasm host. No Rust trait objects across native shared
library boundaries, no new routing daemon, no provider credentials in plugins.
Both paths feed the same host validation and atomic admission. Built-ins are
trusted application code, not magically sandboxed by their trait signature.

Start with durable affinity and normalized least-load selection. Keep consistent
hashing, P2C, and cache/load/cost scoring as named alternatives under the same
contract. Avoid a plugin for every filtering or bookkeeping step. Stateful queue
fairness is a separate responsibility, not something an account selector can
claim to solve merely by returning account rankings.

No upstream code was copied, no repository was installed, and no package license
or runtime performance acceptance was completed. Review exact file/package
licenses before implementation reuse. No runtime tests or benchmarks were run.

## 5. Specification Revision Standard

The user supplied this [engineering report](https://github.com/weiyangzen/codex-rust-deslop/blob/648ca87e2add56a12081221c3e6f51e29fe04fbd/codex-0.148-rust-refactor-report.zh-CN.md)
as a specification quality bar. It was fetched through GitHub's API after the web
page fetch failed. Its relevant requirements are explicit domain ownership,
typed admission results, stated effect timing, compatibility at persistence
boundaries, cancellation cleanup, and controlled race tests. These become
concrete local contracts rather than external implementation dependencies.

Additional local evidence: `WorkerBudgetLedger` journals candidate state before
publishing it and is currently session-scoped, not a shared account quota ledger.
`Backend::complete_with_control` is the current canonical model-call boundary.
Release builds use `panic = "abort"`; native strategy panic recovery therefore
cannot be promised through `catch_unwind`. The revised specification must state
shared-account persistence and partial-commit recovery explicitly, not merely
claim atomicity across unrelated session files.

## 6. Lightweight Multi-Process Revision

The requirement now includes 5000 concurrent CLI clients and shared outbound
connections. It supersedes the earlier process-local/no-service deployment
decision, not the pure strategy contract. A database shares records, not live
HTTP sockets. A per-process thread pool cannot own connections for other processes.

Local source: OpenAiCompatibleBackend owns a ureq::Agent and constructs it with the
backend. The [ureq Agent documentation](https://docs.rs/ureq/latest/ureq/struct.Agent.html)
states clones share an underlying connection pool within a process. The concrete
change is to construct these clients in one shared broker, not once per CLI.

[Mio documentation](https://docs.rs/mio/latest/mio/) describes readiness-based,
nonblocking I/O over OS event facilities. A small ingress reactor plus bounded
blocking upstream workers fits the current synchronous transport better than
introducing a general asynchronous service framework. Version/MSRV and size
remain implementation checks, not an installed dependency claim.

[SQLite WAL documentation](https://sqlite.org/wal.html) states one writer at a time
and same-host requirements. SQLite can be a later persistence implementation,
but is neither a socket pool nor required for the first lightweight broker.
Redis client documentation could not be fetched; no specific Redis behavior or
library choice is accepted here. The first release uses one journal writer,
bounded in-memory indexes and Unix domain IPC, without Redis or SQLite.

The local broker owns placement, credentials, quota admission and upstream I/O.
Clients keep Agent Loop and tools. An explicitly configured remote endpoint can
implement the same logical broker protocol; raw provider APIs are not broker
protocol implementations. No automatic local bypass on remote failure is allowed.

## 7. Service Engineering And Performance Evidence

Inspected Redis revision: 669b2a1316f5b35ecf964281b77c054ff28dc934.
[Event loop](https://github.com/redis/redis/blob/669b2a1316f5b35ecf964281b77c054ff28dc934/src/ae.c)
uses OS readiness backends and a fired-event batch.
[Networking source](https://github.com/redis/redis/blob/669b2a1316f5b35ecf964281b77c054ff28dc934/src/networking.c)
contains pending-write processing, client limits and output-buffer enforcement.
These are source inspections, not Redis benchmark executions or an assertion
that all current Redis processing is single-threaded.

[Client handling](https://redis.io/docs/latest/develop/reference/clients/) documents
nonblocking connections, descriptor limits, output-buffer hard/soft limits, and
aggregate client-memory accounting. Local broker limits must be much smaller
than general database defaults. A slow model-stream consumer needs bounded
backpressure and an explicit terminal outcome, not unlimited buffering.

[Latency guidance](https://redis.io/docs/latest/operate/oss_and_stack/management/optimization/latency/)
explains persistence stalls and grouping writes before durable acknowledgement.
The broker must wait for its durability barrier before authorizing a paid request;
it cannot copy a relaxed fsync policy and claim the same budget safety.

[Benchmark guidance](https://redis.io/docs/latest/operate/oss_and_stack/management/optimization/benchmarks/)
requires matching payloads, connection counts, pipelining and persistence settings.
The broker will separately measure connection churn, in-memory control latency,
durable admission, and mock-provider streaming. No Redis-equivalent throughput
or measured 5000-client capacity is claimed. No source code was copied.

## 8. Bounded source learning receipt

The [source manifest](../learn/subsets/redis-account-broker/source_manifest.tsv)
locks selected windows in ae.c, networking.c and aof.c at the revision above.
The [route record](../learn/subsets/redis-account-broker/route_decision.md)
declares direct root-owned understanding, not a cron or worker launch.
File and folder indices preserve one-to-one coverage. Each file note separates
observed control flow from concrete broker behavior and failure tests.

Product contracts specify the resulting implementation rules directly:
generation-safe connections and bounded reactor turns in broker section 4,
soft/hard backpressure and separate cancel receipts in section 5, and
written-versus-durable admission in section 7. These rules do not require
implementers to read upstream code and do not import a database dependency.
Only the selected learning windows are reviewed; runtime features and
performance gates remain unaccepted.
