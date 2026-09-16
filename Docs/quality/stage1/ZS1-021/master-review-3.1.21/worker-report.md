# ZS1-021 — packages/coding-agent/src/core/resource-loader.ts

Worker candidate: provisional；learn_mode: understand。仅本源文件完整理解与有界目标映射，master 独立语义 G-FILE 尚未通过。

source_id: SRC-0931
source_path: packages/coding-agent/src/core/resource-loader.ts
source_hash: 1877e9535820cb8b45e5a84598ac8ae581058bb0fbee6f0c64ec672f8ab986dd
source_bytes: 40180
source_lines: 1097
coverage: 完整字节区间 [0,40180)，按 1–220、221–440、441–660、661–880、881–1097 连续全文阅读，包含声明、注释、构造器、所有私有辅助函数和文件结尾；未执行源代码。小于 256 KiB，不触发大文件强制分块。

唯一标准产物：`Docs/learn/stage1_pi_mono/files/packages/coding-agent/src/core/resource-loader.ts_learn.md`。捕获前搜索本 worker `.ops`/`Docs` 的 ZS1-021/resource-loader.ts 及主库 Docs/learn，没有找到既有完整 ready 或标准报告；标准路径在 master/worker 均 absent。因此新建本报告，不覆盖旧结论，不借其他项的接受状态。输入 authority、manifest/index、checker 精确副本与 hash 在 evidence/inputs.json；目标仅使用 evidence/context-references.json 指定的行、字节和 hash 片段，无目标整文件接受声明。报告沿用 G-FILE 的身份、全文语义、行为判据、目标映射和证据限界格式。

## 职责、类型和状态所有权（1–53、159–303）

该文件是资源编排器：通过 SettingsManager/DefaultPackageManager 获得资源路径，再分别加载 executable extensions、skills、prompt templates、themes、context files、system/append prompt。它不实现技能 Markdown/YAML 语法、模板参数展开、扩展工具实际执行、包安装协议、模型调用或 session 持久化。导入的路径/Git/BOM/source-info/timing/theme/extension 工具各有独立 owner；这里说明其调用条件和结果使用，不把依赖模块算作已整文件复核。

ResourceExtensionPaths 只有 skillPaths/promptPaths/themePaths 三组 `{path, metadata}`；名字中的 Extension 表示扩展贡献资源，不允许用它动态添加新的 extension module。ResourceLoaderReloadOptions 仅有可选异步 resolveProjectTrust 回调，参数为预加载的 extensionsResult、结果 boolean，无 AbortSignal。ResourceLoader 接口有九个资源 getter、extendResources 和 reload；类的公开 loadProjectTrustExtensions 不在该接口中。ResourceCollision/ResourceDiagnostic 是外部声明的 re-export，本文件不另定义其 schema。

DefaultResourceLoaderOptions 必须给 cwd、agentDir；可注入 settingsManager/eventBus，可添加四类路径、inline extensionFactories、五种 no* 开关、显式 systemPrompt/appendSystemPrompt，以及 extensions/skills/prompts/themes/agentsFiles/systemPrompt/appendSystemPrompt 七类 override。override 同步执行，可替换/删除/重排结果，也可抛异常；不是验证器或隔离沙箱。类型系统约束不能当作运行时数据验证。

构造器 resolve cwd/agentDir，缺省创建 settings/eventBus/packageManager；保留选项路径数组和工厂数组引用，未冻结或复制。结果初始为空 arrays/diagnostics，extensionsResult 有新 runtime，systemPrompt/sourcePath 为 undefined，last*Paths、三个 extension source-info Map、metadata Map 为空，loaded=false。构造器没有调用 reload；创建默认 SettingsManager 的具体 I/O 不归本文件保证为纯操作。

getExtensions 直接返回内部结果对象；getSkills/getPrompts/getThemes/getAgentsFiles 仅新建外层 wrapper，内部数组/对象共享。getSystemPrompt 是字符串/undefined；getAppendSystemPrompt 返回共享数组；两个 source getter 构造新 `{path}` 对象/数组。没有版本号、不可变 generation、锁、深复制或并发调用互斥。调用者持有旧引用与后续字段替换、对象原地 sourceInfo 修改的关系必须分别理解。

## 文本输入与上下文发现（54–158）

resolvePromptInput：undefined 或空字符串直接返回 undefined；存在的 input 路径 readFileSync UTF-8 并 stripBom；读失败输出黄色 console.error 后返回原 input 字符串；不存在路径也按字面文本返回。这里不先做 cwd/tilde/file URL 规范化，不检查必须普通文件、不设读取字节上限。因而存在但不可读的路径并非使整个 reload 必然失败，也不能保证返回的是文件内容。

loadContextFileFromDir 按 AGENTS.override.md、AGENTS.md、AGENTS.MD、CLAUDE.md、CLAUDE.MD 顺序尝试；跳过不存在和非普通文件；stat/read 错误警告后尝试下一个。每目录只取第一个成功项，stripBom；都失败返回 null。不同大小写在具体文件系统可能指同一文件，本文件没有另外大小写归一化。

findShadowedContextFile 先 findGitPaths，再 canonicalize commonGitDir/repoDir，取 dirname(commonGitDir) 为主库候选；只在 worktree root 真正位于该主库内、且主库 `.git` canonical 路径等于 commonGitDir 时处理。worktree root 自己找到 context 后，返回主库下同 basename 的候选，避免 nested worktree 与祖先主库的同名逻辑副本重复。不比较内容 hash，不把不同文件名当同一副本，不清除所有祖先；普通 repo、sibling worktree、bare 容器、submodule 不满足该结构时不屏蔽。返回值是 canonical 主库目录 join basename；不能将注释推成对任意 context 文件 symlink 的完整规范化保证。

loadProjectContextFiles 先 resolve cwd/agentDir、加载 global context 并登记原路径，再从 cwd 一直向 filesystem root 走，每目录一项，用 unshift 形成根→叶顺序，最后接在 global 后。seenPaths 按返回路径字符串去重；shadow 比较才 canonicalize context path。不会在 git root 停止，不用 projectTrusted 过滤 context（与项目 SYSTEM/extensions 等不同）。读取 ancestor 文件即使随后判为 shadowed 也已经发生。没有目录深度/总内容大小/取消限制；不写 context 文件。错误容忍主要在目录候选读取，导入 Git/path 工具异常是否兜底属于依赖模块。

