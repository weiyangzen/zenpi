# 本地账号池与统一连接服务规格

```yaml
schemaVersion: feature-contract/v1
version: 1.0.0
status: proposed-feature
authoritative: false
implementationAccepted: false
deployment: one-broker-per-user-state-root
defaultTransport: unix-domain-socket
defaultPersistence: single-writer-journal
defaultUpstreamConcurrency: 16
```

本规格承接多开与 5000 并发提交要求，替代“每个 CLI 进程内初始化调度资源”的部署方式。
它只规定共享服务、连接与资源生命周期；选择算法仍由账号调度插件规格定义。
首次使用必须由用户显式批准 broker 配置；之后 CLI 才可按需启动已批准的服务。
服务生命周期是独立管理命令，不增加 RunMode；--mode 仍只有 tui/headless。
仓库自动化 worker 的嵌套进程/服务禁止规则不变，本合同不授予 worker 启动 Broker 的权限。
以下数值是待验证的初始合同，不是已测性能。蓝图登记待实施工作，运行时代码与现有验收结果不变。

## 1. 概念分开

| 名称 | 内容 | 所有者与创建时机 |
|---|---|---|
| AccountPool，账号池 | 已配置账号集合、资格、共享额度 scope | Broker 启动时加载一次，配置修订时更新 |
| TransportPool，连接池 | 到 Provider 或外部 Broker 的 HTTP/TLS 可复用连接 | Broker 首次使用目标时延迟创建，有界保留 |
| WorkerPool，执行线程池 | 同步网络调用与有限后台工作 | Broker 启动后按需增长到固定上限，不按客户端增长 |
| SessionBinding，账号绑定 | 某任务/分支选中的账号、模型、策略版本 | 首次需要模型时建立，不创建新账号池 |
| BrokerClient，客户端句柄 | 指向共享服务的本地 IPC 连接及请求身份 | 每个 CLI 一个，进程内多任务复用 |

数据库只能共享状态，不能共享活着的 HTTP 连接；公共线程只能在所属进程内使用。
因此必须由一个共享进程真正持有并执行上游请求，而不只是替 CLI 选择账号。
同一账号池可服务多个项目，但工具权限和任务上下文不因此合并。

## 2. 最小架构

```text
zenpi CLI / TUI / headless x N
  Agent Loop + 本地工具 + SessionStore
          |
          | BrokerClient，Unix Domain Socket
          v
同一 zenpi 二进制的 broker 子命令：一个共享服务进程
  非阻塞接入 + 单一状态所有者 + 有界等待索引
  AccountScheduler + StrategyRegistry + Auth + Quota Owner
  有界网络 WorkerPool + 共享 TransportPool
          |
          +--> Provider API
          或
          +--> RemoteBrokerAdapter --> 显式配置的外部统一服务
```

Broker 不执行 shell、不接管工作区，也不运行第二个 Agent Loop。
CLI 不初始化账号目录、OAuth 刷新器、策略实例或 Provider 连接池，不打开共享配额文件。
实际请求经 Broker 发送，统一事件返回原 CLI；断开共享服务不得退回客户端直连 Provider。

首版一个二进制、一个按需共享进程、一套有界 journal，不引入数据库集群、服务注册中心、
通用 RPC 框架或分布式选主。安装式 Wasm 插件按需启用，不属于轻量默认启动成本。
SQLite/Redis 不是首版依赖；只有持久化实测成为瓶颈才替换 Broker 内部存储端口，
不能让所有 CLI 改成直接访问数据库。外部服务接入属于传输端口，不属于数据库端口。

## 3. 单实例启动、认证与退出

实例身份是 OS 用户身份与规范化 state_root。默认所有 cwd 共享一个 state_root；
显式不同根代表隔离实例，不承诺跨根共享配额。相同凭证跨根使用时须归属同一权威服务。

