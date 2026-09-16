# ZS1-121 candidate report

顶部+目录picker候选：15个TUI项目测试、2个配置测试及真实release PTY通过；验证顶部+、Esc/无效路径不建空tab、Unicode/空格路径、同basename隔离、两目录shell写入、切换/重启/晚到输出归属与terminal恢复。需要主控逐文件阅读及独立证据闭包。

状态：candidate_only；没有修改 blueprint checkbox，也没有声明 master accepted。

当前源码 hash 与运行日志见 `candidate-manifest.json` 及同目录测试/PTY日志。

## 增量 tab-close 证据

最新 release PTY 项目 smoke 已增加并通过 `Ctrl-W closes tab and cwd reopens journal`：空提示符 Ctrl-W 关闭当前 tab projection，保留 session journal，再通过同一 cwd 重新打开；运行中的 owner 仍走关闭拒绝边界。此增量不改变 BentoBox，仍待依赖闭包和主控正式验收。
