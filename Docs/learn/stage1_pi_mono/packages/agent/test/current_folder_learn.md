# ZS1-058 — controller independent directory integration review

Decision: accept only the active blueprint's frozen subset of packages/agent/test. Authority is 3.1.20 / 8d525b351d066ce9b0487a485337647d47233275782e4f4d154423a623fe1645. The controller has already accepted the sole scoped direct file ZS1-014. No in-scope direct subdirectories exist. The current directory item can therefore close after this separate integration review; ancestors 061/064/065 and products 101/102 remain open.

## Direct inventory and evidence

The controller inspected the full new 11686-byte directory report, full old 4250-byte report, inventory, verifier, mapping identities, 23-row case audit and isolated runtime bridges/configuration. The frozen entry manifest is 0626277a63874cdfae524362c9580182f137af8c14af85c4cba9f5525e3d7aca. Five payload hashes and the archived old058 package's 32 payloads were checked. The archive was extracted only after rejecting absolute paths, parent traversal, symlinks and nonregular/non-directory members.

Actual upstream enumeration confirms six direct children. agent-loop.test.ts is the sole frozen file, 47809 bytes / 1610 lines / c359921a73525d67cf42481ad04210952fe36a9517d7b099f41710734438987d. agent.test.ts, e2e.test.ts and proxy.test.ts were only inventoried and hash checked. harness and utils were inventoried as context-only directories without recursive review. This is the existing frozen scope, not a new scope reduction.

All eight source-closure files remain byte-identical to the recorded upstream sources. Ten copied master014 evidence files were compared against actual current master files, not merely against a self-contained manifest. The current child receipt passes the active checker. The old collection, old actual results and independently executed master014 results have exactly the same 23 full case titles; current actual results are 23 passed in one file, zero failed/pending/todo. Collection is not execution, and this directory review introduces no new test execution, collection, installation or build. The master's previous actual replay and full 1610-line subject review remain the leaf evidence.

The initial controller integrity helper incorrectly used the validator's dictionary-only JSON reader on the mapping list; it stopped before any directory report or state change. The corrected helper uses a list-capable JSON parse while preserving every identity check. verify_current.py then passed. This preparation error is not a product defect or test failure.

## Independently traced cross-file boundaries

For this directory integration the controller read the complete 803-line agent-loop.ts in ordered 1–280, 281–560, 561–803 sections, plus the complete stream-fn.ts and EventStream implementation. These support reads contextualize the accepted leaf, and do not accept additional directory children.

The subject calls agentLoop/agentLoopContinue wrappers; the old report's shorthand that it directly calls runAgentLoop/runAgentLoopContinue is corrected by the new report. The wrappers forward to the async runners and adapt emitted AgentEvents into EventStream; the isolated package bridge forwards real setDefaultStreamFn, EventStream and validateToolArguments. MockAssistantStream is a scripted provider stream, not HTTP/model inference. The full upstream barrel and broad Vitest discovery are not exercised by this isolated file.

runAgentLoop creates a new messages array while retaining message object aliases. Continuation copies the context object but shares its messages array. prepareNextTurn may replace context/model/reasoning; post-prepare steering is polled again only for an initially empty pending batch. Context transformation precedes conversion at the LLM boundary. This explains why one/all queue modes and late arrivals need independent target tests; original test 1031 proves only changed system prompt and invocation counts.

Tool argument preparation precedes validation, then beforeToolCall receives the validated mutable object. The leaf deliberately proves mutation to number 123 after string validation without another validation pass. It is source behavior and does not authorize weakening Zenpi's schema, path, approval or policy validation. Parallel preparation is sequential; prepared closures launch under Promise.all. Tool execution-end events may arrive in completion order, whereas tool-result messages enter the next model context in source order. An explicit sequential tool forces a sequential batch. The source itself supplies no equivalent of Zenpi's governance concurrency bound in this fixture.

