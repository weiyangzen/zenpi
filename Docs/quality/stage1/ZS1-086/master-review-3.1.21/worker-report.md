# ZS1-086 · src/view_model.rs · 3.1.21 全文件复核候选

本项完成当前 main `src/view_model.rs` 的连续全文审读及必要宿主接线复核，只提交本文件学习报告候选，等待 master 独立验收。产品代码、main、蓝图、authority、claim 和 receipt 均未修改。没有新增 Cargo、PTY、HTTP、产品行为或历史 runner 执行；离线证据核验单独计数，不能作为产品通过数。

当前 ViewModel 将 core/provider 输出变成 transport-neutral 数据，并提供显式字段校验、单块累积以及有界内存事件 replay。它没有项目所有权、工具执行权限或审批决策能力。TUI 依靠自己的 job/project 状态投影这些事件，不能从 ViewModel 的枚举或序号推导实际执行成功、审批已生效或全会话隔离已验证。

## 输入身份、继承与读完范围

| 输入 | 字节 / 行 | SHA-256 |
| --- | --- | --- |
| 蓝图冻结基线 | 40591 / 1220 | `1089a9f352b5aaebc755b51ece8189c1daaca1aa82060d2f009630b915b3abee` |
| 本轮 main 当前源 | 42009 / 1255 | `094833f051ab66d30ce3f6cc74fcc0334ed938e79f5fd8e9269341b62bed08fe` |
| 当前外部 tests/view_model.rs | 12246 / 425 | `4cadeff6ed161894a4e7265170b2461621c6ebc24f09db7770f26066c9f9c9c3` |
| 旧 3.1.20 报告 | 13693 / 86 | 身份保存在完整 history 清单中 |

当前源捕获时间为 `2026-09-13T01:24:48.052166+00:00`。连续阅读 1–250、251–500、501–750、751–1000、1001–1255，覆盖全部 42009 字节；外部测试连续读 1–220、221–425。共 44 个真实函数声明，包括 `pub const fn kind`；源内测试为 0，外部测试 11，普通 assert 宏位置 52，snapshot 0。断言宏位置不是全部行为断言数，unwrap 的成功要求亦存在；这些均为源码审读，执行次数 0。

旧 086 ready 从 worker A 的 38de 工作树完整保留 16 个普通文件；旧报告 86 行本轮全文读完。它的 current 源与本轮当前源完全相同，但其基线全文阅读和旧测试结果不重复计入本轮。086 基线保留并计算精确 diff，本轮不宣称重新全文审读基线。旧压缩档保持原始字节，不解包、不执行，不把旧 108 的测试或旧 headless 接线当当前通过证据。

