# ZS1-099 — Unix mio 事件读取器完整文件理解

本报告的唯一源路径为 `vendor/crossterm/src/event/source/unix/mio.rs`，唯一正式产物路径为 `Docs/learn/stage1_pi_mono/targets/zenpi/files/vendor/crossterm/src/event/source/unix/mio.rs_learn.md`。模式 understand；worker 完成理解候选，不写接受状态，不代替 ZS1-129/131 的产品集成验收。

本轮权威为主库 `Docs/stage_1_v3_pi_mono_blueprint.md` 3.1.21 和 `Docs/execution/active_requirement.json`，run `zenpi-stage1-20260911`，权威摘要见下表。

| 权威字段 | 冻结值 |
|---|---|
| requirement_digest | `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d` |
| baseline_snapshot_sha256 | `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884` |
| selector snapshot_sha256 | `7d39ece35ec197cda6e6589057df93631c2aa60f27a82a39aa41a30b78346c73` |
| 本次清单 | 121 项，58 文件，32 目录；已有 28 项接受，099 和祖先未接受 |
| 原始完整文件 | 9026 B，229 行，CRLF，SHA256 `57f4a90b828444fcc6dd7199a73907bbbccf2df4b7808f9e8a0a4b0771e12e39` |
| 最终完整文件 | 18871 B，477 行，CRLF，SHA256 `1afaaa245a132ea4670bd212d1c1c42a078b1fd4b950636147112485589753ef` |

主库读取时已为最终 1afaaa…，正式报告尚不存在。源基线仍是 57f4…；不因主库集成就改写冻结原始哈希。两份源快照来自已封存 129 ready，其 manifest SHA256 为 `1451782d0282b1c8ffb66f5812191b290bcfb93ef730225db29a58945785e738`；本轮以新代码离线逐项核对旧包 125 个 payload，未执行旧 verifier、归档程序或产品。本轮重新连续阅读原始 1–229、最终 1–250 和 251–477；没有用旧 129 主题报告抵充此次逐文件理解。精确半开字节区间及块哈希附后，原始 CRLF 保留。两版本全量 diff 为证据包 `source-only.diff`，它是阅读辅助材料，不是本次拟集成补丁。

## 文件职责、输入输出与真实接线

本文件把 Unix 终端字节、SIGWINCH 和可选显式 wake 转成 `io::Result<Option<InternalEvent>>`，不拥有用户草稿、项目 ID、模型请求、HTTP、journal、布局或 kill buffer。原始与最终 `EventSource` 实现均提供 `try_read(Option<Duration>)`；`Some` 是等待预算，`None` 允许一直等待。成功事件包含 key、paste、mouse、focus、terminal query 回复等 parser 产物，SIGWINCH 另产生 Resize。无事件且普通超时返回 None；最终 EOF 或 I/O 故障返回错误。没有 public 新协议或持久数据格式。

实际接线由 context 快照固定：根 Cargo.toml 18、48–50 使用 crossterm 0.29 加 bracketed-paste，并 patch 到 vendor；vendor 默认 features 有 events、bracketed-paste、windows、derive-more。`event/source/unix.rs` 1–11 在未启用 use-dev-tty 时导出本 mio 类型；启用该 feature 才导出另一个 tty.rs 实现。根直接依赖 libc 不等于启用 crossterm 的 libc feature；生产已有 evidence 为 mio/rustix 路线，libc+event-stream 是额外独立配置的底层检查。Windows 后端不编译本文件；headless JSONL 输入也不是本 owner 的 TTY 事件链。

`event/source.rs` 13–26 的 trait 要求 Sync+Send。`event/read.rs` 18–31 创建 Unix source，再 `.ok()` 转成可选 Box：构造错误的原始信息会丢失，后续 poll 52–59 返回 Failed to initialize input reader；不是本文件伪造成功。该 reader 自有 events 和 skipped_events（capacity 32 仅预分配），不同于 mio 的 OS readiness 与 Parser 队列。poll 先找满足 filter 的已有事件，再调用 source；未匹配事件临时保留，超时或匹配后恢复。Interrupted 在 75–80 转成 `Ok(false)`，这一分支没有立即 drain skipped_events；其它错误向上传播。read 97–125 把未匹配事件暂存，得到匹配事件才放回，其 `poll(None)` 允许阻塞。这些是上下文边界，不在099内改写或声称完整验收 read.rs 的全部测试。

`event.rs` 147–178 的全局 mutex 保持单一 reader，220/265 的公共 poll/read 经 EventFilter 到上面的 reader。poll_internal 会把获取锁的耗时扣入 timeout，read_internal 持锁读取。trait 的 waker 注释说唤醒至 Ok(None)，本文件实际用 Interrupted 表达，reader 再转 false；须按实码理解两层差别。

Zenpi `src/tui.rs` 当前上下文 12072–12123：editor active 时暂停常规读取；普通循环处理 dirty/render/checkpoint，再 `event::poll(wait)`、`event::read()`，Resize 设置 resize_pending，事件交给 `state.handle_event`。因此底层未交付 Ctrl-U 时，上层不会执行 kill；上层一列软换行规则与 canonical draft/fold 映射属于 TUI，原生 composer 测试不能证明内核 readiness 交付。TUI 上下文当前哈希及范围在 context.json，未把这百余行冒称当前整份 TUI 重新全读。

## 完整字段、构造与释放

原始 1–22／最终 1–22：VecDeque、io、Duration 与 mio Poll/Events/Interest/SourceFd/Token 是同步事件读取依赖；Signals 仅注册 SIGWINCH。TTY_TOKEN=0，SIGNAL_TOKEN=1；event-stream 时 WAKE_TOKEN=2。1024 字节固定缓冲是单次读取尺寸，不是单次粘贴或队列的上限。原注释提到曾最多读1022字节，不是可证明的跨平台约束。最终 import 顺序变化为格式差异。

