# Provider auth 工作交接（2026-09-22）

接手/复查这份工作的人先读本文。设计合同在 `Zenpi_Provider_Auth_Rust_Blueprint.md`，
实施台账与逐批收据在 `Zenpi_Provider_Auth_Implementation.md`（§6.8–6.13 是本轮的），
真实机器上的排障方法在 `Zenpi_Provider_Auth_Debugging.md`。

---

## 1. 起点与终点

**起点**：Codex 的长任务（goal「执行 pi-ai rust 认证模块」）在 2026-09-22 11:03 因
`usage_limit_exceeded` 全体中断。中断时 PA00–PA13 里 `verified` 的只有 PA00/PA01/PA06，
PA07–PA13 未完成，另有两个**已写完但零调用**的函数留在工作区未提交
（`auth::store::update_legacy`、`providers::connection::api_key_destination`）。

**终点**：这两个在途函数都已接线；PA07/PA09/PA11/PA13 完成，PA10/PA12 部分完成。
分支 `feat/provider-auth-rust`，PR #13（Draft）。工作树干净。

| 子任务 | 状态 | 一句话 |
|---|---|---|
| PA07 auth 管理命令 + 合并门 | 完成 | 7 条命令落地；legacy 写回并入凭据存储同一把锁 |
| PA09 原子连接选择 | 完成 | 六道栅栏 → 单条 durable 事件 → infallible swap |
| PA11 headless connection 控制 | 完成 | capability 声明 + `type:"connection"` 严格协议 + 幂等 |
| PA13 调试文档 | 完成 | 每条命令都在 release binary 上实跑过 |
| PA12 有序内容 | **部分** | DTO/校验/journal/恢复已完成；**编码侧未接线** |
| PA10 TUI 登录引导 | **部分** | 启动前引导已完成；**运行中 slash 入口未做** |

---

## 2. 改了什么

`origin/main..HEAD`：10 笔提交，23 个文件，+5758/−187。

### 2.1 凭据与配置（PA07）

- **`src/auth/store.rs`**：`STORE_KEY` 提升为 `pub(crate)`；新增
  `CredentialStore::new_credential_id()`。原有的 `update_legacy`（Codex 留下的半成品）现在
  **真的被调用了**——legacy 写回全部走它。
- **`src/config.rs`**（本轮改动最大）：
  - `save_auth` / `import_codex_profile` / `pair_from_codex` / `revoke` 不再整文件重写
    `auth.json`，改为经 `CredentialStore::update_legacy` 在同一把稳定锁内改 legacy 根字段；
    namespace 恒取自磁盘实时值，调用方快照里的同名键会被剥离。
  - 新增 `credential_store(paths, create)` 与 `resolve_existing_prefix`：store 逐段
    `O_NOFOLLOW` 打开路径，必须先解析掉符号链接祖先（macOS 的 `/var`、`/tmp`），否则
    临时 HOME 下一切显式连接都起不来。
  - 新增命令面：`auth_list`、`add_auth_apikey`、`bind_credential_profile`、
    `profile_credential`、`revoke_credential`。
  - `status_for_profile` 的 `auth_binding_state` 不再硬编码 `unresolved`，改为投影存储的
    非秘密快照；`is_ready`/`is_authenticated` 因此只在凭据真的可用时为真。
- **`src/providers/connection.rs`**：`api_key_destination`（Codex 留下的半成品）补齐测试并接线；
  新增 `api_key_destinations`——内置多协议 provider（DeepSeek）**按路由分别授权**，
  不是合并成一个更宽的前缀。
- **`src/auth/mod.rs`**：`api_key_credential` 构造 API key 型凭据。
- **`src/core.rs`**：三级子命令解析（`config auth list`、`config add auth …`）、命令分发、
  CLI 登录宿主（线程 + channel，URL/device code 只进 stderr）、`pair revoke` 新语义。

### 2.2 原子连接选择（PA09）

- **`src/core.rs`**：`SelectionSnapshotV1`（版本化选择快照）、`ConnectionSelection`、
  `ConnectionSelectionOutcome`、`ConnectionRejectCode`、`Agent::select_connection`、
  `Agent::selection_revision`、`connection_change_fence`、`require_same_connection`；
  `set_model`/`set_reasoning_effort`/新会话复制 统一走同一个快照构造。
- **`src/backend.rs`**：`ConnectionSnapshot` + `Backend::connection_snapshot` 默认实现，
  `OpenAiCompatibleBackend` 覆盖它。
- **`src/approval.rs`** `has_pending()`、**`src/input_queue.rs`** `InputPort::has_pending()`：
  栅栏需要的无副作用查询（原来只有按 id 的 `is_pending`）。
- **`src/project_workspace.rs`**：arch owner 标注 `owner_label = "arch"`。

### 2.3 headless connection 控制（PA11）

- **`src/protocol.rs`**：`CAPABILITIES`、`ConnectionOwner`、`ConnectionAction`、
  `ConnectionRequest`、`Command::Connection`、`into_command` 的严格互斥校验。
