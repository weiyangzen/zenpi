# Zenpi 轨迹功能交互设计 - 确定方案

## 确定的设计方案

### 布局方案：B - 全屏切换 + 面包屑导航

```
┌─────────────────────────────────────────────────────────────┐
│ Zenpi TUI  [Gantt → worker-2 → Turn 3 → #17]     [KV: ✓]   │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│ [全屏视图内容]                                              │
│                                                              │
│                                                              │
│ 底部: [1]Gantt [2]Trajectory [3]Inspector | Quick: g/a/c    │
└─────────────────────────────────────────────────────────────┘
```

**确定理由**：
- 每个视图有充足空间显示完整信息
- 面包屑提供清晰的导航上下文
- 快捷键支持快速切换，不会感觉割裂
- 适合深入分析和监控

### 功能范围 - MVP 版本

#### ✅ 包含功能

1. **三层视图**
   - 甘特图：显示所有 agent 的任务时间线
   - 轨迹列表：显示单个 agent 的操作记录
   - 检查器：显示单条记录的详细信息

2. **基础导航**
   - 面包屑导航
   - Agent 选择器（按 `a`）
   - 快捷键切换视图

3. **Agent 控制**
   - 暂停 agent（按 `p`）
   - 继续 agent（按 `r`）
   - 显示 agent 状态

4. **数据展示**
   - Token 用量（input/output/reasoning）
   - KV Cache 信息（read/write）
   - 计时信息（duration/TTFT）
   - 错误高亮

5. **搜索功能**
   - 搜索记录（按 `/`）
   - 下一个/上一个结果（`n/N`）

#### 🔜 暂缓功能（后续版本）

- 修改参数（需要权限系统）
- 插入 steering 指令（需要 safety 审查）
- 标记功能（`m`）
- Cache 统计面板（`c`）
- 导出轨迹
- 折叠/展开优化
- 时间线可视化

## 详细界面设计

### 视图 1：甘特图模式

```
┌─────────────────────────────────────────────────────────────┐
│ Zenpi TUI - Gantt Chart                   [5 agents active] │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Agent         Progress    Status      Duration    Actions  │
│  ─────────────────────────────────────────────────────────  │
│  master        ████████    running     00:12:35    [⏸]      │
│  worker-1      ████████    completed   00:08:22    [✓]      │
│> worker-2      ██████░░    running     00:10:15    [⏸]      │
│  worker-3      ████░░░░    running     00:06:45    [⏸]      │
│  worker-4      ░░░░░░░░    pending     00:00:00    [▶]      │
│                                                              │
│  Timeline:                                                   │
│  00:00 ───────────────── 00:12:35 (now) ────────> 00:20:00 │
│                                                              │
│  Legend: █ Completed  ░ Remaining  [⏸] Pause [▶] Resume    │
│                                                              │
│                                                              │
│                                                              │
│                                                              │
│  ↑↓ Select  Enter View Trajectory  p Pause  r Resume  ? Help│
└─────────────────────────────────────────────────────────────┘
```

**关键元素**：
- Agent 列表（当前选中用 `>` 标记）
- 进度条（█ 已完成，░ 剩余）
- 状态文本（running/completed/pending/paused）
- 持续时间
- 操作按钮（pause/resume）

### 视图 2：轨迹列表模式

```
┌─────────────────────────────────────────────────────────────┐
│ Zenpi TUI  [Gantt → worker-2]                     [Running] │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Turn 3                                          [00:02:35] │
│  ───────────────────────────────────────────────────────────│
│  #15  10:23:45  REQUEST   Execute task-3            2.5K in │
│  #16  10:23:46  TOOL      read_file config.yaml      0.2K  │
│  #17  10:24:01  TOOL      write_file output.txt      0.1K  │
│> #18  10:24:05  ASSISTANT Task completed ✓          1.8K out│
│                                                              │
│  Turn 4                                          [00:01:20] │
│  ───────────────────────────────────────────────────────────│
│  #19  10:25:10  REQUEST   Verify results               1.2K │
│  #20  10:25:11  TOOL      read_file output.txt        0.3K  │
│  #21  10:25:15  ASSISTANT Verification passed ✓       0.8K  │
│                                                              │
│  Turn 5 (in progress)                            [00:00:08] │
│  ───────────────────────────────────────────────────────────│
│  #22  10:26:30  REQUEST   Cleanup...                   0.5K │
│                                                              │
│  📊 Stats: Total 7.4K tokens | Avg TTFT 0.3s | Cache 95%   │
│  🔄 KV Cache: Read 1.2K | Write 0.5K                        │
│                                                              │
│  ↑↓ Navigate  Enter Inspect  Esc Back  a Agent  / Search   │
└─────────────────────────────────────────────────────────────┘
```

**关键元素**：
- 按 Turn 分组显示
- 记录行：#索引 时间 类型 描述 token数
- 当前选中记录用 `>` 标记
- Turn 标题显示总耗时
- 底部统计栏（总 token、TTFT、Cache）
- 进行中的 turn 标记 "(in progress)"

