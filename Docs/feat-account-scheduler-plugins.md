# 账号分配与调度插件规格

~~~yaml
schemaVersion: feature-contract/v1
version: 1.0.0
date: 2026-09-13
status: proposed-feature
mode: extension
researchMode: targeted
authoritative: false
implementationAccepted: false
goal: replaceable-account-placement-with-one-admission-path
constraints: one-owner-per-domain; typed-effects; immutable-attempt; bounded-resources
touchedAreas: config; backend; governance; session; runtime; extensions
chosenDirection: trusted-builtins-and-optional-sandboxed-placement-plugins
verification: refusal-invariants; race-barriers; recovery-fixtures; resource-benchmarks
~~~

本文规定后续实现必须做什么、在哪个时机做、失败后留下什么状态，以及如何验收。
本文是功能 PR 的实现合同，不代表已实现；所有新实现项保持未验收。
本文件是账号调度插件的单一细则稿，覆盖原版本；产品行为不依赖外部项目文档。
多进程部署、账号池和连接池、单实例启动、统一外部服务接口及性能门槛由
[本地账号池与统一连接服务规格](feat-account-broker.md) 定义，本文件不复制其实现。

## 1. 一页合同

AS-01：账号分配只经过 AccountScheduler；主会话、Goal、Flow、Subagent 不自选账号。
AS-02：算法只返回候选优先级或等待建议，不能持有凭证、配额写权限或模型发送能力。
AS-03：每个模型请求尝试都取得独立 Permit；任务绑定不长期占用模型并发槽。
AS-04：同一 attempt_id 最多一次从 Reserved 进入 Dispatching，不承诺远端 exactly-once。
AS-05：配置修改先校验后接受；Rejected 不改绑定、配置、队列、预算或模型调用计数。
AS-06：执行中的请求使用不可变路由快照；换算法、账号或模型都不改写该快照。
AS-07：运行中的策略更新有明确作用域和生效边界；已接受不等于已生效或已发送。
AS-08：账号、共享套餐、任务预算分别校验；多 Key 不得扩大真实共享额度。
AS-09：取消先封住新准入；Runtime 负责已有执行收尾，未知远端用量不自动退款。
AS-10：恢复使用精确策略版本和记录的账号绑定；compact 不改变这些运行状态。
AS-11：拒绝、等待、取消、超时、未知提交分别建模，不使用一个 bool 或错误字符串混同。
AS-12：新路径接管全部调用者并通过兼容测试后，才移除旧执行入口，不长期双轨运行。

非目标：调度插件不管理任务 DAG、公平队列算法、工具执行、OAuth 流程或远端 KV。
它不运行提示词，不为了选账号调用模型，也不建立第二个 Agent Loop。
AccountScheduler 在共享 Broker 内实例化，CLI 通过 BrokerClient 使用，不各自建账号池。

## 2. 唯一所有者与代码边界

| 领域及代码落点 | 唯一所有者与工作内容 | 禁止承担的职责 |
|---|---|---|
| src/account_scheduler/mod.rs，新建 | AccountScheduler：绑定、准入编排、等待、策略生效和脱敏回执 | HTTP、工具执行、直接增减额度 |
| src/account_scheduler/strategy.rs，新建 | 策略契约、只读快照、具名结果；注册内置算法 | 另建会话或配额状态 |
| src/account_scheduler/plugins.rs，新建 | 插件安装、自检、精确版本解析与资源回收 | 模型工具注册、修改任务权限 |
| src/config.rs，扩展 | Provider/Account/Pool 描述与策略配置解析 | 用全局环境变量切换当前账号 |
| src/governance.rs，扩展 | Quota Owner：账号共享 scope、任务预算、预留与结算规则 | 选择算法、模型重试 |
| src/session.rs，扩展 | SessionStore：本地会话与服务回执投影；Broker journal 独立持有共享绑定与配额 | 从 prompt 推导账号绑定、客户端争写服务账本 |
| src/runtime.rs，扩展 | Runtime：执行 epoch、取消、后台任务归属与收尾 | 复制账号分配或重试循环 |
| src/backend.rs，扩展 | Gateway：统一模型请求边界、认证、适配、单一重试编排 | 擅自切换账号或扩大候选池 |
| src/core.rs，适配 | 接受执行意图、冻结模型能力、构造上下文并接收统一事件 | 匹配策略 ID、解析厂商帧 |

