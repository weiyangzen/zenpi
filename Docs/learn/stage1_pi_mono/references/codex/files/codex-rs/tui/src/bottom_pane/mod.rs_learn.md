# ZS1-305 — bottom_pane/mod.rs 全文件理解报告

状态：[_]；唯一 owner：`Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/bottom_pane/mod.rs_learn.md`。源 `codex-rs/tui/src/bottom_pane/mod.rs`：72998B / 1974L / `97af02eed39dbfdf69d1ec3e14dc13b9a91f4b33f5e4b985cc1f2cbae4105823`，连续读取 1–700、701–1245、1246–1974 至 EOF，单块覆盖全文件（小于 256KiB，`chunk_manifest.tsv` 无本文件条目）。本报告只把该文件作为 Codex TUI 消费者理解，不转译成 zenpi 已实现能力或安全保证。

## 文件职责与状态

`BottomPane` 是底部输入区的编排容器，持有 `ChatComposer`（可编辑输入）、`view_stack: Vec<Box<dyn BottomPaneView>>`（弹窗/模态栈）、`AppEventSender`、`FrameRequester`、`StatusIndicatorWidget`、`UnifiedExecFooter`、`PendingInputPreview`、`PendingThreadApprovals` 及 `context_window_percent/used_tokens`、`is_task_running`、`has_input_focus` 等状态。文件声明 `chat_composer`、`approval_overlay`、`textarea`、`skill_popup`、`file_search_popup`、`command_popup`、`paste_burst`、`footer` 等子模块并重导出其类型；具体交互语义归子模块（ZS1-300…304、306…309），本文件只做组合、路由、渲染和定时提示。

`CancellationEvent::{Handled,NotHandled}` 是取消协商结果。`QUIT_SHORTCUT_TIMEOUT=1s` 与 `DOUBLE_PRESS_QUIT_SHORTCUT_ENABLED=false` 说明「再按一次退出」提示当前默认关闭。文件不持久化会话、不实现 agent 调度、不授予 shell/权限。

## 焦点与输入路由

`handle_key_event` 分层路由：录制语音时（非 Linux）全部按键先给 composer；`view_stack` 非空时 Release 事件被丢弃，`Esc` 在 `prefer_esc_to_handle_key_event()` 为 false 时先尝试 `view.on_ctrl_c()`，返回 `Handled` 且 view `is_complete()` 才 pop 栈并 `on_active_view_complete()`；否则把事件交给栈顶 view 并按其完成度清栈或安排 paste-burst redraw。栈空时若任务运行、非 `/agent` 命令且 composer 无 popup，`Esc` 触发 `status.interrupt()` 发送 `Op::Interrupt`；否则转发 composer。栈顶视图优先于 composer 是稳定焦点优先级，调用方必须把模态关闭与后续事件顺序绑定。

`on_ctrl_c` 让栈顶 view 先消费；已消费且完成时 pop 并显示退出提示；无 view 且 composer 非空时清空草稿并返回 `Handled`，composer 为空时返回 `NotHandled` 交上层（`ChatWidget`）决定退出。`handle_paste`、`insert_str`、`pre_draw_tick` 同样先视图后 composer，并 `sync_popups` 后请求重绘。`push_approval_request`、`push_user_input_request`、`push_mcp_server_elicitation_request` 先调用栈顶 view 的 `try_consume_*`，消费成功则不新建模态，否则创建对应 overlay、暂停状态计时器、禁用 composer 输入并给出占位提示；模态完成时 `on_active_view_complete` 恢复计时器与输入。

## 渲染与状态视图

`as_renderable` 是渲染入口：有活动 view 时直接借用该 view；否则用 `FlexRenderable` 按顺序组合 status 行、unified-exec summary（仅在无 status 时独立成行）、pending thread approvals、pending input preview 和 composer，并按是否存在预览/状态行插入空行分隔。`render`、`desired_height`、`cursor_pos` 全部委托 `as_renderable`。`set_task_running` 在未运行时创建/保持 `StatusIndicatorWidget` 并同步内联消息，完成时隐藏状态指示器但保留其他模态。`set_context_window`、`set_status_line`、`set_active_agent_label` 只在值变化时请求重绘，避免线程切换抖动。

