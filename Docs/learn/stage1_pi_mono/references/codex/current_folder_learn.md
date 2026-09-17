# Codex 参考树根目录（`.`）整合报告

冻结目录 `.`（`reference:codex`，HEAD `b3b3d262787f4902a7449f17d793241a34d311ad`，参考仓 `/Users/mac/GitHub/codex`）的物理直属库存为 31 个普通文件与 15 个直接子目录（`.codex/`、`.cron/`、`.devcontainer/`、`.github/`、`.ops/`、`.vscode/`、`Docs/`、`codex-cli/`、`codex-rs/`、`patches/`、`scripts/`、`sdk/`、`shell-tool-mcp/`、`third_party/`、`tools/`，不计 VCS 元数据 `.git/`），无其他直属条目。蓝图第 436–449 行冻结的直属子项恰为 1 个目录：`codex-rs/`（ZS1-351）；本目录内不存在冻结的直属文件——冻结的 10 个 `.rs` 文件全部位于 `codex-rs/tui/src/` 子树内，分属 ZS1-353 与其孙目录 ZS1-354。因此本目录的冻结直属闭包即这 1 个目录项。权威绑定：claim `blueprint_digest = 7ec024fb2e1d194492a718846474734918d7ebab88d675ea5e8e72945e492175`（本机 worktree 蓝图实测一致），源指纹以蓝图第 438–447 行表格为准，历史报告内旧 revision hash 一律被第 31–37 行「源冻结重签声明」取代。

| 子项 | 源目录 | 源字节 | 源 SHA-256 | 目录报告字节 |
| --- | --- | --- | --- | --- |
| ZS1-351 | codex-rs/（目录） | — | — | 10521 |

上表 1 个子目录为目录项，其冻结直属子目录 ZS1-352（`codex-rs/tui/`）、ZS1-353（`codex-rs/tui/src/`）、ZS1-354（`codex-rs/tui/src/bottom_pane/`）与 10 个文件叶 ZS1-300..309 均由子报告 ZS1-351 逐层覆盖；本项只整合其目录级候选结论，不重述叶级证据。本项已按机器迁移重冻结声明（蓝图第 31–37 行），以蓝图第 438–447 行表格指纹为权威，在本机 `b3b3d262787f4902a7449f17d793241a34d311ad` 上逐一重算 10 个冻结源文件的字节数与 SHA-256，全部一致（见下方闭包检查）。

**in-scope（纳入本目录集成）**：唯一直属子目录 ZS1-351 `codex-rs/`，只整合作目录级候选，不重述其子树内 ZS1-352/ZS1-353/ZS1-354 及 10 个文件叶的单文件证据。

**context-only（仅作边界引用，未纳入本目录覆盖）**：其余 14 个直接子目录（`.codex/`、`.cron/`、`.devcontainer/`、`.github/`、`.ops/`、`.vscode/`、`Docs/`、`codex-cli/`、`patches/`、`scripts/`、`sdk/`、`shell-tool-mcp/`、`third_party/`、`tools/`）与 31 个直属普通文件（`AGENTS.md`、`README.md`、`LICENSE`、`NOTICE`、`SECURITY.md`、`CHANGELOG.md`、`BUILD.bazel`、`MODULE.bazel`、`MODULE.bazel.lock`、`defs.bzl`、`rbe.bzl`、`justfile`、`flake.nix`、`flake.lock`、`package.json`、`pnpm-lock.yaml`、`pnpm-workspace.yaml`、`announcement_tip.toml`、`cliff.toml`、`workspace_root_test_launcher.{sh,bat}.tpl`、`.bazel*`、`.codespell*`、`.gitignore`、`.markdownlint-cli2.yaml`、`.npmrc`、`.prettier*` 等）均未在本项全文复核，不能宣称本目录完整覆盖。其中 `codex-cli/`、`sdk/`、`shell-tool-mcp/`、`scripts/`、`third_party/` 承载真实 CLI/SDK/MCP/构建入口，`Docs/` 是参考仓自身文档，`BUILD.bazel`/`MODULE.bazel`/`flake.*`/`justfile`/`defs.bzl`/`rbe.bzl`/`pnpm-*`/`package.json` 是构建与工作区定义，`AGENTS.md`/`README.md` 等是说明资产；本项验收边界只到冻结子集，这些条目的物理存在仅作边界引用。

