# zenpi Execution Blueprint v2

> **Review draft, version 2.0.0 (2026-09-04).** This document is a
> self-audit and re-plan for the terminal-agent product. It is intentionally
> **not** the current authoritative checklist and it does not mutate or
> supersede `Docs/Zenpi_Execution_Blueprint.md` until the Master accepts the
> migration described in this file. The v1 checklist therefore remains the
> source of truth for the already recorded execution receipt; this document is
> the source of truth for the proposed next product contract.

```yaml
schema_version: execution-blueprint/v2
blueprint_version: 2.0.0
revision_date: 2026-09-05
status: proposed-audit
authoritative: false
predecessor: Docs/Zenpi_Execution_Blueprint.md
stable_id_pattern: '^V2-[0-9]{3}$'
product_modes: [tui, headless]
per_item_code_loc_cap: 5000
per_item_code_loc_rule: "estimated_loc < 5000 (exclusive)"
loc_basis: per-item forecast of implementation and test code attributable to that row; documentation, configuration, and generated artifacts count as zero
status_values: AUDITED|PARTIAL|PLANNED|DEFERRED|ACCEPTED
required_for_v2_acceptance: all rows except DEFERRED rows
layout_name: bentobox
first_class_domains: [blueprint, goal, learn]
runtime_domains: [compete, loop]
gui_scope: future-separate-workspaces
```

The review draft is checked independently of the frozen v1 authority:

```text
python3 tools/validate_blueprint_v2.py
Blueprint v2 valid: 38 rows, max LOC 2800 < 5000
```

`tools/validate_blueprint.py` continues to validate the single authoritative
v1 Blueprint; the v2 checker verifies this draft's version, status vocabulary,
dependency DAG, duplicate IDs, and strict per-item LOC cap.

## 1. Why a v2 review is necessary

The previous framework work made the provider, session, tool, and transport
boundaries much stronger, but a green compile is not the same thing as a
usable Claude Code/Codex-style terminal agent. In particular, the v1 contract
was deliberately limited to a single transcript and a fixed vertical TUI. It
does not define multiline editing, slash commands, a BentoBox workspace,
first-class blueprint/goal/learn operations, or a rendering model for code and
Markdown blocks. It also records several capabilities as complete before a
user-facing end-to-end acceptance test exists.

This v2 draft makes the distinction explicit:

* `AUDITED` means the current tree and an executable observation support the
  statement. It is not a promise that the final v2 gate has passed.
* `PARTIAL` means a useful slice exists, but an important interaction,
  failure mode, or proof is missing.
* `PLANNED` means the contract is now specified but implementation is not
  accepted.
* `DEFERRED` means intentionally outside the lightweight v2.0 binary; it is
  not silently counted as done.
* `ACCEPTED` means that row's applicable acceptance evidence and gates pass; it
  is not a promise that the complete v2 matrix has passed. `V2-999` remains the
  master gate for the whole contract. A v2 row must never be marked accepted
  merely because a nearby v1 row is `[x]`.

The `Estimated LOC` value on every row is an independent forecast. The number
of rows is not a target, and there is no aggregate 5,000-line limit. Every
forecast below is strictly less than 5,000.

## 2. Current-state audit

**Latest executable slice (2026-09-05):** `src/domain_execution.rs` now owns a
bounded local `/blueprint run ID[@VERSION]` step. It selects one dependency-ready
item, persists a private running/terminal receipt, enforces the linked Goal
budget, and resumes a running receipt without allocating a duplicate attempt.
Both TUI and headless hosts call the same owner; it performs deterministic local
evidence only and does not invoke a provider, shell, scheduler, or nested agent.
The installed smoke now proves session GC and the Blueprint run owner: it
installs the production no-fixture binary, persists a Blueprint/Goal, executes
two dependency-ordered items across two processes, verifies private receipt
recovery and Goal completion, and asserts that no provider request occurred.

The following table is the evidence-ledger snapshot taken from the current
checkout on 2026-09-05. Paths and symbols are intentionally named so the audit
can be repeated after each integration pass.

