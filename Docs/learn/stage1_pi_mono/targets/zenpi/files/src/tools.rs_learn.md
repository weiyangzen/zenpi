# `src/tools.rs` per-file learn report (ZS1-077)

- Source: `src/tools.rs`
- Complete read range: byte `[0,119778)`; 3,321 lines
- Bytes: `119778`
- SHA-256: `c3149b926933e5800cd41a2e8d3fcf6f109f8a455f2ef6ed8e2d48d696dbe33c`

## Policy and gate ownership

`ToolSideEffect`, `SideEffectPolicy`, `ToolOrigin`, `BlueprintPolicySpec`, `BlueprintGate`, `BlueprintRevocation`, and `PolicyEvidence` compile host-owned policy snapshots. Built-in credential/prohibition rules cannot be overridden by model arguments. Path checks normalize workspace-relative paths, reject credentials and traversal, and distinguish inspection, write, and command grants. Revocation is checked at admission and during cancellable execution.

## Tool contracts and results

`ToolDefinition`, `ToolCall`, `ToolPreview`, `ToolResult`, `ToolFailure`, `ToolErrorCode`, `ToolError`, and `ToolContext` define model discovery, typed failures, bounded human approval previews, fixed workspace roots, output capture, origin, search backend, and blueprint gates. `ToolContext` revalidates every call locally, independent of catalog metadata. `ToolExecutionMode` and `Tool` separate read-only parallel work from sequential side-effect barriers.

## Registry and execution

`ToolRegistry` registers read-only/all built-ins, validates unique definitions, exposes capability metadata, builds approval previews, and routes `execute`, `execute_cancellable`, and compact-result paths through the same gate. `execute_inner` preserves call correlation, cancellation, output bounds, and typed failures. `compact_tool_result` keeps model-visible output bounded without hiding failure state.

## Built-in tools

`ReadToolOutputTool`, `ReadFileTool`, `ListDirectoryTool`, `SearchTextTool`, `FindFilesTool`, `WriteFileTool`, `EditFileTool`, and `RunCommandTool` implement workspace-scoped reads, search, exact writes/edits with previews and digests, and supervised commands. Search process budgets reserve all potential child starts before dispatch. Writes render bounded unified diffs and use atomic replacement. Commands use explicit argv confinement, process-tree termination, bounded stdout/stderr capture, and cancellation; user-shell execution remains a distinct host-owned path.

## Bounds and safety helpers

Path/identifier/string validation, UTF-8 truncation, limited streams, diff source/digest helpers, coarse linear unified diff rendering, process-tree termination, and atomic writes keep model-controlled inputs bounded. `read_bounded_text` rejects clipped exact-edit sources, while `read_diff_source` reports clipping for preview without pretending it is exact.

## Mapping and limits

This file maps to pi-mono's tool registry, read/write/search/bash tools, and approval previews, adding Zenpi's policy snapshot/revocation, owner context, workspace confinement, process budgets, atomic edits, and bounded output receipts. It does not prove TUI approval rendering, core admission, headless routing, or real PTY behavior; those remain cross-file obligations.
