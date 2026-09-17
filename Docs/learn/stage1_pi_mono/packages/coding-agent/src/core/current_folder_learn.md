# ZS1-056 — packages/coding-agent/src/core (directory integration)

Worker candidate: `[_]`, provisional. This report integrates the frozen source subset of `packages/coding-agent/src/core` under the re-frozen authority of `Docs/stage_1_v3_pi_mono_blueprint.md` (blueprint_version `3.1.22`, section `源冻结重签声明`, 2026-09-17). It does not accept any direct file, direct subdirectory, sibling directory, parent directory (`ZS1-060`), target owner file, or product item. Status stays `[_]` until an independent master writes `Docs/learn/stage1_pi_mono/receipts/ZS1-056.master.json` and passes G-STAGE.

## Frozen subset and authoritative fingerprints

Frozen immediate children are exactly the six G-FILE items plus the two G-DIR items declared as dependencies:

| Item | Frozen child | Kind | Authoritative bytes / SHA256 (blueprint §3 table) | Current checkout verified |
|---|---|---|---|---|
| ZS1-018 | `session-manager.ts` | file | 42166 / `404b056c6b60125470e2e3d94b310f99419a93b52739a848b208256fc307dc73` | yes |
| ZS1-019 | `skills.ts` | file | 14654 / `ba3a8fb8580502f5f352e8651ff0acd1e3c83b8ba252f4619663cd42fdc305e4` | yes |
| ZS1-020 | `prompt-templates.ts` | file | 8693 / `a6e7bfd0a5e68f7ecaad523841c9923ddd200e207b8048805d43fa1475985368` | yes |
| ZS1-021 | `resource-loader.ts` | file | 30731 / `9acb127ae635c05235df4ab19e149a3a721b6e766a6ddfc03a26aff84d0a1f4e` | yes |
| ZS1-028 | `agent-session.ts` | file | 106904 / `b42bb063f411835fed213dd829fb9464f40bc5ba6985767feb662548ca8bf485` | yes |
| ZS1-030 | `model-registry.ts` | file | 24978 / `17ae1e49f351465f8de92bad05c23e829c71b1ce7182f30599399fb44fc63305` | yes |
| ZS1-052 | `extensions/` | dir | child `types.ts` 48540 / `12ab5bd74a7c91b55a321402a43d859ed240f27fd653434fef361ae97cbf0bd5`; child `runner.ts` 28360 / `5f13ad4e38dbd0d10fab5216710aff1d8ab8b2e75d9209bd1b1be0b89d22950b` | yes |
| ZS1-053 | `tools/` | dir | child `bash.ts` 15740 / `27d966667a71c95b2c8735a403de5e786fc448c2af3d0302302af7336b95b913`; `grep.ts` 13734 / `ab1f9f5c3ad134164c6d374140d6ccbf6019cae92e1a946d9bb6288d6ae52af1`; `find.ts` 11615 / `08be04c6e9b5725eeb9a93d7f0c2c5ae2e85596c948437a14b097dff4b876dec`; `output-accumulator.ts` declared but absent at this revision | partial |

The source repository HEAD is the re-frozen `23282f60782f02b9e22b787e4b22af441454fa16` (`/Users/mac/GitHub/pi-mono`), verified clean by `git rev-parse HEAD`. Every present table row above was recomputed from the working checkout with `shasum -a 256` and matched the blueprint §3 fingerprint byte-for-byte. `tools/output-accumulator.ts` does not exist at this revision; the re-freeze declaration explicitly lists `ZS1-024` among the paths whose source is unavailable and binds them as historical-report consistency only, and states this is **not** a blocked condition.

### Historical-report reconciliation (non-blocking)

The child learn reports and `file_learn_index.tsv` carry older-revision hashes/sizes (for example ZS1-018 report `57bc70a7…`/54054 B, ZS1-019 `055dbfde…`/14687 B, ZS1-020 `e94b8504…`/8079 B, ZS1-021 `1877e953…`/40180 B, ZS1-028 `17116255…`/120916 B, ZS1-030 `b94ea364…`/5139 B; ZS1-025/026/027 index hashes `b76645f5…`/`88be3d00…`/`b06bcae6…`). None of those equal the authoritative §3 table fingerprints now bound here. Per `源冻结重签声明` lines 34–37 those embedded hashes are superseded by the re-frozen table and are recorded as a semantic note, not as content inconsistency. This report binds only the table fingerprints above; the structural consequence (the stale `file_learn_index.tsv` no longer matching the re-frozen scaffold) is a harness artifact reported under "Verification" below, not a defect of this report.

