# ZS1-115 pipe_deadline_tests 修复候选 — 关联 ZS1-117 门禁，3.1.20

候选只修改 `src/extension_runtime.rs` 的 `#[cfg(all(test, unix))] mod pipe_deadline_tests`。按 blueprint owned_paths 归属 ZS1-115，解决 ZS1-117 门禁暴露的问题；当前为 provisional、未主控接受。生产部分逐字节不变；不宣称 115 或 117 已验收。38de 本来没有该文件，因此在新建私有冻结构建目录中验证，并交付主库精确 before → after 单文件增量 patch，未向 main 或其他 owner 写文件。

## 诊断结论

原先的夹具每场景临时编译 C 程序，并在被测 process_request 的 800ms 截止时间内执行这个新文件。编译已经在外层计时之前完成，但这不保证该新文件被 exec 后能及时进入 main。历史全库日志为 90 passed / 1 failed / 5 ignored；API1 stdout 的失败日志只有 helper-entry、host-start、host-return，没有 native-parent-entry 或任一 pid 文件。返回为 CommandTimeout(800)，外层约 932ms。这是逃逸前置条件未成立的证据，不是生产超时机制失效的证据。

本次不变源码的隔离相关测试一次运行，复现同一 API1 stdout 失败：3 passed / 1 failed / 1 原有 helper ignored；该场景 host-start 到 host-return 约 801ms，外层 809ms，仍无 native entry。独立首次执行同一 C 源的诊断也在约 803ms 未看到入口，随后按观察上限清理。把夹具改为新建 Python shebang 可执行脚本仍失败（2 passed / 2 failed / 1 ignored）。这些结果都保留，没有重试原实现直到变绿。

最终改为直接启动已安装的 `/usr/bin/python3`，把新建的私有 `plugin` 脚本作为参数读取，不把该新 inode 传给 exec。脚本仍 fork、子进程 setsid、按 stdout/stdin 模式关闭相反端点、写 pid/阶段日志、持有目标管道 10 秒，父进程等待逃逸 pid 再退出。解释器异常、缺失、启动过慢或未逃逸仍会使测试失败。没有预热、重试或新增 skip。

证据可将原因定位到“新文件执行至夹具入口的延迟”，但没有系统追踪来确定是 macOS 签名/安全扫描、调度还是其他内部机制，因此不把其中任何一个当作已证实根因。直接解释器路径在当前 macOS 实验解决了该前置条件，不能据一次运行宣称所有宿主都无抖动。

## 测试语义与范围

保留全部 12 场景：API1/2 × stdout/stdin 的 timeout/cancel，以及 API2 × stdout/stdin 的 revoke/unwind。保留 800ms timeout、1000ms 其他模式、200ms cancel 条件、1500ms inner 上限、2s outer watchdog 和 500ms 清理 grace。保留 900000 字节 stdin 负载、真实 process_request 调用、lease、escaped-ready 检查、直接子进程已回收、逃逸 holder 存活/为 session leader、FD 数不增长，以及所有原有返回值断言。

夹具仍加载私有目录的合法 catalog，随后仅在测试内 clone LoadedExtension，将 executable 和 args 指向已有解释器及私有脚本。这意味着该管道单元测试不证明“从 catalog 加载后直接执行原 executable 字段”或 catalog 的路径限制端到端行为；生产 catalog 和 process_request 没有改动。新增测试前置条件是 `/usr/bin/python3`（脚本需要 Python 3.7+ 的 clock_gettime_ns，实测版本见 interpreter-version.log）。Linux、没有该路径的 Unix、全库与生产可执行扩展兼容性需 master 后续验证。

## 所有运行结果

各命令的 argv/cwd/时间/退出码/输出 SHA 在同名 `.run.json`，原始输出在 `.log`。run-source-bindings.json 将每次测试绑定到保留的具体源码，build-context-capture.json 列出 128 份冻结编译输入。全部 Cargo 使用 native `+stable-aarch64-apple-darwin`、`--offline --locked` 和独立 CARGO_TARGET_DIR；没有 PTY/HTTP/网络，也未改 HOME/CODEX_HOME。

