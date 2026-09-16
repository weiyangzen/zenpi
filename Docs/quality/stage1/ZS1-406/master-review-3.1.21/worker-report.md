# ZS1-406 — zenpi tests 目录独立理解报告（3.1.21）

本报告是 `Docs/learn/stage1_pi_mono/targets/zenpi/tests/current_folder_learn.md` 的唯一候选；406 尚待 master 独立审阅及 G-STAGE --item ZS1-406。当前权威蓝图明确 406 是 zenpi tests，直属冻结依赖只有已接受的 ZS1-133。本轮没有修改主库、claims、蓝图、旧 ready 或 receipt，没有运行 runtime/build/Rust test/PTY/budget，没有接受 091/root 或其他测试文件。

权威版本 3.1.21，run `zenpi-stage1-20260911`；requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`；baseline snapshot `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`。authority 与 authority-final 保存本轮前后捕获。目标树的 manifest/index 在 `Docs/learn/stage1_pi_mono/targets/zenpi/`，不能误用顶层 pi-mono 源树 index 来推断 tests 范围。

本轮主控独立接受 050 后，selector snapshot 从 `d171b642554d2a40f3f30b814bc1e22d813356634cb560ed9f412e036262a618` 刷新为 `57bf99b6ace8b992351bda9ff152deb81d5196efb57a140a1731a72542c4386a`；requirement/baseline 不变，133、406、091 逐项记录也未改变。050 的接受不构成 406 证据。

## 冻结范围与物理直属库存

实际 `/Users/wangweiyang/GitHub/zenpi/tests` 有 69 个直属普通文件，无直接子目录。唯一 in-scope 是 `headless_project_workspace.rs`；其余 68 个都在附录逐一标记 context-only 并记录字节/hash。不存在漏过的 common/fixtures/support 子目录，也不把 Rust 文件内部 mod 当作物理子目录。

闭包为 `091 → 406 → 133`：133 是本次可复用的已接受叶子；406 只理解它在 tests 中的组合与外部连接；091 的其他依赖（090、093、097、098、405）不由此审核，091 本身也不接受。测试目录存在更多文件，不代表冻结 manifest 扩充为 69 项。目录接受不产生完整产品百分比，也不替代各产品项、工作流或 release 的单独验收。

## 133 的精确理解复用与当前身份

133 master receipt 2669 字节，SHA `a926079eeb6dbe58c891c4dafd087e4755d791cfe8c32a2024c6eabe97c8e60f`；complete=true、manual decision=accepted。receipt 绑定 baseline `6e87b7649b0e61c8fded9ec1b7bab953a479d6442020868a3e76d408d411248d`，23349 字节/583 行；连续 byte ranges `[0,2885)`、`[2885,12297)`、`[12297,23349)` 对应 1–95、96–335、336–583 行。master 另完整阅读新增 404 行及整份 worker 报告，并在 controller-read-ranges 中记录 current hash。

当前主库与本轮冻结文件都是 39333 字节/987 行，SHA `8375f8bab5042cd3a49a177fb5d933d03eb3eb3a615d40c22be45c28f023d313`。离线重建验证：current 的前 95 行加第 500–987 行，逐字节还原 baseline；第 96–499 行是新增的五个函数，其中只有一个新增顶层 test。canonical 133 报告 19814 字节，SHA `1db4d25d44079d366b99be474069f2190ebf0345571b69a7f1d01bb3851d175d`，恰为原 17951 字节 worker 报告、两个换行和 master 1861 字节 review；两个正文均完整阅读，历史“worker not accepted / 未集成”等文字没有回写。

本轮精确复用上述完整阅读链，不伪称新全读 baseline/current 的全部测试体。另新读 current 的原 helper、busy deadline/provider/父子进程清理及尾部断言等边界，用以审视目录生命周期。receipt 所列八个 artifacts 全部复制并校验，包括 master-busy-test-shell 的 review、verification 和原始 integration-evidence.tar.gz。tar SHA `4a83e72147aadc78a36fe60b395b1ff121d4c7cf6d8a0acc2c0edd04dfbbc33f`，103882793 字节；只离线读取所需日志和 baseline，不执行其中的程序。读取/hash 全包不等于逐文件语义阅读。

## tests 目录承担什么职责

这里的文件把产品公开合同转成独立可失败的回归场景：通过实际 Agent/SessionStore、协议编解码、project pool、共享 TUI 模型和必要的局部服务，检验输入、响应与持久状态是否属于同一个 owner。目录自身没有生产调度器，也不是持久化层。133 的核心作用是同时观察 wire 身份和实际项目文件/journal 内容，防止“标签正确、执行 owner 错误”被单层断言漏掉。

Cargo.toml 未声明 tests 自定义路径或关闭自动测试发现，默认 features 为空，dev-fixtures 是显式可选特性，tempfile 为 dev-dependency，lib.rs 导出 zenpi 公共模块，main.rs 只是调用 core::run。133 通过 `use zenpi::…` 接入库，启动的是 `run_async_streams`，不是生产二进制或 PTY。目录不能一概描述为纯黑盒：导航与限定源码阅读确认 resources.rs、tools.rs 用 `#[path = "../src/…"]` 引入源模块；mode_boundary.rs 还通过 CARGO_BIN_EXE_zenpi 检查真实 CLI 拒绝非法 mode 的副作用边界。这些兄弟条目只用于说明目录形态，没有被逐文件接受。

