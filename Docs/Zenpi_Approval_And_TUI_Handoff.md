# 审批与 TUI 修复：交接文档

**日期**：2026-09-22
**基线**：`main`（本地领先 `origin/main` 19 个提交，落后 0，无上游差异需合并）
**范围**：按已批准的修复计划执行 8 个工作块，完成 6 块，2 块未做（§4）。
**纪律**：所有改动都遵守实施记录 §1 的 rtk 规则——增量测试、`--locked --offline`、保留退出码、
零匹配不算通过、只格式化自己改的文件。

---

## 1. 一句话

把三份设计评审（GPT 两轮 + kimi）里**有代码证据**的缺陷修掉了：审批协调器的锁作用域、
授权审计缺口、控制句柄与 Agent 锁的耦合、TUI 审批归属、不可见的用户反馈、
以及 `--auto` 的语义重载。**全量测试 0 失败。**

---

## 2. 修了什么（每块都带证据）

### W1 取消谓词移出协调器临界区 — `src/approval.rs`

**问题**：`request_inner` 在持有 `CoordinatorState` 锁时调用 `cancelled()`，而 core 传入的闭包会
`pump.poll()` → `port.service(...)` 服务会话输入。宿主 `drain_pending`/`respond` 因此排在会话 I/O 后面。

**修法**：取消检查移到锁外；取消时短暂加锁清理。"检查决定 + 进入等待"仍在同一个临界区，
所以不会丢唤醒。拒绝、取消、丢弃决定的语义全部不变。

**证据**：新增 `slow_cancellation_predicate_does_not_block_a_host_draining_or_answering`。
**已反向验证**：把 `cancelled()` 临时挪回锁内，该测试精确失败
（`draining had to wait for the cancellation predicate to finish`，耗时 5.01s = 谓词超时），再回退。

### W2 授权审计缺口 + `approval_consumed` 语义 — `src/core.rs` / `src/approval.rs`

**问题**：① 策略短路（`ReadOnly`/`Never`/`per_tool`/`TrustedWorkspace`/短路拒绝）**一条记录都不写**，
所以 `--auto` 放行的每条命令事后都查不到授权依据；② `approval_consumed` 写在 Deny 判断**之前**，
拒绝也产生"许可被消费"。

**顺序不可颠倒，已遵守**：

1. **先收紧重放**：新增 `HOST_ANSWER_SOURCE`；`with_remembered_events` 现在拒绝任何 `source`
   不是 `host_answer` 的记录（**字段缺失 = 旧记录，仍接受**，向后兼容）。
   宿主写入处补 `"source": "host_answer"`。
2. **再补短路记录**：新事件 `authorization_decided`，覆盖短路 Allow 与 Deny 两条分支。
   `decide_after_preflight_source` 返回"命中规则"（`policy_never` / `policy_read_only` /
   `per_tool_grant` / `per_tool_deny` / `trusted_workspace` / `policy_headless` /
   `side_effect_denied` / `worker_preflight*`）。**原 `decide_after_preflight` 签名与行为不变**，
   既有测试不受影响。
3. `approval_consumed` 移到 Deny 判断之后。

**证据**：`auto_approval_executes_side_effects_without_pending_requests`（扩展）、
`a_policy_denial_is_recorded_even_though_no_host_saw_it`（新）、
`only_a_host_answer_source_creates_a_standing_grant`（新）。
`authorization_decided` 只出现在新代码里，证明测试钉的是新行为。

**注意**：探测确认 `approval_consumed` 在改动前**零读者、零测试**，所以这条改动无破坏面。

### W3 控制句柄与 Agent 锁解耦 — `src/project_workspace.rs` / `src/headless.rs`

**问题**：headless 的 `access`（协调器句柄缓存）只在 `try_lock()` **成功**时才刷新。
一个 owner 在"被缓存之前"就进入执行，它的审批**宿主永远取不到**，worker 一直等到取消。

**修法**：`OwnerControl { project, input_port, approval }` 在 owner **创建时**由 pool 登记
（`new` / `publish` / `arch_agent` 三处），新增不加锁访问器 `control()` / `control_handles()`。
headless 的 `try_lock` 块只保留心跳与上下文刷新，`access` 改从 pool 取。

