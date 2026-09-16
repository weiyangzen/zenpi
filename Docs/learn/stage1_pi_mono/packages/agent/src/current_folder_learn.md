# ZS1-057 — packages/agent/src 独立目录理解复核，3.1.21

本候选仅对应 `Docs/learn/stage1_pi_mono/packages/agent/src/current_folder_learn.md`。010、011、054 已分别由 master 接受；057 仍待独立语义审阅和 G-DIR / G-STAGE --item ZS1-057，061 仍未接受。本轮没有执行上游、产品、旧 runner、npm、构建、模型、PTY 或预算实验，只新建私有证据及这一报告的可审阅补丁。

Authority 3.1.21，run `zenpi-stage1-20260911`，requirement `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`，baseline `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`。capture.json 保存当前 selector、完整蓝图和索引身份；其它项目并行推进不构成本目录源语义变化，也不改变全局 checker 政策。

## 实际目录与正式子项

物理目录有 7 个直属文件、2 个直属子目录。正式直属文件仅 agent-loop.ts / 010 和 agent.ts / 011；正式直属目录仅 harness / 054。index.ts、node.ts、proxy.ts、stream-fn.ts、types.ts 为五个 context-only 文件；search 为 context-only 子目录。search 下一层实际只有 index.ts，内容为接口。harness 下一层有 12 文件、7 目录，已逐项记录在 direct-inventory.json；不能把整个物理目录写成仅有三个正式子项。

闭包为 057 → 010、011、054，其中 054 → 050、051，050 → 012，051 → 013。057 的父项 061 另依赖 058，本轮不接受父项、额外文件或非冻结子目录，也不重新接受已完成叶子。撤回仅撤回057报告，保留所有子项、历史失败及产品状态。

## 精确接受链与阅读边界

010 receipt 7955B / `d1f8cdc79be1067343fe080876e861b0437545e1d4bd25e3650076b4304e81f1`，011 receipt 7321B / `df42d45b520b6472177059da1b38fc5b37d170aad09d9ad5b70d9630d898d45c`，054 receipt 2632B / `d918808ee31ec05bf3e369a7bc54f69bce49b4f3ce44757c283ab4883a544280`。三者 complete=true / manual accepted，并与本轮 requirement、baseline 一致；分别列出的 39、35、9 件产物全部复制并核验，不从 checkbox 单独推断接受。

本轮完整读了010、011 canonical 报告、原3.1.19 master review、3.1.21 rebind，以及054完整 canonical（含历史候选和接受附录）与 master review。010/011原完整源阅读区间分别是 [0,22796)、[0,18767)，源 SHA 为 `1e16404a231912fbd7643d8317b15ec4cc6245ed8cedc37582a31d430a1cc6ac`、`25b52fda7c8fa09d4d5cc8bcbb04db63e9d785b978c846d8c59a367ba91fe045`。通过已接受的完整理解、原源副本和当前逐字节相等复用，没有把它们虚报为本轮新读1395行。原010的12场景/16进程、011的9场景/7进程属于各自历史 master 执行，不在本轮重跑或相加计数。

054 当前 canonical 为27554B / `9c0045bd2aa32efc790808819f6b97d47e12ec737d67e848edfe980d619e50bc`；当前 worker manifest 为 `8f5cbc32c1b6d4b622b01ac320f2d881509337ad114d8ba3e1744851b050926d`，封存 tar 为 `1784b0dfb5705db50b962c1e26c72d91383d04f0a4bd8202f6cf46033456b6e5`。本轮只读取 archive 成员以核验157 payload，不执行其中脚本；其24份 current-source 与当前上游一致。054 的050/051、012/013完整理解和有界 runtime 调用方阅读由该精确接受链复用。它不是旧057所引用的3.1.17 provisional054被改名后的测试。

新增完整上下文阅读为 index.ts 152行、node.ts 2行、proxy.ts 402行、stream-fn.ts 20行、types.ts 446行、search/index.ts 27行和上层 package.json 89行，共1138行。read-ranges.json 逐文件保存全字节区间、行区间与 hash。它们用于目录集成理解，未变成额外正式文件接受。旧057全部31份源拷贝与当前相同；旧 manifest 13661B / `c014979b18a87abd1c49e983f64e9ea50a43e72aac980baf4bb0d3fee2466f3f` 的62 payload完整保留在 historical057，原3.1.18状态不改写。

