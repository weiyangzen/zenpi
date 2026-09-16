# ZS1-307 — file_search.rs 交互审查

源 `codex-rs/tui/src/file_search.rs`：4009B / 133L / `7aaa33ac7fd28cbe5fa3405cd3490b30e614272122bb572c9c352acc418b71d9`，完整读取 1–133 至 EOF。

该模块封装文件搜索 popup 的结果项、排序/选择状态和键盘导航数据。搜索结果供 composer 插入路径或图片附件；模块本身不执行命令、不授予路径权限。

关键缺口：搜索结果可能来自异步扫描，旧 query 的迟到结果必须按 query/generation 丢弃；选择索引在结果刷新、空结果和窗口 resize 时要复位。显示路径与实际路径需明确区分，不能把 UI 文本当作已验证 containment；图片识别和 metadata 读取失败应回退为普通路径而不吞错。建议覆盖 query 竞态、空结果、重复路径、排序稳定性、长路径截断、目录逃逸输入、取消/重开和迟到选择事件。

本轮未运行产品、Cargo、测试、PTY、网络或 runner，未修改主控源码。
