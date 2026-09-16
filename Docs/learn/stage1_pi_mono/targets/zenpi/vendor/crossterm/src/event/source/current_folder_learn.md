# ZS1-401 — vendor/crossterm/src/event/source 独立目录理解候选

本报告唯一正式路径是 `Docs/learn/stage1_pi_mono/targets/zenpi/vendor/crossterm/src/event/source/current_folder_learn.md`。只解释该目录冻结子集及必要跨层连接，understand 模式，产品实现 LOC 为 0。对子目录的语义复用与对子目录的正式接受是两件事：开始研究时 ZS1-400 蓝图仍为 [ ]、master receipt 不存在，已保存 initial-authority 原始字节和缺失事实；因此研究及候选准备不等于 G-DIR 已通过。最终依赖状态见末尾封包时点，以实际凭据为准。

权威是主库 `Docs/stage_1_v3_pi_mono_blueprint.md` 3.1.21 及 `Docs/execution/active_requirement.json`。requirement digest 为 `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`；冻结 baseline 为 `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`。蓝图 ZS1-401 只 Depends ZS1-400，G-DIR 要求冻结直属文件和直接子目录先各自 [x]，再独立审阅当前目录的调用、数据、错误、取消和持久化连接。库存、哈希或检查计数不代替该语义门禁。

## 逐项物理库存与冻结范围

本轮直接枚举 source 目录，包含隐藏项，不递归将孙项算成直属项。物理内容恰好两个普通文件和一个普通目录，均非符号链接，无其它隐藏项、设备项或额外子目录。父层 `event/source.rs` 在本目录外；`sys/windows/` 也在本目录外，不能因 Rust 模块接线相近就改变物理边界。

| 直属项 | 字节与 SHA256 / 角色 | 本次处理 |
|---|---|---|
| unix.rs | 294 B，11 行，`253ed074fbe63e7270fc19e5c85186407c3a91454f43291c29cf5717575c74a6`；context-only | 完整复用 400 的已读字节，重新确认当前完全一致；它是子目录实现的编译选择器，没有独立冻结文件项 |
| windows.rs | 3656 B，100 行，`e25bdd13432dfda96401a76f3e2d6aa2a404fbaa4f1b99b55bb0bdec2ac75336`；context-only | 本轮从首行到 EOF 全读，保留原字节，不创建 Windows 文件验收 |
| unix/ | 唯一物理直接子目录、唯一冻结直接依赖 ZS1-400 | 复用 400 候选和 099 已接受链；400 是否正式接受须另有实际主控凭据 |

file index 在本目录的冻结直属文件数为 0；folder index 直接子项仅 400。unix/ 内 mio.rs 与 tty.rs 是孙级，前者是 ZS1-099，后者仍为 context，不提升为 401 的直属文件，也不重复接受 099。路径闭包是 099→400→401→402(event)→403(src)→404(crossterm)→405(vendor)→091(root)；本包只生成 401 报告，不跳过或合并父层目录，406/tests 不属于此分支。

## 阅读来源和可复用的边界

400 原 ready manifest 为 `805ac1beeab2d643a549a411574985d57db0757fd0a1be1dbef075390125e61d`，其候选报告 20939 B / 124 行 / SHA `f004a5b0b5095af3b70801601352129012b4b1d5cac501a7fff642ef616af84f`。本包完整复制并校验该不可变包，旧包没有回写。本轮将当前 15 份既有完整 context 与该包逐字比对，全部相等，因此按相同完整 [0,EOF) 范围复用：source.rs、unix.rs、tty.rs、read.rs、filter.rs、timeout.rs、stream.rs、sys.rs、sys/unix.rs、Unix waker 选择器与两个实现、file_descriptor.rs、根与 vendor Cargo.toml。复用不是声称本轮重新全读所有不变文件，也不依赖 400 已被接受才承认此前真实读过的字节。

mio 当前仍为 25801 B / 655 行 / `41321e242e21fa85533dbae42314bcf80ab916e1d55e065d5ce98076002ac93f`，与 400 物理快照、099 current source 精确相等。099 的 57f4 原始 229 行、1afa 477 行与追加 178 行组成六段连续读取链；冻结 099 source_hash 仍是原始 57f4，正式接受由 prior400/accepted099 的 master receipt 及 canonical 报告绑定。本包不把当前实现 hash 填进冻结 source_hash，也不把语义复用写成又接受了文件。

