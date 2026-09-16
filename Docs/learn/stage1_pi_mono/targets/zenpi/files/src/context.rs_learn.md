# ZS1-073 — 当前 src/context.rs 完整逐文件复核，3.1.21

Worker 候选，主控独立验收待定。唯一 owned：`Docs/learn/stage1_pi_mono/targets/zenpi/files/src/context.rs_learn.md`。本轮只读研究、私有报告交付，产品运行 **0**；没有 Cargo、PTY、HTTP、旧 runner、release、117/131、FIFO、DTrace、预算或预热执行，没有产品修复。文中测试均是本轮实际读取的测试源码或明确标为历史的日志；所有新增行为判据均未在本轮运行。离线冻结审计如执行，仅验证证据结构，不计作产品测试或接受门禁。

当前 subject：**30,013 字节 / 807 行**，SHA256 `256d9ac1657e272ba61ebfbc242ece2970ed0607f3ebb1583b1711f18b7bf229`。本轮按 **1–300、301–600、601–807** 顺序完整阅读全部内容；没有内嵌测试、`#[cfg(test)]` 或测试模块，内嵌测试数 0。随后全文读取当前三个外部测试文件共 **1,497 行**，包括所有 helper 和条件编译分支；见 reading-ledger.json 的实际阅读记录。初始 capture/chunk-plan 的 unread 状态原样保留，不能拿初始登记冒充完成阅读。

冻结登记基线仍是 **8,199 字节 / 246 行**，SHA256 `bc4ce7999a0f4d162effece8c6f839aca3a230e97675a1d7908fca5d6a7fe909`。基线原文件、旧压缩文件及差异另存；本轮没有以当前 hash 覆盖 blueprint/source_manifest/file index 的基线，也不把旧基线阅读重新计作本轮全文阅读。当前 subject 与三个旧候选记录的扩展实现相同，新增 **21,814 字节 / 561 行**；相同 hash 只证明这一个文件的身份，不证明整个宿主闭包或全部测试未变。

任务捕获 authority **3.1.21**，requirement `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`，snapshot `aa9412fe226f42c2dc6898b09cf94ba34e76ee26f268abd4d43d13fad57e7dc1`，主控状态 50/121。正式依赖只有 ZS1-001；012/013 为源比较上下文。三个 accepted master receipt 与其中 **123 个 artifact 引用**均另存，身份核验不冒称这些 artifact 全文语义阅读。073 接受、claims、blueprint 与 master receipt 仍归主控。

## 旧证据保全与这次的差异

三个旧 ready 均先全量原样复制到 prior，并登记每一文件的 bytes/hash，旧脚本不执行：`target073-ready` manifest 4096 B / `5b0224277186c3b550dfad642a404407f68c1f517e25d1b883f1fa15ecac8337`；`target073-review-3.1.20-ready` manifest 4610 B / `c934a5627654ad5e622a213e420b5fba9d44dd8bc52ef33df2b1ab538121bb87`；`target073-refresh-3.1.20-ready` manifest 7671 B / `8f5b3fe87d980e407db70ffc56943478db0ceabe4b0265b89aab431487d107b8`。旧 canonical 候选 12,667→27,482→46,410 字节的全文及其历史前缀全部保留；本轮重新读了旧 report、appendix、基线报告、说明、assertion-review、五个 probe 和 stdout/stderr。较大旧 replay 闭包和 tar 包按字节保留，不解包执行；旧工作目录保持原状，外置日志另复制。

旧 3.1.16 实测是 4 context + 8 checkpoint + 12 semantic + 5 probe = **29 项顶层通过、0 失败**，两个 ignored 子进程 helper 不重复加总。负向断言中的预期 Err/取消不等于被删除的失败，原 stderr 和嵌套输出均保留。旧 refresh 的单次离线 exit 0 及原 stdout/stderr 也保持原样；这些都不是这次的新通过。

这次当前 `tests/context.rs` 是 2301 B / 77 行，`tests/stage1_context_checkpoint.rs` 是 11776 B / 373 行，两个文件身份与旧包对应内容相符；当前 `tests/stage1_semantic_compaction.rs` 已是 **38971 B / 1047 行**，SHA256 `797df7962c8c50d0f82609e8624ab2d5c8021f8fe6e197b8d9569005bb19cbdd`，包含手动压缩队列恢复的新 parent/helper。当前三文件的静态声明合计 **28 个 #[test]：25 个普通测试声明 + 3 个 ignored helper**，其中 Unix 限定项按源码平台条件解释。声明数不是运行数；本轮一项也没执行。旧 29 不能改成当前闭包的通过数量。

## 责任边界和完整数据流

context 是纯计算层：`turns → estimate / legacy prepare`，或 `完整选中 ancestry → semantic plan → summary request data → Completion 严格校验 → finalize → restore 投影`。它不选择磁盘 session tree、不发送模型请求、不验证供应商账单、不写 journal、不管理运行中 operation、队列 ticket、审批或 UI。core/session/backend/governance 分别承担这些职责。本轮读取它们的有界片段只是证明 073 的消费者边界，不接受其他整文件。