## 增量扩展资源和 reload 顺序（304–548）

extendResources 先 normalizeExtensionPaths 三类条目：path 与可选 metadata.baseDir 用 resolveResourcePath，baseDir 存在时 shallow copy metadata。随后把每类 createSourceInfo 写入对应 Map，同一路径后写覆盖；三组 Map 先写完，再按 skill→prompt→theme 分别 merge(lastPaths, 新 paths)、更新 lastPaths、重载该类全部资源。空条目组不触发该类重载。旧路径先于新路径，不等于新贡献同名资源必胜；无清除 API、不加载新 extension、不刷新 context/system。它可在首次 reload 前使用；override/归属 stat 抛错时 Maps、lastPaths 或更早类别可能已经变化，无全组回滚。下一次完整 reload 清空这些贡献 source-info Maps，并重建 lastPaths，因此之前通过 extendResources 添加的路径须由外部生命周期再次贡献，不能假称自动持久化。

loadProjectTrustExtensions 明确 setProjectTrusted(false)，await settings.reload，再 loadCurrentExtensionSet(includeInlineFactories=true)。它返回预加载结果，不直接赋 this.extensionsResult；自身也不恢复原信任状态。回调执行前用户/允许的资源及 inline factories 已可能执行，不能称为没有执行代码的扫描。

reload 的精确阶段如下：

1. resetTimings("extensions")；仅 loaded=true 时 clearExtensionCache。若上一次首次 reload 在尾部失败，loaded 仍 false，但前面加载过模块，下一次本入口不会因为该失败自动清缓存。
2. 若提供 resolveProjectTrust，先执行上述不可信预加载，await 回调，再把返回信任写入 settings。随后无论是否回调，再 await settings.reload（注释说明保留 trust），await packageManager.resolve，await resolveExtensionSources(additionalExtensionPaths,{temporary:true})。
3. 至此才替换 resourceMetadataByPath，并清空三组扩展贡献 source-info。局部 getEnabledResources 对所有 ResolvedResource 首次路径保存 metadata，即使 disabled 也保存，然后筛 enabled；getEnabledPaths 再取 path。元数据“first path wins”与是否加载不是同一规则。
4. 收集常规四类资源，其中 skills 经过 mapSkillPath；给 CLI extension/skill 结果路径在未登记时预写 `{source:"cli",scope:"temporary",origin:"top-level"}`。CLI prompts/themes 没有同样这两段强制标记，随后使用其资源 metadata；重复路径不会覆盖既有记录。
5. extensionPaths 在 noExtensions 时仍取 CLI enabled extensions，否则 merge(CLI,常规)。await loadFinalExtensionSet 后，对 additionalExtensionPaths 中 isLocalPath 且不存在者追加 error；赋 extensionsOverride 后结果，再 applyExtensionSourceInfo。
6. skills：noSkills 时 merge(CLI skills,additionalSkillPaths)，否则 merge(CLI skills + enabled mapped skills,additionalSkillPaths)。先保存 lastSkillPaths，再 updateSkillsFromPaths。对额外 local 缺失路径且尚无相同 path diagnostic 才补 error。
7. prompts 同 skills 的路径构造，但使用 additionalPromptTemplatePaths/noPromptTemplates/updatePromptsFromPaths。缺失 local 额外路径补相应 error。
8. themes 同样构造并更新，额外主题路径不先用 isLocalPath 筛选。先前 loadThemes 已有同 path warning 时，不再补 error；“额外缺失 theme 一律 error”不成立。
9. 根据 noContextFiles 决定 [] 或 loadProjectContextFiles，再 agentsFilesOverride 后赋值。这个开关不阻止 override 注入 context。
10. systemPromptSource 使用显式 `??` 自动发现：显式空字符串抑制自动发现但解析为空；resolvePromptInput 后 systemPromptOverride 再赋值。source path 独立按原输入 existsSync 判断并 resolvePath；不因 override 返回文本而取消来源，也不代表读取一定成功。
11. appendSources 用 falsy 判断：undefined 自动发现最多一个文件，显式 [] 为 truthy 因而禁用自动发现；按输入顺序解析，过滤 undefined，appendSystemPromptOverride 后赋数组。sourcePaths 独立只保留存在的原输入并规范化，不按 override 内容回推、不去重，也不保证与最终文本逐项对应。最后 loaded=true。

这些 await 和连续赋值之间没有 candidate snapshot/commit 或 catch rollback。包解析/信任回调失败时可能只改过 settings/cache/runtime；扩展成功而 skill/prompt/sourceInfo/override 失败时可留下新 extensions 和旧后续资源；lastPaths 甚至可先于结果改变。loaded=true 只表示至少一次完整执行到末尾，不证明当前字段属于同一批。reload/extendResources 无取消参数、timeout 或并发 guard；并发 reload 可以交错，外部停止等待不撤销 I/O 或 extension side effects。这里没有任何 session journal、原子 rename 或重启恢复。

## 扩展预加载复用与诊断（549–635、946–969、1060–1097）

loadCurrentExtensionSet 重新做两次包路径解析、enabled 过滤和 CLI-first/noExtensions 路径选择，再 loadExtensionsCached。includeInlineFactories=false 原样返回；true 则串行加载 inline，把成功及错误分别 append。该辅助入口不执行冲突诊断和 sourceInfo 归属，不能把预加载结果当最终结果。

resolveExtensionLoadPath 用 cwd 及 normalizeUnicodeSpaces:true；resolveResourcePath 用 cwd 及 trim:true。两条规范化调用选项不相同，不能声称所有路径都采用同一 canonical key。

