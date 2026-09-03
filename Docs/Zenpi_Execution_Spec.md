# zenpi Execution Specification

> Frozen repository-local policy for the `execution-cron-builder` workflow.
> This file describes how work is claimed, isolated, validated, integrated,
> and published. It is not a second product checklist; the authoritative
> checklist is `Zenpi_Execution_Blueprint.md`.

```yaml
schema_version: execution-spec/v1
repository_root: /Users/mac/Github/zenpi
authoritative_blueprint: Docs/Zenpi_Execution_Blueprint.md
gantt_projection: Docs/Zenpi_Execution_Gantt.md
gantt_naming: exact-prefix-Blueprint-to-Gantt
checklist_marks: '[ ]|[_]|[x]'
stable_id_pattern: '^ZP-[0-9]{3}$'
worker_lifecycle: bounded
desired_live_workers: 2
hard_worker_cap: 2
nested_agents: forbidden
worker_transport: tmux_codex_tui
worker_goal_command: /goal
app_server_workers: forbidden
automatic_goal_continuation: forbidden
max_outstanding_requests_per_execution: 1
product_modes: [tui, headless]
per_item_code_loc_cap: 5000
loc_policy: "every Blueprint item has an integer Estimated LOC with 0 <= value < 5000; scope is a per-item forecast of implementation/test code attributable to the row (Rust/Python/Shell), not a current-file inventory; docs/config/generated artifacts count 0; the declared forecast has a strict upper bound of 5000 (exclusive); aggregate repository LOC is informational"
```

## 1. Product boundary

zenpi is a Rust migration of the useful, well-bounded behavior in the
official `pi-agent` TypeScript implementation. The public binary has exactly
two modes selected by `--mode`:

| Mode | Contract | Output discipline |
|---|---|---|
| `tui` | Interactive terminal conversation with status, transcript, input, resize handling, and clean terminal restoration | Human-readable terminal control sequences only |
| `headless` | Long-lived stdin/stdout JSONL process for composition with another agent | One complete JSON object per LF-terminated input/output record; diagnostics only on stderr |

The parser must reject `print`, `json`, `rpc`, `server`, `daemon`, and any
unknown mode with a typed error before opening a session or backend. These are
not hidden aliases and no third mode, HTTP listener, broker, daemon, or app
server is in scope. TUI and headless call the same core, backend trait, session
journal, and handoff codec; mode-specific code owns only transport and view.

The migration inventory is based on these read-only local evidence points:

| Source | Revision/evidence | Boundary used by zenpi |
|---|---|---|
| `pi-mono/packages/coding-agent` and `pi-mono/packages/tui` | `/Users/mac/Github/pi-mono` at `23282f60782f02b9e22b787e4b22af441454fa16` | Session/core separation, event-driven input, terminal cleanup, and interactive rendering concepts |
| `b3ehive` execution/learn/compete/looper skills | `/Users/mac/Github/b3ehive` at `76cb266a79ef1555a7c974af0b7340333dbd39e8` | Claim identity, durable handoff, checksum evidence, bounded ownership, and fail-closed validation; not the full controller |
| `codex-rust-deslop` report | `/Users/mac/Downloads/codex-rust-deslop`, report SHA-256 `e92528d14bd26369ac7b7fbfc1edc87fa10fe3cbd8f22876a25d4c3cf5261be2` | One owner/entry/state machine, typed errors, explicit ownership, compatibility at boundaries, failure-first tests, and deletion of redundant paths |

Research notes must preserve source revisions and distinguish observed facts
from design decisions. Network access is not required for a local build; a
missing remote or credential is a research blocker, never a reason to guess.

## 2. Lean b3ehive subset

The following b3ehive concepts are first-class zenpi data, not an external
service. They are inert, bounded records; they do not start workers or make
scheduling decisions:

1. A bounded, versioned `Handoff` record carries `from`, `to`, an immutable
   handoff/claim reference, summary, artifact references, session context,
   creation time, and (when exported) a content digest.
