# ZS1-132 — 最新 release 入口与配置/worker 作用域补验

本次补齐已合入 `/new` 在当前 release 的实际入口证据，并新增两项跨会话权限回归。蓝图 3.1.20，ZS1-132 仍为 `[ ]`；不替代逐文件/逐目录验收或全 Stage 1 门禁。

## 当前 release 实际入口

固定 macOS ARM release SHA256 `180ec9f70d5bedda5ca3b417a88bf313fefa069df8cf3c4488c7669133e149bf`，6,032,944 bytes。本轮不重建或重复启动预算。原预算首次 1044.823959ms > 1000ms 的失败继续保留，见 [预算记录](../../ZS1-117/master-release-budget-3.1.20/review.md)。

- `/new` 原有 8 组、授权/跨项目 9 组、进程恢复 3 组全部新运行通过：7 TUI、11 JSONL 进程、31 次 loopback HTTP。验证同项目空会话、旧历史精确前缀、Unicode/fold 草稿与模型偏好、取消草稿恢复、并发 writer 冲突、request replay、审批及 scheduled/interrupted 工作、旧授权记忆边界和提交前后进程恢复。
- 项目 picker 12 项断言新运行通过：顶部加号直接打开目录选择、取消及无效路径不增加分页、Unicode/空格和同名目录、项目配置对应实际 backend model、两项目真实 shell cwd、迟到输出不串项目、重启保存选中项目及草稿。这里的 12 是断言数，不与 `/new` 的 20 个场景组混算。
- BentoBox 11 项检查新运行通过，3 个独立 TUI：键盘焦点及列宽、鼠标行拖拽、折叠、1列到180列 resize、项目草稿/布局切换、两次重启、恢复后编辑和退出 terminal。单一子目录 fixture 的“+ → Right → Enter”是该条件下的3次手势，不宣称任意路径都只需3次。

实际 command、返回码、PTY/JSONL/HTTP 原始记录与各次结果位于 `release/`，冻结 fixture 在 `fixtures/`。project-picker 既有 helper 成功时只保存断言结果，未输出成功 PTY 原始流；不能冒称该用例也有原始 PTY 日志。BentoBox 和 `/new` 保存了原始 PTY。构建输入225项在运行前逐项核实，仅两处 Gantt 报告生成器与构建时快照不同；本轮后续测试文件改动单列，不改变被验证 release 的产品源码。

## 配置与绑定 worker 的共享 owner 回归

`tests/session_new.rs` 追加2个顶层测试、7个场景。通过真实 UnixStream JSONL host、Agent、新journal/checkpoint、审批与文件工具，provider 为确定性 Backend fixture；这些是库集成测试，不能冒称 release HTTP 或独立进程的 worker 验证。

1. configured `write_file=Deny` 配合 process `Never`，在新旧会话均拒绝实际写入，产生拒绝结果，无人工审批，旧journal前缀不变。
2. worker六种情形：允许、禁止路径、切换后撤销、显式Deny、缺失gate、已消耗action额度。新会话成功而原规则不放松；合法调用保留同lease/policy审计且不记忆授权；拒绝调用留有真实tool结果且不创建目标文件。额度在旧会话用尽后，新会话结果仍为 `action_budget_exhausted`。

最终源码 `session_new` 13项 + `approval_owner` 8项，共21个不同顶层测试通过。两项是新增测试，先前与最终重复运行不累计。严格 Clippy `--test session_new -- -D warnings` 与单文件 rustfmt 检查通过。已有 vendor crossterm 警告保留。

## 原始失败与修正

首次新增测试12通过/1失败：spent_budget 情形已经没有实际文件写入且返回denied，但测试错误要求没有 `tool_execution_started` 审计。逐读 ToolContext::check_call_gate/admit_builtin 与原始journal确认，额度在invocation阶段原子扣取，开始审计可先于额度拒绝。修正为实际第二次tool结果必须存在且outcome=denied、错误含action_budget_exhausted、文件不存在；其余预检拒绝仍要求无execution_started。原失败测试源码及日志保留，没有把该失败描述成产品漏洞，也没有修改产品实现。

首次 Clippy 报新增测试 `% 2 == 0` 应使用 `is_multiple_of(2)`，已按建议改写并在最终字节重新运行21项测试及严格Clippy。失败日志不覆盖。

## 尚未完成与回退

configured/worker上述7场景已有库集成证明；独立 release worker入口的全作用域组合、不合作线程及postcommit/grace时序、其余busy操作/外部编辑器并发、全量通用UX矩阵与最终release预算仍需独立覆盖。当前冷启动失败未解决。本次没有改src产品源码、没有新建agent session、没有把任何整项提升为 `[x]`。

回退仅反向应用 `policy-boundaries/tests.patch` 的新增测试，保留本轮之前的文件内容及之后合法修改；不删除用户journal、分页或草稿。证据目录为本次独立新增，不覆写历史包。
