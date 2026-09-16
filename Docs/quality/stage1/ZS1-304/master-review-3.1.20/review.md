# ZS1-304 主控逐文件独立验收（3.1.20）

接受范围是 Codex `codex-rs/tui/src/bottom_pane/pending_thread_approvals.rs` 的完整文件学习与 Zenpi 操作映射，G-FILE。源文件 4105B、147行、SHA256 `a401d7c6ba3051fcf2c20152a9e86a3f4f8751a87470cbc7f518001326618a2f`，主控连续全文阅读，包括3个内联测试、辅助函数及2个内嵌snapshot。完整阅读候选报告141行/28429B，SHA256 `b2e081b19f5f9d52df1b6fd8bce3e9d6a9e9d8a716df5f922b77d7d6004baddb`，以及17个冻结目标片段和一个旧审批测试片段。这里没有声称完整阅读 Zenpi 的 TUI 或 approval owner，也没有声称运行了3个源测试。

## 独立语义判断

1. `Vec<String>` 只存显示名；不包含请求身份、active过滤、权限、取消或键盘路由。`set_threads` 比较完整有序列表，完全相同才返回false；顺序变化、重复和空字符串均原样保留，没有去重、容量上限或触发dirty的操作。空列表与只有空字符串的列表不同。
2. 空列表或宽度小于4返回空renderable。非空时只展示前三个名字，但每个可换成多行，不能把take(3)理解为最多三行或固定高度；第四个以后用dim/italic省略行提示，不删除内部请求。`/agent to switch threads` 是样式文字，实际切换路由来自调用方。
3. render遇空矩形直接返回，没有显式清空已有buffer。高度测量与绘制都重新构建相同renderable，没有缓存。样式、Unicode/grapheme和极窄宽度边界没有被这3个源测试完整覆盖。
4. snapshot_rows只读取每格symbol的第一个字符，丢弃style，再将空格换为点。多线程snapshot中的全点行不能独立区分实际省略号与空白。测试分别验证空列表宽40高度、单名字宽40、四名字宽44；未测试setter布尔值、宽3/4、长Unicode、请求身份、焦点或owner生命周期。测试专用threads getter没有被这3个测试使用。
5. 已读Zenpi片段显示审批视图默认拒绝且不记忆、按request_id去重和视图128上限；coordinator的BTreeMap drain按key顺序，不足以证明全局arrival FIFO或owner128上限。决定必须绑定project/request/turn/call和真实pending，不能用可见标签授予权限。retire后空map项、正常终态移除、QueueFull取消未被接受等分支需分开理解，不能仅凭contains_key静态推断永久死锁，也不能把Interrupt requested文案等同取消成功。
6. picker解除审批焦点；Esc/slash关闭review保留请求，Alt-A明确重新进入。切换项目本身并不自动把所有pending视图设为focused。旧cross-project检查实际是`/approve stale once`，其通过不等于已测真实完整身份tuple的所有跨项目组合；旧full_diff_scrolling只覆盖指定fixture。

## 冻结快照与当前产品

候选绑定TUI `93a68354f529aed3259b41aa36c10edf3e0c3694b8b0bfbcdd58a0f498778ce8` 的17个必要目标片段。在该快照中确实没有后台审批来源badge；报告这项观察保留为历史事实。

当前主库TUI已是 `d2a98934d4931a5aa202e542acd556bdda5667f0e60df11a68731dacf00eccf1`：后台待审批数量投影到现有项目分页，footer最多列3个非当前来源及剩余项目数，并提示切回项目、Alt-A审阅。无需照搬Codex的`/agent`，也不新增占用BentoBox的面板。当前草稿、真实cwd、项目名和审批权限仍由现有owner管理。之前已实际验证52项Rust、4项新PTY、14项审批及11项BentoBox检查；这些是已有增量证据，本次文件接收新增产品测试数为0。见[后台审批主控记录](../../ZS1-124/master-background-attention-3.1.20/review.md)。候选305的调用方分析也指出源widget在active modal存在时不进入render tree；本文件本身不能证明源提示始终显示。

提醒仍受单行宽度裁剪及未保存草稿警告优先级约束。新badge只有既有debug最终二进制证据，下一次自然release还需覆盖。ZS1-124与132继续未完成，ZS1-117冷启动1044.82ms超过1000ms的失败继续保留。本文件接收不替代这些功能或目录验收。

## 证据与验证边界

候选完整包manifest `0f45c4db1bc94d7efa8be1ead4dfaf66102803ada90bf7e378cb8d28d011c579`，63项产物。主控已阅读两个离线校验脚本并实际执行verify_package.py：退出0，日志1138B/hash `181cfda335d0d32e140a2a9897ff1f82bfacb86d1498586afea0b8b00035f3f1`。核对产物集合及逐字节hash、46个输入、完整read-log、3源测试身份、10能力组和17目标片段；两份delta各用独立临时Git仓库执行8项正逆apply/check及无关sentinel保护，接收端报告基底不存在。脚本完整性通过不能代替上述主控语义判断。

历史H132的9+8+3组通过与4个原失败结果按其各自run/manifest保留；H124旧before、after及legacy同样只作原范围历史。主控没有重新运行这些历史命令，也没有把P01–P10最小待验标准计作已执行测试。较新的configured Deny/worker作用域库回归另见[132主控记录](../../ZS1-132/master-release-policy-3.1.20/review.md)，不冒充该候选源测试。

冻结候选在[worker包](worker/README.md)完整保留，可以从包内原verify_package.py重验。候选报告的原worker绝对链接保持原字节；迁移后的目标片段和历史副本在[冻结证据目录](worker/Docs/quality/stage1/ZS1-304/worker-pending-approvals-review-3.1.20/inputs.json)，避免把可变工作区当唯一证据。当前接收报告及完整包各自受主控receipt hash约束。

回退仅撤销304接收的报告、证据、receipt和本项状态，恢复本次备份的完成面；不覆盖任何产品源文件，不删除用户项目、会话或其他worker产物。
