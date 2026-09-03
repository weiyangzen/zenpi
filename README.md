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
are synchronous in this small release.

### Quick start

```bash
# Install the `zenpi` binary (or use `cargo run --` while developing).
cargo install --path . --locked
zenpi --help

# Interactive terminal mode, using the credential-free echo backend.
zenpi --mode tui

# Scriptable JSONL mode.
printf '%s\n' '{"type":"prompt","id":"1","text":"Say hello"}' \
  | zenpi --mode headless --backend echo --session ./session.jsonl
```

The default backend is deterministic `echo`, which makes local tests and
protocol integration safe without credentials. Select `--backend openai` for
an OpenAI-compatible endpoint and configure `ZENPI_API_KEY`, optional
`ZENPI_BASE_URL`, and `ZENPI_MODEL`.

This is a usable synchronous MVP: the `echo` path works offline, while the
OpenAI-compatible path sends a non-streaming chat-completions request. The
provider call intentionally blocks the current turn; streaming and background
worker orchestration are not implemented in this release. This repository is
currently distributed as source rather than a crates.io package or prebuilt
GitHub Release, so installation requires Rust 1.88 or newer and a local clone.

### Headless protocol

Input and output are one JSON object per LF-terminated line. A payload may
contain U+2028 or U+2029; only LF frames a record. Diagnostics go to stderr so
stdout remains machine-readable.

Supported commands are `prompt`, `steer`, `status`, `handoff`, `resume`, and
`shutdown`. In this synchronous release, `steer` can only be evaluated between
completed provider requests; stdin is not consumed while a provider call is in
flight. Every accepted prompt and resulting assistant message is appended to
the session JSONL file before its completion event is emitted. A malformed line
receives a typed error and does not mutate the session.

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

默认 `echo` backend 不需要凭据，适合本地测试。需要模型服务时可配置
OpenAI-compatible backend。协议、资源预算、验收门和迁移证据见 `Docs/`。
可用方式是先执行 `cargo install --path . --locked`，然后运行 `zenpi --mode tui`，
或使用 `zenpi --mode headless --backend echo` 接收 JSONL。OpenAI-compatible
调用是同步的非流式请求；本版本不包含后台 worker 编排。
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
この小さなリリースでは provider 呼び出しは同期式で、ネットワーク遅延中は
入力処理が一時的に待機します。
`cargo install --path . --locked` で `zenpi` をインストールし、
`zenpi --mode tui` または `zenpi --mode headless --backend echo` として実行できます。
OpenAI-compatible 呼び出しは同期・非ストリーミングで、バックグラウンド worker の
オーケストレーションはこのリリースに含まれません。
各 Blueprint item には実装・テストコードの `Estimated LOC` 予測（5000 未満）を記載します。リポジトリ全体の
Rust 行数は情報表示のみで、プロジェクト全体の上限ではありません。

## License

MIT. See [LICENSE](LICENSE).