- **`src/headless.rs`**：`connection_response`（applied/status/rejected/commit_uncertain 的
  wire 映射，data 与 error 互斥）；status 响应新增 `capabilities`；`core.rs` 新增
  `connection_candidate` 供 headless 按 profile 构造候选 backend。

### 2.4 有序内容（PA12，部分）

- **`src/protocols/content.rs`**：`StoredContentV1`/`StoredPart`/`MediaRef`/`MediaSource`/
  `MediaScope` 及全部校验；`parse_stored_content`（缺字段 `Ok(None)`、字段非法 `Err`）。
- **`src/backend.rs`**：公共 `InputContentPart`/`InputContentSource`（不接受客户端自带的
  handle/identity/hash）。
- **`src/core.rs`**：用户 turn 的 metadata 写入 `zenpi_content_v1`（文本在前、附件按准入顺序）；
  采纳会话时校验，旧 turn 无该字段则行为完全不变。
- **`src/tools.rs`**：`ToolResult::Success` 新增可选 `content`（默认 None，序列化时省略）。

### 2.5 TUI 登录引导（PA10，部分）

- **`src/tui/bootstrap.rs`**（新文件）：`auth_gate` 分级 + `run_auth_bootstrap`（登录/选连接/退出）。
  接入点在 TUI 分支里**原本就会失败**的位置，已有可用连接的启动路径完全不受影响。
- **`src/tui.rs`**：注册 `pub(crate) mod bootstrap;`。

---

## 3. 过程中发现并修复的两个真 bug

两个都**只有真实端到端运行才暴露**，单测抓不到——这是本轮最值得记住的部分。

### 3.1 `connection_snapshot` 写错了 impl 块

方法写进了 `impl OpenAiCompatibleBackend`（固有 impl），而调用方通过 `&dyn Backend` 访问，
于是解析到 **trait 的默认实现**（永远返回 `None`）。后果是显式连接一律被报成 `legacy`：
选择快照为空、`require_same_connection` 的冲突检查形同虚设、有序内容的 media scope
退化到 legacy 绑定。

单测抓不到的原因：测试用的 backend 是 `new_with_settings` 构造的、本来就不绑定连接，
对它返回 `None` 是**正确**的。是 release binary 的一次 `connection status` 暴露的。

修复：移进 `impl Backend for OpenAiCompatibleBackend`（提交 `a4a1a31`）。

### 3.2 `backend_from_effective` 没解析符号链接祖先

它用 `std::path::absolute(paths.auth)`，而 store 逐段 `O_NOFOLLOW` 打开路径，
macOS 上 `/var` 是指向 `/private/var` 的符号链接 → `UnsafePath` → 表现为
`backend authentication: auth_storage_failed`。后果：**任何临时 HOME 下显式连接都启动不了**。

修复：改用 `config::credential_store`，与各 config 命令同一条路径解析（同一提交）。

### 3.3 一个自己造成的回归

PA11 把 owner 判定从内部 `request_owner_id` 改成 host 标签 `owner_label` 时，
PA09 的测试仍在传内部 id，**整个 `session_recovery` 目标被我弄红了**，而且是提交之后
靠一轮回归扫描才发现，单独补了修复提交（`12c5f9a`）。

---

## 4. 验证

### 4.1 增量测试（按实施记录 §1 的 rtk 规则，未跑全库）

```text
config 31 · security 10 · backend 26 · explicit_runtime 2 · provider_admission 12
session_recovery 45 · headless_protocol 45 · stage1_reasoning_owner 5
stage1_model_registry 16 · provider_multimodal 10 · tools 41 · tui_interaction 51
stage1_host 7 · stage1_semantic_compaction 16(+2 ignored) · stage1_input_queue 17(+1)
```

新增用例：`tests/config.rs` +6、`tests/session_recovery.rs` +6、`tests/headless_protocol.rs` +2、
`tests/provider_multimodal.rs`（新，10 条）、`src/tui/bootstrap.rs` 内嵌 4 条、
`src/config.rs` 内嵌 5 条、`src/auth/mod.rs` 内嵌 3 条、`src/providers/connection.rs` +4。

**合并门的新用例在修复前的实现上全部失败**（其中一条直接复现"legacy 写入把并发提交的
新凭据回退成旧 token"）——这是它们有意义的证据。

### 4.2 真实服务收据（用户授权后）

用户授权使用其 DeepSeek API key，在 release binary 上做了一次真实调用：
`config doctor` 报 `ready`（exit 0），一条 headless prompt 返回 `success=true code=ok`、
19 条事件、模型文本恰为 `ok`。链路（凭据存储 → 路由解析 → header 注入 → 真实 HTTPS → SSE）
由此从"本地 fixture 通过"升级为"真实服务验证过"。

**未覆盖**：OAuth（Codex）真实登录与刷新、401 恢复、真实工具续接与多模态、Linux/Windows。

### 4.3 clippy

104 warnings → 42。剩下的不用 `allow(dead_code)` 掩盖（实施记录明令禁止）。

---

## 5. 未完成的部分（接手时从这里开始）

