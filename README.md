# zenpi

zenpi is a small Rust agent runtime derived from the useful boundaries in the
official `pi-agent` TypeScript project. It has exactly two public modes:

- **TUI** for an interactive terminal conversation.
- **headless** for strict LF-delimited JSONL over stdin/stdout.

Both modes use the same core, session log, backend interface, and compact
b3ehive handoff records. There is no third `print`, `json`, `rpc`, server, or
daemon mode.

## English

### Why zenpi

zenpi keeps the agent useful when it is alone and cheap when it is composed
with other agents. A headless process can be started with one pipe, persist a
recoverable session, and exchange a bounded handoff without a broker. The TUI
uses a coalesced render loop and terminal-buffer diffing so resize and rapid
local updates do not replay the whole transcript unnecessarily. Provider calls
use the configured OpenAI-compatible endpoint.

### Quick start

```bash
# Install the `zenpi` binary (or use `cargo run --` while developing).
cargo install --path . --locked
zenpi --help

# Pair with the provider already configured for Codex.
zenpi config import-codex --profile codex
zenpi config doctor

# Interactive terminal mode, using the imported provider.
zenpi --mode tui --profile codex

# Scriptable JSONL mode.
printf '%s\n' '{"type":"prompt","id":"1","text":"Say hello"}' \
  | zenpi --mode headless --session ./session.jsonl
```

Named profiles can be inspected and switched without exposing credentials:

```bash
zenpi config list
zenpi config use codex
zenpi config doctor --profile codex --json
```

The production default is the configured OpenAI-compatible provider. zenpi
never fabricates an answer when credentials are missing: it exits before
creating a session and tells you to run `zenpi config import-codex` or set
`ZENPI_BASE_URL`, `ZENPI_API_KEY`, and `ZENPI_MODEL`. The profile is stored in
`~/.zenpi/config.toml`; on Unix the imported key is stored in the owner-only
`~/.zenpi/auth.json` (platform-specific ACL enforcement remains separate).
`config doctor` reports the effective endpoint host, API family, model, and
credential presence without printing the key. A read-only
fallback can use `~/.codex` on the first run; the explicit import persists it.
Provider quota/rate/usage-limit failures are returned as errors; zenpi never
turns them into a mock answer.

The Responses API is the primary wire protocol (`/responses` and
`/v1/responses`) and is consumed as SSE, including text deltas, completion
usage, and Codex gateways that insert NUL padding. Chat Completions remains an
explicit compatibility adapter. `--backend echo` exists only in builds made
with `--features dev-fixtures`; normal release binaries cannot use the mock.

This checkout contains the v1 provider/session/runtime baseline and a set of
bounded v2 candidates. A green compile is not a claim that a Claude
Code/Codex-equivalent product is complete. The current baseline evidence is
the release/install/user-smoke flow and the focused tests; those checks cover
the baseline and selected slices, not every v2 interaction or close hook.
multiline editing, Markdown rendering, file diff previews, tool lifecycle
folding, slash-command/domain models, and responsive layout primitives still
need the end-to-end acceptance work described by their v2 rows. BentoBox tabs,
durable first-class domain dispatch, browser/PTY panes, and socket-level
cancellation are explicitly partial, planned, or deferred rather than hidden
behind a completion claim. The frozen v1 receipt remains in
[`Docs/Zenpi_Execution_Blueprint.md`](Docs/Zenpi_Execution_Blueprint.md); the
versioned audit and re-plan is
[`Docs/Zenpi_Execution_Blueprint_v2.md`](Docs/Zenpi_Execution_Blueprint_v2.md).
Installation currently requires Rust 1.88 or newer and a local clone.

Session and extension lifecycle commands are available outside either runtime
mode:

```bash
zenpi session list --json
zenpi session inspect ~/.zenpi/sessions/example.jsonl --json
zenpi session fork SOURCE DESTINATION
zenpi session gc --retain-newest 20 --older-than-seconds 2592000 --yes

zenpi extension install ./my-extension
zenpi extension list --json
zenpi extension disable my-extension
zenpi extension upgrade ./my-extension-v2
```

Headless prompts can attach bounded workspace images/files without putting
binary bytes in the journal:

