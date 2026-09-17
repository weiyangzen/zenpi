# ZS1-060 — packages/coding-agent/src (directory integration)

Worker candidate `[_]`, provisional and self-tested only. This report integrates the frozen source subset that sits directly under `packages/coding-agent/src` and binds the re-frozen authority `Docs/stage_1_v3_pi_mono_blueprint.md` (`blueprint_version 3.1.22`, section `源冻结重签声明（机器迁移重冻结，2026-09-17）`, `snapshot_sha256 5755b56c6038713d1eaeca0cf927e3113032099ddba5cff3552ba2c6e0c1e6d6`, `requirement_digest f69d1f67dc6a8238f891ca8583071c72397891a3adaa17169a6d9e8516b7bc76`). Claim `ZS1-060-20260917T025319-109`, run `20260917T025319-109`, baseline `a7e6bf9`. Owned artifact is exactly `Docs/learn/stage1_pi_mono/packages/coding-agent/src/current_folder_learn.md`.

It accepts no direct file, no direct sibling, no child-directory artifact, no parent directory (`ZS1-063`), no target owner and no product item. Status stays `[_]` until an independent master writes `Docs/learn/stage1_pi_mono/receipts/ZS1-060.master.json` and passes `G-STAGE --item ZS1-060`.

## Frozen immediate children and authoritative fingerprints

The blueprint directory closure for `packages/coding-agent/src` is: **zero frozen direct files and exactly one frozen direct subdirectory**, `core/` = `ZS1-056`. Everything else physically present here is context-only. The twelve frozen files assigned to `packages/coding-agent/src/...` all live below `core/`, so they are integrated by `ZS1-056` and its own children (`ZS1-052`, `ZS1-053`), not re-accepted here.

| Item | Frozen file (under this subtree) | Blueprint §3 bytes / SHA256 | Current checkout verified |
|---|---|---|---|
| ZS1-018 | `core/session-manager.ts` | 42166 / `404b056c6b60125470e2e3d94b310f99419a93b52739a848b208256fc307dc73` | yes |
| ZS1-019 | `core/skills.ts` | 14654 / `ba3a8fb8580502f5f352e8651ff0acd1e3c83b8ba252f4619663cd42fdc305e4` | yes |
| ZS1-020 | `core/prompt-templates.ts` | 8693 / `a6e7bfd0a5e68f7ecaad523841c9923ddd200e207b8048805d43fa1475985368` | yes |
| ZS1-021 | `core/resource-loader.ts` | 30731 / `9acb127ae635c05235df4ab19e149a3a721b6e766a6ddfc03a26aff84d0a1f4e` | yes |
| ZS1-028 | `core/agent-session.ts` | 106904 / `b42bb063f411835fed213dd829fb9464f40bc5ba6985767feb662548ca8bf485` | yes |
| ZS1-030 | `core/model-registry.ts` | 24978 / `17ae1e49f351465f8de92bad05c23e829c71b1ce7182f30599399fb44fc63305` | yes |
| ZS1-022 | `core/extensions/types.ts` | 48540 / `12ab5bd74a7c91b55a321402a43d859ed240f27fd653434fef361ae97cbf0bd5` | yes |
| ZS1-023 | `core/extensions/runner.ts` | 28360 / `5f13ad4e38dbd0d10fab5216710aff1d8ab8b2e75d9209bd1b1be0b89d22950b` | yes |
| ZS1-024 | `core/tools/output-accumulator.ts` | declared `c601ddd8e10934be6f3db30696eacba7b9544f2dd1648a5ec3c9935f0d4625c3`; **absent at this revision** | n/a (historical binding) |
| ZS1-025 | `core/tools/bash.ts` | 15740 / `27d966667a71c95b2c8735a403de5e786fc448c2af3d0302302af7336b95b913` | yes |
| ZS1-026 | `core/tools/grep.ts` | 13734 / `ab1f9f5c3ad134164c6d374140d6ccbf6019cae92e1a946d9bb6288d6ae52af1` | yes |
| ZS1-027 | `core/tools/find.ts` | 11615 / `08be04c6e9b5725eeb9a93d7f0c2c5ae2e85596c948437a14b097dff4b876dec` | yes |

