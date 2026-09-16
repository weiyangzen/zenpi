# ZS1-015 / SRC-0440 — packages/ai/src/types.ts

本报告是 authority 3.1.19 下的单文件源码审查候选，未作主控验收，未覆盖目录验收。唯一 Pi 源对象为 `/Users/wangweiyang/GitHub/pi-mono/packages/ai/src/types.ts`：37,411 bytes，881 lines，SHA-256 `f2edab10f093a15ba2cdeaaf4fa1634d76ee943e6f62b2f44bdb2d14321834e0`，与当前主库 active_requirement.json 的 ZS1-015 冻结项一致。需求 digest 为 `3ce9613262de0a8499837571137782becc01daa7b56923a53b11f7f04058f60d`。

本次重新完整读取 1–220、221–440、441–660、661–881 行。`Docs/quality/stage1/ZS1-015/worker-types319/input-manifest.json` 给出连续、无重叠的半开字节范围及逐段 hash；四份 read 文本拼接等于完整源快照。`export-inventory.json` 列出全部 72 个具名 type/interface 声明的起始行；另有第15行 type-only re-export。旧本地报告保存在 prior-local-report.md，仅作历史保全；其中旧行号、旧目标实现状态、其他文件和运行时测试声明不充当本次证据。

## 完整文件合同分区

| 源行 | 全量分区及其边界 |
|---|---|
| 1–16 | TelemetryContext、10个 API options、diagnostics、event-stream 的 type-only import，以及 event-stream 的 type-only re-export。此文件引用它们的类型，未在本次扩展读取或执行这些 Pi 文件。 |
| 17–80 | 10个 KnownApi 与可扩展 Api；独立 ImagesApi；KnownProvider、ProviderId 和图片 provider 身份。Api 的 `string & {}` 保留自定义字面量通道，ProviderId 接受任意 string；字符串可表示不等于实现存在。 |
| 82–122 | ToolChoice 仅 auto/none；ThinkingLevel 为 minimal/low/medium/high/xhigh/max；ModelThinkingLevel 另含 off。ThinkingLevelMap 为部分映射，值可 string/null；ThinkingBudgets 只有 minimal/low/medium/high。ChatTemplateKwargValue 接受标量/null 或 thinking.enabled/effort/budget 变量及 omitWhenOff。另定义三种 reasoning token budget 字段、CacheRetention、四种 Transport、env/header/fetch/session affinity 和 HTTP response 形状。 |
| 124–177 | 通用 ProviderRequestOptions：signal、telemetryContext、apiKey、fetch、env、onPayload/onResponse、headers、timeoutMs、maxRetries、maxRetryDelayMs。这里只声明输入和注释合同，没有 HTTP、取消、重试或参数校验实现。 |
| 179–236 | StreamOptions 增加采样、token、transport/cache/session、websocketConnectTimeoutMs、metadata；ProviderStreamOptions 开放额外字段。DeferredFetchOptions.wait 默认语义为一次状态检查；DeferredCancelOptions 是通用 options 的别名，注释明确 best-effort。 |
| 243–303 | ApiOptionsMap 对已知 API 指向具体 options 类型；ApiStreamOptions 条件类型对自定义 API 回退通用形状。ProviderStreams 定义 stream、streamSimple 及可选 fetchDeferred/cancelDeferred；ProviderImages 定义 Promise 图片生成，ImagesOptions 共享认证/回调合同。 |
| 305–343 | 开放 ProviderImagesOptions；AnthropicAllowedFallbackModel 的 provider/model/cost；SimpleStreamOptions 的 toolChoice、reasoning、deferred 窗口和 thinkingBudgets；StreamFunction 与 ImagesFunction 泛型签名。同步 missing-auth 与流内失败合同见下节。 |
| 345–381 | TextSignatureV1、TextContent、ThinkingContent、ImageContent、ToolCall。签名是回放资料，redacted thinking 可保留不透明密文；图片为 base64 + MIME；工具参数是 Record<string, any>，还有 thoughtSignature、namespace。类型本身不校验签名、base64、JSON schema 或工具执行权限。 |
| 383–420 | Usage、7种 StopReason、递归 JsonValue、DeferredHandle。usage 中 reasoning 是 output 子集，cacheWrite1h 是 cacheWrite 子集；deferred 保存 provider/model/api/id、过期/轮询提示及可选 JSON 重建数据。 |
| 422–490 | UserMessage、AssistantMessage、ToolResultMessage、Message 联合；独立 ImagesContext/AssistantImages。assistant 同时保存请求 model 和可选 responseModel/responseId、native effort、diagnostics、usage、stopReason、deferred、error/rawStopReason/endTurn；endTurn 注释明确当前不控制 agent 流程。tool usage 不计入主 LLM context accounting；addedToolNames 表示新工具加载点。 |
| 492–528 | type-only TSchema import；GrammarFormat/GrammarVariants；ConstrainedSamplingConfig 的 json_schema strict prefer/require 或 grammar variants；Tool 泛型 schema 及 constrainedSampling=false；Context 的 systemPrompt/messages/tools。并非运行时 validator。 |
| 530–562 | AssistantMessageEvent 的注释状态机与12个事件分支：start，text/thinking/toolcall 各 start/delta/end，done，error。共享 partial、开始顺序、终止分支及 redaction 例外见下节。 |
| 564–643 | OpenAICompletionsCompat：字段/角色/usage/finishReason、token field、工具历史格式、11种 thinkingFormat、chat template args/kwargs、两类 routing、zai tool stream、thinking budget 字段与旧别名、grammar/strict、cache、session affinity、deferred tools、long retention、vllmPriority。URL 推断和格式转换都发生在别处；这里没有 adapter 逻辑。 |
| 645–665 | OpenAIResponsesCompat：developer、session affinity、long retention、strict/grammar、additional tools/tool search、explicit prompt cache、max output 支持开关。注释内各默认值不能被理解成该 interface 自动填值。 |
| 667–737 | AnthropicMessagesCompat：eager tool streaming 与旧 beta header、cache/session/tool-cache、temperature、adaptive thinking、empty signature、strict tools、mid-conversation effort、允许的 fallback model/cost、tool references；BedrockCompat 的 strict。无 fallback 数组或为空时需省略字段，是声明的适配器义务。 |
| 739–825 | OpenRouterRouting 全部字段：fallback、参数支持、data collection/ZDR/distillation、order/only/ignore、quantizations、sort、max_price、吞吐/延迟标量或分位阈值；VercelGatewayRouting 的 only/order。外链是源码注释，未联网核实当前服务行为。 |
| 827–881 | ModelCostRates、ModelCostTier、ModelCost；Model 的身份、baseUrl、reasoning/map、input、cost、contextWindow/maxTokens、sampling/header 与条件 compat；ImagesModel 通过 Omit 移除 api/provider/reasoning/contextWindow/maxTokens/compat，再给独立身份和 output。它仍继承未被 Omit 的 thinkingLevelMap、cost、headers、samplingParams 等，不能泛称删除所有 reasoning 相关字段。 |

