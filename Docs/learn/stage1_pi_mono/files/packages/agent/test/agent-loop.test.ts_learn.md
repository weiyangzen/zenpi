# ZS1-014 — packages/agent/test/agent-loop.test.ts

source_hash: c359921a73525d67cf42481ad04210952fe36a9517d7b099f41710734438987d

# ZS1-014 — complete agent-loop test source review, master 3.1.20

The controller read the complete 1,610-line / 47,809-byte packages/agent/test/agent-loop.test.ts in contiguous ranges 1–410, 411–830, 831–1230 and 1231–1610. Source SHA-256 is c359921a73525d67cf42481ad04210952fe36a9517d7b099f41710734438987d. All fixtures, 23 tests and assertions were read, together with the worker report, case table, replay/configuration/export bridges and source/runtime verifiers. The 24 frozen payloads and eight source copies match their identities and the current upstream tree. Supporting files are context; this review does not independently accept them or their directories.

The package was created under historical authority 3.1.15 and its entry supplement under 3.1.19. Current 3.1.20 changes neither this file's frozen bytes nor its scope, prerequisite001 or validators. Historical reports are preserved; the current mapping and assertion qualifications below supersede outdated target descriptions.

## Executable scope

The master replay uses actual pinned Vitest 4.1.9 and TypeBox 1.3.27, unchanged source test/agent-loop/EventStream/argument-validation implementations, and Node 24.19.0. The package export bridges change module resolution only: pi-ai exposes frozen EventStream and validateToolArguments, while the package-index bridge exposes frozen setDefaultStreamFn. Type-only imports are erased by the transpiler. Neither the full monorepo barrel/configuration nor whole-project TypeScript checking is executed.

The original MockAssistantStream inherits real EventStream; queueMicrotask supplies scripted done messages. createModel uses example.invalid as metadata, createUsage constructs zero-price usage, and echo/edit/slow/fast tools are actual in-memory JavaScript callbacks with Promise gates. Their execution is real at this callback boundary. It is not provider HTTP, model inference, OS file tools, durable session persistence, PTY interaction or subprocess recovery. The default stream registration is restored in finally. No skip/todo/only or altered expectations are used.

The actual master run finished with exit0 in 2.553790667 seconds: 23 passed, zero failed/pending/todo, in one actual test file. Four reporter suites include describe groups and are not four source files. Every actual test title/status was matched to the case review. The private runtime contains 2,062 regular files and five symlinks, with the same compact per-file identity before/after; only the exactly named generated Vitest cache is excluded. The historical014 package lacked an installed-file inventory, so this establishes current replay identity rather than claiming an old post-run dependency inventory. Node executable identity and all source copies were rechecked after the run.

## Complete test behavior and assertion limits

Lines1–118 define the stream/message/model fixtures and legacy default-stream compatibility. Reflect.apply omits streamFn and the test asserts one registered fallback invocation. Lines119–273 check two resulting message roles, existence of lifecycle event types, custom notification filtering and transformed/conversion array lengths. Event existence is weaker than a complete event order, and the context test covers a pruning fixture rather than an asynchronous preparation race.

Lines274–443 execute an echo callback and inspect successful tool start/end and afterToolCall usage replacement. A length-terminated assistant containing one tool call produces no tool execution, an error mentioning the output token limit and a subsequent scripted model call. This establishes handling of the explicit fixture stopReason, not correctness of a provider wire parser.

Lines444–585 explicitly allow a beforeToolCall hook to mutate a schema-valid string argument into number123 and execute without revalidation. The next case rewrites legacy oldText/newText into an edits array before TypeBox validation. The former is an upstream behavior observation; it is not authorization to weaken Zenpi's schema, path, preview or immutable-policy checks after extension mutation.

Lines586–786 use real Promise/timer gating to observe callback overlap and reverse completion order: tool_execution_end is tool2/tool1, while tool-result message_end and turn_end arrays retain tool1/tool2 source order. The word persist in the title refers to in-memory results/events here. The sequential steering case makes a queued message available after one tool starts, then asserts that both tool results precede its message event and that the next scripted model context includes it.