本轮新读五份完整 Windows context：source/windows.rs 100 行、sys/windows.rs 48 行、sys/windows/poll.rs 86 行、sys/windows/waker.rs 40 行、sys/windows/parse.rs 378 行。最后一份分两段连续读 1–205、206–378，无遗漏；每份原始 CRLF 字节、[0,EOF) 范围和 SHA 都在 fresh-context-read.json。需要读这些文件，是因为 windows.rs 的事件消耗、错误及 wake 返回约定由它们共同决定；本轮没有运行 Windows 构建或测试，也没有深入 crossterm_winapi 的所有外部依赖实现。有关 Console/Handle/Semaphore 内部关闭细节不作超出已读代码的承诺。公共 event.rs、Unix parse 深层语法及 TUI/reset 接线按 099 已接受的限定合同复用，不声称完整新读那些大文件。

## 编译选择和目录对外合同

`event/source.rs` 按目标平台声明 unix 或 windows 模块，定义 `EventSource: Sync + Send`，核心方法 `try_read(&mut self, Option<Duration>) -> io::Result<Option<InternalEvent>>`；event-stream 才增加 waker 方法。Sync+Send 是类型约束，实际读取仍需要可变引用或父 reader 的同步所有权，不代表多个 source 可以无协调地同时消费同一终端。

| 配置条件 | source 路线 | 对应通知及已知限制 |
|---|---|---|
| Unix + events，未 use-dev-tty | source/unix.rs 导出 unix/mio.rs 的 UnixInternalEventSource | event-stream 时 sys/unix/waker.rs 同条件选择 mio Waker；当前生产默认路线 |
| Unix + events + use-dev-tty | 同一导出名改为 unix/tty.rs | waker 同时改为 UnixStream 写端；编译时互斥选择，不是 mio 初始化失败后的运行时回退 |
| Windows + events | source/windows.rs 的 WindowsEventSource | sys.rs 选择 windows Waker；其内部持有 WinApiPoll/Semaphore，不读取 Unix 字节流 |
| event-stream 开关 | 增删 trait waker、后端唤醒资源及 async 上层 | 不自动选择 use-dev-tty；当前 Unix 默认产品测试不覆盖 Windows stream |
| libc 开关 | Unix fd/系统调用适配 | 不决定 mio/tty 选择，根直接依赖 libc 不等于启用 crossterm 的 libc feature |

根 Cargo 使用 crossterm 0.29、bracketed-paste 且保留默认 features，patch 到本地 vendor。vendor 默认含 events/windows/derive-more/bracketed-paste，events 启用 mio/signal-hook 相关依赖；use-dev-tty 增 filedescriptor/rustix process，event-stream 增 futures-core。Cargo feature 名 windows 与 `cfg(windows)` 目标条件不同：在 macOS 默认 features 含 windows，不意味着 Windows source 被执行。未启 events、其它平台和 feature 全组合不在本轮验证范围。

父 `read.rs::Default` 根据目标平台构造一个 source，将 `Result` 经 `.ok().map(Box)` 存为 `Option<Box<dyn EventSource>>`。具体构造错误因此可能丢失；后续 poll 在 source=None 时只返回通用初始化错误，而 waker 会 expect panic。Unix/Windows 或 mio/tty 之间没有自动重试、合并队列或动态切换。目录只产出一个统一 InternalEvent；公共过滤、全局 reader 锁和 TUI 行为属于父层 owner。

## 所有权和数据流：统一接口并不意味着相同后端

Unix 子目录的瞬态 owner 已在 400 分解：mio 持 Poll/Events、pending_tokens、tty_read_started、Parser 的 partial 与已完成队列、1024 字节输入缓冲、FileDesc、Signals、可选 Waker。先消费已解码事件再续同批 readiness，pending 空才 poll 新批。TTY 已开始读取后用 FIONREAD 避免 blocking fd 排空后的再次阻塞；正常退 TTY 时仅尝试完成确认只有一个 ESC 的 pending 序列。后续 SIGNAL/WAKE token 不因前一个事件返回而消失。目录接线必须维持这个对象跨多次 reader.poll/read 存活，不能每读一个事件重建 source 或在正常 Resize 时 reset。