## 冻结子集内的调用关系与数据所有权

- `.` 是 Codex 参考仓根，唯一冻结直属子项 `codex-rs/`（ZS1-351）是 Rust workspace 根，聚合数十个成员 crate；冻结子集只覆盖其中 TUI crate 的交互投影切面。
- 输入边界：编辑与粘贴的 owner 在 ZS1-354 的 ZS1-300 `chat_composer.rs`/ZS1-301 `textarea.rs`/ZS1-302 `paste_burst.rs`；`codex-rs/tui/src` 直属的 ZS1-306..309 只提供命令元数据、`@` 搜索候选、快捷键显示与状态投影，不拥有编辑缓冲。
- 焦点边界：焦点优先级、active-thread 归属与 popup/overlay 生命周期属于 TUI crate 根（context-only 的 `app.rs`/`chatwidget.rs`）与 ZS1-354 的 `bottom_pane` 组合 owner；冻结文件不得被当作权限、审批或 session 的权威来源。
- 取消边界：ZS1-309 的 `interrupt()` 只经 `AppEventSender` 发 `AppEvent::CodexOp(Op::Interrupt)` 请求，不含确认、超时或强制停止；真正的取消、终结态与迟到事件代际丢弃属于上下文 owner。
- 目录级所有权声明：整个冻结闭包只描述 TUI 交互投影层，不持有 provider 请求、session journal、approval policy 或子进程生命周期；这些 owner 位于冻结闭包之外（context-only 的 `codex-cli/`、`sdk/`、`shell-tool-mcp/` 以及 `codex-rs` 内的 `core/`、`protocol/`、`exec*`、`app-server*` 等 crate）。

## 错误、取消与持久化路径

- ZS1-351 子树内 ZS1-306/307/308 分别为纯元数据、异步搜索适配与纯显示叶子：ZS1-307 的 `create_session` 失败只 `tracing::warn!` 并保持 `session = None`，陈旧结果靠 `session_token` 换代 + 空 query/`pending_query` 双重丢弃，无持久化。
- ZS1-309 为只读状态行，`interrupt()` 仅发请求，计时器单调（`saturating_duration_since`），`render` 在动画开启时每 32 ms 请求下一帧，无持久化。
- ZS1-354：resize/空列表/关窗必须保持光标、滚动与 pending request 一致；迟到事件不得更新新 thread 或复用旧 generation；approval overlay 只投影待决请求，实际 policy/session/generation 属于上层 owner。
- 目录级：TUI 退出时的终端恢复、前台进程组与草稿保存由 TUI crate 根入口与 context-only 的 `codex-rs/tui/tests/`、`app.rs` 承担，不属本冻结闭包；本目录不声称已验证恢复路径。

## 跨文件不变量

- 显示提示（ZS1-308/309 的 key hint、状态文案、截断）与命令元数据（ZS1-306 名称/描述）都不能替代 schema、路径、审批或预算校验；UI 文本不构成已授权动作。
- 所有跨模块异步事件必须携带 session/thread/generation 或稳定 action/request id；消费方必须能比较“最新 query/代际”与“已显示结果”，否则丢弃（ZS1-307 与 ZS1-354 的一致要求）。
- 任务终结态、取消/超时/拒绝策略属于上层 owner；冻结子集内任何视图只做投影，不得把失败原因压缩成恒定 busy 或固定成功。
- 焦点、滚动与输入草稿在项目/会话切换时必须保持一致，属于目录级 owner 职责，但本项未运行行为验证。

## 根到叶闭包检查

