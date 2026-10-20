# Zenpi 轨迹视图实现说明

本文对应 `DESIGN_FINALIZED.md` 的落地实现。设计文档中的 blessed/TypeScript 方案已改为
Zenpi 现有的 Rust + ratatui 栈；数据模型、三层导航和快捷键保持设计意图。

## 入口

| 入口 | 行为 |
|------|------|
| Gantt 面板（`Tab` 切到 Gantt 热区） | `↑↓` 选择 agent，`Enter` 打开它的轨迹，`z` 放大成 Agent 总览，双击同 `Enter` |
| `Alt-T` / `/trajectory`（TUI） | 直接打开主会话（master）的记录列表 |
| `/trajectory`（headless） | 返回 JSON：记录数、轮次、用量、缓存命中率、平均 TTFT、`kv_cache` 问题列表 |

`Ctrl-T`（目录选择器）和 `Ctrl-G`（外部编辑器）已被占用，因此没有使用设计稿中的组合键。

## 分层

```
provider usage ──> backend::Usage { cache_read/cache_write/reasoning }   (protocols/*)
core 请求循环  ──> turn.metadata.request { started/first_token/completed,
                                           instructions/tools/context digest,
                                           context_turns, shared_turns }  (core.rs)
Turn 历史      ──> trajectory::Trajectory { records, turns, stats }       (trajectory.rs, 纯逻辑)
Trajectory     ──> tui::trajectory::TrajectoryViewer                      (tui/trajectory.rs, 仅绘制/导航)
```

- `src/trajectory.rs` 与 UI 无关（对应 ui-trajectory 的 snapshot builder + record formatting）。
- `src/tui/trajectory.rs` 只负责导航和绘制（对应 TrajectoryView / Table / Inspector）。

## 数据采集

1. **用量细分**：`Usage` 新增 `cache_read_tokens`、`cache_write_tokens`、`reasoning_tokens`
   （为 0 时不序列化，旧 journal 与 headless 输出保持不变）。
   - Anthropic：`cache_read_input_tokens` / `cache_creation_input_tokens`（`input_tokens` 仍为总输入）
   - OpenAI Chat / Responses：`*_tokens_details.cached_tokens`、`reasoning_tokens`
   - DeepSeek：`prompt_cache_hit_tokens`
   - Gemini：`cachedContentTokenCount`、`thoughtsTokenCount`
2. **每次请求的元数据**：工具调用步骤（`assistant-tool-call-*`）现在也持久化 `usage`，
   所有请求都会写入 `request`：开始 / 首 token / 完成时间，以及前缀指纹。
3. **前缀指纹**：instructions 摘要、工具集摘要，以及上下文 turn 的链式摘要。与同一会话的上一次请求
   比较，得到 `shared_turns`（共享的前缀 turn 数）。

## KV cache 校验规则

逐次比较相邻的 provider 请求：

| 标记 | 级别 | 条件 |
|------|------|------|
| `⚠` | Warn | instructions 摘要变化（系统提示词被重新渲染：skills / persona / resources） |
| `⚠` | Warn | 工具集摘要变化 |
| `⚠` | Warn | `shared_turns < previous_context_turns`（之前的 turn 被改写） |
| `ℹ` | Info | 同上，但两次请求之间发生了压缩（属预期） |
| `✗` | Miss | 前缀稳定、provider 有缓存计数，且上一次可复用 token ≥ 1024，但 `cache_read` 不足预期的 50% |
| `⚡n%` | Ok | 本次输入由缓存命中的比例 |

检查器的 Metrics 页显示判定、原因和建议。在记录列表中按 `c` 跳到下一个问题。

## 交互

- **L1 Agent 总览**：状态、轮次、请求数、token、缓存命中率、KV 标记、时长，以及共享时间轴上的甘特条。
- **L2 记录列表**：按 turn 分组（`Space` 折叠）、虚拟化窗口、跟随尾部、底部统计栏，
  `/` 搜索，`n`/`N` 跳转匹配项。