loadFinalExtensionSet 无 preTrust 时正常 cached load、附加 inline、addExtensionConflictDiagnostics。有 preTrust 时：排除 path 以 `<inline:` 开始的项，按 resolvedPath 建预加载 Map；错误路径经 resolveExtensionLoadPath 入 failed Set；只加载最终列表中既非成功也非失败 preload 的剩余路径，传入同一个 preTrust runtime。remaining 成功覆盖同 key，按最终 extensionPaths 重排能找到的成功模块，再 append 原预加载 inline；返回全部 preTrust errors + remaining errors 和旧 runtime。预加载失败本次不重试；预加载成功但最终未列入者不会放进最终数组，但已发生的副作用没有撤销；预加载错误也不按最终路径过滤。inline 复用而非再执行。路径字串/解析结果一致性依赖 extension loader，本文件没有额外realpath归一去重 Map。

loadExtensionFactories 对每个输入顺序 await loadExtensionFromFactory，共享 cwd/eventBus/runtime。函数输入用 `<inline:1>` 等一基序号；命名输入用 `<inline:name>`，成功时 extension.hidden=isNamed && input.hidden（可为 false/undefined），失败捕获 Error.message 或固定字符串并继续下一项。没有并行、超时、取消、释放 hook；工厂抛错前产生的副作用不回滚。

addExtensionConflictDiagnostics 把 detectExtensionConflicts 的 `{path,message}` 转成结果 errors，不卸载任何扩展。detectExtensionConflicts 只遍历 tools.keys 和 flags.keys，两张 owner Map 记录首个 ext.path；同名且不同 path 时给后者报冲突，不覆盖首 owner；同 path 重复不报跨扩展冲突。实际未遍历 commands，尽管上层注释说 tools/commands/flags。本文件仅报告 load-order 冲突，最终工具选择和同名 command 的调用名归 ExtensionRunner；S2/S5 测试片段期望 deploy:1/deploy:2 共存、CLI tool 获胜，但不是该函数直接构造这些别名。

## 分类加载、路径、sourceInfo（636–864）

mapSkillPath 只对 metadata.source==auto 或 metadata.origin==package 的目录尝试精确 SKILL.md；stat 失败/非目录返回原路径，SKILL.md exists 即映射并首次复制 metadata，不先检查该文件是否普通文件。非 auto 且非 package 直接保留，不对 CLI skills 统一调用它。这样 auto/package 技能目录可只读入口，显式目录仍留给技能 loader 的发现语法；不是本文件实现 YAML 或禁用模型调用策略。

updateSkillsFromPaths 在 noSkills && paths为空时先给空结果，否则调用 loadSkills(includeDefaults:false)。该开关关闭默认发现、并不禁显式输入。skillsOverride 后 shallow copy 每个 skill，sourceInfo 顺序是扩展额外归属/包 metadata 查找 → skill 原 sourceInfo → 默认归属；再替换 diagnostics。本文件不自己实现技能同名裁决，须区分路径去重和 loadSkills 的名称冲突合同。

updatePromptsFromPaths 同理 noPromptTemplates + 空路径特殊分支；正常 loadPromptTemplates(includeDefaults:false)，接着本文件 dedupePrompts。override 发生在去重之后，可重新引入重复。逐 prompt shallow copy 补 sourceInfo，diagnostics 原样接收 override 结果。模板文本不作为可执行 extension，也不解释为技能。

updateThemesFromPaths 的正常分支 loadThemes(paths,false)、dedupeThemes，合并读 warning 与 collision diagnostics，再 themesOverride；主题对象的 sourceInfo 是原地修改，sourcePath 缺失时保留原值。三种 updater 都可能在 override/sourceInfo 抛出时留下先前阶段的修改；没有统一错误封装。

applyExtensionSourceInfo 用 extension.path（不直接用 resolvedPath）找 metadata 或默认归属，并把同一 sourceInfo 赋给每个 command/tool；flags 不在这段传播。它不保留 extension 原 sourceInfo 作为中间 fallback。inline 尖括号路径归临时来源。

findSourceInfoForPath 空路径 undefined；以 `<` 开头先走默认归属。普通路径用 path.resolve（不是统一 canonicalizePath）；extraSourceInfos 遍历插入顺序，匹配等于或带 sep 的祖先即返回 shallow copy 且 path 改为当前资源；再查 metadata 的 normalized exact/original exact，最后遍历 metadata 祖先。没有 longest-prefix 选择，宽祖先早插入可胜过晚插入更窄祖先；extra ancestor 也先于 metadata exact。完全无匹配返回 undefined。

getDefaultSourceInfoForPath 对完整 `<...>` 产生 source=内部冒号前首段或 temporary、scope=temporary/origin=top-level。其他路径按 agentDir 下 skills/prompts/themes/extensions 四根先匹配 user，再按 cwd/CONFIG_DIR_NAME 四根匹配 project，source=local/baseDir=对应根；否则 scope=temporary、baseDir 由 statSync 判断目录本身或父目录。最后 stat 没有 catch；override 虚构不存在的资源可令 reload/extendResources 抛错。只以 `<` 开头但不以 `>` 结尾的输入并不获得完整虚拟路径豁免。isUnderPath 规范 root 后做等于或带 sep 前缀，避免 skills2 被当 skills；它不校验 symlink containment。

mergePaths 按 primary 再 additional 遍历，各项 resolveResourcePath 后 canonicalizePath 做 seen 去重，保留第一次的 resolved 路径。返回列表不是 canonical 路径本身；顺序稳定但依赖上游枚举，不排序；相同实际路径不同写法只占首次位置。对于常规 package 返回的 project/user 优先次序，本文件不重新排序；S1 展示组合合同 project 胜 user，不能单凭 mergePaths 推出 package resolver 的所有排序分支。additionalExtensionPaths 的 CLI-first 和 additionalSkill/Prompt/ThemePaths 在尾部不是同一个优先策略。

## 主题文件、同名去重和系统文本发现（865–945、970–1059）

loadThemes(paths,includeDefaults=true) 可先读 agentDir/themes、cwd/CONFIG_DIR_NAME/themes 两个默认目录；当前 updateThemesFromPaths 明确传 false，不能把私有默认参数当本轮 reload 真实的 user-first 发现序。逐 path 规范化：不存在 warning；目录直接枚举；普通 .json 文件加载；其他类型/后缀 warning；stat/目录调用错误转 warning。不递归、不排序、不设文件数/大小上限，实际 JSON theme 解析和校验委托 loadThemeFromPath。

