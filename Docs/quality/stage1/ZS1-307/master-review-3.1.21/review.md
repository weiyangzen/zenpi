# ZS1-307 主控独立验收与当前目标更正（3.1.21）

本次接受 Codex `codex-rs/tui/src/file_search.rs` 单文件理解，完整 4009 B / 133 行，SHA256 `7aaa33ac7fd28cbe5fa3405cd3490b30e614272122bb572c9c352acc418b71d9`。主控独立顺读 1–133 全文、7 个函数、14 个连续语义单元，完整阅读本轮 27111 B 候选报告和离线验证器；并独立阅读 36 个源/目标上下文片段，补读当前目标和源依赖的 12 个范围。正式源无测试，本文件没有新增 Rust/PTY/HTTP 实验。这里只接受 307，不关闭 123、父目录、底层搜索库或整个目标 TUI。

## 完整语义判断

manager.new 只保留 root/sender 和空状态。update_search_dir 即使 root 相同也丢弃 session 并清 latest；不会立即建立新搜索或增 token。on_user_query 使用精确 String 去重，无 trim、字节限制或本层 debounce，改变后先保存 latest；空 query take/drop，非空按需 create 再 update。每次 create 尝试 wrapping_add token，包括失败；创建失败仅 warn、session None，latest 仍保留，所以相同 query 被提前去重而不重试。

reporter 锁内检查 token、非空 latest 和非空 snapshot.query，但不检查两 query 相等，也不检查 session Some。它在解锁后发出复制的 query/matches，AppEvent 不附独立 root、token 或 generation；matches 的 FileMatch 实际有 root，不能说全部 payload 不带 root。下游 composer 先检查当前 token starts_with(query)，popup 再要求 pending_query == query，正常不同 query 的迟到结果不覆盖当前 pending。旧 root 的同 query 已进入事件队列时没有这个身份保障；本轮未执行调度反例，也没有全文审阅 App 退出/队列重置，因此保留可达时序的未验证边界。

on_update 只转发，on_complete 为空，没有 loading/终态通知。create 的 limit20/threads2/ignore 默认参数、compute_indices=true 已在库 1–235 核对；本层不发 snapshot 的 scanned/total/walk_complete。update_query 通过 unbounded 通道发送，错误忽略；Session Drop 只置 shutdown 并发 Shutdown，不 join；create 启动两个显式 worker，但 Nucleo 内部线程与完整 matcher/walker 不在本次范围。三个 lock.unwrap 仍有 poison panic 边界。AppEventSender 全 28 行另行补读：它记录入站事件，unbounded send 失败只记录错误，不给调用者失败反馈；未展开 session_log 实现，不将其记录动作宣称同步持久化。

源 popup 全 154 行与完整文件选择 handler 已核对：set_query 保留旧 matches，waiting 时旧行仍 enabled，selected_match 只读 path，不检查 waiting/display_query；选择 handler 同样未 gate waiting。因此“迟到结果被过滤”不能推导“等待时不可选择旧缓存”。普通路径插入替换 @token、移除 @，带空白且不含双引号时加引号；图片尝试以所选相对 path 读取尺寸，成功后 attach，失败回退普通插入。Tab/无修饰 Enter 有候选时纯选择，无候选 Enter 可回到普通提交，不能概括为永远两次 Enter。Esc 可能先切 footer 提示，再隐藏并记住 token；popups_disabled 只是 hide/return，不能把所有隐藏等同停止搜索。菜单与选择是 context，不替代 300/301/302/305 文件验收。

主控补充一个候选未明确列出的生命周期事实：完整正式文件没有 FileSearchManager Drop；SearchState.session → FileSearchSession.inner → SessionInner.reporter → TuiSessionReporter.state 形成强 Arc 引用环。补读库 337–363 的 SessionInner 字段后可确认这条持有路径。session.take() 会断开它，但直接丢弃仍持非空 session 的 manager，不能依赖自动 drop 状态来触发 Session Drop。App 是否在所有退出路径主动发送空 query 未全文审核，所以此处是条件明确的结构性保留/清理边界，不声称已经实测进程资源泄漏、线程永不退出或外部安全漏洞。不能将这个源生命周期模式原样移植并宣称自动回收成立。

