# zenpi provider/auth 路径链路调试

本文档配合 `Zenpi_Provider_Auth_Rust_Blueprint.md`（设计合同）与
`Zenpi_Provider_Auth_Implementation.md`（实施台账与收据）使用。
蓝图给的是**要求**，台账给的是**哪些已经做到**；本文只讲**怎么在真实机器上定位问题**。

> 原则：本文列出的命令都是本分支实际存在的。文档不拿旧安装的帮助文本证明新命令存在，
> 也不把本地合成数据的通过当作真实账号已验证。**未实现或未验证的项在文中显式标注。**

---

## 0. 本分支已实现 / 未实现

先看这张表，避免调试一个不存在的命令。

| 能力 | 状态 |
|---|---|
| `config import-codex` / `pair import-codex`（legacy API key 导入） | 已实现 |
| `config auth list [--json]` | 已实现 |
| `config add auth apikey … --stdin` | 已实现 |
| `config add auth codex …`（browser/device 登录） | 已接线，**未经真实账号验证** |
| `config doctor [--profile NAME] [--json]` | 已实现，状态取自存储的凭据 |
| `pair revoke --profile NAME --yes` | 已实现（撤销整个 credential） |
| `config use NAME` | 已实现 |
| headless `type:"connection"` 控制 + capability 声明 | 已实现 |
| TUI `/auth` `/login` `/profile` 与未认证引导 | **未实现**（PA10） |
| headless 有序内容输入（`prompt.content` v2） | **未实现**（PA12 剩余） |
| 工具 typed content 送达模型 | **未实现**（PA12 编码侧剩余） |

---

## 1. 版本与构建

```sh
cd <本分支工作树>
cargo build --release            # 产物：target/release/zenpi
target/release/zenpi --help
```

- crate 版本固定为 `0.1.0`（`Cargo.toml`）；分支 `feat/provider-auth-rust`。
- **不要用已安装的 `zenpi` 验证新命令**：安装版本来自其他分支，它的 `--help` 里没有
  `config auth list`、`config add auth`。始终用本次构建的绝对路径调用。
- 本文所有示例都用隔离的临时 HOME，不触碰你的真实 `~/.zenpi`：

```sh
export ZENPI_HOME="$(mktemp -d)/.zenpi"     # 状态目录
export CODEX_HOME="$(mktemp -d)/.codex"     # 导入时读它；不设置会回退到真实 ~/.codex
Z=./target/release/zenpi
```

**两个容易踩的坑**：

1. `ZENPI_HOME` 只重定向 zenpi 自己的状态目录。**未设置 `CODEX_HOME` 时，读取会回退到真实的 `~/.codex`**——
   做隔离复现时一定要把它一起指向临时目录，否则你以为在用测试配置，实际用的是本机 Codex 配置。
2. zenpi 的状态目录必须是**属主私有**的：目录 `0700`、`auth.json` `0600`。权限不对时的报错是
   `credential store: credential store path or permissions are unsafe`——这是 fail closed，不是 bug。
   `config` 的各条命令会自动把目录收紧到 0700；手工造 fixture 时要自己 `chmod`。

---

## 2. 两种导入的明确结果

### 2.1 API key 型导入

```sh
printf '{"OPENAI_API_KEY":"<key>"}\n' > "$CODEX_HOME/auth.json"
echo 'model = "gpt-5"' > "$CODEX_HOME/config.toml"
$Z config import-codex --profile codex
```

`find_api_key` 只接受**精确键名** `OPENAI_API_KEY`（或 `profiles.<name>` / `providers.<name>` 下的同名键）。
它**不会**递归扫描，也**不会**复制 `tokens.access_token` / `refresh_token`：Codex 的 ChatGPT 订阅
OAuth token 不被导入，导入 OAuth-only 的 Codex 目录只会得到配置、没有凭据。

### 2.2 OAuth-only 导入

`config import-codex` 对 OAuth-only 目录**可以 exit 0**，因为它导入了 provider/model/endpoint。
这**不等于已连接**：随后启动会因为没有凭据而失败。判断依据不是退出码，而是：