## Agent、loop 与 harness 的责任连接

011 Agent 是内存状态和单次运行生命周期 owner：持有公开 messages/tools、pendingToolCalls、activeRun promise、每次新的 AbortController，以及独立 steering/follow-up 队列。公开数组只做顶层复制，消息和工具的嵌套对象仍可能共享。默认队列 one-at-a-time，无容量和 durable ID；enqueue/clear/drain 不包含持久事务。runWithLifecycle 捕获执行异常并尝试发错误生命周期，finally 清 activeRun；awaited agent_end listeners 也是 settlement 的一部分。listener 本身继续抛错时，不能保证完整错误事件序列。

010 loop 负责 provider turn 和工具批次。它在一整个工具批次完成、turn_end 和显式 shouldStop 检查之后才取 steering；pending 输入或工具要求继续时才进入后续 turn。prepareNextTurn 可替换 context/model/thinking，先前已经取到 pending 时不再次 drain；先前为空时准备结束再 poll，接住准备期间到达的输入。follow-up 在内部工具/steering循环自然结束后才取；shouldStop 显式退出不会消费它。并行工具完成事件按完成顺序，toolResult artifacts 按源序；某一工具要求 sequential 时整个批次串行。signal 转交与等待工具不等于强制回收任意副作用。

Agent.createLoopConfig 把队列 drain 和 prepare callback 连接到 loop。现代 prepareNextTurnWithContext 优先于旧 callback。大部分配置在 run 创建时捕获，prepare wrapper 又可读取当前 prepare 属性；公开 setter 不能倒改已经 drain 的局部 pending 数组。普通 transformContext → convertToLlm → getApiKey → stream 是每次请求的转换顺序，不是给公开 Agent.history 自动提交一份压缩记录。

054 harness 是另外一层持久 session、lane、operation、hook/model准入和恢复 owner。Agent 自身不因为与 AgentHarness 同被 index.ts 导出，就自动创建 Session 或把 prepare callback 接成 durable compaction。057旧测试中的适配器是调用者显式提供的：snapshot.messages 转为临时 message Entries → prepareCompaction / compactWithRequest → 人造 compaction 对象 → buildSessionContext → 返回替换 context → convertToLlm。这个回路实际调用了 helpers，但没有 journal writer、tip publication 或完整 AgentHarness runtime。

054已接受的普通 generation 路径是从当前 lane tip 扫至最近 compaction，buildSessionContext 顺序调用 custom projector，再 transformContext / toProviderMessages。结构化 summary preparation 消费 raw entries，不自动走同一 projector 管线。失败 assistant 的普通投影可过滤，而 raw summary prompt 仍可包含其文本。此资格差异会影响跨层摘要合同；不应把“导出了 compaction/session helper”写成“自动保存并恢复全部用户状态”。

## 公共入口、默认流及类型合同

index.ts 是显式出口集合：低层 Agent/loop、harness facade 和 context/session/compaction/reducer/工具等 helpers、proxy、search、types，以及 pi-ai uuid 与遥测 context。stream-fn 只在此导出 setter，内部 getter 仍由使用者模块调用。模块级 defaultStreamFn 起初 undefined；setDefaultStreamFn 可设置或清空，getter 缺省时抛出明确配置错误。它不是自动注册 model/provider 的过程，调用者需显式传 streamFn 或安装默认流。

node.ts 只额外 re-export NodeExecutionEnv，再 export index。package.json 的 name 为 @earendil-works/pi-agent-core、version0.85.1、type module；root/node 和几个 harness subpath 指向 dist 的 js/d.ts，engine要求 Node>=22.19.0。当前只核对声明与源出口连接，未构建 dist、未证明任意 bundler/browser 环境可加载，也未接受上层 packages/agent 目录。types.ts 的 declaration-merging 示例仍写 @mariozechner/agent，和包名不一致，属于已读文档上下文差异，不据此推断运行时错误。

