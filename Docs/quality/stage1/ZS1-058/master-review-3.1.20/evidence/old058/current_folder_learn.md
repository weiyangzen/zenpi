# ZS1-058 — packages/agent/test

Provisional [_], authority 3.1.18 / cb96a3db9b57ea0d9d3ad43f0be954129aabf69bc5d438210a14b674a4a8af85. The frozen subset contains only agent-loop.test.ts (014), with no in-scope child directories. Actual agent.test.ts, e2e.test.ts, proxy.test.ts and harness/utils directories are context-only. The complete immediate inventory is recorded; their presence is not acceptance.

014's immutable package already executed the exact 1,610-line test file with real Vitest: 23 passed, zero failed/skipped/todo. This directory does not repeat that run. It retains the historical result, verbose logs and 23-row assertion review. A new real Vitest collection resolves the same unchanged test module and finds exactly the same 23 full titles. case-audit.json joins every collected title to the historical actual result and verifies every reviewed case title at its original source line. Collection is not counted as a new passing behavior test.

## Actual test-to-owner connections

The original file calls runAgentLoop/runAgentLoopContinue and the default stream registry. Its custom MockAssistantStream extends the real EventStream and emits scripted done messages via queueMicrotask. TypeBox argument validation executes the actual implementation. A narrow barrel bridge forwards only setDefaultStreamFn to the unchanged definition; the original package barrel is retained for review but not initialized in this isolated run.

The historical assertions establish these concrete boundaries:

- Transformation precedes provider conversion; custom message handling is supplied by a fixture converter.
- Tool callbacks and after-hook result replacement reach tool-result messages. Argument preparation precedes validation, but a before-tool hook can subsequently mutate validated arguments without another validation.
- A length-truncated assistant's tool call is not executed. Completion events can arrive in completion order while stored results preserve source order.
- Steering waits for the existing tool batch; follow-up is polled after normal stopping. Prepared context is used for the next model turn.
- Stop/terminate decisions, blocked calls, mixed termination and continuation guards are tested at their specific callback boundaries. Existing user input is not re-emitted on continuation.

These are actual assertions in the retained execution, not conclusions inferred from test titles. In particular, the mixed slow/fast case only checks that slow starts first and fast appears; it does not independently establish a completion barrier. Several “whole batch termination” cases contain only one tool call. The preparation case checks a changed system prompt and call counts, not steering arriving during long preparation. Every limitation stays attached to its case in case-audit.json.

## Error, cancellation and recovery limits

The test file uses JavaScript tools, synthetic model metadata/usage and Promise gates. It has no actual provider request, OS child supervision, durable journal or process restart. Dedicated kill/reap, interrupted external outcomes, queue admission IDs and capacity exhaustion are absent. Cancellation/recovery are therefore not marked passed by this directory.

010/011 provide separate actual-process source evidence;057 adds real Agent/loop preparation and queue integration observations. Those packets remain separate and are not counted as014 cases. In particular057's early-drained-input loss on preparation failure is not covered by the original014 preparation assertion.

The upstream package Vitest config has broader discovery and package aliases. This isolated configuration selects only the frozen test file; it does not claim npm test, neighboring harness tests, API e2e tests or the full barrel pass.

Target mapping is tests/runtime.rs, tests/core_session.rs and existing input/batch owner suites. These must independently prove target admission preservation, cancellation, restart and resource limits rather than inherit source fixture assumptions.

014 remains unaccepted, so058 cannot satisfy G-DIR acceptance yet. The closure is014→058→061. No child directory in the frozen subset is skipped or double accepted. No product or old evidence files were changed.