## In-scope vs context-only closure

In-scope (acceptance remains with the named child item): six direct files `session-manager.ts`, `skills.ts`, `prompt-templates.ts`, `resource-loader.ts`, `agent-session.ts`, `model-registry.ts`; two direct subdirectories `extensions/` and `tools/` with the frozen children listed above.

Context-only, read as integration background and never promoted: all other immediate files — `agent-session.ts` siblings `auth-storage.ts`, `bash-executor.ts`, `defaults.ts`, `diagnostics.ts`, `event-bus.ts`, `exec.ts`, `footer-data-provider.ts`, `index.ts`, `keybindings.ts`, `messages.ts`, `model-resolver.ts`, `output-guard.ts`, `package-manager.ts`, `resolve-config-value.ts`, `sdk.ts`, `settings-manager.ts`, `slash-commands.ts`, `source-info.ts`, `system-prompt.ts`, `timings.ts`; and sibling subdirectories `compaction/` (owned by ZS1-050/054) and `export-html/`. Nothing in this list is accepted, counted for coverage, or used to claim a target obligation.

Root-to-leaf closure: `docs/…/core` → children `{018,019,020,021,028,030}` (files) + `{052,053}` (directories), which is exactly the `Depends` set on the ZS1-056 row. No directory is skipped (`packages/coding-agent/src` is ZS1-060 and remains `[ ]`), no accepting item is duplicated, and no context-only file is presented as reviewed. The two child directories in turn depend only on their own immediate files (`052←022,023`; `053←024,025,026,027`), so the closure is complete and non-overlapping below this directory.

## Call relationships inside the frozen subset

`agent-session.ts` is the orchestration hub. It imports the type/contract of `SessionManager` plus `CURRENT_SESSION_VERSION` and `getLatestCompactionEntry` from `session-manager.ts`; `expandPromptTemplate`, `PromptTemplate` from `prompt-templates.ts`; `ResourceExtensionPaths`, `ResourceLoader` from `resource-loader.ts`; `ModelRegistry` from `model-registry.ts`; `ExtensionRunner` and `wrapRegisteredTools` from `extensions/index.js`; and `createAllToolDefinitions` plus `createToolDefinitionFromAgentTool`/`wrapToolDefinition` from the `tools/` directory. It constructs an `AgentSession` over an `Agent`, a `SessionManager`, a `SettingsManager`, a `ResourceLoader`, a `ModelRegistry`, and an `ExtensionRunner` reference, then wires agent events, tool hooks, and prompt admission.

`resource-loader.ts` is the discovery/orchestration layer below the hub. It imports `loadSkills` / `Skill` from `skills.ts`, `loadPromptTemplates` / `PromptTemplate` from `prompt-templates.ts`, and `createExtensionRuntime`, `loadExtensionFromFactory`, `loadExtensions` from `extensions/loader.js` with the `Extension*` contracts from `extensions/types.js`. `extensions/index.ts` is the export edge over `types.ts`/`runner.ts`/`loader.ts`/`wrapper.ts`. `tools/index.ts` is the export edge that `createAllToolDefinitions` composes from the individual tool modules. `model-registry.ts` consumes `auth-storage.ts` and `resolve-config-value.ts` (context-only) and the `pi-ai` provider/model catalog.

Dependency direction is acyclic and layered: `session-manager.ts` (standalone durable store) and `model-registry.ts` (catalog/auth) sit at the bottom; `skills.ts` and `prompt-templates.ts` are pure discovery/formatting leaves; `resource-loader.ts` composes them; `extensions/` and `tools/` are handler/tool edges; `agent-session.ts` couples all of them into one session lifecycle.

## Data ownership and cross-file invariants

