# ZS1-011 — packages/agent/src/agent.ts

source_hash: 25b52fda7c8fa09d4d5cc8bcbb04db63e9d785b978c846d8c59a367ba91fe045

# ZS1-011 complete Agent source understanding, master 3.1.19

The controller read all592 lines and18767 bytes of unchanged packages/agent/src/agent.ts in ranges1–305 and306–592, along with the entire worker report, README, probe, actual child-process tool harness, loader, export bridge, runner and source verifier. All20 frozen payloads and5 original source copies match their inventories. The previously accepted010 loop remains unchanged dependency context, not additional acceptance. Node24.19.0 executable identity was checked before and after the run. A private copy of the controller-installed TypeBox1.3.27 dependency used the exact same integrity lock and1385-file complete inventory, verified before and after replay; no package reinstall or source rewrite was needed.

The controller ran the unmodified Agent and loop via the explicit package-resolution bridge. Nine actual scenarios passed with seven real Node children. Every spawn/close PID, started marker, successful stdout/product and cancellation marker was independently checked; successful tools close0 and cancelled tools close23 without a product. The model StreamFn is a deterministic substitute. These tests establish actual Agent state and queue/lifecycle behavior with real tool-process effects, not provider transport or upstream bash-tool execution. Child signal handling, termination and close waiting belong to the fixture tool.

Complete source review identifies unknown zero-capability default model, default user/assistant/toolResult filtering, shallow top-level message/tool array copies and directly exposed nested objects. Queue enqueue is unbounded; all mode drains a copy and one-at-a-time shifts one message. Separate steering/follow-up queues default to one-at-a-time. Clear APIs discard queued input and reset also clears transcript/runtime error state but rejects active work. Prompt normalizes text/images, singleton messages and arrays; image handling here is shape only. Continue rejects an empty transcript, gives assistant-tail steering precedence over follow-up and suppresses a duplicate initial steer poll after pre-draining. Ordinary continuation uses the existing snapshot.

Each run snapshots context/config before the loop emits agent_start. Transport/session/reasoning/hooks and retry cap are forwarded; prepareNextTurnWithContext takes precedence over the legacy callback. The wrapper's preparation callback reads current preparation properties when invoked, while most config fields are captured at run creation. Queue modes can affect future drains, not already-drained snapshots. Should-stop captures its callback and supplies the active signal. Steer/followUp do not abort running work. A fresh AbortController is allocated per run; abort forwards its signal and offers no general tool rollback or forced termination guarantee.

runWithLifecycle installs activeRun and streaming state before execution, catches executor errors through handleRunFailure and always invokes finishRun in finally. The failure reducer builds an assistant error/aborted message with model identity and empty usage, then emits message_start/message_end/turn_end/agent_end. These events can themselves throw through listeners; source finally still clears active state, but complete failure-event publication is not guaranteed for throwing listeners. waitForIdle resolves only after execution and awaited listeners settle; it is a settlement promise rather than a failure-propagating promise. processEvents updates messages, streaming state, pending-call sets and errorMessage before awaiting subscribers in subscription order. agent_end clears streamingMessage but activeRun/isStreaming remain until settlement. Removing a subscription takes effect on subsequent emissions; listeners receive the current run signal.

The nine replay cases establish actual parallel A/B tool completion with B first and source-ordered results, per-turn S1/S2 then F1/F2 drains, continuation's one-item pre-drain and mode/clear/reset behavior, context/config snapshot versus listener mutation and modern prepare precedence, file-barrier async agent_end subscription ordering and rejection of concurrent prompt/continue/reset, sequential and parallel abort with actual child close plus a fresh next-run signal, real ENOENT transform failures producing error/aborted lifecycles without any model-substitute call, and default conversion/image argument/follow-up-all behavior. Runtime evidence is intentionally scoped to those scenarios; other branches above are complete-source observations.

Current Zenpi mapping was checked in core input_queue_request/input_boundary and input-queue state recovery/consume, InputPort service/InputPump cancellation polling, and runtime CancellationToken. Zenpi already provides durable ticket admission, session-owned queue persistence, scope checking and boundary consumption. InputPump services tickets while keeping SessionStore single-writer. Its job cancellation/completion state is distinct from steering and from source Agent activeRun. The worker's historical language about what an added owner should do is superseded by this current mapping; full target snapshots preserve context and do not accept target files or101/102/104. Source Agent itself has no durable input IDs, edit/retract ledger, capacity, restart replay or provider retry guarantee.

Only011 source understanding is accepted. Its source bytes, file scope, dependencies and validators remain unchanged from the historical3.1.15 worker package. The historical report remains verbatim below a current qualification. Other files, directories and complete product/release acceptance remain independently open.


## Historical worker candidate, retained verbatim

The following old status and target mapping are superseded by the current master review above.

# ZS1-011 — packages/agent/src/agent.ts

状态：[_] worker阅读候选，主控未接受。
source_id: SRC-0118
source_path: packages/agent/src/agent.ts
source_hash: 25b52fda7c8fa09d4d5cc8bcbb04db63e9d785b978c846d8c59a367ba91fe045
source_bytes: 18767
read_ranges: [[0, 18767]]
run_id: zenpi-stage1-20260911

