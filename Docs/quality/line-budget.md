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

The final validation run recorded `5164` physical lines across 17 files (4604
under `src/`, 560 under `tests/`). That count may exceed 5,000 because the
acceptance rule is evaluated independently per row; the maximum declared
per-item estimate is `1400`.

Dependencies remain intentionally narrow: Ratatui plus crossterm for one TUI
stack, Serde/serde_json for typed JSONL, thiserror for errors, unicode-width
for safe layout, ureq for one blocking OpenAI-compatible adapter, sha2 for
content digests, and tempfile only in tests. No async runtime, second UI,
server framework, broker, scheduler, or plugin host is linked. `Cargo.toml`
declares Rust 1.88, matching Ratatui 0.30's published MSRV.