2. `ResourceBudget`/`ResourceEnvelope`/`ResourceLease` account for bounded
   tokens, attempts, wall time, and disk use without running a scheduler.
   `SideEffectGate` records an explicit allow/deny decision for publish,
   protected-path, network/spend, or destructive actions.
3. `EvidenceRecord`, `RouteDecision`, `EstimatorPolicy`, and `LooperLog` carry
   compact validation, route, estimate, and feedback evidence. They are
   append/transfer data only; zenpi does not implement the b3ehive controller,
   optimizer, competition, or looper around them. A `ParentLeaseRef` may
   describe work performed by an external host, but zenpi never spawns or
   hides a nested agent.
4. The append-only session journal records user input, assistant output,
   lifecycle events, errors, and handoffs in sequence order. A resume validates
   the sequence and ignores a truncated final line rather than corrupting prior
   records.
5. A result manifest records changed repository-relative paths, validation
   commands/outcomes, and a checksum. Workers can self-test (`[_]`), while only
   the canonical Master can accept (`[x]`).
6. Handoff exports are bounded canonical JSONL values. The session owner uses
   append plus flush and `sync_data` for each record; it never rewrites a valid
   prefix. Atomic replacement is reserved for a future derived projection, not
   the append-only journal. No broker or background service is needed.

The following are explicitly out of scope under the Occam constraint: a
general b3 scheduler/cron daemon, persistent worker pool, nested-agent tree,
proposal competition, looper/ROI controller, remote queue, dashboard, plugin
marketplace, or a second protocol/server. If a future feature needs one of
these, it requires a new specification revision and a fresh checklist item;
the implementation must not smuggle it into either public mode.

## 3. Runtime and data contracts

### 3.1 Core and backend

`src/core.rs` owns turn state and the single agent entry point. `src/backend.rs`
owns the backend trait and deterministic `echo` backend; an optional
OpenAI-compatible backend is an adapter, not a second core. Backend errors are
typed, preserve retryability, and never leak credentials. The runtime phase is
explicit (`idle`, `running`, `closed`); a failed provider returns the agent to
`idle` with a typed error, and a TUI interrupt cancels admission without a
backend call. Invalid transitions are rejected without mutating the journal.

### 3.2 Session journal

`src/session.rs` owns all persistence. Each LF-delimited JSON object contains
`schema_version`, `session_id`, monotonic `seq`, RFC3339 `timestamp`, `kind`,
and a typed payload. User input and the resulting assistant message are
durably appended before a completion response/event is emitted. Writes are
bounded and use append, flush, and `sync_data` under the sole store owner;
malformed or out-of-order records are skipped with a recovery warning and never
overwrite the valid prefix. On Unix, session files are created or tightened to
mode `0600` because prompts and completions may contain private material. The
default location is configurable, but tests use a temporary directory supplied
by the caller.

### 3.3 Headless JSONL

`src/protocol.rs` owns parsing/encoding and `src/headless.rs` owns the stdio
loop. Supported request types are `prompt`, `steer`, `status`, `handoff`,
`resume`, and `shutdown`. Every request has an ID and receives one terminal
response; unknown fields are tolerated only when they do not change semantics.
Input is framed strictly by LF (U+2028/U+2029 are payload characters). A
malformed non-empty frame gets a stable error code, never mutates session state,
and the loop remains usable; blank LF frames are ignored. Stdout is
protocol-only; diagnostics and tracing go to stderr. EOF performs an orderly
shutdown and flush. The provider boundary is synchronous, so headless reads the
next command only after the current provider request completes; `steer` is a
between-turn state-machine command in this release, not live in-flight input.

### 3.4 Handoff

`src/b3.rs` owns validation and serialization of the lean records. Limits are
64 KiB per record, 16 KiB summary, 64 artifacts, and repository-relative
artifact paths only. A handoff export digest covers canonical JSON bytes
excluding the digest field; import verifies schema, recipient, digest, and
session identity before appending. Budget/evidence records use the same
bounded identifier/text/list helpers. Evidence may name a validation command,
but zenpi never executes it; handoff summaries reject secret material and
artifact paths reject absolute paths or binary blobs. This module never launches
a worker or scheduler.

