# ZS1-050 — compaction 目录 G-DIR 独立理解复核，3.1.21

本轮只提交 `Docs/learn/stage1_pi_mono/packages/agent/src/harness/compaction/current_folder_learn.md` 候选。直属冻结子项 ZS1-012 已获当前 master 接受，满足本目录开始独立复核的前置条件；050 仍待 master 审阅和 G-STAGE --item ZS1-050，不在此修改勾选、receipt、claims 或 authority。054 及所有祖先均不接受。

本轮进行了源码阅读、历史证据复核、字节/hash 和 fixture patch 校验；新 runtime 测试、npm 安装、模型请求、产品/TUI/budget 测试均为零。历史五项结果及 012 的 37 项 master 结果均明确复用，不计作本轮执行。

## 当前身份与直属闭包

Authority 3.1.21，run `zenpi-stage1-20260911`，requirement `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`，baseline collection `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`。物理源目录 `/Users/wangweiyang/GitHub/pi-mono/packages/agent/src/harness/compaction` 恰有三个直属文件，无子目录；原包记录 upstream revision `bbb61e34aaf231639fdaaad1adbd757947034eac`，本轮以实际字节匹配为准。

| 直属文件 | 字节 | SHA-256 | 冻结范围与阅读依据 |
| --- | ---: | --- | --- |
| branch-summarization.ts | 9537 | 38fed20de82bad088ab0a49c7c46e8c801c0bf448dc8a86d7c0f8f225d43792e | context-only；本轮全读 300 行，未逐文件接受 |
| compaction.ts | 27410 | 6e7aec0d27cb566f8f85f7dd13850eda98f5ddf7e78f9a919fb3dd03c3ea3a8a | 唯一 in-scope，SRC-0121 / ZS1-012 [x]；精确复用完整阅读链 |
| utils.ts | 4312 | 85ea768b590e683727aecaf6ac72cf4218806e66313bc3ed2e1593867592d6c0 | context-only；本轮全读 132 行，未逐文件接受 |

叶子闭包是 `050 → 012`，没有未列直接子目录。物理库存中的两个 context-only 文件有明确集成解释，但不产生两项额外验收。向上的 `054 → 050` 只描述结构关系，不反向接受 054；同级 session 也不由此重复接受。source_manifest、file_learn_index、folder_learn_index、blueprint 和 selector 的快照均在新包 authority 中。

封包前主控接受 133 后刷新 checkbox/snapshot；selector snapshot 从 `7d39ece35ec197cda6e6589057df93631c2aa60f27a82a39aa41a30b78346c73` 变为 `d171b642554d2a40f3f30b814bc1e22d813356634cb560ed9f412e036262a618`，其余 selector 字段不变。原始 authority 与 authority-final 分别保留；逐项比较确认 012、050、054 记录不变，未把 133 的新接受外推为目录接受。

## 012 完整理解证据的精确复用

当前 `ZS1-012.master.json` 为 14202 字节，SHA `0f8fc23e6c09a9db20c67e4d0a819a8df33d4d18dcc9c98d53c1075194d6f782`；complete=true、manual decision=accepted，source_hash 与上表一致，read_ranges 为 `[0,27410)`。当前 canonical 012 报告 21934 字节，SHA `48e5480481dc5164e98250c7ac9af78ed16f02723e1c4dcd80fec165a57f2a8b`。本轮完整阅读该报告、master 3.1.19 review 和 3.1.21 rebind review，并逐一校验 receipt 所列 69 个 artifact 的复制身份。

master 原完整源阅读为 865 行，连续 1–290、291–596、597–865，覆盖整个 compaction.ts；3.1.20/21 rebind 沿用相同条目义务和完整证据链。3.1.21 review SHA `74d271b772da0124cf1f77671ecc6cb64cf17f1244c055407c6687973a6525ef`，明确只保持 012 接受。本轮不把这条链写成自己新全读 865 行，也不把 69 个文件 hash 校验写成 69 个语义全读。

012 原 master 实际执行 exit 0、37 passed、0 failed/pending/todo，三份测试文件为 22 个 compaction 用例、2 个 sibling 用例、13 个 supplemental 用例；Vitest 的五个 suite 计数含 describe group。250-budget sweep 是一个用例。真实冻结算法、context、序列化、UUID、usage、retry 被执行，响应由 scripted callback / faux Models 提供，branch reader 为内存 map；这些历史结果不证明 provider 网络、模型语义保真、真实计费、磁盘事务或进程恢复。本轮只复核报告与结果身份及计数，不声称重新逐行审阅其所有测试和 13 份观察。