模块是职责边界，不要求每个名词单独建 crate。内部实现默认私有或 pub(crate)。
跨模块只暴露使用者确实需要的类型；不以方便迁移为由增加长期 re-export。

当前 Backend::complete_with_control 已是模型调用入口，保留统一请求/事件语义。
当前 WorkerBudgetLedger 是会话内预算，不是跨会话账号账本；须先补共享 scope，
不能给现有内存计数改名后就宣称实现多账号并发保护。

绑定的领域决策属于 AccountScheduler，由 Broker journal 持久化；SessionStore 保存
服务回执投影，不成为第二个绑定写入者。共享服务的状态发布与 journal 写入分工见部署规格。
所有 UI/headless 投影从接受事件读取，不各维护一份可变“当前账号”。

## 3. 先定义类型，再接调用者

下列为必须实现的语义接口，非当前已存在 API：

~~~rust
enum Admission {
    Granted(RequestPermit),
    Waiting(WaitTicket),
    Rejected(AdmissionRejection),
}

enum ChangeReceipt {
    Applied { revision: Revision },
    Scheduled { change_id: ChangeId, boundary: ApplyBoundary },
    Rejected(ChangeRejection),
}

enum PlacementDecision {
    RankedCandidates(NonEmptyCandidates),
    Wait { reason: WaitReason, until: WakeCondition },
}

trait AccountStrategy: Send + Sync {
    fn select(&self, input: &SchedulingSnapshot)
        -> Result<PlacementDecision, StrategyError>;
}
~~~

系统 I/O 错误另用 Result 表达；写入状态不确定返回 CommitUncertain，
不能把可能已接受的操作伪装为 Rejected 后直接重做。

AccountScheduler 对外方法固定如下，I/O 失败统一携带 operation_id 供恢复：

| 方法 | 输入与输出 |
|---|---|
| bind | ExecutionIntent -> BindingHandle 或类型化拒绝 |
| acquire | BindingHandle + AttemptSpec -> Admission |
| begin_dispatch | 消费 RequestPermit -> DispatchGuard 或类型化拒绝 |
| settle | 消费 DispatchGuard + AttemptOutcome -> SettlementReceipt |
| change_strategy | 目标 pool/binding + expected_revision + StrategyRef + 生效范围 -> ChangeReceipt |
| rebind | BindingHandle + expected_revision + 新账号/模型要求 -> ChangeReceipt |
| cancel | task/binding 身份 + expected_epoch -> CancelReceipt |
| explain | task/binding 身份 -> 已记录的脱敏视图 |

begin_dispatch 和 settle 仅向 Gateway/恢复所有者开放，不向策略插件暴露。
恢复所有者按 PermitId 核对记录，不能重新构造已消费的 DispatchGuard 来重复发送。

| 类型 | 必须封装的内容与约束 |
|---|---|
| ExecutionIntent | session/branch/task 身份、模型能力要求、允许账号池、用途边界、预算引用、截止时间 |
| StrategyRef | 稳定 ID、实现版本、ABI、内容摘要、已验证参数修订；不解析 latest |
| SessionBinding | 账号/接入点/模型/cache_scope、策略引用、绑定 revision、execution_epoch |
| AttemptSpec | 逻辑请求 ID、独立 attempt_id、上下文修订、模型快照、输入估量、输出上限 |
| RequestPermit | attempt_id、reservation_id、不可变路由、绑定 revision、epoch、过期时间 |
| WaitTicket | 原 attempt_id、首次排队时间、截止时间、唤醒条件；重复请求返回同一身份 |
| SchedulingSnapshot | 合格候选、亲和信息、有界指标、固定时间和种子；不得包含凭证或 prompt |

RequestPermit 字段私有，不实现 Clone，不通过 Deserialize 为任意调用者构造发送权限。
可复制的是 PermitId 和脱敏视图，不是发送能力；跨进程还必须校验持久状态。
Gateway 消费 Permit 取得单次 DispatchGuard，结束后消费 Guard 完成结算。
Rust move 约束不能替代崩溃恢复与账本幂等校验。

策略快照在内置路径中借用；跨 Wasm 边界只序列化必需的脱敏数据。
不可变策略代码可以共享 Arc；不为绕过借用检查给请求、操作或整份上下文添加 Clone。

## 4. 运行时机与具体动作