捕获 authority 为 3.1.21、50/121，蓝图 SHA `aa9412fe226f42c2dc6898b09cf94ba34e76ee26f268abd4d43d13fad57e7dc1`。requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`。蓝图 115 行与 source_manifest 第18行仍引用冻结基线，204 行规定唯一 owned report。main 捕获时该报告不存在，交付补丁为仅此路径的创建，回滚仅删除此新报告；并非替换当前源。

## 基线到当前的确切变化

净增 1418 字节、35 行；精确 diff 保存在 baseline-to-current.diff。变化为 ReasoningDelta 的枚举、校验和 turn/block 关联，以及 Provider reasoning 独立块与 Agent ToolProgress 的哈希块接入。其他逻辑仍须按当前完整源码理解，不能只看增量。

ReasoningDelta 总是派生 `turn-prefix:reasoning`，忽略传入 block_id。普通 TextDelta/TextDone/Refusal 优先采用显式 block_id，否则派生 `turn-prefix:assistant`。Reasoning 与 assistant 在正常短 turn ID 下区分；私有默认 ID 通过截短 turn 前缀保持 UTF-8 完整，总长不超过 128 字节。不同长 turn ID 可能共享截断后的 block_id，消费者需以完整 turn_id 与 block_id 联合索引。默认 ID 不是哈希唯一键。

ToolProgress 的键为 `output-` 加 SHA-256(turn 字节 + NUL + call_id 字节 + stdout=0/stderr=1 标签)。request_id 不参与，同一 turn/call/stream 快照稳定更新一个块；最终 envelope 校验 turn ID，不能假设任意上游字符串均已合规。载荷交给 `OutputProgress::canonical_kind`，本轮未读该实现全文；旧报告对快照正文截断的描述作为旧证据保留，不能把它升级为本轮 tool_output owner 的完整结论。

schema version 保持 1，但新增 reasoning_delta 会使不识别该 variant 的旧 decoder 失败。版本值未变不能证明所有旧消费者兼容。

## 预算、校验与脱敏

| 层级 | 明确限制 | 限制未覆盖的内容 / 验收标准 |
| --- | --- | --- |
| ID / token | ID、工具名、错误 code 128 B；language 64 B；diff path 4096 B | trim 后为空拒绝，但保留合法字符串原空格；非路径授权、非工具注册校验；补验边界和全空白 |
| 单 text | 256 KiB；空串允许；LF、CR、TAB 允许，其余 is_control 拒绝 | 没有自动 terminal sanitize、bidi/format 字符统一过滤或脱敏；T04只测 ESC 与超长 |
| List | 非空，最多128项，每项独立 text 限制 | 没有列表总字节限额，理论文本可达32 MiB；补验0、128、129项及聚合量 |
| ViewMessage | 非空，最多128块，每块 validate，可选 turn_id | 没有消息总字节预算；128个最大 List 理论文本聚合4 GiB，实际分配受宿主限制；不是已执行压力结果 |
| ViewEvent | schema=1，关联条件，编码后最多512 KiB | serde_json::to_vec 已分配完整 JSON 才检查，转义扩张计入；不是反序列化前输入上限 |
| ViewEventBuffer | capacity / max_bytes 最小1；按编码事件字节驱逐 | 不计结构、String容量、返回clone与墓碑堆开销；无配置上限，需宿主合理配置 |

错误 TooLarge 的 max 文案写 bytes，但 blocks/items 传入数量；不能把错误显示误当字节精度计量。validate_text 先长度再控制字符，validate_token 先空白再长度再控制字符，错误优先级可观察。validate_optional_* 仅校验 Some。Heading level 必须1–6；Code language 允许内部空格，只排除控制和全空白。Diff 不检查文件存在或 patch 语法。Approval arguments 为字符串，不要求 JSON；ToolCallReady 的直接构造也是字符串，而 provider adapter 从 JSON 值序列化而来。Usage 不要求 total=input+output。

所有公开字段与派生 Deserialize 允许绕过构造器语义校验。构造器主动 validate，解码后的消息和事件仍须由接收者显式 validate；deny_unknown_fields 的结构限制不能替代预算、schema、必需关联或授权。ViewEvent 的 event 为 flatten，未知字段的具体 serde 行为本轮没有动态实验，不作超出源码的兼容承诺。

redacted 返回新块，正文、列表每项、代码文本、diff patch、tool output、approval arguments、error message 通过 shared security::redact_text，known-secret 参数为空。language/path/call_id/name/approval_id/tool/code 等结构字段保留，Rule 无改动。该方法不再次 validate，未审读 shared security 的全部识别规则。StreamingBlock、ViewMessage、ViewEvent、buffer 与 adapters 不自动调用它。T02仅验证一个 Error Bearer 样例隐藏且保留形状，不能证明所有秘密、字段或输出路径都已脱敏。没有依据认定当前宿主已泄露真实凭据。

## 11类块、Markdown 与增量冻结

PlainText、Paragraph、Quote 校验 text；Heading 附 level；List 附 ordered/items；Code 附 optional language；Diff 附 optional path；ToolStatus 附 call/name/status/optional output；Approval 附 id/tool/arguments/state；Error 附 optional code/message/retryable；Rule 无载荷。kind 的11个分支一一对应。角色仅 User/Assistant/Tool/System/Error，Reasoning 不是 ViewRole，而是 ViewEventKind 单独增量类型；TUI另有 MessageRole::Reasoning。

from_markdown 把现有 MarkdownBlock 克隆为这些共享块，然后 validate；每个 parser list item 单独转换为一个单项 List，保留 ordered 布尔但不保留源数字编号。from_markdown_text 先调用 render parser 再检查最多128块，然后逐块转换；空输入可以返回空 Vec，ViewMessage::from_markdown 随后的 new 会拒绝空块消息。该解析前置的输入截断/sanitize 来自同 hash 的 096 render 审读，直接构造 ViewBlock 不走 parser，二者不能混淆。

StreamingBlock 新建先校验 id。append 先拒绝 completed，再校验整个 delta，再用 saturating_add 检查累计长度，最后追加；无部分截断，非法或过长增量保留先前文本。complete 也先拒绝 completed；None 保持全文，Some 先校验再 clear+替换（合法空串会清空），成功后置 completed 并返回 Paragraph 副本。Some 的额外长度分支被先前 validate_text 的同一上限涵盖。任何校验失败前不清原文；不存在 reset/reopen，clone 是独立状态。T06覆盖 hel+lo、complete(None)、完成后append/再次complete拒绝，未覆盖Some替换、失败保持、累计上限和clone。

096 的 tail renderer 负责显示窗口的有界保留、grapheme 宽度和省略行；ViewModel 不保存宽度、滚动位置或持久锚点。头部 parser 接受前256KiB，尾部窗口是该已接受前缀的视觉后缀，并非恢复被输入字节截断的内容。窗口重新排版和正文恢复不应通过复用 StreamingBlock 的 completed 状态来推断。

## 20类事件的关联、合法状态与未覆盖标准

表中 turn/block 指 validate 的必填条件；无必填仍会校验显式提供的 ID。公共 envelope 不验证这些 ID 对应真实项目、会话或正在运行的任务。

| 事件 | turn / block | payload 条件与分支验收 |
| --- | --- | --- |
| RequestAccepted | 必填 / 可选 | mode三枚举；T07 StartIfIdle，另补StartOrSteer/Steer |
| RequestRejected | 可选 / 可选 | reason枚举；T07 NoActiveTurn，补其余6种 |
| TurnStarted | 必填 / 可选 | optional response/model token；补None、空Some、超长 |
| TextDelta | 必填 / 必填 | text；T05缺turn、T07默认assistant，补独立缺block与边界 |
| ReasoningDelta | 必填 / 必填 | text；补显式block被忽略、无turn、Unicode长ID |
| Block | 必填 / 必填 | block.validate全部11类；T07 Paragraph，补其余与非法嵌套 |
| ToolStarted | 必填 / 可选 | call/name token；T07工具投影，补unknown call lifecycle |
| ToolCallDelta | 必填 / 可选 | optional call/name，arguments_delta text；二者None可合法，补分片非JSON |
| ToolCallReady | 必填 / 可选 | call/name token、arguments text；T07 provider JSON串，补直接非JSON与超大 |
| ToolFinished | 必填 / 可选 | call token、optional output text；Queued/Running也为合法状态；补各状态与没有Started |
| ApprovalRequired | 必填 / 可选 | id/tool token、arguments text；补非法字段及无实际coordinator请求 |
| ApprovalResolved | 必填 / 可选 | id token；Pending合法，不要求先Required；补unknown/duplicate/Pending |
| Usage | 可选 / 可选 | 三u64照传，无算术一致性条件；补不一致计数 |
| Handoff | 可选 / 可选 | id token、optional to token；不自动结束turn；补None/非法Some |
| Warning | 可选 / 可选 | text，可无turn；T10作为终态后拒绝样例，补其他上下文 |
| TurnCompleted | 必填 / 可选 | optional response/model；T10终态，补provider多response续轮 |
| TurnFailed | 可选 / 可选 | optional code、message，retryable布尔；补无turn不建墓碑与两adapter不同retryable |
| TurnCancelled | 可选 / 可选 | optional reason text；补None和无turn墓碑行为 |
| Dropped | 可选 / 可选 | count必须>0；stream四枚举；补0拒绝与各流，buffer不自动制造该事件 |
| Closed | 可选 / 可选 | 无payload限制；T11关闭后拒绝；补携带已终态turn的拒绝顺序 |

总计12种必须 turn，3种必须 block。ViewEvent::new 不提供关联，不能用于需要 turn/block 的事件；with_context 统一生成 schema 并依次检查版本、所有可选ID、载荷、所需turn、所需block、编码大小。不存在sequence单调校验于单个event.validate中；该条件只在buffer.append执行。

## 两组投影以及 provider 完成的语义

AgentEvent 九分支：TurnAccepted→RequestAccepted；TurnRejected→RequestRejected无turn；AssistantMessage→Paragraph Block带assistant默认ID；Handoff→Handoff无turn；ToolCall→ToolStarted；ToolProgress→哈希block与委托payload；ToolResult→Succeeded/Failed且output=None；Provider→交给provider adapter；Error→TurnFailed、code=None、retryable=false且无turn。adapter克隆内容，不自动截断或脱敏；最终验证失败会返回Err，而不是静默修正为合法事件。

ProviderEvent 十一分支：ResponseCreated→TurnStarted；TextDelta→TextDelta；ReasoningDelta→ReasoningDelta；TextDone→Paragraph Block；Refusal→Error Block(code=refusal,retryable=false)；ToolCallDelta照传optional call/name与args片段；ToolCallDone→Ready并序列化JSON arguments；Usage照传；Warning照传；Completed→TurnCompleted；Failed→TurnFailed(code=provider_failed,retryable=true)。TextDone仅生成数据块，不调用StreamingBlock.complete。Refusal是Block/Error而非TurnFailed，buffer不会因其类型自动设终态。

除Reasoning外，显式block_id对非内容事件也保留并被validate；省略block仅为TextDelta/TextDone/Refusal派生assistant。Reasoning只能由turn派生，给block但不给turn仍失败。Provider Failed可无turn，Completed要求turn。retryable表示展示建议，不是重试授权。

core有多response工具循环，单次provider Completed不等于整个runtime job结束。直接把每个Completed都追加到同turn的ViewEventBuffer，会把该turn放入终态集合并阻止下一轮事件。当前TUI的直接Provider drain分支在模型信息更新后跳过Completed，最后由owning RuntimeEvent::Completed处理全轮结果。Agent::Provider仍由from_agent_event代理，其路径不可从直接Provider的跳过条件推导为全部过滤；完整生产者闭环超出已读片段。core 3276–3289先捕获active_turn_id再收集provider事件，避免成功/失败清active后丢关联；3532–3533最后发布AssistantMessage。live工具事件有sink则发送，否则放events；进度队列drain后无sink不积累快照。均不代表这些路径已运行验证。

## 缓冲区状态、丢弃与恢复

new 把两预算0归一为1，next_sequence=0、bytes=0、dropped=0、空deque/终态集合、closed=false。访问器只读；iter借用；take_dropped清计数但不改变事件、cursor或终态。clone复制整个内存状态，之后各自独立；不是共享订阅。

push 使用当前next构造event，然后append（重复validate）。append顺序：event.validate → closed → sequence必须恰好next → 已知终态turn拒绝 → encoded_len与buffer单事件预算 → 若有turn的Completed/Failed/Cancelled记录墓碑 → Closed置true → checked_add更新next → 加字节、入队 → 超数量或超字节则从最旧逐个移除，dropped饱和累计。validate发生在closed检查前，非法输入对关闭buffer可能先返回字段错误；带已记终态turn的Closed可能先被TerminalTurn拒绝而未关闭。早期错误不消耗序号；不能泛化成全部错误严格不变。

next_sequence达到u64::MAX时，checked_add返回SequenceExhausted之前可能已经修改墓碑或closed，事件尚未保留。next私有且从0开始，当前公开API没有短程到达该状态的路径；这是理论错误原子性边界，未作为实际复现或高风险结论。T09只覆盖普通SequenceDiscontinuity不修改状态。

终态记忆最多256个带ID的turn，独立于事件队列驱逐或drain。第257个淘汰最早墓碑后，旧ID可再次进入；无turn Failed/Cancelled不会记录任何墓碑。没有started集合，没有call/approval先后转移，也没有项目命名空间或永久去重。T10只覆盖一个已完成turn拒绝后续Warning、另一turn可开始，不覆盖256边界。

retained_bytes按序列化长度计算。驱逐重新编码最旧事件并用unwrap_or(0)处理编码错误；它不是实际常驻内存测量。单event在入队前已≤max_bytes且capacity≥1，常规算术条件下新event自身可留存，但不应据此承诺任意配置的总内存安全。

drain返回所有保留event并将bytes清零，保留next、dropped、墓碑、closed；没有reopen或会话reset。replay_from先判future，再看空队列/首序号：大于next为Future；空且小于next为Gap(first_available=next)；非空小于首序号为Gap；否则克隆所有sequence≥请求值的事件，因此inclusive，等于next返回空。已关闭仍可replay保留记录；返回clone的内存不计在buffer预算。丢弃正文与drain后的数据不在此层可恢复，需上层canonical message或持久化记录；本文件没有磁盘重连、网络游标或跨进程恢复。

## 当前 TUI 的实际投影、重置和项目归属

TUI 捕获 `668438 B / 16361 L / e45131dcf190d55068e369e55b8a3cc0dda18234ccee2c2ca335c9cefdd15904`，与096审读输入同hash。当前并行主线F2会话列表/mouse与B122F1不归本报告；冻结时重新比较origin，若漂移独立记录，不覆盖本捕获或借用未读新行。headless及另外两份external测试仅捕获身份，没有本轮正文阅读credit，不替换084。

begin_stream_for_job在job变化时清旧审批活动、canonical镜像并重置计时；之后无论job是否相同都清reasoning归属、streaming role/started，并重建StreamingBlock。因此同job重复begin也会重置累积内容，不能视为幂等继续。append_stream_for_job先拒绝job不符和空chunk；有StreamingBlock则append失败直接return，没有部分追加；首段新建消息，后续通过最近同role消息追加，不用event.block_id定位assistant正文。reasoning独立按reasoning_job找到最近Reasoning消息，并按MAX_MESSAGE_BYTES剩余空间UTF-8截断，行为与StreamingBlock整体拒绝不同。

finish_stream调用block.complete但忽略其错误，仍按bound_text完成显示并清job；finish_stream_for_job只核对job并使用finish_stream_impl，未调用block.complete。complete_stream_for_job取最近同role内容作为最终内容；discard只清stream状态，已显示部分消息仍保留。因此不能把所有TUI结束路径说成由StreamingBlock冻结验证成功。runtime成功若有assistant.content，以完整结果替换provisional；无assistant则discard。最终结果自身也受TUI文本上限，不能宣称无损恢复任意长回答。

apply_view_event_for_job入口只查streaming_job_id，未统一重新validate、比较schema/sequence/request/turn或project；它依赖上游normalization。审批两个分支额外event.validate且要求activity未结束。TextDelta追加assistant；Paragraph只在尚未开始streaming_message时采用，已有delta时TextDone不覆盖；BlockError或TurnFailed结束Error消息；ToolStatus只处理Some(output)，按job与block组合键替换；ToolFinished再按call_id更新当前job的工具块状态。其Queued/Running显示为Running，不证明工具仍真实运行。

ApprovalRequired把ID记入最多128的canonical镜像、暂停计时并显示Pending块；重复ID不增加镜像，饱和时不再记入ID并设置饱和标记，但仍显示审批块。ApprovalResolved非Pending才移除镜像并可能恢复计时，显示系统消息，不直接响应coordinator，也不把原Pending块改成决策结果。TUI内联测试14773–14815显式断言canonical resolution不能退休真实请求；14818–14841检查旧job不能暂停、恢复或取消替代job。这是测试源码证据，非本轮运行结果。

Usage只更新当前job计数；Dropped显示丢弃警告；Cancelled/Closed discard；Completed完成当前消息；RequestAccepted/Rejected/Started/Handoff/Warning等在这个match中无通用显示处理。每次drain的view_sequence重新从0开始，只对成功规范化递增，不能当全会话replay cursor。normalization错误在drain被跳过；直接Provider Warning额外推送System消息在apply之外，不能用apply的job guard证明这个辅助显示分支也逐项检查job。真实外层retain仅保留active_job缓冲，且过期RuntimeEvent::Completed清除旧buffer后continue，减少迟到事件进入显示的机会。此处没有伪造输入实验，不认定存在已复现跨项目泄漏。

ProjectDraft保存/恢复streaming_job_id、role、started、StreamingBlock；主循环12208–12223临时选中active_job_project再drain。当前读取说明项目归属在宿主层，并非ViewEvent字段。没有项目id嵌入envelope，不能复用不同项目但同turn/block字符串到一个共享索引而假设安全隔离。多项目切换、替代、取消和后台完成需由owner完整测试闭环支持；本项只读取相关保存恢复与drain片段，不申领整个TUI owner完成。

TUI内联canonical_view_events测试15260–15317覆盖hello delta、tool row、Paragraph不结束stream及Completed清job，但没有断言完整block覆盖hello；canonical_approval_and_drop测试15320–15360检查Pending可见和Dropped提示。15225–15257仅为此前overflow测试尾部片段，不能计为完整测试函数。两项审批计时完整函数和两项canonical完整函数均只作为局部上下文阅读，不混入086零内联/11外部的统计。

## 同 hash 的096 / 308 / 309复用与产品责任

完整保留096 ready 564、308 ready 149、309 ready 216普通文件，连同旧086总945文件。reference-reuse.json记录旧manifest、正式源身份和本轮live同字节检查时间。没有重新执行旧auditor或旧测试，不把这些参考文件重新计成086源码全文。

096 render贡献的边界是输入截断、sanitize、grapheme包裹与尾部省略元数据；086只产出typed块与事件，不带窗口位置。308 key_hint只负责键形状匹配/格式化，不负责执行审批、绑定registry或clipboard动作；按键提示存在不能证明对应安全状态可操作。309 status_indicator_widget的暂停/恢复是clock展示，其bool不是job identity；调度重绘请求不是执行ACK，interrupt事件发送也不是取消完成。Zenpi的真实job guard、coordinator请求与终结处理必须保持独立。旧报告中的headless路径与历史运行计数保留为历史，不用于填补本轮未读接线。

## 外部测试覆盖、遗漏与验收条件

下附逐函数行号、body hash绑定与断言宏计数。T01只覆盖四类块roundtrip和Paragraph kind；T02只Error样例；T03映射heading、两个list、quote、code和Rule kind；T04仅ESC、129列表项和256KiB+1文本。T05检查缺turn及JSON schema/type/关联/roundtrip，未单独测缺block或恶意Deserialize。T06只None冻结；T07覆盖Accepted/Assistant/Provider文本/Ready/Rejected/Error部分投影；T08容量2驱逐、replay gap/future和dropped清计数，未单独触发byte驱逐。T09普通错序不变；T10一个turn终态；T11Closed。这些不是全分支运行覆盖。

优先补验标准是：所有不可信接收路径解码后validate；aggregate message分配预算；同turn多provider response的终态划分；Reasoning显式block忽略与长ID复合键；ToolProgress三维哈希关联；drain和第257墓碑后的行为；结构字段和嵌入载荷的脱敏策略；TUI重复begin、late warning辅助显示、后台project归属与最终全文替换。每个semantic unit已绑定operation-map中的现有测试或明确未执行criterion；枚举每分支见表。工作范围内不修产品、不增加低价值镜像测试、不运行专项owner门禁。

## 历史失败与证据核验范围

旧086 preflight保留一次离线validator exit1：补充测试声明误标318，实际319，随后旧包修正。旧报告还保留历史108错误在嵌套非git目录应用补丁未生效，以及registry fixture streaming=false导致14 pass/1 fail和Peer disconnected，后续fixture修复通过属于历史。旧压缩包原样保留，本轮未重建其历史39文件输入闭包。不得删失败、执行旧程序或把后续pass覆盖原始失败。

本轮准备阶段只有本地086 locator无匹配返回1，随后有界查找定位38de旧包；不计全文阅读。新auditor先完整静态阅读并AST检查后冻结，最多执行一次，只检查快照、逐字节阅读覆盖、函数/测试/映射、历史完整性、唯一报告补丁正反向以及冻结包不可变。临时补丁应用位于独立scratch，不写main。原始stdout/stderr和执行receipt在ready外；任何失败保留且不重跑。

本报告仍是worker候选，G-FILE / G-STAGE 与master验收结论由协调方独立作出。新离线audit通过只能证明其已实现的机械一致性检查，不能证明Rust产品安全、全部边界运行通过或当前部署已验收。

## 连续语义单元索引

| 单元 | 当前行 | 责任 / 映射 |
| --- | --- | --- |
| U01 | 1–31 | 版本和预算 / schema |
| U02 | 32–105 | 增量块 / stream |
| U03 | 106–172 | 错误与状态枚举 / schema |
| U04 | 173–229 | 块数据形状 / blocks |
| U05 | 230–296 | 显式脱敏 / redaction |
| U06 | 297–373 | 块语义校验 / validation |
| U07 | 374–414 | 种类与Markdown单块 / blocks |
| U08 | 415–426 | Markdown全文转换 / markdown |
| U09 | 427–498 | 消息形状与校验 / message |
| U10 | 499–623 | 事件枚举 / events |
| U11 | 624–705 | 载荷校验 / event_validation |
| U12 | 706–731 | 关联必需条件 / association |
| U13 | 732–802 | 事件信封 / envelope |
| U14 | 803–915 | Agent投影 / agent |
| U15 | 916–1002 | Provider投影 / provider |
| U16 | 1003–1061 | 缓冲区初始化与访问 / buffer |
| U17 | 1062–1132 | 追加与驱逐 / append |
| U18 | 1133–1168 | drain与replay / replay |
| U19 | 1169–1190 | 模式与拒绝原因转换 / conversion |
| U20 | 1191–1206 | 版本与默认块ID / ids |
| U21 | 1207–1255 | token和text校验 / tokens |

## 全部函数绑定

每个函数正文的精确范围、字节数和SHA见function-bindings.json；同名方法按行号区分。

| 函数 | 当前行 | 映射 |
| --- | --- | --- |
| new | 42–50 | stream |
| block_id | 52–54 | stream |
| is_completed | 56–58 | stream |
| text | 60–62 | stream |
| append | 64–80 | stream |
| complete | 82–103 | stream |
| redacted | 234–295 | redaction |
| validate | 298–371 | validation |
| kind | 374–388 | blocks |
| from_markdown | 392–412 | blocks |
| from_markdown_text | 415–424 | markdown |
| new | 455–467 | message |
| from_markdown | 469–475 | message |
| validate | 477–496 | message |
| validate | 625–704 | event_validation |
| requires_turn_id | 706–722 | association |
| requires_block_id | 724–729 | association |
| new | 749–751 | envelope |
| with_context | 753–770 | envelope |
| validate | 772–795 | envelope |
| encoded_len | 797–801 | envelope |
| from_agent_event | 803–914 | agent |
| from_provider_event | 916–1000 | provider |
| new | 1020–1032 | buffer |
| next_sequence | 1034–1036 | buffer |
| len | 1038–1040 | buffer |
| is_empty | 1042–1044 | buffer |
| retained_bytes | 1046–1048 | buffer |
| dropped | 1050–1052 | buffer |
| take_dropped | 1054–1056 | buffer |
| iter | 1058–1060 | buffer |
| push | 1062–1073 | append |
| append | 1075–1131 | append |
| drain | 1133–1136 | replay |
| replay_from | 1138–1166 | replay |
| from | 1170–1176 | conversion |
| from | 1180–1188 | conversion |
| default_view_model_version | 1191–1193 | ids |
| default_block_id | 1195–1205 | ids |
| validate_id | 1207–1209 | tokens |
| validate_optional_id | 1211–1216 | tokens |
| validate_optional_text | 1218–1226 | tokens |
| validate_token | 1228–1239 | tokens |
| validate_text | 1241–1255 | tokens |

## 外部测试逐项绑定

| ID / 名称 | 当前测试行 | assert宏 / 本轮执行 |
| --- | --- | --- |
| T01 · blocks_and_messages_round_trip_with_bounded_shapes | 17–51 | 2 / 0 |
| T02 · block_redaction_preserves_shape_and_hides_credentials | 54–71 | 3 / 0 |
| T03 · markdown_parser_maps_to_shared_blocks | 74–88 | 6 / 0 |
| T04 · validation_rejects_control_bytes_and_unbounded_blocks | 91–126 | 3 / 0 |
| T05 · event_envelope_requires_associations_and_flattens_payload | 129–161 | 9 / 0 |
| T06 · streaming_block_merges_deltas_and_freezes_after_completion | 164–186 | 5 / 0 |
| T07 · agent_and_provider_adapters_preserve_turn_and_block_correlation | 189–263 | 10 / 0 |
| T08 · bounded_buffer_keeps_order_and_reports_replay_gaps | 266–329 | 9 / 0 |
| T09 · buffer_rejects_sequence_discontinuity_without_mutating_state | 332–354 | 3 / 0 |
| T10 · buffer_rejects_events_after_terminal_turn_but_allows_next_turn | 357–406 | 1 / 0 |
| T11 · buffer_rejects_events_after_stream_closed | 409–425 | 1 / 0 |

共29条正文阅读记录，5条正式源连续全文、2条外部测试连续全文，其余为有界上下文与旧报告。19项operation-map将21个连续单元绑定到owner与现有测试/未执行验收标准。完整复制不等于全文阅读。

冻结漂移补记：TUI 更新为669057 B / 16374 L / `7c0d9e5c2cb5f351412aa6e98ac233f5c55580ee1885a431653d8c91b908ba4b`。完整差分为session-list mouse横向边界/滚动索引、history-search排除Super、SessionList单行显示及共享scroll helper；改变的源行未与本轮TUI已读片段相交。新增源单独存freeze-drift，不加全文阅读或运行credit；正文行号仍绑定e451捕获。