## 目录内调用、数据和所有权

`compaction.ts` 从 utils 取得文件操作收集、排序与标签格式化以及会话序列化；`branch-summarization.ts` 也复用这些工具，并从 compaction 导入 estimateTokens、共享系统提示和 request/retry 包装。方向为 branch summary → compaction 公共 helper → utils，compaction 不反向依赖 branch generator。二者经 messages.convertToLlm 将业务消息转换成模型消息，再 serializeConversation；harness/context 把调用者的 abort signal 和 telemetry 传入请求。目录内没有 journal writer、branch tip owner 或进程恢复循环。

012 的准备流程从调用者提供的路径中取最后一次 compaction，将其 retainedTail 投影成虚拟 entries，与后续 entries 合并，再分出 history、split-turn prefix 和 retained tail。previousSummary 单独供历史更新提示使用；旧 compaction 的 typed readFiles/modifiedFiles 会带入新的文件事实。它允许切开 user turn，未对任意 orphan toolResult、未完成 tool call、祖先关系或跨 session 来源进行全面校验。settings.enabled 由 shouldCompact 检查，而 prepareCompaction 本身没有这道 guard。token 估计混用可用 assistant usage 与后续近似值，通常按字符/4、图片 1200 tokens 估计，不能当作 provider 精确计量。

准备结果共享嵌套 message/settings 引用，retainedTail 的引用也可继续进入返回结果。创建新的数组和 fixture 输入 JSON 不变，只说明具体操作的局部行为，不构成 deep immutable snapshot。调用方负责选定祖先、隔离并发修改和持久化准备信息。

utils 的 FileOperations 是 read/written/edited 三个可变 Set；只从 assistant 的 read/write/edit toolCall 中读取非空 path。它记录操作意图，失败或未完成调用也能贡献路径；modified 并集覆盖 read-only 后排序，标签拼接没有额外转义。序列化保留可见 text/thinking 和调用名、参数，safe JSON 可处理参数值中的循环；toolResult 文本限 2000 字符，丢失调用/结果 ID、isError 等信息，非文本图片内容不保留。因此不能从摘要中出现路径推断文件确实写成功，也不能从提示词重建原始审计记录。

branch 模块的 collectEntriesForBranchSummary 用注入的 branch/session reader 找旧路径与目标路径首个公共祖先，沿旧 parentId 收集离开的一侧并 reverse 成时序；无旧 tip 返回空，缺失 entry 抛错，reader 的顺序/祖先有效性由调用方负责，没有显式 cycle guard。该 helper 的行为在 012 历史 sibling/supplemental 支持证据中复用，不由旧 D050-01 覆盖。

prepareBranchEntries 对候选 entries 提取 message，忽略 toolResult，把已有 branch/compaction summary 转成摘要消息；旧 branch summary 的 typed file lists 可进入 branch 自己的 metadata。它从近到远做 token 限制，但文件事实的收集可能早于被预算排除的文本；在占用低于预算 90% 时也可能保留一个超预算旧摘要。generateBranchSummary 默认 window 为 128000、reserve 为 16384；WithRequest 使用 2048 maxTokens，接收可替换/附加的指令，返回加前导说明和文件标签的 summary、usage、file lists。空 messages 返回 No content 与空文件列表，可能丢弃准备阶段已有的 file facts。

跨文件最容易误解的是 branch file tags 与 compaction typed details 的差别：branch summary 的文本会经 convertToLlm 进入后续 compaction 提示词，其中能看见 shared.ts；compaction 不因此解析 branch_entry.details 并合并到自己的 readFiles/modifiedFiles。旧 compaction 的 typed details 则有明确继承路径。两类信息不能互换，也不能保证模型复述了标签或全部待办事项。

## 调用者、错误、取消、持久化与恢复连接

本轮对实际 runtime 做限定范围阅读，作为 owner 边界上下文。lane.acceptCompaction 在 command 中拒绝 busy lane，scanBranch 从当前 tip 到最近 compaction，reverse 后调用 prepareCompaction，空准备返回 NothingToCompact，再记录 operation 和 durable preparation。acceptNavigation 从 observed tip 与 target 的 newest-first 路径取公共祖先，实际直接调用 prepareBranchEntries；之后在 command 中重新核对 tip、目标与忙状态，存储准备结果。它没有直接使用上述 collectEntriesForBranchSummary helper，不能把两条入口混写成同一次调用。

