# tui 目录整合报告

冻结目录 `codex-rs/tui`（`reference:codex`，HEAD `b3b3d262787f4902a7449f17d793241a34d311ad`，参考仓 `/Users/mac/GitHub/codex`）的物理直属库存为 5 个普通文件（`BUILD.bazel`、`Cargo.toml`、`prompt_for_init_command.md`、`styles.md`、`tooltips.txt`）与 3 个直接子目录（`frames/`、`src/`、`tests/`），无其他直属条目。蓝图冻结的直属子项恰为 1 个目录：`src/`（ZS1-353）；本目录内不存在冻结的直属文件（蓝图第 438–447 行冻结的 10 个 `.rs` 文件全部位于 `src/` 子树内，分属 ZS1-353 与其子目录 ZS1-354）。因此本目录的冻结直属闭包即 1 项。

| 子项 | 源目录 | 源字节 | 源 SHA-256 | 目录报告字节 |
| --- | --- | --- | --- | --- |
| ZS1-353 | src/（目录） | — | — | 10206 |

上表 1 个子目录为目录项，其冻结直属文件 ZS1-306..309（`slash_command.rs`、`file_search.rs`、`key_hint.rs`、`status_indicator_widget.rs`）与直属子目录 ZS1-354（`bottom_pane/`）由子报告 ZS1-353 覆盖；本项只整合其目录级候选结论。ZS1-353 报告内 4 个直属源文件的字节数与 SHA-256 已与本机 `b3b3d262787f4902a7449f17d793241a34d311ad` 逐一重算核对（见下方闭包检查），与蓝图第 444–447 行冻结指纹一致。

**in-scope（纳入本目录集成）**：唯一直属子目录 ZS1-353 `src/`，只整合作目录级候选，不重述其 4 个直属文件叶与 1 个孙目录（ZS1-354）及其 6 个文件叶的单文件证据。

**context-only（仅作边界引用，未纳入本目录覆盖）**：其余 5 个直属普通文件与 2 个直接子目录（`frames/`、`tests/`）均未在本项全文复核，不能宣称本目录完整覆盖。`Cargo.toml`/`BUILD.bazel` 是 crate 与构建定义，`styles.md`、`tooltips.txt`、`prompt_for_init_command.md` 是 TUI 文案/初始化提示资产，`frames/` 是快照资源，`tests/` 是 crate 级集成测试；它们承载真实终端入口与恢复路径，但本项的验收边界只到冻结子集。

## 冻结子集内的调用关系与数据所有权

- 目录 `codex-rs/tui` 是 Codex TUI crate 的根，`src/` 是其模块树与二进制入口的唯一 owner。冻结子集只覆盖 `src/` 内的输入编辑/焦点/取消切面：ZS1-306 `slash_command.rs`（命令元数据）、ZS1-307 `file_search.rs`（`@` 搜索异步适配）、ZS1-308 `key_hint.rs`（显示绑定）、ZS1-309 `status_indicator_widget.rs`（状态投影），以及 ZS1-354 `bottom_pane/`（组合 pane 与 popup/overlay）。
- 输入边界：编辑与粘贴的 owner 在 ZS1-354 的 ZS1-300 `chat_composer.rs`/ZS1-301 `textarea.rs`/ZS1-302 `paste_burst.rs`；父目录 ZS1-306..309 只提供命令元数据、搜索候选、快捷键显示和状态投影，不拥有编辑缓冲。`@` 搜索（ZS1-307）的 session/token 与结果必须回到组合层（ZS1-354 的 `file_search_popup.rs`）与 crate 根（context-only 的 `app.rs`）消费。
- 焦点边界：焦点优先级与 active-thread 归属属于 crate 根（context-only 的 `app.rs`/`chatwidget.rs`）与 `bottom_pane` 组合 owner；冻结文件不得被当作权限/审批/session 的权威来源。
- 取消边界：ZS1-309 的 `interrupt()` 只经 `AppEventSender` 发 `AppEvent::CodexOp(Op::Interrupt)` 请求，不含确认、超时或强制停止；真正的取消、终结态与迟到事件代际丢弃属于上层 owner。
- 目录级所有权声明：本目录只描述 TUI 交互投影层，不持有 provider 请求、session journal、approval policy 或子进程生命周期；这些属于 crate 根之外的 owner。

## 错误、取消与持久化路径

- ZS1-306/307/308：分别为纯元数据、异步搜索适配与纯显示叶子。ZS1-307 的 `create_session` 失败只 `tracing::warn!` 并保持 `session = None`，陈旧结果靠 session_token 换代与空 query/`pending_query` 双重丢弃；无持久化。
- ZS1-309：只读状态行，`interrupt()` 仅发请求，计时器单调（`saturating_duration_since`），`render` 在动画开启时每 32ms 请求下一帧；无持久化。
- ZS1-354：resize/空列表/关窗必须保持光标、滚动与 pending request 一致；迟到事件不得更新新 thread 或复用旧 generation。
- 目录级：TUI 退出时的终端恢复、前台进程组与草稿保存由 crate 根入口与 context-only 的 `tests/`、`app.rs` 承担，不属本冻结闭包；本目录不声称已验证恢复路径。

