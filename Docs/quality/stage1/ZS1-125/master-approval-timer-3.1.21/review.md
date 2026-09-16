# ZS1-125 — 审批等待计时与底部提示的主控整合（3.1.21）

当前主库已修复实际审批等待计时与隐藏审批的底部提示。最终源码 `src/tui.rs` 为 668438 B，SHA256 `e45131dcf190d55068e369e55b8a3cc0dda18234ccee2c2ca335c9cefdd15904`；`tests/tui_transcript_ux.rs` 为 18860 B，SHA256 `57a0d41b339c096b17f306c87004bdf9839ddbec0a07ee5821016f49f26b5254`。本次仅这两个产品/测试文件改变。此记录接受本次局部修复，不关闭整个 ZS1-125，不替代其全部 G-CODE/G-HOST、source/target G-FILE 或 G-STAGE。

## 具体行为

原 `set_status` 按文案是否包含 Approval 暂停/恢复，真实请求出现/退役不驱动计时；因此隐藏审批窗后 Alt-C 显示复制提示就会继续计时，最后一个真实审批完成后又可能不恢复。现在状态文字只显示信息，计时由本项目的 busy、真实 ApprovalView 请求、当前 job canonical 待批 ID 与终态共同控制。审批先于 busy 出现也暂停；未知退役、重复/未知/Pending resolution、隐藏弹窗与项目切换均不能错误恢复。最后有效请求清除后继续累计工作耗时，已获接纳的取消只结束等待，真正终态结束活动。Elapsed/Instant 和 canonical 状态随既有 ProjectDraft 存取，后台项目不会因隐藏重置区间；未引入跨进程计时持久化。

canonical 事件统一拒绝非当前 stream job；只调用公共 apply_view_event_for_job 的测试需要显式 begin job。生产 Model/UserShell 入队已有 begin，真实 provider Completed 在 drain 中被排除，不能把一次 tool round 的 provider 完成当成整个 runtime 完成。当前 job 的 Completed/Failed/Cancelled/Closed 结束计时，旧 job 不得终结替换任务。真实与 canonical 列表各限 128，超过上限采用饱和标志保守暂停，直到取消接纳/终态/替换现有 job 清理；这不解决原始审批界面队列上限，更不宣称无限审批可用。

主控补充修复了真实 host RespondApproval 的错误分支：应答报错本身不是有效 decision，不能直接 retire 请求。该分支现在只显示错误，由既有 coordinator `!is_pending` 对账决定退役。respond_tui_approval 原有 project/request/turn/call 校验和协调器应答顺序保留；其错误身份/真实拒绝/重复应答测试通过。此次没有单独通过故障注入触发整个 host 错误分支；该分支变更有直接源码审核，不能将底层函数测试冒充 host 故障注入。

真实终端观察还发现底部提示叠字：隐藏审批时 render_approval 在已经绘制的快捷键 footer 上仅覆盖短字符串，留下 `reviewU line kill ...`。同一路径覆盖 `Draft not saved` 警告。两条独立 TestBackend 原失败分别证实。现由 render_footer 单点选择提示，优先级为保存失败、当前隐藏审批、后台审批、其它快捷键；render_approval 只绘制聚焦弹窗。新测试覆盖 180/40/80/180 列、草稿和 pending 不变，以及保存失败仍可见。没有修改 BentoBox 布局、顶部加号选目录、项目命名/cwd、审批默认 Deny、Enter 确认、隐藏/取消快捷键。

## 主控独立阅读与迁入

完整阅读 worker README、全部 candidate.patch、verify_packet.py、PTY v4 脚本和本次主控增量；核对当前 TranscriptUx、ProjectDraft 存取、present/retire/respond、状态与 job/terminal 入口、真实 host 对账/取消/完成和事件缓冲调用段，及 footer/modal 两个生产绘制调用点。是本次变更及其必要上下文的语义审核，不是 src/tui.rs 全文件 G-FILE；085 独立任务负责全文阅读。

worker 基于 b156/639010 B；主库已含 123 文件补全和矮屏修复的 a19/648110 B。原补丁 `git apply --check` 在测试插入点冲突，未直接覆盖主库。21 个 hunk 逐一迁入，生产 hunk 精确匹配，测试块移至同一测试模块的 normal_unicode 测试前，保留所有 123 测试。root-candidate.patch 保存 a19→9dea，root-footer-increment.patch 保存 9dea→最终，root-final.patch 保存完整 a19→最终。palette 的 32042 B/9d4e562f 与 composer 的 52941 B/30d9b19b 未改变。

