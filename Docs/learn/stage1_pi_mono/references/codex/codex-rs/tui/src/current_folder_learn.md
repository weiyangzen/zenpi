# src 目录整合报告

冻结目录 `codex-rs/tui/src`（`reference:codex`，HEAD `b3b3d262787f4902a7449f17d793241a34d311ad`，参考仓 `/Users/mac/GitHub/codex`）的物理直属库存为 60 个 `.rs` 文件与 13 个直接子目录（`app/`、`bin/`、`bottom_pane/`、`chatwidget/`、`exec_cell/`、`notifications/`、`onboarding/`、`public_widgets/`、`render/`、`snapshots/`、`status/`、`streaming/`、`tui/`），无其他直属普通文件。蓝图冻结的直属子项恰为 4 个文件：ZS1-306 `slash_command.rs`、ZS1-307 `file_search.rs`、ZS1-308 `key_hint.rs`、ZS1-309 `status_indicator_widget.rs`；冻结的直属子目录恰为 1 个：`bottom_pane/`（ZS1-354）。因此本目录的冻结直属闭包即这 4 个文件加 1 个子目录，共 5 项。

| 子项 | 源文件/目录 | 源字节 | 源 SHA-256 | 报告字节 |
| --- | --- | --- | --- | --- |
| ZS1-306 | slash_command.rs | 8020 | 9e53ca5bff5a5e7b86aa97731da985082db5d130c453e2d363edb9c5758ebc5a | 988 |
| ZS1-307 | file_search.rs | 4009 | 7aaa33ac7fd28cbe5fa3405cd3490b30e614272122bb572c9c352acc418b71d9 | 7482 |
| ZS1-308 | key_hint.rs | 3306 | 07f34aab630b2658560d2d4e73719481ec8638e33eb38401d7a23643689f13c8 | 861 |
| ZS1-309 | status_indicator_widget.rs | 15810 | e35e3efbd18784b55343366b2001bda08dfc1543f7e4a88700a715562d519cc5 | 1888 |
| ZS1-354 | bottom_pane/（目录） | — | — | 3733 |

上表 4 个源文件的字节数与 SHA-256 已在本机 `b3b3d262787f4902a7449f17d793241a34d311ad` 逐一重算，与蓝图第 444–447 行冻结指纹完全一致；`bottom_pane/` 为目录项，其 6 个冻结直属文件由子报告 ZS1-354 覆盖，本项只整合其候选结论。

**in-scope（纳入本目录集成）**：ZS1-306/307/308/309 四个直属文件，以及直属子目录 ZS1-354 `bottom_pane`（只整合作目录级候选，不重述其 6 个孙项的单文件证据）。

**context-only（仅作边界引用，未纳入本目录覆盖）**：其余 56 个直属 `.rs` 文件（如 `app.rs`、`chatwidget.rs`、`lib.rs`、`history_cell.rs`、`diff_render.rs`、`wrapping.rs`、`text_formatting.rs`、`line_truncation.rs`、`shimmer.rs`、`render/` 的消费者等）与其余 12 个直接子目录（`app/`、`bin/`、`chatwidget/`、`exec_cell/`、`notifications/`、`onboarding/`、`public_widgets/`、`render/`、`snapshots/`、`status/`、`streaming/`、`tui/`）均未在本项全文复核，不能宣称本目录完整覆盖。

## 冻结子集内的调用关系与数据所有权

