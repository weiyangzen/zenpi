# ZS1-027 — find.ts

状态：[_] worker 完整阅读及实际源行为候选，主控未接受。
新增证据绑定 authority 3.1.16 / `c0262492bc6e3b4d5f56b35bedc7c1d1658854df11c4a975eb612a514cceef8f`。
source_id: SRC-0949
source_path: packages/coding-agent/src/core/tools/find.ts
source_hash: b06bcae6821a0e9fda1b63be613a7ce28eb0e66e1d01d16564e59e83f9ce10ea
source_bytes: 11178
read_ranges: [[0, 11178]]
run_id: zenpi-stage1-20260911

完整阅读 1–318 行（1–300、301–318）。schema为glob pattern/path/limit默认1000。relativizeFindResultPath使用指定平台path模块把绝对结果相对searchPath并统一POSIX分隔符，保留尾目录分隔符；它本身不验证结果是否逃出root。defaultFindOperations.glob是占位[]；真实default分支执行fd，不能拿占位分支当生产能力。

customOps.glob分支先exists、前后abort检查，传ignore node_modules/.git与limit，返回结果统一相对化并按bytecap truncateHead，>=limit即resultLimitReached。custom返回的绝对路径可被相对化成..而没有workspace拒绝合同，zenpi不能直接复用该宽松假设。

default调用ensureTool(fd)，可能下载；本次zenpi机器只有rg15.2，没有fd，候选不下载、不假装存在fd，而用真实rg文件枚举+glob过滤提供明确engine= ripgrep。原fd参数--glob/color=never/hidden，查searchPath祖先.git；仅非gitrepo加no-require-git，避免跨nestedrepo边界继承父.gitignore（代码引用issue5960）。max-results使用effectiveLimit；含/的pattern启用full-path，并为普通相对pattern加**/以匹配绝对路径；Windows替换分隔符字符类。

fd stdout逐行收集到数组；stderr无单独cap；close后把行trim（因此首尾空格文件名会变化），规范CR、统一相对化、截总bytes、附resultLimit标志。非零exit且已有stdout会继续返回部分结果；只有无output才reject，zenpi候选选择保守错误，不把执行失败包装成成功完整集合。AbortSignal早期和exec中都会settle，kill child后Promise可在回收完成前拒绝；zenpi候选必须由SupervisedChild Drop/cleanup完成group kill+wait后返回。

112保留tool=find的显式glob入口、workspace相对路径、ignore/hidden选择；使用NUL分隔保留空格、控制路径长度并拒绝换行/NUL/../外部canonical目标及symlink结果。限制文件数、深度、输出、时间和累计child-start预算，缺已安装rg返回Unsupported；默认search_text literal不受缺rg影响。不复刻fd全部平台/目录结果模式，报告不能宣称fd等价全量能力、worker advanced已获授权、或已完成主控host预算接线。


## 3.1.16 新增实际源证据

先完整读本地既有报告，再独立完整读取find.ts 1–318行；11,178 bytes和冻结hash与上文一致，源HEAD为`bbb61e34aaf231639fdaaad1adbd757947034eac`。旧段落“机器没有fd、候选不下载”是先前Zenpi产品候选环境记录，不适用于本轮源行为验证：本次未改产品，而是在`.ops/stage1-worker/source027/agent-home/bin`内用未改上游ensureTool实际下载/解包fd10.5.0。二进制2,970,480 bytes，SHA256 `cf3bde435da174f41cf9589a2efeaf03804df7c250fc15e8b8a1e9bfc66ebc9a`，Darwin arm64；后续所有测试设PI_OFFLINE=1，不再查询latest或下载。冻结包`.ops/source027-ready`及Docs镜像包含该二进制的lossless gzip、版本/hash收据与上游v10.5.0 MIT/Apache许可证，重放固定同一二进制，不依赖未来latest。

Node24.19.0，真实Vitest4.1.9/TypeBox1.3.27/cross-spawn7.0.6。find、path-utils、tools-manager/management-http、config/path/child-process/text、truncate、wrapper均未改。仅TUI renderer导入被替换为findRenderers={}，原renderer冻结为context-only，不计UI验证。工具获取首次bootstrap实际网络属于工具供应流程，不是模型provider网络；无需任何模型credentials。原test完整读取后原样执行：3302四项真实fd glob，3303两项真实fd ignore，6104十项POSIX/Windows path纯函数加一项custom glob。Windows path模块的数学结果不代表在Windows执行fd。