| 触发时机 | 必须执行的动作 | 不允许发生的行为 |
|---|---|---|
| CLI 启动 / Broker 加载配置 | CLI 连接唯一服务；Broker 一次加载账号目录、scope 和插件；配置按修订更新 | 每 CLI 重建账号池、模型调用探测额度 |
| 创建 Goal 或主会话 | 保存执行意图；首次需要模型时 bind | 为整个 Goal 永久预占并发槽 |
| 第一次模型请求 | bind、冻结模型能力、组装上下文、acquire | 按模型 A 组装后直接发送给能力不同的 B |
| 每次工具续接/下一轮请求 | 复用绑定；用新 attempt_id 重新申请配额 | 一张 Permit 覆盖整个任务 |
| 等待额度 | 登记唯一 WaitTicket；槽释放/窗口恢复/配置更新时唤醒 | TUI 自建轮询、插件无限重试 |
| 每次模型请求结束 | 记录结果、回收本地执行资源、结算已知用量并唤醒等待者 | 在任务总结束时才释放所有模型槽 |
| 切换算法 | 校验配置与版本，返回 Applied/Scheduled/Rejected | 自动切账号、清空上下文或重跑任务 |
| 切换账号或模型 | 登记变更；在安全边界 rebind，再验证上下文能力 | 中途修改已发请求 |
| subagent 启动 | 创建子绑定；账号池、额度、插件权限与父授权取交集 | 覆盖主会话账号或扩大授权 |
| compact | 持久路由状态不变；上下文修订变化，后续请求重新估量 | 把摘要内容当作恢复账号的唯一依据 |
| 暂停/停止 | 推进 epoch 并封住新准入；撤销未发 Permit、等待项；请求 Runtime 收尾 | 先归还未知远端额度再声称停止完成 |
| continue/继续 | 恢复任务与精确绑定，先核对不确定请求，再准入 | 重放已知完成任务或未知副作用请求 |
| 进程崩溃后恢复 | 解码旧记录、核对预留/发送状态、恢复等待年龄 | 自动退款所有未完成预留、自动使用最新版插件 |

安全边界定义为当前绑定不存在在途模型请求和可消费的旧 Permit。
到达边界时先处理已接受的路由变更，再接受下一次 acquire，避免连续请求饿死变更。
工具执行归 Runtime 管理；路由更新不改变已开始工具的权限快照。

普通 steer 仍走主线输入管理：默认 KR/task 边界注入；Esc 提升干预优先级，
必须等当前模型尝试可安全结束后组装下一次请求，不能修改已发送 HTTP 请求体。
再次 Esc 中断走同一停止路径。/goal plan 的 fork 使用独立子绑定和实际调用预算，
返回主线的是计划结果；不得把子绑定、Permit 或子模型设置一起覆盖到主线。

## 5. 一次请求的完整流程

1. Core 接受 ExecutionIntent，校验父授权、用途、模型要求及账号池。
2. AccountScheduler 解析精确 StrategyRef，建立或读取账号绑定，不预留并发。
3. Core 根据冻结的模型能力完成上下文组装；Gateway 确认凭证可用。
   刷新凭证有界且由 Auth 负责，不在额度锁内联网，不把 Token 传入插件。
4. acquire 接收 AttemptSpec，验证绑定、epoch、截止时间和上下文修订。
5. 宿主剔除禁用、无权限、不满足模型/用途要求的账号；临时无容量候选只提供等待信息。
6. 必要时调用插件排名；保持当前绑定时可跳过重复排名，但不能跳过准入检查。
7. 宿主验证插件结果；变更账号仅按已批准的迁移规则提出 rebind 候选。
8. Quota Owner 对每个实际 scope 检查并预留，完成第 7 节提交协议后返回 Granted。
9. Gateway 按最终路由确认对应凭证、准备请求体；若刷新超过 Permit 有效期则重新准入，
   不能拿初选账号的凭证发送。然后用 Permit 调用 begin_dispatch，再次核对撤权、epoch、
   路由 revision 和有效期。只有 Reserved -> Dispatching 成功后才能进入网络发送。
10. 收到模型流后转换为统一事件；终止时 Runtime 收尾，Gateway 提交结果和用量结算。

begin_dispatch 是“本地已授权本次发送”的线性化点，不证明服务商已接收。
在该点之后、发出首字节之前崩溃也可能属于无法判定的发送状态，按 unknown 核对。
不得承诺通过本地状态机消除远端重复执行；远端支持幂等键时才使用其明确契约。

