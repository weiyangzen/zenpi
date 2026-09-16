# Pipe fixture phase evidence: minimal test-only candidate

Only `src/extension_runtime.rs`, inside `#[cfg(all(test, unix))] mod pipe_deadline_tests`, changes. Baseline is the current read-only master SHA256 `3cd763f944b7a6dd5cbdc0959d71c5f514c240d501239773d443c87bb8e97f7a`; final is `7526689d532d21e31fee36ab703909a191b08c55b3e811b1a701614be0dc0297`. The patch adds40 lines and removes0. Master source and frozen130 package were not written. Removing only these additions recovers every original byte, including the C fixture, all deadlines, assertions and production prefix.

The parent now owns a `FixturePhaseLog` guard created immediately after its private fixture directory. On ordinary success, helper failure, watchdog return or Rust unwind, it snapshots `phases.log` before TempDir destroys that directory. Raw read is limited to8192+1 bytes; output identifies truncation and prints at most8192 raw bytes after lossy UTF8 decoding (at most3x expansion). Missing/unreadable logs remain explicitly unavailable. NOFOLLOW/NONBLOCK and regular-file metadata prevent symlink following or FIFO blocking. The guard does not change the test's success/failure result or create a persistent product recovery store.

`pipe_probe_child` writes `helper-entry` immediately after acquiring its private directory from the environment, before reading case options, loading the catalog or counting FDs. This distinguishes entry into the test body from later host-start; it is not an OS loader start marker. An outer process killed by SIGKILL/abort cannot execute Rust destructors; the guarantee here is the existing parent surviving and handling child exit/watchdog or its own unwind.

## Small, actual negative evidence

Per master's concurrent production-PTY constraint, no cargo build, full library test or PTY was started. One small standalone Rust probe links the existing libc rlib and compiles with `-D warnings`. `build_probe.py` extracts the actual candidate phase writer/guard and FixturePids code; its complete watchdog tail is byte-identical to both candidate and current-master `isolated_case`, verified before compilation. Build argv, input/code/dependency/binary hashes and timestamps are recorded.

The private negative creates a real independent child of this small probe. That child writes `helper-entry` and `intentional-watchdog-stall`, then sleeps10 seconds. The original2-second watchdog and500ms cleanup grace kill and reap it. The before/after comparison uses the same probe and child, toggling only whether the parent phase guard is installed; it is an observation-control comparison, not a run of the frozen production executable or actual API1/API2 pipe protocol.

- Before: outer_timeout=true, SIGKILL, observed_fixture_success=false, parent exit1; the94-byte phase file exists immediately before temporary directory removal, but its contents do not survive in stdout.
- After: outer_timeout=true, SIGKILL, observed_fixture_success=false, parent exit1; the same94-byte phase stream is present in the parent stdout before its temporary directory is removed. Recorded elapsed2506ms/2503ms includes the original cleanup grace and is not a relaxed product deadline.

Thus the intentional failure remains a failure; the diagnostic wrapper passes only because it verifies expected failure status plus evidence retention. No failed original test was converted to success or skipped.

Ten single-run cases cover this fixed before/after negative, real normal child exit, real nonzero17 child exit, caught parent unwind that still exits1, exact8192 bytes,9000-byte truncation to8192, missing file, FIFO rejection without blocking, and symlink rejection without disclosing its external sentinel. `probe-runs.json` records each command, expected/actual exit, stdout/stderr hashes and timestamps. No negative was retried until green.

## Integrity and scope

`integrity.json` verifies format, one-file patch replay, exact byte reversal to baseline, identical production prefix and all original assertions/deadlines. The test-only C source,800/1000ms product caps,200ms predicate threshold,1500ms inner assertion,2s outer watchdog,500ms failure cleanup and original success assertions are untouched.

This proves compilation and actual behavior of the extracted observation code with the unchanged watchdog. It does not claim complete crate compilation, API1/API2 endpoint coverage, a fix for native startup latency, or current master full G-CODE. Master must run its appropriate targeted compile/test when the concurrent workload permits. No product patch, language replacement, prewarming, timeout changes, broad investigation or new subagent/task was introduced.

Apply only `changes.patch` against the declared one-file master baseline. Rollback removes only these40 test-observation lines, preserving all previous fixture and130 code. Existing frozen130 manifest remains `067fcead5c8c65df8c12814f25feafbf896ae6c633947799a1d6cbd9c4e0fa32`.
