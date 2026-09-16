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