### 3.5 TUI

`src/tui.rs` owns terminal I/O and view state while delegating turns to core.
It uses crossterm with one Ratatui in-memory double buffer (and no second UI or
async framework): input, transcript, status, spinner/progress, help, and error
regions have stable bounds. Updates are coalesced on approximately a 16 ms
tick; Ratatui's buffer diff emits no unchanged cells/rows. A resize invalidates
the buffer and performs one full redraw using clamped dimensions and Unicode
display width. The provider callback is synchronous in this release, so network
latency can delay input and Ctrl-C cannot cancel an in-flight provider request.
Raw mode, cursor visibility, alternate-screen state, and orderly EOF/interrupt
cleanup are restored by the terminal guard. External unhandled termination such
as `SIGKILL` is outside the cleanup guarantee.

## 4. Rust deslop rules

These rules are acceptance criteria, not style suggestions:

- Every concept has one owner and one public entry point. Do not add wrappers,
  duplicate loaders, or compatibility aliases merely to make a call compile.
- Use enums/newtypes for modes, lifecycle, errors, and protocol messages. Keep
  retryability and refusal reasons typed; do not match on diagnostic strings.
- Prefer moves and borrows (`Cow` where it removes a real allocation) over
  blanket `Clone`, `Arc`, or `Mutex`. Every unavoidable clone/lock is justified
  in a nearby comment or design note.
- Runtime code has no `unwrap()`/`expect()` on fallible input, no panic-based
  control flow, and no `unsafe` without a reviewed safety proof. Failures at
  protocol, terminal, filesystem, or backend boundaries fail closed and leave
  prior durable state intact.
- Cancellation, child processes, file handles, raw terminal state, and temp
  files have explicit owners and drop/cleanup tests. Compatibility conversion
  stays at persistence/protocol boundaries.
- Keep diffs narrow and commits in English. `cargo fmt`, strict Clippy, tests,
  and a source-line audit are mandatory before Master acceptance.

## 5. Execution policy

The controller uses bounded executions. Each claim gets a unique runtime root
`${TMPDIR:-/tmp}/zenpi-execution/tasks/<claim-id>/<run-id>/` containing
`work/`, `codex-home/`, `tmux.sock`, `claim.json`, and `result.json`. The
canonical checkout is read-only to workers. Only declared repository-relative
owned paths may be changed; secrets, credentials, full-checkout copies, and
pushes are forbidden. Nested workers are forbidden, so one claim always maps
to one execution identity.

Codex workers, if admitted, must use one task-local tmux server/socket and one
interactive Codex TUI process with a private writable `CODEX_HOME` and exactly
one authenticated `/goal`. `codex exec`, app-server, shared daemons, shared
tmux, and no-tmux workers are hard failures. The controller acquires an atomic
running-turn lease and outbound-request lease before Enter; each execution has
at most one outstanding request. Bounded completion terminalizes the goal and
stops its transport; no automatic continuation is allowed.

Conservative defaults (all may be overridden only by an explicit operator
policy and recorded in the claim ledger) are:

| Limit | Default | Binding reason |
|---|---:|---|
| logical claims / admitted executions | 2 / 2 | Keep a standalone agent cheap |
| startup fanout / live TUI transports | 1 / 2 | Avoid resize/startup storms |
| authenticated running turns | 1 | Bound CPU and provider pressure |
| outbound request starts | 2 per 10 s | Rate-limit guard |
| in-flight requests / per-execution outstanding | 1 / 1 | No duplicate turns |
| integration workers / validator leases | 1 / 1 | Canonical write serialization |
| request-storm breaker | 3 starts per 10 s or any unauthorized continuation | Fail closed; reset is explicit and audited |
| scheduler cadence / tick budget | 30 s / 5 s | Retry-safe, non-blocking ownership |

