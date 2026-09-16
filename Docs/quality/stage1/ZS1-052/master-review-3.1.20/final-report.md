# ZS1-052 — packages/coding-agent/src/core/extensions

Worker candidate: [_], provisional. 022/023 are independently written candidates, not accepted [x]; therefore this integration report does not pass G-DIR acceptance yet. Source subset contains only types.ts and runner.ts. Other direct files below were fully read as context and are not promoted to extra final per-file artifacts. No direct subdirectories exist.

## Direct inventory

- `index.ts`: 4204 B / 193 lines, `3502526cb694e66e79b729a72956dd8be4be4556c8c60c3a0b534ac3bf7ad1ce`; context-only direct dependency.
- `loader.ts`: 25877 B / 809 lines, `c96284a217cd4e2eee8d7335a7c25b106135968c716ce39834b4714f5ff8f771`; context-only direct dependency.
- `runner.ts`: 39774 B / 1286 lines, `6d5101ab0551c2ddd904a8089cc221b3c448fcfe36c27e36da02cdacc869c084`; selected candidate file.
- `types.ts`: 63623 B / 1797 lines, `96e20f8038027f0b0172f311b6b9d9ddf42123b2f5b2a018f533af026fd3371e`; selected candidate file.
- `wrapper.ts`: 1832 B / 45 lines, `c89397e268ccf27aa465fa1307fdbe324a3457966803e3fc6ec4d982ae2d379d`; context-only direct dependency.

## Verified calls, state and errors

index.ts is a pure barrel of loader/runner/types/wrapper exports (fully read). loader.ts (809 lines, read 1–300/301–580/581–809) uses jiti with virtual bundles or aliases, checks factory exports and caches by cwd+generation. Runtime begins with throwing action stubs, guarded API methods, idempotent invalidation and tracked event-bus unsubscriptions. Per-extension load stages flag defaults/provider changes, commits after factory success and discards subscriptions on failure. Runtime registration is immediate after bind. Individual load errors collect while later paths continue. Discovery scans direct TS/JS or one-level index/package manifest entries, then project→global→explicit paths with resolved-path deduplication. No recursion beyond one level; source symlinks are followed for candidate resolution. This loading model is context only, not a target requirement to embed jiti.

runner.ts consumes loaded handler maps/tools and binds host actions; wrapper.ts fully read (43 lines) adapts registered tools to guarded current context, compares active tool lists before/after execution and reports additions only when no previous tool was removed. Interception itself belongs to AgentSession/agent-core, not wrapper. Context and result transformations are serial; tool-call block wins and exceptions propagate, other emitters generally catch and notify. Loader invalidation tears down bus subscriptions; runner context checks prevent captured old APIs after session replacement. No evidence of a subprocess kill/reap guarantee exists in this source directory.

## Target integration

115 will negotiate a subprocess manifest, serialize typed hook events, bound request/result/time, revoke a host-owned session-generation lease and join/reap all owned work. Tool dispatch must use existing schema/path/approval/policy owners after rewrite. Canonical native provider history remains immutable. Failed reload stages an entire replacement before publication; old capability revocation is only after success. Actual positive/negative/cancel/restart validators remain the implementation item's responsibility. Directory review never accepts parent core or unselected siblings.

## Candidate validation follow-up

115 actual owner candidate evidence is in `Docs/quality/stage1/ZS1-115/worker-hooks`: real HTTP and production JSONL exercise input chaining, typed results, lifecycle, schema/path/approval/policy revalidation, bad replies, kill/reap cancellation and new-process lease identity. `extensions-input-event.test.ts` was additionally read completely (125 lines): source continue/transform/handled, image preservation/replacement, chaining, source/streaming behavior and exception isolation. It was not executed. Target intentionally supports continue/transform text only; no handled/images or transcript replacement API. Input/context failure aborts that admission/request without publishing partial output; lifecycle/after-result failures keep owner state. The directory remains provisional until main accepts its independent child file reports.

# ZS1-052 独立现状补充 [_]