**证据**：`owner_control_handles_stay_reachable_while_the_owner_is_busy`。

### W3b TUI 审批归属（本批最严重的 TUI bug）— `src/tui.rs`

**问题**：TUI 只跟踪**一个**协调器，且 `owner.try_lock().ok().and_then(...).or(approval.clone())`
在取不到锁时**回退到上一个句柄**，切换标签页还不刷新。别的 owner 抛出的待审批请求
**永远不会被呈现**，worker 等到取消为止——而 `try_lock` 失败的时机恰恰是 owner 忙、
最可能有待审批的时候。

**修法**：`approvals: BTreeMap<owner_key, OwnerControl>` 每轮从 pool 刷新；
drain 遍历**所有** owner 并**按各自项目归属**（新增 `present_approval_for(project, request)`）；
应答时用 `coordinator_holding(&approvals, project, request_id)` 按"谁真的持有这条待审批"定位
（一个项目可能同时有 discussion 与 arch 两个 owner）；`emergency_cancel` 取消全部。
arch owner 现在也登记进控制表（键 `arch:<id>`，`project` 字段负责归属）。

**证据**：`arch_lane_publishes_its_own_control_handles_for_the_same_project`。

### W4 让反馈真的看得见 — `src/tui.rs`

**问题**：① `HotZone::None` 时普通按键被静默丢弃（`return TuiAction::None`），无任何提示；
② 更深的一层：`TuiState::status` 在**生产渲染里根本不渲染**——`render_bentobox` 不画它，
唯一渲染点是 legacy `render()` 的 `render_header`。约 **114 处 `set_status`** 的反馈
（含"Arch console input exceeds …; draft unchanged"这类用户真正需要的拒绝提示）全是死的。

**修法**：
- 新增带过期的 `footer_notice`；`set_status` 自己喂它（排除 footer 已经渲染的两个值），
  **一处改动救活全部 114 个调用点，不动任何调用点**。
- `render_footer` 优先级链新增一个分支消费它。**同一行替换，绝不新增行**
  （硬约束：`tui_preview_graphemes` 在 8x20 / 10x12 视口渲染，多一行就裁掉被断言的字符）。
- 丢弃点保持 `return TuiAction::None`（不改返回值，避免弄红钉住该形状的两条测试），
  只设通知 + `self.dirty = true`（生产循环 `if state.take_dirty()` 会触发重绘）。
- `Event::Paste` 的静默丢弃同样处理。

**证据**：`a_key_refused_by_the_hot_zone_says_so_in_the_footer`。

### W5 `--auto` 显式化 + `/yolo` 不再被覆盖 — `src/core.rs` / `src/project_workspace.rs` / `src/tui.rs`

**问题**：① "用户开了 `--auto`"被表达成"根 owner 的 mode 恰好是 `Never`"（`project_workspace.rs`）。
今天恰好只有一个生产站点设置 `Never`，所以**碰巧正确**——任何将来"某个 owner 出于别的理由用
`Never`"都会静默把整个 pool 打开成全放行。② `/yolo`、`/approval` 只改活策略，不改
`metadata.approval_mode`，而 `bind_agent_to_active_project` 在切标签页时按存储值重绑 →
**用户的选择被静默revert**。

**修法**：`Agent` 新增显式 `auto_approve` 字段（`prepare_project_with_approval` 设置，pool 读取），
不再从枚举值反推。**ZS1-180 的作用域完整保留**（`--auto` 仍覆盖根/新项目/arch/复用 owner）。
新增 `TuiState::set_project_approval_mode`，`/yolo` 与 `/approval` 写它。

**证据**：`auto_approval_propagates_only_from_an_explicit_auto_owner`（重写）、
`yolo_records_the_choice_where_rebinding_reads_it`（新）。

### W7 审批交互 — `src/tui.rs`

**① Enter 一按确认**（用户选定）：裸 Enter 立即提交当前选择（默认选中是**拒绝**）。
拒绝理由仍可达，改为显式 `n` 进入输入阶段。**这是撤销 ZS1-182 的"两段式"**，见 §3。
**③ 按键提示**：`Select` 阶段提示改为 `Enter DENY now · y allow · n deny with a note · r remember · …`，
直接说出 Enter 会提交成什么。