133 以 `#![cfg(unix)]` 限定整份文件，UnixStream、Unix 权限、process group、libc poll/kill 都是实质条件。其余文件有不同的文件级/局部 cfg，不能从同属 tests 推导跨平台等价。CI 配置在 Ubuntu 使用 stable、fmt、all-targets/all-features Clippy 和串行 all-targets/all-features/locked tests，注释说明短期 loopback listener 的资源竞争；另有独立 blueprint、runtime budget、JSONL/用户烟测及 production package 步骤。读取 CI 只说明计划入口，不证明这些步骤已在本轮或当前修订通过。历史 master 的 macOS 56 项仅选中两个测试 target，也不是 CI 的整个矩阵。

## 直属子集的调用、输入和 owner

133 的 `agent(root)` 在明确 workspace 打开 initial journal，安装内置 tools 与 ToolContext，通常用 EchoBackend 并设置 ApprovalMode::Never。审批语义由产品 policy 与请求路径决定，不能由枚举名猜测“无需批准”。多数原测试用 Echo 隔离模型网络，但真实 journal、工具、审批、pool 和 wire 行为仍运行。

`Wire` 创建两对 UnixStream，主测试持有输入写端与输出 BufReader，后台线程取得 Agent 与另两端，并调用 `run_async_streams → run_async_stdio`。send 写一条 JSONL；next 读取、拒绝 EOF、解析 JSON，并累计 records；response 按 type=response 与 request id 消费，期间事件仍留在 records。project 发送 v2 envelope；finish 发 shutdown、验证成功、半关闭输入并 join host。原 Drop 仅关闭写端，没有 join 保证；每次 read timeout 不是整个 response loop 或线程 join 的总 deadline。

协议层 ProjectControl 对 action 使用 tagged enum 与 deny_unknown_fields，项目入口要求当前 schema/version、id、合法 cwd 或 project id。133 的原始重复-key 字符串用例不能被普通 JSON Value roundtrip 替代。项目匹配、请求去重、cached terminal、event correlation 和 live scheduling 属于 headless；workspace canonical id、ProjectContext、Agent handle 和 session mapping 属于 ProjectOwnerPool；journal 由 SessionStore 持有。Arc<Mutex<Agent>> 允许后台请求占住 Agent 时前台继续选择/检查项目，响应必须使用入场捕获的 owner，不能晚取 active tab 代替。