Authority 3.1.18，requirement_digest `cb96a3db9b57ea0d9d3ad43f0be954129aabf69bc5d438210a14b674a4a8af85`。按主控明确要求，在055→059→062依次独立冻结后执行本补充。旧052报告4209B、SHA `1fab8fd0dc7b1fc91db8277eeb2d84d9643bc75340ec0d2284842d486d2d771f`仍原样位于extension115-ready/reports；prior052-report.md保存相同副本。当前交付目录报告只在旧内容后追加本补充，不回写旧包。旧报告中的未来时态和“未执行source test”属于历史时点，不能当作当前状态。022/023及052主控master receipt仍缺失，provisional不变，没有继承或新增接受标记。

## 子集、原报告与新增证据的区分

冻结子集仅types.ts（022，63623B/1797行，SHA96e20f8038027f0b0172f311b6b9d9ddf42123b2f5b2a018f533af026fd3371e）和runner.ts（023，39774B/1286行，SHA6d5101ab0551c2ddd904a8089cc221b3c448fcfe36c27e36da02cdacc869c084）。直属共5文件、0目录；index.ts4204B/193行、loader.ts25877B/809行、wrapper.ts1832B/45行全部context-only。旧正文一次写wrapper为43行是笔误，旧inventory45行正确；本补充不通过改这个数字新增覆盖。inventory.json逐个scope/hash；没有未申报递归目录。

022后来真实源码测试12场景已实际调用defineTool与全部运行时判别函数：defineTool原对象/handler不变，guard只按toolName判断，畸形对象也可能匹配，均不是schema验证或授权。023后来真实Runner测试20场景覆盖输入/上下文/工具/生命周期、错误分类、guarded context、注册分派及UI提示深度；唯一mock import为theme常量。完整两份原报告及原probe/日志/收据在original-evidence。023的较早18场景日志保留，不与最后20场景相加；没有把Bun擦除类型当tsc，未跑完整upstream/UI。

本轮新增9个组合场景调用实际types.defineTool、Runner和context-only wrapper/tool-definition-wrapper。handler、runtime/actions/model/session输入是显式fixture，唯一模块mock仍是theme；没有mock上述被验证实现。不是AgentSession、loader、jiti或事件总线真实运行。source-combination.log SHA `776aac850c297f902877203e000b133e40ba43129f7f57fd150808923f0a31c4`，9/9通过。

## 目录实际连接

index是loader/runner/types/wrapper导出barrel；type-only导出不运行接口。loader为每个扩展构造handlers/tools等Map，以jiti加载factory（源码/打包/Node发行方式选择不同alias），缓存受cwd+generation控制。factory执行阶段暂存flag默认和provider变更，成功后commit；失败discard本次subscriptions/pending。createExtensionRuntime开始为抛错action stubs，Runner.bindCore注入真实actions并刷新provider队列。API通过runtime.assertActive拒绝已失效作用域。目录发现按project→global→显式路径，resolved-path去重，单层TS/JS/index/manifest入口；source允许部分符号链接候选，不能直接复用为目标的路径安全规则。本轮重读上述连接代码；旧报告已有loader全文件阅读。本轮没有执行loader的模块缓存/失败回滚/订阅清理，故这些仍只是context-only源码观察。

Runner按加载的扩展顺序、每个handler注册顺序串行执行。types为参数/结果约定，Runner实际决定input/context一般catch继续、tool_call抛错传播/block短路、tool_result合并且隔离单个异常。wrapRegisteredTool只创建当前guarded context、调用真实definition.execute、前后比较active tools；它不发tool_call/tool_result。读取AgentSession._installAgentToolHooks确认拦截由Agent.beforeToolCall/afterToolCall调用Runner，回调执行时读取当前_extensionRunner；reload可替换runner。外部AgentSession及tool-definition-wrapper仅连接上下文，不扩冻结文件集合，也没有在本次测试中执行完整AgentSession。

## 九个真实组合结果与状态边界