Windows source 的状态完全不同：Console、WinApiPoll、一个 Option<u16> surrogate_buffer 和三个布尔 mouse_buttons_pressed。new 首先获取当前输入 Handle 构造 Console，event-stream 开启时创建 poll/waker 也可能失败；它不持 Unix Parser、byte buffer、pending token 队列或 SIGWINCH。OS console 记录留在系统队列，source 每轮先等待，再查询记录数，非零且 event_ready 才读取一个 InputRecord；能转换成 Event 即返回，否则继续在同一 PollTimeout 剩余预算内等待。没有把 ignored record 退回 OS 的逻辑。

| Windows 记录 | 转换与内部状态 | 父层可见结果 |
|---|---|---|
| KeyEvent | handle_key_event 可暂存一个 UTF-16 surrogate；正常可转换键会清旧 surrogate | 转换成功仅返回 InternalEvent::Event(Key)，半对或无映射为 None 后继续 |
| MouseEvent | 使用前一次按钮状态推导变化，随后无论解析是否产出事件都更新当前三按钮 | 屏幕坐标获取失败在 handle_mouse_event 中被折为 None，非所有错误都成为公开 io Err |
| WindowBufferSizeEvent | x/y 各按源码转 i32 加 1 再转 u16 | Resize；本目录未验证所有输入范围或真实 Windows 尺寸正确性 |
| FocusEvent | set_focus 决定 Gained/Lost | Focus 事件 |
| 其它 InputRecord | 匹配兜底 None | 已读记录被消耗，不生成事件，不持久化 |

parse.rs 的完整阅读澄清 owner 边界。surrogate 首个值保存，下一值清缓存并 decode_utf16，非法组合返回 None；未映射记录不会执行正常 Key 分支的清缓存，鼠标/焦点/Resize 也不清这个字段。有效非 surrogate 键到来才明确清理旧值，不能宣称每个无关输入都重置 Unicode 状态。surrogate 合成通过 KeyEvent::new，普通键则用 new_with_kind 按 key_down 标 Press/Release；顶部旧注释称通常忽略 release，与实际末尾生成 Release 的代码不完全一致，报告以代码为准。Alt code/numpad、特殊虚拟键和控制字符键盘布局查询分别处理，本实现没有展开 repeat_count 为多个逻辑事件。

键盘布局查询是 best effort：按当前前台线程取布局，可能晚于实际按键发生时间；ToUnicodeEx 无映射、dead key、非法 Unicode 或多字符结果均可成为 None。`[0u16, 16]` 在源码是两个元素的数组字面量，并不是 `[0u16; 16]`；本轮不修正该上下文代码或声称 Unicode 全范围已验证。大小写变换只接受一对一字符转换。mouse 解析先取屏幕窗口 top 将 y 转相对位置，可失败；多按钮变化按 left/right/middle 的分支优先序只返回首个匹配变化，移动、拖拽和滚轮另处理。因此接口统一的是事件类型，非原始 Windows 记录一对一无损映射，也不能把 Unix bracketed-paste/UTF-8 边界测试转为该路径的证明。

父 reader 另有 events VecDeque 和 skipped_events，source 不拥有 Filter、project/session ID 或 UI 草稿。poll 先找队列已有匹配项，再向选定 source 请求剩余时间内的一项；不匹配项暂存，匹配/正常超时才恢复，匹配项放前便于随后 read。Windows 的 EventFilter 全 true；Unix 查询响应有独立 filter。poll Interrupted 直接 false、其它 Err 直接传播都可能先于 skipped_events drain；read 的局部暂存不匹配队列遇后续 poll Err 会随栈退出丢弃。不能用正常过滤顺序保证所有错误路径也无事件损失。源码 FakeSource 测试只覆盖所写场景，其 waker 未实现，不构成真实 OS 取消证明。

## EOF、超时、错误和 wake 的跨层契约

