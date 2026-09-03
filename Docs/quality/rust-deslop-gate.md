# Rust Deslop Gate

The gate applies the pinned `codex-rust-deslop` practices without adding a
second framework. Each concept has one owner: core state, session persistence,
headless framing, TUI lifecycle, backend adaptation, and b3 records. The
public runtime has only `RunMode::Tui` and `RunMode::Headless`.

Run the following commands from the repository root:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
rg -n 'unwrap\\(|expect\\(|unsafe|panic!' src --glob '*.rs'
python3 tools/check_modes.py
```

The final scan is reviewed rather than used as a blind text rule: fallible
runtime paths use typed `Result`; test fixtures may use `unwrap` for concise
setup. There is no `unsafe`, child process, shared lock, async runtime, or
background scheduler in the production path. Requests move bounded payloads
into the core, while compatibility conversion is limited to the session/b3
boundary. Invalid protocol, path, digest, budget, and terminal states fail
closed before mutation.

Recorded gate result on 2026-09-04: format, Clippy, and `cargo test --all-targets`
passed; the seven integration targets reported 22 passing tests. The headless
smoke reported 12 typed output responses and 4 durable session records, and
`python3 tools/check_modes.py` reported exactly `tui/headless`. The installed
release user smoke also passed the release build, isolated install, echo,
cross-process resume, local OpenAI-compatible fixture, invalid-input checks,
and PTY terminal restoration.