The source repository HEAD is the re-frozen `23282f60782f02b9e22b787e4b22af441454fa16` (`/Users/mac/GitHub/pi-mono`), verified by `git rev-parse --verify HEAD`. Eleven of twelve present files were recomputed with `shasum -a 256` and match the §3 table byte-for-byte. `core/tools/output-accumulator.ts` does not exist at this revision; the re-freeze declaration (lines 34–37) names `ZS1-024` among the paths unavailable at this revision, binds it as historical-report consistency only, and states this is **not** a blocked condition. This report binds only the table fingerprints above.

### Historical-report reconciliation (non-blocking)

Older-revision hashes still embedded in child learn reports and in the pre-re-freeze `file_learn_index.tsv` are superseded by the re-frozen table per the declaration. This report does not use them as identity and records the mismatch as a harness artifact under "Verification", not as a content defect of this directory.

## Immediate inventory: physical vs frozen

Physical immediate children: **5 files + 5 subdirectories** (10 entries).

| Name | Kind | Bytes / lines / SHA-256 | Role |
|---|---|---|---|
| `core` | directory | 25 files + 3 dirs at next level | **in-scope: ZS1-056** |
| `bun` | directory | 2 files (see below) | context-only |
| `cli` | directory | 6 files (see below) | context-only |
| `modes` | directory | 46 files (see below) | context-only |
| `utils` | directory | 15 files (see below) | context-only |
| `cli.ts` | file | 449 / 16 / `82f7ecfb212835f71edd44f2d1681e6536ca2ecc1de7917959be54b888f3b118` | context-only |
| `config.ts` | file | 7922 / 241 / `3336d45f34323babd88e6bb32ce65c00fc31fb56a04ca1fa7c002e8c56016f0d` | context-only |
| `index.ts` | file | 8123 / 348 / `45786d5114ae9c87f45621ac08bd3696ae3b174ed6cd8af008dc8cfbb2ceb506` | context-only |
| `main.ts` | file | 28402 / 895 / `bf990fbe55858919b59ca51562058bfda773e5ddff9937f54743c7dae486a60b` | context-only |
| `migrations.ts` | file | 8560 / 295 / `426c12ffd55d937cd796f27c15f0760e4f085a788433eb6179e682fdbd67b390` | context-only |

Context-only descendant inventory (names only; none is a frozen item and none is accepted, counted, or promoted):

- `bun/`: `cli.ts`, `register-bedrock.ts`.
- `cli/`: `args.ts`, `config-selector.ts`, `file-processor.ts`, `initial-message.ts`, `list-models.ts`, `session-picker.ts`.
- `modes/`: `index.ts`, `print-mode.ts`, `interactive/` (`interactive-mode.ts`, `components/**`, `theme/**`), `rpc/` (`jsonl.ts`, `rpc-client.ts`, `rpc-mode.ts`, `rpc-types.ts`).
- `utils/`: `changelog.ts`, `child-process.ts`, `clipboard-image.ts`, `clipboard-native.ts`, `clipboard.ts`, `exif-orientation.ts`, `frontmatter.ts`, `git.ts`, `image-convert.ts`, `image-resize.ts`, `mime.ts`, `photon.ts`, `shell.ts`, `sleep.ts`, `tools-manager.ts`.

`core/` physical contents are enumerated in the `ZS1-056` report; its frozen children are `ZS1-018/019/020/021/028/030` and `ZS1-052` (`extensions/`) and `ZS1-053` (`tools/`). Non-frozen `core/` siblings (`auth-storage.ts`, `bash-executor.ts`, `defaults.ts`, `diagnostics.ts`, `event-bus.ts`, `exec.ts`, `footer-data-provider.ts`, `index.ts`, `keybindings.ts`, `messages.ts`, `model-resolver.ts`, `output-guard.ts`, `package-manager.ts`, `resolve-config-value.ts`, `sdk.ts`, `settings-manager.ts`, `slash-commands.ts`, `source-info.ts`, `system-prompt.ts`, `timings.ts`, plus `compaction/` and `export-html/`) are context-only from this directory's perspective as well.

## In-scope vs context-only closure

