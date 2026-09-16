# ZS1-307 · file_search.rs 独立完整复核（3.1.21）

仅此正式文件的 understand 候选，主控 G-FILE 待审。唯一交付路径 `Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/file_search.rs_learn.md`；大调用方仅有界context，不声称它们、父目录、整TUI或ZS1-123已验收。保留BentoBox，不改产品、蓝图、索引、claims或正式receipt。当前run `zenpi-stage1-20260911`，requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`；捕获snapshot `ca9704da623173c944c5040aa6eb90a731219a8e029a89cd21b4f2f97e99e078`，所有结论绑定本包捕获与实际阅读范围。

正式源 `/Users/wangweiyang/GitHub/codex/codex-rs/tui/src/file_search.rs`，HEAD `b3b3d262787f4902a7449f17d793241a34d311ad`，**4009 B / 133行 / SHA256 `7aaa33ac7fd28cbe5fa3405cd3490b30e614272122bb572c9c352acc418b71d9`**。与3.1.21 manifest和旧307完整读链逐字节相同，source delta 0。本轮独立顺读1–133，连续 `[0,4009)`，覆盖全部7函数/14语义单元和全部分支；正式源不含测试。源测试存在数0、执行数0；新产品/Cargo/PTY/HTTP/平台/预算实验均0。

[正式源快照](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference307-search321-ready/evidence/capture/codex/codex-rs/tui/src/file_search.rs)；[实际读档字节绑定](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference307-search321-ready/evidence/read-binding.json)；[逐单元语义与覆盖](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference307-search321-ready/evidence/source-units.json)；[12项映射与未执行判据](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference307-search321-ready/evidence/capability-matrix.json)。

## 旧链复用与新范围

在worker2267定位旧 `zs1-307-ready`（54 regular）及 `codex-reference-ready`（17 regular），完整原样保存。旧307报告 **27363 B / 140行 / SHA `4d17b05c8feea6caae2ef4f364f242dcb89c3bfabb36aea572df663045358387`**，本轮按1–48、49–100、101–140完整读；2040 B原报告 `eb836e68285dea1791ca042f633bd0f252f0f66ea51555ec970ab83659400d2d` 也全文读。旧manifest `e21c3e0d2176ecd29cb0d35b61ca701d5a5024d882680fa80789885f20b009a7`，旧read-log `73e9491e5afc087efd903878ce83e48fec135cb69957ac90d3d620cdb87cdebf` 绑定 `[0,4009)`。旧源纯编排结论可按相同hash复用，但本轮仍重新完整理解，未执行旧脚本。

旧目标TUI为d2a98934… / 579524 B，本轮为 **638305 B / 15607行 / `0deb8e0e13cfdad0aa00e07d79caf573e9aa36095a9bcefb4219ed8f70936557`**。当前必要片段已重读，不以旧行号套新文件。[历史完整包](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference307-search321-ready/evidence/history/zs1-307-ready/manifest.json)和[旧/新context精确比较](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference307-search321-ready/evidence/history-comparison.json)保存每个旧片段是否还能在当前字节中精确找到；找到不等于整个目标文件已验收。复制整份context用于身份绑定，不当整份阅读信用。

相对旧报告的新阅读明确补足：①库默认参数、update通道、Drop和create的线程入口；②FileMatch实际有root，不能简写整个event无root；③完整popup绘制/selected_match和composer文件选择，揭示waiting仍可选旧缓存、空prompt的实际文案以及图片尺寸读取；④当前目标项目cwd缺失fallback、按键选择、新Unicode重开测试、in-turn附件拒绝与实际附件构造/路径校验。历史未知已由读到的事实缩小，仍未展开matcher/walker、BackgroundRunner完整协议、最终内容读取与provider上传。

## 正式文件全部语义单元

|行|单元|输入、状态、输出、错误与副作用|
|---|---|---|
|1–14|文档与依赖|@token变化通过AppEvent进入编排器；外部file_search库负责搜索实现，Arc/Mutex保护共享状态。无键盘、绘制或文件内容读取。|
|15–27|两类状态|manager持共享SearchState、独立search_dir、sender；SearchState含latest_query、Option session和usize token。没有查询代次、项目ID、错误态、结果缓存或持久记录。|
|28–40|new|接收root/sender，初始化空query、None session、token0；不校验根路径、不启动搜索、不产生事件。|
|41–51|update_search_dir|先换search_dir，unwrap锁后take/drop session并清query；相同root也执行。不立即重建、不递增token、不直接清UI；新查询才创建新session。|
|52–61|query去重|unwrap锁；String精确相等直接return。不同则clear/push新query，无trim、大小写归一、长度限制或本层debounce；空白仍非空。|
|62–66|空query|latest已经为空时take/drop session并return；相等空query在上一分支提前return。不主动发结果、取消确认或UI清除事件。|
|67–74|非空更新|session缺失先尝试创建；存在才update_query。不同query复用session/token；创建失败则不update。调用发生在state锁内。|
|75–82|session token|每次创建尝试先wrapping_add(1)，失败也消耗token；reporter保留当次token和共享state/sender。usize回绕不是永久唯一保证，未作为实际复现缺陷。|
|83–91|创建|一个root，compute_indices=true，其余默认，外部cancel_flag=None。库默认与实际生命周期见下文新读context；本文件本身不设扫描/时间预算。|
|92–101|创建结果|Ok保存session；Err仅warn并保持None。失败query仍是latest，同query再次调用直接被去重，需改query或清root状态才重试；无UI错误事件。|
|102–107|reporter状态|持共享state/sender和token；没有独立root、查询generation或取消确认字段。|
|108–117|snapshot过滤|unwrap锁，拒不同session token、空latest、空snapshot.query。没有严格snapshot/latest相等判断，也不检查session是否Some；正常旧query由下游继续过滤。|
|118–126|发送|clone snapshot.query，显式释放guard，再clone matches发送AppEvent。解锁后状态可改变；只传query/matches，省略快照计数与walk_complete，无epoch回执/队列撤回。|
|127–133|trait实现|on_update转send_snapshot；on_complete为空，不清session/query、终止UI loading或重试。正式源无测试，两个trait方法也在本轮完整范围。|

源只持有搜索状态，没有持久会话恢复或权限授权。三处lock().unwrap的poison会panic；clippy expect不是恢复。对比query的String相等、session_token的epoch相等和最终popup pending相等是三种不同门槛。创建失败保留latest导致同query不重试，是可由53–99直接推出的确定行为；用户是否遇到永久等待仍需真实调度和UI验证。

## 新读的库合同与消费链

库context 1–235中，FileMatch含score/path/match_type/**root**/indices，full_path可root.join(path)。snapshot还含total_match_count、scanned_file_count、walk_complete；manager只复制query/matches，计数和完成状态未进入AppEvent。默认limit20、threads2、exclude空、respect_gitignore=true，manager覆盖compute_indices=true。默认说明涉及ignore规则；本轮未读walker，不能据此宣称所有.gitignore行为、递归排名或速度已验收。

库update_query 143–148仅向unbounded工作通道发送QueryUpdated，忽略发送错误；Drop 151–156置shutdown=true并发Shutdown，**不join、不等待线程结束、不调用reporter**。create 158–211检查至少一root，build_override_matcher可能返回错误，构造Nucleo并启动matcher_worker和walker_worker，返回session，没有在此保存JoinHandle。None外部cancel flag会建立内部false flag。这里是比旧报告更具体的取消事实；后台线程和Nucleo内部生命周期仍未全文审阅，也没有测硬时间上限。源在持state锁时create/update/drop，但新读的两个短方法没有同步回调reporter，不把未读依赖风险直接称已发生死锁。

启动App2196取config.cwd建manager；已读resume2498–2525在换config后update_search_dir并创建新ChatWidget。只覆盖该resume分支，不等于所有fork/new/project切换。App2751–2756转query/result，ChatWidget8638–8641→BottomPane1099–1102→composer1231–1244；BottomPane总请求重绘，拒绝结果仍可重绘。composer要求当前@token starts_with(result query)，popup67–78再要求query==pending_query。同session a→ab后迟到a因此不会替换pending=ab的matches；不能从manager未严格相等直接断言UI发生旧结果覆盖。

event没有独立请求root/token/generation字段，但每个match有root；**当前已读consumer未核对match.root**，popup.selected_match只返回path。源锁内检查通过后释放锁再send，旧root同query已入队event可能不受后续token变化约束；新root同pending仅凭query不能证明旧事件被隔离。这里只确定字段与过滤边界，实际可达时序和其它App队列隔离未跑，不能写成已复现错文件或安全漏洞。

## 源菜单交互的实际差异

popup set_query只改变pending与waiting，保留旧matches/display_query。完整154行实现显示：等待仍画已有行，is_disabled=false，selected_match按index直接取旧path，不检查waiting或display/pending；source选择handler1642–1764也不查waiting。因而“新旧query到达过滤正确”和“等待时旧缓存仍可选择”可同时成立。空列表时waiting显示loading...，否则显示no matches；set_empty_prompt虽注释说hint，实际置waiting=false/清matches，落no matches。空@在文末可得到empty token；current_prefixed_token的allow_empty=false主要影响特定空白边界，不可简单推断空@永远不触发。

源Up/Down及Ctrl-P/N选择；Esc先可能切footer提示，再隐藏popup并记住同token，保留草稿。同dismissed query不重开，变化后可重开；sync_popups在history/command/mention/无token路径可发送空query，但popups_disabled分支只hide并return，不能一概说任何隐藏都停止搜索。

源Tab或无修饰Enter有selected path时选择并返回InputResult::None；无候选Enter转普通输入handler，可能继续提交，Tab则只关popup。普通文件insert_selected_path替换整个@token（移除@），加尾空格；有空白且无双引号才外加引号，不做目标同样的转义规则。图片路径先用相对sel_path构造PathBuf尝试image_dimensions，成功移除token并attach_image，失败回退普通插入；该分支可能在选择时读取图片尺寸，不能把“manager无内容读取”推广成整个选择链零文件读取。此处未审attach_image、进程cwd同步或图片格式判断函数，不判定图片错根已发生。源选择只取path未取FileMatch.root是单独待验的根归属边界。

## 当前zenpi owner与路径选择映射

|能力/源范围|当前owner/真实行为|差异、缺口或范围排除|可运行判据（本轮均未执行）|
|---|---|---|---|
|C01 root与状态 / 16–50|TUI 562–568, 2653–2657, 2540–2575：查询对象完整相等含root/input/prefix/path/raw_selection；project_cwd从active metadata取cwd，缺metadata落“.”；切project清cache并激活draft_epoch，但query不含epoch。|源session与目标每query扫描不同；真实owner cwd应始终存在，同root同text不是会话身份。|合法/无效root初始化不扫描；两root同text结果只归所属root；缺metadata与同root新会话分别观测。|
|C02 查询与去重 / 52–74|TUI 580–636, 4657–4733, 11750–11765：只尾部token/四种slash路径生成query；fold按等byte空格mask；改变完整desired才取消旧job并submit；空@可列根目录。|源空query停止session，源空popup显示no matches；目标空path浏览是有意差异，不能假称递归fuzzy等同。|空→a→同a→ab→空；目标光标中部/fold/闭合token尾空格/四slash入口分别确认只产生支持的查询。|
|C03 创建、失败与重试 / 75–101|TUI 639–723, 11547–11556, 11750–11783：目标独立BackgroundRunner调用complete_file_query；try_submit失败清last以便下轮重试；Completed Failed显示错误但不清last。|源create错误只warn并去重同query；目标有提示仍需改query才能重试，同query恢复入口缺失。|可控目录不存在→恢复且query不变，分别记录当前不重试及明确重试入口的预期；不得靠无限轮询。|
|C04 结果过滤与在途事件 / 108–126|Codex composer 1231–1244; popup 43–78; TUI 4721–4733, 11766–11783：源manager粗过滤→composer前缀→popup pending严格相等；目标active JobId→完整query两层过滤。|不同query迟到拒绝已接线；源event无独立root/epoch，但matches每项有root，消费端未核对它；不能称全部payload无root。|a→ab后旧a；A root event先入队再切B同query；目标不同root及同root新session分别调度，禁止误归属。|
|C05 取消和完成 / 44–49,62–65,90,127–133|Codex lib 141–156,158–211; TUI 672–685,11751–11783：库Drop置shutdown并发Shutdown，不join；update_query发unbounded通道信号；目标try_cancel结果忽略，旧ID不apply，扫描协作检查cancel。|source on_complete无UI动作；目标loop只处理matching Completed。Rejected/Closed完整runtime合同未读，不宣称永久loading已实测。|query变/清空/root变后迟到成功、失败、取消、关闭；确定无错误候选覆盖与可恢复状态，线程停机另按依赖验。|
|C06 结果规模与完整性 / 83–91,118–123|Codex lib 90–125; popup 112–154; TUI 675–722：源默认limit20/threads2/ignore开启且indices真；snapshot含total/scanned/walk_complete但manager丢弃；目标最多2048枚举、150ms软截止、128结果。|两侧UI均未在已读模型传递截断完整性；目标单层前缀筛选，排序仅覆盖已采集子集，预算非硬deadline。|129候选、2049枚举末尾唯一匹配、受控软截止；最大128且明确不完整/可继续，不把截断空集等同无文件。|
|C07 路径与内容边界 / 全部1–133|TUI 644–714,11468–11503; slash_actions 561–601,608–698：补全拒absolute/parent/control、非UTF8条目和symlink子路径；提交使用Agent附件root，再canonical/相对路径/逐组件symlink/containment/文件/大小检查，保留原生相对路径。|发现阶段只是名称和metadata；root本身信任与fallback需区分；构造InputAttachment还不是完成读取/上传审计，后续重开仍须独立验证。|Unicode/引号/Unix反斜杠、父路径/子symlink/root别名、发现后替换文件，再明确提交；核验owner和实际读取来源，保留草稿。|
|C08 等待中选择 / 1–6,118–123|Codex popup 43–52,94–108,112–154; composer 1642–1764; TUI 4874–4879,5020–5042：源set_query保留matches，waiting仍绘旧行且selected_match不检查pending；Tab/Enter可选择，源图片还会读尺寸；目标缓存不匹配即不出choices，query变清cache。|源旧已缓存列表可选与旧新到达结果被拒是不同路径；非图片普通插入移除@，目标保留@并quote，不应照搬source行为。|已有a结果后输入ab并在新结果前Tab/Enter，确认用户看到并选择哪项；普通文件仅编辑草稿，图片metadata路径单独观测。|
|C09 键盘、鼠标、取消与modal / 全部1–133无按键实现|Codex composer 1642–1764,3246–3310,3509–3550; TUI 5245–5259,5744–5815：源上下/Ctrl-P/N，Esc可能先footer提示再dismiss；目标Tab歧义需已绘菜单或单项，鼠标只选择实际行，Esc保稿，新字符重开；Enter exact可继续正常提交。|不可一概说所有Enter都只补全或所有hide都取消：source popups_disabled分支仅hide；目标query未显式gate history/approval。|directory→file；Esc同query及改字；点击/未绘多项Tab；exact Enter/no候选Enter；各modal下输入与后台结果分别检查。|
|C10 忙碌提交及来源 / 全文件不授权提交|TUI 10209–10242,11450–11503：当前in-turn队列Enqueue/Edit对非slash @引用明确拒绝，提示使用scheduled或idle prompt；Model worker明确解析并构造附件。|补全本身可在busy出现，不等于当前队列允许附件；scheduled路径全链本轮未复核，不能把提示当支持证明。|忙碌补全后选择queue与scheduled/idle分别观测；拒绝保稿、成功请求附件归正确owner且发现阶段不自动发出请求。|
|C11 锁、资源和持久化 / 17,47,55,75–96,111–123|Codex lib 141–211; TUI 663–685,11547–11556：三个unwrap锁可能poison panic；源create/update/drop在guard内；新读update/Drop没有同步reporter调用，create起matcher/walker两worker，未持joinhandle。|不据此判定完整依赖无死锁；线程总数/Nucleo内部池/回收与慢FS不在范围。搜索缓存进程内，无恢复协议。|有限替身观测close/poison/慢FS返回和队列压力；不跑FIFO或无界阻塞，不改预算；重启只重新搜索而不声称恢复在途query。|
|C12 测试和历史复用 / 1–133无源测试|tests/tui_command_palette 240–289,413–429; TUI 14327–14357：三个外部目标测试及Unicode Esc重开单测全文读，全部未运行；旧54+17文件包原样保全，历史H123仅旧证据。|直接API断言不覆盖真实后台调度/队列重排/HTTP/性能；正式源测试0不填入别处测试数。|未来按原名有限跑4项目标测试并单独生产交互；既有历史negative误选new binary保持记录，不重计当前positive。|

目标file_completion_query先排除dismissed、光标不在末、directory picker、transcript browser；此函数没有显式history_search/focused approval gate，不把其它输入路由推测为全面取消。四slash入口为/attach、/diff、/review、/reload；reload raw，其它去引号并处理转义；非slash解析最后word-boundary @，只能在尾部且不得与fold重叠。path>4096或过多引用由解析器报错，在query生成处ok()?转None；查询生成不承诺显示这些错误。空@和/attach空参数可产生empty path并列根目录。

扫描只读一个parent目录、按name.starts_with(needle)大小写敏感前缀匹配，无本函数递归/模糊/内容搜索；显式拒absolute、ParentDir、RootDir、Prefix、control，逐parent symlink_metadata拒链接/非目录，忽略非UTF8、control名称及symlink/特殊条目。root本身此处未canonicalize，project_cwd缺metadata落“.”，应由真实项目初始化合同保障。取消检查发生在parent walk之后及各项迭代中，150ms起点也在parent walk之后；read_dir/file_type等同步FS调用不能被软截止打断，所以不得称150ms硬时限或有界取消join。

最多枚举2048，再按directory优先/input字典序排序已采集子集，truncate128；任何entry/file_type错误可使整个结果Err。返回Vec没有has_more/truncated/reason，当前用户无法据列表区分完整无匹配与未扫到匹配。源manager也丢弃snapshot总量/完成信息，本项应保留“说明结果是否完整”的UX缺口，不能靠加预算或虚构更多按钮消除。

生产loop仅在desired变时take旧active ID并try_cancel（返回忽略）、清缓存、submit新query；submit即时错误清last允许下轮再试，matching Completed成功再次核验完整query，失败仅当前query仍等last才显示File completion错误，last不清。Rejected/Closed等没有在这段专门处理；没有读Runtime完整状态机，因此保留有限生命周期验证项，不声称其必然留下永久active。常见迟到旧ID及不同root结果均被拒；完整query无project ID/session generation，即使切换清cache并激活draft_epoch，同root同输入也不是自动新的查询身份。

当前缓存query不等时slash_choices不给文件行；选择directory继续browse，file加尾空格并dismiss，都清cache。Tab多项只有菜单已画才选择单行（单项例外）；Up/Down/鼠标按可见菜单选择，Esc保稿，普通字符/Backspace/Delete清dismissed。Enter如果exact且非submenu可继续正常提交，因此“永远需要两次Enter”不准确；普通非exact选择才返回Redraw。目标保留@且quote/escape路径，source普通文件插入不保留@，产品不能直接照抄。

Model worker从request.owner取Agent，非slash请求重新解析引用，拒空/未闭合，再用agent.attachment_workspace_root构造附件交process_with_cancel_and_events。新读attachment_for_path先canonical_workspace、resolve_relative，再文件/大小检查；resolve拒空/超上限/control/absolute/parent、逐组件symlink，并canonical后确认仍在workspace。root自身别名在canonical_workspace解析，属于根可信合同；检查与后续重开之间不保证原子无TOCTOU。返回InputAttachment只含原生相对path、kind/mime，避免Unix反斜杠被显示归一化成分隔符；没有在这个构造器读完整文件体，后续哈希/准入重开本轮未展开。发现/选择、metadata校验、内容读取和对provider发送是不同边界。

in-turn InputQueue Enqueue/Edit当前明确拒非slash @引用，提示scheduled或idle，不能从补全可出现推断所有busy提交支持附件；scheduled完整调度和失败恢复在本轮范围之外。BentoBox顶部项目分页、焦点/布局/折叠保持兼容要求，本报告不修改布局、不引入第二个应用owner。

## 已读测试、历史失败与未验证范围

正式源133行无test模块，测试存在0、执行0。目标测试只读：file_completion_is_cwd_bound_quoted_and_stale_query_safe（240–266，Unicode空格路径两次Tab/旧完整query拒绝/尾空格）；slash_paths_and_negative_boundaries_do_not_mutate_draft（267–289，quote、父路径、cancel、Unix子目录symlink）；completion_result_and_path_limits_remain_bounded（413–429，200→128、4097路径拒绝）。当前TUI单测normal_unicode_typing_reopens_file_completion_after_escape（14327–14357）确认Esc后正常Unicode输入走handle_event_at可重开，这是旧307报告没有记录的新增当前上下文。4项均只读，未运行；不把它们称Codex源测试，也不把direct API夹具称真实后台事件重排/HTTP验证。

旧54文件包包含历史H123 reviews/manifests/results/run及失败记录，字节保留；本轮只完整读旧307报告，不重新逐项审核所有H123 raw日志，不重计23检查、12HTTP等为本轮结果。旧报告记载negative2 wrapper错选new binary，是额外positive replay；原negative-final在资源F2处失败，尚未到file附件；保留这些限制，不改写成文件搜索负例通过。旧原报告/后续完整报告/当前报告的结论归属分别绑定hash。

本轮准备阶段宽泛rg长时间无输出，仅终止自己的PID88700，退出143；初次旧报告与其它输出合并被截断，后来按三个小片段重读完整；第一次popup请求1–220超154行，AssertionError发生在读档证据写入前，随后按1–154完整读取。其它截断rg仅作定位，相关代码/蓝图之后按准确范围读取。均保留preparation-issues.json，不属于产品失败。没有跑旧脚本、FIFO、预热、新预算或阈值实验，没有改变HOME/CODEX_HOME。

## 实际读取范围及交付校验

以下为新读取的捕获文件范围，含少量边界辅助行；只有正式file_search.rs算本项文件验收对象，完整popup仍只是context。历史报告额外范围见read-binding；AGENTS原文仅作工作约束，未改Rust所以格式/测试规则不触发。slash.rs仅捕获身份但本轮未读，不给它任何新结论。

|捕获文件|完整SHA256|实际阅读行|
|---|---|---|
|capture/codex/codex-rs/tui/src/file_search.rs|`7aaa33ac7fd28cbe5fa3405cd3490b30e614272122bb572c9c352acc418b71d9`|1–133|
|capture/codex/codex-rs/tui/src/app.rs|`75ffbc77f8e8f65fd31602b7e025fa7d4fccadadec138cf099c568fec65845fc`|2190–2200, 2498–2525, 2748–2758|
|capture/codex/codex-rs/tui/src/app_event.rs|`037e0394523f4e07585d489d01858e73a92e9156f98cdcf285304efcac6837e8`|116–131|
|capture/codex/codex-rs/tui/src/chatwidget.rs|`0453f643d88c62d0ad11aa01217af783719400b6c966b71d980ae8c4eac4829a`|8635–8644|
|capture/codex/codex-rs/tui/src/bottom_pane/mod.rs|`97af02eed39dbfdf69d1ec3e14dc13b9a91f4b33f5e4b985cc1f2cbae4105823`|1095–1104|
|capture/codex/codex-rs/tui/src/bottom_pane/chat_composer.rs|`234189c66f50c8654ef72e86ad5463eea5dd1e0c09c98f8d64c898449161b6da`|1620–1735, 3219–3310, 3509–3553, 1736–1823, 1229–1246, 2095–2157, 2015–2094, 1990–2014|
|capture/codex/codex-rs/tui/src/bottom_pane/file_search_popup.rs|`32d07857a1c7ef0054b56705e9b680275e23ac31016bf946b55f277a1a71884d`|1–154|
|capture/codex/codex-rs/file-search/src/lib.rs|`75a6457abe072ba2ad0a330fafe26a8c31c2fef1983471af77923ee7d5582b0c`|60–235, 1–59|
|capture/zenpi/src/tui.rs|`0deb8e0e13cfdad0aa00e07d79caf573e9aa36095a9bcefb4219ed8f70936557`|550–735, 4657–4738, 11735–11795, 2540–2575, 2653–2671, 4874–4894, 5020–5046, 5135–5164, 11450–11503, 11543–11560, 14323–14363, 5766–5823, 5236–5267, 10208–10242, 5744–5765|
|capture/zenpi/tests/tui_command_palette.rs|`3b84deaf2d41b34eae9014861570b5e9e9ad7c1789a939554acd53caa096b41e`|240–290, 413–431|
|capture/zenpi/Docs/stage_1_v3_pi_mono_blueprint.md|`ca9704da623173c944c5040aa6eb90a731219a8e029a89cd21b4f2f97e99e078`|1–13, 135–143, 481–487, 409–429|
|capture/zenpi/src/slash_actions.rs|`f5dbc7862462d314bb8880ad6ee0f02d7c38aafa8b96196a08acb9773490b497`|559–710|

唯一canonical报告以新增补丁交付；freeze时核验主库该路径不存在。[候选补丁](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference307-search321-ready/candidate.patch)及[反向补丁](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference307-search321-ready/rollback.patch)只作用本报告。[便携验证器](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference307-search321-ready/verify.py)核对manifest精确集合、源/历史源身份与133行连续覆盖、7函数/14单元/12映射、所有读档片段、capture selector和源manifest、旧包未变、正逆补丁，再在独立临时git仓库做check/apply/重复拒绝/rollback并保全无关sentinel。它只验证静态完整性与补丁可用性，不替代主控G-FILE语义审阅或G-HOST，也不生成正式验收receipt。

[捕获身份](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference307-search321-ready/evidence/capture.json)；[历史差异范围](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference307-search321-ready/evidence/history-comparison.json)；[准备失败记录](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference307-search321-ready/evidence/preparation-issues.json)。冻结后便携验证只跑一次，日志与运行记录保存到ready外的新私有工作目录；不修改已冻结manifest或旧ready。本项交付后停在307单文件边界。