1. CLI 先读取小型服务发现记录并连接，核对协议、服务 epoch、状态根和同用户身份。
2. 未就绪时尝试非阻塞 startup lock；仅取得锁者允许启动 broker 子命令。
3. 子进程先取得 lifetime lock，再加载账号、journal 和线程资源；拿不到锁就退出，
   不进行第二次账号池初始化。启动者保持 startup lock 到 Ready 或明确失败。
4. 其他 CLI 按 50-500ms 抖动退避连接，总启动等待默认 10 秒；不各自 spawn 或忙轮询。
5. lifetime lock 持有到服务退出；PID 文件仅作诊断，不能用 PID 或过期心跳替代锁。

运行目录权限 0700、发现信息与 socket 仅用户可访问；校验 peer 用户身份。
不依赖“socket 路径难猜”作为认证。不同用户默认不得接入；本机特权管理员不在隔离承诺内。
同 UID 只证明连接身份，不授予任意模型或管理权限。服务签发的 capability 绑定调用者、
pool、模型集合、预算租约、用途、策略修订、有效期和可撤销 epoch，校验后才接受 Submit。
受限 worker 只能收到窄化 capability，不能读取/铸造管理员能力、修改配置或安装插件；
OS 隔离必须阻止其读取控制凭证或通过其他路径冒充宿主，无法保证时拒绝 worker 接入。
任意未隔离的同 UID 程序属于用户信任域，不承诺靠 IPC token 抵御其读取用户私有文件。
使用带随机 epoch 的 socket 文件名，避免直接删除不明旧 socket。孤立文件仅在确认
旧 owner 已结束后进入系统回收站；禁止 unlink/rm 清理。锁文件长期复用，不靠删除锁解锁。

新任务只在 Ready 接受；恢复失败返回 RecoveryRequired，不边恢复边发送。
默认无连接、无队列、无在途工作 60 秒后退出，释放空闲 HTTP 连接；未知用量先持久化。
退出/升级先 Draining，停止新接入执行、回收网络工作、提交账目，最后释放 lifetime lock。
只发送终止信号不代表已回收。前景服务由调用者 wait；共享后台实例启动时必须明确
交给 OS 用户级监督/收养路径，原 CLI 退出不结束服务，也不保留 5000 个 waiter。
平台没有受支持的后台归属时只允许显式前景启动，自动启动返回 LifecycleUnsupported。
服务自身负责 join 全部工作线程；短命 CLI 无权直接杀掉其他客户端共用的实例。

## 4. 事件循环与线程分工

| 执行面 | 数量 | 工作与限制 |
|---|---|---|
| 接入及状态循环 | 1 | OS readiness 事件、短命令、状态转移、预算与等待索引；不做阻塞 I/O |
| journal writer | 1 | 按序写入与同步，返回 durable sequence；不私自改变领域状态 |
| 上游网络工作线程 | 默认最多 16 | 复用现有同步 HTTP 客户端；一个活动流占一个工作位 |
| 策略计算工作位 | 最多 2 | 执行有界算法/Wasm，不阻塞接入；结果回到状态 owner 做版本检查 |

接入使用小型 readiness 库封装 kqueue/epoll，不自行实现通用 async runtime。
网络沿用现有同步传输，不为 5000 个客户端启动 5000 个线程；空闲时不预建 16 条 TLS 连接。
工作线程只返回事实事件，只有状态 owner 提交绑定、配额与取消决定。
网络/插件/磁盘结果以身份和 revision 返回，不能持锁回调主循环。

连接生命周期固定为 AcceptedSocket -> Authenticating -> Ready -> Closing -> Retired。
认证前只解析一个小型握手帧，不接受 pipeline、正文或配额操作；容量检查先于大缓冲分配。
连接身份使用 slot+generation，禁止以可被 OS 复用的裸 FD 关联请求；每次 callback 与
后台结果返回后都重检 generation。Closing 不再接新命令，回调退出后才回收 slot。

