> Latest main regression: binary `43c2e98c626d366134bc6c7cb316ae624cbfb75bdfc6ac01ffe4c6930915e850` includes108strictJSON and123long-path provenance. The current12-case production matrix passed138checks, including20command-menu checks. All8budgets and149joint Rust tests passed; the same binary passed the three real JSON host tests. Receipt: `.ops/stage1_execution/provenance-ui-integration/master-verification.json`. This records the tested incremental implementation; large paste, later-catalogue full-source inspection, kill/yank, remaining generic UX and all required independent acceptance stay open. Earlier results below are historical.

> Main review, authority3.1.16: this file preserves historical worker receipts and the current gap inventory; it is not product acceptance. Both complete documents were read before import. Main independently verified the12-case/135-check aggregate for binary02e1e70b589e1b5f1c9f6a40b278f749a72918372ce6914ae9915f355deb750d; later code changes need their own matching evidence. Ordinary-key paste is implemented; large-paste fold/expand is registered work in progress. Kill-buffer/Ctrl-Y, long-path resource provenance and all remaining generic rows stay open. Per-file and per-directory master acceptance remains separate.

# UX implementation and reference matrix

Status: [_] worker candidates; master acceptance owns all completion marks.
Worker: `/Users/wangweiyang/.codex/worktrees/2267/zenpi`. Initial frozen HEAD: `6f252a20c628e9b1ede14e2887acc04657c71d7c`, inherited dirty preserved.

## ZS1-120 / ZS1-121 project candidate

The topmost project strip now has a fixed visible +. Mouse + or Ctrl-T opens the terminal-native picker. Type/paste a path, Tab completes or enters it, arrows browse, Enter/Open confirms and Esc/Cancel leaves the active project unchanged. Paths normalize against an explicit browsing root; canonical directory IDs distinguish equal basenames and deduplicate aliases. The strip uses terminal cell widths, clips names, keeps active selection reachable at narrow widths and disambiguates duplicate titles with parent and short ID. The working directory is visible in the status header.

Project tabs also support the short close paths expected from a terminal workspace: middle-click a tab, or press Ctrl-W with an empty prompt, to close only that tab projection. Ctrl-W keeps its existing word-delete behavior while a draft is non-empty; middle-click also refuses when the target tab is inactive but retains a non-empty per-project draft. The final tab cannot be closed. Closing refuses when that project owns running or queued work, and never deletes its journal, so the same cwd can be reopened through the + picker or session browser.

ProjectRuntimeHost retains one independent Agent per canonical ID. Preparing a new Agent loads project config, actual backend, skills, tools and explicit-cwd session before publishing the new view. Project config overlays user defaults; environment and CLI retain priority. New sessions live in separate project-ID directories under the startup session parent. Existing session headers must match selected cwd. No global chdir is used. Runtime requests carry their original Agent; streaming, completion and approvals route to the source view. Another project can be selected while a shell/provider owns its lock. Work from another project is queued with its project identity; cancelling one project's queue keeps other queues. Closing a running/queued owner is refused without affecting selection.

Each project preserves draft/cursor/history/scroll, transcript, active and internal pane layouts, terminal status and stream state. Legacy directory metadata migrates to canonical IDs. Checkpoints are bounded and written atomically with private file permissions; reads reject symlinks/oversize/invalid canonical workspace. Saving runs independently of keyboard input with a250ms limit. Restore prepares the selected saved runtime instead of mislabelling the startup Agent.

BentoBox pane composition, split/row weights, collapse/focus, breakpoints and layout preferences remain the existing model in src/layout.rs. Project/Goal/Learn/Review/Session are internal pane presets, not project identities.

| Capability | Candidate status | Evidence / remaining boundary |
|---|---|---|
| Top row mouse +, terminal directory picker, cancel/error | Implemented and real PTY passed | ux-pty-result.json; tests/tui_project_workspace.rs |
| Canonical identity, same basename, alias reuse, bounded checkpoint | Implemented | 7 project_workspace tests; 6 host/picker tests |
| Tab close and journal-preserving reopen | Implemented candidate | middle-click/empty-prompt Ctrl-W unit coverage; real project PTY closes and reopens by cwd; master acceptance pending |
| Real selected cwd/tool/session and flat project config/backend | Implemented and PTY passed | Two actual shell marker files, header cwd and /status model checks |
| Profile-specific deep config merge | C owner fix integrated by main |121 frozen snapshot retains its original shallow overlay; final profile policy is the C/main layer |
| Switch during running shell; late output isolation | Implemented and PTY passed | Original shell sleeps while second project opens; late marker/output remains source-owned |
| Pending approvals | Request coordinator stays with running owner; cross-project responses rejected | Full focus/diff/multi-request UX belongs to124 |
| Draft/selected runtime after restart; terminal restoration | Implemented and PTY passed | SIGTERM with saved draft, reopen same checkpoint, actual model query, clean alternate-screen exit |
| Multiline editing/history/editable queue | Existing basics plus per-project retention | Full122 work remains |
| Commands/file completion/model picker | Actual resource/file/registry menu and model/effort owner;17 release PTY checks | [_] master review pending |
| Reasoning, scroll/follow/copy/context status |125 worker candidate implemented |87 focused/regression tests; actual SSE/release PTY and121 PTY regression; master acceptance pending |
| Headless shared project owner | Pure identity and independent Agent preparation APIs available |126 remains; TUI pool does not count as headless completion |

