# Zenpi 轨迹功能交互设计

## 背景理解

### Zenpi 的定位

**Zenpi** = 专业工程开发 Agent Harness for b3ehive

- **传统 b3ehive**：使用系统 cron + 计算机组件（进程、文件系统等）
- **Zenpi TUI**：优化集成这些能力，提供可视化监控和引导

### 核心场景

```
b3ehive 编排（compete/execution/learn/optimization/looper）
    ↓ 生成
多个 subagent 并行/串行工作
    ↓ Zenpi TUI 提供
1. 甘特图 - 查看所有 subagent 的任务时间线
2. 轨迹视图 - 深入查看单个 agent 的完整上下文
    ↓ 用户能力
引导/监督/诊断 agent 工作
```

## 交互设计理念

### 设计原则

1. **层次化导航**：Overview → Detail → Deep Dive
   - L1: 甘特图 - 所有 agent 的宏观视图
   - L2: 轨迹列表 - 单个 agent 的操作历史
   - L3: 检查器 - 单条记录的完整细节

2. **快速切换**：在多个 agent 之间快速跳转
   - 甘特图上选中 agent → 直接跳到其轨迹
   - 轨迹视图中跳转到其他 agent
   - 关联操作快速导航（parent/child agent）

3. **实时监控**：流式更新，无需手动刷新
   - 新记录自动追加
   - 进行中的操作实时更新
   - 错误和异常立即高亮

4. **引导能力**：不只是查看，还能介入
   - 暂停/继续 agent
   - 修改参数（需要权限）
   - 插入指令（steering）
   - 标记问题和决策点

## 核心交互流程

### 流程 1：从甘特图到轨迹

```
用户在甘特图中查看所有 subagent
    ↓ 发现某个 agent 状态异常或感兴趣
选中该 agent（Enter 或 点击）
    ↓ 打开轨迹视图
显示该 agent 的完整上下文轨迹
    ↓ 需要查看详细信息
选中某条记录（Enter）
    ↓ 打开检查器
查看输入/输出/token 用量/KV cache
    ↓ 发现问题
标记问题（m）或插入引导（i）
```

### 流程 2：KV Cache 诊断

```
用户察觉某个 agent 响应异常
    ↓ 快捷键 Ctrl+T 打开轨迹视图
在轨迹列表中查看 cache 统计
    ↓ 发现 cache read 异常低
高亮显示缓存不一致的记录（红色标记）
    ↓ 选中异常记录
检查器显示详细的 cache 信息
    - 预期 cache: xxx tokens
    - 实际 cache: yyy tokens
    - 差异原因: [prompt 变更 / context 截断]
    ↓ 决策
[选项 1] 重启 agent with cache reset
[选项 2] 调整 prompt 策略
[选项 3] 记录问题，继续监控
```

### 流程 3：多 Agent 协作监控

```
用户运行 execution-cron-builder
    ↓ Zenpi TUI 显示
甘特图：master + 3 个 worker agent
    ↓ 用户想监控 worker-2
选中 worker-2 → 打开轨迹视图（分屏）
    ↓ 同时监控
左侧：甘特图（宏观进度）
右侧：轨迹视图（worker-2 细节）
    ↓ worker-2 完成 claim
轨迹自动更新，显示新的 submit 记录
    ↓ 用户切换监控目标
Tab 返回甘特图 → 选中 worker-3 → 再次打开轨迹
```

## 界面布局设计

### 方案 A：分屏模式（推荐）