一次 acquire 最多检查 32 个不同候选、刷新快照 2 次；达到上限返回有界等待或拒绝。
等待被唤醒后可再评估，但不重置任务截止时间。Gateway 的远端重试单独计数，
每次产生新的 attempt_id 与 Permit；插件重算不能伪装成远端重试。

## 6. 接受、拒绝与生效

| 操作与结果 | 成功确认点 | 允许的副作用 |
|---|---|---|
| bind -> Bound | Broker 绑定接受事件持久化成功 | 新绑定，不扣模型调用额度 |
| acquire -> Waiting | 唯一等待项已被 owner 接受 | 新等待项；不持有模型并发槽 |
| acquire -> Granted | 各预算预留与接受记录完成核对 | 唯一 Permit；尚无模型请求 |
| strategy change -> Scheduled | 待生效变更事件持久化成功 | 待生效引用；当前配置仍不变 |
| strategy change -> Applied | 目标 revision 持久化且发布 | 后续决策使用新策略 |
| 任一 -> Rejected | 校验失败，未提交业务变更 | 只允许脱敏拒绝诊断；不记成功、不扣调用 |
| 任一 -> CommitUncertain | I/O 结果不足以证明是否已提交 | 停止新发送，按 operation_id 恢复核对 |
| begin_dispatch -> Started | 发送栅栏和 Reserved -> Dispatching 持久化成功 | Gateway 可发送这一尝试 |
| cancel -> Requested | 停止栅栏生效 | 不再新发；在途请求仍可能待核对 |

配置先 preview，不修改原对象；通过后才提交候选新状态。
同时修改同一 policy_revision，只接受一个，另一个返回 RevisionConflict，
不得合并成用户未批准的混合配置。取消与 begin_dispatch 在同一准入栅栏内判定：
取消先成功则零发送；发送先成功则取消回执必须表明存在在途工作。

账号临时忙碌可以 Waiting；账号被禁用、用途禁止和无合法模型是类型化拒绝。
账号禁用/撤权更新必须进入同一准入栅栏并发布新权限 revision；各进程发送前核对它，
不能只修改一个进程的配置缓存。已经 Started 的尝试按在途取消/核对处理。
插件 Trap 不是“额度不足”，不能通过无限等待隐藏。拒绝不改变旧策略的健康状态；
确认插件执行违规产生独立 Quarantined 事件，而不是伪装为无副作用的参数拒绝。

## 7. 状态、配额与跨文件恢复

### 7.1 请求状态

~~~text
Prepared -> Reserved -> Dispatching -> Finished -> Settled
    |           |
    +-----------+-> CancelledBeforeSend

Dispatching -> OutcomeUnknown -> Finished / ReconciledUnknown
~~~

Prepared：只预留，不能发送。Reserved：接受记录已核对，Permit 可消费。
Finished：本地执行已收尾，结算信息已记录；Settled：所需用量维度已经结清。
没有服务端证据时可以保持 ReconciledUnknown，不强行改成已知成功或零用量。
本地连接结束只证明本地结束；远端并发不确定量不得等同本地活动连接数。

单次 Permit 从 reserved 转入 active 时只转移计数，不同时计入两者。
模型请求实际结束后释放已知并发；未知远端并发单列并受服务端已知有效期、
查询证据或明确人工核对约束，不能仅凭本地超时清零。
金额、token、次数按各自实际结果结算，不因并发已释放而一起退款。

### 7.2 账本所有权

本地工具预算保留原任务治理 owner；模型任务预算、账号及套餐 scope 由 Broker 内的
Quota Owner 管理，客户端只能使用服务颁发的预算租约，并可施加更严格本地限制。
Broker journal 是绑定与共享额度唯一权威，所有 CLI 经 IPC 访问，不直接读写它。
复用现有 journal 的帧校验与恢复规则；单 writer 和有界索引由部署规格定义。

现有 WorkerBudgetLedger::reserve 在耗尽时可能记录停止事件，不能直接当作无副作用
preview。新增准入准备接口先计算候选状态，保留原 worker 停止语义及其测试，
不能为满足新接口的 Rejected 合同而无条件改写旧调用者行为。

Account/API Key 可以映射多个 scope：组织、项目、套餐、模型窗口、Key 自身限额。
同一 scope 只预留一次；所有受影响 scope 在短事务锁内统一检查、写入和发布。
并发、RPM、TPM、金额分别有单位和窗口，不能合成一个“剩余额度百分比”代替。
远端观测带 observed_at 和可信度，过期观测不能覆盖本地新预留。
本机预算与远端实际限额分开显示，不能宣称控制其他机器或其他客户端的消耗。

