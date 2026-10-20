# 🎉 Zenpi 轨迹功能设计阶段完成

## 概览

已完成 Zenpi TUI 轨迹展示功能的完整设计阶段（Phase 1-3）。

**分支**：`feature/trajectory-integration`  
**提交**：`f34b2c6` - feat(trajectory): complete design phase

---

## ✅ 已完成的工作

### Phase 1: 学习 ui-trajectory

使用 `learn-cron-builder` 系统分析了 deepseek-harness 的 `ui-trajectory` 包：

- ✅ 分析了 31 个源文件
- ✅ 提取了核心数据模型（TrajectoryRecord, TrajectorySnapshot, TrajectoryContribution）
- ✅ 理解了快照构建、事件投影、布局派生等核心逻辑
- ✅ 识别了可复用的设计模式

**产出**：
- `source_manifest.tsv` - 源文件清单
- `architecture_overview.md` - 完整架构分析

### Phase 2: 提取核心设计

从 ui-trajectory 提取了可复用的设计元素：

- ✅ 数据模型定义（与 UI 无关）
- ✅ 核心逻辑识别（需要提取的函数）
- ✅ Web UI → TUI 适配策略
- ✅ MVP 实现路径（4 个阶段）

**产出**：
- `core_design_extraction.md` - 核心设计提取
- `LEARNING_REPORT.md` - 完整学习报告

### Phase 3: 交互设计

确定了 Zenpi TUI 的交互方案：

- ✅ 布局方案：**方案 B - 全屏切换 + 面包屑导航**
- ✅ 功能范围：三层视图 + Agent 控制（暂停/继续）+ 搜索
- ✅ 快捷键设计：直觉化 + Vim 友好
- ✅ 数据结构定义：AgentState, TrajectoryRecord, TurnGroup

**产出**：
- `INTERACTION_DESIGN.md` - 初步交互设计
- `DESIGN_FINALIZED.md` - 最终确定方案（⭐ 主文档）

---

## 🎯 最终设计方案

### 核心理念

**三层导航体系**：
1. **L1 甘特图** - 宏观：所有 agent 的并行任务时间线
2. **L2 轨迹列表** - 中观：单个 agent 的操作历史
3. **L3 检查器** - 微观：单条记录的完整细节

### 布局设计

```
┌─────────────────────────────────────────────────────────────┐
│ Zenpi TUI  [Gantt → worker-2 → Turn 3 → #17]     [KV: ✓]   │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│ [全屏视图内容 - 甘特图/轨迹/检查器]                        │
│                                                              │
│ 底部: [1]Gantt [2]Trajectory [3]Inspector | Quick: g/a/c    │
└─────────────────────────────────────────────────────────────┘
```

**选择理由**：
- 每个视图有充足空间
- 面包屑提供清晰导航
- 快捷键支持快速切换
- 适合深入分析和监控

### MVP 功能

#### ✅ 包含

1. **三层视图**：甘特图、轨迹列表、检查器
2. **基础导航**：面包屑、Agent 选择器、快捷键
3. **Agent 控制**：暂停（`p`）、继续（`r`）
4. **数据展示**：Token 用量、KV Cache、计时、错误
5. **搜索**：记录搜索（`/`）、结果导航（`n/N`）

#### 🔜 暂缓

- 修改参数、插入 steering（需要权限和 safety）
- 标记功能（`m`）
- Cache 统计面板（`c`）
- 导出、折叠优化、时间线可视化

### 关键快捷键

| 功能 | 按键 | 说明 |
|------|------|------|
| 返回甘特图 | `Ctrl+G` / `g` | Go back |
| 打开轨迹 | `Ctrl+T` / `Enter` | - |
| 切换 Agent | `a` | Agent switch |
| 暂停/继续 | `p` / `r` | Pause / Resume |
| 搜索 | `/` | Search |
| 视图切换 | `1` `2` `3` | Gantt / Trajectory / Inspector |
| 导航 | `↑↓` / `jk` | Vim 友好 |

---

## 📚 文档清单

### 核心文档

1. **`BLUEPRINT.md`** ⭐  
   总体蓝图和进度跟踪

2. **`DESIGN_FINALIZED.md`** ⭐  
   最终确定的交互设计方案（包含完整界面、快捷键、数据结构）

3. **`LEARNING_REPORT.md`**  
   完整的学习报告（执行摘要、关键发现、实现路径）

### 参考文档

4. **`architecture_overview.md`**  
   ui-trajectory 完整架构分析

5. **`core_design_extraction.md`**  
   核心设计提取和 TUI 适配方案

6. **`INTERACTION_DESIGN.md`**  
   初步交互设计（包含多个方案比较）

### 数据文件

7. **`source_manifest.tsv`**  
   ui-trajectory 源文件清单（31 个文件）

8. **`source_files.txt`**  
   文件列表

---

## 🚀 下一步：实现阶段

