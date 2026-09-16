# ZS1-071 — src/runtime.rs 完整 owner 复核

状态：[_] 独立阅读/行为候选，主控未接受。authority 3.1.16；requirement digest `c0262492bc6e3b4d5f56b35bedc7c1d1658854df11c4a975eb612a514cceef8f`。

本次 subject 为主控只读快照 `src/runtime.rs`，912行、33333字节、SHA256 `7a42c428083f8f3edb0019311296b479004ff562b548d7cdc504729f7cbd1f21`。按1–340、341–660、661–912完整逐行读取，字节范围并集为[0,33333)，各段hash见 `read-coverage.json`。没有以函数索引、旧摘要或测试通过代替全文阅读；本次未改任何product源文件。

旧报告为28121字节/hash `2183b622d96a8fdc084784af8f537fa09a4e1703b5ff70de3bc459d29fb05315`。重新计算证明它恰好对应当前文件前763行；因此只把其基线定位作为同hash背景复用，新增764–912行必须重新审阅。旧报告和该前缀原字节均冻结。尤其纠正旧报告“原子状态”的含混表述：当前取消/完成是两个AtomicBool，不是一个保证先到者赢的CAS终态机。

## 职责、owner 与来源映射

本文件拥有两个不同层次：BackgroundRunner是泛型、进程内、单active job调度器；InputBoundaryGate是一个既有job内部下一次model turn之前的可撤销输入边界凭据。前者的pending FIFO不是steer/follow-up持久队列，JobId也不是用户稳定input ID。后者不启动线程、不调用provider、不执行tool、不直接写journal，真正消费/持久化由InputQueue与core完成。

与冻结的pi `agent-loop.ts`（ZS1-010）和 `agent.ts`（ZS1-011）比较，源实现中等待整批工具、准备后输入再检查、steer优先于最终follow-up等语义需要core/InputQueue共同实现。runtime的gate只提供“当前整批未完/prepare未完/取消时不可取boundary”的局部能力。不能把名字中的pending follow-up或单个Gate当成ZS1-101全部完成，也不能从本文件给工具进程回收、provider transport、extension lease、session recovery等其他owner打勾。

当前快照core在创建gate时先new再with_context_parent(turn_id)，之后才使用boundary；headless在Agent成功返回并跨过durable completion后调用mark_completed。这些是已定位的适配器使用约束，不是本文件自行证明持久化完成。TUI也有独立的完成标记调用点；本次没有跑native TUI/PTY。

## 逐段职责与全部控制分支