本轮对当前 headless 的直接连接复核确认：busy `/diff` 分支从捕获的 `project.cwd` 调用 `slash_actions::diff_value_at`，不必取得正被 provider 占用的 Agent mutex。其他 slash 路径仍走 try_lock/各 owner 分派。diff_value_at 从显式 workspace canonicalize，再 resolve_relative，执行 Git 并返回内容与路径结果；因此 133 在进程 cwd=A、活动项目=B 时读取不同 Git 内容，是定位这一连接的有效反例。仅检查 response.project=B 无法替代内容断言。

project pool 的 publish 先构造 next_sessions、准备实际 Agent/config/tool cwd，并检查替换运行 owner 的约束，再写 workspace checkpoint，之后发布内存 owner/context/session 映射。resume 的 commit_resumed_session 先把新 session mapping 提交到 checkpoint，再更新映射；实际 Agent 替换由调用者持有旧 Agent 的路径完成。/new 则有自己的 durable commit/context 更新接口。这些 owner 连接解释了原测试为什么要核对 persona、session id、journal turns 与独立 host restore，而不能只核对 tab。

## 跨文件不变量与各证据层的关系

133 中的原十二条通过已接受完整报告复用为四组明确合同：项目 open/select/close 的稳定身份与 independent-host 恢复；busy 期间 queue/cancel/approval/tool 的 A/B 归属；strict version/key/path/context 的拒绝；checkpoint stale writer/权限/损坏、resume/tree 的提交和实际 owner 恢复。它们的职责相互连接，但不是 12 个互相独立的平台保证。

新第十三条填补“同一 busy owner 上的本地 diff 内容”缺口。A/B 创建真实 Git 提交和不同 UNIQUE_DIFF，B 有逃出到 A 的 symlink；child cwd 固定 A，选择 B 后三轮接收 loopback provider 首个 SSE delta，terminal 由零容量 release channel 控制。四个 B diff 在 gate 打开前已经采样，最后才汇总内容断言；延后的是报错时机，不是把采样移到空闲状态。期间还验证 parent/symlink/foreign diff 拒绝、/new 的 project id 不变但 session id 改变、切 A 的真实 A diff、完整 B cache replay、foreign cancel 拒绝、切回 B、busy close、正确取消与 late events。完整场景语义来自 133 接受链，本轮没有重新运行。

相邻 project_workspace.rs 本轮完整读其模型合同：候选 workspace 构造不改旧值、canonical path/alias 身份、tabs 上限、序列化与非法 checkpoint 拒绝；它不持有真实 Agent pool 的全部事务。tui_project_workspace.rs 的 helper 与局部断言使用 TuiState/ProjectRuntimeHost/TestBackend、直接 key/event 和 checkpoint dirty 状态；这层不是 PTY。headless_protocol.rs 的 helper/Content-Length reader 与已接受 master 集成报告提供并行请求、重放和 SSE 夹具的上下文；133 没有 import 该兄弟文件，而是在自己的测试 target 内复用 Wire 并采用相似服务结构。没有共享 tests 子目录意味着不能假设所有 helper 统一实现相同 timeout/清理策略。

不可跨层替代的四个不变量是：入场 project/session 身份须随所有 event/terminal 保持；实际工作目录与 journal 必须与身份一致；失败的 owner 切换不得仅发布 metadata；缓存重放必须是原已接受响应，而非依据当前活动 tab 改写。新用例的 /new、切换和取消共享持久 owner，属于单一生命周期场景；不能由 56 总数推出每个失败分支、跨进程恢复或所有 TUI 行为都覆盖。

## 副作用、错误、取消与回收

测试资源属于私有 temp 根：A/B Git 工作树、配置、journal/checkpoint、UnixStream、loopback listener、host/provider 线程以及一个 self-spawn 测试子进程。Git、Unix 权限和普通用户身份是环境前提；未使用远端模型输出证明摘要/推理语义。child Command 的 cwd/env 隔离不改父进程 cwd 或全局环境；只清理 child 的 proxy/OPENAI/ZENPI 覆盖并设置私有 ZENPI_HOME/Git config，HOME/CODEX_HOME 不变。

