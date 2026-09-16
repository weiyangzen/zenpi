# ZS1-133 — tests/headless_project_workspace.rs 独立 baseline/current 理解报告

mode: understand；worker self-tested / master not accepted；文件报告不替代123/132产品整项、406目录或091根目录验收。

run_id: zenpi-stage1-20260911；blueprint_version: 3.1.21；requirement_digest: `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`；baseline_snapshot_sha256: `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`。权威selector、蓝图相关合同及目标manifest已读取并冻结，133 baseline hash逐字节一致。蓝图全文snapshot在捕获时为7d39ece35ec197cda6e6589057df93631c2aa60f27a82a39aa41a30b78346c73。

| 版本 | 字节/行 | SHA-256 | 本报告含义 |
| --- | --- | --- | --- |
| 3.1.21冻结 baseline | 23349 / 583 | 6e87b7649b0e61c8fded9ec1b7bab953a479d6442020868a3e76d408d411248d | 原12测试和全部helper，全文读取 |
| 私有最终 current 候选 | 39333 / 987 | 8375f8bab5042cd3a49a177fb5d933d03eb3eb3a615d40c22be45c28f023d313 | 仅添加404行长期回归；全文件复核与完整diff |

这里 current 指本次提交的测试候选，非假称主库已集成。主库测试文件在本轮检查仍为6e87；主库产品则并行变化，详见后文。旧helper1–95与原十二个测试全文逐字节保留；原测试从baseline97起对应候选501起，统一+404行。完整读记录、连续字节块与baseline/candidate diff分别位于ready的evidence/reading.json、evidence/read-evidence、candidate.patch。组合工具返回在450–565附近曾截断，该重叠区间明确补读f85aa2；不把哈希校验本身当作语义阅读。

文件只在 Unix 编译，使用临时目录、UnixStream、真实 asynchronous JSONL host、持久 SessionStore 与 ProjectOwnerPool。它不是进程 CLI 端到端集合，部分条目直接测 pool、协议解析器或共享 TUI host 模型。测试权限场景依赖 Unix 普通用户写权限，不宜外推 root 或其他平台。

## 原 helper 的职责和边界

- `agent`（20–34）：创建属于明确 workspace 的 initial journal，EchoBackend 保证多数测试不需要网络；显式安装内置工具和 ToolContext，并设 ApprovalMode::Never。此策略使需批准的工具流真实触发审批；不能仅从 enum 名推断所有命令行为。配置和项目恢复仍走产品接口。
- `Wire`（35–95）：input/read 与 write/output 两对 UnixStream 连接真实 `run_async_streams`。`new` 给输出设置十秒每次读 timeout，并启动 host 线程。`send` 序列化单条 JSONL；`next` 拒绝 EOF、解析 Value 并保留顺序 records；`response` 持续消费直到匹配 id 的 response，过程中事件仍入 records；`project` 构造严格 v2 project envelope；`finish` 发 test-stop、要求成功、关闭输入写端、join host，并返完整 records；Drop 只关闭输入，不保证 join。原 helper 没有整体 response deadline，也没有对 host join 的总时限。新用例不弱化这些断言，以自身子进程总时限和附加 deadline 做边界保护。

## 原十二条行为链