完整范围为1–592行。createMutableAgentState在tools/messages顶层赋值时复制数组；DEFAULT_MODEL明确unknown与零能力，defaultConvertToLlm仅保留user/assistant/toolResult。PendingMessageQueue的enqueue无界append，drain按all或one-at-a-time取出，clear丢弃队列；实例分别拥有steeringQueue与followUpQueue，两者默认one-at-a-time。

公共steer/followUp只排队，不abort；clear各队列与clearAllQueues分别清理。prompt/continue/reset拒绝已有activeRun；prompt转换字符串/图像为user，数组直接采用。continue若assistant尾部先drain steer并设置skipInitialSteeringPoll，避免同次重复取条；否则取follow-up，再否则报错。普通continue从现有context继续，不重复添加用户消息。

createLoopConfig把当前model/reasoning/session/transport/retry预算/工具hooks转换为loop配置，prepareNextTurnWithContext优先于旧signal签名；getSteeringMessages用一次skip标志然后drain，getFollowUpMessages独立drain。配置setter影响后续取队列，不会修改已经drain的消息snapshot。

runWithLifecycle在执行前建立promise+AbortController并标streaming，异常经handleRunFailure形成error/aborted assistant及message/turn/agent终态事件，finally清空activeRun、resolve。processEvents更新streamingMessage、messages和pendingToolCalls后按订阅顺序await listeners；agent_end事件后仍非idle，直到所有listeners settle和finishRun。abort只发signal，不等于任意副作用回滚；waitForIdle包含listener处理。

行为映射：zenpi runtime CancellationToken和job Closed/Completed保留原语义；双队列应是已有会话owner内状态，不是新增后台scheduler。源类无持久化、用户可指定ID、单条编辑/撤回、严格容量；这些不能由源码类“已有队列”推断为已完成。源agent-loop.test.ts覆盖loop消费边界；agent.test.ts中continue one-at-a-time测试是context-only定位，不纳入本项额外文件覆盖。目标新增owner需在journal事件落盘后更新状态，恢复applied tombstone，避免重放；实际TUI/JSONL接线与cancel模型请求仍由core/host集成门禁验收。

## 实际源码补证（authority3.1.15）

[_] worker A候选，主控尚未接受。已完整重读1–80、81–360、361–592行；subject及依赖副本与冻结原始SHA在执行前后均一致。先冻结010后补011，010不可变manifest为0d7a286e411aed952a3b98c51e109910dd0f35af76265ee7854b6e229cfe7107。011独立证据包`.ops/source011-ready`，可提交副本`Docs/quality/stage1/ZS1-011/worker-source`；所有实际上下文、事件、监听器观察的Agent state、PID、stdout、close和文件副作用见probe.json及manifest。

| 目标行为 | 源码实际执行观察 |
|---|---|
| steer/followUp整批边界 | 两个真实工具并行；B先完成时Agent.pendingToolCalls只剩A；下一turn仍未开始；steer不abort，A完成后按S1,S2,F1,F2逐turn消费 |
| continue重复取队列 | assistant尾部先取S1并跳过initial poll；显式shouldStopAfterTurn保留S2/F1，下一次continue再取S2，第三次取F1；无输入才拒绝 |
| 模式与清理 | all模式同批S3/S4或F1/F2；独立clear和clearAllQueues、reset均实际清队列；constructor/state顶层数组赋值为浅复制 |
| prepare与运行快照 | agent_start监听器修改state.tools/system/model不影响已创建的首turn snapshot；prepareWithContext优先于旧callback，返回的B工具/model/system/thinking送入下一turn并执行真实B进程 |
| listener/idle | 第一个agent_end listener以真实文件barrier挂起时waitForIdle未完成且isStreaming=true；并发prompt/continue/reset拒绝；按订阅序等待全部listener后才清activeRun，unsubscribe实际生效 |
| 真实取消 | Agent.abort发真实signal；串行只关闭A，B无产品/启动；并行A/B均close23且无产品；新prompt使用新signal。进程回收由fixture tool实现 |
| error/aborted reducer | transformContext执行真实ENOENT，Agent产生error或已取消时aborted的message/turn/agent终态；零provider调用，finally回到idle |
| 默认转换/输入形式 | unknown零能力model、custom role在LLM边界过滤但仍在state、字符串+image输入形状、followUp all模式均经实际Agent验证；图片内容未送真实provider |

9场景、7真实子进程在Node24.19.0通过。未修改agent.ts/agent-loop.ts/EventStream/validation/stream-fn；真实TypeBox1.3.27有锁文件和安装树hash。仅StreamFn为明确替身，不能认作provider/wire完成；工具是probe实现的真实OS执行AgentTool，不冒充上游bash工具。快照只保证顶层数组复制，不夸大为消息/工具对象深冻结。当前authority3.1.15为ownership-only更新，A范围未变；本报告不变更主控8accepted/100open计数，不改产品代码、不新增额外源项覆盖。
