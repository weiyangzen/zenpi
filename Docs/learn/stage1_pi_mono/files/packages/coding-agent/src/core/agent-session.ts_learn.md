# ZS1-028 — packages/coding-agent/src/core/agent-session.ts

Source-file understanding report for SRC-0884. This report is limited to the single frozen source file; it does not accept the parent directory or Zenpi product integration.

source_id: `SRC-0884`
source_path: `packages/coding-agent/src/core/agent-session.ts`
source_hash: `17116255610ad2a3f3a7f6870a12b14b993ace65fbfa461c4a43d0ad9a013237`
source_bytes: 120916
source_lines: 3552
coverage: complete byte range [0,120916); lines 1–3552, read in order including comments, declarations, and EOF.
review_baseline: frozen pi-mono source input; no target implementation completion is claimed.

## Complete behavior review

- **Public contract and lifecycle (1–930):** `AgentSession` wraps an `Agent`, `SessionManager`, settings, resource loader, model runtime, extension runner, tool registries, queue state, retry/compaction controllers, and bash controllers. Construction subscribes to agent events, installs tool and next-turn hooks, and builds a definition-first runtime. `dispose` aborts retry/compaction/branch-summary/bash work, aborts the agent, invalidates extension context, disconnects listeners, and releases session resources. Event listeners receive agent events plus queue, compaction, retry, persistence, and bash updates.
- **Tool and prompt surfaces (939–1173):** active tools are filtered through allow/deny lists and rebuilt into agent definitions; extension tool hooks can block or replace tool results, normalize images, and preserve error/usage metadata. System prompt snippets and guidelines are normalized and rebuilt whenever the registry changes. The next-turn hook performs threshold compaction before the next assistant response and refreshes system prompt, tools, model, and thinking level from current state.
- **Prompt admission and input expansion (1175–1466):** `prompt` rejects invalid lifecycle states, dispatches extension input handlers, expands `/skill:` blocks and prompt templates when enabled, handles queued next-turn custom messages and image attachments, resolves request auth through the model runtime, and starts the agent stream. Streaming input must declare `steer` or `followUp`; steer interrupts the current turn and follow-up waits behind it. Skill expansion is source-backed and unknown commands pass through unchanged. `steer`, `followUp`, and `sendUserMessage` preserve queue ownership and image/content parts.
- **Queue, abort, and model controls (1478–1930):** extension commands are rejected from ordinary prompt paths; custom messages can target the next turn or follow-up queue. Queue clearing returns both queues without silently losing text. Abort covers agent, compaction, retry, branch summary, and bash work. Model changes authenticate before mutation, preserve per-model thinking preferences, optionally persist defaults, and scoped model cycling never escapes the configured scope. Thinking levels are clamped to model capabilities and emit change events. Steering/follow-up modes are explicit and independently configurable.
- **Manual and automatic compaction (1947–2459):** manual compaction prepares the current branch, invokes extension hooks that may cancel or replace the result, persists the compaction entry, emits start/end events, and keeps abort/error state explicit. Automatic compaction reacts to context thresholds and recoverable overflow, prevents repeated overflow recovery, may retry summarization with bounded callbacks, and resumes the interrupted turn only when the resulting context is valid. A failed or aborted compaction does not claim a successful summary.
- **Extension binding and reload (2468–2995):** bindings install UI/mode/command/abort/shutdown/error handlers; resource discovery resolves skill, prompt, and theme paths while keeping source metadata. Reload resets provider state, rebuilds the extension runner and tool registry, refreshes the current model, and rebinds the same session without retaining stale extension contexts. Retry logic classifies retryable assistant failures, emits attempt lifecycle events, applies bounded delays, and stops after configured limits.
- **Bash, session tree, and stats (3006–3552):** bash execution tracks per-command abort controllers, emits incremental updates, records results as context or excluded history, and flushes pending bash messages at a safe turn boundary. Session naming is sanitized and persisted. Tree navigation refuses streaming/compacting states, runs before-tree hooks, can summarize or label branches, and updates the session leaf atomically from the session manager. Stats count user/assistant/tool messages, usage, cost, compaction, and branch summaries; context usage reflects the active branch after the latest compaction. HTML/JSONL export, last-assistant lookup, replacement-session context, and extension-handler queries are read-only helpers.

## Errors, cancellation, recovery, and side effects

Auth failures are converted to provider-specific guidance; OAuth failures direct the user to re-authenticate. Tool hooks can block execution or replace normalized results. Prompt admission preserves queues and draft ownership on invalid input, missing auth, unsupported streaming mode, or absent model. Abort controllers cover every long-running operation. Compaction and retry distinguish cancelled, failed, overflow, and successful outcomes; no failed operation is represented as accepted context. Persistence appends user/assistant/tool/custom entries through `SessionManager`; compaction and branch summaries are written only at their dedicated boundaries. Reload invalidates old extension contexts so stale callbacks cannot mutate the replacement runtime.

## Source behavior criteria

A faithful target should preserve: skill/template expansion only when explicitly enabled; steer versus follow-up queue ordering; extension replacement before persistence; image normalization after tool hooks; model/auth mutation before provider calls; bounded retry and overflow recovery; compaction cancellation and branch-summary cancellation; per-command bash cancellation; atomic tree navigation with before-tree hooks; and session statistics derived from the active branch rather than all orphaned entries. These criteria describe source understanding and do not claim that Zenpi implements every upstream API.

## Zenpi mapping and gaps

The source lifecycle maps to Zenpi's shared session/owner paths in `src/session.rs`, `src/core.rs`, `src/headless.rs`, and `src/tui.rs`. Zenpi must retain project-owned cwd/session identity, BentoBox layout, draft/focus isolation, explicit approval ownership, bounded transcript/source inspection, and stale-owner rejection. Upstream extension, compaction, retry, and branch semantics are evidence for comparison; this file-level receipt does not accept ZS1-056 or any L3 product item.

## Controller completeness note

The controller read the complete frozen source in byte order `[0,120916)`, verified byte count, line count, and SHA-256 above, and reviewed the lifecycle, queue, tool, extension, compaction, retry, bash, tree, and export paths. No upstream test execution or target product execution is claimed by this file report.