`estimate_tokens` 给出的策略估算和 `Completion.usage` 的供应商报告量有不同来源。前者恒 `approximate=true`；后者是 adapter/调用者提供的 `u64` 数据，即使和式、上限都通过，也不证明计费真实性。文件 SHA 保证被比较字节一致，摘要 SHA 不证明摘要没有遗漏用户要求。`SUMMARY_INSTRUCTIONS` 约束模型如何总结，但不是语义保真或注入免疫证明。

## 全部定义、函数、分支与未运行行为判据

下表每行以本轮完整源阅读为依据；“判据”均是可在获授权的产品测试中构造的输入与可观察结果，不表示本轮运行。实际已读取的外部断言在后表逐项对应。

| 当前行 / 定义 | 行为与分支 | 源测试或具体未运行判据 |
| --- | --- | --- |
| 1–47 两常量、ContextBudget/default、TokenEstimate、CompactionCheckpoint、PreparedContext | 默认 max=128000、reserve=8192。legacy 六字段 checkpoint 无 version/session/branch；返回 owned Vec 是副本。legacy/types 的 serde 派生没有统一 deny_unknown_fields。 | 默认值及字段可逐值断言；mutate 返回 Vec 不影响源。legacy JSON 解码为 semantic 应失败；不要把未标严格的类型声称为全拒未知字段。 |
| 49–63 estimate_tokens | 每条 `content.len()`（UTF-8 字节）+16+metadata JSON 字节；u64 saturating_add；编码失败以 MAX 替代该 metadata 长度；总字节 ceil/4，空输入为0，approximate 恒真。 | current context estimate 只断言正数/approximate；旧 probe 对增加4000字符 metadata 断言至少+1000。新增判据：同可见字符数的 ASCII/中文/emoji 应按不同 UTF-8 字节估计；ID/timestamp 改变不独立计入，但 digest 改变。 |
| 65–79 has_opaque_provider_state | metadata 顶层 signature/thinking_signature 只要键存在，或 annotations 数组元素 type=native_history / 存在 signature 键即 true；null 值也算存在；不递归扫描任意深层 key。 | native 测试覆盖包裹原生 blocks 的 annotation；补 signature:null、错误形状 annotations、无关嵌套 signature。这个匹配器不能承诺识别未知 provider schema。 |
| 81–88 whole_turn_start | 仅 User，且无 parent 或 `metadata.steer.strategy == cancel_reissue`；有 parent 的普通 User 不成为切点。 | 对同一带 parent User 更改该 metadata 值应改变可选 cut；非 User 即使 metadata 相同也不成为 cut。此值没有验证 steer 授权。 |
| 93–112 prepare_context 入口 | max<=reserve 先 InvalidBudget；已 fit 直接 clone/None checkpoint，连 cancel predicate 都不调用；超预算才查一次 cancel。 | max=reserve 和 max<reserve 均拒；fit+永真取消仍成功且 polls=0（旧 probe）；需压缩+取消返回 Cancelled 且 source 不变（current context test）。 |
| 113–143 legacy 后缀 | 所有 System 单独 clone；逆扫非 System，后缀目标 `available.saturating_mul(3)/4`，首条即使超目标也留下；后续加入一条导致超目标则 pop 该较旧条并停止。first kept ID 的首次原位置定义 cut；没找到则 len；cut=0 报 BudgetExceeded。 | deterministic 测试验证新尾部和预算；重复 ID 不会被此层验证，可影响 position；全 System、单条巨型消息、System 已占满预算需分别检查最终拒绝/可切行为。该目标算式在 u64 饱和区域不等价数学实数的3/4。 |
| 144–196 legacy hash/summary/最终预算 | 覆盖 `[0,cut)` 整 Turn JSON；数四类 role，生成固定 count+SHA 文本，无旧语义。summary ID 用 digest 前16字符，timestamp 取 prefix 最后记录；metadata 保存范围/hash。输出所有 System+摘要+非 System suffix，再估算，超过 available 返回 BudgetExceeded，否则含 before/after 的 checkpoint。 | 同一个源 Vec 两次结果字节一致；重新创建内容相同但 timestamp 不同的 Turn 不保证同 digest。旧 probe 可切掉 u1 而保留 parent=u1 的 assistant/tool；不能宣称完整 tool turn 保留。巨大保留 System 或最新消息即使已有摘要仍可能失败。 |
| 202–262 restore_checkpoint | start=0、0<end<=len；重算 prefix digest、固定摘要并逐值匹配，否则 InvalidCheckpoint。保留所有当前 System+重建摘要+剩余非 System；不校验 estimated 字段，没有 budget/cancel/session/branch/parent 参数。 | current changed-source test 修改prefix后拒；旧probe改两个估算字段为0并追加100000字节尾部仍可恢复。合法大尾部恢复不是“满足旧预算”；end=len 可合法，source_start 非0不可。 |
| 264–276 ContextError | InvalidBudget、Cancelled、BudgetExceeded{estimated,available}、Encoding(String)、InvalidCheckpoint；本层不写副作用。 | 对应错误优先级按入口/验证顺序；Encoding 是序列化错误映射，不伪造在普通 serde_json::Value 上易达的实际失败；内存分配失败不由此 Result 统一捕获。 |
| 280–325 version2/64KiB、CheckpointSource、UnresolvedContextCall、SemanticCheckpoint、Plan | schema version2；source 有session/branch/tip/records/全hash，另有 covered hash、retained IDs、两文件集、未解决call、摘要hash/provider/model/usage。三个持久 semantic 结构 deny_unknown_fields；Plan checkpoint 字段私有。 | version/retained/cut/covered/source/summary 篡改 current checkpoint 测试覆盖；不能从外部构造任意私有plan。outer strict 不推出 Usage 或 Turn metadata 的所有嵌套值都严格。 |
| 327–363 digest / ID / failed assistant | digest 对完整 Turn JSON 编码；ID 非全空白、<=protocol128 UTF-8字节、无 Unicode control，允许非全空白的前后空格。failed 仅 Assistant，顶层 stop_reason/stopReason/finish_reason 或 annotations.finish_reason 是 error/aborted/deferred/cancelled。 | same content 改parent/time/hash应变化；ID128/129B、Tab/NUL、全空白及合法周边空格分别构造；非 Assistant 的相同 metadata 不被此谓词过滤；annotations 的 stop_reason 不等价 finish_reason。 |
| 372–385 skeleton 前置 cut | session/branch ID 合法；0<end<len，turns[end] 是完整 User起点；prefix 不得含 opaque，且至少一个非System。短路检查保护后续下标。 | 空、单记录、只有System前缀、无safe User、原生状态在所有候选之前均拒；native 全部钉住的当前测试期望零HTTP和原history不变。 |
| 386–435 skeleton ancestry / calls | 扫描全部输入；ID合法且唯一，parent如有必须先出现，未要求该parent是User。Tool无metadata拒；metadata出现tool_calls时只能是非failed Assistant、有parent、数组；call id/name合法、arguments object，同(parent,call_id)唯一。可选path只按字符串保存。 | 当前变坏parent、缺result和duplicate-result有断言；新增同parent重复call/不同parent复用call、非数组/null tool_calls、空数组与错误role、非object args的判据。旧probe证明System parent可通过，不能改写成强User-role约束。 |
| 437–467 skeleton Tool / files | Tool需parent和字符串tool_call_id匹配先前call，拒重复result；tool_name只在可解析为字符串且不匹配时拒，缺失/非字符串不强拒。仅 outcome=unknown_outcome 标unknown；仅 prefix内 succeeded+path 计文件：read_file/list_directory/search_text→read，write_file/edit_file→modified。 | denied write不计modified，unknown prefix拒（当前测试）；补缺失/未知outcome不自动按unknown，空/非规范path仍可被收集。两个BTreeSet分别排序去重，可重叠；本层不读盘、不确认真实副作用。 |
| 469–503 skeleton call边界 / source | prefix call无result、result>=cut或unknown全部拒；tail 无result或unknown列 unresolved。按BTreeMap键(parent,call_id)生成稳定顺序。source记录整输入，covered仅prefix，tailIDs按原顺序；初始摘要字段为空/usage零。 | 结果跨cut不能压；未完成call留在tail，即使后来追加u3也不能越过该call；tail合法追加与未知对象是不同阶段。大量calls/paths/ids无显式数量上限，不以64KiB摘要限制替代。 |
| 505–531 prepare_semantic_compaction | 入口和每候选cut前查cancel；从最近完整User向前找 tail estimate>=keep_recent_tokens 且 skeleton成功的首个cut。keep_recent是下限，没有输入上限参数；candidate skeleton失败被忽略后继续，无可用cut最终InvalidCheckpoint。 | keep_recent=0仍至少留末尾完整User段；过大下限可导致无候选；大单turn无法split压缩。入口取消与候选间取消可测，扫描/估算/hash内部无逐条取消。 |
| 534–539 source_end/source | 只读返回计划切点和source引用；无修改接口。 | prepare出的end/source应绑定源；只读API不能被当作已持久化或已计费receipt。 |
| 546–585 finalize | 先cancel，重建完整skeleton且必须等于plan，拒source变化；填summary SHA/provider/model/usage，调用restore验证完整性和预算；最后再cancel。没有调用 validate_summary_completion。 | 当前测试追加后finalize拒、空summary拒；旧probe普通非JSON文本+零usage可低层finalize/session写入。补合法预算但summary>64KiB、provider/model非法、末次取消时不返回checkpoint。 |
| 589–625 restore semantic旧身份 | 先 budget max>reserve；version/session/branch/records不超过当前len、summary非空<=65536 UTF-8字节、provider/model合法、summary SHA相同；以原source.records切片重建skeleton，补摘要字段后逐字段比较。 | current篡改七字段、原history修改、跨session/branch拒；records=0经skeleton拒而非panic；错误summary hash在取前24字符之前拒。usage在低层只被按输入值复制比较，不验证正数/和式。 |
| 626–658 restore semantic新增与投影 | 全当前ancestry再校验；prefix内System保留，插入summary System（hash前24字符ID、prefix末时间、source/文件/unresolved元数据），追加tail且过滤failed Assistant。估算最终输出<=available才返回。 | 原checkpoint允许合法追加，而finalize不允许source变化。追加坏parent或重复result拒；旧unresolved元数据仍是checkpoint快照，追加结果不会重写它。过滤failed assistant若其他普通记录parent指向它，输出不另行检查父节点是否还在；不能声称投影总是完整parent图。 |
| 660–676 SemanticSummary / instructions | 必需九数组：六语义组、read_files、modified_files、unresolved_tools；unknown字段拒。指令要求只总结、保持要求和未完成项，不把unknown当完成。 | 缺组、额外键、markdown fences、错误元素类型应拒；合法JSON/非空section不证明事实正确。System角色摘要是provider输入设计，并非不可信文字天然失去指令效果。 |
| 682–725 summary_request_data | 当前skeleton必须等于plan；previous存在时用MAX/0预算restore，要求previous.end<new.end；发送previous_summary字符串+新增覆盖records（id/parent/role/content/metadata，省timestamp，过滤failed assistant），附当前plan文件/未解决对象。 | 首轮null；次轮不得重发已覆盖前缀（当前HTTP测试源码断言）；stale/跨branch/不推进拒。MAX只是完整性验证，不是实际请求额度。先构造完整records Vec/JSON字符串，无流式或单独字节/行数上限。 |
| 733–767 validate completion外层 | usage必须Some，input/output>0、output<=limit；input+output checked_add不能溢出，total>=sum（可大于）。拒任何Some refusal、toolcalls、空/超64KiB content；annotations truncated=true，或四种status键含length/max_tokens/error/aborted/cancelled/incomplete时拒。 | current invalid-summary源码覆盖主要11种场景；补u64加法溢出、总量不足、空refusal、顶层与非字符串annotation值。deferred不在该黑名单，旧probe确认可过；与failed-context规则差异保留未修。 |
| 768–807 strict JSON /规范返回 | 六语义组不可全空；八字符串组每组<=64条，每项trim非空、<=4096 UTF-8字节且无NUL。read/modified/unresolved必须与plan精确逐项、顺序相同。serde规范编码后返text和usage。 | 64/65条、4096/4097B、仅空白、NUL、改文件顺序/遗漏call分别判定；非NUL控制字符并非统一拒；unresolved_tools未套八字符串组64条限制，但有整体content字节限制和精确对象比较。规范化编码可能改变字节长度；finalize/restore再次检查64KiB。 |