Lines787–1030 cover a sequential-marked tool called twice, a mixed slow/fast batch and an explicitly parallel tool called twice. The single sequential-tool case observes no overlap and source-order results; the all-parallel case observes overlap. The mixed case only asserts that slow starts first and fast appears, so it does not independently establish that fast waits for slow completion. There is no separate global sequential overlap assertion in this source file.

Lines1031–1106 replace a preparation snapshot systemPrompt and assert two stream calls and one prepare call. This does not test two inputs arriving while preparation is pending or exactly-once consumption. Lines1107–1486 cover shouldStopAfterTurn, result terminate, blocked/terminating before hooks, mixed termination and terminating after hooks. The explicit shouldStopAfterTurn test verifies one stream/tool call, one initial steering poll, no follow-up poll, callback context and the exact lifecycle event sequence. The every-result and after-hook termination examples each contain one tool call; multi-tool universal termination cannot be inferred from those titles. The mixed tests do verify continuation with a non-terminating second call and ordered resulting roles.

Lines1487–1610 cover continuation: an empty context throws synchronously without calling the stream; an existing user message is not re-emitted and only the new assistant result/event is returned; a custom last message is converted by the caller fixture. No actual provider-role acceptance follows. This file has no dedicated cancellation-signal scenario, persistent input-ID edit/retract test, resource-exhaustion test, forced child termination or crash recovery. Previously accepted010 or separate Zenpi evidence remains separate.

## Current Zenpi mapping

The controller checked current core input-boundary/preparation and parallel-tool paths, plus the relevant input queue, input host and batch tests. Current core begins and finishes InputBoundaryGate preparation around semantic provider-context preparation. Only an initially empty boundary may consume late input; a committed nonempty boundary remains frozen. The InputPort preparation guard and InputPump connect admission and cancellation to the session owner.

tests/stage1_input_host.rs prepare_arrivals contains four actual Agent preparation fixtures: one/all mode crossed with initially empty/nonempty input. They observe port.is_preparing, submit late-a/late-b, require the preparation condition to have been reached, and assert each input ID is applied exactly once. These use a CaptureOnly backend and actual SessionStore, not HTTP. The worker's older statement that these tests must still be added is obsolete. Corresponding gate-level cases in tests/stage1_input_queue.rs cover empty and committed nonempty snapshots.

Current core parallel_tool_concurrency accounts for global/per-tool sequential policy, extension hooks and available governance capacity; invoke_parallel_tools calls the shared bounded executor. tests/stage1_tool_batch.rs contains actual HTTP-backed Agent/read-tool tests with overlap, source-ordered provider context and same-process SessionStore reopen, plus explicit dispatch policy and stale-preview cases. This review inspected these mappings but does not rerun or accept their full product obligations. Same-process reopen is not independent crash recovery. Runtime FIFO tests alone do not satisfy same-turn input or batch behavior.

Only source014 understanding is accepted by the accompanying receipt. Directory058, target files and complete101/102 product requirements remain open and need their own evidence. The replay results and immutable runtime/source audit describe the precise run used for this acceptance.


## Historical worker report, preserved verbatim

Its old statement that late-prepare tests still need to be added is superseded by the current master mapping above. Historical authority/counts are not current acceptance.

# ZS1-014 — packages/agent/test/agent-loop.test.ts

状态：[_] worker阅读候选，主控未接受。
source_id: SRC-0209
source_path: packages/agent/test/agent-loop.test.ts
source_hash: c359921a73525d67cf42481ad04210952fe36a9517d7b099f41710734438987d
source_bytes: 47809
read_ranges: [[0, 47809]]
run_id: zenpi-stage1-20260911

完整范围为1–1610行；本文件所有describe/it、fixture和断言均逐段阅读，原阅读阶段未运行源vitest；此次补证以未改原测试实际运行23项全部通过，依赖边界见下。MockAssistantStream只模拟AssistantMessage事件，createModel为example.invalid零费用模型，identityConverter过滤标准消息；不能将这些单测当真实HTTP/PTY或模型效果证明。