| 行 | 实现与分支 | ownership / 限制 |
|---|---|---|
| 1–47 | 模块契约、默认shutdown grace 1s、上限30s、JobId新类型/get | 不承诺强杀任意Rust代码；ID用于一次runner日志关联 |
| 49–99 | CancellationToken::new/cancel/is_cancelled/mark_completed | cancel先读completed，已完成返回false，否则swap cancelled；is_cancelled同时要求cancelled=true且completed=false；mark_completed无条件置completed |
| 101–134 | RuntimeConfig/default/normalized | 默认command32/event128/pending32/poll10ms；三个容量均至少1、poll至少1ms；不存在“0禁止排队”的配置语义 |
| 136–193 | SubmitError Display/Error、JobOutcome、RuntimeEvent | QueueFull/Closed有transport与worker两处来源；Accepted与Started、Completed与Closed含义分别独立；Cancelled不是rollback |
| 195–234 | Command、Active、ShutdownState、JobExecution | active持有独立token、oneshot接收器和join；deadline剩余时间saturating并取poll上限；返回结果与panic分开 |
| 236–306 | BackgroundRunner字段、Debug、spawn | 两个sync_channel；闭包Arc；WorkerClosedGuard负责全部退出路径的closed位；主worker线程spawn失败直接expect panic，无Result型错误返回 |
| 308–350 | try_submit/try_cancel/try_shutdown/try_shutdown_with_grace | try_send非阻塞；Full或Disconnected映射明确；try_submit先fetch_add再发送，失败仍消耗ID；不会等待worker接受；grace在入口夹到30s |
| 352–374 | next_event/recv_timeout/try_next_event/join | 三种接收包装；裸join要求调用者先drain Closed，不能作为任意状态的非阻塞close |
| 376–421 | shutdown_and_join系列 | 入shutdown命令时如command满就同步消费event腾空间；随后继续drain直到Closed/closed位/断开，再join；已消费Closed也可安全调用；丢弃这些被helper消费的事件是该API的明确取舍 |
| 424–441 | Drop | best effort零grace请求，不阻塞，取走join后detach；命令队列满时发送可失败；不等同于显式shutdown_and_join承诺 |
| 443–509 | worker循环首先检查done | 成功收到结果先join再classify，再finish_active；若停止则返回，否则pending.pop_front启动下一job；done断开单独映射Panicked并走同样收尾；Empty才去读命令 |
| 511–542 | shutdown期限与读取命令 | 已过期限且active存在则丢Active，关闭done接收器、detach job、发Cancelled及剩余关闭序列；active时recv_timeout，无active阻塞recv；命令端断开转默认grace shutdown |
| 544–589 | Submit三分支 | idle且未停止：Accepted(false)后start；active且未停止：pending满则Rejected QueueFull，否则push并Accepted(true)/Queued(depth)；stopping时读到的Submit发Rejected Closed |
| 590–616 | Cancel | 命中active则token.cancel并发CancelRequested（不检查cancel返回值，因此已完成/重复取消也可有该通知）；命中pending则删除并发CancelRequested、Completed Cancelled；未知ID无事件 |
| 617–638 | Shutdown | 新deadline仅能缩短既有期限；有active置取消并发请求事件；无active则cancel_pending后Closed并返回 |
| 640–674 | classify_execution/finish_active | token当前有效取消优先，甚至覆盖闭包Err/Panicked；否则映射Succeeded/Failed/Panicked；完成事件先于shutdown时pending取消和Closed；所有发送失败立即退出 |
| 676–710 | cancel_pending、grace归一、WorkerClosedGuard::drop | FIFO逐个取消，发送失败停止；guard不依赖event通道，退出时Release写closed |
| 712–763 | start_job/emit | 新token/容量1结果通道；job内部catch_unwind；线程spawn失败发Rejected Closed，不发Started；正常先保存Active，再发Started；emit是阻塞send |
| 765–805 | InputBoundary字段/accessor/is_current、Gate状态 | boundary复制model ID/parent/would_stop并共享epoch/cancelled；is_current要求未取消且epoch一致；外部不能直接伪造私有字段，但能通过公开Gate创建合法形状凭据 |
| 806–831 | new/with_context_parent | IDs调用protocol同一验证；起始空tools、未prepare、epoch0、未cancel；builder设置parent不递增epoch |
| 833–855 | begin_tool_batch | 空/超过32/已有tools/正在prepare拒绝；所有ID合法且去重完成后才递增epoch并替换集合；失败不半写tools、不撤销旧boundary |
| 857–887 | tool_completed/begin_prepare/finish_prepare | 完成未知或重复tool拒绝；每次有效完成epoch++；未整批结束不能prepare；重复begin、未begin的finish拒绝；有效切换均使旧boundary失效 |
| 889–912 | boundary/cancel | 存在tool、prepare或cancelled时拒绝；否则生成快照。would_stop是调用者给的布尔值，并非gate探测agent状态；cancel永久置位，没有reset；cancel后部分状态修改方法仍可返回Ok，但始终不能再取boundary |

## 数据流与故障语义

请求先经过command通道，再由worker决定Accepted/Rejected，accepted的active/pending才有已建立的job生命周期。command容量与pending容量不是同一上限。事件通道的bounded含义是内存受限；它是阻塞send，因此host必须持续drain。Started发送前job线程已经spawn，host看到Started的时刻不等于副作用刚开始。done优先于新命令，已经完成的结果可先于排队取消被分类。

shutdown grace从worker实际处理Shutdown时开始，不能覆盖此前事件背压或命令等待；非协作job到期detach后可继续执行任意副作用，迟到结果不能回到已断开的done接收器。这里的“可能短暂继续”不是一个job退出时间上限。已有测试自己释放非协作fixture；不应把fixture最终释放误算为runtime强制回收。

源码中job线程spawn失败仅有Rejected事件，错误原因被压成Closed。静态读取还表明：若启动pending失败但Rejected发送成功，start_job返回true且active为空，剩余pending没有在该分支立即继续启动；下一轮可能等新command。这是未执行的系统线程耗尽分支，不宣称已有可重复故障证据或修复。本次也不声称覆盖主worker spawn失败、done sender异常断开、JobId u64回绕、全部内存顺序交错。

