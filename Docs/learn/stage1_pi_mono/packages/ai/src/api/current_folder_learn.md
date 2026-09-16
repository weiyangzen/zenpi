# ZS1-055 packages/ai/src/api 目录复核 [_]

Authority 3.1.17，requirement_digest `4b4bbb809fc660d55186317e9e12cfdccc7f3a2ba4567dc94b93d96858584f1a`。本报告为 provisional 候选，不继承子项接受状态。捕获时 016/017/029 的主控 master receipt 均不存在。本轮不修改产品源码，也不扩展冻结源文件集合。

## 范围与直属清单

冻结子集只有 016 anthropic-messages.ts、017 google-generative-ai.ts、029 openai-completions.ts；各文件完整原报告和实际源码探针的原件副本在 `Docs/quality/stage1/ZS1-055/worker-directory-review/original-evidence/{016,017,029}`，本报告不替代其逐文件复核。目录有 32 个直属文件、0 个直属目录。精确字节数、行数、SHA 和逐个 scope 见 inventory.json；三个源 SHA 分别为 `8e105cc5b2dd304547ed613b70de4740c4db93cee830dc452c2de945052b12b7`、`1395c3f2a90bf701b8b5fc1b6f7de8d323193ed339e07a60630900099a543757`、`5874bf0b121db117bd4eba8fffa750252f5f7617f38306c6eb1f746575896319`。

另外 29 文件仅作直属 inventory / 连接上下文：anthropic-messages.lazy.ts、azure-openai-responses.lazy.ts、azure-openai-responses.ts、bedrock-converse-stream.lazy.ts、bedrock-converse-stream.ts、cloudflare-ai-binding.ts、cloudflare.ts、constrained-sampling.ts、github-copilot-headers.ts、google-generative-ai.lazy.ts、google-shared.ts、google-vertex.lazy.ts、google-vertex.ts、lazy.ts、mistral-conversations.lazy.ts、mistral-conversations.ts、openai-codex-responses.lazy.ts、openai-codex-responses.ts、openai-completions.lazy.ts、openai-prompt-cache.ts、openai-responses-shared.ts、openai-responses.lazy.ts、openai-responses.ts、openrouter-images.lazy.ts、openrouter-images.ts、pi-messages.lazy.ts、pi-messages.ts、simple-options.ts、transform-messages.ts。没有对其余 provider 宣称完整覆盖。

## 实际调用与数据流

三个 adapter 互不调用。各自 lazy factory → lazyApi → lazyStream 同步返回外层 AssistantMessageEventStream，异步 import 本 adapter 后调用 stream/streamSimple。forwardStream 按顺序转发同一事件引用，末尾等待内层 result；不是复制快照、重排器或校验器。三个 factory 都没有启用 deferred fetch/cancel 能力。外层 setup rejection 构造空 assistant/error，usage 为零，模型身份来自传入 model；因此入口不同会改变缺鉴权异常的传播方式。

streamSimple 先处理鉴权、通用采样/输出上限和 reasoning，再调用各自 stream；stream 构造请求、SDK、累积消息和流事件。simple-options 合并模型采样默认值与调用者覆盖，按 contextWindow 减估算输入及 4096 余量限制输出，预算 thinking 至少给答案预留 1024；这属于请求参数计算，不等于真实 token 计量或 host 花费落账。Anthropic 使用 Messages SDK 与原生 blocks；Google 使用 GoogleGenAI，经 google-shared 转换 contents/parts；Chat 使用 OpenAI SDK，处理 choices/delta/tool_calls 与多种 reasoning 字段。SDK版本分别0.124.0、2.21.0、6.40.0，旧 probe 的 package/lock/install 文件保留可核查依赖来源。

Anthropic/Chat 的 transform-messages 在请求历史上保留同模型签名、清理跨模型不兼容签名、规范化外模型 tool ID 并关联 result；丢弃 error/aborted assistant，给未配对调用补合成错误 toolResult。Google 有自己的 parts/signature 转换。合成结果只修整后续请求上下文，不证明工具执行过，也不能代替持久化 journal 的事实结果。utils/event-stream 位于本目录外，仅为连接上下文：FIFO、terminal 识别、result promise 与可变 partial 引用的语义，不属于本目录新增文件覆盖。