| 边界 | 实际行为 | 不能推导的更强承诺 |
|---|---|---|
| mio 字节读取 | 正常排空/WouldBlock 退当前 TTY；EOF 为 UnexpectedEof，其它 read/ioctl/poll 错误按分支传播 | 不表示所有平台 EOF 都这样编码，也不清空全部保留事件以掩盖失败 |
| tty context | read_complete 首个成功 read 即返回，只重试 Interrupted，WouldBlock 转 0；0 同 EOF 进入 break | 没有与 mio 相同的 EOF 区分和 FIONREAD/pending/idle ESC 保障；输入 tty 不因通知 pipe 非阻塞而成为非阻塞 |
| Windows console | poll/查询数量/read_single 的错误经 ? 返回；记录数为 0 只继续等待/检查超时 | 没有实现 Unix read(0) 意义的 EOF，不将空 console queue 说成输入永久关闭 |
| PollTimeout | Instant 相对时限，None 无限，剩余值到期钳为零 | 并非外部取消 token，不强制打断每个库调用或 terminal::size 的慢路径 |
| 零超时 | mio 优先已有 Parser，tty while 先检查余量因而可能不取已有 Parser；Windows 至少先调用一次 poll | 接口注释不消除后端次序差异，无法推成全配置完全等价 |
| wake | Unix mio/tty 和 Windows semaphore 都可令 source 返回 Interrupted | trait 注释称 force Ok(None)，实际 reader 将 Interrupted 转 Ok(false) 才实现上层无事件语义 |

Windows WinApiPoll 每次重新取得当前输入 Handle，再 WaitForMultipleObjects：非 stream 只等 console，stream 还等克隆 semaphore handle；console 索引结果为 Some(true)，wake 索引先尝试 reset semaphore（忽略 reset 错误），再返回 Interrupted。WAIT_TIMEOUT 和 WAIT_ABANDONED_0 在这里均映射 None，WAIT_FAILED 返回 last_os_error，其它码为 Other。poll 实际没有产出 Some(false) 的分支。它保留 source 构造时的 Console，但 poll 再取当前 Handle，若外部更换输入句柄，二者一致性没有由本文件重新绑定保证；本轮没有模拟该环境变化。

Windows timeout 将 duration.as_millis() 强转 u32：亚毫秒会截断、极大 Duration 有窄化风险，None 传 INFINITE。这里未做饱和转换，不宣称任意时长都严格等价。console 返回记录还需转换才可交付，持续不可转换记录会消耗预算；一次 read/解析/外部 WinAPI 调用的耗时不被 PollTimeout 本身抢占。共享 console 记录队列若有其它消费者，查询 count 与 read 之间并无本目录事务锁，不能推导多读者绝不阻塞或记录不竞争。

## 释放、取消和持久化 owner

本目录没有自定义线程 owner、磁盘格式或会话事务。Unix source 的 fd 借用/拥有由 FileDesc 区分：isatty stdin 借用，/dev/tty 通常拥有；Drop 不应关闭借用 stdin，不承担 raw mode 恢复或 OS 输入 flush。tty 的 signal 注册 ID 未保存、代码注释依赖 singleton，没有本地显式 unregister；不能继承 mio Signals 的清理保证。Windows source 未实现自定义 Drop，字段会随对象释放，但 Console/Handle/Semaphore 的底层 CloseHandle 细节属于未全读的外部依赖，报告不伪造关闭次数或并发安全证明。

Windows Waker 是 Arc<Mutex<Semaphore>>，wake 调 release，reset 用新 Semaphore 替换旧值，semaphore() 在锁内克隆后供 poll 使用。clone 可延长资源存活，mutex poison 的 unwrap 会 panic；reset 创建失败则保留旧值，但 poll 忽略其错误。Unix mio Waker 同样是 Arc/Mutex 包装，tty 是 UnixStream 写端且写失败可返回 Err。它们是唤醒输入等待的机制，不是项目 shutdown、杀掉 provider、停止任意 Rust 代码或可靠提交 journal 的机制。

父 EventStream 完整读取合同仍精确复用：容量 1 task channel、executed/shutdown 原子标志、后台 poll_internal(None) 和执行器 waker。poll_next 先零超时尝试可用事件，必要时只提交一个等待任务；Drop 置 shutdown 并忽略底层 wake 错误，没有保存 JoinHandle 或 join。不能因 stream 注释说线程会退出，就说 Drop 返回时已同步等待结束；后台 poll 错误在循环中的处理也不能提升为全部取消错误会及时交给调用者。此 async 路线不是当前默认 Unix 同步 PTY 证据。