## 目标现状更正：优先于候选中的旧捕获描述

候选捕获 zenpi TUI 0deb8e0e/638305 B，本次主控当前为 e45131dc/668438 B。旧报告 C06 和随后的“Vec 没有完整性标记、用户无法区分截断空集”准确描述旧捕获，**已经不适用于当前主库**：此前123修复引入 FileCompletionResult.entries/scan/display_limited，区分完整、2048枚举上限、150ms软截止和128展示上限，并展示 partial/空结果原因及缩小路径建议。恰好2048条仍保守判 EntryLimit，因为没有越过上限多读一条确认穷尽。取消在排序前增加检查，仍非原子发布/硬 deadline。current_file_completion 再核对完整 query，slash_choices、选择与详情读取统一使用它，避免新输入读到旧缓存。

主控完整补读当前 562–803、4817–4919、5039–5059、5183–5210、5230–5435：partial/空提示由同一菜单绘制，矮屏先保留一个可选项，再使用剩余行显示原因；空集需可用4行，否则不显示这个信息框。条目枚举仍是单目录、大小写敏感前缀匹配，不提供 Codex 递归 fuzzy 语义；root/path/input/prefix/raw_selection 全等是有效结果条件，但 query 没有 project/session generation。当前按键、runtime completion runner、三个外部测试及 Unicode Esc 重开测试已按新位置补读或逐字节绑定。原3个外部测试在刚完成125主控回归的 command_palette22 项中执行过，来源保持在125记录；本次307不重跑或另计为新的源测试。Unicode单测本次仅阅读，不将过去运行记录改称本次执行。

此前123与当前125改变了返回类型、菜单完整性展示、状态计时和 footer；当前元数据、slash/文件发现范围、附件构造与输入队列约束保留。四种 slash 路径、尾部 @、fold mask、当前 cwd fallback、发现时拒绝父路径/子 symlink、提交阶段 canonical/逐组件校验与原生路径字符串等已读。发现、图片元数据、InputAttachment 构造、后续重新打开/哈希/上传是分开的边界，不将构造器的 metadata 检查当作全部内容无 TOCTOU。in-turn Enqueue/Edit 拒绝附件引用，不从 scheduled/idle 提示推断其完整链已经验收。

## 身份、证据与接受范围

worker frozen manifest 36374 B，SHA256 `e186fe8e23861ef632de296580ac84f1e787a1dfb598f791ba72a5578ec004ab`，154 payload。主控全文静态检查后在新复制包执行一次离线 verify.py：200 静态检查通过，exit0，产品执行0。临时 git 只验证单报告 check/apply、重复拒绝、反向补丁和无关 sentinel 保留。没有运行任何旧脚本、历史 source/Rust/PTY/预算或平台实验。

current-inputs.json 保留全部捕获/current 身份；source及所读 Codex调用者不变，目标TUI/测试与已接受项导致的蓝图/索引漂移单列，未重写旧捕获。current-target-read-bindings.json 记录旧片段的唯一精确匹配，变化片段另行补读；root-reading-ledger.json 记录主控实际范围。本文不把 hash 匹配本身当作阅读。旧307的54文件包与aggregate17文件包原样保留；主控此轮未重读两个历史报告或重新独立验证它们的全部历史运行主张，历史只作留存链。本次正式结论由重新完整阅读源文件与当前上下文建立。

源0测试、200包检查、先前产品22项回归是三种不同证据，不混为验收数量。通过正式 G-FILE/当前 G-STAGE 后仅提升307；L3产品与目录依赖继续分别接受，BentoBox及顶部选目录交互保持现状。回滚只撤本报告、307 receipt与状态，并刷新派生清单，不回滚产品、其它已验收文件或历史证据。
