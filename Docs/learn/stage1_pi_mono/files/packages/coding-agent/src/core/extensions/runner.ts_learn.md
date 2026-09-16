# ZS1-023 — packages/coding-agent/src/core/extensions/runner.ts

Worker candidate: provisional；learn_mode: understand；master 独立语义 G-FILE pending。

source_id: SRC-0908
source_path: packages/coding-agent/src/core/extensions/runner.ts
source_hash: 6d5101ab0551c2ddd904a8089cc221b3c448fcfe36c27e36da02cdacc869c084
source_bytes: 39774
source_lines: 1286
coverage: 完整 [0,39774) 字节；按 1–220、221–440、441–660、661–880、881–1100、1101–1286 连续全文阅读。声明、常量、默认UI、公开/私有方法、闭包、所有dispatch和异常分支、文件尾均覆盖，非符号搜索替代全文。小于256KiB，无强制大文件分块义务。

唯一标准报告 `Docs/learn/stage1_pi_mono/files/packages/coding-agent/src/core/extensions/runner.ts_learn.md`。本轮先检索自身.ops/Docs及主库Docs/learn，未找到已有完整同source ready/标准报告；master/worker标准路径捕获时均absent，因此新建，不覆盖旧结论。022 types.ts报告由另一worker处理，本轮没有读写该报告或接受其整文件；仅C2返回类型片段作为本文件dispatch解释的上下文。052目录整合未执行，须待主控独立接受依赖。

## 类型、职责与顺序（1–201）

本文件负责已加载extensions的发现查询、冲突视图、ctx绑定、事件顺序和结果合并、错误分发、UI prompt通知与陈旧ctx保护。它不加载TS文件、不执行具体tool/body、不实现provider wire、持久session写入或进程隔离。ExtensionRuntime/Extension/Event/Context/Command/Tool/Result等类型来自types.ts，ModelRegistry/SessionManager/Theme等由外部owner提供；本报告说明实际调用，不把这些依赖算整文件审阅。

RESERVED_KEYBINDINGS_FOR_EXTENSION_CONFLICTS包含editor/global动作：app.interrupt/clear/exit/suspend、thinking.cycle/toggle、model.cycleForward/cycleBackward/select、tools.expand、editor.external、message.copy/followUp、tui.input.submit/copy、tui.select.confirm/cancel、tui.editor.deleteToLineEnd。保留的是canonical action ID，不是写死某几个键；其他picker-specific动作不自动保留。

BuiltInKeyBindings是KeyId到{keybinding,restrictOverride}的Partial Record。buildBuiltinKeybindings遍历resolvedKeybindings，undefined跳过，单键转换数组、键lowercase；相同键已有reserved且当前非reserved时保持旧reserved，否则后者覆盖。因此reserved与nonreserved共用键时不受遍历顺序影响；多个reserved互撞仅保留后者描述。这里只lowercase，不做其它键别名/组合顺序语义规范化。

BeforeAgentStartCombinedResult聚合可选messages数组和systemPrompt。RunnerEmitEvent从ExtensionEvent排除tool_call/project_trust/tool_result/user_bash/context/before_provider_request/headers/before_agent_start/message_end/resources_discover/input，要求专用emit方法；这是TypeScript类型约束，不是运行时拒绝非法类型字符串。SessionBeforeEvent仅switch/fork/compact/tree四类，SessionBeforeEventResult对应四种返回类型，RunnerEmitResult用条件类型选它们，其它事件返回undefined。

ExtensionErrorListener同步void。NewSessionHandler可接parentSession/setup/withSession，ForkHandler有entryId/position(before或at)/withSession，NavigateTreeHandler接targetId及摘要/instructions/replace/label，SwitchSessionHandler接path/withSession，返回cancelled；ReloadHandler异步void，ShutdownHandler同步void。这些是绑定点，不在runner实现新session/fork/navigation文件行为。

绝大多数dispatch按extensions数组顺序、每extension handlers数组顺序串行await；没有排序、并行、去重、handler快照、统一timeout或AbortSignal驱动的loop退出。数组/Map在await期间可被外部修改，本文件不保证不可变注册表。

## 独立生命周期助手与默认UI（202–269）

emitSessionShutdownEvent先hasHandlers(session_shutdown)，有则await runner.emit(event)后true，否则false。true只说明发过分派；generic emit隔离了handler错误时仍可能true，不代表全部shutdown成功；该助手不invalidate、不释放资源、不设置一次性标志。