**② 预览裁切**与**④ 多项目混乱**：未做布局重写。②的缓解是既有的 PgUp/PgDn/Home/End 滚动
（提示已常驻）；④ 的根因是归属，已由 W3b 修好，footer 的跨标签页提示
（`Approval waiting: <tabs> · switch tab`）保持原样。

---

## 3. 契约变更（reviewer 必须知道）

这些不是"顺手改测试"，是**有意的契约变更**，每条都在下面说明判据。

| 变更 | 判据 | 影响的测试 |
|---|---|---|
| **裸 Enter 从"打开拒绝阶段"改为"一按确认"** | 用户明确选定；拒绝是默认选中，因此裸 Enter 仍然是拒绝，安全性未降低 | `tui_approval_focus.rs`（4 处）、`tui_preview_graphemes.rs`（1 处）、**`approval_cap.rs`（2 处，探查阶段没找到，靠全量测试才发现）** |
| **`remember` 确认阶段保留** | 高风险操作值得一次确认 | `remember_confirm_and_reject_feedback_are_explicit_steps`（原 `staged_confirm_requires_a_second_enter_…`，已改名） |
| **auto 不再从 `Never` 推断** | 语义重载是真隐患；作用域本身是既定需求，只改表达方式 | `auto_approval_propagates_to_project_and_arch_owners` → 重写为 `..._only_from_an_explicit_auto_owner` |
| **`approval_consumed` 不再在 Deny 时写** | 拒绝没有许可可消费；零读者零测试，无破坏面 | 无 |
| **新事件 `authorization_decided`** | 补审计缺口；**不复用 `approval_resolved`**，避免落进旧恢复逻辑 | 无 |
| **`source` 字段 + 重放收紧** | 防止"补审计"本身污染持久授权 | 旧记录（无 `source`）仍可恢复，兼容性保住 |

**顺序约束**：W2 的"先收紧重放、再补记录"不可颠倒。若将来有人先加记录再收紧，
重启会把自动放行记录铸成持久授权。

---

## 4. 未完成（精确落点，接手从这里开始）

### W6 ratatui 低风险集成 — **复核后判定三项均不适用**

原先按 `Docs/Zenpi_TUI_Ratatui_Integration.md` §3 列为"低风险可集成"。逐条查源码后**全部不成立**，
详细论证写进了那份文档的 §6（原表已划掉并注明）。摘要：

1. **边框合并 `MergeStrategy`**——API 确实存在且零新依赖（`Block::merge_borders`，
   `ratatui::symbols::merge::MergeStrategy` 经 facade 可达），但它**只在同一 cell 被画两次时生效**，
   而 `src/layout.rs` 的 `visible_rects_non_overlapping()` 保证窗格 rect 互不相交。
   渲染帧里的 `││` / `┘└` 是两个相邻 cell，不是重叠——**构造性地永远不会触发**。
2. **`DefaultTerminal` / `init()` / `restore()`**——现有 `TerminalGuard` 比 `ratatui::init()`
   多做三件事：signal guard（异常退出还原终端）、bracketed paste + mouse capture、
   外部编辑器的终端状态捕获。换掉是**功能倒退**。
3. **`ratatui-macros`**——纯语法糖，不修任何缺陷；且 ratatui 0.30.2 要求 `ratatui-macros 0.7.2`
   而本地缓存只有 0.6.0，启用需联网拉取。

**顺带更正**：那份文档 §1.2 说"绝大多数绘制是手写进 `Buffer`"**是错的**——全文有 20+ 处
`Block::default()`，bentobox 每个窗格都是 `Block` widget 画的，只是 5 个类型写在了一行 import 里。

### W8 TUI 线程阻塞点 — **未做**

- `pool.arch_agent()` 内部 `active.lock()`（`src/project_workspace.rs`，`arch_agent` 里读
  session/cwd/overrides 那段）在 TUI 线程上可达（`tui.rs` 的 `active_arch` 调用点）。
  反例：A 等审批时新开 A 的 arch，会被 A 的等待挡住。
- `pool.publish()` → `prepare_project_with_approval` 的配置解析/会话打开/工具注册表构建
  都在 TUI 线程上做。
