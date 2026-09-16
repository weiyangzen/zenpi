# 扩展管道测试阶段日志 — 主控定向复验

仅 src/extension_runtime.rs 的 Unix test-only 区域增加40行，生产prefix、原C fixture、全部deadline和成功断言逐字节保持。主控逐行复核guard、析构顺序及实际watchdog，验证33个冻结artifact和精确当前基线后合入。父guard在TempDir销毁前按8KiB+1有界读取普通phase文件，拒绝symlink/FIFO，标识截断和不可用；helper入口增加最早test-body marker。此marker不代表OS loader开始。

主控完整crate实际编译后，仅运行原失败escaped_pipe_cancel_is_bounded一次，命令exit0，一项顶层测试覆盖API1/2×stdin/stdout四个实际子案例。四次真实结果均Cancelled，FD4→4，原escaped-holder生存断言通过；每次parent保留329 bytes/8条phase，包含helper-entry、host-start、native-parent-entry、escaped-ready和host-return。未再次运行完整lib门禁。历史83pass/1fail/5ignored依然保留，当前窄测试成功不能证明启动延迟修复。

worker固定before/after故意watchdog负例均真实SIGKILL/reap且parent exit1，新增guard保存94字节阶段后再删临时目录。该负例来自直接抽取候选guard和原watchdog的独立Rust程序，不能冒充主控实际API1/2 watchdog复现。主控这次实际case没有触发watchdog；两个证据范围分开。已审查probe抽取与运行脚本、源和全部artifact哈希，不把negative wrapper成功解释为原失败变成功。

本项只修复可复现的诊断信息丢失，不更改timeout/预热/解释器/产品逻辑，不关闭115/130整项。父进程自身SIGKILL不能执行Drop，不在观测保证内。