Gate的撤销是epoch/cancel状态的协议，不是Rust对象生存期撤销。begin_tool_batch/begin_prepare/finish_prepare/tool_completed会使旧凭据过期，但with_context_parent和Drop都不会。它与InputQueue的防重复model-turn应用、真实session绑定是不同层；仅Gate不能证明model-turn所属session或would_stop的真实性。

## 实际行为证据

本次在未修改product的完整只读输入快照中执行：`tests/runtime.rs` 12通过；`tests/stage1_input_queue.rs` 17通过、1独立进程helper在顶层标ignored；新 `target071_review_probe.rs` 4通过。合计33个顶层通过、0失败，不把parent调用的helper再加一次。真实线程、sync_channel、文件操作和session journal被执行；provider只用已有echo fixture，无外部provider。现有panic测试的stderr中“fixture panic”是预期catch_unwind路径，不是套件失败。

新增探针逐项观察如下，均是对当前行为的陈述，不是把缺口测试绿化后宣称功能修好：

1. event_capacity=1时让Accepted占满event通道，job线程已运行、worker阻塞发送Started。try_cancel返回Ok后25ms，job token仍未取消；drain后才置位。再预先排入Shutdown及一个tail Submit，tail的try_submit成功返回JobId(2)，但直到Closed都没有该ID的Accepted/Rejected/Completed事件。只有“进入command通道”，没有worker承诺。这一点比旧摘要“拒绝有显式事件”更窄；读到Submit的stopping分支会Rejected，但Shutdown后未读到的command可随receiver释放而消失。
2. job先实际观察到token.is_cancelled=true，再调用mark_completed，则is_cancelled变false，最终Succeeded(7)。它不是仅阻止“晚到取消”的通用原子仲裁；adapter必须只在真实语义完成后调用，不能先标记再执行未完成工作。
3. max_pending=0、poll=0的配置允许一条pending（depth1），下一条被worker明确Rejected QueueFull，证明归一化行为而非零容量模式。
4. 先取parent=None的boundary，再对Gate调用with_context_parent("new-parent")，随后提交非法重复tool batch并Drop gate；旧boundary仍能把真实journal中的队列项Applied，parent仍为None。当前core正常构造顺序避免这个builder用法，但公共能力不承诺builder/Drop撤销；要撤销应明确cancel或有效epoch状态变更。

现有runtime测试另覆盖off-thread admission、同Agent跨job保留session、FIFO、pending饱和、active/queued取消顺序、late marker、panic后worker存活、已消费Closed再join、双channel饱和shutdown drain、非协作job bounded detach。InputQueue套件另覆盖真实双文件工具批次后边界、prepare前后至多一次应用、lane优先级、编辑revision/取消/容量、过时session writer、跨session拒绝、独立进程恢复、截断尾记录和畸形事件。这些是交互证据，不能据此把input_queue.rs或其目录的全文阅读项一起接受。

## 结论与候选边界

071提供一份当前hash完整阅读报告和可重放行为包；没有产品补丁、没有把上述API局限自动扩展成跨owner修复。主控需要在host职责中保留持续drain、区分transport admission/worker acceptance、正确调用mark_completed、显式撤销boundary四项约束。代码中未证实的系统资源故障分支保留静态标注。acceptance、ledger、其他文件和所有既有候选保持不变。

---

## 3.1.20 完整复核补充（2026-09-12，worker candidate）

前12129字节是旧3.1.16完整报告，SHA256 `56782ccc4a357807c7cc88a1feaa734a703ed07eac5d04b4e6285888da369517`，逐字节保留。其“本次执行”等表述属于旧执行记录；本补充是本轮的完整基线/当前复读与集成边界，**新测试、新Cargo、新PTY、新HTTP、新安装均为0**。当前authority为3.1.20，requirement digest `8d525b351d066ce9b0487a485337647d47233275782e4f4d154423a623fe1645`。仅处理ZS1-071 `src/runtime.rs`，不改产品、主库、权威状态或其它文件学习项，不生成主控验收。

### 两份输入均完整有序阅读