- **In-scope (acceptance remains with the named child):** the single direct subdirectory `core/` = `ZS1-056`, and transitively `ZS1-052`/`ZS1-053` and the twelve frozen files under it. No frozen direct file exists at this level. Blueprint state of `ZS1-056` is `[x]`.
- **Context-only, read as integration background and never promoted:** the five direct files and the four non-frozen direct subdirectories listed above; `core/` non-frozen siblings. Nothing here is accepted or presented as reviewed.
- **Root-to-leaf closure:** `packages/coding-agent/src` → `{ZS1-056}` (no direct files). `G-DIR` is satisfied structurally because the parser (`tools/validate_stage1_blueprint.py::parse`, lines 298–309) computes the immediate in-scope children of a folder as the direct frozen files plus direct frozen subdirectories, and requires that set to equal the folder item's `Depends`. For `ZS1-060` that resolves to exactly `{ZS1-056}`, which is the row's declared dependency — no missing, extra, skipped or duplicated child.
- **No jump / no duplicate:** the parent `ZS1-063` is still `[ ]` and is not accepted here. Each frozen child is named once; the twelve files are not simultaneously claimed through `core/` and through this directory.

## Call relationships inside the frozen subset (integration semantics)

`cli.ts` is the shipped process entry: it suppresses `process.emitWarning`, installs an `undici` `EnvHttpProxyAgent` as the global dispatcher, then dynamically imports and calls `main(process.argv.slice(2))` from `main.ts`. `bun/cli.ts` is a Bun-only parallel entry that registers the Bedrock provider module before importing `../cli.js`.

`main.ts` is the process-level orchestrator above the `core/` owner. In order it: short-circuits `install/remove/update/list` and `config` subcommands; runs `runMigrations(process.cwd())` (one-time auth/session/dir migrations plus deprecation warnings); constructs `SettingsManager`, `AuthStorage`, `ModelRegistry`, and `DefaultResourceLoader`; loads extensions once to discover their CLI flags and applies pending provider registrations; re-parses argv with those flags; handles `--version/--help/--list-models/--export` and piped stdin; resolves the session (`createSessionManager` → `SessionManager.inMemory/open/continueRecent/create/forkFrom`, extension `session_directory` hook, `--resume` picker via `selectSession`); builds `CreateAgentSessionOptions` (model/thinking/tools/scoped models) and calls `createAgentSession` from `core/sdk.js`; clamps thinking level by model capability; and finally dispatches to `runRpcMode`, `InteractiveMode.run()`, or `runPrintMode`.

`index.ts` is a pure barrel: it re-exports the `core/` SDK surface (agent session, session manager, compaction, extensions, skills, tools), the `modes/` run modes and interactive components, `main` from `main.ts`, plus `config` and a few `utils` helpers. `getAgentDir`/`VERSION` come from `config.ts`.

`config.ts` is a leaf owned by path resolution and package identity only: it detects Bun binary/runtime and install method, resolves package/theme/export-template/JSON/README/docs/examples/CHANGELOG paths, reads `package.json` at module load to derive `APP_NAME`, `CONFIG_DIR_NAME` and `VERSION`, and computes all `~/.<app>/agent/*` user paths (`auth.json`, `models.json`, `settings.json`, `sessions`, `prompts`, `tools`, `bin`, debug log). `migrations.ts` depends only on `config.ts` and `fs`/`path`; it owns the legacy auth→`auth.json` merge, the v0.30.0 session-relocation fix, `commands/`→`prompts/` and `tools/`→`bin/` moves, and deprecated-directory warnings.

Inside the in-scope subtree (`ZS1-056`): `agent-session.ts` is the session-lifecycle hub and composes `session-manager.ts` (durable JSONL session/parent-graph/active-leaf), `skills.ts` + `prompt-templates.ts` (via `resource-loader.ts`), `model-registry.ts` (catalog + request-time auth), and the `extensions/` + `tools/` edges. `resource-loader.ts` is the discovery/composition layer; `model-registry.ts` and `session-manager.ts` are the bottom; `extensions/` and `tools/` are handler edges. Dependency direction is acyclic and layered. `modes/` consumes the `AgentSession` produced by `core/sdk.js`; `cli/` and `utils/` are leaf helpers used by `main.ts`.

## Data ownership and cross-file invariants