emitProjectTrustEvent在LoadExtensionsResult.extensions内串行调用每个project_trust handler，同extension可多个。仅handlerResult.trusted===undecided才继续，其他返回直接作为result与之前errors返回；catch把Error.message/stack或String(nonError)放errors继续。它不调用runner、不用其errorListeners或stale状态；ctx由调用者提供。类型期望yes/no/undecided，但运行时未验证只有yes/no可决定：undefined/null访问.trusted会抛并被记录，普通{}等非undecided结果会提前返回。S1正常断言undecided之后no获胜；不能把这项扩展为完整恶意结果schema测试已通过。无任何决定返回{errors}，记忆trust的持久化由调用者处理。

noOpUIContext是共享fallback：select/input/editor/custom返回undefined（custom只是类型cast），confirm=false；notify为空操作，onTerminalInput返回空退订函数；setStatus/setWorkingMessage/setWorkingVisible/setWorkingIndicator/setHiddenThinkingLabel/setWidget/setFooter/setHeader/setTitle/pasteToEditor/setEditorText/addAutocompleteProvider/setEditorComponent/setToolsExpanded均空操作；getEditorText空串、getEditorComponent/getTheme undefined、getAllThemes空、getToolsExpanded false、setTheme返回success:false与UI not available；theme getter读取导入theme。它让无UI模式也能调用UI接口，但并不实际展示或获取用户批准。

## 构造、core与command绑定（270–440）

ExtensionRunner的constructor保存extensions/runtime/cwd/sessionManager/modelRegistry引用，uiContext初始noOp、mode=print。errorListeners Set、两个diagnostics数组、staleMessage、uiPromptDepth/activeUIPrompt属于本实例。默认context actions为getModel undefined/getScopedModels空、isIdle和isProjectTrusted true、signal undefined、无pending/contextUsage、systemPrompt空/options cwd；abort/compact/shutdown/wait/reload no-op，new/fork/tree/switch默认返回cancelled:false。这些默认返回不证明会话已实际改变；调用方必须绑定真实actions。

bindCore先把actions的sendMessage/sendUserMessage/appendEntry/setSessionName/getSessionName/setLabel/getActiveTools/getAllTools/setActiveTools/refreshTools/getCommands/setModel/getThinkingLevel/setThinkingLevel直接赋到共享runtime，所有引用该runtime的extension API后续可观察这些更新。本层没有对每个赋值做assertActive wrapper，也没有bind一次限制。

随后保存contextActions的model/scope/idle/trust/signal/abort/pending/shutdown/usage/compact/systemPrompt函数，getSystemPromptOptions缺省回cwd。按顺序flush pendingProviderRegistrations，再pendingNativeProviderRegistrations：优先providerActions对应callback，否则ModelRegistry.registerProvider（native也用其重载）。每项异常emitError含extensionPath,event=register_provider,message/stack并继续；每类完成后把pending数组置空。若error listener抛，flush可中断、数组未清，前面成功注册不回滚，下次调用可能重放；不宣称事务注册。

flush完成后runtime.registerProvider/registerNativeProvider/unregisterProvider变成立即调用外部callback或ModelRegistry的方法，不再排队，也无本层catch。register/unregister不自动校验活动generation，不记录session provenance；调用入口是否有runtime.assertActive是loader API包装的其它范围，不能把此处直接赋callback解释为沙箱。

bindCommandContext传actions则绑定waitForIdle/new/fork/navigate/switch/reload；缺省参数明确重置为no-op和cancelled:false，与bindCore可选provider callback回退不同。setUIContext有UI则wrapUIPromptContext，否则noOp，并保存mode；hasUI依对象身份判断，不仅取mode是否tui。getUIContext返回内部UI引用，外部保存后不会在每次直接UI调用自动重新走ctx.ui getter。

## UI prompt计数与事件（441–490）

wrapUIPromptContext先object spread ui，再只包装select/confirm/input/editor/custom为withUIPrompt；其余方法复制引用，getter经spread会被求值为属性，不保留原UI所有descriptor。包装函数用原ui对象调用原方法，custom无title。before/after通知不包括notify或其他非prompt UI方法。

withUIPrompt先depth++，只有从0进入时保存first prompt并排ui_prompt_start；finish递减，仍>0就不发end，最后归0清active并排对应end。同步run抛调用finish再抛；正常Promise用finally(finish)覆盖成功/拒绝。并发而非真正嵌套的UI调用也共用计数和最早active prompt，不是每个并发窗口独立事件。空title不入事件。emitUIPromptEvent使用queueMicrotask后void this.emit，start/end通知异步排入、不阻塞UI；emit因errorListener而reject时无此处catch。没有取消UI、超时、promise强制终止或invalidate后清depth的逻辑。

