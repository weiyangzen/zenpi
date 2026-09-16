# ZS1-117 / release-diff-20260912 首次启动只读诊断

结论：本次失败仍为 **1109.425125 ms > 1000 ms，实际预算 exit 1，7/8 gates 通过**。本轮找到首样时间窗内与实测 zenpi 路径和当前二进制签名标识关联的 **1044.2375 ms syspolicyd 扫描至结果区间**。它是目前最有区分力的证据，使系统首次执行策略/XProtect扫描等待成为领先解释；但原记录没有目标子进程PID、exec或Rust main时间，不能宣称全部1109 ms已被因果归属，不能据此确认产品初始化时间、减去系统耗时或宣布117修复。

这是独立只读诊断包，不是源码understand完成receipt、正式产品修复或新budget结果。083 ready完全保持不可变。本轮没有执行zenpi/Cargo/Node/PTY/HTTP/网络/FIFO或任何旧基准/旧观察器/旧发布脚本，无新task/subagent，无主库/product/authority/claims/旧ready写入。

## 1. 原始输入身份与时间线

实测目录 `/Users/wangweiyang/GitHub/zenpi/.ops/stage1_execution/release-diff-20260912`。budget.run.json记录在主库cwd执行 `python3 tools/bench_runtime.py --samples 3 --output .../summary.json --startup-evidence-dir .../startup`，仅记录的env_diff为CARGO_NET_OFFLINE=true、CARGO_BUILD_JOBS=2；15:09:00.855615Z开始，15:10:30.052425Z结束，exit1。此处env_diff并非全部继承环境快照。

budget.log完整hash `6083ceb4bac97b81e11cdcb4ccc28c89b2e18316b431051c04f2c6b78c6653fc` 与run一致；其JSON部分逐对象等于summary.json。前缀记录crossterm unused_parens警告、Compiling zenpi、Finished release optimized 49.78s；build在样本timer之外，不是把49.78s构建成本算到首样。runtime_budget_probe执行在三个startup之后，也不在首样区间。

主库冻结zenpi与target/release/zenpi当前均为6072608字节，SHA256 `e9ba9476f7699d715ad3b0b5fe4a07aeee9f280508d3cb495d59cfa94fff6449`，等于三个sample及summary记录的hash。冻结原目录副本mode0555，target mode0755；inode分别12811717与12811136、同device，mtime均1789225790848810650ns。冻结副本与实测target不是同一inode，不能把冻结副本的签名缓存/首次执行状态当作原target状态。本worker又复制字节到包内作证据，未运行任何副本。

224项inputs-before对当前逐hash核对：223项一致，仅tools/generate_stage1_gantt.py漂移；当前src/main.rs、core/config/session/headless、bench/helper、Cargo输入均与该构建输入记录一致。未读取所有224文件作为源码验收，只做内容hash匹配。inputs.json另保存实际文件metadata；build-input-check.json保存逐项结果。当前签名状态也是事后只读观察，不是启动前签名策略缓存快照。

| 样本 | UTC开始 → 结束 | elapsed ms | Popen ms | communicate ms | process_observed ms | 外层差值 ms | wrapper PID | RSS bytes |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| 0 | 15:09:50.871269 → 15:09:51.980442 | 1109.425125 | 2.026250 | 1106.984250 | 1109.010500 | 0.414625 | 87568 | 5193728 |
| 1 | 15:09:51.982217 → 15:09:52.024618 | 42.552459 | 3.091292 | 39.274833 | 42.366125 | 0.186334 | 87572 | 5210112 |
| 2 | 15:09:52.025690 → 15:09:52.068100 | 42.552500 | 2.000833 | 40.382500 | 42.383333 | 0.169167 | 87574 | 5144576 |

所有sample与summary.observations逐对象一致；stdout均144B，同hash `25ec3e42f157458bc2bef231093a547f8ca61277aea7a7400a24075c4f67a8af`，解码得到唯一bench-stop shutdown success、closing=true/drained=true；stderr各777B，没有截断，完整base64解码后长度/hash核对。三次returncode0、timed_out=false。子进程成功退出和budget超时限是不同判定；不能把前者说成门禁通过。

