# ZS1-117 私有五阶段首启诊断

本轮经主任务授权，只进行一次私有新构建与该产物的一次首次启动。五个标记均完整，目标 PID 97022、PPID 97021，退出 0，无超时，wrapper 已回收且进程组已消失。本次父进程启动到 Rust main 首条语句为 **935.707583 ms**，main 到返回前为 **40.391917 ms**，观察器外层为 **977.101334 ms**。因此，本次诊断的主要延迟确实在 main 前；尚未证明原 1109.425125 ms 失败的全部根因，也没有修复或通过 ZS1-117 正式门禁。

## 测量范围与身份

- 工作根：`/Users/wangweiyang/.codex/worktrees/2267/zenpi/.ops/zs1-117-fivephase-work`。
- 新构建：`cargo +stable-aarch64-apple-darwin build --release --locked --offline --bin zenpi`；cwd 为工作根下 `source`，独立 `target`，`CARGO_NET_OFFLINE=true`、`CARGO_BUILD_JOBS=2`。UTC 2026-09-12 15:34:41.078307 至 15:36:02.016602，exit 0；一条 crossterm `unused_parens` warning 原样保留。rustc/cargo 均 1.94.0，native aarch64，详见版本原文。
- 新产物：工作根 `target/release/zenpi`，6072768 字节，SHA-256 `19d5887b424ef1bc53f8ed3793c0c7982c4fea57a80ac3c758ef50f4edf7a9ad`；device 16777232、inode 12838807、mtime_ns 1789227361998587865。`binary/zenpi` 是运行之后保存的同字节数据副本，不能当作另一轮首启样本。
- 唯一命令：`/usr/bin/time -l <工作根>/target/release/zenpi --mode headless --session /var/folders/zh/sdgyp1m967jc7703v5f9pzw40000gn/T/zenpi-bench-6_j9ow4h/session-0.jsonl`；cwd `/Users/wangweiyang/GitHub/zenpi`。
- stdin 固定 38 字节 shutdown / `bench-stop`，与原 benchmark 一致。沿用原五项 ZENPI 环境覆盖：临时 ZENPI_HOME、openai、`http://127.0.0.1:1`、benchmark-no-network、benchmark-placeholder，另显式保留上述两个 CARGO 环境值。完整覆盖项在 `observation.json`；没有输出其余继承环境或真实凭证。
- `launch-claim.json` 独占写入，记录 intended_launches=1；`observation.json` launch_attempts=1。没有预热、第二次目标执行、重新签名、签名预评估、第二次构建或正式预算重跑。

## 五阶段证据

| 区间 | 含义 | 毫秒 |
| --- | --- | ---: |
| parent launch → M | 父进程 Popen 前到 Rust main 首条标记 | 935.707583 |
| M → H | main、core 初始化至 headless 调用前 | 18.926292 |
| H → R | headless 准备至事件循环前 | 7.558958 |
| R → A | 循环至 shutdown 成功响应写入并 flush 后 | 13.715500 |
| A → X | 响应完成至 main 返回前 | 0.191167 |
| M → X | main 内整体 | 40.391917 |
| X → parent capture end | main 返回前至父进程捕获完毕 | 1.007833 |

`trace.bin` 恰为 160 字节，五个 32 字节记录顺序 M/H/R/A/X，magic `Z117PH01`、保留字节全零、目标 PID/PPID 一致、时间单调。原始 ticks 和逐段运算位于 `phase-validation.json`、`phase-intervals.json`。父子均用 `mach_absolute_time`，同一 boot `FED0AF13-2278-4483-A836-B2FAD5427DA6`，timebase 125/3；此处直接相减有同钟基础。M 是进入 Rust main 后第一条语句的调用，不是内核 exec 或 dyld 的入口，parent→M 包括 wrapper、进程启动、装载、调度、系统策略及其他 main 前活动，不能进一步拆分。