### 7.3 提交与客户端回执

服务端将模型预算、账号 scope 预留、绑定 revision 与 attempt 接受写入同一提交组，
取得 durable sequence 后才返回可执行接受回执。未确认预留也计入容量，禁止提前发送。
客户端 Session 不参与共享准入事务，只幂等追加 receipt_id；回执丢失按原 attempt_id
向服务核对，不重新扣款或发起模型调用。完整状态持久化与批处理规则见部署规格第 7 节。

写入不确定返回 CommitUncertain；已发而结果未知保留 unknown；服务崩溃恢复时先
封住旧 epoch、核对 journal，再允许发送。不能以客户端断开或本地超时证明远端未执行。
客户端 Session 不参与账号账本的提交事务，不保留第二套模型准入路径。

## 8. 算法实际行为

算法版本包含全部比较规则、参数默认值与同分处理，不能修改代码后沿用同一摘要。

| 策略 | 必须实现的比较规则 |
|---|---|
| pinned | 候选集合仅指定账号；短暂满额等待，失去资格则拒绝；禁止 fallback 换账号 |
| sticky | 已绑定账号合法且可准入就保留；临时忙碌默认等最多 3000ms；之后只在显式允许迁移时重选 |
| spread | 仅为新任务或允许迁移的绑定比较 (active + reserved) / approved_concurrency；同分按输入种子打散 |
| auto | 在同一合法模型集合中比较等待、冷启动、费用、风险；满足改善阈值和保持时间才提出迁移 |
| remote | 固定已配置入口；只校验本地明确限额，远端账号选择不再由本地重复实现 |

sticky 等待时间可配置，受总截止时间限制；不允许迁移时继续有界等待或超时退出。
approved_concurrency 为正数；未知服务端限额采用明确批准的本地保守上限。
绑定默认逐任务/分支持有；会话继续轮次保持绑定，不逐轮随机分配账号。

auto 的规格公式为：
score = w_wait*N(wait_ms) + w_cold*N(cold_ms) + w_cost*N(cost) + w_risk*N(error_rate)。
N(x)=min(1,max(0,x/reference))；reference 必须为正，权重非负且和为 1。
cost 使用同一配置货币的每请求估价，不在热路径查询汇率。启用某项必须配置尺度和
缺失值；未启用项权重为 0，未知指标不能暗中按 0 处理。
新候选至少改善 min_improvement，且保持时间达到 min_hold_ms，才可申请 rebind。
该评分只优化软目标，不突破 pinned、套餐用途、硬预算和模型能力要求。

插件不拥有私有账本；负载、EWMA 和冷却记录由宿主按标准口径维护后输入。
首版不提供任意插件 KV 状态。需要有状态算法时，必须先扩展带版本、容量和 CAS
规则的状态合同，再发布新 ABI，不能借全局变量获得未记录的跨任务行为。

100 个独立任务在 A/B/C 并发 2/3/1 的账号池中，其他限额允许时先发最多 6 次调用。
剩余 task 保留在现有任务 owner；准备执行首个模型请求时才绑定账号。
每次槽释放后分配下一个可执行任务，同一 task 的后续轮次保持自身账号。
工具运行、等用户和轮次间隙不持模型槽。批次记录各 task/attempt 的归属与用量，
不只记录总任务数。存在共享 scope 时总并发还必须服从共享上限。

## 9. 插件包与宿主执行

一个接口支持内置 Rust 和显式启用的 Wasm 插件；不支持原生动态库或任意脚本。
内置策略受发布审查约束，不声称被沙箱隔离。当前 release 使用 panic=abort，
因此内置 panic 属宿主故障，不能承诺 catch_unwind 接管；恢复遵循第 7 节。

新增插件 manifest v2 的 account-strategy 类型，保留现有工具 manifest v1 decoder：

~~~toml
manifest_version = 2
kind = "account-strategy"
id = "team.capacity-aware"
version = "1.0.0"
abi = "zenpi.account-strategy/v1"
runtime = "wasm"
artifact = "strategy.wasm"
artifact_sha256 = "<64 lowercase hexadecimal characters>"
config_schema = "config.schema.json"
~~~