| 场景 | 实际结果 | 责任解释 |
| --- | --- | --- |
| before→defineTool/wrapper→after | before把string参数改成123，execute实际收到123；更新回调执行；新增tool被标注；after details合并 | 源hook后无schema复验；fixture显式串起宿主调用顺序，不假称wrapper自动拦截 |
| 只执行wrapper | before/after计数均0 | 拦截责任在宿主/AgentSession连接 |
| preabort且tool忽略signal | signal同一对象、已aborted，execute仍运行并返回 | wrapper只传信号，不强制终止 |
| cooperative preabort | tool调用throwIfAborted，执行副作用前抛错，wrapper传播 | 合作式取消是tool实现责任 |
| 调用前invalidate | getActiveTools guard报错，execute不运行 | stale检查在实际入口生效 |
| await期间invalidate | tool恢复后已执行一次副作用，wrapper读取activeAfter时拒绝结果；捕获ctx属性也抛stale | 拒绝结果不等于撤销副作用，不证明进程kill/reap |
| 同时移除旧tool并新增tool | wrapper返回原result对象，不附加新增names | additions只有旧集合全部保留才发布 |
| before_tool异常→执行 | 显式await拦截时异常阻止execute | 需要宿主遵守拦截调用顺序 |
| shutdown异常→invalidate | 第一handler抛错，第二仍运行，记录错误；ctx仍可读直到显式invalidate | terminal事件不是自动回收；runtime invalidation只执行一次 |

这些9例与023旧20例部分语义重叠，不总计为41种覆盖。没有外部工具副作用，场景计数使用内存变量；不宣称OS进程、持久化失败、并发加载、消息总线退订异常或完整重载恢复已执行。源码ctx属性有guard，但in-process闭包、已开始的异步工作和引用并不会凭类型声明被撤销。

## 当前目标的实际映射与已执行证据范围

| 当前目标连接 | 本次源码确认 | 与Pi组合差异 |
| --- | --- | --- |
| extensions.rs ExtensionCatalog::load/register_with_lease | TOML API1/2、路径/manifest界限、有序目录、声明工具 | 原生可执行扩展，非jiti/JS closure；拒绝目录/manifest符号链接等独立规则 |
| extension_runtime.rs prepare/chain/input/context/before_tool/after_tool | HookKind是有限集合；请求/结果typed JSON，chain边界2s，hook64KiB | 无源UI/provider任意mutation/handled-images/transcript替换全兼容承诺 |
| core.rs prepare_extensions/configure_extensions/publish_extensions | idle、非worker前提；先candidate+registry；取消检查和journal写入后发布，旧runtime.close在替换时执行 | 失败准备保留旧集合，源factory阶段partial commit不能直接视为同等事务 |
| core.rs工具执行链 | 原policy/schema检查→before_tool→validate_hook_call复验→prepare_tool审批/策略执行；only Success进入after_tool | 不接受source已演示的number参数绕过；after不能把拒绝/失败变成功 |
| extension_runtime.rs process_request/exchange_frame | spawn前后取消/lease检查，request ID与capability完整匹配；Unix supervisor持有非阻塞stdin/stdout、期限/字节界限、OwnedChild回收 | 超出in-process signal转交；不依赖逃逸后代关闭pipe；非Unixfail-closed |
| close/Drop与ExtensionLease | close先revoke旧lease，再用私有单次closing lease通知，随后撤销；重复close不再执行 | 与source shutdown emit不invalidate的实测差异明确；能力不是可反序列化授予的authority |

当前target snapshot及SHA在inventory；core仅读取上述连接区段，不借snapshot文件大小声称全core复核。115原包与native fixture/pipe/轻量管道fixture修复包manifest/hash在target-evidence-references.json。历史17工具hook场景和12真实管道场景归属于对应115证据；后者timeout/cancel/revoke/unwind及setsid持有端点的测试结果不是本052新执行。主控已通知115夹具修复合入，报告依旧以冻结证据原source/时间边界为准，不把主控当前220总通过数搬来作本目录覆盖。主控接受/预算、非Unix、真正OS隔离、取消前已发生副作用可回滚均不由此报告证明。

