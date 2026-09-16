# ZS1-126 / C / 3.1.20 独立审查

对冻结的当前集成版本进行审查和 8 项独立测试，没有复现需要修改产品代码的缺陷。本包仅交付审查、测试和复核材料，不提供产品补丁，不表示整个 target 验收完成。

## 源码与审查范围

输入为主库 src/tui.rs，621728 bytes，SHA-256 `994215c7ca6cc471bb35247a04460d6c5ef52192999e4b5585ea1c818a429578`。128 个 Cargo.toml/Cargo.lock/src/vendor 编译输入冻结在 context/，其余 127 个文件与捕获输入相同。私有 TUI 仅追加 cfg(test) 模块入口，编译源码 hash `389be613eaecb96eee489d044342191751e6ea40b63b3e3ab5594943082fe1e5`；独立测试位于 context/independent/session_browser_review.rs。

A 原包完整保留 54 个普通文件，manifest SHA-256 `6519e3b93606376da3e259b54e8c46ffff67d19f373a973a28fc60575a2727ce`。A-to-integrated.diff 显示其 after 与当前冻结文件的差别仅在 transcript tail 渲染适配；本次未改变该部分或 BentoBox、project plus、扫描策略。

本次逐函数审查了 SessionBrowserRefresh/Scope/Key/Request/Projection/Host 和 scan_session_browser，关联 SubmittedInput 恢复、项目草稿保存/恢复、refresh_session_snapshot/browser、同步 List/Search 兼容入口，以及 run_async 的创建、首次刷新前启用、项目意图后的 poll、同步 dispatcher 前的 route、错误返回和退出顺序。read-bindings.json 将所读区间绑定到精确源码。run_async 只读相关调用与错误/关闭路径；不宣称逐行阅读整个 TUI 或全部依赖。A 自己的整文件阅读和历史测试原样保留，未计为 C 的新证据。

## 关键判断

- 一个扫描线程，命令和结果各容量 1；host 只持有一个 active 和一个可替换 pending。新意图分配 checked-add generation；同 query 的旧请求也不能覆盖新请求。
- 提交投影之前同时检查 desired 完整 key 和当前 project/path/session ID/directory。可见项目切换由真实 host 在处理 project intent 后 poll 观察；内部暂时切换项目再恢复用于投影后台结果，不应按每次 select 强制失效。
- 失败回执只恢复匹配归属的 SubmittedInput，且既有恢复规则保护非空新输入、尚在分类窗口的普通粘贴字符和 rejected 状态。此结论不扩展为所有编辑历史的 epoch 保证。
- rows 与文本 receipt 分开表示成功/失败。两次读取之间数据可能变化；不能把一侧失败概括为两侧都保持旧状态。双失败测试验证保留旧列表/过滤及新草稿。
- 生产 pane 用 owner directory；List receipt 仍走 headless::session_lifecycle_view 的 ConfigPaths.sessions；Search 两次读取均用 owner directory。读取的 session/headless 源码片段支持该范围判断。没有重写为统一目录。
- async 标志在首次 reload/snapshot 前启用，snapshot 只复制归属。List/Search 在同步 dispatcher 前截获；正常循环结束及循环 I/O 错误走 guard.leave 后 Drop。启动失败时还未派发浏览扫描，局部析构断开线程通道。Drop 先断开双通道再 join。

## 新执行结果

原生 stable-aarch64-apple-darwin，Cargo --offline --locked，独立 CARGO_TARGET_DIR 和任务内 TMPDIR。首次与最终运行均为 8 passed / 0 failed / 0 ignored；最终日志 `commands/independent-tests-final.log` SHA-256 `895a435f3761cc050d452073a8a13719ca65020e3c9115486c9a07258aeeaf7e`。这是 8 个独立用例执行两轮，不能算 16 个用例。首轮有测试名称 non_snake_case 警告，保留当轮测试源和日志；最终只格式化测试并将 old_A 改名 old_a。最终 rustfmt --check 退出 0。vendor/crossterm 既有 unused_parens 警告仍保留。

| 独立用例 | 断言范围 |
| --- | --- |
| same_query_generation_and_latest_pending | 首次请求屏障阻塞时连续替换 pending，回到同 query；旧失败不写回，只有最新 generation 执行和生效 |
| observed_project_roundtrip | A→B→A 每次可见切换后 poll，保留各自草稿、过滤及选择，旧 A 失败被丢弃 |
| same_directory_path_or_session_id_change | 同目录分别注入 path 或 session ID 变化，旧结果不投影；这是 key 隔离测试，不是完整 resume 流程 |
| active_failure_preserves_new_draft | 当前请求双失败时保留新输入或尚未完成分类的单字符草稿，以及旧列表/过滤 |
| worker_closure | 注入扫描线程 panic，恢复最新 pending 输入，拒绝后续命令并保持新草稿；日志 panic 为预期故障注入，测试通过 |
| no_owner_overflow_and_periodic | 无 owner、generation=u64::MAX 不派发扫描；刷新间隔内不重复派发，过期后可刷新 |
| unread_reply_drop | 不消费结果就 Drop，有限等待内可断开通道并 join；不验证底层 I/O 取消 |
| local_pane_and_global_receipt | 两个临时目录的真实 SessionStore/list_sessions，经 scanner 注入独立 receipt，host 不混淆二者 |

测试用同步屏障和 2/3 秒失败截止控制先后关系；没有 FIFO、PTY、HTTP 或网络探测，没有读取用户全局会话目录。双目录用例未执行真实 ConfigPaths 发现，全局生产调用仅由源码核实。没有重新运行 A 的历史用例、主任务报告的 68 项 Rust/Clippy/fmt 或真实 browser/termios；不将这些数字标为本次通过。

首次包完整性验证退出 1：阅读记录末尾误写为 15195，但精确冻结文件实际为 15190 行；验证器的范围边界断言正确拒绝。已将记录修正为 14837–15190，没有修改源码或测试。首次 manifest、错误阅读记录及失败日志保留于 failed-first-package/ 和 commands/frozen-package-verify.*；整个首次包也原样留在同级 ready/。本最终包为 ready-final/，失败不计为产品测试失败，不能隐去或混用其 manifest。

## 限制与保全

单 worker 的当前扫描仍会让最新 pending 等待。底层文件系统调用不可取消，可能在 terminal 恢复后延迟进程退出；本次不作退出时延、吞吐或恶意文件系统安全的整体保证。没有 Linux、真实终端或全量 target 验收。

preservation-after.json 记录本次时点验证：78 个既有 ready 包的 11370 个普通文件、145 个已跟踪普通文件、A 原包 54 文件、主库 TUI 均逐字节一致。工作区产品文件未编辑、未提交、未暂存；所有新增材料只在本任务 .ops/stage126-independent320 下。

复核包完整性：`PYTHONDONTWRITEBYTECODE=1 python3 verify.py`。复跑测试时，在包外新建专用 TMPDIR 和 CARGO_TARGET_DIR，再执行 `cargo +stable-aarch64-apple-darwin test --manifest-path context/Cargo.toml --offline --locked --lib independent_session_browser_review -- --test-threads=1 --nocapture`。依赖缓存需已具备；verify.py 本身不执行 Cargo、终端或网络操作。commands 的运行 receipt 保留原始绝对路径，run-source-bindings.json 补充实际 TMPDIR/target 与每轮测试源码身份。