runtime/drive/checkpoint 的 runCheckpoint 调用 prepareCompactionThreshold；后者检查 enabled/model、触发 entry、是否已有更新 checkpoint、shouldCompact。overflow 准备有自身的一次恢复标记。structural.performStructuralAttempt 注入 SummaryRequest：before_request/before_payload hook、request intent、usage identity、gate.admit、Models 请求与 request outcome 记录均在 runtime。目录内 compactWithRequest 按 history 后 prefix 串行调用，首个失败不启动后一个，第二个失败不返回 CompactResult；error/aborted 响应成为 Result.err，而普通 promise rejection 向外抛出。branch wrapper 同样传播 request rejection。

目录只转发 Context 中的 signal/telemetry，并强制 no-cache；它不独立执行请求前后 aborted 检查。历史 012 已证明忽略 signal 的 callback 仍可成功，不能从 D050 的 cooperative aborted reply 推导出“任意取消都不能发布”。runtime 的 StructuralCancelled/gate 与 durable control 负责更外层中断管理，aborted provider response 若与仍 running 的 durable control 冲突会抛 invariant 错误；普通错误是否 retry 取决于最后 response、policy 和 attempt。

structural.publishAttemptResult 将成功结果交给 publishStructuralOutcome；后者构造 compaction entry（parent=current tip、summary、retainedTail、details、usage）或 branch_summary entry（parent=navigation target、fromId=原 tip、typed file lists），通过 lane.continueOperation 提交 writes 与 branchTip 更新，再按 boundary 决定 resume checkpoint 或 terminal 处理。recoverStructuralGeneration 将未知外部结果的 orphaned effect 记为 structural_interrupted，按 policy 进入新编号 attempt 或失败。这里建立的是源码中的 owner 连接；本轮没有执行 lane store/commit/gate 恢复流程，没有验证底层事务原子性或重启后的 exact-once 效果。

session/context.buildContextEntries 选择最后 compaction 及其后的 entries；projectStandardEntry 生成 compactionSummary 加 retainedTail，过滤 error/aborted/deferred assistant，branch summary 非空才投影；自定义 entry 可交给异步 custom projector，错误会传播。messages.convertToLlm 把 branch/compaction summary 包装成 user 消息；excluded bash 被略过，custom 转成 user，基础 user/assistant/toolResult 保持。buildSessionContext 只是内存投影，不写盘、不替调用者验证整条祖先，也不自行恢复中断操作。只有上层持久化并重新提供有效 entries 后，这个 projector 才能重建模型上下文。

## 旧五项真实证据与失败历史

原 A ready 共有 55 个 manifest payload，另有 manifest 本身；manifest SHA `5c47c0e60cad693d1d45239430efd75dc933427c4efb511916a87693920d788d`。本轮完整阅读旧目录报告、prior report、directory.test.ts、observations、原结果/stdout、fixture 失败与修正，检查 config、runner、export bridge 和锁定配置；只读脚本，未执行。包内 27 份源拷贝逐一与当前 upstream hash 比较。旧命令记录 Node 24.19.0、Vitest 4.1.9，Node SHA `27db838bb204ef7c21df2931f5656e4c8fb32e6e947f363a402b49714d32b5b1`。本轮不重建其 node_modules 或声称当前 runtime 重新执行。

| 旧用例 | 实际观察 | 不能推出的结论 |
| --- | --- | --- |
| D050-01 | prepareBranchEntries 记录失败 write 的意图；提示词有 write/shared.ts、无 FAILED_WRITE_MARKER；branch 文本及标签进入 compaction prompt，但 result.details 为空；投影为 summary+tail | 名称中的 collector 未执行；不能证明写盘成功或 typed details 自动继承 |
| D050-02 | 旧 compaction 的 OLD_PENDING 和 typed read/modified files 进入更新；JSON roundtrip 后只投影最新 summary、retained assistant 和后续 user，过滤 stale ancestor/error/aborted/deferred | JSON fixture 含简化/重复 IDs，不是有效真实 journal；没有 OS restart 或磁盘恢复 |
| D050-03 error | split turn 的 history 成功，prefix 回 error，返回错误且输入 JSON/投影不变 | 不存在实际 checkpoint commit，无法证明回滚磁盘事务 |
| D050-04 aborted | 正确 AbortController context 被转发；history 后 prefix 执行 abort 并回 aborted，返回错误、投影不变 | 依赖 cooperative callback，不覆盖忽略 signal 的发布竞态 |
| D050-05 | compact 与 branch 两 wrapper 均把同一 REQUEST_THROWN rejection 向外传播，输入不变 | 没有验证 provider transport 或未知请求结果的进程恢复 |

