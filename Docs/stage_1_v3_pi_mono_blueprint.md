# zenpi Stage 1 v3 — pi-mono 差距执行 Blueprint

> 2026-09-13；用户已授权完整执行。保留现有BentoBox，强化顶部项目分页及Codex CLI交互体验。

```yaml
schema_version: execution-blueprint/stage1
blueprint_version: 3.1.22
revision_date: 2026-09-13
review_date: 2026-09-13
status: bootstrap-active
authoritative: true
predecessor: none
reference_audit: none
stable_id_pattern: '^ZS1-[0-9]{3}$'
status_values: '[ ]|[_]|[x]'
per_item_code_loc_cap: 5000
per_item_code_loc_rule: 'estimated_loc < 5000'
source_repo: /Users/mac/GitHub/pi-mono
source_revision: 23282f60782f02b9e22b787e4b22af441454fa16
source_revision_note: 'machine-switch: original frozen bbb61e34aaf231639fdaaad1adbd757947034eac is no longer reachable in origin; per-file declared hashes remain historical learn-report declarations and are re-verified for report consistency against the available checkout'
target_repo: /Users/mac/Github/zenpi
target_revision: dc836b6f0d5ee2a90570e4b86d4f877743fde1e6
target_baseline: current-worktree-with-user-changes
audit_mode: understand
product_modes: [tui, headless]
source_scope: explicit-file-list-in-section-3
worker_acceptance: self_test_only
master_acceptance: integrated-behavioral-evidence
```

### 源冻结重签声明（机器迁移重冻结，2026-09-17）

- 迁移前冻结的 pi-mono revision `bbb61e34aaf231639fdaaad1adbd757947034eac` 在 origin `weiyangzen/pi-mono` 已不可达（`git fetch <sha>` 返回 `not our ref`），本机亦无对象，无法恢复。
- 本次按当前可达 revision `23282f60782f02b9e22b787e4b22af441454fa16` 重新冻结：第 3 节源/目标行与第 8 节 Codex 参考行的 `SHA256`（含可用的字节数列）已按本机现存文件重算，作为本 run 的权威源指纹；第 36 行的 tracked-content 摘要同步重算。
- Codex 参考仓库按蓝图冻结 revision 从 `openai/codex` 恢复为 `b3b3d262787f4902a7449f17d793241a34d311ad`（`/Users/mac/GitHub/codex`）。
- 下列行的源路径在当前 revision 已不存在，无法回源重算，声明为历史报告一致性绑定，**缺源不构成 blocked**：`ZS1-012`、`ZS1-013`、`ZS1-016`、`ZS1-017`、`ZS1-024`、`ZS1-029`。
- 历史 learn 报告内部若仍声明旧 revision 的 hash，一律视为被本声明取代；worker 必须绑定本节表格指纹，语义差异记录为非阻塞备注，不得因此把整项判为 blocked。

## 1. 交付边界与证据基线

本阶段把 pi-mono 已有的若干运行行为补到 zenpi：对话输入的生效边界、工具并发与进度、语义压缩、会话分支、Chat 流式输出、模型能力与两种原生 provider、完整工具输出引用、搜索、标准技能/模板与扩展生命周期。外部 worker 结果的证据绑定是这些行为在 Blueprint 执行中的验收基础。目标是这些明确功能的真实入口可用；没有把整个 pi-mono 的全部产品、全部 provider 或旧 Rust 转换工程纳入本轮实现范围。

基线取实际源码，旧文档用于定位。pi-mono HEAD 为 `23282f60782f02b9e22b787e4b22af441454fa16`（机器迁移后原冻结 `bbb61e34aaf231639fdaaad1adbd757947034eac` 已不可达），检查时 clean；zenpi HEAD 为 `dc836b6f0d5ee2a90570e4b86d4f877743fde1e6`。逐文件比较必须读取这些未提交实现。不得 reset、stash、clean 或用 HEAD 覆盖现有工作。

当前 tracked-content 摘要：pi-mono `5ddc387f8afb35ad215c007115eaa66f029509042a11af0b96cf0df72dd92643`；zenpi `d3f5063d19d9947436d9a8ba9b55c2f8b72184ea68b4d40acd5e524c8b76cf8b`（机器迁移后重算）。算法为按字节排序的 git tracked paths，累计 `path + NUL + SHA256(file bytes)` 后再 SHA256；缺失文件用 MISSING。它标识审计输入，不证明功能一致。

原 learn 工程为 `/Users/mac/GitHub/learn_pi_mono`。其 `source_manifest.tsv` 的 source_id/path/hash 可复用为导航。本文件列出的源文件 hash 已逐个与当前源码核对。旧 `[x]` 不继承为本阶段的验收状态：`rust/packages/agent/src/agent.ts.rs` 的 MissingRuntimeAdapter 仍会拒绝调用；`rust/packages/ai/test/bedrock-convert-messages.test.ts.rs` 的 run_local_tests 返回测试数量和零失败，不能代表源测试真的执行。原报告、状态及转换代码保留为历史，本阶段不修改。

蓝图依据：`execution-cron-builder`、`learn-cron-builder` 与逐文件/目录覆盖合同。本机代码转换的正式模式是 `learn_mode=transform`，不是 migrate/transfer。本阶段比较研究使用 understand；将来真正移植时另外冻结 transform 的 target_contract、mapping_policy、validation_policy 和 traceability_index，不能用研究报告抵充代码功能。

### Rust 规范对齐：`weiyangzen/codex-rust-deslop`

