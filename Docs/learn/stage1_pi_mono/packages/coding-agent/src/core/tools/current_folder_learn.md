# ZS1-053 — independent directory acceptance under 3.1.19

The controller accepts only the frozen tools directory subset after independently checking all four child receipts: output-accumulator024, bash025, grep026 and find027 are now [x]. There is no in-scope child directory. Actual 17 immediate children were compared with the upstream directory listing; every immediate file hash and directory classification matched the frozen inventory. The other tools and renderers directory remain context-only. Parent056 and product111/112 remain unfinished. Worker statements that025/027 are pending are historical;027 was already accepted in the package's own ledger and025 was independently accepted before this closure.

The controller read the complete directory report, five source scenarios, producer, independent artifact-reader process, loader/renderer substitution, native setup, replay script and Vitest configuration, and the complete context-only index.ts factory dispatch. It verified all84 package files,23 unchanged source/context copies,18 decompressed original fixtures, fixed fd10.5.0 and host rg15.2.0 binary hashes. Package-lock bytes exactly match the already verified025 runtime;2099 dependency runtime hashes were checked again before private copying, excluding only the generated .vite result cache. An initial controller setup script incorrectly expected a path field on directory entries and stopped before copy/execution; it was corrected to directory/name without changing the inventory or evidence.

The new controller run executed all five scenarios with actual Bash, raw filesystem output, native fd and native rg: five passed, zero failed or pending. The pipeline's raw spill hash was independently checked after execution and its full bytes are preserved. Bash writes2501 lines into OutputAccumulator and returns a clipped tail plus a path; the caller explicitly hands that path to find then grep. A fresh independent Node process runs the original grep wrapper against the same file. These are explicit caller operations, not automatic Bash callbacks, model-provider execution, journal recovery or private-artifact authorization. Original source rendering alone is replaced with empty renderer factories; UI behavior is not accepted.

The same actual suite verifies hidden/ignore/glob differences, regex versus literal matching, CRLF context formatting, unlike zero-limit semantics and search truncation metadata, pre-abort before Bash side effects, actual rg syntax errors and missing paths. A gated readFile operation after real rg closure demonstrates ignored late cancellation and changed-file context rereading; only that asynchronous gate is injected, while search/stat/readFile execute real implementations. This finding does not establish a stable source content snapshot or universal cancellation. The worker's initial incorrect positive-glob expectation remains preserved alongside its corrected behavioral observation.

Ownership is separate across siblings: Bash owns command launch and callbacks; OutputAccumulator owns raw spill and decoded tail; find and grep independently resolve native executables and search roots. Promise resolution in the exercised Bash path follows output close, but a returned path does not prove fsync, private permissions, quota, immutable completion metadata or arbitrary restart safety. Search clipping has no Bash-style full-output artifact. Existing024 nine groups,025 fifteen cases and027 twenty-five cases are reused as child evidence and are not rerun or added to the five directory cases. The prior026 acceptance was static source understanding; new actual grep execution is directory evidence, without retroactively changing that receipt.

Current target mapping was checked in RunCommandTool/CommandOutputCapture and SearchBackend search/find: target shell captures stdout/stderr separately with finite timeout and supervised ownership; private output uses session/call identities and bounded authorized reading rather than handing arbitrary paths to search. Target advanced search admits bounded workspace files, reads match/context from the same rg JSON result, checks result membership/private inodes and rejects find limit0. It does not copy the source's late context reread or zero-limit behavior. These inspected mapping ranges and retained snapshots do not accept entire target files or prove every product cancellation, durability or policy obligation.

All accepted child scope/hash/receipt bindings remain unchanged across the worker's3.1.18 and current3.1.19. This directory report covers only the explicit four-file subset and its calls/data/errors/cancellation/output handoff. No sibling source, parent directory, complete tool registry, whole pi-mono repository, platform or product is additionally accepted.

## Historical worker candidate, preserved

# ZS1-053 — packages/coding-agent/src/core/tools

Provisional [_], authority 3.1.18 / cb96a3db9b57ea0d9d3ad43f0be954129aabf69bc5d438210a14b674a4a8af85. The frozen direct files are output-accumulator024, bash025, grep026 and find027, with no in-scope child directory.024/026 are accepted;025/027 remain pending. Actual siblings and renderers/ are enumerated as context-only in directory-scope.json. No extra file is accepted by this synthesis.

## Preserve each file's evidence meaning

