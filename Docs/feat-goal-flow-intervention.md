# Goal, Flow, and Runtime Intervention Contract

> Design revision: 2026-09-13. Documentation-only feature contract, not an
> implementation receipt. The execution checklist remains
> [Zenpi_Execution_Blueprint.md](Zenpi_Execution_Blueprint.md); acceptance mapping
> lives in [the v2 review matrix](Zenpi_Execution_Blueprint_v2.md#7-v2-work-matrix).
> This document is self-contained: implementing it requires no external project,
> extension, prompt package, scheduler, or undocumented compatibility behavior.

## 1. Scope and Current State

Goal defines the outcome, Flow supplies procedural prompt guidance, and Runtime
performs model and tool execution. Plan and DAG are Flow presets, not competing
execution modes or separate engines. A single Core input owner serves ordinary
TUI execution, Goal execution, and headless clients.

The current tree has background jobs, cancellation, session checkpoints, domain
records, Blueprint DAG validation, and handoff foundations. It does not yet
provide this contract: natural-language Goal execution and Plan routes refuse
execution; Goal lacks an Objective and KRs; ordinary compaction does not restore
a Goal contract; busy TUI prompt submission cancels/reissues model work; Esc
clears a nonempty editor rather than promoting its text to steering. These
behaviors remain current until the new owners and acceptance gates are wired.

Non-goals: a second Agent Loop, a workflow DSL, a mandatory executable task graph,
a new database, a second event bus, a persistent worker pool, a hidden daemon,
automatic recursive delegation, or relaxed permissions. Flow prompts cannot
grant tools, bypass approval, alter budgets, or declare evidence verified.

## 2. Ownership and Durable State

| Owner | Responsibility | Must not duplicate |
|---|---|---|
| Core input owner | Atomic admission, target binding, one bounded intervention queue, promotion, consumption, resume admission | TUI/headless queue and cancel/reissue decisions |
| Goal controller inside the shared core | Outcome state, default guidance, progress, completion gates, settled continuation intent | Provider/tool/session loops or a second scheduler |
| Session/domain store owners | One authoritative representation per record, durable identity, recovery and compatibility decoding | A UI-owned Goal snapshot treated as truth |
| Runtime | Jobs, cancellation, request configuration snapshots, bounded dispatch and resource reaping | Goal completion decisions based on a job ending |
| Context assembler | Current Goal and scoped Flow projection plus unconsumed interventions | Authority reconstructed from a lossy conversation summary |
| TUI/headless adapters | Input intents and event projections; TUI alone owns focus and key gestures | Independent business state machines |

One session has at most one active root Goal; history may contain multiple Goals.
Goal stores a stable ID, semantic revision, execution epoch, project/session
identity, Objective, KRs, constraints, status, budget ledger reference, scoped
Flow guidance, a bounded progress checkpoint, task references and evidence refs.
Blueprint linkage is optional. Each KR has a stable ID and a checkable outcome:
a threshold, artifact condition, executable check, or explicit human acceptance.

Use the existing Goal status vocabulary: queued, running, paused, blocked,
cancelled, done. Awaiting confirmation or an external event is an explicit wait
reason, not a completed task; budget, quota and interruption pauses have typed
reasons. Understanding/planning/executing/verifying are progress phases, not a
second authority for lifecycle state. Progress updates cannot rewrite KRs.
Semantic edits increment revision; resuming creates a new execution epoch but
does not reset cumulative usage. Old epochs cannot complete current work.

## 3. Goal and Flow Commands

| Command | Contract |
|---|---|
| `/goal <objective>` | Create an explicit outcome Goal; understand the objective, propose KRs and a short plan, then wait for confirmation before implementation |
| `/goal`, `/goal status`, `/goal list`, `/goal supervise` | Read-only current status, history or task projection; no provider call and no new functional top-level TUI tab |
| `/goal plan [instruction]` | With an active Goal, fork its task context and use the instruction to generate a Plan proposal; return it through the ordinary intervention owner |
| `/goal dag [instruction]` | Same fork and return contract, with dependency-analysis guidance instead of sequential-plan guidance |
| `--task <id>` on Plan/DAG | Restrict the proposal to a known task; never interpret the ID as a new objective |
| `/goal approve [revision]` | Approve the displayed proposal revision; headless must provide the expected revision; this is not a tool permission grant |
| `/goal edit <objective>` | Explicitly revise the outcome; KR or scope changes require confirmation rather than silently narrowing success |
| `/goal steer <text>` | Explicit immediate steering through the shared input owner; task-scoped when requested, otherwise root guidance is delivered to active branches with per-target receipts |
| `/goal pause`, `/goal stop` | Stop dispatch and pause owned work while retaining progress; stop is a pause alias |
| `/goal resume [id]` | Resume an eligible paused/blocked Goal after validation; refuse a conflicting active Goal and never reopen done/cancelled Goals implicitly |
| `/goal cancel`, `/goal clear` | Cancel the Goal, or stop owned work and detach its active binding; preserve the audit history and do not delete files |
| `/goal rebuild <id>` | Stop and reconcile the old task attempt, then request a revised approach; preserve unrelated outcomes and invalidate affected downstream evidence |
| `/goal handoff <id>` | Transfer a bounded task only after recipient acknowledgement; never retain two active execution owners |
| `/goal rollout --force` | Request the simplest remaining path and summary, without lowering KRs or bypassing approval, budget or verification |

Without an active Goal, Plan/DAG with an explicit instruction creates a Goal and
uses that preset for its initial proposal; with neither Goal nor instruction it
refuses without mutation. Existing Goal plus instruction means a proposal about
that Goal, not objective replacement. `/plan` reuses planning without creating a
root Goal. Known malformed commands fail typed parsing, never fall through as
ordinary objective text. Existing show/list/put forms get boundary compatibility;
legacy transition-to-done must not bypass the outcome gate.

A simple task needs only short procedural guidance. A complex task may use local
DAG guidance inside a Plan. Save scope and return position so local guidance
does not overwrite the outer task. A generated dependency description does not
automatically spawn workers. Only actual tracked/delegated tasks need stable IDs.
At the same scope the latest explicitly applied guidance revision supersedes the
old one, while completed work survives. Distinct user instructions are not
silently coalesced; cancellation or replacement must name the prior request.

## 4. One Intervention Admission Contract

Conceptually, an intervention contains an ID, source, session/Goal/task binding,
expected semantic revision and epoch, exact content, and delivery intent:
`AtBoundary(boundary_id)` or `Immediate`. Sources distinguish user text from a
planning-fork proposal. Prompt content is task data, not a higher-priority policy.
Preserve user bytes; trim only for empty-input and reserved-command detection.

The Core owner validates and reserves capacity before accepting an operation.
Admission returns an accepted identity or a typed rejection such as queue-full,
target-mismatch, closed, missing-checkpoint or invalid-command. Rejection leaves
the draft, queue, model settings, task state and workspace unchanged. An accepted
operation has one owner and one consumer; adapters must not clone it into a
second queue. Repeated request IDs return the prior result; conflicting reuse
fails closed. Capacity is bounded by item count and bytes.

Accepted, durable, consumed-at-a-request-boundary and observed-in-a-model-request
are distinct events. Acceptance is not proof of persistence or model execution.
The owner journals accepted content before any provider dispatch; persistence
failure prevents delivery and reports recoverable failure without claiming the
message was applied. Recovery uses the durable request/operation ledger and
never retries an unknown provider or tool effect solely to manufacture an ACK.

Deferred interventions are displayed in a pending projection but are excluded
from current provider context. Default binding is the current explicit task;
otherwise its current KR; otherwise the current ordinary execution run. A
completed tool call is not a task boundary. Task closure and KR verification
must be typed owner events, not matching words in an assistant response.
Before admitting the next task or committing root completion, drain eligible
interventions in FIFO order. Stop, failure or missing boundary evidence leaves
them pending and visible; it does not discard them or apply them to a different
session. Explicit promotion changes the same intervention, not a copied message.

## 5. TUI Input and Two-Press Esc

These rules apply in the main conversation editor during either Goal or ordinary
execution. Slash commands and the explicit local shell route remain typed
commands, not text accidentally executed by a steering handler.

| Gesture | Required behavior |
|---|---|
| Enter on ordinary text during execution | Admit deferred intervention; current work continues; clear editor only after acceptance |
| First Esc with a nonempty draft | Submit the draft with immediate intent; accepted text enters durable conversation control history, but model effect is reported separately |
| First Esc with empty draft and pending interventions | Promote the selected pending item, or the oldest item when none is selected; preserve its ID |
| First Esc without text or pending work | Send no empty message; arm stop confirmation for the current execution |
| Second consecutive Esc | Pause the current root execution and cancel its owned activity, including children and planning forks; prevent automatic Goal restart |
| Typing, a different key, focus change or execution identity change | Reset the two-press gesture; terminal key-repeat events never count as the second press |
| Esc inside a palette or modal | Dismiss that surface only; do not also steer, approve or stop work |

Use a small typed editor gesture state, not scattered boolean flags. Bind the
armed state to the root execution, not an incidental provider job ID that changes
during steer. A rejected submission preserves the draft and does not arm a
successful-steer state. Once no root execution exists, Esc cannot cancel a later
unrelated task. UI reports pending, awaiting-safe-point, applied, stopping and
stopped/unknown from real owner events, not optimistic key handling.

Immediate means no wait for task/KR completion; it does not mean rewriting an
HTTP request already sent or rolling back tool side effects. Runtime uses native
steering where supported, or cancels sampling and reissues at a safe boundary.
Maintain valid tool-call/result pairs and inspect unknown effects before retry.
If immediate effect is currently impossible, show the precise waiting reason.
Steering never means approval; superseded operations invalidate their approvals.

## 6. Stop, Continue, Recovery and Settings

Stopping first fences all new dispatch and automatic continuation, then cancels
owned operations and collects checkpoints/results. The resource owner must
wait/reap children and flush records even if a UI waiter goes away. An external
worker without a confirmed cancellation remains stopping/unknown, never falsely
stopped. No new execution may race an unreconciled old writer.

The exact trimmed input `continue` (ASCII case-insensitive) or `继续` requests
resume only when the current session has a resumable interrupted execution.
This works without a Goal as well. Longer sentences remain ordinary prompts.
Without a checkpoint, report no resumable task; while already running, do not
create another run. During cancellation, retain at most one resume intent and
start it only after safe reconciliation. Preserve Goal, KRs, work, cumulative
usage and unconsumed interventions; resume never blindly repeats the original
prompt. Budget or quota still unavailable means resume is refused, not refunded.
Revalidate retained user input against the resumed scope before rebinding it to
the new epoch; unresolved conflicts remain visible for explicit disposition.
This does not authorize delivery of a planning result invalidated by stop.

Compaction persists progress and intervention delivery state before rebuilding
ordinary history. Reinject the current Goal/KRs, scoped Flow and task checkpoint
from authoritative state at the next request; inject pending text only when its
delivery rule permits. Failed compaction leaves the last valid checkpoint.
Stopped/cleared Goal contracts must be explicitly inactive so an old summary
cannot revive work. Restoring a process restores state, not permission to start
paid work automatically; explicit resume is required for interrupted execution.

Model/configuration updates are validated before commitment. Each model request
uses a fixed configuration snapshot; accepted changes affect the next request,
not an in-flight request. Recheck tools, protocol and context-window support,
compact when necessary, and preserve the old selection on refusal. Never reset
Goal usage or mark a task done because its model changed. Running children keep
their captured model; new children inherit the current default unless pinned.

## 7. Read-Only Planning Fork and Return

Plan and DAG use the same bounded planning operation with a different Flow
prompt. Fork the latest committed, causally complete task context, including
Goal/KRs, relevant guidance and the user's instruction. Record source session,
sequence, task, revision, epoch and configuration. Do not clone a live Agent,
running tool operation, pending approval or cancellation token into an active
duplicate. Incomplete tool exchanges must be reconciled or explicitly omitted
from the snapshot before provider serialization.

The mainline continues while the planning fork runs. The fork shares the Agent
Loop implementation, not mutable execution state. Allow only bounded read-only
investigation; deny workspace writes, execution commands, authority to complete
the root Goal, and further delegation. Charge the parent ledger and reserve
fork concurrency/usage before starting. Admission failure changes no mainline
progress. Host-owned private journal writes are allowed; planning is not a
promise that no state is ever persisted.
Reserve one pending position and a bounded result-byte allowance in the same
intervention queue at fork admission. Readiness fills that position; it does not
create a second queue entry. An oversized result fails explicitly. Timeout,
failure or cancellation releases the execution reservation and resolves the
pending position with an error receipt so it cannot block FIFO indefinitely.

A context fork is not a filesystem snapshot. Capture hashes/revisions of files
actually consulted and expose changed or inconsistent inputs. Return only the
proposal, assumptions, relevant source references, affected scope and validation
requirements; do not merge the fork's entire conversation or mutate mainline
task state. A useful simple proposal needs no mandatory graph schema.
The DAG preset asks for cycle checks, missing dependencies, KR coverage, write
conflicts and concurrency-budget feasibility. Validate structured dependencies
when present with the existing graph validator; unsupported or uncertain edges
remain explicit questions, not a claim that prompt reasoning proved safety.

Return the proposal as an intervention, deferred by default. First Esc on a
pending planning request sets immediate-on-ready; it never injects a nonexistent
result or sends the slash command as raw model text. Second Esc stops the root
and its fork. Fork failure is an explicit terminal planning error, not an empty
successful plan or a mainline retry trigger.

Before delivery, recheck root identity, revision, epoch and request disposition.
If the task finished but the same Goal remains active, offer the proposal as a
stale-source suggestion about remaining work; the mainline must reconcile it.
Changed objective, stopped root, cancelled request or replaced planning request
retains an audit artifact but does not auto-deliver or restart work. A proposal
with changed files is advice, never accepted evidence. The mainline may accept,
partially use or reject it; changing KRs, budget or permissions requires explicit
confirmation. Within already-approved scope, refinement need not stop unrelated
work. All proposals retain provenance and never impersonate user approval.

Example: task A is running under Plan revision 1. `/goal plan <instruction>`
starts proposal P without restarting A. A subsequent `/goal dag <instruction>`
starts a distinct proposal D, subject to the same bounded admission. Neither
command itself changes the active Flow. An unfinished earlier proposal occupies
its FIFO position; D does not silently leapfrog it. The user may explicitly
cancel/replace P or promote D. At delivery the mainline reconciles each proposal
with current progress; applying D at the root supersedes root Plan guidance,
while applying D to a complex subtask preserves the outer Plan. Completed A
and its verified evidence are retained in either case.

## 8. Goal Continuation and Bounded Children

A low-level run ending only records continuation intent. Dispatch exactly one
continuation after runtime settlement: no active request, outstanding tool
finalizer, provider retry, compaction retry, eligible user input or prior
continuation delivery. Pending boundary interventions take precedence over
starting the next task and over the final completion decision. Waiting for
confirmation, external events, limits or real blockers does not busy-poll a model.

Goal completion requests must reference evidence for every KR and match the
current identity/revision/epoch. Execute declared checks where possible; require
explicit human acceptance for non-machine-checkable outcomes. A model's summary,
tool-free response or control-plane receipt is not proof of outcome completion.

Only the root host may create bounded child executions with task identity,
scoped KRs/Flow, owned paths, permissions, budget reservations and evidence
requirements. DAG guidance may run serially; it never automatically grants
parallelism. Reject overlapping writers unless isolation or serialization proves
safety. Children cannot change the root Goal or recursively spawn children.
Child context compaction restores its scoped contract. Root steering has an
individual delivery receipt per affected child; root model changes do not
silently retarget running children. Results require parent validation and stale
results remain audit-only. Rebuild and handoff fence old attempts and reconcile
effects before replacement ownership. No detached child may outlive its owner
without an explicit tracked stopping/unknown state and a reaping responsibility.

## 9. Compatibility, Conflicts and Migration

| Existing contract | Resolution |
|---|---|
| Goal must link an immutable Blueprint | Add versioned outcome records with optional links; decode old records at the persistence boundary without inventing KRs or launching work |
| Planning forbids all tools and persistence | Define planning as no implementation side effects; allow controlled reads and host journals |
| Busy text immediately cancels/reissues; Esc clears text | Migrate both adapters to one input owner; keep user shell and approval routes distinct |
| Legacy status transition can mark done | Preserve read compatibility, but route new completion through KR evidence validation |
| No product-host child execution | Permit only the specifically gated root-host planning/child lanes after acceptance; recursive workers, hidden services and repository automation workers remain prohibited |
| Multiple flow modes or queues | Plan/DAG are scoped prompt presets; fork results and user text share intervention admission |

This is a product design revision, not authorization to launch repository
automation workers. The execution specification's frozen worker transport,
concurrency and no-nested-worker rules are unchanged. New product-host children
require their explicit implementation, isolation and acceptance gates; no current
runtime policy is weakened by this document.

Migration order: introduce typed identities and compatibility readers; connect
the shared intervention owner; move TUI/headless callers; add Goal context and
settled continuation; add read-only planning forks; add bounded child ownership;
then close end-to-end gates. Retire superseded routes only after caller inventory
and compatibility tests. Reverting a slice must preserve stored records and
refuse unsupported execution rather than silently reinterpreting it. Do not
introduce a second provider adapter, session writer, scheduler or fallback loop.

## 10. Acceptance Matrix

Every row below is future executable evidence, not a test-pass claim. Test both
TUI PTY interaction and headless equivalent intents through the production Core
owner. Use controlled scheduling/barriers, not timing luck or string-only mocks.

| Case | Required evidence |
|---|---|
| Boundary input | Enter does not cancel active work; exact text stays out of provider context until the bound task/KR/run closes |
| Boundary/completion race | An input admitted before closure is handled before the next task or Goal completion; an expired target is explicitly rejected or held, never silently retargeted |
| Esc with draft/pending/empty input | First Esc admits/promotes once; second Esc stops; rejected input preserves draft; repeats and palette dismissal do not accidentally stop |
| Immediate steer | Correct correlation and tool-result pairing; no false applied event, replayed side effect or stale approval |
| Stop and continue race | One resumed execution after old owner reconciliation, preserved usage, and no detached writer or stale completion |
| Recovery aliases | Exact continue words work for Goal and ordinary interrupted runs; longer text, running state and missing checkpoints do not start duplicate work |
| Compaction/restart | Goal, Flow, task and pending-intervention state survive; old contracts stay inactive; durable applied inputs are not duplicated |
| Model change | Paused fixture proves old in-flight snapshot and new next-request snapshot; refusal leaves settings and budget intact |
| Plan/DAG fork | Mainline advances concurrently; planning cannot write or delegate; parent ledger is charged; only proposal content returns through intervention admission |
| Planning drift/late result | Source hashes, finished task, edited Goal, pause, clear, replacement and epoch change cannot silently overwrite progress or restart execution |
| Child control | Scoped steering receipts, cancel/reap, handoff acknowledgement, no recursive delegation, writer conflict rejection and parent evidence acceptance |
| Resource/refusal paths | Queue byte/item limits, fork reservation, persistence failure, timeout, cancellation and shutdown yield typed outcomes without hidden work |
| Outcome gate | Every KR has validated evidence or explicit human acceptance; local bookkeeping, prose success and budget exhaustion cannot complete a Goal |

Structural document validation uses `python3 -B tools/validate_blueprint.py` and
`python3 -B tools/validate_blueprint_v2.py`. Runtime acceptance additionally needs
new focused tests and installed PTY/headless fixtures for the matrix above;
passing the document validators is not product acceptance.