Every underfilled slot records one of `dependency`, `path_conflict`,
`startup`, `host_resource`, `external_limit`, `route`, or `validator`; a
generic capacity message is insufficient. Harvest handoffs before pruning
claims. Cleanup is scoped to recorded task roots, PIDs, sockets, and locks and
never deletes the canonical repository.

## 6. Validation profiles and completion

The canonical Master runs these profiles from the repository root:

```text
format: cargo fmt --all -- --check
lint: cargo clippy --all-targets --all-features -- -D warnings
unit: cargo test --all-targets
protocol: cargo test --test headless_protocol --test session_recovery
tui: cargo test --test tui_resize -- --nocapture
item_loc: every Blueprint row has integer Estimated LOC in [0, 5000); aggregate source inventory is informational
mode_scan: binary help + source scan; exactly tui/headless, forbidden aliases rejected
handoff: schema/digest/path-limit and round-trip tests
release: cargo build --release --locked; tools/headless_smoke.sh --release
installed_user: python3 tools/user_smoke.py (isolated cargo install plus public-path smoke)
publish: draft2repo dry-run, remote receipt and local-source-retained checks
```

`Estimated LOC` is a per-checklist-item forecast of implementation/test code
attributable to that row (Rust/Python/Shell), not a current-file inventory or a
project-wide total. The declared forecast has a strict upper bound of 5,000
(exclusive), and `0` is valid for a documentation, reconciliation, or
publication-only item. The
physical Rust inventory under `src/`, `tests/`, `examples/`, and `benches/` is
reported for visibility only; its aggregate is not an acceptance cap. Tests must cover refusal and
failure paths, malformed/truncated records, out-of-order envelope records,
resize during rapid local updates, terminal cleanup, and resource ownership,
not only happy-path output. The append owner is deliberately single-threaded;
cross-process locking is outside this release.

The release artifact must be usable outside Cargo's development target. The
Master therefore validates both `target/release/zenpi` and an isolated
`cargo install --path . --locked` binary through the same public headless and
TUI smoke paths; a green compile without those runtime checks is insufficient.

### 6.1 End-user smoke acceptance

Completion is not established by a compile or unit test alone. Before marking
the release usable, the Master must exercise the built artifact through the
public user path: `cargo build --release --locked`, `cargo install --path .
--locked` into an isolated root, `zenpi --help`, an offline `echo` prompt over
headless JSONL with a durable session, a resume/status cycle, a PTY TUI prompt
with terminal restoration, and a local OpenAI-compatible HTTP fixture that
verifies the request path, model, and bearer header without contacting a real
provider. Invalid backend configuration and invalid session paths must fail
before creating a session. The resulting evidence belongs to ZP-101, ZP-105,
ZP-107, ZP-201, and ZP-202; it is separate from the structural Blueprint
validator.

Completion requires every required Blueprint row `[x]`, zero `[ ]`/`[_]`, no
pending handoff/integration/repair, all applicable profiles green, a current
Gantt digest, and a verified `draft2repo` receipt for `weiyangzen/zenpi`.
The local `/Users/mac/Github/zenpi` directory must remain present and its
source-deletion receipt field must be `false`. A later fast-forward update may
advance the remote beyond the immutable creation receipt; in that case current
local/remote/API equality is checked separately and the original receipt is
retained as publication provenance.

## 7. Change control

Only the canonical Master may alter this specification, dependency edges,
limits, or `[x]` marks. A change records motivation, evidence, old/new value,
and affected IDs in the same English-language commit. On 2026-09-03 the LOC
policy was corrected from an aggregate 5,000-line Rust budget to the strict
per-item `Estimated LOC < 5000` forecast; ZP-005, ZP-207, and ZP-302 carry the
corresponding policy and validator changes, while aggregate source lines remain
informational. After any change the
Master must regenerate the Gantt atomically and rerun the parser, digest,
cycle, duplicate-ID, mode, per-item-LOC, and source-inventory checks. Research notes are
supporting evidence; they never become a competing checklist.
