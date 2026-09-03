# Contributing to zenpi

All code, comments, commit messages, issue templates, and development notes
are written in English. The README is intentionally also available in Chinese
and Japanese; translations do not change protocol names or command spelling.

zenpi has exactly two public runtime modes: `tui` and `headless`. A proposed
third mode is a design change and must update the authoritative execution
blueprint before implementation.

## Required checks

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
python3 tools/validate_blueprint.py
python3 tools/generate_gantt.py
python3 tools/check_rust_loc.py
python3 tools/check_modes.py
tools/headless_smoke.sh
tools/headless_smoke.sh --release
python3 tools/user_smoke.py
```

The repository checks are intentionally dependency-free. `validate_blueprint.py`
parses the authoritative checklist, rejects duplicate or cyclic dependencies,
and verifies the same-prefix Gantt's source/specification digests, complete
monitoring index, and each row's strict `Estimated LOC < 5000` field.
`check_rust_loc.py` inventories physical `.rs` lines under `src/`, `tests/`,
`examples/`, and `benches/`; pass `--json` for informational telemetry.
`headless_smoke.sh` builds the deterministic echo backend, exercises malformed
and valid LF-JSONL frames, and confirms durable session and handoff records. It
accepts `--bin PATH` when a prebuilt binary is available; use `--allow-skip`
only in environments without a Rust toolchain.
`user_smoke.py` builds and installs the release into an isolated root, then
exercises help, echo, resume, the OpenAI-compatible adapter, invalid input, and
the PTY TUI through the installed binary.

Keep every checklist item's estimated implementation/test code attributable to
that item below 5,000 LOC. The aggregate source inventory is informational.
Prefer ownership-preserving APIs, typed refusal states, explicit
resource cleanup, and focused contract tests over convenience clones or broad
abstractions. Do not add a dependency without recording its reason and cost in
`Docs/Zenpi_Execution_Spec.md`.