每轮按就绪批次处理；每客户端最多处理 8 个控制帧或 32KiB，整个处理片段目标不超过
1ms。尚有数据的客户端回到就绪队列，不能被一个长 pipeline 垄断。
使用批量写入和缓冲复用，按请求结构解析一次；默认不将消息反复转为 JSON Value 再编码。
增量解析保留 cursor 与未完成帧，半包不是错误，多包也不得越过每轮预算。
同一 request_id 重复同摘要返回原结果，不同摘要返回 RequestConflict，不重新执行。
待写队列以连接身份去重；每轮先有界尝试写已有回复，仅未写完的连接注册写就绪。
写就绪只在存在待写数据时注册；不轮询所有空闲 FD。到期等待使用有界计时索引，
取消用 request_id 索引，额度释放按 scope 唤醒有限候选，不逐次扫描整个账号池/任务队列。

## 5. 接入与执行不是同一个并发数

初始配置与硬限制如下，用户提高配置前必须通过对应资源检查：

| 资源 | 默认上限 | 达到上限时 |
|---|---|---|
| 客户端连接 | 8192，另留内部 FD 预算 | 明确 CapacityExceeded；不无界 accept |
| 全局等待请求 | 8192；每连接默认 1 个等待请求 | QueueFull + retry_after，不隐式丢请求 |
| 等待元数据 | 每项 <=2KiB，总计 <=16MiB | 拒绝超大元数据；不保存完整 prompt |
| 上游活动调用 | 16，且服从全部账号/任务 scope | 等待，不创建额外线程绕过配额 |
| 同时正文上传 | 4；每请求 <=2MiB，总活动正文 <=32MiB | 暂停发放上传信用或拒绝 |
| 单连接输入初始缓冲 | 按需分配，通常 <=4KiB；正文分块 | 大缓冲仅供获准上传者使用 |
| 单流待输出 | 软限 32KiB 持续 5 秒，硬限 64KiB | 背压后 SlowConsumer，停止并核对上游 |
| 全客户端缓冲总量 | 64MiB，含解析/输入/输出计量 | 停止接纳，保留控制面和收尾预算 |

启动时检查 RLIMIT_NOFILE、实际监听容量及内存预算，保留不少于 128 个内部 FD；
不能满足声明容量时显示有效容量和限制原因，不假装已经支持 5000 客户端。
不修改系统级限制或自动提升权限。测试环境须先具备足够的系统硬限制。

5000 个命令先提交小型请求头并排队；服务给出 ReadyForBody 后才上传完整规范化请求。
上传有期限，正文摘要/长度必须与提交头一致；上传完成并通过能力校验后才正式请求配额。
等待期间保留的是任务身份和估量，不是 5000 份上下文和 HTTP 连接。
客户端可流水发送少量控制命令，但不能用 pipeline 绕过每客户端及全局执行限制。

客户端慢读时先停止该流上游读取，使压力传回网络工作位；达到时间/硬限后取消，
记录 SlowConsumer 和不确定远端用量。不得吞掉模型增量后伪造完整成功。
控制命令有独立的小型预算；数据流拥塞时 Cancel/Status 仍能处理，控制面也需限速。
低于输出软限时清除持续超限计时；连续超限才触发软限关闭，硬限立即进入 Closing。
CancelRequested 可先确认内存停止栅栏已建立，但必须另报持久化与收尾结果；其低延迟
回执不能冒充“已停止远端/已完成退款”。writer 迟到的准入结果仍须经过取消栅栏检查。

5000 进程本身的 RSS 和操作系统 fork 成本不由 Broker 消除。大批量任务优先通过
单个 CLI 的有界批量入口提交，复用一个 IPC 连接；这不取消多开支持，也不伪称整个
操作系统中的 5000 个完整 Agent Loop 可以零成本运行。

## 6. 真正的 HTTP 连接池

TransportPool 只在 Broker 持有，不在每个 Backend 请求构造器里 new HTTP client。
从现有模型 Backend 分离不可变请求设置和共享传输句柄；模型、reasoning 等属于请求，
不属于每次必须重建的 client。同一传输键只延迟构造一次，工作线程借用/共享该 client。

