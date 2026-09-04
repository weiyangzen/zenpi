# Rust Deslop Gate

The gate applies the pinned `codex-rust-deslop` practices without adding a
second framework. Each concept has one owner: core state, session persistence,
headless framing, TUI lifecycle, backend adaptation, and b3 records. The
public runtime has only `RunMode::Tui` and `RunMode::Headless`.

Run the following commands from the repository root:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
rg -n 'unwrap\\(|expect\\(|unsafe|panic!' src --glob '*.rs'
python3 tools/check_modes.py
```

The final scan is reviewed rather than used as a blind text rule: fallible
runtime paths use typed `Result`; test fixtures may use `unwrap` for concise
setup. The owned background runtime joins its job threads, child processes use
scrubbed environments and bounded group termination, and the few Unix signal
calls are documented `unsafe` FFI boundaries. Requests move bounded payloads
into the core, while compatibility conversion is limited to explicit adapter
and session boundaries. Invalid protocol, path, digest, budget, and terminal
states fail closed before mutation.

Recorded gate result on 2026-09-04: format, strict Clippy, and all-target/all-
feature locked tests passed, including provider retry/multimodal fixtures,
live steer, replay protection, extensions, governance, recovery, diagnostics,
and security. The release headless smoke emitted 15 typed records with one
durable prompt path, and `python3 tools/check_modes.py` reported exactly
`tui/headless`. The installed release smoke passed isolated install,
cross-process resume, a Responses fixture, resize, streaming interruption,
and PTY terminal restoration. A separate production archive passed SHA-256,
SBOM, credential/fixture scan, and packaged-binary execution.