## 门禁、原件保留、回滚

独立verifier校验5直属文件/0目录、selected和context源SHA、022/023manifest、原证据、旧052原件以及9场景执行收据；哈希校验不是行为测试。G-STAGE只读主仓按3.1.18运行，缺052master时保留exit1，机器结构不授予语义接受。补充冻结包单独前向apply/hash及reverse恢复验证。主仓尚无052报告，因此交付为新文件，内容含逐字节历史前缀及本补充；不改extension115-ready中的原件。回滚仅撤回新052报告/本次证据；不得递归删除022/023、改旧包或回滚已合入115产品代码。

## 2026-09-12 · 3.1.20 独立逐目录复核补充

本节是当前目录理解记录。此前 **12,934 bytes**、SHA256 `837ef4965b4ab5655f860a0da913bd6b06340412a7afae36a38789cd183cca47` 完整保持；其中更早 **4,209 bytes**、SHA256 `1fab8fd0dc7b1fc91db8277eeb2d84d9643bc75340ec0d2284842d486d2d771f` 也逐字节保留。旧正文的候选状态、未来时态、测试计数、取消/清理断言只代表各自历史时点，不能覆盖以下新限定。旧目录包 manifest 为 `dfff741af4ace48098e522e5e1e97da3a4aede913ff5ebfdd43112a99086f367`，整包只读复制到新证据 history/old-ready，未执行其中任何 probe。

当前 authority3.1.20，requirement digest `8d525b351d066ce9b0487a485337647d47233275782e4f4d154423a623fe1645`，捕获 blueprint snapshot `d13f344a5974c5ef00ba247b166235aa02a331ec59b807c368c207c3d8007af3`。唯一 canonical 为 `Docs/learn/stage1_pi_mono/packages/coding-agent/src/core/extensions/current_folder_learn.md`，主库捕获时 absent。本轮只交付报告及可逆补丁，不写主库、不产生目录接受回执、不改状态。

### 正式覆盖闭包与时点变化

冻结目录行 ZS1-052 属于 L2，直接依赖且仅依赖 ZS1-022、ZS1-023。其上层闭包为 ZS1-056(core)→ZS1-060(src)→ZS1-063(coding-agent)→ZS1-064(packages)→ZS1-065(root)；本报告不跳过或接受这些父目录。当前直属清单恰为五个文件、零直接子目录：

| 直属文件 | bytes / lines / SHA256 | 本目录角色 |
|---|---|---|
| types.ts | 63,623 / 1,797 / `96e20f8038027f0b0172f311b6b9d9ddf42123b2f5b2a018f533af026fd3371e` | 唯一正式子文件之一，ZS1-022 已接受 |
| runner.ts | 39,774 / 1,286 / `6d5101ab0551c2ddd904a8089cc221b3c448fcfe36c27e36da02cdacc869c084` | 唯一正式子文件之一，ZS1-023 已接受 |
| loader.ts | 25,877 / 809 / `c96284a217cd4e2eee8d7335a7c25b106135968c716ce39834b4714f5ff8f771` | context-only，有限连接范围，不授予逐文件验收 |
| index.ts | 4,204 / 193 / `3502526cb694e66e79b729a72956dd8be4be4556c8c60c3a0b534ac3bf7ad1ce` | context-only；本轮完整看导出边界仍不计正式子项 |
| wrapper.ts | 1,832 / 45 / `c89397e268ccf27aa465fa1307fdbe324a3457966803e3fc6ec4d982ae2d379d` | context-only；完整工具适配连接仍不计正式子项 |

两份已接受 canonical、master receipt、完整 publication 与独立 review 均先冻结并阅读：

