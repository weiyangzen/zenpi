# ZS1-081 — src/domain_execution.rs 当前源码全文理解候选

候选状态 `[_]`，等待 master 独立阅读与验收。唯一正式阅读源是 `src/domain_execution.rs`；唯一 owned 报告是 `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/domain_execution.rs_learn.md`。下发时全局为 53/121，本报告不修改该计数。必要消费者仅局部取证，不给其他文件、owner 或目录增加完整阅读信用。

## 1. 身份与完整阅读证据

需求 3.1.21，requirement_digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`，捕获 blueprint snapshot `3c01628b900c9078f7c1771f3638cc366b2e295c6fede7cfea191aa0620c5c2d`。2026-09-13 的只读 main 捕获文件为 **112548 bytes、2981 行、SHA-256 `2f5ee732fdf80b99af0ab973eb4594b2369786ae4acc6a86768fc3ba49be890f`**。冻结登记的同一路径另为 **69219 bytes、SHA-256 `f46237f1ef60d830ddf7f33edb064b3843917e861702e92add3b7aa6fe56a180`**。本次以 current 全文为依据，不把 frozen 登记、历史散列或片段定位当作 current 阅读，也不反向宣称已读 frozen 正文。

全文顺序分八块读取：1–400/a50959，401–800/e8b755，801–1200/342735，1201–1600/856fd3，1601–2000/d131c3，2001–2400/6f65ea，2401–2800/126524，2801–2981/a7b571。全部完整输出；最后一行是 `validate_external_records` 的结束括号。Unix、非 Unix 分支均读过，文件内没有 `#[test]`、`#[cfg(test)]` 或内联测试模块。内联测试读 0、执行 0；这不表示仓库没有集成测试。

包内 reading-progress.json 保存连续行/字节边界、片段 hash 与下一行 2982；definitions.json 是读完后提取的函数导航及语义绑定，不代替阅读。historical-source-catalog.json 仅登记已有历史来源字节和散列，不导入旧执行信用；历史包、报告、失败均不修改、不重跑。按 learn understand 的执行约束，仅在原 session 构造候选，未建立新 task/subagent/cron。master 通知的 render 修复和 85 项 Rust 结果只是外部消息，不属于本次081执行证据。

## 2. 当前职责：三种成功必须分开（1–400）

当前文件既有早期本地确定性记账，又有外部 handoff、候选 manifest、主控准备与验收。文件头 1–9 行“does not invoke a shell”只适合本地 `run_next` 切片，**不再能概括整个当前文件**：后部 `accept_external_candidate` 真正调用 RunCommandTool 执行主控冻结的 shell 验证器。handoff 本身只发布请求，仍不会启动 worker、提供者或接受命令。

| 状态或返回值 | 代码实际承诺 |
| --- | --- |
| ExecutionStatus::Succeeded / RunOutcome::Complete | 本地 receipt 操作已成功；局部调度依赖它，不代表 Blueprint 产品工作完成 |
| HandoffStatus::Queued | 已有不可变外部任务请求；宿主明确返回 zenpi_started=false |
| EvidenceStatus::Candidate | 声明已按身份与格式导入；worker 的 passed、exit、hash 或文案不能自行获得接受 |
| EvidenceStatus::Accepted 且 receipt Succeeded | 主控路径检查文件清单并运行冻结验证器，保存观察后同一 execution snapshot 更新为接受 |
| GoalStatus::Done | headless 消费者另行要求所有 item 的最新 receipt 均 is_master_accepted，再写 domain store |

`ExecutionReceipt` 保存 execution/goal/blueprint id、version、digest、item、attempt、status、cost、descriptive evidence、external_work_executed、manifest_checksum、error。validate 要求 attempt 至少 1；成本严格为 1 attempt、1 ms、512 bytes、tokens<5000。tokens 是 estimated LOC 形成的记账值，不是实际模型计量。evidence 受单 record 字节限制且不能含任何控制字符；error 至多 4096；Running 不能附终态 error；记录序列化后还受 MAX_DOMAIN_RECORD_BYTES。external=false 必须 checksum=None；external=true 必须有小写 SHA-256。这一布尔和 checksum 的组合仍不是独立验收证明，外部记录提供更强交叉约束。

`ExecutionError::code` 把预算超量/溢出合并为 execution_budget_exceeded，单独表示 receipt/handoff 冲突和任务缺失，其他多种 schema/IO/JSON/路径错误折叠为 store_error。不能按表层 code 推断底层没有文件 IO 或持久化失败。

`BlueprintHandoff` 包含 claim_digest、精确 Blueprint 身份、attempt、dependencies、estimated_loc、instruction、acceptance_commands，status 只能是 Queued。validate 复用 BlueprintItem/Task 验证并限制编码大小；它本身没有像 receipt 一样显式检查 attempt!=0。validate_against 验证 Goal/Blueprint 绑定并重新构造整个请求后做相等比较，不应把一般格式验证和已绑定原始任务的验证混为一谈。

## 3. 持久存储与不可变规则（402–1106）

ExecutionStore 与 HandoffStore 各拥有 path、generation 与 records；execution 快照还包括 ExternalEvidence。外层 Snapshot/HandoffSnapshot 没有 deny_unknown_fields；各个公开关键 record 大多严格拒绝未知字段。unsigned digest 对 receipts/external 或 requests 的序列化内容计算，保留数组顺序，**不覆盖 schema_version/generation**，也不是签名。拥有写权限者可以重写内容并重算 digest，这些校验主要建立内部一致性而非对恶意存储作者的认证。

可变 open 会创建父目录；既有文件先拒绝末级 symlink/非普通文件，收紧权限，再读有界内容；全空白返回 MissingSnapshot。路径不存在会立即持久化 generation0 空快照。open_read_only 不创建缺失路径、不收紧权限，缺失返回内存 empty，既有空白则交给 JSON decode 失败。它返回的 Store 类型本身仍有 mutation 方法，名字不意味着编译时只读 capability；宿主是否只调用读方法是另外的约束。

