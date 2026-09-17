# ZS1-306 — slash_command.rs 交互审查

源 `codex-rs/tui/src/slash_command.rs`：8020B / 217L / `9e53ca5bff5a5e7b86aa97731da985082db5d130c453e2d363edb9c5758ebc5a`，单块完整读取 1–217 至 EOF（8020B，低于 256 KiB 上限，无需分块）。

该模块定义 slash command 的类型、名称、描述、参数提示和可用性元数据，供 composer popup、帮助和分发层共享。它只描述命令，不执行命令；参数字符串、权限和副作用由上层 owner 处理。`SlashCommand` 含 42 个变体与 `kebab-case` 序列化，枚举顺序即 popup 展示顺序；`description`、`command`、`supports_inline_args`、`available_during_task` 覆盖全部变体，`is_visible` 以编译期 `cfg`（`target_os`、`debug_assertions`）过滤 `built_in_slash_commands` 的可见列表。源测试 2 个：`stop_command_is_canonical_name`、`clean_alias_parses_to_stop_command`。

命令查找按规范化名称匹配，别名（如 `clean`→`stop`）和编译期 `cfg` 影响可见列表。关键缺口：未知命令、禁用命令和缺失参数必须返回明确的 typed error，帮助文本不能作为实际可用性证明；枚举顺序或平台 `cfg` 变化会静默改变 popup 内容并可能让别名冲突。建议 zenpi 将命令 registry 与执行 owner 分离，拒绝通过字符串拼接绕过参数/schema/policy 检查，并为大小写、别名冲突、禁用命令、额外参数和 popup 选择后迟到事件增加负例。

本轮未运行产品、Cargo、测试、PTY、网络或 runner，未修改主控源码。
