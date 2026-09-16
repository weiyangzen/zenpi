# `src/domain_execution.rs` per-file learn report (ZS1-081)

- Source: `src/domain_execution.rs`
- Complete read range: byte `[0,112548)`; 2,981 lines
- Bytes: `112548`
- SHA-256: `2f5ee732fdf80b99af0ab973eb4594b2369786ae4acc6a86768fc3ba49be890f`

## Durable execution owner

`ExecutionReceipt`, `ExternalResultManifest`, `ExecutionStatus`, `ExecutionError`, `ExecutionStore`, `HandoffStore`, `BlueprintHandoff`, `RunOutcome`, and `HandoffOutcome` define a bounded local receipt owner and an explicit external-worker handoff boundary. The local owner records deterministic evidence bookkeeping only; it never invokes shell/provider/agent instructions. Running receipts are terminalized after restart or cancellation rather than inferred as success.

## Store and lifecycle

`ExecutionStore` and `HandoffStore` open bounded snapshots, validate digests/IDs/attempt identity, reload atomically, upsert receipts, cancel running goal work, attach/import external manifests, and persist via bounded atomic replacement. Handoff requests are immutable data; claims and external execution are owned elsewhere. `path_for_session` and `handoff_path_for_session` keep state beside custom sessions or in the explicit user store.

## Selection, budgeting, and evidence

`BlueprintExecutor::run_next`, selection helpers, deterministic cost, budget checks, execution/handoff IDs, and receipt validators select dependency-ready work and enforce per-goal/attempt budgets. External candidate reading, manifest-contract validation, safe relative evidence paths, worktree inspection, bounded file diffs, preparation, and `accept_external_candidate` separate worker declarations from host-verifiable evidence. Git/control paths, symlinks, hardlinks, oversized files, and malformed evidence fail closed.

## Mapping and limits

This maps to b3ehive/pi-mono execution and receipt ownership while adding explicit local-vs-external semantics, immutable handoffs, bounded snapshots, deterministic costs, evidence hashes, and master acceptance checks. It does not itself execute product commands, render TUI, or establish provider/headless behavior; those remain host responsibilities.
