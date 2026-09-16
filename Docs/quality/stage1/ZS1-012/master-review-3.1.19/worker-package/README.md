# ZS1-012 — unchanged harness compaction source execution

Worker candidate [_]; controller acceptance pending. Authority 3.1.15 / e1753520cd7ab1e8429b355eaaa9c431b2d1ee0910cb5f8c188bbd3fbac15e40. No acceptance-count change (8 accepted / 100 open).

Subject is packages/agent/src/harness/compaction/compaction.ts, NOT the separate coding-agent implementation. Frozen upstream HEAD bbb61e34aaf231639fdaaad1adbd757947034eac; 865 lines, 27,410 bytes, SHA-256 6e7aec0d27cb566f8f85f7dd13850eda98f5ddf7e78f9a919fb3dd03c3ea3a8a. Existing complete controller report/source identity and separate 104 product evidence were read first; prior-evidence.json hashes those records, and prior-controller-report.md preserves its earlier text. learn-report.md supplies the complete independent reread and new executable evidence, without replaying old product test counts.

## Actual result

Node 24.19.0 Darwin arm64, genuine pinned Vitest 4.1.9. Three test files, 37 passed / 0 failed / 0 skipped / 0 todo. Twenty-two cases execute the unmodified original compaction.test.ts (827 lines); two use the unmodified sibling branch-summarization.test.ts (71 lines); 13 supplemental scenarios cover gaps. A 250-budget loop inside one scenario is not 250 separate tests. Original sibling cases prove collection using map-backed readers, not persistence or another accepted file.

The 27 byte-identical source files form an isolated runtime closure. Package-export bridges forward to actual upstream pi-ai contentText/retry/uuid/Models/fauxProvider, chord/context and telemetry. Real registry/lazy stream/auth-resolution/in-memory stores/event stream/usage functions remain unchanged. Type-only imports are erased. No Vitest shim, assertion-count stub, fake timer, mocked random generator or replaced compaction implementation exists. Only Vitest and its real transitive dependencies are installed externally, fixed by package-lock.json and installed-file hashes. The original monorepo test config is not used; our isolated config selects exactly the three test files.

## Fixture boundaries and findings

Original fauxProvider/fauxAssistantMessage are intentionally synthetic model responses; one original usage test directly overrides Models.completeSimple. Supplemental SummaryRequest queues/Models.completeSimple supply scripted results for prompt, usage, error, cancellation and retry checks. Branch/session readers use in-memory maps; a telemetry object tests identity propagation only. Actual AbortSignal/context, retry scheduling, serial request order and source projection/compaction execute normally. No real model, provider network, billable token/cost evidence, OS tool execution, session writes or independent-process restart is claimed. Older Zenpi 103/104 HTTP/process evidence is separate and was not rerun or changed.

Evidence demonstrates normal complete-turn and split-turn cuts, well-formed parallel call/result pairing for a bounded fixture, prior retainedTail reconstruction, exact file tags, previous-summary/pending-work prompt content, sequential history/prefix requests, summed usage, output options, first/second-request failures, actual signal propagation, retry and abort during backoff, and adjacent branch-summary boundaries.

Observed source limitations are explicit rather than patched: orphan results/unresolved calls are accepted; file lists reflect tool-call intent even for failed/unconfirmed writes; branch-summary non-message text is not counted by the cut loop; nested preparation objects are shared; response length/empty/toolCall outputs are accepted; request exceptions escape; an ignored abort can still succeed. Tool results are truncated at 2,000 characters and lose call IDs/error status in serialization. Summary fidelity is not validated. A prefix-only run with empty history drops previousSummary and uses 'No prior history.', so an old-only pending item does not reach that request. Sibling branch summarization drops all tool results, may include file facts for over-budget text, can exceed budget for summary messages and fixes output maxTokens at 2048. These facts do not amend Zenpi's stronger contract.

case-review.json maps every passed case to its actual owner and fixture boundary, and notes weak original assertions. probe-observations.json captures prompts, responses, cuts, options and callback sequences. test-results.json and stdout/stderr are from the final actual run. Initial dependency-symlink setup failed before tests; its genuine stderr is preserved under initial-setup-failure and is not counted as a source/test failure. No original source or test was edited to make execution pass.

## Replay and integrity

From a writable copy, use Node >=22.19.0 and npm from that installation (recorded Node is 24.19.0):

```sh
npm ci --prefix runtime --ignore-scripts --no-audit --no-fund
NODE_BIN=/path/to/node sh run.sh
```

run.sh verifies copied source hashes, creates an isolated replay.* output directory and runs real Vitest against unchanged originals and the supplemental probe. It leaves the recorded result files intact. UUIDs/timestamps and timings vary; assertions and fixture observations determine success. The readonly upstream repository is not required for replay.

source-inventory.json records originals before execution; source-verification.json confirms matching source/copy hashes and upstream HEAD after the final run. installed-dependency-files.json records the actual dependency installation; no node_modules is shipped. manifest.json hashes all frozen regular files except itself. Shell syntax, artifact hashes and prior 010/011/014/018 frozen artifacts were verified after packaging; no post-packaging repeat test count is claimed. Product sources and prior frozen evidence packages were not modified. Controller source acceptance, product validation and directory closure remain separate.