```
┌─────────────────────────────────────────────────────────────┐
│ Zenpi TUI - Trajectory View          [Agent: worker-2]      │
├────────────────────┬────────────────────────────────────────┤
│  甘特图 (30%)     │  轨迹列表 (70%)                        │
│                    │                                        │
│  master ████      │  Turn 3 ▼                              │
│  worker-1 ███     │    #15 [REQUEST] Execute task-3       │
│  worker-2 ████    │    #16 [TOOL] read_file config.yaml   │
│  worker-3 ██      │    #17 [TOOL] write_file output.txt   │
│                    │    #18 [ASSISTANT] Task completed ✓   │
│  [进行中]         │                                        │
│  worker-2: 50%    │  Turn 4 ▼ (collapsed)                 │
│                    │                                        │
│  [快捷键]         │  Cache Stats: Read 1.2K / Write 0.5K  │
│  ↑↓ 导航          │  Total Tokens: 2.5K (in) 1.8K (out)   │
│  Enter 查看       │                                        │
│  Tab 切换面板     │  ↑↓:Navigate Enter:Inspect /:Search   │
└────────────────────┴────────────────────────────────────────┘
```

**优点**：
- 保持上下文（甘特图始终可见）
- 快速切换 agent
- 空间利用率高

**缺点**：
- 甘特图显示受限

### 方案 B：全屏切换模式

```
[甘特图模式 - 快捷键 '1']
┌─────────────────────────────────────────────────────────────┐
│ Zenpi TUI - Gantt Chart                       [5 agents]    │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  master     ████████████████████████ [running]              │
│  worker-1   █████████░░░░░░░░░░░░░░ [completed]            │
│  worker-2   ███████████████░░░░░░░░ [running] ◄ 选中        │
│  worker-3   ████████░░░░░░░░░░░░░░░ [running]              │
│  worker-4   ██░░░░░░░░░░░░░░░░░░░░░ [pending]              │
│                                                              │
│  Timeline: 00:00 ──────────────────> 02:30 (current)        │
│                                                              │
│  快捷键: ↑↓ 选择  Enter 查看轨迹  Space 暂停/继续          │
└─────────────────────────────────────────────────────────────┘

[轨迹模式 - 快捷键 '2' 或 Enter from 甘特图]
┌─────────────────────────────────────────────────────────────┐
│ Zenpi TUI - Trajectory View          [Agent: worker-2]      │
│ ← Back to Gantt (Esc)    Switch Agent: [1]master [3]wrk-3  │
├─────────────────────────────────────────────────────────────┤
│  Turn 3 ▼                                          [Cache ✓]│
│    #15  10:23:45  [REQUEST] Execute task-3         2.5K in  │
│    #16  10:23:46  [TOOL] read_file                   0.2K  │
│    #17  10:24:01  [TOOL] write_file                  0.1K  │
│    #18  10:24:05  [ASSISTANT] Completed ✓           1.8K out│
│                                                     ━━━━━━━  │
│  Turn 4 ▶ (collapsed)                              0.5K CR  │
│                                                     0.3K CW  │
│  📊 Stats:  Total: 4.8K tokens  |  TTFT: 0.3s  |  TPM: 450  │
│  🗂️  Cache: Read 0.5K (10%)  Write 0.3K  |  Hit Rate: 95%   │
│                                                              │
│  ↑↓ Navigate  Enter Inspect  Space Toggle  / Search  Esc Back│
└─────────────────────────────────────────────────────────────┘

[检查器模式 - 快捷键 '3' 或 Enter from 轨迹]
┌─────────────────────────────────────────────────────────────┐
│ Inspector - Record #17                   [Agent: worker-2]   │
│ ← Back (Esc)     [Tab] Content│Metrics│Raw                  │
├─────────────────────────────────────────────────────────────┤
│ ┌─ Input ──────────────────────────────────────────────────┐│
│ │ Tool: write_file                                         ││
│ │ Args:                                                    ││
│ │   path: "output.txt"                                     ││
│ │   content: "Task 3 completed successfully.\n..."         ││
│ └──────────────────────────────────────────────────────────┘│
│                                                              │
│ ┌─ Output ─────────────────────────────────────────────────┐│
│ │ Success: true                                            ││
│ │ Bytes written: 1024                                      ││
│ └──────────────────────────────────────────────────────────┘│
│                                                              │
│ ┌─ Metrics ────────────────────────────────────────────────┐│
│ │ Duration: 0.15s  │  Tokens: 100 (in) + 50 (out)         ││
│ │ Cache: Read 0  Write 0  │  Error: None                  ││
│ └──────────────────────────────────────────────────────────┘│
│                                                              │
│  Esc Back  Tab Switch  ↑↓ Scroll  y Copy                   │
└─────────────────────────────────────────────────────────────┘
```