公共 event.rs 的 reader mutex、poll/read 与 reset 约束通过已接受 099 合同复用：正常 Resize 只推动布局，不重建 reader；外部 editor 交接先停读者，再按约定清内部预读/partial，reset 不 flush 内核输入。Windows sys/windows.rs 的鼠标 capture 模式保存/恢复是另一组显式函数，用 AtomicU64 保存首次模式，不由 source Drop 自动调用。source 状态销毁意味着 surrogate、partial、按钮历史和预读状态结束，并不写入草稿或保证下次重建可以恢复。

TUI 的编辑、kill/yank、选中项目、提交 HTTP，以及 headless 的 transport/runtime/Agent shutdown 各有其它 owner。本目录只把终端输入转成事件；没有 session ID、WAL/JSONL writer、close hook、project resolver 或持久化恢复协议。本报告的回滚仅撤回自身文档，不递归回滚子项、产品或任何用户会话。

## 行为证据的版本与新旧时点

099 和原 400 保存的证据保持原样：57f4 的 >1024 读入停顿负例、1afa 的 socket 1023x+ESC 滞留与修复后通过；公共 event::poll/read 的单 ESC PTY 前后本来都通过，缺少每次 read trace，不能改写为生产 PTY 原先失败。旧 headless944049+mio41321e 集成的 144 Rust、vendor 8/9、Clippy/fmt 是那个版本的记录，8/9 重叠，旧 400 封包时没有新 release/budget 的知识也不倒写。

本轮另已完整阅读主控新公开 `Docs/quality/stage1/ZS1-132/master-shutdown-release-3.1.21/review.md`，并提取/绑定 master-verification 与现有 run/报告证据。该公开验证绑定的 headless 为 `31ea4fcc152f4c9bc067b721a0aea18dbbec2eb3b1cbc9a5081fc462d70bc932`，mio 仍 41321e；主控此次 Rust 是 68，不能将旧 144 改称 31ea 的运行。新生产 release 6073392 B / `6891df5d13a373eff656f8c99a5b5367f9ddb4e502f159d0d925162ec6bccbc4`，原预算 8 门通过，3 次冷启动 464.950750 / 42.529875 / 41.345167 ms，同一二进制项目 PTY 12 与 kill/yank 29（3 TUI、4 HTTP）通过。这是主控在新 source 集合上的已记录结果，本轮没有执行产品/预算/PTY。只封存相关公开证据选集及其原 manifest，未声称复制或重新审阅全部大型 integration archive。

新 132 review 对非合作 job 的 deferred cleanup、进程立即退出可截断清理、ordinary EOF 仍 drain、并发新 journal writer 等限制均保留；这些结果不把输入 Waker 变成 Agent 清理保证，不接受全 084/132/129/131 或本目录。主控记录的无关 Gantt generator 漂移不改变 source 语义，不因该文档变化重封旧 400/099，也不重新跑 runtime。当前目录相关身份检查限于物理 source、必要 context、依赖接受凭据与已引用的产品身份。封包时发现主库 headless 已变为 368974 B / `521c2102519b502fbd6184f31ffb53cba8eeef840bf84d19450836429d9b49f0`，首次严格产品身份断言因此中止（保留未封包目录和记录）；mio、tty 和20份新读/复用context仍相同。调整为显式记录当前与公开身份差异，没有将6891二进制与其运行结果重新绑定521c，也没有重跑产品或声称审阅该无关headless增量。

## 候选门禁与停止点

跨文件不变量是：平台/source/waker 编译条件成对；父 reader 持有唯一选定 source，创建失败无后端 fallback；source 保留自身瞬态状态，reader 保留过滤队列；EOF/None/Interrupted 在底层不同，在父层按明确分支转换；Drop 不自动恢复 raw mode、完成 thread join 或提交项目持久化。Windows 的记录转换和 Unix 字节解析只在 InternalEvent 类型层汇合，不能互相借用测试结论。

即使 400 后续接受，也只满足 401 的子项前提，401 自身仍需主控独立语义审阅。本包 accepted=false，G-STAGE 本轮未运行，没有 master receipt、没有 402–405/root 接受。新离线 verifier 只检查库存、0→EOF 读链、精确字节复用、实际依赖状态、选集 artifact hash 与唯一报告 patch；校验成功不能代替目录理解或产品验收。

