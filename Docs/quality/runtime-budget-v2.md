# Runtime And Size Budget Gate

`tools/bench_runtime.py` is the reproducible V2-002/V2-302 regression gate.
It builds the locked release binary, inventories direct normal dependencies
and feature flags, starts the real production headless binary with an isolated
home, measures that process with the platform `time(1)`, and runs the checked-in
Rust example `runtime_budget_probe` against the real runtime, Ratatui test
backend, render scheduler, and BentoBox layout model.

Run from the repository root:

```text
python3 tools/bench_runtime.py --samples 5 --output /tmp/zenpi-runtime-budget.json
```

The JSON receipt includes every cold-start sample and reports min, median, and
max. It does **not** calculate or claim p95: five local samples cannot justify a
tail-latency statement, and host-to-host timings are not directly comparable.
The limits are deliberately loose regression tripwires rather than product
performance promises.

Optional near-cap evidence is available with `--near-cap`. On the observed
arm64 Darwin host it generated a valid `251,658,240` byte journal (240 MiB),
then completed a release headless shutdown in `251.08 ms` with peak RSS of
`891,158,528` bytes. This is diagnostic evidence rather than a normal startup
gate; the current recovery owner retains validated records and event payloads
in memory, so large-valid-journal streaming/indexing remains an explicit
optimization item.

## Gates

| Surface | Limit | Meaning |
|---|---:|---|
| Direct normal dependencies | `<=16` | Cargo metadata, target dependency included, dev dependency excluded |
| Locked release binary | `<=8 MiB` | Unstripped `target/release/zenpi` on the measurement host |
| Cold headless shutdown | max `<=1000 ms` | Process startup, session open, one JSONL shutdown and clean exit |
| Cold process peak RSS | max `<=96 MiB` | Per-process `time(1)` maximum RSS, not `RUSAGE_CHILDREN` |
| Runtime queue round trip | `<=2000 us/op` | 2,000 submit-to-terminal cycles through `BackgroundRunner` |
| Ratatui render | `<=10000 us/frame` | 500 changing TestBackend frames, alternating standard/compact width |
| Layout compute | `<=100 us/op` | 60,000 tab/breakpoint computations including zero and one cells |
| Coalescing | exactly one retained frame | 10,000 dirty notifications before the same scheduler deadline |

The queue probe is intentionally serial. It measures bounded owner/event
round trips, not provider latency and not maximum throughput. The render probe
uses Ratatui's in-memory TestBackend, so it excludes terminal-driver and PTY
latency. Those scopes prevent an attractive number from being mistaken for an
end-to-end user promise.

The CI integration-test gate also runs with one test thread. Several loopback
provider fixtures use short-lived listeners; on macOS, parallel fixture
teardown can intermittently return `io: Invalid argument` even when the same
tests pass repeatedly in isolation. Serial execution retains the full
`--all-targets --all-features` matrix and removes that resource race from the
required gate.

## Current receipt

On the observed `arm64` Darwin host, the 2026-09-09 run passed every gate:
12 direct normal dependencies; `6,251,904` release bytes; three cold samples
`255.80/37.98/31.42 ms` (max/median/min); maximum process RSS `3,899,392`
bytes; queue `1659.95 us/op`; render `629.52 us/frame`; layout `1.077 us/op`;
and one scheduled frame after 10,000 dirty requests. The first cold sample
includes one-time operating-system/cache effects and is retained rather than
discarded. This receipt is a fresh local measurement after the canonical block
and external lifecycle changes.

The 2026-09-09 arm64 Darwin rerun after toolchain selection also passed every
gate: `6,301,904` release bytes; cold samples `317.37/37.15/34.19 ms`; maximum
process RSS `3,883,008` bytes; queue `1673.05 us/op`; render `630.52 us/frame`;
layout `1.072 us/op`; and one scheduled frame after 10,000 dirty requests.

The near-cap rerun reached the configured `251,658,240` byte journal boundary
and completed cleanly in `359.56 ms` with `891,830,272` bytes peak RSS. This
confirms the refusal boundary is exercised, while also making the retained
in-memory recovery cost explicit; streaming/indexed recovery remains open.

This receipt is evidence for this host only. Wall-clock results are not
deterministic, although the workload and pass/fail thresholds are. CI should retain its own JSON
artifact and compare the hard gates, not copy these sample values as universal
expectations.