新增8项探针，最终4个测试文件合计25 passed、0 failed/skipped/todo：前4项真实fd/真实临时FS；第5/6项显式自定义FindOperations；第7/8项替换本次隔离目录内的fd依赖可执行文件以驱动真实spawn/readline/error/cancel分支，分别是受控Node程序及不可执行文件/离线缺失。异常注入结束finally恢复原fd，并再次校验完整SHA。没有把模拟fd输出当原生fd搜索结果，没有改源实现绕过失败。UI、AgentSession、完整tools.test.ts/上游整包、Linux/Windows原生fd没有运行。

### 全文件控制流及实测补充

relativizeFindResultPath接收平台path模块，保留目录尾分隔符并统一POSIX分隔符；绝对路径用relative，不检查是否越过搜索根或其它盘。schema只定义Number而没有正整数minimum。默认limit1000通过nullish fallback，0/负数/小数不会被本文件提前归一化。参数使用ctx.cwd优先，路径经resolveToCwd处理绝对/相对/~/@/Unicode空格；这些路径规则是依赖边界，不构成workspace confinement。

execute建立settled/stopChild，预先aborted立即拒绝，注册一次abort监听。settle只防止重复resolve/reject并去掉监听/stopChild引用；它不取消异步custom操作，也不等待正在执行的流程终结。try/catch保留Error或转成Error字符串，若signal已经aborted则统一Operation aborted。

customOps.glob存在时先exists，前后检查abort，传入ignore node_modules/.git及effectiveLimit。之后直接信任返回数组，没有slice限制、排序、去重、目录存在性、symlink或../校验。实际自定义fixture返回4项而limit1，4项全部显示并标志1 results limit reached；外部绝对路径显示../outside.txt。empty返回固定文案，exists false在glob前拒绝。真实Promise门控显示abort后工具已拒绝而glob尚未结束；释放fixture后glob继续完成，只是最终返回被settled忽略。FindOperations API没有signal参数，不能宣称底层远程工作被取消。

defaultFindOperations.glob=[]仅占位，不是实际默认搜索。真实默认先ensureTool，再检查abort/缺工具；default不会用ops.exists检查searchPath，fd自己报missing root。ensureTool源码优先隔离bin已有文件，再系统fd/fdfind --version，离线缺失返回undefined，否则可查询最新release并下载；本轮bootstrap固定了实际产物，仅重放同一hash。执行前向祖先寻找.git（文件或目录都可由exists命中）；非repo添加--no-require-git，repo则不添加。args包含--glob、--color=never、--hidden、--max-results，含/的pattern加入--full-path及适当**/前缀；Windows替换分隔符仅静态/纯path测试，不声称原生执行。

实际fd验证：非repo层级.gitignore按各子树生效，隐藏文件出现；没有明确.gitignore时node_modules/visible.txt可出现，不能把custom分支的ignore数组当作default固定排除。嵌套.git目录让上层ignored.txt规则停止传播，nested自己的.gitignore仍生效。实际src/**/*.spec.ts、绝对pattern、ctx.cwd均匹配，显式../sibling搜索被允许并返回该根的相对路径。此源没有Zenpi工作区授权合同。

实际fd默认用newline输出，readline逐行收集；输出数组、stderr字符串没有本文件独立字节上限。close之后才normalize和truncateHead到50KiB，maxLines设为MAX_SAFE_INTEGER。真实650个约109字符文件名超过byte cap，totalLines650、显示content<=51200bytes、没有fullOutputPath/spill。追加notice本身使整个content字段可比cap更大，cap限制的是路径正文。默认结果是否按稳定顺序出现由fd负责，本文件不排序。

名称反例实际验证并修正旧泛化：fd给绝对路径时，basename的前导空格处于整行中间，因此` lead.txt`保留；尾空格被trim删除，`trail.txt `被显示成不存在的`trail.txt`。带newline文件名分裂成`first`与`second`两条路径，无法无损还原。Unicode正常名字保留。指向目录的symlink可以被列出，但本次默认fd未跟随进入secret.txt；本文件不canonicalize或禁止返回symlink，不能当作安全隔离证明。

真实limit1恰有1条时仍标resultLimitReached，即使没有遗漏；0传给fd10.5.0表示不设置正数结果上限，2项都返回并标0 results limit reached。负数、小数由真实fd参数解析报错。返回notice建议double limit，但没有遍历分页或完整集证明。custom返回超过limit时也不被主动裁切。

default子进程用普通spawn，不detached、不创建进程组，无工具级timeout。onAbort调用child.kill默认SIGTERM，随后立刻settle reject，没有wait/reap承诺。受控fd替身装SIGTERM handler并延迟350ms退出；工具拒绝时PID仍活，之后观察signal marker和PID消失；fixture有6秒安全退出。该测试证明的是未改find的生命周期边界，不是原生fd10.5.0会刻意延迟退出。它不覆盖脱离child的后代回收或主进程退出。

