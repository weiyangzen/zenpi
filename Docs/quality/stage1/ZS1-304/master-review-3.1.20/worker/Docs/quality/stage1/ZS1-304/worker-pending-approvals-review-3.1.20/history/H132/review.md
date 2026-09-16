# ZS1-132 — 跨项目新会话与审批/排队边界增量

蓝图 3.1.20；本次只接受列明的增量实现与证据，ZS1-132 保持 `[ ]`。

## 实现

后台项目生成期间，空闲项目通过 `/new` 创建新会话。单独的有界控制 worker 使用该项目已有 Agent 和 mutex，模型 worker 继续服务原项目；两套 JobId 不共用结果表。界面短暂读取 Agent 时控制 worker 最多等待 100ms，期间可取消，界面线程不等待锁。一次 try_lock 误拒绝的实际失败保留。

当前项目准备新会话时保留 busy 和提交草稿；Ctrl-C、`/interrupt` 路由到该项目控制任务，关闭分页及后续输入遵守相同 owner。成功后刷新项目会话游标、转录与 InputPort，保留 cwd、分页、BentoBox 和偏好。扩展准备取消的工具错误转换为 Interrupted，恢复 `/new` 草稿。实际返回值独立保留，运行时 shutdown 超时不会被当成已回滚而重新授权尚在运行的 Agent；本次没有单独验证不合作线程的 grace 超时，不能据此宣称全部关闭路径有界。

逐文件变更：`src/tui.rs` 为上述生产改动；`tools/tui_session_new_smoke.py` 新增 `--policy-only` 的九组实际检查、JSONL 审批事件读取和正常退出读取线程 join。其余候选构建输入未改变。

## 实际验证

- 8 套 Rust 回归：121 passed / 0 failed / 0 ignored，分别为 headless_project_workspace 12、project_workspace 7、resume_compact_owner 6、session_new 11、session_recovery 39、slash 19、tui_bentobox 16、tui_project_workspace 11。
- native aarch64 严格 Clippy（lib + tests，`-D warnings`）和 fmt --check 通过；保留 vendored crossterm 既有依赖警告。
- 固定 debug/default-feature CLI SHA256 `9e76676bc87d7a859409954ae5d19ebc0560070a4685096f2dff87ea15e918dc`。8 组原有入口：4 TUI、5 JSONL、6 HTTP；9 组授权/忙碌入口：3 TUI、2 JSONL、21 HTTP；3 组进程恢复：4 JSONL、4 HTTP。合计20组、7 TUI、11 JSONL、31 HTTP，不把子断言重复计数。
- 新增九组覆盖：真实生成中拒绝同项目 new 并可重试；待审批工具拒绝 new 后仍可回答；同会话记忆 Allow/Deny 在新会话不沿用；process-only Never 同进程保留、独立重启不恢复；后台项目继续运行时当前项目 new 成功；TUI 审批仍可通过 Alt-A 明确进入 review 后回答；已接收排队任务及重启后的 interrupted 任务阻止 new，显式取消后才成功，未自动执行 shell。
- 原有八组重新验证空历史/同 cwd 工具/模型偏好/Unicode 折叠草稿/独立重放/Ctrl-C/第二真实 writer 的 CAS 冲突和旧 owner 后续请求；三组恢复验证准备前强杀、提交后强杀及已提交 replay 打开失败后的恢复。
- 225 个候选输入逐个 hash 校验无漂移；新 source 和测试 helper 在最终实际运行前分别冻结。所有最终实际运行使用同一固定二进制。主库两处 Gantt 生成器的用户请求更新单独保留；合入产品文件与候选逐字节一致。

## 保留失败与范围

`policy-background-cursor` 为原主库全局 busy 拒绝的反例。首次独立 worker 的单次锁探测仍误拒绝（policy-control-deps）；短时重试修复。`control-after` 实际暴露扩展取消被显示为 Request failed，修复后 control-after-cancel 全部通过。`policy-control-expanded` 夹具在 slash 退出审批焦点后未用 Alt-A，产品保持等待；按现有真实交互补齐 Alt-A 后九组通过，未修改审批产品逻辑。

首次控制 build 的 cwd 错为主库，控制器只保留此记录，不当作候选构建；control-build-private 报 recv API 参数错误，修正后构建通过。policy-control-first 因冻结 helper 缺少传递依赖而未进入运行，补齐后另记新运行。这些失败均未覆盖。

尚未证明 configured per-tool Deny 和绑定 worker 的完整 new 作用域组合、不合作线程的 postcommit/grace 组合、全量其它 busy 操作与 Stage 1 通用交互矩阵。此前 release 冷启动 1208.732708ms 超出1000ms的失败仍未闭合；本次 debug 运行不是 release 预算验收。文件/目录主控验收和最终 release/全入口门禁继续独立推进。旧历史复核记录不改写。

## 回退

仅应用本记录 product.patch 的反向差异，保留之后用户修改及已创建 journal/项目草稿；需要恢复时使用既有 session open。禁止删除用户会话或重置整个工作树。