## 事件、错误、取消与重试

成功流先 start，再 partial 更新，最后 done。请求设置在生成开始前失败可以直接 error；已经 start 的失败也以 error 结束。updates/done 不得早于 start。done.reason 仅 stop/length/toolUse/deferred；error.reason 仅 aborted/error；pending 不能作为 terminal reason。每个 event 的 message/partial 自身 stopReason 未被联合类型进一步约束，因此仅靠类型不能证明 event.reason 与 message.stopReason 一致，也不能证明单 terminal、顺序正确或 EOF 完整。

partial 是共享 live response-so-far 引用，不是事件时刻快照。普通 text/thinking block 在 start 时为空，经 delta 生长，以 end 为权威内容；redacted thinking 可以 start 就完整且没有 delta。toolcall_start 参数形态由 provider 决定，后续 delta 是 JSON 更新；消费者应等待 toolcall_end 的最终 ToolCall，不能把中间参数当成可执行调用。这些都是源码注释规定的协议，不是本文件执行的状态机。

StreamFunction 注释允许直接 streamSimple 在认证缺失时同步 throw；一旦返回 stream，请求/model/runtime failure 应在流中表达。错误终止要求 assistant 的 stopReason 为 error/aborted 并提供 errorMessage，不过 interface 将 errorMessage 声明为可选，没有类型级强制。必须把“声明义务”和“实际执行器验证”分开。

