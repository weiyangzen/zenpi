# feat: 账号体系（OAuth / apikey / 别名）+ plan 模式 + goal 模式 + compact 增强

## 概述

本 PR 为 zenpi 补齐四大能力：统一的账号登录体系、plan 模式
（先计划、确认后执行）、goal 自主执行模式（未完成不停止，
含监督/引导/重建/收尾/移交控制命令）、以及 context compact 的
可配置化与可引导化。

## 功能一：账号体系

### 新增命令

- `zenpi config auth list [--json]`
  列出所有已配置的 agent 账号：别名、provider、base_url、
  认证方式（apikey / oauth）、是否默认账号，凭证全程脱敏。
- `zenpi config add auth codex [email]`
  通过 OpenAI OAuth（ChatGPT 账号）浏览器授权登录；传入 email
  作为登录提示与账号别名，例如
  `zenpi config add auth codex lzjaaaaa@gmail.com`。
  授权凭证持久化后自动刷新，失效时提示重新登录。
- `zenpi config add auth apikey <base_url> <provider> <apikey> [--alias 别名] [--json]`
  非交互式添加 API Key 账号。
- `zenpi config import-codex --profile codex` 增强
  除 API Key 外，现在也能识别并导入 Codex 的 OAuth 登录凭证。

### 影响

- 所有账号（apikey 与 oauth token）统一持久化到
  `~/.zenpi/auth.json`，文件权限保持 0600；旧格式配置无缝兼容。
- 每个账号有唯一别名，别名即 profile，切换账号沿用现有习惯。

### 效果

- 用 ChatGPT 账号（Gmail）登录的用户不再需要手动管理 API Key。
- 多账号、多 provider 集中管理，一目了然，随时切换。

## 功能二：plan 模式

### 新增能力

- `/plan [instruction]`：先设定计划——模型针对指令生成完整的
  执行计划提案并展示给用户。
- 用户确认或优化后，再 exec 执行：计划经用户确认（可修改、可
  驳回重新生成）后才进入执行阶段，未确认不产生任何副作用。
- 脚本化场景可用 `zenpi --headless "/plan ..."`。

### 影响

- 计划阶段零副作用：不落盘、不执行任何工具；只有用户确认后的
  exec 阶段才真正执行，且执行仍受现有审批体系约束。

### 效果

- "先规划、后动手"：执行前可审阅、可优化计划，大幅降低误操作
  风险，也为 goal 模式提供前置规划手段。

## 功能三：goal 模式

### 核心语义

- `/goal <instruction>`：创建 goal 并进入自主执行模式——
  **只要 goal 没有完成，就会一直执行**：agent 围绕完成判据
  跨多轮持续推进，直到完成、被用户叫停或预算耗尽。
- 执行结构支持 **DAG 或 flow 等多种拆解方式**：goal 可按
  有向无环图（DAG）做依赖编排，也可按线性 flow 或其他结构
  组织任务分枝，按目标性质选择最合适的拆解。

### 控制命令

- `/goal supervise`：监督命令。通过只读快照查看 goal 执行进度，
  TUI 生成 tree 标签页，树形展示目标 → 任务分枝 → 各执行单元的
  实时状态，全程不干扰正在运行的任务。
- `/goal steer <内容>`：引导命令。输入引导信息后自动分发到所有
  正在执行的分枝，下一轮即生效，全程留痕可审计。
- `/goal rebuild <节点> [--steer "引导词"]`：部分重建。只重做
  执行结构中指定的子分枝，其余分枝不受影响，可附带新引导词。
- `/goal rollout --force`：强制收尾。所有智能体按最速、最简单
  的方案处理当前全部剩余任务，直接收尾并输出结果摘要。
- `/goal handoff <节点>`：将指定任务分枝移交给外部执行者，
  移交后进度仍可在 supervise 中跟踪。

### 影响

- 持续执行不等于失控：设有轮次/时间预算上限，超限自动暂停；
  全程受现有审批与权限体系约束，不绕过任何工具确认；
  用户可随时通过 steer / rebuild / rollout 或叫停介入。

### 效果

- 一句话交代目标，agent 自主拆解（DAG / flow）、持续执行、
  直到完成。
- 进度可视、过程可引导、局部可返工、整体可一键收尾。

## 功能四：compact 增强

### 新增能力

- compact 配置：可在配置文件中设定自动压缩阈值、保留轮数、
  默认压缩提示词。
- `/compact --force`：强制立即压缩，无需等待阈值触发。
- `/compact [提示词]`：压缩时附加额外指令（如"完整保留迁移
  方案""额外参考某文档"），指导压缩保留重点内容。

### 影响

- 压缩行为从"固定不可控"变为可配置、可强制、可引导；
  附加提示词随压缩记录留存，便于审计。

### 效果

- 长会话压缩后关键信息不丢失，压缩时机和策略由用户掌控。

## 验收标准

- [ ] 三条 auth 命令可用，凭证脱敏，auth.json 权限 0600 且旧格式兼容
- [ ] import-codex 能导入 OAuth 凭证且重复执行幂等
- [ ] /plan 先生成计划，用户确认/优化后才 exec，确认前零副作用
- [ ] /goal 未完成则持续执行，支持 DAG / flow 等执行结构，
      完成自动收尾、超限自动暂停、可随时叫停
- [ ] supervise 展示进度树且不干扰执行；steer 自动分发并留痕
- [ ] rebuild 只重做指定分枝；rollout 一键强制收尾并输出摘要
- [ ] handoff 移交外部执行，进度可跟踪，重复移交被拒绝
- [ ] compact 可配置、可强制、可附加提示词