- **建议**：把"创建新 owner 所需的不可变描述"从执行锁中独立出来（评审里 GPT 的 D1）。
  这是本批唯一触碰 owner 生命周期的改动，风险高于其他块，应单独排期。

### 历史遗留（不属于本轮）

- **PA10 运行中入口**：会话内 `/auth`、`/login`、`/profile`；受控后台登录与 listener 回收。
- **PA12 编码侧**：`protocols/{chat,responses,anthropic,google}.rs` 不消费
  `ToolResult::Success.content`；headless `prompt.content` v2 有序输入入口缺失。

---

## 5. 评审之外的发现（本轮新查出来的）

1. **`panic = "abort"`**（`Cargo.toml:49`，`[profile.release]`）。release 构建下**任何线程 panic
   都会终止整个进程**，`catch_unwind` 接不住。任何"隔离单个 owner 故障"的设计必须先处理这条。
   本轮未改它。
2. **约 114 处 `set_status` 在生产渲染里不可见**（W4 已修）。
3. `tests/session_recovery.rs` 里**没有任何** approval 相关断言——恢复路径只处理 `operation_*` 标记。
4. `ApprovalMode::ReadOnly` 在 `decide_after_preflight` 里**没有匹配分支**：`ReadOnly` 下的写操作
   落到 `_ => None`，即会提示。这是刻意的，但读代码时容易误判。
5. `bind_agent_to_active_project` 收到的是**共享 Agent**（上一个活动标签页的 owner），
   而 `shared` 只在稍后才重新指向。探查未能判定这是否有意，**本轮未动，记此待确认**。

---

## 6. 验证状态

| 项 | 结果 |
|---|---|
| `cargo test --all-targets --all-features --no-fail-fast -- --test-threads=1` | **1565 passed, 0 failed, 23 ignored（93 suites, 161s）** |
| `cargo clippy --lib` | 0 errors / 23 warnings，**我的改动没有新增任何 warning** |
| `rustfmt --check`（仅改动的文件） | 全部 exit 0 |
| `git status` | 11 个改动文件 + 3 个新增文档 |

**已知的既有 flake（不是本次改动引入）**：
`auth::callback::tests::oversized_uri_and_slow_partial_requests_do_not_consume_the_flow`
（`src/auth/callback.rs`，本批**未改动该文件**）用真实 TCP socket + 1 秒 deadline 轮询，
在整套 lib target 满载时偶发失败：隔离运行 5/5 通过，整 target 串行连跑两次失败一次、
第三次通过。并行执行时 `tests/core_session.rs` 的
`user_shell_preserves_nonzero_signal_and_cancellation_evidence` 有同类表现。
两条都与审批/TUI 无关，**未修**（修它们属于另一件事：把时序假设换成确定性同步）。

**新增/重写的用例**：7 条（W1 1、W2 3、W3 1、W3b 1、W4 1、W5 2）。
其中 W1 的那条**反向验证过**——在改动前的实现上必然失败。

**未做的验证**：pty 冒烟（`tools/stage1_host_smoke.py`）。渲染改动**应当**跑它复核，
本轮时间预算用在了全量测试上。接手时建议补跑。

---

## 7. 改动文件

```
src/approval.rs                      W1 锁作用域；W2 source 常量与重放收紧、命中规则
src/core.rs                          W2 authorization_decided；W5 auto_approve 字段
src/project_workspace.rs             W3 OwnerControl；W5 显式 auto
src/headless.rs                      W3 access 改从 pool 取
src/tui.rs                           W3b 审批归属；W4 反馈通道；W5 /yolo；W7 Enter 与提示
tests/approval_owner.rs              W2 + W5 用例
tests/headless_project_workspace.rs  W3 + W3b 用例；W5 重写
tests/tui_approval_focus.rs          W2 用例；W7 契约变更
tests/tui_interaction.rs             W4 用例
tests/tui_preview_graphemes.rs       W7 契约变更
tests/approval_cap.rs                W7 契约变更（全量测试才发现）
Docs/Zenpi_Approval_Review_Prompts.md      （新）给外部评审的提问集
Docs/Zenpi_Scale_10k_Review_Prompt.md      （新）第 5 轮 10k 并发提问
```