## token、字节、字符、行与临时内存不能混用

估算单位是 ceil((UTF-8 content + JSON metadata + 每记录16)/4)，不是 tokenizer，也不是将完整 provider wire JSON 除4。Turn ID、parent、created_at不单独计量；metadata中的附件path/url/file_id/size/hash作为文本计入，附件实体未进入这个函数。工具 arguments/result如果存在metadata/content会被估算，但工具定义、wire结构、图片实际token或远程file内容不是该函数的参数。读取全文的807行只是研究覆盖单位，源文件或消息没有“每行固定token”保证。

当前 core 的 context_budget 将配置与模型元数据/未知模型后备窗口取界；普通provider路径先减资源/extension instructions 的字节估算，生成投影后替换 template/rendered 与 extension input，再重估，最终附工具 definitions 和 active attachments 发请求。它有额外输入估算约束，但这些有界片段没有把所有附件/工具schema开销精确计量的实现，不能宣称与模型实际输入计数严格相等。

附件 path materialization 在 core 中按能力、InputAttachment.validate、workspace read_attachment、单件10MiB和本次累计20MiB处理；backend还定义每turn最多8件。journal metadata只存引用、size、digest，RequestAttachment.data另外持有Vec。backend attachments_for_context仅让仍在投影里的turn ID拿到对应active attachment。context summary_request_data传的是历史metadata引用，不装载或再次附上被覆盖图片实体；摘要API默认attachments为空。不能据“metadata被计入”声称旧图片的视觉事实已被摘要模型重新读取或保留。这里不接受附件owner整个生命周期/跨重启能力。

