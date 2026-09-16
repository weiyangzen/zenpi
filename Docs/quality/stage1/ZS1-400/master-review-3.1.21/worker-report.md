# ZS1-400 — vendor/crossterm/src/event/source/unix 目录理解

本报告只整合蓝图冻结子集 `vendor/crossterm/src/event/source/unix` 的直属文件与跨目录接线。唯一正式产物为 `Docs/learn/stage1_pi_mono/targets/zenpi/vendor/crossterm/src/event/source/unix/current_folder_learn.md`。模式understand、实现LOC0；不生成产品补丁，不修改或自行接受099、401–405、129/131及其它文件。inventory证明物理边界，下面的调用/所有权/错误分析才是目录理解依据。

权威为主库蓝图3.1.21及唯一active selector，run `zenpi-stage1-20260911`，requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`，baseline snapshot `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`。条目400只Depends ZS1-099；G-DIR要求冻结直属文件与直接子目录全[x]后独立整合，不能顺带接受未入scope的文件。

开始读取时099仍[ ]、master receipt不存在，已保存initial-authority及缺失事实，工作保持provisional上下文研究。封包前主控已独立接受099：蓝图和file index为[x]，master receipt complete=true、manual_review.decision=accepted；主控通知当时31/121、snapshot `2bb12c5f41ba8b290fb0a6f74cac1a98bab8faa095775f2463991e58003b246d`，requirement/baseline不变。报告在该依赖成立后编写/封包，初始未接受状态没有被回写抹除。目录400仍[ ]，本包accepted=false，依赖已接受不等于目录已接受。

## 物理清单与冻结子集

目录枚举包含隐藏项检查，实际恰好两个普通文件，均非symlink；没有直属子目录、隐藏文件、额外符号链接或设备项。父目录的unix.rs是兄弟路径上的模块选择器，不在本物理目录内部；不能重复算成本目录直属文件。

| 物理直属项 | 当前字节/行数/SHA256 | 冻结角色和处理 |
|---|---|---|
| mio.rs | 25801B/655行/`41321e242e21fa85533dbae42314bcf80ab916e1d55e065d5ce98076002ac93f` | 唯一in-scope文件ZS1-099，已由主控接受；蓝图原始基线仍9026B/229行/57f4a90b828444fcc6dd7199a73907bbbccf2df4b7808f9e8a0a4b0771e12e39 |
| tty.rs | 10162B/278行/`453e854ca82cd2d37e57494d9766a9eea728883a8a1c5fec186f7c6b83dbd64e` | 物理sibling，context-only；本轮完整新读所有278行，没有把它建立为新正式文件项或继承099接受 |
| 直属子目录 | 0 | 物理枚举与folder index均无本目录的直接子目录依赖，不伪造空目录报告 |

冻结闭包仅099→400。其后400→401(source)→402(event)→403(src)→404(crossterm)→405(vendor)→root091仍逐目录独立处理；本包不输出这些报告。其它目录406或目标133与本目录没有直接子项关系。原始第三方77文件库存不等于本目录或整个vendor语义验收。

## 已接受099与完整阅读链的复用

主控099 canonical报告45666B/SHA `f97e77dd1cba0c152d169a03a2cb94bf90858579f1b2319b78b2e9d043193825`；其worker报告前缀41895B/SHA `568e8cf7f69e103a1d5b889db9d990752ebb542a30529b82dbf15532a898ef5f`，再附独立主控审阅。主控review3769B/SHA `702758bfcbd05cd9c4e4863bf316baf1045c4ad5954417cdc0fb3698969b1499`本轮完整阅读；receipt按原始source_hash57f4绑定冻结输入，manual findings明确理解覆盖当前41321e，不把source_hash擅自改为实现hash。

mio完整理解精确复用刚完成的099：原57f4的229行连续全读，1afa的477行全读，再完整读1afa→41321e的全部178新增行，六段连续映射闭合655行。该新099 ready manifest `6821362d624841bf22b4670072cb7218ca3e6059df64d829516f10345248bb24`；本轮逐字核对当前mio等于其sources/current-mio.rs，不声称又重新全读未变化655行。字段、构造与RAII、所有poll/read/error分支、Parser与全部9个mio内置测试由这条语义阅读链复用，文件接受来自主控receipt而非本包哈希计数。

本轮为目录接线完整新读15个必要context文件：物理tty.rs；父层source.rs、source/unix.rs；sys.rs、sys/unix.rs、sys/unix/waker.rs和两个waker实现；read.rs436行及其全部测试/FakeSource；timeout.rs92行及其全部测试；file_descriptor.rs154行；filter.rs115行及其全部测试；stream.rs146行；根Cargo.toml50行与vendor Cargo.toml240行。每份完整字节及0→EOF读块记录在context-read.json。Parser更深的实现和event.rs公共mutex/reset、TUI接线通过099已接受报告中的明确接口与原快照范围复用，不把该跨层合同复用冒称本轮重新全读整个parse.rs/event.rs/TUI。没有为了目录报告接受这些context文件。

## feature选择和跨目录调用

目录存在两套同名UnixInternalEventSource实现，但父选择器在编译时只导出一个。`event/source/unix.rs`完整11行在use-dev-tty开启时声明/导出tty，否则声明/导出mio；它们不是运行时失败回退列表。`event/source.rs`按unix/windows选择模块，trait为Sync+Send，提供try_read(Option<Duration>)，event-stream时另提供Waker。

| 配置 | 实际source | 同步waker选择与验收范围 |
|---|---|---|
| Unix，events，未use-dev-tty | mio.rs | event-stream关闭时无WAKE；开启时sys/unix/waker.rs选择mio Waker，与source使用相同feature条件 |
| Unix，events，use-dev-tty | tty.rs | 可选event-stream会选择UnixStream写端Waker；不使用mio的pending_tokens/FIONREAD/idle-Esc实现，也不能继承mio测试结论 |
| libc开启/关闭 | 两source内部fd适配分别走libc/rustix | libc开关不改变选择哪个source；根直接依赖libc不等于crossterm启用libc feature |
| Windows | 另一个windows source | 本目录不编译，windows默认feature列出不代表Unix目录在Windows执行 |
| events未启用或其它feature组合 | 不在当前生产入口证据内 | 不把默认或libc+event-stream测试推广为所有组合编译/行为已验证 |

根Cargo.toml18使用crossterm0.29、bracketed-paste且未禁用其默认features，48–50 patch到vendor。vendor默认features包括bracketed-paste/events/windows/derive-more；events启用mio、signal-hook、signal-hook-mio，event-stream另启futures-core，use-dev-tty启filedescriptor与rustix/process。vendor列出的examples及dev-deps只是构建清单上下文，未因整份Cargo阅读就宣称examples或测试全部执行。主控当前默认组合走mio；use-dev-tty不是为41321e回归采取的fallback。

实际调用边界如下，每条边都区分所属目录与消费含义：

| 调用/数据边 | 本目录承担什么 | 对方owner与不变量 |
|---|---|---|
| read.rs Default→父UnixInternalEventSource导出→new | 初始化选中source并注册终端/信号 | new()使用tty_fd，构造错误返回给reader，reader的.ok()可能抹去具体原因 |
| read.rs poll→try_read(leftover) | 从OS字节/readiness和保留Parser状态产出至多一个InternalEvent | reader管理filter和自身事件队列，source不持有上层Filter或项目身份 |
| source→file_descriptor tty_fd/read/raw_fd | 借用isatty stdin或拥有/dev/tty描述符，读取固定缓冲 | fd可能blocking；所有权、close行为由FileDesc负责，source不能任意改共享fd flags |
| source Parser→sys/unix/parse::parse_event | byte前缀和more标志映射为InternalEvent，partial跨调用保留 | 深层key/mouse/query/UTF8/paste规则由parse owner实现；容量预分配不是硬cap |
| source→terminal::size | SIGWINCH后查询当前尺寸，产生Resize | size可能ioctl失败后走外部tput，耗时/error不由本目录强deadline保证 |
| source.waker→sys::Waker→reader.poll | 底层wake造成Interrupted | reader把Interrupted转换为false；trait注释所说Ok(None)不是mio实际直接返回形态 |
| 公共poll/read、可选EventStream→read.rs | 间接消费本目录事件 | 全局reader锁、async任务、TUI草稿/布局由其它owner管理；本目录不是公共入口或UI owner |

filter.rs Unix EventFilter仅接受InternalEvent::Event，游标位置、键盘增强与PrimaryDeviceAttributes可由独立filter选取；Windows EventFilter全true。键盘增强filter有意也接受PrimaryDeviceAttributes作为不支持增强的响应路径。其五个内置filter测试新读了全部断言，但本轮未运行；它们验证枚举筛选，不验证内核readiness或终端输入交付。

## 数据所有权和事件顺序

mio在本冻结子集里保存Poll/Events、单batch pending_tokens、tty_read_started、Parser的partial byte buffer与已解码队列、1024 read buffer、FileDesc、Signals和可选Waker。回调无持久化；同一次try_read优先交付已解码事件，然后消费保留token，仅当pending空才poll下一batch。TTY读到事件提前返回仍保留其readiness，续读前FIONREAD避免排空后猜测blocking read；正常排空退TTY后才尝试单ESC的idle完成。SIGWINCH/wake提前返回不丢同批后续token。这是099已接受的跨文件不变量，本目录说明为何父reader可以多次调用而不要求额外输入键。

read.rs自己还拥有events VecDeque和skipped_events Vec，不得把它们与mio pending或Parser队列合并理解。reader.poll先检查已有匹配项，source产生未匹配项时保存在skipped_events；匹配或超时后恢复它们，匹配项置前以支持随后的read不再阻塞。read在本地VecDeque暂存不匹配项，找到匹配时才放回。这保证正常filter消费不会把其它类型事件循环出入同一队列；但所有错误路径都保存暂存事件的更强承诺并不存在：poll遇Interrupted或其它错误可在drain前返回，read的本地暂存则在后续poll返回Err时随调用结束销毁。源码测试中的error传播/恢复使用FakeSource，并未注入每一种真实系统错误或与所有未匹配事件排列组合。

tty.rs作为必要context完整阅读，实际结构明显不同：它持有Parser、1024缓冲、tty FileDesc、winch_signal_receiver和可选WakePipe。nonblocking_unix_pair把通知通道两端设为nonblocking；它没有把输入tty设nonblocking。new/from_file_descriptor在Signals替代实现中注册SIGWINCH到UnixStream sender；这里不保留返回的注册ID，注释以singleton解释无需unregister。没有自定义Drop或本地注销逻辑，因此不能从本文件宣称反复重建该后端时注册必然完整撤销；具体底层库清理语义未在本轮作为正式子项验证。现有reset跨editor交接的约束不能据此扩展成use-dev-tty后端全部资源行为已验收。

tty的read_complete名义为read complete，实际上loop只重试Interrupted，首次Ok(n)就返回，WouldBlock转Ok(0)，其它Err传播；它没有一次调用填满buffer的实现。try_read每次建立2或3个pollfd，先检查while leftover非零，再取Parser已有事件，再filedescriptor::poll；TTY→signal→wake固定检查顺序。TTY正字节advance并返回首事件，零字节break；signal/wake分别借用通知fd循环drain，signal查询size产生Resize，wake返回Interrupted。所读的278行无内置测试。其Parser仍按256/128预分配、逐字节Some入队清当前序列、None保留、Err丢无效序列、next弹前，与本轮mio新增idle helper不同。

这使context tty存在必须写明的静态差异：timeout=0在Parser队列读取之前就跳过while，返回None；它没有mio pending batch/FIONREAD/idle ESC接点；read_complete把EOF和WouldBlock都折成0，不能继承mio UnexpectedEof的断言。POLLHUP/POLLERR没有被这里单独解释，只有POLLIN位触发read。固定信号通知通道drain和输入缓冲有限并不证明所有blocking tty场景或连续信号压力下有严格时限。本轮未执行use-dev-tty、不把这些静态差异说成当前默认mio回归或经过新反例证实，也不借报告任务越界修复它们。

## 错误、取消、释放和恢复

| 层次 | 错误/取消行为 | 恢复或限制 |
|---|---|---|
| 构造 | fd/Poll/registry/signal/waker失败经?返回；已经拥有的字段按RAII释放 | reader Default把Err转None，后续poll仅给Failed to initialize input reader，waker()对缺source expect会panic；没有自动选择tty后端重试 |
| mio OS读取 | WouldBlock/FIONREAD0正常退队；EOF为UnexpectedEof；其它read/poll/ioctl错误按099分支传播 | 不清所有partial或剩余token来掩盖错误；错误注入覆盖仍有限 |
| Parser | parse Some完成事件，None保留，Err丢当前无效序列 | byte invalid不必成为公开io Err；partial内存没有硬上限；41321e只消费确认完整的单ESC |
| timeout | PollTimeout用Instant记录开始，leftover到期钳零，None永不到期 | Mio入口先取Parser，tty入口先看剩余时间；timeout不是统一取消token或每个字节强deadline |
| wake | mio Waker为Arc<Mutex<mio::Waker>>，tty Waker为Arc<Mutex<UnixStream>>写一个0字节 | 对source返回Interrupted，reader转换false；wake底层错误可返回，mutex poison unwrap会panic，clone延长资源生命周期 |
| FileDesc Drop | libc仅close_on_drop时关闭且不重试close错误；rustix Owned析构，Borrowed不关stdin | 不恢复termios、不flush OS输入、不落盘；stdin borrowed与/dev/tty owned必须区别 |
| 重建/重启 | 内存Parser/队列/readiness随reader丢弃；新的reader重新注册 | 丢弃不等于将partial可靠恢复成草稿；draft/checkpoint/session由Zenpi其它owner负责 |

stream.rs的可选EventStream也完整新读：Default创建容量1的同步task channel并spawn后台线程，持有底层poll waker和两个AtomicBool；poll_next先poll_internal ZERO，有事件则read并Poll::Ready，无事件通过compare_exchange最多提交一个等待task并Pending；线程阻塞poll_internal(None)直至成功或shutdown标志，随后释放executed并wake executor。Drop设置shutdown再尝试底层wake，没有保存JoinHandle或同步join，wake错误也被忽略；因此不能把其注释“quit thread before drop”当作函数已经等待线程退出的事实。后台poll错误在该loop被忽略直到下一轮或shutdown检查，不能宣称异步所有错误/取消都全面有界。这是跨目录取消边界，非Zenpi当前默认event-stream执行证据，也不是新线程工作方案。

read.rs全部436行的15个测试通过队列/不存在source/FakeSource模拟超时、匹配、跳过事件、连续消费和错误传播/之后继续；FakeSource的waker是unimplemented，不能借这些测试证明真实wake或线程取消。timeout.rs五项测试覆盖None、不超时和人工过去Instant的已到期/余量；本轮只读源测试，不将其数量记为本轮通过。目录调用合同分别依靠代码语义和099保存的实际OS/PTY证据，不能让FakeSource替代真实后端。

公共event.rs的mutex、poll/read/filter桥接和reset，以及TUI循环的Resize dirty/后续handle_event已在099准确记录并按其hash复用：正常Resize不reset reader；editor交接必须停止读者，reset只丢内部预读/partial，不flush OS输入。这里没有项目ID/session/HTTP/journal/kill buffer所有权；本目录交付事件不意味着上层输入必定被用户提交或写入持久草稿。目录无独立持久化格式、迁移或事务回滚；撤销本报告不改任何source或会话。

## 真实证据的层次与保留反证

099主控接受明确审阅原229行、旧477行和全部178新增行及全部原readiness测试，六段当前映射属于完整性证据。可靠实际输入负例分别是原57f4超过1024未读字节停顿，以及1afa一次1023x+ESC的socket滞留；后续相同fixture修复通过。孤立ESC公共event::poll/read的真实PTY before/after均通过，缺少每次read trace，不能凭猜测改成生产PTY先失败后通过。本目录保留这些反证，不把两个不同缺陷的失败/成功拼成一场实验。

主控公开证据在 `Docs/quality/stage1/ZS1-129/master-escape-boundary-3.1.21`，当前mio41321e＋headless944049组合已实际通过主控统计144条Rust通过（13+43+7+19+16+46，其中项目13包含self-spawn child）、vendor默认8、libc/event-stream9、Clippy与fmt。8/9有重叠，默认还含reset锁测试；不能累加成17项独立后端证明或整个vendor目录验收。主控master-verification的complete=false保留，因为完整产品项尚未闭合，production_release_rebuilt=false和budget_rerun=false；不能因这些局部检查通过将其写成完整129或131完成。

本轮读取公开review及原始run/log/master-verification/输入绑定后存档，不运行旧runner或产品。主控结果是源集成行为辅助证据；400只在冻结直属子集内整合调用和所有权。旧93f348的预算、29项完整kill-yank矩阵仍是其revision的历史证据，不能改绑到41321e；无新production release/正式budget/全UX矩阵或跨平台验证的声称。当前已验证平台是记录中的macOS arm64及那些实际配置，Linux/其它Unix/use-dev-tty/Windows等界限保留。

## 门禁、剩余边界与交接

本目录唯一in-scope直属项099已由独立主控接受，物理tty及15个context的完整新读不使它们变成正式已接受文件；没有直属子目录待跳过。G-DIR语义结论仅限默认mio路线在冻结子集中的接线：编译选择器和waker同feature切换，source持有瞬态输入状态，reader独立过滤/排队，fd借用/拥有与raw/reset所有权分开，signal/wake顺序和partial保留不能靠额外键/resize reset维持。错误、取消、重启的边界已明确，不以库存或测试计数抵充语义。

本轮离线verifier只做目录枚举快照、完整context读块、精确mio阅读/接受链、receipt artifact hash、唯一报告patch和新manifest核验。G-STAGE脚本本轮不执行，不伪造400 master receipt；主控通知099 G-STAGE已实际exit0作为依赖接受背景保留，不说成worker替400运行。Report accepted=false，401–405及根091状态不改，whole129/131接受不变。

全部输出在全新私有.ops；主库、pi-mono、source、claims/status/authority、旧ready只读。worker HEAD4dbd330d0a8cf58576109713f774969babd499ed与94条非.ops dirty保持。主控集成前应检查mio仍为41321e、tty/context与099 receipt未漂移，再独立审阅400报告并执行自己的G-STAGE/语义接受；若source变化则补充增量，不能继续拿旧hash盖章。回滚仅撤销本目录报告，不递归撤销已接受099、不覆盖产品或其它目录。完成封包后停止，不另开任务、会话、subagent或后续实验。


## 封包时点和完整context读块

封包前最后核对2026-09-12T17:07:03.421108+00:00：099蓝图/receipt已接受，400仍[ ]；selector snapshot `2bb12c5f41ba8b290fb0a6f74cac1a98bab8faa095775f2463991e58003b246d`。mio仍41321e、物理tty仍453e854，正式400报告不存在。receipt及全部artifact哈希与当前canonical一致；源码snapshot和15个context均只读。初始receipt不存在的原证据仍单独保留。

|完整新读context|行|字节半开范围|SHA256|
|---|---|---|---|
|vendor/crossterm/src/event/source/unix/tty.rs|1–278|[0,10162)|`453e854ca82cd2d37e57494d9766a9eea728883a8a1c5fec186f7c6b83dbd64e`|
|vendor/crossterm/src/event/source.rs|1–27|[0,925)|`f606a124266b6d86b58969348bb713f7f89fb09886f51bfb6fe16b766e113bb7`|
|vendor/crossterm/src/event/source/unix.rs|1–11|[0,294)|`253ed074fbe63e7270fc19e5c85186407c3a91454f43291c29cf5717575c74a6`|
|vendor/crossterm/src/event/sys.rs|1–9|[0,248)|`3ec336b2d4c36cfc895fc7cf586e92f7763473130ca99e8168bead1ba3918d1a`|
|vendor/crossterm/src/event/sys/unix.rs|1–5|[0,110)|`d651928295213cd12a4df7edeb0c8be5a4e4487345fc9f2c2ff59d6411047cf2`|
|vendor/crossterm/src/event/sys/unix/waker.rs|1–11|[0,258)|`fe7a1e77718306e9f84b77337f4e9ea9990bed4cf533036228508316363add0e`|
|vendor/crossterm/src/event/sys/unix/waker/tty.rs|1–28|[0,685)|`49132cd71d20b7cc27b79d28a490743da77241bed24a178e22d9ad6e5e98e185`|
|vendor/crossterm/src/event/sys/unix/waker/mio.rs|1–34|[0,1026)|`cbc89c146a004ad689db7f17e3cfc9ce6198c115f31e519d25c68a441db3ecb4`|
|vendor/crossterm/src/event/read.rs|1–436|[0,14606)|`704eb054bff7408e7591e7b5869cf4606794a43b69dc7a1edef67fa7e26ccb8a`|
|vendor/crossterm/src/event/timeout.rs|1–92|[0,2662)|`cbe518ad2cb666b588f593b4b84704eb4480e49d9bd3f28c72d696dae6e7b5e7`|
|vendor/crossterm/src/terminal/sys/file_descriptor.rs|1–154|[0,4249)|`d215ff60394ec2150c2df6340343d84a3922570c1feba813663742d0eb85d13f`|
|vendor/crossterm/src/event/filter.rs|1–115|[0,3786)|`092515c0939799af80768e7b87d2ea4ffc5b1bcda021982f6f02922378809df5`|
|vendor/crossterm/src/event/stream.rs|1–146|[0,5092)|`1df7f35a1842d54b6a8d2b82c96661305d7bd87bbdf123ce6670c62fec5f3cb1`|
|Cargo.toml|1–50|[0,1228)|`d5fafd029ef77454e178222c2b62530d28859374f72a2ce6bd972a465bed5884`|
|vendor/crossterm/Cargo.toml|1–240|[0,4455)|`78b5e8b5cd6660067195979533e61d4b33c375c74613f24180efd94c86f7d295`|