## Validation

Frozen121 delivery: `.ops/project-ui-ready/manifest.json`, `.ops/project-ui-ready/scoped.patch` and fixed source/test/smoke copies. Imported skills/Cargo files are excluded from this lane's changes. Core/config/session also have fixed `.ops/project-owner-ready/` copies for the skills lane.

`cargo +stable-aarch64-apple-darwin test` across15 named integration targets:197 passed,0 failed. Full log `Docs/quality/stage1/ux-final-tests.txt`. New host suite has6 tests and identity suite7; other184 are regressions.

`python3 tools/tui_project_workspace_smoke.py --binary target/release/zenpi`: passed against a real release binary; exact hash and assertions in `ux-pty-result.json`. The fixture preserves real HOME, isolates through ZENPI_HOME and an explicit fake credential file, and never submits a provider request. It writes real files through the production shell tool after real approval. The screen assertion helper reconstructs cursor-addressed PTY cells and Unicode widths; it does not supply application behavior.

The first PTY run found a real checkpoint scheduling gap (only input events saved). It was fixed by moving bounded saves into the loop. Harness iteration also corrected treating incremental ANSI output as a full screen and accidentally clearing a draft before a restart test. Final passing evidence uses the corrected harness and actual saved/runtime state.

## Codex source audit

Local root `/Users/wangweiyang/GitHub/codex`, HEAD `b3b3d262787f4902a7449f17d793241a34d311ad`; CLI `0.153.4`. CLI help confirms --cd/-C, resume picker and --no-alt-screen.

| Source | Read coverage | Mapping |
|---|---|---|
| key_hint.rs |112/112 lines; independent candidate report | Exact modifiers, press/repeat, platform naming and AltGr; Zenpi hints must match actual bindings |
| status_indicator_widget.rs |440/440 lines including7 tests; independent candidate report | Bounded busy details, elapsed timer with pause/resume and interrupt hint priority |
| slash_command.rs |Full file including2 tests | Authoritative order/aliases/visibility and explicit busy/argument capability; standalone candidate |
| file_search.rs |Full file; no inline tests | cwd/session/query identity and stale-result consumer boundary; standalone candidate |
| chat_composer.rs |9785/9785 lines including136 tests; two bounded byte chunks | Full composition, paste, attachment, history and submit-state candidate |
| textarea.rs |2449/2449 lines including32 tests | UTF-8/grapheme editing, atomic ranges, wrapped cursor and kill buffer candidate |
| paste_burst.rs |572/572 lines including5 inline tests | Pure timing/flush/IME/Enter suppression state machine; standalone302 candidate |
| approval_overlay.rs |1556/1556 lines including19 tests | Origin identity, advertised decisions, cancel/queue lifecycle candidate |
| pending_thread_approvals.rs |Full file including3 tests | Bounded inactive-thread attention rendering candidate |
| bottom_pane/mod.rs |1974/1974 lines including19 tests | Modal/editor precedence, retained draft and status timer integration candidate |

Target src/render.rs was read completely (901lines including11 inline tests) and has an independent frozen-hash report. The renderer remains bounded, cell-aware and responsible for sanitization, not clipboard or runtime identity. All ten formal Codex files now have independent whole-file candidates; directory acceptance remains pending until all registered direct file/child dependencies are master-accepted. The reference matrix and overall128 acceptance remain open.

## ZS1-125 transcript candidate

Only src/tui.rs and the new transcript test/smoke are modified relative to frozen121; src/render.rs is included unchanged for source mapping. Imported Chat SSE/backend/view_model files are owner dependencies excluded from the patch. The fixed delivery is `.ops/transcript-ui-ready`, against blueprint3.1.3 digest `b054960f3a433fa4d1f1425b88acd4b7d40f28220b55acfacd0cb70f36ba37c6`.