summary原始content上限65536字节，单字符串4096字节，组内64条；不是65536字符，也不是65536token。Unicode原文与JSON转义有不同字节长度；core日志截断在UTF-8边界回退，展示原bytes/hash并标truncated。Completion规范化返回仍可能改变序列化长度，之后的restore重查尺寸。这些是接收/结构限制，不能限制生成该对象前的所有内存分配。

legacy suffix每步clone/reverse/重估，最坏可反复处理增长后缀；semantic多个候选会重复估算整个tail、扫描ancestry、构造BTree容器和编码digest。restore在旧source和全current上分别重建；summary_request_data会构造records JSON Vec、组合Value再String；estimate metadata用to_vec分配；core response annotation先to_vec后才决定保留或摘要，raw response本身已存在。无统一history记录数/输入byte/临时内存上限，无逐记录取消，不能由saturating arithmetic或64KiB输出检查推出固定内存/延迟。saturating_add避免计数回绕，却不是provider保守token上界的数学证明；Encoding Result也不等于捕获OOM。

## 有界消费者顺序、压缩取消与提交

当前 core1739–2036 先外层cancel，再验证budget、取当前semantic投影估算；fit不发summary。超限取selected_history、真实session/branch、previous checkpoint，keep尾部取available的一半，通过InputPump进行计划准备并finish；生成request-data，选模型描述，output_limit=min(request reserve,4096)。summary请求另估算为request Turn+instructions字节/4+64，再检查加output是否超request max。计划下限、投影预算、summary请求预算是三个约束，不能共用一个“token已测准”结论。