| # | Requested experience | Status | Evidence in the current tree | What is still missing |
|---:|---|---|---|---|
| 1 | Input and multiline editing | PARTIAL | `src/tui.rs` now has UTF-8-safe cursor movement, Shift-Enter/Ctrl-J newline insertion, line navigation, bounded prompt scrolling, and focused cases in `tests/tui_resize.rs`; the installed-binary smoke also proves one exact bracketed-paste multiline submission through a real PTY. | Cursor movement across lines, multiline history, paste limits, bounded prompt scrolling, and the remaining resize/PTY matrix still need acceptance. |
| 2 | Streamed AI replies | PARTIAL | `src/backend.rs` emits typed `TextDelta`/`TextDone` events for Responses SSE; `src/tui.rs` and `src/headless.rs` consume provider events while a `BackgroundRunner` job runs. Headless provider/agent mailboxes and the TUI provider mailbox are bounded by both event count and serialized bytes; both hosts expose explicit drop/truncation markers, and focused tests cover SSE, UTF-8 byte accounting, oversized events, accounting reset, and blocked-output flooding. | Canonical lifecycle ordering and request correlation, global control-input fairness, and the complete slow-stream PTY/headless matrix are still missing; Chat compatibility remains blocking. |
| 3 | Tool-call status | PARTIAL | `src/core.rs` records `AgentEvent::ToolCall`/`ToolResult`; the TUI now maintains stable call-ID rows for running/succeeded/failed/cancelled states and tests them in `tests/tui_logs.rs`. | Headless and TUI do not yet consume one canonical lifecycle schema, and provider/core/tool status ordering still needs protocol-level acceptance. |
| 4 | Command confirmation | PARTIAL | `src/approval.rs` provides a first-response-wins coordinator; TUI and headless answer real pending requests. Write/edit requests carry a bounded pre-execution diff, execution revalidates the approved source digest, stale previews fail without writing, and the installed no-feature binary against a local OpenAI-compatible Responses fixture proves provider-to-approval-to-write-to-continuation. The durable approval receipt stores metadata/digest, not the diff body. | Approval focus, multiple simultaneous pending requests, timeout/cancel semantics, and the full PTY plus JSONL matrix still need acceptance. |
| 5 | Error recovery | PARTIAL | `src/backend.rs` has retry/backoff and circuit state; `src/session.rs` recovers a valid JSONL prefix and retains validated durable sequence envelopes. TUI/headless `/resume [sequence]` now returns a bounded journal suffix with a continuation cursor and durable marker; `/compact` writes a deterministic checkpoint that is restored for later provider context after restart. Session switching rejects empty, malformed, non-session, missing, and symlink paths without mutating them; startup journal reads are capped at 256 MiB and over-limit files are rejected before permission changes; invalid or exhausted sequence records no longer poison later appends. | Retrying an interrupted provider/tool side effect, automatically repairing a stuck active marker, concurrent journal-writer serialization, large-valid-journal streaming/indexing, and the kill/restart PTY/headless acceptance matrix remain open. |
| 6 | File modification diff | PARTIAL | `src/tools.rs` attaches typed bounded diffs to write/edit approvals before mutation, rejects unreadable sources, and revalidates the source digest immediately before execution; a stale approved preview returns `stale_preview` without writing. TUI shows the proposal and headless emits it in `approval_request`; the installed no-feature binary against a local OpenAI-compatible Responses fixture asserts it before approving. Workspace `/diff` also covers tracked/untracked/no-HEAD inspection. | Move TUI presentation to the shared Diff block, define binary-file behavior, and add atomic-write failure plus PTY approval acceptance. |
| 7 | Log folding | PARTIAL | Transcript messages are retained in a bounded `VecDeque`; `src/tui.rs` now adds `ToolRunStatus`, fold state, Ctrl-O, summary rendering, and per-tab/profile layout persistence, with focused coverage in `tests/tui_logs.rs` and `tests/tui_bentobox.rs`. | Add PTY evidence that a pending approval/error can never be hidden. |
| 8 | Session history | PARTIAL | `src/session.rs` owns append-only JSONL recovery; `Agent::history`, TUI up/down history, CLI `session list/inspect/fork/import/export/gc`, real `/session list`, `/session open PATH`, explicit-source `/session fork\|export\|import SOURCE DESTINATION`, and bounded durable `/resume [sequence]` routes exist in TUI/headless. Maintenance sources must be clean existing zenpi journals and destinations must not exist or be symlinks, so no path silently means the active session and no destination is overwritten. Listing includes the legacy root journal; opening/replaying needs no provider call and the replay path survives process restart. Startup recovery eagerly loads a valid journal but now has a hard 256 MiB byte cap. | `/session gc`, the in-workspace browser/search UI, and history-aware conversation/goal navigation are still missing; large valid journals still need streaming/indexed recovery and near-cap startup measurement. |
| 9 | Interrupt and cancellation | PARTIAL | `src/runtime.rs` has a cooperative `CancellationToken`; core/tool loops check it; TUI/headless expose cancel/live-steer paths (including typed slash `/cancel`), including bounded promotion of a deferred headless steer before shutdown. Responses SSE body reads now poll cancellation at a bounded 100 ms interval and close a stalled body, with a focused no-retry test; production hosts use owned runners and deterministic tests cover deferred steer admission and shutdown ordering. | Chat/non-streaming requests and DNS/connect/send/header phases still have no external abort handle and remain bounded by ordinary timeouts; byte budgets for other event/control queues, control-input fairness, and complete shutdown/close-hook semantics remain to be accepted. |
| 10 | Terminal resize adaptation | PARTIAL | `crossterm` resize handling and `tests/tui_resize.rs` cover the current view; `src/layout.rs` plus `tests/layout.rs` provide wide/standard/compact/narrow/zero-cell geometry, capability collapse, ratios, tabs, presets, and deterministic focus/ratio APIs. The production async TUI now uses a bounded BentoBox adapter, tab bar, keyboard workspace controls, and profile/tab layout restore/save. | A full PTY acceptance matrix is still missing; the legacy synchronous callback keeps the compact vertical renderer. |
| a | Code/text/simple-Markdown and block rendering | PARTIAL | `src/render.rs` is wired into the TUI for Assistant/System messages (headings, lists, quotes, fences, inline styles); `src/view_model.rs` now defines bounded plain/Markdown/code/diff/tool/approval/error blocks and adapters, while User/Tool/Error messages in the current renderer remain literal and sanitized. `tests/render_markdown.rs`, `tests/tui_markdown.rs`, and `tests/view_model.rs` cover the bounded subset. The audit note [`Docs/research/zenpi-rendering.md`](research/zenpi-rendering.md) records the remaining boundary. | Carry the shared block model through headless events, add streaming fence boundaries, and accept deterministic golden snapshots. |
| b | Slash command area and common commands | PARTIAL | The hand-written CLI parser covers process commands such as `config`, `session`, and `extension`; `src/slash.rs` defines the shared typed grammar. Production TUI and headless route commands before provider submission. Goal status/show, bounded diff, attachment staging, domain read/put, Learn evidence/checkpoint inspection, session list/open/fork/export/import/GC, durable resume/compact, real approval decisions, cancel, redacted `/models` and `/doctor`, BentoBox `/layout` and `/pane` owner actions, and journal-only compete/loop intent submit/status are exercised through owners. `/session gc` requires an explicit bounded retention policy plus `--yes`, excludes the active journal, skips unowned/domain data, and returns a bounded receipt. Headless runtime-intent request IDs are deduplicated across restart and conflicting payload reuse fails closed. Unsupported actions return errors rather than fake success. | Wire natural-language plan/domain execution and the external compete/loop delivery, acknowledgement, and result lifecycle, then prove complete command-owner parity; headless layout/pane requests still require the interactive TUI owner. |
| b | b3ehive first-class domains | PARTIAL | `src/b3.rs` contains bounded handoff/budget/lease/evidence records; `src/domains.rs` adds typed, content-addressed Blueprint DAGs plus bounded Goal and Learn records, including the strict per-item `estimated_loc < 5000` invariant; `src/domain_store.rs` now provides a bounded, private, digest-checked JSONL snapshot store with atomic replacement and Goal-to-Blueprint validation. Read-only projections and explicit JSON `put` paths are wired in TUI/headless. A local Blueprint execution owner (`src/domain_execution.rs`) now selects one dependency-ready item at a time, persists running/terminal receipts, enforces the linked Goal budget, resumes an interrupted receipt without a duplicate attempt, and is exposed by `/blueprint run ID[@VERSION]` in both hosts; this is deterministic local evidence execution, not a shell/provider scheduler. Learn evidence owner commands validate a bounded repository-relative artifact, persist an idempotent reference, and return a hash receipt; Learn `resume` is explicitly a read-only checkpoint with external execution untracked. Compete/loop persist bounded typed runtime intents with explicit `journal_only` delivery and `execution_state: untracked`; headless keyed intents are restart-idempotent, and zenpi starts no hidden scheduler. | Goal-level parallel execution/cancel/worker handoff, actual Learn worker resume/result handoff, and external runtime delivery/acknowledgement/result import remain open. |
| c | Tabs and BentoBox workspace | PARTIAL | `src/layout.rs` defines Project/Goal/Learn/Review/Session tabs, capabilities, presets, breakpoints, safe geometry, focus/split controls, and strict profile/tab persistence. The production TUI renders the Ratatui workspace and uses separate single-slot workers for a bounded Resources pane and a bounded Blueprint/Goal summary projection in the center Gantt pane. Session switches clear the prior projection and reject stale completions. | This is a domain summary, not yet a complete Markdown progress Gantt: item execution status/evidence and Learn/Review/Session pane content remain open; periodic resource/domain polling is absent, and browser/PTY adapters stay disabled by default. |
| d | Scaling and platform quality | PARTIAL | Ratatui/crossterm provide a small terminal surface; release settings use size-oriented optimization; the layout model and production async BentoBox adapter compute compact fallbacks safely, and profile/tab layout state is persisted with bounded JSON and fail-closed migration. V2-302 now measures pure layout and TestBackend render budgets. | No production pane/PTY end-to-end budget or measured GUI surface. A Svelte-quality GUI for macOS/Linux/Windows is deliberately future work, not a v2.0 dependency. |

### Reproducible evidence snapshot

These observations are deliberately narrower than a completion claim:

