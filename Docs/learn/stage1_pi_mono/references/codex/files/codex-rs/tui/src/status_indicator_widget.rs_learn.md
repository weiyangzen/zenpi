# ZS1-309 — status_indicator_widget.rs 交互审查

源 `codex-rs/tui/src/status_indicator_widget.rs`：15810B / 440L / `e35e3efbd18784b55343366b2001bda08dfc1543f7e4a88700a715562d519cc5`，完整读取 1–440 至 EOF。

该模块渲染底部状态指示器、spinner、进度/等待文本和错误提示，将运行状态投影为 TUI 小部件。它不拥有任务状态、取消或执行权限。

关键缺口：状态更新异步到达时需按 session/generation 丢弃迟到事件；spinner/计时器停止必须与任务终结绑定，避免旧状态持续显示。文本截断、颜色和空状态不能掩盖失败原因；取消、超时、拒绝与完成应使用不同 typed 状态，不能只显示“busy”。建议 zenpi 为状态视图绑定稳定事件 ID、终结态和 monotonic sequence，并覆盖快速成功、失败后重试、取消、超时、重连、窗口 resize 与迟到更新负例。

本轮未运行产品、Cargo、测试、PTY、网络或 runner，未修改主控源码。