- `config.ts` owns filesystem path facts and the `package.json`-derived identity (`APP_NAME`, `CONFIG_DIR_NAME`, `VERSION`, `ENV_AGENT_DIR`). It reads and parses `package.json` at import time, so a missing/invalid manifest throws during module init; every other owner treats these as constants.
- `main.ts` owns the single CLI process lifecycle: two-pass argument parsing, extension-flag publication, resource reload, session selection, backend/session construction, mode dispatch, and the process exit code. It writes process env (`PI_OFFLINE`), takes over/restores stdout for non-TTY output, and calls `process.exit`/`process.exitCode` directly on several error branches.
- `migrations.ts` owns one-time startup mutations of the user config tree. Its operations are best-effort and non-transactional: `writeFileSync`/`renameSync`/`rmSync` with broad `try/catch` that swallows failures, and warnings (not errors) for deprecated `hooks/`/`tools/` directories.
- `core/` owners: `session-manager.ts` owns the versioned JSONL transcript, `byId` index, single active leaf and parent graph; `resource-loader.ts` owns the resource catalogue and `extendResources` contribution path (accessors are non-owning views); `skills.ts` owns skill metadata only; `prompt-templates.ts` owns eager template records and pure substitution; `model-registry.ts` owns the merged model list and request-time auth resolution; `extensions/` owns the typed lifecycle contract and dispatch; `tools/` owns tool definitions/wrappers; `agent-session.ts` owns the cross-cutting invariants (one active turn, one selected model/thinking level, compaction only at dedicated boundaries, extension replacement before persistence). These details are recorded in the `ZS1-056` report and are referenced, not re-accepted.

## Error, cancellation, persistence and reload paths (directory view)

- **Process lifecycle:** `cli.ts` has no error handling of its own; failures surface as rejected promises from `main`. `main.ts` prints user-facing errors and exits for model-resolution, `--api-key`-without-model, RPC-`@file`, `--fork` conflicts, export failures, `PI_STARTUP_BENCHMARK` misuse, and "no models available" in non-interactive mode.
- **Extension/load failures:** extension load errors are printed and startup continues; extension `session_directory` handler throws are caught per-extension; `resourceLoader.reload()` and extension factory loading have no `AbortSignal` on this frozen subset, so there is no cancellation token and no atomic catalogue publication at this layer.
- **Migrations:** all failures are swallowed; there is no fsync, no transactional rename contract, and no rollback of a partially migrated config tree.
- **Persistence:** durable session persistence lives in `core/session-manager.ts` (first physical write deferred until an assistant message exists; incremental `appendFileSync` without fsync; malformed JSONL lines skipped; in-memory version migration). This directory does not add persistence of its own beyond the migration moves.
- **Cancellation:** long-running operations receive abort controllers inside `agent-session.ts`; `main.ts` itself has no cancellation signal.

## Target mapping (zenpi owners named by the blueprint)

Mapping references only; this report does not accept any target file or product item.

| Source responsibility (this directory / its frozen child) | zenpi owner / item | Direction |
|---|---|---|
| config path resolution, package identity, user config dirs | `src/config.rs` — ZS1-076 | G05/config |
| CLI entry, arg parsing, mode dispatch, host process lifecycle | `src/headless.rs`, `src/tui.rs`, `src/slash_actions.rs` — ZS1-084/085/087; host smoke `tools/stage1_host_smoke.py` — ZS1-117 | G01–G10 entry |
| startup migrations / config compatibility boundary | `src/session.rs`, `src/config.rs` — ZS1-074/076 | compat boundary |
| barrel/programmatic SDK surface | `src/lib.rs` — ZS1-088 | module wiring |
| session lifecycle, steer/follow-up, tool batches | `src/core.rs`, `src/runtime.rs`, `src/input_queue.rs` — ZS1-070/071/101/102 | G01/G02 |
| durable session, parent graph, active leaf | `src/session.rs`, `src/session_tree.rs` — ZS1-074/105 | G04 |
| semantic compaction / retained tail | `src/context.rs`, `src/session.rs` — ZS1-073/103 | G03 |
| skills, prompt templates, resource reload | `src/skills.rs`, `src/prompt_templates.rs`, `src/resource_loader.rs`, `src/slash.rs` — ZS1-078/079/090/113/114 | G08 |
| extension hooks / generation revocation | `src/extensions.rs`, `src/extension_runtime.rs` — ZS1-080/115 | G09 |
| model catalogue, capability negotiation, request auth | `src/providers/registry.rs`, `src/backend.rs`, `src/config.rs` — ZS1-107 | G05 |
| tool output accumulation, search | `src/tools.rs`, `src/tool_output*`, `src/tool_runtime.rs` — ZS1-077/111/112 | G06/G07 |