价格存在时按窗口/输出限额预留成本，代码注释明确是上界预留而非真实账单；candidate ledger先验证input/output/network/cost整组额度再持久化。取消可发生在reservation持久化之后、operation开始之前；纯context取消不修改源，但不能扩写成宿主取消零journal变化或零配额变化。operation.begin与summary_requested事件属于宿主，发生在HTTP之前；请求的tools为空、SUMMARY_INSTRUCTIONS单独传、purpose=semantic_compaction。

流中TextDelta累计字节超64KiB报错；返回后先pump.finish/response错误，再记raw摘要事件及usage成本估价、补充超过预留的报告量；之后才再cancel、校验actual reported input+output_limit、**validate_summary_completion → finalize(重新selected_history) → append_semantic_checkpoint_for_operation → restore**。因此“旧checkpoint未替换”不意味着失败时没有response日志或资源记账，也不证明没发请求。实际usage仍是reported tokens按记录价格估价。

session1225–1387 的 append 有operation参数时要求已有未终结Compaction marker，先cancel，再选择tree ancestry，要求checkpoint.source.records恰等于当前selected长度及当前branch，再restore；完全相同event返回false，否则再次cancel后append。低层append不调用Completion strict validator，plain summary/零usage兼容边界仍存在。restore允许在checkpoint后追加合法turn，但提交时仍要求精确当前source；这两条不能混成“任何追加都可提交旧plan”。读取latest会先验证候选自己的tip ancestry及depth，再跳过其他branch/不可见tip，最后按当前turns/budget恢复。

checkpoint与operation correlation同一个durable event，finish_operation是之后的独立步骤。append成功后restore或finish仍可能返回错误；不能把任意compact Err都概括为checkpoint一定未写入。core状态映射只把Backend Cancelled标Cancelled，其他包装为InvalidTurn/Session错误的取消不自动得到相同枚举；保持该层差异，不虚构取消传播统一性。070已冻结的post-dispatch分类和finish?跳过release两项缺陷证据完整保留，本073不修改它们，也不进行fault injection。

当前 core2044–2101 的手动compact要求Idle、非Closed且无recovery；设置manual-compaction input scope和preparation guard，保存prepare结果，drop guard、清scope，再传播错误。新外部测试对success/cancel都检查durable queue Received、port不再preparing/idle拒票及独立进程恢复，区别于普通active turn late-steer被Applied的测试。context本身没有票据owner或TUI响应循环；不把UnixStream测试描述成PTY。主控告知F2/TUI hash `6459360343a260c6dfe7b403a81cd22a8fcac0bb3e10fcabed2bd3c0b8567254`及143项检查，仅作为主控本轮外部进展记录，worker未执行且未阅读/编辑TUI。随后B122 history guard整合若改变TUI应另记delta，不回填旧125/070包。

core4894–4910在有semantic checkpoint时恢复provider输入，否则返回完整selected history；没有把legacy count/digest当成保留语义的替代。无safe cut、单个完整turn太大、opaque钉住、summary source太大、provider/usage无效时可以明确失败，不应把失败说成已有语义fallback。

## 当前测试全文的实际断言与限度（全部未运行）