**优点**：
- 每个模式空间充足，信息完整
- 清晰的导航路径
- 适合深入分析

**缺点**：
- 需要来回切换
- 失去其他视图的上下文

### 方案 C：浮动面板模式（混合）

```
[基础视图：甘特图]
┌─────────────────────────────────────────────────────────────┐
│ Zenpi TUI - Gantt Chart                                     │
├─────────────────────────────────────────────────────────────┤
│  master     ████████████████████████ [running]              │
│  worker-1   █████████░░░░░░░░░░░░░░ [completed]            │
│  worker-2   ███████████████░░░░░░░░ [running] ◄             │
│  ...                                                         │
└─────────────────────────────────────────────────────────────┘

[快捷键 't' 打开轨迹浮动面板]
┌─────────────────────────────────────────────────────────────┐
│ Zenpi TUI - Gantt Chart                                     │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ Trajectory: worker-2              [x] Close        │   │
│  ├─────────────────────────────────────────────────────┤   │
│  │ Turn 3 ▼                                           │   │
│  │   #15 [REQUEST] Execute task-3                     │   │
│  │   #16 [TOOL] read_file                             │   │
│  │   #17 [TOOL] write_file                            │   │
│  │ Cache: 0.5K read / 0.3K write                      │   │
│  │ ↑↓ Navigate  Enter Details  Esc Close             │   │
│  └─────────────────────────────────────────────────────┘   │
│  worker-3   ████████░░░░░░░░░░░░░░░ [running]              │
└─────────────────────────────────────────────────────────────┘
```

**优点**：
- 保持甘特图上下文
- 快速查看轨迹
- 灵活调整大小和位置

**缺点**：
- 遮挡部分甘特图
- 实现复杂度高

## 推荐方案：分层导航 + 面包屑

结合方案 B（全屏切换）和快速导航：

```
┌─────────────────────────────────────────────────────────────┐
│ Zenpi TUI  [Gantt → worker-2 → Turn 3 → #17]     [KV: ✓]   │
├─────────────────────────────────────────────────────────────┤
│ [当前视图内容]                                              │
│                                                              │
│ [底部状态栏]                                                │
│ View: [1]Gantt [2]Trajectory [3]Inspector  |  Quick: g/a/s  │
└─────────────────────────────────────────────────────────────┘
```

**面包屑导航**：
- 点击任意层级快速跳转
- 显示当前位置和上下文
- 视觉上连贯

**快速跳转**：
- `g` - 返回甘特图（Go back）
- `a` - 切换 agent（Agent switch）
- `s` - 搜索记录（Search）

## 关键交互细节

### 1. Agent 选择器

```
[在轨迹视图中按 'a' 打开]
┌─────────────────────────────────────┐
│ Select Agent                        │
├─────────────────────────────────────┤
│ > master          [running] 100%    │
│   worker-1        [done]    100%    │
│   worker-2 ●      [running]  75%    │
│   worker-3        [running]  50%    │
│   worker-4        [pending]   0%    │
│                                     │
│ Type to filter: _                   │
│ Enter Select  Esc Cancel            │
└─────────────────────────────────────┘
```

**特性**：
- 显示所有 agent 及状态
- 当前 agent 标记（●）
- 模糊搜索
- 显示进度百分比

### 2. 轨迹记录行设计

```
基础信息行：
#17  10:24:01  [TOOL] write_file output.txt              0.1K  0.15s

展开状态：
#17  10:24:01  [TOOL] write_file output.txt              0.1K  0.15s  ▼
     Input:  path="output.txt" content="Task 3 comp..."
     Output: Success: true, Bytes: 1024
     Cache:  Read 0  Write 0
```