## 事件、异常、取消和恢复

| 边界 | 实际源码组合行为 | 证据与限制 |
| --- | --- | --- |
| 正常流 | start → 分块 start/delta/end → done，result 与终态 message 是同一对象 | 新 lazy-native 3 个成功场景逐个断言，真实 SDK 和本机原始 SSE，3 次 HTTP |
| 鉴权缺失 | 直接 streamSimple 同步抛错；lazy 调用返回流，只有 error，空 content，没有 HTTP | 旧三份源码 probe 验证直接调用；新3场景验证 lazy，没有 mock imports |
| 预取消 | signal 传至 adapter/SDK；终态 error 事件 reason=aborted，没有 HTTP | 新3场景，终态唯一、result identity 均断言 |
| 末尾错误 | 三者都检查 signal 和合法 stop/finish reason；缺终态变 error，保留已有 partial 并清除临时解析字段 | 旧真实源码23/17/33场景包含结束原因、参数修复、tool end 早于后续错误等；不是本轮新增73场景 |
| Chat 兼容 EOF | 显式 supportsFinishReason=false 可按是否有工具推断 stop/toolUse；普通模式缺 finish_reason 必须报错 | 029 原探针和当前读取的源码尾部相符 |
| length | 源 stop reason 可以 length 并发 done，不能自动等价于工具参数完整或目标可接受结果 | 旧映射场景；目标策略另见下表 |
| 取消传播范围 | lazy 本身不取消 import promise；通用 stream 不证明外部进程退出或会话落账 | 本轮仅执行 preabort，未执行半包中断、网络停顿、并发消费、模块加载失败及恢复重试 |
| 可重放性 | source history 转换是下一次请求的数据准备；错误 partial 仍可被调用者观察 | 没有源码持久化/session 恢复测试；不能将合成 tool result 当成执行证据 |

23/17/33 是各独立 source probe 的场景数，不与9个目录组合场景或历史 target cases 相加称端到端覆盖。016 初次同模型ID预期错误及修正后的成功记录原样保留。actual SDK 是未修改依赖；loopback server、model/context/options 是显式测试输入。没有外网模型调用、Bun tsc、upstream 全套测试或性能预算结论。

## 当前目标的实际责任映射

当前目标完整快照和 SHA 在 inventory.json 的 snapshots 中；读取重点是函数入口、原生分派、finish 和校验边界。目标测试仍沿用原证据的旧 source hash，不声称本次重新运行当前 Rust 快照。

| Source 责任 | 当前目标函数/数据边界 | 差异及验收归属 |
| --- | --- | --- |
| Anthropic 请求/blocks/terminal | src/providers/anthropic.rs request_body、Fold::event、Fold::finish、read_response；backend.rs 原生分派 | finish 要求 message_stop、合法 blocks 与 stop/tool 一致，能力允许后才发布 ToolCallDone/Completed；source 的早 toolcall_end 不能当 host 执行授权 |
| Google 请求/parts | src/providers/google.rs request_body、Fold::chunk/finish、derive_calls/read_response | 目标校验 responseId、请求模型与 modelVersion、parts/signature/capability；不是 source 宽松候选零消费策略的逐字移植 |
| Chat 累积/终态/reasoning | src/backend/chat_stream.rs Stream::event/finish/read、Reasoning::fold/annotate、validate_history/replay | 当前已有结构化 reasoning 历史校验，不能沿用旧报告“尚无此能力”的表述。finish 完整检查 tool IDs/names/arguments 后才发完成事件；BTreeMap 按索引收集与source首次出现顺序有差别 |
| 请求控制 | backend.rs complete_with_control、transport::send_json、各 read_response/read(cancelled,sink) | 短接收轮询、取消谓词和 sink 返回错误由 Rust host 路径承担，不存在 TypeScript lazy import 的同构目标目录 |
| 模型与 reasoning | providers/registry.rs ModelDescriptor::effective_capabilities/validate_reasoning；backend.rs validate_model/model_descriptor | exact provider/id 描述与 wire 能力相交；定价为整数带来源，未知不是免费；没有由 source 所在目录推出目标 owner |

