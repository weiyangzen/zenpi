# ZS1-061 packages/agent 独立目录整合候选 [_]

本轮仅交付这一 package 目录报告；正式直接子目录057/src、058/test均有当前独立主审接受回执，061仍需自己的主审。没有正式直接文件；benchmark、docs、scripts与7个直接文件均为context，不因本报告的读取而自动接受。父项064和整个上游包的构建、平台兼容、发布或 Zenpi 产品能力不由061关闭。

Authority 3.1.21，run `zenpi-stage1-20260911`；requirement `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`；baseline `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`。捕获 snapshot `e4197a9fb55d6f4a3a8ab87ebc631bdfecf3c139fb03c5fecac41bed292274b6`；44/121只是当时主控进度，不是package测试数。相关蓝图/索引和最终观察原件随包保存。

## 真实清单与阅读边界

实际有7个直接文件、5个直接目录。全12项列在下表；完整SHA与五目录下一层的每项名称/类型/大小保存在 direct-inventory.json。docs 下一层12文件/3目录仅清点，没有把大型设计文档读成实现证明，也没有递归接受其子树。

| 直接项 | 字节/行 | 范围与用途 |
| --- | ---: | --- |
| `CHANGELOG.md` | 26895/709 | context全文；历史版本与接口迁移 |
| `README.md` | 17606/517 | context全文；消费示例、事件与状态约束 |
| `benchmark` | 目录 | context；1文件/1目录，session脚本全文 |
| `docs` | 目录 | context；12文件/3目录，仅库存与生成目标定位 |
| `package.json` | 2685/89 | context全文；包出口、依赖和脚本 |
| `scripts` | 目录 | context；唯一generator全文 |
| `src` | 目录 | 正式057；7文件/2目录，已独立接受 |
| `test` | 目录 | 正式058；4文件/2目录，仅014为其正式叶子 |
| `tsconfig.build.json` | 492/15 | context全文；src编译与dist声明依赖 |
| `vitest.benchmark.config.ts` | 771/23 | context全文；session benchmark发现 |
| `vitest.config.ts` | 1077/27 | context全文；普通源alias测试配置 |
| `vitest.harness.config.ts` | 1231/34 | context全文；harness发现与coverage |

本轮新完整阅读28份源码/配置/文档、共3603行：7个直属文件、根 package.json 与 tsconfig.base.json、telemetry docs generator、benchmark/tsconfig 和 session 下10文件、src 的 Agent/index/node/stream-fn/messages/session-context 六文件，以及415行 e2e.test.ts。reading-ledger.json 逐文件绑定全字节/行区间及源码副本。这里“完整”指列出的28份文件，不扩张为 harness/runtime、全部测试、所有依赖或整个 docs 的完整阅读。

057完整 canonical 包含旧候选20238字节与5430字节主审附录；本轮完整读取并采用当前主审限定。057回执12件产物全量核对复制，包含175载荷原worker归档；当前主审62份 source input 全部仍相同。058完整 canonical19550字节与3.1.21 rebind完整读取，其原3.1.20主审全文已包含在canonical中；回执59件产物全部核对复制，保留原23例结果、case audit、旧包/bridge/配置和目标快照。总计71个直接回执附件，不把内部归档文件数相加成行为通过数量。

正式子项理解依赖精确接受链：057→010/011/054，054→050/051；058→014。057已限定临时provider context与公开transcript/持久session的区别、drain后prepare失败的输入丢失反例、proxy共享partial与异常传播；058已限定23例真实断言强度、宽容参数修改与目标重新校验责任。这些限定在package层继续生效。不能只写“两子项已完成”便跳过以下独立入口、构建和恢复接线分析。

## 包入口、依赖和构建边界

package.json 声明 `@earendil-works/pi-agent-core` 0.85.1、ES module、Node>=22.19.0。七个代码export分别是根、node、harness/context、harness/env/nodejs、harness/runtime/reducer、harness/session、harness/session/testing，另有package.json导出。每个代码export的dist JS/d.ts声明与实际源路径都核对存在，记录在 public-export-identities.json；这不证明dist已生成或npm tar可安装。没有任意内部文件通配符export，测试直接相对路径导入不能代替公开子路径可用性验证。

