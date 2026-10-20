# UI-Trajectory 包架构学习笔记

## What

### 目的

`ui-trajectory` 是 deepseek-harness 的轨迹可视化组件，用于展示 Agent 对话过程中的完整事件记录。它提供：
- 按轮次（turn）组织的事件记录表
- 交互式时间概览（timeline）
- 详细检查器（inspector）显示 token 用量、耗时、输入输出等

### 入口点

**主组件**：`TrajectoryView.tsx` - 轨迹视图的主容器组件
**数据契约**：`trajectory-contract.ts` - 定义所有核心类型和接口
**快照构建**：`trajectory-snapshot-builder.ts` - 从会话事件构建轨迹快照

### 核心数据流

```
会话事件 (Session Events)
    ↓
TrajectoryConversationViewNode (投影节点)
    ↓
TrajectorySnapshot (快照数据)
    ↓
TrajectoryTurnModel (轮次模型，包含分组和单元格)
    ↓
TrajectoryTable + TrajectoryTimeline (UI 渲染)
```

## 关键抽象

### 1. TrajectoryContribution（贡献类型）

轨迹记录的基本单元，支持多种类型：
- `system-prompt` - 系统提示词
- `node` - 一般会话节点
- `assistant` - 助手消息（包含部分流式输出）
- `tool` - 工具调用
- `request-header` - 请求头（包含 seq、time、prompt、location）
- `compaction` - 压缩请求
- `session-end` - 会话结束
- `turn-end` - 轮次结束

源文件：`trajectory-contract.ts:23-58`

### 2. TrajectoryCellProps（单元格属性）

每个轨迹记录的完整数据模型：
- **身份**：index (1-based)、recordId、sourceSeq、callId
- **类型**：kind (system/user/context/compacted/message/tool/subtool)
- **内容**：text、previewMarkdown、inputDetail、outputDetail、thinkingDetail
- **计时**：timeSeconds、startedAt、assistantMetrics (TTFT、decode throughput)
- **用量**：input、output、think、cacheRead、cacheWrite tokens
- **状态**：isError、selected

源文件：`trajectory-record.ts:40-105`

### 3. TrajectorySnapshot（轨迹快照）

从会话状态投影的完整轨迹数据：
- `systemPrompts` - 未加载请求头时的独立系统提示词
- `eventNodes` - 完整事件节点列表
- `eventLocations` - 事件位置映射（seq → location）
- `requests` - 所有请求视图
- `callSchemas` - 工具调用的模式定义
- `partial` - 进行中的助手输出
- `runningCalls` - 运行中的工具调用

源文件：`trajectory-contract.ts:69-78`

### 4. TrajectoryTurnModel（轮次模型）

UI 层的轮次组织结构：
- 每个轮次包含多个 group（分组）
- 每个 group 包含多个 cell（单元格/记录）
- 提供折叠/展开、选择等交互能力

源文件：`layout.ts`（推断）

## 核心组件

### TrajectoryView（主视图）

**职责**：
- 管理视图状态（折叠、选择、搜索、时间线选择）
- 协调 Table、Timeline、Toolbar 子组件
- 处理历史加载（按需加载更早的页面）
- 维护搜索索引

**关键状态**：
```typescript
- collapsedTurns: Set<number>           // 折叠的轮次
- collapsedAssistants: Set<string>      // 折叠的助手记录
- timelineSelection: TimeRange | null   // 时间线选择的区间
- searchQuery: string                   // 搜索查询
- selectedTimelineIndex: number | null  // 选中的时间线索引
```

源文件：`TrajectoryView.tsx:132-150`

### TrajectoryTable（记录表）

**职责**：
- 渲染轮次组织的事件记录表
- 虚拟化长列表（只渲染可见行）
- 显示请求编号和累计用量
- 处理记录选择和检查器展开

源文件：`TrajectoryTable.tsx`

### TrajectoryTimeline（时间概览）

**职责**：
- 投影记录的真实开始时间和耗时
- 区分 TTFT 和解码时间
- 支持拖选区间、缩放、平移
- 悬停显示精确时刻和耗时详情

源文件：`TrajectoryTimeline.tsx`

## 数据依赖

### 外部依赖

1. **conversation 系统**：
   - `ConversationNode` - 会话节点
   - `ConversationPromptSnapshot` - 提示词快照
   - `RequestView` - 请求视图
   - `AssistantMessageNode` - 助手消息节点
   - `ToolCallBlock` - 工具调用块

2. **slots 系统**：
   - `SnapshotSelectorHook` - 快照选择器钩子
   - `renderSlot` - 插槽渲染（用于图片等）

3. **attachment 系统**：
   - `ImageAttachmentRef` - 图片附件引用
   - `FileAttachmentRef` - 文件附件引用

源文件：`trajectory-contract.ts:1-6`

## 关键特性

### 1. 虚拟化渲染

