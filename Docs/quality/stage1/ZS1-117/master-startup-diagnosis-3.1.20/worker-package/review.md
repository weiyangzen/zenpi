# ZS1-117 bounded cold-start diagnosis — candidate, cause still unresolved

The original gate remains **FAIL: 1208.732708 ms > 1000 ms**. No product patch, budget edit, build, signing/quarantine/cache change or acceptance update was made. Master authority supplied for this request is 3.1.20 / `8d525b351d066ce9b0487a485337647d47233275782e4f4d154423a623fe1645` (112/15/97); this is not the SHA of the blueprint Markdown. The observed Markdown and source file hashes are separate in read-identities.json.

## Evidence that advances the diagnosis

Read-only macOS unified logs retained an event pair during the original budget command (06:02:50.853468–06:03:34.410434 UTC). The original log also reports a 0.13 s release build, outside the five startup timers.

| Historical event | Local timestamp (+0800) | Correlation |
| --- | --- | --- |
| GK performScan | 14:02:51.944684 | hashed path ba2be7534db6fb27 |
| GK evaluateScanResult | 14:02:52.381298 | same hashed path; Identifier zenpi-cdb646c6c40059cd |
| Found provenance data | 14:02:52.381496 | same hashed path and Identifier |

The scan/result interval is **436.613667 ms**, calculated from the two log machTimestamp values, matching boot UUIDs, and the local mach_timebase_info ratio 125/3. Wall timestamps independently agree at displayed precision. codesign -dvvv on the unchanged release reports that same Identifier and an ad-hoc linker signature. This command only displays existing signing information.

This is specific evidence of execution-policy activity associated with an executable carrying the release's Identifier, not merely an inference from a slow first sample. It is **a candidate contributor, not a proven root cause**: the logged path is hashed, the historical event does not carry the release SHA or sample target PID, and the old benchmark has no sample wall/monotonic endpoints, exec, main, or phase records. A code-signing Identifier alone is not a unique file-content identity. The event interval is not established as target-blocking time, nor does it explain all 1208.733 ms. Do not subtract 436.614 ms from the gate or call the remainder product time.

The same syspolicyd thread has earlier trust/network activity. It later processes other path hashes. Therefore thread reuse is explicitly visible: the earlier network/trust records cannot be charged to Zenpi, and “0.87 s in one policy thread” is not a valid target duration. Raw selected logs, exact predicates, return codes and timebase computation are retained. No system setting was changed.

## Fixed, separately labeled new observations

Before launching, both the binary (6,000,768 bytes, SHA `7a374a3b0764c7a4db9b3b53e36dc07d3d8b17b80f5711805a932a2090e60774`) and the original tools/bench_runtime.py hash matched the previous audit. No --help or warmup launch occurred. Exactly five new launches were chosen in advance, coordinated with B and root while neither was building/running PTYs or tests. All five completed; no retries or discarded launch results. This coordination reduces one known source of contention, not a claim that the entire machine was idle.

These are **already-executed-binary diagnostic observations**, not recreation of the old first-ever launch/cache/signature state. The parent uses Popen plus selector timestamped pipe reads instead of subprocess.run. Fixed inputs match the original shutdown workload: /usr/bin/time -l, original binary path and cwd, inherited environment with the same five overrides, one initially absent shared ZENPI_HOME, five distinct new session files, 5 s deadline. File persistence and RSS parsing happen after the recorded capture endpoint. Each round retains raw stdin/stdout/stderr (including all time(1) counters), command, wrapper PID, same-clock marks, return code, and full fixture/journal/reconnect snapshots; previous sessions are intentionally present in later shared-root snapshots.

| Round | Parent total ms | Popen return ms | First stdout observed ms | First stdout → wrapper reaped ms | time(1) real | RSS bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 50.243208 | 2.166542 | 49.221583 | 1.021375 | 0.04 | 5160960 |
| 1 | 42.206084 | 1.497209 | 41.536750 | 0.668959 | 0.03 | 5226496 |
| 2 | 36.007542 | 1.239750 | 34.916958 | 1.090084 | 0.03 | 5177344 |
| 3 | 40.131875 | 1.526667 | 39.482542 | 0.649042 | 0.03 | 5193728 |
| 4 | 41.454875 | 1.511917 | 40.715833 | 0.738709 | 0.03 | 5226496 |

All five exit 0 and emit exactly one successful shutdown response. Raw time(1) has rounded 0.00 user/sys, zero page faults and swaps; these short new rounds say nothing about the unretained counters of the old slow round. First response observation is pipe-read time, not the exact internal flush time. No target PID is claimed from the wrapper PID; journals retain their own session IDs but no sampled exec lineage. Journals have a session header and extensions_selected with an empty tool list; reconnect receipts retain shutdown response. Journal wall-clock timestamps are **not** subtracted from parent monotonic marks.

## Boundaries established versus missing

| Phase requested | What this packet actually establishes |
| --- | --- |
| Process launch | parent launch-begin → Popen-return; this is wrapper admission, not successful target exec or loader completion |
| Config/session initialization | source route is preserved by the prior audit; no same-clock internal entry/completion, so duration unavailable |
| Actual command | input-supplied and first successful shutdown response observed; intervening initialization/read/drain cannot be separated |
| Exit | response observation → stream EOF → wrapper reaped; target-exit time distinct from wrapper is unavailable |

All new parent marks use the same perf_counter_ns clock. The historical OS pair has its own same-clock ticks. No mapping between the original missing Python timer and OS ticks is invented. /usr/bin/time's rounded real interval is kept raw, not mixed as an exact parent timestamp.

`nm` exposes only the Mach-O header as a defined symbol, not named Rust product functions. This limits symbol-based phase tracing; it does not mean entry-address disassembly is impossible. A bounded DTrace capability probe failed with `DTrace requires additional privileges` and SIP notice. No privileged retry, debugger attachment or product instrumentation was attempted. The private observe.py is the exact executed observer, not a general replay/test runner: its timeout branch kills only the wrapper and does not guarantee descendant cleanup; no timeout occurred in these five observations. Do not adopt this prototype as CI tooling without fixing that path and coordinating a new window.

## Next discriminating step and disposition

The five new short rounds do not reproduce a hotspot, so a product patch is unjustified. Preserve the old failure and all eight gate definitions. The useful next measurement is at the next naturally new release's **first launch**, preserving raw streams, process identity and a common-clock exec/main/config/session/host/response/exit trace alongside execution-policy events before any warmup. Reliable internal markers would require either supported trace access or an explicitly separate instrumented candidate; changed bytes would remain diagnostic and could not rewrite this historical gate. Do not induce coldness by clearing OS security/provenance caches or loosening signature checks.

This bounded investigation is complete as evidence collection, while the requested exact cause remains **unresolved**. Stronger historical scan correlation narrows where to observe next; it is not ZS1-117 acceptance or a performance fix. Only this new Docs packet is staged; the master checkout, original packets and inherited dirty product state remain untouched.