### 视图 3：检查器模式

```
┌─────────────────────────────────────────────────────────────┐
│ Zenpi TUI  [Gantt → worker-2 → Turn 3 → #18]      [Content]│
│ Esc Back          [Tab] Content│Metrics│Raw                 │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│ ┌─ Input ──────────────────────────────────────────────────┐│
│ │ Type: ASSISTANT (response)                               ││
│ │ Request: Execute task-3                                  ││
│ │                                                          ││
│ │ Task 3 has been completed successfully. The output file  ││
│ │ has been written to output.txt with 1024 bytes.          ││
│ │                                                          ││
│ │ All validation checks passed.                            ││
│ └──────────────────────────────────────────────────────────┘│
│                                                              │
│ ┌─ Output ─────────────────────────────────────────────────┐│
│ │ Status: Success ✓                                        ││
│ │ Blocks: 1 text                                           ││
│ └──────────────────────────────────────────────────────────┘│
│                                                              │
│ ┌─ Metrics ────────────────────────────────────────────────┐│
│ │ Tokens:    Input 2500  Output 1800  Reasoning 0          ││
│ │ Cache:     Read 0      Write 0                           ││
│ │ Timing:    TTFT 0.28s  Total 3.45s   Throughput 521 t/s ││
│ │ Error:     None                                          ││
│ └──────────────────────────────────────────────────────────┘│
│                                                              │
│  Esc Back  Tab Switch  ↑↓ Scroll  y Copy                   │
└─────────────────────────────────────────────────────────────┘
```

**关键元素**：
- 三个标签页：Content、Metrics、Raw
- Input 区域：显示输入内容或请求信息
- Output 区域：显示输出内容或结果状态
- Metrics 区域：详细的 token、cache、timing 信息
- 支持滚动查看长内容

### Agent 选择器（按 `a` 打开）

```
┌─────────────────────────────────────────────────────────────┐
│ Zenpi TUI  [Gantt → worker-2]                               │
│ ╔══════════════════════════════════════════════════════════╗│
│ ║ Select Agent                                             ║│
│ ║──────────────────────────────────────────────────────────║│
│ ║ Filter: _                                                ║│
│ ║                                                          ║│
│ ║   master          [running]   █████████░   90%          ║│
│ ║   worker-1        [completed] ██████████  100%          ║│
│ ║ > worker-2 ●      [running]   ███████░░░   75%          ║│
│ ║   worker-3        [running]   █████░░░░░   50%          ║│
│ ║   worker-4        [pending]   ░░░░░░░░░░    0%          ║│
│ ║                                                          ║│
│ ║ ↑↓ Select  Enter Confirm  Esc Cancel  Type to filter    ║│
│ ╚══════════════════════════════════════════════════════════╝│
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

**关键元素**：
- 过滤输入框（支持模糊搜索）
- Agent 列表：名称、状态、进度条、百分比
- 当前 agent 用 ● 标记
- 选中项用 > 标记

## 快捷键完整定义

### 全局快捷键

| 按键 | 功能 | 说明 |
|------|------|------|
| `Ctrl+G` | 返回甘特图 | 从任何视图快速返回 |
| `Ctrl+T` | 打开轨迹视图 | 针对当前选中的 agent |
| `Ctrl+Q` | 退出 | 退出 Zenpi TUI |
| `Ctrl+R` | 刷新 | 刷新当前视图 |
| `?` | 显示帮助 | 显示快捷键帮助面板 |

### 视图切换

| 按键 | 功能 | 说明 |
|------|------|------|
| `1` | 甘特图模式 | 快速切换到甘特图 |
| `2` | 轨迹模式 | 快速切换到轨迹列表 |
| `3` | 检查器模式 | 快速切换到检查器（需先选中记录）|
| `Esc` | 返回上一级 | 检查器→轨迹→甘特图 |
| `Tab` | 切换标签页 | 在检查器中切换 Content/Metrics/Raw |

### 导航

| 按键 | 功能 | 说明 |
|------|------|------|
| `↑`/`k` | 向上 | Vim 风格可选 |
| `↓`/`j` | 向下 | Vim 风格可选 |
| `PageUp` | 向上翻页 | 一次翻一屏 |
| `PageDown` | 向下翻页 | 一次翻一屏 |
| `Home`/`gg` | 跳到开头 | Vim 风格可选 |
| `End`/`G` | 跳到结尾 | Vim 风格可选 |
| `g` + 数字 | 跳到指定记录 | 例如 g18 跳到 #18 |

### 操作

| 按键 | 功能 | 说明 |
|------|------|------|
| `Enter` | 选择/打开 | 甘特图→轨迹，轨迹→检查器 |
| `a` | 切换 Agent | 打开 agent 选择器 |
| `p` | 暂停 Agent | 暂停当前选中的 agent |
| `r` | 恢复 Agent | 恢复暂停的 agent |
| `/` | 搜索 | 在轨迹中搜索记录 |
| `n` | 下一个结果 | 搜索结果导航 |
| `N` | 上一个结果 | 搜索结果导航 |
| `y` | 复制 | 复制当前内容到剪贴板 |

## 数据结构定义

### Agent State

```typescript
interface AgentState {
  id: string              // agent ID
  name: string            // 显示名称（master, worker-1, ...）
  status: 'running' | 'completed' | 'pending' | 'paused' | 'failed'
  progress: number        // 0-100
  startTime: number       // Unix timestamp (ms)
  duration: number        // 持续时间（秒）
  tokenUsage: {
    input: number
    output: number
    reasoning: number
    cacheRead: number
    cacheWrite: number
  }
  turnCount: number       // 总轮次数
  recordCount: number     // 总记录数
}
```

### Trajectory Record

```typescript
interface TrajectoryRecord {
  index: number           // 记录索引 #1, #2, ...
  recordId: string        // 唯一 ID
  turn: number            // 所属轮次
  timestamp: number       // Unix timestamp (ms)
  type: 'REQUEST' | 'TOOL' | 'ASSISTANT' | 'SYSTEM' | 'USER'
  description: string     // 简短描述
  
