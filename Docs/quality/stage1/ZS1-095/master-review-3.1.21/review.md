## 主控独立逐文件验收 — ZS1-095 / 3.1.21

主控顺序完整读取当前 src/approval.rs 的 L1–260、261–520、521–780（29396 B，SHA-256 5488ad365670e515eba5e7d82a91dbf935be854b848c20fd99725f102e1f185f），并完整读取本次 37582 B / 213 行候选报告和 80 行离线 verifier。前两块在前一执行轮读取，最后一块及候选在本验收轮完成；没有把摘要、manifest 或函数数量当作阅读。当前源码包含冻结 c592686334a95047f749d7c61c793abc03ce93d5312b22aadeefa5aed9c85844 的全部 737 行，仅插入 43 行 / 1790 B 的会话记忆恢复；本轮直接检查精确差异并保存匹配区间。基线不改写，也不虚报再次全文读旧源码。

本文件的请求校验、serde 默认、四个共享集合、29 个函数（23 个生产函数和 6 个源测试）、策略优先级、两类错误与所有条件分支均已复核。六个源测试实际包含 14 个普通 assert 文本位置、0 快照；本轮没有执行这些测试。worker 的 31 连续语义单元和 16 个功能映射对照完整报告，分别关联具体状态、源测试/可运行判据及实际 owner。结构计数仅证明覆盖材料完整。

核心结论：默认策略 Always 不等于默认全拒绝，普通只读在显式 Deny 和 worker/preflight 分支之后才被允许；TUI 默认选中 Deny。请求只做通用结构校验，不认证 policy digest/lease，不校验工具专有 schema 或自动脱敏。协调器共享 Arc，pending 按键排序且只 drain 一次，没有本模块容量或 TTL。respond 不以可见性为前置；首响应只在当前 pending/decision 生命周期内获胜。accepted 是内存记录，mark_persisted 不做 IO，也不阻止等待者提前得到 response。persist 回调在锁外，失败撤回不一定恢复 pending；并发 persist 可重复调用外部 writer，mark 失败可能发生在外部写成功之后。这些 API 限制不是未经证明的宿主可达漏洞。

取消也有独立语义：cancel_all 只拒绝仍 pending 的请求，不改已接受 Allow；emergency_cancel 增加 epoch，真正停下依赖调用方闭包读取 epoch，无法撤销已发生副作用。取消闭包在锁内执行，不应重入协调器。retract 在 decision 已被消费时返回 Unknown；未显示请求没有恢复快照，有显示快照的恢复仍保留 visible，不能承诺再次 drain。accepted 尚存时 ID 可重新登记；当前 core 派生 ID 依赖 session/turn/call/policy 生命周期，协调器自身没有永久消费墓碑。

主控新读 core L2590–2768、4240–4450、3300–3317、5311–5335，确认实际 user_shell 在成功 append 审批事件、按需 remember、拒绝/取消/gate 检查、begin_operation 和 tool_execution_started 之后才调用真实 RunCommandTool。普通工具 prepare 路径在 persist_accepted、remember、approval_consumed、Deny 和取消/binding 检查之后才继续执行准备；这不是本轮完整审阅其余 dispatch 或底层 fsync/崩溃恢复。富响应可先于落盘返回与 core 的落盘后执行顺序不矛盾。读 core L1246–1287、1338–1363、1654–1687、5188–5215，确认独立配置基准、会话记忆恢复和同步 reveal/respond 两锁接口。配置 Deny 受恢复筛选保护；remember 本身无 guard；恢复事件需要可信 session 来源，不能把 origin 字符串当认证。

主控新读 headless L5630–5710、6335–6408、4505–4598、7207–7248，确认原子响应 ack 只表示 accepted；project owner 协调器由宿主选择；EOF 普通拒绝和显式 shutdown epoch 取消各有用途。本轮未将 headless 缓存刷新或全部跨项目并发路径宣告已穷尽。

主控新读当前 e451 TUI L1051–1184、1180–1295、1344–1387、12214–12246、12601–12644：项目/request/turn/call 校验后才响应，y/n 选择后 Enter 提交，Esc 保留，worker 不提供 remember，目录选择器优先；失败响应只显示错误，不把错误当决策退役，下一轮协调器状态负责 reconciliation。视图的 128 容量不等于协调器容量。全部 tests/tui_approval_focus.rs 428 行已读（12 个上下文测试），另读 tests/approval_owner.rs L1–203、385–400；静态断言分别覆盖默认拒绝、草稿、owner、重复、记忆重放和取消。没有把 TestBackend 当 PTY，也没有把 executes_once 名称当故障下恰好一次证明。

worker 捕获时主控 125 仍在运行的语句保留为历史时间点。当前已完成 125 局部计时/footer 整合，主控记录在 Docs/quality/stage1/ZS1-125/master-approval-timer-3.1.21/review.md：195 项 Rust 与新 debug 真实 PTY 14 项通过，原失败未删除。本项不重复运行或合并那些测试。Codex 303 对照沿用 worker 独立源审阅；主控本轮没有重读整份 303 或宣告它已验收。旧 095 完整包及旧失败只作保留历史，不继承旧运行数作为当前证明。

主控完整静态读取新的 portable verifier 后，在新的主控副本仅运行一次，159 项离线检查全部通过，0 产品运行。它只核文件绑定、patch/rollback、完整包和临时目录文档操作，不运行 Cargo、PTY、HTTP、旧 runner、release 或预算。该结果支撑交付完整性，接受依据仍是上述源码和调用路径审阅。

仅接受 095 的完整文件理解（G-FILE），保留冻结身份与当前实现身份。审批完整交互 124、计时/长回复 125、生命周期 132、core/headless/TUI 整文件、相关目录及全阶段各自待验。回退仅撤销本项报告/receipt 状态，产品源码、BentoBox、顶部加号目录选择行为与历史证据不变。