```json
{"schema_version":2,"type":"prompt","id":"p-2","text":"Review these","attachments":[{"kind":"image","mime_type":"image/png","path":"screenshots/ui.png"},{"kind":"file","mime_type":"text/plain","path":"logs/result.txt"}]}
```

### Headless protocol

Input and output are one JSON object per LF-terminated line. A payload may
contain U+2028 or U+2029; only LF frames a record. Diagnostics go to stderr so
stdout remains machine-readable.

Supported commands are `prompt`, typed slash `command`, `steer`, `cancel`, `approve`, `status`,
`resources`, `handoff`, `resume`, and `shutdown`. The `command` request carries a slash
command such as `{"type":"command","id":"c1","text":"/status"}` and never
enters the model turn. In the production owned/async headless
path, accepted prompts emit typed v2 progress events and one v1-compatible
terminal response. The borrowed synchronous embedding API is intentionally
smaller and does not expose in-flight cancellation. These are the current
transport guarantees, not proof that every v2 UX row is accepted. A v2 `resume` can request
`from_sequence`; duplicate in-process request IDs for ordinary terminal
operations receive the cached terminal result while that bounded cache entry
is retained (evicted or session-reset IDs may be retried), rather than repeating
provider or tool work. A replay request
re-emits its requested event suffix and is not
treated as a response-only cache hit.
Path-bearing JSONL `resume` requests, like `/session open PATH`, only switch to
an existing regular journal; a missing or symbolic-link target is rejected
without creating or modifying a file.
When a `steer` arrives before `shutdown` but is still waiting for turn
admission, the owned host gives that deferred request a bounded promotion
window; if the window expires it returns an explicit `runtime_closed` error
instead of silently dropping the request.

The common local session routes are executable in both hosts: `/session list`
lists configured journals (including the legacy `~/.zenpi/session.jsonl`),
`/session open PATH` switches an idle agent to an existing journal, and
`/session fork SOURCE DESTINATION`, `/session export SOURCE DESTINATION`, and
`/session import SOURCE DESTINATION` use explicit clean-source and
non-overwriting-destination validation. `/session gc` and the in-workspace
session browser remain explicit host errors until their owner adapters are
accepted. Learn evidence/checkpoint commands are likewise local and read-only
with respect to execution: `/learn evidence ID REPOSITORY-RELATIVE-REF`
stores a bounded hash receipt, while `/learn resume ID` reports a validated
checkpoint with `zenpi_started: false` until an external owner is present.

Example:

```json
{"type":"prompt","id":"p-1","text":"Summarize the task"}
{"type":"handoff","id":"h-1","to":"worker-b","summary":"Task is ready","artifacts":["Docs/plan.md"]}
{"type":"shutdown","id":"s-1"}
```

### Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

Each Blueprint checklist item declares an `Estimated LOC` forecast for the
implementation/test code attributable to that item, and every value is
strictly below 5,000. This is a **per-item** forecast: it does not mean 5,000
Blueprint items and it is not a 5,000-line cap on the repository. The
aggregate Rust source inventory is reported for visibility, not used as a
project-wide cap; generated files, vendored dependencies, documentation, and
build output are not item estimates.

The frozen v1 execution receipt is [`Docs/Zenpi_Execution_Blueprint.md`](Docs/Zenpi_Execution_Blueprint.md).
The current product review and next execution contract is the non-authoritative
versioned draft [`Docs/Zenpi_Execution_Blueprint_v2.md`](Docs/Zenpi_Execution_Blueprint_v2.md)
(`2.0.0`; the Cargo crate remains `0.1.0`).
The frozen local policy is [`Docs/Zenpi_Execution_Spec.md`](Docs/Zenpi_Execution_Spec.md),
and its read-only monitoring projection is
[`Docs/Zenpi_Execution_Gantt.md`](Docs/Zenpi_Execution_Gantt.md).

## 中文

zenpi 是一个轻量 Rust agent runtime，借鉴官方 `pi-agent` TypeScript 项目
中清晰的边界，但只保留两种公开模式：**TUI** 交互终端界面，以及通过
stdin/stdout 传输严格 LF-JSONL 的 **headless**。不存在第三种 `print`、
`json`、`rpc`、server 或 daemon 模式。