传输键至少包含 scheme/host/port、proxy、TLS 信任与客户端证书身份、账号隔离域、
连接相关认证代次。普通请求级 Token 刷新不必丢弃 TCP/TLS 连接；若认证绑定连接、
证书变化或账号撤权，则停止旧池新借用，等在途结束后淘汰。默认不跨账号共享连接。
不能将 Authorization、模型或会话标识放入可变全局默认 Header。

池总数默认最多 64，每键最多保留 2 个空闲连接，全局空闲连接最多 32，闲置 60 秒淘汰。
活动连接由全局和账号 Permit 控制，不因池空闲上限较高而增加并发。
全局空闲预算由 TransportPool 分配到具体 client，不能每个 client 都各自允许 32 个空闲连接。
容量已满时淘汰无在途引用的闲置池；没有可淘汰项则等待，不构造第 65 个隐形池。
响应体按协议正确读完才能安全复用，取消/截断时关闭不可复用连接。
首版不宣称现有同步客户端支持 HTTP/2 多路复用；传输实现确有能力并通过测试后才启用。

同连接不等于同远端会话；会话 ID、cache key、上下文前缀按任务绑定独立维护。
连接复用节省握手，账号亲和降低缓存失效风险，两者都不能证明远端 KV 命中。

## 7. 日志、批处理与恢复

服务内只有一个配额/绑定状态 owner 和一个 journal writer；客户端不争写共享文件。
本地实现使用有界内存索引加追加日志，启动只恢复一次，不每条请求重读全部历史。
绑定、策略引用、服务端配额状态以 Broker journal 为权威；SessionStore 只保存其回执投影。
本地工具预算仍由原任务 owner 负责；服务端模型预算必须由服务颁发的预算租约约束，
不能把客户端自报“还有额度”当作授权。父子任务继承服务端模型预算，避免多开超领。

服务端预算预留、绑定 revision 和 attempt 接受写同一提交组，提交成功才发布可发送状态。
writer 最多合并 64 条记录或等待 2ms 后执行一次 durability barrier；序号未确认的请求
不得发送。pending 预留也计入可用容量，不能因等待 fsync 而被第二个请求再分配。
IO 不确定时保留 PendingCommit 并停止相关新准入，不能返回无副作用 Rejected。
日志同步不能阻塞接入线程；磁盘慢会降低已确认吞吐，不得改成先发送后补账。
提交组有长度、校验、序号及 commit marker；只有完整校验通过的组可恢复为接受状态。
部分尾帧不构成接受，保留诊断并恢复到新段，不通过永久截断文件清理证据。

模型日志提交与客户端 Session 写入不构成原子事务。服务返回稳定 receipt_id，客户端
幂等追加回执；回复丢失用同 attempt_id 查询，服务不得再发一次。客户端持久化失败
触发停止/核对，不允许 Broker 为此重跑模型。服务不读取任意客户端文件路径验证提交。

持久化预留、发送授权、结束与未知用量；不逐 token fsync，不默认持久化完整 prompt。
等待项接受与是否已持久化用不同回执表述；未持久等待在重启后由客户端用原 ID 重登记。
日志检查点按字节/条数触发，分段生成有界快照，不 fork 服务或长时间冻结接入。
快照完成并验证 durable sequence 前保留原日志；退役文件只进回收站，不永久删除。
达到日志/磁盘硬预算时停止新准入；不能淘汰未结清用量或用 OOM 后重启替代背压。

## 8. 本地与外部统一接口

BrokerClient 只依赖 AccountBrokerTransport，不依赖 socket、数据库或 Provider SDK。
统一能力包括 Bind、Submit、Attach、Cancel、ChangeRoute、Inspect；Submit 的结果
区分 Queued、ReadyForBody、Accepted、Rejected、CommitUncertain，模型响应流为规范化事件。
内部 RequestPermit/DispatchGuard 不跨 IPC 变成用户可构造的发送权。