根index显式导出低层Agent/loop、AgentHarness、session/compaction/projector/工具、telemetry类型与函数、proxy和search。node.ts额外导出NodeExecutionEnv再导出根。共享barrel并不自动把Agent接到AgentHarness或SessionRepo，也不意味着每个运行环境能加载所有传递依赖。README将SQLite后端放在独立包；本package依赖声明没有SQLite native runtime，不能把独立SQLite后端列作本目录已验证能力。

直接外部运行依赖diff8.0.4、ignore7.0.8、typebox1.3.27、yaml2.9.0固定；chord/pi-ai/pi-telemetry使用内部^0.85.1范围。根workspace提供tsgo native-preview、shx、tsx等开发工具。这里只核对声明，不声称当前安装树或新dependency lock已审计；旧061真实执行依赖身份沿用其原lock和validation记录。

package build只执行 `tsgo -p tsconfig.build.json`。其rootDir=src、outDir=dist，include src/**/*.ts，排除声明输入；继承根ES2022/Node16、strict、erasableSyntaxOnly、声明和source map及相对TS扩展重写配置。路径别名读取chord/telemetry/ai的dist声明，而不是测试用的源别名。根build/build:offline顺序先构建chord、tui、telemetry、ai，再agent；两条链在ai的模型生成阶段不同。由此可见直接单独编译agent依赖这些声明的准备状态，不能由测试源alias成功推导独立消费者构建成功。

clean另有脚本；package prepublishOnly仅调用build，不自动clean或跑测试/coverage/telemetry文档校验。根prepublishOnly才组合clean/build/check；根check还包含 `biome --write`，因此不是本任务可替代portable审计的只读工具。files声明dist和README，不在本轮执行pack来断言最终发布包的全部自动包含项。未编译、安装、发布或执行任何这些脚本。

## 测试、benchmark与文档的真实入口

普通vitest配置使用Node环境、30秒testTimeout、source条件和对telemetry/agent/ai/compat的精确源alias；没有显式include将它限定为014。harness配置显式选择test/harness/**/*.test.ts；coverage的目标为harness和agent/loop，输出text/html/lcov到coverage/harness。test:session:conformance还通过CLI过滤memory-conformance.test.ts。这些是发现与统计边界，不是已经运行的覆盖率或所有case通过。

本轮完整读取当前e2e.test.ts，其10个it使用注册的faux provider与脚本响应，覆盖代码包括basic prompt、calculator、延时abort、生命周期、多轮、thinking和continue守卫/输入类型；afterEach撤销注册。没有从文件名“e2e”推断付费模型/真实网络。它确实通过完整agent源barrel和pi-ai/compat接线，但本轮只读，未执行；并未因此把它添加为058的正式文件。另一端，058的旧23例使用隔离bridge和专用单文件配置，不等同于普通package test。无完整package测试发现/collection执行。

benchmark timing仅匹配benchmark/session/**/*.bench.ts。两个target表当前分别只注册MemoryStorage与MemorySessionRepo，没有SQL/JSONL对比结果。完整读取的公共benchmark helper把read fixture预备/结果校验放在计时外，计时复用同一fixture；write先独立验证，再预备5次warmup+30次measurement所需不同fixture，在callback中逐个shift，耗尽则报错。beforeExit登记异步dispose；这是代码清理机制，不证明崩溃/强杀时回收。

内存launcher为每个target/dataset创建独立Node进程，带--expose-gc/tsx；worker先构建空fixture并GC三次取baseline，生成/导入数据后再GC三次，输出heapUsed/rss/external差值。allocation worker预先生成transaction和空fixture，再通过inspector以4096字节间隔采样commit分配，包含后来被GC对象并汇总前5站点；不是精确对象计数或resident peak。两launcher顺序await子进程，解析stdout，没有本任务的新profile。README明确synthetic数据和“不是CI门禁”；因此这些脚本也不替代117生产冷启动预算。本轮未进入其运行路径。

