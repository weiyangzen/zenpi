# ZS1-405 · zenpi/vendor 逐目录理解候选

本报告唯一对象是主库 `vendor` 容器目录。该目录真实直属文件为 0、直接子目录为 1（`crossterm`），冻结索引直属文件也为 0、唯一正式目录子项为 ZS1-404。报告梳理本地依赖的接入、跨目录调用和证据边界；405 自身仍是候选，403/404 尚未主审，不宣布 vendor 或 root 已验收。

## 1. 身份、旧报告与本轮范围

主库只读根为 `/Users/wangweiyang/GitHub/zenpi`。快照时间为 2026-09-12T18:18:18.573947+00:00，蓝图/selector 对应 snapshot `dd9a350e6caca8f8701acb6e07529b44f911a1192711b16b775a215252c75727`，requirement `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`，baseline `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`。唯一 owned path 为 `Docs/learn/stage1_pi_mono/targets/zenpi/vendor/current_folder_learn.md`。

主库与 worker checkout 的该 canonical 405 报告都不存在。另在两处 `.ops` 中按 `current_folder_learn.md` 文件名检索，排除 build*/target 生成目录，未找到 `/vendor/current_folder_learn.md`；这是明确搜索范围内的缺失观察，不捏造旧405正文或旧阅读链。首次只读库存命令发现 worker checkout 没有 `vendor` 后退出 1，后续未执行的读操作随后针对主库完成；该观察单独保留。

worker HEAD 为 `4dbd330d0a8cf58576109713f774969babd499ed`，非 `.ops` 状态仍为原有94条。全部新增文件只在本轮私有 work/ready 中。131 上一轮中断的代码、实验和日志保留；本轮不恢复、不运行、不将其提升为完成或405证据。

## 2. 直属库存与职责

| 直属项 | 真实类型 | 正式范围 | 职责及本轮处理 |
| --- | --- | --- | --- |
| `crossterm/` | 普通目录，非 symlink | ZS1-404，尚未接受 | 本地终端依赖源码；核对其直接库存，复用404阅读链解释接入 |
| 直属普通文件 | 0 | 冻结直属文件0 | 没有需要伪造文件理解条目的对象 |

`vendor` 本身没有 `Cargo.toml`、README、`.gitignore`，也没有第二个 vendored crate、直属脚本或链接。本轮读到的项目根 `.cargo` 及 `.cargo/config{,.toml}` 不存在；这仅排除了这些项目路径下的额外配置，未检查用户级 Cargo 配置或环境覆盖，不据此声称所有机器构建方式唯一。

唯一子目录 `crossterm` 的直接库存逐项与404一致：9文件 `.cargo_vcs_info.json`、`.gitignore`、`.travis.yml`、`CHANGELOG.md`、`Cargo.lock`、`Cargo.toml`、`Cargo.toml.orig`、`LICENSE`、`README.md`；4目录 `.github`、`docs`、`examples`、`src`。该表用于确认直接子目录的身份与角色，没有把孙目录递归转成405验收项目。其内部源码所有权、平台分支和既有缺陷仍由404/403/402等各层独立证据承载。

因此 `vendor` 是受根项目选择的源码容器，没有自己的程序入口、独立 workspace 清单或事件循环。终端原始模式、命令输出、事件读取、解析和 reader reset 属于 crossterm 实现；项目状态、编辑器会话、审批、持久化和进程清理并不会因为源码位于 vendor 而转移到该目录。

## 3. 根 Cargo 如何接入唯一子目录

根 `Cargo.toml` 50行完整阅读已在400建立，本轮与404保存的对应全文逐字节一致。根依赖声明为 `crossterm = { version = "0.29", features = ["bracketed-paste"] }`，未设置 `default-features = false`；另有 `ratatui = { version = "0.30", default-features = false, features = ["crossterm"] }`。根 `[patch.crates-io]` 将 crossterm 指到相对根清单的 `vendor/crossterm`，不是指向本层 `vendor` 自己，也不是把任意 vendor 后代自动加入 Rust 模块树。

子目录规范化 `Cargo.toml` 声明 crate `crossterm` 0.29.0，库入口 `src/lib.rs`，default features 包含 bracketed-paste/events/windows/derive-more。这与根 version/feature 声明吻合。根另一个 `libc` 依赖本身不等于给 crossterm 启用其可选 libc feature；404已区分默认 Unix rustix/mio 路径与 use-dev-tty 等选择。这里不改变任何 feature、依赖、API 或锁文件。

根 `Cargo.lock` 中只读相关块给出了静态接线：zenpi 0.1.0 的 dependencies 包含 crossterm、ratatui；ratatui 0.30.2 包含 ratatui-crossterm；ratatui-crossterm 0.1.2 又包含 crossterm。该锁文件的 crossterm 0.29.0 块不带 registry source/checksum，与根本地 path patch 相符；ratatui 及其 backend 块则有 registry source/checksum。可据此识别当前锁图里的直接和间接引用，不能用锁文本证明本轮 Cargo 已成功解析、所有feature已运行或所有平台已验证。