```text
Audit summary (equivalent to `git status --short`, not a literal command dump)
  v2 slices: runtime/headless safety, multiline TUI, diff, log folding,
  Markdown renderer, slash grammar plus TUI dispatcher, domain records, and
  BentoBox layout adapter

cargo tree --depth 1
  12 direct normal dependencies (dev dependencies excluded by the budget probe);
  no tokio, reqwest, clap, or rusqlite

tools/bench_runtime.py (2026-09-05 arm64 Darwin receipt)
  6,249,216 release bytes and 12 direct normal dependencies; exact size and
  timings remain host/build-profile dependent (see `Docs/quality/runtime-budget-v2.md`)

Existing focused evidence
  tests/backend.rs          Responses/Chat fixture parsing and provider events
  tests/headless_protocol.rs JSONL framing, duplicate/in-flight, replay,
                             shutdown paths (full v2 acceptance pending)
  tests/runtime.rs          bounded runner, cancellation, panic, and shutdown cases
  tests/tui_resize.rs       mock-terminal resize and small-cell rendering
  tests/tools.rs            path/policy/tool-result contracts
  tests/session_cli.rs      session lifecycle commands
  tests/config.rs           profile/import/doctor and secret redaction
  tests/domains.rs          Blueprint/Goal/Learn bounds, DAG, digest, transitions
  tests/layout.rs           BentoBox preset geometry and collapse invariants
  tests/render_markdown.rs  bounded Markdown parsing/rendering
  tests/slash.rs            typed slash grammar, aliases, routing, bounds
  tests/tui_logs.rs         stable tool lifecycle rows and folding
  tests/tui_markdown.rs     live TUI renderer integration
```

The focused and full Rust gates pass for these slices, but that is deliberately
not recorded as product acceptance. A compile, a unit test, or a generated
Gantt file cannot prove the ten interactive experiences; `V2-305` still
requires user-facing PTY and headless acceptance for each remaining matrix
row. The installed smoke has executable proof for bracketed-paste multiline
submission, but that single path does not close the broader editor contract.

## 3. Lightweight technology decision record

The goal is a capable terminal agent without shipping a desktop/browser stack
or an idle server. The default binary should stay small; capability-heavy
adapters must be optional and must not leak into headless startup.

| Choice | v2.0 decision | Reason and gate |
|---|---|---|
| Rust | KEEP | Ownership, typed errors, one portable core, and straightforward process/terminal cleanup match the failure-sensitive product. |
| Ratatui + crossterm | KEEP | Already provides a single terminal buffer, resize events, and compact rendering. The current BentoBox adapter uses pure layout data plus Ratatui; do not add a second TUI framework. |
| serde + serde_json | KEEP | One typed representation can serve the JSONL protocol, session records, layout presets, and b3 handoffs. |
| JSONL journal | KEEP for v2.0 | Append-only, inspectable, crash-prefix recovery, and no database dependency. Add an index only after a measured history query problem. |
| `std::thread` + bounded channels | KEEP provisionally | The measured no-feature release is 6,249,216 bytes with 12 direct normal dependencies on the observed arm64 Darwin host (see the runtime receipt). It is enough to keep a blocking provider off the UI thread and is cheaper than introducing an executor solely for scheduling. It is not called an async executor. |
| Tokio + reqwest | DEFER behind a measured gate | Adopt together, not piecemeal, only if tests require external abort of an in-flight socket, nonblocking Chat streaming, or concurrent tools that bounded threads cannot satisfy. A v2.1 migration must replace the provider boundary coherently; a Tokio runtime around blocking `ureq` is explicitly rejected. |
| `ureq` | KEEP while the gate is green | It keeps the current synchronous adapter small and works with the dedicated worker. Responses SSE body reads now use a bounded cooperative poll, but Chat/non-streaming and pre-body network phases still have no external abort handle; the limitation is explicit rather than hidden. |
| clap | DEFER | The current parser has no dependency/compile cost and can be wrapped by a typed slash registry. Reconsider when command grammar, completion, and generated help exceed the hand parser's testable surface; do not add clap only for branding. |
| SQLite | DEFER / optional index | A single session owner does not need locking or migrations in v2.0. Introduce SQLite only with a benchmark showing JSONL history or multi-process indexing is the bottleneck, and keep the append-only journal as the recovery source. |
| Browser pane | DEFER / external adapter | An embedded browser is a large security and binary-size commitment. v2 models a browser pane as an optional capability that can show a bounded external URL/snapshot; no browser process starts by default. |
| Terminal pane | DEFER / external PTY adapter | A terminal pane may later own a PTY and child lifecycle, but v2.0 keeps `run_command` approval and reaping in the existing tool boundary. No hidden shell multiplexer or daemon is introduced. |
| GUI (Svelte-quality) | DEFER / separate workspaces | No GUI is built now. A future macOS, Linux, and Windows client will consume the stable headless/core protocol from separate crates or workspace members, so GUI dependencies never enter the lightweight TUI/headless binary. |

### Hard size and ownership rules

1. `cargo tree --depth 1`, release binary size, cold-start time, and peak RSS are
   recorded for every dependency decision. A feature is not "lightweight" by
   intent alone.
2. The default build has exactly two public transports: `tui` and `headless`.
   Optional browser, PTY, GUI, Tokio, reqwest, or SQLite adapters are feature
   or workspace boundaries, not hidden modes.
3. Blocking work runs outside the render/input loop. Every queue has a byte or
   item bound, and the target shutdown contract joins owned workers or reports
   why a join is impossible. The current async stdin reader is still a
   documented join gap in V2-109. Session journal startup reads are separately
   capped at 256 MiB before parsing or permission mutation.
4. Each blueprint row below forecasts fewer than 5,000 implementation/test
   LOC independently. The forecast is not a promise that the whole repository
   will contain fewer than 5,000 lines.

## 4. Product and ownership model

### 4.1 One core, two transports, one view model

The target v2 ownership contract is: `src/core.rs` owns turn admission, tool
policy, approval, recovery, and durable events; `src/headless.rs` owns only
LF-JSONL framing and stdout/stderr discipline; and `src/tui.rs` owns terminal
lifecycle and input while consuming the same typed event stream as headless.
The current tree has not yet completed the shared view-model layer that would
normalize provider text, Markdown blocks, tool states, diffs, approvals, and
errors before either renderer sees them.

The **target v2** event lifecycle is explicit. The current headless adapter now
emits correlated, replayable runtime acceptance, queue, start, and cancellation
events alongside its provider/agent events, but the view-model/block projection
is not yet one canonical stream, so lifecycle parity remains a partial row:

```text
request.accepted
turn.started
assistant.delta*
tool.started
tool.output.delta*
tool.succeeded | tool.failed | tool.cancelled
approval.required | approval.accepted | approval.denied
turn.recoverable_error | turn.cancelled | turn.completed
```

Headless provider/agent mailboxes and the TUI provider mailbox have count and
serialized-byte bounds with explicit loss markers. Other event/control queues,
mandatory request correlation, fair output scheduling, and the complete
ordering matrix remain open. For turn operations, the terminal response follows its
durable operation marker; control-plane responses are not operation-marked. A
dropped stream never synthesizes a successful completion.

### 4.2 First-class b3ehive domains

The v2 product target treats these as local, durable user concepts. The current
`src/domains.rs` provides bounded typed records and validation;
`src/domain_store.rs` provides a private snapshot store, and read-only/explicit
put host routes are wired. Learn evidence and read-only resume-checkpoint owners
are also wired, but they do not start a worker: actual execution and result
handoff remain planned or partial rows. Fields such as current item,
mapping manifests, and result handoffs below describe the target schema, not
fields currently persisted by the in-memory records:

* **Blueprint**: a versioned DAG of bounded work items, dependencies, owned
  paths, acceptance commands, and per-item LOC forecast. A blueprint has an
  immutable content digest and can be inspected or validated without running
  work.
* **Goal**: a user-facing execution intent linked to one blueprint/version,
  with status (`queued`, `running`, `blocked`, `cancelled`, `done`), budget,
  lease reference, current item, and evidence links. A goal may be resumed or
  cancelled without rewriting prior journal records.
* **Learn**: a source-to-target transformation task with source manifest,
  target contract, mapping/evidence records, and a bounded result handoff. It
  is not an opaque prompt alias.

`compete` and `loop` are runtime calls, not hidden local schedulers:

1. `/compete` creates a typed route/handoff request with a parent goal and
   resource envelope.
2. `/loop` requests a bounded continuation/feedback pass and records its lease,
   attempt, and evidence.
3. An external b3ehive runtime may execute those requests. zenpi imports only a
   validated result manifest and never spawns a nested agent, cron daemon, or
   competition controller on its own.

This distinction keeps blueprint/goal/learn useful when zenpi is standalone,
while preserving the user's b3ehive composition model.

### 4.3 Slash-command contract

The target command area is a typed command registry, not shell evaluation. The
current `src/slash.rs` supplies the parser/catalogue and both TUI input and
headless JSONL `type=command` now dispatch local commands before provider
submission. Read-only domain inspection, bounded resource snapshots, explicit
JSON `put` operations, bounded Learn evidence/checkpoint inspection, and
explicit-source session fork/export/import are wired; durable domain execution,
session GC, and complete command parity are not yet fully wired. The v2 gate requires commands to
be parsed before a model turn, bounded by protocol limits, rendered as
system/tool events, and exposed through an equivalent headless `command`
request so TUI and headless behavior cannot drift.

| Command | v2 meaning | Domain |
|---|---|---|
| `/help [command]` | list or inspect commands | core |
| `/model [id]` / `/models` | inspect or select an allowed model/profile | core/config |
| `/doctor` | run redacted configuration/runtime diagnostics | core/config |
| `/settings [key] [value]` | inspect or change a bounded user setting | core/config |
| `/init [path]` | initialize a project blueprint/goal context without a model call | first-class |
| `/plan [instruction]` | propose a plan that can be saved as a Blueprint | first-class |
| `/blueprint show\|status\|validate\|put\|run` | inspect, persist, or execute a versioned DAG | first-class |
| `/goal put\|show\|run\|pause\|resume\|cancel` | inspect/persist or manage one durable goal | first-class |
| `/learn put\|show\|resume\|evidence` | inspect/persist or manage a source-to-target learn task | first-class |
| `/compete submit\|status` | hand a bounded proposal request to the runtime | runtime call |
| `/loop start\|status\|stop` | hand a bounded continuation request to the runtime | runtime call |
| `/session list\|open\|fork\|export\|import\|gc` | navigate durable sessions | core |
| `/resume [sequence]` | replay a bounded event suffix or recover a turn | core |
| `/diff [path]` | inspect pending file changes and bounded hunks | review |
| `/attach [path]` | add a bounded workspace attachment to the next turn | core/provider |
| `/permissions show\|set` | inspect or change explicit side-effect policy | approval |
| `/compact` | compact context with a durable marker | core |
| `/approve [id] once\|always\|deny` | answer a pending side-effect request | approval |
| `/cancel` | cancel the active turn/tool/goal operation | core |
| `/layout [show\|preset\|reset\|save] [tab]` | inspect, select/reset, or persist a BentoBox preset | TUI owner (headless returns an explicit owner-required response) |
| `/pane [show\|focus\|collapse\|expand\|toggle] [name]` | inspect, focus, or change pane visibility | TUI owner (headless returns an explicit owner-required response) |
| `/clear`, `/quit` | clear view or close cleanly | TUI |

Unknown commands, ambiguous arguments, and shell-looking payloads fail before
mutating the journal. `/compete` and `/loop` never imply that zenpi itself has
started a scheduler.

## 5. BentoBox workspace contract

The upper tab bar is a named workspace selector. Each tab now owns an
independent preset, focus, collapsed-pane state, and bounded split ratios in
memory. The production TUI restores and atomically saves those values under
the active profile in bounded `layout.json`; a malformed snapshot fails closed
to built-in presets and is reported visibly. The layout model is data
(`TabId`, `PaneId`, `Split`, `Visibility`, `min_width`, `min_height`), so it can
be tested without a terminal and later consumed by a GUI.

### 5.1 Pane vocabulary

| Pane | Purpose | Default capability |
|---|---|---|
| `project_conversation` | project-level conversation and current context | always on |
| `resources` | workspace files, changed paths, and bounded CPU/memory/disk signals | always on |
| `goal_conversation` | goal-specific prompts, approvals, and execution notes | always on when a goal is active |
| `gantt` | Markdown-rendered blueprint DAG/progress board | always on for blueprint/goal tabs |
| `browser` | optional bounded external web/snapshot view | off unless an adapter is enabled |
| `terminal` | optional approved PTY/command view, with the quality bar of a modern Herd-like terminal workflow | off unless requested and approved |

### 5.2 Presets for each upper tab

Ratios are the target starting values, not pixel promises. The pure
`src/layout.rs` model records them and safely computes geometry, and the
production async TUI consumes it through `BentoBoxLayoutAdapter`. The current
adapter renders the tab bar, conversation pane, a live bounded Resources
projection, and a bounded Blueprint/Goal summary in the Gantt pane; keyboard
focus, bounded ratio editing, per-tab state, automatic profile persistence, and
keyboard reset are now wired. Resources and domain summaries refresh through
separate single-slot workers and retain the last good snapshot, but they are not
periodically polled. The Gantt content is still a summary rather than item
progress/evidence, and real browser/terminal content remains deferred. The smallest pane
will never be allowed below its declared minimum.

| Tab | Left column (30%) | Center (45%) | Right column (25%) |
|---|---|---|---|
| `Project` | top `project_conversation` (45%), middle `resources` (30%), bottom `goal_conversation` (25%) | `gantt` with current project blueprint and activity | top `browser` (55%), bottom `terminal` (45%), both collapsible |
| `Goal` | top `goal_conversation` (50%), middle `resources` (25%), bottom `project_conversation` (25%) | `gantt` (70%) and goal event log (30%) | `browser`/`terminal` stack, hidden when no capability |
| `Learn` | source/target conversation (45%), source tree/resources (35%), learn queue (20%) | learn mapping, Markdown blocks, and evidence progress | browser for source references, terminal for validation |
| `Review` | diff conversation (45%), checks/resources (35%), approval queue (20%) | file diff/Markdown review with foldable hunks | terminal for tests; browser optional |
| `Session` | session list (45%), selected conversation (35%), replay/goal controls (20%) | event timeline and Gantt projection | terminal/browser optional |