- **L3 检查器**：Content / Metrics / Raw 三个标签页（`Tab`），`←→` 切换记录，`y` 复制当前页。
- 通用：`Esc` 返回上一层，`1`/`2`/`3` 切换层级，`q` 关闭，`?` 帮助。

## Gantt：多 agent DAG 协作

Gantt 面板顶部新增 Agents 区（`src/tui/agents.rs`），下方保留原有的 Blueprint/Goal 投影。

- **行**：master 固定在第一行，下面是 DAG 节点树（`.zenpi/dag.json` 或 `ZENPI_DAG_STORE`，用 `├`/`└` 表示父子关系），最后是本项目中不属于 DAG 的 headless worker。
- **状态**：`● running`、`⏸ approval`（master 在等审批）、`◌ idle`、`○ open`（还没有 worker）、`⚠ stale`（worker 进程已退出但节点仍 open）、`✓ done`（green）、`✗ failed`（red）。
- **数据合并**：
  - DAG 节点提供结构、状态、摘要、心跳和信箱，`✉n` 表示未读消息数；
  - 进程表提供存活状态（按 `pid:N` 或 journal 文件名匹配）；
  - worker journal（`.zenpi/workers/<node>.jsonl`）提供时间轴和轨迹。
- **刷新**：每秒刷新一次；`dag.json` 的大小和修改时间不变时不重新解析；超过 4 MiB 的 store 不在终端线程上解析。
- **放大**：轨迹视图改为和 Resources 放大一致的内嵌浮层。Agent 总览即放大后的 Gantt，显示每个 agent 的 token、缓存命中率、KV 状态、时长和并行时间轴。

## 审批流程

- **在输入框里确认**：审批不再用全屏浮层弹出，而是显示在发起审批的那个会话的输入框里：
  - 讨论会话的审批显示在底部 Prompt；
  - Arch 会话的审批显示在 Arch prompt（通过 owner 控制表中的 `arch:<id>` 键区分来源）。

  输入框会临时加高，显示工具名、参数或 diff 摘要（文件、增删行数、字节数）和可用按键；对话区和其它面板保持可见。
- **选项（参考 Claude Code）**：表单下方是编号选项列表，`❯` 表示当前项：
  ```
  Do you want to proceed?
    1. Yes
    2. Yes, and don't ask again for run_command
  ❯ 3. No, and tell zenpi what to do differently
  ```
  `↑↓`（或 `k`/`j`）移动，`Enter` 确认当前项，数字键直接选中并确认。默认停在 “No”，直接回车不会误批（ZS1-182）。选第 2 项直接生效，不再二次确认；blueprint worker 的请求没有第 2 项。`n` 拒绝并附一句说明给模型。`Tab` 切到下一条审批。`v` 或翻页键打开完整详情（底部仍是同一组选项，空间不够时折成一行），`Esc` 回到输入框；在输入框表单里按 `Esc` 是先去写草稿，审批保持待处理，这一点和 Claude Code 不同。
- **输入 `y`/`n`**：在对应输入框里单独输入 `y`/`yes` 或 `n`/`no` 再回车，会回到表单并预选对应选项，不会再被当成插话发给 agent。
- **等待状态**：只有当前会话有待处理审批时，它的标题才显示 `⏸ 等待审批`（Prompt/Conversation 对应讨论会话，Arch 对应 Arch 会话）。
- **对话区提示**：写进发起审批的会话自己的对话区，指明在哪个输入框确认；不再显示 Request/Policy 哈希，详情窗口里仍有。

## 未实现 / 后续

- **暂停 / 继续 Agent**：运行时只有 cancel / steer，没有 pause 原语，需要先在 core 设计挂起点。
- **指纹的跨进程比较**：重启后的第一次请求没有 `shared_turns`；instructions / tools 摘要仍可跨进程比较。
- **DAG 起止时间**：DAG 节点本身不记录开始时间，时间轴用 worker journal 的创建和修改时间近似。
- **其他**：导出、标记、独立的 cache 统计面板。