`vendor/crossterm/Cargo.lock` 是子crate的另一份锁文件。根项目构建接入该crate，不能把这份 vendor 锁当成根依赖图整体替代，也不能把两份锁下的测试数合并成同一配置结果。404已明确 root/vendor 的 mio、rustix、signal-hook、signal-hook-mio 选择版本不同；405精确复用其相关范围，没有新执行任何构建。

404也区分 `Cargo.toml.orig` 作者清单与规范化 Cargo 输入、源码版本与VCS元数据、README旧版本/feature样例与实际清单、examples指南引用的缺失目录。上述差异在本层仍保留，不把依赖文件夹存在、上游仓库元数据或MIT许可证当成当前定制补丁的完整性/行为证明。

## 4. 已直接阅读的项目调用边界

本轮 `src/tui.rs` 只读下列5个明确范围，总270行：1–45、12100–12135、12940–12975、13117–13188、13245–13325。存储全文是为了字节身份与可移植复核，不表示完整阅读该大型文件；所截范围也不覆盖整套 TUI、整个编辑器状态机或所有错误分支。

导入块直接使用 crossterm 的 cursor Hide/Show、事件类型/启停鼠标与粘贴命令、execute、raw mode 与 alternate-screen 命令，并使用 Ratatui 的 `CrosstermBackend`。两个事件循环片段分别调用 `event::poll(wait)` 和 `event::read()`，然后把事件交给 `state.handle_event`，Resize 也在主机侧转为待处理状态。终端输入在 crate 中解析为事件，项目如何响应、何时调度绘制仍由调用者决定；这一结论来自具体调用点，不假定所有主机共享一套实现。

编辑器调用片段在挂起前传入 `event::reset_event_reader`，启动失败或会话返回时也把同一函数传入恢复步骤。`TerminalGuard::suspend_editor` 的完整函数（13274–13293）先 reset_reader，再 original.flush，切换active状态，发出离开屏幕/鼠标/粘贴/光标命令，再关闭 raw 和恢复原终端。`resume_editor` 的完整函数（13295–13323）先 reclaim，再 reset、flush、restore(false)、enable_raw，设active并发送进入屏幕等命令，最后 resize。

这说明项目自己编排 reader 缓冲清理、OS终端刷新和终端控制序列；它们是多个有顺序且可失败的动作。suspend 中 screen/raw/original 各自先求值，之后通过 `and` 组合结果，并非失败后所有后续副作用都不发生的事务。resume 用多个 `?` 顺序传播失败，已完成的动作也不会自动回滚。注释要求调用者独占事件所有权，但函数指针参数不会自动建立跨线程互斥或事件所有权证明。

既有402源码阅读已确认 reset 是内部 reader 状态操作，并有feature/平台/锁竞争边界；它不等同于 OS 输入flush、线程join、编辑器退出或持久化成功。405仅把该已读语义连接到当前 TUI 实际回调位置，不声称本轮验证了完整终端交接，也不把读取调用片段等同于通过真实PTY。

## 5. 阅读链、状态和源漂移

原404 ready整体按原 manifest `1290e7e4a02df8bbc0636b042ada3ee02cacb8bb71da51c4e5d04db8ddccf99e` 保存为 `prior404/`，原524 payload及嵌套403/402/401/400/099保持原样。404报告身份为21348 B、161行、SHA `14d0d361b432c5ac3f47fe75debde04c049b966dbf8f77bbe57825727a1e5260`。复用其理解不代表替主控接受404。

本轮精确复用42份完整context：404已有33份完整阅读链，加404新读9份完整文件。另复用其3份明确范围阅读（CHANGELOG与两锁），合计281行；这3份从未被标作完整阅读。全部42份全文及3份指定范围与本轮主库字节一致。`parse.rs` 仍与402的55081 B / `689b765d67be62703f7935abfa1475d98994987461e413836b54c7936966ce2b` 完整阅读身份相同，131私有候选没有变成主库事实。

新context仅2个路径：上述TUI范围270行；根锁4个package块及头部共67行，其中头部1–4与crossterm204–219共20行已有404精确阅读，ratatui909–921、ratatui-crossterm943–954、zenpi1710–1731共47行为本轮新增必要范围。因此本轮直接检查10块337行，其中真正新增范围8块317行，另2块20行为先前范围的复核。`new-context-read.json`、`read-blocks.json`分别保留路径、整文件身份、范围、字节offset和每块SHA。

mio 当前仍为25801 B /655行 / `41321e242e21fa85533dbae42314bcf80ab916e1d55e065d5ce98076002ac93f`，与099→404精确链一致。主库 `src/headless.rs` 当前仅做身份观察：370207 B / `bf252fec95a859b26f9aea402c918e9efbf6c2c55d3e24b403831f86d80ec06e`，相对先前521c…身份已漂移。本轮未读其新正文，不解释该变更、不给它追加完成声明，也不重封历史报告或复跑历史产物。