The target wide layout matches the requested shape: project conversation at
upper-left, resource awareness at left-middle, goal conversation at lower-left,
the Markdown Gantt board in the center, browser at upper-right, and terminal at
lower-right. The production async TUI now has the tab/adapter and keyboard
focus/split controls; the legacy synchronous `run_with_state` path remains
vertical. V2-206 completes real pane content and narrow-terminal stack behavior
instead of forcing unreadable six-way splits. V2-207 retains the PTY and
migration acceptance work for the already-wired persistence path. The typed
`/layout` and `/pane` commands now dispatch to this TUI owner; headless accepts
the grammar but returns an explicit owner-required response rather than
mutating layout state without a terminal.

### 5.3 Scaling and focus rules

* Wide (`>=160` columns): target behavior is to show the three columns and all enabled panes.
* Standard (`100-159`): keep the three columns, collapse optional browser or
  terminal panes when their minimum width would be violated.
* Compact (`80-99`): show left + center; right panes become tabs in a single
  auxiliary stack.
* Narrow (`<80`) or transient zero/one-cell resize: show one focused pane and
  preserve state; never panic, overlap text, or resize the command buffer to
  zero.
* `Ctrl-1..Ctrl-5` selects the upper tab; `Tab`/`Shift-Tab` and `Ctrl-Arrow`
  move focus; `Ctrl-Shift-Left/Right` adjusts the focused split by a bounded
  step; `Ctrl-0` resets the active layout. Automatic profile/tab save and
  restore are wired through bounded `layout.json`; the `/layout` and `/pane`
  slash parser and TUI owner are wired. Headless parses these commands but
  reports that an interactive TUI owner is required.

## 6. Rendering contract

The view model splits each message into bounded blocks before rendering:

```text
PlainText | Paragraph | Heading(level) | List | Quote | Code(language?)
Diff(path, hunks) | ToolStatus(state) | Approval(request) | Error(retryable)
```

Streaming may append to the current text/code block, but a completed block is
immutable. Code and diff blocks use a monospace style and explicit truncation
markers; simple Markdown is parsed without a browser or a full web layout
engine. Unsupported Markdown is rendered as safe plain text. Folded tool/log
groups target a one-line summary (`N calls, M succeeded, K failed`) and can be
expanded without re-reading the provider.

The target is for the same block model to be serialized in headless progress
events. Typed adapters and bounded envelopes exist, but canonical block parity
is not emitted yet; current headless clients receive the existing event schema
and can use its plain-text fields until V2-004/V2-301 are accepted.

The current headless replay cache is process-local and bounded by count and
bytes. Session-path switches clear that namespace while preserving global
sequence monotonicity; a restart therefore requires the still-planned durable
replay work rather than implying journal-backed event replay.

## 7. v2 work matrix

This is the proposed execution sequence. `Paths` are intended ownership
boundaries; a row may not silently broaden them. Every `Estimated LOC` is an
independent forecast and is `<5000`.

### A. Contracts and decisions

| ID | State | Deliverable | Paths | Depends | Gate | Estimated LOC |
|---|---|---|---|---|---|---:|
| V2-001 | AUDITED | Freeze this audit, version metadata, status vocabulary, and v1-to-v2 migration map | `Docs/Zenpi_Execution_Blueprint_v2.md` | - | Draft is explicitly non-authoritative and every gap has a row | 0 |
| V2-002 | ACCEPTED | Record dependency, binary-size, startup, RSS, queue, and feature-flag budgets; `tools/bench_runtime.py` now emits a versioned JSON receipt with explicit regression thresholds, raw cold-start samples, and per-process RSS | `Cargo.toml`, `Docs/quality/line-budget.md`, `Docs/quality/runtime-budget-v2.md`, `tools/` | V2-001 | Reproducible baseline and regression thresholds | 250 |
| V2-003 | PARTIAL | Define one lifecycle event schema for provider, tools, approvals, recovery, and terminal state; `src/view_model.rs` supplies versioned envelopes, correlation validation, and bounded buffering, and headless now emits correlated/replayable runtime admission, queue, start, and cancellation transitions, while block projection is still split between legacy wire events and host adapters | `src/protocol.rs`, `src/backend.rs`, `src/core.rs`, `src/view_model.rs`, `tests/` | V2-001 | Sequence/correlation and terminal-state invariants in one emitted schema | 900 |
| V2-004 | PARTIAL | Add the bounded block model for text, Markdown, code, diff, tool status, approval, and error; typed blocks and Markdown adapters now exist in `src/view_model.rs`, while the current renderer covers a tested TUI subset | `src/render.rs`, `src/view_model.rs`, `src/tui.rs`, `src/headless.rs`, `tests/` | V2-003 | Golden block tests and headless parity for all declared block kinds | 1100 |
| V2-005 | PARTIAL | Define typed slash-command grammar and TUI/headless parity; parser/catalogue plus TUI and JSONL `/command` dispatch slices are present, while complete command-owner parity remains planned | `src/slash.rs`, `src/protocol.rs`, `src/tui.rs`, `src/headless.rs`, `tests/` | V2-003 | Unknown/ambiguous commands fail without mutation in both transports | 1200 |
| V2-006 | PARTIAL | Extend b3 records with first-class Blueprint, Goal, Learn, and external runtime-call references; bounded data types plus a digest-checked JSONL snapshot store now exist, and read-only host projections cover part of the command surface | `src/b3.rs`, `src/domains.rs`, `src/domain_store.rs`, `src/session.rs`, `tests/` | V2-003 | Durable digest, owner, budget, lease, and evidence round trips through every owner command | 1400 |
| V2-007 | PARTIAL | Define pure BentoBox layout data, tabs, pane capabilities, minimums, and breakpoints; geometry and a production TUI adapter now exist | `src/layout.rs`, `src/tui.rs`, `tests/` | V2-004 | Layout computes safely and all interactive pane behavior is accepted | 1200 |

### B. Core terminal-agent experiences

