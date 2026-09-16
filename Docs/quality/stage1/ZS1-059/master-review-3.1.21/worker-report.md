# ZS1-059 packages/ai/src 独立目录整合报告 [_]

Worker A，authority 3.1.21。本候选仅审阅 `packages/ai/src` 的目录整合关系，正式直接子项为已独立接受的 ZS1-015 `types.ts` 与 ZS1-055 `api`。它们的接受不自动关闭059；父项062、其他物理文件/子目录及 Zenpi 产品能力均不由本候选验收。只提交这一份报告补丁，等待主审。

绑定 requirement `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`；baseline `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`；run `zenpi-stage1-20260911`。捕获 selector snapshot 为 `1b02a7d9b19e1ad6caa2c1bb0c068af7bc911962ab6f8a69435280fb3d17207a`。权威全文、相关索引和末次观察随包携带；无关全局进度变化不代替本项范围检查。

## 实际范围、阅读与依赖

真实目录为19个直接文件、5个直接目录；正式集合只有上述两项。下表列全24项。`direct-inventory.json` 记录字节、行数、完整SHA及五个目录的下一层真实清单；下一层清单不是递归源码全读或独立验收。

| 直接项 | 字节/行 | 范围与职责 |
| --- | ---: | --- |
| `api` | 目录 | 正式055；32文件、0目录；接受后的目录整合复用 |
| `auth` | 目录 | 上下文；5文件、1目录；只读所列认证连接文件 |
| `bedrock-provider.ts` | 140/6 | 上下文全文；Bedrock静态实现入口 |
| `bun-oauth.ts` | 911/21 | 上下文全文；Bun显式OAuth loader注册 |
| `cli.ts` | 4295/119 | 上下文全文；独立登录CLI和相对路径存储 |
| `compat` | 目录 | 上下文；1文件；兼容 OAuth 类型全文 |
| `compat.ts` | 10596/298 | 上下文全文；旧全局文本注册及分派 |
| `env-api-keys.ts` | 7560/188 | 上下文全文；旧环境认证查询和ambient标记 |
| `image-models.generated.ts` | 21014/789 | 上下文全文；图片静态目录数据 |
| `image-models.ts` | 1667/42 | 上下文全文；兼容图片目录查询 |
| `images-api-registry.ts` | 1640/53 | 上下文全文；图片全局协议注册 |
| `images-models.ts` | 9380/275 | 上下文全文；图片容器、认证与刷新 |
| `images.ts` | 727/21 | 上下文全文；旧图片生成入口/注册副作用 |
| `index.ts` | 2343/48 | 上下文全文；新容器入口与显式导出 |
| `legacy-api-aliases.ts` | 6009/108 | 上下文全文；旧固定 API lazy 别名 |
| `model-catalog.ts` | 920/27 | 上下文全文；生成目录分组扁平化 |
| `models-store.ts` | 1662/45 | 上下文全文；存储接口和内存克隆 |
| `models.generated.ts` | 6569/124 | 上下文全文；静态模型目录组织 |
| `models.ts` | 34837/957 | 上下文全文；模型集合、刷新发布、认证应用、分派和成本 |
| `oauth.ts` | 281/10 | 上下文全文；兼容OAuth类型导出 |
| `providers` | 目录 | 上下文；87文件、1目录；仅所列工厂/注册入口全文 |
| `session-resources.ts` | 671/24 | 上下文全文；同步资源清理集合 |
| `types.ts` | 37411/881 | 正式015；完整阅读身份复用 |
| `utils` | 目录 | 上下文；23文件；仅所列必要工具全文 |

本轮实际完整阅读18个直接上下文文件；另完整阅读19份必要上下文：上层 package.json，api/lazy.ts，auth/context、credential-store、resolve、helpers、types、oauth/load，utils/abort、provider-env、event-stream、diagnostics，providers/all、四个具体工厂以及 images/register-builtins，compat/extension-oauth-types。合计37份、4663行，逐份全字节区间与行区间在 `read-ranges.json`，原件副本在 `source/`。生成文件只阅读其当前静态内容，没有执行生成器、数据同步或上游构建。其他 auth OAuth flow、provider catalog/data、工具模块及大型非正式 API 的完整实现未因本次清单或导入关系而获覆盖。

