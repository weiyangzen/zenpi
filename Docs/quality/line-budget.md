# Source and Dependency Budget

The hard source budget is 5,000 physical lines across `src/`, `tests/`,
`examples/`, and `benches/`. `target/`, lockfiles, generated documentation,
and vendored directories are excluded. The authoritative checker is:

```text
python3 tools/check_rust_loc.py --json
```

The final report is archived with the acceptance evidence. It must contain
`within_budget: true` and `total_lines <= 5000`; the Blueprint validator repeats
the same count so a stale quality note cannot close the gate.

Final pre-publication result on 2026-09-03: `4923` physical Rust lines across
16 files, leaving `77` lines of headroom.

Dependencies are intentionally narrow: Ratatui plus crossterm for one TUI
stack, Serde/serde_json for typed JSONL, thiserror for errors, unicode-width
for safe layout, ureq for one blocking OpenAI-compatible adapter, sha2 for
content digests, and tempfile only in tests. No async runtime, second UI,
server framework, broker, scheduler, or plugin host is linked. `Cargo.toml`
declares Rust 1.88, matching Ratatui 0.30's published MSRV.