路径解析器允许 ZENPI_EXECUTION_STORE / ZENPI_HANDOFF_STORE 非空环境变量直接指定 PathBuf；默认 session 指向 HOME/.zenpi，对自定义 session 则取同目录 sibling，裸文件路径回退当前目录文件名。`normalize_path` 只拒绝空路径，不 canonicalize、不要求工作区内、不阻止 ParentDir。这里描述产品路径规则，本次没有设置或改写这些环境变量。

HandoffStore::insert 对相同 ID 且全字段相同返回 Unchanged；同 ID 改内容或另一个 ID 占用同精确 scope/attempt 都冲突。最多4096 requests。新值在 clone 内 push→commit→成功后替换 self；decode 再验每条记录、唯一 ID、attempt tuple、schema、digest。find_claim 线性查找 claim；一般 insert 没有单独以 claim_digest 集合证明全局唯一，而正常请求构造会确定该值。

ExecutionStore::upsert_receipt 同 ID 下固定 goal/blueprint id、version、digest、item、attempt 和 cost；全相等是幂等 no-op。已有 terminal 后不能重新打开或改结论；已有 Running 可以变为新 lifecycle payload，甚至改变 descriptive evidence，只要记录验证成立。另 ID 占相同 attempt tuple 也拒绝，最多4096 receipts。latest_receipt_for 按完整 scope 过滤后取最高 attempt，不能搜索“曾经成功”来覆盖更新的失败/取消。

`cancel_running_for_goal` 按 goal_id 取消所有 Running receipt，跨同 Goal 的不同 Blueprint scope 也会命中；terminal 不动。它只更新 receipt，不把外部 Candidate/Validating 自动转为 EvidenceStatus::Cancelled，不发送 worker 或 OS 进程终止信号。重复没有 Running 是 Unchanged，改动用一个 snapshot 提交。reload 使用可变 open 重读后替换，仍可能有创建/权限副作用。

## 4. 本地调度、预算和 handoff 发布（1108–1760）

BlueprintExecutor 包装同一个 ExecutionStore，可取只读/可变引用或 into_store。run_next 首先校验 Goal 与 Blueprint，只有 Queued/Running 可运行。select_item 按 Blueprint.items 原顺序选择首个最新 receipt 非 Succeeded、所有依赖的最新 receipt 为 Succeeded 的 item；它不使用 master acceptance，属于本地记账调度。如果没有选择，全部 item 最新成功则 Complete，否则返回第一个 unresolved item 的 waiting_on。selection 已完成后才检查第一次取消，因此预算、状态、选择错误与 Complete/Blocked 可先于 cancellation 返回。

新 attempt 取 latest+1（checked_add，初始1），已有 Running 复用原 attempt。首次取消发生在 Running 写入前时不新增成本；新 attempt 先检查预算、构造 Running、upsert 持久化，然后第二次取消若为真，再保存 Cancelled terminal。取消后已占用的 attempt/cost 不退回。若终态写失败，Running 可能留在磁盘供恢复；恢复不会再次加成本，但仍以现有 spent 和请求0检查当前 budget。源码注释的“no external side effect”不能理解为零文件副作用：两次 receipt snapshot 本身就是 durable IO。

spent_for 汇总精确 goal/blueprint id/version/digest 的所有 attempts，不只计成功；checked_add 溢出时饱和为所有维度最大值，后续 ensure_budget checked_add 或比较失败关闭。预算维度 tokens、wall_clock_ms、attempts、disk_bytes，等于 limit 可通过，超过失败。deterministic_cost 使用 estimated_loc、固定1ms/1attempt/512B；不监测真实 worker 消耗，真实 validator 使用的是另一套 WorkerBudgetLedger。

handoff_next 使用 `select_handoff_item`，选择和跳过条件都要求最新 receipt 的 `is_master_accepted`。因此本地 succeeded 不释放外部依赖，也不会被当成已完成外部 item。已有 Running 仍复用 attempt，否则取新 attempt；无 ready item 返回明确错误而非假完成。task 必须存在。request 包含冻结 instruction/acceptance_commands，但只写 queued 请求。已有 handoff_id 时直接返回该 existing，不再在该快捷分支逐字段比较；严格相等验证位于 insert/validate_against，不应把这些边界混写。

ensure_external_pending_receipt 根据 item 与 attempt 构造确定 execution_id，已有 Running 且 external=false 即 Unchanged；该公共方法本身没有完整重复 handoff_next 的 Goal 状态/绑定校验，正常 admission 依赖先调用 handoff_next。新记录必须为 `external_owner_pending external_work_executed=false`，预算检查后 upsert。`admit_external_handoff` 虽 clone 两个 owner，但 handoff_next 已先把队列写到磁盘，再写 pending receipt；失败尝试 remove_exact，忽略删除错误。若原请求本来已 queued，也走按 ID 删除尝试。因此它是两次独立快照加 best-effort compensation，**不是跨文件事务**；失败可能留下磁盘与 caller handle 状态差异，不能声称绝无残留或所有旧队列原样不动。

identity 使用 SHA-256 对 Goal ID、Blueprint digest、item ID 的 NUL 分隔文本与 big-endian attempt 计算；execution_id 加 exec-，handoff_id 的 hash 增加 handoff\0 域，claim_digest 使用 zenpi-blueprint-claim-v1\0 域。使用完整64位十六进制，不因变量名 short 而只有截断 hash。Blueprint task 正文通过 Blueprint digest 间接绑定；生成 ID 没有直接连接任意长原始 ID。

## 5. 快照文件 IO 的精确保证（1774–1905）