## 跨文件不变量

- 显示提示（ZS1-308/309 的 key hint、状态文案、截断）与命令元数据（ZS1-306 名称/描述）都不能替代 schema、路径、审批或预算校验；UI 文本不构成已授权动作。
- 所有跨模块异步事件必须携带 session/thread/generation 或稳定 action/request id；消费方必须能比较“最新 query/代际”与“已显示结果”，否则丢弃（ZS1-307 与 ZS1-354 的一致要求）。
- 任务终结态、取消/超时/拒绝策略属于上层 owner；冻结子集内任何视图只做投影，不得把失败原因压缩成恒定 busy 或固定成功。
- 焦点、滚动与输入草稿在项目/会话切换时必须保持一致，属于目录级 owner 职责，但本项未运行行为验证。

## 根到叶闭包检查

- 根到叶链：root ZS1-350 → `codex-rs` ZS1-351 → 本目录 `codex-rs/tui` ZS1-352 → `codex-rs/tui/src` ZS1-353 → { ZS1-306/307/308/309（文件叶），ZS1-354 `bottom_pane` → ZS1-300..305（文件叶）}。父链各依赖方向与蓝图冻结子集一致（ZS1-352 Depends: ZS1-353）。
- 无跳目录：本项只对冻结直属闭包（1 个子目录）出结论；`frames/`、`tests/` 及 5 个直属普通文件明确标注 context-only，不跳级接受。
- 无重复接受：`src/` 唯一映射到 ZS1-353，其下 4 个文件叶与 1 个孙目录由 ZS1-353 唯一覆盖，本项不重复接受。
- 无遗漏：冻结闭包唯一子项存在；4 个源文件指纹已重算并逐一匹配蓝图（8020/9e53ca5b…、4009/7aaa33ac…、3306/07f34aab…、15810/e35e3efb…），10 个孙叶文件指纹亦与本机源一致。蓝图标记 ZS1-353 为 `[x]`，依赖满足。
- 非阻塞一致性备注（依蓝图第 37 行，以第 438–447 行表格指纹为权威）：`folder_learn_index.tsv` 中 ZS1-352/ZS1-353 仍显示 `[ ]`，`file_learn_index.tsv` 中 ZS1-301..303/305/306 仍显示 `[ ]`，与本蓝图 `[x]`/本项依赖不一致，属索引陈旧；不影响本目录闭包，不构成 blocked。

## 目标映射到 zenpi

下表为理解性映射，仅指向 `Docs/stage_1_v3_pi_mono_blueprint.md` 已登记的 owner，不表示 zenpi 已实现。

| 冻结能力 | 源事实 | zenpi 目标与最小判据 |
| --- | --- | --- |
| 输入编辑/历史/粘贴与排队消息 | ZS1-300/301/302（经 ZS1-354） | ZS1-122 `src/tui.rs`、`src/input_queue.rs`：编辑/粘贴/历史与可编辑排队消息 |
| 命令/技能/文件补全与模型选择 | ZS1-306/307 + ZS1-354 popup | ZS1-123 `src/tui.rs`、`src/slash.rs`、`src/slash_actions.rs`：真实 popup 与执行 owner |
| 审批焦点与取消体验 | ZS1-303/304/305（经 ZS1-353 整合） | ZS1-124 `src/tui.rs`、`src/approval.rs`：默认 deny、焦点转移与取消终结态 |
| 滚动/复制/状态与终端恢复 | ZS1-308/309 | ZS1-125 `src/tui.rs`、`src/render.rs`、`src/view_model.rs`：终结态与迟到代际丢弃 |
| 项目分页与共享上下文 | 目录级（TUI 入口） | ZS1-120/121/126 `src/project_workspace.rs`、`src/tui.rs`、`src/headless.rs`：项目身份、cwd、非活动 tab 不污染 |
| 大段粘贴/删除缓冲/外部编辑器 | ZS1-300/301/302 + 目录级 | ZS1-129/130 `src/tui.rs`、`src/external_editor.rs`：逻辑行编辑与终端交接 |
| Codex 交互矩阵回归 | 整棵冻结树 | ZS1-128 `Docs/quality/stage1/ux-reference.md`、`tests/tui_bentobox.rs`：逐操作真实入口验收 |

## 边界声明

本报告是目录整合候选 `[_]`：其冻结直属子项 ZS1-353 在蓝图 `Docs/stage_1_v3_pi_mono_blueprint.md` 中已标记 `[x]`，本报告仍待主控按 G-DIR 独立审阅后方可接受。本项未运行产品、Cargo、测试、PTY、网络或 runner，也未修改 Codex 源、zenpi 主库或任何子项报告；撤回只影响本目录报告。
