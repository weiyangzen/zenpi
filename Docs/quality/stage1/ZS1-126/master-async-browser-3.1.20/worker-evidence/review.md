# ZS1-126 / worker A / 3.1.20 candidate

真实 TUI host 的会话浏览刷新和 List/Search 已接入一个有界后台线程。交付仅 src/tui.rs 的 exact patch；没有主库修改或产品 commit，没有声明 ZS1-126 / ZS1-085 验收完成。主任务继续 receiver apply、PTY 和总体 gate。

## Exact input / output

| | Bytes | Lines | SHA-256 |
|---|---:|---:|---|
| before.rs | 591434 | 14461 | e0e509e507c7c28e4e4ca24709a088c9cedf167c692cf62c3d9bc0ac3658f336 |
| after.rs | 621191 | 15176 | 79b486ef11cccfe5b4fd017c6682e30f23ffcfb1fb6b191165e8befcb674c05c |
| candidate.patch | 34363 | 807 | 见 scope.json / manifest.json |

主库冻结时间 2026-09-12 12:08:32.840427 UTC。12:30:37 UTC 的只读接收端观察仍为同一 before hash，见 receiver-readonly.json；这是时点观察，应用时仍须核对。私有 worker HEAD 保持 4dbd330d0a8cf58576109713f774969babd499ed；继承的脏产品文件未被本任务编辑或暂存。

## 可达性、改动及范围

旧真实链路：run_async_with_profile → loop/drain_agent_tool_events → refresh_session_snapshot → refresh_session_browser → list_sessions/search_sessions。500 ms 限制调用频率，不能释放当前被阻塞的输入/绘制线程。旧 Search dispatcher 还再次调用 headless search，同步产生文本回执。

新真实链路：startup 首次 reload 前设置 async_session_browser；所有 snapshot 刷新只复制 directory/path/session ID。真实 loop 调用 SessionBrowserHost.poll，真正的 List/Search 在同步 dispatcher 前由同一 host.route 截获。单 worker 扫描后返回投影；没有 Agent mutex、terminal 或 TuiState 进入扫描线程。默认同步嵌入者及 public search helper 保留兼容，真实 async host 不会到达这两条同步浏览入口。

Host 只持有 1 个 active 请求和 1 个可替换 pending 请求；请求/结果各 sync_channel(1)。query ≤256 bytes，输入回执 ≤256 KiB，结果 ≤32 rows、文本回执使用原 bounded_display（512 chars）。中间过期的 pending 查询直接被最新意图替换。比较 project、session path、session ID、directory、query 和 checked-add generation 后，才允许投影列表、焦点、文字回执或失败输入恢复。

周期更新保留按路径选择；文件删除时 clamp cursor；搜索成功从第 0 行开始并提交过滤；空结果有效；搜索失败留旧 query/rows/cursor；失败输入经原 SubmittedInput 归属恢复且不覆盖更新的草稿。按项目恢复过滤和选择。List 清过滤后异步刷新。所有旧渲染和 transcript_window 逐字保留，见 unchanged-spans.json（特别是 before 12479–14461 的完整相同段）。

**原有范围差异保留：** List pane 扫描当前 owner 目录，而文字回执仍调用原 headless::session_lifecycle_view(List, active path)，由该函数发现全局 ConfigPaths.sessions。Search pane 和文字回执仍均读取 owner 目录。两个读取/回执放入同一后台请求，但保留各自的成功/失败结果，不把性能修复变成范围收缩。为保持原 headless 格式和语义，显式 Search 的两次读取也保留在 worker 内；本单没有修改 headless 或 session owner。

## 验证与失败记录

Native rustc 1.94.0 / aarch64-apple-darwin，独立 target126-session-browser-3.1.20/cargo-target；全部 Cargo 均 --offline --locked。每条完整 argv、cwd、源哈希、起止时间、退出码和原始日志在 *.run.json / *.log；run-source-bindings.json 指向每轮实际源码。不是从后来的绿色结果反推历史结果。