| 文件 / 测试或helper | 本轮读到的断言，不冒称新通过 |
| --- | --- |
| context / token_estimate_is_explicitly_approximate | approximate=true、input>0；无供应商tokenizer精度对比。 |
| context / compaction_is_deterministic_and_keeps_newest_turns | 同输入两次checkpoint/turns相同，checkpoint存在、末ID保留、estimate<=1200。 |
| context / cancellation_during_needed_compaction_preserves_source | need-compaction+true得到Cancelled，源Vec与clone相同。 |
| context / restoring_a_checkpoint_rejects_changed_source_records | prefix第一记录内容变动后InvalidCheckpoint。 |
| checkpoint / whole_turn_cut_keeps_multi_call_pairs_files_and_source_identity | schema2、cut6、tail u2、文件集精确、unresolved空；输出System/System/User，首s、摘要Unicode、末u2。 |
| checkpoint / changed_source_cut_tail_parent_summary_and_branch_are_rejected | 改cut/tail/covered/summary/tip/branch/version七情况拒；改source、missing parent、删除result、other session也拒。 |
| checkpoint / unresolved_tool_results_stay_in_tail_and_failed_assistants_are_not_projected | pending列unresolved且u2，输出pending但无aborted；追加u3仍cut6。 |
| checkpoint / failed_writes_are_not_reported_as_modified_and_unknown_prefix_is_not_compacted | denied不计modified，unknown_outcome令prepare失败。 |
| checkpoint / cancellation_empty_summary_and_budget_failure_leave_existing_checkpoint_unchanged | 初append真，cancel错误不改journal bytes，重复append假，prepare取消、空summary、1token输入budget均失败，latest与原字节不变。 |
| checkpoint / stale_preparation_and_duplicate_result_rejected | prepare后加a2 finalize失败；同call第二result即使新record ID也失败。 |
| checkpoint / durable_checkpoint_survives_a_new_process_and_retains_appended_turns + checkpoint_recovery_child | 父存checkpoint后加a2，启动ignored child并检查成功及1 passed；child查Unicode、末a2。helper单独声明但不增加父测试运行分母。 |
| checkpoint / legacy_digest_checkpoint_remains_readable_without_becoming_semantic | legacy JSON不能解SemanticCheckpoint，legacy restore仍可。 |
| semantic / real_http_summary_preserves_constraints_files_usage_and_iterative_restart | fixture检查purpose/输出1000/无tools/无response_format/instructions；首previous=null、次previous含约束/new_records无old-0；后续wire含BentoBox/待办/中文path且去长历史；两checkpoint递进、restart一致，cost reservation12000/首usage估价470、HTTP3。固定模型回复不是开放模型质量实验。 |
| semantic / actual_http_invalid_summary_never_replaces_previous_checkpoint | 11分支empty/json/missing_usage/missing_input/length/over_output/file_loss/refusal/tool_call/over_input/over_cost；建有效checkpoint后第二失败、重开仍旧checkpoint、各HTTP2。11分支不当11个顶层。 |
| semantic / quota_cost_unknown_price_and_oversized_source_fail_before_http | cost/unknown-price/network/input/output/source六分支拒、无checkpoint、HTTP0；这证明测试预期，不是context自己实现价格owner。 |
| semantic / http_cancellation_retains_valid_checkpoint_and_does_not_retry | 第二summary触发取消后错误、旧cp不变、HTTP2。 |
| semantic / native_state_is_pinned_counted_and_branch_bound | 原生Turn完整保留，其他branch拒；20k签名metadata令estimate>5000。不是native所有wire字段覆盖。 |
| semantic / semantic_process_helper + separate_process_commit_and_reopen_use_semantic_context_on_real_wire | helper compact/interrupted/resume三模式；父先运行compact，再人为截journal至checkpoint模拟commit边界重开，两个子进程成功、HTTP2。该case不是fsync途中kill。 |
| semantic / unknown_tool_result_stays_in_retained_tail_and_summary_does_not_claim_success | request未解决call ID精确，cp unresolved1/modified不含unconfirmed，restore含完整call及unknown result。固定good_summary不能证明任意模型永不虚报。 |
| semantic / actual_http_prepare_services_late_steer_and_consumes_it_once | summary等待时is_preparing；late enqueue ticket Ok；延续wire文本一次，HTTP2，queue恢复Applied/history一次，公开events无critical_facts。 |
| semantic / async_headless_manual_compact_remains_responsive_and_cancellable_during_http | UnixStream驱动真实async host；等待HTTP时status response先到（不另断言status.success），cancel使compact.success=false；join后无摘要泄露、HTTP1、重开cp无。是有界fixture而非TUI或PTY。 |
| semantic / summary_http_failure_does_not_use_configured_automatic_retries | 503仅n0，配置3retry但HTTP1、无cp、ledger网络数1。 |
| semantic / opaque_state_before_every_cut_fails_without_losing_history_or_starting_http | 首条native钉住所有cut，失败/history不变/无cp/HTTP空。 |
| semantic / killed_summary_process_preserves_previous_commit_and_requires_explicit_recovery | 先有效cp，第二请求started后kill+wait非成功；重开旧cp、recovery非空、普通process拒、HTTP2。未断言自动恢复。 |
| semantic / manual_compaction_accepts_durable_queue_input_before_http_cancellation + manual_compaction_queue_case | Unix父对cancel=true/false都执行辅助case：阻塞summary，先收queue响应，再cancel或放行，compact success==!cancel、HTTP1、重开cp presence对应结果；queue response成功，恢复input Received，port不preparing、idle list拒；可按环境变量导出证据。两case不加顶层数量。 |
| semantic / manual_summary_queue_recovery_probe | ignored独立进程读同journal，input Received/text精确，applied event数0。父检查子退出成功并打印结果；这是新增声明，旧12项历史semantic日志没有它的这次通过。 |