Alt-R folds/expands separate reasoning. Alt-C copies the latest assistant answer. Alt-B opens a source-block inspector with Up/Down selection, PgUp/PgDn reading, Enter/Copy, Esc/Close and interrupt support. Clipboard transport is bounded OSC52; the UI reports bytes sent. New output preserves a history anchor until End or mouse Latest. The header separates configured and returned model names, actual provider usage and estimated history/context budget. Narrow project arrows remain beside +.

Validation:87 tests in8 targets (`ux125-tests.txt`); real production Chat SSE release PTY (`ux125-pty-result.json`) covering11 checks, including exact clipboard payloads, long failing shell output, cancellation, normalized provider errors, narrow navigation and terminal restoration. Original121 project PTY passes (`ux125-project-regression.json`). Exact final binary hash is recorded by both runs. HTTP error bodies are normalized by the backend; the inspector cannot invent suppressed details. The initial harness compared a partial terminal draw; it now drains the remainder of that frame before asserting scroll position.

These implementation and supporting test reports do not enlarge the authoritative56 files /25 directories, accept any directory, or accept the formal Codex source subset.

## ZS1-123 worker candidate

The resource/command/file menu uses the actual project Agent resource snapshot and current registry. Skills/templates retain source/hash metadata without preloading their bodies; owner admission expands templates exactly once and validates attachments. A cancellable completion worker bounds discovery to2048 directory entries,150ms cooperative work and128 results, rejects parent/absolute/symlink/control paths, and applies results only for the exact project cwd and input query. Slash syntax, unavailable busy controls, invalid effort/model combinations and file access errors preserve submitted input; queued resource controls execute after current work.

Model and reasoning selection use the imported107 registry/effort owner. Model metadata and selections are project-local. Reasoning options intersect the active descriptor with effective wire capability. `default` omits the request field; supported `none` remains an explicit level. The actual owner persists selection before publishing it; unsupported combinations fail without changing saved state. `/models` retains project profiles and includes actual owner selection. `/reasoning` is a terminal control over the shared Agent API, not a second provider or persistence owner.

Final evidence is `ux123-final-tests.txt` and `ux123-pty-result.json`: 17 successful production release PTY checks,12 actual local HTTP Responses requests, resource and model/effort independent process restarts, invalid controls/draft retention, and final terminal restoration. Binary SHA256:46b2cacd5b7a0944f06e09092ae2a1359600450027ce384dca8b9cd6ac33b2c8. Supporting limits tests cover128 completion results and4096 path bytes. Fixed candidates and dependency exclusions are recorded in `.ops/command-ui-ready/manifest.json`; status remains [_] pending master acceptance.

The PTY fixture explicitly registers its synthetic models, uses complete Responses terminal events, and preserves HOME/CODEX_HOME. Two earlier attempts exposed harness ordering assumptions: journal append precedes the UI completion drain, and an immediately queued reload completes with `Resources ready`. The final harness waits for the current rendered idle header before issuing another selection; it does not weaken production admission or inject synthetic UI completion events.

## ZS1-122 initial worker candidate [_]

Grapheme editor/history/search/paste and real owner input queue integrated. Existing future-job FIFO exposes revision/project guarded edit/cancel plus explicit restart retry. 99 tests / 9 targets passed; production release PTY 15 checks / 9 HTTP passed, including independent restart. Binary b878b122c8353e2556b1ea8af90ffa9f54443a5b9b95adb516870b62a726fc5a (5175328 bytes, below unchanged8MiB). Three inherited context.rs strict-clippy diagnostics remain; TUI is clean. Parent dependencies and release strip excluded from owned patch. Fixed five-file candidate `.ops/composer-ui-ready/manifest.json`; master acceptance pending.

## ZS1-124 complete worker candidate [_]

Project/call/request-bound approval view, independent full diff, deny-first selection, explicit confirmation, draft/picker focus restoration and real cancellation. Shared owner now restores remembered human policy per session; configured deny and worker restrictions remain authoritative. 118 tests/9targets,14 production approval PTY checks/12HTTP and15 composer regression checks/9HTTP passed. Binary 4014ded4df49becb273c90393305794e1e55a893ea4bcc09c995a3ec8a6f1237 (5178208B < unchanged8MiB). Owner has no independent approval deadline; no timer or invented timeout state added. Three inherited context.rs strict-clippy diagnostics remain, no owned diagnostics. Fixed six-file candidate `.ops/approval-ui-ready/manifest.json`, authority3.1.7; parent acceptance pending.

## ZS1-126 shared project owner candidate [_]

Canonical project IDs now resolve the same actual owner pool in TUI and JSONL. Frozen `.ops/shared-project-ready/manifest.json` binds six files under3.1.10.160 regressions/12targets +48 final-envelope checks/2targets, strictlibClippy,12mixed HTTP/JSONL/PTY checks/4HTTP and12projectPTY assertions passed. Binaryc418de4e0c125e53a6f9cb3af4d983d1380c9c511e6e565fb3999e6fd5d23586 (5330576B). Parent acceptance pending; next ZS1-128 closes the full interaction matrix after remaining root integrations.