- ZS1-306 `slash_command.rs`：定义 `SlashCommand` 枚举及 `description()`、`command()`、`supports_inline_args()`、`available_during_task()`、`is_visible()` 与 `built_in_slash_commands()` 的静态元数据（名称/描述/别名/内联参数/任务中可用性/平台可见性）。它只描述命令，不持有状态，也不执行命令；`command()` 经 `IntoStaticStr`/`Into<&'static str>` 生成无前导斜杠的名字。执行所有权在 context-only 的 `chatwidget.rs`/`app.rs`，popup 与解析消费在直属子目录 ZS1-354 的 `command_popup.rs`、`slash_commands.rs`、`chat_composer.rs`。
- ZS1-307 `file_search.rs`：`FileSearchManager`（`state: Arc<Mutex<SearchState>>`、`search_dir: PathBuf`、`app_tx: AppEventSender`）是 `@` 文件搜索的 TUI 适配层，`SearchState` 持有 `latest_query`、单个 `session`、`session_token`。异步搜索由外部 `codex_file_search` crate 的 worker 线程完成；本文件只按键更新 query、在 query 变空时丢弃 session、并在 producer 侧过滤陈旧快照后经 `AppEventSender` 发出 `AppEvent::FileSearchResult { query, matches }`。消费者在 ZS1-354 的 `file_search_popup.rs`（`set_matches` 按 `pending_query` 再丢弃一次）及 context-only 的 `app.rs`（resume 时 `update_search_dir`）。
- ZS1-308 `key_hint.rs`：纯显示叶子，定义 `KeyBinding::new/equal/is_press`、`plain/alt/shift/ctrl/ctrl_alt` 构造器、`modifiers_to_string`、`From<&KeyBinding> for Span`、`has_ctrl_or_alt`/`is_altgr`（Windows AltGr 判定）。它把 typed key 映射为 UI hint，不执行命令、不决定权限。in-scope 消费者是 ZS1-309（渲染 `esc to interrupt`）；其余消费者（`bottom_pane/approval_overlay.rs`、`list_selection_view.rs`、`multi_select_picker.rs`、`pending_input_preview.rs`、`popup_consts.rs`、`update_prompt.rs`、`model_migration.rs`、`tui/job_control.rs`）为 context-only。
- ZS1-309 `status_indicator_widget.rs`：只读渲染 composer 上方状态行。持有展示状态（`header`、`details`、`details_max_lines`、`inline_message`、`show_interrupt_hint`、`elapsed_running`、`last_resume_at`、`is_paused`）以及 `AppEventSender`、`FrameRequester`、`animations_enabled`。`interrupt()` 只经 `AppEventSender` 发 `AppEvent::CodexOp(Op::Interrupt)` 请求；`pause_timer_at`/`resume_timer_at` 只累积单调 elapsed；`update_*`/`set_interrupt_hint_visible` 只改展示。它不拥有任务状态、执行权限或取消决策。in-scope 依赖 ZS1-308（`key_hint::plain(KeyCode::Esc)`），另用 context-only 的 `exec_cell::spinner`、`shimmer`、`line_truncation`、`render::Renderable`、`text_formatting`、`tui::FrameRequester`、`wrapping`。
- ZS1-354 `bottom_pane`（目录）：组合 pane 与 popup/overlay/选择视图，持有 `ChatComposer`（TextArea、PasteBurst、history、slash/file/skill popup、附件）与 `FileSearchPopup`；其候选报告声明 `mod.rs` 组合 pane 与 popup/overlay，approval overlay 只投影待决请求，实际 policy/session/generation 属于上层 owner。

## 错误、取消与持久化路径

- ZS1-306：无运行时错误路径；未知/禁用命令、别名冲突与缺失参数的可见性由 popup/分发消费者负责。文档帮助文本不是可用性或权限证明。
- ZS1-307：`create_session` 失败只 `tracing::warn!` 并保持 `session = None`（无 UI 错误、无重试）；取消依赖 `FileSearchSession` 的 `Drop`（`cancel_flag` 传 `None`）；`on_complete` 为空实现；陈旧结果由 producer 的 `session_token` 换代 + 空 query 守卫与 consumer 的 `pending_query` 比较双重丢弃；`update_search_dir` 不递增 token，靠清空 `latest_query` 生效。无持久化。
- ZS1-308：无错误、取消或持久化路径；平台分支只影响 macOS/非 macOS 的 Alt 前缀显示。
- ZS1-309：`interrupt()` 只发中断请求，不含确认、超时或强制停止；计时器暂停/恢复语义单调（`saturating_duration_since`），`render` 在 `animations_enabled` 时每 32ms 请求下一帧；`details` 超过 `details_max_lines` 时截断加 `…`。无持久化。
- ZS1-354：resize/空列表/关窗时的光标、滚动与 pending request 一致性由目录 owner 负责；迟到事件不得更新新 thread 或复用旧 generation。