原始time(1)三次分别 `1.10/0.03/0.03 real`，user/sys全部显示0.00（有限精度，绝非精确零CPU）；首样510page reclaims、0page faults、0block input/output、20voluntary/32involuntary switches、38,758,317 instructions；后二者约29.9/29.7M instructions。低CPU与长elapsed兼容等待，不能仅凭time计数排除产品里的阻塞I/O、调度或进程外系统工作，也不能把page faults=0解释为没有代码页验证/缓存活动。

八个gate实值：normal deps15<=16；release6072608<=8388608；cold max1109.425125>1000失败；RSSmax5210112<=100663296；queue1566.9865us<=2000；render1162.82us<=10000；layout1.0571833us<=100；coalesced1<=1。样本数固定3，未排除首样、改no-fail或使用near-cap。

## 2. 完整读过的入口与测量边界

本轮完整读src/main.rs 11行、tools/bench_runtime.py 338行（1–240 /241–338）、tools/runtime_budget_probe.rs104行、tools/test_bench_runtime.py90行及Cargo.toml全文。源码字节已冻结，reading-coverage.json保存每段连续行/半开字节/hash；没有把core或其它大dependency的局部阅读冒充整文件验收。20份明确bounded范围另见末尾定位表。

main.rs只调用zenpi::core::run，Ok返回ExitCode::SUCCESS，Err打印zenpi错误并FAILURE；它没有任何阶段时间标记。在进入main以前还存在OSexec/动态装载/Rust运行时等区间，不能以main代码短就证明启动必然快。

bench_runtime.py完整流程：dependency_receipt先cargo metadata/tree；build_and_measure_startup调用release build然后hash目标；创建一个TemporaryDirectory，并复制父环境后覆盖ZENPI_HOME、ZENPI_BACKEND=openai、ZENPI_BASE_URL=http://127.0.0.1:1、ZENPI_MODEL=benchmark-no-network、ZENPI_API_KEY=benchmark-placeholder。只给shutdown JSONL，不提交provider turn。所有样本共用该临时ZENPI_HOME，每次不同session-i.jsonl，fresh_session=True是代码生成的记录，不是独立验证每次所有资源/缓存都冷。此代码没有warmup循环；首次不同于后续还包括shared home/系统缓存/项目读缓存等潜在状态，不能从后二样快反推某一种原因。

outer perf_counter_ns在构建、hash、临时目录及session Path创建之后、进入timed_process之前开始，后者返回后结束。Darwin timed_process包 `/usr/bin/time -l target/release/zenpi --mode headless --session ...`；内层started在Popen前，spawned在Popen返回，finished在communicate完成后。Popen使用stdin/stdout/stderr匿名pipe、start_new_session=True，返回的PID是time wrapper，不是zenpi子PID。communicate立即发送固定payload并等stdout/stderr EOF与wrapper结束，timeout=5秒；超时才killpg/reap，本次未触发。

父Popen返回只说明time wrapper启动到一定阶段，不说明time已fork/exec到目标或Rust main已开始；communicate包含wrapper、目标首次exec/系统检查、产品初始化、读shutdown、输出、关闭/回收、pipe EOF及调度。返回前还有UTF-8解码与time RSS解析，构成outer-extra的一部分。base64/hash/写sample JSON在outer timer停止后执行，不在1109.425125ms内。三个extra仅0.415/0.186/0.169ms；因此“保存诊断JSON/哈希造成首样秒级超限”与源码顺序及原始分段不符。Popen2.026ms也不是差异主体。time包装内部在spawned之后的成本仍未完全分解，不能宣布time进程本身绝无影响。