types.ts 描述 StreamFn 必须返回 AssistantMessageEventStream，request/model/runtime failure 应以 error/aborted 终态表达，不能普通 throw/reject。convertToLlm、transformContext、getApiKey、shouldStop、队列 getter 也有 safe fallback 合同；这是调用方义务，TypeScript 类型不在运行时强制。低层 wrapper 对任意 callback throw 不保证正常终态，Agent外层捕获能力也不涵盖 listener 再抛的全序保证。prepareNextTurn 合同允许异步返回替换状态，其错误恢复不能从其它 callbacks 的文档保证类推。

工具类型描述 before hook 在参数验证后运行并可 block；after hook 按字段覆盖 content/details/isError/usage/terminate，不深合并。terminate 需非空批次的所有最终结果同意，usage 为工具自身消耗、不自动计入主模型上下文。late update 在 execute settle 后应忽略；tool.replay 的 never/safe 字段描述未知 durable effect 的恢复策略，但接口声明不让低层 Agent 自动具备 durable journal。shouldStop 的 newMessages 是本次调用新增部分，不能误认完整历史或 Session snapshot。

search/index.ts 只定义 SearchQuery、SessionSearchHit、EntrySearchHit、SessionSearchService；其中只有 searchEntries 可选；searchSessions、sync、notify、remove、close 都是必需方法。notify 同步返回 void，其余这些必需方法返回 Promise。没有索引 backend、embedding、持久存储、重建、鉴权或实际检索执行。本目录导出 search 合同不验收这些服务能力。

## proxy 的请求、事件与异常边界

streamProxy 立即返回实际 EventStream 子类，在内部异步 fetch POST `${proxyUrl}/api/stream`，外层 Authorization 使用 authToken；body 包含 model/context 和显式筛选的可序列化选项。signal 是本地 fetch/reader 取消信号，不进入 JSON，authToken/proxyUrl 也不作为 provider options 转发。options.headers 是序列化 payload 的一部分，不能混称它会替换这个代理 HTTP 请求的外层认证头。

proxy 初始化一个 pending assistant，usage为零，之后在同一 partial 对象上重建 text/thinking/toolcall。text/thinking delta/end 检查已有 block 类型；toolcall_delta 追加 partialJson 并 parseStreamingJson，同时浅拷贝该工具 block 触发响应式更新。toolcall_end Object.assign 完整 toolCall 后删 partialJson，若 block 类型不符则返回 undefined；不与 text/thinking mismatch 的 throw 行为混为一谈。done/error 更新 stopReason、usage、可选 providerThinkingLevel 并发终态。同一个 partial 多次暴露，不承诺历史事件快照深冻结。

传输读取按换行切块，只处理以 `data: ` 开头且非空的行，每行独立 JSON.parse；EOF flush decoder 并处理无结尾换行的最后一行。没有收到 done/error 的 clean EOF 转为错误 assistant，避免把半截流当成功。HTTP非OK优先使用可解析的 error 文本；fetch、reader、JSON、事件类型不匹配等异常归入 catch，signal已aborted时标aborted，否则error，最后移除 abort listener。abort handler 取消 reader，fetch也接同一signal。

这些是对当前完整源码的静态理解。协议为本代理约定的逐行 JSON，不能仅凭这个 parser 声称支持任意多行 SSE data、任意未知服务端事件或全字段运行时校验；TS的 as cast并非schema验证。消费循环并非遇terminal立即break，当前源码也不在finally显式releaseLock；这些边界未新做网络或竞态实验，不报成复现bug。本轮未验证真实代理服务器、认证、模型输出、取消及时性、网络重连或provider安全策略。

## 历史057三项实验的逐项审阅

完整阅读当前三用例、helpers、bridge、runner/config、actual observations、结果及失败 stderr；初始 test 通过完整现有文件与精确两处diff复原审阅。helpers 虽包含processTool实现，057实际只调用 gatedTool，故不能借010/011真实子进程数量给057加分。scriptedStream 使用真实 AssistantMessageEventStream 和确定性响应，summary request也是替身；没有外部provider。

