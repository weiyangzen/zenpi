# Provider/Auth Rust 实施任务与核查记录

日期：2026-09-22。设计唯一来源：[Rust 蓝图 v1.0](Zenpi_Provider_Auth_Rust_Blueprint.md)。
本文只记录执行拆分、命令核查、实现证据和交付状态，不复制另一份设计规格。
代码起点 `1b8d7c6`，实现分支 `feat/provider-auth-rust`；原 PR #11 保持文档范围。

> **接手/复查请看 [`Zenpi_Provider_Auth_Handoff.md`](Zenpi_Provider_Auth_Handoff.md)**：
> 本轮改了什么、验证到什么程度、未完成的部分从哪里接手、CI 为什么是红的。

## 1. RTK 先行规则

已核查本机 `rtk 0.49.0`、Rust `1.96.1`；项目 MSRV 仍为1.88，不因本机编译器升级最低要求。
本机帮助与 [RTK v0.49.0 固定源码](https://github.com/rtk-ai/rtk/tree/b1c0dc00649c50fbe8930f849c800d4d6ca12091) 为执行依据。
上游默认分支是 develop；[核验时 README](https://github.com/rtk-ai/rtk/blob/b748a5f75563f410103551689097650be7210d99/README.md) 仅作背景，不自动升级或安装 hook。
`No hook installed` 提示不影响显式调用；本任务不运行 rtk init、不修改用户全局配置。

普通开发核查使用以下模式；替换目标/测试名后执行，不能照抄占位符。保留 --locked，已缓存依赖时加 --offline。

```sh
rtk cargo check --locked --offline --lib
rtk cargo check --locked --offline --test <target>
rtk cargo test --locked --offline --lib <module_filter>
rtk cargo test --locked --offline --test <target> <test_name> -- --exact
rtk proxy cargo fmt --check
rtk git status --short
rtk git diff --stat
rtk proxy git diff --check
rtk proxy git diff -- <owned_paths>
```

- 只跑修改模块及直接调用链的增量功能测试，不运行 cargo test 全库、--all-targets、--all-features、smoke 脚本或 dev-fixtures 产品模式。蓝图中全量回归清单不是本轮执行授权。
- 每个 worker 负责自身模块测试，返回完整命令、目标/过滤器、实际执行数量、退出码和失败说明。零匹配不算通过；通过编译不算完成接口接线。
- RTK cargo 支持 build/test/check/clippy，不提供 fmt 子命令；fmt 用 proxy。只格式化拥有文件，禁止全仓格式化造成无关改动。
- Cargo/proxy 保留子进程退出码（信号为128+signal）。禁止用 head、tail 或 `|| true` 掩盖测试失败；摘要只用于导航，完整 diff 才用于审阅。
- 出现被省略输出时先用 `rtk recall <hash> --full` 或局部 --from/--lines，避免为取日志重新执行副作用。recall 有容量/保留期限，不能当永久测试收据。
- 不同 worker 共享 target，Cargo 的锁由 Cargo 管理。确认已有 session ID 后继续等待同一进程，不因等待超时另开重复构建；不杀其他工作进程。

依据：[Cargo wrapper](https://github.com/rtk-ai/rtk/blob/b1c0dc00649c50fbe8930f849c800d4d6ca12091/src/cmds/rust/cargo_cmd.rs)、
[退出码](https://github.com/rtk-ai/rtk/blob/b1c0dc00649c50fbe8930f849c800d4d6ca12091/src/core/utils.rs)、
[recall 配置](https://github.com/rtk-ai/rtk/blob/b1c0dc00649c50fbe8930f849c800d4d6ca12091/docs/guide/getting-started/configuration.md)。

### 1.1 真实认证调试的例外

RTK 不是脱敏器：proxy 仍记录命令参数/cwd/统计，默认 recall 可能保存原始错误输出；关闭遥测不等于关闭本地记录。
真实登录、API key stdin、token 刷新调试使用原生命令，不经 RTK/proxy/recall；前置下列变量防止 hook 重写和记录：

```sh
RTK_DISABLED=1 RTK_TELEMETRY_DISABLED=1 RTK_RECALL=0 zenpi <真实认证子命令>
```

不得把 token 写在 argv/URL/调试日志，不读取真实 auth.json 或转储环境；绕过 RTK 也不代表终端和 shell 没有记录。
普通功能测试只使用临时目录内的合成凭据，可走 RTK。真实服务调试必须区分“功能测试通过”和“真实账号/端点已验证”。
依据：[tracking](https://github.com/rtk-ai/rtk/blob/b1c0dc00649c50fbe8930f849c800d4d6ca12091/src/core/tracking.rs)、
[recall](https://github.com/rtk-ai/rtk/blob/b1c0dc00649c50fbe8930f849c800d4d6ca12091/src/core/retriever.rs)、
[遥测](https://github.com/rtk-ai/rtk/blob/b1c0dc00649c50fbe8930f849c800d4d6ca12091/docs/guide/resources/telemetry.md)。

## 2. 起点命令与实现核查

### 2.1 import-codex 当前能做什么

**可导入 API key/provider/model/endpoint；不能接入 ChatGPT OAuth 订阅认证。**

```sh
zenpi config import-codex --profile codex
zenpi config doctor --profile codex --json
zenpi --profile codex
```

以上是实际已有命令，不代表本轮已运行真实导入/请求。已核查 `zenpi --help` 与源码；子命令当前不接受 --help/--json/--yes 或 --profile=NAME，只有分开的 --profile NAME。

| 事项 | 当前实现事实 |
|---|---|
| 来源/目标 | CODEX_HOME 或 ~/.codex；ZENPI_HOME 或 ~/.zenpi；本轮不读取这些真实文件 |
| 配置 | import_codex_from_root 读取根 model_provider/model 和 provider 表；不解析 Codex profile/env_key/keyring/command auth |
| key | find_api_key 仅取 OPENAI_API_KEY 或明确 provider/profile key；忽略 tokens.access_token/refresh_token |
| 无 profile 导入 | 合并源中存在的平面字段；无源 key 保留旧 key，不改已有 default_profile |
| named profile 导入 | 整表替换并设为默认；无源 key 仍保留该 profile 旧 key |
| 幂等/事务 | 同内容不重写；config/auth 分两次持久化，不是跨文件事务 |
| 成功的含义 | 导入分支不以 is_ready 阻断；OAuth-only 可 exit0，但不等于已连接，启动可能失败 |
| 风险边界 | 新 endpoint + 保留旧 key、隐式默认切换、缺默认 endpoint 等在 PA07 明确处理，不能把旧语义带入新 OAuth 登录 |

调用链：core::parse_args -> config::import_codex[_profile] -> import_codex_from_root/find_api_key -> 分别持久化 -> 脱敏回执。
启动链：core::make_backend -> config::resolve_workspace/resolve -> backend_from_effective -> OpenAiCompatibleBackend。
证据：[config.rs](../src/config.rs)、[CLI/core](../src/core.rs)、[现有导入测试](../tests/config.rs)。
API key 解析优先级仍为显式 override > ZENPI_API_KEY > OPENAI_API_KEY > auth_env > 当前 profile/root key；named profile 不回退根 key。

### 2.2 基线证据

| 核查 | 结果 |
|---|---|
| 工作树 / 分支 | 起点干净；从纯文档提交创建独立实现分支 |
| rtk --version / --help / cargo test --help / proxy cargo fmt --help | 已核查上述参数存在 |
| rtk proxy cargo metadata --offline --no-deps --format-version 1 | 单 crate；lib+bin；target 位于仓库 target/ |
| rtk proxy cargo check --locked --offline --lib | exit0；首次构建21.09秒；vendor/crossterm 已有 unused_parens 警告，不做无关修复 |
| import-codex API key/OAuth | 源码和帮助核查完成；3条 focused tests 已执行，见第6节；真实凭据未读 |

## 3. 子任务与所有权

状态只允许 pending / implementing / verified；verified 必须同时具备代码接线和对应增量测试证据。live-gate 独立，不用本地 fixture 代替。
下表是完整实施范围，不以第一批基础模块完成冒充整体完成。每次派发明确文件所有权；core/backend/config/lib.rs 等共享文件串行交接。

| ID | 依赖 | 子任务/文件边界 | 设计/验收 | 当前实现核查 | 状态 |
|---|---|---|---|---|---|
| PA00 | 无 | 本文 RTK、命令/源码核查、导入基线 | P0 / A01 | 命令与源码核查完成；3条 Codex 导入功能测试通过 | verified |
| PA01 | PA00 | 协议提取：backend、protocols、原 providers/anthropic/google；保留兼容入口，不改 Agent Loop | C13 / A03/A12 | 四 wire 已接共享编解码；75条集成+3条单元通过；不代表 Codex 新 dialect 完成 | verified |
| PA02 | PA00 | 静态 provider 定义、connection 路由与 config 字段；一 provider 一定义文件 | C08/C14 / A01-A03/A13 | 显式配置已接 Core/backend factory，匿名配置到生产 headless 的功能链路已测；凭据路由/跨模型验收见6.6 | implementing |
| PA03 | PA00 | auth/mod/store：私有 DTO、文件事务、稳定 OS 锁、CAS/tombstone/刷新 marker | C01/C03/C11 / A08-A10 | 存储模块30条测试通过；登录库、请求 resolver 与 legacy writer 均已消费同一把稳定锁，合并门已闭合 | verified |
| PA04 | PA01/PA02 | security SecretHandle expiry/scope；Backend RequestControl 与每 HTTP 准入 | C12/C15 / A03/A16 | 已接两个 Core 请求入口、每个 HTTP retry 和 PA06 刷新准入；scope/期限/撤销/预算已测，动态选择 scope 待 PA09 | implementing |
| PA05 | PA03 | auth/codex/callback：PKCE、浏览器/manual/device、取消/限长/提交 | C04-C07 / A04-A07 | 登录库与 callback 共23条测试通过；CLI 入口已由 PA07 接上，但**未经真实账号验证**，不代表用户可登录 | implementing |
| PA06 | PA03/PA04/PA05 | auth/resolve：每请求鉴权、5分钟预刷新、同身份401恢复、撤销 | C02/C12 / A07-A10/A16 | resolver27条及 HTTPS 集成14条通过；已接生产请求，初始在途刷新/终态分类也已验证；不代表 live OAuth 资格或登录 CLI 完成 | verified |
| PA07 | PA02/PA03/PA05 | config/core CLI：auth list/add/doctor/revoke、import-codex 明确来源/边界、默认值兼容 | C15 / A01/A17 | 命令已落地并经合成凭据端到端验证；legacy 写回并入同一稳定锁；**真实 OAuth 登录与真实服务调用仍未验证** | verified |
| PA08 | PA01/PA02/PA04/PA06 | Codex/DeepSeek request dialect、headers/SSE/tools/reasoning/错误；唯一推理重试 | C09/C14 / A11-A13/A16 | route-aware codec/headers 与唯一401重试循环已接线，HTTPS14条/协议15条测试通过；opaque history 仍待 PA12 | implementing |
| PA09 | PA02/PA06/PA08 | Core/session 原子连接选择、owner/queue 栅栏、scope、选择事件恢复 | C15 / A18/A19 | `Agent::select_connection` 已落地：owner/phase/队列/附件/审批/worker/未决 operation 栅栏 → revision 复核 → 候选校验 → 单条 durable 事件 → infallible swap；拒绝不改运行状态。恢复拒绝把保存的选择套到另一个 profile/credential 上。**TUI/headless 的宿主接线分属 PA10/PA11** | verified |
| PA10 | PA07/PA09 | TUI bootstrap/auth 任务/菜单；复用宿主状态机，迟到 callback/取消 | C15 / A17 | **部分完成**：`tui/bootstrap.rs` 的 `auth_gate` + `run_auth_bootstrap` 已接入 TUI 启动前，只捕获明确未配置/需登录，坏 TOML 与权限错误如实抛出；登录复用 CLI 同一个 driver，不建第二套状态机。**未完成**：运行中会话的 `/auth`、`/login`、`/profile` slash 入口，以及受控后台任务登录 + 取消回收 listener；引导界面目前是终端提示而非 ratatui 私有界面 | implementing |
| PA11 | PA09 | protocol/headless v2 connection 控制/capability/回执/replay；无秘密 JSONL | C15 / A18/A19 | `connection_v1`/`ordered_content_v1` 已在 status 声明；`type:"connection"` 的 select/status 严格校验、data/error 互斥、request ID 幂等复用既有 replay cache；**ordered_content_v1 的行为面仍待 PA12** | verified |
| PA12 | PA01/PA02/PA09/PA11 | 有序内容 DTO、ToolResult、Core 物化、协议映射、journal/compact/resume | C10/C15 / A14/A15/A19 | **部分完成**：`zenpi_content_v1` 有序 DTO、`InputContentPart` 公共输入、逐 turn 有序快照落盘、采纳时的跨账号/限额/版本校验、`ToolResult::Success` 可选 typed content 均已落地并测试。**协议编码侧未接线**：编码器仍只发 `turn.content`，typed content 不会到达模型；headless `prompt.content` 的 v2 有序输入入口也未实现 | implementing |
| PA13 | PA07-PA12 | 路径链路调试文档、命令再核查、用户真实调试与版本收据 | P6 / A20 | `Docs/Zenpi_Provider_Auth_Debugging.md` 已交付；文中命令均在 release binary 上实跑过，未实现项（TUI 引导、有序内容输入、工具 typed content 送达）显式标注 | verified |

PA01 可先完成 native 提取再迁 Chat/Responses；PA02 可先完成定义/路由再接 config。阶段性证据分行记录，不提前将整行标 verified。
第一批允许 PA01/PA02 的不重叠文件与 PA03 并行；依赖方以真实模块 API 集成，不复制另一套 enum/store/loop。
新增第三方依赖先核验锁文件/MSRV；保留同步 ureq，不添加 Node/Bun、Tokio、Broker、数据库或动态 provider 插件层。

### 3.1 首批共享边界

- 协议 worker 独占 backend.rs、backend/chat_stream.rs、protocols/**、providers/anthropic.rs/google.rs。原公开 CAPABILITIES 路径保留转发；新 provider 定义先由 providers/mod.rs 的静态分发连接，之后在同一拥有者交接时收进品牌文件。
- 定义/连接 worker 独占 providers/mod.rs、providers/connection.rs/openai.rs/codex.rs/deepseek.rs。先交付可测试的纯定义/路由模块，不改 native 文件、config/core/backend；PA02 的配置与生产接线仍保持未完成。
- 存储 worker 独占 auth/mod.rs/store.rs。AuthBinding 是此模块的唯一枚举：LegacyApiKey、StoredApiKey{credential_id}、CodexOAuth{credential_id}、Anonymous；不在 connection 再复制。
- auth 模块提供非秘密 AuthIdentitySnapshot（provider、credential ID、account ID 可选、identity_generation、credential_revision、allowed_destinations）。AllowedDestination 明确 origin、path_prefix、允许 protocol/header 字符串集合；连接模块负责规范 URL 与规则匹配，store 不做网络。
- CredentialStore 使用显式 auth 文件路径，不自行读取真实 HOME；私有 token DTO 无公开 Debug/Serialize，只能专用持久化。Keep/Replace/Revoke 与 committed/failed/uncertain/CAS 结果按蓝图，不静默丢 legacy 根字段。
- parent 只负责 lib.rs 注册、必要 Cargo 依赖与整体集成。各 worker 不改 Cargo/lib/core，代码就绪先通知 parent 注册与检查交集后，再执行自身增量测试；不把尚未接生产的模块称功能完成。

上述边界只用于首次并行批次；native 品牌文件已在 PA01 完成后交给 PA02，定义现已移回各品牌文件，不再留 mod.rs 临时表。
下一批接 config/运行时前，按已实现类型更新派发，不长期保留临时兼容复制。

## 4. 每项增量验证

以下为允许的命令目标，非一次执行脚本。现有 target 已核查；新单元 filter/target 标为待实现，落地后先确认确实执行了测试。
所有测试使用临时目录/本地协议服务验证真实生产 Rust 路径，不调用 smoke runner，也不把假凭据结果当真实账号验收。

| 子任务 | 命令模板 / 精确现有用例 | 核查要求 |
|---|---|---|
| PA00/PA07 | rtk cargo test --locked --offline --test config pairing_imports_codex_and_is_idempotent_without_leaking_key -- --exact | 现有；需执行1条，验证字节幂等/权限/脱敏 |
| PA00/PA07 | rtk cargo test --locked --offline --test config cli_import_honors_codex_home_as_the_profile_root -- --exact | 现有；隔离 CODEX_HOME，不访问用户源 |
| PA00/PA07 | rtk cargo test --locked --offline --test config codex_import_ignores_unrelated_nested_credentials -- --exact | 现有；不得复制 OAuth token |
| PA07 | rtk cargo test --locked --offline --lib config::legacy_lock_tests | 新 filter；legacy 写回与 refresh 串行、不覆盖最新 namespace、no-op 不写盘 |
| PA07 | rtk cargo test --locked --offline --test config | 新增命令端到端：临时 HOME 内合成 key 的 add/list/doctor/revoke；不含真实凭据 |
| PA07 | rtk cargo test --locked --offline --lib providers::connection | 登录 grant 与请求路由同源，Codex/非 HTTPS/未知 header 拒绝 |
| PA01 | rtk cargo test --locked --offline --test stage1_anthropic；对应 stage1_gemini/stage1_chat_stream/backend | 仅迁移涉及的 target；检查 body/headers/history/SSE，不跑全库 |
| PA02 | rtk cargo test --locked --offline --lib providers::connection；--test config --test stage1_project_config | 路由24条；两个配置 target 32条；端点/auth/能力拒绝零网络 |
| PA03 | rtk cargo test --locked --offline --lib auth::store | filter 已有22条（含1个子进程 helper）；覆盖子进程锁/落盘故障/CAS/保留 legacy 字段 |
| PA04 | rtk cargo test --locked --offline --test security；对应 backend/provider_admission；--lib governance::provider_tests | per-send/expiry/scope、owner/worker 双预算、原 admission 期限与恢复 |
| PA05 | rtk cargo test --locked --offline --lib auth::codex；对应 auth::callback | filter 已落地；控制时钟/取消、loopback、device 参数/状态；真实账号仍未验证 |
| PA06 | rtk cargo test --locked --offline --lib auth::resolve | 新 filter 待实现；刷新只一次、同身份、未知结果 fail closed |
| PA08 | protocols 中新增 Codex/DeepSeek 精确 filter；相关 backend 功能测试 | 新 filter 待实现；对端捕获完整 URL/header/body，验证终态与401预算 |
| PA09 | session_recovery / stage1_reasoning_owner / stage1_semantic_compaction 中关联精确测试 | 原子提交/replay/owner 隔离；不能只测 setter |
| PA10 | stage1_host 中新增认证/bootstrap/选择精确测试 | 不启动资源任务；UI Applied 后才改变 |
| PA11 | headless_protocol 中新增 connection 精确测试 | 用生产 binary JSONL；严格 envelope、错误/data 互斥、跨项目/会话 |
| PA12 | 新有序内容/工具内容测试 + session_recovery 关联测试 | 输入 -> Core -> wire -> journal -> resume；跨账号拒绝，不隐式丢图 |
| PA13 | 实际 --help/doctor/status/配置链路 + 用户真实服务调试文档 | 不运行全量/echo/smoke，不假称用户 live 已通过 |

任何共享 crate 编译失效先定位当前拥有者，不能 revert 他人改动。验证失败保留错误/命令，修复后重跑同一小范围。
编译检查与新增测试数量不计作产品验收；每个任务必须核对从命令入口到真实请求/存储/恢复的调用点。

## 5. 交付与真实调试边界

用户已授权修改 Rust 并要求完成后提交实现 PR。实现与增量验证完成后单独提交，不混入原文档 PR #11；不得将未接线的库或未完成任务宣称为整体完成。

开发完成后在 `Docs/Zenpi_Provider_Auth_Debugging.md` 交付可复制的真实链路说明，至少包括：

1. 当前版本/构建路径/installed binary 差异；使用本次实际 binary，不拿旧安装的帮助证明新命令存在。
2. API key 型 Codex import、OAuth-only import 的明确结果、独立 browser/device 登录；不读取或打印 token。
3. profile -> provider -> protocol -> base URL -> credential -> request 的非秘密定位方法与实际可用命令。
4. Codex TUI/headless 发请求、工具续接、刷新/401、取消、退出/恢复、换连接/换账号；定位每一层错误而非只看 exit0。
5. DeepSeek 三协议的独立 profile 与同凭据目的地授权；真实文本/工具/图片/跨 turn 恢复，并区分模型能力。
6. OAuth endpoint 限制、media scope、撤销共享凭据与恢复不确定结果；禁止危险重试与旧 token 备份回滚。
7. 用户填写实际模型/账号权限/地域/运行日期及失败证据；私密挑战不入日志。未运行的 live 项目显式待用户验证。

本目标不自动使用用户账号、读取 auth.json、授权外部 OAuth client 或发付费模型请求。真实服务许可/套餐需用户侧确认，文档不以 smoke 数据替代。
也不自动将开发 binary 覆盖已安装版本；最终给出针对该分支的确切构建/运行命令，实际安装另行明确。

## 6. 实施记录

| 日期 / 子任务 | 实现与命令证据 | 结果 |
|---|---|---|
| 2026-09-22 / PA00 | RTK/导入调用链/CLI 参数核查；增量 lib 基线编译 | 编译 exit0；导入功能测试未执行，其他任务尚未验证 |
| 2026-09-22 / PA00 | rtk cargo test --locked --offline --test config codex_ | exit0；3 passed / 15 filtered，1.02秒；临时目录功能测试，不是 live/smoke |
| 2026-09-22 / 依赖核查 | 直接声明已锁定 getrandom0.2.17/url2.5.8；cargo metadata --locked --offline --filter-platform aarch64-apple-darwin --format-version 1 | exit0；锁文件仅根包新增2条依赖，不升级包。未限定平台的 metadata 因未缓存 Windows 依赖失败，不算通过 |

### 6.1 历史暂停点（已重新授权）

安全审查仍以先前已结束的“仅文档”范围拒绝 PA01 的源码 patch，随后也拒绝撤回该 worker 的未完成机械迁移。
当时已停止后续源码写入/测试，向用户请求确认实施权限，没有改用其他工具绕过拒绝。
2026-09-22 用户明确授权“允许修改 Rust 源码，并先修复本次未完成迁移”；现恢复原拥有者的修复工作。
暂停时工作树包含未接线的协议迁移和 provider 定义；上述编译/3条导入测试证据均来自迁移前基线，不能作为修复后通过的证据。

- PA01：旧 backend/chat_stream.rs 原文在 protocols/chat.rs；原 native 文件原文在 protocols/anthropic.rs/google.rs，旧 native 路径仅转发 CAPABILITIES；protocols/mod.rs 尚未注册实现。
- PA02：providers/mod.rs/openai.rs/codex.rs/deepseek.rs 已写，但 connection.rs 尚未创建，定义模块未闭合。
- PA03：auth/mod.rs/store.rs 初始实现已写，尚无新增测试、模块注册或编译验证；worker 已暂停，无后台执行会话。
- 三份迁移原文经 worker 逐字节核查与 HEAD 相同，没有丢失；授权明确后先修复这次未完成迁移，再继续实施/测试，不提交半成品为完成结果。

### 6.2 暂停期间的只读复核

parent 再次独立比较 Git HEAD 原文件与迁移后文件，三份内容逐字节一致；backend.rs 也未变。
这证明源码内容没有在迁移中丢失，不证明模块接线或运行时通过。没有为本次复核重跑构建/测试，也没有通过其他工具重试被拒绝操作。

| HEAD 原路径 | 暂停时保存路径 | 字节数 | SHA-256 |
|---|---|---|---|
| src/backend/chat_stream.rs | src/protocols/chat.rs | 25466 | ecdb621613da119ab036afee540c3ce8a9735c758681411a8e3750d66423805d |
| src/providers/anthropic.rs | src/protocols/anthropic.rs | 33692 | 92b9b1f36fdfc37d6a1d1f260ed972e7e3bacd61c319ff6ba4ef757b825b4906 |
| src/providers/google.rs | src/protocols/google.rs | 29333 | f0d56a364cda38b5a10bff74e68057ad76bf539000c883ee6e820c2105081890 |

暂停时具体断点：backend.rs 仍声明已不存在的 chat_stream；native 转发引用未注册的 protocols；providers/mod.rs 引用了不存在的 connection.rs；auth 尚未注册。
恢复后先在原拥有者范围内完成这些引用，确认可编译，再按 PA01/PA02/PA03 分别验证；不能仅补一个空 connection.rs 让编译过关。
Cargo.lock 核查仅根 zenpi 的依赖列表增加 getrandom/url，两者版本未改变；没有新增测试结果或新功能完成声明。

### 6.3 PA03 接线前审阅问题

以下由独立子智能体只读审阅、parent 检查对应实现确认；现已修复并复审，实际验证见6.4。此表保留修复前的问题与验证目标。

| ID / 优先级 | 修复前代码问题 | 修复与增量验证目标 |
|---|---|---|
| AR01 / P1 | auth/store.rs 的 PrivateCredential 为 crate 可见且 derive Serialize；CommittedCredential 的内部字段也 crate 可见，其他模块可修改秘密后自行包装“已提交”凭据 | committed 构造封闭在 store；运行时秘密对象与私有持久化 DTO 分离。核查模块边界不能自行构造 committed，不能通用序列化秘密快照；只有持久化确认路径可发放 |
| AR02 / P2 | secure_fs::Directory::replace 把全部 renameat 错误映射 StorageFailed，未区分无法证明目标未改变的 I/O 结果 | rename 前确定失败与提交结果不明分开；后者 CommitUncertain 并重读核对。故障注入验证目标已替换但结果不明时不发放凭据、不重做 code exchange |
| AR03 / P2 | StrictValue 先构造完整 JSON，Namespace 再反序列化 accounts，最后才检查128账号上限 | 为 namespace/accounts 做有界 visitor，第129个键即拒绝、不读取其大型值；保留 legacy 根字段兼容。验证128/129边界与拒绝时机 |

暂停时未注册/未测试的问题已经修复；尚未接入登录和生产请求，不能将存储模块测试通过等同于完整认证功能验收。

后续在本表记录每个 worker 的所属文件、实际测试命令/数量/退出码及集成差异，不另建散落的 smoke 收据或重复任务规格。

### 6.4 本次迁移修复与增量验证

用户明确授权修改 Rust 后，完成以下恢复，不通过空模块或回退他人代码绕过断点：

- PA01：Backend 实际调用 protocols::encode_request/read_response；Chat、Responses、Anthropic、Google 编解码及共享附件辅助函数迁入 protocols。保留公共 Backend/OpenAiWireApi、native CAPABILITIES 路径及 legacy 默认值、header、重试、取消 transport。content.rs 仅迁移既有辅助函数，不表示 PA12 有序多模态已完成。
- PA02a：补齐 connection.rs 的 ModelRoute/ProviderConnection/私有构造 ValidatedRoute，先生成完整 URL 再检查 origin/path/protocol/header 授权；固定 Codex OAuth 目的地。openai/codex/deepseek/anthropic/google 各一个定义文件。精确模型路由、能力交集和身份摘要为纯本地计算，不做 HTTP。
- PA03：注册 auth 模块；秘密 DTO 及 committed 构造私有，运行时秘密对象不支持 Serialize/Debug；修复 macOS libc mode 类型；实现权限/no-follow、稳定锁、CAS/tombstone、刷新 marker、有界 JSON 读取和账号/字符串限制。
- 提交核对：rename 尝试后的不明错误为 CommitUncertain；读回比较完整规范化内容而不只看 revision。读回同 revision 但内容不同、rename 后报错等情况不发放 committed 对象。
- 并发修复：首次创建刷新锁时观察到 openat 返回 ENOENT；预建锁可消除该失败，但底层 OS 原因未证实。最终只对 create=true + NotFound 沿原目录 FD/no-follow、受取消与期限约束重试；正式测试不预建锁，不 truncate/unlink 稳定锁文件。

| 责任 / 实际命令 | 最终结果 |
|---|---|
| PA01 / rtk cargo test --locked --offline --test backend --test stage1_chat_stream --test stage1_anthropic --test stage1_gemini | exit0；75 passed，4 suites，6.82秒 |
| PA01 / rtk cargo test --locked --offline --lib protocols:: | exit0；1 passed，224 filtered |
| PA01 / rtk cargo test --locked --offline --lib backend::tests | exit0；2 passed，223 filtered |
| PA02a / rtk cargo test --locked --offline --lib providers::connection | exit0；24 passed，203 filtered |
| PA03 / rtk cargo test --locked --offline --lib auth::store | exit0；22 passed，205 filtered，0.48秒；包括1个子进程 helper |
| PA03 / rtk cargo test --locked --offline --lib auth::store::tests::subprocess_peer_refresh_rechecks_after_lock_and_rotates_once -- --exact | 原首次创建场景额外连续3次 exit0；每次1 passed、226 filtered |
| parent / rtk cargo test --locked --offline --test config codex_ | exit0；3 passed，15 filtered；迁移后再验证 API key 导入兼容 |
| parent / rtk cargo check --locked --offline --lib --bin zenpi | exit0；库和二进制均可编译；仍有尚未接生产的 dead_code 与既有 vendor 警告，不声称零警告 |
| parent / rustfmt --check（仅本批17个 Rust 文件，skip_children=true）及 git diff --check，均经 rtk proxy | exit0；没有全仓格式化 |
| parent / 三份本批文档的相对文件链接检查 | 12个链接全部存在；历史源码链接固定到设计审阅提交，避免迁移后行号误指 |

中间失败未隐去：第一次编译因 Darwin mode 类型失败；存储首次测试为21 passed/1 failed，定位并修复锁创建后才得到上表结果。
各 filter 的 filtered 数不同是并行新增测试导致，不代表全库运行；合计127个 harness 测试条目通过（其中1个是子进程 helper），另有3次精确并发复测。
AR01-03 经另一 worker 只读复审；parent 额外核查协议分发、路由授权与完整读回比较。所有测试只使用临时合成凭据、本地协议服务或真实子进程，未使用真实账号、全量/smoke/dev-fixtures 模式。

本检查点只恢复未完成迁移并验证基础模块。PA02 的 config 字段/生产 Backend 消费、PA04-PA12 的登录/每请求认证/TUI/headless/多模态仍未实现；Codex/DeepSeek 新 dialect 的参数和 JSON object/schema 区分仍属 PA08。
新模块的 dead_code 警告明确反映这个边界，未用全局 allow 隐藏。Linux/Windows 未验；非 Unix 新凭据持久化 fail closed。没有安装覆盖用户 binary、发真实模型请求或发布实现 PR，原 PR #11 保持文档范围。

### 6.5 配置、逐 HTTP 准入与登录库

本批仍是增量实施，不是全部蓝图完成。新增/修改职责如下：

| 所属文件 / 函数 | 实际行为与边界 |
|---|---|
| config.rs：resolve / EffectiveConfig::provider_connection | 接 auth_method/ref/header 与精确 model_routes；显式认证不读取旧 auth、环境 key 或 Codex fallback，不混用 CLI API key。项目配置不能选择显式账号或降级用户的显式默认配置；纯 legacy 选择保持 |
| config.rs：status/doctor | 显式配置显示 unresolved，不凭字段齐全就宣称 ready。Core 的 backend_from_effective 暂时明确拒绝显式配置，待 PA06 替换该守门代码，不能以环境 key 偷渡成功 |
| backend.rs：RequestControl / complete_with_request_control | 普通请求、工具续接、压缩均走新方法；每次物理发送重查取消、期限、scope 与 owner 准入。禁止自动重定向，3xx 不跟随/不重试；响应事件已交付后不重放。唯一原重试器保留 |
| Backend trait / Echo | 默认受控方法返回 controlled_request_unsupported，不能伪装成只计一次的 legacy 调用。Echo 显式零 HTTP；测试本地替身也逐个显式声明，测试辅助宏不进入产品代码 |
| core.rs：provider_request_scope / provider_request_deadline / admit_provider_send | 冻结 owner/session/operation/purpose/route/identity/policy/lease；复用 InputPump 单 writer。逐发送持久计数，取消覆盖等待 HTTP 与 backoff；期限取原全局/worker wall/lease 最小值。correlation-only worker metadata 不冒充真实 grant |
| governance.rs：charge_provider_request | 复用原 worker admission，新增 lease 下不可回退的 provider_requests 次数；不伪造零 wall/concurrency reservation，不提前结算 HTTP。原 admission 必须活跃、未结算、未撤销、无未知结果且未超时；同 item 换 lease 不重置预算 |
| security.rs：SecretScope / new_scoped / with_scoped_secret | route/identity/policy 及到期时间共同约束借用，锁内重新检查；旧 with_secret 不能绕过 scoped handle。当前 legacy route identity 不代表 OAuth account，真实凭据 resolver 属 PA06 |
| auth/mod.rs：LoginControl / LoginRequest / LoginOutcome | 登录与模型预算分离；可信私有交互，不序列化秘密 challenge；总期限最多15分钟，取消/flow ID/发送预算受控 |
| auth/codex.rs：begin_* / login_* / dispatch | PKCE/state、browser/manual、device begin-poll-exchange、固定端点/表单、token/account/expiry 解析；复用 transport，单 HTTP 最多15秒、更短期限优先、256KiB 上限、无自动重定向。store commit/readback 成功才返回 LoginSuccess |
| auth/callback.rs：ManualSubmission / CallbackServer | 固定 loopback1455，不接管其他进程；有界非阻塞 listener。只接完整匹配 URI/state，错误 state 不消耗 winner，正确 OAuth error 终结，manual/callback 只能一次成功 |

配置/登录/refresh 的接线不得混淆：登录库已调用 store，但 CLI/TUI 尚未调用登录库；refresh_request 目前只有固定表单编码，生产刷新、guarded reload、401 恢复仍未实现。AuthRefresh 的 Core 测试只证明控制合同共用预算，不冒充实际刷新 HTTP 收据。

本批审阅发现并修复：

- worker HTTP 原尝试使用零时间/零并发 reservation，被既有账本正确拒绝；改为原 admission 内专用不可退回计数，保留通用 reservation 校验。
- summary 的 scope 校验原在 begin_operation 后，拒绝会留下悬挂操作；现提前到 token/cost 预留及 begin_operation 前，并验证零 HTTP、无待恢复记录。
- 默认 Backend 新方法原只调用一次 hook 再委托 legacy，无法保证其内部 retry 计数；现默认拒绝，真实 HTTP Backend 与 Echo 分别显式实现。
- 工具续接功能测试发现既有 persist_tool_invocation 在第一个 tool 结束后结算了整个 worker，后续模型请求失去原 admission。现工具只结束自身 operation；沿用公开 settle_blueprint_worker，由 host 确认整个 worker 终态/reap 后结算，持久化成功才清对应 ID。测试覆盖续接成功和正式结算后拒绝再发，不放宽已结算 admission 的限制。
- token error 的嵌套重复 code 原有 last-wins 歧义；改 typed string/object DTO 拒绝重复字段。device 的3xx不能靠 pending/slow_down 正文继续轮询；driver 测试确认没有下一次 poll/兑换。
- macOS 非阻塞 listener 接受的测试 socket 仍非阻塞，首次夹具读出现 WouldBlock；测试 peer 显式切回 blocking，不修改生产 callback 的非阻塞机制。一次 parent 测试在 Drop 二次 panic 导致 SIGABRT，夹具现保留原 panic 并 join，不再遮蔽最初错误。

快照兼容边界：旧 worker snapshot 缺 provider_requests 时读取为零；空 map 不写新字段。出现非零新计数后，旧 binary 会因严格未知字段拒绝该快照，不能宣称可降级运行。新版本拒绝后续删除/降低计数的快照，不用旧 binary 或备份回滚规避预算。

实际执行收据如下；重复命令是修复后复验，不相加成去重测试总数。不同 filtered 数来自并行新增单元测试。

| 责任 / 实际命令 | 结果 |
|---|---|
| PA02b / rtk cargo test --locked --offline --test config --test stage1_project_config | exit0；32 passed / 2 suites |
| PA04 / rtk cargo test --locked --offline --test security --test backend | exit0；35 passed，security10 + backend25；默认 trait 修正后的 backend 单独再验如下 |
| PA04 / rtk cargo test --locked --offline --test backend | exit0；26 passed；默认不支持拒绝、Echo 零 HTTP、302/307 不跨域 |
| PA04 / rtk cargo test --locked --offline --lib security:: | exit0；5 passed；注入时钟验证锁内到期/撤销/scope |
| PA04 / rtk cargo test --locked --offline --lib governance::provider_tests | exit0；8 passed；parent 加空 map 省略规则后再验仍8 passed |
| PA04 / rtk cargo test --locked --offline --test governance | exit0；22 passed；受控测试替身变更后的恢复计数另验1条 |
| parent / rtk cargo test --locked --offline --test provider_admission | exit0；最终12 passed，含真实重试/等待 HTTP 取消、双预算、summary 拒绝、工具续接、Echo 零 HTTP |
| parent / rtk cargo test --locked --offline --test provider_admission --test stage1_semantic_compaction | exit0；当时 provider_admission 为11条；RTK 汇总27 passed、2 ignored、16 filtered / 3 suites（含子进程输出）；两个 ignored 是仅由指定父用例拉起的 helper，不是跳过产品失败 |
| parent / rtk cargo test --locked --offline --test core_session worker_ | exit0；11 passed / 27 filtered；原 worker admission/correlation/权限回归 |
| PA05 / rtk cargo test --locked --offline --lib auth::codex | exit0；最终14 passed / 248 filtered；driver 对重复 error.code/3xx 无下一次发送 |
| PA05 / rtk cargo test --locked --offline --lib auth::callback | exit0；8 passed / 254 filtered；loopback/占用不接管/大小与 winner 边界 |
| parent / rtk cargo test --locked --offline --test config codex_ | exit0；5 passed / 20 filtered；原3条导入回归 + 新2条显式配置/Codex fallback 检查；仍不导入 OAuth token |
| parent / rtk cargo check --locked --offline --lib --bin zenpi | exit0；0 errors；155 warnings，主要为未接入口的认证/路由代码及已有 vendor 提示，未隐藏 |
| 各 worker owned-file rustfmt / parent 对本批14个核心与测试文件 rustfmt --check（skip_children=true） | exit0；不全仓格式化，新 untracked 文件也在检查中 |
| parent / rtk proxy git diff --check | exit0；仅反映 tracked diff，不替代上面的 untracked 源文件检查 |
| parent / 三份任务文档相对文件链接核查 | exit0；12个链接存在，移除因 Core 实施漂移的旧行锚点 |

受控 trait 默认拒绝后，以下实际测试替身调用路径逐个增量验证；共同命令前缀为 `rtk cargo test --locked --offline --test`，未执行这些大 target 的全量回归：

| Target / filter | 结果 |
|---|---|
| core_session provider_tool_calls_execute_and_continue_until_final_text | exit0；1 passed |
| governance resuming_a_session_reloads_governance_from_the_replacement_journal | exit0；1 passed |
| headless_protocol async_eof_ | exit0；2 passed |
| headless_event_budget（紧凑 target 无 filter） | exit0；1 passed |
| headless_input_shutdown（紧凑 target 无 filter） | exit0；3 passed |
| resume_compact_owner compact_checkpoint_is_durable_and_reconstructs_after_restart | exit0；1 passed |
| stage1_skill_md duplicate_tool_registration_preserves_registry_and_existing_agent_runtime | exit0；1 passed |
| stage1_input_host cancelling_a_ticket_does_not_cancel_provider_or_another_ticket | exit0；1 passed |
| approval_owner remembered_allow_is_durable_and_executes_once | exit0；1 passed |
| session_new old_port_cannot_acquire_the_new_sessions_running_turn | exit0；1 passed |

中间失败已保留：parent provider_admission 曾11 passed/1 failed，工具续接触发“provider worker admission is unavailable”；修复过早结算后最终12/12。修正计数时一次 filter/try_fold 顺序笔误、测试 helper 的一次模块位置错误均造成编译失败，定位修正后才取得最终证据，不把失败构建算通过。

所有测试仍仅本地合成数据，不读真实凭据，不使用全量/smoke/dev-fixtures 产品模式。真实登录资格、Linux/Windows、生产刷新和用户 CLI/TUI/headless 链路仍未验证。下一依赖是 PA06 的已提交凭据 resolver 与固定路由消费，不能在接线前删除 Core 的显式认证 fail-closed 守门；PA07-PA13 保持 pending。

parent 与另一 worker 对最终 Core/账本/worker 结算调用链只读复核，未发现新的重大问题。两套既有账本不是联合事务：后一笔持久化失败可能留下保守计费，但不放行 HTTP；不能把该性质表述为跨账本原子提交。本批未提交或发布实现 PR，未覆盖已安装 binary，原 docs-only PR #11 未改变范围。

### 6.6 PA06/PA08 显式请求接线

本批边界：auth worker 独占 auth/mod/store/codex/resolve；协议 worker 独占 protocols 与 deepseek 定义；route worker 修复 connection 后转为独占 backend/explicit_tests 与合成 TLS fixture；parent 集成 backend/core、Cargo、生产 binary 配置链路测试与本文。共享文件不并行覆写。

- Core 已用 EffectiveConfig::provider_connection 构造显式 backend；固定非秘密初始身份，后续刷新不跟随同 ID 的新登录代次。legacy 构造入口和环境优先级保留。
- 每个请求先纯 route/body 校验，再 resolve；Inference 准入后再 final_preflight，可阻止准入回调中发生的撤销。头部按已验证 header policy 注入，不以 wire 猜测 Bearer/x-api-key；SecretHandle 只在局部认证上下文借用。
- Codex 401 只在没有输出事件、非摘要且还有推理重试额度时恢复一次；恢复重发消耗原 max_retries 计数，不加第二个循环。普通重试重新 resolve；token HTTP 同样调用原 owner 的 AuthRefresh 准入。
- 本批新增 dev-only rustls 是已有锁定依赖，用于本地 HTTPS/证书校验与固定 Codex URL 的 CONNECT 捕获；不添加生产 URL override 或跳过 TLS 校验。
- DeepSeek Chat/Responses JSON mode 不冒充 JSON Schema；明确 provider 的 reasoning 扩展不会被旧 Chat 通用能力表错误清空。新显式连接尚未有 PA12 的 opaque identity scope，签名/加密历史续接拒绝；不影响旧 legacy 路径，不能宣称新路径已支持完整有签名思考的多轮对话。
- 独立审查发现初始 store.identity 将 revoked/uncertain/in-flight 全部降为 login_required，已改 AuthResolver::initial_identity：保留终态错误；允许冻结 InFlight 的非秘密身份，首次 resolve 等待 peer commit，不重新登录或重复交换。额外两条 backend HTTPS 用例覆盖构造与真实发送链路。

| 命令 | 当前证据 |
|---|---|
| rtk cargo check --locked --offline --lib --bin zenpi | exit0；76 warnings，主要为尚未接 CLI/TUI 的登录入口，不用 allow(dead_code) 掩盖 |
| rtk cargo test --locked --offline --test explicit_runtime | 最终 exit0；2 passed；真实生产 binary 读取显式匿名配置、忽略 ambient key/URL 和坏 legacy auth、经 headless 发本地 SSE 并写 journal；缺 credential 在创建 session 前失败 |
| rtk cargo test --locked --offline --lib auth::resolve | 最终 exit0；27 passed；scope/expiry/锁/CAS/不确定结果/初始状态与一次刷新 |
| rtk cargo test --locked --offline --lib auth::store | exit0；24 passed，含一个子进程 helper；受控借用/持久事务/跨进程锁 |
| rtk cargo test --locked --offline --lib auth::codex | exit0；15 passed；登录状态机与复用的受控刷新 HTTP loopback |
| rtk cargo test --locked --offline --lib providers::connection | exit0；32 passed；同身份 revision 上升、撤回目的地、代次变化及 JSON mode/schema 区分 |
| rtk cargo test --locked --offline --lib protocols:: | 最终 exit0；15 passed；Codex/DeepSeek 编解码、严格终态/工具一致性/有界 SSE、各显式 wire 的未绑定 opaque 历史拒绝 |
| rtk cargo test --locked --offline --lib backend::explicit_tests | 最终 exit0；14 passed；真实 HTTPS/SSE、三个 key header、Google 最终路径、Codex 固定服务/账号/UA、401 同额度、无信任根拒绝、撤销零 socket、peer 初始刷新 |
| rtk cargo test --locked --offline --test backend | exit0；26 passed；在与 provider_admission 同次命令中独立成功 |
| rtk cargo test --locked --offline --test provider_admission | 最终 exit0；12 passed；原 owner/worker 预算、工具续接/摘要、取消及截止时间 |
| stage1_anthropic 的 native_headers_system_tool_schema_effort_and_usage_are_exact / streamed_text_tool_arguments_and_opaque_thinking_are_folded_separately | 两次精确 filter 各1 passed/18 filtered，exit0；legacy native 行为未被新 route 模式替换 |

上述 binary 测试首次1 passed/1 failed：fixture 对 stream=true 返回普通 JSON，被严格 route codec 拒绝；改为合法 SSE 后重跑通过。强化 auth_not_configured 断言后又定位 fixture 的目录0700与 macOS临时路径canonicalize要求，修 fixture 而非放宽存储校验；最后2/2通过。
provider_admission 原100ms总预算在并行 fsync 压力下会在 socket 前过期，造成11/12；改为1s预算/2.5s服务端延迟，仍严格断言2.2s内中断、仅1次物理发送，最终12/12。HTTPS fixture 的 Proxy 借用编译错误、临时权限与 SSE字段问题也在各自模块修复重跑，不算通过证据。
测试结果按命令列出，不把 helper、重复回归累计为独立用户场景。本批当时 PA07 与 PA09-PA13 尚未实现；PA07 已在 §6.8 完成，PA09-PA13 仍未实现：TUI 登录管理、原子连接选择、headless connection_v1、有序多模态恢复及真实调试文档仍待后续。未读用户真实凭据、未联系真实 OAuth/模型服务、未手动运行全量或 smoke 测试。

PA07 的额外合并门（**已在 §6.8 闭合**）：config::save_auth/write_auth_if_changed/revoke 当时仍是 legacy 整文件写回，未与 CredentialStore 的事务锁协调。闭合方式是把 legacy 根字段更新纳入同一稳定锁并保留最新 namespace，并以 import/revoke 与 refresh 并发的用例固定；仍禁止通过旧 binary 或手工 token 导入绕过该门。引入管理命令后仍不能把库验收当用户 OAuth 部署就绪：真实登录/服务调用属 A20 live gate。本轮已本地运行 clippy（104 warnings → 42），但未运行 CI 的全量 -D warnings 门。

远端核查：git fetch origin main 成功，main 没有本地尚未取得的新提交。当前实现分支基于文档提交1b8d7c6；原文档 PR #11 仍 OPEN，因此实现 PR 对 main 的 diff 暂包含其两个文档提交，需说明依赖，不能宣称已有文档合并。

### 6.7 Draft PR 交付收据

2026-09-22：实现提交 `e51452f9e3f6391a8de9512bd5e01589a360a071` 已推送至 `JerryLookupU/zenpi:feat/provider-auth-rust`。
已创建 [实现 PR #12](https://github.com/weiyangzen/zenpi/pull/12)，并以 `gh pr view` 核实 `OPEN`、`isDraft=true`、base=`main`、head=`feat/provider-auth-rust`、head owner=`JerryLookupU`。
原 [文档 PR #11](https://github.com/weiyangzen/zenpi/pull/11) 未修改。PR 描述已明确文档依赖、增量验证、现有警告和上述合并门；未标 ready、未合并、未宣称完整蓝图完成或正式部署就绪。
本节及任务表的状态同步仅记录交付，不改变 Rust 行为，也不将历史阶段收据改写为当前功能承诺。

### 6.8 PA07 收据：管理命令与合并门

本批只做 PA07，不推进 PA09-PA13。工作树起点是 11:02 中断时留下的两个零调用函数
（`auth::store::update_legacy`、`providers::connection::api_key_destination`），两者都由本批接线。

交付的命令面（`zenpi --help` 可见）：

```text
zenpi config auth list [--json]
zenpi config add auth apikey BASE_URL PROVIDER --stdin [--wire W] [--header H] [--alias NAME] [--model NAME]
zenpi config add auth codex [EMAIL] [--alias NAME] [--model NAME] [--device | --no-browser]
zenpi config doctor --profile NAME [--json]
zenpi pair revoke --profile NAME --yes [--json]
```

行为边界（均以代码与测试固定，不是承诺）：

- key 只从 stdin 读，没有接受 key 的 argv 形式；授权 URL/device code 只写 stderr，不进 journal、日志或 status JSON。
- `config add` **不**改 `default_profile`；切换后续启动默认仍只由 `config use` 完成。
- 别名冲突不覆盖：别名已指向另一个 credential 时拒绝，要求先 `pair revoke`。
- credential 与 profile 分两个文件提交，回执分别携带 `credential_committed`/`profile_bound`；
  credential 成功而绑定失败时返回该 credential ID 与 `binding_error`，并明确不是认证失败。
- `pair revoke --profile` 解析该 profile 的整个 credential，先列出全部受影响 profile，
  回执写 `local_revoked=true` / `remote_revoked=false`；**解绑不是该命令的同义词**，
  绑定保留，操作者据此看到连接为何失效。
- `list`/`doctor` 本地只读：不 refresh、不 probe、不执行 key command。
  显式绑定的状态取自存储的非秘密快照（ready/expired/revoking/uncertain/login_required/revoked/unconfigured），
  `is_ready()` 不再凭字段齐全宣称 ready。
- 内置多协议 provider（DeepSeek 三路由）在添加一个 key 时**按路由分别授权**并逐条显示，
  不合并成一个更宽的路径前缀；内置名不得指向其它服务。
- 未知 provider 名按显式 custom 处理：必须给出协议、header 与 base URL，缺一拒绝。

本批发现并修复的真实缺陷（由功能验收而非单测发现）：

| 现象 | 原因 | 处理 |
|---|---|---|
| `config auth list` 在尚无状态目录时报 `UnsafePath` | store 逐段 `O_NOFOLLOW` 打开，`/var`、`/tmp` 是符号链接；只对已存在目录 canonicalize 不够 | 新增 `resolve_existing_prefix`：解析最深的已存在祖先再拼回其余段 |
| `config add auth apikey` 不认 `--model` / `--json` / `--profile` | 三级子命令未进入组标志白名单 | 补齐白名单，并禁止 auth 命令接受 `--profile`（它会覆盖位置参数） |
| `config add auth codex --json` **静默启动真实登录并尝试打开浏览器** | 未知标志被位置参数臂吞掉，成了可选 EMAIL | 位置参数臂拒绝以 `-` 开头的值；`--json` 现已明确报错 |
| 内置多路由 provider 无法绑定 profile | 绑定未记录 `base_url`，且三路由的 canonical 前缀不同 | 按路由各自取定义端点；仅非内置 provider 记录 base URL |

合并门（实施记录第 355 行）已闭合：`save_auth`、`import_codex_profile`、`pair_from_codex`、`revoke`
不再整文件重写，全部经 `CredentialStore::update_legacy` 在同一把稳定锁内改 legacy 根字段；
namespace 恒取自磁盘实时值，调用方快照里的同名键会被剥离并被 store 拒绝写入。
新旧行为的差异由 5 条 `config::legacy_lock_tests` 固定——它们在修复前的实现上**全部失败**
（其中 `legacy_write_preserves_a_credential_committed_after_its_snapshot` 直接复现凭据被回退到旧 token）。

| 命令 | 结果 |
|---|---|
| rtk cargo test --locked --offline --lib config::legacy_lock_tests | exit0；5 passed / 341 filtered |
| rtk cargo test --locked --offline --test config | exit0；31 passed（原25 + 新6），1 suite |
| rtk cargo test --locked --offline --lib auth::store | exit0；30 passed / 315 filtered |
| rtk cargo test --locked --offline --lib auth::（mod/codex/callback/resolve/store） | exit0；83 passed / 263 filtered |
| rtk cargo test --locked --offline --lib providers::connection | exit0；36 passed / 310 filtered；新增登录 grant 与请求路由同源断言 |
| rtk cargo test --locked --offline --lib auth::tests | exit0；3 passed；api-key 凭据形状与多路由授权组 |
| rtk cargo test --locked --offline --test config pairing_imports_codex_and_is_idempotent_without_leaking_key -- --exact | exit0；1 passed / 30 filtered |
| rtk cargo test --locked --offline --test config cli_import_honors_codex_home_as_the_profile_root -- --exact | exit0；1 passed / 30 filtered |
| rtk cargo test --locked --offline --test config codex_import_ignores_unrelated_nested_credentials -- --exact | exit0；1 passed / 30 filtered |
| rtk cargo test --locked --offline --test stage1_project_config | exit0；7 passed |
| rtk cargo test --locked --offline --test security | exit0；10 passed |
| rtk cargo test --locked --offline --test backend | exit0；26 passed |
| rtk cargo test --locked --offline --test provider_admission | exit0；12 passed |
| rtk cargo test --locked --offline --test explicit_runtime | exit0；2 passed |
| rtk cargo test --locked --offline --test stage1_model_registry | exit0；16 passed |
| rtk cargo test --locked --offline --test stage1_gemini | exit0；19 passed |
| rtk proxy rustfmt --edition 2024 --check（本批 6 个改动文件） | exit0；不全仓格式化 |
| rtk proxy git diff --check | exit0 |
| cargo clippy --all-targets --all-features | 104 warnings → **42 warnings**；登录流程的整簇 dead code 由本批接线消除，剩余为 PA10/PA11 宿主待消费的接口，不用 `allow(dead_code)` 掩盖 |

功能验收（生产 binary + 临时 HOME + 合成 key，零真实凭据、零真实服务调用）：

| 步骤 | 观察 |
|---|---|
| `config auth list --json`（无状态目录） | 输出 `[]`，exit0，且**不创建** `~/.zenpi` |
| `config add auth apikey https://api.deepseek.com deepseek --stdin --model deepseek-flash` | exit0；credential 落盘 0600；三条路由分别授权（`/chat/completions`、`/responses`、`/anthropic/v1/messages`）；stdout/stderr 均无 key |
| `config list` / `config auth list` | profile 指向 credential，`auth list` 报 `state=ready profiles=deepseek` |
| `config doctor --profile deepseek --json` | `auth_binding_state=ready`，exit0；撤销后为 `revoked`，exit1 |
| `pair revoke --profile deepseek --yes` | `local_revoked=true remote_revoked=false`，列出受影响 profile；auth.json 保留 tombstone，state 转 `revoked` |
| 错误路径 | 非 HTTPS、未知 header、未知 provider 缺协议、空 key、codex 走 apikey、未知子命令、`--device --no-browser` 同用：全部 exit1 且不回显 key |

**未验证项**（不得据此宣称可用）：真实 OAuth 登录（browser/device 两条流程的代码路径已接，但从未对真实账号运行，
属蓝图 A20 / P6 live gate）；真实 provider 请求与刷新；TUI/headless 入口；Windows 的凭据持久化
（按蓝图 §7.2 在未验收前 fail closed）。`config add auth codex` 的浏览器打开动作在功能验收中**未执行**
（唯一一次误触发生在参数吞噬缺陷修复前，见上表第 3 行；未完成授权，未产生凭据）。

### 6.9 PA09/PA11 收据：原子选择与 headless 连接控制

本批只做 PA09 与 PA11，并在结束后跑了一轮受影响 target 的回归。
PA10/PA12/PA13 仍未实现，任务表按实际状态标注。

PA09 的契约顺序（`Agent::select_connection`）：owner 标签 → phase → 队列/附件/审批/worker/未决 operation 栅栏 →
`expected_selection_revision` 复核 → 候选 backend 的 model 与 history 校验 → **一条** durable `model_selected` →
不可失败的 swap。任何一步拒绝都不改变运行中的 backend、model、journal 与 UI。
现有 `model_selected` 被扩展而非新建第二条事件：`model`/`descriptor`/`digest`/`reasoning_effort` 语义不变，
新增字段全部 `serde(default)`，旧 writer 的事件仍按旧行为恢复。
选择 revision 取该 session 最后一条选择事件的序号，它不是配置 revision、也不是 credential revision。

PA11 的契约：capability 声明在既有 status 响应内，不新增启动握手；
`connection` 命令与 prompt/approval/tree/attachment 严格互斥；拒绝带 typed code 且**不带 data**；
request ID 幂等复用既有 replay cache（重复 ID 返回原提交快照，同 ID 不同 body 为 `request_id_conflict`）；
session 写入失败在 I/O 类错误下报 `connection_commit_uncertain`，不冒充 applied 或 rejected。

本批发现并修复的真实缺陷：

| 现象 | 原因 | 处理 |
|---|---|---|
| 每次 select 都会被 `connection_owner_mismatch` 拒绝 | Agent 只有内部 `request_owner_id`（不透明、每进程生成），协议侧 owner 是 `discussion`/`arch`，两者从不相等 | Agent 新增 `owner_label`，arch owner 在 `project_workspace` 里标注；选择按标签核对 |
| PA11 提交后 `session_recovery` 目标失败 | PA09 的测试仍传内部 owner id | 改传 `owner_label()`；由提交后的回归扫描发现并单独修复提交 |

| 命令 | 结果 |
|---|---|
| rtk cargo test --locked --offline --test session_recovery | exit0；45 passed（PA09 新增 6 条） |
| rtk cargo test --locked --offline --test headless_protocol | exit0；45 passed（PA11 新增 2 条） |
| rtk cargo test --locked --offline --test stage1_reasoning_owner | exit0；5 passed；旧选择/effort 行为未变 |
| rtk cargo test --locked --offline --test stage1_model_registry | exit0；16 passed |
| rtk cargo test --locked --offline --test stage1_semantic_compaction | exit0；16 passed、2 ignored（既有 helper） |
| rtk cargo test --locked --offline --test stage1_host | exit0；7 passed |
| rtk cargo test --locked --offline --test tui_interaction | exit0；51 passed |
| rtk cargo test --locked --offline --test stage1_input_queue | exit0；17 passed、1 ignored |
| rtk cargo test --locked --offline --test backend / provider_admission / explicit_runtime / security | exit0；26 / 12 / 2 / 10 passed |
| rtk cargo test --locked --offline --test headless_event_budget / headless_input_shutdown / headless_domain_owner / headless_project_workspace | exit0；1 / 3 / 7 / 13 passed |

**未验证项**：TUI 侧的选择与登录引导（PA10 未实现）；ordered_content_v1 的实际内容通路（PA12 未实现，
本轮只声明了 capability）；真实服务调用与真实 OAuth 登录（A20 live gate）。

### 6.10 与 pi-rs 的对照结论

按用户要求对照了 `https://github.com/jshachm/pi-rs`（一个 Pi 的 Rust 移植）。结论是**它比 zenpi 小得多**
（9.6k 行 / 66 文件 vs 108k 行 / 100 文件），在蓝图覆盖的 7 个领域里有 5 个几乎为空：
无 OAuth PKCE/device/refresh/锁/事务（`auth/storage.rs` 仅 168 行的内存 HashMap）、无 Responses/Codex/DeepSeek wire、
8 个 provider 的流式全部返回 "Streaming not implemented"、无 headless/JSONL、无重试（`tokio-retry` 声明未用）、
内容模型只有扁平 `Text|Image`。因此它**不能当作 zenpi 的遗漏基准**。

它确有而 zenpi 没有的，集中在蓝图之外的旁路产品能力，按价值排序：
主题/配色系统（zenpi `grep -i theme src/` 零命中）、sandbox + `epkg` 工具、
Moonshot/Groq/Mistral 的内置默认端点（zenpi 用 `custom` profile 已可打通）、会话 label/书签事件、
`-c/--continue` 这类便利旗标。这些都不在 PA00-PA13 范围内，未纳入本轮。

对照中发现的**真实蓝图内缺口**：蓝图 §8.3 要求错误至少区分
`provider_unsupported_capability`/`provider_permission_denied`/`provider_usage_limit`。
本批只补齐了后两者的来源（403/429 的 `BackendError::code()` 映射，401 映射为 `auth_login_required`）；
`provider_unsupported_capability` 需要在 `protocols/mod.rs` 的十余处能力拒绝点逐一区分，属 PA08 剩余工作，未做。

### 6.11 PA13 收据：调试文档

`Docs/Zenpi_Provider_Auth_Debugging.md` 已交付。文中每条命令、每个输出字段都在本分支的
release binary 上实跑确认过；未实现的项在文档开头单独列表，不靠"未标注"蒙混。

写文档的过程本身发现并修复了两个真实缺陷，这是它没白写的原因：

| 缺陷 | 触发方式 | 处理 |
|---|---|---|
| `connection_snapshot` 写在固有 impl 里 | 通过 `&dyn Backend` 调用时解析到 trait 默认实现，显式连接一律被报成 `legacy`：选择快照为空、恢复的冲突检查形同虚设、有序内容的 media scope 退化到 legacy | 移进 `impl Backend for …`；单独的提交与回归 |
| `backend_from_effective` 用 `std::path::absolute` | store 逐段 `O_NOFOLLOW`，macOS 上 `/var` 是符号链接，导致**任何临时 HOME 下显式连接都启动不了**（`auth_storage_failed`） | 改用 `config::credential_store`，与各 config 命令同一路径解析 |

文档中另外记录了三个只有实跑才会遇到的坑：未设置 `CODEX_HOME` 时会回退到真实 `~/.codex`；
状态目录必须 0700 / `auth.json` 0600（否则 `unsafe` fail closed）；
DeepSeek 的 Messages 路由要用它自己的规范前缀 `https://api.deepseek.com/anthropic/v1`，
用根地址会被 `noncanonical builtin API prefix` 拒绝（**不要**改用 `custom` 绕过）。

**剩余**：PA10（TUI bootstrap 与 `/auth`/`/login`/`/profile`）仍未开工。

### 6.12 PA10 收据：TUI 首启引导（部分）

本批只做了 PA10 的**启动前**那一半：未认证时不再先建 Agent 再失败，而是进入
`tui/bootstrap.rs` 的 `run_auth_bootstrap`——开放登录（browser/device）、选连接、退出，别的什么都不做。
`auth_gate` 的分级是这块的关键合同：只有 `unconfigured` / `login_required` / `revoked`
以及"完全没配置过"才进引导；**坏 TOML、权限不安全、凭据存储不可读一律向上抛**，
不会被伪装成"你没登录"。`expired`/`refreshing`/`uncertain`/`anonymous_pending_route` 也不进引导——
它们各自有可用的既有路径（请求时刷新，或本就不需要凭据）。

接入点是 TUI 分支里原本就会失败的位置，因此**任何已有可用连接的启动路径完全不受影响**。
headless 保持 fail closed，不做引导。

| 命令 | 结果 |
|---|---|
| rtk cargo test --locked --offline --lib tui::bootstrap | exit0；4 passed / 346 filtered（空装=首启、坏 TOML 向上抛、缺凭据要登录、可用连接不打扰） |
| rtk cargo test --locked --offline --test tui_interaction | exit0；51 passed |
| rtk cargo test --locked --offline --test tui_composer | exit0；54 passed |
| rtk cargo test --locked --offline --test stage1_host | exit0；7 passed |
| rtk cargo test --locked --offline --test config | exit0；31 passed |

release binary 实跑确认：空装 → 引导菜单（`q` 退出码 0）；已绑定连接 → 不出现引导，直接进 TUI；
坏 TOML → 报 TOML 解析错误而非"未登录"；headless 空装 → 仍然 fail closed。

**PA10 未完成的部分**（不声称可用）：运行中会话的 `/auth`、`/login`、`/profile` slash 入口；
由受控后台任务驱动的会话内登录与取消回收 listener；引导界面是终端提示，不是蓝图要求的 TUI 私有界面。
`tui_interaction` 的 51 条既有用例证明本次改动没有破坏既有 TUI 行为，但不证明上述未实现的部分。

### 6.13 首次 live 收据：DeepSeek API key 真实调用

2026-09-22，用户明确授权后，用其 DeepSeek API key 在本分支 release binary 上完成一次真实调用
（此前所有验证都只用合成数据）。隔离在临时 `ZENPI_HOME`/`CODEX_HOME`，key 仅经 stdin 传入。

结果：`config doctor` 报 `ready` 且 exit 0；一条 headless prompt 返回 `success=true code=ok`、
19 条事件、模型文本恰为 `ok`。链路（凭据存储 → 路由解析 → header 注入 → 真实 HTTPS → SSE 解析）因此
从"本地 fixture 通过"升级为"真实服务验证过"。

**仍未验证**：OAuth（Codex）真实登录与刷新、401 恢复、真实工具续接与多模态、Linux/Windows。

同时记录一个与本次实现无关但影响 PR 状态的事实：**`main` 的 CI 在本轮开始前就是红的**。
`a274e5a` 那次运行的 "Format, lint, test, and contract checks" 与
"Stage 1 production host smoke and bounds" 两个 job 均失败，失败原因与本次 PR 相同
（clippy `-D warnings` 撞上未接宿主的 dead code；smoke 的 `shared-projects` 用例失败）。
本 PR 未使该状态变差，也未修复它——修复前者要么补完 PA10/PA12，要么加 `allow(dead_code)`
（实施记录明确禁止用 allow 掩盖）。
