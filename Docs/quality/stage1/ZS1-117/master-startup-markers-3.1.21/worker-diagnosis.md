# ZS1-117 单次启动阶段诊断，3.1.21

单次诊断定位到最大长段：父端Popen开始至Rust main第一项取时为 **1096.025 ms**；main内M0–M8为 **40.610 ms**。父端等待总计1137.568 ms，M8之后至wait返回0.933 ms。它把这一次诊断的主要延迟定位在进入Rust main之前，尚未区分time包装器、调度、映像加载、Rust入口前运行时或系统策略。没有直接证据将其归因签名、macOS或某个产品函数。

生产release的原预算仍然失败，不被本诊断替代。原5f1e513355f4e9aa228ada8ef6213e0255ec2247bd414e13668b44cdaa948588二进制6093328B，三样本1039.683291 / 38.870666 / 42.970375 ms，cap1000ms，max超39.683291ms，7/8门禁通过，exit1。生产binary、budget JSON/log/run及三个原startup记录原样封存；本轮没有执行该binary或更早d0cc，没有生产预算重跑或预热。

## 构建身份、权限范围和完整失败链

主控明确授权冻结diagnostic-design.md中的一次私有分段诊断。全新工作目录 target117-single-markers-3.1.21 复制当前主库225项输入，加README共226文件。与原release记录相比只有tools/generate_stage1_gantt.py从13809B/262934ec…变为13871B/464f98a7…，属于非Cargo输入的主控文案变化；224项其余输入逐字节相同。初次“全部225都必须相同”准备断言因此失败，记录preparation-failure-01.json后按当前输入捕获，未启动目标。最终复核所有当前复制输入仍与主库相同。

仅私有src/main.rs、core.rs、headless.rs、lib.rs和新增startup_diag.rs不同；Cargo.toml/lock、vendor、协议、负载及其它产品源码没有改。diagnostic.patch包含完整差异；source-before/source-diagnostic和输入清单可独立复核。记录模块使用固定9槽AtomicU64时间/TID与AtomicU32调用计数，标记点没有堆分配或文件IO。M8之后才创建独立诊断文件、一次write_all再flush；不占协议stdout，也不增加stdin握手。文件路径固化于此私有构建；源码归档仅供审阅，离线核验不重建或执行它。

build-01 在23:03:19.916871–23:03:40.673161 UTC失败exit101：Rust libc的pthread_t实际为usize，诊断模块误传null_mut指针给pthread_threadid_np。失败源码五件、完整JSON编译输出、stderr、工具链与差异全部保留。修正仅将该参数改为libc::pthread_self()，compile-fix.patch记录唯一一行变化；没有执行失败产物。build-02在23:05:22.422515–23:06:13.335879 UTC exit0。两个构建都使用cargo +stable-aarch64-apple-darwin build --release --locked --offline --bin zenpi --message-format=json-render-diagnostics；没有Cargo test或产品实验。原vendor一条unused_parens警告保留，不顺手修改。

诊断binary为6093552B，SHA **7ce01ad7cef17b8466c2fd2eacd37732ae3507633ac503bfcd7c9e654d95946d**，与生产不同；构建完成、唯一运行前后及封包副本身份一致。编译工具进程不计入“唯一诊断目标进程”；本轮实际诊断target launch恰一次，无重复启动、签名/隔离/缓存/系统策略操作，无131/DTrace/FIFO探针，无HOME/CODEX_HOME修改，无主库产品/权威写入。记录型私有代码不作为产品补丁提交。

## 固定标记与准确的线程含义

M0在真正main函数第一项源级可执行操作调用clock_ns，早于core::run、参数解析、配置与产品初始化；clock_ns内部零初始化timespec后调用系统时钟。它仍不等价于Mach-O加载、OS exec或Rust运行时入口第一条机器指令。M0取时之后才记录TID和槽位，避免先做文件、环境或动态缓冲初始化。

| marker | 实际私有位置 | 已跨越边界 |
| --- | --- | --- |
| M0 | main.rs:4 | 第一项系统单调取时，后续record_at保存 |
| M1 | core.rs:6082 | make_backend成功返回，含此前CLI/config |
| M2 | core.rs:6092 | SessionStore::open、Agent构造、acknowledge_recovery返回 |
| M3 | core.rs:6125 | 工具、资源、扩展、model选择完成，调用run_stdio_owned之前 |
| M4 | headless.rs:4511 | pool/replay/live owner初始化、stdin与BackgroundRunner创建调用返回，主循环前 |
| M5 | headless.rs:6348 | main线程已匹配Command::Shutdown、保留ack并设stopping=true |
| M6 | headless.rs:5083 | RuntimeEvent::Closed分支的write_cached_versioned_response成功返回 |
| M7 | headless.rs:4751 | shutdown_and_join返回，replay.close/close_async_owners闭包也返回 |
| M8 | main.rs:13 | core::run及其返回收尾结束，诊断文件输出与返回ExitCode之前 |

M6的真实调用链是write_cached_versioned_response → write_cached_response → remember_terminal（如有id）→ output.write_all → output.flush → 返回；因此M6确实晚于响应写入和flush，不拿入队当flush。M7在join和close_result闭包之后，唯一运行成功exit0也证明正常路径的结果检查通过。

实测M0–M8的TID全部为30184765；这些全部是主线程标记。没有给后台工作线程插入或伪造ready标记。M4只表示spawn调用返回，不说明stdin线程或runtime线程何时真正开始执行；M5是主线程处理完成的边界，不是stdin线程读到第一字节的时刻。read-ranges.json、marker-locations.json和完整私有差异保存实际阅读范围/行号，未虚称全读巨大core/headless文件。