目标源 snapshot 只证明本次读到的实现；主控的当前整体验收、会话重载、工具重新校验、预算持久化分别归属其 owner，不由055承接。历史target-native日志和owner-hashes保留原时间/范围，禁止提升为本次当前目标测试结果。

## 验证、交付和回滚

新增 lazy-native.log SHA `4582475b24653c4df9a61a0689d3f9e90cb45e6f8827727c37305e9acd6f1907`，9/9通过、3次HTTP；runner receipt 记录实际命令、时间、HOME/CODEX_HOME保留。evidence verifier 校验直属inventory、源/快照、原依赖证据与日志收据，不把哈希核对当行为测试。G-STAGE 单独保留主仓只读运行结果；缺master回执时不得宣布接受。冻结包由patch前向/反向全字节验证。回滚仅移除本055报告与独立证据目录，不递归删除子报告、不修改旧冻结包或产品源码。


## 3.1.21 独立目录复核追加（Worker B，ZS1-055 候选）

本节是当前判断；前文 3.1.17 历史报告逐字保留，不把其中当时的依赖未验收状态、旧目标快照或测试数字解释为当前状态。本轮仅交付 `packages/ai/src/api` 目录候选，不签发 master receipt、不关闭依赖、不修改主线勾选。正式子项只有 ZS1-016、ZS1-017、ZS1-029。捕获时 016、017 有 3.1.21 master accepted receipt；029 只有刚完成的 Worker B 候选。目录 055 本身仍待主审。父目录 059 和其他 provider API 不由本报告验收。

