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
zenpi config import-codex
zenpi config doctor

# Interactive terminal mode, using the imported provider.
zenpi --mode tui

# Scriptable JSONL mode.
printf '%s\n' '{"type":"prompt","id":"1","text":"Say hello"}' \
  | zenpi --mode headless --session ./session.jsonl
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
explicit compatibility adapter. `--backend echo` is a test fixture only and
must be named explicitly; it is not an AI provider.

This checkout is under complete-framework delivery. The current foundation
includes real provider requests, Codex pairing, bounded SSE parsing, a typed
read-only tool registry, and a bounded background runner. The complete feature
contract and remaining acceptance work are tracked in
[`Docs/Zenpi_Complete_Feature_Gap.md`](Docs/Zenpi_Complete_Feature_Gap.md);
the older execution Blueprint records the original MVP contract and must not
be read as proof that the full framework is finished. Installation requires
Rust 1.88 or newer and a local clone.

### Headless protocol

Input and output are one JSON object per LF-terminated line. A payload may
contain U+2028 or U+2029; only LF frames a record. Diagnostics go to stderr so
stdout remains machine-readable.

Supported commands are `prompt`, `steer`, `status`, `handoff`, `resume`, and
`shutdown`. Every accepted prompt and resulting assistant message is appended
to the session JSONL file before its completion response. Full non-blocking
headless event streaming, in-flight cancellation, and provider tool-call
continuation remain tracked complete-feature work rather than being silently
represented as synchronous success.

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
当前版本的 provider 调用是同步的，网络延迟可能暂时阻塞输入处理。

生产默认 backend 是配置的 OpenAI-compatible provider，不再静默使用
`echo`。首次使用先执行 `zenpi config import-codex`，它从
`~/.codex/config.toml` 读取 URL、Responses API 和模型，从
`~/.codex/auth.json` 读取 key，并将非 secret 配置写入
`~/.zenpi/config.toml`、key 写入权限为 0600 的 `~/.zenpi/auth.json`。
随后执行 `zenpi config doctor`，再运行 `zenpi --mode tui` 或不带
`--backend` 的 headless。也可用 `ZENPI_BASE_URL`、`ZENPI_API_KEY`、
`ZENPI_MODEL` 覆盖配置。只有测试时才显式传 `--backend echo`；缺少 provider
时 zenpi 会在创建 session 前失败，不会伪造回复。

当前基础层已支持真实 provider、Codex 配对、Responses SSE（含 NUL padding
兼容）、有界只读工具注册表和后台 runner；完整工具循环、实时取消/steer、
上下文压缩、session 分支、skills/extensions 和发布包仍按完整功能清单验收，
不会用 MVP 的绿色测试冒充完成。
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
`echo` mock は使いません。最短手順は `zenpi config import-codex`、
`zenpi config doctor`、`zenpi --mode tui` です。最初のコマンドは
`~/.codex/config.toml` の URL、Responses API、モデルと
`~/.codex/auth.json` の key を読み、`~/.zenpi/config.toml` と権限 0600 の
`~/.zenpi/auth.json` に安全に保存します。環境変数
`ZENPI_BASE_URL`、`ZENPI_API_KEY`、`ZENPI_MODEL` も使用できます。テスト用の
`echo` は `--backend echo` を明示した場合だけ有効で、provider がない場合は
session 作成前に失敗します。

Responses SSE と Codex gateway の NUL padding、Codex pairing、型付きの
read-only tool registry、background runner は実装済みです。完全な tool loop、
in-flight cancel/steer、context compaction、session fork、skills/extensions、
release artifacts は完全機能清单に従って引き続き受け入れます。
各 Blueprint item には実装・テストコードの `Estimated LOC` 予測（5000 未満）を記載します。リポジトリ全体の
Rust 行数は情報表示のみで、プロジェクト全体の上限ではありません。

## License

MIT. See [LICENSE](LICENSE).
