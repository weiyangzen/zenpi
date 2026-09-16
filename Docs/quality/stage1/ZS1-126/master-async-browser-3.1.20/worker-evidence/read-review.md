# ZS1-126 冻结全文件理解记录

对象只有 `before.rs`，对应主库 src/tui.rs，SHA-256 e0e509e507c7c28e4e4ca24709a088c9cedf167c692cf62c3d9bc0ac3658f336，591434 bytes / 14461 lines。2026-09-12 12:08:32.840427 UTC 冻结。连续完整阅读 1–14461；11801–12600 的首次工具输出被截断，已拆成 11801–12200、12201–12600 重读；14001–14461 在上下文压缩后拆成 14001–14250、14251–14461 重读。`full-read.json` 记录最终无缺口覆盖，各块不超过 256 KiB。哈希仅作绑定，以下内容才是语义理解。

- 1–1400：TUI 投影类型、session pane 的内存 envelope 摘要、Gantt generation 与路径归属、消息/工具输出容量和状态；这些投影不是目录扫描 owner。交互状态与 Agent 解耦，不能让 worker 捕获 Agent 锁。
- 1401–2200：输入回执、项目草稿与 TuiState。SessionBrowserRefresh 的 directory/query/last_checked 与列表、cursor 一起按项目暂存恢复。refresh_session_snapshot 从当前 session 内存构建 pane，却调用 refresh_session_browser 引入同步 I/O。不能只移动最外层一次调用，否则项目恢复及 drain 仍可进入同步路径。
- 2201–3800：项目持久化、metadata/cursor/草稿 epoch、输入队列与目标消息归属；暂时切换项目还用于后台 owner 的投影。浏览 worker 以真实 poll 时的可见项目和 session 路径/ID 定位，不能让每次临时 select 都强制重新扫描导致饥饿。
- 3801–5400：提交输入元数据、粘贴折叠、历史/编辑器、普通按键粘贴分类与恢复规则。异步失败必须使用原 SubmittedInput 的元数据，并经 restore_submitted_input 保留更新的草稿，不把旧搜索命令覆盖到新输入上。
- 5401–6746：BentoBox、pane、鼠标/键盘导航和 session 文本渲染；既有 cursor 最多 32 项、路径选择与 viewport 映射是保留边界。没有调整布局或渲染器。
- 6747–7440：同步 refresh 的 500 ms 节流只限制频率，不限制一次 read_dir / inspect / search 的耗时；owner/directory 改变会绕过节流。成功按路径找新 cursor，文件删除则 clamp；失败留旧结果。公开 search helper 同步读、成功才改 query，允许空结果且失败不改过滤。
- 7441–8689：slash dispatcher 的 list 同步刷当前 owner pane，再调用全局 headless lifecycle list 生成回执；search 先搜索 pane，再次 headless 搜索 owner 目录生成含 snippets 的文本。二者范围并不完全相同，性能修复不能悄悄统一。Open/Resume/其它 lifecycle 仍归原 durable owner，非本单范围。
- 8690–10200：ProjectRuntimeHost 的恢复/切换/重载，InputControls 的异步回执，后台输入/项目归属。多个内部路径调用 refresh_session_snapshot；因此真实 host 在 startup 首次刷新前就固定 async 模式，而同步嵌入者默认保持兼容。
- 10201–11000：队列、持久化与 run_async 组装既有 provider/resources/completion/Gantt workers；会话目录扫描与这些工作解耦，单独一个线程，无 Agent mutex 或 terminal 句柄捕获。
- 11001–11800：startup 恢复、terminal guard 和真实 host loop；drain_agent_tool_events 非阻塞 try_lock 之后仍间接触发同步浏览。新的 poll 在同一真实 loop 接收有限结果并提交最新请求；线程初始化失败在 terminal 启动前传播。
- 11801–12600：真实键盘 slash 路由、busy 门禁、项目/SessionNew/Open/Resume 等现有 interception、退出恢复顺序与同步嵌入 callback。新的 List/Search interception 在原 dispatch 之前返回本轮，不能留下一个没有 host 接线的独立工具。退出先 guard.leave，再断开双 mailbox 和 join。
- 12601–13400：同步 callback host、drain、provider 事件和各 pane 投影；同步 callback/public helper 的历史行为保留。真实 async host 的 flag 使所有间接 drain 快照更新仅复制 browser owner，无目录读。
- 13401–14461：recent transcript window 与相关状态/工具/队列/浏览测试。transcript_window 和全部原测试保持逐字不变；新 barrier、generation、四维归属、选择/失败、双目录回执和 worker 关闭测试独立追加。

只读依赖片段：session.rs 2240–2370（list/search：4096 个目录 entry cap、query 非空且 ≤256 bytes、hit limit 32、snippets 上限）；headless.rs 1490–1528、1611–1730（owner search 回执及 ConfigPaths 全局 list 入口）；error.rs 1–90（io::Error 到 ZenpiError）。这些文件只为编译冻结归档和接口片段绑定，不宣称完成其整文件 review，不修改它们。整个 128 文件构建上下文归档；除 TUI 外 127 文件哈希未变。