1. `project_controls_are_real_idempotent_and_restore_across_independent_hosts`（97–159）从 A 打开 B、重复相同 open id 得相同响应、相同 id 不同操作冲突；B prompt 的结果属于 B，A initial journal 无 turn。独立 host 重启从 checkpoint 恢复 B 与两 tab，并读到 B 的对话；选择 A、关闭 B 成功，关闭最后 A 被拒。其关键是实际 session 内容与路由一致，不只是 tab label 正确。
2. `busy_switch_keeps_queued_owners_events_approval_and_tools_in_their_project`（161–219）以 A 的 user_shell 生命周期 request_started 为切换依据，再开 B。A 未完成时，B 的 foreign cancel 被拒；B 的写文件请求排队，busy close 被拒；审批事件属于 B，批准后 B 写入自己的 marker。最后遍历 A/B 事件与响应的项目身份，两个目录的 marker 内容各自正确，进程 cwd 不变。既有 shell 内含 sleep 0.4；候选保持原文，不把它当作新 gate 的时间依据。
3. `cancelled_invalid_and_mismatched_requests_leave_selection_and_journal_unchanged`（221–255）没有 cwd 的 open 是取消，missing/损坏配置/unknown select 都失败，foreign project prompt 不能入场。比较前后完整 workspace，并检查没创建 projects 且 initial journal 无 turn，覆盖失败原子性而非只看 error code。
4. `shared_checkpoint_rejects_concurrent_stale_writer_and_corrupt_input_atomically`（257–288）两个 pool 共享 checkpoint；first 写入后 second 的 stale write 被拒且其内存 workspace 不动。过大 checkpoint 与指向 journal 的符号链接恢复也失败，前内存状态不变。没有测试所有损坏字段或任意竞争调度。
5. `tui_and_jsonl_share_the_checkpoint_and_actual_session_owner`（290–327）直接调用 ProjectRuntimeHost/TuiState 打开 B，记录项目/session；JSONL host 恢复相同 owner，再建 TUI host 得相同活动项目与 canonical workspace。这是共享持久模型互通；不是 PTY、终端渲染或键盘交互验证。
6. `project_protocol_is_strict_versioned_bounded_and_owned`（330–362）有效 guarded input_queue 可解析；额外字段、legacy project、缺 id、空 cwd、超过4096字符 cwd、动作多余字段被拒；借用型 run_headless 收 project 操作返回 requires_owned_host。它区分解析边界和 owned-host 能力边界。
7. `project_envelope_preserves_strict_identity_and_action_parsing`（366–386）使用原始 JSON 字符串保留重复 key，以覆盖顶层 id、select id、input_queue project_id 的重复键拒绝，另拒绝未知字段；合法 guarded tree 被接受。普通 serde_json Value 无法替代重复键测试输入。
8. `typed_tree_mutation_runs_in_the_selected_project_owner`（389–419）打开 B，使用匹配 project/session guard 发送 typed tree enable。响应完整 context 等于 B；A initial journal 不出现 session_tree；从 checkpoint sessions 映射读取 B 的真实 journal 确有树事件，防止标签正确但持久写到 A。
9. `project_capacity_and_checkpoint_permission_failure_preserve_the_previous_owner`（422–456）先保存 B，再移除 checkpoint 父目录写权限，选择 A 必须失败；恢复权限后验证内存 workspace 与 checkpoint 字节不变。构造至64 tab，额外 open 被 limit 拒绝且内存不变。权限恢复不是 RAII，意外提前 panic 的清理限制需要承认；本轮不扩大修改范围。
10. `selected_project_bounds_inspection_resume_and_reopening_the_initial_tab`（458–492）A 上恢复 B journal 被拒；选 B 后 resources `..` 失败而 `.` 成功并归 B。关闭非活动 A、重开 A 应恢复同 project id，独立 host 再启动也选择 A。这覆盖初始 tab 特殊身份的生命周期，没有覆盖 busy /diff 的真实 Git 内容。
11. `resumed_journal_restores_as_the_real_agent_in_the_shared_project_pool`（495–519）创建有对话的 alternate journal，JSONL resume 后重启，session id 保持 alternate、status turn_count 为2。保证恢复的实际 Agent 内容与 advertised session 一致。
12. `rejected_resume_checkpoint_keeps_actual_owner_and_host_usable`（522–583）对 typed resume 与 slash session open 各执行一轮。两个 session 的 persona ESFP/ISTJ 作为实际 owner 探针；只读目录使 checkpoint commit 失败，旧 context、persona、checkpoint 字节都不变。恢复权限、换 request id 重试成功，persona/session id 转到目标并跨重启保持。这是事务提交失败后的真实可用性与重试测试；不等同当前 C owner 正在扩展的 QueueFull/Resume 身份覆盖。

## 缺口与候选意图

旧集合没有“真实 Git A/B 不同内容 + B Agent 被 provider 锁住 + 同期本地 /diff”组合；现有 busy 用例检查审批/工具及事件身份，不能排除返回标签 B 而 diff 内容来自 cwd A。新测试复用 Wire 和 headless_protocol 中按 Content-Length 读取 loopback 请求、先发送 SSE delta、channel gate 后发 terminal 的结构；不引入 dependency，也不复制独立 JSONL host harness。独立 test process 的 current_dir=A 避免全局 chdir，子进程层清除代理/OPENAI/ZENPI 覆盖并使用私有 ZENPI_HOME，HOME/CODEX_HOME 不动。

候选记录三次 HTTP body/JSONL 交互，逐轮先看到匹配 delta 再发 /diff；零容量 release channel 保证 terminal 只能在主测试显式释放后发送，局部响应必须先到。第一轮后 /new 检查 project id 不变、session id 改变；第二轮在 A 回放 B 的已缓存响应并切回 B 再查 diff；第三轮检查 busy close 拒绝和 B cancel/terminal。每个 prompt 的所有累计事件/响应归属都校验。最终按四个 B 响应的 Git 内容统一断言，便于旧版运行完整收尾再报告实际差异；没有把旧错误作为允许结果。