两种模式共享同一个 core、session 日志、backend 接口和精简的 b3ehive
handoff 记录。headless 可以通过一条管道启动、持久化可恢复会话，并在
agent 之间传递有边界的 handoff；TUI 使用合并渲染和终端缓冲区差分，减少
窗口调整及快速流式更新时的重复绘制。
生产 owned/async 路径把 provider 工作放在后台，TUI/headless 在流式响应期间
可继续处理输入和协作式取消；legacy 同步入口和阻塞 socket 读取仍有限制。

生产默认 backend 是配置的 OpenAI-compatible provider，不再静默使用
`echo`。首次使用先执行 `zenpi config import-codex --profile codex`，它从
`~/.codex/config.toml` 读取 URL、Responses API 和模型，从
`~/.codex/auth.json` 读取 key，并将非 secret 配置写入
`~/.zenpi/config.toml`、key 写入权限为 0600 的 `~/.zenpi/auth.json`。
随后执行 `zenpi config doctor`，再运行 `zenpi --mode tui --profile codex` 或不带
`--backend` 的 headless。也可用 `ZENPI_BASE_URL`、`ZENPI_API_KEY`、
`ZENPI_MODEL` 覆盖配置。`echo` 只存在于启用 `dev-fixtures` feature 的测试
构建；正常 release 无法启用它。缺少 provider 时 zenpi 会在创建 session
前失败，不会伪造回复。
provider 的额度、限流或 usage-limit 错误会原样作为失败返回，不会退回 mock。

当前仓库包含 v1 的 provider/session/runtime 基线，以及正在审核的 v2 候选实现。
“编译通过”不等于已经达到 Claude Code/Codex 级别的完整可用体验。真实
provider、Codex 配对、Responses SSE、附件、工具、审批、session、skills/
extensions 等已有可执行测试；多行编辑、Markdown/diff 渲染、工具状态折叠、
slash/domain 模型和响应式布局也有边界实现，但 BentoBox 多 tab、blueprint/
goal/learn 的持久化调度、浏览器/PTY pane，以及 socket 级取消仍须按 v2
验收矩阵补齐。它们不会因为 v1 的 `CF-*` 勾选而被伪称完成。
v1 冻结收据在 `Docs/Zenpi_Execution_Blueprint.md`，版本化自查和下一轮
契约在 `Docs/Zenpi_Execution_Blueprint_v2.md`。
该草案版本为 `2.0.0`；Cargo crate 的发布版本仍是 `0.1.0`，两者分别表示产品契约与包版本。
会话可用 `zenpi session list|inspect|fork|export|import|gc` 管理；扩展可用
`zenpi extension install|list|disable|enable|upgrade|remove` 管理。headless v2
的 `prompt.attachments` 可引用工作区内的图片或文件，二进制内容不会写入日志。
两种运行模式都可执行 `/session list`、`/session open PATH`，以及带明确源和目标的
`/session fork SOURCE DESTINATION`、`/session export SOURCE DESTINATION`、
`/session import SOURCE DESTINATION`；切换只针对已有会话文件，维护操作拒绝符号链接、
脏 journal 和已存在目标，旧版 `~/.zenpi/session.jsonl` 也会被列出。`/session gc`、
工作区内 session 浏览器以及真正的外部执行仍会在 owner adapter 完成前明确返回错误，
不伪造成功。`/learn evidence ID REPOSITORY-RELATIVE-REF` 只保存有界 hash receipt，
`/learn resume ID` 只检查持久 checkpoint，不启动 worker。
JSONL 的带路径 `resume` 也只允许切换已有的普通 journal；缺失或符号链接目标
会在不创建、不修改文件的情况下返回错误。
如果 `steer` 已在 `shutdown` 前被接收但仍等待 turn admission，owned 路径会在有界窗口内先完成取消/重发；超时则明确返回 `runtime_closed`，不会静默丢弃请求。
每个 Blueprint item 都为其实现/测试代码声明严格小于 5000 的 `Estimated LOC` 预估值；这里是每个 item 的
预估，不是 5000 个 Blueprint item，也不是仓库 Rust 总行数上限。仓库 Rust 总行数只作信息性盘点。
在会话中还可用 `/help`、`/model`、`/models` 和 `/doctor` 查看命令、模型配置
与脱敏诊断；这些命令不会把 secret 写入输出。

