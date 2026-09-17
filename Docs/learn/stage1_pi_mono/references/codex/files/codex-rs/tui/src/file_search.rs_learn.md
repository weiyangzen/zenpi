# ZS1-307 — file_search.rs 完整单文件 learn 复核

状态：worker 候选（self_tested，主控 G-FILE/hash/chunks/源测试/操作映射 待审）。唯一 owner：`Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/file_search.rs_learn.md`。本轮只读复核并重写本报告；不修改 Codex 源、不修改 zenpi 主库、不宣称 zenpi 已实现这些能力。

## 来源、读取与 hash

唯一正式源 `codex-rs/tui/src/file_search.rs`，Codex 参考仓 `/Users/mac/GitHub/codex`，冻结 revision `b3b3d262787f4902a7449f17d793241a34d311ad`。4009 bytes / 133 行 / SHA256 `7aaa33ac7fd28cbe5fa3405cd3490b30e614272122bb572c9c352acc418b71d9`，与冻结蓝图 `Docs/stage_1_v3_pi_mono_blueprint.md` 第 437 行一致。源小于 256 KiB，单连续 byte chunk `[0,4009)` 已按 1–133 读到文件结束（第 133 行后到 EOF）；`chunk_manifest.tsv` 因此不含 ZS1-307 行。本报告不含源文件拷贝。

## 模块职责与状态

`FileSearchManager`（16–20 行）是 `@` 文件搜索的 TUI 适配层，字段为 `state: Arc<Mutex<SearchState>>`、`search_dir: PathBuf`、`app_tx: AppEventSender`。`SearchState`（22–26 行）保存 `latest_query: String`、`session: Option<file_search::FileSearchSession>`、`session_token: usize`；`TuiSessionReporter`（102–106 行）保存同一 `state` 克隆、`app_tx` 与本 session 的 `session_token`。

模块 doc（1–6 行）给出契约：`ChatComposer` 把 `@token` 的每次变化发布为 `AppEvent::StartFileSearch(query)`，本 manager 为当前搜索根持有单个 `codex-file-search` session，按键更新 query，query 变空时丢弃 session。本文件不执行命令、不打开文件、不授予路径权限，也不做路径 containment 校验；真实搜索由外部 `codex_file_search` crate 的 worker 线程完成。

## 交互语义

### 查询生命周期与去重（53–73 行）

`on_user_query` 先上锁，若 `query == latest_query` 直接返回（56–58 行，重复键去抖）。否则先清空并写入 `latest_query`（59–60 行），再判断：空 query 时 `st.session.take()` 丢弃 session 并返回（62–65 行）；`session.is_none()` 时调用 `start_session_locked`（67–69 行）；最后对既有 session 调 `update_query`（70–72 行）。

注意顺序：`latest_query` 在尝试建 session 之前就被设成新 query，所以后续同文本 query 会被去抖分支拦截。

### session 生命周期与 generation token（75–99 行）

`start_session_locked` 先把 `session_token` 用 `wrapping_add(1)` 自增（76 行），把该 token 写进新建的 `TuiSessionReporter`（78–82 行），再调用 `file_search::create_session(vec![search_dir], FileSearchOptions { compute_indices: true, ..Default::default() }, reporter, None)`（83–91 行）。成功则 `st.session = Some(session)`；失败只 `tracing::warn!` 并保持 `session = None`（92–98 行），没有错误回传 UI 或重试。`cancel_flag` 传 `None`，取消依赖 `FileSearchSession` 的 `Drop`（信号 shutdown）；未使用多根搜索，只传单元素 `vec![search_dir]`。

### snapshot 上报与陈旧结果过滤（109–133 行）

`TuiSessionReporter::send_snapshot` 上锁后，满足任一条件即直接返回，不发事件：`st.session_token != self.session_token`（112 行，丢弃被换代 session 的迟到快照）、`st.latest_query.is_empty()`（113 行，丢弃空查询/切根后的残留）、`snapshot.query.is_empty()`（114 行）。通过后克隆 `snapshot.query`、释放锁，再 `app_tx.send(AppEvent::FileSearchResult { query, matches })`（118–123 行）。`on_update` 转调 `send_snapshot`（128–130 行）；`on_complete` 为空实现（132 行）。

