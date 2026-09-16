# ZS1-309 · status_indicator_widget.rs 完整单文件复核（3.1.21）

本文件 understand 候选，master G-FILE 待审。唯一产品树交付路径为 `Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/status_indicator_widget.rs_learn.md`；本轮不改产品、主库、authority、claims、索引或接受标记，不接受父目录、整项125或其它文件。run `zenpi-stage1-20260911`，requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`；捕获blueprint snapshot `e4197a9fb55d6f4a3a8ab87ebc631bdfecf3c139fb03c5fecac41bed292274b6`。完整阅读和以下语义结论均绑定本轮快照。

正式源 `/Users/wangweiyang/GitHub/codex/codex-rs/tui/src/status_indicator_widget.rs`，HEAD `b3b3d262787f4902a7449f17d793241a34d311ad`，**15810 B / 440行 / SHA256 e35e3efbd18784b55343366b2001bda08dfc1543f7e4a88700a715562d519cc5**。顺读1–154、155–310、311–440，连续覆盖[0,15810)全部字节。30个语义单元、27个函数定义（17生产、3测试访问器、7源测试），包含全部注释、类型、分支、测试。7源测试完整阅读，18普通断言+3 snapshot调用；执行0。

[正式源](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference309-status321-ready/evidence/capture/codex/codex-rs/tui/src/status_indicator_widget.rs)；[完整阅读账本](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference309-status321-ready/evidence/read-binding.json)；[30逐单元范围/字节/语义](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference309-status321-ready/evidence/source-units.json)；[27函数清单](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference309-status321-ready/evidence/function-inventory.json)；[14操作映射](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference309-status321-ready/evidence/capability-matrix.json)。上下文复制不产生整文件阅读信用；每段实读范围见账本。

## 旧证据完整保留与本轮身份

旧worker2267 `zs1-309-ready` 的88个regular及聚合 `codex-reference-ready` 17个regular完整保留。旧完整报告28091 B/216行/SHA `f8392c09463e9f81fe6b1ea2cbecbb2d9074b77cef07bd45652569cefc5f9813`，本轮按1–73、74–146、147–216全部读；最初1875 B/20行报告 `697590640b31170f46becab07b6db969f11b2da655f8c04be517a13ded9dbfc6` 亦全文读。旧manifest `6df39987202ba51f35d30baa6bf8dca4dcab43aa380d52b1e65aa05c08e90b81`，旧read-log `cd6136a005935fba156c3972e2b4a8ef526898a4acef6dfcaae8fea3986c1a6f`。正式源旧/新完全相同，delta0；不重跑旧runner、静态checker、release或smoke。

31个旧上下文片段中26个在当前对应捕获文件中找到精确同字节，其余5个保留并标记不精确复用；完整记录见[旧新身份比较](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference309-status321-ready/evidence/history-comparison.json)。本轮独立补读了发送失败处理、scheduler接收/合并/退出循环、capitalization实际Unicode规则、主行截断helper、word_wrap_lines全量迭代和worked_elapsed_from；这些使旧报告中仅依赖注释或未读helper的结论进一步明确。

实际main在捕获时的TUI为 **638305 B /15607行 /0deb8e0e13cfdad0aa00e07d79caf573e9aa36095a9bcefb4219ed8f70936557**，不是上一任务stage122候选 b15661a1…；本轮采集时刻 `2026-09-12T23:38:12.743006+00:00`。主控另项正集成122，冻结时逐捕获文件比较实际main并记录漂移，禁止把本候选当当前main或替换已有捕获。目标大文件仅按明确范围学习，不构成085整文件接受。

## 全部源单元

|行|单元|分支与含义|
|---|---|---|
|1–33|模块与依赖|显示组件无任务/project身份；imports连接外部动画、事件、换行和帧请求。|
|34–59|类型与常量|默认详情3行；CapitalizeFirst/Preserve；持有显示字段、运行累计与暂停bool、两个发送句柄和动画开关。|
|60–76|fmt_elapsed_compact|秒→分钟→小时；无24小时回绕；纯u64格式化不计时。|
|77–98|new|Working、空details/inline、hint开启、计时起点now、未暂停；构造不调frame。|
|99–102|interrupt|仅发送Op::Interrupt，不看hint/暂停/焦点，不提供ack。|
|103–107|update_header|原样替换String，不trim/比较/重绘。|
|108–126|update_details|max至少1；原串非空才trim_start并按策略转首字符；空白串可能Some空串。|
|127–137|update_inline_message|先trim再判空；不折叠内部换行。|
|138–142|header|仅cfg(test)借用访问器。|
|143–147|details|仅cfg(test)访问器。|
|148–151|set_interrupt_hint_visible|仅改bool，不关闭动作或请求重绘。|
|152–156|interrupt_hint_visible|仅cfg(test)访问器。|
|157–160|pause_timer|now委托_at。|
|161–164|resume_timer|now委托_at。|
|165–172|pause_timer_at|重复pause无变化，否则累加饱和delta并置paused；不调frame。|
|173–181|resume_timer_at|仅paused→running更新起点并请求一次frame；无嵌套计数。|
|182–189|elapsed_duration_at|暂停时累计，运行时加当前段；Duration普通加法。|
|190–193|elapsed_seconds_at|as_secs向下取整。|
|194–197|elapsed_seconds|now读取实时运行秒数。|
|198–230|wrapped_details_lines|空/width0返回；全量word_wrap_lines后按max截断，再修改末span按chars取数附省略号；不是字节/内存上界。|
|231–235|desired_height|1+转换成u16的详情行数，转换失败按0；width0仍1；极大行数与+1边界不验证。|
|236–290|render|空area返回；动画开则每次请求32ms即便paused；拼接header/elapsed/hint/inline后列宽截断；详情受配置行数和area剩余行双重限制。|
|291–304|测试模块依赖|TestBackend和dummy requester不是实际终端或帧调度证明。|
|305–317|fmt_elapsed_compact_formats_seconds_minutes_hours|10个格式边界断言。|
|318–331|renders_with_working_header|80x2默认状态snapshot。|
|332–345|renders_truncated|20x2 snapshot显示中断说明被截断。|
|346–370|renders_wrapped_details_panama_two_lines|30x3；无动画/hint；直接冻结私有计时字段；两行ASCII详情。|
|371–392|timer_pauses_when_requested|受控Instant5→暂停仍5→恢复8，不是真实等待。|
|393–412|details_overflow_adds_ellipsis|width6/max3，行数及固定末span省略号。|
|413–440|details_args_can_disable_capitalization_and_limit_lines|Preserve/max1/width24，原命令大小写和一行省略号。|

## 状态、计时与取消的实际边界

widget没有运行任务ID、project/session、取消ack或持久化身份。`new`保存调用者给的句柄及动画开关，直接从Instant::now起计，不调度第一次绘制。`interrupt`调用AppEventSender发送CodexOp；已读sender 1–28确认Op不在此重复写inbound session log，发送失败仅tracing::error，不返回结果或改变widget。不能把“已调用interrupt”写成“已停止provider”。显示hint=false、paused=true均不禁止方法调用。

暂停是单个bool：pause已暂停则无动作，否则累加饱和delta；resume只在暂停时改起点并发一次立即重绘。重复同状态幂等，但无嵌套计数或暂停来源。饱和delta防早于起点的负间隔，不校验注入时间顺序；Duration累加未提供总时间上限。as_secs舍弃亚秒；fmt纯格式化到25h及更大hour，不按日回绕。elapsed不是墙上时钟、费用或跨进程连续时钟。

动画与计时分离：非空area且animations=true时每次render请求32ms后重绘，即使paused；关闭动画后本widget不自发周期tick，elapsed只能在其它原因重绘时更新。exec_cell/render 182–196：关动画显示dim的•，真彩色走shimmer，非真彩色每600ms在•/◦切换。resume更换last_resume_at会改变后者相位；shimmer全文件1–80按首次使用的进程级OnceLock起点、2秒周期扫过逐scalar的header，真彩色插值，fallback按强度DIM/普通/BOLD。暂停timer不冻结shimmer。

FrameRequester 1–128实际用unbounded Instant通道，new spawn scheduler，schedule方法忽略发送失败；scheduler收deadline并取最早，经过rate_limiter后deadline分支broadcast draw，所有sender drop后退出。合并draw不等于mailbox有界。32ms仅请求延迟，不是实测帧率；120 FPS来自scheduler注释，rate_limiter独立实现未在本轮阅读或验收。test_dummy创建并立即丢receiver，所以源测试不证明调度成功。读取到测试模块150行仅为相邻上下文，未完整阅读该测试，不计本轮测试覆盖。

## 文本、裁剪、资源与恢复

header直接替换，不trim、比较或redraw。details先排除原空串再trim_start，因此非空全空白可以存Some(空串)；Preserve仍去前导空白，尾空白不在此清除。capitalize_first实际对首Unicode scalar做to_uppercase，可能扩张为多个字符，后面原样拼接，不是grapheme/title case。inline先trim两端再排空，所以全空白变None，但内部换行保留。max_lines仅下限1，三个setter不负责请求重绘，调用方负责。

details初始前缀4列、续行相同列空格，break_words=true；已读word_wrap_lines 793–817确认遍历所有输入行、调用word_wrap_line并收集全部结果，之后widget才truncate。max_lines是显示数限制，不是输入、内存、CPU上界。desired_height与render可分别wrap，存在重复工作；本轮不制造巨量输入或预算测试。

配置溢出时只重写最后一行最后一个span，以chars().take计数再附…，不是按grapheme或显示列宽截断，也不扣除此行其它内容span宽度。前缀UnicodeWidthStr不使后续步骤自动Unicode安全；宽字符/组合字/emoji和小于前缀的宽度属于待测边界，不能在未运行时称已复现越界。主行另走line_truncation 1–100：按span显示宽预扫，溢出预留1列，按char_indices累加列宽切UTF8边界，继承末span style附…。完整span快路径与逐scalar慢路径不构成所有grapheme保证。

desired_height(width0)仍为1，而render(empty area)直接返回。u16转换失败回落0；极端65535行时1+转换值有整数边界，默认3行不等于API输入限制。配置max_lines截断会加…，area.height不足后的take(height−1)不再加省略提示。实际高度是1+details行数，默认最多4行；模块“one line”指spinner/header/inline组合。长header或窄屏能截掉取消说明，inline放在后面只是顺序优先，不能保证hint始终可见。

没有widget专属Drop、外部进程、文件清理、恢复journal或terminal模式切换。上述通道与host决定生命周期；本轮恢复结论仅是读取到的调用顺序，不是终端恢复实验。

## 必要调用方逐段事实

bottom_pane 633–647：存在status才更新header/details并request_redraw，不存在不新建。717–767：set_task_running在false→true时若缺失才new，显示hint，同步inline；false隐藏，hide不改running；ensure重建会获得新起点。838–857：统一执行进程摘要由footer model提供，widget只存summary_text。1130–1173：active_view独占绘制；无view时status在composer上方，status存在时避免重复独立footer行；隐藏widget也意味着render内部请求不会自动发生。

bottom_pane 903–950、1015–1046：新审批/问题/MCP modal前pause，已有view消费的请求可直接返回；active view完成时resume并重新启用composer。399–454：Ctrl-C完成可pop一层后立即complete，普通完成clear整个stack；无view的Esc在Press/Repeat、running、非/agent、无popup且有status时发interrupt，不检查hint bool或mods。widget无法保证所有叠层暂停来源正确，需要调用层验证；本轮不把局部顺序泛化为全modal验收。

chatwidget 2964–2985：undo开始ensure status，隐藏hint并设置标题；结束hide并写结果。1727–1765与3106–3138：读取elapsed给FinalMessageSeparator；3147–3155的worked_elapsed_from扣上次分隔符elapsed，若当前下降则基线归0，随后保存当前。故最终Worked数是相邻区间差值，不能直接等同widget累计秒数。全turn回放、undo或chatwidget并未完整学习。

## 7个源测试逐项审阅

|测试/行|实际验证|明确未覆盖|
|---|---|---|
|fmt_elapsed_compact_formats_seconds_minutes_hours 306–317|10 assert_eq；0/1/59/60/61/185/3599/3600/3661/90123|无Instant、极值或实际时钟|
|renders_with_working_header 320–331|80×2，animations=true，1 snapshot|dummy frame，无真实终端/帧率/inline|
|renders_truncated 334–345|20×2，1 snapshot，esc…|不保证窄屏完整提示|
|renders_wrapped_details_panama_two_lines 348–370|30×3，1 snapshot，关动画/hint，直接改paused/elapsed|未实际调用暂停，ASCII|
|timer_pauses_when_requested 373–392|3 assert_eq，注入5→5→8秒|无重复/嵌套/逆序/真实调度|
|details_overflow_adds_ellipsis 395–412|2断言，width6/max3/末span…|无Unicode整行列宽断言|
|details_args_can_disable_capitalization_and_limit_lines 415–439|3断言，Preserve/max1/width24|无max0/空白/超大行数|

当前3份引用snapshot已全文读取：80列为“• Working (0s • esc to interrupt)”；20列为“• Working (0s • esc…”；30×3为Working(0s)、首行“  └ A man a plan a canal”、续行panama。它们是冻结预期文本，本轮未生成实际屏幕。额外两个queued_messages/macOS snapshot虽保存全字节，当前正式源无同名测试引用，未计为309测试或完整阅读信用。

测试没有覆盖interrupt channel事件、hint false动作、inline、空白归一化、max0/巨大行数、Unicode末span宽度、零area、paused×animation矩阵、嵌套暂停、发送失败或真实刷新。阅读7测试后决定本轮不执行：这是只读逐文件语义复核，没有产品变更；既有断言范围已能确认其证明边界，执行不能补上这些缺项。新Cargo/源测试/PTY/HTTP/release/预算均0；静态verify只检字节/账本/单报告补丁，不提供G-FILE语义接受。

## Zenpi实际操作、差异与后续判据

表中源范围以正式440行文件为准，target范围统一绑定本轮实际main捕获。owner主要为ZS1-125的src/tui.rs状态与host，审批/项目/输入分别属于124/121/122，目录353需另审。每一行都包含现状、差异和可执行判据，判据全部未本轮新执行。

|ID/操作|绑定范围|当前事实|差异与最小行为判据|
|---|---|---|---|
|M01 格式化与运行计时|src/tui.rs L741–792,3800–3814,6687–6784|TranscriptUx已有累计Duration+Option<Instant>；busy显示Ns，无源m/h compact。pause take起点幂等；busy false→true清零，连续true不重置。|已有实现+显示差异；源格式10断言/计时3断言；目标受控顺序5运行/5等待/3恢复应明确5→5→8，未新运行。|
|M02 等待暂停/恢复|src/tui.rs L3787–3797,4143–4149,4198–4220|set_status仅字符串变化才执行；包含大小写敏感Approval即pause，其它状态在busy且无起点时resume。Resolved只追加消息，ToolStarted改变status；先Approval再busy true会重新启动计时。|潜在顺序缺口，未动态复现；同串/大小写/先后顺序、resolved无后续ToolStarted逐一测；不能把文本当typed等待状态。|
|M03 动画与刷新|src/tui.rs L3816–3821,6691–6700,7673–7737,12345–12385,13206–13240|busy时tick%4字符；两host由poll tick+dirty请求RenderScheduler，due才draw。默认poll100ms/frame16ms/min1ms；无源shimmer或widget32ms自调度。|有意架构差异；动画paused矩阵、idle计时刷新和真实draw频率需独立实测；不以常量宣称FPS。|
|M04 header/详情/inline归一化|src/tui.rs L3787–3797,6687–6784,13852–13854|set_status经bound_text限制256KiB；不是源header/details/inline三字段；独立消息/错误浏览器承载长细节。原始source没有对应字节限。|不同数据与容量合同；源None/空/空白/换行、Preserve与大写扩张判据；目标状态边界/长错误浏览器测试只证明其各自入口。|
|M05 宽度与高度|src/tui.rs L6687–6784,7064–7102|BentoBox第二行固定header，Paragraph裁剪；没有源主行ellipsis或三行详情widget。源max行数与area高度双裁剪不能移植成布局要求。|有意布局差异+待测可见性；长header/20列/1×1/宽字符，确认提示可能被裁剪；不改BentoBox。|
|M06 提示与取消路由|src/tui.rs L5809–5834,6099–6126,13042–13077|Ctrl-C busy空草稿或非空草稿返回Interrupt，idle空CtrlC/D退出；Esc先历史/菜单/草稿处理。source hint bool不是权限，Zenpi不用source Esc全局替换。|已有目标按键合同；hint false/true+mods/Repeat+modal需要源调用级断言；目标PTY需动作与所属owner结果对应。|
|M07 取消接受/确认/恢复|src/tui.rs L13042–13077,13104–13130,13321–13339|生产host先取消本地diff/new，再拒绝错误project；runner.try_cancel QueueFull时保留审批，但仍能显示Interrupt requested。简化host只改busy/status。退出guard.leave先于join。|请求不是完成确认；QueueFull/错项目/实际cancel终态/terminal错误分别验证；未用简化host充当provider停止。|
|M08 status生命周期|src/tui.rs L3800–3814,7064–7102；source bottom_pane L717–767|目标忙闲改变标记，不是Option widget new/hide；source hide不改running，ensure重建起点。源undo隐藏hint是其应用生命周期。|不等价，不机械复用；busy重复值、hide→ensure、undo结束；source字段无身份/持久化，不赋予目标new/undo验收。|
|M09 项目切换与重启|src/tui.rs L2518–2558,2572–2601|目标ProjectDraft保存/恢复ux、busy/status，同进程Instant随字段转移；切换本身未pause。不是持久化时钟。|已有同进程归属；重启不作保证；后台运行/审批/切回/new/独立重启需owner时间合同；本报告不验收session/headless。|
|M10 model/usage/history/background|src/tui.rs L4080–4103,6687–6784|实际usage只接受当前streaming job；显示in/out、tokens —、model及last model、history ~、后台数量、cwd。原始旧报告说没有这些已过时。|已有能力；当前usage测试注入8/7，断言123/45可见且999不见；不是实际provider/窄屏全部可见证明。|
|M11 详情完整性与安全取消|tests/tui_transcript_ux.rs L332–373|80行错误浏览器PgDown看079；busy CtrlC得到Interrupt；目录picker保留其焦点，不打开copy browser。|已有上下文测试，未本轮执行；这些使用handle_key/TestBackend，不证明真实终端/provider ack或计时暂停。|
|M12 source计时被其它显示消费|source chatwidget L1727–1765,3106–3155|FinalMessageSeparator消费elapsed，再经worked_elapsed_from按上次分隔符差分；下降时基线归0。目标header秒数不是同一分段时间。|目标暂无线性等价合同；重建计时下降/多分隔符差值用0/5/8/2验证，属调用上下文，不接受chatwidget整文件。|
|M13 通道/错误与资源|source app_event_sender L1–28,frame_requester L1–128；target RenderScheduler L7673–7719|source Op发送失败只日志，frame发送失败忽略；unbounded mailbox合并draw不等于队列有界。目标本地dirty bool合并不同。source详情先全量wrap再truncate。|源机制理解，不作安全保证；关闭receiver/大量调度与超长details需有界独立实验；本项禁止新增预算或网络探测，故仅记录判据。|
|M14 测试专用/样式/非适用|source L139–156、292–440及helpers|3个cfg(test) getter不计生产API；资本化首Unicode scalar可扩张；shimmer按scalar、颜色能力依赖helper；widget不持有provider成本、资源lease、任务身份、I/O恢复或session存储。|范围排除有理由；7源测试与3个当前snapshot可读；2个queued历史snapshot不被正式测试引用，不能计测试覆盖。|

目标上下文完整读3个测试（actual_usage…、error_details…、directory_picker…）和3个helper，共8个普通断言，执行0。helper直接handle_key或apply_view_event_for_job，render用TestBackend；不冒充生产PTY事件、实际provider、cancel ack或长时计时验证。source7测试与这些上下文测试分账，前一任务122的157回归和PTY不并入309证明。

最初1875B报告说目标没有elapsed、token/model或background摘要已过时；旧216行报告已更正，本轮当前读取再次确认它们存在。M02、M05、M07记录顺序/裁剪/请求确认风险，均为静态推论与待测判据，未写成已触发产品故障。工作状态、推理内容和最终答复仍不同层；状态报告不得取代消息或改变BentoBox。

## 历史结果、漂移和交付边界

旧309包里保留早期transcript binary cf3659…、后续release binary1d7b2…及相关成功/失败说明。历史记录中的87测试、各PTY布尔/字符串/对象检查、冷启动失败1044.823959ms与1208.732708ms均仍留在旧包；本轮不宣称重新验证这些执行。旧日志缺run.json不能补exit；run记录sha若绑定输出日志不能改称结果JSON的hash。历史预算不是本轮预算，原始结果可能是有限fixture或未附全失败目录，其边界沿旧完整报告保留。

新证据独立目录仅含captures、逐段read ledger、单元/函数/测试/映射清单、历史完整副本、输入漂移/保留核对、唯一报告及正反补丁。preparation-issues.json保留路径定位错误、读取界限拒绝、上下文输出截断及新Python脚本长UTF8行解析失败，修正都只在新私有目录；正式源完整3段读取没有遗漏。

candidate.patch从当前main报告absent新增唯一标准报告；rollback.patch只删除这份报告，临时目录实际check/apply/reverse后比对字节和sentinel。集成时若标准路径已经存在，必须重新核对基底，不能force覆盖。冻结便携包只运行一次只读离线verify，所有输出放包外；它检manifest、source/read范围和单报告差异，不能代替master逐段语义接受。完成交付后停止，不开始父目录/125实现或任何新探测。

## 冻结时主库漂移核对

29份捕获输入中1份发生变化；完整身份见[freeze-input-state](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference309-status321-ready/evidence/freeze-input-state.json)。原捕获、旧ready及全部失败均未重写。主控已通知122修复合入；实际main TUI现为 639010 B / 15622行 / `b15661a160a855ef974e82dfb40a63c8d0c5777623cd553d1d4cf2069d8b59a1`，本报告行号仍绑定旧捕获638305 B/15607行。19段已读目标TUI中19段在冻结时main精确找到同字节；这是片段身份复用，不是重新运行测试或完整阅读新TUI。

- `capture/zenpi/src/tui.rs`：`0deb8e0e13cfdad0aa00e07d79caf573e9aa36095a9bcefb4219ed8f70936557` → `b15661a160a855ef974e82dfb40a63c8d0c5777623cd553d1d4cf2069d8b59a1`。

冻结前100个既有ready/29508个regular、145个tracked及105个外部历史文件全部原身份保留。唯一标准报告在main仍absent，新增补丁基底未变。