- 长记录表只挂载可见行窗口加少量缓冲
- 初始从尾部加载 50 个 target Node
- 向前补页时保持键和 ARIA 索引不变
- 流式更新时只更新内容，不重复写入滚动位置

源文件：README.zh.md:65-66，`trajectory-virtual-rows.ts`

### 2. 按需历史加载

- 初始定位到尾部
- 首行控件点击加载更早的一页历史
- 加载期间显示共享的 ongoing loading 状态
- 页面大小：`HISTORY_PAGE_NODES = 50`

源文件：`TrajectoryView.tsx:35`

### 3. 时间线交互

- 拖选区间：聚焦到闭区间内任何时刻处于活动的记录
- 滚轮缩放：缩放时间域
- 右键清除：清除选中区间
- 右键拖动：平移已放大的视口
- 自动跟随：流式更新时停留在尾部，向上滚动暂停跟随

源文件：README.zh.md:46

### 4. 检查器（Inspector）

显示选中记录的详细信息：
- Token 用量（input、output、reasoning、cache read/write）
- 耗时（TTFT、decode time）
- 输入输出内容（Markdown 渲染）
- 思考内容（紧凑 Markdown，固定字号）
- 图片和附件（使用 slot 系统）
- 工具模式（call-time schema）

源文件：README.zh.md:31-40

### 5. 搜索功能

- `TrajectorySearchIndex` 维护可搜索索引
- 节流更新（`SEARCH_INDEX_THROTTLE_MS = 3000`）
- 覆盖 React 可见窗口（不是完整历史）

源文件：`TrajectoryView.tsx:34, 146`，`trajectory-search-index.ts`

## 副作用和风险

### 副作用

1. **投影开销**：从会话事件构建轨迹快照需要遍历和转换
2. **搜索索引**：维护索引需要额外内存和计算
3. **虚拟化复杂性**：需要精确的高度测量和滚动位置管理

### 风险

1. **内存占用**：长会话历史可能占用大量内存
2. **渲染性能**：复杂的嵌套工具调用可能影响渲染性能
3. **同步问题**：流式更新时需要保持 UI 和数据同步

### 未知项

1. **压缩机制**：compaction 的具体实现和触发条件
2. **嵌套工具**：subtool 的深度限制和展示策略
3. **缓存策略**：图片和附件的缓存策略细节
4. **错误恢复**：网络错误或数据损坏时的恢复机制

## 适配到 Zenpi TUI 的关键考虑

### 1. 从 Web UI 到 TUI

- **布局**：不能使用 CSS Grid/Flexbox，需要使用 TUI 布局库（如 blessed、ink）
- **虚拟化**：TUI 通常自带虚拟滚动，但需要适配
- **交互**：鼠标拖选改为键盘导航
- **渲染**：Markdown 需要转换为纯文本或简化的 ANSI 格式

### 2. 数据模型复用

可以直接复用的抽象：
- `TrajectoryCellProps` - 单元格数据模型（与 UI 无关）
- `TrajectorySnapshot` - 快照数据结构
- `TrajectoryContribution` - 贡献类型系统
- 轮次和分组的概念

### 3. 核心逻辑提取

需要提取的纯逻辑：
- `trajectory-snapshot-builder.ts` - 快照构建逻辑
- `trajectory-event-projection.ts` - 事件投影逻辑
- `trajectory-record.ts` - 记录格式化函数
- `timeline.ts` - 时间线计算逻辑（可简化）

### 4. 功能优先级

对 Zenpi TUI 最重要的功能：
1. **核心轨迹展示**：显示完整的上下文轨迹
2. **KV cache 校验**：显示 cache read/write，帮助诊断不吻合问题
3. **按需加载**：支持长历史的分页加载
4. **基本导航**：上下滚动、跳转到特定记录
5. **检查器**：查看选中记录的详细信息

可以暂缓的功能：
- 复杂的时间线可视化（可简化为时间戳列表）
- 搜索功能（初期可不实现）
- 图片和附件预览（TUI 限制）
- 嵌套工具的完整展开（可平铺显示）

### 5. 集成接口

Zenpi TUI 需要提供：
- **数据源**：Agent 的会话事件流
- **存储**：轨迹快照的持久化（可选）
- **导航**：与主界面的集成（侧边栏或全屏模式）
- **权限**：访问 Agent 内部状态的权限

## 下一步

- [ ] 详细分析 `trajectory-snapshot-builder.ts` 的快照构建逻辑
- [ ] 理解 `trajectory-event-projection.ts` 的事件投影机制
- [ ] 研究虚拟化实现（`trajectory-virtual-rows.ts`）
- [ ] 提取可复用的数据模型和工具函数
- [ ] 设计 Zenpi TUI 轨迹面板的 UI mockup
- [ ] 定义 Zenpi Agent 到轨迹系统的数据适配器