快照中402主控receipt存在，complete=true/manual_review=accepted，2619 B / `f3424d284ffff63c95874d8e43b6762ca609f82f9ad7eaebf740873f6685c6c1`；403、404、405 receipt均不存在。当前状态观察与404包封存时402尚未接受的历史快照并列保存，不能倒写旧状态。405唯一直接依赖仍是未接受的404；即使更深层099/400/401/402有真实验收，也不能越级闭合403/404/405。

## 6. 交付及验证限制

唯一可应用差异是新增本报告的 `report.patch`，回滚仅删除本报告，不撤回任何子项，不改产品、Cargo、claims、selector、蓝图或旧ready。目录理解依赖真实库存、明确的源码阅读和可追溯上下文；manifest/离线脚本只验证证据一致性，不代替人工语义验收。

本轮只运行新写的可移植只读 `verify.py` 一次，检查封包清单、原404完整身份、直属/冻结库存、阅读块、Cargo接入文本、402状态与403/404待审、唯一报告patch；实际输出与退出码外置保存。校验器没有subprocess、网络或写文件操作，不执行嵌套旧verify/runner。产品执行、Cargo、PTY、旧实验、预算、G-STAGE全部为0。

本候选完成的是 vendor 容器职责与唯一直接子目录接线的静态说明。依赖未接受、未读完整TUI/headless、未运行产品这些边界持续有效；405与root ZS1-091的主审状态保持未接受，封包交接后停止。


## Controller independent directory acceptance — 3.1.21

The controller independently read the complete10907-byte405 report, its full portable verifier and all10 specified current-context blocks. The source excerpts cover270 lines of TUI imports, both event-loop poll/read call sites, editor callbacks and complete suspend/resume methods. Root lock excerpts cover67 lines,20 already read for404 and47 new lines identifying ratatui, ratatui-crossterm and zenpi. This totals337 directly inspected lines with317 newly read lines; neither full TUI nor full root lock is claimed.42 complete contexts plus3 explicitly partial contexts reuse the controller's actual404/403/402/401/400 readings by exact byte identity.099's current655-line mio chain is separately verified.

The root independently enumerates vendor as zero regular files and one direct directory,crossterm, without a symlink or hidden extra entry. Its child's direct9file/4directory inventory and file hashes match404. There is no vendor-level Cargo manifest or root .cargo/config in the inspected project paths. This says nothing about uninspected user/global configuration. The frozen scope has no formal direct file and exactly404 as its child. The actual now-accepted404 receipt and all11 artifacts are checked; the original package's missing403/404 snapshot and initial absent worker/vendor read failure remain historical evidence. The copied545-payload package passed its new read-only verifier once from /tmp with external logs. That integrity result does not substitute for this semantic review.

Root Cargo's patch explicitly points crossterm to vendor/crossterm, not vendor itself or every descendant. The direct dependency leaves defaults enabled and adds bracketed-paste. Ratatui's crossterm backend creates a second reference to the same locked crate: zenpi→ratatui0.30.2→ratatui-crossterm0.1.2→crossterm0.29.0. The selected crossterm lock block has no registry source/checksum. These text connections are not a new Cargo resolution/build result. The vendor lock is a separate development graph and does not replace the host lock. Root's libc dependency does not by itself select crossterm's optional libc feature. Prior404's normalized/original manifest distinction and stale README/example paths remain qualified.

The new TUI excerpts concretely show imported cursor/event/raw/alternate-screen primitives and CrosstermBackend. Two host loops call poll then read and pass the event into TuiState; resize/render/project ownership stays in the host. These snippets do not establish that every host has identical behavior or complete the large TUI file's review. External-editor startup/resume paths explicitly pass reset_event_reader into TerminalGuard. Suspension resets the internal reader, flushes through the original terminal, sets active=false, and evaluates screen,raw andoriginal restore actions before combining their Results. Errors do not make these effects transactional. Resume reclaims, resets, flushes, restores, enables raw, sets active before output and resizes. Its successive question-mark returns can leave already completed effects; the active flag allows later leave handling but is not blanket rollback proof.

Event reader reset, OS input flush, raw state, output terminal modes, foreground process ownership and persisted drafts are separate operations. The function-pointer callback does not itself prove exclusive cross-thread ownership. The vendor container creates none of those owners and has no program entry point, session/WAL or provider lifecycle. Known parser/query/Drop limits stay with their independently reviewed child scopes; they are not fixed or runtime-tested by this report. No product/Cargo/PTY/budget or archived runner was executed for405. Current headlessbf252 observed by the worker is identity-only in its405 report; the root's separate shutdown-input product review must not be relabeled as vendor semantic evidence.

Accept405 only after independent404 acceptance. This closes the frozen vendor-container directory understanding, not091/root or every product feature depending on it. Whole TUI/headless, all third-party configurations, terminal-editor fault paths and131 remain independently open. The current d0cc production first-start1066.50ms still fails its1000ms gate, even though95 Rust and23 selected PTY checks passed in the separately published product integration. No result is retried, weakened or made a405 test. Rollback withdraws only405 report/receipts/state and preserves all child acceptances, product bytes, historical failures and user state.
