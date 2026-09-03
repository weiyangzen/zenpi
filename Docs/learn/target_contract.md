# Learn Transform Target Contract

`learn_mode=transform` converts a locked subset of the TypeScript pi-agent,
b3ehive contracts, and codex-rust-deslop report into Rust zenpi behavior and
English decision notes.

## Source scope

The ten rows in `source_manifest.tsv` are the complete scope. The manifest is
locked by SHA-256 and source byte count; files outside those rows are context
only and cannot satisfy completion.

## Target contract

- Public runtime modes are exactly `tui` and `headless`.
- Shared Rust ownership is split among `core`, `session`, `protocol`,
  `headless`, `tui`, `backend`, and `b3`.
- Headless framing is LF-only JSONL over stdio with durable session records.
- TUI rendering is bounded, resize-safe, Unicode-width-aware, and coalesced.
- b3 integration carries inert handoff, lease, gate, evidence, route, estimate,
  and feedback records; it does not embed a scheduler or daemon.
- Rust quality gates follow the deslop note: typed refusal, ownership-first
  APIs, boundary compatibility, fail-closed errors, explicit cleanup, and
  failure-path tests.

## Traceability and rollback

Every target artifact names its source path and source hash. A failed transform
is rolled back by removing only the target artifact and reopening its manifest
row; source files are never modified. Master acceptance requires a manifest
path/hash audit plus the Rust tests, mode scan, and line-budget gate.

Route and instrument feedback examples are recorded in
[`evidence_records.md`](evidence_records.md); they are evidence, not another
checklist.