父进程 Popen 耗时 1.599292 ms；A 到父进程看到首个 stdout 换行 0.047375 ms。外层 perf_counter 977.101334 ms 与各 mach 区间和略有差异，因为采样位置不同。`/usr/bin/time -l` 原文为 0.97 real / 0.00 user / 0.00 sys，最大 RSS 5144576 字节、507 page reclaims、0 page faults、0 block I/O、19 voluntary / 54 involuntary context switches。不得把四舍五入后的 CPU 时间当作绝对零 CPU 工作。

stdout 144 字节且只有一条对应 bench-stop 的成功响应，closing/drained 均 true；stderr 777 字节保留完整 time 输出。原临时 fixture 保留，同时复制 session 455 字节和 reconnect journal 935 字节；journal 的 terminal line 与 stdout 完全一致。trace SHA-256 `0f507d17aa66cab9c00a06d8daa90f7717dbc019658823579f89e4d36055003a`。所有流 EOF、无 observer errors、无超时、退出和回收检查均通过。该 passed 仅指诊断证据完整性。

## 新产物系统日志：关联存在，时钟限制必须保留

只读取 UTC+8 2026-09-12 23:38:08 至 23:38:12 的精确新二进制路径/工作根，随后同窗口按已出现的 path token `c4e8a5e276eb24ff` 读取一次。两条 log show 命令均退出 0，各取得 2 与 12 条记录，去重共 13 条。同 boot；系统日志 PID 97597 属于 syspolicyd，不能替代实际目标 PID 97022。

日志 `GK performScan` ticks 48044392946423 到 `GK evaluateScanResult` ticks 48044415118489，相差 22172066 ticks，按记录 timebase 为 **923.836083 ms**。同 token 的 XProtect results 含新产物完整 URL，随后 scan complete、evaluate、provenance、内核 allowed/cache。这支持“新产物首启附近存在较长系统策略扫描区间”的关联。原始日志也明确出现 **`Skipping up front XProtect scan`**，所以不能把整个 923.836 ms 命名为 XProtect 杀毒执行时间。后续 tracking GK violation 记录不等于本次启动被拒；内核 allowed 与目标 exit 0 均保留，不解释未公开的结果码。

**时钟不能直接叠加。** Unified log 的原始 machTimestamp 约 48044…，父子 `mach_absolute_time` 约 27980…，起点不同。运行后只读 ABI 采样显示 continuous 与 absolute 的偏移约 20063768065270 ticks，但这不是启动当时的映射，不能假定偏移全程不变并追溯对齐。日志显示 performScan 墙钟为 23:38:09.653950，父进程 before_popen 为 23:38:09.672235，前者竟早 **18.285 ms**；该不一致也必须保留。因此没有跨两域相减，没有严格区间包含、逐毫秒阶段叠图或确定的因果归因。系统内的 923.836 ms 和父子同钟的 935.708 ms 是两项分开报告的观察。

运行后仅执行 codesign display，得到 adhoc/linker-signed、CodeDirectory 47224 字节、1472 pages，CDHash `8a9f623e16589ebc76feeea20731cd6833f89ce1`。Identifier `zenpi-cdb646c6c40059cd` 与原失败产物相同，而 SHA/CDHash 不同；Identifier 不能唯一识别产物。本次关联依靠完整新路径、token 与 artifact hash，未把相同 identifier 当作相同字节的证明。

## 私有改动和阅读边界

冻结 master 的 155 个输入文件；只改私有 `src/main.rs`、`src/core.rs`、`src/headless.rs`，新增 `src/startup_phase.rs`，共 156 个诊断输入。Cargo、lib.rs、benchmark、原 main、authority、claims 及旧 ready 未由本轮改动。所有 before/after 哈希及完整 patch 均交付。

main 17 行、emitter 19 行、最终 observer 全文及完整诊断 diff 已读。core/headless 只读限定上下文与 diff，不宣称全量 owner 阅读或验收。冻结 core 6073–6129、headless 3750–3802、4482–4520、5000–5029、8770–8822 的上下文解释 H/R/A；尤其 A 所在 writer 链在 write_all、flush 成功后才返回。阅读记录和带行号片段见 `read-evidence`。core 新 module 声明插在 run 文档注释后，使该注释附着 module；这是私有诊断的小范围结构变化，未作为产品补丁提交。