## 迟到事件与归属边界

本文件不做 session/thread/generation 归属校验：异步结果通过 `on_history_entry_response(log_id, offset, …)`、`on_file_search_result(query, …)`、`push_*` 等入口进入，是否接受由 composer/子模块按 `log_id`、`request_id`、`thread_id` 自行判定。审批和用户输入请求携带 `thread_id`/`request_id`，但本层只保证「栈顶 view 优先消费」；迟到事件不得更新新 pane 的责任落在 owner 侧，需以 request/generation 绑定并拒绝旧事件。

## 源码测试（静态清点，未执行）

`mod tests`（1243–1974）含 19 个 `#[test]`，文件总计 134 个函数。测试分组：Ctrl+C/模态取消（`ctrl_c_on_modal_consumes_without_showing_quit_hint`）；模态与 overlay 层级（`overlay_not_shown_above_approval_modal`、`composer_shown_after_denied_while_task_running`）；状态/footer 高度与快照（`status_indicator_visible_during_command_execution`、`status_and_composer_fill_height_without_bottom_padding`、`status_only_snapshot`、`unified_exec_summary_does_not_increase_height_when_status_visible`、`status_with_details_and_queued_messages_snapshot`、`queued_messages_visible_when_status_hidden_snapshot`、`status_and_queued_messages_snapshot`）；附件/远程图片（`remote_images_render_above_composer_text`、`drain_pending_submission_state_clears_remote_image_urls`）；Esc 与中断优先级（`esc_with_skill_popup_does_not_interrupt_task`、`esc_with_slash_command_popup_does_not_interrupt_task`、`esc_with_agent_command_without_popup_does_not_interrupt_task`、`esc_release_after_dismissing_agent_picker_does_not_interrupt_task`、`esc_interrupts_running_task_when_no_popup`）；路由契约（`esc_routes_to_handle_key_event_when_requested`、`release_events_are_ignored_for_active_view`）。本条未运行任何测试。

## zenpi 操作映射（需求契约，非实现声明）

| Codex 本文件行为 | zenpi 需实现的行为 | 类别 |
|---|---|---|
| 活动 view/模态先于 composer 消费按键 | 同一顶部模态优先路由，关闭后恢复 composer 焦点 | 通用交互 |
| `Esc`：popup→关 popup、运行任务无弹窗→interrupt、`/agent` 文本→编辑 | 中断只在无弹窗且非命令编辑语境触发，需显式 typed action | 通用交互 |
| `Ctrl+C`：模态自退、非空草稿清空、空草稿上报退出 | 取消与退出两段式，state 归属上层，不隐式 quit | 通用交互 |
| 审批/用户输入/MCP elicitation `push_*` 且 `try_consume` | 以 request/session generation 绑定，关闭后拒绝迟到结果 | 通用交互 |
| status 指示器、unified-exec footer、pending 预览 | 渲染截断不替代数据完整性，高度变化需重算 | 通用交互 |
| `on_history_entry_response(log_id,…)`、`on_file_search_result` | 异步回填按 log/query 校验，不得污染新会话 | 通用交互 |
| 语音录制键路由、Windows 降级 sandbox 开关 | provider/平台专属，未接线不得以固定成功抵充 | provider 专属 |
| 退出提示/双按退出实验开关 | 调试/实验开关，默认关闭，不构成产品入口 | 调试命令 |

## 边界声明

本报告基于静态源码阅读，未运行 Cargo、产品、测试、PTY、网络或 runner，未修改主控源码。渲染、键盘时序、跨平台 cfg（Linux 语音路径不编译）与真实终端行为均未验收；manifests 的 `[ ]→[_]→[x]` 只能由对应 owner/master 推进，`[x]` 需主控独立复核。
