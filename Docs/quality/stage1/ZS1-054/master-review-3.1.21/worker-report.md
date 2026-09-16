# ZS1-054 — packages/agent/src/harness 独立目录理解复核，3.1.21

本轮唯一候选为 `Docs/learn/stage1_pi_mono/packages/agent/src/harness/current_folder_learn.md`。054 的两个冻结直属子目录 050/compaction、051/session 均已由当前 master 独立接受，满足开始本目录复核的前置条件；054 仍待 master 审阅及 G-STAGE --item ZS1-054。本报告不接受 057、061、其他祖先、直属 context-only 文件或非冻结子目录。

Authority 3.1.21，run `zenpi-stage1-20260911`，requirement `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`，baseline snapshot `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`。本轮只读当前蓝图/selector/manifest、旧包、接受链与必要源码，并做新路径离线校验；runtime/npm/build/provider/产品/PTY/budget 执行均为零。所有历史 pass/fail 保持原归属，不混成新组合。

## 物理直属库存与冻结闭包

实际目录 `/Users/wangweiyang/GitHub/pi-mono/packages/agent/src/harness` 有 12 个直属文件和 7 个直属子目录。冻结 manifest 没有任何直属文件，只代表 compaction050 和 session051 两个子目录；其余五个物理子目录 env/execution/runtime/tools/utils 及全部 12 个文件都是 context-only。下面的完整库存逐一记录文件字节/hash和子目录归属；direct-inventory.json 还保存每个直属子目录的实际下一层库存，避免把物理 harness 缩写成只有 compaction/session。

本目录闭包为 `054 → 050 → 012` 与 `054 → 051 → 013`。050 内唯一冻结文件是 compaction.ts，051 内唯一冻结文件是 session/context.ts；branch-summarization.ts、compaction/utils.ts、session 的 jsonl/testing 及其余兄弟存储实现不因容器接受而逐文件接受。上游 `057 → 054`、`061 → 057` 只是路径连接，本轮不沿闭包向上勾选，也不重复接受两个叶子。

## 接受证据及完整阅读的精确复用

| 项目 | 当前 receipt 字节 / SHA-256 | canonical 报告字节 / SHA-256 | 本轮使用方式 |
| --- | --- | --- | --- |
| 050 | 2630 / 2429cc4dc9313fabf980c409635ce193bb32fe70e078eeddc66867e46d71f319 | 25349 / 9543ddaf3a9623ac540074e72d69702bd0fc7353d80d3970c29bd84b2bbb1dd2 | 9 件 receipt artifact 全校验；新全读 master 附录，原 22285 字节候选复用本任务已完成的完整理解和精确字节 |
| 051 | 4549 / 7bbc94dc34fa8156290e685e2e312f706b26bdf97bcdd5c5fdb7e5f40bd05270 | 2497 / 5a8d4594b06835aee2bba3ee5bf12d7ba3dafa028a86a69bce5a358a1bd99ff4 | 20 件 artifact 全校验；新全读目录报告、原 master review 与 3.1.21 rebind |
| 013 支持链 | 4630 / 501a6978b097e4b690b85d0948d1c24c798b568dc6cc4b8e4c980e6b94412904 | 2724 / ca5daf0c09a8694e4a66878b934878d64e0a811459772d1707206d5ebaacf816 | 20 件 artifact 全校验；新全读 leaf 报告及 64 行 session/context.ts，用于确认投影边界，不重复接受 |

以上 receipt 均 complete=true、master accepted，requirement/baseline 与本轮一致，children 分别为012/013。050 canonical 恰为此前候选加 master 接受附录；012 的 865 行全读与 37 项历史 master 测试通过 050 内封存的精确链复用，不声称本轮重新全读 compaction.ts 或重跑 37 项。051 的历次 rebind artifact 保留原文字与 hash，3.1.21 明确保持未变义务；hash 链校验不等于本轮逐份重新语义全读所有 rebind。

旧 A directory054-ready 的 manifest 为 11933 字节，SHA `8423b5cf9f97bceb31782722a4ff364b9a61e8a88155e059558a40fef1628a8f`，59 个 payload 加 manifest 共 60 件。完整复制为 historical054，不运行旧 run.sh/npm 或 verifier。其 3.1.17 / `4b4bbb809fc660d55186317e9e12cfdccc7f3a2ba4567dc94b93d96858584f1a`、050 provisional、051 accepted 是历史状态，原报告在文末原样保留；当前变化是 050 已独立接受，不是把旧四项测试换标成当前执行。

## 两条 context 路径及数据所有权

