# ZS1-123 主控文件补全状态与矮屏修复 — 3.1.21

已合入局部修复，仅src/tui.rs和tests/tui_command_palette.rs。完整123仍[ ]，不接受全TUI或release预算。用户在/attach、@以及已有文件查询入口能看到结果是否完整、扫描或显示受限的原因；完整空扫描仅说明当前目录无匹配，提前结束的空结果明确只覆盖已扫描部分。Tab/Enter仍选择文件，Esc保留草稿，顶部加号直开目录与BentoBox保留。

主控完整阅读worker README、两文件candidate.patch、扫描/查询/接收/菜单/选择/鼠标与键盘有界上下文、新单元与集成用例、verify_packet.py、pty_completion_probe.py和原运行记录。没有宣称完整阅读648110字节TUI、全部旧测试或所有历史证据。扫描继续take2048、合作式150ms软截止、显示128；恰好2048保守标EntryLimit而不额外窥读一项，不保证一定有遗漏。deadline无法中断单个阻塞文件系统调用。新增cancel检查位于枚举结束后、排序之前，不是不可分割的最终发布事务；当前query含root并受active job接收检查。

root独立新执行已读worker便携校验一次：723/723通过、exit0，cwd=/tmp，无新产品或旧runner执行。完整450payload/manifest以及原worker失败保留。worker before两失败与其99回归、11项PTY有明确私有基线，本次主控另外建立自己的下列证据，不把重叠检查混加。

主库原TUI b15661a…639010字节、palette 3b84deaf…25893字节。先新build并保留原生ARM debug，再仅放入worker最初两条显示反例：1旧例通过、2新增失败exit101，证明原菜单缺partial和完整空提示。随后集成原worker实现、添加主控独立矮屏用例（9/10/11行）仍真实失败exit101：9行时此前可用的候选完全消失。原worker candidate和失败屏幕都保留。

主控补充修复：先保留原有一行可选候选，新增说明只使用额外可用行；当屏幕较矮时菜单标题仍带partial，不能因两条说明塞不下而隐藏全部文件。没有扩张BentoBox或引入快捷键。最终定向lib3+palette6通过；之后十组完整回归共162通过（layout11、layout_persistence7、project_workspace7、approval12、bentobox16、busy_diff3、palette22、composer51、interaction22、tui_project11）。定向6已包含在162内，lib3另算，总共165个不同Rust测试。new短屏用例同时验证实际可见文件、partial标题与Tab选中同一文件。5行以下菜单原有空间限制仍存在；未以这3个高度证明任意尺寸。

owned rustfmt检查和原生ARM locked/offline clippy --lib --test tui_command_palette -- -D warnings均exit0；vendor原括号warning保持，不声称所有依赖零warning。主控新after-build成功，当前debug二进制50764248字节/SHA256 5e631c840c337c449c48930aa0c41cc2ba79a3bab9929029ccc6635d0fec6a8d；before50742696字节/a4b24e76689e202dc009167e7c0b95612c98a287177c804c1ca35ff4207bff80，均存档。

主控新派生独立PTY脚本，源脚本先完整读取，未运行归档runner；新输出目录/项目/配置/会话，绑定本次after binary。140×40实际PTY一次11项全部通过：partial及缩小路径/前缀引导可见，输入199后结果更新为单文件并清掉partial，Tab补入candidate-199.txt而非提交；完整空提示、Esc保持draft、正常exit0及waitpid回收、alternate-screen退出序列、0HTTP和0turn。raw及4个screen投影与完整fixture保存。该投影仅解释有限cursor/erase，不冒称像素截图、全终端模拟或termios恢复实测；9/10/11行由TestBackend回归证明，未伪报真实矮屏PTY。

202捕获构建输入每个执行阶段前后核对，除两owned文件按阶段改变外身份一致。HOME/Cargo/依赖未修改。公开API complete_file_query现返回FileCompletionResult，调用方需取.entries；本仓src/tests调用均适配，外部消费者兼容性不由这些测试证明。缩小同目录前缀无法保证越过2048原枚举范围，用户亦可缩小目录路径或输入已知路径。完整模型review及其它123要求仍待独立验收。

本次不运行release/冷启动预算/117/131/FIFO/DTrace，不抹去既有失败。回滚限两文件此次差异，完整before/worker/root候选和正差异分别保留，不能全文件覆盖其它后来改动。审批计时三个主控组件反例另在125/master-timer-observation-3.1.21，B正独立修复；该未完成修复没有混入当前输入。