| 字段 | 状态所有权与不变量 |
|---|---|
| poll | OS poll 实例及 registry 来源；构造一次，后续 poll 读取就绪 |
| events | 最近一次 poll 输出，初始 capacity 3；原版直接迭代，最终把 token 拷贝到 pending_tokens 后消费 |
| pending_tokens（最终新增） | VecDeque<Token>，仅空队列才 poll；一批未完成时不覆盖、不追加下一批，避免单事件返回丢同批后续项。capacity 3 是预分配，不把 API 的 capacity 文义说成通用硬上限；逻辑上只存一个 poll batch |
| tty_read_started（最终新增） | 当前队头 TTY 已读到正字节则 true；返回 Parser 事件仍保留它；该 TTY 退队后重置 false，决定续读前是否查询残余字节 |
| parser | 不完整字节 buffer 与已解码 internal_events，跨调用保留；与 pending_tokens 不可互相替代 |
| tty_buffer | `[u8;1024]`，只将 read_count 对应前缀送解析，旧尾部不参与 |
| tty_fd | `FileDesc<'static>` 持有实际输入描述符；可能借用 stdin 或拥有 /dev/tty，不必为 nonblocking |
| signals | Signals 实例及 SIGWINCH 通知读取能力；本文件调用 pending，不自行保存每次 resize 历史 |
| waker（条件编译） | 持有 registry 的可克隆 wake 包装，方法 waker() 返回 clone |

原始35–65／最终37–69：new 先 tty_fd()?；from_file_descriptor 创建 Poll，取得 registry，用局部 raw_fd 建 SourceFd，以 READABLE/TTY_TOKEN 注册；创建 SIGWINCH Signals，再以 SIGNAL_TOKEN 注册；可选 Waker 注册失败同样 `?` 返回。全部成功才构造 Self。局部 SourceFd 是借用适配器，实际 fd 移入 Self；没有保存指向局部 raw_fd 的 Rust 引用。参数、注册和资源任一步失败均通过 RAII 释放已拥有对象，借用 stdin 不关闭；没有重试构造或自动回退到另一 EventSource。

文件没有 UnixInternalEventSource 的自定义 Drop；字段按其自身析构规则释放，Parser/队列内存随之丢弃。`file_descriptor.rs` 92–104 的 libc Drop 仅 close_on_drop 为真才 close，忽略 close 错误且不重试；rustix OwnedFd 负责 owned 释放，Borrowed 不关闭。tty_fd 在 isatty(stdin) 时借用 stdin，否则 read/write 打开 /dev/tty；这里没有设置 O_NONBLOCK。Signals 的具体内部注销时序由依赖实现承担，本次不把未独立阅读的第三方内部 Drop 说成已验收。Waker 包装是 Arc<Mutex<mio::Waker>>，clone 可能仍在外部，不能声称 source 一 drop 就销毁所有外部 clone。waker.reset() 是无操作 Ok(())，wake 锁中毒会 unwrap panic；这些是包装代码的既有语义。

释放此 source 不恢复 terminal termios，不保存/恢复 parser，也不清 OS 输入队列。现有 `event.rs` 151–164 的 reset_event_reader 在 unix 且非event-stream配置，先 try_lock，否则 WouldBlock；成功 take/drop reader，下次 poll/read 重建。它有意丢掉已解码及半截输入，用于已暂停读者后的外部 editor 交接。正常 Resize 不能调用它，否则会破坏099要求保留的 partial/pending 状态。raw mode 的保存/恢复由 terminal/sys/unix.rs 的独立 owner 完成，099修复不修改它。

## 原始读取状态机及最终差异

原始68–159／最终91–213 首先 `parser.next()`；已有事件立即返回，即使 timeout=0 也不先 poll。这保证先前读取的一批按顺序交付，同时意味着 wake/resize 不抢占已解码事件。随后创建 PollTimeout；leftover 将负余量钳制为零，None 不到期。每轮处理完当前 batch 才检查 elapsed；不是每个字节和每个系统调用都检查的强 deadline。

原始每次 parser 队列空后都重新 poll events。poll Interrupted 重试，其它 poll 错误返回；空 events 当作 timeout 返回 None。迭代一批 token 时，TTY 读到数据且 parser 出一个事件就 return；SIGNAL 得到 Resize 同样 return；WAKE return Interrupted。函数没有保存“本批已处理至何处”。因此剩余同批 token 不被后续调用消费；对边沿触发路线也不能假定 OS 会再次报告未改变的可读状态。即使第一 token 是 TTY，本次1024字节生成一批 key 后，内核中其余字节也可能没有新 edge。本轮不把 trace 当作直接观察到每个 token 排列，根因由代码、自然单写反例和修复前后入口证据共同支持。

最终101–119 只有 pending_tokens 空时才 poll，并把 token 迭代结果延长到空队列；121 起始终看 front。一次调用提前返回时队列成为显式续点。下表覆盖所有 match 分支，unknown token 的 unreachable 要求注册与处理保持同步，不是对任意输入数字的公共反序列化。

| 情况 | 原始行为 | 最终行为与后续调用 |
|---|---|---|
| parser 已有事件 | 直接 pop 返回 | 相同；pending 不动，顺序优先 |
| poll EINTR | continue | 相同；leftover重算，但持续EINTR没有显式退出点 |
| poll其它错误／空batch | Err／None | 相同，未谎称有事件 |
| TTY第一次正字节 | advance；一事件就返回 | 标记tty_read_started；advance后一事件返回仍保留TTY队头 |
| TTY续读前可读字节=0 | 无此分支，可能继续blocking read | pop当前TTY/reset false，退出该TTY循环，继续同批其它token |
| TTY正字节但尚未成事件 | 内循环继续 | 内循环继续；下次read前FIONREAD避免已排空的blocking fd停住；partial保留 |
| TTY WouldBlock | break | pop/reset然后break；后续新readiness才开始新TTY读取 |
| TTY Interrupted | continue | 相同，不清parser；不能泛化成所有连续signal压力下有严格deadline |
| TTY其它read错误 | 未返回，可能重复read | pop/reset并返回原始错误；其它pending token保留 |
| TTY EOF Ok(0) | 未break，可能空转 | pop/reset并返回UnexpectedEof（terminal input closed）；已有parser事件先被优先交付 |
| FIONREAD错误 | 不适用 | `?`立即返回；当前TTY与started仍保留，后续调用会重试查询。一次调用没有自旋，未承诺重复调用会自动修复永久ioctl错误 |
| SIGNAL WINCH | 查询terminal size并返回Resize，丢余batch | 先pop该token，pending().next为WINCH才size()?并返回Resize；剩余TTY/WAKE保留 |
| SIGNAL无WINCH | 跳过 | 已pop后继续；不产生虚构Resize |
| size错误 | 返回Err | 返回Err；该SIGNAL已消费，其它token仍保留；没有对该次size查询自动重放 |
| WAKE（event-stream） | Interrupted返回，余batch可能丢失 | 先pop再Interrupted返回，余batch保留 |
| unknown token | unreachable panic | 同样，要求注册内部不变量 |
| 本批处理结束timeout到期 | None | None；无deadline时继续等待下一poll |