emitter 单次抓取 mach ticks，用栈上 32 字节记录 PID/PPID，再对 fd 198 做一次 libc::write，不重试、不分配堆、不读环境/配置、不做日志或 fsync。观察器使用 **匿名 pipe**，写端 O_NONBLOCK、继承 fd 198，读端持续 drain；总 160 字节。没有创建文件系统 FIFO。

观察器采用 selectors 并行读取三条流，最多 10 ms select tick，与原 benchmark 的 communicate 不同；还增加五次 marker 调用和写入，不能声称观测无扰动或将本次替换正式预算。Popen 返回后设 5 秒 deadline，失败路径进程组 SIGKILL、有限 drain/reap；实际没有触发。Popen 本身不在该 5 秒 deadline 内，本次 Popen 1.599292 ms。观察器的启动前校验或 host helper 同样不属于该 target deadline。超时保护并非对全部 Python 操作的全局墙钟保证。

## 主机并发与实际失败

构建前 15:34:40.899178 UTC，C 的 cargo 94396/rustc 95249 活跃，rustc 快照 100% CPU，cwd 为 ff51 worktree 的 stage132-busy-diff320/build-input。构建后审阅窗口 15:36:48.775890，A 的 cargo 96820 活跃，cwd 为 38de worktree 的 target129-ctrl-u-resize-3.1.20/candidate。launch-before/after 两张筛选快照未见 cargo/rustc，但这不是连续调度轨迹，**不声明 CPU 独占**。出于范围限制，只保留相关进程的名称/PID/PPID/%CPU 与编译 cwd；全 ps 原文只保留字节数和 hash，未保留无关进程内容。

新编译与目标执行各一次，均 exit 0。启动前静态审阅曾增强 observer 异常分支的有限 drain，前版本与 review note 原样保留；该修改发生在唯一执行前，不是失败后重跑。实际发生过一次纯离线 JSON 分析失败：Python 3.9 对 `2026-09-12 23:38:09.653950+0800` 调用 fromisoformat 抛出 ValueError、exit 1。`analysis-attempt1.json` 保留 tool chunk c9c4a2、完整所见 traceback 和已知失败表达式；原始 stdin 全文未单独保存，此证据限制明确记录。改用 strptime 后只重读已有日志，`analyze_offline.py` 成功生成 `system-analysis.json`，没有再运行目标或查询系统。此前原门禁失败、历史失败及旧诊断的离线失败未被覆盖。

## 交付与门禁状态

独立 ready 含本轮原始证据、冻结诊断源码、未执行的数据二进制副本和完整上一轮只读诊断包副本。旧包原 manifest SHA-256 `24edf0ea9aab3adf2e7b87b5ff6a8e98be9e865c64872f8e5940c40f0e8bb34a`，原二进制 SHA-256 `e9ba9476f7699d715ad3b0b5fe4a07aeee9f280508d3cb495d59cfa94fff6449`。原三个样本仍是 1109.425125 / 42.552459 / 42.5525 ms，1000 ms 首启门禁 exit 1、7/8；历史 1044/1208 ms 失败及 751 ms 自然通过记录继续保留，不能用本次诊断的 977 ms 洗掉失败。

运行后校验私有 156 输入与新 binary hash 一致，启动前清单中的旧 ready 3278 文件不变。master 可由其他任务独立演进；首个运行后检查只看到 `tools/generate_stage1_gantt.py` 漂移，最终再检查的漂移清单单独存档，不宣称 master 全局静止。离线 verifier 只校验包哈希、私有 diff 范围、原始 trace/区间、响应/journal、命令原文和历史包，不启动产品。其通过不是代码 owner 接收、正式测试或预算 gate 通过。

本轮没有产品修复、正式测试、HTTP 探测、PTY、文件 FIFO、新任务或子代理。固定 shutdown 不要求网络请求，但未做连续网络监测，不能把“无主动 HTTP 探测”升级为整个主机零网络。后续是否继续调查 main 前系统策略、装载或调度，交由主任务决定；本轮按单次授权交付后停止。