scripts唯一文件generate-telemetry-docs.ts从src/harness/telemetry的两个schema渲染Markdown表，转义竖线/换行，区分start必需属性、可选end属性和events。直接脚本入口由argv路径与import.meta.url比较，默认写docs/telemetry-schema.md；--check仅比较完整现有文本，不匹配/缺失时throw。普通build不调用它。本轮没有执行render或check，没有声称当前生成doc恰好同步；schema本体和大量docs设计记录只作库存/依赖边界。文档生成正确性、遥测安全性和完整性是另外层次。

CHANGELOG全文保留为历史说明：0.84.4调整prepare运行时机、0.84.0更换lane session并移除旧接口、0.81.1恢复显式streamFn fallback、0.65.0改变state与listener settlement、0.58.4改为整批后steering。这能解释旧报告的措辞为何不能直接用于当前源码。旧0.32“跳过剩余工具”的历史说明仍保留，不能当当前合同；更新记录中的“Fixed”也不等于本轮已重现验证。

## src与test之间：必须由调用方连接的行为

README quick start显式createModels→setProvider→getModel→绑定models.streamSimple给Agent。当前AgentOptions声明streamFn必需，runtime为旧编译调用者容忍缺省并调用模块级getDefaultStreamFn；未配置会在构造期间throw，不会先产生一个正常流。setDefaultStreamFn只修改当前模块进程内变量；换进程不继承，这是旧061第三场景的连接背景。新源index只导出setter，getter是内部路径，不把包名当默认provider注册机制。

Agent持有内存messages/tools/两个队列和activeRun。数组赋值只复制顶层，callback/context可能共享消息对象；sessionId转给provider缓存，不构成磁盘会话ID/恢复登记。默认convertToLlm只保留user、assistant、toolResult；harness.convertToLlm另把compactionSummary/branchSummary包装成user文本，把custom或bashExecution按其规则转换。它们虽然同包导出，Agent不会自动选择harness转换器。临时prepare后的provider context不自动回写公开历史或提交checkpoint，057的D057-01已直接显示这个区别。

本轮完整读取592行Agent，核对文档边界：continue的README与方法注释写末尾应为user/toolResult，但实现对assistant尾先drain steering，再尝试follow-up，有队列时走新prompt；只有没有队列才拒绝。非assistant的custom尾会进入低层续跑，具体provider消息仍由converter决定。故不能把简短注释当严格只允许两角色的完整运行时校验。此处是静态说明，没有追加场景。

事件处理先更新Agent state再依次await listener；普通末尾agent_end之后还要等listener完成才finishRun/idle。低层EventStream消费者则是观察者，消费方await不自动约束producer进入下一阶段；058的wrapper .then拒绝边界也没有因此消失。Agent外层捕获执行错误并尝试错误生命周期，listener再次抛错仍可能中断该序列。模型error对象、异常Promise、idle与持久commit必须分别判断。

buildSessionContext先从给定数组找最后compaction并丢弃前缀，投影其summary及过滤后的retainedTail，再处理后来条目；错误/aborted/deferred assistant过滤。它不沿parentId重新遍历实际存储，也不验证这组数组的ancestry/seq/checkpoint provenance。自定义projector被顺序await，未定义的custom项不产生消息；没有统一持久性、强杀或失败回滚合同。057与058的既有类型/工具参数/队列边界继续约束调用方。

## 旧061实际三场景：按原证据强度复用

旧63载荷包manifest SHA `621c1debcbfd932e5b6341cf69df926a965a53d60b7299b8ac776ffc999934e7` 完整原样复制，当前37份原source闭包逐字一致。完整重读directory.test.ts、restore-child、fixture helpers、loader/bridge、runner/config、validation、原结果/日志和28870字节observations。旧authority3.1.18及子项pending原文保留；现在依赖接受由新回执说明，不重写旧结果。