write_startup_observation保存每流最多64KiB prefix并有完整hash/bytes，此次两流远小于上限，原始流均可重建；用open('x')防覆盖已用sample路径。临时目录在函数结束删除；本轮metadata确认原sample路径只作已删除fixture的历史路径，不依赖其不存在以推断当时内容。bench没有保存当时session/replay/资源目录字节、目标PID、exec/main、首次stdout接收时间或父mach绝对epoch，因此历史产品phase无法补造。

run_probe实际helper进行60000次布局、500次TestBackend render、10000次dirty request合并、2000次队列往返并关闭runner，只在startup后运行；它不是首样产品初始化的一部分。test_bench_runtime四个fixture测试静态覆盖字节流、失败记录、timeout/group kill、64KiB及拒覆盖；本轮未运行它们，也不据其历史测试通过证明首样无包装误差。Cargo release opt-level=z、lto=thin、strip=symbols、codegen-units=1、panic=abort；默认features空。不能仅从优化参数推断OS扫描耗时。

## 3. 精确时间窗的系统证据

只运行两条只读unified log查询，均限定当地 `2026-09-12 23:09:49` 至 `23:09:53`：先eventMessage含zenpi或实际CodeDirectory hash，再用已见path token `ba2be7534db6fb27` 精确查询。均exit0，原始JSON/argv/开始结束时间/stdout/stderr/hash保存commands。未读广泛系统日志、未重试历史被拒DTrace/特权探针，也未发spctl assessment、codesign verify、签名修改或清缓存命令。

事后`codesign --display --verbose=4`（target及冻结副本）显示相同Identifier `zenpi-cdb646c6c40059cd`，flags `0x20002(adhoc,linker-signed)`，embedded CodeDirectory、SHA256、CDHash `f64dff4ca0096b87915790873ff0ec43715612d6`，无TeamIdentifier/CMS签名身份绑定；这不是codesign验证通过或notarization判定。`file`确认arm64 Mach-O；otool -L仅列libSystem.B.dylib/libiconv.2.dylib；xattr只列com.apple.provenance，没有列com.apple.quarantine，但这是事后状态，不能据此排除本次Gatekeeper/XProtect活动。sw_vers为macOS26.6.2/25G83；原report平台为Darwin25.6.0/arm64/Python3.9.6。

去重后6条相关事件均同boot `FED0AF13-2278-4483-A836-B2FAD5427DA6`，当前sysctl boot UUID匹配：

| 当地时刻 | 进程/线程 | 原始事件关联 | machTimestamp |
|---|---|---|---:|
| 23:09:50.896822 | syspolicyd97597 /29518620 | GK performScan，path ba2be7534db6fb27 | 48003624000108 |
| 23:09:51.940797 | syspolicyd97597 /29518633 | GK Xprotect results，同path，并明确file:///Users/wangweiyang/GitHub/zenpi/target/release/zenpi | 48003649055514 |
| 23:09:51.941036 | syspolicyd97597 /29518620 | scan finished, waking up any waiters，同path，id zenpi-cdb646c6c40059cd | 48003649061246 |
| 23:09:51.941059 | syspolicyd97597 /29518633 | GK evaluateScanResult，同path和id | 48003649061808 |
| 23:09:51.941173 | syspolicyd97597 /29518633 | Found provenance data on target，同path和id | 48003649064544 |
| 23:09:51.941300 | kernel0 /29518631 | evaluation result ... exec, allowed, cache ... 精确target路径 | 48003649067582 |

本轮通过只读libSystem mach_timebase_info返回numer125/denom3、rc0；同boot scan→evaluate相差25,061,700 ticks，换算 **1044.2375ms**；scan→wake **1044.214083ms**。这是系统事件内部同一时钟域的有效差值，不是拿不同进程wall clock减父perf_counter。

以UTC/当地wall时间作相关性定位，整个scan区间落在首样父观察开始/结束之内，scan开始约在父UTC开始后25.553ms，kernel allowed事件到父结束约39.142ms。后二样在该扫描完成后启动；这与每次后续约42.55ms相容。但父记录只保存自己的perf_counter差值和UTC标记，没有与mach绝对时钟配对，因此这些跨域位置只称wall-time相关，绝不作为严格可相减成本分账。