| 输入 | 身份 | 此轮独立完整阅读 |
|---|---|---|
| Blueprint冻结基线 | 28121B /763行；`2183b622d96a8fdc084784af8f537fa09a4e1703b5ff70de3bc459d29fb05315` | 1–300、301–560、561–763，先完整读完基线 |
| 当前主库只读快照 | 33333B /912行；`7a42c428083f8f3edb0019311296b479004ff562b548d7cdc504729f7cbd1f21` | 随后独立读1–340、341–660、661–912，未以同hash替代阅读 |
| 旧071实际执行subject | 33333B /912行；同当前hash | 仅字节身份复用，不重新执行 |

两份输入的源字节、行范围、连续byte范围和分块hash位于新 `worker-target-review-3.1.20` 目录。基线恰好是当前前28121字节/763行，当前只追加149行/5212字节的InputBoundary/Gate。BackgroundRunner、JobId、取消/完成及shutdown代码没有相对基线变化。下面所有行号同时适用于当前和基线的1–763部分；764之后仅当前存在。

### 全文件职责、状态与错误分支复核

前文完整逐段表仍适用，当前重读核对如下：1–134的配置、两个取消原子位与ID；136–234的错误/事件/active/done/deadline；236–306的线程与双通道构造；308–441所有提交、接收、关闭及Drop入口；443–638完成优先、命令分支、pending与shutdown；640–763分类、终态顺序、guard、job spawn与阻塞emit；765–912输入boundary的epoch/cancel、工具批次和prepare状态。没有遗漏默认值、归一化、Display/Debug、线程spawn失败、通道断开、取消未知ID、重复shutdown和非法gate状态等分支。

本文件调度的是泛型`I -> Result<O,E>`，请求被移进command通道和pending队列，处理闭包以Arc共享，job token和一次性结果通道每次新建。它不认识project/session/Agent、authorization、durable checkpoint、provider、journal或HTTP。每个runner最多一个active job；不同runner可并发，但是否共用同一Agent以及如何加锁/拒绝忙碌请求由host决定。`pending follow-ups`是普通FIFO job，不能等同于对话steer/followUp持久输入队列。InputBoundaryGate是在已有job内的局部凭据，不提供第二个scheduler。

### JobId、排队与取消路由

`JobId`（38–47）仅包一个u64，派生的Eq/Hash不含runner身份；`spawn:301`每个新runner都从1开始，`try_submit:309–316`在try_send前fetch_add。失败提交也消耗ID；u64回绕无显式防护，本轮没有执行回绕。两个runner各自的首个成功提交都可得到数值1；同一JobId类型允许误传给另一个runner。`try_cancel:320–326`没有校验来源，worker的590–615只按本地active/pending数字匹配。未知ID静默，不产生否定回执。

因此新的provider/control双runner若共用host状态表，键至少需要保留**runner实例/通道身份与JobId**，还需绑定实际project/session/request身份。按`id.get()`单独存表或把一个runner的取消发给另一个runner可作用于错误作业。这是当前源码直接可见的合同；历史33项没有双runner碰撞实验，本轮也不新增实验。

`try_submit Ok(id)`仅意味着进入command通道，worker在544–587才决定Accepted或Rejected。command容量、pending容量、active槽分别存在；默认32/32/1，event容量128，所有可配容量至少1、poll至少1ms，max_pending=0不是禁用排队。worker每圈先读done，再读命令；完成路径会直接弹出pending并启动下一job，然后continue。已排在command通道的Cancel/Shutdown不具有抢占优先级：在worker实际读取前，前一job可以完成，下一pending也可以开始。成功的try_cancel不是“token已经置位”，更不是“工作已停止”。

正常同一runner生命周期：idle Submit先Accepted(false)，然后spawn并Started；active Submit若pending未满先push，再Accepted(true)、Queued(depth)；pending取消删除项并CancelRequested→Completed(Cancelled)；active取消只置token并发CancelRequested，等待结果或shutdown期限。CancelRequested不检查token.cancel返回值，所以可在重复取消或mark_completed之后仍出现；Completed的结果也未必Cancelled。线程spawn在Started发送之前已经发生，Started不是允许副作用开始的授权屏障。

旧真实probe保留一个重要窄边界：event_capacity=1让Accepted占满通道，job已经执行而worker卡在Started发送；try_cancel虽成功入队，25ms后token仍未置位。drain后取消生效。另一个tail Submit在Shutdown后成功进入command通道却直到Closed都没有任何该ID事件，因为worker退出时未读它。只有“已经被worker接受的pending”有正常shutdown取消序列；不能给所有transport-admitted请求许诺终态。host须在runner关闭时处理仍未被确认的请求，而不是无限等待其Completed。