- 根到叶链：root ZS1-350（本目录 `.`）→ `codex-rs` ZS1-351 → `codex-rs/tui` ZS1-352 → `codex-rs/tui/src` ZS1-353 → { ZS1-306/307/308/309（文件叶），ZS1-354 `bottom_pane` → ZS1-300..305（文件叶）}。父链各依赖方向与蓝图冻结子集一致（ZS1-350 Depends: ZS1-351，ZS1-351 Depends: ZS1-352，ZS1-352 Depends: ZS1-353）。
- 无跳目录：本项只对冻结直属闭包（1 个子目录）出结论；其余 14 个直接子目录与 31 个直属普通文件明确标注 context-only，不跳级接受。
- 无重复接受：`codex-rs/` 唯一映射到 ZS1-351，其子树由 ZS1-351/ZS1-352/ZS1-353/ZS1-354 逐层唯一覆盖，本项不重复接受。
- 无遗漏：冻结闭包唯一子项存在；10 个冻结源文件指纹已在本机 `b3b3d262787f4902a7449f17d793241a34d311ad` 逐一重算并全部匹配蓝图第 438–447 行：

| 文件 | 字节 | SHA-256 |
| --- | ---: | --- |
| codex-rs/tui/src/bottom_pane/chat_composer.rs | 374943 | 234189c66f50c8654ef72e86ad5463eea5dd1e0c09c98f8d64c898449161b6da |
| codex-rs/tui/src/bottom_pane/textarea.rs | 91601 | 8cb5241c9e818210bfff63975e703f45a25a3da91bdb395667893903f7cb4b45 |
| codex-rs/tui/src/bottom_pane/paste_burst.rs | 24594 | c80558dfd3cbde437f2fec9a855b01a3180542a53bb135dc92ce7c81299f1ee7 |
| codex-rs/tui/src/bottom_pane/approval_overlay.rs | 57262 | c924923b402b198f8f77b249f0d37892896c0b84002c0f6e48330917da80cecc |
| codex-rs/tui/src/bottom_pane/pending_thread_approvals.rs | 4105 | a401d7c6ba3051fcf2c20152a9e86a3f4f8751a87470cbc7f518001326618a2f |
| codex-rs/tui/src/bottom_pane/mod.rs | 72998 | 97af02eed39dbfdf69d1ec3e14dc13b9a91f4b33f5e4b985cc1f2cbae4105823 |
| codex-rs/tui/src/slash_command.rs | 8020 | 9e53ca5bff5a5e7b86aa97731da985082db5d130c453e2d363edb9c5758ebc5a |
| codex-rs/tui/src/file_search.rs | 4009 | 7aaa33ac7fd28cbe5fa3405cd3490b30e614272122bb572c9c352acc418b71d9 |
| codex-rs/tui/src/key_hint.rs | 3306 | 07f34aab630b2658560d2d4e73719481ec8638e33eb38401d7a23643689f13c8 |
| codex-rs/tui/src/status_indicator_widget.rs | 15810 | e35e3efbd18784b55343366b2001bda08dfc1543f7e4a88700a715562d519cc5 |

  子报告 `Docs/learn/stage1_pi_mono/references/codex/codex-rs/current_folder_learn.md`（ZS1-351，10521 B）与本目录项构成直接依赖；蓝图第 462 行标记 ZS1-351 为 `[x]`，依赖满足。
- 非阻塞一致性备注（依蓝图第 37 行，以第 438–447 行表格指纹为权威）：`folder_learn_index.tsv` 中 ZS1-350/ZS1-351/ZS1-352/ZS1-353/ZS1-354 仍显示 `[ ]`，`file_learn_index.tsv` 中 ZS1-301..303/305/306 仍显示 `[ ]`，与蓝图 `[x]` 不一致，属索引陈旧；此外本次 worktree 基线未物化 ZS1-351/ZS1-352/ZS1-353 的子报告文件（仅存在于主控 checkout `Docs/learn/stage1_pi_mono/references/codex/codex-rs/current_folder_learn.md` 等路径，本项以只读方式读取并整合其候选结论），不影响本目录闭包，均不构成 blocked。