The blueprint §3 target fingerprints were recomputed from this worktree and all 29 matched (including `src/core.rs` `0d4d0d1a…`, `src/session.rs` `8fb3ff2c…`, `src/skills.rs` `a6d0a2f6…`, `src/context.rs` `fc1138b9…`, `src/backend.rs` `0eae8058…`, `src/config.rs` `f5691bd3…`, `src/tools.rs` `c3149b92…`, `src/extensions.rs` `38a242ef…`, `src/headless.rs` `c2aefb53…`, `src/tui.rs` `83f5aa5b…`, `src/lib.rs` `d9392673…`).

## G-DIR closure check

1. **Direct frozen files:** none (the §3 table assigns zero source files directly under `packages/coding-agent/src`).
2. **Direct frozen subdirectories:** exactly one, `core/` = `ZS1-056`; blueprint state `[x]`.
3. **Context-only:** the five direct files and four direct subdirectories (`bun`, `cli`, `modes`, `utils`) plus non-frozen `core/` siblings, listed above and not accepted.
4. **No jump:** parent `packages/coding-agent` (`ZS1-063`) remains `[ ]` and is not accepted here.
5. **No duplicate acceptance:** the twelve frozen files are named once, through `core/` and its children, never also claimed directly here.
6. **No omission:** `Depends` = `{ZS1-056}` equals the union of immediate in-scope children; the parser enforces this equality.

## Verification performed on this host

- `git rev-parse --verify HEAD` in `/Users/mac/GitHub/pi-mono`: `23282f60782f02b9e22b787e4b22af441454fa16` (the re-frozen revision).
- `shasum -a 256` over the eleven present frozen files under this subtree: all equal the blueprint §3 fingerprints; `ZS1-024` `core/tools/output-accumulator.ts` is absent, matching the re-freeze historical-binding declaration.
- `shasum -a 256` over all 29 target owner files referenced by the blueprint §3 target table (recomputed from this worktree): all match the declared hashes and byte counts.
- `shasum -a 256` / `wc` inventory over the five direct context-only files and the `bun`/`cli`/`modes`/`utils` descendants; identities recorded above.
- Dependency-graph and directory-closure check by importing the blueprint parser (`tools/validate_stage1_blueprint.py::parse`): `ZS1-060`'s immediate in-scope children resolve to exactly `{ZS1-056}` and equal the row `Depends`; no missing/duplicate/skipped directory. `ZS1-056` parses as `[x]`.
- `python3 tools/validate_stage1_blueprint.py --blueprint Docs/stage_1_v3_pi_mono_blueprint.md --evidence-root Docs/learn/stage1_pi_mono --item ZS1-060` was attempted and cannot reach a verdict on this worker checkout for harness reasons, recorded as substitution limits rather than content failure: (a) the active selector `Docs/execution/active_requirement.json` uses `schema_version: active-requirement/1`, lacks the `active: true` key, and carries `blueprint_digest/snapshot_sha256 = ca9bade1…` which no longer equals the current blueprint SHA256 `5755b56c…`, while `selector()` requires `stage1-selector/v1` with `active: true` and a matching digest, so validation errors before checking `ZS1-060`; (b) the pre-existing `file_learn_index.tsv`/manifest scaffold still carries pre-re-freeze hashes that no longer match the re-frozen scaffold. Both are authority/harness artifacts outside this owned path.

**Non-blocking state note.** The blueprint marks `ZS1-056` `[x]` and this report binds that state for the closure check, but no `receipts/ZS1-056.master.json` exists anywhere in this checkout and `folder_learn_index.tsv` still shows `ZS1-056 [ ]`. The child `core/current_folder_learn.md` is present in the integrated canonical checkout (16487 B, SHA256 `0e7ca4fb88108c0a9be5d877a445fa9099368a65c21827abc292e5dcaaba530b`) but is not materialized in this isolated worktree. This is a harness/state inconsistency, explicitly **not** a blocked condition per the claim instruction; this report neither re-accepts nor overturns `ZS1-056`.

Limits: no TypeScript compile, Bun, Node, Vitest, Cargo, PTY, HTTP, network, provider, migration or extension-factory execution was performed. The `[_]` status and the absent `ZS1-060` master receipt are retained; structural checks do not confer semantic acceptance. This directory never claims that `ZS1-056`, the target owners, the parent `ZS1-063`, or any product item is complete.

## Rollback

Withdraw only `Docs/learn/stage1_pi_mono/packages/coding-agent/src/current_folder_learn.md`. Do not recurse into `ZS1-056`, its children, child artifacts or receipts, the twelve frozen files, or any target product code.
