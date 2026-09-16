# 2026-09-11 主控复验：129完整删除恢复与可靠性修复

本轮同一生产release `1de0908bb428de267fe5982cadeaf60cc93dd02e78d1b43a5777323fbe518bdf`（5,980,320 bytes）实际通过15组PTY/JSONL、192项检查；另有111清理5项和116外部证据8项通过。59份源码/辅助输入、binary、各结果及保留fixture hash独立核对。新增129的29项覆盖逻辑行Ctrl-U、canonical全文Ctrl-Y、项目内buffer、预算拒绝后重试、重启、焦点和后台回执；source inspector增加Ctrl-Y不编辑/复制/提交检查，原队列身份与BentoBox回归保留。

工具写入4项旧失败复现后五套136测试通过；115小型native夹具只改测试，原超时/取消/FD/进程断言保持，完整library72通过；129九套189通过后，再合入session三类修复并跑七套220通过（当前library79通过、5个显式helper忽略）。这些不同构建的套数分开记录，不累计或冒充一个全量测试结果。严格all-targets/all-features Clippy与格式检查通过。15直接依赖、8性能门禁全部通过；三次cold start最大486.393ms、峰值RSS5,160,960 bytes；实际测量不是p95。先前115失败及所有修复前失败完整保留。

当前权威3.1.18增加未完成130外部编辑器，正式接受仍10/110，100项未完成；本轮没有提升产品checkbox。当前release不含130，Windows/未执行Linux结果不作通过声明。完整逐文件/逐目录和其余通用UX仍需独立完成。完整证据索引：`.ops/stage1_execution/kill-yank-integration/master-verification.json`。

---

> Latest controller verification (3.1.17): release `1e3d8b92a6dd1d3b73dedb79c7638ea6d0a2b2dc840afcbfbbaec60a0ceec544` (5,963,760 bytes) passed14 real production cases /162 checks, plus5 cleanup recovery and8 external-evidence checks on the same binary. This includes122 identical scheduled input identity,101 steer-shell validation and111 recoverable cleanup. Seven owning suites /137 tests, strict all-target/all-feature Clippy and all8 budget gates passed. Evidence: `.ops/stage1_execution/queue-identity-integration/master-verification.json`. A separate extension pipe fixture failed before ready twice in library validation and is under investigation; this is not a full-library or ZS1-128 acceptance. Registered129 kill/yank and the remaining generic UX/coverage obligations remain open. Earlier release records below are historical.

> Latest targeted main verification: binary `8158228c154ccc17b86b837c160ef0f13c0e8e1d945d5b887ca250a20eaec1a4` adds the101 pre-dispatch failure repair to115 hooks and pipe cancellation. The same binary passed14 shared-project and15 composer checks, two real extension JSONL tests and three Chat JSON tests;192 joint Rust tests and all8 runtime budgets passed. Receipt: `.ops/stage1_execution/input-preflight-integration/master-verification.json`. The earlier115 binary `2ef09602ec226f511c706ecee2defce5b313ab32435a1cfd5e58fc747fc5b877` passed the full12-case138-check matrix (`.ops/stage1_execution/extension-hooks-integration/master-verification.json`). These are incremental results; all remaining UX and independent acceptance gates stay open.

> Latest main regression: binary `43c2e98c626d366134bc6c7cb316ae624cbfb75bdfc6ac01ffe4c6930915e850` includes108strictJSON and123long-path provenance. The current12-case production matrix passed138checks, including20command-menu checks. All8budgets and149joint Rust tests passed; the same binary passed the three real JSON host tests. Receipt: `.ops/stage1_execution/provenance-ui-integration/master-verification.json`. This records the tested incremental implementation; large paste, later-catalogue full-source inspection, kill/yank, remaining generic UX and all required independent acceptance stay open. Earlier results below are historical.

> Main integration record: the actual12-case run at `.ops/stage1_execution/ux128-harness-integration/production-attempt3/manifest.json` passed135checks on binary02e1e70b589e1b5f1c9f6a40b278f749a72918372ce6914ae9915f355deb750d, with all binary/helper/result hashes independently checked. This is a bounded existing-matrix result, not ZS1-128 acceptance: the long-path provenance failure, registered large-paste work, kill/yank and other generic gaps remain open. Subsequent source changes require matching production evidence. Historical worker results below retain their original scope.

# ZS1-128 integrated UX acceptance