新 busy helper 设每个动作 5 秒绝对 deadline，provider poll/读写/channel release 有各自界限，HTTP body 上限 262144、reader take 1 MiB。父测试给 self-spawn child 独立 process group，stdout/stderr 写文件避免 pipe 堵塞，45 秒等待超时后杀组并再等 3 秒；正常路径 join waiter，检查 host/provider joined 标记。provider 取消后的最终写 peer-close 可被记录容忍，其他读/parse/gate 错误失败。普通失败保留 temp root；成功 TempDir 清理，历史完整 stdio 保存了 JSONL/HTTP，但成功 records.json/Git 目录没有永久封存。

这些边界不等于对任意恶意无限行、逃逸 setsid 子进程、所有 deadline 分支的证明。旧 Wire response/join 没有整体 deadline，Drop 不能保证线程 join；原权限测试使用手动恢复 chmod，提前 panic 不保证 RAII 恢复。报告保留原限制，不以新增一个外层 timeout 替整目录担保。原 busy shell 自带 sleep 0.4，并非新 gate 的依据，本轮也未删除或重跑它。

项目 checkpoint 的具体持久化 helper 使用 bounded read、Unix O_NOFOLLOW、lock file/flock、比较 previous bytes、防 stale writer、create_new temp、write+sync_all+rename；错误时删除临时文件，内存 publish 在成功之后。这解释普通权限/竞争/损坏测试的原子性断言。本轮只读这些相关函数，未验证掉电耐久性（函数片段没有 parent directory fsync）、所有跨进程竞态或整套存储恢复。独立 host 重建不等同进程崩溃恢复；live cancel 也不证明所有未知外部副作用可自动回滚。

## 历史运行证据与当前变化限定

主控 133/132 的历史组合是 test `8375…`、headless `9440492db142157995e0b479c2d31b80d9158ae624ca29d08f4d93db34e81509`、mio `1afaaa245a132ea4670bd212d1c1c42a078b1fd4b950636147112485589753ef`。实际 root command 使用新专用 target、stable-aarch64-apple-darwin、offline/locked/jobs2，选择 headless_project_workspace 和 headless_protocol：13+43=56 个顶层 Rust tests，exit0；self-spawn 的嵌套 1 passed 已包含在 13 内，不能加成 57。Clippy all-targets 和 fmt exit0。root-tests.log 54539 字节 SHA `344f0fe9216fd00282535e51ecf8d5b3d243d1a150a0cd917dc855a1cd6c985a`，run receipt 的 argv、起止时间、reaped 和 log hash 原样保存。

离线抽取看到三个 HTTP、三次 release、host/provider joined，以及四个不同 id（diff-0、diff-1、diff-back、diff-2）的 B-only 内容响应。日志另有一次同 id 的 diff-1 cache replay，所以实际抽取五条 response 记录，仍是四个独立内容采样。真实 shell embedding 的原 52 检查中 6 fail/current 52 pass 属于另一组 public run_stdio/run_stdio_owned 验证，不计为 Rust 用例，也不让 406 接受同步 shell 产品义务。其完整报告已读、原始证据归档保留；本轮没有全读其中所有 probe，也未重新执行。

更早 worker 使用 headless 2f083 的成功、958837… 的旧十二条通过、新一条失败反例、共用 target 意外失败均保持历史身份。共用 target 的失败 binary/depfiles 和保留 fixture 未删除；“疑似 stale cache”没有升级为已证明的 Cargo 根因。旧12 pass/new1 fail 的实际区分力通过 133 接受链复用，不与主控当前组合拼成一次执行。