050 消费调用者已选定的 raw Entry ancestry，找到最后 compaction，组合旧 retainedTail 与新 entries，分出历史、可能切开的 turn prefix 和 retained tail，调用 summary request 并返回 summary/details/usage。它不持有 Session writer，不决定真实分支、队列准入或最终 checkpoint 发布。设置与嵌套消息有共享引用，token/cut 为 heuristic，失败 assistant 或失败 tool call 的文本/意图与普通 provider 投影不必使用相同过滤器。050 接受链已明确 call intent 不等于写盘成功、branch 标签不等于 typed compaction file lists、prompt 不等于语义保真。

051 的 buildContextEntries 只选择最后 compaction 及后续 Entry；sessionEntryToContextMessages 把它转成 compactionSummary 加过滤后的 retainedTail，把非空 branch_summary 转成 branchSummary，普通 message 也过滤 assistant error/aborted/deferred，custom 没有内置映射。buildSessionContext 按源顺序 await 对应 customType 的 projector，把 null/undefined 当作无输出。它创建输出数组，但保留部分 message 引用，不承诺深拷贝；假定调用者已选定正确祖先。没有 projector 时 custom 进入 provider context 的结果为空，也没有 journal/model/tool 执行。

直属 messages.ts 作为 context-only，将上述 summary/custom/bash 角色转换为 provider 可消费的 user 消息；excluded bash 可略过，summary text 有包装标签，基本消息直通。这个转换发生在某一条选定路径之后，不能倒推出 raw Entry 已保存 projector 的外部知识。Context 是跨调用传递的对象；拥有 signal 不等于每一个接收者都主动检查取消。

因此本目录关键集成事实不是“050 的摘要直接等同于051的上下文”：普通 provider context 可包含 custom projector 的结果，而 summary preparation 不会自动调用该 projector；普通 provider projection 过滤掉的失败 assistant，也可能进入 raw compaction 提示词。有效合同应明确两条路径的资格差异，否则 custom pending state 在压缩时可能缺席。这个推论基于实际路径与 fixture 的提示词观察，不宣称真实模型已遗忘某条需求，也不声称项目必须照搬上游差异。

## 必要调用方与配置边界的新阅读

AgentHarness facade 的 create 指向 runtime/harness.createAgentHarness。Options 显式接受 Session、Models、model、tools、toolContext、systemPrompt、compaction/retry、toProviderMessages 和 entryProjectors。runtime 创建默认配置时保留这些 callbacks，默认 toProviderMessages 才是 convertToLlm，默认 entryProjectors 为空。config.ts 拒绝重复 tool name、无效 retry 数值和非有限安全整数的 compaction token counts；这些输入验证不证明 projector 自身有时间/字节界限。

createAgentHarness 先验证配置，restoreSession 恢复 configured lanes，再返回 Harness 和 open operations 描述。配置验证在 try 之前，TypeError/RangeError 可直接抛出；恢复错误被包装成 HarnessFault。restoreSession 在 Session.mutate 的一致读取中收集 branch tip/lane configuration/lane state，缺失组合会产生 invariant error，再交 restoreLaneState；本轮只读必要分类/入口，没有逐文件接受 restore 模块或所有恢复状态。构造/恢复不自动启动 provider/tool/hook/timer；open 描述和后续 drive 才连接恢复执行。

普通 generation 的真实路径为：prepareGeneration 检查 model/active tools → runtime/transcript.readBoundedEntries 在 lane.continueOperation 中从当前 tip 扫描到最近 compaction 并 reverse → readBoundedContext 调用 buildSessionContext（带当前 entryProjectors 和 drive.context）→ systemPrompt/before_request → generation intent → execution/assistant.streamHarnessAssistant。后者先浅复制 message 数组，执行 transformContext，再 await toProviderMessages，最后 request/consume stream。transform_context 和 before_payload 是经 HookRegistry.runWithGate 的 hook；runWithGate 在 gate.admit 内派生 signal 并检查 aborted。任意 custom entryProjector 不是自动注册为这个受 gate 包装的 hook，不能把 hook 的准入行为推给051中的普通 callback。

generation 的模型 request 会组合 drive.gate.signal、telemetry/session identity 并走 gate.admit，response 在 finally 关闭；这些外层请求边界可以阻止某些后续工作，但 D054-03 只执行了 buildSessionContext，不足以证明完整 runtime 对任意忽略取消的 projector/transform callback 都有强制终止能力。

