# ZS1-308 — key_hint.rs 交互审查

源 `codex-rs/tui/src/key_hint.rs`：3306B / 112L / `07f34aab630b2658560d2d4e73719481ec8638e33eb38401d7a23643689f13c8`，完整读取 1–112 至 EOF。

该模块定义快捷键绑定、显示标签、修饰键格式化和帮助提示数据。它负责把 typed key 映射为 UI hint，不执行命令，也不决定权限。

关键缺口：同一按键多绑定、平台差异、用户自定义覆盖和 popup 优先级可能导致显示提示与实际路由不一致；Help 文案不能证明 handler 存在或功能已启用。建议 zenpi 为每个 hint 绑定稳定 action id、owner capability 和 enabled predicate，检测冲突并在 macOS/Linux/Windows 修饰键显示与实际事件路由之间做一致性测试。

本轮未运行产品、Cargo、测试、PTY、网络或 runner，未修改主控源码。
