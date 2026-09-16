# ZS1-132 — 同项目 /new 增量集成与主控复核

蓝图 3.1.20；本记录证明这次增量实现及列明场景，**不构成 ZS1-132 或 Stage 1 全项验收**。清单保持 `[ ]`，逐文件/目录依赖与全局门禁继续独立执行。

## 合入行为

`/new` 作为无参数 slash 命令进入现有菜单。TUI 与 JSONL command 共用当前项目的同一个 Agent，创建独立空 journal，原子提交 project→session 映射后再切换内存 owner、撤销旧 InputPort。创建保留 project ID、cwd、分页和 BentoBox，延续 model/effort/persona；创建本身不调用模型或工具。已提交的命令不会成为用户 turn；旧路径仍能通过 session open 恢复。

| 文件 | 本次复核与改动 |
|---|---|
| src/slash.rs | 注册唯一 `/new` 语法、菜单说明及 SessionAction::New；有参数和 `/session new` 仍拒绝。 |
| src/session.rs | 独占创建候选、同父目录有限碰撞、Unix 0600/no-follow、持有文件身份核验、仅清理本次候选；拆出沿用的有序记录编码/写入。新增失败后的重复扩展启动审计同步。 |
| src/core.rs | idle/队列/未知结果/恢复/附件门禁，偏好 seed，复用 prepare→commit→replace；保留配置策略，重建 session 记忆策略；失败仍保留原 owner。 |
| src/project_workspace.rs | 同一持久 owner checkpoint 的 CAS 是提交点；新 journal 预写可恢复 transition receipt。 |
| src/headless.rs | 先恢复项目映射再建立 replay owner；请求绑定旧 session scope；重复 ID、冲突 payload、提交后 replay 打开失败按真实结果恢复。 |
| src/tui.rs | 现有可取消 worker 执行 `/new`，恢复命令草稿并刷新新身份转录；只有已知 session ID 或 journal 路径变化才丢弃旧投影，保留正常分页缓存。 |
| tests/session_new.rs | 11 个测试，覆盖命令、偏好、重放、busy、真实文件失败/替换、准备阶段取消、pending 状态及旧 InputPort 在新 turn 运行时被拒绝。两个原生扩展编译夹具互斥，产品 1000ms 限制保持不变。 |
| tools/tui_session_new_smoke.py | 8 组真实 TUI/JSONL 场景与独立 `--recovery-only` 的 3 组真实进程恢复场景。 |

## 主控发现并修复的两处问题

1. 第二个真实 headless 进程在 TUI 准备阶段抢先提交，TUI 的 CAS 拒绝正确，但另一个进程启动时追加 `extensions_selected` 导致旧 owner 下一条输入报 writer stale。保留实际失败 PTY/HTTP/journal 证据。修复只同步与当前选择完全相同、最多 64 条的重复扩展审计尾部，并核对已有记录前缀及干净恢复状态；不修改旧 journal，不替换 InputPort。出现模型、授权、工具选择变化或损坏尾部时仍要求显式恢复。真实并发复测中旧 TUI 下一请求成功，另一进程的 checkpoint 保持原样。
2. 候选将“没有历史游标”当成“会话已变化”，使关闭另一分页后清掉同一会话的缓存消息。已有 `switching_while_original_owner_is_locked_preserves_drafts_layout_and_owner` 回归实际失败。修复使用已知 ID，缺少游标时使用保存的 journal 路径；现有草稿/布局/消息保留断言和真实重启转录更新均通过。

## 实际验证

- 8 套直接 Rust 回归 **121 passed / 0 failed / 0 ignored**：session_new(11)、project_workspace(7)、headless_project_workspace(12)、tui_project_workspace(11)、resume_compact_owner(6)、session_recovery(39)、slash(19)、tui_bentobox(16)。另 3 个审计恢复单元测试通过，包含修改策略、不同工具选择及损坏尾部拒绝；子用例不另计数。
- `cargo +stable-aarch64-apple-darwin clippy --locked --offline --jobs 1 --lib --tests -- -D warnings` 通过；fmt --check 通过。保留 vendored crossterm 原有 dependency warning，未改 vendor。
- 最终固定 **debug/default-feature CLI** SHA256 `78cf6b9795b5bc6487fd3e3cd280d3f64f84fdcb6a8f1c7a7c4d7b596eceaa6e`：8 组实际入口通过，**4 TUI + 5 JSONL 进程 / 6 HTTP**。覆盖空历史、同 cwd 工具、旧/新恢复、独立重启 replay、Unicode 折叠草稿与偏好、Ctrl-C、真实第二 writer 的 CAS 冲突及旧 owner 后续请求。
- 同一最终二进制的 3 组恢复通过，**4 JSONL 进程 / 4 HTTP**。准备阶段扩展初始化被实际门控后 SIGKILL，重启恢复旧身份；观察到持久 checkpoint 改变、未消费命令响应时 SIGKILL，重启恢复新身份且同 ID 不增加 journal/HTTP；真实 `.reconnect` 目录障碍使提交后 replay 打开失败，stderr 明确报告已创建并提交，解除障碍后同 transition receipt 恢复成功。
- SIGKILL 提交前留下的未提交候选保留在测试证据，未被当成已提交会话或自动选中；正常取消的候选清理通过。进程终止不冒称磁盘断电恢复。
- 225 个最终输入文件在实际入口验证前后哈希一致；合入的 8 个实现/测试文件与这份输入清单逐字节一致。固定二进制留在私有集成目录；公开证据包含源码补丁、输入哈希、命令/返回值、原始 PTY/HTTP/JSONL 与最终 fixture journals。

## 保留的失败和未闭合范围

首次默认并行回归的扩展文件替换夹具未到达初始化门；串行运行该用例通过，最终只串行化两个需要原生编译的扩展夹具后，默认测试线程下 8 套全部通过。未放宽产品超时，也未把未到达前置条件计为取消通过。首次并发实际入口失败、首次既有分页回归失败均保留；成功结果使用新日志，不覆盖失败。

尚未完成：本项全部 busy/待审批/未来输入/其它 owner 操作组合、跨项目独立工作与迟到 reply 的完整矩阵、configured Deny/worker/process-only/remembered approval 作用域的专门新会话实测，以及完整 Stage 1 入口集、最终 release 预算与所有逐文件/目录主控验收。旧的 1208.732708ms 冷启动失败与 1000ms 阈值保持；本次夹具预启动及 debug 二进制不能替代 release 门禁。Windows 和其它平台未执行本次场景。