安装只接受用户批准的本地包。拒绝路径逃逸、符号链接、重复版本覆盖和不兼容 ABI；
不执行安装脚本、不自动联网获取依赖。ID+版本不同内容拒绝覆盖，要求发布新版本。
bundle 摘要覆盖 manifest、模块、schema 与夹具；不是把摘要当作发布者身份证明。
校验后执行同一份模块字节，防止校验路径与执行路径之间被替换。

ABI v1 只提供有界参数验证与 select 操作，由 SDK 统一编码；跨边界不暴露 Rust 对象。
使用长度明确的 UTF-8 JSON，整数无损编码规则在 schema 固定；拒绝非有限数字。
schema 禁止远程引用、限制层数和数组长度，配置只在接受边界验证，不逐请求任意解析。

Wasm 不提供任何宿主导入，不接入 WASI、文件、网络、环境变量、随机或时钟。
禁止 start；分配、配置自检、select 都计入 fuel。每次调用使用新实例，
只缓存不可变模块，不能让实例内存跨任务保留。记录 seed/time 以支持确定性回放。
检查输入输出偏移、长度、整数溢出和候选身份；插件不能从宿主构造合法 Permit。

初始资源合同：模块 <=2MiB，输入 <=256KiB，输出 <=64KiB，单实例线性内存 <=8MiB，
单次 <=1,000,000 fuel，每宿主真实执行实例 <=2、shadow <=1，候选 <=1000。
同时限制栈、表、函数、解析复杂度和模块缓存总量；禁用 Wasm threads/multi-memory。
热路径执行不占 TUI 线程、不持额度锁；任务句柄归 Runtime 的有界执行 lane 管理。
fuel 不是毫秒 SLA；不得只丢弃超时结果而遗留无限运行的线程。
超出任何资源界限返回 PluginBudgetExceeded，不静默截断输入或候选。

## 10. 插件替换与版本生效

~~~text
Installed -> Validated -> Staged -> Active -> Retired
                |                     |
                +---------------------+-> Quarantined
~~~

这些状态按精确 bundle 身份记录。Active 可以被多个 policy 引用；改一个 pool
不会让另一个仍使用旧版本的 pool 自动升级。Retired 只表示不再接受新引用，
被保留任务引用的旧版本仍能恢复，除非已明确 Quarantined。

| 操作时机 | 执行动作 | 生效条件与回执 |
|---|---|---|
| install | 校验包结构、路径、摘要，登记 Installed | 不改变任何当前策略 |
| check | 校验 ABI/参数、沙箱限制、固定夹具 | 成功 Validated，失败不得激活 |
| stage | 固定候选版本与配置，运行脱敏回放 | 不预留模型额度，不改绑定 |
| shadow | 同快照执行候选插件，只比较输出 | 最多配置样本数；丢弃超预算样本，不影响真实选择 |
| activate new-bindings | CAS 更新 pool 默认策略引用 | 成功后创建的绑定使用新版 |
| activate existing-future | 对选定绑定登记 Scheduled | 到安全边界后 Applied，默认不改账号/模型 |
| rollback | 提交指向旧有效配置的新 revision | 仅后续决策变化，不倒退审计序号或用量 |
| disable/quarantine | 阻止新决策，标明受影响绑定 | 已发请求由 Runtime 完成或停止 |
| uninstall | 检查未完成及可恢复绑定引用 | 仍被引用则拒绝；允许移除时进入系统回收站 |

shadow 只评估选择差异，不能拿未实际执行的候选“预测结果”宣称节省了成本。
它不更新真实负载、选择序号、配额或模型调用计数；系统忙碌时跳过 shadow。

existing-future 不立即改写当前引用；下一边界提交成功才发布新 revision。
切换与旧 select 并发时，旧结果不能写入新 revision。已有等待保留 attempt_id、
WaitTicket 和原排队年龄，最多重排一次关联，不复制等待项。
需要强制账号迁移时必须单独 rebind，不给 activate 增加隐含强迁移行为。

## 11. 命令与可观察回执

CLI/TUI/headless/Flow 进入同一个类型化控制入口，命令不交给模型解释：

~~~text
/route plugins
/route plugin install <local-package>
/route plugin check <id>@<version>
/route plugin shadow <id>@<version> --pool <pool> --samples 100
/route strategy set <id>@<version> --pool <pool> --scope new-bindings
/route strategy set <id>@<version> --binding <id> --scope existing-future
/route strategy rollback --pool <pool> --to-revision <revision>
/route explain <task-id>
~~~