015 的37411字节/881行、SHA `f2edab10f093a15ba2cdeaaf4fa1634d76ee943e6f62b2f44bdb2d14321834e0` 与当前源码逐字一致。复用其已接受完整阅读的四段 `[0,8064) [8064,15965) [15965,28201) [28201,37411)`；本轮完整阅读 canonical 与3.1.21 rebind，不把哈希校验称为新重读881行。015回执75项附件全部核对并原样复制，包括旧报告、阅读切片、旧目标快照、失败门禁日志和历史补丁。

055 的当前 canonical 33597字节完整重读，采用最终 master acceptance 对历史候选的限定：016/017/029 现均接受，旧候选捕获时029仍pending的原文保留。055回执13项附件全部核对；其1028载荷 worker archive 和完整 manifest 原样携带，仅以只读 tar/JSON 核对内容，绝不执行归档脚本。015/055 合计88个直接回执附件，不与归档内部文件数量混加为测试数量。源类型的接口契约、三个协议适配器的折叠职责分别依托015和055，以下是目录059补上的入口、模型和认证组合边界。

## 入口和分派：类型、模型、协议三层身份

`index.ts` 是新容器式入口：导出 Models、ImagesModels、内存存储、认证类型/帮助器、lazy 与基础事件/校验工具。根入口没有直接导出全量生成目录、全局 API 注册表、OAuth flow 实现或 CLI；`createModels()` 初始 provider Map 为空，应用通过 setProvider 显式装配。根入口仍导出 faux provider 工具，因此这里只陈述实际导出和初始化路径，不声称任意传递依赖都没有副作用。package.json 区分根入口、compat、providers/api/utils 子路径、OAuth/Bun/Bedrock 入口，并把 compat、images、image register-builtins 标为 sideEffects。打包条件是声明，本轮未构建检验。

`Models` 用 model.provider 选择 provider；`createProvider` 再按单一 ProviderStreams 或 model.api 映射分派。有 stream 函数的单一实现直接调用；映射缺项返回 lazy error stream。model.id 用于查目录和请求，不能凭 provider 名推断具体 wire API。实际四工厂确认：OpenAI 默认 Responses；Anthropic 为 Messages；Google 为 Generative AI；OpenRouter 的表按 API 选择 Anthropic/Chat。055的029 Chat 不能替代所有 OpenAI 请求的验收。provider 的 baseUrl 是元数据；具体请求使用传入 model 和鉴权派生的 requestModel，不因某个 provider.id 相同就自动验证 model 全部字段或强制属于当前目录。

`getModels()` 对 provider.getModels 的同步异常返回空列表；`getAvailable()` 先并发检查凭证/认证，再直接调用 getModels 和 filterModels，没有相同的 catch，可能拒绝。checkAuth 对已有 OAuth 只检查是否存在匹配 handler，不刷新 token，也不探测远端是否接受；有 apiKey.check 时优先用它，否则 resolve 可能有请求时工作。所谓 available 是配置/过滤视图，不是联网验证、真实模型能力或实时服务健康。

`compat.ts` 在模块加载时注册10类内置文本 API、构造内置 Models，并导入图像兼容入口。注册表按 API 覆盖；sourceId 用于批量撤销；registerBuiltInApiProviders 只填空项，reset 则先清表。它同时更新 builtinApiProviderInstances；stream fast path 比较当前注册实例与该记录，再检查 provider 的模型列表是否存在相同 API。通常注册后的新 override 因对象身份不同走注册表；若再显式调用 registerBuiltInApiProviders，记录也会更新。因此“注册函数不覆盖现存项”不等于任意调用序列都保证 stream 执行该 override，本轮不对此做运行验证。匹配检查是相同 API 存在，不是传入 id 必须在 catalog 中。

compat 对非空显式 apiKey 优先，否则做环境注入，跳过 `<authenticated>` ambient marker；Cloudflare 在未获得特定认证字段时走容器认证路径。普通 compat fast path 直接调用 provider；其全局 API 缺项/不匹配可以同步抛错。legacy-api-aliases 的旧 stream 名是固定 lazy factory 的兼容导出，没有额外模型资格验证。它们和经过 Models 的错误传播应分别解释。

## 认证、请求选项与存储所有权

