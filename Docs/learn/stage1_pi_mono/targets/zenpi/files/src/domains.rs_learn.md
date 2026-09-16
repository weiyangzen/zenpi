# `src/domains.rs` per-file learn report (ZS1-082)

- Source: `src/domains.rs`
- Complete read range: byte `[0,24622)`; 752 lines
- Bytes: `24622`
- SHA-256: `c11e10d02a21ed3412e731acd269080053b18f649935bb1f9486b128ee7c79ac`

## Domain model and bounded validation

`DomainError` is the shared validation error for the data-only domain layer. The bounded text, instruction, identifier, version, digest, and record-length helpers reject empty, oversized, malformed, or non-canonical values before they enter a domain record. The limits make serialized records and validation costs predictable.

## Blueprint and dependency graph

`BlueprintItem`, `BlueprintTask`, and `Blueprint` model declarative work, task dependencies, owners, budgets, and evidence metadata. Canonical bytes, canonical JSON, and digest helpers provide stable identity for a blueprint. Dependency validation checks references, duplicate identifiers, and cycles, so the graph is a validated DAG rather than an execution loop. This module stores and validates intent; it does not schedule or run tasks.

## Lease, goal, and learn records

`LeaseRef`, `GoalStatus`, and `Goal` represent ownership, lifecycle transitions, budget accounting, and bounded goal metadata. Transition and budget checks reject invalid state changes, negative or excessive consumption, and mismatched lease/attempt identity. `Learn` and its evidence references capture durable learning facts and their source attestations, with validation of paths, digests, ranges, and bounded descriptions.

## Scope and integration boundary

The file supplies serializable domain contracts consumed by execution, governance, headless, and TUI owners. It contains no provider calls, shell execution, worker spawning, nested-agent orchestration, terminal rendering, or filesystem mutation. Runtime owners must enforce permissions and perform side effects around these validated records.