结构化 summary 走另一条已由050理解的路径：lane 接受 compaction 或 navigation，按实际 ancestry 生成 durable preparation；structural.performStructuralAttempt 的注入 SummaryRequest 执行 before_request/before_payload、nested request intent/usage/outcome，并用 gate 进行模型准入。compactWithRequest 收到的是 raw preparation；它直接做自己的消息转换/序列化，不走普通 generation 的 entryProjectors/transformContext/toProviderMessages 组合。历史054的 facade/structural 行号摘录均与当前源核对；本轮还读了真正 runtime 构造与 transcript/generation/execution 的连接，而不是从 facade 的类型声明猜测调用。

## 错误、取消、持久化与恢复的 owner

051 的 projector rejection 直接拒绝整个 buildSessionContext promise，后续 projector 不再调用；此前已完成 projector 的外部副作用并不会由这个函数撤销。忽略 signal 的 projector 可以在 abort 后继续完成并让后一个 projector 执行。050 内 history/prefix 顺序请求对 error/aborted response 返回 Result.err，普通 request rejection 可向外抛；它也不独立执行完整取消事务。两者都不对任意 callback 建立沙盒或强制取消保证。

实际持久化属于 runtime/session：structural.publishStructuralOutcome 经 lane.continueOperation 组合 compaction entry 与 branch tip 更新，branch_summary 使用 navigation target/fromId；失败/取消/重试策略由 operation state、gate 和 runtime owner 处理。orphaned summary attempt 的未知外部结果进入 interrupted/retry/terminal 策略，不是靠 session/context 重放一个数组就完成。这里明确复用050已读的 lane、structural publication/recovery 源码连接及相同 hash；不把本轮四项 fixture 描述为存储事务或进程恢复测试。

Harness.fault 会 seal lanes、关闭 hooks/events 并发出 fault；close 缓存同一 closePromise，seal lanes 后等待 session.close 和 idle callbacks。底层 Session 的锁、transaction、JSONL crash/recovery、fork 和 conformance 属于未逐文件接受的 session 兄弟实现；这个外层调用图不能证明任意后端的 durable atomicity。目录有 session/jsonl/testing 的物理存在，也不意味着本轮已经接受它们。

跨文件应保持的界限包括：选择祖先与发布 tip 必须归同一持久 owner；历史资格/失败标记的差异不能被摘要包装掩盖；自定义 pending state 是否进入压缩必须显式决定；错误或取消后的内存投影不变，不等于外部 effects 已回滚；context/shallow arrays 不等于不可变快照。对这些界限的理解才是054工作，单纯累计 test 数目不能替代。

## 旧四项真实执行的逐项复核

本轮完整阅读 corrected/initial directory.test.ts、两份 observations、结果/stdout/stderr、runner/config/bridge、原报告与 context 摘录。旧包的 27 份源拷贝与当前 upstream hash 逐一比较；bridge 重新导出真实 frozen helper，未改写算法。测试直接调用 buildSessionContext/prepareCompaction/compactWithRequest/convertToLlm，使用 typed-as-any ancestry、scripted SummaryRequest 和本地异步 Promise gate；没有创建 AgentHarness 或 Session backend。

| 旧用例 | 实际可支持的观察 | 明确限制 |
| --- | --- | --- |
| D054-01 | projector 恰调用一次，CUSTOM_PENDING 出现在 ordinary projection；实际捕获的两段 raw summary prompt 均无该 marker；输入 JSON 不变 | 未把 projector 输出写回 Entry；未断言最终 CompactResult 的完整合同，不能推导持久 summary 已发布 |
| D054-02 | ordinary projection 没有 FAILED_ASSISTANT；实际 raw summary prompt 有该文本 | 单一 error fixture，不冒充所有 stop reason/entry type 或模型语义；没有 runtime hook 执行 |
| D054-03 | 最新 cp 排除旧 custom；first projector 进入后显式 gate 等待；abort 后放行仍按 first-start/end、second-start/end 顺序完成；保留 tail、投影两 custom、后续 user；aborted assistant 被过滤 | same Context/real AbortController 可见，但 callback 忽略 abort；这是“可完成”的反例，不是取消安全通过。gate 是测试 Promise，不是 runtime effect gate |
| D054-04 | first projector 抛同一 PROJECTOR_FAILED，promise reject，second 不执行、输入 JSON 不变 | 不回滚先前外部 effects，也没有 journal/OS 恢复 |

初次真实执行 3 passed / 1 failed，失败仅 D054-03：对 JSON.stringify(llm) 搜索含实际换行的 `<summary>\nSUMMARY_PENDING\n</summary>`。JSON 表示中的换行已转义，文本实际存在。唯一 fixture 改动是从整个 JSON.stringify 结果改为 `(llm[0].content as any)[0].text` 上断言；source 无改动。初次 observations 只有01/02/04，因为03在 records.push 之前失败，这三份观察与修正后的同名记录逐项相等，不能把缺少03记录说成没有执行到 gate。

