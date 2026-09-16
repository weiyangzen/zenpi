# ZS1-029 · openai-completions.ts 完整单文件独立复核 · 3.1.21

候选状态：等待主控独立语义复核。唯一范围 SRC-0306 `packages/ai/src/api/openai-completions.ts`，62416 字节、1717 行，SHA-256 `5874bf0b121db117bd4eba8fffa750252f5f7617f38306c6eb1f746575896319`。仅交付本文件完整理解及当前 Zenpi 必要接线映射；不接受 017、055、108 或任何兄弟、父目录。

权威版本 3.1.21；requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`；baseline `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`；run `zenpi-stage1-20260911`。冻结 source_manifest、file_learn_index 与源实测身份一致，本项依赖 ZS1-001，唯一报告路径是 `Docs/learn/stage1_pi_mono/files/packages/ai/src/api/openai-completions.ts_learn.md`。

## 阅读与复用来源

本轮连续新读 1–230、231–470、471–740、741–990、991–1240、1241–1480、1481–1717 行，对应 [0,7665)、[7665,15795)、[15795,25985)、[25985,36018)、[36018,44240)、[44240,52622)、[52622,62416)。`reading-ledger.json` 记录原文字节、逐段哈希；覆盖完整文件且无重叠或空缺。下面语义说明建立在新完整阅读上，不以函数目录或旧报告代替阅读。

先只读查找 ff51、38de 的 .ops 顶层；ff51 有 source029-behavior-ready、source029-reuse320-ready、source029-current-map320-ready 三个冻结包，38de 无029命名候选。三包全部物理文件原样收在 `historical/`，分别39、192、277个，旧manifest分别列12、63、268个payload。原manifest SHA依次为 `56e2e684a31171db291b193d330e394da6a5351316a5391fdac34bc8d775a713`、`5e5b1205149292a7b72962df100502069ae8fd3a816c041c28523d292ccd7b9c`、`d9b931a15b0ff7d54520a5d7b47cf66fc9bad0578665e0b0f11ae7f12cc69e37`。`historical-inventory.json` 逐物理文件绑定，旧命令/runner/安装/测试/失败输出均仅作数据，不执行或改写。

三个旧报告10771、25992、43717字节的哈希依次为 `ebdf9a3c3a3233f13aab7335f94145c419109cc2cf2532f8722b9ae7b4488d53`、`aaf79c93fafc908ca776c4fdc249e45eea1c170f2874ef22612f8f48585ff2c7`、`0eb71b3f62af25d7c8f8fbccdeb6e75634ae68808ca05cb06b8585682c3eaaa0`，保留精确前缀链。本轮读完最新报告180行。旧reuse320阅读区间 [0,9725)、[9725,19881)、[19881,30064)、[30064,41177)、[41177,51523)、[51523,62416)；current-map320区间 [0,9238)、[9238,19173)、[19173,29404)、[29404,40553)、[40553,51274)、[51274,62416)，其各段hash与原文仍由原input-manifest绑定，离线核对后复用，不算新阅读。

必要上下文本轮新全读 transform-messages.ts、provider-retry.ts、json-parse.ts、constrained-sampling.ts 和目标 chat_stream.rs；其他目标按 `context-reading-ledger.json` 的片段新读。simple-options/event-stream及部分transport/auth/headless上下文复用同任务前项已读的精确字节，单列 `reused-context-inventory.json`，不是新阅读或接受017。完整复制目标文件仅用于身份绑定，不表示完整审查所有目标内容。

## 1–310：契约、鉴权、历史与元数据

imports 把SDK请求、成本、思考级别、流容器、JSON修复、header/env/retry/unicode、schema/grammar、Copilot、cache key与消息转换分给依赖。本文件导出 raw stream、streamSimple、convertMessages 和 options；它不持久化session、不执行工具、不认证服务端签名。

hasHeader 按大小写不敏感名称找非null且trim后非空值。getClientApiKey 优先显式key，否则 **只看 options.headers** 中 authorization/cf-aig-authorization，返回占位unused；缺失则throw。model.headers 后续虽会合入SDK，不能单凭它通过前检。hasToolHistory 检测任一toolResult或assistant toolCall；deferred工具从历史addedToolNames收集Set，按context.tools名称查找，忽略找不到的名字。四类content guard只按type判别。

reasoning detail运行时校验接受非数组对象，公共id可缺/null/string、format可缺/string、index可缺/number；text/summary/encrypted各需对应string，text.signature可缺/null/string。它不是封闭schema，会保留未知字段；index的number判断不是非负整数检查。thinkingSignature JSON需非空数组且每项有效，否则undefined；legacy工具签名额外要求encrypted、非空id/data。相邻同类型text或summary直接拼接，不按不同id/index分组；id/index只补nullish，format/signature补空值；encrypted独立复制。

options扩展原生toolChoice、minimal/low/medium/high/xhigh/max effort、custom budgets。resolved compat明确诸多可选字段。cacheRetention显式值优先，其次scoped env的PI_CACHE_RETENTION=long，默认short；none关闭本层session缓存关联。它只决定请求数据，不保证远端缓存命中。

## 311–724：流状态、工具、终止与错误

raw stream立即返回AssistantMessageEventStream，异步IIFE创建同一个可变assistant output（空content、零usage/cost、pending、时间戳）。依次auth→compat/grammar→cache→client→buildParams→await onPayload；非undefined返回值整体替换params。SDK请求另传signal、timeoutMs、maxRetries:0，经共享retry包住create(...).withResponse()。await onResponse(status,headers)后才发start。因此onPayload/onResponse失败进入外层catch，挂起不会由本文件主动取消；timeoutMs不是包含回调与全重试链的总deadline。

textBlock、thinkingBlock各只有一个；所有内容按首次出现顺序入output.content。contentIndex通过indexOf定位，partial始终指向同一可变output；事件不是不可变快照。tool maps按numeric index优先、truthy id其次查找；缺/未命中则新建，后续补缺id/name/index，新id仍可增加map别名。本文件不检查index非负整数、不拒绝同一index换id/name、不保证最终唯一非空身份。工具类型/SDK声明不能替代运行时校验。

函数arguments逐段拼入partialArgs，调用parseStreamingJson更新arguments；身份片段也发空delta。custom工具使用grammar输入属性，把原始input变成JSON字符串属性的增量，未知名称fallback=input；中途custom可切换并清除旧partialArgs。finishBlock重新解析函数参数，或关闭custom JSON buffer，剥partialArgs/customInput/streamIndex，再发toolcall_end。

逐chunk先略过非对象，保存首个truthy responseId；首个非空且不同请求模型的chunk.model保存为responseModel，之后不核对变化。优先chunk.usage，否则choice.usage；仅choices[0]。truthy finish_reason先映射并标记hasFinishReason，仍处理同chunk delta；后续choice继续处理，可能覆盖raw/stop reason。没有额外choice、身份变化、终止后内容的本文件拒绝规则。

delta按text→首非空 reasoning_content/reasoning/reasoning_text→tools→reasoning_details处理。文本判断非null/undefined且length>0，并非严格string验证；reasoning则要求非空string，避免重复alias累计。thinking块首次alias决定标签，opencode-go的reasoning标签改reasoning_content；后续alias变化不新建块。有效details只进replay元数据，在finalize/catch时序列化到thinkingSignature，不直接发可见detail文本；无效项跳过。源未处理delta.refusal为独立拒绝事件，不能由目标能力反推源支持。

正常迭代结束先对所有块发end，再检查signal、stopReason及finish证据。故 **toolcall_end可以先于整流error**；这些是块结束而非工具执行许可。默认缺finish报错；显式supportsFinishReason=false且正常EOF无finish时才按工具存在推断toolUse/stop。stop/end可done，length也可done，function_call/tool_calls变toolUse且不核对实际工具；filter/network/unknown→error。mapStopReason(null)的辅助分支不由truthy调用点建立有效finish。源允许空内容带有效stop结束，不能等同目标EmptyResponse约束。

迭代直接抛错会跳过正常finishBlock循环。catch保留partial内容、已累计reasoning details，剥内部index及scratch，signal.aborted决定aborted否则error；normalize/format错误后，仅在未出现时追加error.metadata.raw，发error后end。不回滚已见delta，不承诺所有错误全面脱敏。源没有统一frame/总响应/tool JSON/队列上限；各流局部状态相互独立，但无本轮背压/并发/资源执行证明。

共享event-stream FIFO无固定容量、push不等待消费者，done/error都resolve最终AssistantMessage，error不reject result Promise。旧probe structuredClone事件后断言，不能用它证明原始partial引用不再变化。

## 726–1060：客户端、请求、思考格式和覆盖顺序

streamSimple同步做auth检查，buildBaseOptions建立上下文约束maxTokens，clampThinkingLevel后将off转undefined，再委托stream并传thinkingBudgets。raw stream不自动获得这层clamp。simple helper按模型context估算留4096安全tokens、可用输出至少1；独立思考budget helper给answer留1024。

createClient headers顺序：User-Agent/model.headers→Copilot动态header→session affinity→options.headers。OpenRouter affinity用x-session-id；其他格式可用session_id、x-client-request-id、x-session-affinity。构造OpenAI(apiKey,baseURL,dangerouslyAllowBrowser:true,fetch,defaultHeaders)。custom fetch允许；默认headers、请求回调和鉴权前检的职责不同。

buildParams先convertMessages，建立model/messages/stream:true。特定baseUrl子串或long兼容条件决定prompt_cache_key，long且支持写24h；默认include_usage，supportsStore时store=false。maxTokens只有truthy才写，选max_tokens或max_completion_tokens，0被略过、负数仍truthy；temperature只检查undefined。Kimi已deferred名称从顶层activeTools剔除；active为空但有工具历史则tools=[]。支持zaiToolStream时tool_stream=true。随后cache markers、toolChoice、vllm priority、thinking、独立budget、routing。samplingParams最后Object.assign可覆盖前面任何同名字段；外层onPayload还可整体替换。早期budget/cap不是最终出站不变式。

| thinkingFormat | 完整启用/关闭语义 |
|---|---|
| zai | reasoning模型写thinking enabled且clear_thinking=false，未请求写disabled；支持effort时映射为string才追加reasoning_effort。 |
| qwen | enable_thinking布尔；启用且支持时映射string effort。 |
| qwen-chat-template | chat_template_kwargs的enable_thinking布尔及preserve_thinking:true。 |
| chat-template | 解析compat.chatTemplateKwargs，非空才设kwargs。 |
| baseten | 解析chatTemplateArgs；supportsReasoningEffort时启用/关闭均可取对应map，string才写effort。 |
| deepseek | 启用thinking.enabled；关闭仅在off映射不是null时disabled；启用且支持时effort取映射??请求值。 |
| openrouter | nested reasoning.effort；关闭off不是null才写映射??none。 |
| ant-ling | 仅reasoning模型、显式effort且映射string时写nested effort；无通用off对象。 |
| together | nested reasoning.enabled布尔；启用且支持追加effort。 |
| string-thinking | thinking字符串；关闭off不是null时映射??none。 |
| 默认OpenAI | 启用且reasoning/supports时写effort；关闭只在off映射string时写。 |

不是所有分支都同样校验映射string，null/undefined/off在各分支处理不同；上表不能压成一个统一赋值规则。思考token budget字段独立于format：显式thinkingTokenBudgetField优先，兼容布尔才默认thinking_token_budget；启用reasoning时取max_tokens??max_completion_tokens??model.maxTokens，计算level/custom budget并限制answer room，结果>0才返回。模板primitive直接保留；object有omitWhenOff、thinking.enabled、thinking.budget，其他变量走effort映射，undefined则略过；空对象不写入。

OpenRouter routing来自model.compat.openRouterRouting，Vercel only/order构造成providerOptions.gateway。这些是兼容请求数据，未经服务真实性协商。本文件未把继承options里的所有metadata/transport类字段自动转发；只按实际读取字段归属理解。

## 1062–1505：缓存、消息、工具与回放

Anthropic cache格式且retention非none时ephemeral，long并支持时ttl=1h。修改转换后的首system/developer、最后tool、从后找最后可标记的user/assistant/tool文本；非空string变text数组，数组逆向找text（不检查其文本非空），找不到则继续向前。此处没有磁盘或服务端缓存owner。

convertMessages先transformMessages。其新读依赖归一null content、无视觉模型图片占位、按provider/api/model判同模型；跨模型普通thinking变text、redacted thinking与tool thoughtSignature丢弃，正规化tool ID并关联result。第二遍跳过error/aborted assistant，并对未解决工具调用合成No result provided错误结果。正规化callback只在跨模型调用，所以不能说同模型全部ID都受短ID规则约束。

本地normalizer处理Responses call|item：两部分清理字符，合并≤40直接用，过长保留call前缀+原ID短hash8位；普通openai ID仅截到40，其他provider普通ID不改。它未提供数学无碰撞保证。systemPrompt在reasoning且兼容developer时用developer，否则system，并清理surrogate。工具结果后若直接user且compat要求，会插入固定assistant桥。

user文本清理，图像转data URL；空数组跳过。assistant普通非空text合并成string，thinkingAsText时思考加在textparts前；否则保留合法原alias字段或结构化reasoning_details。取thinking块首个有效非空details数组，优先于工具legacy encrypted列表；没有details才由首个非空thinking块的合法标签决定raw字段。空reasoning_content可因兼容要求补齐。最后没有content且没有tool_calls的assistant仍跳过，即使有details；因此“有签名就总能回放”不成立。

工具历史普通function参数JSON.stringify；grammar映射存在时输出custom原始string input（helper要求属性为string）。相邻toolResult逐条发role:tool文本：文本用换行合并，只有图片用see attached image，空结果用no tool output，兼容时加name。符合模型输入能力的图片聚合成后续user图片turn，必要时插assistant桥；Kimi addedToolNames再发无content的system tools声明。group结束更新lastRole，避免简单逐消息模型误判桥的位置。源没有工具执行或journal写入。

convertTools优先受支持grammar，输出custom grammar的lark/regex定义；否则function JSON schema。必要helper：grammar不支持时返回undefined可降普通function，支持但无有效variant或非单一必需string属性时报错，lark优先regex；增量buffer要求输入前缀单调，关闭后不得变。strict schema clone后收窄对象、必填全部属性、可选转nullable，拒绝若干复杂schema；require无法满足则throw，非require可降级，支持strict时字段默认false。不把“strict字段存在”当工具参数已在本层通过执行验证。

## 1507–1717：usage、stop、兼容矩阵

parseChunkUsage以最新快照替换usage。cacheRead按prompt_tokens_details.cached_tokens??prompt_cache_hit_tokens??cached_tokens??0，显式0不会回退；cacheWrite单独字段||0。input=max(0,prompt-cacheRead-cacheWrite)，output=completion_tokens，reasoning_tokens是output子集不再次相加；total重算input+output+cacheRead+cacheWrite，忽略服务的total_tokens，然后calculateCost。floor只保护input，不是对所有字段做非负/有限/单调/一致检查；cache合计超过prompt时total可能大于prompt+output。这个差异来自实际源码，不冒充旧异常数值测试。

stop映射如前述，null辅助分支不绕过caller的truthy条件。detectCompat按provider/baseUrl子串判Zai、Together、Moonshot、OpenRouter、Cloudflare、Nvidia、Ant-Ling、DeepSeek等；多数大小写敏感，仅特定DeepSeek URL lowercased。nonstandard关闭store/developer等，useMaxTokens名单与nonstandard名单不同；Grok/Zai/Moonshot等effort开关、strict支持名单、OpenRouter特定模型developer、Anthropic模型cache格式、affinity/long retention均有独立分支。未知模型默认亦可能被启发为支持，不是保守协商结果。getCompat逐字段??override，false有效覆盖，null/undefined回落；routing/templates/deferred/cache/priority各保持自己的默认。不能将域名子串当身份认证，或把所有兼容默认宣称为远端支持。

## 当前 Zenpi 对应实现与差异

| 边界 | 当前实际接线及限制 |
|---|---|
| 请求/能力 | backend:970–1115验证model与model×wire能力、附件/structured output，Chat构造messages、function tools、auto、system instructions、metadata；1170附近按capabilities修正stream/remove stream_options，cap受model.max_output_tokens约束并必须正数，1180–1212写max_completion_tokens/response_format。1798–1856把图片绑定对应user turn。原生Responses分支有自己的effort配置，不应把它算到Chat。目标没有源十一格式、任意sampling/onPayload、grammar/Kimi、cache/affinity/routing全集。 |
| HTTP | backend:1219–1302共用请求私有transport cancellation；Chat凭据走Bearer（secret handle或api_key），identity encoding与有限body轮询，status≥400生成错误。1342–1434按content-type分SSE/有界JSON。源SDK streaming接口及其callback与此不是同一合同。 |
| Text/reasoning/refusal | chat_stream:1–188已经三个alias首非空、opencode-go、相邻text/summary合并和encrypted保存；所有出现alias类型均检查、details无效直接拒绝，限制64KiB/128项，源更宽松且跳过无效detail。Stream发独立owned delta/refusal，details只在成功annotation中发布。源单一可变partial及contentIndex/end协议不同。 |
| 工具增量与ready | Stream:420–614使用u64 index且<128、BTreeMap按index顺序，ID/name出现后不可变化、有界≤512且无control，全部ID唯一，最终parse_tool_call严格JSON object。允许晚到身份但不允许源那种更名map别名/partial repair/custom类型。整组校验和reasoning annotation完成后才发TextDone/ToolCallDone/Usage/Completed；任一坏工具会阻止全部ready。 |
| Terminal/identity | 仅stop且无工具或tool_calls且有工具完成；length/filter/未知/缺reason失败，空可见内容+无工具+无refusal则EmptyResponse。SSE每choice需index0且唯一，JSON允许缺index但若有须0；响应id/model出现时有界且不可变，**并非都强制存在或等于请求model**。源只保留首次、接受length/end/function_call，且不核对工具存在。 |
| EOF | read:616–697显式4MiB总响应、256KiB frame；未闭合SSE行/data EOF失败，有效finish后clean EOF可成功，无需DONE。DONE要求已有reason；当前已读chunk中的后续data拒绝，完成该chunk后停止读，不能声称检测所有尚未读到尾字节。忽略非data行也不代表实现所有SSE语义。 |
| JSON fallback | validate_json:349–418验证error/status/incomplete_details、唯一choice、工具全集/身份/finish；backend随后抽content、reasoning、annotations，拒绝provider伪造保留envelope后才发事件。JSON若内容空且无工具会拒绝，SSE的refusal允许条件不可直接泛化到JSON。 |
| Replay | chat_stream:190–287读取host native_history/chat_reasoning，限定assistant role、唯一envelope、封闭外层字段、大小、相同wire/provider/model与tuple SHA一致；details优先raw field回放。摘要覆盖reasoning tuple，不认证供应商签名，也不绑定整个assistant text/tool transcript。源跨模型降级/补结果与目标拒绝不兼容历史不同。 |
| 持久化/工具owner | backend:1503–1555预检history，1750–1798 chat_message调用replay；core:3468–3495普通completion保存annotations，3865–3888工具轮保存canonical calls/annotations/model并append_turn，再进入gate与执行owner。源无durable owner；此映射不是完整session实现验收。 |
| usage | backend:2565–2585读prompt/input、completion/output、显式total，缺/非法数值默认0，缺/非法total才saturating_add，结果Some(Usage)。无cache/write/reasoning/cost分项、无单调及total一致检查；SSE仅先约束usage是对象。不能把Google provider较严格usage校验移用到Chat结论。prompt字段存在但非法时不会回退input_tokens。 |
| cancel/retry | Chat read sink wrapper逐事件检查取消，terminal TextDone取消可阻后续tools，首ToolCallDone取消只阻后续ready/Completed，已发事件不能撤回。backend:1610–1698只有本attempt未emitted、retryable、次数可用且非semantic_compaction才重试；SSE开始后的I/O错误变InvalidResponse，避免重复delta。源retry只包SDK建流，默认0，SDK内层0；helper按status/header和可中断backoff，未覆盖callback任意挂起。transport非macOS DNS/nonUnix connect限制仍保留，旧pre-abort不能证明解决。 |

当前chat_stream.rs `80da76080a8ccc6d54a7c7369804513c0a4371601c4e9b62a9805571471a07ba`、backend.rs `d11105597a7e4260c67c9ce7796394687b4219617a6ebfdfeca637ae469c3357`、registry.rs `cf229a62681933e7c1b4058c4d2fada41dff4fcd05c4cd300e18accfe245b944`、core.rs `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869`、stage1_chat_stream.rs `39710fa209ec83b3de359b9574c08b4b8ae1740aff2c93c3925df256829223f3` 与C最后映射五文件身份一致，15个原映射片段逐字节复用。旧“缺reasoning_text/details”的首版结论已被当前源码替代；旧文件不改。

当前测试22个定义。新读489–575的真实Agent/SessionStore案例有三次loopback请求、工具执行、journal重开，断言summary/text合并、encrypted离散、details优先raw和重启回放；1390–1454两项terminal取消测试分别断言零ready或已发一个ready且无Completed。本轮只读断言不执行。当前headless `521c2102519b502fbd6184f31ffb53cba8eeef840bf84d19450836429d9b49f0`、headless_domain_owner `6f121e2ea3e1f7f0ffc19d8470f94662163b1eb1e78cd3a71c97975ad06361e6` 是额外当前组合上下文，前项已读的显式blueprint版本修复与回归按hash复用；它不改变此处五文件哈希，也不使旧CLI运行成为当前全量一致证明。本项不接受该domain功能。

## 实际历史运行、失败与未覆盖

本轮全文新读历史probe.ts（36行、11700字节，SHA `24b10046c6b517ba6aa43299815c00fd22a0b1b928afb1b64c9c9f247d30b1a6`）。原source-native于2026-09-10T19:47:19Z exit0，日志3913B、SHA `b29b0e1048b4fdfebd4074a6035addfa7ed07c89bd1b9b80ebfda27cce9b1bf2`。直接import冻结源，真实openai6.40.0与partial-json0.1.7，Bun.serve随机loopback端口回确定性SSE、fixture-key，mocked_imports=[]。**33个case产生37次HTTP，37不是额外测试数**；包锁/安装日志/原命令精确保留，未安装或运行。

实际断言涵盖端点/auth/developer/output/usage option/onResponse；首alias；首identity/serving model/choice；交错工具index和late ID/first-seen顺序；partial JSON修复；early block end后缺reason错误与显式EOF推断；clean EOF；七种额外finish；cache aliases/writes正常值与reasoning子集；text/encrypted details回放；sampling与top_p；十一种enabled/high格式；simple max clamp与976预算；显式compat/header auth；两条短pipe ID和空tools历史；cache/affinity/routing；缺auth及pre-abort。

必须保留的细分限制：onPayload案例名称虽写without forced stream，实际只加top_p、未改stream:false；IDs without collisions仅比较两个短ID；十一format不证明全部off/null分支或真实厂商兼容；details只断言部分text/encrypted，不穷举summary/legacy；usage未验证每个cost及异常数值；所有源请求maxRetries=0；预abort不发HTTP不是DNS/connect/midstream/backoff取消。grammar/custom、图片结果、Kimi、callback异常/挂起、整个上游suite/tsc、真实服务均无本轮运行。静态发现宽松边界未转成未授权新测试。

原target-chat于2026-09-10T19:47:50Z exit0，日志967B、SHA `b5b5b0e722aa029edc1599eda24630aca7cac0a8c2f7a506c10778e8ae0d1c47`，当时8pass。后续029包引用的108历史收据分别有15 Chat（reasoning）、20 Chat（JSON联合）、22 Chat（contract318；该命令另含15 backend+1 event budget，共38）。原日志SHA依次 `35d9342470d577ff60a94a8e765867790f53ff4abf2f017e78a81c9fe4e2a9dc`、`ceecce102e89e0dc38626b395f2b18c299b7c111e625d5eed9bc14aede154661`、`62397dfc27adc126c76fe8a8194545d1a8ae2588d17bc2fcc731c4d11ab8200e`。这些是不同历史组合，不相加为独立覆盖，不接受联合命令的其它项目；联合日志中其它suite的ignored/filtered记录全部保留，不宣称全组无跳过。当前22定义与较后22案例身份可关联，但不是本轮新pass。

原source33case没有失败；独立离线/门禁失败仍真实存在。reuse320 integrity-final exit1，log328B SHA `26529ee882790d6f280e8191489cf30e5cb35537354b88dea51f37452bb057b6`，因为core由6a881…变52311…；旧initial refs、有限片段复核与后续integrity-reviewed exit0均保留。reuse320和current-map320 G-STAGE都是exit1、structural.ok=true、缺ZS1-029.master.json；原log408B SHA `a50371e0fb52fe2c463c9ee3718ec51b1dc7126970edf4f60a6137ebfe186877`。不改旧哈希制造当前通过，不执行旧门禁掩盖失败。

精确原source/target replay argv、cwd、起止时间在worker-source-probe/source-native.json与target-chat.json，后者是cargo +stable-aarch64-apple-darwin test --locked --test stage1_chat_stream -- --test-threads=1；source是NODE_PATH指向C私有runtime的Bun probe。旧bounded replay wrapper也只作历史文件。新运行数全部0：源/产品测试、Bun/Node、Cargo/build、npm/install、HTTP/网络、PTY、budget。

## 离线核对与交付

`verify_offline.py <包目录>` 仅Python标准库验证包内identity、完整阅读区间、新/旧context、三包物理完整性、报告前缀、实际收据和失败、补丁内存正逆重构、唯一候选报告；不导入或运行旧脚本、不应用产品补丁。`--live`在原机额外比对相关当前代码、029权威行和5813个受保护旧ready文件。对其它项勾选/Gantt只保留捕获，不作为029失败，也不改全局checker规则。

本轮只写新私有报告与证据包，不写主库报告、产品、authority、claims或旧封存输出。静态理解与离线通过均不代替主控语义接受；完整兼容矩阵、SDK实时取消、故障重试、当前CLI组合的执行结论仍按上述证据界限保留。