本项目的 Rust 开发规范在原则层面采用了
[`weiyangzen/codex-rust-deslop`](https://github.com/weiyangzen/codex-rust-deslop)
的报告，但当前 Stage 1 **只能判定为“规范已对齐、实现证据部分对齐”**，不能宣称全仓或本阶段全部功能已经通过 deslop。冻结的参考基线是该仓库 `main` 在 2026-09-13 验证到的
`648ca87e2add56a12081221c3e6f51e29fe04fbd`；报告文件为
`codex-0.148-rust-refactor-report.zh-CN.md`，SHA-256 为
`e92528d14bd26369ac7b7fbfc1edc87fa10fe3cbd8f22876a25d4c3cf5261be2`。
本项目的解释和通用门禁分别记录在
[`Docs/research/codex-rust-deslop.md`](/Users/mac/Github/zenpi/Docs/research/codex-rust-deslop.md)
和
[`Docs/quality/rust-deslop-gate.md`](/Users/mac/Github/zenpi/Docs/quality/rust-deslop-gate.md)。

| 上游原则 | Stage 1 的强制落点 | 当前判定 |
|---|---|---|
| 一个概念只有一个 owner、入口、状态机和安全失败路径；迁移调用者和测试后再删旧路径 | 每个产品项必须声明 `owner scope`、`owned paths`、状态/错误/副作用边界；G-FILE/G-DIR 反向检查 owner 闭包 | 规范已纳入；逐项证据未全部闭合 |
| 用类型表达合法状态和拒绝原因，优先 move/borrow，不把 `Clone`、`Arc`、`Mutex` 当借用错误的万能修复 | 使用 typed command/state/error、bounded record 和有 owner 的队列/资源；新增 clone、共享锁或大值复制必须说明生命周期、所有权和成本 | 规范已纳入；需在实现项 review 中证明 |
| 先守住失败语义，再做 `Cow`、借用 JSON、缓存和分配优化 | 优化不得改变非法输入、旧快照、并发时序或失败原子性；涉及 allocation/lock/clone 的改动必须有场景测试或测量依据 | 规范已纳入；不能由 fmt/Clippy 单独证明 |
| 兼容转换只放在持久化/协议边界 | 旧 journal、JSONL、provider wire 和 adapter 的迁移/兼容必须有显式 fixture、round-trip 或拒绝测试；领域类型不因兼容而全面变弱 | 规范已纳入；按相关 ZS1 项验收 |
| 安全、进程、终端和资源清理在不确定时 fail closed，并明确 wait/reap/close/flush owner | G-CODE/G-HOST 必须覆盖拒绝无副作用、取消、子进程回收、terminal restore、资源配额和清理失败可见性 | 规范已纳入；真实 host 证据仍以各项状态为准 |
| 失败优先测试，而非只看成功结果或一次静态扫描 | 必须覆盖 interleaving、malformed/old format、权限/容量、取消、重启、身份错配和资源边界；`[x]` 只能由 master 的集成行为证据产生 | 当前阶段仍有未完成项 |

因此，`cargo fmt`、严格 Clippy、`cargo test` 或 `unwrap/unsafe` 文本扫描通过，只能证明相应的机械门禁；它们不能替代 owner、兼容性、并发线性化、fail-closed、副作用和生命周期的人工/真实入口审查。Stage 1 中已有的 `[ ]`、`[_]`、标注“尚未执行”的真实验证，以及只在 2026-09-04 旧工作树记录的门禁结果，均不得继承为当前工作树的 deslop `[x]`。任何涉及运行时、协议、持久化、PTY、子进程或资源所有权的变更，都必须重新生成对应的 G-CODE/G-HOST 证据；只要其中一类证据缺失，本 Blueprint 的结论就是“部分对齐”，而不是“完全对齐”。

### 权威切换

用户已明确开始本阶段，本文件现在是本run唯一要求，`authoritative:true`。ZS1-001由主控bootstrap建立唯一 requirement selector，记录本文件 digest、版本、激活者与 baseline；同一 run 的 worker、master、todo 和新阶段 validator 只能读取 selector 指向的这一份清单。selector 必须包含 run_id 与 blueprint digest；新 checker 每次读取并核对它。旧 G-BASE 仅作历史 snapshot 一致性检查，不再决定新 run 的 claim 或验收权威。通过迁移检验后，主控才切换活动指针。旧清单只读归档；既有代码和历史 `[x]` 不自动变成本阶段 `[x]`。ZS1-001 由 master 按明确选定的本文件执行一次 bootstrap，不走普通 worker claim；它先建立并验证 selector，再开放 worker。其它项在 selector 尚未建立或同时存在两个 active requirement 时，不允许 claim。

所有 claim/lease 绑定规范化的 `requirement_digest`：覆盖版本、冻结scope/source hashes、行为要求以及各项ID/依赖/owned paths/validators/rollback/LOC，只排除checkbox状态和明确标识的activation/receipt运行记录。源要求、owned paths或验收判据一旦变更必须生成新requirement版本。另存全文 `snapshot_sha256`，每次状态/receipt写入与selector中的snapshot引用原子更新；正常 `[ ] → [_] → [x]` 不得让相同要求的已有claim失效。ZS1-001的反例测试要证明状态变更不改变requirement_digest，而语义变更必须改变它。

## 2. 当前差距与既有计划的边界

| Gap | 源码行为与 zenpi 当前事实 | Stage 1 增量 | 既有 v2 关联 |
|---|---|---|---|
| G01 | agent.ts / agent-loop.ts 分别处理 steer 和 followUp；zenpi runtime 已有有界 FIFO 作业队列，protocol 仅 StartOrSteer/StartIfIdle/Steer | 同一对话中独立的 follow-up 输入类型、one-at-a-time/all 策略、工具批次后的安全生效点、按 ID 查看/修改/取消 | V2-109、V2-116；复用队列/邮箱，避免新 scheduler |
| G02 | pi 按工具策略串行或并行，进度先于终态；zenpi core 当前 for call 同步处理，Tool 返回最终 Value | 有界工具执行器、sequential barrier、source-order 结果持久化、late-update 拒绝 | V2-003、V2-103；现有 canonical events 继续使用 |
| G03 | pi 有摘要生成、完整 turn 切点、文件操作保留；zenpi context 只生成 role counts + SHA，且恢复校验依赖该格式 | 有内容的语义摘要、版本化 checkpoint、正确的保留尾部和未完成工作、预算与失败原子性 | V2-105；原 journal 恢复能力保留 |
| G04 | pi session-manager 有 parent graph 和 active leaf；zenpi fork_to 复制全部 journal，Turn.parent_id 表示 active-turn 关系 | 同 session A→B/A→C 树、leaf 选择、分支上下文、选中分支 fork | V2-108、V2-115、V2-117；不重建整个会话系统 |
| G05 | pi Chat 真正 SSE、按模型能力及多个原生 wire；zenpi Chat stream:false，能力按 Chat/Responses 常量，/models 是 profile list | Chat SSE；模型能力 registry；Anthropic Messages 与 Gemini 原生请求/流解析 | V2-102、V2-109、V2-002、V2-303；保留 OpenAI Responses 已有输入附件/结构化输出及轻量依赖门禁 |
| G06 | pi OutputAccumulator 有增量尾部和 fullOutputPath；zenpi read_limited_stream 丢弃超过内存上限的输出 | 有配额的完整原始输出 artifact、UTF-8 安全视图、哈希/字节范围/完整性标记和工具进度 | V2-103、V2-304；截断仍可见，磁盘配额同样有界 |
| G07 | pi grep/find 提供 regex/glob/ignore/上下文，并调用实际 rg/fd；zenpi search_text 主要为 literal contains + 有界遍历 | 保持 literal 默认，新增显式 regex/glob/ignore/context 语义与工具能力探测 | 复用现有 tools、安全路径及取消规则 |
| G08 | pi SKILL.md 元数据及按需展开、参数模板、reload；zenpi 仅直接子目录 skill.toml 并一次拼接所有正文 | 标准技能发现、来源/冲突规则、显式调用与渐进披露、非递归模板展开与原子 reload | V2-005；保留现有 TOML、allowlist、close hooks |
| G09 | pi 扩展有 input/context/tool/session lifecycle；zenpi executable extension 主要 tools/call，skill hooks 是固定前缀 | 版本化 subprocess hooks、预算、generation 撤销、失败隔离、关闭回收 | 复用现有 extension 协议和 host 的 policy/approval；不嵌入 JS runtime |
| G10 | pi 的 tool 事件、原始输出及 session 投影可追溯至实际执行；zenpi import_external_manifest 已验证 claim/digest 并导入有界 evidence 字符串，但成功字段仍由提交方宣称 | 把外部结果绑定实际产物、工作树 revision、host 执行的 validators、资源 lease 和主控复验 | V2-119、V2-201、V2-112～114；已有 manifest 导入不能写成不存在 |

原生 OAuth、其它 provider、Web UI/MOM、图像工具、完整 Pi 扩展示例生态及桌面 GUI 不在这一版实现清单内。两种新增 native wire 先使用显式的既有 secret handle/API key 机制。源代码受启发的策略变更必须写清行为差异，例如 zenpi 仍需在扩展修改工具参数后重新执行路径、schema、approval 和 immutable policy 检查。

## 3. 冻结的逐文件比较范围

下表是本轮差距定位的证据索引，同时定义待执行的完整复核子集。行号是定位入口，不代表全文件已验收。每个文件都有第 5 节的独立清单项。没有在表内的源码只可作为 context-only 阅读，不能计入完成率；若它的行为成为新的交付义务，先由 master 扩版 manifest 与本清单，再独立处理。pi子集21文件；目标owner增为27文件；新增Codex参考10文件另列独立manifest。每个子集均逐文件/逐目录完成，不宣称全仓覆盖。

### pi-mono 源文件（source_id 继承旧 manifest；状态重新从 [ ] 开始）

| Item / source_id | 源文件 | 行/函数定位 | 独立复核对象 → 目标 owner | SHA256 |
|---|---|---|---|---|
| ZS1-010 / SRC-0117 | [packages/agent/src/agent-loop.ts](/Users/mac/GitHub/pi-mono/packages/agent/src/agent-loop.ts) | 168–269，420–547 | steer/follow-up 边界、prepareNextTurn、串/并行工具及源序提交 → src/core.rs:1619; src/runtime.rs:566（G01/G02） | `ca5dc8426ddba23fe921a88e6c7f7f292a9b62511519056f1a778e5f1cfd8221` |
| ZS1-011 / SRC-0118 | [packages/agent/src/agent.ts](/Users/mac/GitHub/pi-mono/packages/agent/src/agent.ts) | steer/followUp 队列接口 | 双队列及按条/全量消费的入口 → src/runtime.rs:108; src/protocol.rs:46（G01） | `0660461cce0cfaffdcdf873bb3f6ab667552ae92d86d8a8f1d2145413037b4d8` |
| ZS1-012 / SRC-0121 | [packages/agent/src/harness/compaction/compaction.ts](/Users/mac/GitHub/pi-mono/packages/agent/src/harness/compaction/compaction.ts) | 311、634、800–811 | 语义摘要、完整 turn 切点及文件操作保留 → src/context.rs:71（G03） | `6e7aec0d27cb566f8f85f7dd13850eda98f5ddf7e78f9a919fb3dd03c3ea3a8a` |
| ZS1-013 / SRC-0156 | [packages/agent/src/harness/session/context.ts](/Users/mac/GitHub/pi-mono/packages/agent/src/harness/session/context.ts) | 10–35 | 最新 compaction + retainedTail；剔除 error/aborted/deferred assistant → src/context.rs:172; src/session.rs（G03） | `409078e31155a77bbfa1fff20ab81d55370fa379ad95c12933eecbf832efeaf1` |
| ZS1-014 / SRC-0209 | [packages/agent/test/agent-loop.test.ts](/Users/mac/GitHub/pi-mono/packages/agent/test/agent-loop.test.ts) | 681、1031 | 整批工具结束后收输入、长 prepare 的消息快照测试 → tests/runtime.rs; tests/core_session.rs（G01/G02） | `3bbcddc3a92c45c2ca66becd84dc26e85fa5b22005be5862666d6c114ee85f2e` |
| ZS1-015 / SRC-0440 | [packages/ai/src/types.ts](/Users/mac/GitHub/pi-mono/packages/ai/src/types.ts) | 35、851–859 | provider/model 能力、reasoning、输入类型及 contextWindow → src/backend.rs:121; src/config.rs（G05） | `ddc294dc3ec0f84165e4f3b389dac59fd736cde8c496c4d84dee86ef017551f4` |
| ZS1-016 / SRC-0286 | [packages/ai/src/api/anthropic-messages.ts](/Users/mac/GitHub/pi-mono/packages/ai/src/api/anthropic-messages.ts) | streamAnthropic 对应实际请求/事件分支 | 原生 Anthropic wire 协议映射 → src/backend.rs（G05） | `8e105cc5b2dd304547ed613b70de4740c4db93cee830dc452c2de945052b12b7` |
| ZS1-017 / SRC-0296 | [packages/ai/src/api/google-generative-ai.ts](/Users/mac/GitHub/pi-mono/packages/ai/src/api/google-generative-ai.ts) | 110–180、264、312 | Gemini thinking、tool streaming、签名及 abort → src/backend.rs（G05） | `1395c3f2a90bf701b8b5fc1b6f7de8d323193ed339e07a60630900099a543757` |
| ZS1-018 / SRC-0936 | [packages/coding-agent/src/core/session-manager.ts](/Users/mac/GitHub/pi-mono/packages/coding-agent/src/core/session-manager.ts) | 461、1324、1374、1395 | 父 entry 图、active leaf 路径投影及带摘要分支 → src/session.rs:919（G04） | `404b056c6b60125470e2e3d94b310f99419a93b52739a848b208256fc307dc73` |
| ZS1-019 / SRC-0939 | [packages/coding-agent/src/core/skills.ts](/Users/mac/GitHub/pi-mono/packages/coding-agent/src/core/skills.ts) | 164–168、304–355、409–453 | SKILL.md 元数据、递归发现、索引和按需正文 → src/skills.rs:181; src/core.rs:3794（G08） | `ba3a8fb8580502f5f352e8651ff0acd1e3c83b8ba252f4619663cd42fdc305e4` |
| ZS1-020 / SRC-0925 | [packages/coding-agent/src/core/prompt-templates.ts](/Users/mac/GitHub/pi-mono/packages/coding-agent/src/core/prompt-templates.ts) | 58–74、194、269 | 非递归参数展开、模板发现和命令调用 → src/skills.rs; src/slash.rs（G08） | `a6e7bfd0a5e68f7ecaad523841c9923ddd200e207b8048805d43fa1475985368` |
| ZS1-021 / SRC-0931 | [packages/coding-agent/src/core/resource-loader.ts](/Users/mac/GitHub/pi-mono/packages/coding-agent/src/core/resource-loader.ts) | 388、677、700 | 资源 reload，skills 与 templates 分开加载 → src/skills.rs; src/extensions.rs（G08/G09） | `9acb127ae635c05235df4ab19e149a3a721b6e766a6ddfc03a26aff84d0a1f4e` |
| ZS1-022 / SRC-0909 | [packages/coding-agent/src/core/extensions/types.ts](/Users/mac/GitHub/pi-mono/packages/coding-agent/src/core/extensions/types.ts) | 1257 起的生命周期类型 | context/input/session/tool 生命周期的类型化契约 → src/extensions.rs:25; src/skills.rs:26（G09） | `12ab5bd74a7c91b55a321402a43d859ed240f27fd653434fef361ae97cbf0bd5` |
| ZS1-023 / SRC-0908 | [packages/coding-agent/src/core/extensions/runner.ts](/Users/mac/GitHub/pi-mono/packages/coding-agent/src/core/extensions/runner.ts) | 事件 dispatch 与 handler 路径 | 扩展运行、事件合并与错误处理 → src/extensions.rs:237（G09） | `5f13ad4e38dbd0d10fab5216710aff1d8ab8b2e75d9209bd1b1be0b89d22950b` |
| ZS1-024 / SRC-0953 | [packages/coding-agent/src/core/tools/output-accumulator.ts](/Users/mac/GitHub/pi-mono/packages/coding-agent/src/core/tools/output-accumulator.ts) | 35、64、91 | 有界尾部、完整输出 spill 及 snapshot 引用 → src/tools.rs:2376（G06） | `c601ddd8e10934be6f3db30696eacba7b9544f2dd1648a5ec3c9935f0d4625c3` |
| ZS1-025 / SRC-0945 | [packages/coding-agent/src/core/tools/bash.ts](/Users/mac/GitHub/pi-mono/packages/coding-agent/src/core/tools/bash.ts) | 269、321 | shell 输出更新与 fullOutputPath 的真实工具线路 → src/tools.rs:1855; src/tools.rs:1952（G06） | `27d966667a71c95b2c8735a403de5e786fc448c2af3d0302302af7336b95b913` |
| ZS1-026 / SRC-0950 | [packages/coding-agent/src/core/tools/grep.ts](/Users/mac/GitHub/pi-mono/packages/coding-agent/src/core/tools/grep.ts) | 24、36、119、165、168 | 正则、glob、ignore、实际 rg 调用 → src/tools.rs:1410; src/tools.rs:1529（G07） | `ab1f9f5c3ad134164c6d374140d6ccbf6019cae92e1a946d9bb6288d6ae52af1` |
| ZS1-027 / SRC-0949 | [packages/coding-agent/src/core/tools/find.ts](/Users/mac/GitHub/pi-mono/packages/coding-agent/src/core/tools/find.ts) | 35、172、201、216 | glob/ignore 与实际 fd 调用；不采用61行的 custom-op 占位实现 → src/tools.rs:1410（G07） | `08be04c6e9b5725eeb9a93d7f0c2c5ae2e85596c948437a14b097dff4b876dec` |
| ZS1-028 / SRC-0884 | [packages/coding-agent/src/core/agent-session.ts](/Users/mac/GitHub/pi-mono/packages/coding-agent/src/core/agent-session.ts) | 877、1362 | dispose 撤销回调、skill 命令展开的 session 入口 → src/core.rs; src/skills.rs; src/extensions.rs（G08/G09） | `b42bb063f411835fed213dd829fb9464f40bc5ba6985767feb662548ca8bf485` |
| ZS1-029 / SRC-0306 | [packages/ai/src/api/openai-completions.ts](/Users/mac/GitHub/pi-mono/packages/ai/src/api/openai-completions.ts) | 311、448、622、652、691、809 | Chat SSE 文本/reasoning/工具增量及 terminal finish reason → src/backend.rs:877; src/backend.rs:1009（G05） | `5874bf0b121db117bd4eba8fffa750252f5f7617f38306c6eb1f746575896319` |
| ZS1-030 / SRC-0917 | [packages/coding-agent/src/core/model-registry.ts](/Users/mac/GitHub/pi-mono/packages/coding-agent/src/core/model-registry.ts) | 50、58、66 | 模型索引、lookup 与 request auth 解析 → src/config.rs:1128; src/backend.rs:121（G05） | `17ae1e49f351465f8de92bad05c23e829c71b1ce7182f30599399fb44fc63305` |

### zenpi 目标 owner 文件（独立命名空间）

| Item | 目标路径 | 当前字节数 | 当前 worktree SHA256 |
|---|---|---:|---|
| ZS1-070 | [src/core.rs](/Users/mac/Github/zenpi/src/core.rs) | 279620 | `0d4d0d1aef706528fe1a91ec9846ab4ea365c61e08c67d23a6a794a26c49916e` |
| ZS1-071 | [src/runtime.rs](/Users/mac/Github/zenpi/src/runtime.rs) | 33333 | `7a42c428083f8f3edb0019311296b479004ff562b548d7cdc504729f7cbd1f21` |
| ZS1-072 | [src/protocol.rs](/Users/mac/Github/zenpi/src/protocol.rs) | 45517 | `90a5644d5a818a8051172b42b302614fb3263ebac737ce9dbf4a842791370d27` |
| ZS1-073 | [src/context.rs](/Users/mac/Github/zenpi/src/context.rs) | 30066 | `fc1138b9d5b3696a10135e70210c9aeb01ecea942dc8c0f19d6885607a835997` |
| ZS1-074 | [src/session.rs](/Users/mac/Github/zenpi/src/session.rs) | 176828 | `8fb3ff2c387b13d34d8bbe4533bb2b0a81b5e546f5e03216b74b5cf2c61c9a2b` |
| ZS1-075 | [src/backend.rs](/Users/mac/Github/zenpi/src/backend.rs) | 99056 | `0eae8058d36ef3507b2e1567f5d663843831a72ed73e506f33ec9ea18b034a43` |
| ZS1-076 | [src/config.rs](/Users/mac/Github/zenpi/src/config.rs) | 77782 | `f5691bd3818dcaf1481a88b63cc4f3874bc6b007234059e26124b74046325d12` |
| ZS1-077 | [src/tools.rs](/Users/mac/Github/zenpi/src/tools.rs) | 119778 | `c3149b926933e5800cd41a2e8d3fcf6f109f8a455f2ef6ed8e2d48d696dbe33c` |
| ZS1-078 | [src/skills.rs](/Users/mac/Github/zenpi/src/skills.rs) | 40533 | `a6d0a2f671febdef2e631ca53d2788ab869ab7b86db6c0cb1feaf24d128f83e8` |
| ZS1-079 | [src/slash.rs](/Users/mac/Github/zenpi/src/slash.rs) | 81726 | `d0702d86e3cf8087dbc09a3fe68c9067dd6bed9f37f499abcdb7e0131927a1c9` |
| ZS1-080 | [src/extensions.rs](/Users/mac/Github/zenpi/src/extensions.rs) | 20742 | `38a242eff3ec911f4560e3dea3868b742d511205ff0ccab1b622962b41e420ed` |
| ZS1-081 | [src/domain_execution.rs](/Users/mac/Github/zenpi/src/domain_execution.rs) | 112548 | `2f5ee732fdf80b99af0ab973eb4594b2369786ae4acc6a86768fc3ba49be890f` |
| ZS1-082 | [src/domains.rs](/Users/mac/Github/zenpi/src/domains.rs) | 24622 | `c11e10d02a21ed3412e731acd269080053b18f649935bb1f9486b128ee7c79ac` |
| ZS1-083 | [src/governance.rs](/Users/mac/Github/zenpi/src/governance.rs) | 44516 | `4d80ffb9f29bb987e488bad93bc35546a6994065e793bd645fc3d73802930552` |
| ZS1-084 | [src/headless.rs](/Users/mac/Github/zenpi/src/headless.rs) | 371210 | `c2aefb53e5bc342af94eab463445373a83fbca172aa235246bb0cd9aa64f65bb` |
| ZS1-085 | [src/tui.rs](/Users/mac/Github/zenpi/src/tui.rs) | 681440 | `83f5aa5b91f83cdcde80cc43936baafe8c681eb3f885b4fe9b213affa0217681` |
| ZS1-086 | [src/view_model.rs](/Users/mac/Github/zenpi/src/view_model.rs) | 42009 | `094833f051ab66d30ce3f6cc74fcc0334ed938e79f5fd8e9269341b62bed08fe` |
| ZS1-087 | [src/slash_actions.rs](/Users/mac/Github/zenpi/src/slash_actions.rs) | 76441 | `413a1ddba252ef2d435459af257cefa0cf45ef4c0b6ff9007b0b1454d69a8e9d` |
| ZS1-088 | [src/lib.rs](/Users/mac/Github/zenpi/src/lib.rs) | 879 | `d93926734119ba3a5a0af16a3bd032548dcfecaaa5aeb36dca69f57065dbbc82` |
| ZS1-089 | [.github/workflows/ci.yml](/Users/mac/Github/zenpi/.github/workflows/ci.yml) | 3973 | `44c1d0964cd0c3504f028f3fa2dbcac89c8063ca895d039eba729dd64bc263c9` |
| ZS1-094 | [src/layout.rs](/Users/mac/Github/zenpi/src/layout.rs) | 54724 | `f2f6883b429e012d017bb208b03ecd35a02f9ddb186989d73037c56c07d94af8` |
| ZS1-095 | [src/approval.rs](/Users/mac/Github/zenpi/src/approval.rs) | 29396 | `5488ad365670e515eba5e7d82a91dbf935be854b848c20fd99725f102e1f185f` |
| ZS1-096 | [src/render.rs](/Users/mac/Github/zenpi/src/render.rs) | 61553 | `b98356df9020c5e6c8ec712e9c2c673265c1392cc0cee0c7d5576fc11a8d5a3a` |
| ZS1-097 | [Cargo.toml](/Users/mac/Github/zenpi/Cargo.toml) | 1228 | `d5fafd029ef77454e178222c2b62530d28859374f72a2ce6bd972a465bed5884` |
| ZS1-098 | [Cargo.lock](/Users/mac/Github/zenpi/Cargo.lock) | 46021 | `7311d1133e06e428707d9aa10c95c6a0c1efa983865472f6b5e24b640f4b1348` |
| ZS1-099 | [vendor/crossterm/src/event/source/unix/mio.rs](/Users/mac/Github/zenpi/vendor/crossterm/src/event/source/unix/mio.rs) | 25801 | `41321e242e21fa85533dbae42314bcf80ab916e1d55e065d5ce98076002ac93f` |
| ZS1-133 | [tests/headless_project_workspace.rs](/Users/mac/Github/zenpi/tests/headless_project_workspace.rs) | 39333 | `8375f8bab5042cd3a49a177fb5d933d03eb3eb3a615d40c22be45c28f023d313` |

源树最终产物：`Docs/learn/stage1_pi_mono/files/<source_path>_learn.md` 及 `Docs/learn/stage1_pi_mono/<source_dir>/current_folder_learn.md`。目标树最终产物：`Docs/learn/stage1_pi_mono/targets/zenpi/files/<target_path>_learn.md` 及 `Docs/learn/stage1_pi_mono/targets/zenpi/<target_dir>/current_folder_learn.md`。两个根目录独立验收，不能混树或复用同一个目录报告。

每个 scope 用独立 manifest/index：源树 `source_manifest.tsv`、`file_learn_index.tsv`、`folder_learn_index.tsv`；目标树在 `targets/zenpi/` 下使用同名文件。源 manifest 保留 source_id/hash；目标 manifest 使用本清单 item_id 作 target-review-id。manifest 恰好覆盖表中路径，记录字节数、hash、artifact、对应 checklist ID 和状态。对两个清单逐行核对，严禁从旧 `[x]` 自动盖章。

超过 256 KiB 的输入按不超过 256 KiB 的连续 byte ranges 建 `chunk_manifest.tsv`。当前 headless.rs / tui.rs 各须至少两块，所有块按顺序逐个复核，并合并为该文件唯一最终报告；用 byte-range union 检查无重叠、无遗漏，不能拿 chunk 报告代替 per-file 报告。每个目录都要独立整合，依赖直属文件和直接子目录；包括全部祖先及根目录。分区、批次和多 agent 只安排工作顺序，不能以一个主题总结跳过其中的文件。

## 4. 执行与验收协议

状态仅 `[ ] → [_] → [x]`：worker 逐项 claim、完整执行本项、保存证据后只能写 `[_]`；master 在 integration frontier 复查源码与行为、集成并重跑验收后写 `[x]`。worker按DAG顺序在各自worktree准备候选，依赖未集成的产物保持provisional；master仍只在依赖全 `[x]` 且冲突解决后接受；`[_]` 永远算未完成。claim frontier 与 integration frontier 分开记录。每日 `todos_YYYYMMDD.md` 由本清单生成，只选可领取项，不建立第二份状态真相。

worker上限固定为3，不含主控，不得创建subagent或更多任务。共享文件可在隔离worktree准备provisional差异；主控集成按路径lease串行，禁止多个worker直接覆盖主checkout。src/core.rs、src/backend.rs、src/tools.rs、src/session.rs 等共享文件按路径 lease 集成串行。每 worker 在保留脏基线的隔离工作树工作，记录输入 hash；主控按项集成。`Estimated LOC < 5000` 为每项实现+测试增量预测，文档项为 0；接近上限时先拆项更新 DAG。资源上限来自 operator/ResourceLease；不得通过新线程、嵌套代理或后台服务绕过预算。送审 diff 默认不超过 256 KiB。

`G-FILE`：本文件全部内容的复核记录，具体函数/接口/输入输出/状态机/错误/副作用/取消/恢复行为；每个功能结论对应源测试或可运行行为判据、zenpi owner 与已有实现、缺口/有意差异/不适用理由。目标 owner 项反向核对本文件全部分支的相关性；无关部分也给范围排除理由，不能遗漏。哈希、符号计数、纯摘要、Echo/MissingAdapter 或仅 compile 不能满足此门禁。结构 checker 只能验证 manifest/链接/范围，master 必须逐文件审阅语义证据。

`G-DIR`：直属文件与直接子目录均已 `[x]` 后，单独写本目录在冻结子集中的调用关系、数据所有权、错误/取消/持久化路径、跨文件不变量及目标映射。记录 in-scope / context-only 清单，检查根到叶的闭包，无跳目录、重复接受或遗漏。目录报告不得宣称本目录未纳入文件已验收。

`G-CODE`：正向、反向、取消、适用的重启场景通过，并走实际 owner。证据位于 `Docs/quality/stage1/<item-id>/<attempt-id>/`，包含 item/digest/source IDs、baseline 与 integrated revision、目标二进制 hash、固定输入、命令 argv/cwd/受控 env 名称、开始结束时间、退出码、stdout/stderr artifact 的 bytes/hash、预期与实际判据、资源峰值和回滚。重启/取消等不适用项必须逐项解释。外部 worker 的 command 字符串不能直接当作可信验收命令执行；master 运行本 Blueprint 预定的 validator。

`G-HOST`：生产构建的真实 headless JSONL 与 TUI PTY 入口，连接本地可控 HTTP/子进程 fixtures；验证入口→运行→用户可见结果→durable evidence 的完整链路。fixture 可模拟上游网络，但 host 使用真实 provider/parser/tool/session owner，不能启用 Echo 等假执行路径。真实 provider 账号只用于另行配置的可选网络 smoke，本阶段的确定性门禁不能依赖外部账号。

proposed 文件/脚本当前不存在：ZS1-001 创建 validator；产品项创建相应 integration tests；ZS1-117 创建 Stage 1 host smoke。下文命令是执行后的验收义务，不是在本次文档交付时已经通过的测试。validator 对缺少脚本、缺少 tests、零测试匹配、证据缺失或只满足 metadata 的行为必须报失败。

### 验收命令目录

- G-BASE（既有）：`python3 tools/validate_blueprint.py`；`python3 tools/validate_blueprint_v2.py`。只验证旧蓝图结构，不能冒称验证本文件的行为。
- G-STAGE（由 ZS1-001 新建）：`python3 tools/validate_stage1_blueprint.py --blueprint Docs/stage_1_v3_pi_mono_blueprint.md --evidence-root Docs/learn/stage1_pi_mono`。支持 `--item ID`；默认草稿检查路径/状态/DAG/hash，文件验收还检查对应产物与主控 review receipt。必须分别报告 structural 与 semantic/manual gates。
- G-RUST（代码变更后）：`cargo fmt --check`；`cargo check --locked`；`cargo test --locked`。各项再执行下列指定 test target；测试数量必须非零并有场景清单。全量仅在最终集成或跨模块变更需要时运行。
- G-PROD（最终生产构建）：`cargo build --release --locked`；新建 smoke 命令 `python3 tools/stage1_host_smoke.py --binary target/release/zenpi --report-dir Docs/quality/stage1/ZS1-117`。必须检查 production feature set；命令/schema 由 ZS1-117 固定并加入 CI。

## 5. 逐项执行清单

### L0 — 唯一要求、输入与验证器

- [x] **ZS1-001** — 冻结双树 manifest、激活选择器和执行型 checker；layer `L0` | Depends: — | Owner scope: 只创建阶段执行基础与审计路径 | Owned paths: `Docs/stage_1_v3_pi_mono_blueprint.md`, `tools/validate_stage1_blueprint.py`, `tools/test_validate_stage1_blueprint.py`, `Docs/execution/active_requirement.json`, `Docs/learn/stage1_pi_mono/source_manifest.tsv`, `Docs/learn/stage1_pi_mono/file_learn_index.tsv`, `Docs/learn/stage1_pi_mono/folder_learn_index.tsv`, `Docs/learn/stage1_pi_mono/targets/zenpi/source_manifest.tsv`, `Docs/learn/stage1_pi_mono/targets/zenpi/file_learn_index.tsv`, `Docs/learn/stage1_pi_mono/targets/zenpi/folder_learn_index.tsv`, `Docs/learn/stage1_pi_mono/chunk_manifest.tsv`, `Docs/learn/stage1_pi_mono/route_decision.md`, `Docs/learn/stage1_pi_mono/todos_YYYYMMDD.md` | Validators: G-STAGE；python3 tools/test_validate_stage1_blueprint.py；G-BASE；伪造x/漏文件/跳目录/双authority/错误hash/缺证据均拒绝 | Rollback: 撤回本项新文件及 selector 切换，恢复原活动指针，不改用户基线 | Estimate: 约2天；文档和manifest不算实现LOC | Estimated LOC: 1200

### L1 — 源文件，每行一个文件

- [x] **ZS1-010** — 逐文件复核 SRC-0117 packages/agent/src/agent-loop.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/agent/src/agent-loop.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-010 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-011** — 逐文件复核 SRC-0118 packages/agent/src/agent.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/agent/src/agent.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-011 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-012** — 逐文件复核 SRC-0121 packages/agent/src/harness/compaction/compaction.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/agent/src/harness/compaction/compaction.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-012 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-013** — 逐文件复核 SRC-0156 packages/agent/src/harness/session/context.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/agent/src/harness/session/context.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-013 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-014** — 逐文件复核 SRC-0209 packages/agent/test/agent-loop.test.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/agent/test/agent-loop.test.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-014 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-015** — 逐文件复核 SRC-0440 packages/ai/src/types.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/ai/src/types.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-015 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-016** — 逐文件复核 SRC-0286 packages/ai/src/api/anthropic-messages.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/ai/src/api/anthropic-messages.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-016 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-017** — 逐文件复核 SRC-0296 packages/ai/src/api/google-generative-ai.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/ai/src/api/google-generative-ai.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-017 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-018** — 逐文件复核 SRC-0936 packages/coding-agent/src/core/session-manager.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/coding-agent/src/core/session-manager.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-018 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-019** — 逐文件复核 SRC-0939 packages/coding-agent/src/core/skills.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/coding-agent/src/core/skills.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-019 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-020** — 逐文件复核 SRC-0925 packages/coding-agent/src/core/prompt-templates.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/coding-agent/src/core/prompt-templates.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-020 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-021** — 逐文件复核 SRC-0931 packages/coding-agent/src/core/resource-loader.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/coding-agent/src/core/resource-loader.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-021 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-022** — 逐文件复核 SRC-0909 packages/coding-agent/src/core/extensions/types.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/coding-agent/src/core/extensions/types.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-022 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-023** — 逐文件复核 SRC-0908 packages/coding-agent/src/core/extensions/runner.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/coding-agent/src/core/extensions/runner.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-023 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-024** — 逐文件复核 SRC-0953 packages/coding-agent/src/core/tools/output-accumulator.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/coding-agent/src/core/tools/output-accumulator.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-024 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-025** — 逐文件复核 SRC-0945 packages/coding-agent/src/core/tools/bash.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/coding-agent/src/core/tools/bash.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-025 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-026** — 逐文件复核 SRC-0950 packages/coding-agent/src/core/tools/grep.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/coding-agent/src/core/tools/grep.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-026 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-027** — 逐文件复核 SRC-0949 packages/coding-agent/src/core/tools/find.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/coding-agent/src/core/tools/find.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-027 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-028** — 逐文件复核 SRC-0884 packages/coding-agent/src/core/agent-session.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/coding-agent/src/core/agent-session.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-028 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-029** — 逐文件复核 SRC-0306 packages/ai/src/api/openai-completions.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/ai/src/api/openai-completions.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-029 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0
- [x] **ZS1-030** — 逐文件复核 SRC-0917 packages/coding-agent/src/core/model-registry.ts；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该文件完整源行为及其目标映射 | Owned paths: `Docs/learn/stage1_pi_mono/files/packages/coding-agent/src/core/model-registry.ts_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-030 | Rollback: 撤回该文件报告和本项状态，不改其它文件 | Estimate: 按manifest字节顺序逐个处理；超过上限分块再合并 | Estimated LOC: 0

### L1 — 目标 owner 文件，每行一个文件

- [x] **ZS1-070** — 逐文件复核 zenpi src/core.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/core.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-070 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 279620 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-071** — 逐文件复核 zenpi src/runtime.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/runtime.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-071 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 28121 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-072** — 逐文件复核 zenpi src/protocol.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/protocol.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-072 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 45517 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-073** — 逐文件复核 zenpi src/context.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/context.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-073 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 8199 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-074** — 逐文件复核 zenpi src/session.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/session.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-074 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 176828 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-075** — 逐文件复核 zenpi src/backend.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/backend.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-075 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 98741 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-076** — 逐文件复核 zenpi src/config.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/config.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-076 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 77782 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-077** — 逐文件复核 zenpi src/tools.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/tools.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-077 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 119778 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-078** — 逐文件复核 zenpi src/skills.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/skills.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-078 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 40533 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-079** — 逐文件复核 zenpi src/slash.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/slash.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-079 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 81726 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-080** — 逐文件复核 zenpi src/extensions.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/extensions.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-080 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 20742 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-081** — 逐文件复核 zenpi src/domain_execution.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/domain_execution.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-081 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 112548 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-082** — 逐文件复核 zenpi src/domains.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/domains.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-082 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 24622 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-083** — 逐文件复核 zenpi src/governance.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/governance.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-083 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 38260 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-084** — 逐文件复核 zenpi src/headless.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/headless.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-084 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 312227 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-085** — 逐文件复核 zenpi src/tui.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/tui.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-085 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 323172 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0
- [x] **ZS1-086** — 逐文件复核 zenpi src/view_model.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/view_model.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-086 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 40591 B；大于256KiB须连续分块逐块读，最终一文件一报告 | Estimated LOC: 0

- [x] **ZS1-087** — 逐文件复核 zenpi src/slash_actions.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/slash_actions.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-087 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 24998 B；逐文件审核完整owner/模块接线/CI行为 | Estimated LOC: 0
- [x] **ZS1-088** — 逐文件复核 zenpi src/lib.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/lib.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-088 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 592 B；逐文件审核完整owner/模块接线/CI行为 | Estimated LOC: 0
- [x] **ZS1-089** — 逐文件复核 zenpi .github/workflows/ci.yml；layer `L1` | Depends: ZS1-001 | Owner scope: 仅该owner完整现状与源差距 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/.github/workflows/ci.yml_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-089 | Rollback: 撤回该目标文件报告和本项状态，不覆盖源码 | Estimate: 2480 B；逐文件审核完整owner/模块接线/CI行为 | Estimated LOC: 0

- [x] **ZS1-094** — 逐文件复核 zenpi src/layout.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 完整布局/审批/渲染或依赖基线 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/layout.rs_learn.md` | Validators: G-FILE；G-STAGE逐项校验 | Rollback: 仅撤回此文件报告 | Estimate: 54632 B逐文件读，保留原行为 | Estimated LOC: 0
- [x] **ZS1-095** — 逐文件复核 zenpi src/approval.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 完整布局/审批/渲染或依赖基线 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/approval.rs_learn.md` | Validators: G-FILE；G-STAGE逐项校验 | Rollback: 仅撤回此文件报告 | Estimate: 27606 B逐文件读，保留原行为 | Estimated LOC: 0
- [x] **ZS1-096** — 逐文件复核 zenpi src/render.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 完整布局/审批/渲染或依赖基线 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/render.rs_learn.md` | Validators: G-FILE；G-STAGE逐项校验 | Rollback: 仅撤回此文件报告 | Estimate: 30217 B逐文件读，保留原行为 | Estimated LOC: 0
- [x] **ZS1-097** — 逐文件复核 zenpi Cargo.toml；layer `L1` | Depends: ZS1-001 | Owner scope: 完整布局/审批/渲染或依赖基线 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/Cargo.toml_learn.md` | Validators: G-FILE；G-STAGE逐项校验 | Rollback: 仅撤回此文件报告 | Estimate: 1001 B逐文件读，保留原行为 | Estimated LOC: 0
- [x] **ZS1-098** — 逐文件复核 zenpi Cargo.lock；layer `L1` | Depends: ZS1-001 | Owner scope: 完整布局/审批/渲染或依赖基线 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/Cargo.lock_learn.md` | Validators: G-FILE；G-STAGE逐项校验 | Rollback: 仅撤回此文件报告 | Estimate: 42724 B逐文件读，保留原行为 | Estimated LOC: 0

- [x] **ZS1-099** — 逐文件复核 zenpi vendor/crossterm/src/event/source/unix/mio.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 完整原始与定制文件、事件读取或项目回归合同 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/vendor/crossterm/src/event/source/unix/mio.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-099 | Rollback: 仅撤回本文件报告，不覆盖产品或旧测试 | Estimate: 9026 B按连续字节逐文件读，原始与最终差异独立核对 | Estimated LOC: 0
- [x] **ZS1-133** — 逐文件复核 zenpi tests/headless_project_workspace.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 完整原始与定制文件、事件读取或项目回归合同 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/files/tests/headless_project_workspace.rs_learn.md` | Validators: G-FILE；G-STAGE --item ZS1-133 | Rollback: 仅撤回本文件报告，不覆盖产品或旧测试 | Estimate: 23349 B按连续字节逐文件读，原始与最终差异独立核对 | Estimated LOC: 0

### L2 — 源目录，每行一个目录（仅冻结子集）

- [x] **ZS1-050** — 逐目录整合 pi-mono packages/agent/src/harness/compaction；layer `L2` | Depends: ZS1-012 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/agent/src/harness/compaction/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-050 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-051** — 逐目录整合 pi-mono packages/agent/src/harness/session；layer `L2` | Depends: ZS1-013 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/agent/src/harness/session/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-051 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-052** — 逐目录整合 pi-mono packages/coding-agent/src/core/extensions；layer `L2` | Depends: ZS1-022,ZS1-023 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/coding-agent/src/core/extensions/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-052 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-053** — 逐目录整合 pi-mono packages/coding-agent/src/core/tools；layer `L2` | Depends: ZS1-024,ZS1-025,ZS1-026,ZS1-027 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/coding-agent/src/core/tools/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-053 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-054** — 逐目录整合 pi-mono packages/agent/src/harness；layer `L2` | Depends: ZS1-050,ZS1-051 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/agent/src/harness/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-054 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-055** — 逐目录整合 pi-mono packages/ai/src/api；layer `L2` | Depends: ZS1-016,ZS1-017,ZS1-029 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/ai/src/api/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-055 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-056** — 逐目录整合 pi-mono packages/coding-agent/src/core；layer `L2` | Depends: ZS1-018,ZS1-019,ZS1-020,ZS1-021,ZS1-028,ZS1-030,ZS1-052,ZS1-053 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/coding-agent/src/core/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-056 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-057** — 逐目录整合 pi-mono packages/agent/src；layer `L2` | Depends: ZS1-010,ZS1-011,ZS1-054 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/agent/src/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-057 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-058** — 逐目录整合 pi-mono packages/agent/test；layer `L2` | Depends: ZS1-014 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/agent/test/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-058 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-059** — 逐目录整合 pi-mono packages/ai/src；layer `L2` | Depends: ZS1-015,ZS1-055 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/ai/src/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-059 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-060** — 逐目录整合 pi-mono packages/coding-agent/src；layer `L2` | Depends: ZS1-056 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/coding-agent/src/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-060 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-061** — 逐目录整合 pi-mono packages/agent；layer `L2` | Depends: ZS1-057,ZS1-058 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/agent/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-061 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-062** — 逐目录整合 pi-mono packages/ai；layer `L2` | Depends: ZS1-059 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/ai/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-062 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-063** — 逐目录整合 pi-mono packages/coding-agent；layer `L2` | Depends: ZS1-060 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/coding-agent/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-063 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-064** — 逐目录整合 pi-mono packages；layer `L2` | Depends: ZS1-061,ZS1-062,ZS1-063 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/packages/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-064 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0
- [x] **ZS1-065** — 逐目录整合 pi-mono .；layer `L2` | Depends: ZS1-064 | Owner scope: 直属文件及直接子目录的集成 | Owned paths: `Docs/learn/stage1_pi_mono/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-065 | Rollback: 仅撤回此目录报告，不递归改子项 | Estimate: 逐个验证调用、数据与异常边界 | Estimated LOC: 0

### L2 — 目标目录，每行一个目录

- [x] **ZS1-090** — 逐目录整合 zenpi src；layer `L2` | Depends: ZS1-070,ZS1-071,ZS1-072,ZS1-073,ZS1-074,ZS1-075,ZS1-076,ZS1-077,ZS1-078,ZS1-079,ZS1-080,ZS1-081,ZS1-082,ZS1-083,ZS1-084,ZS1-085,ZS1-086,ZS1-087,ZS1-088,ZS1-094,ZS1-095,ZS1-096 | Owner scope: 目标src子集集成 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/src/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-090 | Rollback: 撤回本目录报告 | Estimate: 核对22个src owner逐文件状态与跨模块调用 | Estimated LOC: 0
- [x] **ZS1-400** — 逐目录整合 zenpi vendor/crossterm/src/event/source/unix；layer `L2` | Depends: ZS1-099 | Owner scope: 本目录冻结直属子集及跨目录连接，其他文件仅context | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/vendor/crossterm/src/event/source/unix/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-400 | Rollback: 仅撤回本目录报告，不递归回滚子项 | Estimate: 每个直属文件与直接子目录独立核对，不接受整个vendor或tests仓库 | Estimated LOC: 0
- [x] **ZS1-401** — 逐目录整合 zenpi vendor/crossterm/src/event/source；layer `L2` | Depends: ZS1-400 | Owner scope: 本目录冻结直属子集及跨目录连接，其他文件仅context | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/vendor/crossterm/src/event/source/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-401 | Rollback: 仅撤回本目录报告，不递归回滚子项 | Estimate: 每个直属文件与直接子目录独立核对，不接受整个vendor或tests仓库 | Estimated LOC: 0
- [x] **ZS1-402** — 逐目录整合 zenpi vendor/crossterm/src/event；layer `L2` | Depends: ZS1-401 | Owner scope: 本目录冻结直属子集及跨目录连接，其他文件仅context | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/vendor/crossterm/src/event/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-402 | Rollback: 仅撤回本目录报告，不递归回滚子项 | Estimate: 每个直属文件与直接子目录独立核对，不接受整个vendor或tests仓库 | Estimated LOC: 0
- [x] **ZS1-403** — 逐目录整合 zenpi vendor/crossterm/src；layer `L2` | Depends: ZS1-402 | Owner scope: 本目录冻结直属子集及跨目录连接，其他文件仅context | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/vendor/crossterm/src/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-403 | Rollback: 仅撤回本目录报告，不递归回滚子项 | Estimate: 每个直属文件与直接子目录独立核对，不接受整个vendor或tests仓库 | Estimated LOC: 0
- [x] **ZS1-404** — 逐目录整合 zenpi vendor/crossterm；layer `L2` | Depends: ZS1-403 | Owner scope: 本目录冻结直属子集及跨目录连接，其他文件仅context | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/vendor/crossterm/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-404 | Rollback: 仅撤回本目录报告，不递归回滚子项 | Estimate: 每个直属文件与直接子目录独立核对，不接受整个vendor或tests仓库 | Estimated LOC: 0
- [x] **ZS1-405** — 逐目录整合 zenpi vendor；layer `L2` | Depends: ZS1-404 | Owner scope: 本目录冻结直属子集及跨目录连接，其他文件仅context | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/vendor/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-405 | Rollback: 仅撤回本目录报告，不递归回滚子项 | Estimate: 每个直属文件与直接子目录独立核对，不接受整个vendor或tests仓库 | Estimated LOC: 0
- [x] **ZS1-406** — 逐目录整合 zenpi tests；layer `L2` | Depends: ZS1-133 | Owner scope: 本目录冻结直属子集及跨目录连接，其他文件仅context | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/tests/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-406 | Rollback: 仅撤回本目录报告，不递归回滚子项 | Estimate: 每个直属文件与直接子目录独立核对，不接受整个vendor或tests仓库 | Estimated LOC: 0

- [x] **ZS1-091** — 逐目录整合 zenpi root；layer `L2` | Depends: ZS1-090,ZS1-093,ZS1-097,ZS1-098,ZS1-405,ZS1-406 | Owner scope: 目标根目录子集集成 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-091 | Rollback: 撤回目标根报告 | Estimate: 核对双树映射与目标范围 | Estimated LOC: 0

- [x] **ZS1-092** — 逐目录整合 zenpi .github/workflows；layer `L2` | Depends: ZS1-089 | Owner scope: 直属文件与直接子目录集成 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/.github/workflows/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-092 | Rollback: 仅撤回本目录报告 | Estimate: 逐个核对CI边界与目录依赖 | Estimated LOC: 0
- [x] **ZS1-093** — 逐目录整合 zenpi .github；layer `L2` | Depends: ZS1-092 | Owner scope: 直属文件与直接子目录集成 | Owned paths: `Docs/learn/stage1_pi_mono/targets/zenpi/.github/current_folder_learn.md` | Validators: G-DIR；G-STAGE --item ZS1-093 | Rollback: 仅撤回本目录报告 | Estimate: 逐个核对CI边界与目录依赖 | Estimated LOC: 0

### L3–L5 — 由逐文件/目录审计驱动的产品实现

- [x] **ZS1-101** — 对话 steer / follow-up 输入合同；layer `L3` | Depends: ZS1-057,ZS1-058,ZS1-091 | Owner scope: G01的实现与本项行为测试 | Owned paths: `src/input_queue.rs`, `src/protocol.rs`, `src/runtime.rs`, `src/core.rs`, `src/slash.rs`, `src/slash_actions.rs`, `src/headless.rs`, `src/tui.rs`, `src/lib.rs`, `tests/stage1_input_queue.rs`, `tests/stage1_input_host.rs`, `tests/headless_protocol.rs` | Validators: G-CODE；cargo test --locked --test stage1_input_queue --test runtime --test core_session --test headless_protocol | Rollback: 禁用新增输入模式，保留旧StartOrSteer协议；迁移项保留可读记录 | Estimate: 约2–4天，含正负/取消/恢复验收 | Estimated LOC: 1700
- [x] **ZS1-102** — 有界工具批次与 sequential barrier；layer `L3` | Depends: ZS1-101,ZS1-057,ZS1-091 | Owner scope: G02的实现与本项行为测试 | Owned paths: `src/tool_runtime.rs`, `src/tools.rs`, `src/core.rs`, `src/lib.rs`, `tests/stage1_tool_batch.rs` | Validators: G-CODE；cargo test --locked --test stage1_tool_batch --test tools --test approval_owner --test core_session | Rollback: 切回串行执行器，保留结果事件/未知结果标记 | Estimate: 约2–4天，含正负/取消/恢复验收 | Estimated LOC: 2300
- [x] **ZS1-103** — 版本化语义 checkpoint 与完整 turn 切点；layer `L3` | Depends: ZS1-050,ZS1-051,ZS1-091 | Owner scope: G03的实现与本项行为测试 | Owned paths: `src/context.rs`, `src/session.rs`, `tests/stage1_context_checkpoint.rs` | Validators: G-CODE；cargo test --locked --test stage1_context_checkpoint --test context --test session_recovery | Rollback: 写入迁移前保留备份/版本标识；停用新写入但不删除已写历史 | Estimate: 约2–4天，含正负/取消/恢复验收 | Estimated LOC: 1700
- [x] **ZS1-104** — 通过真实 backend 生成并使用语义摘要；layer `L3` | Depends: ZS1-103,ZS1-101 | Owner scope: G03的实现与本项行为测试 | Owned paths: `src/context.rs`, `src/core.rs`, `src/backend.rs`, `src/governance.rs`, `tests/stage1_semantic_compaction.rs`, `src/session.rs`, `src/headless.rs`, `src/slash_actions.rs`, `tests/resume_compact_owner.rs`, `tests/stage1_input_host.rs`, `src/tui.rs` | Validators: G-CODE；cargo test --locked --test stage1_semantic_compaction --test resume_compact_owner --test session_recovery | Rollback: 关闭自动语义压缩，沿用完整历史或明确context-overflow错误；不伪造fallback语义 | Estimate: 约2–4天，含正负/取消/恢复验收 | Estimated LOC: 2400
- [x] **ZS1-105** — append-only 分支树与 active leaf 持久化；layer `L3` | Depends: ZS1-103,ZS1-056,ZS1-091 | Owner scope: G04的实现与本项行为测试 | Owned paths: `src/session_tree.rs`, `src/session.rs`, `src/lib.rs`, `tests/stage1_session_tree.rs` | Validators: G-CODE；cargo test --locked --test stage1_session_tree --test session_cli --test session_maintenance_owner | Rollback: 关闭branch新写入并保留备份；树journal不降级破坏性重写 | Estimate: 约2–4天，含正负/取消/恢复验收 | Estimated LOC: 2200
- [x] **ZS1-106** — 分支上下文及 TUI/JSONL 导航入口；layer `L3` | Depends: ZS1-105,ZS1-104 | Owner scope: G04的实现与本项行为测试 | Owned paths: `src/context.rs`, `src/core.rs`, `src/slash.rs`, `src/slash_actions.rs`, `src/protocol.rs`, `src/headless.rs`, `src/tui.rs`, `tests/stage1_branch_owner.rs`, `src/session.rs` | Validators: G-CODE；cargo test --locked --test stage1_branch_owner --test core_session --test headless_protocol --test session_recovery | Rollback: 隐藏新导航入口，保留tree owner读取及旧会话路由 | Estimate: 约2–4天，含正负/取消/恢复验收 | Estimated LOC: 2200
- [x] **ZS1-107** — 模型能力 registry 与 backend 能力协商；layer `L3` | Depends: ZS1-059,ZS1-056,ZS1-091 | Owner scope: G05的实现与本项行为测试 | Owned paths: `src/providers/mod.rs`, `src/providers/registry.rs`, `src/backend.rs`, `src/config.rs`, `src/core.rs`, `src/slash_actions.rs`, `src/lib.rs`, `tests/stage1_model_registry.rs`, `src/headless.rs`, `tests/config.rs` | Validators: G-CODE；cargo test --locked --test stage1_model_registry --test backend --test config --test slash_actions | Rollback: 回到显式旧profile能力配置；保存新字段而不误读为支持 | Estimate: 约2–4天，含正负/取消/恢复验收 | Estimated LOC: 2100
- [x] **ZS1-108** — Chat Completions SSE 流式路径；layer `L3` | Depends: ZS1-107 | Owner scope: G05的实现与本项行为测试 | Owned paths: `src/backend/chat_stream.rs`, `src/backend.rs`, `src/view_model.rs`, `tests/stage1_chat_stream.rs`, `tests/backend.rs` | Validators: G-CODE；cargo test --locked --test stage1_chat_stream --test backend --test headless_event_budget | Rollback: 按显式配置退回旧Chat非流式路径，保留真实能力标志 | Estimate: 约2–4天，含正负/取消/恢复验收 | Estimated LOC: 2100
- [x] **ZS1-109** — Anthropic Messages 原生 adapter；layer `L3` | Depends: ZS1-107,ZS1-055 | Owner scope: G05的实现与本项行为测试 | Owned paths: `src/providers/anthropic.rs`, `src/providers/mod.rs`, `src/backend.rs`, `src/config.rs`, `tests/stage1_anthropic.rs`, `src/providers/registry.rs`, `src/core.rs`, `tests/backend.rs`, `src/backend/transport.rs`, `tests/stage1_transport.rs` | Validators: G-CODE；cargo test --locked --test stage1_anthropic --test backend --test config --test security | Rollback: 禁用该provider选择，保留用户配置且明确unsupported | Estimate: 约3–5天，含正负/取消/恢复验收 | Estimated LOC: 2900
- [x] **ZS1-110** — Gemini 原生 adapter；layer `L3` | Depends: ZS1-107,ZS1-055 | Owner scope: G05的实现与本项行为测试 | Owned paths: `src/providers/google.rs`, `src/providers/mod.rs`, `src/backend.rs`, `src/config.rs`, `tests/stage1_gemini.rs`, `src/providers/registry.rs`, `src/core.rs`, `tests/backend.rs`, `src/backend/transport.rs`, `tests/stage1_transport.rs` | Validators: G-CODE；cargo test --locked --test stage1_gemini --test backend --test config --test security | Rollback: 禁用该provider选择，保留配置及旧provider支持 | Estimate: 约3–5天，含正负/取消/恢复验收 | Estimated LOC: 2900
- [x] **ZS1-111** — 工具原始输出 artifact 与增量进度；layer `L3` | Depends: ZS1-102,ZS1-053,ZS1-091 | Owner scope: G06/G02的实现与本项行为测试 | Owned paths: `src/tool_output.rs`, `src/tools.rs`, `src/tool_runtime.rs`, `src/core.rs`, `src/view_model.rs`, `src/headless.rs`, `src/tui.rs`, `src/lib.rs`, `tests/stage1_tool_output.rs`, `tests/tools.rs`, `src/tool_output/redaction.rs`, `src/security.rs`, `src/protocol.rs`, `src/slash.rs`, `src/slash_actions.rs`, `src/search.rs` | Validators: G-CODE；cargo test --locked --test stage1_tool_output --test tools --test tui_logs --test headless_event_budget | Rollback: 停用新增artifact写入/读取，保留有界输出及明确截断；只清理本lease产物 | Estimate: 约3–5天，含正负/取消/恢复验收 | Estimated LOC: 2800
- [x] **ZS1-112** — 显式 regex/glob/ignore/context 搜索；layer `L3` | Depends: ZS1-053,ZS1-091 | Owner scope: G07的实现与本项行为测试 | Owned paths: `src/tools.rs`, `src/search.rs`, `src/lib.rs`, `tests/stage1_search.rs`, `src/core.rs`, `tests/tools.rs` | Validators: G-CODE；cargo test --locked --test stage1_search --test tools --test security | Rollback: 回到旧literal工具schema并保留现有搜索功能 | Estimate: 约2–4天，含正负/取消/恢复验收 | Estimated LOC: 1800
- [x] **ZS1-113** — SKILL.md 发现与按需调用；layer `L3` | Depends: ZS1-056,ZS1-091 | Owner scope: G08的实现与本项行为测试 | Owned paths: `src/skills.rs`, `src/core.rs`, `src/headless.rs`, `src/resource_loader.rs`, `src/slash.rs`, `src/slash_actions.rs`, `Cargo.toml`, `Cargo.lock`, `tests/stage1_skill_md.rs`, `tests/stage1_resource_host.rs` | Validators: G-CODE；cargo test --locked --test stage1_resource_host --test stage1_skill_md --test skills --test slash --test slash_actions | Rollback: 禁用新SKILL.md reader，保留TOML兼容路径及原配置 | Estimate: 约2–4天，含正负/取消/恢复验收 | Estimated LOC: 3500
- [x] **ZS1-114** — 参数模板与资源原子 reload；layer `L3` | Depends: ZS1-113 | Owner scope: G08的实现与本项行为测试 | Owned paths: `src/prompt_templates.rs`, `src/resource_loader.rs`, `src/core.rs`, `src/headless.rs`, `src/config.rs`, `src/skills.rs`, `src/slash.rs`, `src/slash_actions.rs`, `src/lib.rs`, `tests/stage1_prompt_resources.rs`, `tests/stage1_resource_host.rs` | Validators: G-CODE；cargo test --locked --test stage1_resource_host --test stage1_prompt_resources --test skills --test core_session --test slash_actions | Rollback: 移除新template/reload入口，回到旧静态resource读取 | Estimate: 约2–4天，含正负/取消/恢复验收 | Estimated LOC: 2800
- [x] **ZS1-115** — 类型化 subprocess hooks 与生命周期撤销；layer `L3` | Depends: ZS1-114,ZS1-102,ZS1-052 | Owner scope: G09的实现与本项行为测试 | Owned paths: `src/extensions.rs`, `src/extension_runtime.rs`, `src/core.rs`, `src/tools.rs`, `src/lib.rs`, `tests/stage1_extension_hooks.rs` | Validators: G-CODE；cargo test --locked --test stage1_extension_hooks --test extensions --test approval_owner --test tools | Rollback: 回退到tools/call-only协议，停用hooks并撤销全部新generation | Estimate: 约3–5天，含正负/取消/恢复验收 | Estimated LOC: 2800
- [x] **ZS1-116** — 外部执行结果的可验证产物与主控验收；layer `L3` | Depends: ZS1-111,ZS1-091 | Owner scope: G10的实现与本项行为测试 | Owned paths: `src/domain_execution.rs`, `src/domains.rs`, `src/governance.rs`, `src/session.rs`, `src/headless.rs`, `src/tui.rs`, `tests/stage1_external_evidence.rs`, `src/slash.rs`, `tests/domain_execution_owner.rs`, `src/core.rs` | Validators: G-CODE；cargo test --locked --test stage1_external_evidence --test domain_execution_owner --test domain_execution_host --test governance | Rollback: 停用新接受通道；旧manifest仅保留已导入候选/历史，不补记master完成 | Estimate: 约2–4天，含正负/取消/恢复验收 | Estimated LOC: 2400
- [x] **ZS1-117** — 生产入口、边界故障与轻量预算验收；layer `L5` | Depends: ZS1-106,ZS1-108,ZS1-109,ZS1-110,ZS1-111,ZS1-112,ZS1-115,ZS1-116 | Owner scope: G01～G10的实现与本项行为测试 | Owned paths: `tools/stage1_host_smoke.py`, `tests/stage1_host.rs`, `.github/workflows/ci.yml`, `Docs/quality/stage1/ZS1-117`, `tools/bench_runtime.py`, `Cargo.toml` | Validators: G-CODE；G-RUST；cargo test --locked --test stage1_host；G-PROD；既有budget工具按仓库help契约运行并保存receipt | Rollback: 撤回本项新增CI job与smoke资产；产品回归按产生问题的原item回滚 | Estimate: 约2–4天，含正负/取消/恢复验收 | Estimated LOC: 2300
- [x] **ZS1-118** — Provider 开放与官方/第三方供应商对齐；layer `L3` | Depends: ZS1-107,ZS1-091 | Owner scope: 模型能力默认开放与 provider 格式对齐的实现与本项行为测试 | Owned paths: `src/providers/registry.rs`, `src/backend.rs`, `src/config.rs`, `tests/stage1_model_registry.rs`, `tests/stage1_anthropic.rs`, `tests/stage1_gemini.rs` | Validators: G-CODE；cargo test --locked --test stage1_model_registry --test stage1_anthropic --test stage1_gemini --test backend | Rollback: 撤回未知模型默认开放，恢复显式覆盖；不改变官方目录项 | Estimate: 依赖现有多wire实现的能力协商扩展 | Estimated LOC: 300
- [x] **ZS1-119** — `/sync` 用户要求同步到 single-authority blueprint 并触发执行；layer `L3` | Depends: ZS1-118 | Owner scope: `/sync` 解析、blueprint 单一权威更新与执行派发的实现与本项行为测试 | Owned paths: `src/slash.rs`, `src/slash_actions.rs`, `src/headless.rs`, `src/tui.rs`, `Docs/stage_1_v3_pi_mono_blueprint.md`, `tests/stage1_sync.rs` | Validators: G-CODE；cargo test --locked --test stage1_sync --test slash --test slash_actions | Rollback: 撤回 /sync 命令与本次追加项，保留既有清单项 | Estimate: 解析+幂等追加+触发，含冲突与去重 | Estimated LOC: 700
- [x] **ZS1-140** — TUI 一层 project tab：增/减/调位置/重命名/每项目风格；layer `L3` | Depends: ZS1-121 | Owner scope: 一层 tab 交互与持久化 owner | Owned paths: `src/tui.rs`, `src/project_workspace.rs`, `src/layout.rs`, `tests/tui_project_workspace.rs`, `tools/tui_project_workspace_smoke.py` | Validators: G-CODE、G-HOST；cargo test --locked --test tui_project_workspace --test layout_persistence | Rollback: 撤回 tab 操作，恢复既有项目/布局，不删会话 | Estimate: 增删+改名+左右调位+每项目风格持久化 | Estimated LOC: 1500
- [x] **ZS1-141** — 一层 tab 增加工作文件夹支持本地与 SSH 远端；layer `L3` | Depends: ZS1-140 | Owner scope: 目录选择与远端只读探测 owner | Owned paths: `src/directory_picker.rs`, `src/tui.rs`, `src/project_workspace.rs`, `tests/tui_directory_picker.rs`, `tools/tui_project_workspace_smoke.py` | Validators: G-CODE、G-HOST；cargo test --locked --test tui_directory_picker --test tui_project_workspace | Rollback: 撤回远端路径，保留本地选择 | Estimate: 本地 + ssh config 别名 + 显式 host/user/port/key 两种方式 | Estimated LOC: 1600
- [ ] **ZS1-142** — TUI 第二层 tab：每项目内嵌 worktree tab（默认复用一层；加号单击新建、长按双 logo）；layer `L3` | Depends: ZS1-140 | Owner scope: 二层 tab 状态与 worktree 生命周期 owner | Owned paths: `src/tui.rs`, `src/project_workspace.rs`, `src/layout.rs`, `tests/tui_project_workspace.rs`, `tools/tui_project_workspace_smoke.py` | Validators: G-CODE、G-HOST；cargo test --locked --test tui_project_workspace --test layout_persistence | Rollback: 撤回二层 tab 与 worktree 新建，恢复单层项目视图 | Estimate: 二层 tab 增删/调位 + worktree 新建/原地模式 | Estimated LOC: 2000
- [ ] **ZS1-143** — Runtime 3 用例验收：/blueprint /execute /learn /explore /addloop 与 TUI 区域(pm/arch/resources/execution/terminal/gantt)+BentoBox；layer `L5` | Depends: ZS1-117,ZS1-140,ZS1-142,ZS1-144,ZS1-145 | Owner scope: 端到端 runtime 验收与证据 | Owned paths: `tests/stage1_runtime_modes.rs`, `tools/stage1_host_smoke.py`, `Docs/quality/stage1/ZS1-143` | Validators: G-CODE、G-HOST、G-PROD | Rollback: 撤回验收记录，不删功能 | Estimate: 3 用例真实 TUI→headless + 区域与 BentoBox 断言 | Estimated LOC: 600
- [x] **ZS1-144** — 命令补齐：/execute /explore /addloop 的语义与实现；layer `L3` | Depends: ZS1-119 | Owner scope: 三个命令的解析/动作/路由与测试 | Owned paths: `src/slash.rs`, `src/slash_actions.rs`, `src/headless.rs`, `tests/stage1_sync.rs`, `tests/slash.rs` | Validators: G-CODE；cargo test --locked --test slash --test stage1_sync | Rollback: 撤回三个命令，保留 /loop /plan | Estimate: /execute=蓝图执行派发；/explore=research 循环；/addloop=/loop 别名或独立 | Estimated LOC: 1200
- [ ] **ZS1-145** — TUI 区域补齐：arch 架构区与 execution 执行区（BentoBox 可调）；layer `L3` | Depends: ZS1-140 | Owner scope: 新 pane 的数据与渲染 owner | Owned paths: `src/layout.rs`, `src/tui.rs`, `src/view_model.rs`, `tests/layout.rs`, `tests/tui_bentobox.rs` | Validators: G-CODE；cargo test --locked --test layout --test tui_bentobox | Rollback: 撤回新 pane，恢复原 BentoBox 预设 | Estimate: 新 pane 接入 + 断点/折叠 | Estimated LOC: 1200
- [x] **ZS1-199** — Master 集成验收与阶段交付；layer `L6` | Depends: ZS1-065,ZS1-091,ZS1-117,ZS1-350,ZS1-128 | Owner scope: 仅整合本阶段证据与最终交付 | Owned paths: `Docs/stage_1_v3_pi_mono_blueprint.md`, `Docs/quality/stage1/acceptance.md`, `Docs/execution/active_requirement.json`, `README.md` | Validators: G-STAGE、G-RUST、G-PROD；所有依赖[x]且逐文件/目录覆盖闭包，零缺失或虚假行为证据 | Rollback: 撤回本阶段接受记录与活动指针切换，不删除用户或worker原始证据 | Estimate: 约1天独立主控复验与交付 | Estimated LOC: 0

## 6. 各产品项的完成定义

以下条款与第 5 节同一 ID 的执行行组成一个要求，不能只执行 checklist 的标题。每项证据均使用第 4 节 G-CODE 路径与格式。

### ZS1-101 — 对话 steer / follow-up 输入合同

在原 runtime admission 上增加同一 session 的显式 follow-up 队列类型与 one-at-a-time/all。输入有 ID、顺序、received/applied 状态、确定生效边界；用户可查看、修改或取消未生效项。共享 typed action 由 slash/slash_actions 分发，headless JSONL 与 TUI 命令入口接入同一 input_queue owner，并在本项实现端到端接线。steer 于整批工具完成后的下一模型回合生效；follow-up 只在当前 agent 即将停下时消费。prepare/compaction 期间到达的输入只消费一次。持久队列与会话恢复共用既有 journal/去重机制；不能创建第二个调度服务。

网络鲁棒性约束：Responses/Chat SSE 必须按长连接语义增量读取，fixture 与生产代理不得用未完成的 `Content-Length` 模拟流式响应；优先使用 chunked 或连接关闭分帧。`RecvBody` 超时只用于取消轮询，不得把延长超时当作 framing 修复。可恢复的断流（包括 `Peer disconnected`）进入有界指数退避 retry；已解析的增量保留，续接请求复用同一 turn/request 幂等键并抑制重复副作用，达到上限后才产生用户可见失败。

正：工具运行时排入 follow-up 不打断该工具，steer 顺序一致；长 prepare 后仍按选择策略只消费规定条数。负：重复ID同内容幂等、不同内容冲突，容量耗尽可见拒绝，已applied输入不能修改。取消：排队取消与执行取消分别有终态。重启：已applied不重放，未applied按同ID恢复。

### ZS1-102 — 有界工具批次与 sequential barrier

工具显式声明并行资格与副作用类别；默认保守串行，只对通过现有 policy/approval 的独立工具并行。全局或单工具 sequential 声明形成整个批次的串行约束。并发受现有治理预算限制，调用 ID 不随完成次序变化，结果按源调用顺序持久化。模型 length 终止产生的工具参数不得执行。

正：两个读工具在并发上限内重叠运行；含写工具/屏障的批次串行。负：截断参数、审批拒绝、非法call ID产生失败结果且零副作用。取消：停止待运行调用、取消运行工具并回收子进程。重启：已提交的工具结果不再次执行；未知副作用需reconcile，不能自动重跑。

### ZS1-103 — 版本化语义 checkpoint 与完整 turn 切点

定义摘要版本、source/branch tip、已覆盖记录范围、retained IDs、read/modified files、未解决调用和summary usage。不拆分需成对保留的tool call/result，不把 error/aborted/deferred assistant 伪装成有效上下文。新格式可读旧digest checkpoint，但不能声称旧digest已包含语义。

正：含多工具调用的长历史生成合法边界；读取旧journal不丢记录。负：缺parent/结果对、越界cut、hash不符、不同branch摘要均拒绝。取消：新checkpoint尚未完成时不切换。重启：只恢复最后完整、版本可识别的记录。

### ZS1-104 — 通过真实 backend 生成并使用语义摘要

摘要保留目标、用户约束、决策、当前进度、未完成任务、关键事实、读/改文件及工具未定结果。用已配置backend在现有budget/cancel owner下发请求，记录usage与原始来源；上下文预算采用model metadata，缺失时显式保守估算。摘要合格后原子替换；失败不能悄悄退回计数+hash并声称成功。

正：只存在旧turn中的约束和待办在压缩、进程重启后的实际provider输入仍存在。负：空/错误/length/truncated摘要、费用超额和超预算摘要拒绝替换；既有有效checkpoint仍可用。取消：取消摘要请求，不漏后续steer、不发第二次未授权请求。重启：摘要和tail匹配同一提交边界。

### ZS1-105 — append-only 分支树与 active leaf 持久化

在既有journal引入独立的entry parent graph及active leaf，不复用Turn.parent_id的活动turn含义。保留所有分支记录，以有界索引从leaf回溯。提供选中祖先、命名/摘要、按leaf fork的owner API；迁移线性旧journal为单链。fork使用新owner身份，不能复制锁、ACK或未完成副作用归属。

正：A→B与A→C并存，来回切换不删B；fork-at-B只带A/B ancestry。负：parent不存在、跨session parent、cycle、叶节点不合法、超预算树均不变更旧数据。取消：导航/迁移取消不写半个leaf。重启：active leaf/摘要一致；模拟中断append和竞争owner。

### ZS1-106 — 分支上下文及 TUI/JSONL 导航入口

共享owner对tree/list/select/fork进行类型化分发。TUI显示当前leaf，headless返回同一投影；模型只接收当前ancestry和匹配branch摘要。树浏览不运行provider或历史工具，支持有界分页；保持现有session mailbox/reconnect边界。

正：选C后的下一次实际请求含A/C且无B，重启仍选C；PTY与JSONL显示相同leaf。负：超长/非法ID、旧branch摘要或跨session选择拒绝并保留当前视图。取消：切换中止不留半个context。重启：切换记录与provider context一致，无工具副作用重放。

### ZS1-107 — 模型能力 registry 与 backend 能力协商

将provider wire与model metadata分离：context window、max output、text/image/file/structured-output、reasoning等级、stream/tool能力，以及可选价格的来源/版本。显式用户配置覆盖有来源的内置表；未知字段/能力按约定拒绝或保守处理。/models共享owner呈现可选model和真实能力，保留profiles兼容。

正：切换不同model改变预算和合法输入；现有Responses图像/文件/结构化输出保留。负：不支持的附件/reasoning在请求前拒绝；缺context信息不假设无限；不存在的model不静默映射。取消：查询无provider请求。重启：选定profile/model及来源一致。

### ZS1-108 — Chat Completions SSE 流式路径

发送stream:true，解析文本、reasoning及多tool arguments增量，保持每个call独立索引/ID。复用现有ProviderEvent、bounded mailbox和redaction。记录finish_reason及usage，正常EOF不能替代缺失终态。

正：本地HTTP SSE中第一段文本在结束前通过host可见，多call交错/分帧UTF-8正确组装。负：缺finish_reason、坏JSON、超限frame、截断参数不得报告正常完成/执行工具。取消：关闭连接、无重复请求，保留已有body-read取消。重启：中断流标记为未完成，不能拼成旧完成答复。

### ZS1-109 — Anthropic Messages 原生 adapter

实现原生Messages request/header、system/messages/tool schema、流事件、usage、终止原因与reasoning块，接入统一provider factory。使用既有secret handle、网络限制和取消，不把OpenAI兼容请求冒充Anthropic原生。基于本轮源hash和实际协议文档冻结支持的模型/字段。

正：本地native fixture逐字段检查请求并返回text/tool/thinking增量；tool continuation历史合法。负：坏事件、缺终态、错误凭据、unsupported image/file或参数明确失败。取消：流/等待阶段遵守host边界且无漏进程/socket；重启中断请求不得自动重跑工具。新增依赖必须过现有V2-303 gate。

### ZS1-110 — Gemini 原生 adapter

实现原生Gemini contents/parts、function call/result、thinking配置及必要签名保留，映射stream/usage/finish reason。模型切换时按registry检查历史可表示性；不可丢签名后声称相同history。

正：本地native fixture验证请求和tool/think/text流，切换同兼容family仍能继续。负：签名缺失、结构不合法、unsupported类型、缺终态均返回明确错误；禁止静默丢失工具历史。取消：取消请求并给定终态；重启按未完成operation恢复提示。依赖门禁同ZS1-109。

### ZS1-118 — Provider 开放与官方/第三方供应商对齐

保证 provider 默认开放，对齐 opencode 的供应商集合（models.dev 目录，官方与第三方并存）：主要格式为 OpenAI（Chat Completions 与 Responses）与 Anthropic Messages，并保留既有 Google Generative AI。未编目模型（任何 provider/id）默认继承 wire 能力（text/images/files/tools/structured_output/streaming），不再默认 text-only；未知字段仍可保守。任意的 OpenAI 兼容或 Anthropic 兼容 base_url 无需手写 override 即可使用工具与流式。显式 `model_override` 仍可逐字段收紧或放开，官方目录项的已知能力不被覆盖。未知模型的 context/output 预算仍取保守有限值，不假设无限。

正：未编目模型在 chat/responses/anthropic 三 wire 上默认带 tools 且 stream；官方目录项能力不变。负：显式 `tools=false` 仍拒绝未声明工具调用并失败可见；缺凭据、坏流仍 fail closed。取消/重启：沿用各 adapter 既有边界。对齐范围只要求格式与默认能力，不要求逐一集成 models.dev 的每家 SDK；第三方 OpenAI/Anthropic 兼容端点必须保留可用。

### ZS1-111 — 工具原始输出 artifact 与增量进度

以现有output cap为内存视图上限；原始stdout/stderr写入host管理的受限临时artifact，记录bytes/hash、display range、truncated/complete、call ID。配额覆盖单文件/总量/TTL/会话清理；完整性只有写盘结束并验证后才成立。提供有界范围读取入口，路径不可逃逸或读取凭据。进度共用canonical事件，terminal后关闭updates。

正：超内存上限输出可由artifact重建，显示UTF-8不破碎，未结束先看到进度，终态raw hash一致。负：磁盘满/权限错/spill中断可见，绝不宣称full output；路径/symlink逃逸拒绝，慢消费者出现drop marker。取消：kill/reap后flush已得片段并标incomplete。重启：artifact引用存在/过期/缺失可区分，不造空成功文件。

### ZS1-112 — 显式 regex/glob/ignore/context 搜索

search_text保持literal默认；新增显式regex、glob过滤、gitignore选择、结果前后上下文与行号/截断原因；find提供有界路径匹配。探测已安装rg/fd或使用同语义本地实现，缺能力明确返回unsupported，不能偷偷下载可执行文件。

正：嵌套项目、ignored文件、glob及regex在固定fixture返回精确路径/行号；原literal结果不变。负：非法regex、路径逃逸、超深树、大二进制、缺工具均有界且错误明确。取消：中止搜索子进程/遍历；重启不保留子进程或错误partial索引。

### ZS1-113 — SKILL.md 发现与按需调用

读取标准frontmatter的name/description/disable-model-invocation等支持字段，明确嵌套发现、ignore、项目/用户/显式路径优先级与同名冲突诊断。初始prompt只含描述/位置索引；显式或模型允许调用后读取该skill正文，解析相对资源引用并保留来源。保持skill.toml兼容；同名不同格式优先级固定。

正：本机learn/execution标准SKILL.md可直接发现并调用，未调用正文不入prompt，参数和相对路径正确。负：禁model调用但允许显式调用，过大/坏frontmatter/重复名/越界引用明确处理。取消：加载取消不写半个catalogue。重启：相同来源产生同一index，不复用陈旧正文hash。

### ZS1-114 — 参数模板与资源原子 reload

发现独立prompt templates，支持约定的positional/quoted/default及$ARGUMENTS展开；替换仅一轮，参数中含$1或shell字符仍是字面内容。技能、模板和可reload资源先构建新snapshot再原子替换，明确生效于下一turn与显式reload入口，不改正在运行turn快照。

正：空参、引号、$1/$@/defaults符合源合同，reload后下一turn用新版本。负：非法模板、递归式输入、坏资源/重复定义不破坏旧snapshot，不发生shell执行。取消：加载中止旧snapshot仍用；重启加载已选路径，不依赖内存临时目录。

### ZS1-115 — 类型化 subprocess hooks 与生命周期撤销

扩展manifest版本协商工具及input/context/before_tool/after_tool/session-close hooks；定义固定顺序、结果schema、timeout、bytes预算及错误隔离。每个session generation有host-owned capability；reload/dispose撤销旧callbacks并回收资源。hook改写参数后再次检查schema/path/approval/policy；任何deny优先。

正：合法hook在真实owner运行并影响下一模型input，start/end/close各一次；工具拦截先于spawn。负：坏输出/超时/过大结果/抛错不能绕过gate，过期generation不能写新session，失败reload保留有效旧集合。取消：kill/reap插件进程，拒绝late callback。重启：新进程重新建立lease，旧capability不复活。

### ZS1-116 — 外部执行结果的可验证产物与主控验收

扩展现有manifest导入，不重建claim系统。记录claim/lease/policy、输入baseline与输出revision/diff、实际文件artifact bytes/hash、预定validator ID及host观测的argv/exit/output；将worker declared-success与master accepted分开。导入manifest只能形成候选，master在匹配工作树运行预定validator后才可接受。纯control_plane_only永远不能等价产品完成。

正：小型外部worker真实改一个授权文件、实际validator通过，候选经host验证到达accepted。负：仅acceptance_passed=true、路径存在、错hash、错revision、伪exit、stale lease、越界diff、claim重放和漏validator均拒绝；相同manifest幂等。取消/重启：取消阻止接受，已验证证据不重复执行worker，未知副作用等待reconcile。worker不能自改Blueprint状态。

### ZS1-117 — 生产入口、边界故障与轻量预算验收

建立生产feature集的TUI PTY和headless矩阵，覆盖所有本阶段能力实际入口。HTTP fixtures只模拟远端协议，工具确实读写受控workspace、创建子进程和journal；验证按钮/命令→owner→输出→证据。沿用现有runtime-budget-v2的阈值和bench工具接口，记录启动、RSS、输出/队列峰值及binary大小；不能把静态cap计数当内存测量。

正：每个G01～G10至少一个headless和适用PTY场景；两native wire fixture均真实HTTP。负：重复输入、坏流、无权限、资源耗尽、迟到callback、虚假manifest、stderr协议污染均失败可见。取消：至少一个运行tool/网络请求被host中止。重启：分支、压缩、输入与外部候选在独立新进程恢复。缺测试场景不得以其它场景代替。

## 7. 最终接受条件与两项目的边界

ZS1-199 只能在源文件、目标owner文件、每个源/目标目录及全部产品项分别完成后接受。主控先检查除 ZS1-199 自身以外清单零 `[ ]` / `[_]`、唯一活动requirement、双树manifest/index一致、每个产品判据在已集成revision上有真实证据。上述复验通过后，主控将 ZS1-199 一次提升到 `[x]`，再确认全清单零 `[ ]` / `[_]`。结构通过、完整阅读、单元测试、集成测试、生产入口和实测预算分别记录，不合并成含糊的“1:1完成”。源库不在scope的功能不计入分母，选定功能未通过的项也不能从分母删除。

b3ehive 的审阅件是 `/Users/mac/Downloads/b3ehive_stage_1_pi_mono_harness_blueprint.md`，负责通用 turn signal、checkpoint、replay、tool evidence、capability lifetime 和验收方法。zenpi 负责具体Rust/HTTP/子进程/PTY实现，保留自身policy与budget。两个DAG只在各自repo闭合；zenpi采用本文件明确的本地协议即可执行，不能把“等待另一份草案通过”伪装为实现完成或无限期阻塞。若将来采用b3ehive已接受的contract版本，记录版本映射并复验，不直接复制其状态。

上一阶段只交付蓝图并完成结构检查；本run已经开始执行。所有产品接受仍须真实证据，不继承旧learn的[x]。



## 8. 用户追加的项目分页与Codex CLI交互要求（3.1.0）

原Stage1全部能力继续保留。现有BentoBox的pane组成、分栏/行权重、collapse/focus、breakpoints及保存布局为兼容硬要求；在它上方完善项目分页，不换成单聊天布局。顶部+一次点击立即呼出工作文件夹选择；一次确认创建并激活project tab，名称取所选目录，稳定ID与规范cwd分离显示名，工具/权限边界/provider上下文/会话/项目资源都绑定该目录。取消不产生空tab、不改当前项目。

目录picker至少在终端中可靠可用：键盘/鼠标浏览、路径输入和补全、当前/父目录、确认/Esc取消、无效路径/权限/空目录反馈。native适配可追加，但不能成为缺失时不可用的唯一路径。相同路径激活既有tab；不同路径同basename可区分。切换保存输入草稿、历史滚动、会话及原BentoBox状态；关闭非活动tab不能清空当前会话，最后tab保持可用。运行任务和未决审批归原project，late events不得污染新项目；禁止进程全局chdir造成项目串目录。

Codex交互参考为本机 `/Users/mac/GitHub/codex`，HEAD `b3b3d262787f4902a7449f17d793241a34d311ad`。逐文件覆盖输入编辑/粘贴、历史检索、排队消息编辑、slash/技能/文件补全、模型/思考等级选择、审批权限、状态/用量/取消、滚动/复制/恢复。源中每个命令均需在ux-reference映射到zenpi行为，区分provider专属服务、调试命令和通用交互；已支持操作必须有真实入口，不能用固定成功、空菜单、未接线按钮抵充。

### Codex参考文件，独立manifest与逐项状态

| Item | 源文件 | Bytes | SHA256 |
|---|---|---:|---|
| ZS1-300 | [codex-rs/tui/src/bottom_pane/chat_composer.rs](/Users/mac/GitHub/codex/codex-rs/tui/src/bottom_pane/chat_composer.rs) | 374943 | `234189c66f50c8654ef72e86ad5463eea5dd1e0c09c98f8d64c898449161b6da` |
| ZS1-301 | [codex-rs/tui/src/bottom_pane/textarea.rs](/Users/mac/GitHub/codex/codex-rs/tui/src/bottom_pane/textarea.rs) | 91601 | `8cb5241c9e818210bfff63975e703f45a25a3da91bdb395667893903f7cb4b45` |
| ZS1-302 | [codex-rs/tui/src/bottom_pane/paste_burst.rs](/Users/mac/GitHub/codex/codex-rs/tui/src/bottom_pane/paste_burst.rs) | 24594 | `c80558dfd3cbde437f2fec9a855b01a3180542a53bb135dc92ce7c81299f1ee7` |
| ZS1-303 | [codex-rs/tui/src/bottom_pane/approval_overlay.rs](/Users/mac/GitHub/codex/codex-rs/tui/src/bottom_pane/approval_overlay.rs) | 57262 | `c924923b402b198f8f77b249f0d37892896c0b84002c0f6e48330917da80cecc` |
| ZS1-304 | [codex-rs/tui/src/bottom_pane/pending_thread_approvals.rs](/Users/mac/GitHub/codex/codex-rs/tui/src/bottom_pane/pending_thread_approvals.rs) | 4105 | `a401d7c6ba3051fcf2c20152a9e86a3f4f8751a87470cbc7f518001326618a2f` |
| ZS1-305 | [codex-rs/tui/src/bottom_pane/mod.rs](/Users/mac/GitHub/codex/codex-rs/tui/src/bottom_pane/mod.rs) | 72998 | `97af02eed39dbfdf69d1ec3e14dc13b9a91f4b33f5e4b985cc1f2cbae4105823` |
| ZS1-306 | [codex-rs/tui/src/slash_command.rs](/Users/mac/GitHub/codex/codex-rs/tui/src/slash_command.rs) | 8020 | `9e53ca5bff5a5e7b86aa97731da985082db5d130c453e2d363edb9c5758ebc5a` |
| ZS1-307 | [codex-rs/tui/src/file_search.rs](/Users/mac/GitHub/codex/codex-rs/tui/src/file_search.rs) | 4009 | `7aaa33ac7fd28cbe5fa3405cd3490b30e614272122bb572c9c352acc418b71d9` |
| ZS1-308 | [codex-rs/tui/src/key_hint.rs](/Users/mac/GitHub/codex/codex-rs/tui/src/key_hint.rs) | 3306 | `07f34aab630b2658560d2d4e73719481ec8638e33eb38401d7a23643689f13c8` |
| ZS1-309 | [codex-rs/tui/src/status_indicator_widget.rs](/Users/mac/GitHub/codex/codex-rs/tui/src/status_indicator_widget.rs) | 15810 | `e35e3efbd18784b55343366b2001bda08dfc1543f7e4a88700a715562d519cc5` |

参考manifest为 `Docs/learn/stage1_pi_mono/references/codex/source_manifest.tsv`，同根file_learn_index/folder_learn_index；每文件唯一 `Docs/learn/stage1_pi_mono/references/codex/files/<source_path>_learn.md`，每目录 `Docs/learn/stage1_pi_mono/references/codex/<dir>/current_folder_learn.md`。包括所有祖先/root；超256KiB按连续chunks逐个读后合成一文件。这棵树不混入pi source_id或其计数。

- [x] **ZS1-300** — 逐文件复核Codex codex-rs/tui/src/bottom_pane/chat_composer.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 本文件完整交互语义与zenpi映射 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/bottom_pane/chat_composer.rs_learn.md` | Validators: G-FILE；hash/chunks/源测试/操作映射 | Rollback: 仅撤回本文件报告 | Estimate: 374943 B逐文件读，超256KiB分块 | Estimated LOC: 0
- [x] **ZS1-301** — 逐文件复核Codex codex-rs/tui/src/bottom_pane/textarea.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 本文件完整交互语义与zenpi映射 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/bottom_pane/textarea.rs_learn.md` | Validators: G-FILE；hash/chunks/源测试/操作映射 | Rollback: 仅撤回本文件报告 | Estimate: 91601 B逐文件读，超256KiB分块 | Estimated LOC: 0
- [x] **ZS1-302** — 逐文件复核Codex codex-rs/tui/src/bottom_pane/paste_burst.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 本文件完整交互语义与zenpi映射 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/bottom_pane/paste_burst.rs_learn.md` | Validators: G-FILE；hash/chunks/源测试/操作映射 | Rollback: 仅撤回本文件报告 | Estimate: 24594 B逐文件读，超256KiB分块 | Estimated LOC: 0
- [x] **ZS1-303** — 逐文件复核Codex codex-rs/tui/src/bottom_pane/approval_overlay.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 本文件完整交互语义与zenpi映射 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/bottom_pane/approval_overlay.rs_learn.md` | Validators: G-FILE；hash/chunks/源测试/操作映射 | Rollback: 仅撤回本文件报告 | Estimate: 57262 B逐文件读，超256KiB分块 | Estimated LOC: 0
- [x] **ZS1-304** — 逐文件复核Codex codex-rs/tui/src/bottom_pane/pending_thread_approvals.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 本文件完整交互语义与zenpi映射 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/bottom_pane/pending_thread_approvals.rs_learn.md` | Validators: G-FILE；hash/chunks/源测试/操作映射 | Rollback: 仅撤回本文件报告 | Estimate: 4105 B逐文件读，超256KiB分块 | Estimated LOC: 0
- [x] **ZS1-305** — 逐文件复核Codex codex-rs/tui/src/bottom_pane/mod.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 本文件完整交互语义与zenpi映射 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/bottom_pane/mod.rs_learn.md` | Validators: G-FILE；hash/chunks/源测试/操作映射 | Rollback: 仅撤回本文件报告 | Estimate: 72998 B逐文件读，超256KiB分块 | Estimated LOC: 0
- [x] **ZS1-306** — 逐文件复核Codex codex-rs/tui/src/slash_command.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 本文件完整交互语义与zenpi映射 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/slash_command.rs_learn.md` | Validators: G-FILE；hash/chunks/源测试/操作映射 | Rollback: 仅撤回本文件报告 | Estimate: 8020 B逐文件读，超256KiB分块 | Estimated LOC: 0
- [x] **ZS1-307** — 逐文件复核Codex codex-rs/tui/src/file_search.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 本文件完整交互语义与zenpi映射 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/file_search.rs_learn.md` | Validators: G-FILE；hash/chunks/源测试/操作映射 | Rollback: 仅撤回本文件报告 | Estimate: 4009 B逐文件读，超256KiB分块 | Estimated LOC: 0
- [x] **ZS1-308** — 逐文件复核Codex codex-rs/tui/src/key_hint.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 本文件完整交互语义与zenpi映射 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/key_hint.rs_learn.md` | Validators: G-FILE；hash/chunks/源测试/操作映射 | Rollback: 仅撤回本文件报告 | Estimate: 3306 B逐文件读，超256KiB分块 | Estimated LOC: 0
- [x] **ZS1-309** — 逐文件复核Codex codex-rs/tui/src/status_indicator_widget.rs；layer `L1` | Depends: ZS1-001 | Owner scope: 本文件完整交互语义与zenpi映射 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/status_indicator_widget.rs_learn.md` | Validators: G-FILE；hash/chunks/源测试/操作映射 | Rollback: 仅撤回本文件报告 | Estimate: 15810 B逐文件读，超256KiB分块 | Estimated LOC: 0
- [x] **ZS1-350** — 逐目录整合Codex .；layer `L2` | Depends: ZS1-351 | Owner scope: 冻结子集直属文件与直接子目录 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/current_folder_learn.md` | Validators: G-DIR；依赖[x]后独立审阅 | Rollback: 只撤回本目录报告 | Estimate: 逐个整合输入/焦点/取消边界 | Estimated LOC: 0
- [x] **ZS1-351** — 逐目录整合Codex codex-rs；layer `L2` | Depends: ZS1-352 | Owner scope: 冻结子集直属文件与直接子目录 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/codex-rs/current_folder_learn.md` | Validators: G-DIR；依赖[x]后独立审阅 | Rollback: 只撤回本目录报告 | Estimate: 逐个整合输入/焦点/取消边界 | Estimated LOC: 0
- [x] **ZS1-352** — 逐目录整合Codex codex-rs/tui；layer `L2` | Depends: ZS1-353 | Owner scope: 冻结子集直属文件与直接子目录 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/codex-rs/tui/current_folder_learn.md` | Validators: G-DIR；依赖[x]后独立审阅 | Rollback: 只撤回本目录报告 | Estimate: 逐个整合输入/焦点/取消边界 | Estimated LOC: 0
- [x] **ZS1-353** — 逐目录整合Codex codex-rs/tui/src；layer `L2` | Depends: ZS1-306,ZS1-307,ZS1-308,ZS1-309,ZS1-354 | Owner scope: 冻结子集直属文件与直接子目录 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/codex-rs/tui/src/current_folder_learn.md` | Validators: G-DIR；依赖[x]后独立审阅 | Rollback: 只撤回本目录报告 | Estimate: 逐个整合输入/焦点/取消边界 | Estimated LOC: 0
- [x] **ZS1-354** — 逐目录整合Codex codex-rs/tui/src/bottom_pane；layer `L2` | Depends: ZS1-300,ZS1-301,ZS1-302,ZS1-303,ZS1-304,ZS1-305 | Owner scope: 冻结子集直属文件与直接子目录 | Owned paths: `Docs/learn/stage1_pi_mono/references/codex/codex-rs/tui/src/bottom_pane/current_folder_learn.md` | Validators: G-DIR；依赖[x]后独立审阅 | Rollback: 只撤回本目录报告 | Estimate: 逐个整合输入/焦点/取消边界 | Estimated LOC: 0

### UX实现项与真实入口验收

- [x] **ZS1-120** — 项目身份、cwd与持久化owner；layer `L3` | Depends: ZS1-001,ZS1-074,ZS1-076,ZS1-085,ZS1-094 | Owner scope: 本项真实交互与共享owner | Owned paths: `src/project_workspace.rs`, `src/lib.rs`, `tests/project_workspace.rs` | Validators: G-CODE、G-HOST；cargo test --locked --test project_workspace | Rollback: 撤回本项差异并恢复有效项目/布局，不删用户会话 | Estimate: 约2–4天，含正负/取消/重启/PTY | Estimated LOC: 1500

ZS1-120完成判据：稳定ID和规范cwd分离显示名；同路径重用、同basename区分、取消/错误零变更、有界持久化；迁移已有project_metadata且保存BentoBox数据。 负例覆盖无效输入、资源上限和权限失败；取消保留旧有效状态，独立进程重启验证持久化或明确未完成。单元测试不能替代真实入口。

- [x] **ZS1-121** — 顶部+目录picker与真实项目分页接线；layer `L3` | Depends: ZS1-120,ZS1-070 | Owner scope: 本项真实交互与共享owner | Owned paths: `src/tui.rs`, `src/directory_picker.rs`, `src/lib.rs`, `src/core.rs`, `src/config.rs`, `src/session.rs`, `src/project_workspace.rs`, `tests/tui_project_workspace.rs`, `tests/stage1_project_config.rs`, `tools/tui_project_workspace_smoke.py` | Validators: G-CODE、G-HOST；cargo test --locked --test tui_project_workspace --test stage1_project_config；python3 tools/tui_project_workspace_smoke.py --binary target/release/zenpi | Rollback: 撤回本项差异并恢复有效项目/布局，不删用户会话 | Estimate: 约2–4天，含正负/取消/重启/PTY | Estimated LOC: 2900

ZS1-121完成判据：鼠标点击+立即picker，一次确认同时绑定label/cwd/session/tool root；键盘完成同一路径，Esc不留空tab。真实PTY在两个目录分别写文件，验证绝不串写；关闭/切换不丢草稿或布局。 负例覆盖无效输入、资源上限和权限失败；取消保留旧有效状态，独立进程重启验证持久化或明确未完成。单元测试不能替代真实入口。

- [x] **ZS1-122** — 输入编辑、历史、粘贴和可编辑排队消息；layer `L3` | Depends: ZS1-101,ZS1-300,ZS1-301,ZS1-302 | Owner scope: 本项真实交互与共享owner | Owned paths: `src/tui.rs`, `src/input_queue.rs`, `tests/tui_composer.rs`, `tools/tui_composer_smoke.py`, `Cargo.toml`, `Cargo.lock`, `tools/tui_project_workspace_smoke.py` | Validators: G-CODE、G-HOST；cargo test --locked --test tui_composer；python3 tools/tui_composer_smoke.py --binary target/release/zenpi；python3 tools/tui_composer_smoke.py --binary target/release/zenpi --large-paste-only | Rollback: 撤回本项差异并恢复有效项目/布局，不删用户会话 | Estimate: 约2–4天，含正负/取消/重启/PTY | Estimated LOC: 4400

ZS1-122完成判据：多行光标/词级移动删除、UTF8宽字、bracketed paste不误提交、历史检索与草稿恢复；运行时队列可查看编辑撤回；输入焦点和快捷键可发现，不吞键或把粘贴当快捷指令。 负例覆盖无效输入、资源上限和权限失败；取消保留旧有效状态，独立进程重启验证持久化或明确未完成。单元测试不能替代真实入口。

#### ZS1-122 — 大段文本粘贴的折叠、展开和完整保存

本版本将已识别的通用Codex交互差距落实为122交付义务。原122判据继续有效，其它generic gaps仍未豁免。下列模型、十项行为和全部真实PTY场景共同构成完成定义；不是建议项。源对照仍分别归300/301/302，不能据此替代逐文件验收。


保持 TuiState.input 的完整、已按现有规则清洗的 UTF-8 文本，及其canonical字节cursor。新增有界 Vec<PasteFold>，每项 {id, start_byte, end_byte, char_count} 标识 input 中的真实payload区间；next_paste_id 为单调递增、删除后不复用的u64。**不新增第二份paste payload字符串，不把placeholder写入实际提交文本；ZS1-129明确登记的单条有界kill编辑缓冲为唯一新增例外。**

渲染临时生成折叠显示文本及canonical/display端点映射：区间显示 `[Pasted Content N chars] #ID`，ID对所有长度全局单调，避免靠相同label认内容。标签具体文案可本地化；区间ID才是身份。渲染结果、行高、滚动和可见光标使用同一projection。左右/删除等在canonical文本上按fold边界操作。它不是只截短输出：有真实fold身份、原子移动/删除、可显式展开、可恢复的fold元数据。

选择该模型是Zenpi现有String owner的最小适配：提交、InputPort、后台queue、历史、journal继续得到全文；不必扩展wire协议/core/backend/input_queue数据模型。现有tests中的input()也仍返回全文。相比照搬Codex的display-text+payload-map，更容易保证旧版读取草稿时也不会只读到无payload的标签。


1. **创建条件**：真实Event::Paste和ordinary burst确认的Paste批次统一走一个入口；先按现有CRLF/CR→LF和控制字符清洗规则处理，再以Rust chars()的Unicode标量值计数。<=1000直接编辑；>1000创建fold；正常逐键输入再长也不自动折叠。首ASCII hold的Typed单字flush不得变成paste。burst Enter120ms抑制、Windows30/60ms规则保留。
2. **身份与精确内容**：同长度/同内容可同时存在多份，ID唯一且删除不复用。用户手打或小段paste的相同标签始终是普通文字。展开/删除不做字符串contains/replace查找；Unicode、空白、美元、斜线、反引号及末尾LF不得因折叠而改写。清洗之外不做NFC/NFKC等Unicode归一化。
3. **编辑原子性**：左右在fold两端跳转；Backspace/Delete在相邻边界移除对应整段payload及fold，不能残留半个标签。现有词/行删除和替换触及fold时，扩大到该fold完整区间；前后其它内容不丢失。普通插入在边界外进行，后续区间按字节delta重定位。所有输入编辑入口（含Ctrl-U/K/W、词/行移动删除、补全替换、history/set_input）必须经过统一range变更/失效规则，不能漏掉现有直接String::drain/replace_range路径。
4. **明确展开编辑（本合同新增的Zenpi交互，不声称300已有此手势）**：Alt+Enter在光标邻接fold时展开该段，删除fold元数据但保留payload原字节，将光标置于payload开头，随后用现有编辑器直接修改；不提交/不调用provider。若同时邻接两段，优先光标前的一段。旁边的上下文提示显示“Alt-Enter展开”，复用现有footer空间，不改变BentoBox。没有邻接fold时维持原有键语义。本版本明确登记该手势，不能借外部编辑器完成展开。
5. **Unicode与可见光标**：canonical偏移始终UTF-8边界；普通文字移动保留现有grapheme语义。fold边界不能把组合字/ZWJ拆开。若新paste与两边文字组合成跨边界grapheme，保留全文并显示为普通可编辑文本；后续相邻编辑使已有fold边界不再安全时，展开该fold，不能删掉边缘字符。上下/Home/End依照折叠后可见文本映射回canonical边界，不能走进隐藏的多行payload。窄屏resize只改变projection/wrap，不修改payload或折叠状态。
6. **提交与路由**：唯一Submit出口使用canonical全文；不得把显示标签交给slash解析、模型、InputPort、follow-up/steer、scheduled queue或journal。遵守Zenpi现有明确Enter/命令语法，不引入Codex Tab-submit，不无故trim原始缩进和末尾LF。显式提交时才路由；paste产生的CR/内嵌/exit、!shell不自动执行。
7. **历史与失败恢复**：成功的history记录canonical全文，召回可直接展开显示全文（与300提交后history不保留pending-paste相符）。在搜索/浏览历史期间，保存原草稿全文+cursor+folds+nextID，Esc恢复完整原草稿。拒绝提交/队列admission/后台错误需要恢复对应草稿snapshot；保留root restore_rejected_input的新输入优先规则：用户已经有可见文字、ordinary buffer或rejected burst时，不覆盖新输入、不强制flush窗口。不能只回填标签或只回填fold元数据。取消保留当前草稿。
8. **项目与重启**：每项目Draft拥有folds/nextID，不随后台项目响应串台。保持现有 `draft.input` 为完整文本，新增可选 `paste_folds` 与 `next_paste_id`；旧checkpoint无这些字段则全文正常显示，新checkpoint被旧实现忽略这些字段仍能读到全文。新实现恢复时验证范围有序、不重叠、ID唯一、边界有效、计数与实际文本相符；元数据损坏时展开为保存的完整文本并给出可恢复提示，不清空/截断正文，也不猜测label对应payload。
9. **既有预算不变**：canonical全文含所有隐藏payload仍受256KiB限制；normal prefix+全部fold payload一起计算，ordinary临时buffer也计入。禁止通过短标签绕过owner/queued16条/256KiB边界。fold数上界可以由256KiB/(1000+1)=261推出，不需放宽预算或新增宽松上限；projection大小也不超过canonical量级。4MiB项目checkpoint限制不变；新增元数据/写入失败必须显式可见、保持旧已保存文件和完整live草稿，不能把未落盘状态说成已保存。适配时应在大paste admission检查预计checkpoint上限，避免明知无法落盘仍静默接受为可恢复草稿。
10. **popup隔离**：fold内部对文件/命令候选是不可编辑内容，不因cursor映射或按Tab而解析隐藏payload中的@或/。普通token的补全仍可在fold前后工作，并通过canonical区间替换更新fold坐标；明确展开后按正常输入规则完成。保留root的Unicode palette修复与ordinary paste后的明确Tab重开规则。


| 场景 | 必须观察到的结果 |
|---|---|
|1000/1001ASCII与1001CJK、CRLF边界|1000原样，1001显示占位；saved draft.input仍全文，HTTP收到清洗后精确UTF-8/长度/hash；在明确Enter前0请求|
|同长度不同payload+手打同名标签|ID不同；删除一个只影响它；新paste不复用ID；最终HTTP包含其余payload及literal标签本身，没有幽灵展开|
|多个fold前后编辑/补全/窄屏resize|真实Left/Right、Backspace/Delete、Ctrl-U/K/W、词/行操作保持完整范围，CJK/组合字/ZWJ不破碎；1x1→140x40后正文hash不变|
|Alt-Enter展开后修改段内字符|只展开指定段且0请求；在展开文本中改一处，最后HTTP仅该处改变，未丢整段或邻接内容|
|普通CR键流大paste与分段flush|确认burst达到阈值产生fold，普通打字不会；窗口内Enter不提交，明确Enter得到全文；不执行嵌入的/exit或!shell|
|busy enqueue/edit/cancel/retry|门控真实HTTP保持活动请求；新大段完整进入实际InputPort owner，编辑/取消用真实ID；放行后正文完整且不重复，队列byte限制不能靠标签绕过|
|history搜索Esc/项目切换/后台失败|旧草稿snapshot恢复含fold与cursor，已召回历史全文可发送；背景项目回包/拒绝不覆盖当前新的文字或尚在ordinary hold的单字|
|两个独立TUI进程恢复|第一个在未提交fold草稿上SIGTERM正常退出；第二个恢复同一项目draft.input、folds、cursor并保持0自动请求；明确提交得到与原payload相同正文|
|边界/失败/损坏metadata|总canonical超256KiB、预期checkpoint超4MiB以及可控不可写保存失败，不截断正文、不产生provider请求；恢复有效旧状态或明确显示未保存；fold范围重复/越界/非UTF8边界时安全展开保存全文|


实现复用现有122三个路径src/tui.rs、tests/tui_composer.rs、tools/tui_composer_smoke.py；128总回放另行接入。估算原122及本次增量合计4400行，仍严格小于5000；实际预测超过上限时扩版拆项，不能缩减上述行为。

- [x] **ZS1-123** — 命令技能文件补全与模型选择；layer `L3` | Depends: ZS1-107,ZS1-113,ZS1-306,ZS1-307,ZS1-084,ZS1-133 | Owner scope: 本项真实交互与共享owner | Owned paths: `src/tui.rs`, `src/slash.rs`, `src/slash_actions.rs`, `src/core.rs`, `tests/tui_command_palette.rs`, `tools/tui_command_palette_smoke.py`, `src/backend.rs`, `src/headless.rs`, `tests/headless_project_workspace.rs`, `tests/stage1_model_registry.rs`, `tests/stage1_reasoning_owner.rs` | Validators: G-CODE、G-HOST；cargo test --locked --test tui_command_palette --test stage1_reasoning_owner --test stage1_model_registry --test headless_project_workspace；python3 tools/tui_command_palette_smoke.py --binary target/release/zenpi | Rollback: 撤回本项差异并恢复有效项目/布局，不删用户会话 | Estimate: 约2–4天，含正负/取消/重启/PTY | Estimated LOC: 2500

ZS1-123完成判据：slash可筛选菜单、上下/Tab/Enter/Esc和忙闲可用性真实；技能/文件引用保留来源与路径补全，模型/reasoning picker只展示真实能力；错误不丢输入，不使用伪选项。 负例覆盖无效输入、资源上限和权限失败；取消保留旧有效状态，独立进程重启验证持久化或明确未完成。单元测试不能替代真实入口。

- [x] **ZS1-124** — 审批焦点、权限说明与取消体验；layer `L3` | Depends: ZS1-095,ZS1-070,ZS1-303,ZS1-304,ZS1-305 | Owner scope: 本项真实交互与共享owner | Owned paths: `src/tui.rs`, `src/approval.rs`, `src/core.rs`, `tests/tui_approval_focus.rs`, `tools/tui_approval_smoke.py`, `src/headless.rs` | Validators: G-CODE、G-HOST；cargo test --locked --test tui_approval_focus --test approval_owner；python3 tools/tui_approval_smoke.py --binary target/release/zenpi | Rollback: 撤回本项差异并恢复有效项目/布局，不删用户会话 | Estimate: 约2–4天，含正负/取消/重启/PTY | Estimated LOC: 1900

ZS1-124完成判据：审批关联project/call；diff可读且不被fold遮挡，多请求逐一查看；允许/拒绝/取消/超时明确，切项目不能误批其它项目；焦点恢复，审批前无副作用，deny/policy保持有效。 负例覆盖无效输入、资源上限和权限失败；取消保留旧有效状态，独立进程重启验证持久化或明确未完成。单元测试不能替代真实入口。

- [x] **ZS1-125** — 滚动复制状态与终端恢复；layer `L3` | Depends: ZS1-111,ZS1-096,ZS1-308,ZS1-309 | Owner scope: 本项真实交互与共享owner | Owned paths: `src/tui.rs`, `src/render.rs`, `src/view_model.rs`, `tests/tui_transcript_ux.rs`, `tools/tui_transcript_ux_smoke.py` | Validators: G-CODE、G-HOST；cargo test --locked --test tui_transcript_ux；python3 tools/tui_transcript_ux_smoke.py --binary target/release/zenpi | Rollback: 撤回本项差异并恢复有效项目/布局，不删用户会话 | Estimate: 约2–4天，含正负/取消/重启/PTY | Estimated LOC: 2200

ZS1-125完成判据：历史scroll与跟随最新可切换、回到底部可发现；复制答复/选定块，错误和工具细节可展开，context/token/模型/cwd可见；长输出/窄屏/resize无重叠，退出/错误/中断恢复terminal，不改BentoBox结构。 负例覆盖无效输入、资源上限和权限失败；取消保留旧有效状态，独立进程重启验证持久化或明确未完成。单元测试不能替代真实入口。

- [x] **ZS1-126** — headless与TUI共享项目上下文；layer `L3` | Depends: ZS1-120,ZS1-084,ZS1-072,ZS1-070 | Owner scope: 本项真实交互与共享owner | Owned paths: `src/project_workspace.rs`, `src/protocol.rs`, `src/headless.rs`, `src/core.rs`, `tests/headless_project_workspace.rs`, `src/tui.rs`, `tests/tui_project_workspace.rs`, `tools/tui_project_workspace_smoke.py` | Validators: G-CODE、G-HOST；cargo test --locked --test headless_project_workspace --test headless_protocol | Rollback: 撤回本项差异并恢复有效项目/布局，不删用户会话 | Estimate: 约2–4天，含正负/取消/重启/PTY | Estimated LOC: 1800

ZS1-126完成判据：JSONL open/select/list/close与TUI调用同一owner，显式cwd/ID、幂等请求、迟到事件归属、跨项目工具边界、重启恢复；无隐藏daemon，不让显示label成为workspace authority。 负例覆盖无效输入、资源上限和权限失败；取消保留旧有效状态，独立进程重启验证持久化或明确未完成。单元测试不能替代真实入口。

- [x] **ZS1-129** — 项目内删除缓冲、Ctrl-Y恢复与逻辑行编辑；layer `L3` | Depends: ZS1-122,ZS1-300,ZS1-301,ZS1-302,ZS1-099 | Owner scope: 本项真实输入编辑及共享PTY清空适配 | Owned paths: `src/tui.rs`, `tests/tui_composer.rs`, `tools/tui_composer_smoke.py`, `tools/tui_project_workspace_smoke.py`, `tools/tui_command_palette_smoke.py`, `tools/tui_approval_smoke.py`, `vendor/crossterm/src/event/source/unix/mio.rs` | Validators: G-CODE、G-HOST；cargo test --locked --test tui_composer --test tui_bentobox --test layout_persistence；python3 tools/tui_composer_smoke.py --binary target/release/zenpi --kill-yank-only；python3 tools/tui_composer_smoke.py --binary target/release/zenpi --queued-shell-paste-only | Rollback: 仅撤回本项差异，恢复既有输入操作；不删除项目草稿或会话 | Estimate: 约1–2天，逐路径实现并真实PTY/HTTP验收 | Estimated LOC: 1000

本版本将已识别的Codex编辑差距独立登记，避免把122新增预测推过每项5000行上限。122全部要求继续有效。下列行为与真实验证是交付义务；源300/301/302仍须各自完整接受，不能用此产品项代替。

本项明确允许一个有界编辑例外：每项目最近非空kill的完整String可与yank后的可见草稿同时存在；它不是第二份paste payload索引。其余122的canonical单一权威、fold身份、队列和checkpoint预算均不变。最多64项目、每buffer最多256KiB，理论额外上界16MiB；实际RSS仍须通过既有预算。

1. **单条缓冲，不是kill ring。** 每个打开项目一个有界kill_buffer String，只保存最近一次非空kill的完整canonical字节。连续kill替换前值，不拼接、不轮换；空kill保留前值。普通字符Backspace/Delete、粘贴、补全、set_input、程序性提交清空、历史导航、错误恢复和队列事件不改它。Ctrl-W/Ctrl-Backspace与Ctrl-Delete这些既有词删除入口纳入kill；不新增其它Codex快捷键。
2. **Ctrl-U对齐逻辑行首。** 当前逻辑行按现有fold projection解析，再映射到canonical范围。Ctrl-U删除当前逻辑行首到cursor；若已在非首行行首，则删除前一个LF；首行行首为空kill。Ctrl-K保留当前逻辑行尾/LF行为，词边界继续使用现有grapheme/空白规则。完整fold相交仍扩大为原子范围，不能深入隐藏行。明确更新旧“整个草稿前缀”帮助及测试；共享PTY工具提供明确清空整个草稿的操作，不能再假定一次Ctrl-U清空多行。实际验证普通多行、行首LF、fold及软换行。
3. **统一范围决议后再存被删文本。** 先把相交fold扩展到完整canonical区间，确认UTF8/grapheme边界和操作有效，再提取该实际删除范围并执行现有统一replace入口。不能先截取可见label或未扩展的词范围。被删fold的metadata随原子删除失效；附近仍存在fold按既有规则移位。不能把跨界组合符或ZWJ拆掉。
4. **Ctrl-Y是纯编辑。** 仅在普通composer焦点生效，在cursor插入buffer完整文本；重复Ctrl-Y可重复插入，不消费buffer。不提交、不执行/exit或!shell、不自动enqueue、不触发资源扩展。无buffer则无操作。先遵守现有ordinary-paste modified-key flush顺序，不把插入内容送入paste检测器；不重启或扩大120ms Enter窗口。现有picker/approval/inspector/history-search焦点优先级不变，原始粘贴的Ctrl-Y控制字节不执行快捷键。
5. **yank展开为普通全文。** 从fold kill的内容以完整可编辑文本插入，不重新创建fold、不复用旧fold ID；literal label仍是literal。已有其它fold只按统一range修改移位/安全展开。这与原大paste必须有fold的创建条件不冲突：yank是独立明确编辑操作，不是Event::Paste/ordinary burst。本项不重新折叠，插入后的文本必须可直接编辑。
6. **生命周期清楚。** 同一项目中buffer跨set_input、历史预览/取消、成功发送、slash dispatch、异步失败恢复保留；历史snapshot和SubmittedInput不得把旧buffer快照覆盖到用户的新kill上。项目切换交换各自buffer，新项目为空，关闭项目丢弃其buffer。buffer仅进程内有效，不写checkpoint/journal/队列/系统剪贴板；独立进程启动buffer为空，已经yank到可见draft里的全文按原草稿规则保存。本版本明确登记“ephemeral”，不能把重启后的空buffer当作未完成故障。
7. **预算与拒绝原子性。** 每项目buffer≤现有单条输入256KiB，64个项目的理论新增驻留上界为16MiB（不是已测RSS）；没有多条ring、无界map或新owner。yank后完整input仍≤256KiB，预计pretty project checkpoint仍≤4MiB；超过任一界限时明确提示、保留旧draft/cursor/folds/buffer且0HTTP，不能截断插入。buffer不落盘，所以不占checkpoint字段，但插入后的可见draft仍须按同一个真实pretty serializer验容量。执行删除前必须能保留完整kill；不得以缓冲不足为由先删除再截断保存。
8. **与队列身份完全分开。** 最新50372e…中accepted scheduled ID/job/ticket持有自己的轻量paste metadata，yank buffer不能放进这些回执，也不能随其它项目的后台完成改变。Ctrl-Y只编辑当前项目draft，之后明确Enter才把完整结果按现有owner预算提交；取消/编辑已接受队列不写kill buffer。保证上一轮已修复的同时相同正文身份不回退。
9. **可发现且不改布局。** 在现有输入帮助/footer提供简短“Ctrl-U 行首删除 · Ctrl-Y 恢复删除”或准确的本地化说明，注明单条、项目内、进程内与预算；复用现有footer空间，不动BentoBox布局，不引入新对话框或新应用级clipboard功能。


必须执行以下验证：

- 单条覆盖/空kill保留/重复yank；普通单字删除不覆盖buffer；Ctrl-U逻辑行首/LF、Ctrl-K行尾LF、两个方向词删除；插入点cursor正确。
- CJK/组合字/ZWJ，fold前后同名literal label，原子kill得到全部canonical payload；yank普通全文、0旧ID复活、邻接fold保持正确；1列到140列resize不改正文。
- kill→成功submit→yank、kill→slash→yank、history search Esc、A/B项目切换、同项目async rejection与更新kill并存、窗口内held ASCII→Ctrl-Y处理；重点保留新输入优先规则。
- 实际busy InputPort/scheduled enqueue/edit/cancel的结果与Ctrl-Y draft独立；按真实ID操作相同正文，既有queued-shell身份7组仍通过；明确Enter后真实HTTP收到完整字节/hash，只发送一次。
- 256KiB和预计4MiB拒绝，未写入时旧有效草稿/kill buffer不损坏；2独立TUI验证ephemeral buffer为空但已yankdraft持久化；按键操作不访问系统clipboard、不绕过approval/picker焦点。


- [x] **ZS1-131** — 固定终端依赖与可验证的输入读取器重置；layer `L3` | Depends: ZS1-097,ZS1-098,ZS1-099 | Owner scope: crossterm固定原版逐文件库存、reader reset API、mio就绪消费修复和独立真实PTY验证 | Owned paths: `Cargo.toml`, `Cargo.lock`, `vendor/crossterm`, `tools/stage1_reader_reset_probe.py` | Validators: G-CODE、G-PROD；cargo check --locked；python3 tools/stage1_reader_reset_probe.py --report-dir PRIVATE_NEW_DIR；vendor内部WouldBlock测试；现有生产依赖数量/feature/体积/启动/RSS门禁 | Rollback: 撤回本项Cargo patch及vendor/helper新增文件；依赖此API的130调用同时回退，保留草稿和会话 | Estimate: 固定库存后逐文件核对，reset补丁和真实PTY独立复验 | Estimated LOC: 1100

131固定行为要求：

1. 固定crossterm 0.29.0上游.crate SHA256 `d8b9f2e4c67f833b660cdb0a3523065869fb35570177239812ed4c905aeff87b`，完整77个归档文件按相对路径/bytes/SHA逐文件核对并保留license；Cargo本地安装标记.cargo-ok不进入vendor。写清原始和最终文件/目录库存、允许的src/event.rs重置差异及src/event/source/unix/mio.rs就绪消费差异，各自逐文件核对及回滚；不修改全局Cargo registry。完整第三方原始字节作为可复验依赖基线，不计新增手写LOC，也不冒称第三方全仓语义学习完成。定制代码、测试和helper全部计增量，接近4500必须拆项。
2. 通过根Cargo的本地patch固定唯一crossterm实现，保留原版本和现有features/其它依赖。API仅在Unix且未启用event-stream时可用：全局reader try_lock失败立即返回WouldBlock；成功在锁内take/drop旧reader，清空已解码、skipped及未完成ANSI/UTF8/paste状态，下一次poll/read重新建立输入source。不得使用私有内存布局、TIOCSTI、替换stdin或只用tcflush声称清空用户态parser。
3. 调用方必须已经停止全部read/poll/光标查询且没有并发EventStream；try_lock不是未来并发的排他协议。130在交出终端前及收回后、恢复输入前各重置一次，并分别检查OS输入flush错误。重置或终端恢复失败必须走已有有界退出/join路径，不继续接受输入。运行中重新初始化失败须作为可见错误处理。
4. 独立helper创建私有隔离构建和真实PTY，以相同初始/后续输入对比原版与候选：已解码CR、半截bracketed paste、CSI和UTF8；证明reset后不会跨交接拼接，额外不drain清CR。另至少100次reset/reinit核对FD基线/峰值/末值、每次SIGWINCH的Resize及独立SIGTERM处理器保持。锁已持有时的WouldBlock与释放后成功通过实际内部测试。原失败、实际argv/cwd、输出/二进制/依赖hash、退出码和时间保留；夹具输入或路径修正单列，不重写历史。
5. 131依赖实验不替代130产品验收；Ctrl-G真正编辑器、原草稿/项目身份、无意外Submit、前台PGID、termios/信号/后台owner和BentoBox恢复仍按130逐项验证。主控当前实际平台macOS ARM，Linux代码/测试保留且未运行须明示；不宣称Windows支持。最终同一生产构建重跑原有预算和交互回归，不增加第三个产品模式或运行时依赖。

- [x] **ZS1-130** — 外部编辑器完整草稿往返与终端交接；layer `L3` | Depends: ZS1-122,ZS1-129,ZS1-123,ZS1-124,ZS1-126,ZS1-300,ZS1-305,ZS1-131 | Owner scope: 单个Unix前台编辑器、完整输入往返及现有两种TUI host | Owned paths: `src/config.rs`, `src/external_editor.rs`, `src/lib.rs`, `src/tui.rs`, `tests/config.rs`, `tests/tui_composer.rs`, `tools/tui_external_editor_smoke.py` | Validators: G-CODE、G-HOST、G-PROD；cargo test --locked --lib --test config --test tui_composer --test tui_bentobox --test layout_persistence；python3 tools/tui_external_editor_smoke.py --binary target/release/zenpi | Rollback: 仅撤回本项代码差异，恢复原输入/终端逻辑，保留已有会话和有效草稿 | Estimate: 约3–5天，逐路径实现、前台PGID与真实PTY/HTTP/恢复验收 | Estimated LOC: 3300

本版本登记已识别的外部编辑器通用UX差距。下列条款是行为要求，不是可选建议；主控采用的明确选择覆盖候选包中的待定措辞。其它通用UX差距、122/129要求、BentoBox结构和既有资源门禁均保留。新增私有模块与PTY工具是本产品owned paths；不增加第3节独立源文件/目录的冻结覆盖数字。实现+测试接近4500 LOC时先拆项，严禁压缩或删断言伪装低于5000。

1. **明确入口和焦点**：普通composer Ctrl-G，帮助显示“External editor · save and close to return”。与129一致先完成已有ordinary-paste modified-key flush，随后捕获完整canonical草稿；不扩大Enter抑制窗口。目录/审批/history search/来源查看器/model与slash/file popup有焦点时不启动、不排队请求；关闭popup后可明确重按。全TUI最多一个编辑器session。已有模型任务可以继续；禁止把本项降成仅idle可用而不登记差异。

2. **编辑器选择**：使用启动TUI时捕获的用户环境VISUAL，其缺失才回退EDITOR；二者皆无时清楚提示，不猜vi、不自动安装。已设置但空白、非UTF8、语法错误的VISUAL报错，不能悄悄改用EDITOR。暂不向provider ConfigFile/EffectiveConfig加入editor字段，不读取工作区内“编辑器配置”，不把provider profile或模型输出作为可执行程序来源。`src/config.rs`只新增纯map/OsString输入的editor解析函数和来源标记，可确定性测试。GUI启动器需要用户在变量中显式设置等待参数，如`code --wait`，不自动猜测/追加参数。

3. **argv合同**：变量最多4096 UTF8字节、最多32个argv项、单项最多2048字节；拒绝NUL/CR/LF及Tab以外控制字符；空格/Tab在引号外作分隔符，引号内按下述语法保留。采用明确的小型POSIX式语法：单引号全字面，双引号内只对双引号、反斜线、美元、反引号处理反斜线转义，其余反斜线保留；引号外反斜线转义下一字符；相邻片段拼成一个参数，允许空参数但程序argv[0]不得为空；不完整引号/尾反斜线拒绝。不执行`$VAR`、`~`、glob、`$(...)`、反引号、`;`、`|`、重定向。直接Command(program).args(args).arg(absolute_temp_path)，不隐式`sh -c`；用户显式把shell设为编辑器是用户选定程序，不等于自动shell展开。临时路径只追加一次，不做占位符替换，正文从不进入argv。命令名按用户捕获PATH解析；带目录的相对程序路径相对启动项目canonical cwd解析，子进程cwd保持该origin，不随后台项目变化。解析错误仅显示有界错误/配置来源，不回显完整可能含私密参数的命令。

4. **子进程环境**：本项是用户显式发起的本地编辑器，不能套用模型run_command的审批/输出capture流程。独立allowlist仅传递启动时捕获的PATH、HOME、USER、LOGNAME、SHELL、TERM、COLORTERM、LANG、TMPDIR、TERMINFO、TERMINFO_DIRS、DISPLAY、WAYLAND_DISPLAY、XDG_RUNTIME_DIR、XDG_CONFIG_HOME、XDG_DATA_HOME、XDG_CACHE_HOME、XDG_STATE_HOME、XDG_CONFIG_DIRS、XDG_DATA_DIRS及LC_ALL/CTYPE/NUMERIC/TIME/COLLATE/MONETARY/MESSAGES/PAPER/NAME/ADDRESS/TELEPHONE/MEASUREMENT/IDENTIFICATION对应的LC_变量；每值最多4096 bytes、合计最多64KiB，超限在交接前明确拒绝；不转发ZENPI_/OPENAI_/其它provider credential环境变量或secret handles。只给子进程传原值，不更改父进程HOME/CODEX_HOME。具体平台环境需作为登记表固定，不在实现中无限扩大。

5. **私有文件与有界读取**：使用宿主可信temp base下独占创建的0700目录和0600 `draft.md`；不用工作区固定文件、不把tempfile从dev-dependency升级成第16个生产依赖。目录名有进程/唯一nonce并以原子创建判冲突，失败有限重试；seed仅为≤256KiB完整canonical文字，不含fold label替换、图片合成或工具模板扩展。写入完成后关闭句柄再启动，兼容编辑器原子rename保存。读回重新以NOFOLLOW打开、fstat确认普通文件/当前用户/私有权限/合理link数；不能要求inode不变，否则合法原子保存会被误拒。拒绝丢失、symlink、目录、FIFO、设备、无权限文件、无效UTF8；实际读取最多256KiB+1，超限拒绝，不做先无界read_to_string再截断。编辑中可检查文件体积并中止超限任务，但这不是磁盘配额：任意本地编辑器可能在轮询前短暂写出更大文件，不能声称保证外部进程每次write都≤256KiB。

6. **终端独占交接**：新增可恢复suspend/resume状态，和最终leave分开；保留SignalGuard最终退出责任。成功完成预检和文件写入后才暂停Zenpi的stdin poll/read与所有terminal draw，关闭mouse/bracketed paste、显示cursor、退出alt screen，并恢复编辑器可用的termios。后台owner、结果收取、checkpoint、取消和shutdown继续按原有界轮询运行；不能同步wait阻塞整个host，也不能让另一个线程继续抢stdin。返回先回收前台终端所有权，再恢复raw/alt/mouse/bracketed/cursor及尺寸，invalidate整个Ratatui缓冲并强制重画，最后恢复输入。需清掉交接期间遗留的终端输入，尤其编辑器退出键后跟随的CR，保证不会提交草稿。Unix tcflush不能替代清理crossterm已经预读的内部事件；实现前须验证当前crossterm后端的预读行为并把它列为实际PTY门禁。

7. **进程、信号、取消失败**：Unix子进程单独PGID，并将控制终端前台组交给它；记录旧PGID，所有分支还原，避免Ctrl-C同时杀掉Zenpi。处理初始SIGTTIN与父进程后台tcsetpgrp的SIGTTOU竞态；不能用会阻塞Command::spawn错误管道的pre_exec握手。主loop轮询try_wait/停止状态；编辑器正常退出0才有资格读回，非0/信号/停止/启动失败均取消本次回填。Ctrl-C在编辑器内先服从编辑器自身语义，不能承诺每个编辑器都立即退出；若它退出为信号/失败，则回到旧草稿。SIGTERM/HUP/QUIT给父进程时必须中止编辑器并完成原有shutdown。用户编辑deadline默认1800秒，可用全局`ZENPI_EDITOR_TIMEOUT_SECONDS`在1–7200秒选择；不借provider/tool timeout。取消先TERM自身编辑器组，250ms后仍存活则KILL，再有限等待/reap，失败明确报告；不能kill整个用户shell前台组。Ctrl-Z停止的编辑器按取消处理并清理，不引入新的后台job UI。普通leader+继承同组子进程均要检查收束，不能只看leader exit就宣布清理完成；自行setsid/daemonize的编辑器服务不能冒称已被PGID回收，必须要求可等待的前台客户端并明确该所有权边界。

8. **平台及证据范围明确。** 本项实现macOS/Linux Unix前台PTY编辑器，主控当前可执行验收是macOS ARM；Linux须保留同一Unix实现及可执行测试，未实际运行的平台记录为未验证，不能用macOS日志冒充。non-Unix入口明确报告当前平台不可用，Windows console/进程树差距仍须留在通用UX矩阵，不借本项宣称全平台1:1。完整源目录范围不因这些辅助平台观察扩大。

9. **身份和新草稿优先**：session含单调ExternalEditId、origin项目稳定ID/canonical cwd、session/owner generation与draft edit revision；hash仅验证字节，不能当任务身份。origin草稿留在现有TuiState/ProjectDraft中，进入编辑器不先清空、不进history/queue；因此失败或取消无需拿旧snapshot覆盖新输入。所有内容/历史回填/拒绝恢复/项目切换相关直接input赋值必须审计revision更新；cursor/fold被其它合法事件改变也应使旧结果不可覆盖。返回仅在operation token仍有效、origin owner/session仍匹配且draft revision未变时应用；origin已关闭/重开、会话已换、active项目改变或后台错误恢复了新草稿时，丢弃本次结果并明确“未应用，当前草稿保留”，不能写到另一个项目、旧history或新的buffer。pending项目切换/会话更换尚未commit时拒绝启动，避免两种事务竞争。UI在编辑器前台不处理项目切换键；独立owner变化仍必须由返回身份检查防住。

10. **回填规则**：不照搬Codex的trim_end；保留尾部空格与LF，不做Unicode归一化。按现有统一文本清洗规则处理CRLF/CR、Tab与控制字符，原始读取和清洗后的文本都≤256KiB；非法UTF8直接拒绝。若清洗后与launch时仍有效的canonical文本完全相同，作为no-op保留cursor/folds/nextID；若修改（包括合法空文件），作为普通全文回填、folds清空、cursor置末尾、nextID不倒退，129 kill buffer保持当前值。再次按129共用的真实pretty serializer检查预计checkpoint≤4MiB，拒绝时保持草稿/光标/folds/kill buffer和已有checkpoint不变。成功仍只有编辑动作：0 Submit、0 provider扩展、0shell、0队列入队、0历史追加；之后独立明确Enter才走现有完整canonical提交。文件内容中的`/exit`、`!…`、同名label和CR都不能成为快捷键。

11. **清理和可见失败**：成功/取消/失败只清理自己独占目录内的文件，不跟随symlink或越界递归；editor swap/backup清理最多256 entries、深度4；独占目录创建最多16次有限尝试，超出或不可删除则明确报告保留的私有路径，不能谎报已清理。原始有效草稿已经按既有checkpoint规则保存；未通过身份/容量/内容检查的编辑结果不应用，错误明确说明本次外部修改未接纳。此最小合同不引入持久“编辑器恢复草稿”或结果仓库；若主控要保留拒绝结果供再次应用，必须登记一个独立有界恢复生命周期，不能默默堆积文件。恢复terminal失败属于可见的host失败：恢复能恢复的模式、结束并join所有现有runner、输出有限stderr，不继续在半恢复终端消费输入。


主控补充固定：临时文件读取需O_NOFOLLOW/O_NONBLOCK，fstat先确认普通文件、当前uid、无group/other权限且nlink=1，再有界读取；FIFO等不得因open先阻塞。正常同私有目录原子rename保存可接受。进程PGID只拥有前台client及同组后代，不宣称收回自行setsid的服务。取消TERM后250ms KILL，后续reap等待最多2s；未收束明确失败，恢复终端后原有runner仍需关闭join。未接纳结果按有界清理策略删除，清理失败显示残留私有路径，不增加结果仓库。外部会话元数据不得长期复制canonical正文；最多一个临时读回String及校验所需短暂候选，仍按256KiB/4MiB和RSS门禁检查。

必须执行的真实验证（尚未执行）：

测试editor必须是实际独立可执行程序，继承真实PTY；记录PID/PGID/cwd/argv、isatty、termios、输入seed字节/hash、文件mode及实际退出码。不能只用mock closure回填字符串、os.system返回0或录制屏幕当作子进程证据。

| 组 | 正例与负例、必须留存的实际证据 |
|---|---|
|选择/参数|VISUAL优先、EDITOR回退、带空格Unicode程序路径、quoted/空参数、显式--wait；missing/空VISUAL/非UTF8/未闭引号/过长/不存在程序/无执行权限均不改变草稿和terminal；`$(touch sentinel)`、`;`作为字面argv，不创建sentinel|
|完整往返|>1000字符多fold、同名literal label、Unicode组合字/ZWJ、尾部空格/LF全部进入editor文件；真实editor修改中段后保存退出0；返回普通全文，fold ID不复用，kill buffer仍可Y；未改文件退出0保留原fold/cursor；空文件明确清空|
|实际提交|editor返回含`/exit\n!touch…`文本不执行、不HTTP；原始退出输入中跟随CR不能误提交；随后单独明确Enter，localHTTP逐字节/长度/hash比对一次请求；不能把background本来已有HTTP算作editor误提交或忽略新增请求|
|终端交接|子进程isatty为真、前台PGID正确、canonical/raw与echo达到合同，Zenpi无并发读键/画面；editor实际读键保存，resize 1列→140列、退出后TUI键继续有效，alt/mouse/paste/cursor恢复；特测crossterm预读与退出残留CR|
|busy/身份|门控真实provider持续流，Ctrl-G中job成功/失败/队列回执不干扰editor；空seed时旧job失败回填新草稿，外部结果被明确拒绝且新草稿/新kill保留；实际origin/session变化或关闭重开必须拒旧EditId；scheduled相同文本身份7组继续通过|
|modal|approval/history/picker/source inspector/model与slash/file popup Ctrl-G不产生子进程；正在editor时无第二session；pending项目切换拒绝启动；返回后审批仍需真实显式决定|
|取消/进程|editor退出非0、signal退出、Ctrl-Z停止、超时、spawn失败均原草稿完整；父SIGTERM/HUP时编辑器组与普通后代被回收/reap、无泄漏runner；TERM忽略者实际KILL验证。自行setsid的fixture必须揭示所有权边界，不能虚报已回收；该平台要求由主控先固定|
|文件边界|editor真实产出256KiB和256KiB+1、非法UTF8、symlink目标、FIFO、目录、删除/权限失败；正规atomic rename保存应被接受。近4MiB checkpoint下拒绝而旧文件hash不变，草稿/folds/kill buffer完整。对live oversized file只能报告检测/取消结果，不能称为文件系统硬配额|
|重启/清理|第一个TUI已完成回填后正常SIGTERM，第二个独立进程恢复可见全文且不自动发送；editor期间终止恢复原草稿、不偷偷应用半成品；成功和失败的已知temp路径/进程消失，有界清理失败明确可见且不跟随外部symlink|
|旧门禁|129 29组、queued7、original15、large14、burst7、project/approval/menu及BentoBox/layout回归；实际release≤8MiB、direct deps≤15、原cold/RSS/runtime/render门禁。主控当前macOS真实TTY结果；Linux保持可执行同义测试，未运行平台明确未验证，不冒充全平台通过|


先用当前冻结129生产release记录真实Ctrl-G未启动反例，再以相同fixture验证新实现。

- [x] **ZS1-132** — 同项目 /new 新会话及原子切换恢复；layer `L3` | Depends: ZS1-070,ZS1-074,ZS1-079,ZS1-084,ZS1-085,ZS1-121,ZS1-123,ZS1-133 | Owner scope: 同项目新journal创建、共享owner提交、草稿及请求身份恢复 | Owned paths: `src/slash.rs`, `src/session.rs`, `src/core.rs`, `src/project_workspace.rs`, `src/headless.rs`, `src/tui.rs`, `tests/session_new.rs`, `tests/headless_project_workspace.rs`, `tools/tui_session_new_smoke.py`, `Docs/quality/stage1/ux-reference.md`, `Docs/quality/stage1/ux-acceptance.md` | Validators: G-CODE、G-HOST；cargo test --locked --test session_new --test headless_project_workspace；python3 tools/tui_session_new_smoke.py --binary target/release/zenpi；既有session resume/owner/layout直接回归及全局预算 | Rollback: 撤回本项代码与命令注册，保留已创建journal、旧会话及项目草稿；必要时通过已有session open恢复有效映射 | Estimate: 逐文件复用已有prepare/commit/replace owner，增量接近4500行先拆项 | Estimated LOC: 3900

132固定行为合同（现有通用Codex UX缺口的明确实施项，不豁免其它generic gaps）：

1. 唯一新增公开命令为无参数 `/new`，进入已有slash目录、菜单及help。TUI和现有JSONL `command` envelope的text `/new` 共用 `SessionAction::New` 与当前project session owner。不新增顶层协议消息、并行Agent/session owner或新项目分页。现有 `/clear`、`/session open`、顶部+直接选择工作目录的语义保留。固定Codex slash参考证明New命令及busy不可用；以下持久化与偏好规则为Zenpi明确合同，不冒称参考文件定义了其底层实现。
2. 成功返回同一project_id、canonical cwd、old/new session_id和journal路径。project tab数量/ID/名称、BentoBox布局/焦点/折叠不变；同一个已有Agent owner切换session并更新InputPort。新journal身份独立、初始会话树/turn历史为空；不复制旧消息、summary、工具结果或队列。创建自身零provider HTTP、零工具执行。旧路径可通过已有session open恢复，旧journal不截断、不删除、不复制正文，原记录必须保持精确前缀。必要的有界生命周期/请求审计元数据可以追加，但须列明类型并验证不改写原记录；/new不得成为普通用户turn。
3. 只在当前项目idle且工作已收束时接受。复用已有Running/Closed、队列、unknown outcome/recovery及staged attachment检查，TUI另检查当前项目pending tickets、scheduled/future/interrupted work、审批、外部编辑器、compact/reload/其它owner操作。明确拒绝并保留旧request/票据/状态与可恢复草稿；不自动cancel或清队列。其它项目独立工作继续。处理旧工作后可重试同一命令的新请求。
4. 未发送草稿保持项目级语义：同项目切换保留draft/cursor/paste folds/kill buffer，绝不自动提交。用户已明确提交的/new控制文本正常消费，不写作新用户turn；不承诺恢复已被用户手动覆盖的旧草稿。退出TUI保存的项目草稿在JSONL /new后独立TUI重启仍保留。转录必须根据新session身份重投影，不能因为旧项目缓存messages非空而沿用旧历史。
5. 延续用户已选model/effort/persona，通过已有model_selected/persona_selected格式写入新journal，使首次创建与独立重启一致。configured approval/tool限制继续按原作用域生效：同进程保留configured policy，重启按原配置加载，不将process-only的never/YOLO新增为持久授权；配置Deny及worker限制不放松；旧session记住的human Allow/Deny不复制给新session。不得用空journal resume的默认model覆盖已选偏好，也不得只在内存保留偏好。
6. 新journal在当前durable journal已校验的同一父目录内部命名，不接受用户destination参数。使用create_new独占、明确canonical cwd、0600/no-follow、有限碰撞重试；不能通过会复用现存路径的普通open或复制旧turn的fork代替创建。候选header/偏好seed落盘，资源/model/extensions准备和取消检查完成后，原子提交project→new session映射；该owner checkpoint为唯一提交点，之后再替换live session/撤销旧InputPort/刷新UI。复用已有prepare→commit→replace私有能力，不另建持久化系统。
7. 准备阶段通过现有可取消worker方式保持UI可读，暂存prepared session而非第二Agent。commit前Ctrl-C或JSONL cancel保留旧owner/journal/映射/draft/layout；只清理本次独占创建且身份仍吻合的候选文件，外来替换文件不删除，无法清理须报告路径。commit后取消不能回滚成再次切换，应报告已完成或可恢复的已提交结果。独立进程在commit前/后终止并重启，分别恢复旧/新身份。创建失败不得把新session映射写一半或使旧session无法继续请求。
8. 沿用JSONL request ID/replay：重复同/new请求（含独立进程恢复）返回同一个新session结果，不增加journal或HTTP；相同ID不同payload报冲突并保留映射。旧InputPort/request scope/late reply不能投递进新session。映射已提交但terminal response/replay初始化失败时，保留可恢复transition receipt并如实呈现已创建状态，不能报告“未创建”或再次创建另一个session。

132真实验证固定：先在当前固定生产二进制观察/new未实现，再用同一fixture验证候选。源/目录验收独立，不以此替代070等逐文件理解。

| 场景 | 必须观察到的真实结果 |
|---|---|
| TUI新会话 | 旧会话真实HTTP一轮、Unicode/空格cwd及非默认布局；菜单/new明确Enter后项目/tab/cwd/layout不变，新身份空history，创建零HTTP；随后实际prompt/工具只含新会话上下文，工具仍作用于同cwd |
| 双向恢复 | TUI创建后独立JSONL/TUI恢复新映射；session open旧路径恢复原历史，旧journal原记录前缀不被/new改写，必要有界审计追加单列；再打开新路径恢复新会话 |
| 草稿偏好 | TUI保存Unicode/fold草稿与model/effort/persona后退出；JSONL /new；独立TUI恢复原项目草稿/folds/layout及新session转录，偏好与journal一致，零自动提交 |
| busy与未完成状态 | 真实门控provider/待审批、已接收队列或scheduled work、staged attachment及unknown outcome分别拒绝/new，保留旧request/票据/草稿/journal；旧工作收束后可成功 |
| 文件与提交失败 | 实际目录权限失败、候选碰撞/符号链接/非普通文件替换、第二headless writer抢先提交；不替换旧owner或覆盖外来文件，旧session仍可接下一真实请求；确定性命名hook只控制碰撞条件，不伪造文件结果 |
| 取消与进程重启 | 可控实际extension初始化或文件准备阶段，Ctrl-C/JSONL cancel在commit前生效；旧session下一请求可用；独立进程在commit前/后终止再重启分别恢复旧/新；未到达取消前置条件不得算通过 |
| 重放与旧身份 | 同JSONL ID及独立进程恢复重复/new得到同新身份、文件数量不增加；payload冲突明确；旧InputPort/请求scope不能进入新会话 |
| 直接回归 | 既有session resume/owner/layout与本项直接触达套件通过，顶部+、BentoBox契约保留；不为本项新增项目tab或放宽现有预算，未实测平台明确列为未验证 |


- [x] **ZS1-128** — Codex交互矩阵与BentoBox完整回归；layer `L3` | Depends: ZS1-117,ZS1-121,ZS1-122,ZS1-123,ZS1-124,ZS1-125,ZS1-126,ZS1-350,ZS1-129,ZS1-130,ZS1-132 | Owner scope: 本项真实交互与共享owner | Owned paths: `Docs/quality/stage1/ux-reference.md`, `Docs/quality/stage1/ux-acceptance.md`, `tools/stage1_host_smoke.py`, `tests/tui_bentobox.rs`, `tests/layout_persistence.rs` | Validators: G-CODE、G-HOST；G-PROD；cargo test --locked --test tui_bentobox --test layout_persistence | Rollback: 撤回本项差异并恢复有效项目/布局，不删用户会话 | Estimate: 约2–4天，含正负/取消/重启/PTY | Estimated LOC: 900

ZS1-128完成判据：逐个Codex交互条目映射实际操作与证据；每实现项先做自己的PTY/JSONL，再此综合验收。实测+到有效项目最短路径及原BentoBox resize/focus/collapse/persistence全部回归；零mock button和未接线成功。 负例覆盖无效输入、资源上限和权限失败；取消保留旧有效状态，独立进程重启验证持久化或明确未完成。单元测试不能替代真实入口。

新增测试/命令/smoke文件由对应项先实现再执行。每个功能完成就执行适用的真实TUI/headless入口；ZS1-117/128承担最终综合回归，不将所有用户体验问题推迟到末尾。

当前默认Rust toolchain架构配置不适用，使用已安装 `cargo +stable-aarch64-apple-darwin ...` 等价替换文中cargo前缀，不修改用户全局默认。

主控负责基线、唯一要求、manifest、冲突集成和master接受。用户明确保留当前dirty功能，通用skill的force-sync/stash/push模板由包含working-tree的隔离worktree及基线hash差异集成替代；不自动推送远端或修改现有技能安装。


### 3.1.21 输入交付与项目检查补充

实际故障要求新增两个独立目标文件及其七个目录祖先/容器，清单为121项、58文件、32目录。原56个文件的冻结字节与所有既有义务保留，新增文件冻结的是主库修复前现状，不把原始第三方77文件库存冒称全仓语义学习。新目录必须一个个独立验收；分母增加不改变已有28项接受。

129/131的正常读取期间必须保留同批poll未消费的TTY/SIGNAL/WAKE readiness，以及单TTY事件下超过1024字节的未读内容。已解码事件优先；UTF8/CSI/bracketed-paste部分状态不能因resize被清空。不能增加额外唤醒键、放宽12秒断言、等待未来按键、在每次resize调用reset或改变共享stdin的blocking/termios设置来掩盖停顿。正常WouldBlock/timeout、EOF和读错误须有界处理，event-stream配置的wake及其它token顺序也须独立检查。reset跨外部编辑器交接的原有合同保持。真实PTY必须完成原Unicode/fold/1x1→140x40/Ctrl-U/Enter/HTTP全文场景，loopback测试的代理隔离仅限子进程环境且原版/修复版相同；保留ambient环境失败。

123/132的实际本地diff及异步响应必须绑定请求准入时的项目cwd、session与稳定ID。已有busy/diff私有反例须进入tests/headless_project_workspace.rs长期回归：process cwd A、选B且provider首delta后仍门控时读取B内容，切换或/new后身份不串位，cached旧响应仍属于原项目，foreign guard/路径拒绝/取消和正常关闭保留。Runtime QueueFull与Resume摘要等v2响应也不得省略或改写所属项目；历史事件保留历史项目身份。以上新增测试由完整逐文件理解和实际before/after证据支持，不修改队列、路径、取消、重放预算来取绿。

新文件099、133及新目录400–406保持未接受；129、131、123、132及根091仍逐项验收。当前112项中的任何未完成内容不删除、不降级；最终生产release预算、实际交互矩阵和BentoBox合同继续有效。


### 3.1.23 Provider 开放契约（新增 ZS1-118，清单 122 项）

- 对齐目标：opencode 使用 models.dev 目录（220 家，官方与第三方并存）。zenpi 不逐家实现 SDK，只保证**格式与默认能力**对齐：OpenAI Chat Completions、OpenAI Responses、Anthropic Messages，保留 Google Generative AI。任何 OpenAI/Anthropic 兼容 base_url 均可用。
- 默认开放：未编目模型不再默认 text-only，`ProviderCapabilities` 默认 `text/images/files/tools/structured_output/streaming = true`，再由 wire 求交；`reasoning` 仍保守（需显式等级）。未知模型 context/output 预算仍有限。
- 显式收紧仍有效：`[[model_overrides]]`（含 profile 内）可逐字段 `false/true`；官方目录项能力不被覆盖。文本模型拒绝未声明工具调用的 fail-closed 路径保留。
- 覆盖：`src/providers/registry.rs`；验证 `cargo test --locked --test stage1_model_registry --test stage1_anthropic --test stage1_gemini --test backend`。
- 平台：macOS/Linux 走同一 Unix 传输与 `run_command` 路径（`#[cfg(unix)]`）；Windows 适配推迟。macOS 特有 DNS（`DNSServiceGetAddrInfo`，无 AAAA 主机不再挂起到全局超时）与本契约一同生效。


### 3.1.24 用户要求同步（新增 ZS1-119、140–145，清单 129 项）

本轮用户要求全部登记为本阶段执行义务，`/sync` 是今后把用户新要求写入本单一权威蓝图并立即派发执行的入口。

- **ZS1-119 `/sync`**：`/sync <requirement...>` 把用户要求幂等追加进本文件（新稳定 ID + 验收义务）并写入 durable sync ledger，随后“直接开始做”（派发到现有 loop/compete/domain-execution owner，或标记为待 worker 领取）。去重按规范化文本 hash；已存在同义要求不重复追加；冲突要显式报错而不是静默覆盖。不得改写既有项目/历史项。
- **ZS1-140 一层 tab**：一层 project tab 支持增（指定文件夹）、减、**调位置（左右移动）**、重命名、**每项目基本风格**；绑定 cwd/会话/工具根/权限与风格，持久化到 BentoBox/项目元数据，重启恢复。关闭非活动 tab 不清空当前会话。
- **ZS1-141 文件夹来源**：一层 tab 增加工作文件夹时同时支持**本地**与 **SSH 远端**，远端两种方式都支持：①复用 `~/.ssh/config` 的 Host 别名；②显式 host/user/port/私钥路径。远端只做有界枚举与只读探测，不在本机持久化私钥。
- **ZS1-142 二层 tab**：每个一层 tab 内嵌自己的二层 tab；默认复用一层信息（不强制新建）。二层加号交互：**单击 = 新建 worktree**；**长按 = 横排两个 logo**——左 logo 默认新建 worktree，右 logo 不新建、在当前工作区直接干。二层 tab 可增、减、调位置。
- **ZS1-143 运行时 3 用例验收**：以真实 TUI→headless 跑 3 组用例覆盖 `/blueprint`、`/execute`、`/learn`（3 个 mode 调研）、`/explore`（auto research）、`/addloop`，并断言 TUI 区域 `pm/arch/resources/execution/terminal/gantt` 均正常且可用 BentoBox 调整（resize/fold/focus/持久化）。
- **ZS1-144 命令补齐**：`/execute`（蓝图/域执行派发）、`/explore`（自动 research 循环）、`/addloop`（loop 入口，别名或独立）落地为真实命令，缺一即失败可见，不得用空菜单/固定成功冒充。
- **ZS1-145 区域补齐**：新增 `arch`（架构）与 `execution`（执行）pane，接入 BentoBox 断点/折叠/焦点与持久化；`Browser`/`Terminal` 既有可选 pane 保持。

验收基线：以上各项各自实现+测试后由主控集成；`/sync` 自身必须可被本文件中的后续要求反复使用。3.1.24 追加不改变 3.1.23 及之前已接受项的字节与义务。