child error实际EACCES包装为Failed to run fd；离线工具缺失实际报fd is not available。stderr逐chunk字符串拼接，代码close非零仅在原始joined output为空时抛错。受控fd替身exit7输出partial.txt和错误stderr，find仍返回partial.txt且无partial/error标记；两条blank line令原output非空，normalize后为空，却仍返回空字符串成功。无stdout的exit7才抛stderr错误。这些替身只控制外部子进程输出，真实spawn/readline/close分支执行原文件；不冒称原生fd故障可稳定产生同一payload。

stdout line回调在abort后仍可收集到close，readline cleanup发生于error/close；settled阻止二次结果但不是资源回收屏障。_onUpdate未使用，没有实时进度/fullOutputPath/hash/TTL/redaction或scope budget。getToolPath对已存在bin路径的权限/真实性不是本文件保证；受控不可执行fixture确实走到spawn失败，验证没有隐藏替代engine。所有输出/fixture仅本次范围内数据，源码未改、主树未改、没有新任务。

### 产品映射和验收

旧112/111目标映射继续作历史背景：Zenpi使用受控枚举、NUL/路径校验、有界预算、group kill/reap及sole writer，不能直接继承源的newline/宽松scope/部分错误成功。当前controller产品集成状态不由这个文件报告决定。本轮不重跑旧产品计数、不接受context-only文件或目录。025已先完成冻结且未改；本报告为027顺序补证。候选[_]，8 accepted/100 open、108IDs/56files/25dirs均不变。


## Controller acceptance under3.1.17

# ZS1-027 independent complete source review

The controller read all318 lines /11178 bytes of the original unchanged find.ts, the complete worker report and README, all three original regression files (17 cases), the complete supplemental probe (8 cases), executable fault fixture, fixed-fd preparation, source verifier and replay configuration. All frozen package files and15 upstream source/context/test copies were hash checked. The recorded Node24.19.0 and decompressed native fd10.5.0 bytes were verified. All2106 installed dependency file hashes/symlink targets matched before copying into private scratch. The controller then executed the original25 cases with fresh filesystem fixtures:25 passed,0 failed. The two source checks in the run confirm all15 frozen source copies remained unchanged; the original fd hash was checked after fault cases restored it.

Review covers each declaration and every execute branch: platform path relativization, Number schema/default1000, custom exists/glob contract, pre/post-await cancellation, single settlement, default ensureTool, ancestor .git detection, glob full-path adaptation, spawn/readline/stderr/error/close cleanup, path normalization, byte truncation, exact-limit notice, renderer spread and wrapper export. Default fd execution is separate from defaultFindOperations.glob's placeholder. The renderer import alone is injected with an empty object; original UI rendering was not tested. Custom-operation cases and controlled fd replacement are explicitly fault/contract injection, never claimed as native fd search results. Windows path calculations are pure functions executed on macOS, not Windows-native execution.

The actual cases establish native glob/ignore/nested-repository handling and filenames, conservative exact-limit notices, limit0 behavior and50KiB truncation without a full-output artifact. Custom results can exceed count and escape the search root; abort settles without cancelling that custom operation. The controlled child exposes nonzero-with-partial-stdout success and rejection before the delayed SIGTERM child exits. The source has no process-group reclamation, tool timeout, independent stderr/line-collection cap or workspace confinement guarantee. These source limitations are stated rather than silently promoted into target behavior.

Current target mapping was inspected separately in FindFilesTool (definition/admission/path resolution and search process budget), SearchBackend::find/files/run, validate_glob and checked_result_path. It explicitly uses installed ripgrep, default limit100/max256, hidden=false, NUL-delimited file listing, independent ignore/glob intersection, bounded output/depth/time, private-output exclusion, canonical workspace checks and supervised child cleanup. Native source fd defaults hidden=true/limit1000 and permits broader roots/custom returns; the two contracts are not equivalent and no full1:1 provider/platform claim is made. Target-context snapshots are retained only for those inspected mapping ranges, not full owner-file acceptance or new target behavioral tests.

This accepts only027 complete-file source understanding under3.1.17; its source identity, scope and validators are unchanged from the worker's3.1.16 capture. The original worker report remains preserved, including historical product-environment/status paragraphs. Parent directory, remaining source files, target owners and product112/111 still require their independent acceptance. Runtime dependencies, reference tests and renderer files are context-only and do not expand the frozen coverage subset.

The preceding worker status/counts and earlier lack-of-fd statement are historical. Current controller receipt accepts027 only; source scope is unchanged.
