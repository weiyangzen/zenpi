# zenpi Research Index

This directory contains the English, read-only evidence used to freeze zenpi's
execution plan. Each note records the inspected source revision, observed facts,
design decisions, and acceptance implications. None is a competing checklist;
state lives only in [`../Zenpi_Execution_Blueprint.md`](../Zenpi_Execution_Blueprint.md).

| Note | Purpose | Evidence anchor |
|---|---|---|
| [`pi-agent-ts.md`](pi-agent-ts.md) | Inventory official `pi-agent` TypeScript core, session, terminal, and TUI behavior; record migration omissions | `pi-mono` commit `23282f60782f02b9e22b787e4b22af441454fa16` |
| [`b3ehive-occam.md`](b3ehive-occam.md) | Select inert first-class b3 records and reject redundant orchestration | `b3ehive` commit `76cb266a79ef1555a7c974af0b7340333dbd39e8` |
| [`codex-rust-deslop.md`](codex-rust-deslop.md) | Translate Rust refactoring practices into enforceable gates | `codex-rust-deslop` report SHA-256 `e92528d14bd26369ac7b7fbfc1edc87fa10fe3cbd8f22876a25d4c3cf5261be2` |
| [`zenpi-architecture.md`](zenpi-architecture.md) | Freeze module ownership, data flow, public modes, limits, and verification seams | zenpi execution Spec and current source layout |
| [`README.md`](README.md) | This index and provenance policy | Local repository state at the execution date |

## Evidence policy

Research is performed against a pinned local revision where possible. Paths and
line references are pointers for review, not copied implementation. A claim
that cannot be reproduced is labeled unknown rather than filled by assumption.
Credentials, session contents, generated binaries, and full upstream checkouts
never enter this directory. Changes to a decision require an English commit,
an updated note, and a corresponding Blueprint/Spec review.

## Acceptance links

- Normative execution policy: [`../Zenpi_Execution_Spec.md`](../Zenpi_Execution_Spec.md)
- Authoritative checklist: [`../Zenpi_Execution_Blueprint.md`](../Zenpi_Execution_Blueprint.md)
- Read-only projection: [`../Zenpi_Execution_Gantt.md`](../Zenpi_Execution_Gantt.md)
- Quality evidence: [`../quality/tui-performance.md`](../quality/tui-performance.md), [`../quality/handoff-contract.md`](../quality/handoff-contract.md), [`../quality/rust-deslop-gate.md`](../quality/rust-deslop-gate.md), and [`../quality/line-budget.md`](../quality/line-budget.md)

The Master closes the research rows only after all five files are present,
non-empty, linked, revision-pinned, and free of secret or copied-source
material.