| ID | State | Deliverable | Paths | Depends | Gate | Estimated LOC |
|---|---|---|---|---|---|---:|
| V2-101 | PARTIAL | Multiline editor with newline key, UTF-8 cursor, line navigation, paste, history, and bounded wrapping; the installed-binary PTY smoke now submits one exact bracketed paste | `src/tui.rs`, `tests/tui_resize.rs`, `tools/user_smoke.py` | V2-004 | Shift-Enter/Ctrl-J inserts; Enter and bracketed paste submit exact text; multiline history round trips | 1400 |
| V2-102 | PARTIAL | Stream event backpressure and live rendering without unbounded provider mailbox growth; headless provider/agent mailboxes and the TUI provider mailbox are bounded by both event count and serialized bytes. Headless emits dropped count/bytes before the terminal response and has a real blocked-output flood test; TUI shows an explicit truncation marker, with focused tests for UTF-8 accounting, zero/oversized limits, and accounting reset | `src/runtime.rs`, `src/backend.rs`, `src/tui.rs`, `src/headless.rs`, `tests/headless_event_budget.rs`, `tests/` | V2-003 | Slow consumers stay byte-bounded with explicit loss; global control-input fairness, all other event/control queue budgets, and the full ordering/PTY matrix remain | 1800 |
| V2-103 | PARTIAL | Tool lifecycle events and status rows for queued, running, success, failure, cancellation, and output truncation; `ViewEventKind::ToolStarted/ToolFinished` and conversion helpers now exist, but hosts still emit legacy agent/provider envelopes | `src/core.rs`, `src/backend.rs`, `src/view_model.rs`, `src/tui.rs`, `src/headless.rs`, `tests/` | V2-003 | Same call ID/status sequence in TUI and JSONL | 1800 |
| V2-104 | PARTIAL | Inline approval focus, multiple pending requests, timeout, deny, and no-side-effect proof; TUI/headless decisions reach the first-response-wins coordinator, write/edit approvals expose bounded diffs, approved source digests are revalidated before execution, and metadata-only audit receipts precede side effects | `src/approval.rs`, `src/core.rs`, `src/tui.rs`, `src/headless.rs`, `tests/approval_owner.rs` | V2-005,V2-103 | Focused denial/allow/stale-preview and installed headless tool-owner tests pass; multi-request, timeout, PTY, and full headless matrix remain | 1200 |
| V2-105 | PARTIAL | User-visible retry, resume, interrupted-operation markers, and durable recovery guidance; `/resume [sequence]` pages bounded validated journal records with a continuation cursor and marker, `/compact` persists/restores deterministic provider context, and session switching now opens only a clean existing zenpi journal without mutating rejected paths. Recovery advances sequence state only for accepted records, rejects exhaustion before append, and refuses startup journals over the 256 MiB byte cap before permission changes | `src/context.rs`, `src/core.rs`, `src/session.rs`, `src/headless.rs`, `src/tui.rs`, `tests/resume_compact_owner.rs`, `tests/session_recovery.rs` | V2-003,V2-102 | Restart replay/compaction, startup-cap refusal, and adversarial sequence/path tests pass; interrupted side-effect retry, large-valid-journal streaming, multi-writer serialization, and kill/restart acceptance remain | 2000 |
| V2-106 | PARTIAL | Bounded unified diff is emitted before write/edit approval and returned after execution; unreadable sources fail closed and digest-bound revalidation rejects a stale approved preview without writing. Workspace `/diff` covers tracked/untracked/no-HEAD inspection | `src/tools.rs`, `src/slash_actions.rs`, `src/tui.rs`, `src/headless.rs`, `tests/` | V2-004,V2-104 | Add atomic-write failure acceptance, shared Diff-block TUI rendering, redaction policy, and full allow/deny PTY matrix | 1300 |
| V2-107 | PARTIAL | Foldable tool/log groups with counts, error summaries, keyboard toggle, and preference | `src/tui.rs`, `src/render.rs`, `tests/` | V2-004,V2-103 | Fold/unfold is deterministic and never hides a pending approval/error | 900 |
| V2-108 | PARTIAL | In-workspace session browser, history search, replay suffix, fork/open, and goal/session navigation; slash list/open, bounded durable sequence replay, TUI/headless fork/export/import, and explicitly confirmed bounded GC owners are wired without provider work. Maintenance requires an explicit clean existing source plus a non-existing, non-symlink destination and never overwrites the target. GC requires both retention dimensions and `--yes`, excludes the active journal, skips domain/unowned data, and caps scan/removal/receipt sizes. Startup recovery eagerly loads a valid journal but rejects files over the 256 MiB cap before parsing/permission changes | `src/session.rs`, `src/tui.rs`, `src/slash.rs`, `src/headless.rs`, `tests/` | V2-005,V2-006 | Restarted-process bounded replay, startup-cap refusal, GC active/unowned refusal, and maintenance refusal paths are covered; large-valid-journal streaming/indexing, the workspace browser/search, and navigation UX remain | 1600 |
| V2-109 | PARTIAL | Atomic cancel/steer admission, Responses SSE body-read cancellation polling, socket/read boundary policy, child reaping, joined shutdown, and close-hook coverage | `src/runtime.rs`, `src/backend.rs`, `src/core.rs`, `src/headless.rs`, `src/tui.rs`, `tests/` | V2-102,V2-105 | Focused stalled-Responses-body cancellation has no duplicate request and closes the socket within the bounded poll test; Chat/non-streaming and DNS/connect/send/header phases still lack external abort, while owned-host close hooks and the full PTY/headless matrix remain open | 2400 |
| V2-110 | PARTIAL | Responsive BentoBox resize, keyboard focus navigation, bounded split-ratio adjustment/reset, collapse, terminal restoration, and profile/tab layout restore/save; pure layout APIs plus TUI keyboard bindings now exist | `src/layout.rs`, `src/tui.rs`, `tests/tui_resize.rs`, `tests/tui_bentobox.rs` | V2-007,V2-109 | Wide/standard/compact/narrow/zero-cell PTY and mock-terminal matrix, including persisted focus/ratios | 1800 |
| V2-111 | PARTIAL | Deterministic Markdown/code renderer with plain fallback; current TUI integration covers Assistant/System text and needs diff/streaming blocks | `src/render.rs`, `src/tui.rs`, `tests/` | V2-004,V2-110 | Golden snapshots contain no overlap, unsafe escapes, or broken wide glyphs | 2400 |

### C. b3ehive domains, commands, and workspace panes