Termination requires a nonempty set whose every result has terminate=true. afterToolCall can replace content/details/usage/terminate/isError; source nullish fallback differs from arbitrary replacement. Missing result content is normalized to an empty array; addedToolNames are carried only when nonempty. The accepted tests prove these paths only at their actual assertions. The mixed slow/fast test has weaker assertions than a completion barrier, several all-terminate tests contain one call, and the term persist denotes in-memory state in the source fixture.

Assistant error/aborted messages end the loop without tool dispatch; length truncation synthesizes tool errors. Missing tools and preparation/hook/execution failures have distinct handling. Not every async failure becomes a terminal stream event: wrappers attach only a success continuation, EventStream has no rejection result channel, and rejection while awaiting pending tool update emissions can escape the tool catch. These are source-read observations, not a reproduced hang or a newly proved Zenpi bug. Signal propagation is cooperative and does not establish OS kill/reap, durable recovery, input identity guarantees or capacity handling. End/result and event delivery remain separate mechanisms; do not infer a general multi-consumer lifecycle guarantee.

## Current target mapping

Six target file identities still match the packet. The controller read core.rs 3609–3775 and 3986–4062, input-host tests 800–916, queue tests 135–212, and batch tests 184–263. runtime.rs/core_session.rs are preserved mapping identities, not newly fully reviewed target files.

The actual current core uses an InputBoundaryGate with begin/finish preparation and a durable queue boundary. The four Agent + SessionStore + CaptureOnly preparation fixtures cross empty/nonempty and one/all modes. They assert preparation was actually reached, exact request counts and late-a/late-b placement, exactly one applied journal entry per input ID, and preparation cleared. These are concrete source assertions, not fresh target test runs in this directory review. The old claim that these fixtures still need adding is superseded.

Current parallel dispatch combines registry execution modes, extension hook presence, global sequential mode, MAX_BATCH_CALLS and governance capacity. Extra concurrency is reserved against the same owner. The existing HTTP-backed Agent/read-handler test checks overlap, completion order, source-ordered next-provider results, released concurrency and SessionStore reopen. The adjacent cap/global/per-tool cases assert peak one and ordered completion. Those contracts are more specific than the source fixture; neither same-process reopen nor source inspection proves process-crash recovery or whole-product acceptance.

The original 16/96 counts in the worker packet reflect its capture time. Source015 was subsequently accepted; the live blueprint and Gantt govern current progress. Accept this directory only after its own report and receipts are recorded, without changing any unscoped sibling or ancestor status.


## Preserved worker directory review

The candidate status and captured counts below are historical; the current master decision above governs.

# ZS1-058 — independent directory review of packages/agent/test's frozen subset

Worker A candidate; **058 remains pending master directory review**. Current authority is 3.1.20, requirement digest `8d525b351d066ce9b0487a485337647d47233275782e4f4d154423a623fe1645`. The current master ledger and the copied 014 acceptance receipt record 16 accepted / 96 open out of 112. This packet updates the old directory report's stale prerequisite and target mapping; it never changes the old packet, master tree or acceptance ledger.

## Direct children and closure

The actual immediate inventory was independently re-enumerated and matched to the old six-child inventory. All immediate regular-file hashes still match. No recursive reading or acceptance of context-only directories is claimed.

| Immediate child | Kind | Frozen scope / disposition |
| --- | --- | --- |
| agent-loop.test.ts | file | only in-scope file, ZS1-014; master accepted |
| agent.test.ts | file | context-only; inventoried/hash checked, no complete review here |
| e2e.test.ts | file | context-only; inventoried/hash checked, no complete review here |
| proxy.test.ts | file | context-only; inventoried/hash checked, no complete review here |
| harness | directory | context-only; no descendant traversal/review in this packet |
| utils | directory | context-only; no descendant traversal/review in this packet |

There are **one frozen direct file and zero frozen direct subdirectories**. The frozen dependency chain is 001 → 014 → 058 → 061 → 064 → 065; 061 also requires 057, and its other dependencies cannot be bypassed by accepting 058. 058 is now eligible for independent G-DIR review because its sole scoped child 014 is accepted. Eligibility is not directory acceptance. The source support files under agent/src and ai/src in the executable closure are context across directory boundaries; they are not new children or acceptance claims for this directory.

