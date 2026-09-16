# ZS1-027 find.ts — frozen worker evidence

Status: `[_]` master review pending. Authority 3.1.16, digest `c0262492bc6e3b4d5f56b35bedc7c1d1658854df11c4a975eb612a514cceef8f`. This package only documents source behavior. No product code, authority ledger or acceptance count changes.

Read the pre-existing report first (`prior-worker-report.md`), then all 318 lines / 11,178 bytes of upstream `packages/coding-agent/src/core/tools/find.ts`, HEAD `bbb61e34aaf231639fdaaad1adbd757947034eac`, SHA256 `b06bcae6821a0e9fda1b63be613a7ce28eb0e66e1d01d16564e59e83f9ce10ea`. Fifteen source/context/test copies are byte-for-byte unchanged and checked against the read-only upstream checkout. See `learn-report.md`, inventories and `case-review.json` for boundaries.

## Executed evidence

The final run has 25 passing tests, zero failed/skipped/todo, across four files. The first original-only run has 17 passing tests; these are included in the final 25 and must not be added again. Original tests 3302 (4 cases) and 3303 (2) run native fd with actual temporary files; 6104 has 10 path-function cases and one injected glob case. Its Windows path calculations are not Windows execution. Eight supplemental cases cover four native-fd filesystem scenarios, two custom-operation scenarios, one controlled executable driving actual spawn/readline/error/abort logic, and one real EACCES/offline-missing-tool scenario. `test-results.json` and logs are the final original execution, not reconstructed output. `probe-observations.json` contains actual observed values and PIDs. The controlled child was absent after the cancellation scenario.

Actual execution used Node 24.19.0 (SHA256 `27db838bb204ef7c21df2931f5656e4c8fb32e6e947f363a402b49714d32b5b1`), Vitest 4.1.9, TypeBox 1.3.27 and cross-spawn 7.0.6. Only the UI renderer import is replaced by an empty `findRenderers` object. The original renderer is context-only; no UI/AgentSession/full-monorepo/product or native Linux/Windows claim is made. The find implementation, tools-manager, downloader, path handling, spawn and truncation code are unchanged.

## Native fd provenance and fixed replay

The original unchanged `ensureTool("fd")` was executed with an isolated agent directory and performed a real download/extraction. `fd-provision.json` and provisioning stdout/stderr preserve its receipt. This Darwin arm64 bootstrap selected fd 10.5.0, 2,970,480 bytes, SHA256 `cf3bde435da174f41cf9589a2efeaf03804df7c250fc15e8b8a1e9bfc66ebc9a`. The archive URL follows the executed installer and observed version: `https://github.com/sharkdp/fd/releases/download/v10.5.0/fd-v10.5.0-aarch64-apple-darwin.tar.gz`. The installer deleted the archive; no archive hash or publisher-signature verification is claimed.

`runtime/native/fd-darwin-arm64.gz` losslessly freezes the actual executed binary. `fd-licenses/` contains upstream v10.5.0 LICENSE-MIT and LICENSE-APACHE, obtained from the matching GitHub tag. All tests ran with `PI_OFFLINE=1`; replay restores and verifies these same binary bytes instead of querying latest. Do not rerun `provision-fd.mjs` to reproduce this receipt. During two fault cases only the isolated fd path is temporarily replaced, then restored in finally; the restored binary was verified again. This is explicitly dependency-level fault injection, not native fd search output.

## Reproduction

Copy this package to a scratch directory; keep the frozen package unchanged. On Darwin arm64 with Node >=22.19.0 (prefer the recorded Node 24.19.0), put that Node first on PATH and run:

```sh
npm ci --prefix runtime --ignore-scripts --no-audit --no-fund
NODE_BIN=/absolute/path/to/node sh run.sh
```

The npm installation may need network/cache, while tool execution is offline. `run.sh` checks source hashes, creates a fresh replay directory, verifies/decompresses the frozen fd, executes tests with separate logs/results/observations, and checks source and fd restoration. The supplemental filesystem tree is created under `/tmp/zenpi-source027-*`, outside the checkout so ancestor Git metadata cannot change ignore semantics. Original regression fixtures are removed by their original teardown; supplemental trees remain for inspection and their exact path is recorded. Test processes have bounded cleanup/watchdogs. The initial qualified run command was the same Vitest invocation with stage-local TMPDIR/agent-home and PI_OFFLINE=1; the packaged runner has syntax/source/binary integrity verification, not a second claimed test run.

`installed-dependency-files.json` records installed file bytes and symlink targets; runtime package/lock files pin the installation. `fixture-inventory.json` records 694 actual surviving filesystem entries, including directories and a directory symlink without following it. Each regular file's bytes are frozen in a content-addressed gzip under `fixtures/`; identical empty files share one blob. Do not recreate absolute symlink targets from this historical inventory. The test itself builds fresh owned fixtures. Raw fixture paths, PIDs and output text refer to the recorded run.

## Observed limitations

Native fd shows explicit sibling roots allowed, node_modules visible without an ignore rule, trailing filename space lost, newline filenames split, exact-limit warnings without proof of omission, zero limit not a positive cap, and byte truncation without a spill file. Custom glob returns may exceed limit or escape the root, and cancellation does not stop the underlying operation. Controlled executable cases show nonzero exit with partial/blank stdout accepted as success and cancellation rejection before PID exit. See the report for precise assertions and static-only resource/lifecycle boundaries. These observations do not authorize broader product behavior or accept any context file/directory.

025 was frozen first and remains unchanged. The 8 accepted / 100 open ledger, 108 IDs / 56 files / 25 directories remain unchanged; only the master can accept this candidate.
