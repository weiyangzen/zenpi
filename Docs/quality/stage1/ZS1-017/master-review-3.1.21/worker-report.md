# ZS1-017 · Google Generative AI 单文件独立复核 · 3.1.21

本报告仅对应 `packages/ai/src/api/google-generative-ai.ts`，SRC-0296，16027 字节、526 行、SHA-256 `1395c3f2a90bf701b8b5fc1b6f7de8d323193ed339e07a60630900099a543757`。状态为候选，等待主控独立语义复核；不接受同目录、父目录或 Gemini 产品项。

权威版本 3.1.21；requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`；baseline snapshot `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`；run `zenpi-stage1-20260911`。冻结源 manifest 与当前文件身份一致。唯一拟交付的映射文档是本文件，其余包内容为离线复核证据。

## 阅读与证据分层

本轮新完整阅读源 1–190、191–365、366–526 行，连续覆盖全部 [0,16027)；`reading-ledger.json` 逐段记录实际字节偏移、原文及哈希。新完整阅读必要支持文件 google-shared.ts、simple-options.ts、event-stream.ts，以及当前目标 google.rs；其余目标只读取 `context-reading-ledger.json` 列出的必要片段。完整保存某目标文件用于身份绑定，不代表已完整阅读或验收它。

先只读核实 C 的 source017-behavior-ready、source017-reuse320-ready、source017-main-delta320-ready。三包全部原字节收在 `historical/`，原 manifest、基线、补丁、runner、报告、失败记录均保留，原 runner 不执行。`historical-inventory.json` 绑定每个物理文件；旧 manifest 分别列出 12、40、86 项 payload。三个逐文件报告 7702、20705、32406 字节，哈希分别为 `fa2853bce8507405711530960e8f9cf994271a7bba96c5156dea4dfec5676cc1`、`555192ff2fb817aa4a7aa6e61d80da14de9214b4cbdce6f61478a5f140303a4b`、`485c17e1e492d77bef21ac8b41801d5f3238c706214f6d829d5ce808430d4c23`。本轮读完最新报告全部 144 行，并核对前缀链；旧阅读记录按旧哈希复用，不冒充本轮新阅读。

## 文件完整合同

| 源范围 | 逐段实际行为与边界 |
|---|---|
| 1–52，导入、选项、计数器 | 此文件负责 Google SDK 请求编排、流事件转换和思考参数。共享 helper 负责消息/工具/schema/签名/finish/retry，不能把导入能力全算成本文件实现。GoogleOptions 增加 auto/none/any 工具选择、enabled/budgetTokens/level 思考设置；工具 ID 计数器是模块级状态。 |
| 53–99，建立输出和流 | raw stream 先返回 AssistantMessageEventStream，在异步 IIFE 中初始化可变 assistant output（空 content、零 usage/cost、pending）。自定义 fetch 若不是 globalThis.fetch 即拒绝；缺 apiKey、构建参数、onPayload 和 SDK 建流失败转成 error 事件。onPayload 非 undefined 返回值整体替换 params，不合并或重验。retryGoogleRequest 仅包 generateContentStream 初始 Promise。成功取得 iterable 后才发 start。 |
| 100–172，文本与思考 | 每块只读取 candidates[0]；output.responseId 保留首个 truthy ID。text !== undefined 才处理文本，thought === true 决定 thinking，签名本身不决定类型。同类型连续片段拼入当前块，类型切换先 end 旧块再 start 新块；每块保留最近非空签名。事件 partial 指向同一个可变 output，不是不可变快照。 |
| 174–213，工具 | 独立 if 处理 functionCall，因此一个同时有 text/functionCall 的 part 可走两支。先关闭文本块；缺失或本响应中重复的 ID 使用 name、Date.now、模块计数器重生。name 默认为空；args 仅类型 cast 和 nullish 默认 {}，没有运行时 object 校验。直接发布 start、一份 JSON.stringify(args) delta、end，不等待 finishReason。结束工具块不代表完整响应成功，更不代表工具执行授权。 |
| 216–243，finish 和 usage | 映射候选 finishReason；STOP 且已有工具改 toolUse，MAX_TOKENS 由共享 helper 改 length。每次 usageMetadata 重置整套计数：input=prompt-cache，output=candidates+thoughts，reasoning=thoughts，cacheRead=cache，cacheWrite=0，totalTokens 直接取服务字段，再 calculateCost。缺项使用 \|\|0；未检查负数、有限值、单调性、total 一致性或 cache≤prompt。 |
| 246–291，终止和失败 | 迭代完先 end 当前块，再检查 signal；pending 因缺 finishReason 报错，error/aborted stop 映射为异常；成功发 done 后 end。catch 保留 partial，删除可能的内部 index，按 signal.aborted 分 error/aborted，经 normalizeProviderError/formatProviderError 发 error 并 end。它不回滚已经发过的 text/toolcall_end，也不保证错误文本全面脱敏。 |
| 297–336，streamSimple | 缺 key 同步 throw，与 raw stream 的事件化错误不同。buildBaseOptions 先建立 simple 参数；未给 reasoning 时 thinking.enabled=false。否则 clampThinkingLevel 后 resolveGoogleThinkingLevel；3 Pro/Flash/Gemma4 走 level，其他模型走 budget。这里调用的 context cap 不应泛化到 raw stream。 |
| 338–357，createClient | baseUrl 存在时 apiVersion 设空，让 URL 自带版本；headers 按默认 User-Agent、model.headers、options.headers 覆盖，经 helper 转 record。构造 GoogleGenAI(apiKey,httpOptions)。本层没有 OAuth/Vertex 身份推导或 SDK 无关传输。 |
| 359–416，buildParams | convertMessages 生成 contents；temperature/maxOutputTokens 在不为 undefined 时传入；truthy systemPrompt 先清理 surrogate。非空工具列表使用 parametersJsonSchema，并按共享 strict/model 与 toolChoice 决定 functionCallingConfig。thinking enabled 且 model.reasoning 时 includeThoughts=true，level 优先 budget；disabled 且 reasoning 时调用 disabled config。pre-abort throw，否则 config.abortSignal 接收原 signal；之后 onPayload 仍可把它替换掉。返回 model.id/contents/config。 |
| 419–448，模型族与关闭思考 | 3 Pro/Flash/Gemma4 由 lowercase 正则及明确 latest 别名识别。disabled 3 Pro 用 LOW，3 Flash/Gemma4 用 MINIMAL，均省略 includeThoughts；旧族用 thinkingBudget=0。省略字段不能说成 includeThoughts=false，更不能承诺远端零思考。 |
| 450–484，level | 3 Pro minimal/low→LOW，medium/high→HIGH；Gemma4 minimal/low→MINIMAL，medium/high→HIGH；其他族按 MINIMAL/LOW/MEDIUM/HIGH。模型名启发式和本地映射不等于与服务协商过能力。 |
| 486–526，budget | 自定义值只要非 undefined 即直接返回，包括 0/负值。2.5 Pro 默认 128/2048/8192/32768，Flash Lite 512/2048/8192/24576，Flash 128/2048/8192/24576；先匹配 Lite。这里 model.id.includes 大小写敏感，与前述族匹配不同；未知模型返回 -1。没有本地自定义预算合法区间检查。 |

流循环没有逐 chunk/part/push 的 signal guard，依赖 SDK abortSignal 停止读取，并在末尾复查；初始 Promise 外的 body 断流不在 retryGoogleRequest 重试范围。源没有检查后续 responseId 变化、modelVersion、多候选或 finish 后继续出现内容，也没有本文件级响应/队列总量限制。多个流共享工具计数器，内容状态各自局部；本轮没有并发、背压或资源上限执行证据。

## 必要支持上下文

google-shared.ts 的同 provider/model 且 base64 外形合法签名才回放；带签名的空 text/thinking 保留。异源思考降为可见文本，签名被丢弃；这不是目标当前拒绝不兼容 opaque history 的策略。convertMessages 先经 transformMessages，再生成 user/model parts；Gemini3、claude、gpt-oss 的工具 ID 会规范为有限字符并截至 64，旧 2.5 不在原生 functionCall/functionResponse 加本地 ID。相邻工具结果可合并为 user turn，成功/错误分别 output/error，图像随模型族路由。这里只读取共享文件与可定位调用边界，不宣称重新审完 transformMessages 全部传递依赖。

convertTools 默认 parametersJsonSchema，本源固定 useParameters=false；legacy OpenAPI 降级是共享文件的其他入口，不能说本源实际走它。支持 strict 时 auto/default 可变 VALIDATED，none/any 优先。isThinkingPart 精确判断 true；retainThoughtSignature 留最后非空值。其注释称不跨 distinct part 移签名，但本源实际合并同类连续文本块，因此不能用注释推出逐原生 part 无损保存。

retryGoogleRequest 只为带 status、缺 headers 的 Error 补 headers 属性再委托通用重试；本轮不把其注释的完整重试策略当成新运行证据。simple-options 的 maxTokens 取用户值或模型值，再按估算上下文留 4096 安全 token、至少 1 的空间限制；它转交的 onResponse、timeoutMs、samplingParams、cacheRetention、sessionId、transport、metadata 等字段，并非被 Google adapter 自动落实。event-stream 的 FIFO 无固定容量，push 不等待消费者，done/error 都使 result() resolve 为 AssistantMessage，error 不使 result Promise reject；排队事件持有可变 partial，历史 probe 用 structuredClone 收集事件，故其断言针对拷贝后的观测。

## 旧行为证据的精确复用

源历史 owner 是 worker-source-probe/probe.ts（SHA `aec2b272926225ce8c72505d82953ad95d81fa68d31d105fdb50030af1f17cf1`），本轮已全文读取。它 import 冻结源，Bun.serve 在 loopback 随机端口回放原始 SSE，实际 @google/genai 2.21.0 编码请求和解析响应；未 mock import/client/parser。package-lock 和安装日志原样封存，未安装或执行。fixture-key、fixture model、预设 Response(reply) 证明本地 SDK 路径，不能证明真实 Google 模型推理、分段到达时延、背压或服务兼容性全集。

2026-09-10 source-native 收据 exit0；日志 `ba0f6e777e3d236bb36d09f9b9708aaf41d75aaff562d8f2e6220194817e7d6e`，17 个 case、20 次 HTTP。20 是这 17 个案例的请求数，**不是另外 20 个测试**。实际案例分别覆盖端点/auth/system/output cap；thought 和签名、首 ID；正常 usage；缺/重 ID；early tool end 后缺 finish 的 error；MAX_TOKENS、SAFETY、MALFORMED_FUNCTION_CALL、UNRECOGNIZED 四种 finish；候选0；signed empty 与 foreign 丢签名；2.5 ID-less 工具回放与结果合并；预算/custom 和 3.x 关闭最低档；3.x level/strict；onPayload 改 maxOutputTokens；fetch/缺 auth；pre-abort 无 HTTP。它未覆盖异常 usage、SDK 故障重试、mid-stream 取消、任意 payload 替换、完整思考族或真实服务。

2026-09-10 target-native 收据 exit0；日志 `d5373c17720831ea4079d63c58a9f926bbb39ea7d601eceadb5a587c31862472`，17 passed。owner 是当时 tests/stage1_gemini.rs，含本地 TcpListener/backend 及 CARGO_BIN_EXE_zenpi headless/临时会话重启。当前该文件 19 个 test 定义；后来增加的两个 terminal cancel 测试不在旧 17 收据中。源旧上游测试阅读与目标静态断言均不增加通过数。

旧 replay 命令原样保存在 source-native.json/target-native.json，分别是 NODE_PATH 指向 C 私有 SDK 安装的 Bun probe 与 cargo +stable-aarch64-apple-darwin test --locked --test stage1_gemini -- --test-threads=1。本轮新运行测试数为 0，未运行这些命令，也未重跑任何旧 runner/build/npm/PTY/budget。

## 当前 Zenpi 逐项映射

| 边界 | 当前定位与差异 |
|---|---|
| 请求、认证和分派 | backend.rs:1041/1215/1307 分派 google::request_body/endpoint/read_response。google.rs:57 约束模型 ID 后选择 generateContent 或 streamGenerateContent?alt=sse；251–437 建原生 systemInstruction、contents、inlineData、functionDeclarations、functionResponse、generationConfig。backend:1219–1302 静态确认 Google secret handle/API key 走 x-goog-api-key，发送使用请求私有 cancellation、identity encoding 和有限 body 接收轮询；其旧 HTTP 案例仍只证明当时版本。不把 SDK 对象 config 等同目标 REST 对象。 |
| 参数与输入完整性 | request_body 拒绝 structured output、非法 metadata、远程图片引用、不支持类型、超量附件、工具结果缺失/重复及不完整历史；合并相邻同角色 parts，要求首尾 user；输出 cap 必须正数、最终请求≤32 MiB。源容忍/转换策略不代表目标必须照搬；源温度、任意 onPayload、SDK headers 和 auto/none/any/VALIDATED 工具配置没有完整同形公开映射。 |
| 思考能力 | google.rs:49、416 附近及 registry.rs:255–286 明确 gemini-2.5-flash 的 none/minimal/low/medium/high，预算0/128/2048/8192/24576。源 Pro/Lite/3.x/Gemma4/custom/-1 广泛映射未完整提供。目标 Gemini3 首工具签名检查分支不等于默认支持所有这些模型。 |
| parts 与 ID | google.rs:75–184 校验字段、text/functionCall 恰一、args object、thought boolean、签名可解码且非空；parts≤2048、序列化≤256 KiB、调用≤128。ID-less 由 responseId+part index 的 digest 派生本地 ID，原生 part 不加字段；重复 provider ID 拒绝。源用时钟/计数器重新分配且无同等类型校验。 |
| 文本与工具事件 | Fold.chunk:453–583 拒绝多候选/非0索引/finish后候选，保留各 native part 顺序，发 owned TextDelta/ReasoningDelta/ToolCallDelta。ResponseCreated 在第一份有效解析对象处发出，可能尚无 ID，区别于 SDK start。Fold.finish:584–664 完成全体校验后才发 TextDone、ToolCallDone、Usage、Completed；不是源 contentIndex/start/end 合同的逐字移植。 |
| 完成与身份 | 目标仅 STOP 可完成；需非空 parts、responseId 和匹配的 modelVersion（允许限定数字连字符后缀），检查工具/思考能力，Gemini3 首函数需签名。源 MAX_TOKENS=length 可成功、首 truthy ID 后不核对身份。目标早期 delta 仍可能先于后续错误，但错误不生成成功 Completion。 |
| usage | 目标 u64+checked_add，cache≤prompt、total 一致、累计不下降。目标公共 input 含 cache，output 含 thoughts；源 input 已扣 cache。额外 cache/thought 存 gemini_finish annotation，公共 Usage 不含源全部 cost。没有 usageMetadata 可保持 None，不能说目标强制服务提供所有计数字段。 |
| 签名与持久化回放 | assistant_parts:185–250 核对 wire/provider、parts serialization digest、canonical text/calls；thought/signature 的跨 model 历史拒绝。digest 是本地完整性比对，不是密码学验签。core.rs:3855–3882 保存 completion.content、annotations/model、canonical calls，再 append_turn；后续 request_body 取原 parts。源降级外来签名/思考的策略不等价。 |
| 取消、重试和终止事件 | read_response:665–744 有读循环、最终 finish 及每个 sink event 前取消 guard，JSON/SSE 有总响应/行/帧边界，拒绝截断帧；因此 terminal batch 中取消后后续事件受 guard 阻止。backend:1587–1702 只在本次尝试未发布事件、错误可重试、次数可用且非 semantic_compaction 时 retry。transport:58–103 请求私有取消与线程 join；非 macOS resolver、非 Unix connect 仍有明确平台限制，不能从旧 pre-abort 案例推出全平台立即取消。 |

当前 tests/stage1_gemini.rs:415–476 的三次请求/两次 CLI 恢复断言要求原 parts 一致、ID-less functionResponse 不补 ID；:883、:888 的新测试分别在 TextDone、首 ToolCallDone 取消，断言 late 为空、ready 数符合边界且无 Completed。本轮只读这些真实断言，未执行。终止批中首 ToolCallDone 已发布再取消可以保留该条 ready，不应误写成每种取消都零工具完成。

## 当前版本差异与失败保留

当前 google.rs `f0d56a364cda38b5a10bff74e68057ad76bf539000c883ee6e820c2105081890`、backend.rs `d11105597a7e4260c67c9ce7796394687b4219617a6ebfdfeca637ae469c3357`、transport.rs `d166d173c77aec88ccee1c248b74424a20c9783b6f4cada95fc762cbf3fe1cfd`、core.rs `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869`、registry.rs `cf229a62681933e7c1b4058c4d2fada41dff4fcd05c4cd300e18accfe245b944`、stage1_gemini.rs `0b89fa8b1587cfbd1ed36fb53b7280d79586acd3323da1e2b4280cb033d85367` 与 C main-delta320 绑定的六个文件一致，其13个旧映射片段原字节复用。历史最早 target owner 的漂移记录依然保留；六文件与上一映射一致不能推出整个程序等同最早测试二进制。

当前 headless.rs 是 368974 字节、`521c2102519b502fbd6184f31ffb53cba8eeef840bf84d19450836429d9b49f0`，headless_domain_owner.rs 是15243字节、`6f121e2ea3e1f7f0ffc19d8470f94662163b1eb1e78cd3a71c97975ad06361e6`。新读 headless:2636–2688 与 test:332–407：显式 id@version 精确查找，仅省略版本且唯一时选择；缺版本不得 fallback。回归涵盖 single@missing、single@x@y 拒绝，single 唯一、multi@2、label@x@y 成功，multi 裸 ID 拒绝；失败需 store 原字节不变，成功 queued 并精确 digest/version，未启动 zenpi/provider。此为 domain blueprint 版本选择，不是 Google 模型版本匹配修复。它会改变当前 CLI 组合身份，故旧 Gemini CLI 冻结收据绝不能声称当前全量一致；本项不验收此 domain 行为。

C 两轮 G-STAGE 原收据都是 exit1，structural.ok=true，原因缺主库 ZS1-017.master.json；原 log SHA `7006622e80e458e030f0d63b1e5033f6bd6279845b948ab937a3b20ccf1d5fb5` 完整保留。准备阶段误用 src/registry.rs 的 FileNotFound 说明也保留。源17case无失败收据不等于不存在这些独立门禁/准备失败。旧报告早期 includeThoughts=false、pre-header取消历史措辞按后续纠正理解，不覆盖或改写原文。

## 可移植离线包与提交边界

包根 `verify_offline.py <包目录>` 使用 Python 标准库，只核对包内身份、源完整范围、上下文、旧证据/失败、补丁正逆数据重构和唯一报告，不运行旧脚本或产品。`--live` 可在原工作机附加当前相关源码、017权限记录及旧 ready 不变检查。权威只比较当前版本/digest/baseline/run、017 对应行及源 manifest/index 行；不因其他任务勾选或 Gantt 漂移更改全局门禁，也不把此局部门禁当主控验收。

新运行行为测试数 0。旧冻结输出、产品、主库报告、claims、authority 均不修改。此包仅提供 017 单文件理解及当前可定位映射；未闭合的 SDK故障、并发/背压、mid-stream取消、跨平台传输、完整模型族、真实服务测试仍如实保留，由相应 owner 独立处理。
