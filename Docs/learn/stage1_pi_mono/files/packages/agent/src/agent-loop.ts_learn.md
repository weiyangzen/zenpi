# ZS1-010 — packages/agent/src/agent-loop.ts

source_hash: 1e16404a231912fbd7643d8317b15ec4cc6245ed8cedc37582a31d430a1cc6ac

# ZS1-010 complete source understanding, master 3.1.19

The controller read every byte of unchanged packages/agent/src/agent-loop.ts (803 lines,22796 bytes) in ranges1–405 and406–803. Full worker report, README, probe, actual process fixture, package-resolution bridge, loader and runner were reviewed. EventStream and default stream dependency bodies were read as context. All19 frozen artifacts,4 upstream source copies and the bundled Node24.19.0 binary match their recorded hashes. This dependency context does not accept additional source files.

The package deliberately omits installed node_modules. The controller's initial lookup assumed otherwise and failed before copying; a premature runner then exited127 because work/run.sh did not yet exist. Both preparation failures are retained, not counted as source failures. The controller installed the sole TypeBox1.3.27 dependency into a private runtime using npm ci --ignore-scripts and the unchanged integrity lock. All1385 installed file identities exactly match the frozen inventory before and after execution.

A fresh run on unmodified imported source passed12 scenarios, using16 real Node subprocesses. The controller independently checked each spawn/close pair and PID-bearing started/product/cancel files: successful children produce their exact stdout/product and cancelled children close23 without product. The fixture implements signal forwarding and waits for children. The source loop receives credit for passing the signal and awaiting tools, not for universal process supervision. Model StreamFn is a deterministic replacement; no actual provider HTTP, credentials or generated text is tested.

Full source review covers prompt-context copying versus continuation's shared message array, empty/assistant continuation rejection, event wrapper construction and terminal result flow, initial steer polling, tool/pending inner loop, deferred follow-up outer loop and explicit stop. Preparation replaces context/model/thinking, preserves existing pending input, and re-polls only an initially empty result. Transform/convert/key/stream order, incremental assistant replacement, done/error and iterator-end fallback are identified. Exceptions from emit/preparation/stream callbacks can escape; wrapper launch uses then without a rejection handler, so this is not a guarantee of terminal error-event publication for arbitrary thrown callbacks.

Tools select whole-batch sequential mode when any tool or global config requests it. Parallel preparation occurs in source order before Promise.all dispatch; completion events may arrive in completion order, while result messages are emitted in source order. Length-truncated calls fail without dispatch. Missing tools, schema failure, prepareArguments transformation and before-hook errors/block decisions create immediate failures. The before hook may mutate validated cloned args without a second schema pass. Abort is checked during preparation and execution admission; sequential cancellation stops remaining iteration, while parallel entries already prepared can produce abort outcomes. After hooks can override content/details/usage/terminate/isError or turn thrown errors into tool errors. Batch termination requires a nonempty list whose every finalized result requests termination. Pending async updates are awaited and late updates suppressed. Error updates can still reject through Promise.all; no general callback-fault recovery is inferred. Missing tool content normalizes to an empty array and addedToolNames is preserved only when nonempty. These are complete-source observations; the12 runtime cases test the principal boundaries, not an invented exhaustive branch-coverage claim.

Actual replay verifies B-before-A completion with A-before-B results, steering only after both children finish, deferred follow-up, preparation snapshot/no-double-poll, empty-poll refresh, per-tool/global sequential barriers, sequential/parallel cancellation, zero dispatch for length/schema/unknown errors, explicit-stop follow-up suppression, real before-hook argument mutation, transform boundary order, unanimous termination, update backpressure/late exclusion, wrappers and continuation array sharing. The source contains no durable input IDs, edit/cancel ledger, bounded persistent queue, crash recovery, or immutable policy evidence; Zenpi's added contracts remain independent.

The worker's old statement that current Zenpi only executes synchronous tool calls is stale. The controller read current core input_queue_request/input_boundary, preparation re-poll, batch gate, parallel_tool_concurrency dispatch and invoke_parallel_tools persistence, plus execute_tool_batch's bounded workers, sequential barrier and ordered result collection. InputPort/InputPump already provide durable ticket servicing, including the newly integrated manual-compaction scope. Current Zenpi tracks interrupted operations and policy/cancellation boundaries; runtime FIFO is not the same concept as an in-turn input queue. These inspected ranges establish updated mapping only. The retained full target snapshots are context records, not full target-file acceptance or proof of101/102/104 product completion.

Only010 source understanding is accepted. Its frozen bytes, scope, dependencies and validators are unchanged from the worker's3.1.14 package. The historical worker report is preserved without silently rewriting old counts/status. Parent050, other files and all product/release requirements remain open as independently recorded in the authoritative blueprint.


## Historical worker candidate, retained verbatim

The following old status and target mapping are superseded by the current master review above.

# ZS1-010 — packages/agent/src/agent-loop.ts