loadThemesFromDir 不存在直接返；readdir withFileTypes 遍历文件，symlink 用 stat 跟随，仅指向普通文件才可能加载；broken symlink 静默跳过；非文件/非 .json 跳过。目录读错误 warning；loadThemeFromFile 捕获解析/读取异常 warning 后继续其他文件。故 source 允许主题文件 symlink 的语义不能映射成 target 的统一拒绝策略。没有写主题文件或终端应用主题行为。

dedupePrompts 按 name 保留第一个，后者生成 collision resourceType=prompt/name/winnerPath/loserPath，消息带 /name；dedupeThemes 用 t.name ?? unnamed 为 key，同样 first-wins，缺 sourcePath 时诊断用 <builtin>。空 name 不等于 undefined，不会转 unnamed；三次重名分别指向首 winner。它们只管本类名字，不跨 skills/templates/extensions 检查；不能把 prompt 名与 extension command 名相等视为这里自动裁决。Map 输出保持首次名称插入顺序。

discoverSystemPromptFile/discoverAppendSystemPromptFile 分别检查当前 cwd/CONFIG_DIR_NAME 下 SYSTEM.md/APPEND_SYSTEM.md：projectTrusted 且 exists 时优先，否则 agentDir 同文件，最后 undefined。不会遍历祖先项目、也不合并 global+project。只影响自动发现，显式选项不经此 trust check。它们先 exists 后读取，目录/权限竞态交给 resolvePromptInput，可能退回字面路径。上下文 AGENTS 分层、系统替换文本、append 文本、模板和技能是五条不同通道，不应混为一个 prompt 字符串加载策略。

## 可运行行为判据与已读测试上下文

本轮源/目标产品测试运行均为 0；没有 Cargo、Node 源执行、PTY、HTTP、网络、安装、provider 或扩展工厂执行。以下是明确 fixture/操作/期望，可交由 master 后续运行的语义判据；S/T 引用是已读测试/实现片段，既不等于测试通过，也不构成测试文件全覆盖。

| 合同组 | fixture、操作与应观察结果 | 证据 |
|---|---|---|
| 空状态/getter/选项 | 构造后各结果空；改变 getSkills().skills 数组会影响该实例；显式空 systemPrompt 不自动发现，append=[] 也不发现 | 源构造器/getter/reload 分支推导；需隔离 fixture |
| 上下文候选/祖先 | global override、project AGENTS、leaf override 同时存在，应按 global→project→leaf；候选是目录则 fallback CLAUDE；noContextFiles 返回空（无override时） | S3 |
| nested worktree | 同名 main/worktree context 只保留 worktree；main CLAUDE 与 worktree AGENTS 均保留；bare/sibling/submodule/普通repo/坏gitdir仍保留相应祖先 | S6 |
| trust 两阶段 | 用户模块顶层计数器+project 模块；回调只看到预信任集合，true 后 project→user 且用户计数为1；false 自动 project SYSTEM 不用但 project AGENTS 仍加载 | S2、S3 |
| flags 与显式路径 | noSkills=true + additionalSkillPaths 仍调用 loader；noExtensions=true + CLI/inline 仍可执行；同 canonical 路径重复保留第一次 | 源路径条件推导；并非全禁执行开关 |
| 三类优先 | user/project 同名 skill/prompt/theme 由完整发现组合得到 project；两 prompt 同名只保留输入次序首项并给 loser collision；两个 unnamed theme 同理 | S1；dedupe 分支推导 |
| extension 贡献 | extendResources 注入技能目录与模板文件并给 extension:extra metadata，所得资源 path 是实际入口且 source 为该 metadata；后续 reload 没有重新贡献则不保证保留 | S4；reload 清图/lastPaths 分支 |
| 分类/映射 | auto/package dir 内 SKILL.md 和其他 Markdown，mapSkillPath 选择 SKILL.md；非 auto/nonpackage 原路径保持；模板 loader 不执行扩展工厂 | 源 mapSkillPath/updaters 的可构造判据 |
| 工厂/冲突 | 成功A、抛错B、成功C按序执行，共享runtime，B变error但C继续；tools/flags后者冲突仍保留；commands冲突不生成该类error | 源工厂/检测；S2、S5 |
| preload失败复用 | preload A成功、B失败；final含A/B/C，仅C再次load，A复用、B不重试，errors保留B；inline不再执行 | loadFinalExtensionSet 分支判据，未执行计数探针 |
| 来源规则 | extra宽祖先先插入、窄祖先后插入应选宽者；metadata exact优于metadata祖先但低于extra；override给缺失普通资源触发默认stat异常 | 源 findSourceInfo/getDefaultSourceInfo 推导 |
| 文本/主题错误 | 不存在 system 字符串按字面用；存在但读取失败警告后原字符串；broken theme symlink跳过、坏JSON warning、其余主题继续 | 源读取/catch 分支；无故障注入执行 |
| reload部分更新 | 先成功一次，再让 promptsOverride throw；观察 extensions/skills/lastPromptPaths 可已更新而 prompts/themes/context仍旧，loaded保留先前true | 源连续赋值，无统一回滚的可运行判据 |
| target原子快照 | v1成功后v2技能/模板+缺失文本路径，reload_with_paths错误，Arc::ptr_eq和原选择均保留；成功再发布generation2 | T5、T8已读断言 |
| target旧turn/取消 | 已选技能body和模板/text保留v1；旧snapshot未选skill在文件变更后load_body拒绝；每candidate取消检查点旧Arc保持 | T2、T9；仅已读测试 |
| target host | active turn用generation1时reload发布generation2，status同时显示1/2；失效/cancel资源不发布、不写选择记录 | T6、T10片段（后者含HTTP fixture，本轮未执行） |