最终71–88 的 tty_bytes_available 分两套 cfg：rustix `ioctl_fionread(&tty_fd)?` 转成 io Result；libc 分支 int count 初始化零，FIONREAD ioctl失败用 last_os_error，成功把负数钳零转u64。它观察当前 owned/borrowed fd 的残余，不改共享 stdin flags，也不改 termios。必须先收到 TTY readiness 才允许第一次 read；已有正读取之后才用 FIONREAD 限制续读，避免对 blocking stdin 排空后再猜测读一次。单一 reader 与 fd 的有效读语义是前提；FIONREAD 不是和 read 原子组合的锁，不能据此保证任意竞争消费者/所有canonical设置下均无阻塞。

EOF 在有正读取、之后FIONREAD=0的路径可以先退队，等下一次poll的关闭readiness再真正read(0)；已有测试先排空并timeout，再关闭writer，验证该顺序返回EOF。它没有覆盖每一种数据与关闭同时出现的内核通知排列。SIGWINCH通常表达“需要重新取得当前尺寸”，不是每次历史尺寸日志；size 先 ioctl，失败可 tput fallback，后者耗时不受本方法内部deadline强制中断。这也是保留原始注释所指的外部副作用。

## Parser 全部分支及边界

原始161–229／最终215–283 的 Parser 实现保持一致。Default 给不完整buffer预分配256，internal_events预分配128，均不是硬cap。advance逐字节push，传给parse_event的more为“这次切片后面还有字节 OR 外层read_count==1024”。解析Some时将InternalEvent push_back并clear字节buffer；None保留全部partial；Err吞掉解析错误、clear当前序列并继续后续字节；Iterator仅pop_front。成功或错误clear不保证释放Vec已增长容量。