## 014 evidence now actually accepted by master

The canonical receipt is [master-review-3.1.20/review.md](/Users/wangweiyang/GitHub/zenpi/Docs/quality/stage1/ZS1-014/master-review-3.1.20/review.md). Its review, acceptance/verification, actual results/streams, run record and auxiliary top-level receipts are copied with hashes under `master014/` in evidence.tar.gz. The ledger's 014/058/ancestor lines and identity are preserved in inventory.json.

Master read all 1,610 lines in ranges 1–410, 411–830, 831–1230, 1231–1610, checked 24 original payloads/eight source copies, and actually replayed 23 original tests in one file: exit0, 2.553790667 seconds, zero failed/pending/todo. The current runtime identity covered 2,062 regular dependency files and five symlinks before/after, with only the exact generated Vitest cache excluded. The run uses Node24.19.0, real pinned Vitest4.1.9 and TypeBox1.3.27. That is a current replay identity, not a retroactive installed-file inventory for the original run.

This directory update executes **zero tests, zero collection runs, zero installations and zero builds**. It reuses old058's 23-title collection and its historical014 result, and joins them to the new master's actual 23 full titles/statuses. The old collection itself was not an execution pass. The old058 package's 32 payloads and eight source copies were reverified, including upstream equality. The original agent-loop.test.ts SHA is `c359921a73525d67cf42481ad04210952fe36a9517d7b099f41710734438987d`, 47,809 bytes.

## Calls across the directory boundary

A correction to the old shorthand “the test calls runAgentLoop” is necessary: test lines11–12 directly import **agentLoop / agentLoopContinue** and setDefaultStreamFn. The wrappers in agent-loop.ts:32–96 create an Agent EventStream and forward to runAgentLoop / runAgentLoopContinue with an event sink. Tests iterate that stream and request its final result. The underlying async functions then enter runLoop:156, streamAssistantResponse:279 and executeToolCalls:409. The synchronous continuation guard is in the wrapper as well as the lower-level async implementation.

The test's MockAssistantStream:16 inherits the actual EventStream; queueMicrotask supplies scripted done messages. The isolated pi-ai export bridge resolves real frozen EventStream/AssistantMessageEventStream/validateToolArguments, and the index bridge resolves only real setDefaultStreamFn. The subject also imports TypeBox and Vitest; type-only imports do not establish runtime or whole-project type-check coverage. Full index.ts and upstream Vitest config are retained as context, but the isolated config selects exactly one test file and does not initialize the full monorepo barrel. The original broader alias/discovery configuration, sibling tests and npm test are not validated here.

The supporting agent-loop.ts was independently read in contiguous ranges1–310,311–610,611–803 for this directory integration; stream-fn.ts and EventStream were read in full. This is a context read, not acceptance of those source owners. The 23-row old case-audit retains each assertion boundary, and master014/review.md provides the complete newer subject review.

## Data ownership and ordering

| Boundary | Owner and flow | What the accepted file proves / does not prove |
| --- | --- | --- |
| prompt → current context | runAgentLoop shallow-copies context and creates a new messages array with existing messages plus prompts; message objects still alias | fixture roles and event presence; no deep immutability proof |
| continuation | runAgentLoopContinue shallow-copies context but retains its messages array reference; newly emitted messages are separately accumulated | existing user is not re-emitted; no durable ownership or crash guarantee |
| prepare → convert → stream | runLoop may replace context/model/reasoning from prepareNextTurn; transformContext then convertToLlm feed the scripted stream | prompt replacement / pruning / custom conversion; not long-prepare late-input proof |
| tool argument preparation | prepareArguments runs before validateToolArguments; validation clones the arguments; beforeToolCall receives the validated mutable object | explicit test changes valid string to number123 and executes without revalidation; this is source behavior, not permission to weaken target policy |
| execution → result | callbacks return results; afterToolCall can replace content/details/usage/terminate/isError; tool-result message is normalized | callback execution and usage replacement; title “persist” refers to in-memory context/events |
| parallel completion | preparation is sequential; prepared closures run under Promise.all; completion events emit as calls finish; ordered Promise.all results are later emitted/appended | reverse finish events with source-ordered results; no capacity bound/resource accounting in this source fixture |
| stream terminal | EventStream queues events/waiters and resolves result on terminal event/end | fixture stream lifecycle; no disk queue, input IDs, host drain or recovery |

