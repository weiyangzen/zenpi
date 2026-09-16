# 当前 release：后台审批、项目入口与预算验证（3.1.20）

本次自然重建包含后台审批来源提示的release，并首先执行一次5样本冷启动预算，再在同一冻结二进制上运行7套真实交互检查。release SHA256 `1d7b2e61ecc9096ad0f5e6cd3f3c03c6820cafe8371d9201de3497bb6b4e8f58`，6033168B；构建前后226个输入逐项hash一致。二进制在主库 `.ops/stage1_execution/release-background-20260912/zenpi` 保留。TUI源仍为 `d2a98934d4931a5aa202e542acd556bdda5667f0e60df11a68731dacf00eccf1`。

## 当前预算

8/8门禁通过：正常直接依赖15个、release小于8MiB、5次启动最大751.967292ms小于1000ms、进程峰值RSS 5111808B小于96MiB、queue 1593.783µs小于2000µs、render 662.788µs小于10000µs、layout 1.07245µs小于100µs、10000次dirty合并为1帧。

启动原样本为751.967292、44.427125、45.611625、45.128208、38.709ms。没有预运行新zenpi，没有删除首样本、改阈值、改用median或重复直到变绿。此次通过只证明该新release本次样本，不证明启动波动根因已修复；旧release的1044.823959ms及更早1208.732708ms失败原证据保留，见[此前预算记录](../master-release-budget-3.1.20/review.md)。

首次启动stdout保留完整成功shutdown response，stderr保留time(1)的0.74s real及RSS等统计。记录的PID 3781是time包装进程/独立进程组leader，不是已经辨认的zenpi PID；Popen返回耗时1.384292ms，communicate等待750.189583ms。这只能将父进程所观测耗时粗分为spawn和等待，不能据此归因系统策略、磁盘、产品初始化或某个内部函数。预算进程树1.95GB内存包含编译器，不能冒充zenpi运行时RSS。

## 同一 release 的真实入口验证

- 后台审批4项检查通过，1个TUI、2次loopback HTTP：当前项目、Unicode草稿及布局保持，原项目badge和footer出现，只有切回、Alt-A、y、Enter明确确认后才在原cwd写入，结束后提示消失。此次补齐先前仅debug验证的release缺口。
- 既有审批14项通过、12次HTTP，涵盖显式确认、拒绝、取消、picker焦点、错误项目、顺序请求及独立重启。full_diff_scrolling仍只覆盖80行fixture；错误项目用旧helper的`/approve stale once`，不能扩称全身份tuple任意竞态已验证。
- BentoBox11项通过，3个独立TUI，涵盖键鼠布局、缩放、折叠、焦点、项目间独立布局和草稿、两次重启。顶部+选目录fixture恰有一个直接子目录，所以点击+、Right、Enter三次操作成立；不将它泛化为所有路径都固定三次操作。
- 项目picker套件12项断言通过：顶部+直接打开picker，Esc/无效路径不创建占位tab，Unicode/空格路径及alias、同basename区分、项目配置实际改变backend model、两个选中根目录实际shell写入、session header与tools cwd一致、切项目后的迟到输出保持来源、重启及终端恢复。
- `/new`三套共20组通过：主流程8、policy9、recovery3；共7个TUI、11个headless JSONL进程、31次HTTP。验证同项目新会话、下一请求只含新上下文且tool仍用同cwd、幂等重放、不自动提交保存草稿、恢复model/effort、提交前取消、并发CAS writer、pending/审批/scheduled拒绝、后台项目不阻塞idle项目、remembered授权不泄露到新session、独立重启和提交前后强杀恢复。完整grace/postcommit时序范围仍未由此全部覆盖。

## 诊断脚本改动

此前tools/bench_runtime.py丢弃成功启动stdout/stderr。本次新增每样本固定身份、父进程阶段时间和精确原始字节前缀/完整流hash；每流最多输出64KiB前缀，明确truncated。编码、hash和写文件在外层计时结束后进行；所有最大值阈值和样本聚合表达式保持。每次使用新证据目录，以排他创建防止覆盖首样本；样本非零退出或timeout先保留证据，再抛出错误。timeout清理该探针独立进程组，避免只杀time包装器而遗留其子进程。

4个实际Python fixture测试通过：成功输出保留CRLF原字节/身份/时序，失败的partial输出持久保留，timeout保留输出且延迟写入后代被终止，64KiB界限及拒绝覆盖。fixture不执行zenpi或Cargo，不能计作产品功能测试。两文件product.patch独立临时Git正逆apply/check及sentinel核验通过。仪器EvidenceLint、ROI、ParetoGate和回退记录见diagnostics/instrument-review.json。首次AST比较尝试因literal_eval不支持乘法表达式失败，未改文件；改为比较完整预算AST后通过，原尝试单独记录。

本次仅新增Python诊断测试；此前52项Rust、Clippy及背景审批实现见[原增量证据](../../ZS1-124/master-background-attention-3.1.20/review.md)，不重复累加为本轮测试。

本报告是ZS1-117/124/132的当前release增量证据，complete=false，整项仍为`[ ]`。缺失的逐文件、逐目录依赖、其它生产边界、审批preview及完整UX合同继续逐项处理。304的独立G-FILE在本轮另行接收为`[x]`，总清单21/112；不从7套测试通过推导全蓝图完成。

回退诊断脚本仅反向应用diagnostics/product.patch；回退后台提示使用其原3文件patch并检查后续差异。不得重置仓库、覆盖用户改动、删除项目会话或修改旧失败证据。