用户可见结果必须区分：
“已安装，未启用”“已接受，待当前请求结束后生效”“已生效”“已拒绝，原配置未改变”
“提交状态待核对”“停止已请求，仍有远端结果待核对”。

explain 读取最后已接受决策，不调用插件或模型重新选择；展示策略版本、账号别名、
模型、排除/等待原因、观测时间、配额来源和实际是否使用 fallback。
只读状态命令不追加业务事件、不创建 session、不刷新凭证或改变冷却时间。

默认插件失败关闭；允许用户预先配置一次具名内置 fallback，不允许递归链。
fallback 仍需完整硬约束检查，并记录原错误和实际策略身份。
Flow 只能引用获准策略、账号池和参数，不能安装代码、改信任根或扩大父级预算。

## 12. 现有格式与兼容

新增记录必须带 schema_version、operation_id、session/branch/task、attempt_id、
policy/binding revision、execution_epoch、策略 ID/版本/摘要、预算关联及脱敏结果。
凭证仅保存私有 credential_ref，不写 Token；公开投影隐藏该引用。
Broker journal 保存业务接受、绑定及共享额度；会话记录保存服务回执，恢复按第 7 节对齐，
UI 不自己拼接两个来源来决定是否可以发送。

“现有格式”仅指本仓库已经可读的 Profile、Session 和工具 manifest v1，不指外部项目，
也不指上一版设计稿。实施前以真实文件夹具和已承诺入口登记兼容清单，不假定存在历史
生产数据。尚未实现的调度格式没有迁移义务，不为其预建第二套 decoder/执行分支。
现有 Profile 配置仍可读取，在配置边界解析为单账号 pool 和内置 sticky。
现有会话缺少绑定时，在恢复后首次执行前显式建立绑定，不伪造过去请求的账号或用量。
同一 decoder 服务 resume 和 history projection，旧记录夹具进入测试库。
插件 v1 工具类型不被自动升级成调度类型，不添加虚假工具通过旧校验。

compact 只处理模型上下文，路由/配额记录独立于摘要保留。
fork 创建新分支和新 Permit 身份；可以沿用账号亲和，但不能复用父分支远端私有句柄。
换账号/模型时清理不兼容远端 continuation/cache/file 引用，保留本地可重建上下文。
冷启动估计与实际 cache-hit 分开记录，不能保证服务端 KV 一定命中。

未知 schema 的写恢复失败关闭，允许只读诊断；不得忽略记录后继续发送。
新版本启用前保留可恢复旧二进制及数据快照；不宣称旧二进制天然能写新 schema。

## 13. 实施顺序与阶段出口

| 阶段 | 何时做、具体交付 | 通过后才能进入下一阶段 |
|---|---|---|
| S0 冻结合同 | 确认本规格、公开类型、拒绝副作用和生效时点；列全 Backend 调用者 | 调用图包含 TUI/headless/Goal/subagent；旧格式夹具齐全 |
| S1 单账号贯通 | 新类型 -> BrokerClient/唯一服务 -> AccountScheduler 接管 Backend；先内置 sticky | 多开只初始化一份账号池，所有请求有 attempt 身份；旧入口无生产调用 |
| S2 配额闭环 | 单 writer、共享 scope、服务准入、回执恢复、发送栅栏与取消 | 双客户端争一个槽、提交不确定、崩溃未知均通过；未完成前禁止多账号并发 |
| S3 算法替换 | Registry、spread/auto、显式 rebind、策略修订与等待唤醒 | 换策略不改在途请求；100 任务记录和共享上限正确 |
| S4 可安装插件 | 有界 Wasm lane、manifest v2、安装/检查/激活/回滚 | ABI、资源、恶意输出、取消收尾全部通过；关闭 feature 仍可用内置 |
| S5 收敛发布 | 迁移剩余调用者和测试，移除旧账号选择执行分支 | 无双重重试/账本/loader；完成兼容、体积与延迟门槛 |

每阶段先新增接口，再迁移调用者和测试，再移除已无调用的旧入口。
保留旧数据 decoder，不保留可绕过准入的旧发送路径。
代码与依赖移除须列明替代入口、调用搜索证据及回退办法；文件移除遵守回收站规则。
同一 PR 不顺带重排 Provider 接入、Goal DAG、OAuth 或不相关格式。

回退单位是尚未接受的新配置或完整兼容版本，不切回能绕过账本的旧热路径。
存在未结清请求时先停止新准入并对账；数据 schema 不兼容时禁止直接旧版本写恢复。