| 场景 | 实际原始结果 | 证据限制 |
| --- | --- | --- |
| D061-01 | 子进程PID92980，调用方JSON包含old→cp→new；投影为compactionSummary、TAIL、NEW_INPUT，aborted assistant被过滤。显式harness converter后唯一请求含CHECKPOINT_PENDING、不含OLD_EXCLUDED；continue只发新assistant的message_end，idle且pendingTools=0 | 实际文件读取和独立Node进程成立；checkpoint对象由fixture手写，未创建SessionRepo/JSONL journal，没有操作恢复或真实模型 |
| D061-02 | PID92981，投影仍含summary，但默认Agent converter使唯一request只有TAIL和NEW_INPUT | 这是摘要未交给provider的明确负向接线结果；“投影成功”不能代替“摘要被请求消费” |
| D061-03 | PID92986；父进程注册默认stream并排UNSAVED_STEER，child输出defaultAvailable=false、initiallyQueued=false且没有该文本；通过的父测试还断言父queue仍存在 | 进程内默认注册与队列不自动继承；未测试应如何保存队列、crash后恢复，不能写成durable输入丢失修复 |

三个child均用相同当前Node可执行文件、native strip-types与显式alias loader，通过execFile等待正常退出，8秒timeout和1MiB maxBuffer只是旧fixture保护。PID与父进程不同由测试断言；记录保存三个不同child PID、精确args、JSON输入、stdout和stderr。每次比对调用方文件未变，然后finally移除临时目录；新报告保留原输入/输出证据，不声称原临时文件仍存。loader的ExperimentalWarning原样保留，不因测试通过清除。

三请求使用真实AssistantMessageEventStream和scripted CHILD_REPLY，没有HTTP/inference；helper虽定义processTool，本组三例没有调用它，也没有kill/reap工具实验。直接导入Agent与session helpers、窄pi-ai bridge不等于执行完整公开barrel、dist consumer或全部bundle。测试按success正常重建，未注入坏JSON、错误parent/seq、读取权限错误、projector rejection、启动失败、timeout或中途cancel；不能把execFile和异常清理代码阅读说成这些失败都通过。

旧Node24.19.0/Vitest4.1.9结果为3passed/exit0，原stdout总duration1.01s、tests808ms；这是旧执行时间，不是本轮性能样本。058原014的23例、057旧3例及010/011历史真实进程数均不与061三child相加成新通过数。057初次2pass/1fail及fixture修正、058主控准备阶段JSON reader类型错误和修正继续保留在各自接受链里。package层不抹去任何失败，也不把某个错误路径测试pass当相关业务不变量成功。

## 目标映射、交付与回滚

复用当前057/058主审的责任限定：Zenpi backend/registry承担provider与模型准入，core/input owner承担durable admission和准备边界，context/session承担摘要资格、provenance与持久发布。实际目标已有空/非空、一条/全部输入准备测试及受governance约束的工具并行，不能沿用旧报告“还需从零添加”的措辞。061说明的是源包接线义务，不为目标的schema/路径/授权复核或完整会话恢复盖章。

本轮没有重读当前全部Rust owner或执行产品。058旧6个target-context中5个测试文件仍相同；core.rs从274600字节/6a881…变为278737字节/52311…，完整旧副本和本次身份副本均保留，不能把旧审阅行号/旧结果变成当前全文件验证。主控并行TUI修复不在本目录目标映射范围，不能依全局哈希/进度猜测其成败。最后记录捕获时点、相关scope行和源/回执身份；如目标再变则作为时间差异交主审。

新portable verifier仅使用Python标准库读JSON、tar和文件，在内存核对sole report patch、实际库存/阅读区间、71附件、057175载荷archive、058历史证据和旧06163载荷/三原始观察。哈希/断言检查是证据一致性，不取代这份语义阅读。冻结后最多运行一次，原始UTC、argv、exit、stdout/stderr存ready之外；不执行任何旧runner、上游产品、构建、测试、benchmark、117、131或其他探针。