取消入口是可选 AbortSignal。DeferredCancelOptions 与可选 cancelDeferred 的 best-effort 语义不等价于保证远端任务停止，也没有声明取消延迟、清理、幂等或恢复重放算法。signal 缺席、错误类型、回调 throw、结束后取消等时序都不能由这个文件推出。

重试有三个独立维度：timeoutMs 是支持它的 provider/SDK 的 HTTP timeout；maxRetries 是支持 client retry 的 provider/SDK 最大重试次数；maxRetryDelayMs 对服务器要求的等待设上限，注释默认60000ms，0关闭该上限，超过上限应立即失败并在错误中包含所要求的延迟以便上层透明处理。注释举例 OpenAI/Anthropic SDK 默认10分钟和2次，不是本文件落实的全局默认。文件没有定义可重试状态码、退避公式、累计期限、幂等键或已输出后的重试策略。

## 请求覆盖规则、usage 和模型能力

通用 env 覆盖 process.env。headers 中调用者值覆盖 provider 默认，null 抑制默认字段；Bedrock 例外注释说保留头 x-amz-*/authorization/host 会被忽略以维护签名。onPayload 可同步/异步返回替代 payload，undefined 保持原值；onResponse 可异步，stream 专门承诺在消费 body 之前调用。fetch 默认 globalThis.fetch，不能注入的 adapter 可以拒绝，而且不影响 WebSocket。websocketConnectTimeoutMs 仅涵盖连接握手，之后流空闲受 timeoutMs 管理。

samplingParams 按 key 覆盖 Model.samplingParams，并在已命名请求字段后合并，因此可覆盖命名字段；注释限定 OpenAI-compatible completions/responses/Azure，其他 API 忽略。transport/cache/session/metadata 也是能力相关选项，不能从公共字段存在推断每个 provider 都支持。

Usage 有 input/output/cacheRead/cacheWrite/totalTokens，以及可选 cacheWrite1h、reasoning；reasoning 可能为0，undefined 表示未报告，不能当成额外 output。cost 有 input/output/cacheRead/cacheWrite/total，没有在此文件执行加总或校验非负/有限数，也未规定 totalTokens 必须等于哪个字段公式。ToolResultMessage.usage 明确不属于主 LLM 上下文计费。ModelCostRates 的单位为美元/百万 token；tiers 采用总输入严格超过 inputTokensAbove 的门槛，最高匹配 tier 适用于整次请求，不能推成分段累进价格。源类型没有价格来源/版本字段。

Model API 泛型限制 compat：completions→OpenAICompletionsCompat，Responses/Azure/Codex→OpenAIResponsesCompat，Anthropic→AnthropicMessagesCompat，Bedrock→BedrockCompat，其余为 never。thinkingLevelMap 缺 key 用 provider 默认，null 表示不支持，不能把两者合并。input 仅 text/image，不含通用文件；Model 没有一般 tools/streaming/structured-output 布尔字段。基于 wire 或名字猜模型能力不由本文件授权。类型中的 number 也没有自行验证 contextWindow、maxTokens 或价格范围。

## 当前 Zenpi 对应路径与明确差异

目标引用是主库只读片段快照，不构成这些目标文件的完整验收。`target-references.json` 为每个引用给出文件 hash、行号、字节范围及片段 hash；行号属于该次捕获，不套用旧报告。主库观察 HEAD 为 `6f252a20c628e9b1ede14e2887acc04657c71d7c`，包含用户工作树修改；本报告没有合并或提交。

