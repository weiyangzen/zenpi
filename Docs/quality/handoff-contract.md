# Handoff Contract Gate

`src/b3.rs` is a data-only bridge. It does not spawn a worker, execute an
evidence command, schedule retries, or open a second protocol. New handoffs use
`HandoffRecord` with schema version 1 and a SHA-256 digest over canonical JSON;
the session owner appends the validated record and syncs it before reporting
success.

## Bounds and refusals

- Record: at most 64 KiB; summary: at most 16 KiB; artifacts: at most 64.
- IDs and text reject empty/control/line-bearing values.
- Artifact references are repository-relative and reject absolute paths,
  traversal, URLs, credential filenames, and key material.
- `ResourceEnvelope` and `ResourceLease` reject over-budget checked additions;
  `validate_at`/`heartbeat` expire an inactive lease. `SideEffectGate` fails closed when
  absent, denied, or for the wrong kind.
- `ResultManifest` requires worker self-test before the literal Master can
  accept it; its checksum includes state and acceptance metadata.

## Evidence commands

```text
cargo test --test b3
tools/headless_smoke.sh
```

The b3 integration tests cover digest tampering, path refusal, accounting,
evidence, route, estimator, looper, and gate records. The smoke test proves a
signed `handoff_record` is durable and that invalid input does not add turns.
Validation command strings are labels stored in evidence; zenpi never invokes
them.

Recorded gate result on 2026-09-03: `cargo test --test b3` passed 4 tests and
the executable smoke passed its signed-handoff, sequence, size, path, and
malformed-frame checks.