强关联链：实测argv指向target路径；三样记录binarySHA与当前/冻结字节一致；系统XProtect日志显式同一绝对路径；同path token把scan开始连接到结果；结果identifier与当前embedded signature一致；exec allowed也指向同路径。相比“热样快所以必然OS”推论，这里有独立系统事件支持。

尚缺因果闭环：日志processID97597是syspolicyd，不是87568(time)的zenpi child；kernel event processID0也不是目标PID，eventMessage中的未解释数字不冒充PID。结果identifier非完整文件SHA；日志没给目标exec syscall起止、目标睡眠wait reason、真实main入口或runtime teardown点。“waking up any waiters”是日志文案，不是目标PID确实等待全段的证明。当前query没有其它事件不代表系统未记录或不存在其它活动。也不知道扫描自身为何需要1044ms，不能越过现有证据归因为具体XProtect规则、网络查询、签名大小或磁盘吞吐。

## 4. 产品启动路径：仅已读有界调用链

core::run用parse_args处理--mode headless及--session，非help、非config/extension子命令路径随后make_backend→SessionStore::open→Agent::new→acknowledge_recovery→工具registry/context→approval→restore_resources→configure_extensions→restore_model_selection→headless::run_stdio_owned。未对所有CLI分支或整个core做新owner验收。

make_backend在openai/anthropic/google分支resolve_workspace，再backend_from_effective；构造registry、URL/key/model设置和OpenAiCompatibleBackend。resolve_workspace读配置/认证的**代码**说明默认路径可依ZENPI_HOME临时根，env覆盖提供占位provider设置；本轮没有读取真实认证/环境值。config load_config/load_auth在文件NotFound时返回默认，无ensure_root调用；不能沿用“fresh home一定首次创建全部配置目录耗秒”的猜想。workspace的.zenpi/config.toml仍可参与，默认resource路径还包括cwd/.zenpi/skills与prompts，benchmark不是完全清空项目上下文的隔离运行，继承环境也未完整冻结。

SessionStore::open(...true)调用open_with_options，包含metadata、parent create_dir_all、读session内容/权限及新session后续处理；core.configure_extensions会写extensions_selected并publish/start runtime；restore_resources经loader reload读取资源。由源码可确定产品存在文件读取/写入及同步初始化工作，不能因main11行而忽略。但本轮只有相关有界段，没有获得原首样journal时间或fsync跨度，因此不能归因“某一个初始化函数耗1秒”，也不能声称资源目录为空或完全无I/O。

headless::run_stdio_owned进入run_async_stdio，先ProjectOwnerPool/restore_checkpoint、ReplayState::open、注册live owner，再stdin-reader线程和BackgroundRunner。Shutdown会emergency_cancel、延迟ack直到RuntimeEvent::Closed，随后runner.shutdown_and_join、replay.close/pool.close_all等收尾。原stdout closing/drained只证明整条关闭协议成功观察到，**不是ready-to-first-response延迟已单独测量**。本基准包括完整startup→shutdown→exit和time wrapper回收，而不只是启动到ready。此次不扩展整个headless/session/runtime所有者验收。

before/after ps仅为PID/PPID/%CPU/COMM快照；从中限定提取Python/cargo/rustc/zenpi/time名单，只发现Python87390/ppid87389/%CPU0.0。未复制其它宿主command lines，原两文件全hash保存。它们在启动窗口前后，并不提供1.1s区间内CPU调度/所有系统进程负载，也没捕获短命targetPID；不能以两张快照证明完全无负载。

## 5. 假设分级与历史保留