```sh
$Z config doctor --profile codex --json | python3 -m json.tool
```

`auth_binding_state` 才是权威：

| 值 | 含义 |
|---|---|
| `ready` | 绑定的凭据处于 active 且未过期 |
| `expired` | active 但 `expires_at_ms` 已过（只看时钟，**不刷新**） |
| `refreshing` / `uncertain` | 刷新在途 / 刷新结果不明（后者需要重新登录） |
| `login_required` | 凭据需要重新登录 |
| `revoked` | 已被本地撤销 |
| `unconfigured` | profile 指向的 credential 在存储里不存在 |
| `anonymous_pending_route` | `auth_method='none'`，不需要凭据 |

`is_ready` 只在 `auth_binding_state == "ready"` 时才为真——**字段齐全不再是 ready 的理由**。
`doctor` 的退出码据此给出：ready → 0，否则 1。

---

## 3. 定位一层路径的固定方法

要回答"这次请求到底发给谁、用哪个凭据"，按下面五层逐层看，**不要从错误文本猜账号**。

### 第 1 层：profile 选择

```sh
$Z config list                 # 列出 profile 及 active 标记
$Z config use deepseek         # 只改"后续启动的默认"，不切换正在运行的会话
```

### 第 2 层：provider / protocol / base URL 解析结果

```sh
$Z config doctor --profile deepseek --json
```

关注 `provider`、`wire_api`、`base_url` 三个字段。它们由配置 + provider 定义共同决定：

- `wire_api` 省略时，只有**单路由** provider 能推断出来；`deepseek`/`openai` 这类多路由 provider
  必须显式写 `wire_api`，否则报 `explicit connection requires wire_api when provider has no unique route`。
- `base_url` 对内置 provider 来自定义文件，**不是**你在命令行里随便填的值；把内置 provider 名
  指向别人的服务会被拒：`a builtin provider base URL must address that provider's own service`。

### 第 3 层：凭据与它被授权去的目的地

```sh
$Z config auth list            # 人类可读
$Z config auth list --json     # 字段：credential_id/provider/kind/state/revision/expires_at_ms/profiles
```

`state` 的取值与 §2.2 的表一致。`profiles` 列出**所有**指向该 credential 的 profile——
这是判断"撤销会影响谁"的唯一正确依据。

### 第 4 层：发起方（一次性看出授权范围）

```sh
printf '<key>\n' | $Z config add auth apikey https://api.deepseek.com deepseek \
    --stdin --model deepseek-flash --json
```

回报字段：

| 字段 | 说明 |
|---|---|
| `credential_id` | 新凭据 ID，`cred_` 前缀 |
| `destinations` | **该凭据被授权访问的每一个目的地**，逐条显示 |
| `credential_committed` | 凭据是否已落盘 |
| `profile_bound` | profile 是否已绑定 |
| `binding_error` | 非 null 表示凭据已存但 profile 没绑上——**这不是认证失败**，按 `credential_id` 修复即可 |

内置多协议 provider（如 DeepSeek）在**一次**添加里按路由分别授权三条：
`/chat/completions`、`/responses`、`/anthropic/v1/messages`。它们各自是一条精确 grant，
不是被合并成一个更宽的路径前缀。

### 第 5 层：请求本身

`--json` 输出永远只到 stdout，人类提示走 stderr，所以 `| python3 -m json.tool` 永远安全。
凭据本身**从不**出现在任何输出里：`config auth list` 只给 ID 和状态，
`config add auth apikey` 只从 stdin 读 key。

---

## 4. 凭据生命周期

### 4.1 添加

```sh
# 内置 provider：base_url 必须是该服务自己的地址
printf '<key>\n' | $Z config add auth apikey https://api.deepseek.com deepseek \
    --stdin --model deepseek-flash

# 自建/网关：必须显式给出协议与 header 策略
printf '<key>\n' | $Z config add auth apikey https://gateway.example/v1 custom \
    --stdin --wire responses --header bearer --alias gateway --model my-model
```

`config add` **不会**改 `default_profile`。要让它成为后续启动的默认，另外执行
`$Z config use <alias>`。