Status: [_] work in progress. No master acceptance is claimed. Authority: Blueprint 3.1.16, requirement digest `c0262492bc6e3b4d5f56b35bedc7c1d1658854df11c4a975eb612a514cceef8f`.

The final run must bind one immutable production binary that includes ZS1-117, 121–126 and all shared owner dependencies. B's `c418de4e0c125e53a6f9cb3af4d983d1380c9c511e6e565fb3999e6fd5d23586` binary is a harness development baseline. Historical per-feature successes do not satisfy this integrated gate.

## Reproducible entry point

```
python3 tools/stage1_host_smoke.py --binary /absolute/path/to/fixed-zenpi --report-dir /absolute/path/to/new-result-directory
cargo +stable-aarch64-apple-darwin test --locked --test tui_bentobox --test layout_persistence
```

The report directory must not exist. Each case starts a separate Python harness process, launches the specified actual Zenpi executable, records its SHA256 and persists current result/logs. The runner fails on nonzero exit, missing success state, mismatched binary hash or timeout. It does not build or silently substitute a binary. Fixtures preserve HOME/CODEX_HOME, isolate configuration with ZENPI_HOME and use explicit private fake auth. HTTP servers bind loopback and record real production requests. Unit tests remain separate from production evidence. ZS1-117 must add its core boundary cases and release-feature/budget verification; UX-only passing results cannot certify that gate.

| Case | Actual entry and observable result | Negative / cancellation / restart |
|---|---|---|
| projects | Top-row mouse +, directory picker, actual shell writes under two equal-basename directories; real model config and session cwd; middle-click and empty-prompt Ctrl-W close only the tab projection and reopen the same cwd/journal | Missing path, Esc, aliases, switch while shell runs, late output isolation, close-with-running-owner refusal, journal-preserving reopen, independent TUI restart |
| shared-projects | Three actual JSONL processes and one TUI share canonical IDs, actual Agent model/config/tool cwd and selection checkpoint; four real HTTP requests | Request replay/conflict, queued-owner close, cross-project cancel/approval, invalid config, path escape, cancelled open, real checkpoint-directory permission denial, actual 64-project ceiling, independent JSONL→TUI→JSONL restore |
| composer | Grapheme editing, multiline/paste/history, owner input tickets and persisted future-job edit/cancel/retry; real HTTP payload inspection | Paste cannot auto-submit, busy follow-up/steer ordering, invalid edit, cancellation, restart requires explicit retry |
| ordinary-paste | Unbracketed ordinary CR key stream and ASCII/Unicode prompt latency | No accidental model/shell/exit, idle-flush Enter window, cancel/draft restart, one explicit complete submission |
| project-checkpoints | Gated provider text/reasoning after keyboard checkpoint settles | Active and background intermediate/final bytes saved without new keys, visible draft/layout preserved, independent restart |
| tui-session-resume | Real slash open/resume-last and session-search-list Enter using the shared owner commit hook | chmod0500 and actual second-headless-writer failure keep owner/draft/layout/checkpoint; same-entry retry and independent restart; canonical Session projection layout preserved |
| commands | Actual resource/menu/completion, workspace attachment and model/reasoning owner | Invalid paths/commands/capabilities preserve draft; reload failure preserves snapshot; independent persisted-selection restart |
| approval | Real tool call waits for project/request/turn/call-bound modal decision | Deny default, explicit Enter, unfocused paste, picker and draft restoration, cross-project rejection, multi-request FIFO, Ctrl-C, remembered human policy restart |
| transcript | Actual Chat SSE reasoning/text, tool/error blocks, exact bounded OSC52 clipboard payload | Scroll anchor during stream, follow-latest, long failing shell, cancellation, normalized provider failure, narrow navigation, terminal restore |
| compact | Real semantic summary HTTP runs off UI thread; durable checkpoint feeds next request | Edit draft while summary waits, cancel keeps prior checkpoint, separate TUI process restores summary without reissue |
| local-controls | Actual TUI/JSONL /diff and /review, durable persona and blueprint plan, clear-view semantics, session fork and agent registry | Invalid persona/plan keep prior durable state; independent JSONL restores TUI persona/plan; no provider operation |
| bentobox | Real Tab/Ctrl-arrow focus, Ctrl-Shift-arrow split, collapse/expand, terminal sizes 1×1 through 180×45, valid + folder flow, per-project layout | Invalid layout/unavailable pane, picker cancel, draft preservation, independent restart and a second restart after new layout edits |