| 子项 | 当前 canonical 身份 | 正式接受来源 |
|---|---|---|
| 022 | 50,912 bytes，`e14d2c4437ac989fb9c5d8ef8d9c4570da50c9914a03952662862013fa9427ad` | `Docs/learn/stage1_pi_mono/receipts/ZS1-022.master.json`；`Docs/quality/stage1/ZS1-022/master-review-3.1.20/`，manual decision accepted，仅 types 源理解 |
| 023 | 36,543 bytes，`2b80ebfd84522d809b8cc4c64fc6895e97c8024c63f5686dcf2084f87f58b185` | 对应 `ZS1-023.master.json` 与 `ZS1-023/master-review-3.1.20/`，manual decision accepted，仅 runner 源理解 |

两 receipt 的 integrated_revision 都记录 `6f252a20c628e9b1ede14e2887acc04657c71d7c`，属于已有主控证据，本 worker 未创建或签发它们。022 publication 的 after-promotion 是当时26项接受快照，023为27项；不把全局数字视为目录行为覆盖。它们均明确不自动接受052或产品115。

实际版本比较：旧目录包与现在五个直属源码的 hash **全部相同**，不能虚构 types/runner 源码漂移。漂移的是旧子报告与接受证据：022 旧4,460 bytes / df6babe6… 变为50,912 bytes；023旧4,771 bytes /26579360…变为36,543 bytes。当前报告完整语义、限定和主控回执替代旧“候选未接受”的当前状态结论。旧12个 types、20个 Runner、9个组合案例与新022报告中的历史13个 helper 来自不同证据时点/fixture；不相加、不重跑、不换绑成当前45或其他产品测试数。旧052的 wrapper43行笔误仍在历史原文，正确45行已在旧补充和当前清单说明。

目标 context 中 core.rs 从旧93953e4a…变为当前 `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869`，extension_runtime.rs 从3cd763f9…变为 `002169d07e9d41a3f4f2526c5bbc3b2ecb63e4f6a648e37adefb7f98edf727aa`；extensions.rs保持 `38a242eff3ec911f4560e3dea3868b742d511205ff0ccab1b622962b41e420ed`。本轮重新读对应连接片段，不把旧17 hook/12管道等运行结果绑定到改变的完整输入。完整差异身份在 drift.json、target-drift.json。

### 类型 → 构造 → 绑定：谁持有和调用什么

types 的 ExtensionFactory/Extension/Runtime/Actions 是所有权和形状合同；defineTool只原样返回对象，名称 guard只相等比较，不注册、不验证任意JSON、不授予能力。自定义 isToolCallEventType 的 string 参数没有非空检查：空字符串与 event.toolName 同为空时也按相等判真（源码推导，本轮未执行）；外层null仍非安全unknown guard。不可把 guard 当注册/参数校验。

index 是导出边界：type exports擦除后无接口实现，value exports连接 loader、Runner、helpers、wrapper。称它 barrel 不等于证明一次导入没有依赖模块初始化副作用；本轮未 import 源程序。

loader.createExtensionRuntime 创建共享 runtime，动作起初抛未绑定错误，refreshTools起初no-op，provider两类注册暂存队列。createExtension 创建每个扩展独立 handlers/tools/commands等Map和来源身份。API.on把handler追加数组，registerTool至少检查 parameters 是非null非数组对象，再登记 definition并refreshTools；不是完整TypeBox校验。factory与commit在同一try中执行，失败走discard，由 loadExtension 把错误归为path消息，后续扩展正常继续加载。此结论限所读 loader177–306、430–483、527–649，未宣称全809行重新接受。

加载期 flag默认和provider变更暂存，正常commit依次发布后转active。若commit中后一个apply抛异常，之前写入的flag/provider副作用不会由此代码回滚；discard先标failed，然后逐个退订，一旦退订抛，clearPending及原始错误的重新抛出可被中断。旧文“成功commit、失败discard”必须带此非事务限定；源码没有完整回滚保证。

Runner.bindCore把宿主动作赋入同一runtime，保存context actions，再分别flush两个provider队列，最后换成即时registry回调。handler或provider错误会emitError，但error listener抛可终止flush，已成功注册项留下，队列未必清空。注册→绑定不构成事务。AgentSession._bindExtensionCore把消息发送、entry/name/label、工具查询/刷新、model鉴权/切换、signal/abort/compact、provider更新等接到真实host；void消息方法内部以Promise.catch报告错误，不是返回给扩展的持久化确认。