全部写入仅在新私有 .ops，主库/旧 ready/authority 只读。worker HEAD 与原 94 条 non-.ops dirty 保持。候选封包后停止，不开新任务/subagent，不执行旧脚本/Cargo/runtime/PTY/budget；若主控审阅时相关 source 或依赖凭据已变化，应按实际差异处理，而非利用旧证据直接盖章。

## 封包时点及完整读取索引

封包检查 2026-09-12T17:23:26.187764+00:00：400 仍 [ ]，master receipt 不存在，401 仍 [ ]；selector snapshot `2bb12c5f41ba8b290fb0a6f74cac1a98bab8faa095775f2463991e58003b246d`。本包明确是依赖未闭合的候选，不宣称 G-DIR 已满足。正式 401 报告尚不存在，当前相关 source/context 精确匹配本次读取证据，公开结果仍只绑定 headless31ea 与 mio41321e；当前 headless 的变化见 related-identity-drift.json，本轮不将旧测试改绑。若 400 在封包后接受，须由主控另附当时接受证据，本包历史状态保持。

|读取方式|文件|行数|完整字节半开范围|SHA256|
|---|---|---|---|---|
|新读|vendor/crossterm/src/event/source/windows.rs|100|[0,3656)|`e25bdd13432dfda96401a76f3e2d6aa2a404fbaa4f1b99b55bb0bdec2ac75336`|
|新读|vendor/crossterm/src/event/sys/windows.rs|48|[0,1707)|`130d7da62a0af8b1cf7c55bd8bf78fa0272a9a44803704dac6416db993d30d81`|
|新读|vendor/crossterm/src/event/sys/windows/poll.rs|86|[0,2545)|`232c69f9c9aa058189a418cdaee82134feaf6ef91f89cea8eaaeb5b4cfd722fc`|
|新读|vendor/crossterm/src/event/sys/windows/waker.rs|40|[0,1156)|`a145d8ccf6b7747eb4e9499bdade2616a997539da4f4691f4c14bd4c4d6a472b`|
|新读|vendor/crossterm/src/event/sys/windows/parse.rs|378|[0,15566)|`15afbd3b84edfa2a72f9587f591ce9f7dfe99f1cb85db8b45037c0b70c0346ae`|
|精确复用|vendor/crossterm/src/event/source/unix/tty.rs|278|[0,10162)|`453e854ca82cd2d37e57494d9766a9eea728883a8a1c5fec186f7c6b83dbd64e`|
|精确复用|vendor/crossterm/src/event/source.rs|27|[0,925)|`f606a124266b6d86b58969348bb713f7f89fb09886f51bfb6fe16b766e113bb7`|
|精确复用|vendor/crossterm/src/event/source/unix.rs|11|[0,294)|`253ed074fbe63e7270fc19e5c85186407c3a91454f43291c29cf5717575c74a6`|
|精确复用|vendor/crossterm/src/event/sys.rs|9|[0,248)|`3ec336b2d4c36cfc895fc7cf586e92f7763473130ca99e8168bead1ba3918d1a`|
|精确复用|vendor/crossterm/src/event/sys/unix.rs|5|[0,110)|`d651928295213cd12a4df7edeb0c8be5a4e4487345fc9f2c2ff59d6411047cf2`|
|精确复用|vendor/crossterm/src/event/sys/unix/waker.rs|11|[0,258)|`fe7a1e77718306e9f84b77337f4e9ea9990bed4cf533036228508316363add0e`|
|精确复用|vendor/crossterm/src/event/sys/unix/waker/tty.rs|28|[0,685)|`49132cd71d20b7cc27b79d28a490743da77241bed24a178e22d9ad6e5e98e185`|
|精确复用|vendor/crossterm/src/event/sys/unix/waker/mio.rs|34|[0,1026)|`cbc89c146a004ad689db7f17e3cfc9ce6198c115f31e519d25c68a441db3ecb4`|
|精确复用|vendor/crossterm/src/event/read.rs|436|[0,14606)|`704eb054bff7408e7591e7b5869cf4606794a43b69dc7a1edef67fa7e26ccb8a`|
|精确复用|vendor/crossterm/src/event/timeout.rs|92|[0,2662)|`cbe518ad2cb666b588f593b4b84704eb4480e49d9bd3f28c72d696dae6e7b5e7`|
|精确复用|vendor/crossterm/src/terminal/sys/file_descriptor.rs|154|[0,4249)|`d215ff60394ec2150c2df6340343d84a3922570c1feba813663742d0eb85d13f`|
|精确复用|vendor/crossterm/src/event/filter.rs|115|[0,3786)|`092515c0939799af80768e7b87d2ea4ffc5b1bcda021982f6f02922378809df5`|
|精确复用|vendor/crossterm/src/event/stream.rs|146|[0,5092)|`1df7f35a1842d54b6a8d2b82c96661305d7bd87bbdf123ce6670c62fec5f3cb1`|
|精确复用|Cargo.toml|50|[0,1228)|`d5fafd029ef77454e178222c2b62530d28859374f72a2ce6bd972a465bed5884`|
|精确复用|vendor/crossterm/Cargo.toml|240|[0,4455)|`78b5e8b5cd6660067195979533e61d4b33c375c74613f24180efd94c86f7d295`|