## 最终新增函数逐个复核

- `busy_diff_until`（99–115）：在原Wire.next之上设单次操作5秒绝对deadline，每次读取前把socket timeout收紧为剩余时间，打印消费到的JSONL并按predicate返回；超时/EOF/坏JSON全部失败，不伪造成功事件。底层单行读取仍是原helper，不主张其为任意恶意stream的硬内存上限。
- `busy_diff_call`（117–122）：保留请求id，打印并发送v2请求，只以type=response与相同id收敛。沿途事件进入原records；缓存回放通过原响应完整Value相等断言验证，没有重组project字段。
- `busy_diff_provider`（124–182）：只接受三个loopback HTTP请求，accept前poll最多10秒、socket读写5秒；Content-Length存在且body<=262144，reader最多读1MiB；打印JSON body而非认证头。Chat SSE先写唯一BUSY_GATE_i delta并flush，再阻塞于10秒有界channel gate。没有收到release不能写terminal；超时会panic并由provider.join暴露，不能因超时解锁而让测试静默通过。取消后末帧write/flush的peer-close错误允许并记录，因为取消可能先断开连接，其余读/parse/gate错误失败。
- `busy_diff_uses_selected_project_through_new_replay_switch_and_cancel`（185–266）：正常父测试启动同一test可执行文件的exact单个用例；CHILD_ROOT分支进入场景，避免递归。Command.current_dir只改变child到A。ZENPI_HOME与Git配置、代理/OPENAI/ZENPI隔离只在child Command环境上，HOME/CODEX_HOME不变，也不输出ambient值。子进程自身PGID，stdout/stderr文件避免pipe塞满；wait由线程报告，45秒超时后SIGKILL该组并再等3秒，失败retain目录。成功wait+join后打印完整child stdio并要求host/provider joined标记。普通panic非超时路径亦retain fixture。该保底不承诺回收另行setsid逃逸的任意进程，测试只拥有普通Git子进程。
- `busy_diff_selected_child`（268–499）：在A/B各建真实Git仓库、提交base、各写不同UNIQUE_DIFF，B额外symlink指向A。配置明确loopback chat backend，通过真实Agent::prepare_project_with_options与原Wire进入实际owned async host。三轮各先见匹配首delta和B完整context，再读diff并在release前确认无prompt terminal；四个B diff在全部收尾后统一检查只含B、不含A。第一轮还拒绝parent/symlink与foreign project diff，完成后/new维持project并更换session；第二轮选择A、读取A、完整重放B缓存、拒foreign cancel，再切B读新diff；第三轮busy close被拒，正确B cancel受理，终态backend_cancelled。每个prompt的所有相关事件/响应逐个属于当轮B；shutdown context为当前B，A initial journal始终无turn，最终cwd仍A。

延后的是四项内容断言的报错时机，而不是内容采样或门控约束：Value在gate关闭时已返回并固定，gate释放前还明确检查不存在terminal，之后不从文件或产品重新计算期望值。旧版四项错误保存在wrong列表中，真实回放、取消、关闭路径先完成，最终仍必须失败。`/new`与切回B在同一综合场景内，各阶段观察共享真实持久owner；不是用标签替代Agent状态。旧十二测试仍分别覆盖独立host恢复，新增busy/diff用例未新增进程崩溃恢复测试，也不声称覆盖C的QueueFull/Resume新分支。

## 目标映射、失败和持久化边界

G-FILE的in-scope仅此测试文件。`src/headless.rs`负责准入context、local slash dispatch、replay/response；`src/project_workspace.rs`负责tab与Agent/session映射；`src/slash_actions.rs`提供显式cwd的Git diff/path边界；`src/core.rs`负责真实项目配置/backend与owner；protocol/session/approval/tools/TUI host模型分别由上述十二测试消费。它们的限定上下文及现有测试夹具用于解释调用，不冒充这些文件的完整owner接受。headless_protocol的live-steer测试、read_http_body以及stage1_input_queue的self-spawn模式只是context-only参考，其他tests没有因本报告进入406冻结集合。

文件副作用仅测试私有temp目录、Git仓库、配置、journal/checkpoint、loopback HTTP及UnixStream；没有修改主库产品、测试、authority、claims或旧ready。没有新依赖、生产API、队列/路径/replay预算调整、全局cwd/env变更或新sleep。原busy shell中已有sleep0.4保持不变。原权限测试遇早期panic时没有RAII权限恢复，Wire::Drop只半关闭输入，属于原helper局限，不能据新增用例宣称全文件所有失败路径都有硬deadline。新fixture由子进程总deadline兜底，但没有故意触发所有timeout分支。