### Phase 4: 基础框架（1-2 周）

```typescript
- [ ] blessed 应用框架搭建
- [ ] 视图切换系统
- [ ] 面包屑导航
- [ ] 基础快捷键
```

**目标**：能够在视图间切换，显示占位内容

### Phase 5: 甘特图（1 周）

```typescript
- [ ] Agent 列表显示
- [ ] 进度条渲染
- [ ] 状态更新（running/paused/completed）
- [ ] 基础导航（↑↓ Enter）
```

**目标**：显示所有 agent 的状态和进度，可以选中并进入轨迹

### Phase 6: 轨迹列表（2 周）

```typescript
- [ ] ZenpiTrajectoryAdapter 实现
- [ ] Turn 分组显示
- [ ] 记录行渲染
- [ ] 虚拟滚动
- [ ] 统计栏（token、TTFT、cache）
```

**目标**：显示单个 agent 的完整轨迹，可以选中记录进入检查器

### Phase 7: 检查器（1 周）

```typescript
- [ ] 三标签页布局（Content/Metrics/Raw）
- [ ] 内容显示（Input/Output）
- [ ] 指标显示（Token/Cache/Timing）
- [ ] 滚动支持
```

**目标**：显示单条记录的完整详细信息

### Phase 8: Agent 控制（1 周）

```typescript
- [ ] Agent 选择器（按 a）
- [ ] 暂停/恢复接口
- [ ] 状态同步（实时更新）
```

**目标**：能够控制 agent（暂停/继续）并在界面实时反映

### Phase 9: 搜索（1 周）

```typescript
- [ ] 搜索输入框（按 /）
- [ ] 记录过滤
- [ ] 结果导航（n/N）
```

**目标**：在轨迹中搜索记录

---

## 🎓 关键技术决策

### 1. TUI 库选择

**选择**：blessed  
**理由**：
- 功能丰富（列表、表格、表单、进度条）
- 成熟稳定（被众多 npm 工具使用）
- 文档完善，示例丰富

**替代方案**：ink（React 风格，但功能相对有限）

### 2. 数据架构

**ZenpiTrajectoryAdapter**：
```typescript
class ZenpiTrajectoryAdapter {
  buildSnapshot(agent: ZenpiAgent): TrajectorySnapshot
  getRecords(start: number, count: number): TrajectoryRecord[]
  subscribe(callback: (event) => void): Unsubscribe
}
```

**职责分离**：
- Adapter：Zenpi Agent → 通用轨迹数据模型
- View：通用数据模型 → blessed 组件渲染

### 3. 状态管理

**方案**：纯 TypeScript 类 + EventEmitter  
**理由**：
- 简单直接，无额外依赖
- MobX 可选（如果状态复杂化）
- 避免引入 Redux 等重量级方案

---

## 📊 预期成果

### 用户价值

1. **监督多 Agent 协作**：在甘特图看到所有 agent 的进度，快速定位问题
2. **诊断 KV Cache 问题**：查看 cache read/write，发现不一致
3. **引导 Agent 工作**：暂停异常 agent，调整后继续
4. **深入上下文分析**：查看完整的输入输出和计时信息

### 技术价值

1. **可复用的设计模式**：其他 TUI 工具可参考
2. **清晰的架构分层**：数据层、适配层、视图层分离
3. **扩展性**：后续可添加更多功能（标记、导出、统计）

---

## 📝 备注

### 学习工具

使用了 **b3ehive v2.2.0** 的 `learn-cron-builder` 技能：
- 系统化地学习了 ui-trajectory 包
- 提取了核心设计模式
- 加速了设计决策过程

### 协作流程

1. 用户提出需求：集成轨迹展示 + KV cache 诊断
2. 安装学习工具：weiyangzen/b3ehive 插件
3. 系统学习：使用 learn-cron-builder 分析源码
4. 设计讨论：确定布局方案和功能范围
5. 文档产出：完整的设计文档和实现路径

### 时间估算

- **设计阶段**：1 天（已完成）
- **实现阶段**：6-8 周（Phase 4-9）
- **测试优化**：1-2 周

---

## ✨ 总结

我们已经完成了 Zenpi 轨迹功能的完整设计阶段！

**核心成果**：
- ✅ 深入理解了 deepseek-harness 的轨迹展示设计
- ✅ 提取了可复用的数据模型和核心逻辑
- ✅ 确定了适合 Zenpi TUI 的交互方案
- ✅ 定义了清晰的 MVP 功能范围和实现路径

**技术路径**：
- blessed 构建 TUI
- 全屏切换 + 面包屑导航
- ZenpiTrajectoryAdapter 适配数据
- 6 个实现阶段逐步交付

**准备就绪**：可以开始 Phase 4（基础框架）的实现了！

---

**生成时间**：2026-10-17  
**分支**：feature/trajectory-integration  
**提交**：f34b2c6