Models 默认使用 InMemoryCredentialStore 和默认 AuthContext，可注入应用自有存储/环境查询。共享 resolveProviderAuth 的显式 apiKey（且 provider 有相应 handler）优先于存储；否则使用存储的匹配凭证类型；没有存储凭证时才选择 ambient apiKey 解析。已有存储 OAuth 刷新失败不会悄悄退回另一个环境 key。具体 apiKey handler 仍可把 stored key 与环境配置按字段结合；例如 Anthropic 存储 key 优先，再识别 AUTH_TOKEN 为 Bearer header，之后才取 OAuth token/API key。compat 的 getEnvApiKey 特意不把 AUTH_TOKEN 当普通 key，不能把两入口等同。

共享请求认证会在 OAuth 有效期不足默认5分钟时进入 credential.modify，并在锁内再次检查，防止并发刷新旋转 token；显式 minValidity 与默认窗口取大者。refresh 调用带15秒 timeout 与 caller signal 的组合。这是合作式取消：raceWithAbortSignal 只停止等待，忽略 signal 的实现仍可能运行；timeout signal 不是强杀网络/回调/存储的保证。Models 的目录刷新凭证逻辑另在实际过期时才刷新，不能套用请求认证的5分钟窗口。

CredentialStore.modify 的 undefined 表示不更改，不是删除；logout 走 delete。默认内存实现按 provider 串行 modify/delete，但 read 不排队，且凭证对象不 structuredClone，可观察旧值/共享引用。接口允许应用自有跨进程锁或尽力持久化存储，默认 Map 不提供磁盘恢复。login 先完成交互，再排队写入；若写入已经开始，Models 等待存储的结束，而存储自己的 signal 检查仍可能取消提交，不能声称取消后必然保存成功。

文本 Models.getAuth(model) 将 auth headers 与 model.headers 按不区分大小写的键合并；applyAuth 再用显式请求 headers 覆盖，最后调用 provider.transformHeaders。显式 apiKey 优先，env 按键合并，auth.baseUrl 通过浅复制改写 requestModel。transformHeaders 的最终输出仍由 provider 负责；这些步骤不是深冻结或任意值/schema 验证。provider 在 await auth 前后可能被应用替换，容器未建立覆盖整个异步请求的事务快照。

AuthContext 从 process.env 取非空值并提供文件存在查询；provider-env 支持 override/process/Bun 的另一套回退。空字符串在若干 `||` 回退路径不能遮蔽底层变量。env-api-keys 的 Node/Bun 文件查询通过模块异步导入准备：显式 ADC 路径检查和默认路径缓存条件不同；AWS/Vertex 的 marker 由环境/路径存在条件产生，不代表登录成功。本轮只读源码，未访问实际凭证、auth.json 内容或执行认证。

## 模型刷新、目录数据与成本

文本 Models.refresh 对所选动态 provider 并行开展；每个 provider 有 generation 和 AbortController。新 refresh、替换、删除、清空会 supersede 旧 generation。先读取存储凭证并记录错误，仍尝试 cache-only refresh phase，之后才处理凭证错误或选择网络阶段；即使 allowNetwork=false 也可以恢复缓存。每阶段从 ModelsStore 读出并 structuredClone 后传给 provider，实际刷新与 publish 提案由 provider 实现。

publish 按 provider 串行，写/删 store 前后均检查 signal 和 generation，之后才调用 update。这个协议能拒绝过时的内存发布，但若自定义 store 忽略 signal 且已持久化，后置检查不回滚已发生的外部写入；自定义 update 副作用也没有事务回滚。cancel 可使调用者停止等待，不证明所有后台任务停止。返回 aborted 表示 caller signal 状态，单个被新 generation 替代的工作不必使该字段为 true；失败收集在 errors Map，不宜把无抛异常理解为全部刷新成功。

默认 InMemoryModelsStore 对读写 structuredClone，并检查 signal；ETag 按 opaque string 保存。它与不克隆凭证对象的内存 CredentialStore 不同，且两者都不耐进程退出。createProvider 将动态结果按 model.id 覆盖/补充静态 baseline；恢复缓存时按 provider 过滤，未对整个 model schema、API 或版本做全面验证。返回数组和对象引用的只读 TypeScript 声明不等于 runtime freeze。

`models.generated.ts` 组织静态 provider catalog；`providers/all.ts` 创建具体工厂，builtinModels 显式逐个 setProvider，并没有自动调用网络 refresh/login。catalog helper 使用 Object.assign 扁平化各组，重复 id 后组覆盖，_provider 参数不执行运行时身份校验。类型化 getBuiltinModel/getImagesModel 的查找仍可能在运行时得到 undefined，不能被返回类型强转掩盖。完整 provider 数据和生成器不在本轮上下文覆盖内。

