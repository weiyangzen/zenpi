# ZS1-025 — actual Bash source execution

Worker candidate [_], authority 3.1.16 / c0262492bc6e3b4d5f56b35bedc7c1d1658854df11c4a975eb612a514cceef8f. Subject packages/coding-agent/src/core/tools/bash.ts: all 400 lines / 13,665 bytes read, frozen SHA-256 b76645f5d7b414957c7772eb8ec75d9dee71d27320ebcbf419881451576a6eee at upstream bbb61e34aaf231639fdaaad1adbd757947034eac. Complete review is learn-report.md; prior-worker-report.md preserves existing static evidence. Accepted 024 receipt, report and full referenced-artifact hash verification are reused under accepted024; no second acceptance is claimed.

## Actual result and boundary

Node24.19.0 Darwin arm64, real Vitest4.1.9/TypeBox1.3.27/cross-spawn7.0.6. Fifteen passed, zero failed/skipped/todo: 12 supplemental scenarios exercising actual shell/process/temporary-FS paths and three unmodified original regression cases. The original 5208 case injects BashOperations; original 5303 cases use fake ChildProcess streams and fake timers. They are not real-process evidence. case-review.json distinguishes them. Source and tests are unchanged; tools.test.ts/full upstream package were not run.

The only runtime implementation replacement is renderer-bridge.mjs: createShellRenderers returns {}, and its BASH_UPDATE_THROTTLE_MS=100 matches the frozen original renderer. UI/rendering is not executed or accepted. Actual schema, wrapper, local BashOperations, spawn, environment, child tracking, process-group kill, waitForChildProcess, OutputAccumulator, truncation, config/path and WriteStream closure run unchanged. Some cases supply explicit Context metadata/spawnHook or a resolver selecting /bin/bash -s/missing executable; these are labelled fixtures, not mocked process execution.

Real assertions cover merged streams/control characters, empty/nonzero output, stale PI metadata cleanup and context env/cwd/prefix/hook, invalid/pre-aborted zero-side-effect inputs, real spawn error, stdin transport, 2000/2100 lines, UTF8 chunk splitting, 154000-byte single line, raw invalid bytes, live spill/update/final-close, actual timeout/abort group kills with direct and same-group descendant PIDs observed absent, self-SIGTERM returning null exit as success, active inherited descendant drain and quiet detached descendant survival. The last case records survival then kills only its own known PID and verifies absence. Every helper has a seven-second safety exit. There are no intentionally persistent background processes.

Do not infer universal reap or complete-command output: quiet detached descendants can outlive tool return; idle grace stops reading after 100ms of post-exit silence; killed commands preserve only captured prefixes. Single decoding state across merged stdout/stderr can replace a UTF8 character when another stream inserts bytes mid-character. fullOutputPath can be visible before disk flush and lacks hash/completion/quota/private-access/restart metadata. Source preserves control characters. Error details are often embedded only in text. Windows/WSL/PowerShell, parent crash, disk failure, callback exceptions and product integration remain unexecuted. Context-only support files are not new independent source completions.

## Evidence and replay

source-inventory.json binds unchanged copies before execution, source-verification.json confirms original/copy hashes after final execution. test-results.json/stdout/stderr and probe-observations.json are actual final results. fixture-inventory.json binds exact generated files and raw output preserved losslessly in fixtures/*.gz. Some temporary raw files from preliminary runs are retained in the temp inventory; only the final result/observation counts are claimed. installed-dependency-files.json and package-lock.json identify real dependencies; node_modules is not shipped. manifest.json hashes every regular frozen file except itself. Prior 012/014/018 packages are unchanged. No product edits, acceptance or directory closure.

Replay from a writable copy with supported Node >=22.19.0 and its npm:

```sh
npm ci --prefix runtime --ignore-scripts --no-audit --no-fund
NODE_BIN=/path/to/node sh run.sh
```

run.sh verifies source hashes, isolates temp/config/output directories and writes fresh replay.* artifacts without overwriting the recorded files. Tests execute controlled local shell processes and clean their recorded descendant PIDs. Timing/PIDs/UUID/temp paths differ on replay. The original repository is not needed. Final recorded run used the bundled Node binary, working directory .ops/stage1-worker/source025, TMPDIR=$PWD/tmp, PI_CODING_AGENT_DIR=$PWD/agent-home, PI_PACKAGE_DIR=$PWD/source/packages/coding-agent, and `node runtime/node_modules/vitest/vitest.mjs run --config vitest.config.mjs`; exit0. Shell syntax and all artifact/fixture hashes were checked after packaging, without claiming another test run.