未实际执行的错误/取消/恢复判据不会变成通过计数。源此文件没有 AbortSignal 或 restart journal，因此“取消自动回滚”和“重启恢复资源 generation”属于源不适用/目标增强；仍须分别验证 target owner，不从本源报告获得验收。

## 当前 zenpi 映射与反向范围核对

精确定位与 hash 索引见下文引用表及 evidence/context-references.json；这些是本轮捕获的 master 当前片段，不是假定 worker checkout 与 master 相同。正式源只此 resource-loader.ts，目标 src/skills.rs、src/extensions.rs 与调用方/测试均 context-only。

| 源责任 | 当前 owner/已观察实现 | 差异、缺口或范围排除 |
|---|---|---|
| 技能发现/同名/元数据 | src/skills.rs T1：user→project→explicit，后层替换前层；同scope TOML胜Markdown；collision winner/loser可查询；显式缺失、根symlink、预算超限为错误 | 源这里编排loadSkills和metadata，不拥有TOML hooks；target explicit后条胜与源additional paths尾部/路径first保留不可简单视为等价 |
| 技能正文生命周期 | src/skills.rs T2：按需read_skill_text，hash与发现时不同拒绝；Model调用尊重disable_model_invocation | 源resource-loader只加载Skill对象并归属，不在此load正文/执行模型技能；target旧Arc仅保留元数据不自动冻结未读取文件 |
| 模板独立加载 | src/resource_loader.rs T4/T5将templates与skills分开，统一collisions只含Skill/Template；T8明确explicit template胜project/user，同优先不同文件同名拒绝 | 源prompt first-wins+collision继续，target同优先重复是整次失败；完整模板语法owner为src/prompt_templates.rs/ZS1-020映射，本次未整文件验收 |
| loader结果生命周期 | T5先构造skills/templates/text，再最后提交paths与Arc，checked generation、取消检查、文本总1MiB | 原子发布是target增强；源逐字段赋值且无取消/预算；ResourceSnapshot无theme/extension/context/system专用字段，不能声称已覆盖所有源类别 |
| extension发现与注册 | src/extensions.rs T3：排序直接子目录、跳隐藏/目录symlink，manifest<=256KiB、disabled仅summary，enabled校验可执行文件并拒重名、最多32；register_with_lease先clone registry后整批成功赋回，API2要求runtime | source执行TS/inline并宽容收集errors；target TOML/本地进程catalog不同，不等于实现packageResolver、inlineFactory、projectTrust预加载、命令别名或source metadata全套 |
| host资源+extension reload | src/core.rs T6：clone/new候选loader完成后prepare_extensions，取消复查，append选择provenance再发布skills/loader及extension候选；reload_resources沿当前选择；restore只Idle；status有generation/active_generation | extension不在ResourceSnapshot本体，host负责协调；prepare/publish内部、session append耐久性和旧extension lease全合同超出该片段，不据此宣称跨进程事务 |
| slash入口/路径选择 | src/slash_actions.rs T7：/reload无参沿原选择，有参读workspace内JSON选择文件，拒绝绝对/父目录选择文件，配置内相对路径以workspace为基准再configure_resources | 源文件无slash处理；target选择文件约束不等于其中每个资源路径必须位于workspace，代码没有在此做该承诺 |
| context/system/append | T4/T5可显式选named text_resources并保存source/hash/content，T6负责host接入 | 没有在这些已读owner片段看到AGENTS.override多候选祖先分层、nested worktree去重、SYSTEM trust fallback和append来源数组。是本映射未证明的语义缺口；不能用generic text读取冒充完整对等，也不对未读其他owner作全库不存在断言 |
| themes | 源loadThemes*、dedupeThemes、theme override/sourceInfo完整说明 | 目标选定skills/extensions/resource snapshot没有theme catalog；UI主题属于不同owner，本轮无整合或验收，明确排除 |
| sourceInfo/override/package paths | 源metadata/extra归属、任意同步override、package/CLI解析 | target技能/模板source/hash/scope和extension summary不是完整SourceInfo的source/origin/baseDir语义；目标CLI包路径/JS override注入/两阶段trust均未由本片段证明 |

反向核对：src/skills.rs 的工具允许列表、legacy hooks、模型技能工具注册和沙箱资源读取细节，不属于本源文件直接职责，需其各自源/目标项；src/extensions.rs 的安装/删除/升级、能力broker、子进程调用和会话hook运行也不是本文件实现，排除其整文件接受。src/core.rs 只引用资源配置发布/状态片段，不碰turn loop、session树或provider。源文件所有额外职责（theme、context、system、append、path/sourceInfo、override、inline/preload）已逐项说明并映射/列缺口，不通过只看skills/templates跳过私有分支。

## 证据与门禁

源全文读取/字节连续范围见 evidence/gfile.json；所有context记录有整文件hash、片段行/字节边界和片段hash，可在相同输入重建。本轮只读结构checker，实际命令、cwd、argv、起止时间、exit、stdout/stderr bytes/hash在 evidence/gstage.run.json。checker只能判断结构，不替代本报告语义复核；缺master receipt必须保留provisional，禁止捏造complete或更改authority/index/checklist状态。

候选patch只新增唯一标准报告。source/context/scripts/校验记录均在私有ready证据包，不进入产品diff；master checkout、源库、A/C工作树、018/125和更早ready、scheduler/任务/产品代码无写入。没有G-RUST/G-HOST/G-CODE新通过声明。本项回滚是撤回该新增报告；实际patch只在临时git目录正反check/apply，并核对报告精确字节及无关sentinel不变，不在master应用。

### 本轮实际结构门禁与定位表

G-STAGE 实际 exit 1，structural.ok=true，唯一错误是主库缺少 `Docs/learn/stage1_pi_mono/receipts/ZS1-021.master.json`。没有创建该receipt。G-FILE 本地仅检查source hash、完整范围、report身份和context片段hash；semantic_verified_by_checker=false，master语义复核仍pending。产品行为执行数为0。

以下行/字节为context原文件的一基闭合行范围、零基半开字节范围；片段SHA和source文件SHA完整记录于evidence/context-references.json。这里的定位表不扩大正式复核范围。