## Controller independent directory acceptance — 3.1.21

The controller independently read this complete23446-byte report and its full portable verifier, then executed the verifier once in a fresh copied package:381 payloads verified without historical program execution. It intentionally verifies the frozen candidate's missing400 receipt; this historical result is retained. Since that capture, the controller has separately accepted400, with a real master receipt, full artifact chain and successful G-STAGE. Those current artifacts are independently rechecked and bound here before401 promotion. The earlier provisional state is not rewritten in the immutable worker package.

Physical source/ contains exactly unix.rs, windows.rs and unix/. No symlinks or additional entries exist. There are zero frozen direct files and one direct child directory400;099 is a grandchild and is not accepted again. For this review the controller newly read all100 lines of Windows source,48 lines of sys/windows.rs,86 of poll.rs,40 of waker.rs and378 of parse.rs in continuous1–205/206–378 ranges. All20 fresh/reused context identities were checked against main. The other15 complete context files were fully read by the controller immediately for400 and are now reused byte-for-byte; accepted099 retains its own complete baseline/current reading chain. This is an independent directory review, not another claim of full Windows file acceptance.

The code confirms that Windows owns a Console created from an input Handle, WinApiPoll, one pending UTF16 surrogate and mouse-button history; Unix owns its separate parser/readiness state through400. Ignored console records are consumed. Mouse parsing errors become None while button state still updates; unrelated non-key records do not clear the pending surrogate. Normal keys carry Press/Release, but surrogate completion uses KeyEvent::new. The old release-filter comment is weaker than the actual code. Best-effort keyboard layout lookup can differ from the layout at input time; its [0u16,16] array has two elements. Simultaneous mouse button transitions follow branch priority. These are documented static limits, not newly tested Windows failures.

WinApiPoll reacquires the current input Handle each call while Console retains its original construction identity. Count/read are separate operations, not a multi-reader transaction. Duration milliseconds narrow to u32, None means INFINITE, wake returns Interrupted after ignoring a semaphore-reset error, and parent reader maps Interrupted to false. Windows empty queue is not Unix EOF. Semaphore clones can extend resource lifetime; reset assignment preserves the prior value if creation fails. External WinAPI wrappers were not fully reviewed, so no exact CloseHandle or cross-platform cancellation guarantee is inferred. Source Drop neither restores mouse/raw mode nor joins EventStream's thread or persists a project draft. Compile-time source and waker selection, parent filtering ownership and the absence of fallback were independently checked in400 and reused here.

The report's earlier6891 production and144/68 Rust histories remain bound to their original snapshots. Current main521c+41321e has a separately archived92f95 release:80 Rust checks,55 actual CLI checks and8 original budgets pass in ZS1-132/master-explicit-version-3.1.21. These newer local product results are not needed to prove this directory's unchanged bytes and do not establish Windows behavior. No product, build, PTY, budget or archived runner was executed by this acceptance; no further context file is promoted.

Accept only401 directory understanding after the independently accepted400 dependency.402 and all other ancestors, complete084/117/129/131/132 and whole Stage1 remain open. The static alternate-platform limits remain explicit. Rollback withdraws this report/receipt/status only, without recursively withdrawing400/099 or touching source/user sessions.
