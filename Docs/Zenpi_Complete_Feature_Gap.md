# zenpi Complete Feature Gap and Delivery Blueprint

> **Archived planning input.** The `CF-*` scope in this document was promoted
> on 2026-09-04 into the sole authoritative checklist,
> `Docs/Zenpi_Execution_Blueprint.md`. Status marks here are intentionally
> absent; use the Blueprint and its generated Gantt for current state.

## Why this gap exists

The baseline binary (before the complete-feature work described below) is
useful for protocol wiring, but its default behavior is misleading for an end
user: `echo` is selected by default and returns the latest user text unchanged.
That is a test double, not an AI framework.  The baseline OpenAI-compatible
adapter sends one blocking, non-streaming Chat Completions request.  It has no
provider profile directory or Codex credential import, no Responses API event
stream, no tool execution loop, no cancellation while a request is in flight,
no context compaction, no skills/extensions, and no packaged release artifact.

Any foundational config, Responses, or background-runner code landed while this
proposal is being implemented is still only partial until the end-to-end
scenarios and every corresponding `CF-*` acceptance test pass.

The target of this proposal is **complete feature behavior**, not merely a
successful compile or a green unit-test subset.  “Done” means a fresh user can
install zenpi, import the provider already configured for Codex, send a real
request, observe streamed events, approve or reject tools, resume the session,
and use the same behavior from both TUI and headless mode.

## Non-negotiable product decisions

1. **No silent mock default.**  `echo` is removed from the default path.  A
   provider must be selected from CLI, environment, or configuration.  If no
   usable provider is found, zenpi exits before creating a session and prints a
   redacted, actionable configuration error.  `--backend echo` remains only as
   an explicitly named test fixture (and is disabled in release builds unless
   the operator enables a `dev-fixtures` feature).
2. **Codex-compatible configuration first.**  On first run, zenpi reads
   `~/.zenpi/config.toml` and `~/.zenpi/auth.json`.  When no zenpi profile is
   present, it can import the existing `~/.codex/config.toml` and
   `~/.codex/auth.json` without copying secret material into the repository or
   printing it.  The observed Codex shape is `[model_providers.OpenAI]`
   (`base_url`, `wire_api = "responses"`) and an `OPENAI_API_KEY` field in
   `auth.json`; the importer must also tolerate equivalent provider/key names.
3. **Responses API is the primary wire contract.**  The OpenAI Responses API
   (`/v1/responses`) is the first-class adapter and event model.  Chat
   Completions is a compatibility adapter with an explicit capability warning,
   not the feature ceiling.
4. **One asynchronous runtime, one event model.**  TUI and headless share an
   async core, cancellation token, event bus, persistence writer, provider
   client, tool registry, and context manager.  No mode is allowed to block
   reading input while a provider or tool is running.
5. **Tools are real, bounded capabilities.**  Tool schemas, argument
   validation, workspace/shell sandboxes, approval policy, process limits,
   cancellation, output truncation, and durable tool events are required.
6. **Credentials are local and fail closed.**  `~/.zenpi/auth.json` is
   created with mode `0600` on Unix, never committed, never logged, and never
   included in a handoff, crash report, or prompt transcript.

## Fastest path for the existing machine

After implementation, the shortest supported setup is expected to be:

```text
zenpi config import-codex
zenpi config doctor
zenpi --mode tui
```

The import command reads the existing Codex provider URL and key, writes a
redacted zenpi profile plus a private auth file, and reports only the selected
provider name, URL host, API family, and model.  Equivalent non-mutating
environment overrides remain available:

```text
ZENPI_BASE_URL=... ZENPI_API_KEY=... ZENPI_MODEL=... zenpi --mode tui
```

Configuration precedence is **CLI > `ZENPI_*` environment > `~/.zenpi` >
imported `~/.codex` > provider defaults**.  The precedence and the exact
effective (redacted) profile are printed by `zenpi config doctor`.

The intended file shape is deliberately boring and easy to edit by hand:

```toml
# ~/.zenpi/config.toml
default_profile = "codex"

[profiles.codex]
base_url = "https://api.example.com/v1"
wire_api = "responses"
model = "gpt-5.6-sol"
timeout_seconds = 120

# ~/.zenpi/auth.json (mode 0600; never commit this file)
# {"profiles":{"codex":{"api_key":"REDACTED"}}}
```

For the existing machine, users should not hand-copy the key.  The importer
reads the URL/model/API family from `~/.codex/config.toml` and the
`OPENAI_API_KEY` value from `~/.codex/auth.json`, then writes the equivalent
private profile.  If a user really needs an environment-only setup, the
minimum is `ZENPI_BASE_URL`, `ZENPI_API_KEY`, and `ZENPI_MODEL`; the resulting
request still goes through the Responses adapter rather than a mock.