本地帧使用长度前缀 + 版本化 JSON，控制帧 <=8KiB，正文/事件块 <=64KiB；支持半包、
多包、超时、断连、request_id 关联和有界 pipeline，不实现一套通用命令语言。
Attach 使用 attempt_id、授权及事件游标；只重放有界缓冲可覆盖的事件，越界返回 ReplayGap，
不得自动重跑模型。断连默认撤销等待；已发请求给 2 秒重连宽限，之后取消并核对，
除非该任务明确获得 detached 执行授权；不把关闭终端默认为无限后台执行。

外部模式仍由本地轻量端点汇聚连接，再由 RemoteBrokerAdapter 连接 HTTPS 外部服务；
这一模式本地是有界 relay，不建立第二份账号选择、Provider 凭证或配额权威。
远端服务接收相同逻辑请求/事件，负责账号、模型准入和 Provider 连接池；本地只控自身资源。
同一 pool namespace 只允许一个权威端点；故障时不自动换成另一套独立账本。
远端接入校验 TLS、服务版本、租户权限与能力，使用服务凭证而非传递任意 Provider Key。
未经明确授权，不把本地 prompt/工具输出发到新远端；端点变更要求用户确认数据边界。
普通 /v1/responses Provider 不等于 AccountBroker 协议，仍由 ProviderAdapter 接入。

## 9. 性能门槛与开发顺序

性能不是宣称“像高性能服务一样快”，而是分层给出证据：

1. 接入基准：100/1000/5000 空闲连接及突发建立连接，检查 FD、线程、RSS 和启动实例数。
2. 控制基准：256B/2KiB 请求，pipeline=1/8，测 Status/Cancel p50/p95/p99 与饱和吞吐。
3. 准入基准：开启实际 durability barrier，测 reserve/commit/settle，报告磁盘和 fsync 策略。
4. 模型流基准：FakeProvider 固定首包/生成延迟、16 活动流，5000 待处理任务，混入慢读和断连。
5. 故障基准：启动风暴、磁盘慢/满、插件卡住、服务在提交/发送后崩溃、外部服务失联。

固定基准机、release 构建、客户端机器/进程布局、payload、连接复用和持久化配置；
冷启动与稳态、算法时间与端到端时间分开。吞吐必须与错误率、队列增长、尾延迟一起报告，
不能比较关闭持久化的内存 PING 与开启硬预算的模型请求，不能只报告平均延迟。

初始发布门槛：5000 空闲连接时服务线程数不超过 20，默认无 Wasm 服务空闲 RSS <=32MiB，
5000 连接及有界模拟负载 RSS <=128MiB；进程、共享账号目录初始化次数均为 1。
控制基准在 10000 ops/s、payload<=2KiB 时 p99<=10ms；慢模型流期间 Cancel p99<=20ms。
这是基准机上的待验收目标，不承诺任何机器或真实 Provider 可达到；提交吞吐单独报告，
不得降低 durable 准入保证来满足控制面指标。上限不达标则阻止标记 5000-client-ready。

实现顺序：先 BrokerClient/本地唯一实例和单账号贯通，再单 writer/幂等准入与取消，
再共享 TransportPool/5000 接入背压，最后安装式插件及 RemoteBrokerAdapter。
新增测试目标 account_broker_singleton、account_broker_load、account_broker_recovery，
覆盖同 uid 多 cwd、FD 不足、慢读、错误复用、重复 attempt、启动器崩溃和 relay 失联。
先测瓶颈再调批量和缓冲；只有事件线程真正饱和才增加 I/O 分片，不预建多核调度系统。

与既有代码的硬冲突是 Backend 当前在各进程持有独立 HTTP client；迁移必须将真实
网络发送一起移入 Broker，只移动账号选择不能实现连接复用。现有“不新增服务”
产品边界仅按 Execution Spec 1.1 扩展，仍不新增 RunMode。单状态 owner 与后台 I/O
不是冲突：后台只执行 I/O 并回传结果，不成为第二个配额或绑定写入者。

本轮没有启动服务、安装依赖或运行上述基准。对应蓝图条目保持未验收，只有实现与证据通过后才可关闭。