| 历史用例 | 真实可见结果 | 限制 |
| --- | --- | --- |
| D057-01 | summary暂停时排S1/S2/F1，只有首次模型请求；放行后共4请求，每轮末尾按S1、S2、F1消费。provider context含CHECKPOINT_PENDING和完整当前task/toolCall/toolResult，排除OLD_HISTORY；公开Agent.state仍含OLD_HISTORY且不含summary。原summary prompt确含旧历史 | 人造checkpoint对象，无commit或恢复；公开history与request context是不同责任边界 |
| D057-02 | 准备期间晚到steering尚未drain；abort后summary替身返回aborted，Agent结束并保留该队列。随后同进程fresh prompt携新signal、消费LATE_STEER | cooperative callback主动响应，不是强制取消；fresh run不是进程重启，未证明durable replay |
| D057-03 | 工具等待期间排EARLY_STEER，整批结束后先drain，再prepare抛PREPARE_FAILED。仅1模型请求，错误assistant发出，EARLY_STEER既不在queue也不在message_end/history，Agent回idle | 实际输入丢失反例；“错误路径测试passed”不代表队列保持合同通过 |

初始运行是2passed/1failed，失败D057-01要求排除OLD_HISTORY，但keepRecentTokens=1未产生该预期切分。修正仅将fixture keepRecentTokens改8，并新增公开state不含summary的断言；原排除OLD_HISTORY断言仍保留，没有改算法或弱化原断言。修正后的retainedTail实际恰为task、工具assistant、toolResult，旧两条历史在summary prompt。两份代码、失败和成功日志/JSON全部原样保留。

历史最终执行为Node24.19.0/Vitest4.1.9、3passed、exit0；stdout总duration219ms是当次结果，不是本轮性能测试。31份不变源通过 narrow package bridge 导入真实helpers，TypeBox1.3.27由历史lock记录。旧31份源码identity相同允许引用原结果；不把单纯hash比较描述为新行为测试。本轮offline verifier以存档数据重新检查这些具体请求、消息、retainedTail和两处fixture差异，只证明证据内部一致性。

## 目标映射与结论

复用010/011 master已修正的目标映射：Zenpi已有会话owner下的durable input admission、ticket servicing、边界消费和工具并行机制；不能照抄早期报告“仍然只有同步工具、需要从零加owner”的过时描述。复用054的限定映射：context/session/core 分别承担摘要资格与验证、checkpoint/operation关联持久记录、真实请求及最终发布。本轮未重新阅读或声称这些大型target文件当前全字节不变，保存的accepted target snapshots仅代表原审阅时点，不能验收现有产品、101/102/104或其它release项。

可迁移的目录级约束是：队列admission、临时drain与最终commit应有明确失败恢复规则；准备后的provider context和公开transcript/持久session不可混同；普通projection与raw summary eligibility应显式约定；浅复制和传signal不等于不可变快照、强制取消、外部rollback或重启恢复。D057-03把第一条风险具体化，D057-01把第二条边界具体化，054补充第三条边界。

057的子项接受链、完整物理库存、新入口/类型/proxy/search上下文和实际跨文件数据/异常路径已经形成独立候选。新增离线校验器只读本包，以manifest和真实存档内容核验有限条件；不联网、不写文件、不调用任何旧脚本、不代替master语义审阅。117诊断继续冻结，131维持中断；本包封存回传后停止。

## 直属库存身份

| 名称 | 类型 | 字节/行数/SHA-256 | 归属 |
| --- | --- | --- | --- |
| agent-loop.ts | file | 22796 / 803 / `1e16404a231912fbd7643d8317b15ec4cc6245ed8cedc37582a31d430a1cc6ac` | ZS1-010 |
| agent.ts | file | 18767 / 592 / `25b52fda7c8fa09d4d5cc8bcbb04db63e9d785b978c846d8c59a367ba91fe045` | ZS1-011 |
| harness | directory | 下一层19项；详见库存JSON | ZS1-054 |
| index.ts | file | 4045 / 152 / `f335c42e75a0ab39eab9d8a08a01cd8804b720ed89f1f0a25ea12239210132d3` | context-only |
| node.ts | file | 88 / 2 / `63d8dc9b22d93b3b1172ed1f1e83cc62bb370789f56f8dd31affb5c2a0a8dff3` | context-only |
| proxy.ts | file | 11673 / 402 / `248730db4f1bdb84d435295523aa7c9da02c7e09052ce63f0f84d7f338d63878` | context-only |
| search | directory | 下一层1项；详见库存JSON | context-only |
| stream-fn.ts | file | 683 / 20 / `1f9dee101a5ce1052558458d9ce47e53e05187224e879217899059a80d90bc45` | context-only |
| types.ts | file | 17570 / 446 / `640b6703badd73303f9c3c7e903a1269793493499d23e379894cbcb22338a4db` | context-only |