calculateCost 会原地改 usage.cost；选层依据 input+cacheRead+cacheWrite，总输入严格超过阈值且阈值最高者获选。长时 cacheWrite1h 从总写缓存中拆出，以 input rate 的2倍计算，其余使用 cacheWrite rate。函数没有统一检查负数、NaN、有限范围或版本来源。图片生成表含0及 auto/auto-beta 的负值哨兵；读取这些静态数据不确认实时价格，也不能推出免费或可直接用于 Zenpi 账本。015已指出 usage/cache/reasoning 字段存在跨协议口径差异。

thinking 支持列表受 reasoning、显式 null 和 xhigh/max 的显式映射限制；clamp 先向更高序位寻找，再回退，并非简单最近数值。modelsAreEqual 仅比较 id/provider，忽略 API、URL、成本及能力；hasApi 只比较 API。类型中有 Max，不能沿用“源没有 Max”的旧误读；目标 ultra 或目标能力交集不能由源级别名直接保证。

## 事件、图片、清理和平台入口

文本 Models.stream/streamSimple 通过 async setup 解析 provider/auth，再交给 lazyStream，因此这些 setup 拒绝进入 error stream；complete 只是等待 result，终态 error AssistantMessage 本身不等于 Promise 拒绝。createProvider 的有效实现直调仍可同步 throw，经过 Models 的 async setup 时才被转换。streamDeferredFetch 的不支持错误走流；cancelDeferred 返回 Promise，可拒绝。055三个正式 lazy wrapper 本身均没有 deferred 能力。

lazyStream 对任意直接同步 throw 的 setup 不包 try；lazyApi 的 async setup 才消除该直接同步边界。转发迭代/result 的异常也进入 catch，其新建失败对象用 stopReason error，不保证保留 AbortError 为 aborted。事件逐个同引用传递，不深拷贝、不验证、不背压。EventStream 在 done/error 时解决同一终态对象；之后 push 被忽略。end 没有 result 且未遇终态可能留下 pending result；多个消费者共享队列分取事件而非广播。目录调用层因此不能承诺任意自定义 provider 一定产生终态或有限时间结束。

图片有独立容器、模型目录和全局 API 注册表。ImagesModels.refresh(provider) 包装失败为 ModelsError，而刷新全部用 allSettled 丢弃失败；没有文本 refresh 的 generation/store/publish 协议。createImagesProvider 为并发刷新共享一个 in-flight Promise，成功替换列表。图片 getAuth 直接返回 provider auth，未像文本容器合并 model.headers；generateImages 的 auth/request headers 使用普通对象 spread，键大小写语义也与文本不同。

静态发现：`images-models.ts` 的 generateImages 注释称 Never rejects，但无 auth 分支写 `return provider.generateImages(model, context, options)`，没有 await。若合规自定义 provider 返回一个之后拒绝的 Promise，该拒绝由 async 函数采用返回，却不进入这一 try/catch；有 auth 分支的 `return await` 才会捕获。同步 throw 仍在 catch 内。这是具体源码控制流缺口，尚未运行复现，也没有修改产品或上游。主审应保留该限定，不能从统一图片返回类型推导所有失败都变为 AssistantImages。被 catch 的失败返回 stopReason error，包括相应取消错误，未单独改为 aborted。

旧 `images.ts` 导入 register-builtins 的模块副作用；图片注册函数按 API 无条件写入，模型 API 不匹配时包装器会 throw。内置图片 adapter 采用缓存 import Promise，导入/调用错误返回 AssistantImages，但此局部行为不能为任意 ImagesProvider 弥补上一段缺口。图片 registry 的 sourceId 虽存储，当前文件没有对应批量移除接口，不能沿用文本 registry 的 unregister 契约。

session-resources 仅维护同步 cleanup callback Set；遍历全部 callback、收集异常后抛 AggregateError。返回的注销函数移除 callback；不自动等待 async Promise、不自动触发会话持久化或网络流排空。cli.ts 是独立命令入口，登录后同步写相对 cwd 的 auth.json、读取失败视为空，最后关闭 readline；其模块 main 会执行。这里没有启动 CLI 或访问实际文件。oauth.ts 只导出兼容类型，bun-oauth 静态导入七种 flow 并在显式调用时注册 loader；lazyOAuth 缓存首次加载 Promise，失败 Promise 也会保留。Bedrock 独立入口静态导入真正实现；本轮没有 Bun/Node/浏览器打包证明。

