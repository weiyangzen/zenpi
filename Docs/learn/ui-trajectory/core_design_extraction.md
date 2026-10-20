# UI-Trajectory 核心设计提取

## 核心数据模型（可直接复用）

### 1. 轨迹记录（Trajectory Record）

每条轨迹记录的完整数据结构：

```typescript
interface TrajectoryRecord {
  // 身份标识
  index: number              // 1-based 记录索引
  recordId?: string          // 稳定的记录 ID
  sourceSeq?: number         // 源事件序列号
  callId?: string            // 工具调用 ID
  
  // 类型和内容
  kind: 'system' | 'user' | 'context' | 'compacted' | 'message' | 'tool' | 'subtool'
  text: string               // 单行摘要
  previewMarkdown?: string   // Markdown 预览
  
  // 详细内容（用于检查器）
  inputDetail?: string       // 完整输入
  outputDetail?: string      // 完整输出
  thinkingDetail?: string    // 思考内容
  
  // 计时信息
  timeSeconds: number | null  // 持续时间（秒）
  startedAt?: number | null   // 开始时间（Unix 毫秒）
  assistantMetrics?: {        // 助手指标
    timingRecorded: boolean
    stepStartTime: number | null
    firstTokenTime: number | null  // TTFT
    completedTime: number | null
    usageProvided: boolean
    outputTokens: number | null
  }
  
  // Token 用量
  input?: number         // 输入 tokens
  output?: number        // 输出 tokens
  think?: number         // 推理 tokens
  cacheRead?: number     // 缓存读取 tokens
  cacheWrite?: number    // 缓存写入 tokens
  
  // 状态
  isError?: boolean      // 是否错误
  selected?: boolean     // 是否选中
}
```

**来源**：`trajectory-record.ts:40-105`

**TUI 适配要点**：
- 所有字段都是与 UI 无关的数据
- 可以直接用于 TUI 的数据层
- `previewMarkdown` 在 TUI 中可转为纯文本或简化的 ANSI 格式

### 2. 轨迹快照（Trajectory Snapshot）

从 Agent 会话投影出的完整轨迹状态：

```typescript
interface TrajectorySnapshot {
  // 系统提示词（未加载请求头时）
  systemPrompts?: readonly SystemPrompt[]
  
  // 核心事件列表
  eventNodes: readonly ConversationNode[]
  eventLocations: ReadonlyMap<number, ConversationLocation>
  
  // 请求和工具
  requests: readonly RequestView[]
  callSchemas: ReadonlyMap<string, ToolSchema>
  
  // 流式状态
  partial: PartialAssistant | null      // 进行中的助手输出
  runningCalls: readonly RunningToolCall[]  // 运行中的工具调用
}
```

**来源**：`trajectory-contract.ts:69-78`

**TUI 适配要点**：
- 这是核心数据结构，与渲染无关
- `eventNodes` 包含所有历史事件
- `partial` 和 `runningCalls` 用于显示进行中的操作

### 3. 贡献类型（Contribution Types）

轨迹中不同类型的事件：

```typescript
type TrajectoryContribution =
  | { kind: 'system-prompt'; prompt: SystemPromptNode }
  | { kind: 'node'; node: ConversationNode }
  | { kind: 'assistant'; node?: AssistantMessageNode; partial: PartialAssistant | null; request?: Request }
  | { kind: 'tool'; root: ToolCallBlock }
  | { kind: 'request-header'; header: RequestHeaderState }
  | { kind: 'compaction'; request: CompactionRequest }
  | { kind: 'session-end'; seq: number; time: number }
  | { kind: 'turn-end'; turn: number; time: number; error?: string; errorCode?: string }
```

**来源**：`trajectory-contract.ts:23-58`

**TUI 适配要点**：
- 这定义了轨迹中所有可能的事件类型
- 每种类型有不同的渲染方式
- TUI 实现时可以为每种类型创建对应的格式化函数

## 核心逻辑（需提取和适配）

### 1. 快照构建器（Snapshot Builder）

**职责**：从会话事件流构建轨迹快照

**关键函数**：
- `buildTrajectorySnapshot()` - 主构建函数
- `interruptCompactions()` - 处理被中断的压缩请求
- `applyTurnErrors()` - 应用轮次错误到请求
- `captureSchemas()` - 捕获工具调用的模式

**来源**：`trajectory-snapshot-builder.ts`

**TUI 适配策略**：
- 保留核心构建逻辑
- 简化 Web UI 特有的处理（如 React 状态）
- 使用 Zenpi 的事件格式作为输入

### 2. 事件投影（Event Projection）

**职责**：将原始会话事件投影为轨迹视图节点

**关键逻辑**：
- 从会话窗口读取事件
- 按类型分类和转换
- 维护轮次和步骤边界
- 处理流式更新

**来源**：`trajectory-event-projection.ts`

**TUI 适配策略**：
- 提取纯投影逻辑
- 去除 UI 框架依赖
- 适配 Zenpi 的事件模型

### 3. 布局派生（Layout Derivation）

**职责**：从快照派生出轮次和分组的层级结构

**关键函数**：
- `deriveTrajectoryLayout()` - 派生完整布局
- `appendTrajectoryPartialLayout()` - 追加流式更新

**来源**：`layout.ts`

**TUI 适配策略**：
- 保留轮次和分组的概念
- 简化嵌套结构（TUI 通常是平铺的）
- 调整为适合终端显示的格式

### 4. 记录格式化（Record Formatting）

**职责**：格式化持续时间、Token 数等显示信息

**关键函数**：
```typescript
function formatDurationMillis(milliseconds: number | null, t: Translator): string
function formatElapsedSeconds(seconds: number | null, t: Translator): string
function trajectoryRecordId(cell: TrajectoryCellProps): string
```