常见拒绝及含义：

| 报错 | 含义 |
|---|---|
| `config add auth apikey requires --stdin; keys are never taken from the argument list` | 没有从 stdin 读 key；argv 不接受 key |
| `credential-bearing destinations require HTTPS` | 目的地不是 HTTPS |
| `custom API key providers require explicit protocol and header policy` | 自建 provider 缺 `--wire`/`--header` |
| `Codex requires its OAuth binding` | 想用 API key 走 Codex；Codex 只能 OAuth |
| `header policy is not allowed by the service route` | 该 provider 的路由不接受这个 header |

### 4.2 撤销

```sh
$Z pair revoke --profile deepseek --yes --json
```

- 解析该 profile 指向的**整个 credential**，回执里的 `profiles` 是**全部**受影响 profile——
  `--yes` 确认的是这个范围，不是只解绑你点名的那个。
- `local_revoked=true` / `remote_revoked=false`：zenpi 首期**不做服务端撤销**，
  不要把 `local_revoked` 读成"远端已吊销"。
- 撤销保留 tombstone（revision 递增），因此同一 ID 不会被后续登录悄悄复活。
- **解绑不是这条命令的同义词**：profile 仍指向该 credential，这正是操作者能看出
  "连接为什么不通"的原因。撤销后再 `config doctor` 会看到 `auth_binding_state=revoked`。

---

## 5. headless 链路

headless 是当前唯一**完整可用**的宿主入口。

headless 在启动时**必须先有一条可用连接**，否则 fail closed：
`no provider URL configured; run zenpi config import-codex ...`。
所以先绑定一个 profile 并把它设为默认：

```sh
printf '<key>\n' | $Z config add auth apikey https://api.deepseek.com deepseek \
    --stdin --model deepseek-flash
$Z config use deepseek
```

### 5.1 先看 capability，再发命令

```sh
printf '{"schema_version":2,"type":"status","id":"s1"}\n{"type":"shutdown","id":"q"}\n' \
  | $Z --mode headless --session "$PWD/s.jsonl"
```

status 响应的 `data.capabilities` 声明本服务支持哪些能力，当前是
`["connection_v1","ordered_content_v1"]`。
**客户端只有看到 capability 才发对应命令**；旧服务端会按既有的 unknown command 拒绝，
而不是猜你是否支持。

### 5.2 换连接 / 查连接

```sh
printf '%s\n' \
 '{"schema_version":2,"type":"connection","id":"c1","session_id":"<session-id>","connection":{"action":"status","owner":"discussion"}}' \
 '{"schema_version":2,"type":"connection","id":"c2","session_id":"<session-id>","connection":{"action":"select","owner":"discussion","profile":"deepseek","model":"deepseek-flash","expected_selection_revision":0}}' \
 '{"type":"shutdown","id":"q"}' \
 | $Z --mode headless --session "$PWD/s.jsonl"
```

| 回执 | 含义 |
|---|---|
| `success:true, data.outcome:"status"` | 当前选择、`selection_revision`、`blocked_reason`（有值说明此刻不能服务新推理） |
| `success:true, data.outcome:"applied"` | 已提交；`data.selection` 是提交的快照 |
| `success:false, code:"connection_busy"` | owner 不空闲，或队列/附件/审批/未决操作还在 |
| `success:false, code:"connection_stale"` | `expected_selection_revision` 已过期（先 `status` 拿最新值再重发） |
| `success:false, code:"connection_owner_mismatch"` | `owner` 不是本会话的属主（只接受 `discussion` / `arch`） |
| `success:false, code:"connection_commit_uncertain"` | 落盘结果不明；**不要重做**，同 ID 重发只会核对 |

拒绝一律**不带 `data`**：失败时不要从错误文字猜账号，用 `status` 取现状。

### 5.3 幂等

同一个 `id` 重发**相同** body 会返回原来的终态（不重新执行）；同一 `id` 换成**不同** body
返回 `request_id_conflict`。这条对 connection 与既有命令一致。

---

## 6. DeepSeek 三协议

DeepSeek 一个 key 授权三条路由，但 profile 一次只选一条 wire：

