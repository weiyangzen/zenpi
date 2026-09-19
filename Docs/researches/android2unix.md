# Zenpi Android -> macOS/Linux 研究与实施建议

> 研究日期：2026-09-19。本文是面向当前仓库的架构调研，不表示 Android 客户端、网络监听器或远程 worktree 控制已经实现。

## 结论先行

建议做成 **Android 原生 Kotlin + Jetpack Compose 控制/显示端，Unix 工作机上的 `zenpi-host` 执行端**：

```text
Android Compose
  ├─ workspace（一级 tab）
  └─ worktree（每个 workspace 的二级 tab）
       ⇅ 版本化命令、事件、审批、PTY/日志
macOS/Linux zenpi-host
  ├─ 持有 Agent、session journal、context、project/worktree owner
  ├─ 启动/管理 TUI 或 headless 实际运行
  └─ 通过 QUIC/TLS 直接监听并把同一套 host API 暴露给 TUI 和 Android
```

这里有一个必须在产品定义中说清楚的边界：**“公网、任意 NAT、稳定直连、完全不依赖任何服务器”四件事不能同时保证。** IETF ICE 的模型要求双方先交换 candidate；典型部署有 signaling，STUN 用于 server-reflexive candidate，TURN 用于 relay。[RFC 8445](https://www.rfc-editor.org/rfc/rfc8445.html) 也明确 ICE 本身不负责 signaling 的穿透。没有第三方服务时只能保证“双方已有可达地址”的直连，例如同一局域网、可入站 IPv6、用户已做端口转发，或用户自行提供 VPN/中继。对称 NAT、运营商 CGN、入站防火墙下，不能诚实地承诺公网直连成功。

因此推荐三个明确的连接模式，而不是把失败伪装成“保活”：

| 模式 | 是否有 Zenpi 服务器 | 可达性 | 推荐用途 |
| --- | --- | --- | --- |
| `direct-manual` | 否 | 局域网、可入站 IPv6、端口转发 | 默认的严格无服务器模式；二维码/短码配对后直连 |
| `direct-assisted` | 可选，用户自托管 | ICE/STUN/可选 TURN | 跨公网的实际推荐模式；服务只做发现/打洞/中继，不能解密业务 |
| `nearby` | 否 | Android Wi-Fi Direct 或局域网 NSD | 同场临时连接；不解决远程 NAT |

若用户坚持“任何公网均不依赖服务器”，产品应显示“此网络不可达，请使用 IPv6/端口转发/VPN/自托管 rendezvous”，而不是悄悄加入一个官方云 relay。Iroh 的默认 preset 会配置 DNS address lookup 和默认 relay；若采用 Iroh，必须显式使用空 builder/关闭 relay，并接受可达性下降。[Iroh `Builder::empty` 与 `relay_mode`](https://docs.rs/iroh/1.2.0/iroh/endpoint/struct.Builder.html)、[Iroh N0 preset](https://docs.rs/iroh/1.2.0/iroh/endpoint/presets/struct.N0.html)。更保守的第一版可以直接使用 Quinn QUIC，自己实现配对与 candidate 配置，避免引入隐含的第三方发现服务。

## 1. 当前仓库的事实基线

这部分先于 Android 设计，因为“完整对齐 TUI”必须对齐当前真实边界，而不是对齐一个假想的 GUI。

### 1.1 入口与可复用能力

- 当前 CLI 只有 `tui` 和 `headless` 两个 mode：`src/core.rs:5686-5706` 解析值，`src/core.rs:6189-6193` 分别进入 `run_stdio_owned` 与 `run_async_with_profile`。没有 GUI/Android mode、网络 listener 或移动端 target。
- `src/protocol.rs:20-37` 已有有界的 v1/v2 JSONL 协议：单帧 1 MiB、prompt/text 256 KiB、协议版本与 checkpoint 页限制。`StdioRequest`/`Command` 在 `src/protocol.rs:145-263` 已覆盖 prompt、steer、cancel、status、resources、handoff、resume、checkpoint、mailbox、input queue、session tree、tool output、user shell、approve、shutdown。
- 事件和终端响应已经分离：`StdioResponse` 与带 `sequence/request_id/turn_id` 的 `StdioEvent` 位于 `src/protocol.rs:854-918`，`encode_line` 位于 `src/protocol.rs:994-999`。这适合被包在 QUIC 的长度帧中，但不能直接把 stdin/stdout 当公网服务。
- owned headless host 有 owner 注册、异步事件和本地 replay：`src/headless.rs:4352-4367` 建立 `ProjectOwnerPool`、恢复 checkpoint、打开 replay journal 并注册 live owner；现有 WAL/锁是**工作机本地**的 owner 机制，不是网络认证或 P2P 协议。

### 1.2 workspace、worktree 与并发

- TUI 的二级 tab 在 `src/tui.rs:3123-3178` 懒创建，默认 `main`，每个 project 可保留最多 `MAX_PROJECT_SUBTABS = 32`（`src/project_workspace.rs:903-905`）。
- “原地隔离”新增 tab 在 `src/tui.rs:3198-3221`；git worktree 新增在 `src/tui.rs:3223-3255`，分支名只允许 ASCII 字母数字、`-`、`_`。
- 关闭 worktree 会调用 `git worktree remove --force`，但当前 TUI 忽略了失败（`src/tui.rs:3257-3281`）。远程 API 不应复制这个行为：只有 host 成功删除并完成 checkpoint 后才向 Android 返回成功，失败要保留 tab 并给出可重试错误。
- 每个二级 tab 的并发在 `src/tui.rs:3458-3474` 被限制为 1..64 并持久化；鼠标点按命中 add/select/close/并发加减在 `src/tui.rs:7112-7148`。当前实现主要是 TUI 状态和 quota 读取，并不等于已有可远程伸缩的 scheduler。
- `ProjectContext` 的稳定身份是 `project_id/cwd/session_id`（`src/project_workspace.rs:280-300`）；Android 应使用这些 ID，不要把 tab 名称或列表索引当主键。
- 目前一级 `ProjectOwnerPool` checkpoint（`project-workspace.json`）主要保存 project/session 映射；二级 subtabs、root、name、concurrency 是 TUI 的 `LayoutPersistence` 视图 checkpoint（`src/tui.rs:3817-3842`、`14755-14787`），并没有进入 typed headless owner。Android 要求的两层 tab 不能只读取 TUI 布局文件，必须建立 host-owned 的二级 domain，并把它与 project/session checkpoint 原子关联。

### 1.3 当前 headless 的关键缺口

- **不要用 slash `/project` 伪造远程项目控制。** `execute_headless_slash` 在 `src/headless.rs:7325-7328` 只是回显 `{command, action}`。但异步 owned host 的 typed `Command::Project` 在 `src/headless.rs:5507-5548` 才会真正调用 owner pool 的 open/select/close，并返回项目上下文。因此 Android 必须走 typed owner API。
- **当前 `/worktree` 不执行操作。** `src/headless.rs:8167-8172` 明确返回“layer-2 worktree sub-tabs are owned by the interactive TUI host”。所以 Android 目前不能添加、选择、关闭、移动、重命名 worktree，也不能远程改变并发；这需要 host API 和持久化模型改造，不能在调研中宣称已完成。
- 当前没有 Android/GUI 渲染器，也没有 P2P 配对、证书 pin、端到端设备身份、NAT traversal、网络重连或移动端离线队列。`net_probe`/`cluster` 只涵盖受授权 LAN 探测和 SSH worker 控制，不是 Android 链路。
- README 也明确当前产品只有 TUI/headless、没有第三种 server/daemon mode（`README.md:3-11`）；本文提出的 `zenpi-host` 是新增的网络边界，不应被误解为仓库已有能力。

### 1.4 对齐矩阵

| TUI 能力 | 现状 | Android 方案 |
| --- | --- | --- |
| prompt、stream event、steer、cancel、status | headless typed protocol 已有 | 原样保留 request/response/event envelope，QUIC 长度帧承载 |
| approve、diff preview、input queue、session tree、tool output | 协议和 host 路径已有 | 点按审批、差异预览；所有 destructive action 带 `expected_revision` |
| session list/open/fork/export/import/gc、resume、compact | 多数 slash/host 路径已有，部分要求 idle/owner | 先查询 capabilities/owner_required，再显示可执行按钮；host 是唯一写入者 |
| 一级 project/workspace | owner pool 可执行 typed project；slash shortcut 只是回显 | 增加网络 host adapter，所有 TUI/Android 调用同一 typed API |
| 二级 in-place/worktree、select/close/move/rename | 仅 TUI；headless 明确不执行 | 新增稳定 `subtab_id` 和 `SubtabControl`；成功后才 checkpoint/发 event |
| 每 worktree 1..64 并发 | TUI 状态有 clamp/持久化，scheduler 对接仍需实现 | host 返回 `requested/effective/available`，UI 使用 stepper；变更可审计 |
| PTY shell、TUI 绘制 | 工作机本地 TUI-only | Android 显示结构化输出或独立 PTY stream；不在 Android 运行 shell/ratatui |

## 2. 网络架构建议

### 2.1 工作机端 `zenpi-host`

新增一个 Unix 进程/子命令（名称可调整）作为网络边界，职责如下：

1. 启动或接管一个 `ProjectOwnerPool`，持有 Agent、session journal、context checkpoint、worktree owner 和并发 scheduler。
2. 对本机 TUI、headless 和 Android 暴露同一个 typed host API。TUI 的本地 mouse action 不应再拥有一套远程不可见的 worktree 业务逻辑。
3. 只在用户明确启用远程连接时监听；默认绑定 loopback/LAN 或用户指定地址。启动时打印/显示一次性配对 QR 和证书 fingerprint，不把 provider API key 放入二维码或 Android。
4. 继续在工作机执行 TUI/headless、provider call、工具、git 和 PTY。Android 只是输入、状态投影和审批端。
5. 每个连接绑定 `device_id`、允许的 `project_id` 集合、owner epoch、最近 ack sequence 和 capability 集合；断开不终止工作机上的 Agent。

### 2.2 传输选择

推荐 **QUIC + TLS 1.3**，Unix 端可用 Quinn，Android 端使用成熟的 Kotlin QUIC 实现；若 Android 目标 SDK/库评估后无法稳定提供 QUIC，再退到 TCP/TLS WebSocket，但协议语义不变。

- QUIC 的 connection ID 允许连接在 IP/端口变化时继续，包含 NAT rebinding/网络切换场景。[RFC 9000 §1.1 与 §9](https://www.rfc-editor.org/rfc/rfc9000.html)；这不是 Android 后台保活保证，仍需重连。
- Quinn 的 `max_idle_timeout` 默认是有限值，`keep_alive_interval` 必须小于双方 idle timeout 才有效。[Quinn `TransportConfig`](https://docs.rs/quinn/latest/quinn/struct.TransportConfig.html)。建议活动连接使用应用层 ping 20 秒、idle timeout 90 秒；参数应通过 capability 协商而不是写死在 Android。
- 一条连接至少分为三个逻辑 stream：`control`（命令与 ack）、`events`（结构化增量事件）、`blob/pty`（大输出或附件）。每条消息使用 `u32 big-endian length + UTF-8 JSON`，沿用当前协议的 1 MiB frame cap；后续需要二进制时再按 `content-type` 增加 CBOR/blob，不把 provider token 或任意文件内容塞进日志。
- 业务层沿用已有 `schema_version`, `id`, `session_id`, `request_id`, `turn_id`, `sequence`, `code`, `execution_state`, `required_owner` 字段。QUIC stream 的顺序不能代替业务 sequence：事件、session journal 和 transport replay 是不同游标。

### 2.3 配对、身份与加密

建议先做用户可理解、可撤销的手动配对，再考虑自动发现：

1. Android 第一次启动在 Android Keystore 生成不可导出的设备私钥；Keystore 的 key material 可受授权约束并可由硬件 TEE/StrongBox 保护。[Android Keystore](https://developer.android.com/privacy-and-security/keystore)。
2. 工作机生成持久 host identity 和短期 invite（host public identity、可达地址/candidate、协议版本、过期时间、一次性 nonce、证书 fingerprint）。二维码只包含这些元数据和一次性 token，绝不包含模型/API key/session 全文。
3. Android 扫码后先校验 host certificate/public-key fingerprint，再用设备私钥对随机 challenge 签名；双方显示相同的短校验码，用户点按确认。host 只保存设备公钥、名称、权限和撤销状态。
4. TLS 采用证书/public-key pinning；Android Network Security Config 可配置 custom trust anchor、禁用 cleartext 和 pinning。[Network Security Config](https://developer.android.com/privacy-and-security/security-config)。应用层仍需 nonce、请求 ID、单调 sequence、过期 invite 和重放拒绝，不能只依赖 TLS session。
5. 设备撤销、重新配对、丢手机恢复和“只读/可审批/可执行”权限都必须在 host 端完成。provider credentials 永不下发 Android。

### 2.4 无服务器模式与公网现实

- 局域网：工作机广播 `_zenpi._tcp` DNS-SD，Android 使用 NSD 发现；NSD 只适用于本地网络。[Android NSD](https://developer.android.com/develop/connectivity/wifi/use-nsd)。发现之后仍必须做证书 pin 和配对，不能把 mDNS 名称当身份。
- 同场无路由器：Wi-Fi Direct 可作 nearby fallback，但只解决附近设备，不解决公网 NAT。[Android Wi-Fi Direct](https://developer.android.com/develop/connectivity/wifi/wifi-direct)。
- 公网直连：仅在工作机有可入站 IPv6、用户端口转发/UPnP/PCP 成功，或已有 VPN 时可用。PCP 是“请求上游 NAT/firewall 建立入站映射”的协议，但网络设备可能不支持或拒绝。[RFC 6887](https://www.rfc-editor.org/rfc/rfc6887.html)。
- 两端都在未知 NAT：必须有 candidate exchange/signaling，严格无服务器无法完成可靠打洞。若采用自托管 STUN/rendezvous/TURN，明确在设置中显示“用户自己的基础设施”；relay 只转发加密 QUIC 包，不能读取 session。
- 不要把 Tailscale/DERP/Iroh 默认 relay 称为“无服务器”。以 Tailscale 的说明为例，直接连接失败时会回退到 peer relay/DERP。[DERP 说明](https://tailscale.com/kb/1232/derp-servers)。这可以是可选部署方案，但不能作为用户“不依赖任何服务器”的默认承诺。

“二维码离线交换 offer/answer”只能解决 candidate/signaling 信息如何交给另一端，不能替代 NAT 的入站映射或 relay；在双方都不可达时仍必须失败并给出可执行的网络建议。

## 3. session、context 与断线恢复

### 3.1 谁是事实源

session journal、context、tool side effect、审批状态、worktree 和 scheduler 全部以工作机为 source of truth。Android 只缓存：

```text
host_id, device_id, project_id, session_id, subtab_id
last_event_sequence, last_transport_sequence, context_digest
last_known_capabilities, display cache, pending request ids
```

现有 `SessionHeader`/journal 的 `sequence` 不得与 transport event `sequence` 混用。context compact/checkpoint 应继续由 `context.rs`/session host 计算；Android 只收到必要的摘要、消息和 `context_digest`，不能把本地 UI 缓存当作可提交的完整 context。

### 3.2 握手与恢复

连接建立后按以下顺序：

```json
{"type":"hello","schema_version":3,"device_id":"d1","host_id":"h1","last_event_sequence":812,"session_id":"s1"}
{"type":"capabilities","projects":[],"max_frame_bytes":1048576,"event_head":900}
{"type":"resume","session_id":"s1","project_id":"p1","from_event_sequence":813}
{"type":"event","sequence":813,"session_id":"s1","event":{"type":"..."}}
```

1. host 校验设备、invite/证书、协议版本、project ACL、owner epoch 和 session identity。
2. 若 WAL 覆盖请求范围，重放缺失 event；若已过期，返回 `replay_gap`，随后发送有界 `status/workspace/context checkpoint` 快照，要求 Android 替换缓存而不是猜测中间状态。
3. 所有写命令带客户端 `id`、`expected_project_revision`/`expected_subtab_revision` 和 `session_id`。host 对短期 request ID 做去重；网络超时后 Android 先 replay/status 查询，不能盲目重做 provider/tool/worktree side effect。
4. Android 恢复到前台时通过 `ConnectivityManager.NetworkCallback` 重新获取当前 `Network` 并新建连接；默认网络切换会使旧连接失效。[ConnectivityManager network state](https://developer.android.com/develop/connectivity/network-ops/reading-network-state)。采用全抖动指数退避（1 秒到 60 秒上限），恢复后先补事件再发送新命令。
5. 断线时 host 继续执行已接纳 turn，但 UI 标记 `stale/offline`；未被 host ack 的命令显示“未知结果”，而不是显示失败。审批需要在 host 的 deadline 内显式 deny/expire，不能因 Android 进程被杀而默认 approve。

### 3.3 新增 typed host API（建议）

不要继续扩展仅用于人类输入的 `/worktree` 字符串。增加一个版本化命令，例如：

```json
{"schema_version":3,"id":"r42","type":"subtab","project_id":"p1",
 "action":"add_worktree","name":"review-1","expected_revision":7}
{"schema_version":3,"id":"r43","type":"subtab","project_id":"p1",
 "subtab_id":"st2","action":"set_concurrency","value":4,
 "expected_revision":8}
```

建议动作至少包含 `list/select/add_in_place/add_worktree/close/move/rename/set_concurrency`。响应必须返回新的 revision、完整 subtab entry（稳定 ID、name、root kind、display branch、concurrency、busy state）和对应 event。名称、路径、branch 继续由 host 做 canonicalization 和安全校验；Android 不发送任意 shell 命令替代这些动作。

持久化模型建议把当前按 index 的 `SubTab` 升级为带随机/哈希 `subtab_id` 的 schema v2，并为旧 checkpoint 做迁移。`index` 只用于显示顺序。`close` 应先检查 busy/未提交审批、执行 git remove、确认成功，再删除 durable entry；失败保留 entry 和错误状态。

## 4. Android 生命周期与后台现实

### 4.1 UI 技术与两层 tab

使用 Jetpack Compose Material 3：`PrimaryTabRow` 表示一级 workspace，`SecondaryTabRow` 表示当前 workspace 的二级 worktree。官方 tabs API 的 tab 点击更新 selected state，适合由 ViewModel/`rememberSaveable` 保存选择。[Compose tabs](https://developer.android.com/develop/ui/compose/components/tabs)。

建议交互：

- 一级：横向可滚动的 workspace tabs；末尾独立 `+` 图标按钮打开“路径/名称/权限”对话框；长按或更多菜单执行 select/rename/close。
- 二级：当前 workspace 下的 `main`、in-place、git worktree tabs；`+` 菜单分开“原地隔离”和“新建 git worktree”，避免用户误解。每项显示 branch/status/busy badge。
- 每个二级 tab 的并发用 `−  [1..64]  +` stepper，按钮发送 `set_concurrency` 而非在 Android 本地计数；host 返回 effective value 和 scheduler 状态。并发值保存到 host checkpoint，Android 本地只缓存显示。
- tab 选择、关闭、并发变更、审批、执行中的状态都应有 disabled/pending/conflict/error 状态；网络 stale 时禁止破坏性动作，允许查看已缓存 transcript。

### 4.2 Doze、FGS 和 WorkManager

Android 的后台限制决定了“公网同步复用 session 保活”不能实现为一个永不停止的手机 socket：

- Doze 会暂停网络访问并延迟 jobs/sync/alarms 到 maintenance window；官方明确建议持续消息使用 FCM，而不是依赖应用自己的 persistent socket。[Doze and App Standby](https://developer.android.com/training/monitoring-device-state/doze-standby)。但 FCM 本身依赖服务器，因此不属于严格无服务器模式。
- Android 12+ target 31 在后台启动 foreground service 受限；Android 14 对 while-in-use 权限在后台创建 service 更严格。[后台启动 FGS](https://developer.android.com/develop/background-work/services/fgs/restrictions-bg-start)。
- Android 15 对 `dataSync`/`mediaProcessing` FGS 在后台每种类型合计 24 小时内 6 小时，超时必须在 `onTimeout` 后几秒 `stopSelf`；这不是无限保活方案。[FGS timeout](https://developer.android.com/develop/background-work/services/fgs/timeout)。
- 用户点按“保持连接”且 app 可见时可以启动带通知的 FGS；评估 `connectedDevice` 类型是否符合目标 SDK/权限和 Play policy，并在 manifest 中声明对应 `FOREGROUND_SERVICE_*`。不要借用 `dataSync` 规避限制，更不要为避免 Doze 默认申请电池优化豁免。
- WorkManager 只用于“有网络时 flush command/replay/resume、清理缓存、上报断线状态”。它受 constraints 和系统调度，不能保证低延迟，也不能维持 socket。[Define work with WorkManager](https://developer.android.com/develop/background-work/background-tasks/persistent/getting-started/define-work)。

推荐状态机：`foreground_connected`（QUIC + ping）→ `background_grace`（短暂 FGS/通知，用户可停止）→ `offline_durable`（关闭 socket，host 继续工作，WorkManager 只做机会性恢复）→ 前台后 `resume/replay`。UI 要把 stale、last seen、replay gap 显示出来；不要声称后台始终在线。

## 5. 安全边界与威胁模型

1. **未授权连接**：host identity、TLS pin、Keystore device key、短期 invite、用户 SAS 确认、设备撤销、失败限速。
2. **重放/重复副作用**：一次性 nonce、过期时间、request ID 去重、expected revision、host journal/owner lock；超时后查结果，不自动重跑 git/provider/tool。
3. **路径与 worktree 越界**：所有 cwd/root/path 在 Unix host canonicalize，限制到已授权 project；拒绝 symlink/目录替换和任意 shell 参数。Android 只传名称/结构化动作。
4. **敏感数据泄露**：Android 日志不写 provider token、完整环境变量或未经授权文件；传输与本地缓存都加密；attachments 走有界 blob stream，并以 MIME/路径策略过滤。
5. **审批绕过**：Android 仅显示 host 生成的 diff/approval_id、policy digest、lease；审批绑定 session/turn/owner epoch，连接丢失默认 deny/expire。
6. **恶意/过载客户端**：host 对 frame、prompt、并发、附件、事件 replay 和 worktree 数量使用现有上限或更小的网络上限；每设备 ACL 和项目级速率限制。
7. **TUI/Android 竞态**：所有写操作经同一 owner pool；revision 冲突返回 `conflict` 并要求刷新，不能让 Android 覆盖 TUI 的最近选择。

## 6. 分阶段实施与验收

### Phase A：先统一 host contract

- 新建纯 Rust `remote`/`host_api` 层，把 TUI 的 project/subtab 操作和 owned headless typed command 收敛到同一 trait。
- 加入 `SubtabControl`、stable `subtab_id`、revision/CAS、capabilities、event snapshot；同步补偿 worktree remove 失败和 busy gate。
- 先用 loopback length-framed transport 做集成测试：prompt/stream/cancel/approve、project、每种 subtab action、并发 1/64、冲突、重启恢复。

### Phase B：直连与配对

- Unix 端 QUIC listener、TLS 证书/identity、Keystore device auth、QR/SAS、设备撤销。
- LAN NSD + manual address；测试 IPv4/IPv6、端口转发、地址变化、QUIC path migration、强制断网和重连。
- 记录严格 no-server 的失败原因；不得引入默认公网 relay。自托管 STUN/rendezvous/TURN 作为显式可选 profile。

### Phase C：Android 最小可用客户端

- Compose 两层 tab、ViewModel 状态机、控制/事件/PTY stream、审批/diff、离线 transcript cache。
- 前台连接通知、NetworkCallback、WorkManager resume/replay；Android 12/14/15 真机验证 FGS/Doze/电池策略。
- 不在 Android 打包 provider credentials，不在 Android 执行 shell、git、TUI 或 headless；这些都由工作机 host 完成。

### Phase D：完整 TUI parity 与发布门槛

- 逐项把当前 TUI 命令映射到 typed host capability；任何 `required_owner`, `agent_busy`, `replay_gap`, `conflict`, `unknown_outcome` 都必须有 Android UI 状态。
- 做双客户端（TUI + Android）并发测试、工具审批超时测试、provider 进行中断网测试、worktree dirty/删除失败测试、session journal 恢复和 WAL gap 测试。
- 发布前验收：同一个 session 在 TUI 和 Android 切换后 transcript/context digest 一致；所有副作用至多一次或明确 `unknown`；无可达地址时产品明确失败；Android 被系统杀死后前台恢复能 replay，而非丢失上下文。

## 7. 关键取舍

- **QUIC vs WebSocket**：QUIC 更适合网络切换、分离 stream 和移动端路径变化；WebSocket 更容易实现但通常依赖 TCP/HTTPS 可达入口，不解决 NAT。先锁定 QUIC，保留协议层与传输层分离以便 fallback。
- **Iroh vs 自建 Quinn**：Iroh 的 hole punching/relay/discovery 省工程量，但默认 preset 引入外部发现/relay 语义；严格无服务器需自己配置 disabled relay，并承担 candidate 交换和可达性 UX。第一版用 Quinn + 明确 manual/LAN 模式更容易向用户诚实解释。
- **JSON vs CBOR**：当前协议和测试是有界 JSONL，先使用同一 JSON envelope 加长度帧以降低迁移风险；只有在大规模 event/blob 性能成为证据后再增加 CBOR，不能让 Android 自己解释两套业务语义。
- **“保活”定义**：保活的是工作机 session 和 durable event/context，不是保证 Android 进程或 socket 永生。移动端随时可能进入 Doze/被杀；正确行为是 host 继续执行、Android 前台回来后带游标恢复。

## 参考资料

- 仓库：[`src/core.rs`](../../src/core.rs)、[`src/protocol.rs`](../../src/protocol.rs)、[`src/headless.rs`](../../src/headless.rs)、[`src/project_workspace.rs`](../../src/project_workspace.rs)、[`src/tui.rs`](../../src/tui.rs)。
- IETF：[RFC 8445 ICE](https://www.rfc-editor.org/rfc/rfc8445.html)、[RFC 9000 QUIC](https://www.rfc-editor.org/rfc/rfc9000.html)、[RFC 6887 PCP](https://www.rfc-editor.org/rfc/rfc6887.html)。
- Android：[Compose tabs](https://developer.android.com/develop/ui/compose/components/tabs)、[Doze/App Standby](https://developer.android.com/training/monitoring-device-state/doze-standby)、[FGS background start restrictions](https://developer.android.com/develop/background-work/services/fgs/restrictions-bg-start)、[FGS timeout](https://developer.android.com/develop/background-work/services/fgs/timeout)、[WorkManager](https://developer.android.com/develop/background-work/background-tasks/persistent/getting-started/define-work)、[ConnectivityManager](https://developer.android.com/develop/connectivity/network-ops/reading-network-state)、[Keystore](https://developer.android.com/privacy-and-security/keystore)、[Network Security Config](https://developer.android.com/privacy-and-security/security-config)、[NSD](https://developer.android.com/develop/connectivity/wifi/use-nsd)、[Wi-Fi Direct](https://developer.android.com/develop/connectivity/wifi/wifi-direct)。
- Rust 网络库：[Iroh Builder](https://docs.rs/iroh/1.2.0/iroh/endpoint/struct.Builder.html)、[Quinn TransportConfig](https://docs.rs/quinn/latest/quinn/struct.TransportConfig.html)。
