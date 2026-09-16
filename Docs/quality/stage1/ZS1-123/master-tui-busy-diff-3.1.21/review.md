# 主控复核：TUI 忙时本地差异读取

正式 TUI 在模型忙时会拒绝 /diff、/review；不能借用 Agent 时还可能误用进程工作目录。本次将生产入口改为独立的单槽本地读取线程，在借用 Agent 之前处理 Diff，准入时绑定稳定项目标识、绝对 cwd、会话路径/ID 和草稿 generation。结果仅回原项目和原会话，切换项目不会切换结果归属；替换会话会取消并丢弃旧结果。Ctrl-C、/cancel 和退出传递取消信号，Unix Git 子进程以专属进程组回收并 wait，线程退出时 join。后续输入优先于失败回执的草稿恢复。/review 仍是本地 Diff 别名，不是 Codex 模型审查全功能。

逐文件审查：主控完整阅读原 src/slash_actions.rs 的1644行、全部双文件候选补丁（含新内部测试）、完整report/verify/两个PTY脚本及helper，另读TUI提交路由、草稿恢复/epoch、项目元数据/会话cursor、消息追加和退出边界的明确范围。仅据此审查TUI改动及调用边界，没有声称完整阅读638305字节TUI或完成085/087的整文件理解验收。复制封存worker后仅一次运行已读纯离线verifier，862项包内核验通过；它不等同产品测试。原主控 busy regression 的1通过2失败、worker正式d0cc PTY before的6通过3失败及全部旧夹具/编译/角色断言失败均保留。原tests/slash_actions.rs和主控tests/tui_busy_diff.rs保持原字节。

主控发现并实测修复了候选遗漏：可取消Git状态读取超过32KiB时先截断字节，导致原有截断提示消失。新真实Git回归创建500个普通临时文件，对比原入口和可取消入口，候选实际失败0通过1失败，明确报“controlled status silently omitted its truncation warning”。保留该失败源码和日志后，在受控status路径恢复提示并保持总字节上限。新测试比较两入口完整status相等且都带截断标记。原worker候选未改写；root-vs-worker.patch独立记录此修复及新测试。

当前主库TUI为0deb8e0e13cfdad0aa00e07d79caf573e9aa36095a9bcefb4219ed8f70936557（638305B）；slash_actions为f5dbc7862462d314bb8880ad6ee0f02d7c38aafa8b96196a08acb9773490b497（76284B）。工作线程slash候选3ff4bfdc和最终release a15a595e保持独立身份，不作为主库最终测试的替代。root-product.patch只改TUI/slash_actions；不修改BentoBox布局代码、主库headless或原用户状态。

主库实际107项Rust全部通过：slash_actions内部24、本地host内部5、headless项目13、布局持久化7、原slash集成11、BentoBox16、busy回归3、命令面板17、TUI项目11。fmt与all-targets Clippy -D warnings通过；vendor已有unused_parens警告保留。225个捕获输入在测试、构建和预算/PTY期间逐次保持一致。此前226项root清单额外包含README，单独确认其原始hash仍不变，未伪称本轮225清单包含它。

新主库release为6093328B，SHA256 5f1e513355f4e9aa228ada8ef6213e0255ec2247bd414e13668b44cdaa948588。首次运行之前使用原预算脚本、原3样本、原计时边界、原负载：1039.683291/38.870666/42.970375ms。首样本超过1000ms门限，预算exit1、cold_start失败，其余7门禁通过。无预热、重试、丢弃样本或改阈值。此前d0cc的1066.501958ms失败同样保留；两二进制的差值不是修复因果证据。117仍未解决，根因未确定。

预算后，同一不可变root二进制在全新输出目录运行42项实际PTY判据：本地Diff9、慢Git10、项目12、BentoBox11，均通过。root新脚本复用已完整审查的worker oracle与helper，仅加明确root用途注释，原封存脚本/输出未重跑覆盖。慢Git在stdout之前真实门控，provider在首delta之后门控，仍可键盘输入和切项目；迟到结果仅进原B/session，A/B草稿与布局保持。Ctrl-C在门控未释放时回收，槽位可复用；第三次门控Git在正常quit时回收，无夹具先释放才退出。观察取消0.1141575秒、quit0.638335833秒仅属夹具，不是产品预算。HTTP始终一条。项目与BentoBox官方工具另证顶部加号直开目录选择、选中路径命名/cwd、无效/取消不建占位、shell目录隔离、迟到输出和重启恢复。主控另从保存回执、文件、HTTP及PID/组存活查询独立核对34事实；它们不计作额外PTY/运行次数。

限制：实际平台macOS arm64。新受控入口非Unix明确unsupported，Windows功能与验收仍待补齐；不能以此候选宣布跨平台完成。内核文件系统调用不能合作式抢占，30秒是检查点和Git等待deadline，非任意挂死FS的硬实时关闭保证。同步嵌入入口仍同步，只在缺少tab元数据时使用显式Agent工作区，完全缺少上下文时失败。完整模型review、全TUI命令对齐、085/087完整理解、123全项及117性能都未完成，蓝图保持40/121。本次是已整合且有功能回归证据的具体差距修复。

回退边界：逆向root-product.patch可恢复此次两个产品文件，先核对当前字节且保留其它dirty改动、所有证据和用户状态；不要reset/stash/clean或回滚整库。主控新busy测试来自此前独立反例，不随产品补丁删除。