修正后原 Node 24.19.0/Vitest 4.1.9 执行 exit0，四项 pass、零 failed/pending/todo；stdout 记录单文件、总 duration168ms。初次172ms与修正168ms都不是本轮性能测量。历史 Node 身份 `27db838bb204ef7c21df2931f5656e4c8fb32e6e947f363a402b49714d32b5b1` 和 argv/cwd/env 在 validation 中保留，本轮未安装/复建其 runtime。

050 的历史五项、012 的37项、051/013的接受、054的四项分别属于原来证据链；不相加形成“本轮46项测试”。053、406、headless/mio的新 Rust/embedding/release/PTY进展均不能替代054的目录义务。

## 当前 Zenpi 映射及变化范围

与050已接受的限定目标上下文逐字节比较，当前 src/context.rs（30013B，`256d9ac1657e272ba61ebfbc242ece2970ed0607f3ebb1583b1711f18b7bf229`）、src/session.rs（176828B，`8fb3ff2c387b13d34d8bbe4533bb2b0a81b5e546f5e03216b74b5cf2c61c9a2b`）、src/core.rs（278737B，`52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869`）身份未变。完整捕获三文件，只复用此前已完成的相关片段阅读和050 master语义决定，不伪称本轮新全读这些大型文件。

context 的 semantic plan 使用完整 turn、source/session/branch/digest、tool outcome 与 unresolved facts，request data/filter 与 final validate/restore 有明确合同；session 拥有 checkpoint 与 operation correlation 的 journal append；core 拥有真实 provider请求、usage/governance、取消检查、validate/finalize/append及选中历史恢复。这比上游假定 ancestry、任意projector、弱summary response验证更强，但结构验证仍不证明任意自然语言待办保真。自定义 context hooks 的能力/时间/预算属于相应 target owner，不因 source054 接受而整体通过。

本轮捕获与终检仅判定054条目、050/051/013接受链、实际 upstream目录和必要 target context 是否变化。完整authority前后快照保留；主控其它checkbox、Gantt文案、headless31ea/mio41321及其新68项Rust/53项embedding进展是并行上下文，未借用也未混入本报告历史运行。Gantt不是这里的source/context依赖，更不是本目录测试的Cargo输入，漂移本身不使054理解无效。这个局部离线检查不修改任何全局checker政策或master gate。

## 候选结论与回滚边界

050/051 的子闭包、实际物理库存、raw摘要与普通投影的差异、facade/config/runtime/transcript/execution 的调用，以及错误/取消/持久化/恢复 owner 已独立复核。新的离线 audit 只证明所列快照、历史结果与scope的一致性，不替代master语义审阅。无新运行结果，无主库/claims/蓝图/旧ready写入。

本候选完成后冻结停止，054仍待主控接受。撤回仅影响本目录报告，不递归撤销050/051、012/013，不接受057/061，也不触及已保存失败证据。

## 逐一直属库存

| 直属项 | 类型 | 字节 / SHA-256 | 冻结归属 |
| --- | --- | --- | --- |
| agent-harness.ts | file | 21541 / f00e89cdf1412e4193db0207c9358d89116e240fba1823c9db81eb6e8f1f85f5 | context-only |
| compaction | directory | 目录；下一层库存另存 | in-scope ZS1-050 |
| config.ts | file | 1450 / b95461edf229c73ab181a09a4dcc7ad609593e8422c6a5080529d2eff8fc8659 | context-only |
| context.ts | file | 1140 / de3e6afc553327d7fca456f06393039dce033c4c97508649063c285c968bca86 | context-only |
| env | directory | 目录；下一层库存另存 | context-only |
| events.ts | file | 9852 / 8e69fd404703ff88d487b26323e2b8950500c16b4945e115d8a1ab2bedf38769 | context-only |
| execution | directory | 目录；下一层库存另存 | context-only |
| hooks.ts | file | 17258 / 1f86b9da3ab1a80b6896bbc409eb020f947b296765e26aa8a3672f75bbbfe8a1 | context-only |
| messages.ts | file | 4188 / 3173a94aae9a1e064ddae8a9d3ecaf857e2a3b94e801cf844cc6f045d3bf307f | context-only |
| prompt-templates.ts | file | 9407 / edeed31ed4cea3353b47d1061356a898a52b131c9f50100f20bb21f1fca6b554 | context-only |
| result.ts | file | 4055 / 035a0e603043a1d598e49949936dda132dce1f098923f2b5722fbf09ffbebbd1 | context-only |
| runtime | directory | 目录；下一层库存另存 | context-only |
| session | directory | 目录；下一层库存另存 | in-scope ZS1-051 |
| skills.ts | file | 13789 / b88025e1bcf65de30649aa15ca045caadff23ae46a147e1e078a998610ac253b | context-only |
| system-prompt.ts | file | 1179 / a3a8f4e2e343562277d348e99762447131632aea963c9ee4633369e5d1a9e9f4 | context-only |
| telemetry.ts | file | 18212 / 2a93a876ecea19b8d16609b8985c58540c93d12ef29bdac50496fce8208443ea | context-only |
| tools | directory | 目录；下一层库存另存 | context-only |
| types.ts | file | 17172 / ce92e0a00a6b5f7b95ef9f41e909d7a0e5c269956a782c83c65d885c50680770 | context-only |
| utils | directory | 目录；下一层库存另存 | context-only |

