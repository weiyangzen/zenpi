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
zenpi --mode tui

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
`~/.zenpi/config.toml`; the imported key is stored only in the owner-readable
`~/.zenpi/auth.json`. `config doctor` reports the effective endpoint host, API
family, model, and credential presence without printing the key. A read-only
fallback can use `~/.codex` on the first run; the explicit import persists it.

The Responses API is the primary wire protocol (`/responses` and
`/v1/responses`) and is consumed as SSE, including text deltas, completion
usage, and Codex gateways that insert NUL padding. Chat Completions remains an
explicit compatibility adapter. `--backend echo` exists only in builds made
with `--features dev-fixtures`; normal release binaries cannot use the mock.

This checkout is under complete-framework delivery. Real provider requests,
Codex pairing, Responses streaming, bounded tool continuation, cancellation,
write/shell tools, and host approval are implemented. Context compaction,
true live steer/reconnect, skills/extensions, and packaged cross-platform
releases remain open. The authoritative status is the `CF-*` section in
[`Docs/Zenpi_Execution_Blueprint.md`](Docs/Zenpi_Execution_Blueprint.md); the
gap document is archived planning input, not a second checklist. Installation
currently requires Rust 1.88 or newer and a local clone.

### Headless protocol

Input and output are one JSON object per LF-terminated line. A payload may
contain U+2028 or U+2029; only LF frames a record. Diagnostics go to stderr so
stdout remains machine-readable.

Supported commands are `prompt`, `steer`, `cancel`, `approve`, `status`,
`handoff`, `resume`, and `shutdown`. Accepted prompts emit typed v2 progress
events and one v1-compatible terminal response. Live steer/reconnect remains
open and is not represented as completed behavior.

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
strictly below 5,000. The
aggregate Rust source inventory is reported for visibility, not used as a
project-wide cap; generated files, vendored dependencies, documentation, and
build output are not item estimates.

The authoritative execution plan is [`Docs/Zenpi_Execution_Blueprint.md`](Docs/Zenpi_Execution_Blueprint.md).
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
provider 工作在后台执行，TUI/headless 在流式响应期间仍可处理输入和取消。

生产默认 backend 是配置的 OpenAI-compatible provider，不再静默使用
`echo`。首次使用先执行 `zenpi config import-codex --profile codex`，它从
`~/.codex/config.toml` 读取 URL、Responses API 和模型，从
`~/.codex/auth.json` 读取 key，并将非 secret 配置写入
`~/.zenpi/config.toml`、key 写入权限为 0600 的 `~/.zenpi/auth.json`。
随后执行 `zenpi config doctor`，再运行 `zenpi --mode tui` 或不带
`--backend` 的 headless。也可用 `ZENPI_BASE_URL`、`ZENPI_API_KEY`、
`ZENPI_MODEL` 覆盖配置。`echo` 只存在于启用 `dev-fixtures` feature 的测试
构建；正常 release 无法启用它。缺少 provider 时 zenpi 会在创建 session
前失败，不会伪造回复。

当前已支持真实 provider、Codex 配对、Responses SSE（含 NUL padding）、
流式输出、工具 continuation、取消、读写/shell 工具与审批；真正的 live steer、
重连、上下文压缩、完整 session 管理、skills/extensions 和跨平台发布包仍未
完成。权威状态在 `Docs/Zenpi_Execution_Blueprint.md` 的 `CF-*` 部分。
每个 Blueprint item 都为其实现/测试代码声明小于 5000 的 `Estimated LOC` 预估值；仓库 Rust 总行数只作
信息性盘点，不是项目级上限。

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
`zenpi config doctor`、`zenpi --mode tui` です。最初のコマンドは
`~/.codex/config.toml` の URL、Responses API、モデルと
`~/.codex/auth.json` の key を読み、`~/.zenpi/config.toml` と権限 0600 の
`~/.zenpi/auth.json` に安全に保存します。環境変数
`ZENPI_BASE_URL`、`ZENPI_API_KEY`、`ZENPI_MODEL` も使用できます。テスト用の
`echo` は `dev-fixtures` feature のテスト build だけで有効です。通常の release
では使用できず、provider がない場合は session 作成前に失敗します。

Responses SSE、Codex pairing、streaming、tool continuation、cancellation、
write/shell tools と approval は実装済みです。true live steer/reconnect、
context compaction、完全な session 管理、skills/extensions、cross-platform
release artifacts は Blueprint の `CF-*` 項目として未完了です。
各 Blueprint item には実装・テストコードの `Estimated LOC` 予測（5000 未満）を記載します。リポジトリ全体の
Rust 行数は情報表示のみで、プロジェクト全体の上限ではありません。

## License

MIT. See [LICENSE](LICENSE).
