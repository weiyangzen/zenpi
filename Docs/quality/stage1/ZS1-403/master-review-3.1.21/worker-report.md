# ZS1-403 — vendor/crossterm/src 独立目录理解候选

本报告唯一正式产物为 `Docs/learn/stage1_pi_mono/targets/zenpi/vendor/crossterm/src/current_folder_learn.md`。模式understand，产品实现LOC0，仅整合冻结子集event与必要crate级接口，不能把物理src全树理解或产品行为整体接受。蓝图3.1.21的403只Depends ZS1-402；requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`、baseline `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`保持。开始时402仍[ ]、master receipt不存在；initial-authority保存实际原字节与缺失事实。本文先作候选，最后依赖状态见封包记录，不把库存或离线成功当作G-DIR通过。

## 实际直属库存与冻结边界

本轮直接枚举src，包括隐藏项，恰好十个普通文件、四个普通目录，无symlink、隐藏额外项或其它类型。Rust的同名模块文件与目录是两个不同物理项；event.rs是src直属文件，event/才是唯一正式子目录402。这里不把此前402的父上下文event.rs重复计成正式文件。

| 直属项 | 当前字节/行 | 本次角色与读取方式 |
|---|---|---|
| ansi_support.rs | 1834 B / 46行 | context-only，完整新读；解释Windows Command默认能力判断及初始化副作用 |
| clipboard.rs | 10749 B / 308行 | 仅库存成员；lib.rs表明osc52门控，本次未深入实现，不纳入输入冻结链语义结论 |
| command.rs | 10943 B / 295行 | context-only，完整新读；Command、queue/execute、格式桥接、同步更新 |
| cursor.rs | 15481 B / 504行 | context-only，完整新读；position导出、坐标Command、显式显示/保存恢复和全部源测试 |
| event.rs | 63629 B / 1777行 | context-only，精确复用402完整读取；共享reader、公开类型、reset和Command定义 |
| lib.rs | 10739 B / 263行 | context-only，精确复用402完整读取；crate的模块/feature/导出总接线 |
| macros.rs | 12627 B / 378行 | context-only，完整新读；queue/execute和impl_display等宏及全部源测试 |
| style.rs | 18745 B / 621行 | 仅库存成员；lib.rs声明style，本次不展开颜色/样式实现，不声称全文件已读或已接受 |
| terminal.rs | 18102 B / 568行 | context-only，完整新读；raw/size入口、屏幕/清理/同步更新Command及源测试 |
| tty.rs | 1594 B / 54行 | context-only，完整新读；仅借用fd/handle进行tty检测 |
| cursor/ | 普通目录 | context-only；直属sys.rs与sys/，按必要连接读取选择器及既有Unix查询实现 |
| event/ | 普通目录 | 唯一正式直接子目录ZS1-402；物理6文件2目录与其候选库存逐项相符 |
| style/ | 普通目录 | 仅库存上下文；直属attributes/content_style/styled_content/stylize/sys/types的6文件2目录，不借父层报告接受 |
| terminal/ | 普通目录 | context-only；直属sys.rs与sys/，按必要连接读取选择器及既有Unix状态/查询实现 |

file index在src的冻结直属文件数为0；folder index的直接子项只有402。cursor/style/terminal均无本冻结分支的正式子目录依赖，因此不为它们伪造目录报告或凭据。冻结闭包是099→400→401→402→403→404(crossterm)→405(vendor)→091(root)；已接受099/400/401不允许绕过402，403也不提升404/405/root。完整第三方归档库存与本次冻结子集语义范围分开。

## 完整读取链及有限上下文

prior402整包原样封存，manifest `c401bb83e60305fb3439b109d8cf2a12200791bc8305365295c196f094b93fe2`；它包含原401/400包、已接受400/401的完整receipt与artifact链，以及099当前655行的精确连续读取链。本轮将402的五份新完整context和二十份复用context逐字对比主库，共25份全部相同；不把不变源码说成重新读过，也不要求先接受402才承认其真实完整读取事实。402是否接受仍须独立凭据。

本轮完整新读八份context共1892行：command.rs295、macros.rs378、cursor.rs504、terminal.rs568、tty.rs54、ansi_support.rs46，以及cursor/sys.rs20、terminal/sys.rs27。cursor.rs分1–270/271–504，terminal.rs分1–290/291–568，其余整份连续读，共10块；原始字节、完整[0,EOF)与块hash均保存。先读这八份是为回答event的输出命令如何执行、查询如何接入、raw/reset分别由谁负责；不是为接受其它模块。

clipboard.rs、style.rs和style/只作物理库存、lib.rs导出关系记录，没有声称语义全读。Windows cursor/terminal底层实现与外部WinAPI库也未整树阅读；报告仅解释实际读过的选择器、公开分支及ansi_support，不能把当前Unix证据推广过去。这个明确范围比“src全部已读/全部已接受”的说法更可复核。

## crate级模块、feature与映射

lib.rs将Command、ExecutableCommand、QueueableCommand、SynchronizedUpdate从私有command模块对外导出；cursor/style/terminal/tty是public模块，event受events门控，clipboard受osc52门控，ansi_support仅Windows目标公开，macros模块虽crate可见但macro_export的宏在crate根可用。Windows目标关闭windows feature会compile_error。feature与目标cfg两个维度都要成立，不能把默认features列出windows当作Unix运行Windows后端。

| 连接 | 具体作用 | frozen event关联与边界 |
|---|---|---|
| 根Cargo patch→vendor crate lib.rs | 从本地crossterm提供公共API | 当前默认events路线接到mio41321e，Cargo本轮完全精确复用，无构建 |
| lib.rs→event.rs→event/ | 暴露poll/read、类型及可选EventStream、限定reset | event/402是本目录唯一正式子项；event.rs本身仍context |
| event.rs的各Command→command.rs→宏/Write | 启停mouse/focus/paste/keyboard flags写给终端 | 输入读取不会自动执行这些输出命令，调用者负责配对与错误处理 |
| cursor.rs::position→cursor/sys.rs→Unix查询→event reader | 查询坐标响应使用内部Filter | 受events门控，与普通输入共用reader，reset前必须停查询 |
| terminal.rs→terminal/sys.rs→Unix termios/size | raw状态、尺寸、增强能力查询 | raw模式与事件缓存是两个不同owner，SIGWINCH的size慢路径仍存在 |
| command::sync_update↔terminal Begin/End | 输出同步帧指令 | 不持有event reader锁，不是输入队列事务、UI持久化事务或自动恢复guard |
| tty::IsTty | 根据借用fd/handle返回bool | 不获取读者所有权、不改raw/非阻塞flags、不保证终端能力 |

映射到Zenpi时，这层提供的是终端事件和输出控制原语。TUI收到事件后更新编辑状态、布局或项目owner，何时读/何时绘制/何时提交由上层控制；本crate不持有当前项目、草稿、session、HTTP或任务owner。该映射复用已接受099和402读取的限定合同，不新读/接受整个Zenpi TUI、headless或pi-mono仓库。

## 输入状态与输出状态不能合并理解

event.rs持static Mutex<Option<InternalEventReader>>，reader持过滤events/skipped和一个选中的source，Unix source再持Parser partial/已解码队列、readiness tokens、fd/信号与可选Waker。这些状态随对象存活跨调用保留；其顺序、错误和EOF边界按402完整报告复用。pub poll先把等锁耗时扣入预算，read可持锁无限等目标；正常匹配/超时恢复暂存，异常路径并非无条件无损；查询响应是InternalEvent专用变体，不是普通用户按键。

command.rs不维护另一个全局命令队列。QueueableCommand blanket实现直接对调用者的io::Write调用write_command_ansi；所谓“queue”指写进该writer/底层缓冲，是否已经下沉取决于writer，不是把Command对象留在crate中稍后执行。ExecutableCommand执行queue成功后flush。Windows上若command不支持ANSI，先flush已有writer缓冲再执行WinAPI以避免明显乱序；WinAPI可能作用当前console而非给定writer，所以不承诺所有平台副作用都只落在该writer。

queue!宏以writer.by_ref开始，用and_then顺序链接每个Command，前一个失败后后续表达式不继续执行；execute!再在成功后flush。已有成功写出的字节不会回滚，部分写入/flush失败也不撤销终端副作用。execute!的writer表达式会用于queue和最终flush，不应从宏名推导任意有副作用writer表达式只求值一次。输出flush与读取/清空OS输入队列无关；把flush理解成reset会破坏输入交接边界。

write_command_ansi使用fmt::Write适配io::Write::write_all，记录底层io错误并在fmt错误时返回；若Command自行返回fmt::Error而适配器没记录io错误，代码panic，不能笼统写成所有Command异常都io::Result。对&T的Command转发保留平台能力/执行方法。impl_display调用execute_fmt：ANSI路径格式化序列，Windows不支持ANSI时可能直接执行WinAPI并把错误折为fmt::Error；因此这些Command的Display在所有平台都纯文本、无副作用的承诺不成立。普通event枚举的Display与这里的Command Display不是同一含义。

csi!只是拼接ESC[字面量；osc!实际以ESC反斜杠终止，尽管注释写BEL，以代码为准。terminal::SetTitle又有自己的BEL格式。本次只读这些实现，未改模板/转义或接受clipboard实现。

## 同步更新、屏幕命令和释放边界

SynchronizedUpdate::sync_update依次queue Begin、调用FnOnce closure、execute End，然后返回Ok(result)。closure返回类型T是泛型，若T自身是Result，End仍会在closure正常返回之后执行，外层得到Result<Result<…>>；但closure panic没有RAII/finally保证End执行。Begin写失败不调用closure，End/flush失败使外层Err覆盖返回值；这个结构不是数据库式回滚、不是线程互斥，也不保证写入原子性。Begin/End在Windows强制ANSI=true，未知终端可忽略它们；源码中“alternate screen”相关注释不能把同步帧命令当成Enter/LeaveAlternateScreen。

terminal.rs的Enter/LeaveAlternateScreen、line wrap、Clear/Scroll/SetSize/SetTitle都是显式Command。Unix一般写对应转义；Windows部分走ScreenBuffer或sys函数。调用者必须真正execute/queue并处理输出错误，构造/Drop零大小Command不会自动恢复屏幕、光标或raw。Clear/Purge清的是显示缓冲/历史，不是event Parser、输入队列、用户session或磁盘日志。SetSize发请求，不保证每种终端都产生同样的Resize或即刻完成；WindowSize像素字段可能不可靠，不能将尺寸请求等同于实际读取结果。

cursor.rs公开position受events门控，其它Move/Show/Hide/Save/Restore等命令保持可用。绝对坐标在编码时加1，MoveTo(u16::MAX,…)等极值缺显式饱和/校验，不能声称所有u16输入都安全同义；相对移动的0值在不同命令/平台分支处理不同。Save/Restore在终端全局状态中生效，不是Rust中独立cursor对象或项目快照。某些Windowslegacy blink/style分支直接Ok(())，API成功不总等于可见行为发生；本轮没有实际Windows验证。

terminal public raw函数只是转发sys，Unixsys保留另一把Mutex<Option<Termios>>：is_raw查本库登记，重复enable不引用计数，仅成功修改后登记旧mode，disable恢复成功后才清。它不拥有reader，不等于外部进程未动过termios。cursor/sys选择器仅在Unix+events编译查询；terminal/sys仅events门控增强查询而raw/size持续存在。既有完整Unix查询阅读证明poll/read与普通事件竞争同一reader；两秒poll反复重试或后续无时限DA read使整次查询不保证两秒完成。

reset_event_reader只在Unix且not event-stream导出，try_lock失败立即WouldBlock，成功take并drop旧reader，下次懒建。它清内部预读/partial而不flush OS、不恢复raw或屏幕模式；Drop过程中也没有上层草稿持久化动作。正常Resize不reset；外部editor交接需要调用者停读者/查询并协调终端模式。没有一个crate级Drop guard会自动替所有消费者完成这组操作。

## tty检测、Windows能力和取消语义

tty.rs的IsTty借用AsRawFd/AsRawHandle，Unix libc isatty或rustix借用BorrowedFd，Windows GetConsoleMode，返回bool；错误被折为false，没有新fd生命周期、输入读锁或termios修改。本方法回答“该句柄是否被识别为终端”，不能回答支持paste/kitty/同步更新，也不能让两个读者安全共享同一终端。

ansi_support.rs为Windows的Command默认能力检查服务。Once初始化中先尝试对当前输出Handle启用ENABLE_VIRTUAL_TERMINAL_PROCESSING，成功或TERM存在且不等于dumb即记支持；结果存AtomicBool并缓存。此检查可修改console mode，有初始化副作用，源码没有恢复旧模式的Drop；后续句柄/TERM变化也不会自动重算。源注释说先查TERM，实际表达式先调用enable_vt_processing再短路判断TERM。它是粗粒度能力判断，不证明每条ANSI协议都支持，更不是输入source/waker的选择器。

事件取消仍由子项链负责：OS Waker让source返回Interrupted，经reader变false；EventStream有executor和底层poll两种waker，Drop置shutdown并尝试wake但未保存JoinHandle/join，错误处理界限按402保留。命令输出路径不提供取消token、线程终止、timeout或panic安全清理。本目录不能把raw恢复/End同步帧/输入wake任意一种操作当作项目runtime shutdown，亦不负责非合作provider清理或持久化确认。

## 测试阅读、行为证据与尚未覆盖

完整macros.rs源测试在Unix使用FakeWrite/FakeCommand验证单/多命令、尾逗号与flush状态；Windows测试只断言writer或WinAPI模拟stream其中一个有内容，源码TODO尚未核验选择的sink与实际能力一致。这些源测试不能证明真实console处理或所有error路径。cursor.rs的六个真实位置/移动测试均#[ignore]；terminal resize测试也ignore；raw_mode测试若enable失败直接return，所以即使某历史cargo汇总pass也不必意味着实际切过raw。以上全部只是本轮阅读，未执行或登记新通过数量。

本目录完整复用402的输入测试语义与已接受099真实反例，但不改写其归属：旧57f4/1afa/41321e各自读边界、公共ESC PTY前后均通过的反证、144/68 Rust与6891/31ea历史各有自己的身份。prior402还记载主控401 review中的521c/92f95结果，这不是本轮产品执行。不为并行Gantt或其它产品文件漂移重封旧包，不扩展Windows/use-dev-tty/样式/clipboard或全crate测试矩阵。只核对本次相关源码和目录依赖，未运行旧runner、Cargo、runtime、PTY、预算或G-STAGE。

静态限制按真实所有权保留：输出部分失败无回滚；同步更新无panic guard；raw登记不等于外部状态；query与普通事件共享输入；reset不保证全部资源在统一时限内释放；跨平台Command能力与终端真实效果未全验证。库中的数据结构/serde能力不建立项目持久化schema或恢复事务。src库层不写session、journal、WAL、HTTP或用户草稿，返回Event和发终端命令都不等于用户请求已提交成功。

## 候选门禁、交接与回滚

403仅整合冻结event链与crate公共接口。402尚未有接受凭据时，本报告保持待依赖候选；若封包前或后主控独立接受402，应附实际时点receipt，不能改写initial-authority或旧402包。本包始终accepted=false，即便直接子项接受也仍需403自身独立语义审阅。404/405/root及未纳入文件目录不随它接受。

离线verify只核对物理库存、冻结映射、完整新读连续块、25份精确复用context及099读取链/已接受400与401凭据、实际402状态和唯一报告patch，校验不充当语义审阅或产品验收。回滚只撤回403报告，不递归撤回子项或覆盖源码、会话、claims、authority。

所有新文件限新私有.ops，主库、构建输入、旧ready和状态台账只读；worker HEAD与94条non-.ops dirty保持。完成候选封包后停止，不新建session/task/subagent，不自行继续404或其它目录。

## 封包权威时点与完整读取索引

权威快照固定于 2026-09-12T17:43:56.333337+00:00，封包 2026-09-12T17:46:40.251406+00:00。snapshot `c1975dca162b2c505eee25fb619a8a3256c783f0afadae4a3dea6b02612649ef`；402 master receipt存在=False，dependency_accepted=False；403仍[ ]。该时点初始缺失和实际状态都保留，后续台账或接受进展由主控另附凭据，不用并行全树变化重写历史。正式403报告尚不存在，33份新读/复用context与mio共34份相关身份一致。

|方式|完整文件|行数|完整字节范围|SHA256|
|---|---|---|---|---|
|新读|vendor/crossterm/src/command.rs|295|[0,10943)|`0cefa31bc00f5e1f11fdae16c6031fe46d397bb8c1990cf55dc3b9e83d29e3ce`|
|新读|vendor/crossterm/src/macros.rs|378|[0,12627)|`cb09a33ea7affaad7171b5f2325bbc78aad041cd719450a1b6d44e07793c215e`|
|新读|vendor/crossterm/src/cursor.rs|504|[0,15481)|`0d4ed77fca1b318d0c954cc01fb6694d72b0fa3e09bc24dd88b69ab565616007`|
|新读|vendor/crossterm/src/terminal.rs|568|[0,18102)|`01af7ff59a6a21090da0316b2688d41dc8dd99425413e1eacd3494065b0449d8`|
|新读|vendor/crossterm/src/tty.rs|54|[0,1594)|`9452b86a2dbaa35eadab5615aa9eed131954bcf2ed9e1f83e7c5b73e57a9bdc8`|
|新读|vendor/crossterm/src/ansi_support.rs|46|[0,1834)|`36b902490d60259d99f88a2b45555d2d9d13f2c344cb3693c6a81442670f632b`|
|新读|vendor/crossterm/src/cursor/sys.rs|20|[0,551)|`447f839bb2968a808f8d5bd763516a1bff9024e38fe4d852c57f3cdb4ee070e1`|
|新读|vendor/crossterm/src/terminal/sys.rs|27|[0,755)|`b77c654d581eb939b00e77d565d950c2f6045eb9b64480b21c1b20538665bb14`|
|精确复用|vendor/crossterm/src/event.rs|1777|[0,63629)|`989c472a11a6b471e663e191fd64337593423de2aa4774344cb246fb975bbd42`|
|精确复用|vendor/crossterm/src/event/sys/unix/parse.rs|1506|[0,55081)|`689b765d67be62703f7935abfa1475d98994987461e413836b54c7936966ce2b`|
|精确复用|vendor/crossterm/src/lib.rs|263|[0,10739)|`ec896da229e4b9b02da3cf444bf1d89e0bc8dc900a29ded2b2a1df086d92b6aa`|
|精确复用|vendor/crossterm/src/cursor/sys/unix.rs|56|[0,1711)|`e048206557e41c2090437d2b2b30c0485d17db9bb90e726e44d2e8d3ec5d5d63`|
|精确复用|vendor/crossterm/src/terminal/sys/unix.rs|326|[0,10684)|`db066f11aa75d6c06515fd600ee38336f230d0f8f9d307663debb6ba835b334b`|
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
|精确复用|vendor/crossterm/src/event/source/windows.rs|100|[0,3656)|`e25bdd13432dfda96401a76f3e2d6aa2a404fbaa4f1b99b55bb0bdec2ac75336`|
|精确复用|vendor/crossterm/src/event/sys/windows.rs|48|[0,1707)|`130d7da62a0af8b1cf7c55bd8bf78fa0272a9a44803704dac6416db993d30d81`|
|精确复用|vendor/crossterm/src/event/sys/windows/poll.rs|86|[0,2545)|`232c69f9c9aa058189a418cdaee82134feaf6ef91f89cea8eaaeb5b4cfd722fc`|
|精确复用|vendor/crossterm/src/event/sys/windows/waker.rs|40|[0,1156)|`a145d8ccf6b7747eb4e9499bdade2616a997539da4f4691f4c14bd4c4d6a472b`|
|精确复用|vendor/crossterm/src/event/sys/windows/parse.rs|378|[0,15566)|`15afbd3b84edfa2a72f9587f591ce9f7dfe99f1cb85db8b45037c0b70c0346ae`|