## 本机验证记录（替代与上限）

- 已执行：`shasum -a 256` 重算 10 个冻结源文件 → 字节数与 SHA-256 全部与蓝图第 438–447 行一致（脚本 `/tmp/opencode/zs1_350_gdir_check.py`，exit 0）。
- 已执行：以本机 `tools/validate_stage1_blueprint.py` 自带 `parse()` 组件复核 `ZS1-350`：state `[ ]`、layer `L2`、`loc=0`、`owned_paths` 恰为唯一报告、`validators` 含 `G-DIR`、`depends == (ZS1-351,)`、folder 映射 `('reference:codex','.',报告)`、按解析器规则计算的直属子项 `== {ZS1-351}` 且与 `depends` 完全相等、依赖 ZS1-351 状态 `[x]` 且其报告存在非空、`reference:codex` 恰 10 文件 + 5 目录、根 `.` 无冻结直属文件 → 全部 PASS（同一脚本，exit 0）。
- 替代：声明的 validator `python3 tools/validate_stage1_blueprint.py --blueprint Docs/stage_1_v3_pi_mono_blueprint.md --evidence-root Docs/learn/stage1_pi_mono --item ZS1-350` 本机 exit 1，原因是 `Docs/execution/active_requirement.json` 缺少结尾换行（`incomplete JSON record (missing final newline)`），该 selector 文件不在本项 owned path，本项不修改它；另 ZS1-350 的 master receipt 尚不存在，属主控验收产物而非 worker 产物。故以组件级 G-DIR 结构检查 + 源指纹核对作为最接近等价验证，其上限是不做 receipt/selector 级联校验与语义人工审阅。

## 目标映射到 zenpi

下表为理解性映射，仅指向 `Docs/stage_1_v3_pi_mono_blueprint.md` 已登记的 owner，不表示 zenpi 已实现。

| 冻结能力 | 源事实 | zenpi 目标与最小判据 |
| --- | --- | --- |
| 输入编辑/历史/粘贴与排队消息 | ZS1-300/301/302（经 ZS1-351→ZS1-354） | ZS1-122 `src/tui.rs`、`src/input_queue.rs`：编辑/粘贴/历史与可编辑排队消息 |
| 命令/技能/文件补全与模型选择 | ZS1-306/307 + ZS1-354 popup | ZS1-123 `src/tui.rs`、`src/slash.rs`、`src/slash_actions.rs`：真实 popup 与执行 owner |
| 审批焦点与取消体验 | ZS1-303/304/305（经 ZS1-353 整合） | ZS1-124 `src/tui.rs`、`src/approval.rs`：默认 deny、焦点转移与取消终结态 |
| 滚动/复制/状态与终端恢复 | ZS1-308/309 | ZS1-125 `src/tui.rs`、`src/render.rs`、`src/view_model.rs`：终结态与迟到代际丢弃 |
| 项目分页与共享上下文 | 目录级（TUI 入口） | ZS1-120/121/126 `src/project_workspace.rs`、`src/tui.rs`、`src/headless.rs`：项目身份、cwd、非活动 tab 不污染 |
| 大段粘贴/删除缓冲/外部编辑器 | ZS1-300/301/302 + 目录级 | ZS1-129/130 `src/tui.rs`、`src/external_editor.rs`：逻辑行编辑与终端交接 |
| Codex 交互矩阵回归 | 整棵冻结树 | ZS1-128 `Docs/quality/stage1/ux-reference.md`、`tests/tui_bentobox.rs`：逐操作真实入口验收 |

## 边界声明

本报告是目录整合候选 `[_]`：其冻结直属子项 ZS1-351 在蓝图 `Docs/stage_1_v3_pi_mono_blueprint.md` 中已标记 `[x]`，本报告仍待主控按 G-DIR 独立审阅后方可接受。本项未运行产品、Cargo、测试、PTY、网络或 runner，也未修改 Codex 源、zenpi 主库或任何子项报告；撤回只影响本目录报告。