read_bounded_with_limit 使用 OpenOptions，Unix 末级 O_NOFOLLOW，metadata 大小预检后 `take(limit+1)` 读并复核实际大小，避免单纯相信 metadata。execution/handoff snapshot 限16MiB；manifest 调用传单 domain record 上限。该通用 helper 没有逐祖先 openat、O_NONBLOCK 或打开后明确 regular-file 检查；外层 Store::open 有路径类型预检，但直接 read_external_candidate 使用 helper。它不能获得下文 artifact reader 同等级的 containment/FIFO 竞争保证。非 Unix 没有 O_NOFOLLOW 分支。这里没有进行 FIFO/竞争实验。

atomic_replace_with_limit 限字节，确保父目录；拒绝观察到的目标 symlink/非文件；同目录 `.filename.tmp-PID-ms` 通过 create_new 创建，Unix0600，write_all→file.sync_all→关闭→再次检查目标symlink→rename。若临时名已存在直接报错，不重试；失败清理临时文件是 best effort。Unix目录 sync_all 尝试但错误被忽略，所以不能把函数成功等同于强保证的目录元数据落盘。单文件 rename 提供替换边界，没有多进程锁、磁盘generation compare-and-swap 或跨store事务；两个旧 handle 可覆盖对方新记录。父目录路径解析和最终重命名也不是整条路径均 descriptor 固定。

## 6. 候选契约与文件产物绑定（1907–2307）

FileEvidence 包含规范产品相对 path、bytes、Unix permission mode低9位、sha256；WorktreeEvidence.revision 是排序文件清单 JSON 的 hash，**不是 Git commit ID**。FileChange 带 before/after，可表示新增、删除、修改、权限变化。ExternalContract 保存 session/execution/claim/lease/policy 身份、canonical workspace、owned_paths、host-derived protected_paths、baseline、冻结 validator argv/timeouts、准备时间、policy/lease和 digest。计算 contract.digest 时先留空自身 digest 字段。

parse_external_candidate 对原始 JSON bytes 限大小；存在 schema_version 键则严格解析 v2 VerifiableResultManifest，否则解析 legacy ExternalResultManifest。要求小写 claim/blueprint SHA、有效 execution/item ID、Succeeded、acceptance_passed=true、1–8段每段非空≤4096且无控制字符的声明。这些只决定它是不是合法**成功候选声明**。checksum 是整份原始 bytes 的 SHA，因此相同语义不同 JSON 空白也可能形成不同 candidate，immutable replay 会冲突。

import_external_candidate 校验 expected Blueprint digest、已有 receipt 的 blueprint/item/由 attempt重算的claim；只允许 Running/Succeeded，拒绝 Failed/Cancelled，拒绝已有更新 attempt。已有 candidate 全相等才 Unchanged，否则不可改写。verifiable 分支必须先有 host contract 并匹配它；legacy 可作为没有 contract 的 Candidate/history，但 accept 会拒绝。attach_external_manifest 的 checksum-only兼容入口恒定返回错误。

validate_manifest_contract 要求 contract/claim/execution/lease/policy/baseline及**有顺序的 validator IDs** 全相同；output_revision、diff_sha256 为小写SHA。changes须1–64个、hash吻合、路径唯一、精确属于 owned_paths、before!=after且不允许双None；before必须等于baseline中该路径的FileEvidence，before/after自带path须相等、每份≤8MiB且sha格式正确。这个阶段验证声明与冻结基线关系，不读取输出文件；真正当前文件比对发生在 accept。

evidence_relative_path 拒绝空、>4096、控制字符、反斜线、绝对路径、components非Normal、空斜杠片段、`.git`组件和`.zenpi-output-`前缀组件。需要注意 Rust Path::components 会规范化部分内部 `.`，因此不能仅凭该谓词说它完整检查了所有字符串级规范表示。exact owned_paths 与后续实际 inventory equality 提供另外的约束。

Unix evidence_open 从 canonical root 的目录fd开始，每层openat，NOFOLLOW/CLOEXEC/NONBLOCK，中间要求目录，末级要求普通文件且 nlink==1；逐文件防符号链接与硬链接，不等于以文件内容hash认证作者。非 Unix 分支明确报不支持 verified artifact containment，不是静默弱化通过。inspect_external_worktree 使用 canonical root、路径式 read_dir 队列，忽略根`.git`与host protected exact路径/已存在目录前缀；这部分目录枚举本身并非全descriptor固定，不能宣称整个遍历对任意祖先竞争原子封闭。

预算为最多8192枚举项（排除前计数）、4096文件、目录深度64、单文件8MiB、总读取64MiB。文件名须UTF-8，遇非排除symlink失败；文件通过 evidence_open，8KiB流式hash，循环检查取消及真实累计大小，比较读取前声明长度、读入长度和读取后长度。此检查能发现长度变化，不能证明没有同大小并发改写或在读取后再修改。最终按path排序生成revision；external_file_diff按前后路径集合的排序比较完整FileEvidence。两次inventory是两个观测时段，不是全文件系统原子快照。

## 7. 主控准备：先冻结权威和基线（2312–2523）

prepare_external_evidence 先验证 handoff_against Goal/Blueprint，再比对 preparation claim、policy对应Blueprint/item、lease ID。通过 ledger.active_lease 查询当前未撤销、未过期且policy/item匹配的租约；timeout必须1–120000、owned1–64、提交lease expiry不得晚于ledger版本。创建workspace ToolContext、编译 BlueprintGate、比对编译后的 policy digest，然后对每个排序去重owned产品路径发 write_file WorkspaceWrite gate 检查。编译失败/重复owned/越界在baseline保存前拒绝。

receipt 必须是该item最新attempt、Running，并精确为 external_owner_pending 标记，不能拿本地control_plane_only收据准备验收。已有 external record 时按claim/owned/lease/policy/workspace及timeout检查幂等，返回已冻结contract，不重新取baseline或允许改绑。contract由host产生；worker提交的manifest不能增添排除路径。

