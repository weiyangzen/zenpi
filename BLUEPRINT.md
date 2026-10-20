# Zenpi TUI 轨迹展示功能集成蓝图

## 目标

分析 deepseek-harness 的 `ui-trajectory` 包，理解其交互逻辑和展示形式，为 Zenpi TUI 集成 Agent 上下文轨迹展示功能做准备。

## 背景

- **问题**：在 Zenpi TUI 中，我们经常需要关注 Agent 的上下文轨迹，特别是当发生 KV cache 不吻合时需要校对
- **方案**：deepseek-harness 的 `ui-trajectory` 包提供了很好的 Agent 对话轨迹展示功能
- **挑战**：直接集成不现实，需要先理解其核心逻辑和展示形式，然后适配到 Zenpi TUI

## 学习范围

源码位置：`deepseek-harness/packages/client/ui-trajectory`

需要分析的关键点：
1. 轨迹数据结构（如何表示 Agent 的上下文）
2. 交互逻辑（用户如何浏览轨迹）
3. 展示形式（UI 组件结构和布局）
4. 关键功能（缓存校验、上下文追踪等）
5. 与 Agent 的集成接口

## 任务清单

### 阶段 1：学习 ui-trajectory（使用 learn-cron-builder）

[x] 锁定源码范围，生成 `source_manifest.tsv`
[x] 分析包结构和主要组件
[x] 理解轨迹数据模型
[x] 理解交互流程
[x] 理解 UI 渲染逻辑
[x] 生成学习笔记（`what` 深度）- 见 `Docs/learn/ui-trajectory/architecture_overview.md`

### 阶段 2：提取核心逻辑

[x] 识别可复用的设计模式
[x] 提取轨迹数据结构定义
[x] 提取关键交互逻辑
[x] 识别与 Zenpi 的集成点

**总结**：见 `Docs/learn/ui-trajectory/core_design_extraction.md`

### 阶段 3：适配设计

[x] 设计 Zenpi TUI 轨迹入口
[x] 设计轨迹展示 UI（适配 TUI 环境）
[x] 设计数据获取接口
[x] 设计 KV cache 校验功能

**确定方案**：
- 布局：方案 B（全屏切换 + 面包屑导航）
- 功能：三层视图 + Agent 控制（暂停/继续）+ 搜索
- 详见：`Docs/learn/ui-trajectory/DESIGN_FINALIZED.md`

### 阶段 4：实现与集成

[x] 实现轨迹数据采集（`Usage` 缓存/推理细分 + 每次请求的计时与前缀指纹）
[x] 实现 TUI 轨迹面板（`src/tui/trajectory.rs`，三层视图）
[x] 实现 KV cache 校验（`src/trajectory.rs`，前缀指纹 + 用量对比）
[x] 集成到 Zenpi TUI 主界面（`Alt-T` / `/trajectory`；headless `/trajectory` 返回 JSON）
[x] 测试（`tests/tui_trajectory.rs` + 单元测试）
[ ] Agent 暂停/继续（运行时目前只有 cancel/steer，没有 pause 原语，需先设计）

**实现说明**：见 `Docs/learn/ui-trajectory/IMPLEMENTATION.md`

## Oracle 定义

- **阶段 1**：生成完整的学习笔记，覆盖所有关键文件，通过 `check_depth.py` 验证
- **阶段 2**：提取的设计文档清晰可读，包含具体代码引用
- **阶段 3**：设计文档详细，包含 UI mockup 和接口定义
- **阶段 4**：功能可用，能在 Zenpi TUI 中查看完整轨迹

## 约束条件

- 不直接复制 deepseek-harness 的代码，只学习其设计理念
- 适配 Zenpi TUI 的终端环境（不是 Web UI）
- 保持 Zenpi 现有的工具精简原则
- 优先解决 KV cache 校验的核心需求

## 依赖

- deepseek-harness 源码（已克隆到 `/tmp/deepseek-harness-analysis`）
- learn-cron-builder skill（已安装）
- Zenpi 现有 TUI 架构

## 当前进展

### 已完成

✅ **阶段 1：学习 ui-trajectory**
- 克隆了 deepseek-harness 源码到 `/tmp/deepseek-harness-analysis`
- 生成了源文件清单 `Docs/learn/ui-trajectory/source_manifest.tsv`（31 个文件）
- 创建了架构概览文档 `Docs/learn/ui-trajectory/architecture_overview.md`
- 创建了核心设计提取文档 `Docs/learn/ui-trajectory/core_design_extraction.md`