- **`session-manager.ts`** owns the durable state: a versioned JSONL `SessionHeader` (`CURRENT_SESSION_VERSION = 3`) plus append-only `SessionEntry` records, the `byId` index, the single `leafId`, label resolution, and the parent graph. `getLatestCompactionEntry` and `buildSessionContext(entries, leafId, byId)` derive runtime context by walking leaf→root, emitting the compaction summary first, then the retained tail from `firstKeptEntryId` up to the compaction, then entries after it; `leafId === null` is an explicit empty context. Every mutation method (`appendMessage`, `appendModelChange`, `appendThinkingLevelChange`, `appendCompaction`, `appendBranchSummary`, `appendLabelChange`, `appendSessionInfo`) creates a child of the current leaf and advances the leaf, so transcript, branch, and label projections cannot disagree about the active path.
- **`resource-loader.ts`** owns the resource catalogue (extensions, skills, prompts, themes, agent files, system/append prompt) and the `extendResources` contribution path; the `get*` accessors hand out inner arrays/objects by reference, so callers must treat them as non-owning views. It is not the owner of Markdown/YAML skill parsing, template substitution, extension execution, package installation, model calls, or session persistence — those stay with their respective modules.
- **`skills.ts`** owns only skill metadata (`Skill`, `SkillFrontmatter`, `LoadSkillsResult`), never the body; `formatSkillsForPrompt` renders only metadata and returns empty when nothing is visible. **`prompt-templates.ts`** owns the eager template record and the pure substitution/expansion functions (`parseCommandArgs`, `substituteArgs`, `expandPromptTemplate`); `expandPromptTemplate` rewrites only leading-slash input and is not an authorization layer.
- **`model-registry.ts`** owns the model list and request-time auth resolution (`getAll`, `getAvailable`, `find`, `getApiKey`, `getApiKeyForProvider`, `isUsingOAuth`, `registerProvider`/`unregisterProvider`). Built-in `pi-ai` models, `models.json` custom models/overrides, and OAuth provider model mutation are merged into one list; provider registration is validated and applied through `applyProviderConfig`.
- **`extensions/`** owns the typed extension lifecycle contract (`types.ts`), the event dispatch/command registry (`runner.ts`), and loader/runtime construction. **`tools/`** owns the tool definitions and their wrappers. `agent-session.ts` owns the cross-cutting invariants: one active turn, one selected model/thinking level, compaction only at dedicated boundaries, and extension replacement before persistence.

## Error, cancellation, persistence and reload paths

- **Persistence:** `SessionManager._persist` defers the first physical write until an assistant message exists; while none exists it sets `flushed = false` and returns, so user-only turns are not durable yet. On the first assistant entry it appends every buffered entry, then appends incrementally. Ordinary appends use `appendFileSync` without fsync/transactional rename; malformed JSONL lines are skipped by `parseSessionEntries` and migration (`migrateToCurrentVersion`) rewrites in memory from v1→v2→v3.
- **Discovery errors:** `skills.ts` and `prompt-templates.ts` swallow many read/parse failures (warning-only diagnostics or `null`), and `resource-loader.ts` collects diagnostics rather than failing the whole reload; `model-registry.ts` keeps built-in models when `models.json` fails to load and exposes the error via `getError`. These are source permissive-recovery facts, not target guarantees.
- **Cancellation/reload:** `resource-loader.reload()` sequences extension load, inline factories, skills, prompts, themes, context and system/append discovery with no `AbortSignal`; the frozen subset exposes no cancellation token on discovery and no atomic catalogue publication. `agent-session.ts` is where long-running operations get abort controllers and where extension reload replaces the runner reference while `dispose` invalidates stale contexts.
- **Model/auth:** `getApiKey`/`getApiKeyForProvider` resolve at request time through `auth-storage`; error strings are normalized for user guidance. The frozen subset performs no provider HTTP call itself.

## Target mapping (zenpi owners named by the blueprint)

| Source responsibility | Target owner / item | Direction |
|---|---|---|
| session header, entries, parent graph, active leaf, branch/label projection | `src/session.rs`, `src/session_tree.rs` — ZS1-074/ZS1-105 | G04 |
| semantic compaction cut-points and retained tail | `src/context.rs`, `src/session.rs` — ZS1-073/ZS1-103 | G03 |
| skill metadata discovery, progressive body disclosure | `src/skills.rs`, `src/core.rs` — ZS1-078/ZS1-113 | G08 |
| prompt template parse/substitute/expand | `src/prompt_templates.rs`, `src/slash.rs`, `src/slash_actions.rs` — ZS1-079/ZS1-114 | G08 |
| resource reload orchestration and source/hash provenance | `src/resource_loader.rs`, `src/core.rs` — ZS1-090 | G08/G09 |
| model catalogue, capability negotiation, request auth | `src/providers/registry.rs`, `src/backend.rs`, `src/config.rs` — ZS1-107 | G05 |
| extension hooks, lease/generation revocation | `src/extensions.rs`, `src/extension_runtime.rs` — ZS1-080/ZS1-115 | G09 |
| tool batches, output accumulation, search | `src/tools.rs`, `src/tool_runtime.rs`, `src/tool_output*` — ZS1-077/ZS1-102/ZS1-111/ZS1-112 | G02/G06/G07 |
| session lifecycle, steer/follow-up, compaction controller | `src/core.rs`, `src/runtime.rs`, `src/input_queue.rs` — ZS1-070/ZS1-101/ZS1-102 | G01–G04 |