| 假设 | 此轮证据及判断 |
|---|---|
| 诊断JSON/hash写盘导致秒级包装延迟 | 不支持：写盘/base64/hash在timer之后；outer_extra仅0.415ms。 |
| Python Popen本身1秒 | 不支持：父分段2.026ms；目标exec可在time wrapper内随后发生，不能把Popen小等同目标exec小。 |
| 首次执行系统策略/XProtect扫描是主要等待来源 | **领先、高相关**：同路径/签名标识、同首样窗口、扫描1044.2375ms、exec allowed紧随结果；但无目标PID/main/wait证据，尚未完整因果确认。 |
| 产品同步初始化或shutdown发生阻塞 | 未排除、未定位：实际调用链有配置/资源/session/replay/线程关闭工作；time低CPU不排除阻塞，但无阶段时间证明。 |
| time wrapper内部或OS调度其它延迟 | 未完全排除：communicate内没有exec阶段；外层不足以解释，仍需phase分离。 |
| 扫描慢由某个具体签名/规则/磁盘/网络原因造成 | 无本轮证据，不作结论；本轮没有网络诊断。 |

历史原始budget/run/log另复制为history/old-1044与old-1208，实际分别1044.823959/1208.732708首样超1000、各7/8，旧测量各5样且cold_start字段没有二进制SHA，不能伪造补为当前SHA；对应旧identity由旧主控review/checkpoint独立引用。此前自然新release的751.967292首样/8gate通过也保留（1d7b2e61...、5样）。此前主控review曾用旧同path系统区间436.613667ms作有限相关，不绑定旧目标PID/完整失败。不同产物和历史观察不合并成当前统计，不把之前后续热运行当作本次修复。本轮既未复跑历史被拒DTrace，也未复跑有wrapper-only timeout清理局限的旧observe.py。

本次领先线索与旧线索方向相容，但三个历史失败原因是否完全相同仍未证实。117、本次gate和产品接受都保持未完成；没有当前source修复建议可凭证据直接落实。

## 6. 本轮诊断动作、失败与交付

commands下10份read-only系统命令回执（sw_vers、file、两次codesign display、两次xattr names、otool -L、两次精确log show、sysctl boot identity），每次实际argv/UTC开始结束/exit/stdout/stderr/hash保存，全部exit0；时间查询进程均正常结束，未留probe常驻。另mach_timebase_info只读API实际rc0单独保存。冻结/解析/范围/哈希脚本只处理本地证据，未导入或运行bench/runtime/product代码。

导航原始失败未隐藏：查不存在master scripts目录时rg输出 `No such file or directory (os error 2)`，管道最终exit0（不能伪称整个命令exit2）；后来cat已冻结Cargo.toml及不存在`.cargo/config.toml`时实际exit1，Cargo内容已完整输出，缺文件错误保留。这两者不影响测量，也没有创建缺失配置。一次跨checkpoint导航输出超过tool预算被截断，只作导航；选中的具体checkpoint/review、历史budget及实际源码已另读/冻结，不将截断输出计为全文证明。初始阅读config997–1039和core650–719属未用于结论的相邻导航，正式20段定位不冒用这些无关分支。

全部3170个既有ready文件（含不可变083）hash清单冻结，最终再次核对。新的diagnosis ready只含本报告、最小后续实验设计、原始证据/源码范围/只读回执与验证器；无product patch、没有G-STAGE通过声明或master receipt。本包离线验证只做闭合集合/hash/连续范围/原始sample与log一致性/数值重算，不能验证根因真实性。


## 7. 冻结阅读范围定位

| 完整文件 | 行数 | 字节 | SHA256 |
|---|---:|---:|---|
| src/main.rs | 11 | 234 | `9501abc92cf4c1a4a02250f175f4867cbf8052223c589b2a0e1b00f51113d81b` |
| tools/bench_runtime.py | 338 | 15327 | `ef6701f52659c78f58fb3f31adf87343c31b34229145f57acba2be05b1a9a52c` |
| tools/runtime_budget_probe.rs | 104 | 3594 | `b482152a711bcfbc56f9c6451d5aa5eb0f996f0219496701b3492dcffb1378b6` |
| tools/test_bench_runtime.py | 90 | 4767 | `80cdc26ae49b1b57ce13c73a4956b41bd2c58f63239126c404ccf8adcf937926` |
| Cargo.toml | 50 | 1228 | `d5fafd029ef77454e178222c2b62530d28859374f72a2ce6bd972a465bed5884` |