## 同一时钟及唯一运行合同

父端显式time.clock_gettime_ns(time.CLOCK_MONOTONIC)，子端显式libc::clock_gettime(libc::CLOCK_MONOTONIC)，子端换算tv_sec*1_000_000_000+tv_nsec。当前Python常量、SDK _time.h枚举与实际libc0.2.189定义均为6；静态声明及完整来源文件已捕获。没有使用Instant、perf_counter或墙钟相减。墙钟UTC只记录回执起止，整数纳秒阶段计算全部来自CLOCK_MONOTONIC；字段纳秒单位不宣称硬件精度达1ns。

实际目标PID99639，ppid99638，和父Popen返回的time包装器PID99638相等；父Python PID99637。clock_errors/thread_errors均0，9槽各一次、非零且顺序单调，M0–M8在父launch/wait范围内。父子处于同一次本机进程树的同一单调时钟域，跨进程差值的这组前提均成立。没有目标exec、内核退出或调度切换时间，因此不把父观察锚点改名成OS事件。

唯一执行23:06:24.854000–23:06:25.991752 UTC：原/usr/bin/time -l包装，原cwd=/Users/wangweiyang/GitHub/zenpi，原headless/session参数和单条 `{type:shutdown,id:bench-stop}` JSONL字节。新mkdtemp前缀zenpi-bench-，session-0.jsonl和ZENPI_HOME最初均不存在；仅保留原5个ZENPI覆盖，继承环境没有其它ZENPI键，HOME/CODEX_HOME不变。安全快照只记录枚举/键名、文件字节hash及JSON键结构，不保存用户凭据或载荷内容。

父端使用selector观察stdout/stderr首读、末读、EOF和包装器退出，并写入同一原始shutdown字节后flush/close stdin。它不同于原communicate实现，且诊断文件输出/新增取时本身会扰动，因此本结果不能与原budget计时混为同一测量。Popen返回耗时1.635ms；响应首次被父端观察在M6后0.046ms，不能将此当作传输延迟的精确测量。看见字节与目标flush完成是两个事件。

5秒watchdog未触发，包装器returncode0且已wait回收；没有再启动。stdout完整144B，恰一条bench-stop成功closing/drained响应，SHA25ec3e42f157458bc2bef231093a547f8ca61277aea7a7400a24075c4f67a8af，与原生产样本stdout相同。stderr完整777B，time报告1.13real/0.00user/0.00sys、RSS5324800B；不以舍入CPU或RSS推断原因。原始markers342B和父端所有锚点完整保留。

## 实际阶段表

| 区间 | 毫秒 | 能说明的范围 |
| --- | ---: | --- |
| parent launch → M0 | 1096.025 | time包装/调度/加载/Rust main入口前的合计，不能继续分解 |
| M0 → M1 | 2.814 | CLI/config/backend |
| M1 → M2 | 8.991 | session/open/Agent/recovery |
| M2 → M3 | 6.324 | tools/resources/extensions/model |
| M3 → M4 | 7.473 | stdio owner/replay initialization and spawn return |
| M4 → M5 | 7.447 | main-loop/channel command handling; not worker-ready latency |
| M5 → M6 | 7.443 | shutdown dispatch/runtime close/persisted ack+flush |
| M6 → M7 | 0.047 | post-ack runtime join plus replay/owner close |
| M7 → M8 | 0.071 | remaining core return/main result |
| M8 → parent wait | 0.933 | diagnostic file dump, runtime/OS exit, time wrapper and parent observation |

阶段和恰为1137.568ms。大段1096.025ms位于M0之前，约占父观察区间96.35%；M0–M8共40.610ms。不能从中扣除该1096ms并宣布原cold-start达标，不能将不同binary/不同时间的原1039.683291ms和本1137.568ms相减作归因。此结果只证明本诊断实例的分段。

M1–M2的8.991ms是main内最大一段，M3–M6三个阶段约7.4ms；均远小于入口前长段。没有理由在本轮跳过配置/资源、改shutdown常量、改durable写盘或修改产品逻辑。M6–M7仅0.047ms，不支持将这一次约一秒长段解释成ack之后join/close等待。没有对任意其它运行或其它负载作该保证。

## Fixture、状态与具体下一步

主库cwd下.zenpi在前后均不存在；未采集用户全局配置/秘密。fresh fixture开始为空，结束有session-0.jsonl 455B和.reconnect 935B，身份/键结构见fixture-after.json；不存在新zenpi home目录。原fixture留在回执中的临时路径，未删除或修改，raw内容不复制入便携包。运行前后snapshot是计时区间外工作。最终主库226捕获输入、生产binary、工作树非.ops的94项原状态、旧117/057manifest均保持身份。

下一最小问题是将“父端launch→Rust main”拆开为包装器启动目标、目标exec/映像映射、目标获得调度与Rust入口前运行时。下一轮只有在存在平台允许且可绑定目标PID、可执行SHA和同一时钟的exec/线程状态/映像加载观测机制时才采集该时间线；先静态核对可观察字段与权限，不能重试已拒DTrace或用仅按路径的旧policy日志定责。若拿不到身份明确的机制，应保留入口前合计未分解。当前数据不支持直接提出产品修复或系统策略修改。

本轮到此停止目标执行。新便携verify.py只读封存源码、编译/运行回执、raw标记和字节，重算阶段与核验差异；不运行Cargo、诊断binary、生产binary或workflow-source里的任何程序。不接受117、不回填预算、不撤销原失败。所有编译/准备失败原样保留，主控可独立审阅。