## 实际运行与区分力

最终两组均native stable-aarch64-apple-darwin、locked/offline/jobs2，使用两个从空开始的target目录。两组224个输入除headless外全部相同，测试候选同8375 hash；当前headless=2f0835f33fb831f4ba388289df2ce5505089b333034b07cfcab57ce79b1d35c7，反例只回退headless=958837ad933fea4e55c3ac966f0c8d605dd03674a696e2ef0595b54aca628ef9，slash_actions均e8635a418ccd613ef0587695ee9318d05e679be202494c0f04eb04651c12184e。

| 最终运行 | 实際结果 | 冻结测试可执行文件 SHA-256 |
| --- | --- | --- |
| isolated-current-all13 | exit0；13 passed | c0012e68af2bc60d8a035159e12d41e205f4e228ce733184145a9fbc7869d95b |
| isolated-baseline-all13 | exit101；原12 passed，新1 failed | 7985203f9dc1d21426a5ded5ac35044839befb08400596bd308801a278548a87 |

反例明确在diff-0、diff-1、diff-back、diff-2收到project B的envelope但Git内容UNIQUE_DIFF_A；修复产品这四处均UNIQUE_DIFF_B。两组各三个HTTP、三个gate-release，host/provider线程join、测试child回收。所有其他身份/路径/取消断言正常收束，失败不来自timeout或缺provider。新测试因而对已实证的串位具有实际区分力，而非仅检查响应标签。

初版候选d08b先完成current新测试通过、原12全通过和baseline新测试四处失败。收到3.1.21后只增加parent/symlink与foreign diff guard到8375，不弱化断言。随后一次共用target目录的final-current-all13实际exit101（12过/1失败），源码为2f083却观察到A内容；失败binary`9f6ceef083df61244bbe6957cb7f04f907de89b7facc6f067715bd9647792915`、depfiles、原fixture与日志均保留。怀疑跨source共用同package target造成stale产物，未声称已证明Cargo内部根因。保持8375测试不变，在两个全新独立target重建才得到上表结果；没有删除或重分类该意外失败。今后主控组合验证应使用专用target，避免沿用本轮共享缓存。

最终cargo fmt --check与针对该测试target的clippy -D warnings均exit0；普通cargo check另有命令receipt。vendored crossterm既有unused_parens警告仍原样保留。正式全crate test、release构建、预算、CLI/PTY G-HOST与Linux/Windows未运行，当前只证明macOS ARM Rust integration host行为。原十二条中的TUI项仅模型/checkpoint互通，未启动TUI终端。没有重跑ZS1-117的任一产物或预算。

`G-STAGE --item ZS1-133 --json`实际exit1：structural.ok=true，缺少主库ZS1-133.master.json，semantic_manual.verified_by_checker=false。这是未接收状态的真实记录；本次不伪造master receipt、不写[x]、不以机器hash校验抵充逐文件语义审核。406目录须在133独立接受后另验，本报告不生成406产物。

## 证据、限制与回滚

ready包含候选单文件/完整正反patch、本独立报告、224输入清单、两套冻结源码、初版快照、全部命令argv/cwd/受控env覆盖/时间/exit/stdio bytes与hash、各测试binary、HTTP/JSONL观察和三个实际失败目录。成功fixture由原TempDir清理：保留完整测试进程stdout/stderr与busy_diff_until打印的JSONL，成功的records.json与临时Git目录没有长期留存；不谎称已经封存成功fixture每个文件。失败fixture含完整records、journal/checkpoint/Git对象，原处亦保留。资源峰值RSS没有采样，此交付不提供性能或预算结论；poll、gate、操作及总deadline是有限性约束。现场A/C并发集成不是CPU独占环境，不以本轮耗时作性能比较。

主库已并行合入headless0ece91f883f335e677cf853af7f754d013ba8d00b834018c9044fce69e3f74e4、vendor mio1afaaa245a132ea4670bd212d1c1c42a078b1fd4b950636147112485589753ef，以及tui_composer/Gantt变化；final-integrity记录准确hash和逐文件diff。本候选仍严格基于2f083产品，未混入A/C owner，主控后续组合运行才有资格判断新整合源码的通过情况。

候选送审差异16684B，仅测试文件新增404行，原12测试及helper保留。回滚用独立rollback.patch撤回这段测试增量，report为独立新文件可撤回；不得回退主库已经接受的busy/diff产品修复或删除已保存失败证据。最终是否接受133语义报告、123/132测试增量以及组合验证，由主控分别决定。