## ZS1-128 complete reference inventory (integration pending)

This section supersedes older future-work wording above; earlier counts/hashes remain historical per-item records. Frozen source: Codex HEAD `b3b3d262787f4902a7449f17d793241a34d311ad`. All ten registered files were read completely. Five independent directory analyses are now candidates in `.ops/codex-directories-ready/manifest.json` (SHA256 `7edccc0e13f7a06b873cad8ccf4e16baddfd3d1200a60b011e8f17fd1a0f1d96`). These ten files/five directories are part of the current combined frozen scope of56files/25directories (21pi-mono source files,25zenpi target files,10Codex reference files). Tests within source were inspected, not run in the Codex repository.

### Composer, modal and transcript interaction mapping

Evidence names below are cases in `tools/stage1_host_smoke.py`. They identify the executable check to rerun, not a final integrated pass. Pure contract limits additionally remain covered by their named Rust targets.

| Reference interaction | Zenpi operation / deliberate boundary | Concrete evidence |
|---|---|---|
| Grapheme-aware insert/delete/cursor, wrapped multiline | UTF-8 grapheme editor; Ctrl-J/Shift-Enter newline, Enter submit | composer; tui_composer |
| Word/line editing; kill-buffer gap | Ctrl/Alt word movement and Ctrl-U/K/W deletion exist. Ctrl-Y yank and retained kill-buffer behavior are not implemented or verified | composer; tui_composer cover existing editing only; kill/yank remains open under122 |
| History previous/next and reverse search | Ctrl-P/N, Ctrl-R then query; current draft restored on search cancellation | composer |
| Bracketed paste and enter suppression | Bounded 256 KiB draft; paste stays literal, trailing newline cannot auto-submit | composer; approval |
| Timing-sensitive non-bracketed paste | Implemented ordinary-key ASCII hold, bounded buffering, immediate initial Unicode/IME, idle flush and120ms Enter suppression; explicit later Tab reopens completion | Source302;122 followup c640a75c…; ordinary-paste actual PTY. Root followup covers pending draft restore, Unicode palette and Windows timing; final integrated rerun still required |
| Submit during work | Actual InputPort ticket with captured project owner; follow-up/steer is explicit | composer; shared-projects |
| Queued message editing/removal | Input ticket and scheduled-job controls use actual IDs/revisions; no fabricated delivery | composer |
| Interrupted queue recovery | Durable future jobs require explicit retry after restart; interrupted tasks are not silently resubmitted | composer |
| Slash popup/filter/navigation | Current authoritative command/resource snapshot, selection, argument/busy availability | commands |
| Skill/template insertion | Resource source/hash retained, body expanded by shared owner at admission | commands; stage1_resource_host |
| @ file discovery and stale-query rejection | Bounded worker tied to exact cwd/input; no stale results applied after project switch | commands; tui_command_palette |
| Attachment admission | Actual owner reads allowed workspace file once; 4096-byte path and 128-result completion limits | commands; stage1_resource_host |
| Image/media attachment widgets | No generic image/audio composer claimed by this subset; provider capability support is separate from an implemented UI | Explicit unsupported UI boundary |
| Model/effort picker | /models, /model, /reasoning; actual descriptor/wire intersection and persisted owner | commands |
| Approval focus and original draft | Alt-A opens actual pending request; Esc dismisses view, keeps pending owner request and draft | approval |
| Approval decision/preview | Deny-first, full tool/diff scroll, selection separate from Enter confirmation | approval |
| Unfocused keys/paste and higher-priority picker | Literal text cannot grant approval; directory picker keeps focus precedence | approval |
| Multiple/inactive requests | Project attention and FIFO owner requests; source overlay LIFO and silent bulk clear are not adopted | approval; shared-projects |
| Remembered approval policy | Human scope/session only, worker restriction and configured Deny authoritative | approval; tui_approval_focus |
| Busy indicator/cancel | Actual task state, bounded details; Ctrl-C cancels actual owner rather than only hiding widget | approval; compact; composer |
| Reasoning display | Alt-R folds a distinct reasoning block; actual SSE emits reasoning separately | transcript |
| Scroll anchoring/follow latest | PgUp/PgDn keep history anchor during stream; End/Latest returns to current output | transcript |
| Copy latest/block | Alt-C OSC52 latest assistant; Alt-B inspector and Enter copies selected source block | transcript |
| Usage/model/context status | Separate configured/returned model and actual usage versus estimated history budget | transcript; commands |
| Dynamic project switch | Actual canonical project Agent, cwd, tools, config, session and late-event identity | projects; shared-projects |
| Top-level folder creation flow | Visible + → terminal directory picker → valid owner; no placeholder or global chdir | projects; bentobox |
| Layout retained with composer/transcript | Existing BentoBox resize/focus/collapse/persistence remains authoritative | bentobox; tui_bentobox; layout_persistence |
| Terminal/platform controls | Restore alternate screen on clean exit/cancel; exact modifiers tested on macOS PTY | all PTY cases; other OS behavior is not claimed from this run |