default stream兼容测试通过Reflect.apply模拟旧调用省略streamFn，并finally恢复全局配置。基础loop测试检查AgentMessage事件与最终messages、custom notification过滤、transformContext先于convertToLlm。tool用例验证实际execute回调和afterToolCall usage替换；length-truncated测试确认executed为空且随后允许第二次LLM调用。prepareArguments测试把oldText/newText映射为edits再验证；“mutated beforeToolCall args without revalidation”刻意允许string变number，此行为不适用于zenpi更严格的schema/policy边界。

并发用例用Promise gate控制first/second，断言第二先结束而结果和turn_end按tool-1/tool-2源序。queued steering测试在第一工具执行后可见queued消息，却断言两工具结束/result事件都先于interrupt，第二次模型context含interrupt。单工具sequential用例检查second未在first释放前启动；混合slow/fast用例仅检查slow首先开始且fast最终出现，并未直接断言fast晚于slow完成；此文件没有独立global sequential重叠断言。all-parallel测试确认真实回调重叠。prepareNextTurn snapshot用例替换systemPrompt并检查恰好一次prepare/两次LLM；本文件没有完整的长prepare期间到达两条steer去重测试，zenpi必须新增，不能将此snapshot断言扩大描述。

停止矩阵：shouldStopAfterTurn强停后steering只初poll一次、follow-up零poll；全工具terminate=true停止，beforeToolCall block+terminate停止；混合blocked或混合terminate继续；afterToolCall可把全批标terminate。continuation用例拒绝空context、只产生新assistant事件、允许调用方转换custom末消息。取消仅由loop传signal路径间接涉及，本文件缺少独立进程恢复、持久ID编辑撤回、资源耗尽和kill/reap证明。

目标判据分别归属stage1_input_queue（one/all、两lane、long prepare、ID冲突/编辑撤回、完整journal恢复）与stage1_tool_batch（真实读工具重叠、源序结果、屏障、审批拒绝、截断参数零副作用）。现有tests/runtime.rs FIFO job测试应原样通过，但不抵充上述同turn队列/工具批次门禁。packages/agent/test冻结子集只有此文件，目录整合可在本报告经主控接受后独立执行；其它源测试是context-only。

## 实际原测试执行与完整断言复核

[_] authority3.1.15，主控待接受。已完整重读1–420/421–830/831–1230/1231–1610，原文件/冻结副本均保持c359921a73525d67cf42481ad04210952fe36a9517d7b099f41710734438987d。证据`.ops/source014-ready`及`Docs/quality/stage1/ZS1-014/worker-source`包含未改原测试、被测owner源码、完整23项实际Vitest结果、精确依赖锁、运行配置、逐例case-review.json。Vitest4.1.9+TypeBox1.3.27，Node24.19.0：23passed/0failed/0skip/0todo；未使用计数stub。

测试fixture仍为原文件MockAssistantStream、example.invalid模型和内存echo/edit/slow/fast回调，回调实际经过未改agent-loop.ts及真实TypeBox，但不等于真实模型、HTTP、OS工具或持久化验证。隔离配置把pi-ai映射到冻结实际EventStream/validator，把index.ts唯一所用setDefaultStreamFn映射到真实stream-fn；未执行完整monorepo package barrels，不声称原仓库全量npm test通过。所有import替换仅为可审查的导出解析桥，不替换被测函数或Vitest断言。

补充收窄：标题every tool result terminate及afterToolCall terminate用例都只有一个tool call，不能独立验证多工具every语义；prepare snapshot只测systemPrompt和调用次数。更强的真实进程混合串行、全批terminate、长prepare输入及取消证据归于旧010包，保留原manifest不混算014。原文件没有skip/todo/only、beforeEach/afterEach；default stream全局状态在finally恢复。下面逐例列出被测owner和断言边界。