本轮捕获到 test 仍为 8375、headless 仍为 944049，但 mio 已为 25801 字节、SHA `41321e242e21fa85533dbae42314bcf80ab916e1d55e065d5ce98076002ac93f`。对历史 224 输入逐一只读比较，另一个变化是 tools/generate_stage1_gantt.py（12693→12781 字节，61124…→95dbdd…）；其他 222 份身份相同。完整精确 delta 与当前输入清单已保存，不能因 test 字节相同便宣称 mio 新组合已通过旧 56 项。mio 仅作 build-input 身份上下文，406 不逐文件接受它。主控后续任何新 build/PTY/release/budget 由各自任务负责，本轮不触发。

封包前新增离线审计曾因 live selector/blueprint 快照变化返回 exit1，失败记录 `pre-freeze-live-drift-audit.json` 原样保留；当时 224 个源码输入均未变，唯一失败检查是 authority 与 live 身份不一致。新增 authority-after050 保存 57bf99… 快照，authority-final 更新为 `2bb12c5f41ba8b290fb0a6f74cac1a98bab8faa095775f2463991e58003b246d`，完整 checkbox 差异保留为 patch；逐项再次确认 133/406/091 义务与 requirement/baseline 均不变。这是捕获期间主控权威刷新，不是测试或实现失败，也不扩大 406 接受范围。

随后的一次 live 输入检查真实返回 exit1，原记录 `pre-freeze-input-drift-audit.json` 保留：主控继续集成 headless 与 Gantt 两个文件，133 及其余 tests 未变。初始 current 快照不覆写；新字节位于 postcapture-current，前后完整 patch 和身份在 postcapture-diffs / postcapture-live-input-delta.json。最新 headless 为 369008 字节、SHA `31ea4fcc152f4c9bc067b721a0aea18dbbec2eb3b1cbc9a5081fc462d70bc932`；Gantt 为 12774 字节、SHA `a3aaacaa5b10a8bedabbe7bc14342ae355cfea05e08bb4a5a8b68fc77d874ee4`。相对历史224输入，现在共有 headless/mio/Gantt 三个不同文件发生变化；前面的“两项变化”专指初次捕获，不能读成最终主库仍只有两项变化。

本轮完整阅读新的 headless 差异：shutdown_and_join 后改为先释放 replay ownership，close_async_owners 去重 Arc、try_lock 可用 owner 并 unregister/try_close；WouldBlock owner 交给一个新 cleanup 线程，poison/close/spawn 错误有各自报告路径。该线程等待延后的 Agent 锁，进程可能先退出，代码没有由本测试 join 该 cleanup 线程的保证。原 Wire.finish 的 host join 不能证明这些非合作任务的 close hooks 已全部结束。此次变化不动 busy diff 的显式 project.cwd 分支，但影响关闭生命周期，因此旧56通过严格保留旧944049身份，本轮不宣布新31ea关闭实现已验证或已接受。Gantt diff只更新进度描述，也不充当产品证据。最终审计同时核对初始快照及明确登记的新字节；不会通过忽略任意 live 差异来变绿。

第三次 live 检查失败仅因 Gantt 从 a3aaac… 回到 95dbdd…；headless 保持 31ea4f…，所有 tests 保持不变。该失败、最新全输入观察及完整 Gantt 差异分别保留为 pre-freeze-second-input-drift-audit.json、latest-observed-inputs.json、latest-observed-diffs。本轮已读这个仅进度文案的回变；最终明确登记 Gantt 12781 字节/95dbdd…，不把进度文案当权威产品结果，也不覆写中间快照。

## 本轮结论与交接

冻结直属子集具备已接受的 133 叶子，目录职责、配置入口、真实 owner 连接、跨层不变量、资源生命周期、错误/取消/持久化/恢复边界均已独立说明；以下完整物理库存使 context-only 文件不被隐式接受。新的离线 audit 验证 scope、receipt 八件证据、完整 baseline 重建、历史命令/log 身份、当前 69 个直属文件、224 输入变化与旧 ready 保护。audit 是读/hash/JSON 的证据一致性检查，不代替本报告的语义审阅，也不是新的产品测试。

候选冻结后停止；若需撤回，只撤此目录报告。不得回退主控已接受的 133 测试增量、headless 修复、mio 独立工作或历史失败证据。406 与 091/root 状态保持主控权威，不能从本报告自行写 [x]。