## Current development findings

1. The first resize test did not wait for complete queued terminal redraws before clicking +. The harness now drains for a bounded interval after each actual ioctl; raw attempt 1 remains in `.ops/ux128-bentobox-pre1`.
2. The next test incorrectly treated global `layout.json` preferences as the per-project checkpoint. Runtime source and the actual screen showed the project layout restored correctly. Assertions now inspect the selected project in `project-tabs.json`; attempt 2 remains in `.ops/ux128-bentobox-pre2`.
3. Attempt 3 exposed a product defect: after restore, active layout edits rendered correctly but `project_tabs_json()` serialized the stale cached layout. The new Rust test failed with actual unequal ratios (35/40/25 versus 30/45/25). A separate ZS1-121 follow-up now prioritizes the current active layout and retains cached layouts only for inactive projects. The prior 126 frozen package is unchanged. The repaired binary passed the initial complete BentoBox PTY; the aggregate run additionally checks a second restart after editing the restored layout.
4. In the single-immediate-child fixture, click +, Right, Enter reached the real selected project in three gestures. The measured duration belongs to that actual run, not a general latency or global shortest-path guarantee. Arbitrary absolute-path entry has a different path through the picker.

## Fresh preintegration result

`evidence/preintegration-production/manifest.json` in `.ops/project-persistence-followup-ready` binds the successful eight-case run: **100 checks**, **45 actual HTTP requests**, binary `af6b8ab2f680048b6183e13a834cd4035c0758ed03ef300f9e7b2cfb05011140` (5,330,576 bytes), harness `e347ec16ebac107e46a98e115f1444d84d3baf9310b2a6848da0d8feef379b09`. The BentoBox case has 11 checks including actual mouse row drag, keyboard column resize and three independent TUI processes; its three-gesture single-child project opening took 78 ms. These are observed fixture results, not portable latency guarantees.

The frozen follow-up manifest is `2d1a6665d6f236481aa599172caabd54503a8ad01d9385cfea34f6adec6bec59`. It includes exact-replay baselines, the two production persistence fixes, one 25-line regression, 52 passing Rust tests in five targets and strict library Clippy. The successful runner stops on failures and uses no old result substitution. Earlier aggregate pre5 failed at the actual asynchronous reload draft check; that failure is retained. This remains preintegration evidence, not final ZS1-128 acceptance.

The subsequent `local-controls` case passed seven additional actual entry checks against the same binary (receipt `.ops/ux128-local-controls-pre4/manifest.json`, harness `682840c35d3bc3f88efcf6fa82c3bc60850f5dd6459263442a6727dd9d630174`). Its initial fixture expected an owner validation string but the command parser correctly rejected the invalid persona earlier; the final check asserts actual rejection and unchanged journal. The busy-diff probe verifies a deliberate availability difference: Zenpi rejects while a real shell runs and preserves the command draft; after the shell exits, Tab completes the file selection and Enter executes against the same selected project. Codex advertises diff during work. Probe iterations initially used an incorrect terminal status label and omitted the completion confirmation; no product code was changed for those fixture corrections. These seven checks supplement, rather than rewrite, the frozen 100-check receipt.

## Remaining gates

- Finish fresh all-case run against the main task's integrated fixed production binary; preserve all case hashes, source hashes, durations, raw outputs and observed operation counts.
- Main task verifies ZS1-117 core cases, actual production feature set, unchanged 8 MiB binary ceiling, dependency count, startup/RSS/transport/output budgets and platform boundaries.
- Main task independently accepts Codex 300–309 file reports and bottom-up 354→350 directory reports. Directory candidate analysis is complete; dependency acceptance is still pending.
- Review every row in `ux-reference.md`, including unsupported and intentionally different commands. A mapping is not an implementation or a test pass; source tests were read, not executed as Codex tests.


## Followup runner integration input

The runner now includes three explicit helper modes above; helper paths and argument lists are recorded in each receipt. They reuse the frozen122/126 product fixtures and add no production owner or scheduler. B's final followup binary `b09552815a37a31c5d30a397fbeb266b3532e13b22326add6064e7b1ae16380c` (5,372,160bytes) is still a worker validation binary. The main task must merge its117 harness implementation and its newer106/111/122 production changes before the final all-case run. Root123 command-menu fixtures must use actual separate typing or deliberate Enter when simulating those interactions; raw rapid key bytes remain the paste negative case.