完整 worker 冻结包原样保留：335 payload，manifest SHA256 dd615158393e266b72af8636bc73ae687f3d6138375a41e23686cca9acbc68e0。主控全文静态检查其 portable 后只新执行一次纯离线审计：671 检查通过，exit0；它验证身份与证据，不是 671 个行为测试。202 个当前构建输入逐项冻结，主控每个 Cargo 命令前后校验全部输入；交付候选、初始源和最终源保留。

## 实际验证与失败记录

- 主控对当前 a19 加入六条回归，真实执行 5 失败、1 对照通过。worker 同类旧基线失败也原样保存，两个来源不混淆。
- 第一次 after 错误复用了 before 测试二进制：copy2 保留较早候选 mtime，Cargo 日志未重新编译，仍只有 6 条测试。源码 bytes 确为候选，旧二进制/日志保留；该失败不提供候选语义结论。随后只更新 mtime，记录明确重新编译后的 13 项通过。后续安装使用 write_bytes；无删除缓存或旧证据。
- 初版计时修复：13 新库测试、1 既有 canonical、179 项 11 组集成回归通过，格式/独立 Clippy/debug build 通过。初版主控 PTY 的 12 个交互检查通过、zenpi exit0；额外 journal 计数因错误读取 event.type 将 session/turn 算成 unknown 而失败，driver exit1。全部原始结果保留；完整 6424 B 原 JSONL 离线复核确认一次 local user_shell turn、allow 后执行、输出及子进程回收。没有把初版 driver 改成成功。
- 初版终端显示暴露的两个 footer 问题，均有主控新测试原失败日志。最终修复后：15 项 timer/footer 库测试 + 1 项既有 canonical + 179 项集成回归，合计 195 个不同测试通过。11 组集成是 project_workspace 11、layout 7、layout_persistence 7、tui_approval_focus 12、tui_bentobox 16、tui_busy_diff 3、tui_command_palette 22、tui_composer 51、tui_interaction 22、tui_project_workspace 11、tui_transcript_ux 17。最终 owned rustfmt、独立 Clippy、native ARM debug build 均 exit0；保留既有 vendor/crossterm unused_parens warning，没有修改依赖。
- 最终产品 footer 已改变，因此使用新独立脚本与新私有 ZENPI_HOME 进行第二次主控 PTY，旧脚本/旧输出不动。新脚本同时纠正 journal kind 读取。14 检查全部通过，driver/zenpi 均 exit0；binary 50751432 B，SHA256 `4f0c07beeb14a0761c40054f32f21afc1acf0225c362cec6a3fde511ef7f6d93`。等待审批时 Esc 隐藏、Alt-C 无关提示、干净 footer，计时 0→0 持续 1.3 秒；审批前 gate 未到达；Alt-A/y/Enter 批准后真实 curl 到本机 gate，等待期间 0→1；释放 gate 后输出并去掉 busy，正常退出/alternate screen 恢复。1 个本地 GET /gate，0 模型 HTTP，1 条 user_shell 结果 turn，非“零任何 turn”。PTY raw、屏幕文本投影、完整 journal、输出 artifact 与 run 回执保留，pid 后续 ps 确认退出。

## 保留边界和回滚

上述屏幕为有限 ANSI 文本投影，非完整终端仿真/像素截图；footer 叠字另由真实 Ratatui TestBackend 证实，保存失败优先级是组件测试，未模拟实际磁盘故障。计时测试以私有 Instant/Duration 和真实 coordinator 握手检验，未等待固定秒数制造单测偶然性。最终 PTY 的宽度为 180 列，多个窄屏尺寸由组件测试覆盖，不冒充各尺寸都跑过 PTY。

worker 原广泛 lib 执行的 unrelated diff pidfile ParseIntError、5 ignored、编译 Value/String 失败、canonical 前提缺失和 PTY v1-v3 失败全部保留；主控没有运行或重试那些旧 runner。无 release 构建、117 首启重测/预热/门限更改、131/FIFO/DTrace 探针。117 的 1039.683291 ms/1000 ms 原失败继续有效。既有125长回复修复及主控三计时反例记录保留；此处不提升整项状态。整项还需要依赖及全部滚动/复制/错误/资源上限/权限/取消/重启/终端验收。

回滚仅在当前源码身份仍匹配最终候选时反向应用 root-final.patch 的两个文件差异；如已发生后续修改，逐 hunk 撤销本次增量，保留 123、122、BentoBox、项目目录行为、用户草稿和会话。不得恢复整个旧 worker 文件或删除证据。