protected 集合由session journal、execution store、handoff store、domain store、session output目录及workspace/.zenpi-results/claim.json组成，排序去重。对落在workspace内的host控制文件，若worker policy允许write_file则拒绝；结果manifest目标是特例，可供worker写入。owned不能与protected路径重合或在其下。control路径存在时canonicalize，否则canonicalize父目录再接文件名。对目录型protected，inventory允许排除其子树，故排除权限是host选择的一部分，不能把排除内容算作已经审查过的产品产物。

接着读取完整有界baseline；validators来自已绑定handoff.acceptance_commands，拒绝空白和external-owner:占位命令，冻结为 `["/bin/sh", "-c", command]`；ID为索引加argv hash前16位。最终取消检查后保存 Prepared record。这里不运行validator，也不验证worker未来的实现；prepare必须早于worker文件修改，晚准备只会把已修改内容当成baseline而失去预期“之前”的语义，正常流程应按发布/准备/执行/导入顺序建立证据。

## 8. 主控验收的真实执行与 operation 绑定（2529–2847）

accept_external_candidate 先重建并验证handoff，取latest receipt确认attempt，要求已有contract，确认session_id、claim_digest、canonical workspace一致。Accepted重放直接返回旧record，**跳过重新执行validator、重新读取产品文件及后面的租约/Goal检查**；这表示对已持久化接受的幂等查询，不是对当前磁盘再次背书。Validating会报unknown outcome、需要显式reconcile，不会自动再跑可能已产生副作用的命令。

首次接受要求EvidenceStatus::Candidate、receipt Running且精确external pending标记、v2产物manifest、有效Goal状态、activelease、同item全部worker预算操作已settled；不只检查当前lease是否无reservation。冻结validator命令必须仍等于handoff.acceptance_commands。预取消保存EvidenceStatus::Cancelled；文件扫描出错直接Err；扫描成功但revision/diff与candidate不一致也直接Err，尚未发布Validating。

当前实际worktree须与output_revision及完整baseline→current diff完全相同，才能将record设Validating并先持久化。每个validator再次检查取消和activelease，operation_id为 `validator-` + candidate checksum前32位 + index。这是确定的manifest级operation身份；它绑定全源raw bytes摘要的前128bit，不可声称这里直接用完整hash或随机唯一ID。

每条operation首先向WorkerBudgetLedger.reserve预留timeout+1000 wall_ms、processes1、concurrency1以及输出disk reservation，origin=AgentTool、gate_decision_id=validator.id。reserve错误把外部evidence置Rejected。随后session.begin_operation写Tool kind与retry_requires_confirmation=true，写tool_execution_started（含argv/contract digest），创建SessionOutputStore的CommandOutputCapture，再构造workspace ToolContext、origin=UserShell，调用RunCommandTool::invoke_user_shell_with_cancel。谓词包含用户取消与本次取得的lease expires时间。

此处执行的是**host冻结的验证命令**；worker candidate只提供validator IDs，不能注入新argv。context没有附加preparation的BlueprintGate，不能把它当作worker受限命令执行；它是host的UserShell路径。tools.rs:2259–2405 的必要片段证明origin检查→run_supervised→admit_builtin/参数验证→无gate分支`/bin/sh -c`、process_group、workspace cwd、env_clear后白名单环境、管道与spawn。读取该接线证明存在真实子进程路径，不表示本次启动了它，也不把UserShell枚举本身当作批准授权。

返回观察保留validator_id、operation_id、argv、exit_code、signal、cancelled、timed_out、child_reaped、两份输出ArtifactRef、capture_errors、observed_at。成功需result Ok、exit0、无signal/取消/超时、child_reaped、输出恰2份均finalized+complete、capture_errors为空；只看exit0不够。取消包含返回标志、当前用户谓词以及租约到期。

durable顺序是 tool_output_captured event → external_validator_observed event → observations push并save_external → finish_operation → ledger.settle。settle按实测wall、process1、各输出bytes_stored+8192记disk；失败或budget directive即终结为Rejected/Cancelled，即使进程exit0也不能越过预算指示。多处`?`可在Validating、begin_operation或reservation已落盘后提前返回；没有统一自动补偿所有日志/预算失败。unknown outcome保留及禁止自动重放正是这一层的恢复边界。

所有validator完成后，再查取消、activelease并重扫整个产品inventory，要求与验证前current完全相等，避免validator修改产品文件后仍接受原candidate。随后在同一个execution snapshot clone内同时设置EvidenceStatus::Accepted与receipt Succeeded/external=true/manifest checksum/host_verified描述，commit成功替换self，最后才append external_candidate_accepted event。**最后event写失败可能发生在接受已持久化之后**；重放Accepted快捷返回也不补写这个event。execution/domain/session是不同提交边界，不能用外层Err推断接受尚未发生。

external_terminal仅改外部record status/reason，reason经过redact_text，保存record后返回Ok；receipt仍可能Running，工作区也不会回滚，外部worker不会因该方法被终止。Rejected/Cancelled无法在该accept入口直接重新跑同一candidate；策略变更、显式reconcile或新attempt需由其他owner负责，本次不推断未读恢复实现。

## 9. 重启解码验收记录的边界（2850–2981）

validate_external_records 限external数不超过receipts、execution_id唯一且已有receipt、observations≤8。contract重算空digest字段的hash、匹配execution/claim，validators1–8、owned1–64、baseline≤4096并重算baseline revision；validator argv必须恰为sh/-c/command、timeout1–120000。candidate要求checksum格式与receipt identity相符，verifiable内部declaration相等并再匹配contract。Prepared必须有contract、无candidate和observations；其余状态必须有candidate。