## 日本語

zenpi は公式 `pi-agent` TypeScript 実装の境界設計を参考にした軽量な Rust
agent runtime です。公開モードは **TUI**（対話型ターミナル）と
**headless**（stdin/stdout の厳密な LF-JSONL）の二つだけです。`print`、
`json`、`rpc`、server、daemon という第三のモードは作りません。

両モードは同じ core、session ログ、backend インターフェース、軽量な
b3ehive handoff レコードを共有します。headless は一本のパイプで起動でき、
復元可能な session を保存し、他の agent と限定された handoff を交換できます。
TUI はフレームをまとめ、端末バッファ差分を使うため、リサイズや高速な応答
更新でも不要な全画面再描画を避けます。
本番の既定 backend は設定済みの OpenAI-compatible provider です。暗黙の
`echo` mock は使いません。最短手順は `zenpi config import-codex --profile codex`、
`zenpi config doctor`、`zenpi --mode tui --profile codex` です。最初のコマンドは
`~/.codex/config.toml` の URL、Responses API、モデルと
`~/.codex/auth.json` の key を読み、`~/.zenpi/config.toml` と権限 0600 の
`~/.zenpi/auth.json` に安全に保存します。環境変数
`ZENPI_BASE_URL`、`ZENPI_API_KEY`、`ZENPI_MODEL` も使用できます。テスト用の
`echo` は `dev-fixtures` feature のテスト build だけで有効です。通常の release
では使用できず、provider がない場合は session 作成前に失敗します。
provider の quota/rate/usage-limit エラーも mock 応答に置き換えません。

このリポジトリには v1 の provider/session/runtime 基盤と、レビュー中の v2
候補実装があります。ただし、コンパイル成功は Claude Code/Codex 相当の
完全な利用体験を意味しません。実 provider、Codex pairing、Responses SSE、
添付、tool、approval、session、skills/extensions には実行可能なテストがあり、
複数行入力、Markdown/diff 表示、tool 状態の折りたたみ、slash/domain モデル、
responsive layout にも境界実装があります。一方、BentoBox の複数 tab、
blueprint/goal/learn の永続 dispatch、browser/PTY pane、socket 単位の取消は
v2 の受け入れ行として partial/planned/deferred です。v1 の `CF-*` の印だけで
完成とは扱いません。凍結した v1 の記録は `Docs/Zenpi_Execution_Blueprint.md`、
版付き（`2.0.0`）の監査と次の契約は `Docs/Zenpi_Execution_Blueprint_v2.md` にあります。Cargo crate の公開版は `0.1.0` のままです。
Session は `zenpi session list|inspect|fork|export|import|gc`、extension は
`zenpi extension install|list|disable|enable|upgrade|remove` で管理できます。
headless v2 の `prompt.attachments` は workspace 内の画像・ファイルを参照し、
binary data を journal に保存しません。
両ホストで `/session list`、`/session open PATH`、および明示的な source/destination
を取る `/session fork SOURCE DESTINATION`、`/session export SOURCE DESTINATION`、
`/session import SOURCE DESTINATION` を実行できます。保守操作は既存の正常な journal
だけを source に取り、symlink、壊れた journal、既存 destination を拒否します。旧版
`~/.zenpi/session.jsonl` も一覧に含まれます。`/session gc`、workspace session browser、
外部実行は owner adapter が受理されるまで明示的にエラーです。`/learn evidence ID
REPOSITORY-RELATIVE-REF` は有界 hash receipt だけを保存し、`/learn resume ID` は
検証済み checkpoint を表示するだけで worker を起動しません。
パス付き JSONL `resume` も既存の通常 journal だけを開き、欠落またはシンボリック
リンクの対象はファイルを作成・変更せずエラーにします。
`shutdown` 前に受理された `steer` が turn admission 待ちの場合、owned 経路は
限定時間内に cancel/reissue を試み、期限後は `runtime_closed` を明示して破棄を隠しません。
各 Blueprint item には実装・テストコードの `Estimated LOC` 予測（各 item が 5000 未満）を記載します。
これは 5000 個の item という意味でも、リポジトリ全体の Rust 行数上限でもありません。

## License

MIT. See [LICENSE](LICENSE).
