# bottom_pane 目录整合报告

目录直属文件共 26 个 Rust 模块及 `AGENTS.md`、snapshots/request_user_input 子目录。当前已完成并可绑定的文件级报告为 ZS1-300..309：`chat_composer.rs`、`textarea.rs`、`paste_burst.rs`、`approval_overlay.rs`、`pending_thread_approvals.rs`、`mod.rs`、`slash_command.rs`、`file_search.rs`、`key_hint.rs`、`status_indicator_widget.rs`。其余直属模块（footer、command_popup、file_search_popup、skill_popup、slash_commands、history、scroll_state、bottom_pane_view、pending_input_preview、prompt_args、unified_exec_footer、selection views、elicitation 等）未在本项全文复核，不能宣称目录完整覆盖。

已读文件的跨模块调用链：`mod.rs` 组合 pane 与 popup/overlay；`chat_composer` 管理 TextArea、PasteBurst、history、slash/file/skill popup 和附件；`approval_overlay` 与 `pending_thread_approvals` 只投影/维护待决请求，实际 policy、session/generation、执行与 receipt 属于上层 owner；`file_search` 为路径候选，`slash_command` 为命令元数据，`key_hint` 为显示绑定，`status_indicator_widget` 为状态投影。调用边界要求所有异步事件携带 session/thread/generation 或稳定 action/request id。

目录级关键缺口：焦点优先级（textarea、command/file/skill popup、approval overlay、remote image row）没有统一状态矩阵；窗口 resize/空列表/关闭恢复必须保持光标、滚动和 pending request 一致；迟到事件不能更新新 thread 或复用旧 generation；UI 文案、路径显示和快捷键提示不能代替 schema/path/approval/budget 校验。建议目录 owner 提供 typed intent→owner result 投影、统一 generation 绑定、默认 deny 与取消/超时终结态，并为跨模块组合增加负例测试。

本报告是目录整合候选 `[_]`，依赖 ZS1-300..309 的 master `[x]` 后方可接受；本项未运行产品、Cargo、测试、PTY、网络或 runner，也未修改主控源码。