The twelve-case worker runner would cover135fixture checks with the current helper counts; that number is a development expectation, not a completed aggregate result. Final acceptance must use actual per-case receipts and the integrated script's inventory. Source-file300–309 and directory354→350 acceptance, full generic-gap disposition, production budgets and feature configuration remain separate gates.


主控 116 集成回归：生产二进制 `9b49a3f0688aebb2d71cc3e54be60d8cf05c73950cd4a6d65c86392d10158c95`（5,943,312 bytes）在同一冻结构建上通过 12 组共 138 项既有入口检查，另通过 8 项真实外部接受/取消/重启检查。310 个不同顶层 Rust 测试通过，嵌套 helper 不重复计数；原 Python fixture 初始化失败日志保留，测试 fixture 改用原生实际子进程后原 17 项断言通过，产品时间预算未改。严格 Clippy、格式和原有 8 项预算通过。证据：`.ops/stage1_execution/external-evidence-integration/master-verification.json`。这份记录不替代逐文件、逐目录或产品条目的最终接受；后续 122 粘贴改动需要新的构建与入口证据。


主控 122 大段粘贴与 123 完整来源查看集成回归：固定生产二进制 `f3b204a41cab016b5306b94426f3f9e1ac277c9e1bbb4bcfc368bf964ce4be42`（5,963,104 bytes）通过 13 组 155 项入口检查及 8 项外部接受检查；7 套 158 个不同顶层 Rust 测试与严格 Clippy/8 预算门禁通过。大段粘贴14项与来源菜单23项均经过真实PTY。证据 `.ops/stage1_execution/source-inspector-integration/master-verification.json`。同一生产构建另实际暴露 accepted queued shell 原文恢复缺陷，负例 `.ops/stage1_execution/queued-shell-paste-integration/before-production.run.json` 保留，修复尚未包含在此构建；不得据此宣称122全项完成。


## ZS1-132 主控增量验证（3.1.20）

`/new` 候选经修复并发冲突后的旧 journal 写入恢复，以及缺少游标时的分页消息保留后合入。121 项直接 Rust 回归、3 项审计恢复单测、严格 Clippy/格式检查通过；固定 debug CLI 的 8 组入口与 3 组进程恢复通过，共 4 TUI、9 JSONL 进程及 10 次 HTTP。实际失败日志、命令、源码补丁及最终 fixture 见 [主控复核记录](ZS1-132/master-new-session-3.1.20/review.md)。这是列明范围的增量证据，release 预算、其它 busy/授权组合及全项验收仍未完成。


## ZS1-132 跨项目 new 与授权边界增量（3.1.20）

后台项目运行时，当前空闲项目可独立 `/new`；保留 BentoBox、分页和 cwd。121 项 Rust 回归、严格 Clippy/格式及20组真实入口通过（7 TUI、11 JSONL、31 HTTP）。同项目审批/排队/interrupted 任务仍阻止 new，授权记忆与进程策略作用域已执行列明检查。见 [主控复核记录](ZS1-132/master-new-session-ownership-3.1.20/review.md)。configured Deny/绑定 worker、grace 超时与最终 release 门禁未闭合；整项保持 `[ ]`。


## ZS1-132 最新 release 与权限作用域补验（3.1.20）

固定 release `180ec9f70d5bedda5ca3b417a88bf313fefa069df8cf3c4488c7669133e149bf` 新执行 `/new` 20组、项目picker12项、BentoBox11项均通过。新增2项共享JSONL owner权限回归覆盖7场景，连同直接回归共21项通过；configured Deny、绑定worker规则/撤销/耗尽额度在新会话保留。后者为确定性provider的库集成证明，不冒称独立release worker验证。原冷启动1044.82ms超过1000ms、grace时序及其它完整门禁仍未闭合。见 [本次独立证据](ZS1-132/master-release-policy-3.1.20/review.md)，整项保持 `[ ]`。


## ZS1-124 后台审批发现提示（3.1.20）

现有分页增加待审批数量，footer提示来源项目及切换/Alt-A审核，后台通知不改变当前草稿、布局或授权。固定旧release已观察无提示反例；最终debug实际4项新检查、14项既有审批、11项BentoBox检查及52项Rust通过。见 [主控实现及证据](ZS1-124/master-background-attention-3.1.20/review.md)。新release预算、完整审批/UX与源目录验收仍独立，整项 `[ ]`。