| File | Prior source evidence reused | New directory execution |
| --- | --- | --- |
|024 output-accumulator | Controller executed9 behavioral groups, including raw spill and file-creation failure | Actual accumulator reached through Bash; exact spill bytes checked |
|025 bash |15 tests:12 supplemental real-process scenarios plus3 original cases with explicitly injected/fake process boundaries | Real shell output and spill pipeline; pre-abort rejects before a command side effect |
|026 grep | Accepted whole-file understanding; controller explicitly stated source/tests were not executed | Native rg through the unchanged original wrapper in all five directory scenarios |
|027 find |25 tests with native fd, custom operations and controlled-executable cases distinguished; first17 are included in25 | Native fd locates the spill and searches fixtures; zero-limit and pre-abort behavior checked |

Prior results are referenced without rerunning or adding them to the five new tests. Controller024/026 review, acceptance and3.1.18 rebind records are preserved. New grep execution supplements directory evidence; it does not retroactively relabel the historical026 receipt.

## Calls and ownership

The context-only224-line index routes names/options/cwd to individual definition or AgentTool factories. Coding defaults include bash; read-only defaults include grep/find. The whole barrel is not initialized here. Actual tool wrappers forward parameters, signal, updates and optional context to each definition.

Bash owns shell invocation and feeds stdout/stderr buffers into OutputAccumulator. The accumulator tracks decoded display tail and raw spill; Bash awaits its own finalization before returning in the exercised path. find and grep share tools-manager resolution and path helpers, and use native fd/rg plus shared truncation helpers. They are sibling tools, not automatic callbacks from Bash: the caller explicitly passes its returned fullOutputPath to later tools.

The five new tests use unchanged source and real pinned dependencies, with renderer-only imports replaced by empty renderers. Bash's throttle value remains100ms. UI rendering, AgentSession integration and the full tool registry are not tested.

## Actual behavior

D053-01 executes a real shell producing2501 lines. Bash returns a truncated tail without DIRECTORY_NEEDLE plus a raw spill containing exact expected bytes. Native fd locates the spill outside the tool cwd via an explicit path. Native rg reads the omitted first-line marker. A fresh Node process runs the original grep wrapper against the same file and gets the same result; file bytes remain unchanged. This proves this captured file can be consumed later, not private artifact authorization, durable catalog recovery or arbitrary crash completeness.

D053-02 executes native rg regex/literal, hidden-file and context searches plus fd discovery. Without an explicit glob, the fixture's gitignore excludes ignored.txt; a positive *.txt glob makes rg include it. Literal alpha[0-9] has no matches while regex does. Context output normalizes CRLF and reports the requested neighboring lines. Native fd's same-directory result remains distinct.

The initial run expected positive glob to preserve gitignore exclusion and failed one assertion. Exact source/logs remain under initial-observation. The final test explicitly checks both default exclusion and positive-glob inclusion. No source was edited; the observed override is documented.

D053-03 shows there is no shared zero-limit contract: grep clamps0 to1 and reports a match limit, while find accepts0 and returns both fixture files with a0 result-limit notice. Long grep lines are truncated. Neither search result supplies Bash-style fullOutputPath.

D053-04 pre-aborts all three wrappers; the Bash command's intended marker file is absent. Actual rg rejects an invalid regex, and missing search paths reject. This does not stand in for live-child cancellation/reaping.

D053-05 runs native rg, then pauses only its injected asynchronous context-file read. After the rg child has closed, cancellation is requested and the file is changed. Formatting still succeeds with changed text at the previously matched line number. Thus late abort is ignored in that phase and context formatting is not a stable content snapshot. Default isDirectory and actual readFile remain real; the explicit gate is recorded as fixture behavior.

Five tests passed, zero failed/skipped/todo. Raw child outputs, exact file fixtures compressed by content hash, binary hashes/version strings, environment and source verification are frozen. The native tools are fd10.5.0 and rg15.2.0. fd is restored from027's frozen binary; rg is the recorded host binary copied into an isolated tool directory. Execution is offline. Replay requires that exact rg binary; setup-native verifies its hash. No provider model is involved.

## Limits, target mapping and closure

The caller's path handoff is not a workspace capability. Source search accepts explicit roots outside cwd. A spill path lacks source-level session/operation ownership, quota/hash/completion metadata and an access-control catalog. Search clipping has no spill equivalent.025 separately documents descendant lifetime/idle-drain limits;024 finish alone is not a close/sync proof. Those historical limits are not removed by one successful pipeline.

Zenpi tools/tool_output/search plus core approval, budget and lifecycle owners must preserve raw-output identity, enforce explicit path authorization, distinguish truncation from complete results and handle cancellation throughout formatting. Current Rust fixes and111/112 product validation are separate and were not modified or rerun here.

Directory053 depends on024/025/026/027 and feeds parent056 and downstream product owners. Both pending source dependencies prevent acceptance. No context-only renderer, other tool, parent directory, product task or ledger item is promoted by this package.