## Proposed delivery items

Each item below is a bounded future checklist row.  `Estimated LOC` is a
forecast of implementation and test code attributable to that item only;
documentation and generated files count as zero.  Every estimate is a strict
integer `<5000`, and estimates are not an aggregate project limit.

### F0 — Contract and migration guardrails

| ID | Deliverable | Depends | Acceptance tests | Estimated LOC |
|---|---|---|---|---:|
| CF-001 | Freeze the complete feature contract, protocol versioning policy, capability negotiation, and migration/deprecation schedule. | — | Contract review confirms no silent mock, Responses-first semantics, async input, tool approval, and config precedence; schema fixtures are versioned. | 0 |
| CF-002 | Add a compatibility matrix for OpenAI Responses, Chat Completions, and local OpenAI-compatible gateways. | CF-001 | Matrix tests select the correct adapter from `wire_api`; unsupported capability produces a typed error rather than a fallback to echo. | 350 |
| CF-003 | Introduce release/build profiles separating production code from `dev-fixtures` and test-only echo. | CF-001 | A production build rejects implicit echo; an explicit test build can run echo; binary inspection shows no test fixture enabled by default. | 450 |

### F1 — `~/.zenpi` configuration and Codex pairing

| ID | Deliverable | Depends | Acceptance tests | Estimated LOC |
|---|---|---|---|---:|
| CF-101 | Implement XDG-aware paths with `~/.zenpi/config.toml`, `~/.zenpi/auth.json`, `~/.zenpi/sessions/`, `~/.zenpi/skills/`, and `~/.zenpi/extensions/`, with an explicit `ZENPI_HOME` override. | CF-001 | Fresh-home test creates only the required directories; Unix auth/session files are `0600`; Windows ACL handling is covered; symlink and path traversal cases fail closed. | 900 |
| CF-102 | Define provider profiles (`name`, `base_url`, `wire_api`, `model`, `organization`, timeout/retry policy, capabilities) and redacted config diagnostics. | CF-101,CF-002 | TOML round-trip, unknown-field policy, URL normalization, host-only display, and invalid profile tests; no key appears in `Debug`, errors, or doctor output. | 1000 |
| CF-103 | Implement `zenpi config import-codex` for `~/.codex/config.toml` and `~/.codex/auth.json`. | CF-101,CF-102 | Fixture import reads `[model_providers.OpenAI].base_url`, `wire_api`, model and `OPENAI_API_KEY`; missing/invalid files give actionable errors; source files are never modified; imported key is only written to `~/.zenpi/auth.json`. | 1200 |
| CF-104 | Implement `zenpi config doctor`, `config list`, `config use`, and non-interactive profile selection. | CF-102,CF-103 | Doctor prints effective precedence, API family, model, and endpoint host with key redacted; `--json` output is schema-valid and secret-free; commands return non-zero on missing required fields. | 850 |
| CF-105 | Replace implicit echo selection with provider resolution and an explicit test-fixture gate. | CF-003,CF-102 | `zenpi --mode headless` with no provider fails before session creation; imported Codex profile sends a real HTTP request; `--backend echo` is rejected in production unless the fixture gate is enabled; no response is fabricated. | 700 |
| CF-106 | Add a `zenpi pair` compatibility helper modeled on the existing Codex pairing workflow. | CF-103,CF-104 | `pair status` reports provider/auth readiness without revealing secrets; `pair import-codex` is idempotent; `pair revoke` removes only zenpi-owned credentials after confirmation. | 650 |

### F2 — Provider clients and Responses API