状态：[_] worker阅读候选，主控未接受。
source_id: SRC-0117
source_path: packages/agent/src/agent-loop.ts
source_hash: 1e16404a231912fbd7643d8317b15ec4cc6245ed8cedc37582a31d430a1cc6ac
source_bytes: 22796
read_ranges: [[0, 22796]]
run_id: zenpi-stage1-20260911

完整范围为1–803行。runAgentLoop复制context.messages再加入prompt；runAgentLoopContinue拒绝空context和assistant尾部，保留调用方convertToLlm对custom role的责任。两个EventStream包装器向同一runLoop发送生命周期事件；事件sink异步等待，末尾agent_end结束本次run。

runLoop的初次steering poll在外循环之前。内循环只因工具或pending输入继续；所有tool结果先进入context/newMessages，turn_end与shouldStopAfterTurn之后才再次poll steer。prepareNextTurn可替换context/model/reasoning；若前一poll已有输入，prepare后不再poll，防止one-at-a-time一次消费两条。follow-up只在内循环自然结束时poll；shouldStopAfterTurn显式停止直接return，不消费follow-up。error/aborted直接终止，length让所有工具调用生成失败结果且零执行，再允许模型重发。

streamAssistantResponse顺序是transformContext→convertToLlm→getApiKey→stream；start/增量更新当前assistant，done/error或stream末尾取result并写message_end。并不证明所有wire都存在完整终态校验；zenpi需保留自己的provider严格校验。

executeToolCalls检查全局sequential或任一tool.executionMode=sequential，形成全批串行；并行分支先按源序prepare各调用，再Promise.all启动，tool_execution_end按完成顺序，toolResult按源序。未知工具/schema/before hook拒绝均转错误结果；before hook修改已验证args不再校验，是与zenpi必须重新policy/schema检查的明确差异。after hook可以覆盖content/details/usage/terminate/isError；只有所有结果terminate才终止批次。executePreparedToolCall在终态拒绝late update，等待已发update promise；取消在prepare/execute前及串行每次完成后检查，未承诺任意工具的强制回收。

对应证据定位：agent-loop.test.ts的“inject queued messages after all tool calls complete”“emit tool_execution_end in completion order but persist tool results in source order”“length-truncated”“force sequential”验证上述边界；源测试提供mock stream及内存工具，不是zenpi生产入口证据。目标runtime.rs只在job终态启动FIFO后续job；core.rs:complete_with_tools仍循环同步工具，不能拿job FIFO抵充同turn输入队列。新增InputQueue应保留独立类型、显式下一model-turn ID、已消费snapshot和whole-batch gate；ZS1-102另需core/tools真实并发接线。源文件本身无稳定输入ID、编辑/撤回、容量和持久化去重，均为zenpi阶段新增义务。

## 实际源码补证（authority3.1.14）

[_] 2026-09-11 worker A补证，控制器尚未接受。已重新完整阅读1–405、406–803行；原文件和冻结副本均保持上述SHA。证据包为`.ops/source010-ready`，可提交证据在`Docs/quality/stage1/ZS1-010/worker-source`。完整trace、真实进程PID、stdout、产品文件与取消文件、provider-bound context和原事件序均保存在probe.json，具体哈希以manifest.json为准。

| 核对行为 | 实际观察与证据场景 |
|---|---|
| 整批并行与源序 | parallel_whole_batch_steer_followup_snapshot：A/B均已实际启动，先释放B，完成事件B,A；交付toolResult及下一turn context仍A,B |
| steer/follow-up | B完成但A未完成时无第二次模型调用、无steer注入、follow-up轮询为0；批次结束注入steer；prepare内新增steer延至下轮；follow-up最后进入 |
| prepare快照 | 已有pending时不多取一条；pending为空时prepare结束重新poll，准备期间的新输入进下一turn；context/systemPrompt/model/thinking替换均被实际loop传至stream边界 |
| 全批串行 | per-tool/global各用两个真实子进程验证：B启动发生在A close之后，不是仅比较事件计数 |
| 取消 | sequential仅A执行，B没有started/product文件；parallel两进程均接收信号并实际close23。强制清理由fixture tool实现，源码仅转交signal与等待结果 |
| 拒绝与终止 | length、实际TypeBox schema错误、未知工具均零dispatch；显式stop不消费follow-up；只有全部工具结果terminate才省略下一模型turn |
| hooks与更新 | before hook修改验证后的克隆args实际送入子进程；after hook覆盖结果；挂起update sink时真实子进程虽已close但tool_end未发；后续late update被忽略 |
| EventStream与continue | 实际wrapper发agent_end并完成result；error终止；低层runAgentLoopContinue只浅复制context，因此共享原messages数组且不重发既有user消息 |

依赖边界：实际执行的是未修改agent-loop.ts及冻结EventStream、validateToolArguments、stream-fn.ts，TypeBox1.3.27为真实安装且有lock/inventory。模型StreamFn是明确的确定性替身，所以不算真实provider或wire验证。工具为probe定义的AgentTool，确实启动OS子进程并读写文件，但不冒充上游bash工具。Node24.19.0满足上游>=22.19.0；12个场景、16个实际子进程通过。该补证不更改Zenpi产品代码，不为新增队列/持久化/强制回收义务代验收。