### mark_completed 与晚取消的精确边界

49–99用两个AtomicBool，非单一CAS终态机。cancel先Acquire读completed，若未完成则对cancelled做Release swap；is_cancelled读取cancelled且要求completed仍false；mark_completed无条件Release置completed=true。该标记不会发送done、不加入线程、不释放锁、不做持久提交，也不限制调用次数或调用时机。

普通结果分类（640–652）先看有效取消，再把Ok/Err/捕获panic分别映射Succeeded/Failed/Panicked。因此“先cancel，job已观察true，再mark_completed，最后Ok”会得到Succeeded——这正是旧probe实际验证的观察，并非本轮推测。mark_completed也不会把Err/panic变成Succeeded：它只让取消不再覆盖原结果。不要把取消/提交竞态的先后由这个标记来仲裁；调用者必须先建立真正的语义提交事实，再用它保护返回结果。

另一个此前表述需要明确收窄的路径是**grace到期**（511–523）：若done仍未可读，worker直接drop Active并产Completed(Cancelled)，这里**不调用classify_execution，也不读completed**。所以即使job已经mark_completed，只要仍未返回并进入这一路径，结果仍可被丢弃。done检查先于deadline检查；若done已经可读，则先join再分类，已超过deadline也不走detach分支。这里的“标记后但未返回遇deadline”是代码推导，旧late-marker测试366–411仅执行普通try_cancel、释放fixture让它返回后再shutdown，未覆盖此组合。

对`/new`这样的持久提交操作，runtime Cancelled不能被解释成“新会话没有建立”或“可以盲目重试”。操作可能已过提交点而结果尚未被host接收；应由外层已有持久transition/request身份恢复实际状态。仅本文件不能验证主控正在集成的实现是否已经满足该要求。

### try_shutdown、事件接收、Closed、join 与 detach

| API/阶段 | 当前实际行为 | 接入约束 |
|---|---|---|
| try_shutdown / with_grace（332–350） | 非阻塞try_send，Full要调用者处理；默认1s，最多30s，可0。没有提前设置host-side closed，也没有立即改变token。 | 成功入队不是worker已进入停止态；调用后紧跟Submit仍可能入队。 |
| worker处理Shutdown（617–638） | 此时才用Instant::now建立deadline；重复请求只能缩短已有deadline；有active先cancel并发事件，无active取消pending、Closed、返回。 | grace不覆盖入队等待、之前的事件背压和worker未调度时间。 |
| emit（761–763） | bounded sync_channel **阻塞send**。 | host必须持续drain。双runner事件顺序只在各自通道内成立，不存在跨runner全序。 |
| next_event / recv_timeout / try_next_event（353–365） | 分别是无期限阻塞recv、有期限recv、非阻塞try_recv。 | 主循环不能为了等control事件而停止读取provider/输入；轮询多源需公平性。next_event得到断开也不等于有Closed回执。 |
| done结果（460–510） | 先join job，再产Completed；异常done断开独立归Panicked，不经过token分类。 | 常规Completed表明这个job线程已经join；grace到期的Cancelled例外。线程join忽略内部返回值，job panic由catch_unwind捕获路径区分。 |
| grace到期（511–523） | 丢done接收器与JoinHandle，detach job，再active终态→pending取消→Closed。 | 阻止迟到结果回到该runner，不阻止job继续任意副作用、持锁或持有Arc<Agent>。没有可证明的“短暂”退出期限。 |
| Closed / closed位 | Closed是正常关闭事件，worker guard在全部退出/展开路径置closed位。发送失败可能直接退出而没有Closed。 | Closed或guard只描述调度线程/结果通道，不证明全部被detach作业退出。 |
| join（368–374） | 直接join调度线程，不发Shutdown、不消费事件。 | 先join后drain可卡住；若正常drain已见Closed，再join通常安全，但不要把它当全局无阻塞API。 |
| shutdown_and_join（378–421） | 命令满时边消费事件边重试，再drain直到Closed/closed位/断开并join。其消费的业务事件被丢弃。 | helper解决本runner双通道互堵；不会帮host保存终态业务回执、drain另一个runner或释放调用者持有的Agent锁。 |
| Drop（432–440） | best-effort try_send零grace，忽略Full/Disconnected，再detach调度JoinHandle；字段随后释放。 | 不提供join或必达取消承诺。event接收器释放导致worker发送失败而退出时，active可直接被丢弃，未必已经token.cancel。 |