旧061、057、两个059和两个117冻结包逐个记录并在交付前复核，117原fixture保留。工作树非.ops状态及源/正式证据只读，本轮不写主库、authority、receipt、全局checker、claims或BentoBox。只新增候选路径 `Docs/learn/stage1_pi_mono/packages/agent/current_folder_learn.md`。回滚仅撤去这一候选报告，后续主审状态由主审独立撤回；不递归修改057/058、旧证据或产品。

## 本轮唯一离线审计的失败与未执行修正

2026-09-12T23:49:35.319472–23:49:35.399998 UTC，初始196载荷包的唯一审计exit1。失败是058 archive空文件白名单遗漏了 `old058/reuse/source014-ready-run.stderr.log`；该文件确为0字节，其身份已明确列在058主审empty-artifacts.json。另两份空stderr也无实际副本；全部非空副本比较没有差异。修正改为读取接受链中的empty-artifacts记录并核对字节/SHA，避免手写漏项，未放宽任何内容身份校验。

原196载荷包、原manifest和原脚本/报告/patch/README、原始argv/stdout/stderr均保留。修正封装于独立目录，仍可重建首包全部payload身份。依本轮最多一次的限制，修正verifier没有执行；不能声明全项审计通过，后续未抵达的检查也未冒充通过。目录报告和源码阅读已完成，修正审计候选连同这一限制交主控独立核阅。

## 原始061候选全文保留

# ZS1-061 — packages/agent

Provisional [_]. Authority 3.1.18 / cb96a3db9b57ea0d9d3ad43f0be954129aabf69bc5d438210a14b674a4a8af85. The only frozen direct children are src057 and test058, both pending. There is no in-scope direct file. directory-scope.json lists the real package files and other directories, including README/CHANGELOG/package/build/test configs and benchmark/docs/scripts, as context-only.

## Package entry points and actual child integration

The reviewed package.json exports compiled dist entries for the root, node, harness/context, session and selected helpers. Build uses tsgo; ordinary test and harness-test scripts have different discovery scopes. README shows Agent receiving an explicit model stream function. The original public index.ts exports Agent, loop and harness APIs, but merely exporting them does not make standalone Agent automatically compact or persist history.

Our child-directory evidence resolves a narrow unchanged source closure. It does not initialize every root barrel export, build dist, publish npm artifacts, execute the broad package test command or accept unrelated direct files. The original package/config/barrel files are retained for this distinction.

057 establishes actual Agent/loop queue and caller-preparation behavior;058 preserves the exact original23-test execution and its assertion limits, with collection-only verification. Their runs are referenced, not repeated. At package level the missing connection is host ownership of checkpoint representation, provider conversion and process-local state.

## Three new real subprocess checks

Each test writes a caller-owned JSON fixture in a real temporary directory, starts a different Node process, reads that file there, and executes unchanged session projection, Agent and loop code. The child uses native TypeScript stripping and an explicit alias loader forwarding to real source functions. Raw child stdout/stderr, PID, arguments, input and outputs are captured. The file remains byte-identical after the run.

D061-01 restores older ancestry plus a latest checkpoint and later input. Projection produces summary, eligible retained tail and later user input, excluding old ancestry and an aborted retained assistant. With the actual harness converter supplied to Agent, one scripted provider request contains the checkpoint summary. Continuation emits only the new assistant message, then settles idle with no pending tools.

D061-02 uses the same projection with Agent's default converter. The compactionSummary role is omitted from the request while tail and later input remain. A stored/projected checkpoint therefore does not establish that a provider consumed its summary: the host must connect the appropriate converter.

D061-03 registers a parent-process default stream and enqueues parent steering. The fresh child has neither that stream registration nor that queued input. Only the caller's serialized entries are restored, and parent queue state remains intact. This demonstrates process-local state ownership; it does not claim the fixture format ought to contain queue state or implement automatic queue persistence.