- baseline-tests：1/1。128 个普通长 turn fixture + 1 个 owner 文件，原同步 refresh 14067 μs、search 14784 μs；调用完成前无法回到输入循环。只是本机样本，不作延迟上界或 FPS 声明。
- candidate-build：编译成功，首次插入脚本误在同步 callback host 多建了未使用 worker，产生 unused_mut / unused variable 警告；attempt1-build.rs 保留。已删除该多余创建。没有掩盖此诊断。
- candidate-tests：浏览 9/9（6 个新增，3 个原有），barrier-held drain×20 + key + resize + render 3081 μs。
- final-tui-tests：最终 hash，TUI lib 29/29、0 ignored；barrier 同段 5959 μs。
- final-clippy：最终 hash，`cargo +stable-aarch64-apple-darwin clippy --offline --locked --lib --tests -- -D warnings` 退出 0。vendor/crossterm unix.rs:87 既有 unused_parens 警告仍出现在依赖日志，未修改该依赖。
- 第一次 negative 注入脚本 AssertionError、未改源码（完整失败脚本在 failed-injection-command.py.txt，解释在 negative-injection-failure.json）。shell 随后的 negative-sync-reconnected Cargo 实际在最终候选上通过 1/1；名称不代表有效反例，明确不将其用于反例结论。
- negative-sync-reconnected-actual：移除 ownership-only 分支的 return，真的接回同步扫描，源 c7cb0b14c09815020ce9a80e35f6639b04b26ed9ae776fad2f7610671734ded9。屏障尚未释放却已有列表，`no reply exists before release` 断言失败；0/1、exit 101。随后 worker 的 3 秒超时是测试 panic 展开期间的屏障清理，不是独立产品故障。
- restored-browser-tests：恢复最终 hash 后重新构建，9/9、0 ignored，barrier 同段 3143 μs。最后留下的源码和该 test build 都是最终候选，而非 negative。
- formatting / final-format / final-format-check：退出 0；原 before 整文件已符合相同 rustfmt。格式化中间源也绑定归档。

Barrier 验证的是先后关系：worker 在等待释放时真实 drain/key/resize/render 已执行，而非仅断言某个微秒阈值。102 次查询意图只执行首末 2 个 scan，相同 query 的旧 generation 不投影；另覆盖 project/path/session ID/query 变更、跨项目过滤、删除选中行、失败保留、新草稿、空搜索和 worker panic 后的关闭/回执恢复。

双目录测试为 hermetic fixture：保留真实 owner pane 扫描，在 scanner 注入点用第二个临时目录模拟全局回执数据提供者。它验证两个投影不被 host 合并或截成 owner 范围；并不声称测试了用户的全局配置目录。生产 global headless 调用由 exact 源码和 hash-bound 依赖片段证明保留。

## 接收与边界

`python3 verify.py` 完全离线：核验 manifest、整文件阅读覆盖与不变段、128 文件冻结编译上下文（其它 127 未变）、所有运行源/日志绑定；在新临时 Git 仓库执行正向 apply --check/apply、核对 after、反向 --check/apply、核对 before，并检查无关二进制 sentinel 及干净 diff。application-verification.json 保留第一次执行的逐命令结果。

build-context.tar.gz 只含冻结 Cargo.toml/Cargo.lock/src/vendor 128 个输入，TUI 为最终 after；不是主库当前状态、不是 full-suite 结果。未做 Linux、真实 PTY、HTTP、网络测试，也没有扩充/删除忽略项。两次目录读取虽不再阻塞交互，但仍可能很慢；单线程会让最新请求等待当前扫描结束。底层文件系统调用不可取消；正常退出先恢复 terminal，再断开 mailbox 并 join，极慢调用仍可能拖延进程退出。跨模块 cancellation/目录算法优化不属于本单。