关闭时间界限也需要准确表达：grace限制的是worker**检查到期限时**放弃active所有权的策略，不是任意泛型闭包、事件消费者、资源析构与线程join的严格wall-clock deadline。done可读分支先join而不再检查deadline；Rust线程退出/析构若仍阻塞，该处也没有独立超时。这是静态边界，本轮没有注入线程析构阻塞或操作系统资源耗尽。不能从fixture在500ms内返回推出所有泛型任务在该时间内退出。

旧非协作测试533–577确实测量25ms grace的shutdown helper在500ms内返回，之后设置release AtomicBool。原报告“fixture最终释放”应理解为**发送了允许结束的信号**：该测试没有保留detach线程JoinHandle或等待退出确认；所以不能额外声称它已经证明脱离线程完成join。旧饱和测试493–530把cleanup放到另一线程，并用2s recv_timeout检查helper返回；它不是生产双runner主循环或真实`/new`提交回执测试。

### InputBoundary/Gate 与恢复范围

当前追加的765–912部分保持旧报告的完整结论：凭据保存model_turn_id、context_parent、would_stop和epoch快照，共享epoch/cancel原子位；is_current只校验epoch与cancel。begin_tool_batch先完整验证非空/≤32/合法且去重，再变更epoch/集合；失败不半改。tool_completed、begin_prepare、finish_prepare严格检查顺序并使旧epoch失效。只有无tool、非prepare、未cancel时能取boundary；would_stop完全由调用者传入，gate没有读取Agent。

with_context_parent只改builder字段，不撤销已发凭据；Drop也不撤销。cancel永久生效但不会拦住全部其它状态变更方法返回Ok。旧probe在真实SessionStore/InputQueue上证明旧parent=None boundary经builder变更、非法batch、Drop后仍可被应用。该场景只证明局部能力边界，不能把InputBoundary看作跨session授权或按请求ID恰好一次落盘。这里没有任何磁盘持久化/进程重启代码，BackgroundRunner的active/pending/JobId/标记也全是内存状态；恢复必须交还session/input/host owner。

### 历史证据原样复用，当前新增验证仅为字节校验

旧 `.ops/target071-ready/manifest.json` SHA `006f448bfbcaf5a5ddfcec73309889e0314b175891d8704c6c4fdf32678b7a3f` 的16个payload逐个hash/bytes复核通过；其replay-inputs归档139个文件（138个原输入+1个probe）也逐个验证，未执行unpack runner或Cargo。整个旧包按原字节保存在新证据的historical-target071-ready.tar.gz，原包不改。

历史执行为runtime12、stage1_input_queue17、target071_review_probe4，共33顶层通过、0失败；1个ignored是由恢复父测试调用的独立进程helper，不重复加数。旧原始命令、toolchain版本、stdout/stderr、snapshot receipt和既有报告均保留。4个probe已完整读完，shutdown相关既有测试代码按上述范围复查。其真实范围包括本地线程/通道、特定真实journal/文件操作、echo Agent和该input队列独立进程恢复；不包括真实HTTP/provider、双runner跨项目继续、current `/new`主控交互、所有系统失败/内存交错。相同runtime hash只允许复用其原结论，不能把旧138文件环境和当时测试成功改称当前生产集成成功。

当前host正准备使用独立控制worker这一背景来自主控指令。本轮没有学习或改写host其它文件，因此“保留runner命名空间、持续drain、同Agent所有权/锁/授权、提交后取消恢复”的内容是从runtime合同导出的**接入约束**，不是对未读新host代码的缺陷断言，更不是新功能通过证明。JobId、阻塞send、detach及marker与deadline差异已即时告知主控。

交付仅更新本文件标准learn报告并新增071证据、离线verifier及可逆字节补丁包。主控当前报告不存在，因此ready的canonical报告是明确的new-file delta；本worker提交则是在旧报告后追加，二者目标最终字节相同。离线检查在新临时目录正向应用候选、比较每个payload、反向恢复不存在的基线；它不验证业务行为或生成G-STAGE/master receipt。若主控接收前出现报告，须按实际base另行比对，不能强制覆盖。产品实现和其它学习项保持原样，071仍待主控独立验收。