## Historical appendix: unchanged original 3.1.17 report

# ZS1-054 — packages/agent/src/harness

Provisional [_], authority3.1.17 / 4b4bbb809fc660d55186317e9e12cfdccc7f3a2ba4567dc94b93d96858584f1a. The frozen direct children are compaction050 and session051; there is no in-scope direct file.051 is accepted (controller receipt/report preserved);050 is provisional. No parent acceptance follows.

directory-scope.json enumerates every real direct file and subdirectory. agent-harness.ts, config/context/events/hooks/messages/prompt-templates/result/skills/system-prompt/telemetry/types and env/execution/runtime/tools/utils remain context-only. The directory is not represented as only its two frozen children.

## Actual connections and ownership

050 prepares/generates summaries from caller-selected raw entries.051 selects the latest compaction and projects its summary/tail plus later entries into AgentMessage values. The context-only messages.ts then converts summary/custom/bash roles into provider-compatible user messages. Context and AbortSignal are passed across these calls, but each callback owns its cooperative cancellation.

The AgentHarness facade forwards construction to runtime/harness.ts. Its structural runtime adapter (context-only, captured source excerpt) calls compactWithRequest with durable preparation, wraps the summary request in before_request/before_payload hooks, publishes nested request intent/outcome, and uses the active gate signal for model admission. Thus the host supplies persistence and cancellation ownership that neither050 nor051 implements alone. This review reads that connection but does not claim to execute or accept the runtime directory.

Four new tests execute unchanged source across the two frozen child boundaries and immediate context helpers:

- D054-01: custom-entry projector output appears in ordinary provider context, but prepareCompaction consumes raw entries without that projector. CUSTOM_PENDING does not reach its summary prompt. Projectors are not implicitly applied twice or carried into preparation.
- D054-02: session projection suppresses error assistant messages, while raw compaction preparation can still include their text in a summary request. The FAILED_ASSISTANT marker demonstrates the difference rather than claiming uniform filtering.
- D054-03: the latest checkpoint excludes older custom entries. Gated asynchronous projectors run in source order with the same Context, followed by later input. Real AbortController cancellation is visible, but an ignoring projector still completes and the next projector runs. Actual messages.ts conversion wraps the summary in provider text; failed retained assistants stay filtered.
- D054-04: a throwing projector rejects the whole projection promise and prevents later projector calls, without changing the input ancestry.

Four passed, zero failed/skipped/todo, real Vitest4.1.9 / Node24.19.0. The initial run had one assertion representation error (searching actual newlines inside JSON-escaped text); exact source/logs are retained. Checking the actual converted text field instead produced four passes without modifying upstream behavior.

## Boundaries and target mapping

050's five tests and012's historical37 are referenced, not repeated or summed here.051's accepted report and3.1.17 rebind are preserved separately. New ancestry/model/projector fixtures are explicit; no real model, queue admission, durable journal, OS restart or production harness execution is claimed. Cancellation/exception paths above are actual calls, not hash-based inference.050 already records JSON projection recovery; it is not relabeled as persistence.

A useful cross-file invariant for Zenpi is that summary preparation and live provider projection must agree about eligible history and custom pending state, or record their deliberate difference. Preserve complete turns, unresolved operations, typed file facts and failure status explicitly. context.rs/session.rs own validated checkpoint/tail restoration; core.rs/backend/cancel ownership controls publication and subsequent use. Tests of summary text alone cannot prove those host guarantees.

The closure is012→050 and013→051, then054→057→061. No frozen child is skipped or double accepted; other real children remain outside the frozen subset. Because050 is not master accepted,054 remains provisional even though its four behavioral checks pass.

