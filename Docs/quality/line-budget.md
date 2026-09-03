# Per-Item Code LOC And Source Inventory

The authoritative per-item limit is attached to each checklist row in
`Docs/Zenpi_Execution_Blueprint.md`. Every row has exactly one integer
`Estimated LOC` field, interpreted as the forecast of implementation/test code
lines attributable to that row (Rust/Python/Shell), not a current-file inventory,
and the declared forecast must satisfy the strict rule `0 <= Estimated LOC < 5000`.
Documentation, reconciliation, and publication-only rows use `0`. Estimates
are independent and are not summed into a project-wide cap.

The Blueprint validator is the sole acceptance owner:

```text
python3 tools/validate_blueprint.py --json
```

It checks the per-item cap, missing/duplicate/non-integer values, dependency
states, and the Gantt digest. `tools/check_rust_loc.py --json` remains a cheap
physical `.rs` inventory for visibility and capacity planning; it does not
gate the aggregate by default. An operator may supply `--max-lines` for an
ad-hoc diagnostic, but that optional check is not this Blueprint's acceptance
gate.

Final post-correction result on 2026-09-03: the informational Rust inventory
reports `4923` physical lines across 16 files, while the maximum declared per-item
estimate is `1400`; the acceptance rule is therefore evaluated per row, not by
the aggregate number.

Dependencies remain intentionally narrow: Ratatui plus crossterm for one TUI
stack, Serde/serde_json for typed JSONL, thiserror for errors, unicode-width
for safe layout, ureq for one blocking OpenAI-compatible adapter, sha2 for
content digests, and tempfile only in tests. No async runtime, second UI,
server framework, broker, scheduler, or plugin host is linked. `Cargo.toml`
declares Rust 1.88, matching Ratatui 0.30's published MSRV.