初次真实结果是 3 passed / 2 failed，两项失败均 `TypeError: this[#parent].value is not a function`；调用链来自 chord ContextValue.value → getTelemetryContext → createSummaryRequestOptions → history request。fixture 将 `withAbortSignal(signal, context)` 写成 `(bg, controller.signal)`。纯文本 patch 校验证明唯一改动是交换这两个参数；冻结源没有改动。修正后原执行 exit 0，五项 passed，零 failed/pending/todo。初次 test/results/observations/stdout/stderr 全部原样保留，不把初次失败解释成产品实现缺陷，也不抹去失败历史。

## 当前 Zenpi 映射与有意差异

本轮读取当前 master 文件的相关实现并冻结上下文：context.rs 为 30013 字节，SHA `256d9ac1657e272ba61ebfbc242ece2970ed0607f3ebb1583b1711f18b7bf229`，与 012 历史 target context 相同；session.rs 已为 176828 字节，SHA `8fb3ff2c387b13d34d8bbe4533bb2b0a81b5e546f5e03216b74b5cf2c61c9a2b`；core.rs 为 278737 字节，SHA `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869`。后两者整文件已变化，因此没有沿用旧整文件身份作为“当前”依据；这里只复核相关片段，未接受这些目标文件整体。

context.rs:365–815 的 checkpoint_skeleton/prepare/finalize/restore/validate 实际检查 ID/parent、按 turn 关联的 tool call/result、来源 session/branch/digest、完整 turn 切点。覆盖区间内 unresolved/unknown outcome 被拒绝，保留区间的 unresolved 明确记录；read/modified files 来自 succeeded 的 tool outcome，区别于 upstream 的调用意图。prepare 至少留最新完整 user turn，finalize 重新核对 skeleton 与取消，restore 验证 source prefix 与 summary hash，再保留后续追加记录并检查预算。

summary_request_data 将已校验的 previous summary 与新覆盖 records 合并，文件列表/unresolved 是独立结构化事实。validate_summary_completion 拒绝 refusal、tool calls、空/超长内容、缺失/不合理 usage、已标识的 length/error/aborted/cancelled/incomplete 状态，并要求 JSON 数组、长度和精确 file/unresolved metadata。它不能证明自然语言 goals/pending_tasks 的每个语义都保真；不应把结构验证写成模型语义证明。

session.rs:1237–1385 的 append_semantic_checkpoint_for_operation 校验 operation 存在、kind/state、取消、当前 selected branch 和 source freshness，恢复校验通过后把 checkpoint 与 operation_id 放在同一个 event 中，已有完全相同 event 时幂等返回 false。latest/at_tree_leaf 检查 checkpoint 自己的 source tip 与祖先，排除 sibling/descendant 对当前选中上下文的注入。这是 journal owner 的恢复识别设计，050 五项未执行真实 append 或重启。

core.rs:1739–2038 的 provider context owner 选择当前源和 previous checkpoint，准备 plan，建立 summary operation/request 记录，通过 input pump 传递取消和输入，限制流式字节；记录 response/usage、计入 governance，再检查取消、usage 预算、validate/finalize，调用 session append 后 restore 当前选中历史，最后按成功/取消/中断/失败结束 operation。模型调用、费用、journal、恢复责任在这个 owner；本轮没有 rerun 产品 HTTP、TUI、budget 或 process tests，也未借 050 接受 103/104 等产品义务。

## 本轮结论与历史附录说明

050 的冻结直属闭包、目录内调用和上下游 owner 连接已完成独立理解复核，候选交 master 审阅。新包离线 audit 记录原包、27 份源、012 receipt 链、当前三文件库存、十份上下文快照、旧测试计数、fixture 单行差异与此前 4432 个 ready 文件的保护校验；这些都是离线证据，不是新测试数量。

