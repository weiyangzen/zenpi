# bottom_pane 目录整合报告

冻结目录 `codex-rs/tui/src/bottom_pane`（`reference:codex`，HEAD `b3b3d262787f4902a7449f17d793241a34d311ad`）的物理直属库存为 28 个 `.rs` 文件、`AGENTS.md` 以及 `request_user_input/`、`snapshots/` 两个子目录；`mod.rs` 共声明 28 个同级模块（含 `request_user_input`）外加一个 `tests` 模块。蓝图冻结的直属子项恰为 6 个文件：ZS1-300 `chat_composer.rs`、ZS1-301 `textarea.rs`、ZS1-302 `paste_burst.rs`、ZS1-303 `approval_overlay.rs`、ZS1-304 `pending_thread_approvals.rs`、ZS1-305 `mod.rs`；本目录内不存在冻结的直属子目录，因此本目录的直属闭包即这 6 项。父目录 `codex-rs/tui/src` 的 ZS1-306 `slash_command.rs`、ZS1-307 `file_search.rs`、ZS1-308 `key_hint.rs`、ZS1-309 `status_indicator_widget.rs` 归属 ZS1-353，本项只作为 context 读取，不计入本目录覆盖。

| 子项 | 源文件 | 源字节 | 源 SHA-256 | 文件级报告字节 |
| --- | --- | --- | --- | --- |
| ZS1-300 | chat_composer.rs | 374943 | 234189c66f50c8654ef72e86ad5463eea5dd1e0c09c98f8d64c898449161b6da | 8694 |
| ZS1-301 | textarea.rs | 91601 | 8cb5241c9e818210bfff63975e703f45a25a3da91bdb395667893903f7cb4b45 | 2106 |
| ZS1-302 | paste_burst.rs | 24594 | c80558dfd3cbde437f2fec9a855b01a3180542a53bb135dc92ce7c81299f1ee7 | 1785 |
| ZS1-303 | approval_overlay.rs | 57262 | c924923b402b198f8f77b249f0d37892896c0b84002c0f6e48330917da80cecc | 2040 |
| ZS1-304 | pending_thread_approvals.rs | 4105 | a401d7c6ba3051fcf2c20152a9e86a3f4f8751a87470cbc7f518001326618a2f | 28429 |
| ZS1-305 | mod.rs | 72998 | 97af02eed39dbfdf69d1ec3e14dc13b9a91f4b33f5e4b985cc1f2cbae4105823 | 1425 |

其余直属文件（footer、command_popup、file_search_popup、skill_popup、slash_commands、chat_composer_history、scroll_state、bottom_pane_view、pending_input_preview、prompt_args、unified_exec_footer、list_selection_view/selection_popup_common/multi_select_picker 等选择视图、mcp_server_elicitation、app_link_view、custom_prompt_view、experimental_features_view、feedback_view、popup_consts、skills_toggle_view、status_line_setup，以及 `request_user_input/`、`snapshots/` 子目录）未在本项全文复核，不能宣称目录完整覆盖。

已读文件的跨模块调用链：`mod.rs` 组合 pane 与 popup/overlay；`chat_composer` 管理 TextArea、PasteBurst、history、slash/file/skill popup 和附件；`approval_overlay` 与 `pending_thread_approvals` 只投影/维护待决请求，实际 policy、session/generation、执行与 receipt 属于上层 owner；`file_search` 为路径候选，`slash_command` 为命令元数据，`key_hint` 为显示绑定，`status_indicator_widget` 为状态投影。调用边界要求所有异步事件携带 session/thread/generation 或稳定 action/request id。

目录级关键缺口：焦点优先级（textarea、command/file/skill popup、approval overlay、remote image row）没有统一状态矩阵；窗口 resize/空列表/关闭恢复必须保持光标、滚动和 pending request 一致；迟到事件不能更新新 thread 或复用旧 generation；UI 文案、路径显示和快捷键提示不能代替 schema/path/approval/budget 校验。建议目录 owner 提供 typed intent→owner result 投影、统一 generation 绑定、默认 deny 与取消/超时终结态，并为跨模块组合增加负例测试。

本报告是目录整合候选 `[_]`：其冻结直属子项 ZS1-300..305 在蓝图 `Docs/stage_1_v3_pi_mono_blueprint.md` 中已标记 `[x]`，本报告仍待主控按 G-DIR 独立审阅后方可接受。本项未运行产品、Cargo、测试、PTY、网络或 runner，也未修改主控源码；撤回只影响本目录报告。
