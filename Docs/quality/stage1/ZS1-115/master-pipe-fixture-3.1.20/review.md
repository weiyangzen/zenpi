# 扩展管道夹具修复 — 主库完整 lib 复验，3.1.20

主库执行 `cargo +stable-aarch64-apple-darwin test --offline --locked --lib -- --nocapture`：96 passed、0 failed、5 ignored。此前 90 passed / 1 failed / 5 ignored 的 API1 stdout 超时夹具失败，在当前主库完成复验；新增通过计数还包含上一轮加入的 5 个附件测试。本轮没有把 worker 或 subprocess helper 的重复输出累计为新的顶层测试数。

修复只改变 `src/extension_runtime.rs` 的 Unix 测试夹具。原方案每场景编译新的 C 可执行文件，其首次 exec 可能在进入 main 前耗尽 800ms，尚未建立 escaped pipe 场景。候选改为直接启动已安装 `/usr/bin/python3`，读取私有脚本数据，保留 fork、setsid、目标管道持有及所有时序断言。没有预热、重试、延长门限或跳过失败场景；主库候选全库测试只运行一次。

## 独立复核

主控连续完整读取修改前 1141 行，范围 1–230、231–470、471–720、721–930、931–1141，再完整阅读单文件候选差异和离线 verifier。核对 36 项 worker 原始证据摘要、128 份历史构建输入、全部历史命令/失败退出码以及独立 Git 正向/逆向实际应用和 sentinel。离线验证明确区分证据一致性与运行验证，之后才在主库应用精确补丁。

`#[cfg(all(test, unix))] mod pipe_deadline_tests` 之前生产字节完全一致。除测试内 clone LoadedExtension 并改 interpreter/args、调用借用方式外，原 runner、12 场景矩阵、800/1000ms 超时、200ms 取消条件、1500ms inner 上界、2s outer watchdog、500ms cleanup grace、900000B stdin 输入和全部原断言保持原字节。其它 196 份采集构建输入也保持相同摘要。

## 当前主库结果

12 个子进程场景全部 exit 0、outer_timeout=false：API1/2 × stdout/stdin 的 timeout/cancel，API2 × stdout/stdin 的 revoke/unwind。四个 timeout outer 为 896、891、819、829ms，均得到 CommandTimeout(800)。每场景都实际进入 escaped-ready，直接子进程已回收，逃逸 holder 仍存活且为 session leader，host FD 数前后一致。详细原始输出及 PID/阶段日志见 `full-library.log`，派生索引见 `case-timings.json`。

5 个 ignored 保持原设置；其中 pipe_probe_child 是父测试通过 `--ignored --exact` 实际调用的辅助入口，并非跳过四个管道验证测试。测试内预期 panic 被 catch_unwind 验证，不是顶层测试失败。

主库 Clippy `--offline --locked --lib --tests -- -D warnings` 和单文件 rustfmt 通过。现有 vendor/crossterm 依赖警告保持原样。全部工具运行保留命令、cwd、退出码、日志摘要，HOME/CODEX_HOME 保持原值。

before SHA256：`7526689d532d21e31fee36ab703909a191b08c55b3e811b1a701614be0dc0297`；after：`002169d07e9d41a3f4f2526c5bbc3b2ecb63e4f6a648e37adefb7f98edf727aa`。

## 保留的失败与边界

worker 原 C 方案 3 passed / 1 failed、新 shebang 方案 2 passed / 2 failed 原样保留；只用最终 direct interpreter 版本作候选。worker 的 2s 入口延迟负例仍使全部四种 timeout 场景以 fixture never escaped 失败，不会因 API 返回超时被错误算作通过。主控本轮核验了该负例源码、日志与退出码，没有重新执行负例。独立 Perl 探索的清理 PermissionError 仍明确为 incomplete，不作通过依据。

本次不能证明 macOS 新 executable 延迟的系统根因。测试需要 `/usr/bin/python3` 及 Python 3.7+；未运行 Linux。测试内覆盖的是 process_request 管道所有权，clone 后改 executable 的方式不证明生产 catalog 路径约束。没有新建产品二进制、PTY、HTTP、release 或预算运行，当前源码 release 和其它117完整门禁仍待验证。只关闭这次可复验的 lib 夹具失败，不据此把 ZS1-115、ZS1-117 或整个阶段标成完成。