| 源行 | 原测试标题 | 被测owner与断言范围 |
|---|---|---|
| 85 | uses the configured default when a legacy caller omits streamFn | runAgentLoop + getDefaultStreamFn：Reflect.apply省略streamFn；仅断言fallback被调用一次，finally恢复全局注册 |
| 119 | should emit events with AgentMessage types | runAgentLoop/event lifecycle：user+assistant两条结果与生命周期事件存在；不是精确全序断言 |
| 166 | should handle custom message types via convertToLlm | streamAssistantResponse.convertToLlm：过滤notification后仅user传给转换结果；不验证真实provider角色规则 |
| 221 | should apply transformContext before convertToLlm | streamAssistantResponse transform/convert：转换前后数组长度均2，原context五条；只覆盖裁剪接线，不覆盖异步竞态 |
| 274 | should handle tool calls and results | executeToolCalls/finalizeExecutedToolCall：echo回调实执行hello，start/end成功；after hook usage替换进入toolResult |
| 371 | should not execute tool calls from a length-truncated assistant message | failToolCallsFromTruncatedMessage：单个length工具零回调、错误含output token limit，第二次模型调用允许恢复 |
| 444 | should execute mutated beforeToolCall args without revalidation | prepareToolCall + before hook：schema string参数经before hook变123，execute实际收到123；刻意证明不二次校验 |
| 506 | should prepare tool arguments for validation | prepareToolCallArguments + validator：oldText/newText改写edits后真实TypeBox校验，回调接收一组替换 |
| 586 | should emit tool_execution_end in completion order but persist tool results in source order | executeToolCallsParallel：Promise+20ms gate观察回调重叠，end=2,1，而message_end和turn_end结果=1,2 |
| 681 | should inject queued messages after all tool calls complete | runLoop steering boundary：串行两回调和两result均先于interrupt，下一stream context含interrupt |
| 787 | should force sequential execution when a tool has executionMode=sequential even with default parallel config | executeToolCalls batch mode：单个sequential工具被调用两次，parallelObserved=false且结果源序 |
| 870 | should force sequential execution when one of multiple tools has executionMode=sequential | executeToolCalls mixed mode：弱断言：executionOrder首项slow且包含fast；未直接断言fast晚于slow完成 |
| 957 | should allow parallel execution when all tools have executionMode=parallel | executeToolCallsParallel：全parallel工具两次调用，第一未完成时第二可执行，parallelObserved=true |
| 1031 | should use prepareNextTurn snapshot before continuing | runLoop.prepareNextTurn：只验证systemPrompt替换、prepare一次、stream两次；无长prepare新steer/去重断言 |
| 1107 | should stop after the current turn when shouldStopAfterTurn returns true | runLoop.shouldStopAfterTurn：一次stream/echo，steer只初poll一次、followUp零次；callback context和完整事件全序 |
| 1204 | should stop after a tool batch when every tool result sets terminate=true | shouldTerminateToolBatch：仅一个工具terminate=true，stream一次、一次turn_end；不独立证明多工具every谓词 |
| 1256 | should stop after a blocked tool call when beforeToolCall sets terminate=true | beforeToolCall block + terminate：单工具不执行、stream一次，错误toolResult含Blocked by policy |
| 1315 | should continue after a mixed batch with one terminating blocked call | prepareToolCall + mixed terminate：first被block/terminate，second实际echo，stream两次 |
| 1374 | should continue after parallel tool calls when not all tool results terminate | shouldTerminateToolBatch parallel：两个回调仅first terminate，stream两次且结果角色序含两toolResult |
| 1439 | should allow afterToolCall to mark a tool batch as terminating | finalizeExecutedToolCall after hook：单工具after hook terminate导致stream一次；非多工具全批证据 |
| 1489 | should throw when context has no messages | agentLoopContinue guard：空context同步抛错，stream不得被调用 |
| 1508 | should continue from existing context without emitting user message events | runAgentLoopContinue：已有user不重发，只有新assistant结果及message_end |
| 1550 | should allow custom message types as last message (caller responsibility) | convertToLlm custom continuation：custom尾部由fixture转换成user，返回一assistant；不是实际provider请求 |