Runner.bindCommandContext仅保存模式owner提供的动作；默认 newSession/fork/tree/switch 返回 cancelled:false 却没有实际会话动作。AgentSession._applyExtensionBindings转交 _extensionCommandContextActions，并设置UI/error listener，仍不能从默认返回推断session已建立。未读取/执行全部模式owner的新会话流程，不补造“默认真实newSession”。fresh withSession合同需宿主供应，旧ctx不会被类型自动迁移。

### dispatch → result → 工具宿主

实际连线是：types约定event/result → loader保存handlerMap和tool.definition → Runner创建guarded ctx并串行dispatch → AgentSession agent-core before/after hook调用Runner → wrapper适配definition.execute。**wrapper本身不发送 tool_call/tool_result**。AgentSession.beforeToolCall每次读取当前runner，将同一args引用作为input，Runner throw继续向agent抛以阻止执行；afterToolCall再应用内容/错误/usage结果和图像归一。图像归一、agent最终工具调度/终止批次、session写入属于目录外host，不在052新增正式覆盖。

wrapToolDefinition转发name/schema/prepareArguments/executionMode及params/signal/onUpdate，ctx由factory提供。wrapRegisteredTool先后读取guarded activeTools，await真正execute后比较集合：只在旧工具均未被移除时附加去重的新增名字；若同时移除旧工具，新名字标注被抑制而返回原result。它不会强制检查AbortSignal或撤销已经执行的副作用。调用前失效可在getActiveTools处阻止execute；await中失效可能在activeAfter读取拒绝结果，此时工具副作用已经发生。

| dispatch类别 | 合并/短路与共享对象 | 目录边界后果 |
|---|---|---|
| generic session-before | 最后一个truthy**整个对象**获胜，cancel真即停；普通事件忽略返回 | 不可把summary和label自动逐字段累积；{}可覆盖前结果 |
| tool_call | 同一event/input原地共享，最后truthy结果，block短路，异常直接传播 | 本层无hook后schema复验；terminate批次条件只由更外层负责 |
| tool_result | shallow currentEvent，仅非undefined content/details/isError/usage逐字段覆盖 | false和空数组有效；不深合并，catch不回滚之前或原地nested mutation |
| message_end | 同role整消息替换，非法role报告错误；modified仅记录显式替换 | 原地修改再返回undefined可能泄漏，最终持久化不在Runner |
| context | 只对初始messages structuredClone，此后数组可共享或替换 | 非每扩展独立快照；clone本身失败可在handler catch外逃出 |
| provider payload/headers | payload任意非undefined结果（含null/空串）替换；headers原对象原地改、返回替换对象忽略 | 与context/ToolResult不同；错误前副作用无回滚，无统一安全schema |
| before-agent/resources | systemPrompt链与ctx getter同步，messages累积；资源三类path带ext.path按序追加 | 不去重/校验资源path；后半失败不撤销已追加，宿主另行加载 |
| input/user_bash | input transform链、handled立即停；images null/undefined保旧，[]清空；user_bash首truthy停 | 输入最终值/引用相等可返回continue而已有原地mutation；不是统一last-result规则 |

上述差异沿 types→Runner→host逐条保留。此前“other emitters generally catch/continue”的简写只有在错误listener不抛时成立；不是通用隔离保证。

### 错误、取消、撤销与UI并发不变量