**每条路由要用它自己的规范前缀**，不能都用服务根地址：

```sh
$Z config add auth apikey https://api.deepseek.com deepseek \
    --stdin --wire responses --alias ds-responses --model deepseek-flash

# Messages 路由挂在 /anthropic/v1 下，用根地址会报
# "noncanonical builtin API prefix; use an explicitly scoped custom provider"
$Z config add auth apikey https://api.deepseek.com/anthropic/v1 deepseek \
    --stdin --wire anthropic_messages --alias ds-anthropic --model deepseek-flash
```

内置 provider 的规范前缀来自它的定义文件（`src/providers/deepseek.rs`）。
报 `noncanonical builtin API prefix` 时，**不要**改用 `custom` 绕过——那会让授权范围跟着你的输入走；
正确做法是照着定义文件填规范前缀。

- **能力按路由不同**：`structured_output` 只在 Responses 路由为真；JSON mode 与 JSON Schema 是两回事，
  Chat 路由不会因为接受 schema 就真的支持它。
- 未绑定 opaque 历史的显式 wire 会**拒绝**续接，不会删签名冒充兼容。

---

## 7. 日志与秘密边界

- API key 只从 stdin；授权 URL 与 device code 只写到发起命令的 **stderr**，
  **不进** journal、通用日志或 status JSON。记录问题时不要把这两样贴进 issue。
- journal 只存**引用**（路径 + size + sha256 / provider file id / 不透明 handle），
  不存 base64 bytes、token，或带签名参数的完整 URL。
- 撤销、刷新、登录的失败分类见 `AuthError::code()`（`auth_refresh_uncertain`、
  `auth_storage_failed`、`auth_lock_timeout` 等）。**不确定就不重发**：
  `auth_refresh_uncertain` 要求重新登录，不是重试。

---

## 8. 需要你侧确认的 live 项（本文档不代替）

以下**从未在本分支运行过**，需要你另行授权真实账号后才谈得上验证：

1. **真实 OAuth 登录**（`config add auth codex` 的 browser 与 `--device` 两条流程）。
   代码路径已接，但没有对真实账号跑过；`--no-browser` 可以避免自动打开浏览器。
2. **真实模型调用**：任何一次向真实 endpoint 发请求。
3. **真实刷新与 401 恢复**：需要真实 token 过期。
4. **真实跨账号/跨 profile 的 file ID 拒绝**：本地测试用手工构造的 scope，不是两套真实凭据。

### 已完成的 live 收据（API key 路径）

2026-09-22，用户授权使用其 DeepSeek API key，在本分支的 release binary 上完成了一次真实调用：

```text
运行日期：2026-09-22
zenpi 版本/commit：0.1.0 @ 06b44b2（本 PR）
构建方式：cargo build --release
provider / 模型：deepseek / deepseek-flash
wire：chat_completions（base_url 由定义文件固定为 https://api.deepseek.com）
账号类型与地域：用户自有 DeepSeek API key
隔离方式：临时 ZENPI_HOME + 临时 CODEX_HOME；key 只经 stdin 传入，未进 argv、未回显
命令（不含 key）：
  config add auth apikey https://api.deepseek.com deepseek --stdin --model deepseek-flash
  config use deepseek
  config doctor --profile deepseek --json      -> auth_binding_state=ready, exit 0
  --mode headless 单条 prompt「Reply with exactly one word: ok」
观察结果：prompt 响应 success=true code=ok；19 条事件；模型文本恰为 "ok"
```

这证明的是**这条链路**：凭据存储 → 路由解析 → Authorization 头注入 → 真实 HTTPS 调用 → SSE 解析。
它**不**证明：OAuth（Codex）登录、刷新/401 恢复、真实工具续接与多模态。

真实调试时的建议（用户侧填写，便于回溯）：

```text
运行日期：
zenpi 版本/commit：
构建方式：cargo build --release（或其它）
provider / 模型：
账号类型与地域：
现象的层级（profile / provider / protocol / base URL / credential / request）：
复现命令（不含 key）：
观察到的 code 与 stderr（不含授权 URL、device code、token）：
```