### 5.1 PA12 的编码侧

**现状**：`ToolResult::Success.content` 可以设置，`zenpi_content_v1` 会写进 journal 并在恢复时校验，
但**编码器不消费它**——`protocols/chat.rs` 等仍只发 `turn.content`（序列化后的 ToolResult JSON）。
所以 typed tool content 目前**送不到模型**，蓝图要求的"content 存在时是权威、`output` 仅兼容记录、
不得重复发送"尚未实现。

**入口**：`src/protocols/{chat,responses,anthropic,google}.rs` 的 tool-turn 编码；
`src/core.rs` 的 `persist_tool_invocation`（工具 turn 的 metadata 构造处）。
另需补 headless `prompt.content` 的 v2 有序输入（`src/protocol.rs` 的 `StdioRequest`）。

**注意**：四个协议的能力不同（chat 无 file、responses 区分 input_image/input_file、
anthropic 保留 tool_result 内容、google 不隐式抓 URL），不能一把抹平。

### 5.2 PA10 的运行中入口

**现状**：启动前引导可用；运行中的会话里没有 `/auth`、`/login`、`/profile`。
蓝图还要求"已有会话登录由受控后台任务驱动同一 auth 函数；取消回收 listener"。

**入口**：`src/slash.rs` 的 `SlashCommand` 表、`src/tui.rs` 的分发、`src/runtime.rs` 的
`BackgroundRunner` + `CancellationToken`。登录函数复用 `crate::core::run_cli_codex_login`
或更下层的 `auth::codex::{begin_browser_login, login_browser, ...}`——**不要再写一套状态机**。

### 5.3 已知的、与本轮无关的失败

- `tests/session_new::worker_gate_binding_prohibitions_revocation_and_action_budget_survive_jsonl_new`
  **失败**。已在 `a274e5a`（本轮起点）上复现同样失败，与本轮改动无关。未修。
- `main` 的 CI 在本轮开始前就是红的，失败的是同样两个 job、同样原因（见下）。

---

## 6. CI 状态（重要，别误判）

`main` 最近的 run **全部失败**，包括本 PR 的基线 `a274e5a`。两个 job：

1. **`Format, lint, test, and contract checks`** —— `cargo clippy --all-targets --all-features -- -D warnings`。
   44 条 warning 中约 23 条是未接宿主的 dead code（PA10 的 `/auth`/`/login`、`cancel_login`、
   `CancelDisposition` 等），约 21 条是既有代码的机械 lint。**修前者要么补完 PA10/PA12，
   要么加 `allow(dead_code)`——实施记录明确禁止后者。**
2. **`Stage 1 production host smoke and bounds`** —— `shared-projects` 用例失败，在 `a274e5a` 上同样失败。

本 PR 既没有弄红它，也没有修好它。PR #13 的评论里有同样的说明。

---

## 7. 仓库现状

```text
分支          feat/provider-auth-rust（工作树干净）
本地 HEAD     fe59131
远端          fork = JerryLookupU/zenpi（已推送），origin = weiyangzen/zenpi
PR            #13（Draft，base main ← head JerryLookupU:feat/provider-auth-rust）
PR #11 / #12  均已合并进 main；main 当前 = a274e5a
```

规则提醒（来自实施记录 §1，接手时继续遵守）：

- 用 `rtk cargo test --locked --offline <target>`；**只跑修改模块的增量测试**，不跑全库/smoke。
- 保留退出码，**禁止用 `head`/`tail`/`|| true` 掩盖失败**；零匹配不算通过。
- 只格式化自己改的文件；fmt 用 `rtk proxy rustfmt --edition 2024 <files>`。
- 真实登录 / API key stdin / token 刷新调试走原生命令，并前置
  `RTK_DISABLED=1 RTK_TELEMETRY_DISABLED=1 RTK_RECALL=0`；key 绝不进 argv、URL 或日志。
- 状态只允许 pending / implementing / verified，`verified` 必须同时具备代码接线与增量测试证据。

---

## 8. 与 pi-rs 的对照结论（用户要求做的）

对照了 `https://github.com/jshachm/pi-rs`。**它不能当作遗漏基准**：9.6k 行 / 66 文件，
在蓝图覆盖的 7 个领域里有 5 个几乎为空（无 OAuth PKCE/device/refresh/锁、无 Responses/Codex/DeepSeek wire、
8 个 provider 的流式全部 `Err("Streaming not implemented")`、无 headless/JSONL、无重试、
内容模型只有扁平 `Text|Image`）。

它确有而 zenpi 没有的，集中在蓝图之外：主题/配色系统、sandbox + `epkg`、Moonshot/Groq/Mistral
的内置默认端点（用 `custom` profile 已可打通）、session label、`-c/--continue`。**均未纳入本轮**。

对照发现的唯一蓝图内缺口：§8.3 要求的 `provider_unsupported_capability` 尚未区分
（需要在 `protocols/mod.rs` 的十余处能力拒绝点逐一改）。403/429 已映射为
`provider_permission_denied`/`provider_usage_limit`，401 映射为 `auth_login_required`。