当前源码既没有等同完整模型计量的token测试，也没有在本轮运行取消延迟/内存上界/所有Unicode极限/日志写失败后的checkpoint一致性。上表和函数判据使缺口可运行、可审查，不补造结果。旧五probe全文保留，它们证明旧闭包对fit-cancel、legacy孤立parent/无预算restore、低层plain/zero usage、System call parent、deferred接受的实际观察；当前context同hash提供本文件对应关系，当前消费者顺序另由新阅读支持。

## 与 pi 源实现的精确比较

本轮有界重读上游compaction.ts：147–419（连续三段）、555–611、634–707、753–824；utils.ts1–65；另全文session/context.ts1–64。具体identity/ranges在ledger，源comparison不扩大成012整文件再次understand或目录接受。compaction.ts identity `6e7aec0d27cb566f8f85f7dd13850eda98f5ddf7e78f9a919fb3dd03c3ea3a8a` 与冻结登记一致。

pi从最后可用assistant usage锚点加后续heuristic；usage getter排除aborted/error但不是所有deferred，calculateContextTokens用totalTokens或input+output+cacheRead+cacheWrite。无锚点逐消息估算；JavaScript `.length` 是UTF-16 code units，不是Rust UTF-8 bytes、Unicode scalar或屏幕字符。图片固定估4800 units，thinking/toolCall参数/custom/toolResult/bash/summary分别计量。zenpi统一content/metadata字节估算，不以历史usage校准；两者heuristic都不保证真实token精度。

pi默认 enabled、reserve16384、keepRecent20000，shouldCompact受enabled和window-reserve影响；zenpi本文件默认128000/8192，是否请求由core处理。pi切点容许assistant和其他用户可见entry，排除toolResult，可回溯turn start并分别摘要history与turn prefix；zenpi只完整User边界、强call/result/unknown检查，不能split一个大turn。因此有共同“保留近期上下文”的目标，不具备相同可压缩输入集合。

pi上一compaction.retainedTail生成虚拟entries，再追加新path entries，传previousSummary；zenpi始终以完整原ancestry hash绑定source，summary_request_data仅发送新增覆盖片段，tail从journal投影，持久schema不可交换。pi utils从Assistant toolCall读read/write/edit路径，函数不等成功result；written+edited去重合并modified后从read排除。zenpi只有匹配prefix succeeded结果才计文件，read和modified可重叠，不能因为表名类似就说语义一致。

pi生成通过注入request，输出约0.8reserve受model.maxTokens约束，可customInstructions/thinking，aborted/error映射失败，文本+usage返回并补文件tags；zenpi context不请求模型，生产core用严格九段JSON和source/usage/列表校验，二者自然语言事实保真仍需单独检验。上游session/context取latest compaction及后续entries、过滤error/aborted/deferred assistant、拼summary+retainedTail，并按需await custom projector；zenpi不同checkpoint版本和session branch/hash约束不能从这个投影函数推导为相同owner语义。

## 交付、回退与证据限制

ready下 learn-report.md 是本项完整新候选，旧三份报告及完整包独立保留；本报告不改历史段落的时态或实测结论。receiver-base.md 是当前主库原3036字节基线报告，SHA256 `08f9dc2204499b1431e049bb6e42c5677a24eb4934f4c3672cf3f17063b5219b`。forward.patch 只从这个精确基底更新唯一owned报告；reverse.patch 恢复同一原文。两补丁未应用；其他接收状态须主控核对，不允许强行覆盖。旧ready、主库产品和canonical、blueprint、claims、masterreceipt均未写。

reading-ledger标明本轮实际完整source、三个完整测试、有界消费者和上游阅读，不以索引/hash代替语义阅读。manifest记录全部payload，唯一新纯Python verifier完整静态阅读后最多执行一次，stdout/stderr/result在包外原样保存，失败也不重跑；结构PASS不等于G-FILE/G-STAGE或完整当前行为验收。070两缺陷及全部冻结payload、072完整包、117 fixture和既有worker非.ops文件的身份另受保护。提交精确manifest/report/patch/rollback/阅读身份给主控后停止。


## 主控独立逐文件验收 — ZS1-073 / 3.1.21

主控本轮顺序完整读取当前src/context.rs L1–280、281–560、561–807至EOF（3741ab/5bad3a/9b1233），30013 B / SHA256 256d9ac1657e272ba61ebfbc242ece2970ed0607f3ebb1583b1711f18b7bf229；另外完整读冻结baseline246行8199 B / bc4ce7999a0f4d162effece8c6f839aca3a230e97675a1d7908fca5d6a7fe909（682cce）。完整133行34074 B候选分1–45、46–95、96–EOF读完（ff8bfb/367837/a80c3a）；README和新auditor全文已读。本项不把hash、chunk账或旧报告当作此次全文阅读。

