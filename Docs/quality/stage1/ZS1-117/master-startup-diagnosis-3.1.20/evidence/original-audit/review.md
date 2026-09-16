# Current release startup gate: read-only boundary audit

Worker A, provisional. Main and B/130 remain untouched. No builds, benchmark invocations, warmups, replacement samples or threshold changes. The original failed gate remains failed; this document proposes observation and does not claim a performance fix.

## What failed and what the number measures

Master receipt: `external-editor-integration/budget.run.json`, original command `python3 tools/bench_runtime.py --samples 5 --output .ops/stage1_execution/external-editor-integration/budget.json`, exit 1. Recorded cold-start milliseconds: **1208.732708, 35.488291, 33.633667, 37.030875, 35.111416**. The maximum exceeds the unchanged 1000 ms gate by 208.732708 ms. Median 35.488291 ms does not replace the max criterion. Other seven gates passed. Product size 6,000,768 bytes; `read-identities.json` records the currently read exact release hash and source/receipt identities.

`tools/bench_runtime.py:118–142` starts Python `perf_counter_ns` immediately before `timed_process` and stops after it returns. On Darwin, `timed_process:63–89` starts `/usr/bin/time -l <binary> --mode headless --session <new-file>` using `subprocess.run`, sends one shutdown JSONL frame, captures stdout/stderr, waits for process exit, then scans stderr for peak RSS. The elapsed span therefore includes:

1. Python wrapper setup/child launch and `/usr/bin/time` launch;
2. target executable launch/loader/pre-main runtime;
3. Zenpi argument/config/session/tools/resources/extensions/model initialization;
4. owned headless replay/owner/threads setup, reading the already supplied shutdown;
5. runner drain, cached successful shutdown response, owner/session closure, process exit;
6. wrapper exit/output collection and Python RSS parsing.

The release build occurs **before** the timed loop. Dependency metadata and the later runtime probe are also outside these five spans. The outer `budget.run.json` 43.56 seconds includes the entire tool, not just cold startup. Peak RSS is measured separately and is not a phase timer.

The wrapper's raw time(1) `real/user/sys`, faults and context-switch counters are captured in memory but discarded on successful samples; `budget.json` retains only total elapsed and RSS. `budget.log` contains build warning plus aggregate JSON, not each measured child's raw streams. Temporary fixture roots are deleted on exit. No PID-correlated exec/main/readiness/exit milestones, per-stage durations or startup journal copies remain. **These existing receipts cannot attribute the 1208.73 ms sample to executable loading, a particular initializer, shutdown, or scheduling.** The first-sample pattern is insufficient evidence of an OS cause.

Each sample opens a different new session, but all five share one temporary `ZENPI_HOME`; sample zero sees its initial absent child directories and later samples may see files/directories created earlier. This is a recorded workload distinction, not a diagnosed cause or a request to pre-create those directories. Cwd stays main, so workspace config and `.zenpi/skills`/`.zenpi/prompts` are not necessarily excluded merely because user home is temporary. The benchmark copies the environment then sets only its listed provider/home variables; the old receipt does not inventory all remaining effective configuration.

## Actual product route and separation from B's fixture

| Read-only entry | Work inside the measured lifetime |
|---|---|
| `src/main.rs:3`, `core::run:5747` | Rust main → shared core entry → argument parsing; no readiness receipt emitted. |
| `core.rs:5978–6018` | `make_backend` resolves workspace config; SessionStore open; Agent creation/recovery acknowledgement; builtin registry/workspace; approval setup; restore resources; configure extensions; restore/select model. |
| `core.rs:6244` | Resource defaults include both temporary user paths and cwd `.zenpi/skills`/`.zenpi/prompts`. |
| `config.rs:148`, `extensions.rs:162`, `extension_runtime.rs:115` | `ZENPI_HOME` determines extension root; nonexistent root gives empty catalog. Subprocess initialize only occurs while iterating catalog extensions. A fresh benchmark home has no installed extension fixture. Core still performs empty runtime setup and appends `extensions_selected`, so “no extension subprocess” does not mean that phase has zero cost. |
| `core.rs:6020`, `headless.rs:744/4234` | Headless branch runs ReplayState open, live owner registration, stdin thread, project owner pool/checkpoint restore and bounded background worker. |
| `headless.rs:4925`, `4668–4694` | Runtime closed event emits successful shutdown acknowledgement; runner join and owner/replay/session closure still belong to the timed lifetime. |