Accepted还要求v2candidate、observation数等于validators、receipt external=true+Succeeded+checksum相同；逐项匹配validator ID/argv，exit0、无signal/取消/超时、child_reaped、capture_errors空、输出恰2且complete/finalized，并绑定ArtifactRef.session_id到contract、call_id到operation。这里没有重新打开ArtifactStore比对输出对象真实bytes/hash，也没有直接检查两份ref必为一stdout一stderr；运行路径创建streams是一层来源，解码校验是另一层。原始manifest bytes没有存于ExternalCandidate中供重新计算checksum，checksum格式/契约/整个snapshot摘要不能等同于对原始JSON再次认证。

`is_master_accepted` 是对已验证store状态的轻量projection，只检查Succeeded+Accepted+contract+verifiable；它不是重新运行这一整套validator/文件校验的独立函数。消费者用它释放外部依赖或关闭Goal，依赖持久存储/宿主owner边界保持可信。

## 10. 必要消费者与实际宿主接线

所有消费者均来自本次另行捕获并标记whole_file_credit=false的文件；context-ledger.json限定真正读过的片段。没有读取其他owner全部源码或测试，也没有运行宿主来补证静态路径。

- **core.rs:2500–2575**：Agent.external_evidence_control要求Idle、operation_recovery为空、配置workspace；恢复已有WorkerBudgetLedger，复用现有session与output_store，Some(preparation)调用prepare，否则调用accept。不会另造平行账本；core map将底层ExecutionError折叠为InvalidTurn。
- **headless.rs:3056–3284、3285–3530**：local run解析Blueprint版本、要求唯一匹配Goal，打开execution store调用run_next（取消谓词false），返回control_plane_only标记；sync_goal_status只把Queued启动为Running，本地Complete不会关闭Goal。handoff路径先Idle检查，从匹配Goal中取第一个而非像run那样拒绝多Goal歧义，调用admit_external_handoff后追加claimed event，返回external_owner_required/zenpi_started=false。队列写入和事件也不是同一事务。
- **prepare/accept/import**：external_evidence_action从workspace的有界attachment读取preparation JSON，解析Blueprint、按claim找durablehandoff及其Goal，打开ExecutionStore，调用core共用owner；只有本次accepted且所有item最新receipt均master accepted才转Goal Done。import读取有界workspace-relative manifest，检查handoff claim，再保存candidate并append imported event，返回accepted=false、request_accepted=true、worker_declared_success=true，master_accepted另取projection。接受event、domain状态和候选导入后的event均可能晚于execution持久化失败，不能描述为一份原子收据。
- **取消接线 headless.rs:7128–7190**：先transition_goal到Cancelled，再打开execution store并cancel_running_for_goal。因此receipt取消失败时Goal状态可能已经变更；不等于worker kill已发出。
- **headless实际调度**：7811–7840与7892–7925的同步BlueprintAction分支调用run/handoff/prepare/accept/import，有owner缺失门禁；同步prepare/accept传false取消。异步Command::Slash检查external_evidence_control，5525–5542构造ResourceControl请求并提交runner，4396–4411的owner分支把token.is_cancelled传external_evidence_input→external_evidence_action。
- **TUI真实路径**：9305–9395直接adapter调用同一handoff/import/action；prepare/accept直接adapter传false。活跃UI循环12819–12827把长external控制排入ResourceControl，11745–11764 worker分支传token取消，避免只凭同步adapter断言整个TUI无法取消。显示片段334–420使用receipt.status/attempt及external标记展示状态，不应把任意succeeded文本当作master接受证书。本次不验证画面布局或replay行为。
- **governance.rs:510–555**：active_lease检查ledger可用、撤销/到期、policy/item；require_settled_item检查所有同Blueprint item租约上的未结算operation，所以验收等待的是item级worker账务安定。这里只读必要谓词，不把整个预算实现计为全文或运行验证。

## 11. 重做模块时必须保留的约束与未知

重建应先分开local receipt、queued handoff、worker declaration和host Accepted，再明确执行ID/claim/Blueprint digest/attempt的关系，避免用历史成功或worker布尔释放依赖。保持新attempt预算前置、crash Running复用、terminal不可改写、完整candidate不可改绑。保存两store admission与补偿的实际边界，若未来要跨文件事务，必须设计新协议，不能只扩大“atomic”注释。

外部准备必须在worker修改前完成，冻结owned、host保护路径、input inventory、lease/policy和来自Blueprint的validator argv。导入仅建立Candidate；主控接受前读取实际bytes/hash/mode/diff，真正运行host validators并保存operation/输出/预算观察，验证后重新确认产品未变，最后原子更新execution内accepted+receipt。禁止自动重跑Validating未知结局，保留事件晚失败与domain后提交的可恢复边界。

待独立验证的重点包括：双store写失败与补偿失败、旧handle覆盖、临时文件重名/目录sync失败、同大小并发写、非Unix拒绝、worktree权限/符号链接/硬链接、legacy候选、缺失/篡改输出artifact、租约过期、validator超时/取消/未reap、日志或settle中途失败、accept已提交后event失败及Goal后置更新。所有这些本次都只是静态结论或未执行判据；没有进行故障注入、Cargo、测试、产品、PTY、HTTP、release、FIFO、DTrace或旧runner。

离线审计只允许新auditor完整静读并冻结后执行一次，stdout/stderr/receipt在ready外，失败也不修改或重跑。它核对包身份、连续阅读范围、函数/operation/未执行判据绑定和单路径补丁可逆性，不能将产品行为标为passed。报告、ready及包外收据提交后停止，正式验收、authority/claims和全局完成数由master管理。

## 12. Final read-only identity observation

The report remains bound to the captured identities. Subsequent drift was observed and bytes preserved under final-observed; no reading credit is claimed for changed bodies:

- Docs/execution/active_requirement.json: 9853 bytes / 1 lines / `d681b392539e2e6f2bde512aeddf7ff5e6e9096405f2d5f99c39b5a3c5e55127`
- Docs/stage_1_v3_pi_mono_blueprint.md: 153896 bytes / 632 lines / `7577131ead9a6c2c77481ee58af8a83db2124e8570b0b6aad7ea54a2afc76c80`
- Docs/learn/stage1_pi_mono/claims.json: 2180 bytes / 49 lines / `e12b3c64f0174de60a43f3d3c0e002dee51c29851ca3a91e496ec48b8d42fc7c`

Historical source catalog: 20 hash-only entries; frozen-hash matches: 1. No previous owned 081 report or exact 081 package was found.

## 13. Function occurrence bindings

| Line | Function | Semantic section | Unexecuted criteria |
| --- | --- | --- | --- |
| 111 | `code` | S2 records and errors | AC-01, AC-27 |
| 145 | `is_terminal` | S2 records and errors | AC-01, AC-27 |
| 149 | `as_str` | S2 records and errors | AC-01, AC-27 |
| 201 | `validate` | S2 records and errors | AC-01, AC-27 |
| 250 | `terminalized` | S2 records and errors | AC-01, AC-27 |
| 257 | `is_lowercase_sha256` | S2 records and errors | AC-01, AC-27 |
| 299 | `validate` | S2 records and errors | AC-01, AC-27 |
| 332 | `validate_against` | S2 records and errors | AC-01, AC-27 |
| 452 | `open` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 479 | `open_read_only` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 497 | `path` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 501 | `generation` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 505 | `requests` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 509 | `find` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 515 | `find_claim` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 523 | `insert` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 562 | `remove_exact` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 577 | `empty` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 585 | `commit` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 599 | `persist_snapshot` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 619 | `decode` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 672 | `path_for_session` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 696 | `handoff_path_for_session` | S3 handoff store and paths | AC-02, AC-03, AC-10 |
| 719 | `open` | S3 execution open/cancel | AC-02, AC-05 |
| 747 | `open_read_only` | S3 execution open/cancel | AC-02, AC-05 |
| 765 | `path` | S3 execution open/cancel | AC-02, AC-05 |
| 769 | `generation` | S3 execution open/cancel | AC-02, AC-05 |
| 773 | `receipts` | S3 execution open/cancel | AC-02, AC-05 |
| 777 | `receipt` | S3 execution open/cancel | AC-02, AC-05 |
| 786 | `cancel_running_for_goal` | S3 execution open/cancel | AC-02, AC-05 |
| 815 | `latest_receipt_for` | S3 latest/receipt lifecycle | AC-03, AC-04 |
| 827 | `digest` | S3 latest/receipt lifecycle | AC-03, AC-04 |
| 834 | `upsert_receipt` | S3 latest/receipt lifecycle | AC-03, AC-04 |
| 897 | `reload` | S3 latest/receipt lifecycle | AC-03, AC-04 |
| 905 | `attach_external_manifest` | S6 candidate import | AC-12 |
| 916 | `import_external_manifest` | S6 candidate import | AC-12 |
| 924 | `import_external_candidate` | S6 candidate import | AC-12 |
| 995 | `external_evidence` | S6 candidate import | AC-12 |
| 1000 | `is_master_accepted` | S6 candidate import | AC-12 |
| 1010 | `save_external` | S6 candidate import | AC-12 |
| 1029 | `empty` | S3 snapshot encode/decode | AC-10, AC-25 |
| 1038 | `commit` | S3 snapshot encode/decode | AC-10, AC-25 |
| 1055 | `persist_snapshot` | S3 snapshot encode/decode | AC-10, AC-25 |
| 1060 | `encode_snapshot` | S3 snapshot encode/decode | AC-10, AC-25 |
| 1080 | `decode` | S3 snapshot encode/decode | AC-10, AC-25 |
| 1115 | `new` | S4 executor/admission | AC-09 |
| 1119 | `store` | S4 executor/admission | AC-09 |
| 1123 | `store_mut` | S4 executor/admission | AC-09 |
| 1127 | `into_store` | S4 executor/admission | AC-09 |
| 1134 | `ensure_external_pending_receipt` | S4 executor/admission | AC-09 |
| 1161 | `admit_external_handoff` | S4 executor/admission | AC-09 |
| 1190 | `handoff_next` | S4 handoff selection | AC-08 |
| 1244 | `run_next` | S4 local step | AC-06 |
| 1340 | `select_item` | S4 local step | AC-06 |
| 1387 | `select_handoff_item` | S4 external dependency selection | AC-08 |
| 1427 | `latest_receipt` | S4 scope/budget | AC-04, AC-07 |
| 1436 | `spent_for` | S4 scope/budget | AC-04, AC-07 |
| 1464 | `deterministic_cost` | S4 receipt/claim identity/budget | AC-01, AC-07 |
| 1473 | `make_running_receipt` | S4 receipt/claim identity/budget | AC-01, AC-07 |
| 1503 | `make_external_pending_receipt` | S4 receipt/claim identity/budget | AC-01, AC-07 |
| 1528 | `make_handoff_request` | S4 receipt/claim identity/budget | AC-01, AC-07 |
| 1554 | `claim_digest` | S4 receipt/claim identity/budget | AC-01, AC-07 |
| 1565 | `execution_id` | S4 receipt/claim identity/budget | AC-01, AC-07 |
| 1585 | `handoff_id` | S4 receipt/claim identity/budget | AC-01, AC-07 |
| 1603 | `ensure_budget` | S4 receipt/claim identity/budget | AC-01, AC-07 |
| 1655 | `validate_receipts` | S3/S4 invariant/digest helpers | AC-01, AC-03 |
| 1693 | `receipt_matches_item` | S3/S4 invariant/digest helpers | AC-01, AC-03 |
| 1706 | `same_attempt_identity` | S3/S4 invariant/digest helpers | AC-01, AC-03 |
| 1715 | `digest_receipts` | S3/S4 invariant/digest helpers | AC-01, AC-03 |
| 1726 | `digest_handoffs` | S3/S4 invariant/digest helpers | AC-01, AC-03 |
| 1734 | `bounded_text` | S3/S4 invariant/digest helpers | AC-01, AC-03 |
| 1748 | `bounded_id` | S3/S4 invariant/digest helpers | AC-01, AC-03 |
| 1761 | `validate_digest` | S3/S4 invariant/digest helpers | AC-01, AC-03 |
| 1774 | `normalize_path` | S5 snapshot IO | AC-10 |
| 1781 | `ensure_parent` | S5 snapshot IO | AC-10 |
| 1793 | `read_bounded` | S5 snapshot IO | AC-10 |
| 1797 | `read_bounded_with_limit` | S5 snapshot IO | AC-10 |
| 1822 | `atomic_replace` | S5 snapshot IO | AC-10 |
| 1826 | `atomic_replace_with_limit` | S5 snapshot IO | AC-10 |
| 1899 | `now_ms` | S5 snapshot IO | AC-10 |
| 2026 | `invalid_evidence` | S6 external parsing | AC-11 |
| 2029 | `evidence_hash` | S6 external parsing | AC-11 |
| 2032 | `read_external_candidate` | S6 external parsing | AC-11 |
| 2036 | `parse_external_candidate` | S6 external parsing | AC-11 |
| 2075 | `validate_manifest_contract` | S6 manifest contract | AC-13 |
| 2126 | `evidence_relative_path` | S6 manifest contract | AC-13 |
| 2146 | `evidence_open` | S6 artifact IO | AC-14 |
| 2187 | `evidence_open` | S6 artifact IO | AC-14 |
| 2194 | `inspect_external_worktree` | S6 artifact IO | AC-14 |
| 2289 | `external_file_diff` | S6 diff | AC-15 |
| 2312 | `prepare_external_evidence` | S7 preparation | AC-16 |
| 2513 | `absolute_control_path` | S7 preparation | AC-16 |
| 2529 | `accept_external_candidate` | S8 acceptance lifecycle | AC-17, AC-18, AC-19, AC-20, AC-21, AC-22, AC-23 |
| 2837 | `external_terminal` | S8 evidence terminal | AC-24 |
| 2850 | `validate_external_records` | S9 external record verification | AC-25 |