| 有界ID | 原文件 | 闭合行范围 | 整文件SHA256（仅身份，不声称全读） |
|---|---|---|---|
| C01 | /Users/wangweiyang/GitHub/zenpi/src/core.rs | 5850–5867 | `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869` |
| C02 | /Users/wangweiyang/GitHub/zenpi/src/core.rs | 6073–6129 | `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869` |
| C03 | /Users/wangweiyang/GitHub/zenpi/src/core.rs | 5422–5498 | `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869` |
| C04 | /Users/wangweiyang/GitHub/zenpi/src/core.rs | 5598–5681 | `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869` |
| C05 | /Users/wangweiyang/GitHub/zenpi/src/core.rs | 5710–5849 | `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869` |
| C06 | /Users/wangweiyang/GitHub/zenpi/src/core.rs | 2151–2188 | `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869` |
| C07 | /Users/wangweiyang/GitHub/zenpi/src/core.rs | 2271–2318 | `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869` |
| C08 | /Users/wangweiyang/GitHub/zenpi/src/core.rs | 6347–6376 | `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869` |
| C09 | /Users/wangweiyang/GitHub/zenpi/src/core.rs | 6390–6424 | `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869` |
| C10 | /Users/wangweiyang/GitHub/zenpi/src/config.rs | 148–201 | `f5691bd3818dcaf1481a88b63cc4f3874bc6b007234059e26124b74046325d12` |
| C11 | /Users/wangweiyang/GitHub/zenpi/src/config.rs | 713–749 | `f5691bd3818dcaf1481a88b63cc4f3874bc6b007234059e26124b74046325d12` |
| C12 | /Users/wangweiyang/GitHub/zenpi/src/config.rs | 777–827 | `f5691bd3818dcaf1481a88b63cc4f3874bc6b007234059e26124b74046325d12` |
| C13 | /Users/wangweiyang/GitHub/zenpi/src/config.rs | 1619–1653 | `f5691bd3818dcaf1481a88b63cc4f3874bc6b007234059e26124b74046325d12` |
| C14 | /Users/wangweiyang/GitHub/zenpi/src/session.rs | 407–450 | `8fb3ff2c387b13d34d8bbe4533bb2b0a81b5e546f5e03216b74b5cf2c61c9a2b` |
| C15 | /Users/wangweiyang/GitHub/zenpi/src/session.rs | 511–597 | `8fb3ff2c387b13d34d8bbe4533bb2b0a81b5e546f5e03216b74b5cf2c61c9a2b` |
| C16 | /Users/wangweiyang/GitHub/zenpi/src/headless.rs | 774–842 | `958837ad933fea4e55c3ac966f0c8d605dd03674a696e2ef0595b54aca628ef9` |
| C17 | /Users/wangweiyang/GitHub/zenpi/src/headless.rs | 4272–4366 | `958837ad933fea4e55c3ac966f0c8d605dd03674a696e2ef0595b54aca628ef9` |
| C18 | /Users/wangweiyang/GitHub/zenpi/src/headless.rs | 4715–4753 | `958837ad933fea4e55c3ac966f0c8d605dd03674a696e2ef0595b54aca628ef9` |
| C19 | /Users/wangweiyang/GitHub/zenpi/src/headless.rs | 5007–5031 | `958837ad933fea4e55c3ac966f0c8d605dd03674a696e2ef0595b54aca628ef9` |
| C20 | /Users/wangweiyang/GitHub/zenpi/src/headless.rs | 6220–6245 | `958837ad933fea4e55c3ac966f0c8d605dd03674a696e2ef0595b54aca628ef9` |
