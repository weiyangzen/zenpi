# ZS1-302 — paste_burst.rs 交互审查

源 `codex-rs/tui/src/bottom_pane/paste_burst.rs`：24594B / 572L / `c80558dfd3cbde437f2fec9a855b01a3180542a53bb135dc92ce7c81299f1ee7`，完整读取 1–572 至 EOF。该模块把无 bracketed-paste 的快速按键流转换为显式粘贴或普通输入决策。

`PasteBurst` 保存启用标志、首字符、累计字符、最近时间、Enter 事件和 flush deadline。ASCII 首字符短暂延迟以抑制粘贴期间快捷键闪烁；非 ASCII 首字符立即交给输入以避免 IME 首字丢失，同时继续允许后续 burst 识别。快速 Char/Enter 序列可在 flush 时合并为 paste，普通慢速输入则按 typed key 处理。禁用 burst 会清理进行中的状态，避免残留字符泄漏到后续输入。

`CharDecision`/`FlushResult` 只描述输入分类和待处理文本；实际插入、占位符、历史、附件和提交由 ChatComposer 完成。该模块没有 shell、网络、文件或 agent 调用。定时器依赖调用方 tick；如果 tick 停止，pending burst 会延迟到下一次 flush。取消、focus 切换和 popup 优先级由上层决定。

关键缺口：突发判定依赖时间阈值和事件顺序，无法单独证明真实 bracketed paste 边界；Enter 混入时需验证不会误提交。Unicode grapheme、组合字符和 IME 事件不能只按 char 数量推断；长 burst 的内存和文本上限由上层 composer/协议限制。建议在 zenpi 中采用显式 paste event 优先、对无 bracketed paste 使用有界缓冲与 deadline，并为慢速 ASCII、快速 ASCII、非 ASCII IME、Enter 混入、禁用后残留、取消/focus 切换和超长 burst 增加正负测试。

本轮未运行产品、Cargo、测试、PTY、网络或 runner，未修改主控源码。