1. **错误出口会再次抛。** emitError同步遍历Set，无catch；handler catch中的emitError listener异常可中止剩余handler，让dispatch reject。默认无listener也不自动变成持久日志。ToolCall本来就直接抛；不能用“统一错误隔离”概括。
2. **失效guard是真值字符串。** Runner与loader都以staleMessage的truthiness检查，显式 `invalidate("")` 不让guard变真。默认非空原因先写入再调用下游清理；下游抛后，再次默认invalidate不会重试。直接emit/query/shutdown并非全部都有guard，已获取的UI/session/model引用亦不是可撤销代理。
3. **退订不保证全清。** trackedUnsubscribe先active=false并从Set删除再调用底层unsubscribe；异常时剩余订阅循环/clear停止。loader/runtime stale已设，可能不再重试。dispose只catch abort块，之后invalidate、断监听与cleanupSessionResources仍可被异常截断。clear与事件发出均不是持久成功确认。
4. **取消是合作式。** signal可经ctx/execute转交，但Runner无每handler前后强制abort、timeout或统一取消循环。一个永不完成的Promise会阻塞串行分派；失效不会回滚已开始异步工作或工具副作用。旧内存fixture的preabort/await invalidate只能说明那次合作/guard行为，不是OS kill/reap。
5. **UI depth共享。** UI通过spread复制，再包装五种prompt；并发重叠prompt也共用一个depth与首个prompt身份，只在最外0→1/1→0排一对通知。Promise.finally覆盖拒绝，通知microtask不await emit；listener异常可异步逃出。没有为每个窗口独立生命周期或自动终止promise的保证。
6. **getter与回调绑定有区别。** command context用descriptors保留lazy guard，createContext的model/scopedModels捕获创建时函数，而若干其他动作查runner当前字段；重新bind函数并不统一刷新旧ctx。类型readonly不是deep isolation。
7. **reload顺序不可混用。** Pi AgentSession先shutdown、invalidate旧runner，再settings/resources reload和buildRuntime；失败不由此片段恢复旧runner。类型声明不是failed-reload保旧事务。default newSession的cancelled:false仍不意味着真实动作。

### 目标反向连接与有意差异

当前目标八种HookKind对应36个source subscription中的有限表面，不能把其余28项或UI/provider/command/render/trust能力隐去，也不能把目标host存在同名功能当作插件可调用API。所有下述目标结论均为当前有界源码理解，**本轮没有运行产品**。

- extensions.rs register_with_lease先clone registry，全部注册成功再赋；API2工具需要session runtime。与source每扩展Map/Runner first-tool查询不同，不直接移植source所有冲突策略。
- extension_runtime.chain按catalog有序值串行请求，检查lease/取消和有界结果。Input仅Continue/Transform文本，没有handled/images；Context只请求局部instructions，不授予canonical历史替换。BeforeTool允许typed对象rewrite或deny，保id/name；AfterTool为Output JSON，并非content/details/isError/usage各字段patch。
- core工具路径先检查原deny和原参数，before hook之后再次validate_hook_call，再走prepare_tool审批/策略；拒绝不能被rewrite绕过。只有Success进入after hook，hook或压缩失败保留原结果。这与source允许原地args mutation、Runner不复验、source richer result明确不同。
- core.prepare_extensions在idle/host非worker条件下准备candidate和registry；configure先cancel复查、append selected generation，publish才close旧并安装新/start。failed preparation保留旧lease是目标自身合同，不是Pi reload的等价结论；没有把静态顺序当成所有磁盘/发布失败的原子性测试。
- target lease含session/generation/capability和active状态。close先撤销旧能力，使用独立临时closing lease通知，再撤销；lifecycle errors收集后继续其他extension，mutable链错误传播，与Pi error listener链不同。非Unix可执行pipe显式不支持；Unix OwnedChild尝试group/direct kill和wait但忽略错误返回，不能凭源码宣称所有后代/OS回收成功。

src/core.rs、extension_runtime.rs、extensions.rs，以及Pi AgentSession/tool-definition-wrapper都只是context；052不接受它们整文件、115/117、父目录或全Pi API parity。旧产品89 Rust/15入口、新diff45等任何测试数都不属于本目录证据。

### 本轮独立阅读范围与离线验证

23个精确连接片段附带原文件全hash、闭合行范围、半开字节范围和片段hash，见 context-index.json；完整文件快照只为校验范围，不扩大阅读/接受。类型/Runner已有主控整文件接受，本轮先读两份**完整当前canonical**及各独立主控review/receipt，再围绕连接重读以下范围。

