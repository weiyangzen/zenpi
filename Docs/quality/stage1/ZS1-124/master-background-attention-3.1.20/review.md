# ZS1-124 — 后台项目待审批提示

后台项目收到真实工具审批时，用户留在另一项目只能看到 background 运行数量，不知道需要回到哪个项目确认。本次在现有顶部分页加黄色 `!N` 数量，底部列出最多3个来源项目及“switch tab, Alt-A review”，更多来源显示剩余项目数。即使来源分页不在当前顶部可见范围，底部仍提示。沿用现有同名目录区分标签；没有修改项目名称、cwd、owner、审批规则或 BentoBox 的尺寸/结构。

`src/tui.rs` 只从现有项目 ApprovalView 投影提醒，不增加审批存储或自动授权；当前草稿及焦点不被后台提醒改变。退休/终态清理后数量随现有状态消失。未保存草稿警告保留优先级，当前审批modal/Alt-A已有焦点语义保留。底部单行受终端宽度裁剪，超窄终端不能保证显示完整名字和提示；没有新增占用 BentoBox 区域的面板。

## 对照与实际验证

主控独立顺读冻结 Codex `pending_thread_approvals.rs` 全147行、3内联测试与2内嵌snapshot，SHA256 `a401d7c6ba3051fcf2c20152a9e86a3f4f8751a87470cbc7f518001326618a2f`。该文件证明列出非当前任务的待审批来源及切换提示；其被动Vec不证明上游审批/取消/容量。Zenpi将提醒放到已有分页和footer，使用已有项目切换与Alt-A，不照搬 `/agent` 路由。本增量不构成 ZS1-304 的完整 G-FILE 接受。

先在固定旧 release `180ec9f70d5bedda5ca3b417a88bf313fefa069df8cf3c4488c7669133e149bf` 门控真实 loopback provider，切到第二项目、保存Unicode草稿后再释放tool call：原项目已出现审批请求，但顶部无badge、底部无来源提示；没有实际文件写入。`before.json` status=observed_missing，退出0表示成功观察缺失，不表示实现通过。

最终 native ARM debug/default-feature CLI `e632a430f59e888545af42a8eb189698aea3a3fb7ea2b2ff1037502e6e8b00ad`：

- 新背景审批4项真实PTY检查通过，1 TUI、2 HTTP。当前分页、Unicode草稿和布局不变；来源badge/footer出现；切回后明确Alt-A、y、Enter才在原cwd写入，第二项目无文件；完成后badge和提示消失，切回仍保留原草稿。
- 既有审批14项真实PTY检查通过，12 HTTP：明确决定、拒绝/取消、picker优先、错误项目审批、顺序请求、记忆与独立重启均回归。原有full_diff_scrolling只覆盖80行fixture，不把它扩称截断preview的全量查看证明。
- BentoBox11项真实PTY检查通过，3独立TUI，0 HTTP；键鼠布局、缩放、+选择目录、项目切换草稿/布局及两次重启保持正常。
- 最终源码4套Rust共52项通过：session_new13、tui_approval_focus12、tui_bentobox16、tui_project_workspace11。新增1个渲染回归同时检查传统与BentoBox host、来源tab离屏、无自动焦点转移、数量更新/消失与不改项目名；不把它当作真实HTTP验证。
- 严格Clippy lib+该审批测试 `-D warnings` 与rustfmt检查通过。原vendor crossterm依赖警告保留。

225个输入按最终构建时文件核对无漂移，变更前后3文件及精确正逆product.patch均独立核验。初次候选 `c8e491ec…` 也通过同组检查；随后只将来源标签宽度从20扩到28以保留现有同名目录短ID，最终构建重新运行52测试、Clippy及4+14+11实际检查。两次通过不累加计数。本次没有新的失败实现尝试。

所有原始PTY、HTTP（新增fixture）、stdout/stderr合并日志、命令/退出收据与源码hash保留。本次不重建release，不重复冷启动预算；debug大小不与8MiB release预算比较。旧release冷启动1044.82ms失败继续存在。新badge尚需纳入下一次自然release的最终全量入口/预算验证。

ZS1-124 保持 `[ ]`，其余preview/完整通用UX与逐文件/目录依赖未由此豁免。先前审批选择器证据见 [历史主控复核](../master-approval-submenu-3.1.19/review.md)。回退仅对本次3文件应用product.patch反向差异，保留用户后续改动、项目和journal，不重置仓库。