### All slash variants and alias

The frozen source enum has **42 variants**, plus the `clean` parsing alias for canonical `stop`. Rows preserve source presentation order. “Different” means that a real Zenpi operation exists but has different semantics; “unimplemented” is an explicit gap, not a hidden success. Codex account, sandbox, voice and cloud-specific operations are not transferred by copying their names. The source's busy and inline-argument rules are reference contracts, while Zenpi's current owner determines its own actual availability. Source-only debug/OS gates are stated separately.

| # | Codex command | Zenpi mapping / status | Operation or evidence boundary |
|---|---|---|---|
| 1 | /model | Implemented equivalent with separate effort control | /model, /models, /reasoning; commands |
| 2 | /fast | Codex service-specific; unimplemented | No 2× plan-usage claim or fake toggle |
| 3 | /approvals | Different shared policy vocabulary | Alias of /approval ask/always/never; approval |
| 4 | /permissions | Concept maps to approval/tool policy; alias unimplemented | /approval and actual tool scope; no Codex sandbox-equivalence claim |
| 5 | /setup-default-sandbox | Platform/service-specific; unimplemented | No elevated-agent-sandbox setup UI |
| 6 | /sandbox-add-read-dir | Windows-only source command; unimplemented | Zenpi validates actual project/tool roots; does not mutate Codex sandbox |
| 7 | /experimental | Unimplemented | No generic experiment toggle menu |
| 8 | /skills | Different resource-driven insertion | Actual skill menu/resource snapshot; commands |
| 9 | /review | Different | Bounded change-review projection, not Codex model review mode; local-controls TUI/JSONL |
| 10 | /rename | Unimplemented session rename UI | Canonical project IDs are stable; project naming APIs are not proof of this command |
| 11 | /new | Unimplemented same-project fresh-chat command | + opens a project; it does not claim to create a fresh conversation in the same project |
| 12 | /resume | Different explicit owner session controls | /session list/open/resume-last and /resume sequence; session owner evidence required from 106/117 |
| 13 | /fork | Different spelling, actual session operation | /session fork SOURCE DEST; local-controls actual JSONL, final updated106 owner rerun pending |
| 14 | /init | Unimplemented | No AGENTS.md creation action |
| 15 | /compact | Implemented semantic-summary owner | compact: real summary, cancellation, persisted checkpoint and next HTTP input |
| 16 | /plan | Different | /plan BLUEPRINT :: steps creates persisted blueprint; local-controls TUI→JSONL restart. Does not switch Codex planning mode |
| 17 | /collab | Unimplemented Codex collaboration-mode UI | Runtime orchestration is not this popup |
| 18 | /agent | Different agent inspection/selection surface | /session agents lists actual registry (local-controls JSONL); project picker selects project, not an agent thread |
| 19 | /diff | Implemented bounded inspection | /diff [path]; local-controls actual Git/TUI/JSONL. Zenpi rejects while busy and retains draft for idle retry; Codex allows it during work. Bounded untracked inspection. |
| 20 | /copy | Equivalent key operation | Alt-C latest assistant, Alt-B source block; transcript. Source hides /copy on Android |
| 21 | /mention | Equivalent @ operation | @completion then attachment admission; commands |
| 22 | /status | Implemented | Actual model, usage and owner state; commands/transcript/shared-projects |
| 23 | /debug-config | Partial inspection; exact command unimplemented | /doctor/resources expose current state, not full Codex requirement-source layers |
| 24 | /statusline | Unimplemented configurable status-line menu | Current status rendering is present; customizable fields are not claimed |
| 25 | /theme | Unimplemented syntax-theme picker | Existing theme/rendering does not count as this operation |
| 26 | /mcp | Codex integration-specific command unimplemented | Zenpi's subprocess extension system is a separate contract |
| 27 | /apps | Codex apps management unimplemented | No account-connected apps menu |
| 28 | /logout | Codex account-specific; unimplemented | No claim to revoke account credentials |
| 29 | /quit | Implemented alias | Actual clean TUI/headless shutdown; all PTY cases |
| 30 | /exit | Implemented canonical command | Same owner shutdown as /quit |
| 31 | /feedback | Codex maintainer-upload flow unimplemented | No automatic log transmission |
| 32 | /rollout | Source debug-only; exact command unimplemented | Actual session path inspectable through session owner; no debug command parity claim |
| 33 | /ps | Unimplemented background-terminal listing | BentoBox terminal pane and current task are not an inventory of background terminals |
| 34 | /stop | Different cancellation scope | /cancel or Ctrl-C targets actual current project task; does not stop all background terminals |
| 34a | /clean | Source alias of /stop; unimplemented alias | No parsing alias or bulk-terminal cleanup claim |
| 35 | /clear | Different | Clears visible conversation messages; actual local-controls journal-preservation check. Does not start a new durable session |
| 36 | /personality | Different vocabulary | /persona [MBTI]; local-controls actual selection, rejection and independent restore. Not Codex personality menu |
| 37 | /realtime | Voice-specific; unimplemented | No realtime voice toggle |
| 38 | /settings | Voice-specific source settings; unimplemented | No microphone/speaker settings menu |
| 39 | /test-approval | Source debug-only; unimplemented | Tests exercise actual approval requests, never expose a synthetic production success button |
| 40 | /subagents | Codex thread switcher unimplemented | Runtime agent registry is separate; no new subagents spawned by this work |
| 41 | /debug-m-drop | Source internal debug; unimplemented | No destructive memory action copied |
| 42 | /debug-m-update | Source internal debug; unimplemented | No debug memory-generation action copied |