## 工具、flags、快捷键与renderers（491–650）

getExtensionPaths生成path新数组。getAllRegisteredTools按tool.definition.name Map保留首次，返回新数组但tool对象共享；getToolDefinition则按ext.tools.get(toolName)查首项，正常注册key等于definition.name，运行时被外部构造不一致时两种查法可能不等价。这里不验证parameters schema，不报同名error、不卸载loser；S3断言同名工具first。别处loader/resource-loader负责注册合法性和collision diagnostic，不能把test标题的schema拒绝自动算此方法实现。

getFlags按Map key首个获胜，返回新Map但flag对象共享。setFlagValue任意name与boolean|string直接写runtime.flagValues，无“必须已注册”、default/type校验；getFlagValues返回新Map，值为原primitive。flags与tools first-wins并不适用于commands/shortcuts。

getShortcuts每次清shortcutDiagnostics并重建builtin map，按extension顺序lowercase键。reserved builtin冲突warning并skip；nonreserved builtin冲突warning但接受extension；与已有extension同键warning后覆盖，即后者胜。addDiagnostic把warning/path入数组，无UI时还console.warn，有UI时只留diagnostic；不自己弹UI。getShortcutDiagnostics返回shared数组，下一次getShortcuts会替换它而非append历史；多次查询可再次warn。S2覆盖rebound reserved和reserved/nonreserved同键。

getMessageRenderer按customType找首truthy；getEntryRenderer也首truthy，允许entryRenderers缺省；getMarkdownTransformers按extension顺序收truthy单transformer，不执行、不合成、无collision检查。getModelRegistry直接返引用。查询结果都不是deep readonly或带revocation代理。

## 撤销、错误listener、commands与context（595–842）

invalidate默认陈旧ctx说明；仅!staleMessage时先保存message，再runtime.invalidate(message)。默认非空字符串使首次撤销不可逆，不覆盖第一次原因；本文件无generation计数或session nonce。如果runtime.invalidate抛，runner已stale且下次默认invalidate不重试依赖清理。判据是真值而非专门boolean，显式空message不会使assertActive判真，不能称所有输入都强制stale。

assertActive仅staleMessage真时throw Error。它用于getActiveTools和ctx getter/动作，不普遍在emit、getTools/flags/shortcuts/commands/renderers、getUIContext、bindCore、bindCommandContext、setUIContext或直接shutdown前调用。invalidate不会清extensions/handlers/errorListeners、不停止已经await中的handler，不自动触发session_shutdown；一个不读取ctx的handler仍可能被直接emit执行。stale ctx getter拿到的原始sessionManager/modelRegistry/UI引用此前若已保存，本文件不再代理这些对象的每次调用。

C1有限loader片段显示runtime另存staleMessage并逐个unsubscribe eventBus再clear集合；其assertActive也按truthy。该片段解释runner为什么调用runtime.invalidate，但不证明所有pi API/其它资源都撤销。C4 AgentSession.reload先发shutdown、invalidate旧runner，再reload settings/resources/buildRuntime；失败没有本层恢复旧runner，因此target candidate保旧lease是差异。

onError Set去重，返回delete闭包。emitError顺序调用listener无catch，无listener则错误对象没有此层默认console/durable输出。多数dispatch的handler catch会调用emitError；listener抛可中断后续handler并使整个emit reject，故“所有handler错误都完全隔离”不成立。某些validation路径在try内emitError，listener抛还会被同catch再次作为handler错误emitError，不能承诺一次错误恰好一次listener通知。

hasHandlers扫描ext.handlers.get(eventType)非空，接受任意字符串，无活动guard。shutdown直接调用绑定shutdownHandler，无guard、无自动invalidate或dispose；ctx.shutdown有assertActive，是不同入口。

resolveRegisteredCommands先按extension/Map值顺序收全部commands并按command.name计数，再第二遍记录每个name occurrence和已占invocationName。同name数>1默认name:1/name:2，唯一name直接原name；若候选invocation已被较早命令占用，从occurrence+1继续递增为`command.name:suffix`直到空位。返回shallow复制command加invocationName，原name/handler/sourceInfo仍保留。不预留未来literal names，因此别名由完整顺序决定。例如literal x:1先于两个x时得到x:1、x:2、x:3；两个x先于literal x:1时后者可变x:1:2。每次查询重新计算，没有持久alias/generation；注册表改变可使旧alias失效。