The target owner hashes in the blueprint §3 target table were recomputed from this worktree and matched (`src/core.rs` `0d4d0d1a…`, `src/session.rs` `8fb3ff2c…`, `src/skills.rs` `a6d0a2f6…`, `src/context.rs` `fc1138b9…`, `src/backend.rs` `0eae8058…`, `src/config.rs` `f5691bd3…`, `src/tools.rs` `c3149b92…`, `src/extensions.rs` `38a242ef…`, `src/resource_loader.rs` `121d3b6e…`, `src/prompt_templates.rs` `1b3f2531…`). These are mapping references only; this report does not accept the target files or their product items.

## G-DIR closure check

1. Direct files: six, and each has an independent per-file artifact and `[x]` index status with a master receipt (018/019/020/021/028/030 present under `Docs/learn/stage1_pi_mono/receipts/`).
2. Direct subdirectories: exactly two, `extensions/` (ZS1-052) and `tools/` (ZS1-053); both have master receipts and `[x]` folder-index status.
3. Context-only: all remaining immediate files and the `compaction/` and `export-html/` subdirectories, listed above and not accepted.
4. No jump: the parent `packages/coding-agent/src` (ZS1-060) is still `[ ]` and is not accepted here.
5. No duplicate acceptance: each frozen child is named once; the six files are not also claimed through a subdirectory.
6. No omission: the `Depends` set equals the union of immediate children exactly.

## Verification performed on this host

- `shasum -a 256` over the six direct files and five present subdirectory files: all equal the authoritative blueprint §3 fingerprints.
- `git rev-parse HEAD` in `/Users/mac/GitHub/pi-mono`: `23282f60782f02b9e22b787e4b22af441454fa16`, the re-frozen revision.
- `shasum -a 256` over the ten referenced target owner files: all equal the blueprint §3 target fingerprints.
- Structural blueprint/directory closure was checked by importing the blueprint parser (`tools/validate_stage1_blueprint.py::parse`), which constructs the item DAG, per-scope file sets, and the folder→immediate-child dependency equality; ZS1-056's children resolve to `{018,019,020,021,028,030,052,053}` with no missing or duplicate directory.
- `python3 tools/validate_stage1_blueprint.py --blueprint Docs/stage_1_v3_pi_mono_blueprint.md --evidence-root Docs/learn/stage1_pi_mono --item ZS1-056` was attempted but cannot reach a verdict on this worker checkout for two harness reasons, recorded as substitution limits rather than content failure: (a) the active selector `Docs/execution/active_requirement.json` uses `schema_version: active-requirement/1` without the `active` key and lacks the required final newline, while this validator's `selector()` requires `stage1-selector/v1` with `active: true`, so it errors before checking ZS1-056; (b) the pre-existing `file_learn_index.tsv`/manifest still carry the pre-re-freeze hashes for ZS1-025/026/027, which no longer match the re-frozen scaffold. Both are authority/harness artifacts of this run, outside the owned path, and neither indicates a defect in this report. The closest equivalent checks above (fingerprint recomputation, target-hash recomputation, parser-level closure) were performed instead.

Limits: no source test suite, `tsc`, Bun, Cargo, PTY, HTTP, network, provider, or extension-factory execution was performed. The `[_]` status and the absent ZS1-056 master receipt are retained; structural checks do not confer semantic acceptance. This directory never claims that the target owners, parent directory, or product items are complete.

## Rollback

Withdraw only `Docs/learn/stage1_pi_mono/packages/coding-agent/src/core/current_folder_learn.md`. Do not recurse into ZS1-018/019/020/021/028/030, ZS1-052/053, their receipts, or any target product code.
