# ZS1-303 — approval_overlay.rs 交互审查

源文件 `codex-rs/tui/src/bottom_pane/approval_overlay.rs`：57262B / 1556L / `c924923b402b198f8f77b249f0d37892896c0b84002c0f6e48330917da80cecc`，读取 1–800、801–1556 至 EOF。该模块负责 TUI 审批 overlay 的布局、焦点、选项和结果事件。

Overlay 将待审批请求投影为可滚动文本，展示 command、路径、权限说明、风险提示和可用动作。焦点在 allow/deny/always 等选项间移动；Enter/Space 选择，Esc/取消关闭或返回上层。键盘处理只产生审批结果/事件，由上层 approval owner 执行策略、记录决定并继续或拒绝工具调用。

状态包含请求列表、当前索引、滚动偏移、焦点项、是否显示详细说明及 pending 状态。渲染根据可用宽高截断长 command/path，不能将显示截断当作安全裁剪。Overlay 自身不执行 shell、不访问文件、不验证路径 containment，也不证明调用者身份；权限说明依赖上层传入的结构化 request。

批量审批、重复请求和关闭重开依赖调用方维护 request id 与 generation。若上层在 overlay 关闭后未绑定 generation，迟到的 approve 事件可能落到新请求；若列表更新时索引未重置，焦点可能指向错误条目。建议 zenpi 的审批 owner 使用 request-id/generation 绑定、默认 deny、一次性决定与持久 receipt，并在异步取消和重连后拒绝旧事件。

当前模块的 UI 结果不能替代 policy gate：allow 只表示用户在该 overlay 选择允许，仍需重新检查路径、工具 schema、预算和 workspace ownership；always 选项应限定到明确 scope，不能扩大为全局永久授权。Esc/窗口关闭/取消应保留未决状态，不应隐式 allow。建议覆盖空列表、长文本、滚动边界、重复/迟到 request、焦点切换、deny 默认值、取消后重开和批量审批隔离负例。

本轮未运行产品、Cargo、测试、PTY、网络或 runner，未修改主控源码。