`RunMode::Headless` does not call the TUI `run_async_with_profile` branch or 130's ExternalEditorHost capture/start/poll. This rules out direct execution of those TUI functions for this workload, not a binary layout/loading effect and not an attribution of the observed stall. B's rejected-resume extension fixture uses its own executable/protocol phase; the bench sends shutdown with an empty installed-extension catalog. Do not transfer B's initialize/phase diagnosis or adjust its fixture timeout based on this unrelated total-duration receipt.

## Minimal discriminating observation plan — proposed, not executed

Keep the original failed receipt and fixed release identity. Any diagnostic run must be separately labeled and cannot replace its sample zero or change the eight gate definitions. Do not probe the executable with a warmup `--help` first, subtract a convenient launcher median, or rerun until max passes.

**First retain the observations already available without modifying the product.** In an isolated copy of the benchmark tool, preserve for every original sample: full command/PID lineage, parent start/end timestamps, exact binary hash, raw child stdout/stderr including `/usr/bin/time -l` output, return code, RSS and that sample's final journal/WAL. Persist captured data only after its original timing endpoint so file-writing work is not newly included in the gate. Retain the same fresh-home/session sequence, environment overrides and cwd. Raw wrapper `real` versus Python total can identify a large outer-wrapper/collection discrepancy; user/sys/fault counts can guide the next observation, but none proves “loader delay.” Filesystem event timestamps alone are also not proof of main entry.

**A target-entry timestamp is indispensable for launch versus product attribution.** For the exact unmodified stripped release (`Cargo.toml` has `strip = "symbols"`, thin LTO), use a supported process tracer only if it can resolve the actual entry and correlate: parent launch P0, successful target exec E, earliest Rust main/core entry M, entry into headless H, shutdown response R, target exit X, parent capture complete P1. Trace availability, symbol/offset resolution and permissions have not been established here. Do not pretend a generic exec trace supplies M or that attaching after the stall recovers it. Breakpoints/tracing perturb execution; keep that receipt diagnostic and report tracing overhead rather than certifying the original gate with it.

The useful intervals are P0→E (launcher/exec path), E→M (loader/runtime before product entry), M→H (product initialization), H→R (host setup plus shutdown/drain), R→X (remaining close/exit), X→P1 (wrapper/capture). A shared monotonic clock or an explicitly calibrated clock mapping is required; independently zeroed Rust Instants and Python clocks cannot be subtracted directly. E→M remains a pre-entry interval including runtime/scheduling; it is not by itself proof of loader I/O. Precise loader-only attribution would require loader/system events in the same trace.

**If the fixed stripped binary cannot supply M/H/R, report that limitation.** The smallest fallback is a separately identified isolated diagnostic candidate with opt-in, bounded timestamp markers at main entry, core entry, immediately after backend/session/tool/resource/extension/model phases, before headless, response flush and core return. Use existing libc/standard facilities, one common monotonic clock, fixed metadata-only records and preallocated storage; avoid new files/formatting on every marker where buffering can defer emission. Preserve normal stdout protocol and the same workload. No threshold, warmup or algorithm change. This candidate's changed bytes/loading properties mean it can locate a reproduced product-initialization stall but **cannot retroactively prove the original fixed binary's 1.2-second launch cause**. If it does not reproduce the slow phase, attribution remains open.

Only after an actual long interval is captured should work narrow further. For example, M→H dominated by `SessionStore::open` justifies session I/O investigation; an empty-catalog extension phase can be measured separately from B's child initialize; a large E→M with short M→H supports a pre-product-entry location but no OS-specific conclusion without trace evidence. No startup product patch is currently justified by these aggregate logs.