  // Token 信息
  tokens?: {
    input?: number
    output?: number
    reasoning?: number
    cacheRead?: number
    cacheWrite?: number
  }
  
  // 计时信息
  timing?: {
    duration: number      // 持续时间（秒）
    ttft?: number         // Time to first token（秒）
    throughput?: number   // tokens/s
  }
  
  // 详细内容（用于检查器）
  details?: {
    input?: string
    output?: string
    error?: string
    metadata?: Record<string, any>
  }
  
  // 状态
  status: 'success' | 'error' | 'in-progress'
}
```

### Turn Group

```typescript
interface TurnGroup {
  turn: number            // 轮次编号
  startTime: number       // 开始时间
  duration: number        // 持续时间（秒）
  recordCount: number     // 该轮次的记录数
  records: TrajectoryRecord[]
  status: 'completed' | 'in-progress'
}
```

## Agent 控制接口

```typescript
interface AgentController {
  // 暂停 agent
  pause(agentId: string): Promise<boolean>
  
  // 恢复 agent
  resume(agentId: string): Promise<boolean>
  
  // 获取 agent 状态
  getStatus(agentId: string): AgentState
  
  // 订阅 agent 状态变化
  subscribe(agentId: string, callback: (state: AgentState) => void): Unsubscribe
}
```

## 实现优先级

### Phase 1: 基础框架（1-2 周）

- [ ] blessed 应用框架搭建
- [ ] 视图切换系统（甘特图/轨迹/检查器）
- [ ] 面包屑导航
- [ ] 基础键盘快捷键

### Phase 2: 甘特图（1 周）

- [ ] Agent 列表显示
- [ ] 进度条渲染
- [ ] 状态更新
- [ ] 基础导航

### Phase 3: 轨迹列表（2 周）

- [ ] ZenpiTrajectoryAdapter 实现
- [ ] Turn 分组显示
- [ ] 记录行渲染
- [ ] 虚拟滚动
- [ ] 统计栏

### Phase 4: 检查器（1 周）

- [ ] 三标签页布局
- [ ] 内容显示
- [ ] 指标显示
- [ ] 滚动支持

### Phase 5: Agent 控制（1 周）

- [ ] Agent 选择器
- [ ] 暂停/恢复接口
- [ ] 状态同步

### Phase 6: 搜索（1 周）

- [ ] 搜索输入框
- [ ] 记录过滤
- [ ] 结果导航

## 测试计划

### 单元测试

- [ ] 数据模型转换（ZenpiTrajectoryAdapter）
- [ ] 格式化函数（时间、token 数）
- [ ] 状态管理

### 集成测试

- [ ] 视图切换流程
- [ ] Agent 控制流程
- [ ] 搜索功能

### 端到端测试

- [ ] 完整用户流程（甘特图→轨迹→检查器）
- [ ] 多 Agent 并发监控
- [ ] 长会话性能

### 手动测试

- [ ] 不同终端兼容性（iTerm2, Terminal.app, tmux）
- [ ] 不同窗口大小
- [ ] 快捷键冲突检查

## 文档清单

- [x] INTERACTION_DESIGN.md - 本文档
- [ ] IMPLEMENTATION_PLAN.md - 实现计划
- [ ] API_REFERENCE.md - API 文档
- [ ] USER_GUIDE.md - 用户指南
- [ ] DEVELOPER_GUIDE.md - 开发者指南

## 下一步

1. **创建 UI Mockup**：使用实际的 blessed 代码绘制原型
2. **定义数据接口**：Zenpi Agent → TrajectoryAdapter 的详细接口
3. **搭建基础框架**：blessed 应用结构 + 视图路由
4. **实现甘特图**：作为第一个可见的功能点

准备好开始实现了吗？
