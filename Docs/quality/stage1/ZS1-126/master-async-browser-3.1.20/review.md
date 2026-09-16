# ZS1-126 主控：异步会话浏览接入真实 TUI

主库已将周期浏览、显式 `/session list` 和 `/session search` 的目录读取移到单一后台线程。键盘/绘制主循环仅复制当前归属、非阻塞发送/接收有界请求及投影结果。BentoBox、顶部项目分页、上一轮单条回复尾部渲染均保留。此记录接受局部修复，不接受ZS1-126/085整项。

## 审查及合并

完整阅读A 807行补丁、review和verify；阅读当前主库的快照刷新、草稿回执、浏览/搜索、同步dispatcher、run_async启动/循环/路由/退出、drain以及必要session/headless实现。A的完整旧TUI 14,461行阅读记录及全部不变段、128输入编译上下文由离线verify核验；主控本轮为变更及调用链复核，没有把少量其它owner片段写成其整文件理解完成。

A候选原始before e0e509…；主库合并前已经包含B尾部接线，hash2d3373d72fc5ad68b9e266824fe4c31c37db7b1c732927b4e38f4daecf8e53bd。主控独立验证“当前B上应用A”与“A候选上应用B”的整个TUI文件逐字节一致，组合结果994215c7ca6cc471bb35247a04460d6c5ef52192999e4b5585ea1c818a429578。没有用旧A文件覆盖当前B实现。198个现有Rust/vendor/Cargo/尾部smoke输入中只有src/tui.rs变化，其余197保持。组合后单文件补丁在临时Git中正反check/apply、exact bytes及无关二进制哨兵通过。

请求/结果各sync_channel(1)，host最多1active+1可替换pending；新generation使用checked_add，closed/overflow拒绝显式新意图。投影比较project、session path、session ID、directory、query和generation；失败回执也受相同归属限制。路径选择周期刷新保持，删除后clamp；成功查询才提交过滤并重置到第0行，失败保留旧query/rows/cursor，旧回执不覆盖已有新草稿。项目切换按原ProjectDraft保存恢复这些状态。后台不持有Agent mutex或TuiState。

List的owner pane扫描当前session目录；其文字回执仍是全局ConfigPaths.sessions。Search的pane和文字回执都读取owner目录，显式search原有两次扫描仍保留，现都在后台。默认同步嵌入者保留旧helper行为，生产run_async在同步dispatcher之前截获List/Search；未改headless/session模块的扫描政策。

## 实际主库验证

原生 `cargo +stable-aarch64-apple-darwin`，测试/Clippy/构建均 `--offline --locked`：TUI lib29、headless_project_workspace12、tui_project_workspace11、tui_transcript_ux16，共68通过，0忽略。Clippy --lib --tests -Dwarnings、fmt、debug构建均0。日志保留既有vendor warning与测试主动注入的scanner panic，它是关闭回执测试的输入，不是未解释产品panic。

确定性Rust屏障证明：scanner线程已进入并等待release时，主循环实际同一drain×20、键盘、Resize和BentoBox render已完成（本次3327µs只是观察，不作为性能门限）。102个查询意图只执行首末2个scan，同query旧generation与project/path/session/query替换结果被拒绝。A原始“接回同步”实际negative退出101及最初错误注入脚本完整保留并离线绑定；主控没有重跑该worker negative，也没有将错误命名的通过日志当成有效反例。

本轮冻结debug二进制86810a096d7a2f18c55f1b80eb5a25cd51bbb3b02cc6a2762d08b0eea1e33457（私有zenpi，555）。没有新release预算或全主库suite结论。

## 真实入口和探针失败修复

完整阅读worker probe536行、verify30行、精确screen_adapter及README，拷贝并离线verify；其原包56文件的历史CLI/offline结果不能代替PTY通过。

第一次实际运行原探针：list双范围、selection保持并Enter打开准确路径、有效search失败保留旧投影、project独立query四个场景均通过，退出观察时报ENOTTY25。异常发生在PTY controlling-session leader退出后读取slave termios；整个run仍标failed并保留raw/result/traceback/fixtures，没有将四场景通过提升为终端恢复通过。

接收端仅修改私有probe副本：保留一个类shell的controlling-session supervisor，实际sandbox-exec/zenpi子进程正常退出并回收后，先比较slave termios，再显式释放supervisor。控制使用匿名pipe，环境HOME/CODEX_HOME原值和原macOS隔离策略不变；没有更改产品或删除恢复断言。清理向本次实际supervisor发TERM，其对子进程定向TERM/必要KILL并回收，父进程有界回收supervisor；PID与退出码原样存档。原probe不可变，差异/原因/hash另存probe-receiver。

修正后的新目录实跑一次：四场景与正常退出全通过。实际zenpi exit0、supervisor exit0、alternate enter/leave均在raw中，termios各字段与启动前精确一致。该次只启动1个TUI；preflight --help/session inspect另有2个CLI。8个普通压力文件、每文件<1MiB、总journal<64MiB，无FIFO/symlink/特殊session文件、无provider操作。新旧失败运行均保留原地fixture及JSONL，不改用户数据；网络由sandbox拒绝。

公开入口没有scan-start屏障，因此project.scan_overlap固定UNPROVEN：真实PTY只证明此轮未见跨项目投影，不证明扫描与键盘必然重叠。并发可达性由实际主库Rust屏障承担。

另用组合binary执行既有真实项目入口：12项通过，覆盖顶部鼠标“＋”直接目录选择、Esc/坏路径无空白分页、中文空格路径和别名、同名目录区分、项目模型配置、两目录真实shell写入与session header cwd、运行中切项目、迟到输出归源项目、重启保留选择和草稿、slash选择及终端恢复。该脚本本轮成功路径原本不保存完整raw，保留其确实执行的脚本/argv/hash与assertion结果，不伪称拥有未保存的成功raw。两个TUI进程，操作限制于临时fixture。

上一轮单条回复尾部smoke在本组合binary再运行：14/14通过（2TUI/2loopback HTTP），保留完整raw/http/journals。与12项项目入口合计26项回归，计数不包括68Rust或5个浏览场景。

## 未完成边界

底层文件系统调用仍不可取消，当前scan可拖延最新请求；退出先恢复terminal再join，但进程退出仍可等慢FS。后台返回前仍收集整个原有bounded目录结果后truncate32，并没有优化扫描算法或承诺总耗时/RSS。长期无变化时周期刷新仍发生，性能预算需新release验证。同步嵌入者不享受此异步浏览保证。没有Linux、Windows或异常设备扫描证明。

C正在进行独立语义审查，尚未把其未来结果计入此记录。A另在审查/diff候选；该候选尚未合入。正式清单22/112保持，完整ZS1-126边界、当前源码新release及全阶段验收仍待完成。