纯context负责估算、legacy摘要、semantic计划、完整性验证和provider上下文投影，没有磁盘/网络/票据/UI所有权。默认128000/8192；当前估算按UTF-8 content字节、每turn16和metadata JSON字节，饱和相加后ceil/4，恒approximate。冻结版本只计content，当前metadata/opaque保护与v2是单独delta。此估算不含所有wire/tool定义/图片实体；reported usage仍是adapter提供量，不能和字符数、UTF-16单位、账单真实量混同。64KiB摘要和4096字节单项不限制此前JSON/Vec全部分配或延迟。

完整legacy分支已核：预算先验证；fit时不查cancel；超预算仅先查一次。所有System保留，反向选非System后缀，首条可超目标，按首ID的位置确定cut。固定记录数/role/SHA摘要并不保存自然语言语义，也不保护完整tool/parent图。restore校验范围、hash和固定文本，不校验两个估算字段且无恢复预算。旧API兼容与生产semantic路径分别表述。

semantic skeleton要求合法唯一ID、parent先出现、cut在完整User起点、prefix有非System且无opaque。cancel_reissue元数据可使带parent User成为切点，低层不认证该标志。Assistant tool_calls必须有效数组/参数object，同parent+call ID唯一；父记录角色不强制为User。Tool匹配先前call、拒重复result；tool_name缺失/非字符串不强拒；unknown_outcome有专门保护，其他未知outcome不自动等价unknown。只对prefix内succeeded且有path的指定工具记录read/modified，两个集合可重叠，不等于文件确实存在/操作恰好一次。

prepare从最近完整User向前找满足tail token下限且skeleton有效的切点；keep_recent是下限，并无拆分单个大turn的fallback。finalize重新构造整个skeleton要求与plan完全相同，再填摘要并restore预算验证，前后查cancel；不调用strict Completion校验。restore允许原记录后合法追加，先验证旧source再验证全ancestry，保留prefix System、插入summary System和非failed tail。摘要内unresolved元数据仍是checkpoint时快照，新增tool结果不回写它；过滤failed assistant后也没有重新保证投影parent完整。

summary_request_data必须与plan同源，previous要合法且cut前进，发送旧summary和新增covered记录及精确文件/未解决列表。failed过滤包含deferred；strict Completion黑名单当前没有deferred，两者不对称，尚待本轮产品反例，不在G-FILE中伪称已修。strict层拒空/超64KiB/拒绝/工具调用/缺usage/零input-output/输出超限/和式溢出或不足及已知未完成状态；九数组schema严格、六语义组不可全空、八字符串组各至多64条且单项4096字节/非空/无NUL；文件和unresolved要精确相等。合法结构和SHA都不证明自然语言保真或抗注入。

主控完整读当前tests/context.rs77行、tests/stage1_context_checkpoint.rs373行以及tests/stage1_semantic_compaction.rs1047行共1497行（1e9112/1c73ae/682cce/738728/285957），含所有helper、Unix条件与结束段。28个声明=25普通+3ignored helper；这次没有执行它们，不能把旧29行为结果或新29结构检查套到当前测试。真实HTTP、source/branch/tamper、两进程恢复、取消、队列、unknown工具和native保留各自断言的范围已核，UnixStream不是PTY，人工截journal至commit也不是fsync中断试验。

主控新读core完整prepare_semantic_provider_context及manual compact L1739–2108、session append/selected/latest L1225–1387，确认生产顺序：估算及计划、summary-request budget、候选整组额度预留、operation/request事件、HTTP、pump.finish、response记录与reported用量记账、cancel与strict校验、finalize、append checkpoint、restore、finish operation。摘要失败可以已有request/response/用量/operation记录，不能声称任意Err零journal。append成功后restore或finish还可Err。manual scope/preparation guard在传播prepare错误前清除。session低层append允许非JSON摘要/零usage边界，生产core另行strict保护；分支/record精确提交条件不同于恢复时允许合法追加。这里只是有界消费者审阅，不接受070/074/084整文件或所有并发崩溃合同。

新读backend非流式Chat响应的有界转换L1350–1420，看到finish_reason进入Completion annotations；完整validate_json以及其他provider是否拒绝deferred尚未独立证明，不把一个赋值语句推断成完整HTTP可达反例。上游pi比较沿用worker本轮有界阅读及已有012/013来源报告，主控本轮未重读这些源全文，也没有宣告两套压缩输入集合/文件去重/token heuristic完全等价。附件装载/工具schema/模型能力的完整实现各自待验。

完整auditor静态阅读后，新的主控副本只适配ROOT为显式worker目录，并在运行前核固定manifest；唯一一次运行29结构检查全部通过、0产品运行。它还读当前主库上下文与历史原包，证明交付与当时输入绑定，不是对所有123 receipt artifact重新语义验收。原worker和旧失败/脚本保持不变，未复跑旧runner。本项仅接受073完整文件理解G-FILE；103/104压缩产品、core/session/backend和目录/全阶段仍独立待验。094/128之前的主控进展只作为背景，保持原捕获时态。回退恢复receiver-base.md精确3036字节原报告并撤销本项状态/receipt，不改产品、BentoBox、项目或会话。