### 关键发现

1. **核心数据模型**：
   - `TrajectoryRecord` - 单条轨迹记录（包含身份、内容、计时、用量、状态）
   - `TrajectorySnapshot` - 完整轨迹快照（事件、请求、工具、流式状态）
   - `TrajectoryContribution` - 8 种贡献类型（system-prompt、node、assistant、tool 等）

2. **核心逻辑**：
   - 快照构建器：从会话事件流构建轨迹快照
   - 事件投影：将原始事件投影为视图节点
   - 布局派生：组织轮次和分组的层级结构
   - 记录格式化：格式化时间和 token 数

3. **关键特性**：
   - 虚拟化渲染（只渲染可见窗口）
   - 按需历史加载（初始 50 条，向上加载更早）
   - 时间线交互（拖选、缩放、平移）
   - 检查器（token 用量、TTFT、输入输出）

4. **适配要点**：
   - 数据模型可直接复用（与 UI 无关）
   - 核心逻辑需提取并去除 Web UI 依赖
   - 交互模式需重新设计（键盘导航替代鼠标）
   - 虚拟化可依赖 TUI 库的内置支持

### 最小可行实现计划

**MVP 阶段 1：基础展示**
- [ ] 实现 `ZenpiTrajectoryAdapter`（将 Zenpi Agent 状态映射到 TrajectorySnapshot）
- [ ] 显示记录列表（轮次、类型、摘要）
- [ ] 基本导航（上下滚动）
- [ ] 显示 token 用量

**MVP 阶段 2：检查器**
- [ ] 实现检查器面板
- [ ] 显示完整输入/输出
- [ ] 显示计时信息（包括 TTFT）
- [ ] 显示错误详情

**MVP 阶段 3：KV Cache 校验**
- [ ] 突出显示缓存信息（cacheRead、cacheWrite）
- [ ] 计算缓存统计（命中率、总用量）
- [ ] 标记异常记录（缓存不一致）
- [ ] 提供诊断建议

**MVP 阶段 4：增强功能**
- [ ] 搜索功能
- [ ] 时间线简化版（时间戳列表）
- [ ] 折叠/展开轮次
- [ ] 导出轨迹数据

### 技术决策

- **TUI 库**：~~blessed~~ → 实际为 Rust + ratatui（Zenpi 本身是 Rust 项目，沿用现有 TUI 栈，不引入新依赖）
- **数据适配**：创建独立的 `ZenpiTrajectoryAdapter` 类
- **入口集成**：~~`Ctrl+T`~~ → `Alt-T`（`Ctrl-T` 已被目录选择器占用，`Ctrl-G` 已被外部编辑器占用）+ `/trajectory`
- **KV Cache 重点**：专门的缓存信息面板和统计

## 下一步行动

### 📋 设计阶段已完成（Phase 1-3）✅

完整总结见：`Docs/learn/ui-trajectory/PHASE_1-3_COMPLETE.md`

### 🚀 实现阶段（Phase 4-9）

#### Phase 4: 基础框架（1-2 周）
- [ ] blessed 应用框架搭建
- [ ] 视图切换系统（甘特图/轨迹/检查器）
- [ ] 面包屑导航
- [ ] 基础键盘快捷键

#### Phase 5: 甘特图（1 周）
- [ ] Agent 列表显示
- [ ] 进度条渲染
- [ ] 状态更新
- [ ] 基础导航

#### Phase 6: 轨迹列表（2 周）
- [ ] ZenpiTrajectoryAdapter 实现
- [ ] Turn 分组显示
- [ ] 记录行渲染
- [ ] 虚拟滚动
- [ ] 统计栏

#### Phase 7: 检查器（1 周）
- [ ] 三标签页布局
- [ ] 内容显示
- [ ] 指标显示
- [ ] 滚动支持

#### Phase 8: Agent 控制（1 周）
- [ ] Agent 选择器
- [ ] 暂停/恢复接口
- [ ] 状态同步

#### Phase 9: 搜索（1 周）
- [ ] 搜索输入框
- [ ] 记录过滤
- [ ] 结果导航

---

**预计时间**：6-8 周  
**当前分支**：`feature/trajectory-integration`  
**最新提交**：f34b2c6