## 原始057报告：历史状态保留

# ZS1-057 — packages/agent/src

Provisional [_]. Authority 3.1.18 / cb96a3db9b57ea0d9d3ad43f0be954129aabf69bc5d438210a14b674a4a8af85. Required direct files are agent-loop.ts (010) and agent.ts (011); the direct child is harness (054). These dependencies remain pending. The immediate inventory records index.ts, node.ts, proxy.ts, stream-fn.ts, types.ts and search as context-only.

Agent owns messages, pending tools, the active lifecycle promise and separate steering/follow-up queues. createLoopConfig binds their drain callbacks to agent-loop. The loop finishes tool batches, polls steering, prepares the next turn, injects pending input, and polls follow-up after the inner loop would stop. Preparation polls steering again only if the previous poll returned nothing.

Standalone Agent does not automatically invoke harness compaction or storage. The preparation callback is an explicit caller adapter. Barrel exports alone do not connect these owners.

## Three actual integration tests

D057-01 connects Agent → agent-loop → preparation callback → actual compaction helpers → session projection → harness convertToLlm. While the summary is gated, S1/S2/F1 are enqueued. Four requests observe the initial tool turn followed by S1, S2 and F1, once each. Prepared context contains the checkpoint summary and complete current tool turn, excluding older completed history. Agent finishes idle without pending tools or queued messages.

The prepared provider context does not replace Agent.state.messages automatically: public state still contains old history and no checkpoint summary. The caller created a checkpoint-shaped object for projection; no journal was committed.

D057-02 cancels the actual signal during summary generation. Agent records an aborted terminal message and clears streaming/pending-tool state. Steering added after the earlier empty poll remains queued; a fresh run in the same process consumes it with a new signal.

D057-03 queues steering during a gated tool, then throws from preparation. The loop already drained the item into its local pending array. Agent settles with an error, but the input is absent from both the queue and message-end events/history. Error cleanup does not prove admitted-input preservation.

All three pass with real Node 24.19.0, Vitest 4.1.9, TypeBox 1.3.27 and 31 unchanged source files. Streams use the actual AssistantMessageEventStream with scripted replies. Tools are in-memory gates. Old 010/011 process-tool evidence is referenced without rerunning or recounting it.

The initial one-token retention fixture ended on a tool result with no later valid cut and retained all history. Its failed expectation and exact logs are preserved. The final eight-token fixture retains the entire current tool turn while summarizing older history. No source or history-removal assertion was weakened.

## Ownership limits and closure

Cancellation, async ordering and throws execute real code. Provider networking, external tool effects, durable checkpoint commit, process restart and the AgentHarness runtime are not tested here. A fresh Agent run is not relabeled as restart recovery.

Zenpi input_queue/core/runtime must preserve admitted input through preparation failures. context/session own validation and durable publication; the caller must reconcile public history and prepared provider context. Existing product evidence remains separate.

The closure is 010 + 011 + 054 → 057 → 061. Child 054 includes 050/051. No context-only sibling or package is accepted by this provisional synthesis.



## ZS1-057 主控独立目录验收 — 3.1.21

主控完整阅读 20238 字节候选（包含原始 3.1.18 报告），逐项核对物理 7 文件/2 目录及正式 010、011、054 三个直接子项。harness 下一层 12 文件/7 目录与 search 唯一 index.ts 均独立列清。非冻结的入口、proxy、类型与 search 合同仅为 context，不从此自动接受其它文件或父项 061。

本轮新完整阅读 index.ts 152 行、node.ts 2 行、stream-fn.ts 20 行、search/index.ts 27 行、types.ts 446 行、proxy.ts 402 行与 package.json 89 行，合计 1138 行。另读 agent-loop.ts 150–305 与 agent.ts 420–592 的实际衔接范围，核对队列先 drain 后 prepare、只在先前 pending 为空时补 poll、事件驱动公开 history、错误生命周期与 finally settlement。010/011 全文件理解通过本任务既有接受链复用；重新完整阅读二者原始主控语义 review 和 3.1.21 rebind，以及 054 主控 review。未把完整复制的其它文件或旧目标快照计为本次完整阅读。