权威绑定为 blueprint 3.1.21，requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`，baseline `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`，run `zenpi-stage1-20260911`。检查范围是 055、三个正式子项的实际行、源文件身份与相关代码；不以无关 checkbox、Gantt 或总完成计数作门禁。最终权威快照及差异观察另存，不能因总文件哈希改变就宣称本目录失效，也不能悄悄把子项中途验收变化改写成捕获时已闭合。

### 实际阅读与复用边界

`reading-ledger.json` 记录本轮 41 段实际阅读及字节/行区间：所有 11 个 lazy 文件、lazy 主体、7 个小型/共享文件中的对应全篇、四个目录外 provider 工厂、event-stream 全篇；compat 注册与 models 实际分发片段；当前 Anthropic 请求入口和 finish/sink 片段；旧 055 报告与完整 probe；016 当前 canonical 全文与 rebind；017 主审追加全文。9 个大型非正式适配器仅新读 1–60 行的导入/接口边界，不能称为全文件理解。

正式源文件逐字阅读证据按身份复用：016 的 master receipt 覆盖 `[0,10223) [10223,18033) [18033,30114) [30114,39526) [39526,48616)`；017 本任务此前完整阅读三段 `[0,6110) [6110,11521) [11521,16027)`，现由主审 canonical 与 11 个 receipt artifacts 绑定；029 本任务此前完整阅读七段至 62416 字节，577 文件候选包原样嵌入。这里没有把 receipt/index/hash 校验冒称为本轮重读源代码。Google-shared 全文、constrained-sampling 全文、Google/Chat 当前目标及后端/transport/core/headless 的相关片段，沿用本任务 017/029 的已读证据；必要的真实调用边界本轮另行读代码，结论不只来自文件列表。

### 目录职责及调用关系

该目录承担 provider 协议请求构造、消息历史转换、SDK/传输响应到内部事件的折叠，以及延迟加载接入。三份正式实现不互相调用，也不代表三家供应商唯一的协议实现。`anthropicProvider`、`googleProvider` 分别接入对应 lazy adapter；`openaiProvider` 默认接入 `openAIResponsesApi`，不是 029 的 Chat Completions。OpenRouter 则提供按 API 键选择的 Anthropic/Chat 实现表。`createProvider`（models.ts 750–875）明确区分单一 implementation 与按 `model.api` 取值的 map；缺项经 lazyStream 产生 stream error。单一 implementation 不进行该 map 查找。compat 175–230 的内置注册只填缺失项，reset 清表再注册，因此不能由目录名推导每次请求都固定走某一个正式子项。

路径可概括为：目录外 provider/model 选择 → `*.lazy.ts` 动态 import → `stream` 或 `streamSimple` → 适配器请求/历史转换 → SDK 或 fetch → `AssistantMessageEventStream`。认证解析、模型目录、注册覆盖来自目录外；工具实际执行、回合调度、重试后的持久化、用户界面不由这些 API 文件完成。

共享修改需要按调用者核对影响范围：simple-options 在 Simple 路径合并采样、限额、signal、hook 等参数；transform-messages 先处理图片、跨模型签名/ID，再补缺失的工具结果并跳过 error/aborted assistant；Google 的 native parts 转换经 google-shared 调用 transform-messages，不能写成完全独立于公共转换层。Responses 系列还共享 openai-responses-shared；Vertex 共用 Google 层。这里只说明所读依赖关系，不把所有调用者自动纳入正式验收。

### lazy / event contract 的精确含义

三个正式 `*.lazy.ts` 都是很薄的 `lazyApi(() => import(...).then(...))`。每次调用都会调用 load 函数；实际模块缓存由运行时处理，并非 lazy.ts 自建一次性缓存 Promise。`lazyApi` 的 async setup 把导入、实现选择以及实现同步抛错转换到 Promise 拒绝链。`lazyStream` 自身直接调用 setup；对于违反其 Promise 契约而直接同步 throw 的任意 setup，不应承诺这一层能捕获。正式三个 wrapper 的 async setup 不属于该反例。

`forwardStream` 对内部事件逐个 `target.push(event)`，保持对象引用，没有深拷贝、schema 校验、排序或背压。迭代完成后若有 `result()` 则等待，再调用 end。catch 覆盖的还可能是转发迭代/result 的拒绝，不能仅命名为“import 错误”；构造的失败结果是空 assistant、零 usage、stopReason error。若外流已经收到终止事件，EventStream 忽略其后的 push，已解决的最终 Promise 也不会被后来的 end 覆写，所以不能外推所有后置异常都会产生第二个可见 error。

AssistantMessageEventStream 以 done/error 识别完成并取出同一个 message/error 对象，`result()` 返回该 Promise。partial 是可变对象，不是历史快照。外层 lazy 不建立网络取消机制、不取消动态 import，也不规定 reader.cancel。正常预先取消由适配器检查 signal 并输出 aborted；若先发生导入失败则 setup error 的规则不同。`fetchDeferred`/`cancelDeferred` 仅按 capability 配置暴露；三个正式 wrapper 均未启用。不能拿别的 Responses lazy capability 说明这三个协议已支持 deferred。

目录也不是全体都返回这种流：`openrouter-images.lazy.ts` 实现独立 `ProviderImages.generateImages` async 方法。Bedrock lazy 为静态 Bun 构建提供显式 module override 与 .ts/.js import 路径选择，仅作为上下文静态阅读；本轮没有构建验证。Cloudflare binding 检查并绑定 fetch 接收者、透传参数；Cloudflare.ts 是 URL 模板。该事实不取消 Google 对自定义 fetch 的拒绝规则。Copilot headers 按最后消息角色选择 initiator 并检查 user/tool 图片；prompt cache 截断采用 Unicode code point 的 Array.from，不能写成 64 字节。

### 三个正式子项的整合含义

| 子项 | 源码身份 / 当前证据状态 | 目录层需要保持的差异 |
| --- | --- | --- |
| 016 Anthropic Messages | 48616 字节，`8e105cc5b2dd304547ed613b70de4740c4db93cee830dc452c2de945052b12b7`；118 个主审 artifacts；3.1.21 accepted rebind | native blocks/thinking signatures/tool_use，按 SDK 原生事件折叠；源侧 block/tool end 不等于整个响应通过目标严格终止校验；源 refusal 为 error，当前 Rust 可生成带 refusal 的 Completion。 |
| 017 Google Generative AI | 16027 字节，`1395c3f2a90bf701b8b5fc1b6f7de8d323193ed339e07a60630900099a543757`；11 个主审 artifacts；3.1.21 accepted | google-shared 转 native parts，thought signature 与工具身份需要协议规则；source cache/input 口径与目标聚合口径不同；主审纠正必须覆盖旧候选的过宽措辞。 |
| 029 OpenAI Chat Completions | 62416 字节，`5874bf0b121db117bd4eba8fffa750252f5f7617f38306c6eb1f746575896319`；候选 manifest `70c63be72d4fd8c2d69b03d679bf28a5ad8a49c52be26651ca6cb029218a7322`；捕获时无 master receipt | 多 provider 兼容分支、reasoning replay 与 tool argument 增量修复；Chat 不是 Responses；完整文件阅读和离线审计不自动等于主审验收。 |

017 master-review-3.1.21 已完整阅读，明确采用其三项限定。第一，公共 transform 的 tool ID normalize callback 只有 provider/API/model 三元组不一致才执行；同模型不因为 ID 看起来“不规范”就经过该 callback。Google 自己同 provider/model 的 base64/signature 层规则需另外区分。第二，simple-options 当 contextWindow > 0 时返回 `Math.min(maxTokens, Math.max(1, available))`，下限 1 只保护 available，显式 maxTokens 为 0 或负值仍可能保留；contextWindow <= 0 的分支才对传入 maxTokens 用 Math.max(1,...)。不能写成输出一律至少 1。第三，一些负向测试仅断言 is_err，名称不能单独证明错误来自被命名的条件；saved-model resume fixture 同时可能有不完整 metadata。不得把历史通过当成隔离了所有失败原因。

同一主审还明确模型成本分层选择最高的严格超过阈值层；Google source input 排除 cached token，而 Rust input 聚合包含缓存，不能直接逐字段比较收费或总数。Anthropic source 输入与 cache read/write、目标汇总同样需要说明口径，Chat 也有 reasoning/output/cache 明细；统一事件类型不意味着三个 usage 对象能无条件同义替换。Provider/model/API 身份和签名必须跨历史保留；修复 JSON/补合成 tool result 是源侧宽容转换，不是严格 type/schema 验证，更不等于允许直接执行 partial tool。

016 主审保留了同模型 bad:id 初始预期错误及更正：同模型保留原值，跨模型才归一化为 bad_id 并同步结果引用。还保留主审准备阶段第三方 JSON 文件尾换行断言失败、随后改用 JSON 解析校验的过程。其历史主审确有 23 场景/24 HTTP source 执行，但那是旧执行，不是本轮。source Anthropic 的 onPayload 后强制 stream=true 与 Google/Chat 路径不可混写；thinkingBudgetTokens 的 `|| 1024` 会覆盖 0；reader finally releaseLock 不等于 cancel；hook 等待无独立有界保证。这些已归入正式子项说明，本轮没有因此新增产品修复。

### 当前 Zenpi 映射及旧目标漂移

映射使用 `src/providers/anthropic.rs`、`src/providers/google.rs`、`src/backend/chat_stream.rs`，后端入口仍在 backend.rs，协议/能力分发在 registry.rs。transport 负责受限读取/取消与传输，core/headless 的回合、工具执行与持久化所有权属于另外层次。本目录报告不把 UI、持久化或运行预算算成 API adapter 自身已证明的能力。

旧 055 的 10 个 Pi 快照全部字节相同。旧 5 个目标快照中 backend.rs、registry.rs 相同；Anthropic、Google、Chat 三个文件已有漂移，完整旧副本和新副本及哈希对照均保留在 `old-current-drift.json`。因此旧 17/17/8 target log 不能直接宣称覆盖当前目标。

本轮重新读当前 Anthropic request_body 243–330：附件总量、类型、structured-output 不支持项以及历史工具结果顺序在请求前校验。finish/read_response 664–790 则先要求 message_stop、检查 blocks/stop_reason/capabilities，再形成 Completion；空拒绝归一化后同时保持可见文本与 replay block；usage 使用 checked_add；发 ToolCallDone 前已完成上述终止校验。sink wrapper 每次发送前检查取消，包括终止 batch 中途取消。这与源侧逐 block 发 toolcall_end 的时间点不同。该片段阅读不是整个 Anthropic 887 行全读的声明，其完整行为依照已接受016报告及当前身份复核。

Google 当前文件与017已读快照、Chat 和公共 backend/registry/transport/core/headless 与029已读快照逐字复用核对。已有 metadata/native history 被保存的当前事实，覆盖旧报告中可能把历史能力简化为纯文本的说法。目标的严格 terminal gating、schema/size/capability 检查及取消所有权不能由源宽容 parser 的成功样例推出。当前 Google/Chat 定义数 19/22 与旧执行 17/8 是不同时间层；本轮没有运行这些测试，不相加成新通过数。

### 历史 055 实验可以证明什么

完整旧 probe.ts 已逐句重读。它启动一个本机 Bun HTTP fixture server，用实际三种 lazy factory/SDK，对每个 API 做 success、missing-auth、preaborted 三种场景，共 9 场景、3 HTTP。它检查 streamSimple 调用不同步 throw，返回 async iterable 且有 result，终止 done/error 数为 1，最终 result 与终止事件对象严格同一引用；success 检查 hello/stop、请求路径和认证；缺认证与预取消没有 HTTP。三种 success 的日志记录 start/text_start/text_delta/text_end/done，但 probe 并未逐条严格断言所有中间事件序列。它只显式断言没有 cancelDeferred；未显式断言 fetchDeferred，后者缺失来自 wrapper 代码阅读。

lazy-native 原始 receipt 是 2026-09-10T23:25:58Z 附近、exit 0、约 0.578 秒，日志 1577 字节 / SHA `4582475b24653c4df9a61a0689d3f9e90cb45e6f8827727c37305e9acd6f1907`。SDK 身份来自原依赖锁：Anthropic 0.124.0、Google 2.21.0、OpenAI 6.40.0、partial-json 0.1.7。代码的 15 秒 watchdog 是该 probe 的保险，不证明生产流超时。没有覆盖 load rejection、动态 import 期间取消、首事件后取消、并发消费者、延迟终端、背压或任意 inner.result 拒绝。原始 Google fixture 与目标更严格的 metadata 契约不同，不能当作同一输入的源目标完全等价实验。

旧 leaf 016 的 23/24、017 的 17/20、029 的 33/37 分别保留原日志及失败版本；旧 target 17/17/8 和后来的叶子测试数字都不和这 9/3 相加。旧 evidence-verified exit 0 仅是当时结构核验。旧 G-STAGE exit 1、结构部分 ok、缺少 ZS1-055.master.json 的错误仍原样保存；它既不是本轮源行为失败，也不能删去或改成成功。新 verifier 仅用 Python 标准库读文件、JSON/tar 和内存 patch 重建，不执行这些旧脚本或 SDK，也不启动 HTTP、Rust、Bun 或产品。

### 新发现与交付边界

本轮没有确认新的产品运行缺陷。新补强的是目录声明的证据精度：OpenAI 默认协议与029的区别、图像接口例外、lazy 异常/引用/取消边界、Google共享转换、017主审三项限定，以及旧目标三文件漂移。上述均应由主审逐项核阅，而不是自动以离线哈希检查取代语义判断。

历史 C 包 198 个实体文件、65 个 manifest payload、candidate.patch（554702 字节）和 runner 全量只读复制；新 verifier 在内存中验证每个旧 patch 正向/反向重建与总 patch 拼接，保留空 base 和历史失败。016/017 receipt artifact 链、017归档内manifest及完整阅读区间、029完整候选原包、32个真实直接项清单、新阅读切片、当前目标与最终权威观察均随包携带。原始 B/C 共6080文件的保护清单用于本机只读检查；外机 portable 模式不要求这些绝对路径存在。

离线结果证明文件身份、引用闭合和限定的历史结构事实，不授予055或029 accepted 状态。最终核验输出、冻结 manifest 与原始文件保护结果另交主审；本轮 SDK/产品/构建/预算执行数为0。只提供这一目录候选报告，不写主线目标文件，不更新权限、claims、BentoBox、旧ready或全局checker。


### 真实直接项逐文件清单

当前目录有32个直接文件、0个直接子目录，与旧055清单的名称/类型/字节/哈希逐项一致。以下29个非正式文件均为context-only，不因阅读或位于该目录就获独立验收；完整SHA及行数在direct-inventory.json。

| 直接项 | 字节 | 职责与阅读范围 |
| --- | ---: | --- |
| `anthropic-messages.lazy.ts` | 199 | 相应API延迟导入；新读全文 |
| `anthropic-messages.ts` | 48616 | 正式016；Messages请求与事件折叠；复用主审完整阅读 |
| `azure-openai-responses.lazy.ts` | 206 | 相应API延迟导入；新读全文 |
| `azure-openai-responses.ts` | 11337 | Azure Responses；新读1–60行边界 |
| `bedrock-converse-stream.lazy.ts` | 1148 | 相应API延迟导入及Bedrock构建override；新读全文 |
| `bedrock-converse-stream.ts` | 48022 | AWS Converse；新读1–60行边界 |
| `cloudflare-ai-binding.ts` | 4604 | Workers binding fetch适配；新读全文 |
| `cloudflare.ts` | 842 | Gateway URL模板；新读全文 |
| `constrained-sampling.ts` | 9295 | 受限采样归一化；复用029全文阅读 |
| `github-copilot-headers.ts` | 1184 | Copilot initiator/图片头；新读全文 |
| `google-generative-ai.lazy.ts` | 202 | 相应API延迟导入；新读全文 |
| `google-generative-ai.ts` | 16027 | 正式017；Gemini协议；复用本任务完整阅读及主审 |
| `google-shared.ts` | 15695 | Google native parts/history；复用017全文阅读 |
| `google-vertex.lazy.ts` | 189 | 相应API延迟导入；新读全文 |
| `google-vertex.ts` | 18373 | Vertex SDK接入；新读1–60行边界 |
| `lazy.ts` | 3203 | 加载与事件转发；新读全文 |
| `mistral-conversations.lazy.ts` | 205 | 相应API延迟导入；新读全文 |
| `mistral-conversations.ts` | 30329 | Mistral协议；新读1–60行边界 |
| `openai-codex-responses.lazy.ts` | 206 | 相应API延迟导入；新读全文 |
| `openai-codex-responses.ts` | 54470 | Codex Responses接入；新读1–60行边界 |
| `openai-completions.lazy.ts` | 199 | 相应API延迟导入；新读全文 |
| `openai-completions.ts` | 62416 | 正式029；Chat兼容协议；复用本任务完整候选阅读 |
| `openai-prompt-cache.ts` | 358 | cache key截断；新读全文 |
| `openai-responses-shared.ts` | 29490 | Responses共享转换/折叠；新读1–60行边界 |
| `openai-responses.lazy.ts` | 195 | 相应API延迟导入；新读全文 |
| `openai-responses.ts` | 13908 | Responses SDK接入；新读1–60行边界 |
| `openrouter-images.lazy.ts` | 315 | 图像生成延迟导入，独立ProviderImages；新读全文 |
| `openrouter-images.ts` | 6132 | 图像生成；新读1–60行边界 |
| `pi-messages.lazy.ts` | 185 | 相应API延迟导入；新读全文 |
| `pi-messages.ts` | 13451 | gateway消息协议；新读1–60行边界 |
| `simple-options.ts` | 3295 | Simple基础选项/思考预算；新读全文 |
| `transform-messages.ts` | 7121 | 历史/图片/签名/工具转换；新读全文 |


最终捕获补记：055/016/017/029的相关blueprint行及三个索引的相关行均未变化；029仍无master receipt。blueprint与active_requirement整文件哈希变化来自本范围外的进度，四个权威键保持一致；原始与最终全文均保留，未修改全局检查器。初次新离线核验58项通过；其后扩展的范围、patch及冻结检查以最终输出为准，不能把58当作最终全部数量。


# ZS1-055 independent controller directory acceptance — 3.1.21

The scope is packages/ai/src/api with formal children016 Anthropic,017 Google and029 Chat Completions. All three now have independently accepted current3.1.21 receipts. The candidate correctly records029 as pending at its earlier capture; that immutable historical statement is superseded for current dependency status by the newly archived029 master receipt, not silently rewritten. The controller checked each child receipt and its artifact chain separately. This directory acceptance does not accept059 or the other29 physical context files.

The controller read the complete27218-byte directory candidate, including its complete8634-byte historical prefix, every one of the32 direct inventory rows, all eleven lazy wrappers, lazy.ts, Cloudflare helpers, Copilot headers and prompt-cache helper. It read the four provider factories, compat175–230, models750–875, the nine explicitly bounded nonformal adapter1–60 excerpts, and current Anthropic243–330/664–790. The complete historical54-line Bun probe and actual lazy-native command/log and failed old G-STAGE log were read. The worker's41-row ledger is preserved as its own reading record, not relabeled as41 new controller reads.

Complete source016 understanding is reused through its unchanged accepted source identity and prior independent review;017/029 full readings and corresponding Google/Chat target readings are the controller's already completed individual reviews. simple-options, transform-messages, constrained-sampling, event-stream and google-shared are exact prior-read source contexts. The controller independently checked these bytes against its original017/029 packages. The captured016/017 receipts remain byte-identical to current authority. The current029 accepted report retains the exact copied candidate prefix and adds controller qualifications. No formal file is newly accepted through this parent.

The integration distinction matters: the default openaiProvider factory selects Responses, while OpenRouter selects an implementation map keyed by model.api. The three formal adapters do not call each other. createProvider selects a single implementation by its stream function or dispatches through a map; missing map entries become lazy stream errors. compat registration preserves an existing override; reset explicitly clears the registry first. These are separate selection routes, not proof that every request named OpenAI uses Chat. Auth, model registration, cost catalogs and session/tool execution remain outside this directory.

lazyApi wraps load in async setup, so import and implementation synchronous failures become rejections. lazyStream calls a supplied setup directly, therefore an arbitrary setup that throws before returning a Promise is a separate uncaught boundary. Forwarding preserves references and order and does not validate, copy or provide backpressure. It awaits an optional inner result after iteration. A forwarded terminal resolves the outer result; later pushes are ignored. Cancellation does not cancel import. The three formal factories expose neither deferred operation. Image generation has an independent async interface; Bedrock has a module override and runtime suffix rewriting, which were read but not built here. Cloudflare binding forwards using a bound fetch receiver; the helper comments about upstream service authentication are not independently verified live-service behavior in this review. Prompt-cache truncation counts code points, not UTF-8 bytes or grapheme clusters.

Cross-adapter semantics remain intentionally explicit. Source block-end events can precede overall errors and cannot authorize tool execution. Host protocol folds validate final blocks, identities, arguments, capabilities and termination before ready events. Per-event cancellation stops subsequent terminal events but cannot retract earlier publication. Shared transformMessages normalizes cross-model identities and synthesizes missing tool results only for request preparation; it cannot create durable tool-execution facts. Google also uses that shared conversion through google-shared. Usage/cache/cost counters have different source and target meanings. The017 available-space floor correction and029 primitive-JSON/callback/finish/compatibility qualifications remain in force at the directory boundary.

The historical directory probe has nine scenarios and three loopback HTTP requests through actual lazy factories/adapters/SDKs. It asserts synchronous return, one terminal, result-object identity, success output/path/auth and missing-auth/pre-abort no-HTTP behavior. It records intermediate event names without asserting the entire sequence. It explicitly checks cancelDeferred only; absent fetchDeferred is established by wrapper reading. It does not test import rejection, midstream cancellation, concurrent consumers or recovery. Its15-second watchdog is fixture protection, not a production deadline. Historical leaf23/17/33 scenario counts and target17/17/8 passes remain distinct original runs. The missing055-master-receipt failure and original016 failed expectation are preserved unchanged.

The controller fully read the new standard-library verifier and ran a fresh copied verifier exactly once, from a temporary directory with logs outside the immutable copy:61 integrity checks passed, exit0. No historical scripts, SDK, Cargo, product, PTY or budgets were executed. Before publication the controller separately verifies the live32-entry physical set, every direct identity, all47 captured input identities and current child receipts. Only headless521c-to-bf252 differs among inputs; its complete already reviewed input-shutdown delta is preserved from029. The relevant2610–2688 excerpt remains exact. This is current CLI composition context, not a new runtime result. Old protocol-file drifts remain preserved in the worker archive.

Acceptance is only the independent055 directory integration understanding after016/017/029 completion. It does not assert complete provider parity or fix the still-failing117 cold-start gate. The worker candidate remains byte-identical as the installed report prefix. Rollback removes only055 report/receipts/status and refreshes authority projections; it leaves child reports, product code, historical evidence and BentoBox intact.
