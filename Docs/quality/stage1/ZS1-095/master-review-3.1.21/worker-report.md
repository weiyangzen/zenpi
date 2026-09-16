# ZS1-095 · zenpi src/approval.rs 完整源码复核（3.1.21）

本报告复核当前主库审批模块的全部 780 行，结论是：审批请求的结构校验、协调器的首个响应接收、策略判定和会话记忆恢复均有明确实现，但它们分属不同状态与信任边界。TUI 默认选中 Deny；ApprovalPolicy 的默认模式却是 Always，允许普通只读调用、对其余调用返回待审批。协调器接收 Allow 后即可唤醒等待方，核心调用方继续持久化审批记录、更新记忆、记录消费并检查取消，才进入实际执行路径。因此，选中、提交、接受、记账、执行和终态必须分别判断。

本次仅产生唯一 owned 报告的候选创建补丁。没有改动产品源码、主库报告、权限规则、蓝图、manifest/index、claim 或 master receipt。上下文源码不计入095正式文件读数；旧报告、旧失败记录和旧 ready 原样保留。结构审计结果另存于包外，只证明交付一致性，不替代 G-FILE 的逐项语义审阅，也不代表 G-STAGE 或 095 已验收。

## 身份、范围与证据来源

| 对象 | 本次身份与用途 |
|---|---|
| 正式当前源码 | `/Users/wangweiyang/GitHub/zenpi/src/approval.rs`，29396 B / 780 L，SHA-256 `5488ad365670e515eba5e7d82a91dbf935be854b848c20fd99725f102e1f185f` |
| 蓝图登记基线 | 27606 B / 737 L，`c592686334a95047f749d7c61c793abc03ce93d5312b22aadeefa5aed9c85844`。蓝图L120与source_manifest第23行仍是该身份，不被当前源码覆盖 |
| 差异 | 从完整保留的旧基线重新作精确字节diff，仅增加43行/1790 B的 `with_remembered_events`；不是旧材料中曾误称的52行。旧基线本次只作身份/diff参照，不虚报第二次全文阅读 |
| 正式全文读取 | 四个顺序连续区间L1–200、201–400、401–600、601–780，覆盖全部字节，无空隙；原始片段、区间、hash见read-binding.json |
| 权威捕获 | 3.1.21，run `zenpi-stage1-20260911`；requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`；snapshot `c392c1835d4749d4a6248a799b4317f165351cb6467d5f1527c10a88b7e1c25d`。保留捕获快照，冻结时漂移另记 |
| 正式owner | 蓝图L211：`Docs/learn/stage1_pi_mono/targets/zenpi/files/src/approval.rs_learn.md`，L1，依赖001；验证G-FILE/G-STAGE逐项，回退仅撤该报告。L139明确纯hash/符号/结构/compile不能代替语义证据 |
| 当前真实调用方 | core.rs、headless.rs、tui.rs仅按read-binding所列有界区间新读；tests/tui_approval_focus.rs本次完整读428行，但为上下文测试；approval_owner.rs只读所列区间 |
| TUI原捕获 | 648110 B / 15853 L / `a19dfcf74a5ae07e394dc1e434e563f088bb555fdd739a61e2736acfc3a9d0b8` |
| TUI补充捕获 | 主控125候选增量后666406 B / 16316 L / `9deac1fe3e76331c24bbf1b968bdcdc99adfa7f67c68ace7e49edae7b6d9e09e`。保留原捕获，另存delta及有界新读；不把尚在主控验证的125称为验收通过 |
| 独立参考303 | 已独立复核的Codex approval_overlay.rs：57262 B / 1556 L / `c924923b402b198f8f77b249f0d37892896c0b84002c0f6e48330917da80cecc`；原manifest `a07cd41eb8076ce2e0adaeefd903f7ffe567dcdce1344053551ba63a2f51a93a`，原报告 `865cebfdb22483b9cfc70228aff63bd3dad6140d910329d6126b2ac8accc306e`。本次只复用已验证同一源码的结论，不把303的7次全文片段、19测试、5快照算到095 |

`capture.json`、`delta/identity.json`记录其余上下文的完整字节身份。正文采用095原始行号；TUI明确标为a19或9dea以免漂移行号混用。主库该 owned 报告在捕获时不存在，因此forward创建、rollback删除。工作树中已有20641 B报告只是继承历史，原位不改；其全部内容另存history供审阅。

## 模块责任与数据接口

L1–19的模块注释说明审批允许宿主展示、接收决策；依赖serde、ToolOrigin/SideEffect/Preview、BTreeMap/Set、Arc/Mutex/Condvar、AtomicU64及50ms等待。这里没有文件系统执行、网络调用、会话写入或凭据过滤器。请求会克隆并保留arguments与preview，不能把“without retaining credentials”的注释当作自动脱敏承诺。实际展示/日志是否过滤敏感字段属于调用方与工具数据策略。

| 声明/接口 | 输入、输出与约束 | 边界与有意不承担的职责 |
|---|---|---|
| ApprovalMode L20–38 | serde snake_case，Always/ReadOnly/PerTool/TrustedWorkspace/Headless/WorkerAllowAfterPreflight/Never，Default=Always | 枚举名不能代替L491–523精确优先级；尤其ReadOnly不是一律拒绝写、Always不是每次都问 |
| ApprovalRequest L40–57 | request_id、turn_id、call_id、tool、side_effect、arguments；optional preview/policy_digest/lease_id；origin带serde default | 没有project字段；结构中携带身份材料并不证明调用者获权；非deny_unknown_fields，不能声称拒绝所有扩展字段 |
| validate L59–104 | 四个必需字符串trim后非空、UTF-8字节长度≤128、无控制字符；arguments必须object；digest恰64位小写hex；lease非空≤128无控制；worker必须digest和lease；preview调用自身validate | 不做tool专有arguments schema或总大小限制；合法字符串不trim归一化；不验证digest对应当前策略、不验证lease有效性，也不重新读取文件校验preview匹配实际改动 |
| ApprovalDecision/Response L107–120 | Allow/Deny，响应仅request_id、decision、remember（serde默认false） | 没有project/turn/call/digest签名；没有独立Response.validate；respond只先校验request_id，然后依协调器状态匹配 |
| Coordinator/State L122–148 | 可克隆Arc共享锁和条件变量；pending/visible/decisions为BTreeMap，accepted为Vec；AcceptedApproval保存完整请求和完整响应 | visible为已展示/可恢复快照，不是授权必需条件；accepted不是持久化记录；所有集合无模块内总容量或TTL |
| Default/new L149–161 | 新建空四集合、Condvar、epoch=0；clone共享这些对象 | new不是恢复磁盘队列；无Drop主动取消、持久化、跨进程共享或自动淘汰 |
| ApprovalPolicy L465–483 | mode，trusted_tools集合、per_tool映射；Default Always且空集合；集合有serde默认 | mode字段自身并未serde(default)，不能推导缺失mode反序列化等于Default。trusted_tools只在特定分支查exact tool字符串 |
| 错误 L573–589 | ApprovalError::Invalid(String)、UnknownRequest、Cancelled；PersistApprovalError<E>::Approval/Persistence区分协调器错误与用户回调错误 | 没有Timeout类型、自动retry/backoff、错误持久化或panic捕获；不能把UnknownRequest唯一解释为已执行完毕 |

## 协调器的完整状态转换

`request` L166–174返回Decision，调用内部retain=false；`request_response` L181–188返回完整Response，retain=true。二者都接受可变policy与取消闭包。富响应调用方承担持久化和记忆；旧简化入口在取到响应时直接remember，并不经过journal，因此不能脱离调用方证明“所有审批先落盘”。

| 操作与行号 | 精确状态变化 | 可观察结果、错误与竞争边界 |
|---|---|---|
| request_inner注册 L190–213 | validate在锁前；锁中若pending或decisions已有ID则Invalid；清除同ID旧visible，再插pending | 不检查accepted，因此此ID不是永久唯一的消费凭证；登记后不notify宿主，宿主靠轮询/drain发现 |
| request_inner等待 L214–242 | 每轮先在持锁期间执行cancelled；true时清pending、visible、decisions和accepted并返回Cancelled；否则消费自己的decision，移除pending/visible/decision；retain=false再删accepted并按remember更新policy | 取消检查先于decision，有取消与Allow同到时可优先取消；这个闭包不得重入锁或做长时间阻塞。wait_timeout 50ms释放锁等待，是轮询周期而非总截止时间 |
| drain_pending L247–264 | 找pending里尚无visible的条目、克隆并标visible，返回Vec | 按BTreeMap键排序，不是到达FIFO；第二次不会重复发同条目；锁中毒返回空Vec，不能误判系统确实无审批 |
| reveal L269–287 | 校验ID，再查pending，克隆并置visible | 可重复reveal当前pending；未知UnknownRequest；此动作是公开可见性，不是接受/决策 |
| respond/respond_with_request L291–341 | wrapper丢弃返回请求；原子版本校验ID、锁内查pending且无decision；worker remember=true拒绝；移除pending，把exact request+response加入accepted，再插decision并notify | 不要求先drain/reveal，未展示响应也可接受；首响应获胜只在当前pending/decision生命周期内成立。返回的request和响应在一次锁内关联，比先reveal再respond的两个独立锁步骤更强 |
| is_pending L346–350 | 查询pending存在，锁中毒返回false | false可能是已接受、取消、未登记或中毒，不是durable、已执行、终态的确认 |
| accepted L355–364 | 查accepted中第一个匹配response ID，克隆返回，锁中毒None | 查询不消费、不独占。即使工作线程已拿到Response，retain=true仍保留accepted供调用方写账 |
| persist_accepted L370–387 | 先accepted快照；无则Approval(UnknownRequest)。回调在锁外执行；Err尝试retract并忽略其错误，返回Persistence(E)；Ok则mark_persisted，mark失败返回Approval错误 | 等待方可能在回调前已醒并拿到Response；两个并发persist可同时得到同快照并写两次；外部写已成功后mark失败仍报错。无exactly-once保证，也不捕获回调panic |
| mark_persisted L389–403 | 锁内删所有同ID accepted；未删除则UnknownRequest；notify | 该API不做IO、不验证账本存在、不设置durable位、不阻止先返回Response，也不能把它单独当提交事务 |
| cancel_all L405–430 | 仅处理当前pending：移出并为每个条目写accepted Deny/remember=false、插decision，再notify | 保留visible；已被接受而不再pending的Allow不会被改写；空队列再次调用无事发生；锁中毒静默返回。可用普通拒绝让核心继续记录工具拒绝结果 |
| emergency_cancel/epoch L434–441 | 先fetch_add(1,SeqCst)，后cancel_all；load也是SeqCst | 真正停止依赖外层比较epoch的闭包；不直接kill进程，不自动撤销已发生副作用。u64不是无界数学单调计数；普通request闭包也不自动读取epoch |
| retract_response L446–462 | 必须先成功从decisions移除；否则UnknownRequest且accepted未删。随后删accepted；仅当visible有快照才重放pending；notify | 未展示响应的撤回可返回Ok却没有pending恢复；有visible的恢复仍保留visible，因此drain也不会再发布。decision已被等待线程消费时撤回失败，不能承诺一定可重新选择或恢复等待 |

三条典型顺序必须分开：①登记→展示→响应接受→工作线程取响应→调用方持久化→记忆/消费→允许执行；②登记→cancel_all→accepted Deny→核心持久化拒绝→无该工具副作用；③登记→emergency epoch变化→调用方取消闭包命中→清理并错误返回。①中“取响应”在“持久化”之前是现有设计，安全门在core，不在Condvar。③也可能先形成临时Deny再被取消闭包清除，因此不能声称每个emergency_cancel都留下approval_resolved。

同ID重用是模块边界：旧accepted尚存而旧decision已取走时，可新登记同ID；accepted查询可能拿到较早的记录，mark又会删全部同ID。当前core用session/turn/call/policy派生ID并依赖调用标识生命周期降低重用，但本模块没有永久tombstone。这里记录API判据，不把缺少证据的宿主可达性包装成已复现安全漏洞。

## 策略判定与记忆恢复

L485–487的decide总将preflight=false传入L491–523。后者只返回Some Allow、Some Deny或None，不排队、不展示、不执行工具、不修改策略。优先级必须按下面顺序读取，后续分支只有前面均未命中才生效。

| 顺位 | 条件 | 结果与重要例外 |
|---|---|---|
| 1 | per_tool[tool]明确Deny | 全模式立即Deny，包括只读、trusted及已预检worker |
| 2 | mode=WorkerAllowAfterPreflight | bool true→Allow、false→Deny；在readonly/rememberedAllow之前。布尔只是调用契约，本函数不运行或认证预检 |
| 3 | side_effect=ReadOnly | Allow，适用于未命中1/2的所有模式，因此Always普通只读无需询问 |
| 4 | per_tool有值 | 返回该值；显式Deny已在1处理，此处Allow覆盖剩余常规模式 |
| 5 | TrustedWorkspace且trusted_tools包含exact tool | Allow；不在这里校验工作区路径是否受信 |
| 6 | 剩余Headless / Never | 分别Deny / Allow。Never仍不能越过显式Deny；Headless也不是拒绝只读及所有记忆允许 |
| 7 | 其他 | None，由调用方建立请求。Always、ReadOnly、PerTool及未匹配TrustedWorkspace在这个剩余分支相同 |

因此“默认Deny”只准确描述TUI初始选择，不适合作为整个ApprovalPolicy摘要；“read_only_is_always_allowed”的测试名也必须受显式Deny和worker分支限制。runtime SideEffectPolicy、worker binding、lease和安全门在core先后独立检查，policy Allow不替代这些边界。

`with_remembered_events` L527–566先clone当前配置，按传入事件slice顺序筛选：type必须approval_resolved，remember必须JSON bool true，origin只能agent_tool/user_shell；tool字符串非空、≤128字节、无控制字符，而且原始self.per_tool的Deny不可覆盖；request_id/turn_id/call_id必须各为非空、≤128字节、无控制的字符串；decision只接收allow/deny。合格事件调用restored.remember，后到的有效记录覆盖先前记忆。

这里保护的是基准self的Deny，不是逐次restored的Deny；因此同次重放中先记忆Deny后记忆Allow可以变Allow，配置Deny则永远保留。基准Allow可以被记忆Deny覆盖。若外部将上次restored直接当新基准，原来记忆的Deny会变成受保护配置；实际core保留configured_approval_policy以避免这种混淆。重放的empty检查没有trim，纯空格ID/tool可能通过，与validate的trim规则不同。函数不验证记录是否来自当前session、不验证签名/digest/lease、不要求另有approval_consumed，也不认证“human”来源或设置事件数量上限；这些事件必须来自可信会话存储，不能把任意provider JSON直接当授权证据。恢复不会改mode、trusted_tools或持久化pending队列，worker事件被排除。

`remember` L568–570就是per_tool.insert，没有ID/tool校验、origin限制或持久化，直接调用可以覆盖现有Deny。配置Deny的权威性由decide优先级与with_remembered_events筛选保持，不能泛化到这个低层mutator的所有调用者。

## 与真实执行、宿主归属和生命周期的关联

core原捕获L4240–4405先处理工具注册/worker gate binding、check_call_gate、worker预检绑定、runtime SideEffectPolicy，再调用decide_after_preflight。需要询问时构造含turn/call、policy digest、worker lease及preview的请求；输入泵参与取消轮询，Cancelled转拒绝/取消错误。富响应返回后，核心persist_accepted回调写approval_resolved（完整accepted关联、摘要preview），成功后才按remember更新策略，追加approval_consumed，再检查Deny。L4402–4450接续可见允许路径设置approved_preview；worker预检自动允许写单独remember=false/source worker事件；再次检查取消/binding，再开始准备执行资源。本次未完整阅读该调用函数的后续所有dispatch体，因此不会从这个有界区间声称覆盖每种工具的最终IO。

core L2590–2768的user_shell路径提供一条本次读到真实调用点的因果链：派生UserShell context、输入事件与policy digest；捕获协调器epoch；拒绝worker身份/binding越界，校验call gate和SideEffectPolicy；按`user_shell`策略名决定是否审批（实际工具门使用run_command）；富响应persist_accepted写事件，成功后remember，Deny返回错误，再检查取消和gate，登记operation、写tool_execution_started之后，L2747调用RunCommandTool::invoke_user_shell_with_cancel。日志调用返回成功这一程序顺序能确认；它不等于本次证明底层fsync或崩溃恢复的所有实现。

core L5311–5321对(session, turn, call, policy_digest) JSON元组做hash并加approval-前缀，提供稳定关联而非随机nonce或永久消费库。L3303–3312整轮路径与user_shell路径都将外部取消或host epoch变化组合为取消闭包，说明emergency_cancel的epoch由谁真正读取。不能只用coordinator.cancel_all的测试代替这条宿主路径。

| 宿主入口/生命周期 | 本次读到的事实 | 不能推出的结论 |
|---|---|---|
| TUI a19 L1043–1203 | 请求validate后进入active_project的独立Vec，ID去重、上限128；首次Deny且remember=false；y/n只改选择，r切remember（worker拒绝），Enter才带project/request/turn/call提交；翻页复位，Esc收起、Alt-A回焦点、Ctrl-C发Interrupt；directory picker优先 | 模态收起不等于deny/cancel；选择Allow不等于接受；128只是view容量，不能保证第129个协调器等待者及时终结 |
| TUI 9dea L1344–1385 | owner_project和active_project必须等于提交项目；view中request/turn/call三元组匹配且coordinator仍pending；成功respond才retire并显示submitted | tuple检查与respond间不是同一锁事务；coordinator只认ID，身份强度仍依赖宿主隔离与不重用ID |
| TUI host 9dea L12601–12633 | 使用active_job_project做owner；Err只显示错误，保留请求。L12223–12235下一轮只对coordinator !is_pending做retire，再drain新请求 | 旧a19 host错误分支直接retire已被本次delta修正。错误owner/identity本身不再被视为审批终结；is_pending中毒false仍是底层模糊信号 |
| headless typed L6335–6408、async slash L5630–5710 | Once→Allow false，Always→Allow true，Deny→Deny false；使用传入coordinator.respond_with_request；成功返回accepted及该原子请求的digest/lease | payload主要是ID，不重做TUI四项view校验；accepted不是工具执行成功，也不能凭此认定跨项目可用 |
| headless owner L4328–4331、4511–4545、4599–4687 | 缓存每项目inputport/coordinator，循环取pool.active_context并按project_id更新；审批事件逐项目排放；process_async_line接收当前project/coordinator | 宿主project选择是隔离的一部分；缓存刷新失败/其他路由边界没有在本次全文审查，不能宣称证明所有跨项目并发情形 |
| headless EOF/shutdown L4548–4562 | stopping时遍历coordinators，普通EOF cancel_all，显式shutdown emergency_cancel | 普通EOF可留拒绝结果并等待provider解释；显式shutdown采用epoch取消，二者不是同一种终止语义 |
| headless同步slash L7207–7248；core L1654–1666 | agent.respond_to_approval先reveal再respond并返回前次revealed request；缺registry/coordinator有错误 | 两次锁操作不同于原子respond_with_request；只有调用方持有正确Agent/coordinator才有正确归属 |
| set_tools L1246–1266 | 创建新协调器，从当前session事件重建默认策略，同时另存configured默认策略 | 替换旧runtime这段没有显式cancel旧coordinator；外部clone仍可持有旧Arc，不能把set_tools视为旧等待者的可靠唤醒接口 |
| set_attachment_workspace L1268–1287 | 已有runtime更新context，否则创建空registry、新协调器与只读副作用策略，并恢复记忆 | 不能由此推导附件请求无审批风险或已恢复pending |
| set_approval_policy L1338–1363 | 保存配置基准，用session.events重新计算runtime policy；协调器accessor返回clone | clone不是独立租户；设置策略不重建该coordinator或自动重审已接受请求 |
| session替换提交 L5188–5215 | 撤销inputscope、重建inputport/output store、切session，从configured+新session events重算策略 | 此有界提交段没替换coordinator；本次未完整读取所有session切换前置guard，不能断言任意pending期间切换可达或安全 |

125补充捕获中，TUI L1150–1174的计时暂停依赖本项目真实审批数、canonical_approvals及饱和标志；L3912–3934的status文字不再控制时间。L4062–4079切job清旧审批状态；L4209–4222拒绝旧job事件，终态结束计时；L4341–4403只将有效非终止审批事件纳入canonical ID，非Pending resolution移除镜像，真正coordinator view尚在时仍保持等待；未知retire不改状态，真retire才删除对应canonical镜像并重算计时；超过128的饱和标志仅生命周期clear消除。该计时机制是可见等待时间处理，不改变095的允许条件或持久化门。本次没有运行125测试或宣告其最终验收。

随后主控再次提供9dea→e451的页脚修正：另捕获 `delta/tui-footer.rs`，668438 B / 16361 L / SHA-256 `e45131dcf190d55068e369e55b8a3cc0dda18234ccee2c2ca335c9cefdd15904`；完整读取109行的root-footer-increment.patch。render_approval在未focused时直接返回，由render_footer统一选择文案，优先未保存警告、其次本项目隐藏审批、再后台审批及其他提示，避免后绘制审批文本覆盖警告或残留快捷键。两条新增TestBackend断言分别核resize后整行精确提示/保留草稿与请求、磁盘满警告优先；仅读新增测试，未运行，也不把主控仍在执行的回归写成通过。该增量没有修改095源码或9dea响应/计时语义。

## 对照已独立复核的 Codex 303

复用前核对303正式源码与先前完整阅读、报告manifest身份；以下引用对应原303报告及operation maps，不将其历史测试执行或快照作为095新运行。对照目的在于明确相同交互词汇下的不同契约，而非复制上游授权模型。

| 维度 | Codex approval_overlay.rs 已复核事实 | zenpi当前owner与差异理由 |
|---|---|---|
| 类型与作用范围 | Exec、Permissions、ApplyPatch、McpElicitation分别携ThreadId及不同id/call/server关联；权限/执行前缀/网络规则是不同payload | 095统一Allow/Deny+remember，ToolOrigin+policy/lease约束由core与工具层协作；不应把session权限提升或网络规则决策压成同一种记忆允许。owner124/安全门 |
| 默认与确认 | SelectionList初始index0可直接Enter选择首项；选项键可直接提交，调用方提供决策列表 | TUI初始Deny，y/n选择后Enter提交；typed/headless显式命令直接响应是有意接口差异。owner124/122 |
| 队列 | enqueue push、advance pop产生LIFO；无显式容量/去重；完成对象新enqueue不会自动reset done | 095 pending为BTreeMap字典序drain一次，TUI为项目Vec128；两者都不能口头称可靠FIFO，owner124/132应明确溢出和生命周期 |
| 结束与取消 | overlay current_complete/done只是局部状态；Ctrl-C取消当前并清队列，源回调可有重复发送边界；父BottomPane拦Esc走cancel不同于直接调用overlay按键 | 095 cancel_all拒绝pending，emergency加epoch；TUI Esc保留请求而非取消，Ctrl-C由宿主执行取消。需要host而非只看widget测试。owner124/132 |
| 持久化 | 上游app sender发送失败日志化，局部done不等于服务端确认；overlay不是durable writer | zenpi accepted也只是接收，真正审批记录在core；TUI submitted/headless accepted不可宣传执行或持久化成功。owner095/124 |
| 记忆与选项 | 上游session权限/执行前缀/网络允许有专门类型，formatter与选项构造不认证来源；有同键第一匹配和传入选项组合限制 | zenpi记忆按tool、session重放、配置Deny优先、worker禁止remember；显示文案与origin字符串本身不构成授权。owner124 |
| 显示上下文 | 上游label控制线程导航/部分history抑制、可打开完整文本，源码没有由这些UI属性保证正确归属 | zenpi项目分页、后台approval提示、请求tuple校验已存在；布局/完整diff/鼠标/小终端属于tui，095不绘制屏幕，不虚报本模块有render逻辑。owner123/124/125 |
| 测试归属 | 303共19源测试、5快照，若测试名声称隐藏某选项却从未传入该选项，只能认定实际断言覆盖输入 | 095六源测试和本次有界宿主测试逐项列下；不合并成25测试通过，也不拿静态快照当PTY交互证据 |

## 源测试逐项及行为判据

六个inline测试本次全部读取；按源码文本计14个普通assert宏位置、0快照，未执行任何测试。循环中的assert只算一个文本位置，不冒充多条独立测试。函数体字节区间与hash由source-tests.json绑定；函数签名包含泛型persist_accepted<E>，function-bindings不会漏计。

| 测试与区间 | 真正输入/断言 | 未覆盖与下一次可运行判据（本次不执行） |
|---|---|---|
| T01 explicit_deny_beats_read_only_trust_and_worker_allow L602–618 | 3种模式ReadOnly/TrustedWorkspace/Worker，readonly工具显式Deny，即使preflight=true仍Deny；1文本assert | 不是全部7模式遍历；可用参数表补全模式×sideeffect×per_tool×trusted×preflight，逐一核优先级 |
| T02 worker_mode_without_preflight_denies_even_remembered_and_readonly_tools L621–631 | worker+记忆Allow+readonly、decide(false)返回Deny；1assert | 不执行真实lease/gate预检；需要core真实绑定有效/无效对照，不能只传true模拟安全授权 |
| T03 read_only_is_always_allowed_and_headless_denies_side_effects L634–647 | 空policy Headless readonly允许、workspace write拒绝；2assert | 名字的always不是全域事实；应明确显式Deny、worker及Headless rememberedAllow例外 |
| T04 coordinator_emits_once_and_correlates_response L650–689 | 克隆真实协调器、线程request，轮询drain得到call_id、二次drain为空、respond Allow后join返回Allow；3assert | 等待循环无总timeout；不测错误project/重复/取消/IO。判据需deadline并核exact请求、三条排队键顺序和跨协调器未知ID |
| T05 accepted_response_is_retained_for_durable_ack_and_first_decision_wins L692–741 | request_response、Allow remember；第二Deny Unknown；accepted Allow；mark后join得到Allow/remember；4assert | 先mark后join不证明线程在mark前被阻塞，且没有journal。判据应显式握手观察response可先返回、调用方写账失败阻止dispatch |
| T06 cancel_all_retains_an_unrevealed_denial_for_durable_ack L744–779 | 无drain/reveal，线程cancelled闭包恒false；等pending后cancel_all；accepted Deny；mark后join Deny且不remember；3assert | 不测emergency epoch、闭包清全状态或持久化。判据区分普通取消生成拒绝和真正整轮取消返回Cancelled |

上下文 `tests/tui_approval_focus.rs` 完整静态读到12个测试：默认Deny/确认保草稿；Esc与paste不授权；多请求复位和项目隔离；后台提示两布局TestBackend；worker记忆禁用与Release不确认；diff滚动/resize；真实coordinator错误owner/错call/重复；取消后stale可见请求；非法请求和128容量；会话切换记忆只恢复本会话且配置Deny保留；重放过滤worker/缺call字段/配置Deny；目录picker优先。它们并非095 inline测试，也不是本次运行结果。特别是TestBackend屏幕断言不证明原生终端按键模式或PTY恢复。

上下文 `tests/approval_owner.rs` L1–203使用模拟Backend产生一次write_file，但工具注册、SessionStore临时文件、审批线程和写入是实际接口：缺coordinator产生错误；Deny后note.txt不存在且有approval_resolved；remember Allow后内容正确且日志decision/remember/digest/摘要preview对应，patch不入日志。测试名executes_once的可见断言主要是最终内容和记录字段，没有额外工具调用计数，不夸大为所有故障下恰好一次。L385–400的host_emergency_cancel核process返回Err、无文件、pending消失，关联core读取epoch；本次仅读源码，不复跑这些可产生文件的测试。

## 功能结论反向覆盖与待证明边界

下列每项都有正式源码范围、真实owner及可运行判据；对应operation-maps.json。它们是审查结论和后续验证入口，不是擅自扩展本次产品修改范围。

| 映射 | 源范围与关联owner | 结论/判据 |
|---|---|---|
| M01 数据输入 | L1–121；124/core/tool preview | 结构输入边界清晰，证明标准应包含空白/129字节/多字节/控制字符、非法digest/lease、非object、worker缺字段、preview错误；脱敏和来源认证另审 |
| M02 共享状态构造 | L122–164；124/132 | clone同一owner，new独立；需双协调器同ID与clone共享对照，验证无跨owner接受；不虚称队列持久化 |
| M03 请求等待 | L165–243；core富响应路径/132 | 富/简接口记忆职责不同；需有deadline的线程握手验证取消先决、无response阻塞、response先于durable可返回 |
| M04 展示与恢复快照 | L244–289；TUI/headless | drain字典序只发一次，reveal不授权；需A/B/C键顺序、重复reveal及中毒可观察性判据 |
| M05 响应/归属 | L290–343；TUI1344/headless6335 | 原子接受不要求visible；worker记忆拒绝不消耗pending；同ID重复仅当前生命周期拒绝。结合错误项目、错误call、过期ID与首响应对照 |
| M06 观察状态 | L344–368；宿主reconcile/core | pending/accepted只描述集合，不描述durable/执行；测试响应取走后的accepted仍在，并区分未知/poison/取消 |
| M07 持久化协作 | L369–404；core2590/4240 | callback在锁外、mark无IO；需可控写账错误、mark失败、并发persist及panic边界；实际副作用必须在调用方成功账本路径之后 |
| M08 取消 | L405–444；core3303/host interrupt/EOF | 普通Deny与epoch取消不同；需已接受Allow之后cancel_all、emergency在返回前后、多个pending及闭包清集合对照；不承诺kill已运行进程 |
| M09 撤回 | L445–464；persist错误处理/124 | 未显示无恢复、有显示不重发、消费后撤回Unknown三种情况独立核；避免把retract注释当可可靠重试契约 |
| M10 策略默认与优先级 | L465–525；core gates | 六个优先检查加剩余None完整表；需七模式全组合，preflight bool外部真实gate不被简化跳过 |
| M11 会话记忆 | L526–567；core配置/会话切换 | 仅可信human origin事件、基准Deny保护；需last valid wins、基准Allow/deny、空格ID、不同session、worker、多次恢复基准区别 |
| M12 低层mutator/错误 | L568–590；调用方 | remember无guard，错误类型无超时；测试直接覆盖与非法tool输入不能误用为用户可达漏洞证据 |
| M13 策略源测试 | L591–648；095/124 | T01–03实际3模式/worker false/Headless两sideeffects，共4文本assert；测试名边界已逐项纠正 |
| M14 并发源测试 | L649–780；095/132 | T04–06共10文本assert；源循环有无界等待，mark-before-join不证明durable阻塞，cancel恒false不证明epoch |
| M15 宿主体验 | TUI a19及9dea有界delta；122/123/124/125 | 选择与提交分开，Esc保留；失败响应不退役；计时依真实等待集合。需要真实host错误分支及原生UI交互回归，不能只靠helper测试 |
| M16 跨实现与历史 | 已审303+完整旧095报告；303/095/master | 仅复用同identity语义，不混合源读数/旧HTTP或PTY结果；所有新运行数量为0，最终G-FILE由master审阅 |

文件没有UI绘图、键绑定构造、网络权限细分、MCP elicitation表单、进程启动、session文件恢复或tool preview内容生成，这些不是漏读分支，而是相应TUI/工具/core/session owner的职责。095的导入、serde默认、类型、全部函数、全部错误、cfg(test)及六测试均纳入连续semantic-units；符号数量只帮助核对完整性，以上状态和判据才构成功能审阅。

## 旧证据保留与本次验证边界

完整保留旧target095-review320-ready的204个普通文件和外部zs1-095-ready的61个普通文件（共265），并完整新读其中20641 B/102行和28613 B/148行报告。继承工作树report与旧local报告同字节。旧报告有基线/当前两次阅读、旧782全suite、8 approval_owner、11 TUI/13 palette、legacy 14/12 HTTP及后续main policy-first 6组17 HTTP等叙述，均只属于当时binary/脚本身份；这里不重新运行、不合并为当前结果，也不将HTTP TOOL_RESULT先后关系说成OS写入瞬间证明。

旧材料同时包含integrity-final/integrity-reviewed两次因可变helper漂移失败、之后冻结检查通过；已保存的较晚helper不冒充失败时原始执行脚本。外部旧G-FILE/G-STAGE尝试有master receipt缺失的失败，结构true并不等于验收。旧文档的不准确行数或泛化测试名字在正文纠正，原文与原始失败仍完整保留，不洗掉或重跑历史。

本次读取准备的第一次宽泛authority搜索输出被截断，已在preparation-issues.json记为无全文阅读信用，随后只读相关明确行。另一次新读页脚patch误请求L1–160（实际109行），read.py在写ledger前AssertionError退出；原命令、exit与traceback保存在preparation-footer-read-failure.log，随后按真实L1–109完成读取。该准备错误不删除，也不是冻结审计重跑。新build.py另发生一次metadata字段名冲突：identity的lines数量覆盖unit的lines区间，生成unit后在函数归属查找时报TypeError；原脚本、部分输出、命令/exit/traceback完整保存于preparation-build-v1，随后新build-v2.py排除标量lines覆盖。失败发生在候选与审计生成前，没有运行产品或冻结审计。新的verifier在冻结前静态阅读并用AST核对变量生命周期，避免既往305审计中manifest变量被walrus覆盖导致最后检查异常的问题；305失败本身不修复或重跑。冻结包只运行一次新的离线结构审计，stdout/stderr/exit/time/hash在包外独立保留。该审计只在临时目录应用并回退这一个文档补丁，核source、read片段、函数/测试绑定、历史保留、owned scope和包完整性，不调用Cargo、PTY、HTTP、旧runner、release、预算检查或主库产品。

提交master审阅时应同时查看本文功能判据和包外审计结果；即使离线审计全部通过，也不能自动勾选蓝图095、修改登记基线身份或声称已证明所有并发/崩溃/持久化故障路径。唯一回退是撤销当前创建的 owned 报告，历史材料与产品代码不在补丁内。

## 全函数索引与读取核对

以下为完整源码的29个fn声明（23生产函数、6源测试）。索引包含persist_accepted<E>泛型；Default同名函数依行号区分。状态/输入/错误/副作用结论已在前文逐一说明，索引不替代语义。

| 函数 | 行区间 | 类型 | 语义映射 |
|---|---|---|---|
| `validate` | 59–104 | production | U04 / M01 |
| `default` | 150–152 | production | U07 / M02 |
| `new` | 156–161 | production | U07 / M02 |
| `request` | 166–174 | production | U08 / M03 |
| `request_response` | 181–188 | production | U08 / M03 |
| `request_inner` | 190–242 | production | U09 / M03 |
| `drain_pending` | 247–264 | production | U10 / M04 |
| `reveal` | 269–287 | production | U11 / M04 |
| `respond` | 291–293 | production | U12 / M05 |
| `respond_with_request` | 297–341 | production | U12 / M05 |
| `is_pending` | 346–350 | production | U13 / M06 |
| `accepted` | 355–364 | production | U14 / M06 |
| `persist_accepted` | 370–387 | production | U15 / M07 |
| `mark_persisted` | 389–403 | production | U16 / M07 |
| `cancel_all` | 405–430 | production | U17 / M08 |
| `emergency_cancel` | 434–437 | production | U18 / M08 |
| `cancellation_epoch` | 439–441 | production | U18 / M08 |
| `retract_response` | 446–462 | production | U19 / M09 |
| `default` | 475–481 | production | U20 / M10 |
| `decide` | 485–487 | production | U21 / M10 |
| `decide_after_preflight` | 491–523 | production | U21 / M10 |
| `with_remembered_events` | 527–566 | production | U22 / M11 |
| `remember` | 568–570 | production | U23 / M12 |
| `explicit_deny_beats_read_only_trust_and_worker_allow` | 602–618 | source-test | U26 / M13 |
| `worker_mode_without_preflight_denies_even_remembered_and_readonly_tools` | 621–631 | source-test | U27 / M13 |
| `read_only_is_always_allowed_and_headless_denies_side_effects` | 634–647 | source-test | U28 / M13 |
| `coordinator_emits_once_and_correlates_response` | 650–689 | source-test | U29 / M14 |
| `accepted_response_is_retained_for_durable_ack_and_first_decision_wins` | 692–741 | source-test | U30 / M14 |
| `cancel_all_retains_an_unrevealed_denial_for_durable_ack` | 744–779 | source-test | U31 / M14 |

连续semantic-units共31段覆盖L1–780，含所有空行/注释与测试配置；read-binding的四个正式片段覆盖0–29396字节。上下文和历史读取独立列账，不计成正式源码第二轮阅读。source-tests的14普通assert文本位置、0snapshot与本次runtime执行0分别记录，不能换算成测试通过。