| ID | 上下文文件 | 行 | 字节 | 文件SHA256 |
|---|---|---|---|---|
| T1 | /Users/wangweiyang/GitHub/zenpi/src/skills.rs | 142–262 | [3904,8635) | `a6d0a2f671febdef2e631ca53d2788ab869ab7b86db6c0cb1feaf24d128f83e8` |
| T2 | /Users/wangweiyang/GitHub/zenpi/src/skills.rs | 281–317 | [9325,10744) | `a6d0a2f671febdef2e631ca53d2788ab869ab7b86db6c0cb1feaf24d128f83e8` |
| T3 | /Users/wangweiyang/GitHub/zenpi/src/extensions.rs | 161–311 | [5061,10537) | `38a242eff3ec911f4560e3dea3868b742d511205ff0ccab1b622962b41e420ed` |
| T4 | /Users/wangweiyang/GitHub/zenpi/src/resource_loader.rs | 17–94 | [533,2991) | `121d3b6ea7760d137e0e074e7d9b269023ddc6ca2ecca199f23072890bd06858` |
| T5 | /Users/wangweiyang/GitHub/zenpi/src/resource_loader.rs | 117–226 | [3511,7285) | `121d3b6ea7760d137e0e074e7d9b269023ddc6ca2ecca199f23072890bd06858` |
| T6 | /Users/wangweiyang/GitHub/zenpi/src/core.rs | 2212–2345 | [85956,91566) | `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869` |
| T7 | /Users/wangweiyang/GitHub/zenpi/src/slash_actions.rs | 86–146 | [3477,5992) | `5912f71faa07e7f529b1c9bef875a977873c6a5e152d103e837d0fbac001cd32` |
| T8 | /Users/wangweiyang/GitHub/zenpi/tests/stage1_prompt_resources.rs | 178–242 | [6055,8397) | `1bbdbff6927a996a06c08bb7afe84d9a94f25722b89c757d7bd1867a9c46a40f` |
| T9 | /Users/wangweiyang/GitHub/zenpi/tests/stage1_prompt_resources.rs | 244–322 | [8398,10943) | `1bbdbff6927a996a06c08bb7afe84d9a94f25722b89c757d7bd1867a9c46a40f` |
| T10 | /Users/wangweiyang/GitHub/zenpi/tests/stage1_resource_host.rs | 194–290 | [6997,10521) | `6bfe5c0861bcd58c6dd42b8ca6a8ef4505ee8b030e20974cdc0024ba7695f4ae` |
| S1 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/test/resource-loader.test.ts | 101–160 | [3372,5713) | `4d7043fbae033b5398db34046feea2ac3e910079e8def6a1d198b7c840190fb0` |
| S2 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/test/resource-loader.test.ts | 191–303 | [6799,10495) | `4d7043fbae033b5398db34046feea2ac3e910079e8def6a1d198b7c840190fb0` |
| S3 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/test/resource-loader.test.ts | 348–475 | [12179,17929) | `4d7043fbae033b5398db34046feea2ac3e910079e8def6a1d198b7c840190fb0` |
| S4 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/test/resource-loader.test.ts | 557–622 | [21326,23127) | `4d7043fbae033b5398db34046feea2ac3e910079e8def6a1d198b7c840190fb0` |
| S5 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/test/resource-loader.test.ts | 845–959 | [30076,33595) | `4d7043fbae033b5398db34046feea2ac3e910079e8def6a1d198b7c840190fb0` |
| S6 | /Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/test/resource-loader.test.ts | 960–1119 | [33595,41570) | `4d7043fbae033b5398db34046feea2ac3e910079e8def6a1d198b7c840190fb0` |

---

## 3.1.21 独立逐文件复核补记（本轮候选，尚未主控验收）

本节是本轮结论。前 32,797 字节为 worker B 历史报告原文，SHA-256 `cd8de44c352ac754ea520d258dec0941171e756fb262e95862fdb39db3305a04`，逐字保留；其中当时的搜索结果、authority、路径与未执行说明不改写成本轮事实。本次另找到 C 的 behavior 与 reuse320 封存包，完整复制其物理文件用于身份复用。B 旧报告、C 旧执行、当前源阅读、当前目标有限阅读和本轮离线检查是五种不同证据，不把历史通过次数叠加为新执行次数。

本轮唯一正式文件项为 ZS1-021 / SRC-0931，唯一 owned path 为本报告。正式依赖仅 ZS1-001；不验收 ZS1-056、ZS1-114、ZS1-123 或整个 core 目录。本轮未改产品、main、claims、selector、blueprint、旧 ready 或旧 runner，未新跑 Bun/Cargo/PTY/HTTP/Git-worktree/性能预算，也未导入或执行封存脚本。状态为 source review candidate，`master_accepted=false`；离线检查通过不等于 G-STAGE semantic/manual gate 通过。

### 当前 authority 与证据边界

- version：`3.1.21`；run：`zenpi-stage1-20260911`。
- requirement：`3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`。
- baseline：`92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`。
- 初末完整 selector、blueprint、claims、manifest/index 与001 receipt分别保存在 `authority/`、`authority-final/`。本次六份完整快照初末均相同；独立核对四键、001/021 item行与021 claim，不把其它项状态作为本项验收条件。
- 001当前master receipt的34个artifacts逐个按声明 bytes/SHA核对后复制到 `formal-dependency/`；另实际阅读完整 `master-rebind-3.1.21/review.md`。它仅保留001未变义务的既有验收；并不为021或任何新增项授予通过。
- 初末 main 标准021报告和021 master receipt均不存在。因此补丁是仅创建本报告的候选；不得用补丁自动覆盖以后主控已生成的报告。

### 新完整源阅读覆盖

源 `/Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/src/core/resource-loader.ts` 为 1,097 行、40,180 字节，SHA-256 `1877e9535820cb8b45e5a84598ac8ae581058bb0fbee6f0c64ec672f8ab986dd`。本轮按 1–190、191–390、391–590、591–790、791–990、991–1097 六段连续完整阅读，不把旧 B 的五段或 C 的四段充作新阅读。`reading-ranges.json` 为每段记录1-based行号、半开UTF-8字节区间、原始节选hash和完整快照hash；六段拼接覆盖所有40,180字节，无空洞和重叠。`source/resource-loader.ts` 是完整原文。前三段最初读自实际Pi路径，随后快照与当前源做相同身份检查。