| Pi 合同 | 当前 Zenpi 定位 | 源码可确认的映射与不足 |
|---|---|---|
| Model / G05 / reasoning / context | `src/providers/registry.rs:18` ReasoningLevel，`:60` ModelDescriptor，`:71` effective_capabilities，`:84` validate_reasoning，`:102` context_budget；`src/backend.rs:124` ProviderCapabilities；`src/config.rs:1274` ModelCatalogEntry | 已有 exact provider/id、模型×wire能力交集、reasoning成员校验、上下文/输出上限、字段来源及可选价格；不是旧报告所述“仅 wire 能力”。目标 none/ultra 与源 off/max 词表不同，未映射 levelMap 的 provider-specific string/null，也没有完整源 compat 族。这里只确认所读方法，不宣称目录全部行为。 |
| Context / StreamOptions | `src/backend.rs:169` CompletionRequest 及 builders | turn/turns/model/tools/instructions/metadata/attachments/response_format/max_output_tokens 与部分源输入对应；没有同构的通用 AbortSignal、fetch/env/headers/onPayload/transport/sampling/cache/deferred options bundle。是否在具体配置或 adapter 有局部机制，需要单独确认。 |
| Event stream | `src/backend.rs:229` ProviderEvent，`:301` Completion，`:389` Backend | 使用同步 complete_with_control + cancel predicate + sink(Result)，有 response-created/text/reasoning/tool/usage/warning/completed/failed；不携带 source 的共享 partial/contentIndex 或 thinking end 的完整同构状态。默认 trait complete_with_control 只在 complete 前后检查取消，不能凭 trait 声明保证任意实现的中途取消。 |
| Usage / pricing | `src/backend.rs:292` Usage；`src/providers/registry.rs:42` ModelPrice | 公共 Usage 仅 input/output/total 三个 u64；没有 cacheRead/cacheWrite/cacheWrite1h/reasoning/cost 明确字段。ModelPrice 采用整数单位、来源版本、可选 cached input 价格，未涵盖源 cache-write/tier 成本结构。不要将缺失字段默认为零或假称 usage 全等。 |
| User/assistant/tool messages | `src/core.rs:133` TurnRole，`:143` Turn；`src/tools.rs:540` ToolCall，`:652` ToolResult | Turn 是角色、字符串内容、时间与通用 metadata，不是源内容块联合；ToolCall 有 id/name/JSON arguments，通用结构没有 thoughtSignature/namespace；ToolResult 是 Success/Error 的 JSON output/失败。通用 metadata 可承载额外资料，但类型存在不能证明每个 opaque signature 的保留/回放实现。 |
| Tool/schema | `src/tools.rs:505` ToolDefinition 与 validate | JSON-Schema形状、side_effect、名称/描述/容量/object校验，工具执行器仍是权威validator；未提供源 grammar variants、strict prefer/require、addedToolNames 的完整公共同构字段。 |
| Cancellation / retry | `src/backend.rs:324` BackendError，`:354` is_retryable，`:1587` 具体 complete_with_control | Cancelled/Steered 不重试；Transport 与408/409/425/429/5xx可重试。所读实现只在未发 event、次数未耗尽且非 semantic_compaction 时重试，warning + capped exponent、Retry-After取较大值、等待期间10ms轮询取消，另有circuit。它和源 maxRetryDelayMs 的字段合同不是逐项相同；本次不执行网络或取消验证。 |
| Deferred / image generation / routing | 对照所读 CompletionRequest、Completion、ProviderEvent 和通用 Tool/Turn 结构 | 未见 DeferredHandle/cancelDeferred/fetchDeferred、ProviderImages、OpenRouter/Vercel routing 的同构公共合同；这只描述已核对的公共类型，不据此宣称整个仓库完全没有相关功能。 |

## 验证、适用范围与移交

本次工作仅为单文件逐行审查、hash/连续字节覆盖、72个声明 inventory 和目标片段映射。没有安装依赖，也未执行 TypeScript 完整类型检查、源消费者测试、网络服务或深度搜索探针。现有本地常用位置没有 TypeScript 编译器；完整 typecheck 会解析其他 API 模块，不是完成此单文件审查的必要条件。verification 只能验证引用/字节结构，语义需要主控独立复核。

G-FILE 的源身份、唯一报告、完整范围和artifact hash已用本地主控校验函数核对通过。G-STAGE --item ZS1-015 需要主控 receipt；本工作不会伪造 master receipt、修改主库 selector/checklist，或声称该项已验收。本次实际 G-STAGE --item ZS1-015 返回 exit 1：structural.ok=true，缺少 ZS1-015.master.json；失败日志完整保留在 gstage-main.log，SHA-256 e6d52a5677eb9504629ff83d30bc3c8c7a29c6a256e3fb826b562524263d6ca7。

待主控处理的是本报告是否完整、差异划分是否正确，以及后续实现项如何承接 usage/compat/deferred 等缺口；不在本文件审查中修改产品源码或扩大目录状态。回滚只撤回本报告候选及本次 worker-types319 证据；旧本地报告、主库和所有已冻结包保持保全。中止的深度搜索候选仅在 depth-work-locations.json 中列位置和未完成状态，不继续运行。