目录集成理解：Agent 持有内存 transcript、队列及每次运行的 AbortController；loop 负责 provider turn 和工具批次，并通过 Agent.createLoopConfig 得到 drain、prepare、转换与终止回调。普通请求使用 transformContext→convertToLlm→key→stream。传入的准备回调可替换下一次 provider context，却不会自动修改公开 transcript 或提交持久 checkpoint。AgentHarness 是独立 durable session/lane/operation owner；同在 index 导出不意味着自动连接。054 已接受的普通 projection 与 raw summary preparation 差异继续保留，不能将取消 signal、浅复制或接口字段当成强制取消、深快照、回滚或恢复保证。

新增入口与类型复核：默认 StreamFn 初始未设置，getter 会抛配置错误；setter 可清空，package/dist 声明不证明已构建或浏览器兼容。search 只有接口，searchEntries 唯一可选方法，notify 同步返回 void；不是可用的检索 backend。类型文档中的 callback 不抛合同不能由 TypeScript 强制，prepare callback 抛错也不能类推其它 callback 的 safe fallback。after-tool 按字段替换而非深合并；全批 terminate 与 durable replay 字段的实际 owner 须区分。包名与旧 declaration merging 示例的差异按文档事实保留。

proxy 完整阅读确认：外层代理认证头与 payload options.headers 不同，signal 仅交 fetch/reader，options 序列化只选择声明字段。事件共享 mutable partial；text/thinking/tool delta 检查 block 类型，toolcall_end 错配却返回 undefined。逐行 `data: ` JSON 协议处理尾部 decoder 与无换行残片，clean EOF 缺 terminal 生成 error。未知事件默认警告而非 schema 拒绝，消费循环不会因首个 terminal 立即 break；catch 可继续修改同一个 partial，finally 仅移除 abort listener，没有显式 releaseLock。本轮是完整静态理解，不把后续共享对象变更、真实网络取消或恶意事件推断写成已执行反例。无完整代理服务器/认证/模型运行验证。

历史 057 测试已完整读取实际三用例、fixture helper、runner、bridge、Vitest/config，三个 actual observations、初次失败及最终 stdout/stderr，并逐字核对初末测试只有两处改变。processTool 虽在 helper 中，但本组只使用内存 gatedTool；不借其它项的真实子进程增加本组覆盖。模型流使用实际 EventStream 与 scripted responses；summary 是受控替身，checkpoint 是调用方人造对象，没有 journal 提交或完整 AgentHarness 实例。

D057-01 的四次请求依次接到 S1、S2、F1，准备 context 有 summary 和完整当前工具 turn、排除旧历史，公开 state 仍保留旧历史且没有 summary。D057-02 在 cooperative summary 取消后保留尚未 drain 的 late steering，新一轮同进程 prompt 使用新 signal 消费它；不是重启恢复。D057-03 明确复现 drain 后 prepare 抛错使 EARLY_STEER 同时离开队列且未进入 message_end/history；测试通过只证明这个丢输入反例，不能记为输入保全成功。

初次 2 pass/1 fail 因 keepRecentTokens=1 未产生预期切分，最终只改 fixture 为 8 并新增公开 state 无 summary 的断言，原排除 OLD_HISTORY 断言保留；真实结果 retainedTail 为 user/task、tool-call assistant、对应 toolResult。原失败和最终 3 pass 为历史证据，不是本轮运行，更不是性能或网络证据。

主控新复制冻结包并完整阅读只读 verify.py，从 /tmp 新运行一次，52 项离线事实全部通过。该检查核对 175 payload、历史 62 payload、子项 83 artifact 和 054 archive 157 payload；不执行捕获脚本。发布前另外逐项检查当前直接库存、源文件身份、三个正式 receipt 及其 artifact。上游身份相同用于历史结果复用；本次没有新上游/产品运行。目标映射限于既有 010/011/054 主控记录的范围，不声称当前大型目标文件或产品整项已完成。

仅接受 057 独立目录集成理解。当前 G-DIR 与 G-STAGE --item ZS1-057 由新发布脚本记录；蓝图要求 digest 保持不变。取消、输入保全、完整压缩、目标实现及父目录继续分别验收，历史失败不撤销。撤回只涉及本目录报告/receipt/状态，不改子项或产品。