All three pass with real Vitest4.1.9 and Node24.19.0. Child replies are scripted through the real event stream; there is no network model. This is actual independent-process reconstruction from a caller-owned file, not upstream JSONL session-store recovery, crash/power-loss proof, journal integrity validation or durable admission replay.

## Exceptions, cancellation and closure

The new subprocess tests cover normal reconstruction and two negative integration boundaries. Active cancellation is supplied by057's separate real signal/summary tests and010/011's historical tool-owner evidence; none is rerun or relabeled as a package-level kill/reap test.058 documents what original014 does and does not assert.

Zenpi's lifecycle owner must explicitly persist admission identity, checkpoint provenance and pending state, then restore the converter/context used by the selected backend. Source Agent public history and a prepared provider context can diverge; a hash or restored object alone cannot prove summary use. Existing target runtime/core/context/session owners must validate those contracts independently.

Root-to-leaf closure is061→057(010,011,054→050/051) and061→058(014). All in-scope immediate children are linked; context-only benchmark/docs/scripts and extra source/test branches add no accepted count. Master acceptance remains pending for the dependencies and this directory. No product, master checkout, ledger, skill or old frozen package was changed.



# ZS1-061 independent controller package-directory review — 3.1.21

Accept only packages/agent's frozen directory-integration obligation after independent current057/src and058/test acceptance. There are no formal direct files. The actual7 direct files and5 directories, plus every next-level entry, were separately inspected and compared to the current source tree. Extra benchmark/docs/scripts and source/test siblings remain context. This decision does not accept parent064 or a whole package build, published consumer, runtime backend or Zenpi product item.

The controller fully read the22581-byte candidate including its historical appendix, all28 source/config/document context files totaling3603 lines, the complete057 canonical including its master review, and the complete058 canonical plus current rebind. CHANGELOG was read continuously1–215/216–709. Agent's full592 lines, e2e's415 lines, entry barrels, converter/projector, package/test/build settings, all selected benchmark/session entry scripts and telemetry-doc generator were individually read. docs designs and unselected harness implementations were inventoried only. Accepted child source understanding is reused through its exact prior receipts and archived evidence; it is not another fresh reading of those entire source subtrees.

The package requires explicit integration. Its seven code exports point to compiled JS/declarations, with an additional package.json export. Agent and AgentHarness sharing a barrel does not instantiate a session repository, compact history or persist input. Agent's default stream remains module-local until configured; modern typed options require streamFn while runtime compatibility permits an omitted value and then invokes the fallback getter. No configured fallback throws during construction. Provider sessionId is forwarded for caching, not proof of a durable session identifier.

Build uses dist declarations for sibling packages; tests use source aliases. Package prepublishOnly runs build alone. Root build orders prerequisite packages; root check includes biome --write, so it is not a read-only substitute for this audit. Declared dist exports do not prove installed-package operation. SQLite lives in a separate package. No install, build, publish, source test collection or TypeScript execution occurred in this controller review.

Current Agent.continue differs from the README's abbreviated last-role rule: assistant tails first consume queued steering or follow-up, rejecting only when neither exists. Nonassistant custom tails can enter continuation and then be filtered by conversion. Queue drain, preparation and durable commit are distinct. The057 input-loss counterexample after drain and failed preparation remains a negative source behavior, not an input-preservation success. Its temporary provider context versus public transcript distinction is preserved. Top-level array copying leaves nested objects shared. waitForIdle's promise resolves on finish; that is settlement rather than success. Listener failures can trigger additional error lifecycle handling and can interrupt it again; finally still clears active state. A listener awaiting the same run's waitForIdle would form a self-dependency, a static observation not a new runtime experiment.

buildSessionContext accepts an already-selected entry array and finds its last compaction; it does not validate ancestry, sequence numbers or checkpoint provenance. Ordinary message and retained-tail assistants with error/aborted/deferred are filtered. Custom projector output is appended without that same filtering pass. harness convertToLlm explicitly converts compaction and branch summaries, custom messages and shell observations; Agent's default converter drops those custom roles. custom.display governs a display field, not filtering by this converter; bash excludeFromContext is checked separately. Summary and command strings are assembled as prompt text, without a generic escaping/provenance guarantee. The source's projection output is therefore not sufficient evidence of a summary appearing in a provider request.