The unsupported rows stay visible for master scope review. This table does not enlarge the frozen source scope or convert any generic UI gap into an accepted exemption. Final ZS1-128 acceptance depends on the integrated evidence and the master decision for each mapped boundary.

## Generic interaction gaps requiring an explicit implementation or scope decision

This is a gap inventory, not an exemption list. It was checked against B's126+121-follow-up tree and the frozen ten-file Codex subset; root106/111/115 integration must be checked before claiming its new functionality absent. Source command descriptions establish advertised intent, but their execution owners outside the frozen subset have not been silently accepted. Minimum paths below are proposals for main-task ownership registration, not permission to modify unrelated files under128.

**Historical correction and subsequent implementation:** the original integration-input matrix prematurely claimed a bounded PasteBurst implementation; the independently frozen302 source report only said “if adopted.” The generic-gap supplement corrected that claim. Later122 package `c640a75ca5cd9acb1f85bf124c06f9af519295ae1766d214fe9c0655d9d5a2c7` implemented and tested ordinary-key timing. Keep all earlier packages unchanged for audit; implementation evidence comes from the new package and final integrated replay.

| Generic interaction | Concrete frozen source behavior | Current Zenpi gap | Minimum implementation and test paths |
|---|---|---|---|
| Ordinary-key paste detection / IME-safe flush | paste_burst.rs holds first ASCII, buffers a rapid burst, retro-captures selected Unicode-safe prefixes, flushes before modified keys and suppresses Enter120ms after activity; composer integrates idle flush | Implemented in the122 followup, with root followup boundary fixes pending final integrated replay. Single initial Unicode/IME input remains immediate and does not alone suppress Enter, matching source302; selected-prefix retro-capture is not required by this implementation | src/tui.rs, tests/tui_composer.rs, tools/tui_composer_smoke.py; bounded state may live in a new registered module. Reuse event-loop deadlines, no new thread |
| Large paste as atomic editable elements | chat_composer.rs stores payloads above1000characters behind unique placeholders, expands exactly by element spans, deletes associated payload and preserves literal duplicate labels | Large paste inserts literal text within the existing256KiB bound. No atomic placeholder, payload map, delete/expand/rebase contract | src/tui.rs, tests/tui_composer.rs, tools/tui_composer_smoke.py; owner input admission must retain final byte bound |
| Local/remote image attachment affordances | Composer file selection can inspect image dimensions; local image elements and remote image rows have selection/removal/labeling and submit pruning | Generic file attachment and provider image capability metadata do not supply an image composer, image rows, per-image removal or rich attachment history | src/tui.rs, src/core.rs, src/input_queue.rs, affected backend/provider admission paths, tests/tui_composer.rs and provider tests; register any new media module before work |
| External editor roundtrip | Composer external-editor export expands paste payloads and reconstructs only surviving attachment occurrences on return | No external editor launch/return flow or element rebinding | src/tui.rs, src/config.rs, existing tool/runtime subprocess owner if used, tests/tui_composer.rs, real editor-process smoke; bounded private temporary file, no automatic submit |
| Fresh conversation within the same project | /new advertises starting a new chat; /clear advertises clearing terminal and starting a new chat | + opens/reuses a canonical project; /clear only clears view. Neither creates and activates a fresh durable session in the same project | src/core.rs, src/session.rs, src/project_workspace.rs, src/protocol.rs, src/headless.rs, src/slash.rs, src/slash_actions.rs, src/tui.rs; session/project owner tests and PTY/JSONL rollback/restart |
| Rename current conversation | /rename accepts inline arguments and is advertised during work | No session rename command/UI. rename_project_tab changes a UI project name and is not a durable conversation-title owner | Session metadata/event owner in src/session.rs/core.rs; protocol/headless/slash/slash_actions/tui and session browser tests; stable session identity and busy ownership required |
| Initialize project instructions | /init advertises creating AGENTS.md | No initialization command with an actual file-write result | src/slash.rs, src/slash_actions.rs, src/tui.rs, src/headless.rs, actual core/tool write owner; approval/overwrite/cancel tests. Source initialization execution outside the frozen subset needs review before matching its generation policy |
| Planning interaction mode | /plan advertises switching Plan mode and accepts inline intent | Existing /plan creates a durable blueprint; it does not switch a conversation/tool-execution mode | src/runtime_intent.rs, src/core.rs, protocol/headless/slash/slash_actions/tui, mode/approval tests; constrain actual execution owner, not only change a label |
| Active agent/thread selector | /agent and /subagents advertise switching active agent thread | /session agents lists actual registry; a project tab is not an agent-thread selector. Full root106 session navigation must be rechecked before deciding remaining UI scope | src/tui.rs, existing session/runtime registry owner, protocol/headless/slash paths and tests. Selection must use actual existing owner IDs; this work does not spawn additional agents |
| Collaboration mode selection | /collab advertises changing a collaboration mode | No corresponding mode-selection UI/owner contract; b3ehive orchestration commands are different | src/runtime_intent.rs/runtime.rs, core/protocol/headless/slash/tui plus mode tests; inspect external source execution owner before defining equivalent modes |
| Discoverable permissions selector | /approvals and /permissions advertise choosing permitted actions | Actual approval policies and decisions exist, but the permissions alias/menu is absent; /approval requires typed vocabulary | src/tui.rs and src/slash.rs first; reuse current core/approval policy owner, tests/tui_approval_focus.rs and approval PTY. Do not replace worker/configured-Deny precedence |
| Communication-style picker | /personality advertises a choice of communication style | /persona MBTI persists actual style, but no discoverable style picker or corresponding alias exists | src/persona.rs, src/tui.rs, src/slash.rs, tests; reuse current durable persona owner. Exact source option set is outside the ten-file subset |
| Config-layer/requirement-source inspection | /debug-config advertises configuration layers and requirement sources | /doctor and resources expose current diagnostics, not the full provenance view | src/config.rs, src/tui.rs, src/slash.rs/slash_actions.rs, headless/protocol and config tests; redact secret values while retaining source provenance |
| Customizable status line | /statusline advertises configuring displayed items, idle only | Model/context/cwd/status rendering exists; no field-selection menu or persisted preference | src/config.rs, src/tui.rs, tests/layout_persistence.rs or a registered settings test, actual restart PTY; keep BentoBox geometry |
| Syntax theme selector | /theme advertises selecting syntax highlighting theme, idle only | Current renderer colors do not constitute a selectable, persisted syntax theme | src/config.rs, src/render.rs, src/tui.rs and renderer/settings tests; keep layout/focus contracts |
| Background terminal inventory and bulk stop | /ps lists background terminals; /stop and /clean stop all, advertised during work | Current tool/runtime cancellation targets current project job; no equivalent bounded terminal inventory/bulk-stop owner | src/tool_runtime.rs/tools.rs, runtime/core/protocol/headless/slash/tui and tool cancellation tests; actual process groups, ownership, resource leases, no killing unrelated processes |
| Generic experiment/settings catalog | /experimental advertises feature toggles | No general experimental-feature configuration menu | src/config.rs, src/tui.rs, slash/protocol/headless if exposed; validate actual supported features and restart semantics. A catalog of nonfunctional switches is not completion |