## 14. 验收反例与证据

以下是待实现测试名称及断言，不表示测试已经存在。并发测试用 Barrier/通知点和
假时钟控制交错，不依靠 sleep 碰运气；FakeProvider 记录真实发送次数。

| 测试 | 人为安排的时序/输入 | 必须观察到的结果 |
|---|---|---|
| reject_preserves_state | 非法参数/候选提交前后比较快照 | 配置、绑定、队列、预算及发送数相等；仅拒绝诊断增加 |
| one_slot_two_acquires | 两个客户端同时向共享服务申请唯一空槽 | 仅一个 Granted；另一个 Waiting；共享计数不超 1 |
| duplicate_attempt | 同 attempt 重复 acquire、重复终止事件 | 一个预算预留、至多一次发送转移、幂等结算 |
| cancel_before_dispatch | Reserved 后暂停 Gateway，先取消再放行 | provider_send_count=0，未发预留回收一次 |
| cancel_after_dispatch | Started 后停止，远端不给结果 | 回执为待核对，unknown 用量不清零、不自动重试 |
| swap_during_select | 暂停旧 select，接受并生效新 revision | 旧结果拒绝提交；账号保持，下一次用新策略 |
| scheduled_not_applied | 在途请求中修改现有绑定策略 | 先 Scheduled；旧请求不变；边界后恰好一次 Applied |
| prepared_crash | 服务提交组同步前逐点注入崩溃 | 无发送，恢复核对，不把未确认状态当作可执行 |
| accepted_crash | Accepted 持久化后、客户端 Session 保存回执前崩溃 | 查询返回同一接受身份，不重复扣款或创建第二 attempt |
| send_crash | Dispatching 持久化后不记录响应 | 保留 unknown；恢复不假定未发送 |
| shared_key_scope | 多 Key 归属一个套餐并行请求 | Key 数增加不改变共享套餐上限 |
| shadow_is_readonly | 新旧插件候选完全不同 | 实际路由仅旧版；无额外 Permit、调用、绑定变更 |
| wasm_limits | 非法导入、死循环、内存膨胀、超长输出 | 有界失败，lane 回收，TUI 继续响应 |
| restore_exact_version | 恢复时旧 bundle 缺失但新版可用 | 明确错误，不自动替换；批准迁移后才执行 |
| legacy_roundtrip | 旧 Profile/会话/工具插件样本 | 原语义可读取，新调度记录不伪造旧字段 |
| compact_and_fork | 压缩后继续，另 fork 指定其他账号 | 主绑定不变；子预算独立、父权限不扩大 |
| batch_100 | 100 task，账号槽 2/3/1，加入慢请求与失败 | 每 task 可追踪；总在途受所有 scope 约束，无永久槽泄漏 |

后续实现必须新增测试目标 account_scheduler_contract、account_scheduler_recovery、
account_scheduler_plugins，以及同名基准工具。阶段验收命令：

~~~bash
cargo fmt --check
cargo test --locked
cargo test --locked --features account-strategy-wasm --test account_scheduler_contract
cargo test --locked --features account-strategy-wasm --test account_scheduler_recovery
cargo test --locked --features account-strategy-wasm --test account_scheduler_plugins
cargo clippy --locked --all-targets --all-features -- -D warnings
~~~

account-strategy-wasm feature 和上述测试目标当前尚不存在，S4 必须交付，不能以
“找不到目标”算通过。S1/S2 先交付其对应无 feature 的契约与恢复测试。

性能门槛是待验证的发布要求：固定基准机记录 OS/CPU/Rust/编译参数，release 下
对 1/10/100/1000 候选各预热 100 次、测量 10000 次，记录 p50/p95/p99、分配与 RSS。
100 候选时内置 select p99 <=1ms；Wasm select 含新实例和编解码 p99 <=10ms。
不把账号磁盘事务或网络延迟混入算法时间；它们单独测量并报告，不冒充端到端 SLA。
默认未启用 Wasm 的产物不得链接解释器；启用版本相对基线二进制增量 <=15MiB，
并发上限测试的增量 RSS <=64MiB。若不达标，阻止可安装能力发布，先优化或重新审批阈值，
不能省略沙箱、恢复或计量来通过性能门槛。

本轮完成规格与静态文档检查；未实现接口、迁移数据、执行上述测试或验证性能阈值。
