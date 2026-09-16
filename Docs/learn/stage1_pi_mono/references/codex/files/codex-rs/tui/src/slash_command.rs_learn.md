# ZS1-306 — slash_command.rs 交互审查

源 `codex-rs/tui/src/slash_command.rs`：8020B / 314L / `9e53ca5bff5a5e7b86aa97731da985082db5d130c453e2d363edb9c5758ebc5a`，读取 1–300、301–314 至 EOF。

该模块定义 slash command 的类型、名称、描述、参数提示和可用性元数据，供 composer popup、帮助和分发层共享。它只描述命令，不执行命令；参数字符串、权限和副作用由上层 owner 处理。

命令查找通常按规范化名称匹配，别名和 feature flags 影响可见列表。未知命令、禁用命令和缺失参数应返回明确的 typed error；帮助文本不能作为实际可用性证明。建议 zenpi 将命令 registry 与执行 owner 分离，拒绝通过字符串拼接绕过参数/schema/policy 检查，并为大小写、别名冲突、禁用命令、额外参数和 popup 选择后迟到事件增加负例。

本轮未运行产品、Cargo、测试、PTY、网络或 runner，未修改主控源码。