读取上下文 parse.rs 1–240、812–875：孤立ESC在input_available=false时是Esc键，否则等待更多；ESC+[转CSI，CSI短头等待、方向/功能/鼠标/focus/query等分支交给具体解析器；控制字节0x15形成Ctrl-U，CR形成Enter，LF按raw状态作Enter或Ctrl-J；正常字符走UTF8完整性检查。UTF8合法部分等待补齐，非法开头/续字节或长度达到后仍无效返回错误，由本Parser清理而不是向调用者抛出。bracketed-paste保持从ESC[200~到ESC[201~的完整buffer，最终使用from_utf8_lossy形成Paste；合法Unicode不变，非法paste UTF8并非逐字节保真。未读取/未接受该大parser文件的所有函数和测试，只核实099直接使用的接口及这些路径。

Resize、timeout、WouldBlock不clear Parser，因此跨1024边界和跨两次write的UTF8/CSI/paste能够继续；正常raw读取后的Event::Paste再进入TUI草稿限额，底层不决定fold或budget。持续未闭合paste可以让buffer增大；256/128的capacity及TUI256KiB/4MiB拒绝测试不能证明底层半截paste有内存硬配额或全局取消上限。连续输入内循环或连续EINTR也可推迟timeout检查与wake处理。这些是原有机制的明确限制，不冒称新修复引入，也不通过增加线程/资源规避。

另一个精确静态未覆盖条件：若实际一次read恰好1024字节，其中1023个ASCII后接孤立ESC，read_count==1024使末字节more=true；随后无字节时，最终的FIONREAD=0只是退TTY队列，不调用parse_event(buffer,false)。已解码ASCII耗尽后ESC仍保留，timeout不会idle-flush它。该结论由两个函数分支推导，本轮没有执行新反例；原始版同样没有flush。已向主控说明，不以当前测试宣称所有满buffer末尾ESC都已解决，不改已封存129。部分序列本来要等真正后续字节，与“已有完整的2065个key却等额外write才能读完”是不同判据。

## 最终文件的全部内置测试

原始229行没有本文件内置测试。最终285–477全部重新阅读，无遗漏或选择性只读新增关键字。pair() 创建真实UnixStream pair，将输入端设为nonblocking；libc用IntoRawFd + close_on_drop，rustix用Owned转换；调用真实from_file_descriptor注册内核readiness。key()构造无modifier Char事件。不启动FIFO、伪EventSource或测试专用parser替身。测试用socket确定性控制内核字节；它与真实TTY/termios不是同一覆盖层。

| 测试及最终行 | 判据与实际已有证据 | 不可扩大的结论 |
|---|---|---|
| single_edge_over_1024_bytes_delivers_every_key_without_another_write，315–329 | 一次write 2065个x；每次20ms读取全部2065个key；末尾ZERO None。原始加相同测试在第1024索引得到None，修复通过 | 无新写入/辅助wake的自然反例；不验证全部terminal驱动 |
| single_edge_keeps_split_utf8_escape_and_bracketed_paste，331–366 | 1023个x后“界”跨buffer，接Left CSI、150次组合字/ZWJ完整paste、y；逐项断言及末尾None。原始在界得到None，修复通过 | 验证给定合法Unicode与闭合paste；未覆盖未闭合无限输入或非法paste保真 |
| blocking_descriptor_drains_then_timeout_and_eof_are_bounded，367–394 | blocking socket写x，取x后20ms返回None且耗时<1s；drop writer再读返回UnexpectedEof | 真实blocking fd路径，但非所有TTY canonical模式；无每种read/ioctl错误注入 |
| partial_input_waits_for_next_write_without_discarding_parser，396–422 | 写e7后None，再写95 8c得到界；写ESC[后None，再写D得到Left | 此处等待后续字节是合法partial语义；并未在partial中插入真实SIGWINCH |
| pending_kernel_wake_and_tty_batch_survives_both_return_orders，424–477，event-stream | 实际写x并wake，poll后断言两个token存在；明确sort成两种先后，再预置已解码q，ZERO先得q，随后各顺序得到Interrupted/x，末尾None | 内核产生真实readiness，但顺序是测试安排；不能说自然内核两种顺序随机均观察到，也不能替代TTY/SIGNAL/WAKE全部三者的每种排列 |

最后一项是有限两种顺序与Parser优先的回归，实际SIGNAL/resize充分性另依赖原始PTY场景。WouldBlock分支在正常代码中明确退队；上述读取常由FIONREAD=0先退队，不凭这些计数宣称每一次测试都实际命中过WouldBlock错误。其它read错误/ioctl错误、构造失败、size失败、unknown-token panic均为本轮完整静态分支复核，缺少逐种实际注入证据；它们的可执行后续判据是控制真实fd/系统调用条件，确认返回对应Err且剩余token/partial不被错误清空，而非只看compile。

## 历史行为证据、版本绑定与本轮验证

全部执行发生在旧129、3.1.20轮次（2026-09-12 UTC），本轮3.1.21只读取/离线核验。`prior129`保存引用的原run.json、log、结果、诊断source、失败与固定输入；归档缺省toolchain启动错误和首次cargo -p crossterm未选中独立包错误也保留，不计作运行了测试。各run receipt提供argv/cwd、开始结束、exit_code/timed_out、log bytes/SHA与owner哈希。早期两个vendor运行的额外来源绑定见early-vendor-run-bindings.json，分别c16e0e…原实现+相同两测试、2c5257…修复+相同两测试，不伪称它们运行了最后477行全部5项。

| 历史运行 | 观察 | 输入与限制 |
|---|---|---|
| negative-ready-tests-standalone → fixed-ready-tests | 相同自然两测试0/2失败→2/2通过 | 明确失败值为key1024 None／界None；不是零测试绿灯 |
| fixed-ready-default | 2026-09-12 15:38:24.352655–15:38:25.360100 UTC，exit0，4/4 | 最终1afa；log1600B SHA31ef95eb79235d8320644fdb032d6113ac21fb136dc0ecfb2f313b641b7224af |
| fixed-ready-libc-stream | 15:38:45.112024–15:38:46.981834 UTC，exit0，5/5 | 最终1afa；log1763B SHA66c8e0487723e1e23e13186747b476b3cc24abcfec8c531c0701cb72e52042c3 |
| baseline-composer-native／fixed-composer | 各46项通过 | 上层TUI此前已正确处理输入；不能用46项替代readiness反例 |
| diagnostic-traced-pty | 原Ctrl-U等待12秒失败；失败后附加Ctrl-L才交付Ctrl-U，编辑5682B→1001B不到1ms；HTTP0仍失败 | 临时trace、额外键仅诊断；不算原场景通过 |
| fixed-original-pty／diagnostic-v2-pty | 修复后原Ctrl-U通过；Enter交付后草稿清空，随后backend Peer disconnected恢复草稿、HTTP0 | 保留ambient代理环境失败，不能把HTTP错误说成Ctrl-U成功完成全部场景 |
| baseline-clean-loopback-pty | 15:44:36.323202–15:44:57.033446 UTC，exit1，原Ctrl-U处失败，前11checks | binary e9ba9476f7699d715ad3b0b5fe4a07aeee9f280508d3cb495d59cfa94fff6449；log16640B SHA802e2b96e3a6a717f8d758c7e3bca637dc522ffb29fe603ed94b615b55775759 |
| fixed-clean-loopback-pty | 15:44:14.810502–15:44:45.003751 UTC，exit0，29checks、4个HTTP请求、3个TUI进程 | binary7be42aa377ebb32440163bfdfb5f7362fc7414b9f1a57fe27940a533f4c8f023；log1025833B SHA31d6df32a427218c55e52a32e31935625d35b12b71be7d3647ed198d77603466 |

paired loopback对照使用同一个原始harness `tools/tui_composer_smoke.py` SHA `35899dc9ab17e84d3ab9a4db75614fec850a426495e9dfbe324891af2f53a389`，Unicode/fold、1×1→140×40、Ctrl-U、Enter、原12秒等待及HTTP全文比较保持不变。clean-loopback.py只在child环境移除大小写HTTP_PROXY/HTTPS_PROXY/ALL_PROXY/NO_PROXY名称，实际存在6个代理变量；HOME/CODEX_HOME不改。双方隔离一致，不擦掉ambient失败；成功场景未额外发Ctrl-L、未resize reset、未等待未来键。诊断来源、实际失败与成功没有混合评分。

固定after构建的TUI SHA `994215c7ca6cc471bb35247a04460d6c5ef52192999e4b5585ea1c818a429578`，与before上层owner相同；composer只附加独立回归。before-clean的run.json owners记录的是执行harness所在candidate树，而不是旧binary的编译树；应以结果binary_sha256、binaries.json和旧build-inputs归档区分这两层，不能看该receipt的mio1afa就误认baseline也运行修复版。

底层独立vendor Cargo.lock使用mio1.0.3/rustix1.0.5；产品root Cargo.lock使用mio1.2.3/rustix1.1.4，旧包build-inputs-before.tar.gz记录224项冻结输入。该大归档与binaries.tar.gz只通过旧manifest链接，未在新包重复存放/执行。macOS aarch64本地已运行；Linux、Windows、其它Unix、no-default-features及所有feature交叉组合未验证。4与5项有重叠，不能累加成9个独立语义覆盖；29项包含上层kill、项目/审批/重启/预算边界，也不等于129或099已由主控接受。旧clippy/fmt/release记录为历史辅助，不是本轮新编译或最新组合生产预算。

本轮离线 verifier 仅校验新包manifest、完整连续read ranges、两源hash、复制的旧payload与旧manifest、run/log绑定和关键结果数据、唯一报告补丁及冻结authority关系。G-FILE语义依据本报告的完整分析，仍待独立master审阅；G-STAGE指定脚本本轮未重跑，未生成master receipt，不声称它通过。不得将离线PASS冒充功能验收。

## 缺口、取消/恢复、范围闭包及交接

该文件内没有独立取消token/事务/持久化，唤醒只限event-stream Waker；普通TUI通过有限poll间隔获得继续循环的机会。重启创建新reader，丢弃内存parser/pending；不会恢复未读partial为用户草稿。实际draft checkpoint、重启kill buffer清空、HTTP重放、approval、session身份属于Zenpi owner，099仅说明事件交付如何进入它们。外部editor交接reset是显式上层协议，不能从正常readiness保留推导为跨进程共享输入无争抢。

明确保留的缺口/未测边界：满1024末尾孤立ESC无idle flush、未闭合paste无底层硬cap、连续EINTR/持续输入/size fallback无严格整体deadline、任意竞争读者与非标准terminal模式、各类ioctl/read/size/构造错误的独立实测及所有三token排列。当前最强证据支持自然单edge剩余输入和原PTY Ctrl-U修复，不虚报覆盖这些边界。上述静态发现已单独报主控；本轮没有新执行性反例或源码修复。

099只依赖已接受的ZS1-001。目录链400（source/unix）→401（source）→402（event）→403（src）→404（crossterm）→405（vendor）及root091仍分别验收，不能随099接受。source.rs/read.rs/parse.rs/tty.rs/timeout/fd/waker/event.rs/Cargo/TUI均是context-only；没有声称原始crossterm归档77文件已语义学习。来源archive SHA d8b9f2e4c67f833b660cdb0a3523065869fb35570177239812ed4c905aeff87b属于131供应链库存，099不替代131或098锁文件理解。133及406属于另一路文件/目录，不在本报告中盖章。

交付仅新增私有ready中的report.patch（唯一正式报告新文件）及复核证据；不应用source-only.diff、不改产品、主库、pi-mono、authority、claims/status、旧ready。worker HEAD `4dbd330d0a8cf58576109713f774969babd499ed` 和94条非.ops dirty与已冻结capture一致。集成前主控须核对当前source是否仍为1afa，独立阅读此报告并生成自己的receipt；若source后续修订，补充全部增量并重绑最终版本。回滚只撤回本报告候选，不覆盖原始/最终产品字节，不回退主控的新headless组合。


## 主控3.1.21最新组合的独立证据补充

报告撰写期间主控完成独立组合验证，并明确允许引用主库 `.ops/stage1_execution/readiness-identity-3.1.21`。本包master-current按master-evidence-index.json复制原始review、input清单、run/log、PTY及JSONL结果；没有执行它们。该组与上文旧129的3.1.20执行严格分开。

主控当前组合保持mio1afa、TUI994215，合入headless0ece91及composer c56a65。主控tests.run.json为2026-09-12 16:22:03.928088–16:22:51.898015 UTC，6个target合143项，exit0，log12713B SHA7289124778a1bd1eff4a970c887afdf689bcff02119c2126ba73578f8989fb7c；另存Clippy/fmt与release日志。binary.json为6072912B、SHA `93f3488855c26e3ebb4f9d30a69e4372f3d01070766976ee727d9b7ab4f50fd7`，不同于旧worker after7be42。

主控在该release首次启动前跑唯一固定3样本预算，2026-09-12 16:24:45.359576–16:25:28.260720 UTC，原始样本460.119959/36.881584/37.090333ms，结果8项gate为true，normal deps实际15。此处只引用该次原始记录，不重跑、不把它等同普遍启动根因已定位，也不擦掉旧1109ms失败。

主控随后用原harness做before/after：原e9ba在16:25:28.496356–16:25:49.706340 UTC再次于Ctrl-U前11项后失败；组合93f348在16:25:49.714165–16:26:20.423227 UTC通过原29checks、4HTTP、3TUI。after log1027789B SHAa3f2031ab35c0814ba9f3cdc28dd710ec22e81219bb702f1da4d7e24697f2a5d。主控run记录两端removed_proxy_names均为空数组、HOME/CODEX_HOME保留，review说明采用同样child-only隔离；不能把旧worker实际删除6个变量的事实搬到这一组。新组合JSONL在16:26:23.247659–16:26:25.663393 UTC通过35/35，log19721B SHAbe3c117aaed74d824c381630b7da5e163fab784e11ecc92f7cc8dd50876885b5；此为headless身份接线旁证，不是099新增scope或目录接受。

最新组合已有上述主控实测，纠正“尚无最新组合验证”的时间性判断；本worker本轮产品执行仍为零。主控明确孤立满buffer末尾ESC疑点下一轮做新的有限真实反例，属于既有键盘范围，无须放宽129合同。本轮到此冻结文件理解，不补旧ready，不预先称静态疑点已通过或已修复。主控review同时明确完整129/131/132/117、所有文件/目录及生产UX矩阵尚未闭合，099无master receipt，仍不能自行接受。

## 连续全读的精确字节证据

以下range为0-based半开区间，行号1-based含端点；每版从0无缝覆盖EOF，内容以原始CRLF字节hash绑定。哈希证明输入和范围，语义依据上文逐分支报告，不将范围完整性当作自动理解。

| 版本/文件 | 行 | 字节范围 | 字节数 | 块SHA256 |
|---|---|---|---|---|
| original / sources/before-mio.rs | 1–229 | [0,9026) | 9026 | `57f4a90b828444fcc6dd7199a73907bbbccf2df4b7808f9e8a0a4b0771e12e39` |
| final / sources/after-mio.rs | 1–250 | [0,10699) | 10699 | `f9eb62cbdf899b643b4f404a10e93288cbc1c8d9185f1a7d727a04154c2c0e99` |
| final / sources/after-mio.rs | 251–477 | [10699,18871) | 8172 | `3b68a558c2a23a1c785b9cf13f12afa4b64721353638a425d9dddf1c0bc8928f` |

## 099 增量补充：满缓冲孤立 ESC 候选 41321e

本节是旧099报告封存之后的独立补充，前文28350B保持精确原文前缀（SHA `61ff3b41324aaf5b403f25eeccba2e02bbac46c1ddb18ab1497485d9d46cfb61`）。前文“静态疑点尚未执行”“最终1afa”“当前主库”等说法均保留其当时捕获时点；本节说明后来发生的试验和候选变化，不倒改原始报告为“当时已经实测”。仍只有同一个正式文件报告，不把099主题拆成第二个正式产物。

补充开始时主库mio是18871B/477行/1afaaa，主控正在审阅新候选；headless已经944049，跟新ESC试验冻结的0ece91不同。本报告不把候选41321e说成主库已集成，也不拿0ece91上的独立vendor运行冒称最新主库全产品验收。末尾记录封包前最后一次只读身份核对，以该时点为准。

### 三个源版本与阅读方法

| 版本 | 字节/行数/SHA256 | 阅读与证据角色 |
|---|---|---|
| 冻结原始 | 9026B /229行 /`57f4a90b828444fcc6dd7199a73907bbbccf2df4b7808f9e8a0a4b0771e12e39` | 保留旧099连续全读，仍是蓝图冻结source hash，不改为候选hash |
| 先前最终 | 18871B /477行 /`1afaaa245a132ea4670bd212d1c1c42a078b1fd4b950636147112485589753ef` | 保留旧099全读链与字段/接口/构造/Drop/状态机/五项原测试；该版本是此次产品增量唯一base |
| 本次补充目标候选 | 25801B /655行 /`41321e242e21fa85533dbae42314bcf80ab916e1d55e065d5ce98076002ac93f` | 178行新增：生产/说明15行，测试/辅助/cfg/空行163行；无删除，无原函数或原五项测试被偷换 |

旧099 ready manifest `2befdcaf00424f8310c0092441d172af786bbc995281c13830c2e3b594b62a07`；新ESC产品候选 ready manifest `ee4b7c7f491a9c57312f23e5a754f706259b766fff22738e2a19938fe3d37cd0`。本轮先离线核对两旧包全部payload，再直接读取新source diff的全部新增行及上下文；没有执行任何旧verify、runtime、Cargo、PTY或budget。两个旧包均作为不可变历史复制/引用。

655行理解使用“精确未变字节复用＋全部新行重新逐行复核”：新1–172映射旧1–172，新179–257映射旧173–251，新267–492映射旧252–477；新173–178、258–266、493–655全部本轮新读。后附六段半开字节覆盖表无重叠/遗漏，既绑定复用片段的旧hash，也绑定新增片段hash。这不是声称又从头重新读了未变477行，也不以range自动证明语义；新增分支的理解如下。

### 生产变化逐行解释与跨分支约束

173–175的三行注释指明：满1024 read可能使末字节ESC保持未定，只有TTY readiness真正排空才解析。176–178新增一次私有helper调用：Some立即返回这个内部事件，None继续原SIGNAL/WAKE等batch处理。位置在TTY内层read循环退出之后，正常能到这里的是FIONREAD=0和WouldBlock两个break；它们之前已经pop TTY并重置tty_read_started，因此新Esc返回不会让空TTY被反复处理。

258行定义Parser::finish_pending_escape，返回Option<InternalEvent>而非新的public API。259–261严格比较buffer只含单字节0x1b，否则返回None；UTF8首字节、CSI ESC[、paste ESC[200~等均不匹配，内存一个字节不改。262将该孤立ESC交给原parse_event(buffer,false)，.ok().flatten()?只取已有解析器产出的完整事件；意外Err/None退出且不清buffer。263仅在已得到完整事件时消费该序列，264返回该事件，265结束helper，266为空行。这里clear是正常消费一个已确认事件，与reset全reader或清空partial/已解码队列不同；internal_events从未被该helper清空。

不变量是“真正排空后才把尚未交付的单Esc完成”。若下一read的Alt字符已在内核等待，FIONREAD>0仍进入原read/advance，让ESC与后缀形成Alt；不能先触发helper。若afterread已有多个解码事件，try_read入口仍优先逐个pop，helper只在Parser队列耗尽、TTY正常排空后触达。新return仍保留同批未消费的SIGNAL/WAKE，不改变已建立的pending顺序。SIGWINCH处理、size查询、waker Interrupted、构造失败、unknown-token panic、EOF、其它read/ioctl错误及Drop全部沿用1afa；这些错误直接return的路径不会绕到idle helper，因此错误不被掩成正常Esc。

这一策略不引入歧义计时器：如果Alt后缀未来才写，而当前已经排空，则孤立ESC可立即交付。它与旧短于1024的read传more=false时的行为一致，不承诺任意时间间隔的两次write都会重组Alt。既有256/128容量仅预分配，连续EINTR/持续未闭合paste/竞争读者/size fallback的限制仍保留，绝不从20ms有限测试泛化为无限压力下全面有界。fd的blocking/termios设置、1024缓冲尺寸与OS事件注册均不变。

### 新163行测试及辅助函数完整复核

493空行、494 cfg(test)、495模块到655闭括号均完整阅读。导入沿用真实UnixInternalEventSource/KeyEvent/Write/UnixStream；pair(blocking)创建socket pair，input.set_nonblocking(!blocking)，libc支把IntoRawFd转换为close_on_drop=true的FileDesc，rustix支Owned转换；真实from_file_descriptor注册内核readiness，output由测试持有。没有伪EventSource、FIFO或额外应用线程。key产生NONE modifier事件；drain_x在每一次20ms读取中断言字符和索引，不能跳过prefix消费只读最后Esc。

| 新测试 | 全部输入、分支与断言 | 实测和覆盖限制 |
|---|---|---|
| one_write_full_buffer_trailing_escape_is_delivered_without_another_event | 遍历nonblocking、blocking；一次write1023x+ESC，逐个消费x；20ms读Esc且操作<1s，ZERO读None；drop writer后20ms返回UnexpectedEof | 原版在第一nonblocking分支失败，后面blocking分支未执行，不能说它也实测失败。候选两分支均通过；不等于每种tty canonical/竞争模式都验证 |
| full_buffer_escape_uses_already_pending_alt_suffix | 一次write1023x+ESC+a，共1025B；取完x后要求Alt+a，不允许单Esc或重复事件，末尾ZERO None | 原版、候选均通过，验证已有内核后缀不被新idle完成拆开；未来才到的后缀不在该断言内 |
| full_buffer_partial_utf8_csi_and_paste_survive_idle | 三组prefix使第一次write精确1024B：e7、ESC[、ESC[200~；取完x后20ms None，直接断言Parser.buffer==prefix；再写95/8c、D或组合字ZWJ加paste terminator；要求完整界、Left、原Paste，末尾ZERO None | 原版与候选均通过。合法partial后的第二write是完成输入，并非给孤立Esc追加救援键。无无限paste试验，无非法UTF8保真承诺 |
| escape_idle_completion_retains_real_wake_in_both_batch_orders（event-stream） | 一次1024B加真实waker；poll后断言TTY/WAKE存在，显式排序wake_first true/false；分别在x/Esc之前或之后取Interrupted，取满1023x并ZERO得到Esc，最后ZERO None | 候选两顺序通过。readiness来自真实内核，顺序由测试排序；不冒称自然观察到所有排列或三token含SIGWINCH全部排列 |

这四项新测试外，原五项zenpi_ready_tests逐字保留，仍覆盖2065字节无追加write、多read UTF8/CSI/paste、blocking普通timeout/EOF、跨write partial、已有Parser事件优先和双wake顺序。默认运行名过滤zenpi_同时选中既有event.rs的reset锁测试：8/8是旧mio4＋新3＋reset1；libc/event-stream9/9是旧mio5＋新4，reset不在该cfg。两配置互相重叠，不得合计成17个独立场景，也不能据此接受所有event reader或目录。

### 静态疑点之后才发生的实际证据

旧099只根据代码推导满buffer末尾ESC滞留；随后新产品轮才执行下面这些试验。本报告补充轮只读取其原始文件，不运行它们。所有时间为2026-09-12 UTC。新socket negative源码98ef991…为1afa原文加相同测试；最终41321e经过rustfmt，新增测试仅空白及可选尾逗号排版变化，fixture/阈值/断言未变。run.json记录的是实际子命令exit_code；外层归档脚本正常结束不意味着negative通过。

| 历史试验 | 明确结果 | 绑定 |
|---|---|---|
| negative-socket，16:40:55.084564–16:41:11.349908 | 3项：2通过、孤立ESC失败，left None/right Esc，exit101 | 4693B log SHA99d4a1cb2145fca97225e265f125db2557c04e2e359465693472d7a9e14c779b；source98ef991ba195fae668f9b44a62d9825272ab8871fe2647c9248a7803a84edf2e |
| fixed-default，16:42:22.359402–16:42:23.343200 | 8/8，exit0，原readiness回归保留 | 2078B log SHAd0b6ae9dad505ddfaafa9b22973ec9241e2c59919d361210ca771a4c10b89a2a；source41321e |
| fixed-libc-stream，16:42:37.809473–16:42:39.407404 | 9/9，exit0，含新Esc/wake双顺序 | 2271B log SHA09a4e0622714979fb42a02ed86958dfa94108a55367af9b003b38a3cfc124c29；source41321e |
| before公共PTY，16:41:48.587494–16:41:49.022832 | 原版也通过：1023x、Esc、other0，812µs，exit0 | 探针binary76a7782666c7e5b18f16f807f3b49cac37e10847ebf206c9271bdd78d411c3e8 |
| after公共PTY，16:43:09.626840–16:43:10.610349 | 同一fixture通过：1023x、Esc、other0，637µs，exit0 | 探针binary261562a5dc51f31c2d9a9479a0198bee0d1654337d9c6bcd496e371ba826f1c3 |

公共reader使用同一个21行Rust探针，enable_raw_mode、poll ZERO初始化、打印READY后在200ms固定总窗口以10ms poll/read消费；Python建立真实PTY，slave作为stdin，stdout/stderr为pipe，等READY后只write一次1024B，payload SHA808b153a414fe681a205a0f961c52059e074f3637560b1a0f43c1f6cd3dc6829。两次均无额外输入/resize/reset，恢复原termios且process reaped。探针源码SHA0eea4a385bbaabbe0e92ed7a42cc1298ce740446db54a4eb2be82e63c571b04d；Python fixture SHAfbcc361085b8523ba49aa17818c1f34d99c83f2dc2afc308c6988174fb1e6a3c。它们是独立公共event reader探针，不是Zenpi release或BentoBox交互测试。

必须保留的反证：公共PTY原版已经通过，本次没有记录每次read的实际长度，所以“PTY可能分成短read”只是推断，不能写成已观察事实；不能把socket可靠失败改称真实生产PTY先失败后通过。没有重跑直到得到想要的negative，也没有用附加键/增加buffer/放宽超时取绿。当前可以主张的结果是：真实socket精确满read边界的有限缺陷已复现，候选同夹具修复；公共PTY给出该次入口行为保留证据。

这些试验用独立vendor锁文件mio1.0.3/rustix1.0.5，在macOS arm64执行；冻结headless0ece91仅是构建上下文，非被测TUI/HTTP链。主控后来headless944049不进入这次试验。旧主控93f348组合的原29项kill-yank、budget、JSONL记录仍只证明编译时的mio1afa；本补充不据此宣布41321e最新组合通过。Linux/其它Unix/所有feature组合、错误逐项注入、任意连续EINTR及未闭合paste有界性依旧未验证。

### 本轮报告门禁与范围

G-FILE的增量语义由上面的逐行分析与实测边界支持，未变部分由精确全读链覆盖；本轮新离线verifier只验证报告原文前缀、三源hash、六段连续范围、两旧manifest及复制证据、run/log绑定和唯一报告patch，不执行旧脚本或产品。G-STAGE指定脚本本轮未运行，master review receipt未伪造，接受仍false。hash/范围/测试计数不能替代主控独立语义审阅。

候选只新增或更新099唯一正式报告，产品source.patch仅在历史证据目录内，绝不能作为此报告任务的送审patch应用。原229行基线、旧477行分析、静态疑点当时的状态均不删除；主控可在审阅后为655行版本生成独立接受证据。400–405目录链、root091、129/131以及其它文件均不随此补充接受。回滚只撤回本次报告增量，保留旧报告历史与所有产品字节，不修改主库、authority、旧ready、共享TUI或主控headless。


### 封包前主库身份与连续范围

最后只读核对时点为2026-09-12T16:55:38.971072+00:00：主库mio 25801B / SHA `41321e242e21fa85533dbae42314bcf80ab916e1d55e065d5ce98076002ac93f`，已与本报告655行目标相同；headless 366788B / SHA `9440492db142157995e0b479c2d31b80d9158ae624ca29d08f4d93db34e81509`。这与补充开始时mio1afa不同，是主控独立应用所致，本worker没有写主库。主控已明确组合回归正在执行，本轮不读取/宣称其结果通过，也不将旧93f348或独立vendor测试改绑成最新组合验收。正式099报告此时尚不存在，report.patch为该报告新文件。

|655行目标范围|字节范围|方法|SHA256|
|---|---|---|---|
|1–172|[0,7395)|复用1afa行1–172|`8bd510b2ed2a529c4bc23126105bd0c32cad14ae749729c6ecd69569c2d6d335`|
|173–178|[7395,7799)|新读全部新增行|`118e8f55382068f4f99b6716ba90ae4c22c3d00d7d1451a3844298ea3cc4fc92`|
|179–257|[7799,11118)|复用1afa行173–251|`108054d1c280fd62c9a9b1b53ed42d38f0e05eb5eb60d1509e98b498039d8271`|
|258–266|[11118,11402)|新读全部新增行|`a666af627453ccbff8a3dc358d58fe357f336051c47520fa2ce09695f1f21770`|
|267–492|[11402,19559)|复用1afa行252–477|`be4308a827e19f840ba1cdec99b255a943ecf8d8671ef687da85c546a20b3350`|
|493–655|[19559,25801)|新读全部新增行|`dc760582662a2dffdf9006e3d80c7076438279ff5f2b15d9311cd8385f7bc588`|

所有新运行仅为本包离线完整性检查，其外置receipt记录argv/cwd、开始结束、退出码和log/hash；未执行旧脚本、runtime、正式budget或任何产品测试。worker HEAD与94条非.ops dirty保持，两个旧包全部payload未变。独立主控语义接受仍待完成。


## Controller independent complete file acceptance — current41321e, 3.1.21

The controller previously read the complete28350-byte original099 report and complete477-line1afa source, and now independently read the entire new appendix, complete178-line Esc patch and worker product review. In this acceptance turn the original9026-byte229-line frozen source was read again in full, along with current production polling/parser context and the entire unchanged readiness-test module. This covers original fields, construction/drop, all poll/read/error paths, parser state, original and added tests; unchanged blocks reuse their exact source identities rather than claiming a new unrecorded whole-file reread. The current655-line identity is reconstructed by the six contiguous ranges supplied with the worker chain; the manifest/ranges are integrity evidence, not a substitute for the semantic reading above.

The original source lost unconsumed edge-triggered readiness across early event returns and did not reliably handle EOF/other read errors. The first1afa change retained a pending token batch, avoided speculative blocking reads after progress with FIONREAD, kept parser events first, retired drained tokens, and returned EOF/read errors. The subsequent41321e change completes only a single pending Esc when that TTY token is actually drained; it does not consume incomplete UTF8/CSI/paste, discard already queued Alt suffixes or change fd flags, terminal mode, capacities, signal or wake ownership. The helper consumes only a successfully parsed sequence. Existing allocation capacities do not enforce bounds; repeated EINTR, endlessly incomplete paste, competitive consumers, initialization-error masking in the caller, size fallback and untested error injections retain the documented limits.

The worker report preserves the original static Esc suspicion and separately records the later reliable real-socket negative. Its public PTY before and after both pass; no per-read trace proves the cause of that difference, and no production-PTY failure is invented. All old source/read/test chains remain immutable. The current report-only verifier was fully read and executed once in a new copied package:262 payloads, three source versions, six contiguous current ranges,178 new lines, original report prefix and11 historical run-log bindings pass. No historical product program was rerun by this verifier.

After the worker's final identity snapshot, controller integration independently completed with main mio41321e, headless944049 and project-test8375:144 top-level Rust tests, vendor default8 and libc/event-stream9, Clippy and fmt pass. These overlapping vendor configurations are not17 independent tests. Root source and vendor lockfile checks have separate scope. Product evidence is published in ZS1-129/master-escape-boundary-3.1.21. No production release or formal budget has yet been executed for this combination; prior93f348 evidence remains historical. The earlier appendix statement that combined checks were still running is preserved as its captured-time statement, qualified here by the actual completed controller result.

Accept only099 complete file understanding of frozen57f4 baseline and current41321e source. The blueprint source hash remains57f4; it is not replaced with an implementation hash. The source has no project/session/persistence or model-transport ownership. TUI integration, full129/131,400–405 directories and whole Stage1 remain separately unfinished. Platform coverage is the actual macOS arm64 configurations described above; it does not establish Linux, all Unix features, use-dev-tty or Windows behavior. Those limitations belong to the file's understood contract, not hidden claims of whole-product completeness.