The following already have real alternative operations but still need an explicit semantic decision: Tab is completion/focus in Zenpi rather than Codex's idle-submit/busy-queue shortcut; `/diff` is idle-only instead of available during work; `/review` is a bounded diff projection instead of model review; `/resume` and `/fork` use explicit shared session-owner controls rather than the same source popup/spelling. Keeping BentoBox constrains key remapping, but does not automatically excuse a missing operation. Root106 integrated behavior and final production evidence determine the remaining session navigation work.

Codex service-specific Fast/account logout/apps/MCP management, voice/microphone selection, Windows sandbox elevation/read-root setup, maintainer log upload, and internal debug memory operations are separately identified in the full42+alias matrix. They are not used to hide any generic gap above. No unsupported item is marked complete by this inventory.


## Subsequent cross-entry closure evidence (still worker candidates)

- 122 ordinary-key paste: immutable candidate `c640a75ca5cd9acb1f85bf124c06f9af519295ae1766d214fe9c0655d9d5a2c7`, seven actual PTY checks/two independent TUI processes/one deliberate HTTP submission. The exact old-binary CR fixture emits an unintended HTTP request. Source LF/Ctrl-J already inserted newline; it was not the negative case. Main-task fixes for pending rejected-input restoration, Unicode palette reopening and Windows30/60ms timings must be retained in the final run.
- 121 asynchronous transcript checkpoint: candidate `8645bd5d1f7746977ed388c02e6e14439acad263924fb4ef935f3f49c2b9dc95`, nine gated active/background stream/restart checks without synthetic keypresses. Root111 direct tool-block mutations require its additional dirty flags.
- 126 owned headless resume: candidate `1ce2f36629da49065251a6f7339da689accdd4819e7868035b97d584eeb04a9f`,12real fault checks across typed resume,/session open,resume-last,21processes/90requests. Shared checkpoint commits before replacing live Agent and InputPort.
- 126 production TUI resume plus121 layout recovery: candidate `f4e692c2e437ec6c1481b69c36ff8c942f2559e62f0e671c7362aec712b1e488`,12checks across two real faults and three TUI entries,12TUI+9headless processes/two HTTP-seeded sessions. Error preserves current owner,draft,layout and shared bytes; retry/restart uses committed owner. The included121 dependency restores schema3 canonical per-project feature projections without adding top-level tabs.