The10 e2e cases use a registered faux provider, not remote model inference. The current source asserts basic prompt state, tool pending-call ordering, cancellation state, selected lifecycle presence/order, context-sensitive Alice recall, thinking blocks and continuation guards/results. The tool-result continuation reply is scripted as8 without a callback checking request contents; its title must not imply stronger provider-consumption validation. The lifecycle test does not assert an exact entire sequence. These assertions were read, not executed. The058 isolated23-case evidence remains its own historical run and does not become full package-test coverage.

Benchmark timing validates fixtures outside measurement, reuses prepared read fixtures and consumes a separately prepared pool for write invocations. The5 warmup/30 measurement constants are configuration, not this review's observed execution counts. Only memory targets are registered. Loaded footprint collects a baseline before synthetic data generation/ingestion; allocation sampling prebuilds transactions before sampling commits. The two measurements have different inclusion boundaries and neither establishes peak resident memory, exact object counts or production traffic. beforeExit disposal cannot establish crash cleanup. These scripts were not run and do not replace117's failed production startup gate.

The documentation generator reads two schemas, emits tables and has an exact-text check mode. It escapes selected table cell values, not every interpolated Markdown fragment. No generated-doc equality or telemetry runtime safety was tested. CHANGELOG's historical Fixed entries explain changing interfaces but supply no current behavioral proof. In particular its old skip-remaining-tools steering description is superseded by the current whole-batch behavior already established in057.

The controller read the original061 test, child, fixture helper, alias loader, narrow pi-ai bridge, runner/config, validation receipt, all test results/raw logs and all fields of the three observations. Three distinct historical child PIDs92980/92981/92986 read caller-created JSON and run unchanged source with scripted CHILD_REPLY through the real event stream. Explicit conversion includes CHECKPOINT_PENDING in the one request; default conversion omits it while retaining TAIL and NEW_INPUT. The third case demonstrates process-local default registration and unsaved steering, including the parent queue's continued existence. The caller file is asserted unchanged. This is real independent-process reconstruction from a hand-written checkpoint-shaped fixture, not SessionRepo/JSONL recovery, journal provenance validation, crash testing or remote inference. processTool exists in the helper but is never called by these three tests. Timeout/maxBuffer and temporary cleanup code do not establish tested failure handling. The experimental-loader warnings and historical3-pass/exit0 result are preserved; none was rerun or counted as a new pass.

Before publication, all28 new context identities,62 source inputs from057,37 historical061 source references, seven public-export source mappings and six target references were checked again:140 records, all identical to their candidate-current identities. The058 old core delta remains qualified: its prior274600-byte snapshot differs from current278737-byte52311…; this review does not claim a fresh full current core reading. The two current master receipts and all71 named artifacts exactly match the copies. Target responsibilities for durable input, checkpoint validation and controlled tool concurrency remain with their separate accepted mapping and product evidence.

The controller fully read the corrected portable verifier, including the accepted empty-artifacts lookup that addresses the original worker's incomplete hard-coded list. A fresh copied verifier was run exactly once, with logs outside the immutable packet:62 integrity checks passed, exit0. It checked206 payloads, the reconstructable196-payload failed first package,71 dependency artifacts,057's175 payload archive,058's43-member archive including declared zero-byte entries, and original061's63 payloads. The original worker failure and its unexecuted corrected-worker status remain preserved; this new independent result does not rewrite their history. Hash/anchor checks supplement semantic reading and are not behavior tests.

The installed report preserves the entire candidate prefix and appends this controlling review. Required directory/item checks are recorded by publication. Rollback removes only061 report/receipts/status and refreshes projections while preserving057/058, every child, historical packets, product code and BentoBox. No sibling, ancestor, release budget or full feature is accepted here.