Cross-file invariants must be kept at the demonstrated strength. Tool-result IDs/order survive callback completion reordering into the next model context. Steering arrives after the current complete tool batch, while follow-up is polled when the loop would stop. The source's post-prepare poll is conditional on an empty pending batch; a preexisting nonempty batch is retained. The original test at1031 only changes systemPrompt and checks counts, so the latter source branch must not be labeled race-tested by014. The mixed slow/fast test only observes slow starting first and fast appearing; it does not independently prove a completion barrier. Single-call termination tests do not establish every-result behavior for multiple calls.

## Error, cancellation and persistence boundaries

The empty-context continuation case proves a synchronous guard. The source also guards an assistant tail, but this is not a dedicated014 assertion. A custom tail remains the caller's conversion responsibility; there is no provider acceptance request.

The source's explicit assistant error/aborted stopReason ends the turn/agent without tools. Length truncation synthesizes tool errors instead of dispatch. Missing tool, validation/preparation/before-hook failure, execution failure and after-hook failure have their own normalization paths in the supporting code. Not all those failure paths are tested by these23 cases: do not translate a source read into passing failure tests.

Callback or async-sink rejection is not uniformly normalized. In particular, the stream wrapper attaches a success `.then` without a rejection-to-stream completion branch; prepare/transform/conversion/sink rejection can propagate out of the lower loop. The source EventStream has no reject-result channel. This is a source-level boundary found by reading, not a reproduced hang or new product defect. The execute callback's pending update emissions are awaited and can themselves reject. Tool-hook catches must not be generalized to “every async failure always produces a terminal result.”

AbortSignal is passed through transforms/stream/hooks/tools, with abort checks around preparation and dispatch and a sequential break. No dedicated signal cancellation scenario, OS child kill/reap, resource exhaustion, durable input ID edit/retract, journal restart or crash recovery is exercised by014. The fixture callbacks use JavaScript memory and Promise gates; example.invalid is model metadata. No actual provider HTTP/model inference or OS tool effect is established by this directory. Separate010/011/057 evidence is not inherited as058 test results.

## Current target mapping, independently checked

| Source contract | Current Zenpi entry and evidence inspected | Remaining scope |
| --- | --- | --- |
| batch input / late preparation | core.rs:3697/3726 begin/finish preparation; tests/stage1_input_host.rs:800–915, queue cases140/183 | four real Agent+SessionStore/CaptureOnly fixtures cross one/all with empty/nonempty boundary, assert is_preparing was reached, enqueue late-a/b and each ID applied once; not HTTP and not new executions here |
| parallel result order / dispatch policy | core.rs:3986 parallel_tool_concurrency and4019 invoke_parallel_tools; tests/stage1_tool_batch.rs:184–263 | actual HTTP-backed Agent/read-handler test source checks overlap, source-ordered provider results, governance release, and same-process SessionStore reopen; serial policy cases inspect peak1; no independent crash-recovery inference |
| runtime and core integration | tests/runtime.rs and tests/core_session.rs remain the blueprint mapping; input queue/host and shared batch owner provide concrete contracts | FIFO runtime coverage alone cannot establish same-turn input semantics, bounded batch policy or host recovery |

The above target source ranges were read and full-file identities recorded in mapping-identities.json. They are mapping evidence only, not fresh target test passes or full101/102 acceptance. The old statement that the four long-preparation host tests still need adding is obsolete. Target extension argument mutation still requires its own schema/path/preview/policy enforcement even though the upstream before-hook fixture deliberately mutates already validated data.

A master may now independently review this directory integration using the sole accepted leaf and its bounded closure. No unscoped siblings or subdirectories are silently included, no ancestor is skipped, and no source-case count is multiplied into a new058 pass count. Run verify.py for read-only integrity/title/scope checks; no product or tests are launched.