These receipts close the identified implementation defects in their exact worker binaries. They do not replace128's single integrated production run or settle the remaining generic interaction gaps. Read the root's current tree before declaring a gap absent or completed.

## Main followup: source catalogue beyond status summary

The123 long-path fix preserves filename/hash in the menu and full paths in the bounded resource status response. The menu can contain2048entries while `Agent::resource_status` returns only16 per kind. Full-path inspection for later selected entries therefore remains an open123 UX requirement; the existing status-summary bound must remain intact. Current long-path checks do not prove coverage of this larger catalogue case.


## ZS1-132 主控增量（3.1.20）

同项目 `/new` 已进入 TUI/JSONL 的共享持久 owner 流程；无参数创建新 journal，保留项目分页/cwd/BentoBox、草稿及已选偏好。此行为对应冻结 Codex slash 的新会话入口；持久化与恢复细节按 Zenpi 蓝图合同实现。此前表格中该项“尚无实现”的描述为历史基线。见 [主控实现与边界](ZS1-132/master-new-session-3.1.20/review.md)。全项验收仍未闭合。


## ZS1-132 跨项目 new 与授权边界增量（3.1.20）

后台项目运行时，当前空闲项目可独立 `/new`；保留 BentoBox、分页和 cwd。121 项 Rust 回归、严格 Clippy/格式及20组真实入口通过（7 TUI、11 JSONL、31 HTTP）。同项目审批/排队/interrupted 任务仍阻止 new，授权记忆与进程策略作用域已执行列明检查。见 [主控复核记录](ZS1-132/master-new-session-ownership-3.1.20/review.md)。configured Deny/绑定 worker、grace 超时与最终 release 门禁未闭合；整项保持 `[ ]`。


## ZS1-132 最新 release 与权限作用域补验（3.1.20）

固定 release `180ec9f70d5bedda5ca3b417a88bf313fefa069df8cf3c4488c7669133e149bf` 新执行 `/new` 20组、项目picker12项、BentoBox11项均通过。新增2项共享JSONL owner权限回归覆盖7场景，连同直接回归共21项通过；configured Deny、绑定worker规则/撤销/耗尽额度在新会话保留。后者为确定性provider的库集成证明，不冒称独立release worker验证。原冷启动1044.82ms超过1000ms、grace时序及其它完整门禁仍未闭合。见 [本次独立证据](ZS1-132/master-release-policy-3.1.20/review.md)，整项保持 `[ ]`。


## ZS1-124 后台审批发现提示（3.1.20）

现有分页增加待审批数量，footer提示来源项目及切换/Alt-A审核，后台通知不改变当前草稿、布局或授权。固定旧release已观察无提示反例；最终debug实际4项新检查、14项既有审批、11项BentoBox检查及52项Rust通过。见 [主控实现及证据](ZS1-124/master-background-attention-3.1.20/review.md)。新release预算、完整审批/UX与源目录验收仍独立，整项 `[ ]`。