getRegisteredCommands清commandDiagnostics后返回解析结果；本文件从不往commandDiagnostics追加冲突，getCommandDiagnostics返回内部数组。getCommand仅按invocationName精确找，不按原name fallback；两个x时裸x不存在，不删除任一扩展。S4断言两同名shared-cmd:1/:2及空diagnostics。

getActiveTools先assertActive再runtime.getActiveTools。createContext本身不立即assertActive，返回对象每个getter/动作调用才检查。ui/mode/hasUI/cwd/sessionManager/modelRegistry/thinkingLevel以及isIdle/isProjectTrusted/signal/abort/hasPendingMessages/shutdown/getContextUsage/compact/getSystemPrompt都经runner当前字段读取；model与scopedModels特别先把this.getModel/getScopedModels函数捕获到局部变量，创建后重新bindCore换“函数对象”时旧ctx仍调用旧函数，虽旧函数内部可动态读当前state。注释call-time动态并不代表所有重新绑定都反映到旧ctx。底层对象原样返回，无深copy。

createCommandContext用Object.getOwnPropertyDescriptors(createContext())复制到{}，保留lazy guarded getters，避免spread提前取值；再添加getSystemPromptOptions/waitForIdle/newSession/fork/navigateTree/switchSession/reload，每个先assertActive再转发当前绑定handler和参数。没有自动waitForIdle、先invalidate或session替换，withSession/setup都原样给host；handler返回Promise后runner不再检查是否被撤销，也不会因新session而修改旧ctx。未绑定动作返回cancelled:false并无动作，需由host正确配置。

## Generic emit和message/tool事件（843–1032）

isSessionBeforeEvent只判switch/fork/compact/tree。emit创建一个ctx供本次所有handler共享，遍历各handlers，await handler(event,ctx)；四种before事件对truthy返回整体赋result，cancel真立即短路，其他继续，最终返回最后一个truthy结果。它不按字段合并：先{summary:...}后{label:...}会丢前summary；undefined不抹掉先前result，{}truthy可以覆盖。普通事件忽略返回值。handler异常转ExtensionError后继续（error listener例外）；同一个event对象共享且未clone，原地mutation可被后续看到，抛错不会回滚已改event或外部副作用。

emitMessageEnd保存event.message为currentMessage，逐handler创建shallow事件副本，message字段指当前消息。无truthy result.message跳过；返回role与当前role不同则emitError并忽略替换，role相同则整体替换并modified=true。最后仅有合法返回替换才返currentMessage，否则undefined。没有content/schema校验、deep clone或字段patch merge；handler直接改message后返回undefined仍可能通过共享引用改变原输入，却不设modified；抛错前原地改动也未回滚。内容null规范化和将替换对象写回Agent state/session属于AgentSession owner，不在runner本身。

emitToolResult先shallow copy原event，所有handler共享currentEvent。truthy返回的content/details/isError/usage中非undefined字段逐项覆盖并modified=true；false isError、空content数组可生效，undefined不能清字段，未typed的null不被这里拒绝。后handler只改isError会保留前content/details/usage；无任何显式字段覆盖返undefined，否则返回这四项当前完整值。不合并nested details，不改变toolName/id/input的返回合同；直接原地nested mutation可泄漏给原event且modified仍false。S6证实partial patch保留先前修改；C3调用方之后统一resize images。

emitToolCall是关键例外：所有handler await没有try/catch，不走本地emitError；任意异常直接reject，后handler不跑。truthy返回整体替换result，block真立即返回，未block继续，最后truthy结果返出。所有handler收到同一原event，修改参数按C2协议原地修改event.input，不是返回args patch；本文件不重验schema/path或实现terminate整批条件，只传回result。C2定义terminate是工具batch终止提示，具体如何满足“所有finalized结果”归Agent owner；C3把异常继续向agent抛以阻止执行。

emitUserBash串行找第一个truthy返回，立即结束；错误emitError后继续；全部无结果undefined。operations/result两条含义由C2类型指明，但runner不执行bash、不验证二者冲突、不撤销已经开始的handler副作用。

## Context、provider payload与headers（1033–1130）

emitContext先structuredClone(messages)，此clone在handler try外，DataCloneError可直接reject。每handler新建{type:context,messages:currentMessages}；truthy result.messages替换当前数组（[]也truthy），没有结果可通过原地修改currentMessages生效；错误记录后保留此前数组/已改内容继续。初始输入深clone隔离，但之后handler返回的外部数组不再clone，无法宣称每个extension隔离快照或持久history防护；此方法只返回上下文数组，写入哪个request由调用者决定。