| ID | Deliverable | Depends | Acceptance tests | Estimated LOC |
|---|---|---|---|---:|
| CF-201 | Build a provider capability registry and request model for text, images, tools, structured output, streaming, and reasoning controls. | CF-002,CF-102 | Capability negotiation tests cover OpenAI Responses, Chat Completions, and a minimal local gateway; unsupported fields are rejected or downgraded only under an explicit policy. | 950 |
| CF-202 | Implement the OpenAI Responses API request/response adapter, including `input`, `instructions`, model, tools, reasoning, metadata, and response IDs. | CF-201 | Local HTTP fixture asserts the configured Responses path (`/responses` or `/v1/responses`, without silently rewriting a gateway path), payload, auth, model, and correlation ID; response text, refusal, annotations, usage, and provider request ID persist correctly. | 2200 |
| CF-203 | Implement SSE/HTTP streaming with typed `response.created`, output text deltas, tool-call deltas, completion, refusal, and error events. | CF-202 | Chunk boundaries can split UTF-8 and JSON; all events are reassembled losslessly; a dropped connection emits a terminal error and leaves a resumable journal prefix; no duplicate final assistant message. | 2200 |
| CF-204 | Keep a Chat Completions compatibility adapter for gateways that advertise only that API. | CF-201 | Fixture tests map messages/tools/usage to the common event model; capability warning is surfaced once; adapter never silently changes a Responses profile. | 1500 |
| CF-205 | Add bounded retries, exponential backoff, `Retry-After`, idempotency keys, request timeout, and circuit-breaker policy. | CF-202,CF-203 | 408/409/425/429/5xx retry according to policy; 4xx/configuration errors do not retry; cancellation interrupts backoff; one logical turn has one durable idempotency key. | 1400 |
| CF-206 | Support multimodal input and attachment upload/reference policy. | CF-202,CF-203 | Image/file fixtures enforce MIME, size, and workspace policy; binary data is never placed in JSONL transcript unboundedly; unsupported provider capability returns a typed error. | 1800 |

### F3 — Async core, event bus, and interactive control

| ID | Deliverable | Depends | Acceptance tests | Estimated LOC |
|---|---|---|---|---:|
| CF-301 | Move the backend boundary and turn state machine onto one approved async runtime. | CF-201,CF-203 | `cargo test` uses deterministic paused time; provider, tool, and persistence tasks are owned and joined; no blocking call runs on the async executor; shutdown drains tasks. | 2200 |
| CF-302 | Add a typed event bus for lifecycle, token delta, tool request, tool result, usage, warning, error, and terminal events. | CF-301 | Subscribers receive ordered events with a monotonic sequence; slow consumers are bounded; terminal event is emitted exactly once; headless and TUI snapshots agree. | 1400 |
| CF-303 | Implement cancellation tokens and interrupt propagation through provider streaming, retries, tools, persistence, and UI. | CF-301,CF-302 | Ctrl-C and headless `cancel` stop an in-flight request within a deadline; no assistant completion is synthesized; journal records cancellation and remains resumable; child processes are reaped. | 2200 |
| CF-304 | Implement live `steer` while a turn is running, with explicit semantics for queued, superseding, and rejected steering. | CF-302,CF-303 | A steer arriving during streaming is correlated to the active turn; provider adapters either use native continuation or cancel/reissue according to capability; stale expected IDs are rejected; no lost or duplicated user message. | 2300 |
| CF-305 | Rewrite the TUI around async event consumption, streamed transcript rendering, input editing, approval prompts, resize, reconnect, and clean terminal restoration. | CF-302,CF-303,CF-304 | PTY tests type while tokens stream, interrupt a slow fixture, approve/reject a tool, resize through zero/one-cell bounds, and verify cursor/raw/alternate-screen cleanup on every orderly exit. | 2800 |
| CF-306 | Extend headless JSONL to protocol v2 event streaming while retaining a v1 compatibility reader. | CF-302,CF-303,CF-304 | A client can send prompt, steer, cancel, status, approval response, resume, and shutdown without blocking; every event has request/turn IDs; v1 clients receive a documented compatibility projection. | 2400 |
| CF-307 | Add reconnect, resume-from-event-sequence, and duplicate-event suppression for long-lived headless clients. | CF-302,CF-306 | Client disconnect/reconnect resumes at a supplied sequence; replay is deterministic; a duplicate request ID does not execute a second provider/tool call. | 1300 |

### F4 — Tool framework and execution loop