**来源**：`trajectory-record.ts:112-147`

**TUI 适配策略**：
- 直接复用这些工具函数
- 调整本地化策略（简化或使用固定格式）

## 交互模式（需重新设计）

### Web UI 交互

1. **时间线交互**：
   - 拖选区间 → 聚焦记录
   - 滚轮缩放 → 调整时间域
   - 右键拖动 → 平移视口
   - 悬停 → 显示详情

2. **记录表交互**：
   - 点击记录 → 打开检查器
   - 折叠/展开 → 管理轮次和助手
   - 滚动 → 虚拟化渲染

3. **工具栏交互**：
   - 搜索 → 过滤记录
   - 切换实际/相对时间
   - 加载更早历史

### TUI 交互适配

1. **导航**：
   - `↑↓` - 上下移动记录
   - `PageUp/PageDown` - 翻页
   - `Home/End` - 跳到首尾
   - `g` + 数字 - 跳到指定记录

2. **查看**：
   - `Enter` - 打开检查器（新面板或全屏）
   - `Space` - 折叠/展开当前轮次
   - `Tab` - 在不同区域切换（列表/检查器/搜索）

3. **搜索**：
   - `/` - 打开搜索
   - `n/N` - 下一个/上一个匹配
   - `Esc` - 退出搜索

4. **其他**：
   - `r` - 刷新/重新加载
   - `?` - 显示帮助
   - `q` - 退出轨迹视图

## 虚拟化策略（需简化）

### Web UI 虚拟化

- 使用 React 虚拟滚动库
- 测量每行高度
- 只渲染可见窗口 + 缓冲区
- 保持语义行键稳定

**来源**：`trajectory-virtual-rows.ts`，README.zh.md:65-66

### TUI 虚拟化适配

大多数 TUI 库（blessed、ink）已经内置虚拟滚动：
- blessed: `scrollable: true` + `height: '100%-5'`
- ink: 使用 `<Box>` 的 `overflow` 属性

**简化策略**：
1. 依赖 TUI 库的内置滚动
2. 只维护当前可见范围的数据
3. 按需从完整历史加载窗口数据
4. 预估行数，避免频繁重新测量

## 性能优化要点

### 1. 按需加载

- 初始只加载最近 N 条记录（如 50 条）
- 向上滚动时加载更早的历史
- 使用游标或序列号分页

### 2. 增量更新

- 流式输出时只更新变化的记录
- 不要重建整个列表
- 使用记录 ID 追踪变化

### 3. 内存管理

- 限制内存中的记录总数
- 超过限制时卸载最早的记录
- 保留索引，需要时重新加载

### 4. 搜索优化

- 使用倒排索引
- 只索引可见窗口
- 节流搜索请求（如 300ms）

## Zenpi TUI 集成方案

### 1. 数据适配器

创建 `ZenpiTrajectoryAdapter`，将 Zenpi Agent 的内部状态映射到 `TrajectorySnapshot`：

```typescript
class ZenpiTrajectoryAdapter {
  // 从 Agent 内部状态构建快照
  buildSnapshot(agent: ZenpiAgent): TrajectorySnapshot
  
  // 监听 Agent 事件，增量更新快照
  onAgentEvent(event: AgentEvent): TrajectoryContribution
  
  // 获取指定范围的记录
  getRecords(start: number, count: number): TrajectoryRecord[]
}
```

### 2. TUI 组件

创建 blessed 或 ink 组件：

```typescript
// blessed 示例
class TrajectoryPanel extends blessed.Box {
  private recordList: blessed.List
  private inspector: blessed.Box
  private toolbar: blessed.Box
  
  render(snapshot: TrajectorySnapshot): void
  selectRecord(index: number): void
  toggleCollapse(turn: number): void
}
```

### 3. 入口集成

在 Zenpi TUI 主界面添加轨迹入口：
- 快捷键：`Ctrl+T` - 打开轨迹视图
- 侧边栏：添加 "Trajectory" 按钮
- 右键菜单：添加 "View Trajectory" 选项

### 4. KV Cache 校验

重点显示缓存相关信息：
- 突出显示 `cacheRead` 和 `cacheWrite`
- 计算缓存命中率
- 标记缓存不一致的记录
- 提供缓存统计面板

## 最小可行实现（MVP）

### 阶段 1：基础展示

- [ ] 实现 `ZenpiTrajectoryAdapter`
- [ ] 显示记录列表（轮次、类型、摘要）
- [ ] 基本导航（上下滚动）
- [ ] 显示 token 用量

### 阶段 2：检查器

- [ ] 实现检查器面板
- [ ] 显示完整输入/输出
- [ ] 显示计时信息
- [ ] 显示错误详情

### 阶段 3：KV Cache

- [ ] 突出显示缓存信息
- [ ] 计算缓存统计
- [ ] 标记异常记录
- [ ] 提供诊断建议

### 阶段 4：增强功能

- [ ] 搜索功能
- [ ] 时间线简化版（时间戳列表）
- [ ] 折叠/展开
- [ ] 导出轨迹

## 技术栈建议

- **TUI 库**：blessed（功能丰富）或 ink（React 风格）
- **数据管理**：RxJS 或简单的 EventEmitter
- **状态管理**：MobX 或纯 TypeScript 类
- **测试**：Jest + blessed-testing 或 ink-testing-library

## 参考文件

- `Docs/learn/ui-trajectory/source_manifest.tsv` - 源文件清单
- `Docs/learn/ui-trajectory/architecture_overview.md` - 架构概览
- `/tmp/deepseek-harness-analysis/packages/client/ui-trajectory/` - 源码位置