emitBeforeProviderRequest对payload不clone，用currentPayload；每handler新event包当前payload，返回只要!==undefined就替换，包含null/false/0/空串；undefined保持，原地改对象可生效。异常emitError继续但已改对象不回滚；最终可能返回任意unknown，schema/密钥/网络地址策略不在本方法实现。

emitBeforeProviderHeaders完全采用调用者headers原对象，每handler收到指向同对象的event.headers，忽略返回值；await后继续，即使handler抛之前修改了headers也保留。赋event.headers为另一个对象与修改原headers属性不同，前者不会重绑方法内headers变量；本层不删除null、不合并大小写、不过滤认证header。S7断言加X-Turn-Index保User-Agent，throwing handler之后good继续；不等于header安全校验通过。输出是相同对象身份。

## before-agent、资源和input（1131–1286）

emitBeforeAgentStart复制ctx descriptors，重写ctx.getSystemPrompt为guard后读currentSystemPrompt，确保后handler在event.systemPrompt与ctx getter看到同一个链式值。prompt/images/systemPromptOptions仍按调用者值共享，无deep clone。每truthy result.message累积一项；result.systemPrompt!==undefined则替换并标modified，显式空串有效。错误emitError后继续。只有messages非空或systemPrompt曾显式修改才返combined，否则undefined；最终字段各自缺省undefined。一个handler先push message、后读取result.systemPrompt getter抛，也没有撤销已push项的事务。S5断言base→first→second且ctx getter同步。这里不将message持久化，不做nullcontent规范化。

emitResourcesDiscover每handler新建event(cwd,reason)，返回三类skill/prompt/theme paths分别map为{path,extensionPath:ext.path}并append，保持load/handler/path顺序。没有去重、canonical化、存在性检查、scope/trust过滤或扩展module paths。无handler也返回三空数组。catch继续且已成功追加的早一类不会因后一类map失败撤回；路径进一步metadata/加载归AgentSession+ResourceLoader，不是runner读取skills本体。

emitInput构造当前text/images加固定source/streamingBehavior；handled返回该result并短路全部后续；transform更新currentText，images用result.images??currentImages，因此undefined/null不清旧图但[]清图。其它结果继续。异常emitError并保留之前转换；只在末尾用text值不等或images引用不等决定返回transform，否则continue。transform后又改回原值/引用最终可continue；原images数组原地mutation但引用没变也可continue，却已修改调用者数据。没有自动重跑command dispatch、技能/template展开、图片规范化、输入大小或source可信性验证。

## 错误、取消、状态及重启总边界

本文件无文件写入/恢复journal、没有进程创建/HTTP调用和定时重试循环，但handler/provider/UI callback可做任意外部副作用。除了emitContext初始clone，多数输入共享或浅复制；catch只改变控制流并汇报错误，不回滚message/header/input/runtime/provider注册。error listener未经保护是各隔离分支共同的逃逸口，emitToolCall则有意直接传播。promise永不完成的handler会阻塞本次串行分派；ctx.signal允许合作式取消，但无每次handler前后检查或强制abort/timeout。

invalidate用于ctx/runtime的陈旧引用保护，不等于dispatch循环终止或全API撤销，且无数字generation；target lease identity为独立设计。session绑定只保存初始sessionManager/cwd引用及可替换actions；本类没有setSessionManager、new-session文件操作、自动epoch迁移或durable alias恢复。重启自然重新构建runner，其原始内存flags/diagnostics/bindings是否保存由host决定；本源无重启保存合同。async并发emit/UI调用共享runtime/字段，不存在统一mutex或不可变snapshot。

## 可运行行为判据与证据身份

本轮Node/Cargo/PTY/HTTP/网络及源/目标产品测试执行数均为0。S1–S7是已读源测试断言，不是历史成功回执或本轮通过；T7明确echo_agent用于lease场景，不作为G-HOST生产证据；T8含本地HTTP/进程fixture但本轮未运行。未读测试其它分支不计覆盖。