| ID | Deliverable | Depends | Acceptance tests | Estimated LOC |
|---|---|---|---|---:|
| CF-401 | Define a versioned tool schema/registry with JSON Schema validation, capability metadata, and provider mapping. | CF-201,CF-302 | Tool names and schemas round-trip; malformed arguments are rejected before execution; only advertised tools enter a provider request; schema size/count limits are enforced. | 1200 |
| CF-402 | Implement the provider-to-tool execution loop: emit call, request approval, execute, append result, and continue until final response. | CF-401,CF-303 | A fixture produces two sequential and two parallel calls; each call has a stable ID and bounded result; loop terminates on final output, cancellation, refusal, or max-iteration policy. | 3000 |
| CF-403 | Implement workspace file tools with repository-root confinement, atomic writes, diff preview, and binary/size limits. | CF-401,CF-402 | `read`, `write`, `edit`, and `list` reject absolute paths, `..` escapes, symlink escapes, oversized files, and binary content where disallowed; crash leaves the previous file intact. | 2800 |
| CF-404 | Implement shell/process tools with explicit allow/deny policy, environment scrub, cwd confinement, timeout, output cap, and process-group cleanup. | CF-401,CF-402 | Tests cover approval deny, timeout, SIGTERM/SIGKILL escalation, descendant reaping, output truncation, secret environment removal, and non-zero exit mapping. | 3200 |
| CF-405 | Add approval policy modes: always, trusted workspace, per-tool, read-only, and headless callback. | CF-303,CF-404 | TUI displays a structured approval request; headless receives an approval event and can answer; deny never starts the process; policy decisions persist without storing credentials. | 1800 |
| CF-406 | Add tool result compaction and artifact references for large outputs. | CF-402,CF-405 | Results above the configured byte/token budget are summarized with a retrievable repository-relative artifact; provider receives the bounded representation; artifact cleanup is safe and auditable. | 1600 |
| CF-407 | Add extension/MCP-compatible tool registration behind an explicit local-process trust boundary. | CF-401,CF-405 | Extension manifests are validated, child process I/O is framed, capabilities are least-privilege, shutdown/restart is bounded, and an untrusted extension cannot read auth files. | 3000 |

### F5 — Context, session, and recovery

| ID | Deliverable | Depends | Acceptance tests | Estimated LOC |
|---|---|---|---|---:|
| CF-501 | Add token accounting and provider context-window metadata to every turn. | CF-201,CF-302 | Usage from provider is reconciled with local estimates; unknown tokenizer is marked approximate; a request cannot exceed the configured context budget without a typed decision. | 1300 |
| CF-502 | Implement deterministic context compaction with summary checkpoints and protected system/tool/user records. | CF-501,CF-406 | A long session compacts before the limit; summary digest and source sequence range persist; replay yields the same prompt; cancellation during compaction preserves the prior checkpoint. | 2400 |
| CF-503 | Implement session list, inspect, fork, export, import, and garbage-collection commands under `~/.zenpi/sessions/`. | CF-101,CF-502 | Commands never overwrite a source session; imported journals validate digest/sequence; fork has a new ID and shared immutable prefix; GC requires an explicit retention policy. | 2200 |
| CF-504 | Add crash recovery for interrupted provider streams, tool calls, and compaction. | CF-302,CF-402,CF-502 | Kill/restart fixtures recover the last durable event, mark the operation interrupted, and offer resume/retry without duplicating side effects. | 2200 |

### F6 — Skills and extensions users can actually install

| ID | Deliverable | Depends | Acceptance tests | Estimated LOC |
|---|---|---|---|---:|
| CF-601 | Define a skill manifest and loader for project-local `.zenpi/skills/` and user `~/.zenpi/skills/`. | CF-101,CF-401 | Manifest schema validates name/version/instructions/tools; precedence is project > user > built-in; path traversal and duplicate IDs fail closed; skill instructions are visible in the effective profile. | 1700 |
| CF-602 | Implement skill lifecycle hooks for prompt preparation, tool policy, context compaction, and session close. | CF-601,CF-502 | Hooks run in a documented order, have bounded time/output, cannot mutate auth, and a failing optional hook is isolated and reported. | 2200 |
| CF-603 | Define an extension manifest, version compatibility, permissions, and upgrade/disable workflow. | CF-407,CF-601 | `extension list/install/remove/disable` is deterministic; incompatible extensions are rejected before loading; disabled extensions cannot receive events. | 1900 |
| CF-604 | Add a local “pair”/profile bridge so skills and extensions can request capabilities without receiving raw provider keys. | CF-103,CF-603 | Capability broker issues redacted, scoped handles; extension logs contain no secrets; revocation invalidates outstanding handles. | 1900 |

### F7 — Security, observability, and release distribution

| ID | Deliverable | Depends | Acceptance tests | Estimated LOC |
|---|---|---|---|---:|
| CF-701 | Centralize secret loading, redaction, file permissions, and memory/log hygiene. | CF-101,CF-103,CF-404 | Secret scanner finds no test fixture key in logs, panic text, sessions, handoffs, artifacts, or diagnostics; auth files are private; redaction covers headers, URLs, JSON, and child environments. | 1800 |
| CF-702 | Add structured diagnostics, request correlation, metrics, and opt-in tracing without leaking prompt or key material. | CF-302,CF-701 | Trace fixtures correlate provider/tool/session IDs; default logs are quiet; opt-in debug output redacts secrets and truncates prompt content; metrics have bounded cardinality. | 1600 |
| CF-703 | Add resource governance: token, wall-clock, disk, process, concurrency, and network budgets. | CF-205,CF-402,CF-406 | Budget exhaustion cancels work, emits a typed terminal event, and prevents retries/tools from bypassing the limit; accounting survives resume. | 2200 |
| CF-704 | Produce reproducible release artifacts for macOS arm64/x86_64, Linux x86_64/arm64, and Windows; publish checksums and SBOM. | CF-003,CF-701 | Clean-machine install smoke runs the packaged binary without Cargo; signatures/checksums verify; artifact contains no credentials or development fixture; upgrade preserves sessions. | 2500 |
| CF-705 | Add end-to-end acceptance matrix and migration documentation for MVP users. | CF-105,CF-305,CF-306,CF-402,CF-503,CF-704 | CI covers real Responses streaming, Codex import, tool approval, cancellation, resume, TUI PTY, headless reconnect, and packaged install; docs explain echo deprecation and one-command setup. | 900 |