## 14. Operation boundaries (all unexecuted)

- **OP-01 snapshot_open**, source 452–495: Optional create/chmod/read; mutable open may persist empty state. Criteria AC-02, AC-10.
- **OP-02 receipt_upsert**, source 834–893: Validate -> clone -> persist -> publish; terminal immutable. Criteria AC-03, AC-04.
- **OP-03 handoff_admission**, source 1134–1182: Queue store commit -> pending receipt commit -> publish handles; compensation best effort. Criteria AC-08, AC-09.
- **OP-04 goal_cancel**, source 786–806: Clone matching Running receipts -> one execution commit; no worker termination. Criteria AC-05.
- **OP-05 local_run**, source 1244–1338: Select -> cancel -> budget -> Running commit -> cancel -> terminal commit. Criteria AC-06, AC-07.
- **OP-06 candidate_import**, source 916–994: Raw candidate/claim validation -> immutable Candidate commit; no acceptance commands. Criteria AC-11, AC-12.
- **OP-07 artifact_inventory**, source 2194–2288: Canonical workspace -> bounded traversal -> per-file descriptor hash -> sorted inventory revision. Criteria AC-14, AC-15.
- **OP-08 host_prepare**, source 2312–2511: Handoff/lease/gate/ownership checks -> baseline -> frozen argv/contract hash -> Prepared commit. Criteria AC-16.
- **OP-09 accept_preflight**, source 2542–2633: Scope replay/unknown-state checks -> settled item -> actual candidate output proof -> Validating commit. Criteria AC-17, AC-18, AC-19.
- **OP-10 validator_launch**, source 2634–2708: Lease -> ledger reserve -> session begin/start event -> output capture -> actual UserShell invocation. Criteria AC-20.
- **OP-11 validator_observe**, source 2709–2738: Capture facts -> output and external observation events -> execution snapshot observation commit. Criteria AC-21.
- **OP-12 validator_settle**, source 2739–2784: Finish session operation -> settle actual ledger usage -> reject/cancel if unsuccessful or directive. Criteria AC-22.
- **OP-13 accept_commit**, source 2786–2835: Cancel/lease -> unchanged worktree -> accepted evidence and receipt same execution commit -> final session event. Criteria AC-23.
- **OP-14 evidence_terminal**, source 2837–2847: Redacted reason and evidence state commit only; receipt/files/process distinct. Criteria AC-24.
- **OP-15 reopen_validate**, source 2850–2981: Snapshot structural and evidence cross-binding checks; no re-execution or artifact file rehash. Criteria AC-25.
- **OP-16 host_goal_projection**, source 2529–2539: Core owner supplies existing session/ledger/output; headless commits Goal Done separately after all latest accepted. Criteria AC-26.

## 15. Independent acceptance criteria (all UNEXECUTED)

