# ZS1-071 主控完整文件复核与接收（3.1.20）

接受范围：`src/runtime.rs` 的逐文件完整现状和差距学习，G-FILE。不把文件学习接受解释为runtime全部边界无缺陷、ZS1-101/132功能交付或目录已经接受。

主控在当前主库按1–340、341–660、661–912顺序完整阅读912行/33333B，hash `7a42c428083f8f3edb0019311296b479004ff562b548d7cdc504729f7cbd1f21`。冻结基线28121B/763行/hash `2183b622d96a8fdc084784af8f537fa09a4e1703b5ff70de3bc459d29fb05315`与已读当前文件前28121B逐字节相同；阅读覆盖该完整前缀，没有声称另做重复全文阅读。当前仅追加InputBoundary/Gate。完整阅读26628B候选报告、5090B离线verifier、历史4probe源码及当前tests/runtime.rs全部12测试的实际断言。两版runtime都没有内联测试。

## 独立结论

1. 两个有限通道、单active、pending FIFO的所有Submit/Cancel/Shutdown/完成/panic/断开分支与报告一致。try_submit只承诺transport admission，关闭时未读取的尾请求可无终态；JobId在各runner从1开始、失败也消耗、未知cancel静默，不能用数字跨runner共用表。normalized把max_pending=0也升为1。
2. emit阻塞，host必须持续drain；Started前job已spawn。done优先于新命令，普通完成先join再分类。两个AtomicBool不是先到者赢的原子终态：mark_completed可遮蔽早已观察的取消。grace到期直接detach并返回Cancelled，不经过completed分类，因此不能以Cancelled推断持久提交回滚。
3. Closed与guard只证明调度线程/结果通道终止，不能证明所有detached线程释放Agent。shutdown helper会消费并丢弃业务事件，Drop只是best effort；done线程退出析构/事件背压也不受统一wall-clock上限保护。系统线程spawn耗尽、析构阻塞、ID回绕及所有内存交错未实测，保持静态范围。
4. Gate的合法ID、整批tool去重/≤32、prepare状态、epoch失效和cancel永久拒绝boundary均已逐方法核对。builder换parent及Drop不撤销已发boundary；would_stop来自调用者，不能独立提供session归属或授权证明。InputQueue/Core负责真正消费与持久化，这份报告不能代替它们的逐文件审阅。
5. 历史probe的4个具体观察与源码匹配：事件背压推迟cancel/tail admission无回执；marker遮蔽已观察cancel；零pending归一到1；旧boundary在builder/非法batch/Drop后仍可应用。历史runtime12+input_queue17+probe4=33仅原执行环境证明，1个ignored恢复helper不重计；并非本轮重跑33或完整当前host验证。

## 新主控验证

当前主库实际执行 `cargo +stable-aarch64-apple-darwin test --locked --offline --jobs 1 --target-dir .ops/stage1_execution/new132-integration/target --test runtime`：12 passed / 0 failed / 0 ignored。测试源码与历史冻结runtime套件逐字节一致；主控读完全部断言：off-thread admission、同Agent会话连续性、FIFO、pending容量、active/pending取消顺序、晚marker、panic后存活、显式关闭、消费Closed后join、饱和通道关闭和25ms非协作detach。非协作测试最后仅发release信号，没有join脱离线程，不能把其注释当更强证明。

离线verifier验证8个候选payload、旧16个payload与139个归档输入逐个hash/bytes、基线/当前连续读范围、历史测试命令/返回值；临时Git正向精确应用、反向恢复接收端不存在基底均通过。归档只用于保留可追溯历史，没有执行旧归档runner或篡改旧输出。报告开头旧“本次执行/3.1.16/[_]”为完整保留的历史段，3.1.20补充明确本轮worker行为实验0；主控新12测试另记此处。旧第一段把28121B对象称作“旧报告”的措辞指源基线身份，不用作报告字节数证据，实际旧报告12129B/hash56782ccc…已核验。

最近跨项目/new生产增量已有独立20组实际入口证据，调用方分离JobId和保存实际结果符合这里列出的合同；该增量没有覆盖全部grace/postcommit组合，仍在132未闭合范围中。此文件接收不改变那个状态，也不抹掉release冷启动失败。

回退：只撤回071新报告、证据和本项状态，恢复已备份完成面；不覆盖src/runtime.rs或用户产品改动，不删除会话。