结合前述完整报告，新独立复核确认以下容易误读的边界：

1. **构造与状态所有权**：constructor只保存选项、normalize cwd/agentDir和建立空catalogue/runtime，不隐式reload；多数getter暴露可变数组/结果引用，source-path getter才构造新记录。配置数组和override/factory等引用也可由调用方持有。本文件是发现/编排层，并非模板解析器、模型执行器、持久会话或扩展命令路由的完整owner。
2. **提示与上下文**：resolvePromptInput对存在路径读UTF-8并strip BOM；读取失败warn后返回原输入文字，非存在输入本来就是文字，不提供统一read预算或取消。显式空system字符串会阻止自动发现、最终返回undefined；显式空append数组阻止自动append。自动SYSTEM/APPEND只在信任允许时用cwd `.pi`，否则/缺失时用global，每类至多一份，不搜祖先。context文件另按global→root-to-cwd祖先顺序，每目录AGENTS.override.md、AGENTS.md、AGENTS.MD、CLAUDE.md、CLAUDE.MD选择首个成功regular file；读失败允许尝试下一候选，不受同一个project trust过滤，也不在普通git根停止。
3. **嵌套worktree**：只在common git目录/主仓真实路径关系满足时跳过嵌套主仓中的同名context副本；不同basename仍可能同时保留。普通仓、兄弟worktree、bare/submodule不能从此特例推广；路径去重与影子过滤不是内容去重，影子文件可能已先读入再被过滤。
4. **发现与来源**：no*关闭默认集合仍允许CLI/additional、inline或override提供内容；package/CLI结果携带第一条path metadata，包括disabled记录。自动/package技能目录可映射到存在的SKILL.md，CLI skill列表不走同一映射。extendResources先更新三个额外来源map，再按canonical路径合并旧路径优先、保留首个拼写，顺序重载skills→prompts→themes。来源不是最长prefix：额外map的首个祖先匹配在metadata精确匹配之前，之后metadata才查normalized/raw exact及首个祖先；普通路径resolve与canonical identity用途不同。默认来源优先检查user roots再project roots，fallback stat可能抛错。
5. **信任与扩展**：bootstrap阶段先以false信任reload settings，再预载允许的global/CLI和inline，回调决定后续信任；异常不保证恢复之前trust状态。预载成功按resolvedPath复用，预载失败集合阻止重试，inline不会第二次执行；预载错误不因最终过滤而消失。扩展工厂的外部副作用无回滚。本文件冲突检查实际只枚举tools/flags，尽管注释也提到commands；发现冲突追加diagnostics而保留module，不能由此宣称commands已经拒绝注册。
6. **加载、覆盖与失败**：skills override后重新构造带sourceInfo的skill，prompts先按name首项优先并记录collision再override（override可重新引入同名项），themes去重后override且原对象sourceInfo会原位赋值。theme目录只直接扫描JSON文件，允许指向regular file的symlink，坏JSON诊断warning；没有递归或文件数限制。已有同path诊断可能阻止追加missing-path error，不能把所有无结果归类error。
7. **部分提交**：reload不是事务：早期清理metadata/贡献map、保存路径、随后逐项赋值，后段override抛错可能保留新prompts与旧system等混合状态；extendResources也相同。`loaded`只在成功尾部置true，因此首次失败后下一次不会通过该条件清cache，第二次以后的失败又可能已经清cache。没有并发序号、AbortSignal/统一取消、journal或恢复owner；getter也不是持久不可变snapshot。这是源契约记录，不能为了迁移“对齐”而消除Zenpi已有的原子发布/取消保护。

### 当前 Zenpi 映射：实际新读范围及差异

完整目标快照仅用于hash和便携比对，除 `src/resource_loader.rs` 1–280整文件外，不宣称其它完整目标文件都已阅读。本轮新读范围列在reading ledger；`core.rs`最初按旧行号读到6244–6315，发现当前内容是无关tool-call校验，明确作为incidental read记录，未据此写资源恢复结论。C原片段实际逐字存在于当前6347–6418，这只计身份定位；扩展prepare/publish另在当前2108–2211完整新读。

| 当前文件与实际新读范围 | 当前行为及与Pi的关系 |
| --- | --- |
| `src/resource_loader.rs:1–280`，SHA `121d3b6ea7760d137e0e074e7d9b269023ddc6ca2ecca199f23072890bd06858` | ResourcePaths显式user/project roots、extra技能/模板和named text；new建立generation0的Arc空snapshot。reload_with_paths先normalize并完成skills/templates/text，再取消检查、提交paths和snapshot；失败保留两者旧值。generation checked_add，text最多64份、总1MiB，名字1–128 ASCII alnum/_/-，duplicate报错。ResourcePaths deny_unknown_fields，TextResourcePath不作同样声明。绝对路径不全等于canonical；text parent存在时canonical化而保留末段供读取检查。snapshot字段只有skills/templates/text，不包含本文件级Pi themes/context/system/append/extension列表；不由此断言其它Zenpi模块完全没有这些功能。Arc catalog也不等于技能正文冻结。 |
| `src/skills.rs:142–262,281–317` | 临时catalog按user→project→explicit加载；后作用域覆盖前者，explicit后项赢，同scope TOML覆盖Markdown；有collision、数量/索引字节与取消检查，symlink root拒绝。正文调用时重新读取并校验source hash；changed要求reload。不是照搬Pi prompts首name优先，也不是Arc保证延迟读取内容恒定。 |
| `src/prompt_templates.rs:78–195` | 直接.md子文件、排序、重复同一文件去重；同scope同name错误，跨scope后者覆盖并记录collision。显式路径、目录项、catalog字节有界，拒绝symlink，支持取消；与Pi prompt warning并保留首项不同。 |
| `src/extensions.rs:161–311` | TOML manifest/本地可执行目录catalog，disabled summary保留，验证兼容性、路径与32个enabled上限；register_with_lease克隆registry后统一替换，API2需要session runtime。它不是Pi jiti/factory或包管理系统等价实现。 |
| `src/core.rs:2108–2211,2212–2345` | prepare_extensions要求Idle且拒绝BlueprintWorker权限；准备candidate runtime/registry。configure_resources先资源candidate，再已配置扩展candidate、取消检查、append provenance event，之后发布skills/loader/extension。prepare失败不会发布，但publish_extensions关闭旧runtime、替换后启动新runtime，hook失败转AgentEvent::Error，不回滚已append journal或新资源。不能把资源构建原子性夸大为扩展副作用/日志完整事务。restore_resources只在Idle重建路径并标记stale；其helper完整实现不在本轮新读范围。 |
| `src/slash_actions.rs:46–146` | 资源控制通过Agent owner；/reload无参数重载当前paths，有参数限workspace内的relative selection JSON，无absolute/ParentDir且最长4096字节；取消透传、各相对资源路径锚定workspace。该段没有证明整个host调度/跨平台/PTY并发。 |