下面保留旧 3.1.17 报告正文原字节，作为历史附录。其“012 尚未接受 / provisional”是当时状态，不回写；当前变化由以上 3.1.21 附录明确说明。更早 3.1.6 prior-folder-report.md 和原失败记录在 historical050 中保留。旧报告中 immutable 或 collector 等说法以本轮对真实引用与实际调用的明确界限解读。新包冻结后停止，不接受 054 或祖先。

---

## 历史附录：原 3.1.17 current_folder_learn.md（原文）


# ZS1-050 — packages/agent/src/harness/compaction

Provisional worker synthesis, status [_]. Authority 3.1.17 / 4b4bbb809fc660d55186317e9e12cfdccc7f3a2ba4567dc94b93d96858584f1a. Dependency012 is still unaccepted in the captured master ledger. G-DIR acceptance remains pending.

The frozen subset has one direct file, compaction.ts (012), and no child directories. Real siblings branch-summarization.ts and utils.ts are context-only. Their execution does not accept extra obligations. Immediate inventory, source hashes, dependency state and reused file-packet identities accompany this report. The earlier directory draft is preserved as prior-folder-report.md. Its “immutable” preparation wording is corrected: nested message/settings references are shared.

## Calls and data ownership

Caller-selected ancestry enters prepareCompaction. The latest checkpoint supplies previous summary, retained messages and typed file details. Virtual retained entries plus later ancestry become history, split prefix and retained tail. Token estimation uses session projection; ancestry selection/validation belongs to the caller.

compaction.ts and branch-summarization.ts share utils.ts serialization, file sets, sorted lists and tags. Branch summarization also calls compaction's token estimate and request-option helpers. File sets describe assistant tool-call intent, including a failed write; they do not establish a successful write. Serialization drops result IDs/error status and truncates result text.

D050-01 executes branch preparation → shared utilities → branch summary → compaction preparation/generation → session projection. A scripted branch summary and its file tags reach the compaction prompt, but branch_summary.details do not populate compaction's typed readFiles/modifiedFiles in this fixture. They remain empty although the prompt contains shared.ts. In contrast, prior compaction details carry forward in D050-02. Prompt text and typed metadata are separate channels.

The caller owns the summary callback. History and prefix requests run sequentially; successful usage is combined and a result is returned for a host to commit. Error/aborted responses prevent that result, while callback throws escape both wrappers. D050-03/04/05 compare input and projected context before/after these failures. These functions publish no partial journal entry themselves; this is not proof of a host transaction.

Session projection selects the latest compaction, emits its summary and eligible tail, and adds later entries. D050-02 round-trips fixture entries through JSON and verifies ordered summary/tail/later input, suppression of older ancestry and error/aborted/deferred assistants, and carried typed file lists. This tests representation and projection, not upstream storage or an independent-process restart.

## Executable evidence

Five new tests passed, zero failed/skipped/todo, using real pinned Vitest4.1.9 and Node24.19.0. Source closure, command/results, prompt observations, source verification and dependency lock are frozen. Models return scripted messages; no provider network or model fidelity is claimed. Context, AbortSignal, algorithms and projections execute unchanged upstream code.

The first run passed three cases and failed two because the supplemental fixture reversed withAbortSignal arguments. Its exact source/logs remain under initial-fixture-failure. Only the fixture was corrected to the actual signal-first signature; all five then passed. No source or assertions were weakened.

The complete012 review and its37 actual-source tests are reused by reference, not rerun or added to five. Shared utility/branch-summary implementations, session projection and Context signatures were checked at their actual call boundaries.

Cancellation is cooperative: the actual signal and aborted response path are exercised.012 separately proves a callback ignoring abort may still succeed. Queue admission, tool execution, durable commit, power-loss recovery and lifecycle locking are not owned here; they are N/A rather than counted as passed.

## Target and closure

Zenpi context.rs/session.rs own checkpoint validation/projection and journaling; core.rs and its backend/budget/cancel owner invoke production summaries. Target full-turn cuts, pending-work/file retention, strict summary/provenance checks and commit-before-publication remain stronger obligations. Prior103/104/074 product evidence is separate and was not rerun.

The subset path is compaction050 → harness054 → agent/src057 → agent061. No direct child directory is skipped inside050, and context-only siblings add no accepted count. Master acceptance of012 and independent050 review are still required.