| ID | State | Deliverable | Paths | Depends | Gate | Estimated LOC |
|---|---|---|---|---|---|---:|
| V2-201 | PARTIAL | Durable Blueprint store, digest, DAG validation, read-only `/blueprint show`/`status`/`validate`, explicit `/blueprint put <json-path>` persistence, and a bounded local `/blueprint run ID[@VERSION]` owner now exist. The owner selects one dependency-ready item, writes a private receipt snapshot with running/terminal state, enforces the linked Goal budget, and resumes a running receipt without allocating a duplicate attempt; TUI and headless share the adapter and never call the provider for this local evidence operation. Production TUI still projects a 32 KiB/96-row Blueprint/Goal summary through an independent single-slot worker, retains the last good result, and rejects stale results after a session switch. This is not yet item-progress/evidence Gantt semantics or a general worker scheduler | `src/b3.rs`, `src/domain_execution.rs`, `src/domains.rs`, `src/domain_store.rs`, `src/session.rs`, `src/slash.rs`, `src/headless.rs`, `src/tui.rs`, `tests/domain_execution_owner.rs`, `tests/domain_execution_host.rs` | V2-005,V2-006 | Duplicate/cycle/missing dependency, budget/cancel/restart receipt, and shared host-owner tests pass; multi-item worker parallelism, Goal cancellation, and external handoff remain | 2400 |
| V2-202 | PARTIAL | Durable Goal records are linked to immutable Blueprint digests with bounded status transitions; headless and TUI `/goal show`/`list`/`status`/`transition` inspect and durably transition goals, and `/goal put <json-path>` persists a validated record; natural-language create/run/resume/cancel owner commands remain open | `src/b3.rs`, `src/domains.rs`, `src/domain_store.rs`, `src/session.rs`, `src/core.rs`, `src/tui.rs`, `src/headless.rs`, `tests/` | V2-006,V2-201 | Goal status transitions and restart recovery are typed and idempotent through the command owner | 1600 |
| V2-203 | PARTIAL | Bounded Learn records, `/learn show`, and explicit `/learn put <json-path>` persistence now exist. `/learn evidence <id> <repo-relative-ref>` validates and hashes a bounded local artifact, persists only an idempotent reference, and returns a receipt in both hosts; `/learn resume <id>` validates and exposes a durable read-only checkpoint with `zenpi_started: false`, `execution_state: untracked`, and `external_owner_required` rather than pretending to run a worker | `src/b3.rs`, `src/domains.rs`, `src/domain_store.rs`, `src/session.rs`, `src/slash.rs`, `src/headless.rs`, `tests/learn_owner.rs` | V2-006,V2-202 | Add actual source-to-target worker resume, mapping/result handoff, and external execution lifecycle while preserving bounded traceability | 1900 |
| V2-204 | PARTIAL | `/compete` and `/loop` now create bounded typed route/envelope/optional-parent-lease intents, recover them from the session journal, and expose submit/status in TUI and headless. Headless source request ID plus payload fingerprint provide cross-restart replay/no-second-append and conflict detection. Responses explicitly say `route: runtime_intent`, `delivery: journal_only`, `zenpi_started: false`, and `execution_state: untracked`; this is not external runtime execution | `src/b3.rs`, `src/runtime_intent.rs`, `src/session.rs`, `src/headless.rs`, `src/tui.rs`, `tests/runtime_intent_owner.rs` | V2-006,V2-202 | Add external delivery/claim/acknowledgement, result-manifest import, multi-writer journal serialization, and lifecycle evidence without a hidden scheduler or nested agent | 1600 |
| V2-205 | PARTIAL | Complete the slash-command dispatcher, completion/help metadata, aliases, and command events; TUI/headless owner paths now include goal status/show, bounded diff, attachment staging, domain read/put plus Learn evidence/checkpoint inspection, session list/open/fork/export/import and explicitly confirmed bounded GC, durable resume/compact, real approval decisions, cancel, redacted `/models` and `/doctor`, and journal-only compete/loop intent submit/status | `src/slash.rs`, `src/core.rs`, `src/config.rs`, `src/tui.rs`, `src/headless.rs`, `tests/` | V2-005,V2-201,V2-203 | Remaining plan/domain execution and external runtime lifecycle actions need success/error/abort parity | 1800 |
| V2-206 | PARTIAL | Add the Project, Goal, Learn, Review, and Session tabs with the requested six-pane BentoBox presets; production async TUI tab/adapter, keyboard focus, bounded split controls, independent state, a live bounded Resources pane, and a 32 KiB/96-row Blueprint/Goal summary pane are present. Separate single-slot workers keep refreshes off the renderer, retain last-good data, and ignore stale domain results after session changes | `src/layout.rs`, `src/tui.rs`, `tests/` | V2-007,V2-110,V2-201 | Preset snapshots match ratios, pane ownership, capability visibility, and focus; periodic polling, true progress-Gantt, Learn/Review/Session domain content, and browser/PTY content stay open | 2800 |
| V2-207 | PARTIAL | Persist per-profile/tab pane ratios, collapsed state, focus, and reset/migration through bounded `layout.json`; production TUI restores on startup and saves dirty mutations atomically | `src/layout.rs`, `src/config.rs`, `src/tui.rs`, `tests/` | V2-206 | Corrupt/out-of-range layout fails closed and leaves a valid prior config; full PTY persistence proof remains open | 1500 |
| V2-208 | PARTIAL | Bounded workspace/resource collector, headless `resources`, and a live TUI Resources pane now exist; startup and `/resources` refreshes run through an independent single-slot bounded worker and retain the last good snapshot | `src/resources.rs`, `src/headless.rs`, `src/tui.rs`, `tests/tui_bentobox.rs` | V2-206,V2-207 | Non-blocking refresh and rendering are covered; periodic polling and portable disk metrics remain open | 1800 |
| V2-209 | DEFERRED | Optional browser capability pane via an external, bounded snapshot adapter | `src/adapters/browser.rs`, feature docs, `tests/` | V2-206 | Feature is absent from default build; no browser process or credential leak by default | 2200 |
| V2-210 | DEFERRED | Optional PTY terminal pane with explicit child ownership and approval | `src/adapters/pty.rs`, feature docs, `tests/` | V2-104,V2-206 | Feature is absent from default build; child, resize, signal, and cleanup tests pass when enabled | 2200 |

### D. Headless parity, quality, and release gates

| ID | State | Deliverable | Paths | Depends | Gate | Estimated LOC |
|---|---|---|---|---|---|---:|
| V2-301 | PARTIAL | Expose the currently declared events/commands over strict bounded JSONL while retaining stdout protocol-only; runtime admission, queue, start, and cancellation transitions are correlated, monotonic, and replayable, and `view_model` adapters/resources are available, but canonical block parity is not yet emitted | `src/protocol.rs`, `src/headless.rs`, `src/view_model.rs`, `tests/headless_protocol.rs`, `tests/` | V2-003,V2-005,V2-103 | Split/overlong/malformed frames, replay gaps, duplicate IDs, EOF, and cancellation matrix; canonical block/event parity remains open | 2200 |
| V2-302 | PARTIAL | Benchmark queue memory, stream latency, startup/RSS, render frames, and layout computation; the checked-in probe measures locked release size, cold headless exit/RSS for a new/empty session, 2,000 runtime round trips, 500 TestBackend renders, 60,000 layout computations, and deterministic render coalescing with raw samples and no p95 claim | `tools/bench_runtime.py`, `tools/runtime_budget_probe.rs`, `Docs/quality/runtime-budget-v2.md` | V2-002,V2-102,V2-110 | Current host passes repeatable limits; near-256 MiB journal startup/RSS, stream first-paint/end-to-end latency, and direct queue-memory measurement remain open | 700 |
| V2-303 | PARTIAL | Apply the Tokio/reqwest/clap/SQLite decision gates using measured evidence; current evidence keeps all four deferred/rejected at their ownership boundary, with explicit reopen requirements, while the incomplete V2-302 stream/queue-memory evidence prevents final acceptance | `Docs/quality/technology-decision-v2.md`, `tools/` | V2-002,V2-302 | Any migration is all-at-once at its ownership boundary; rejected additions stay absent | 300 |
| V2-304 | PARTIAL | Re-audit secrets, paths, approvals, diff content, external adapters, and redacted diagnostics; session symlink/O_NOFOLLOW, existing-only resume/session opens, over-limit journal refusal before permission mutation, headless status/session path redaction, and sibling domain-store listing negatives are now covered | `src/security.rs`, `src/config.rs`, `src/approval.rs`, `src/b3.rs`, `src/session.rs`, `src/headless.rs`, `tests/` | V2-103,V2-104,V2-106 | Negative tests prove no credential, absolute path, shell escape, session-link mutation, missing-resume creation, oversized-journal mutation, domain-store corruption, or hidden side effect | 1300 |
| V2-305 | PARTIAL | Run the ten-experience acceptance matrix in both TUI PTY and headless fixture lanes; user smoke installs a production no-fixture binary and, against local OpenAI-compatible Responses fixtures, proves Responses SSE, slow-provider EOF drain, bounded pre-write diff, approval, actual workspace write, provider continuation, and durable tool result. Its feature install covers TUI resize, exact multiline paste, stream interruption/restoration, and selected `/models`, `/doctor`, `/help`, `/diff`, `/attach`, `/compact`, `/resume`, `/compete`, and session-reopen paths | `tests/`, `tools/user_smoke.py`, `tools/headless_smoke.sh`, `Docs/quality/` | V2-101,V2-102,V2-103,V2-104,V2-105,V2-106,V2-107,V2-108,V2-109,V2-110,V2-111,V2-301 | Complete remaining rows, especially cursor/history/paste-limit/narrow wrapping, PTY approval/diff, atomic failure, history UI, socket-level cancel outside Responses body reads, and all resize classes; no compile-only acceptance | 1500 |
| V2-306 | PARTIAL | Reconcile README/spec/Gantt, version notes, release archive, SBOM, and size receipt | `README.md`, `Docs/Zenpi_Execution_Spec.md`, `Docs/Zenpi_Execution_Gantt.md`, `Docs/quality/` | V2-303,V2-305 | Docs never call partial/deferred work complete; release reproduces the recorded size | 500 |