初末9个目标完整快照相同。与B旧引用比较，16个原节选全部逐字一致，只有T7所处slash_actions完整文件发生历史→当前变更：旧41,343字节/current76,284字节，旧86–146本身未变；不把整文件hash不等误报为该片段漂移。C的11个原节选也均可逐字定位，其中core旧6244–6315迁至6347–6418；core和slash完整文件hash发生历史变化，其余5个C目标文件身份相同。完整列表、原/新hash和行号见两个comparison文件。旧目标tests的范围/行为只按历史报告和逐字片段复用，不计本轮新test阅读或运行，更不宣布当前集成二进制测试通过。

### 行为证据复用与真实失败链

本轮新完整阅读C保存的71行dense `probe.ts`，读取run metadata与reviewed逐案日志，核对三个日志实际bytes/SHA，并把reviewed日志23个case名称按顺序与probe里的23个`await check(...)`逐一比对。源控制器加12个依赖共13个源文件的旧SHA与本轮当前快照完全一致；依赖只做身份复用，未充作新完整阅读。源码和FS控制器证据因此可在下列明确边界内复用，无法升级成完整上游suite或当前Zenpi执行。

- 原始 `source-native`：2026-09-10 20:18:18 UTC，exit1；输出3,309字节，SHA `805766d54873c475dca41cbab6ad1f16e98770a445556d3f95f2e4524c7d1c93`。原probe错误期待missing skill诊断为error，实际是此前已有warning；源控制器并未为此修改。原始失败栈和期待表达式保留。
- `source-native-final`：同日20:18:42 UTC，exit0；输出3,322字节，SHA `ee045f4efee7e648c9bd1aea16bf101f1e1e229f6dbdc49d6edc12f2d12e8ace`，修正诊断期待后23例通过。
- `source-native-reviewed`：同日20:19:35 UTC，exit0；输出3,331字节，SHA `baf79e0a7483f784cf3f507ac50231f93545ba8cd41230a2a2762c1897c7c2af`，额外强化no-resource场景确实包括additional prompt，仍是23个独立场景，不是再加23例。本轮运行次数0。

场景覆盖：空constructor getter、真实prompt碰撞/来源、no*下CLI/additional、disabled与额外来源优先、真实skill parser自动SKILL.md、symlink canonical去重、extend path/baseDir与first ancestor、getter可变引用、reload读新prompt并丢贡献、missing诊断、reload/extend晚期失败部分状态、system/append信任路径、literal/file来源、目录无法当文件读取时warn+literal、pretrust成功失败复用与inline只执行一次、tools/flags冲突且commands保留、inline错误隔离/hidden、theme JSON诊断、context顺序/文件名/BOM、重复context及非文件候选、嵌套worktree同名影子、普通仓祖先context。详细23条逐案名称及结果在 `historical-execution-audit.json` 中原样保存。

四个明确替身是 package-manager（预定resolve/CLI结果，无真实install/network）、settings-manager（注入trust及reload记录，无真实设置持久化）、extensions/loader（预定extension/runtime及cache计数，受控factory，不是真实jiti/cache/provider系统）和theme模块（对真实fixture bytes作JSON.parse，不是完整schema/render）。其余controller、FS、skills/prompts parser、path/sourceInfo、Git finder使用源实现。Git场景是手写`.git/HEAD/commondir` metadata，不是执行git worktree；目录读取失败不是EACCES权限实验；override抛错是公开选项，不是私有方法替换。未证明Windows、真实主题渲染、安装、权限竞态、并行reload、所有扩展生命周期或生产host。

B历史G-STAGE exit1/structural true与C reuse320历史G-STAGE exit1（缺021 master receipt）均原样保留，没有重跑或转写为pass。本轮387项输入离线检查全部通过，内容为物理副本/旧manifest/13源身份/三次日志/23场景名称/原节选身份及只读123 manifest核对；这387项不是387个行为测试。最终便携检查另产生外置receipt，校验manifest全payload、六段覆盖、scope与正式依赖、report前缀与补丁，不访问开发机器的绝对源路径，也不导入任何旧脚本。

### 交付与主控处理

便携包根目录包含标准report、单文件创建补丁、全部原始历史包副本、初末authority与目标快照、源快照/reading ledger、identity audit、formal001依赖证据，以及新写的纯Python离线 `verify_packet.py`。`manifest.json`列举除自身外所有payload的相对路径/bytes/SHA，`README.md`说明运行方式与本轮限制。生成脚本也只是新私有证据，旧runner全部保持惰性数据。

主控需独立完整审阅本文件报告与源覆盖、确认上述迁移差异和复用边界，再决定整合唯一owned报告、运行/出具当前项正式gate/receipt；本包不写任何worker/master acceptance receipt。若移交后main已经有报告，应人工merge本轮补记，不盲目应用create补丁。撤回范围只限本报告和本轮私有包，不改其它文件项。完成本轮后停止，不自行扩展到目录、产品修复或预算工作。