这里形成 producer 侧两层陈旧保护（generation token + 空 query 守卫）；consumer 侧还有一层：`bottom_pane/file_search_popup.rs` 的 `set_matches` 在 `query != pending_query` 时丢弃结果（该文件不在本 item 范围，只作消费者边界引用）。

### 搜索根切换（44–50 行）

`update_search_dir` 接收 `&mut self`，替换 `search_dir`，上锁后 `session.take()` 并清空 `latest_query`（46–49 行）。它不递增 `session_token`，而是靠清空 `latest_query` 让旧 session 的迟到快照在 113 行被丢弃。调用点在 resume 路径：`app.rs` 中 `self.file_search.update_search_dir(self.config.cwd.clone())`。

## 源测试清单（0 个）

本源文件不含 `#[cfg(test)]` 模块，也不含任何 `#[test]`（0 个），仅有 7 个 `fn`（`new`、`update_search_dir`、`on_user_query`、`start_session_locked`、`send_snapshot`、`on_update`、`on_complete`）、3 个 `struct`、3 个 `impl`。`codex-file-search` crate 自身另有测试，但那是另一个源文件，不属于 ZS1-307 范围。未运行任何测试；测试数量为 0 不等于行为已验证。

## 操作映射到 zenpi

下表的“源事实”来自本文件（及只作边界的消费者）；“zenpi 目标”为 `Docs/stage_1_v3_pi_mono_blueprint.md` 中已登记的 owner（ZS1-122/ZS1-123/ZS1-126 等），仅作理解映射，不表示 zenpi 已实现。

| 能力 | 源事实 | zenpi 目标与最小判据 |
|---|---|---|
| `@` 触发异步搜索查询 | composer 发布 `AppEvent::StartFileSearch`，manager 按键转 `update_query`（1–6、53–73 行） | ZS1-122 `src/tui.rs`、`src/input_queue.rs`、`tests/tui_composer.rs`：需有真实输入入口与 popup 生命周期 |
| 陈旧 query/generation 丢弃 | 去抖（56）+ token 换代（112）+ 空 query（113–114）；消费者 `set_matches` 再按 pending_query 丢弃 | ZS1-123 `src/tui.rs`、`src/slash.rs`、`src/slash_actions.rs`：补 query 竞态、迟到结果、重复路径负例 |
| 搜索根随会话切换 | `update_search_dir` 丢弃 session 并清空 query（44–50 行） | ZS1-126 `src/project_workspace.rs`、`src/headless.rs`：resume/切项目后不得复用旧根结果 |
| 候选路径插入 composer | 结果以 `(query, matches)` 交给上层，供插入/图片附件判断 | ZS1-122/ZS1-123 `src/tui.rs`：显示路径与实际路径区分，UI 文本不作为已授权路径 |
| 失败与取消语义 | 建 session 失败仅 warn、静默无结果（92–98 行）；cancel 靠 Drop；`on_complete` 空（132 行） | ZS1-123/ZS1-124 `tests/tui_command_palette.rs`、`tests/tui_approval_focus.rs`：需 typed 失败、取消/重开与 loading 状态负例 |

## 缺口与建议（均为静态映射，未运行）

若 zenpi 的 TUI 补全仅维护单个查询字符串，需显式区分“最新 query”与“已显示结果 query”，并为会话换代/搜索根切换准备可比较的 generation 标识，避免旧结果覆盖新结果。`on_user_query` 的同文本去抖会在 `create_session` 首次失败后阻止同 query 重试（59 行先写 `latest_query`，67–69 行不再重建），zenpi 应把“建 session 失败”与“查询未变”分开处理。建议为 query 竞态、空结果、重复路径、排序稳定性、长路径截断、切根/resume、取消/重开与迟到选择事件增加负例测试。这些是理解性建议，不是对 zenpi 现状的实现承诺。

## 边界声明

本轮未运行 Cargo、产品、PTY、网络或旧 runner，未修改 Codex 源或 zenpi 主库，未复制整份源。报告只支持对 `@` 搜索查询/session/结果上报语义的理解；不宣称 zenpi 已实现上述能力，也不替代主控独立源码复核。