### E. Explicitly future GUI boundary

| ID | State | Deliverable | Paths | Depends | Gate | Estimated LOC |
|---|---|---|---|---|---|---:|
| V2-401 | DEFERRED | Publish a stable core/headless/layout protocol that a future GUI can consume | `Docs/Zenpi_GUI_Future_Contract.md` | V2-003,V2-007 | Contract names macOS, Linux, and Windows without adding GUI dependencies now | 0 |
| V2-402 | DEFERRED | Future macOS GUI workspace (Svelte-quality interaction target) | `Docs/Zenpi_GUI_Future_Contract.md` | V2-401 | Requires a separately approved blueprint/version and platform acceptance matrix | 0 |
| V2-403 | DEFERRED | Future Linux/Windows GUI workspace (same shared protocol) | `Docs/Zenpi_GUI_Future_Contract.md` | V2-401 | Requires a separately approved blueprint/version and platform acceptance matrix | 0 |
| V2-999 | PLANNED | Master acceptance, v1 archive/migration, Gantt regeneration, and final user receipt | `Docs/`, `tools/`, `.github/` | V2-001,V2-002,V2-003,V2-004,V2-005,V2-006,V2-007,V2-101,V2-102,V2-103,V2-104,V2-105,V2-106,V2-107,V2-108,V2-109,V2-110,V2-111,V2-201,V2-202,V2-203,V2-204,V2-205,V2-206,V2-207,V2-208,V2-301,V2-302,V2-303,V2-304,V2-305,V2-306 | All required rows `ACCEPTED`, zero unresolved partials, clean gates, and size receipt; deferred adapter/GUI rows are not v2.0 blockers | 0 |

## 8. Acceptance matrix for the ten core experiences

The installed smoke now includes the local Blueprint owner path: it installs
the no-fixture binary, persists a valid Blueprint and linked Goal, runs two
dependency-ordered items across two processes, verifies private receipt
recovery and Goal completion, and asserts that no provider request occurred.
This executable evidence supersedes the older shorthand in the V2-205/V2-305
row text; the remaining gaps listed in those rows are still intentionally open.

The installed production smoke now covers `/blueprint run` as well as session
GC: it persists a valid Blueprint and linked Goal, executes two dependency-
ordered items across two processes, verifies private receipt recovery and Goal
completion, and asserts that no provider request occurred. The following tests
are mandatory evidence, not illustrative examples:

| Experience | Positive proof | Failure/edge proof |
|---|---|---|
| Multiline input | Type two lines with Shift-Enter, move the cursor across a UTF-8 boundary, submit one exact message; installed PTY smoke submits an exact bracketed paste | Paste at the byte limit, empty lines, history recall, narrow wrapping, and 1-cell prompt |
| Streaming reply | Delayed fixture emits several deltas; TUI paints each before completion and headless emits ordered events | Slow consumer/backpressure, malformed SSE, dropped stream, refusal, and no synthesized success |
| Tool status | One read, one write, and one command expose stable started/result states | Tool failure, cancellation, output truncation, duplicate call ID, and queue depth |
| Confirmation | TUI and headless allow a permitted write/command after explicit approval | Deny, timeout, cancel, two pending approvals, and proof the handler was never called |
| Recovery | Kill/restart after a durable operation marker and resume/retry once; reject an oversized startup journal before parsing or permission changes | Truncated tail, malformed record, stale active marker, retryable provider error, duplicate request, and near-cap valid-journal memory behavior |
| File diff | Preview and final result contain bounded unified hunks and changed paths | New file, deletion-like replacement, large truncation, NUL/secret redaction, atomic write failure |
| Log folding | Toggle a tool group and retain a count/status summary | Pending approval/error cannot be hidden; fold state survives resize and tab switch |
| Session history | Open/replay/fork a session from the Session tab and headless command | Corrupt prefix, old sequence, missing file, and no provider call during replay |
| Cancel/interrupt | Ctrl-C or `/cancel` stops a delayed provider/tool and returns to idle; a stalled Responses SSE body observes cancellation at the bounded poll interval | Half-line read, retry backoff, live-steer race, Chat/non-streaming or pre-body socket phases, child reaping, shutdown, and panic cleanup |
| Resize | Exercise wide, standard, compact, narrow, 1x1, and transient zero-cell layouts | No overlap, panic, lost input, stale cursor, or terminal raw-mode/alternate-screen leak |

Additional acceptance is required for Markdown block rendering, every slash
command, all five tab presets, resource polling bounds, b3 digest/lease rules,
near-cap valid-journal startup/RSS, and the remaining provider cancellation
boundaries. The default-build dependency/size gate is recorded separately in
V2-002 and its host-specific receipt.

## 9. Versioning and migration policy

* `2.0.0` is a product-contract revision, not a claim that the rows are done.
  Additive v2 fixes use `2.0.x`; additive compatible features use `2.1.y`;
  protocol, event, or layout incompatibilities require `3.0.0`.
* Until `V2-999` is accepted, the v1 file and v1 Gantt remain authoritative
  only for the historical v1 receipt and its validator. A worker must not mark
  a v1 row complete to imply a v2 row is complete.
* On acceptance, the Master will archive the v1 document, migrate stable IDs
  with an explicit table, update the Spec and README, regenerate the same-name
  Gantt, and run all structural and executable gates. A hand-edited Gantt or a
  compile-only receipt is not a migration.
* GUI, browser, PTY, Tokio/reqwest, clap, and SQLite work may not be smuggled
  into v2.0 by changing a dependency or adding an unlisted mode. Each requires
  a row, an estimate, a gate, and (for a contract change) a new version.

## 10. Open decisions for the Master review

1. Keep the lightweight synchronous provider boundary for v2.0 while retaining
   bounded queues, joined shutdown, steer admission, and Responses-body
   cancellation polling, or approve a complete Tokio+reqwest migration if
   external abort is required for Chat/non-streaming or pre-body socket phases.
2. Keep the hand-written parser plus typed slash registry until command
   completion/help tests demonstrate a real need for clap.
3. Keep JSONL as the source of truth; add SQLite only as an optional derived
   index after a measured history workload.
4. Keep browser and PTY panes disabled by default and model them as external
   adapters, rather than making the lightweight binary a desktop shell.
5. Accept `blueprint`, `goal`, and `learn` as first-class local domains now;
   route `compete` and `loop` through b3ehive runtime handoffs with explicit
   evidence instead of implementing a second scheduler inside zenpi.

The recommended answer to "should we discuss and update the blueprint again?"
is therefore **yes**: accept this v2 contract and audit first, then execute the
rows in dependency order. Do not call the current v1 checkmarks proof that the
ten experiences or the BentoBox/b3ehive product surface already exists.
