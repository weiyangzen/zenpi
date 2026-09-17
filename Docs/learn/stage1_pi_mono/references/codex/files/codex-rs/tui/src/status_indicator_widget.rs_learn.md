# ZS1-309 — status_indicator_widget.rs 交互审查

源 `codex-rs/tui/src/status_indicator_widget.rs`：15810B / 440L / `e35e3efbd18784b55343366b2001bda08dfc1543f7e4a88700a715562d519cc5`，完整读取 1–440 至 EOF。

该模块只读渲染 composer 上方的状态行：spinner/shimmer 动画（`animations_enabled` 时每 32ms 请求下一帧）、`({elapsed} • esc to interrupt)` 提示、最多 `STATUS_DETAILS_DEFAULT_MAX_LINES=3` 行带 `…` 的 details，以及可选 inline message；窄宽按 `truncate_line_with_ellipsis_if_overflow` 截断。它不拥有任务状态、执行权限或取消决策。

操作映射：`interrupt()` 经 `AppEvent::CodexOp(Op::Interrupt)` 只发出中断请求，不含确认、超时或强制停止；`pause_timer_at`/`resume_timer_at` 只累积单调 elapsed；`update_header`/`update_details`/`update_inline_message`/`set_interrupt_hint_visible` 只改展示状态；`fmt_elapsed_compact` 输出 0s/1m 00s/1h 00m 00s。源内联测试 7 个：`fmt_elapsed_compact_formats_seconds_minutes_hours`、`renders_with_working_header`、`renders_truncated`、`renders_wrapped_details_panama_two_lines`、`timer_pauses_when_requested`、`details_overflow_adds_ellipsis`、`details_args_can_disable_capitalization_and_limit_lines`。

关键缺口：状态更新异步到达时需按 session/generation 丢弃迟到事件；spinner/计时器停止必须与任务终结绑定，避免旧状态持续显示。文本截断、颜色和空状态不能掩盖失败原因；取消、超时、拒绝与完成应使用不同 typed 状态，不能只显示“busy”。建议 zenpi 为状态视图绑定稳定事件 ID、终结态和 monotonic sequence，并覆盖快速成功、失败后重试、取消、超时、重连、窗口 resize 与迟到更新负例。

本轮未运行产品、Cargo、测试、PTY、网络或 runner，未修改主控源码。