| 合同组 | 固定操作与可核对期望 | 证据性质 |
|---|---|---|
| trust | undecided、no、yes按序，返回no且yes不调用；undefined handler产生error再继续；{}非undecided可提前返回 | S1正常断言，其余源分支推导 |
| core绑定 | queued有效provider A、抛错B、有效C，error listener不抛时A/C注册、B记录，数组清空；listener抛则后续可中断且已注册项不回滚 | 源bindCore，未执行 |
| UI | 无UI confirm=false，mode=print/hasUI=false；两重叠prompt只最外一对start/end，reject也配对finish；microtask通知不代表同步listener完成 | 源noOp/UI计数判据 |
| 工具/flag | 两同名工具/flag保首对象；setFlagValue未声明name也写Map；返回flagValues Map改动不改原Map | S3工具断言+源getter分支 |
| shortcut | app.interrupt绑定ctrl+x则extension该键skip；同键reserved+非reserved仍skip；普通builtin警告但extension胜，两extension同键后胜 | S2 + 源循环 |
| command alias | 两shared-cmd→:1/:2，diagnostics=[]；裸shared-cmd查不到；literal x:1与重复x改变插入序可触发不同后缀 | S4 + 精确算法推导 |
| stale ctx | 先创建ctx再invalidate默认消息，所有guarded getter/action抛；直接emit里不读ctx的handler仍可运行；已保存UI引用不自动代理 | 源guard位置+ C1有限依赖，不声称全runtime验收 |
| ctx重新绑定 | createContext后bindCore换getModel函数，旧ctx.model仍调用创建时捕获函数；isIdle读新的runner.isIdleFn | 源闭包捕获判据 |
| generic before | H1返summary、H2返label，最终只H2对象；H2 cancel=true短路；普通event返回值丢弃；error listener抛可reject | 源emit分支 |
| message_end | 同role替换链有效，异roleerror后保当前message；直接改对象再undefined不设modified但共享mutation存在 | 源方法判据；不等价于AgentSession已持久化 |
| tool参数/结果 | H1 mutate input，H2见改值；H2 block短路，throw直接reject；result H1 content/details、H2 isError保所有已修改字段 | S6/C2/C3 + 源tool_call规则 |
| context/payload/headers | context初始clone保护原数组；provider payload返null有效；headers返新对象无效、原地加属性有效且对象身份同一；抛前改header不回滚 | S7正常链+源差异 |
| before_agent_start | H1 ctx.prompt加first、H2加second，event与ctx均看到最新值；多个message按序聚合 | S5 |
| resources/input | 三类路径重复原样保留并带ext.path；input transform链再handled短路；images=[]清空，undefined保留，改回原值最终continue | 源循环，可用手造Extension.handlers执行，未运行 |
| target撤销/隔离 | failed/cancel配置旧lease仍active；成功新identity、旧inactive；first deny后后extension before_tool不执行 | T7/T8只读断言，非本轮行为通过 |

## 当前zenpi映射与反向范围

精确引用表附后，完整行/半开字节/文件hash/片段hash在evidence/context-references.json。target仅T1–T8选定片段，source依赖仅C1–C4，不读写022报告、不给目标或source依赖整文件接受。

| 本源责任 | 当前目标片段/行为 | 差异、缺口或排除 |
|---|---|---|
| handler顺序与输入 | extension_runtime T2 chain按catalog.loaded.values筛hook串行process_request、序列化HookResult；input只允许Continue/Transform text并check_text | source按extension数组及每event多个handler；target依赖catalog的Map顺序，每extension协议reply非同样TS handler列表，且没有source handled/images入口 |
| context | T2 context只接受request-local instructions文本 | source structuredClone整个AgentMessage数组并可替换，target明确不把canonical/native/tool历史交插件当可变权威，是有意能力收窄，不是全等context ABI |
| tool_call | T2 before_tool允许Continue/Rewrite对象arguments/Deny，保call id/name；Deny立即Err；T8后hook不执行 | source输入原地mutation+block/terminate结果，targettyped rewrite+拒绝语义不同；目标schema/审批/路径复验属于其它owner，此片段不包揽 |
| tool_result | T2 after_tool接受Continue/Output JSON，最终redact_json | source content/details/isError/usage逐字段patch和image内容链不同；target不以此片段证明usage patch、same-role message_end替换或图像归一全套 |
| 生命周期错误 | T3 notify_with_lease每extension捕获process/protocol错误入Vec继续；start一次、close一次；T4 extension_errors转脱敏AgentEvent | source emitError listener可抛影响隔离；target以Vec汇报避免同样listener链，但全部host副作用与事件消费不在本项整读 |
| 活动/撤销 | T1 LeaseIdentity含session_id/generation/capability，active共享AtomicBool；T2 chain前后check和cancel；T3 close先revoke旧lease，用独立active但同identity临时closing lease通知，再revoke；Drop尽力close并revoke | source stale字符串守ctx/runtime，不是generation counter，直接emit无统一check。target process_request内具体进程回收未在本项整读，不宣称仅AtomicBool就已回收全部child |
| timeout/预算 | T2/T3链使用单handler timeout和CHAIN_TIMEOUT取消predicate，serialized event超MAX_HOOK_BYTES错误 | 源runner无时间/字节配额；目标值定义与完整IPC边界在未读其他行，仅确认这些检查调用，不捏造具体数值 |
| tool registry | extensions T5 clone registry后全部注册成功才赋值；API2要求session runtime；core T4 candidate准备、剔旧tools并注册 | source查询first tool名胜，不在此做整批registry transaction；目标注册失败行为由ToolRegistry决定，不把source first-wins当target冲突合同 |
| reload和session | core T4 Idle/host工具/非worker权限条件，prepare候选、cancel复查、append选择记录，再publish close旧/start新；T7失败保旧lease | source runner不自己reload，C4 host先invalidate再重建。candidate保旧是目标增强，未读取所有session切换/fork实现，不冒充完整session owner验收 |
| skills/resources | skills T6模型调用禁用/参数上限/hash变化拒绝/取消；source emitResourcesDiscover只聚合path并归ext.path | 技能正文读取不是runner执行责任，targetskills只是边界对照；无据证明目标扩展可动态贡献所有source三类path与sourceInfo/baseDir规则 |
| flags/commands/shortcut/UI/renderers | 源first flags、同名command别名、后胜shortcut/保留action、UI prompt深度、renderer查询均完整说明 | T1 HookKind仅列Input/Context/BeforeTool/AfterTool/SessionStart/AgentStart/AgentEnd/SessionClose，选定target片段未证明source command/flag/UI/render注册ABI；明确缺口而不全库不存在断言 |
| provider与trust | 源queued/immediate provider registry、project_trust first decision、任意payload/in-place headers | target所读hook类型/协议未提供直接同等能力；模型鉴权/header/provider trust各owner应另验。未以通用Input/Context替代它们 |