| 运行 | 结果与解释 |
| --- | --- |
| before-build | exit 0，仅相关 lib test 编译，非行为通过 |
| before-tests | exit 101，3 passed / 1 failed / 1 原有 helper ignored，同一 API1 stdout 未逃逸 |
| after-tests（attempt1-shebang.rs） | exit 101，2 passed / 2 failed / 1 ignored；未采用的执行方式，失败原样保留 |
| direct-interpreter-tests（after.rs） | exit 0，4 passed / 0 failed / 1 原有 helper ignored；12 个真实独立子进程场景全部运行 |
| fixture-fmt | exit 0，rustfmt 只检查本文件 |
| negative-startup-delay-tests | exit 101，预期负例；在最终夹具 Python 入口前加 2 秒 sleep，四组 timeout 全部以 fixture never escaped 失败，实际仍为 CommandTimeout(800) |
| toolchain / interpreter-version | exit 0，执行完成后记录工具链与解释器版本，不用于预热 |

最终 12 场景在 host-start 后 93.494–115.243ms 写出 escaped-ready；四 timeout 的 outer 时间为 812/827/817/839ms，inner 为 800/821/803/830ms。cancel/revoke/unwind 的 outer 为 213–224ms。全部 outer_timeout=false，原有 FD/存活/回收断言通过（有一组 FD 为 6→6，其他 4→4，不把绝对 FD 数写死为 4）。case-timings.json 是从完整日志派生的索引。

1 个 ignored 是原有 pipe_probe_child 入口，父测试在每场景通过 `--exact ... --ignored --nocapture` 实际调用它；并未忽略四个验证父测试。负例证明“API 返回超时但夹具未逃逸”仍然失败，不被改成通过。negative-startup-delay.rs 不是候选；负例后构建上下文已恢复到 after.rs，其余 127 份输入保持原始哈希。没有复用负例二进制充当最终候选成功证据。

独立启动诊断的完整原件在 startup-diagnostic.tar.gz。C 编译 exit 0，native-first 在 800ms 观察界内未进入；Python 直接首次执行约 23ms 见 escaped-ready，结束码 -9 来自有意清理，不能当作生产返回。Perl 探索在写出 ready 后的 killpg 清理遇到 PermissionError，退出码未完成记录；以 incomplete 明确保留，不作为通过依据、不重试。该 holder 自带 10 秒寿命。诊断脚本及失败说明原样保存，未用这一不完整探索支撑最终验收。

## 精确接收与复验

before：40084 字节 / 1141 行 / SHA-256 `7526689d532d21e31fee36ab703909a191b08c55b3e811b1a701614be0dc0297`，完整连续读取。after：38357 字节 / 1093 行 / SHA-256 `002169d07e9d41a3f4f2526c5bbc3b2ecb63e4f6a648e37adefb7f98edf727aa`。candidate.patch 只有该源路径。scope.json 记录主库 before 核对时间；若主库该文件变化，应重新精确核对，不用模糊上下文强套。

`python3 verify.py` 仅离线验证文件、日志、构建归档、生产字节/原有断言 sentinel、负例数据和 Git 正反向 apply check/实际往返；不是重新执行产品测试。manifest.json 绑定全部证据文件与 patch，application-verification.json 记录独立临时 Git 基线的往返结果。

master 可在独立目录解包 baseline-build-context.tar.gz，选择新的独立 CARGO_TARGET_DIR，以 `cargo +stable-aarch64-apple-darwin test --offline --locked --lib extension_runtime::pipe_deadline_tests -- --nocapture` 复验 before；对精确 before 应用 candidate.patch 后以同命令复验 after。未缓存依赖或解释器缺失应明确失败。全库检查由 master 决定，本候选未跑全库，也不覆盖 Linux、发布预算、真实用户配置或真实扩展。