## Dependency DAG

The compact dependency graph is:

```text
CF-001 -> CF-002 -> CF-201 -> CF-202 -> CF-203 -> CF-301 -> CF-302
   |        |          |          |          |          |        |
   |        +-> CF-003  |          +-> CF-204  +-> CF-205  +-> CF-303 -> CF-304
   |                    |                                     |         |
   +-> CF-101 -> CF-102 -> CF-103 -> CF-104 -> CF-105          +-> CF-305  +-> CF-306 -> CF-307
                          |          |                          |
                          +-> CF-106 +-> CF-401 -> CF-402 -> CF-405 -> CF-406
                                                   |          |
                                                   +-> CF-403 +-> CF-404 -> CF-407

CF-501 -> CF-502 -> CF-503 -> CF-504
    ^         ^
    |         +---------------- CF-406
    +-------------------------- CF-201/CF-302

CF-601 -> CF-602
    |         ^
    +-> CF-603 -> CF-604

CF-701 -> CF-702
    |         ^
    +-> CF-703 -> CF-704 -> CF-705
```

The table dependencies are authoritative if this proposal is promoted into a
future execution Blueprint.  Parallel work is allowed only for disjoint owned
paths after dependencies resolve: configuration can proceed beside the
provider capability model; TUI/headless views can proceed beside tool schema
work; security and packaging remain integration gates.

## Required end-to-end acceptance scenarios

The following scenarios are deliberately broader than unit tests:

1. **Codex pairing:** with a fixture `~/.codex/config.toml` containing a
   Responses provider and a fixture `~/.codex/auth.json`, `zenpi config
   import-codex && zenpi config doctor` creates a private profile, prints a
   redacted endpoint host, and sends a real Responses request using the
   imported endpoint path (`/responses` for the native Codex gateway or
   `/v1/responses` for standard OpenAI-compatible gateways).  The
   fixture must observe the imported key, while no output or session record
   contains the key.
2. **No mock surprise:** a clean `ZENPI_HOME` with no provider fails with an
   actionable message and no session file.  The only way to use echo is an
   explicit test-fixture flag unavailable in the normal release build.
3. **Streaming and steer:** a slow Responses fixture emits deltas; TUI and
   headless both render them while still accepting `steer` and `cancel`.
   Cancellation closes the request and tool processes, and resume does not
   duplicate the assistant message.
4. **Tool loop:** a provider requests a workspace edit and a shell command.
   The user sees arguments and policy, can deny either action, and approved
   actions produce bounded, durable tool events and a final assistant response.
5. **Compaction and recovery:** a long session crosses the context budget,
   creates a digestible summary checkpoint, is killed during a stream, then is
   resumed from the last event without losing user input or repeating a side
   effect.
6. **Installable release:** a clean machine installs the packaged artifact,
   runs `zenpi config import-codex`, starts TUI/headless, and upgrades without
   losing `~/.zenpi/sessions/`.

## Promotion and completion policy

This proposal must not be “completed” by checking boxes in the existing
25-item MVP Blueprint.  Promotion requires:

- a new versioned execution specification that permits an async runtime,
  Responses events, tools, skills, extensions, and packaged releases;
- one future Blueprint row for every `CF-*` item (or a documented, reviewed
  split/merge preserving the same acceptance coverage);
- an explicit migration commit removing implicit echo from production default;
- real fixture-backed and packaged end-to-end evidence, not only compile or
  unit-test output;
- updated English, Simplified Chinese, and Japanese user documentation with
  the one-command Codex import path and provider troubleshooting;
- a release receipt containing artifact checksums, source revision, and a
  secret scan result.

Until those conditions are met, zenpi should be described as an incomplete
MVP and must not claim that the AI framework or complete feature list is
implemented.