反向核对：src/extensions.rs安装/升级/删除/manifest全发现、src/skills.rs发现解析与工具访问、extension_runtime.rs prepare/IPC/process sandbox全部分支、src/core.rs provider loop/审批/资源turn admission等均不属于本项target整文件范围。源file所有独立UI/renderer/flags/commands/context/provider/trust/resource/input分支均已说明与判据闭合，没有只核对tool hooks而跳过其它功能。C2类型摘录仅支持返回合同，本轮既不产types.ts标准报告也不触碰A的候选；目录052继续pending。

## 门禁与可撤回产物

evidence/inputs.json记录冻结源与当前authority/manifest/index/checker文件hash；gfile.json记录六段连续字节及段hash、报告身份与有界context校验。结构checker不能证明语义，G-FILE必须master逐文件复查；缺master receipt仍provisional。gstage.run.json含实际argv/cwd/起止时间/exit及stdout/stderr bytes/hash；失败不会改写成通过。

本轮产品运行数0，未跑Node/Cargo/PTY/HTTP/网络，也未创建任务/subagent或写主库/产品/状态/旧ready。candidate.patch只新增唯一标准报告；source/context/scripts是私有ready证据，不进产品diff。回滚只撤回新增报告；临时git正反check/apply/hash/sentinel验证不应用到master。先前018/021/028/125与其它旧ready均保持，不从其接受结论继承本项语义成功。

### 本轮实际结构门禁与定位表

G-STAGE 实际 exit 1，structural.ok=true，唯一错误是主库缺少 `Docs/learn/stage1_pi_mono/receipts/ZS1-023.master.json`。没有创建该receipt。G-FILE 本地仅检查source hash、完整范围、report身份和context片段hash；semantic_verified_by_checker=false，master语义复核仍pending。产品行为执行数为0。

以下行/字节为context原文件的一基闭合行范围、零基半开字节范围；片段SHA和source文件SHA完整记录于evidence/context-references.json。这里的定位表不扩大正式复核范围。