**信息层次**：
- **第1层**（一行）：索引、时间、类型、操作、token、耗时
- **第2层**（展开）：输入输出摘要、缓存信息
- **第3层**（检查器）：完整内容、指标、原始数据

### 3. KV Cache 可视化

```
Cache 统计面板（按 'c' 打开）：
┌─────────────────────────────────────────────┐
│ KV Cache Statistics - worker-2              │
├─────────────────────────────────────────────┤
│ Total Read:   1.2K tokens (15 requests)     │
│ Total Write:  0.5K tokens (8 requests)      │
│                                             │
│ Hit Rate:  95.2%  ████████████████░░        │
│                                             │
│ Cache Timeline:                             │
│ #12 ━━━ 0.3K read                          │
│ #13 ━━━ 0.1K read                          │
│ #14 ━━━━ 0.2K read + 0.1K write           │
│ #15 ━━ 0.1K read                           │
│ #16 ━━━━━ 0.5K read + 0.2K write  ⚠️       │
│     └─ Warning: Expected 0.8K read         │
│                                             │
│ Issues:  1 mismatch detected at #16         │
│          Possible cause: Prompt modified    │
│                                             │
│ Enter Details  Esc Close                    │
└─────────────────────────────────────────────┘
```

**高亮规则**：
- 绿色：缓存命中率高（>90%）
- 黄色：缓存命中率中等（70-90%）
- 红色：缓存命中率低（<70%）或检测到不一致

### 4. 引导和标记功能

```
[在记录上按 'm' 标记]
┌──────────────────────────────────────┐
│ Mark Record #17                      │
├──────────────────────────────────────┤
│ Type: [✓] Issue  [ ] Decision        │
│       [ ] Question  [ ] Note         │
│                                      │
│ Comment:                             │
│ ┌────────────────────────────────┐  │
│ │ File write took longer than    │  │
│ │ expected. Investigate disk I/O │  │
│ └────────────────────────────────┘  │
│                                      │
│ Tags: #performance #io              │
│                                      │
│ Ctrl+S Save  Esc Cancel             │
└──────────────────────────────────────┘

[标记后显示]
#17  10:24:01  [TOOL] write_file output.txt  🏷️  0.1K  0.15s
     └─ Issue: File write took longer than expected
```

**用途**：
- 记录问题和决策点
- 团队协作和知识传递
- 后期复盘和优化

## 快捷键方案

### 全局快捷键

```
Ctrl+T    打开轨迹视图
Ctrl+G    返回甘特图
Ctrl+Q    退出
Ctrl+R    刷新
?         显示帮助
```

### 视图切换

```
1         甘特图模式
2         轨迹模式
3         检查器模式
Esc       返回上一级
Tab       切换面板/标签页
```

### 导航

```
↑↓        上下移动
←→        左右切换（在检查器中切换标签页）
PageUp    向上翻页
PageDown  向下翻页
Home      跳到开头
End       跳到结尾
g + 数字  跳到指定记录（例如 g17 跳到 #17）
```

### 操作

```
Enter     选择/打开
Space     折叠/展开
/         搜索
n         下一个搜索结果
N         上一个搜索结果
a         切换 agent
c         打开 cache 统计
m         标记当前记录
i         插入引导（steering）
p         暂停 agent
r         恢复 agent
y         复制内容
```

### Vim 风格（可选）

```
j/k       下/上移动（替代 ↑↓）
h/l       左/右移动（替代 ←→）
gg        跳到开头（替代 Home）
G         跳到结尾（替代 End）
/         搜索
:q        退出
:w        保存标记
```

## 下一步实现

1. **UI Mockup**：使用 ASCII art 绘制完整的界面原型
2. **数据流图**：设计 Zenpi Agent → Trajectory Adapter 的数据流
3. **blessed 组件树**：定义完整的组件层次结构
4. **快捷键配置**：实现可定制的快捷键系统

你觉得这个交互设计方向如何？有哪些需要调整或补充的？
