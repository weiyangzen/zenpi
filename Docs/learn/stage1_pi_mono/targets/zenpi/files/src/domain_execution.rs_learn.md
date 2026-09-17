# `src/domain_execution.rs` per-file learn report (ZS1-081)

- Source: `src/domain_execution.rs`
- Complete read range: byte `[0,112548)`; 2,981 lines
- Bytes: `112548`
- SHA-256: `2f5ee732fdf80b99af0ab973eb4594b2369786ae4acc6a86768fc3ba49be890f`

## Durable execution owner

`ExecutionReceipt`, `ExternalResultManifest`, `ExecutionStatus`, `ExecutionError`, `ExecutionStore`, `HandoffStore`, `BlueprintHandoff`, `RunOutcome`, and `HandoffOutcome` define a bounded local receipt owner and an explicit external-worker handoff boundary. `BlueprintExecutor::run_next` and `handoff_next` perform deterministic local bookkeeping only: they select one dependency-ready item, write an evidence string and lifecycle transitions, and invoke no product command, provider, or nested agent. A receipt left `running` by a crash or cancellation is resolved by the owner (`run_next` re-selects and writes a terminal status; `cancel_running_for_goal` marks still-running goal attempts `cancelled`), never inferred as success.

## Store and lifecycle

`ExecutionStore` and `HandoffStore` open bounded snapshots, validate schema version, digests, IDs, and attempt identity, reload, upsert receipts, cancel running goal work, import external candidates, and persist via bounded atomic replacement with `O_NOFOLLOW` reads, a private temporary-file mode, and `restrict_private_file`. Stores are capped at 16 MiB and 4,096 receipts/requests; handoff inserts are idempotent by `handoff_id` and reject conflicting identity. Handoff requests are immutable data; claims and external execution are owned elsewhere. `path_for_session` and `handoff_path_for_session` keep state beside custom sessions or in the explicit `~/.zenpi` user store.

## Selection, budgeting, and evidence

`BlueprintExecutor::run_next`, `select_item`, `select_handoff_item`, `deterministic_cost`, budget checks, execution/handoff IDs, and receipt validators select dependency-ready work and enforce per-goal/attempt budgets. External-candidate reading, manifest-contract validation, safe relative evidence paths, worktree inspection, bounded file diffs, `prepare_external_evidence`, and `accept_external_candidate` separate worker declarations from host-verifiable evidence. `accept_external_candidate` is the module's only command-execution path: it runs the host-frozen validators (`/bin/sh -c <acceptance_command>`) recorded in the immutable `ExternalContract`, after the candidate matches that contract and the current worktree diff; worker-supplied commands are never executed. Git control paths, symlinks, hardlinks, oversized files, and malformed evidence fail closed.

## Mapping and limits

This maps to b3ehive/pi-mono execution and receipt ownership while adding explicit local-vs-external semantics, immutable handoffs, bounded snapshots, deterministic costs, evidence hashes, and master acceptance checks. It renders no TUI and establishes no provider/headless behavior; validator process execution, budget settlement, session events, and output capture are delegated to the `tools`, `governance`, `session`, and `tool_output` owners it imports.