| IDs | 文件与实际行范围 |
|---|---|
| R01–R03 | types.ts 409–515、1015–1084、1588–1797 |
| R04–R06 | runner.ts 270–490、590–650、723–1286 |
| R07–R09 | loader.ts 177–306、430–483、527–649，context-only |
| R10–R12 | wrapper.ts 1–45；index.ts 1–193；目录外 tool-definition-wrapper.ts 1–47，均context-only |
| R13–R16 | AgentSession 482–536、875–901、2528–2700、2841–2875，context-only |
| R17–R19 | target extension_runtime.rs 35–90、203–426、641–685，context-only |
| R20–R22 | target core.rs 2104–2210、4500–4580、4678–4725，context-only |
| R23 | target extensions.rs 269–311，context-only |

G-DIR结构核对只确认冻结子集为022+023、双方已有source-only master receipt、没有直接子目录/遗漏/重复接受；上述调用/数据/异常语义是独立人工审查内容，机器结构不能替代。G-STAGE `--item ZS1-052` 最终实际exit1，structural.ok=true，唯一错误为缺 `Docs/learn/stage1_pi_mono/receipts/ZS1-052.master.json`；semantic verified_by_checker=false。此缺口如实保留，未创建receipt或改[x]。

准备历史也保留：最初误查worker不存在的canonical目录和旧包不存在的README；context捕获首次把完整47行 tool-definition-wrapper 末尾误填49，被断言拒绝后改为真实EOF；第一次运行私有门禁缺validate_blueprint辅助模块，启动失败并保留空stdout/326B stderr，补齐只读源码副本后才得到上述真实门禁结果。这些不是产品失败，也未用它们冒充缺052 receipt的门禁结果。

本轮 **Cargo/Node/PTY/HTTP/network/product/FIFO执行全部为0**，无依赖安装、任务/subagent、主库/Pi/product/authority/claims/status/旧ready写入。离线verify只检查全payload、子receipt引用、历史前缀、目录清单、片段、实际门禁回执，以及两种精确报告补丁正向/反向；不运行旧probe、产品或receipt内命令。子项通过提供052审查前提，不自动提供052接受。

交付包含create-report.patch（main absent）与append-report.patch（接收方恰有12,934B历史前缀），二者互斥且输出同一完整canonical。其他基线必须由主控协调，不能覆盖。回滚只撤回本次报告创建或补充，不动022/023报告、receipt、已合入产品或旧052历史包。主控还需独立审查本目录语义、集成唯一报告并签发052自己的master receipt；交付后worker停止。


## 主控目录独立复核 — 3.1.20

主控逐项核对当前物理五文件、零子目录，正式子项严格为 ZS1-022 types.ts 与 ZS1-023 runner.ts。两份当前 canonical、master receipt 与当前源码逐字节一致，先前完整逐文件人工阅读仍绑定同一源 hash；R01–R06沿用该完整阅读，未声称重新全读。R07–R23共17段连接上下文在本次独立读取，全部23段另与真实源码范围/hash匹配。index/wrapper虽全文作为连接背景读取，仍不新增正式覆盖。

本目录理解确认 types/loader/Runner/AgentSession/wrapper 的注册、绑定、分派、结果和撤销关系；保留 hook 后参数复验、失败 reload 保旧等目标有意差异。直接读取54行历史组合 probe：9项是手工 host/runtime/callback 夹具调用真实源 helpers，theme mocked，没有执行 AgentSession/loader，没有实际子进程回收保证。本次不运行旧 probe，不合计旧12/20/9或新13计数。

加载 commit/discard 与 runtime invalidate 均可能因回调或退订抛出而部分完成；错误监听器也能中断分派。工具包装器只转发signal，不强制abort；失效发生在await期间时，拒绝结果不能撤销工具副作用。上述边界与当前源码相符，不能把目录接受理解为全Pi插件API兼容、115运行验收或父目录接受。整项接受仅在安装本唯一报告、独立目录回执和实际G-STAGE通过后成立。