## 跨文件不变量

- 显示提示（ZS1-308/309 的 key hint、状态文案、截断/颜色）与命令元数据（ZS1-306 的名称/描述）都不能替代 schema、路径、审批或预算校验；UI 文本不构成已授权动作。
- 异步结果（ZS1-307）必须携带 session/generation 与 query 身份，并在搜索根切换、会话换代或 query 清空时被丢弃；任何 consumer 都必须能比较“最新 query”与“已显示结果 query”。
- 任务终结态与取消/超时/拒绝策略属于上层 owner（context-only 的 `chatwidget.rs`/`app.rs` 与 ZS1-354 的 approval 层），ZS1-309 的状态视图只做投影，不得把失败原因压缩成恒定的 “busy”。

## 根到叶闭包检查

- 根到叶链：root ZS1-350 → `codex-rs` ZS1-351 → `codex-rs/tui` ZS1-352 → 本目录 `codex-rs/tui/src` ZS1-353 → { ZS1-306/307/308/309（文件叶），ZS1-354 `bottom_pane` → ZS1-300..305（文件叶）}。父链各项在本蓝图中的依赖方向与冻结子集一致。
- 无跳目录：本项只对冻结直属闭包（4 文件 + 1 子目录）出结论；未纳入的 56 个直属文件与 12 个直接子目录明确标注 context-only，不跳级接受。
- 无重复接受：4 个源文件各自唯一映射到一个 item 与一个单文件报告；`bottom_pane` 的孙项由 ZS1-354 唯一覆盖，本项不重复接受。
- 无遗漏：5 个冻结直属子项均存在且源指纹已核对；蓝图标记 ZS1-306..309 与 ZS1-354 均为 `[x]`，依赖满足。
- 非阻塞一致性备注（依蓝图第 37 行，以本节表格指纹为权威）：`file_learn_index.tsv` 中 ZS1-306 及 ZS1-301..305 仍显示 `[ ]`，与蓝图 `[x]` 不一致，属索引陈旧；单文件报告 `slash_command.rs_learn.md` 自述 “8020B / 314L”，而本机冻结源为 8020B / 217 行（hash 与蓝图一致），属报告行数笔误。二者均不影响本目录闭包，不构成 blocked。

## 目标映射到 zenpi

下表为理解性映射，仅指向 `Docs/stage_1_v3_pi_mono_blueprint.md` 已登记的 owner，不表示 zenpi 已实现。

| 冻结能力 | 源事实 | zenpi 目标与最小判据 |
| --- | --- | --- |
| 命令注册与 popup 元数据 | ZS1-306 静态枚举 + `built_in_slash_commands` | ZS1-123 `src/slash.rs`、`src/slash_actions.rs`、`src/tui.rs`：registry 与执行 owner 分离 |
| `@` 异步搜索与陈旧丢弃 | ZS1-307 session/token/pending_query 三层过滤 | ZS1-122/123/126 `src/tui.rs`、`src/project_workspace.rs`：query 竞态、切根/resume 负例 |
| 快捷键提示绑定 | ZS1-308 typed binding → Span | ZS1-123/125 `src/tui.rs`、`src/render.rs`：hint 绑定稳定 action id 与 enabled predicate |
| 任务状态行与中断提示 | ZS1-309 只读投影 + `Op::Interrupt` 请求 | ZS1-125 `src/tui.rs`、`src/view_model.rs`：终结态、generation 丢弃迟到更新 |
| 目录组装边界 | ZS1-354 组合 pane/popup/overlay | ZS1-122/123：resize/取消/迟到选择事件负例 |

## 边界声明

本报告是目录整合候选 `[_]`：其冻结直属子项 ZS1-306..309 与 ZS1-354 在蓝图 `Docs/stage_1_v3_pi_mono_blueprint.md` 中已标记 `[x]`，本报告仍待主控按 G-DIR 独立审阅后方可接受。本项未运行产品、Cargo、测试、PTY、网络或 runner，也未修改 Codex 源、zenpi 主库或任何子项报告；撤回只影响本目录报告。