- **AC-01 Receipt and handoff validation**, source 201–357: Check receipt attempt0, bounded IDs, control characters, deterministic cost and external/checksum pairing; distinguish standalone handoff attempt validation from validate_against. Execution count: 0.
- **AC-02 Snapshot open versus read-only**, source 452–495: Missing mutable open creates a snapshot; read-only missing does not; whitespace, symlink, nonfile, schema/digest failures and permission side effects remain distinct. Execution count: 0.
- **AC-03 Receipt/handoff immutability**, source 523–662: Identical replay is no-op; same-ID changes, alternate-ID duplicate attempts and terminal rewrites fail; Running payload has narrower invariants than full immutable content. Execution count: 0.
- **AC-04 Latest attempt scope**, source 815–893: New failed/cancelled attempt wins over old success within exact goal/Blueprint id/version/digest/item; separate scope stays separate. Execution count: 0.
- **AC-05 Goal receipt cancellation**, source 786–806: Only Running receipts of matching goal_id become Cancelled, across versions; repeated calls no-op; no worker process kill or external evidence transition. Execution count: 0.
- **AC-06 Local run and crash resume**, source 1244–1382: Pre-start cancel adds no receipt; post-Running cancel retains spent cost; crash resumes same attempt; local Complete does not imply external acceptance. Execution count: 0.
- **AC-07 Accounting limits and overflow**, source 1436–1653: estimated_loc units and fixed costs are not actual usage; all attempts count, equality permitted, addition overflow and over-limit fail; resume charges zero new cost. Execution count: 0.
- **AC-08 Dependency-ready external queue**, source 1190–1236: Only master-accepted latest dependencies release handoff; request records declarative task and does not start a worker; same-ID fast path returns existing request. Execution count: 0.
- **AC-09 Two-store admission compensation**, source 1134–1182: Inject queue/pending receipt/compensation failures, including already queued case; observe persisted leftovers and handle state without claiming cross-file atomicity. Execution count: 0.
- **AC-10 Bounded snapshot IO and platform**, source 1774–1897: 16MiB, final nofollow, ancestor/path/FIFO race limits, tmp collision, fsync/rename/cleanup and stale concurrent writer behavior; no runtime experiment in this task. Execution count: 0.
- **AC-11 Legacy/v2 parse and raw identity**, source 2032–2074: Presence of schema_version selects v2; candidate must declare bounded success; changed raw JSON formatting changes checksum; declaration never proves acceptance. Execution count: 0.
- **AC-12 Import claim and supersession**, source 905–1009: Reject checksum-only attach, wrong digest/item/attempt claim, failed/cancelled/superseded receipt and conflicting immutable candidate; v2 requires preexisting host contract. Execution count: 0.
- **AC-13 Manifest contract and owned changes**, source 2075–2143: Exact contract/lease/policy/baseline/ordered validators; 1..64 unique changed owned paths, before equals baseline, diff hash, file limits; distinguish string normalization. Execution count: 0.
- **AC-14 Artifact containment and bounded inventory**, source 2145–2288: Unix nofollow component walk, regular nlink1, nonUnix rejection, protected exclusions, UTF8, entries/depth/file/byte budgets and cancellation; same-size race remains unproven. Execution count: 0.
- **AC-15 Canonical content revision**, source 2289–2307: Sorted path union captures additions/deletions/content/mode differences; inventory revision is serialized file evidence hash, not Git commit or an atomic filesystem snapshot. Execution count: 0.
- **AC-16 Frozen host preparation**, source 2312–2511: Require exact pending marker/claim/current lease/policy gate/unique owned paths, no host-control overlap, baseline before changes, immutable preparation and frozen nonplaceholder validators. Execution count: 0.
- **AC-17 Live lease and settled worker boundary**, source 2592–2615: Goal runnable and ledger active/nonrevoked policy+item lease required; all operations on same item settled before accept, rechecked lease for each validator and before commit. Execution count: 0.
- **AC-18 Acceptance replay and unknown outcome**, source 2542–2582: Accepted replay checks original session/workspace then returns durable old evidence without current artifact/lease rerun; Validating refuses automatic command replay. Execution count: 0.
- **AC-19 Actual output proof before validation**, source 2616–2633: Pre-cancel can terminalize evidence; actual inventory/output revision and full diff must match candidate before durable Validating; malformed precheck may leave Candidate. Execution count: 0.
- **AC-20 Real host validator launch**, source 2634–2708: Reserve budget, begin Tool operation, journal start and output capture before UserShell command; frozen Blueprint argv, timeout/cancel/lease-expiry; no commands sourced from worker manifest. Execution count: 0.
- **AC-21 Successful process is more than exit0**, source 2709–2738: Require Ok/exit0/no signal,cancel,timeout/child reaped/two finalized complete outputs/no capture errors; persist host observations tied to candidate/contract. Execution count: 0.
- **AC-22 Operation and budget failure boundaries**, source 2739–2784: Finish session operation then settle ledger; budget directive rejects even exit0; intermediate failures may retain Validating, operation marker or reservation without autoretry. Execution count: 0.
- **AC-23 Post-validator inventory and durable acceptance**, source 2786–2835: Second inventory equals prevalidator current; atomic execution accepted+receipt flags precede final accepted journal event; event failure may follow durable acceptance. Execution count: 0.
- **AC-24 Evidence terminal is separate from receipt**, source 2837–2847: Rejected/Cancelled save redacted reason but do not roll back files, kill external worker, or necessarily change Running receipt. Execution count: 0.
- **AC-25 Accepted-record decode invariants**, source 2850–2981: Contract/baseline digests, candidate identity, ordered successful observations and ArtifactRef session/call binding checked; no raw output file rehash or manifest-byte checksum reconstruction here. Execution count: 0.
- **AC-26 Headless/TUI owner and Goal projection**, source 2529–2539: Core Idle/recovery gate uses existing session+ledger+output; direct and async adapters reach same actions; only all latest master-accepted items permit separate Goal Done commit. Execution count: 0.
- **AC-27 Authority and implementation boundaries**, source 1–55: Current/frozen source separate, report candidate only, current file includes shell accept path despite introductory local-slice comment; zero product/tests execution. Execution count: 0.

Historical preservation note: the one matching frozen source was copied verbatim to historical-frozen/src/domain_execution.rs with historical-frozen-provenance.json. Its presence is preservation and identity evidence only, not whole-read or execution credit.
