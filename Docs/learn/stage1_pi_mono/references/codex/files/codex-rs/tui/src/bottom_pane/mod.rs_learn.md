# ZS1-305 — bottom_pane/mod.rs 交互审查

源 `codex-rs/tui/src/bottom_pane/mod.rs`：72998B / 1974L / `97af02eed39dbfdf69d1ec3e14dc13b9a91f4b33f5e4b985cc1f2cbae4105823`，读取 1–1000、1001–1974 至 EOF。

该模块组合 ChatComposer、审批 overlay、pending approvals、命令/文件/技能弹窗、footer 和底部 pane widget，负责焦点状态、输入路由、布局渲染、任务运行状态及 AppEvent 转发。它是 UI 编排层，不是权限或执行 owner。

关键边界：多个 popup/overlay 同时有 pending 状态时，焦点优先级必须稳定；关闭 overlay 后应恢复原 textarea 光标和滚动。异步 AppEvent 必须按 session/thread/generation 归属，迟到事件不能更新新 pane。面板 resize、空内容和长错误文本会改变可见区域，渲染截断不能替代数据完整性。审批结果、取消、提交、queue 和 shell/tool 事件都应保持显式 typed action，不能通过字符串猜测。

建议 zenpi 将 TUI 视图状态与 domain/session owner 分离：面板只派发 typed intent，owner 返回带 request/session generation 的结果；所有焦点切换、overlay 关闭、重连和窗口 resize 采用可恢复快照；为同时存在审批、命令弹窗、文件搜索和输入禁用状态增加优先级矩阵与迟到事件负例。

本轮未运行产品、Cargo、测试、PTY、网络或 runner，未修改主控源码。