| ID | 上下文文件 | 行 | 字节 | 文件SHA256 |
|---|---|---|---|---|
| T1 | /Users/wangweiyang/GitHub/zenpi/src/extension_runtime.rs | 35–90 | [998,2542) | `002169d07e9d41a3f4f2526c5bbc3b2ecb63e4f6a648e37adefb7f98edf727aa` |
| T2 | /Users/wangweiyang/GitHub/zenpi/src/extension_runtime.rs | 203–344 | [6223,11250) | `002169d07e9d41a3f4f2526c5bbc3b2ecb63e4f6a648e37adefb7f98edf727aa` |
| T3 | /Users/wangweiyang/GitHub/zenpi/src/extension_runtime.rs | 345–426 | [11250,14079) | `002169d07e9d41a3f4f2526c5bbc3b2ecb63e4f6a648e37adefb7f98edf727aa` |
| T4 | /Users/wangweiyang/GitHub/zenpi/src/core.rs | 2104–2210 | [82108,85867) | `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869` |
| T5 | /Users/wangweiyang/GitHub/zenpi/src/extensions.rs | 269–311 | [8993,10537) | `38a242eff3ec911f4560e3dea3868b742d511205ff0ccab1b622962b41e420ed` |
| T6 | /Users/wangweiyang/GitHub/zenpi/src/skills.rs | 281–317 | [9325,10744) | `a6d0a2f671febdef2e631ca53d2788ab869ab7b86db6c0cb1feaf24d128f83e8` |
| T7 | /Users/wangweiyang/GitHub/zenpi/tests/stage1_extension_hooks.rs | 515–542 | [18515,19626) | `aa7ed6e8fefe1ec513fcbf0753f17e082b103e381b0eb8c653a6b4cd126d0453` |
| T8 | /Users/wangweiyang/GitHub/zenpi/tests/stage1_extension_hooks.rs | 936–953 | [33442,34121) | `aa7ed6e8fefe1ec513fcbf0753f17e082b103e381b0eb8c653a6b4cd126d0453` |
| C1 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/src/core/extensions/loader.ts | 178–235 | [7230,9691) | `c96284a217cd4e2eee8d7335a7c25b106135968c716ce39834b4714f5ff8f771` |
| C2 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/src/core/extensions/types.ts | 1125–1188 | [37636,39517) | `96e20f8038027f0b0172f311b6b9d9ddf42123b2f5b2a018f533af026fd3371e` |
| C3 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/src/core/agent-session.ts | 482–536 | [17412,19010) | `17116255610ad2a3f3a7f6870a12b14b993ace65fbfa461c4a43d0ad9a013237` |
| C4 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/src/core/agent-session.ts | 2841–2868 | [97848,98897) | `17116255610ad2a3f3a7f6870a12b14b993ace65fbfa461c4a43d0ad9a013237` |
| S1 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/test/extensions-runner.test.ts | 122–158 | [3903,4986) | `83aacf2e5db37b3a14ee14c61c870f3211180a16a465f53e76aaf36f421fd5cf` |
| S2 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/test/extensions-runner.test.ts | 236–281 | [7930,9745) | `83aacf2e5db37b3a14ee14c61c870f3211180a16a465f53e76aaf36f421fd5cf` |
| S3 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/test/extensions-runner.test.ts | 419–454 | [14576,15796) | `83aacf2e5db37b3a14ee14c61c870f3211180a16a465f53e76aaf36f421fd5cf` |
| S4 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/test/extensions-runner.test.ts | 502–527 | [17484,18784) | `83aacf2e5db37b3a14ee14c61c870f3211180a16a465f53e76aaf36f421fd5cf` |
| S5 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/test/extensions-runner.test.ts | 763–804 | [27130,28502) | `83aacf2e5db37b3a14ee14c61c870f3211180a16a465f53e76aaf36f421fd5cf` |
| S6 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/test/extensions-runner.test.ts | 854–896 | [30097,31339) | `83aacf2e5db37b3a14ee14c61c870f3211180a16a465f53e76aaf36f421fd5cf` |
| S7 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/test/extensions-runner.test.ts | 1012–1060 | [35345,37246) | `83aacf2e5db37b3a14ee14c61c870f3211180a16a465f53e76aaf36f421fd5cf` |

## 主控独立复核补充 — 3.1.20

主控已完成本源1286行全文、报告全文和19个精确有界上下文的独立阅读；验收只覆盖ZS1-023源文件理解。上文worker provisional、零产品运行及当时G-STAGE缺master的记录作为历史证据保留。当前接受凭据与实际门禁结果见 `Docs/quality/stage1/ZS1-023/master-review-3.1.20/`；不因此接受目标扩展能力、整项115或目录052。

C1 loader片段的退订循环没有catch：一个unsubscribe抛异常会阻止后续调用和末尾clear；第一次非空stale已保存，再次invalidate不会重试清理。该事实进一步限定上文“逐个unsubscribe再clear”为正常路径，不能解释为所有资源一定清理成功。source generic before的cancel短路、ToolCall直接异常传播、emitError监听器逃逸、共享对象mutation无回滚、默认cancelled:false无真实会话动作等资格均继续有效。只读测试断言与实际运行证据保持区分。