## 附录：逐一物理直属库存

以下为捕获时字节身份；全部文件均为普通文件，无物理子目录。名称与 hash 的逐一列举不声称 68 个 context-only 文件的实现已全部语义阅读。

| 直属文件 | 字节 | SHA-256 | 冻结归属 |
| --- | ---: | --- | --- |
| approval_owner.rs | 15046 | f0450d952d6324f10af1b4c99374d6e0acd9a9c9b19c3cb87f8917d01703a11d | context-only |
| b3.rs | 6637 | 48bfef55f80b6fa5de579a2b512bae24b5a62d9f9e2bcf1f86ed9d4f44c902e1 | context-only |
| backend.rs | 42058 | 5bc0d728c8c51c35e09c5ed172de971fe9be9f9b9837863ee4dbd5f8720fc60b | context-only |
| config.rs | 21526 | 4d989e7b21ed1fff3bc0e022ee1f4191a13452930254f14fe3f982b8f2db0fd9 | context-only |
| context.rs | 2301 | 0cf5a72cbe2d94a348688f5680b6a052b1215880a7f864d7eb4ca2295088cfd8 | context-only |
| core_session.rs | 47139 | e517fbb10658845ff8fee6c2cc8570ae9398f4effa8a7ab78fafcab2ccd65a43 | context-only |
| diagnostics.rs | 2304 | 2b39e902a04aa7d0b0448a3e2edd0d878e4ac5bef0d0bc2bfb8c2728d427ecf5 | context-only |
| domain_execution_host.rs | 10358 | fd335222c330c25e4bd500557279c572b37b482915c88c551c6edb4a1b2ca4c6 | context-only |
| domain_execution_owner.rs | 21741 | 32b46f673146f204695f16809091c8760ac9796dd788ad17f7546737b1ab0960 | context-only |
| domain_store.rs | 10078 | 620e39629490e7aa72dec5823953922ff19198e0f1a4ad54ee7fe172ecce4aff | context-only |
| domains.rs | 8780 | be9ed66f4a5c69b75ceb15f3f858cde8f0045a9a262d32c5815604cce4b00235 | context-only |
| extensions.rs | 6302 | 00a3873257e010a9fdd337d8b913fda3f2efe101710c45ae225975ab59c2237d | context-only |
| governance.rs | 26457 | d84c597cd9bb87212d0a19f5b19d2470d69e11fbd3ee1bf6df986978196e6ce0 | context-only |
| headless_domain_owner.rs | 12107 | c4bbc4e80ea3b00b28bb850e67de1dded0bdce3acb981fe4f2904bfb143acc3f | context-only |
| headless_event_budget.rs | 7020 | 972b44c6def7be8c76e8c3b05fbf183f812da3a0009b3be5c1f67959f79a3a45 | context-only |
| headless_project_workspace.rs | 39333 | 8375f8bab5042cd3a49a177fb5d933d03eb3eb3a615d40c22be45c28f023d313 | in-scope ZS1-133 |
| headless_protocol.rs | 94554 | 38254448191f442346a1846ecf716baf204ffaa9ea2d80f146e8adf426a740e4 | context-only |
| layout.rs | 9505 | 92f295df2aa4ddb0133459be696ff440b31df6654d2680298c5d6f0eb7f68062 | context-only |
| layout_persistence.rs | 6463 | 92b67b85caa6874a8d7e0e644550075ba31bbfae0d82d1e867fbd2d6e978ac36 | context-only |
| learn_owner.rs | 11354 | b3203b563f1caa04abfd024c0fa31806325d3c6dfa0bc9d657a7f3a67192297d | context-only |
| mode_boundary.rs | 898 | 26b48e33581f8a0aaf523f03f31cfeaef832d89aa59bb5ac39d67ad6c682db4c | context-only |
| project_workspace.rs | 8102 | a8554809789df1835ac9def276bc38af6b544af804767fa1fd01d4218f967f9e | context-only |
| render_markdown.rs | 2001 | eeaad94245eba83dd9ec32b261dd79f50eb29672e2a58d48634e37ebeb09b664 | context-only |
| resources.rs | 3175 | cb582df351db9360d0efeb107ccbe8a9af421e7dfdeca8c6c32fb0f89c045abe | context-only |
| resume_compact_owner.rs | 9368 | f7569431831772eb821128e884cc7b43b34d00d6248cb44e6679d5c8eaba2438 | context-only |
| runtime.rs | 19172 | ea811a89941921209a27f2cbd3d8a63aafa26a12f47529f1a2c2709112d5b79f | context-only |
| runtime_intent_owner.rs | 16358 | 1a6469d599f57f1e1564c9fc931f872e9b32db2eed1b4b43ac40557ddc6e04ba | context-only |
| security.rs | 4362 | 9abf63b1b7ca935c2aa35c05f3d3e6551c4e8e7f433cf60f9a72c9c10a13ead8 | context-only |
| session_cli.rs | 5057 | 7b04e38376e33aea34f142d532f4d646b7765b4578e5d147eaf1a20077f9549a | context-only |
| session_maintenance_owner.rs | 12526 | 0dabd67a538416bec9b13fd8728589edc92065e00f8dd523a7702a90bdf9a2ee | context-only |
| session_new.rs | 33557 | c747a4b10c1dba4b910cb68854381b74be91711d187ec7e2e8cb1d84d8285211 | context-only |
| session_recovery.rs | 50603 | 883bfa58f4b5650772aa902ec3ac95d4029bbd1939f56b4fbf0c8f294275474b | context-only |
| skills.rs | 3689 | eb6bed86ce15a75ae28856fbf8b0dc0547c4fcaea886b32cb4258a042ba9f157 | context-only |
| slash.rs | 18879 | 59f168427f1567b9721d766c7f445b6e141a513e1cdf1ae69522f7644fe02b01 | context-only |
| slash_actions.rs | 11437 | 31259e720ffaa30dc1c54c2abf07a2413eb3cefaee0881c74a85e55dd1ae4180 | context-only |
| stage1_anthropic.rs | 36677 | bab4b7d3e846d2dadc5e8b7083ad5713acbe23e3315742c675f7ac5f79b02550 | context-only |
| stage1_branch_owner.rs | 23092 | 5552aaf3c061601c36c455d82b4a8d761172839de96b01cfc817522b028615dc | context-only |
| stage1_chat_stream.rs | 54356 | 39710fa209ec83b3de359b9574c08b4b8ae1740aff2c93c3925df256829223f3 | context-only |
| stage1_context_checkpoint.rs | 11776 | efb00e5ad9066defbc3925ef6b665d03e9dc6106d5afa3e250cfcd35709a8628 | context-only |
| stage1_extension_hooks.rs | 36581 | aa7ed6e8fefe1ec513fcbf0753f17e082b103e381b0eb8c653a6b4cd126d0453 | context-only |
| stage1_external_evidence.rs | 28662 | fa9a2d185df8482adecce9b1062b32c3655399c848d44cd6b34f93ec1b1d7888 | context-only |
| stage1_gemini.rs | 35033 | 0b89fa8b1587cfbd1ed36fb53b7280d79586acd3323da1e2b4280cb033d85367 | context-only |
| stage1_input_host.rs | 56599 | 87726f7d1e9aadc411e3421fa80ecab0ef88b93dd7523220036136d079dbe143 | context-only |
| stage1_input_queue.rs | 22941 | 4a089e0fbb995189e5407db251f165e88ce386e141a679cd019f5504ffa32115 | context-only |
| stage1_model_registry.rs | 30356 | 8a15bfabff83fe2cab6be977d20d65afe116301faa6c39c1ebfdda6e125ec87b | context-only |
| stage1_project_config.rs | 3698 | 234e444d060178c1afdc0569a4294b6c3d3db3d5404fa29d60361c49d206ecf5 | context-only |
| stage1_prompt_resources.rs | 16193 | 1bbdbff6927a996a06c08bb7afe84d9a94f25722b89c757d7bd1867a9c46a40f | context-only |
| stage1_reasoning_owner.rs | 10051 | 6eac44dab480a2dea83204799d78a56efcafc8d6440c5c923d89c966ce6f2a48 | context-only |
| stage1_resource_host.rs | 23253 | 6bfe5c0861bcd58c6dd42b8ca6a8ef4505ee8b030e20974cdc0024ba7695f4ae | context-only |
| stage1_search.rs | 33049 | 6f3aeec6040d2c9602d15e1b4ee2664d877f80470fdbc77d83d991bb90f76e1a | context-only |
| stage1_semantic_compaction.rs | 38971 | 797df7962c8c50d0f82609e8624ab2d5c8021f8fe6e197b8d9569005bb19cbdd | context-only |
| stage1_session_tree.rs | 20398 | 4840bf1afa38d55d38af01f4c8232b25b8b66124441f60e6b22f0604b3c6a340 | context-only |
| stage1_skill_md.rs | 43190 | 7f3399a0b765e66a7613262ab1c38aafe7bdc24afb1aba49de46161e9fcbe060 | context-only |
| stage1_tool_batch.rs | 35892 | d9db142d2d167f9c22c24b3f46af95c2e66ec665de636247b732b3115f0c4b72 | context-only |
| stage1_tool_output.rs | 96655 | 613f447e823baa5068fc107b72627e6ed4efdab486f3aa170234a288d39f29a4 | context-only |
| stage1_transport.rs | 10900 | 563af0a75630653ad1af61179832183955d0fc2be36b56f6c886db636265adf7 | context-only |
| tools.rs | 42797 | 8c9ac008bd8a2e253c821883a91f25c3392f5c1275f168dc9c2c066ab2527dd2 | context-only |
| tui_approval_focus.rs | 14718 | 6399e0f364781639a49e27643596c2eb965773147c2aae965db1a69318026b87 | context-only |
| tui_bentobox.rs | 19019 | 0b847c54c537ec5c81a175d23cac846824a6545d522e7b5934043018bc22be17 | context-only |
| tui_command_palette.rs | 25893 | 3b84deaf2d41b34eae9014861570b5e9e9ad7c1789a939554acd53caa096b41e | context-only |
| tui_composer.rs | 47476 | c56a654e5f03ef691f9bb4ffb0ae57ee3577b6630309dee9367debe503da210b | context-only |
| tui_goal_owner.rs | 7946 | c1f061cb625dbe024507f056839b67c7f639b315b78b54c2c8acc64caf375f74 | context-only |
| tui_interaction.rs | 18433 | d5b701b328fe2053f1017be0bb8ce5c60fdeb5fe5470d20294c1f673ddc5f697 | context-only |
| tui_logs.rs | 6421 | 0704b297d4d0de447cceb3ff5577e130b2f7be7246a53c3fd87691b50b88c483 | context-only |
| tui_markdown.rs | 2192 | d932bf09f23142975046a3647a4f75609e8c3416ba329a2c96d00e06b0de30e8 | context-only |
| tui_project_workspace.rs | 16429 | a11bdc9f15ac3b9b4b7461d48e84a0975aa0ba177667afa8b27a78c9c2e4b180 | context-only |
| tui_resize.rs | 11582 | 6ee3fd86146ce7575d7fc5ea2b32d2aa26577ad3969d8c8dab4085783d342633 | context-only |
| tui_transcript_ux.rs | 16949 | 26562ab62ebf73a8eae1fd951ac0a247eb4adb9fc8711e87a3fc55d0b5ebd5d7 | context-only |
| view_model.rs | 12246 | 4cadeff6ed161894a4e7265170b2461621c6ebc24f09db7770f26066c9f9c9c3 | context-only |