## 与 Zenpi 的责任映射及历史证据

目录层新理解不意味着 Rust 已具有全部 TypeScript 模块的同构实现。015 的类型映射和055的 API 映射仍是边界依据：backend/registry 负责原生分派、模型身份、能力与 reasoning 校验；各 provider fold 和 Chat stream 校验终止、块/工具参数，再发布完成事件；core/headless 负责回合、工具执行、会话和取消组合。源侧 partial/toolcall_end、补合成 toolResult 和宽容 JSON 修复不能充当 host 工具授权或持久化事实；真实定价元数据、整数边界和 provenance 属于目标所有者。

本轮没有重新阅读整个当前 Rust owner 集合。为防止误用旧测试，按055归档的47项 input inventory 做当前身份核对：Pi 输入全部相同；目标输入只有 headless 从521c…的368974字节变为bf252…的370207字节，与055 master 已注明的差异一致。完整 drift 记录和当前目标副本仅用于身份审计，不宣称新执行覆盖；055的 headless-delta.patch 及 review 原样保留。015旧 core 274600字节快照仍按原时间保存，不能当当前 core。055更早的三个协议目标漂移也保留在其历史记录中，不被此次47项比较抹掉。

本轮完整重读055历史54行 probe、命令收据和1577字节原始日志。2026-09-10T23:25:58 的旧执行为9场景/3次 loopback HTTP，经三个真实 lazy factory、adapter 和 SDK。每 API 分 success/missing-auth/preaborted，断言同步返回流、单一终态、result 同对象；成功检查 hello/path/auth，其他两类检查无HTTP。中间事件名称被记录，但未完整逐条断言；只显式检查 cancelDeferred，不检查 fetchDeferred。没有覆盖中途取消、模块导入失败、多消费者、凭证刷新、Models.refresh 发布、图片或真实进程恢复。15秒 watchdog 是旧探针保护。本轮只读这些文件，未启动 Bun/HTTP/SDK。

旧 leaf 23/17/33 场景、24/20/37 HTTP，旧 target 17/17/8 通过，均保持原时间与输入，不与9/3或离线哈希校验相加。015 缺 master receipt 的 G-STAGE exit1、历史首次未初始化 git patch 应用失败及修正，055缺本目录回执的失败、016错误预期及修正全部保留。搜索本地指定 packet 根未找到先前059包或 canonical；该记录不宣称全机不存在历史059。不得凭没有找到本目录旧包而删除任何子项证据。

## 交付、校验和回滚

新 verifier 仅用 Python 标准库读取封装文件、JSON、tar，并在内存比对报告补丁；源锚点检查只证明相关代码存在，语义结论仍需独立阅读。校验内容包括88个依赖附件、015完整阅读切片、055的1028载荷归档、真实直接项与正式索引、37份新阅读范围、历史日志限定、当前身份及单一路径补丁。实际命令/UTC/exit/stdout/stderr 保存在 ready 外的工作目录。未运行历史脚本、G-STAGE、SDK、构建、产品、性能、FIFO 或131实验，本轮运行行为测试数为0。

旧117只读诊断包与single-markers包全部 payload 在开始及末次只读核对；原 fixture 保留原内容，不清理、不再启动。117 FAIL 状态及预主入口时段结论不由059改变。工作树94条非.ops状态保持原样，产品源码、主线报告/receipt、全局checker、claims、Gantt和BentoBox未写入。

候选补丁只新增 `Docs/learn/stage1_pi_mono/packages/ai/src/current_folder_learn.md`。主审核阅后独立决定059；回滚仅撤去本候选报告，若主审后续建立receipt/status则由主审同步撤回相关投影，不动015/055、其他上下文源码、旧ready或产品。062仍需自己的目录整合审阅。

离线审计准备补记：首次167载荷包的审计于2026-09-12T23:34:33Z exit1，原因是校验器用首次split匹配getAvailable，命中了接口声明而非实现；更正为最后一次匹配。原包、原manifest、原verifier及失败日志全部保留。修正版在独立目录封装，未修改任何源码或旧证据；该准备失败不是运行行为失败，修正后的实际审计结果见外部新日志。
