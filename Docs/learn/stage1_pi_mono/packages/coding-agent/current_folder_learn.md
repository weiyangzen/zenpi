# ZS1-063 — packages/coding-agent (directory integration)

Worker candidate `[_]`, provisional and self-tested only. This report integrates the frozen source subset that sits directly under `pi-mono/packages/coding-agent` and binds the re-frozen authority `Docs/stage_1_v3_pi_mono_blueprint.md` (`blueprint_version 3.1.22`, section `源冻结重签声明（机器迁移重冻结，2026-09-17）`, blueprint `snapshot_sha256 cbec7c513b35a3c2b9a4dd562e90defe7a0d3915cfeae50f77c522e9b1328251`, `requirement_digest f69d1f67dc6a8238f891ca8583071c72397891a3adaa17169a6d9e8516b7bc76`). Claim `ZS1-063-20260917T025724-115`, run `20260917T025724-115`, baseline `71e3ecd0686cb8c8cbef8ece8468e48567ffaead`. Owned artifact is exactly `Docs/learn/stage1_pi_mono/packages/coding-agent/current_folder_learn.md`.

It accepts no direct file, no sibling package, no child-directory artifact, no parent directory (`ZS1-064`), no target owner and no product item. Status stays `[_]` until an independent master writes a receipt for this item and passes `G-STAGE --item ZS1-063`.

## Frozen immediate children and authoritative fingerprints

The blueprint directory closure for `packages/coding-agent` is: **zero frozen direct files and exactly one frozen direct subdirectory**, `src/` = `ZS1-060`. The twelve frozen coding-agent source files all live below `src/core/`, so they are integrated there (`ZS1-060` → `ZS1-056` → `ZS1-018…028/030`, `ZS1-052`, `ZS1-053`) and are not re-accepted here. Everything else physically present at this level is context-only.

| Item | Frozen child (under this directory) | Kind | Blueprint §3 fingerprint | Current checkout verified |
|---|---|---|---|---|
| ZS1-060 | `src/` | directory | folder `docs/…/packages/coding-agent/src/current_folder_learn.md`; `Depends ZS1-056` | yes (subtree present) |

Transitive frozen coding-agent file fingerprints that this directory's single child carries (recomputed, see Verification):

| Item | Frozen file (under `src/`) | Blueprint §3 bytes / SHA256 | Current checkout |
|---|---|---|---|
| ZS1-018 | `core/session-manager.ts` | 42166 / `404b056c6b60125470e2e3d94b310f99419a93b52739a848b208256fc307dc73` | match |
| ZS1-019 | `core/skills.ts` | 14654 / `ba3a8fb8580502f5f352e8651ff0acd1e3c83b8ba252f4619663cd42fdc305e4` | match |
| ZS1-020 | `core/prompt-templates.ts` | 8693 / `a6e7bfd0a5e68f7ecaad523841c9923ddd200e207b8048805d43fa1475985368` | match |
| ZS1-021 | `core/resource-loader.ts` | 30731 / `9acb127ae635c05235df4ab19e149a3a721b6e766a6ddfc03a26aff84d0a1f4e` | match |
| ZS1-028 | `core/agent-session.ts` | 106904 / `b42bb063f411835fed213dd829fb9464f40bc5ba6985767feb662548ca8bf485` | match |
| ZS1-030 | `core/model-registry.ts` | 24978 / `17ae1e49f351465f8de92bad05c23e829c71b1ce7182f30599399fb44fc63305` | match |
| ZS1-022 | `core/extensions/types.ts` | 48540 / `12ab5bd74a7c91b55a321402a43d859ed240f27fd653434fef361ae97cbf0bd5` | match |
| ZS1-023 | `core/extensions/runner.ts` | 28360 / `5f13ad4e38dbd0d10fab5216710aff1d8ab8b2e75d9209bd1b1be0b89d22950b` | match |
| ZS1-024 | `core/tools/output-accumulator.ts` | declared `c601ddd8e10934be6f3db30696eacba7b9544f2dd1648a5ec3c9935f0d4625c3`; **absent at this revision** | historical binding |
| ZS1-025 | `core/tools/bash.ts` | 15740 / `27d966667a71c95b2c8735a403de5e786fc448c2af3d0302302af7336b95b913` | match |
| ZS1-026 | `core/tools/grep.ts` | 13734 / `ab1f9f5c3ad134164c6d374140d6ccbf6019cae92e1a946d9bb6288d6ae52af1` | match |
| ZS1-027 | `core/tools/find.ts` | 11615 / `08be04c6e9b5725eeb9a93d7f0c2c5ae2e85596c948437a14b097dff4b876dec` | match |

The source repository HEAD is the re-frozen `23282f60782f02b9e22b787e4b22af441454fa16` (`/Users/mac/GitHub/pi-mono`), verified by `git rev-parse --verify HEAD`. All eleven present coding-agent frozen files were recomputed with `shasum -a 256` and match the §3 table byte-for-byte. `core/tools/output-accumulator.ts` does not exist at this revision; the re-freeze declaration explicitly lists `ZS1-024` among the paths unavailable at this revision, binds it as historical-report consistency only, and states this is **not** a blocked condition. This report binds only the table fingerprints above.

### Historical-report reconciliation (non-blocking)

Older-revision hashes still embedded in child learn reports and in the pre-re-freeze `file_learn_index.tsv` are superseded by the re-frozen table per the declaration (lines 34–37). They are recorded as a semantic note, not as content inconsistency, and never used as identity here. A whole-blueprint recomputation found 15 source files matching the current checkout and exactly 6 absent — `ZS1-012`, `ZS1-013`, `ZS1-016`, `ZS1-017`, `ZS1-024`, `ZS1-029` — which is precisely the unavailable set named in the declaration. No present frozen file mismatched.

## Immediate inventory: physical vs frozen

Physical immediate children: **7 files + 5 subdirectories** (12 entries).

| Name | Kind | Bytes / lines / SHA-256 | Role |
|---|---|---|---|
| `src` | directory | 131 files + 14 dirs (next level: 5 files + 5 dirs) | **in-scope: ZS1-060** |
| `test` | directory | 97 files + 25 dirs | context-only |
| `docs` | directory | 27 files + 1 dir (`images/`) | context-only |
| `examples` | directory | 120 files + 15 dirs | context-only |
| `scripts` | directory | 1 file (`migrate-sessions.sh`) | context-only |
| `.gitignore` | file | 12 / 1 / `fdad769d0f4c1a584922a490dc712ba4ddb31b7ae73cdda4f5c1145c6907c0c0` | context-only |
| `CHANGELOG.md` | file | 265129 / 3300 / `36872fa04681b336367307c4cecb7d4694357b862875d4a18c2dbc3dcdb27866` | context-only |
| `README.md` | file | 22910 / 596 / `0013c97d6a81191b9cf18ab0b3b0d8724f5ed13f8be8f31683e9763e836439e2` | context-only |
| `package.json` | file | 3259 / 99 / `01fa5b00bb725b5e41269b6a1812c577328fbe1bbf493e373ff86bdff66c114d` | context-only |
| `tsconfig.build.json` | file | 209 / 9 / `5d1d8e87055b879ea8525af3d2061952705dac28f2a79450cc86b138828eff3f` | context-only |
| `tsconfig.examples.json` | file | 496 / 16 / `e68adef25cb46d30ba206dafc65253f6087da11db83f9e8ae396102caf9e54a0` | context-only |
| `vitest.config.ts` | file | 287 / 14 / `6a0a3456f8996f31a80b0f9030473af92552aa8c31c604348db4e17bf95cee0e` | context-only |

Context-only descendant inventory (names only; none is a frozen item and none is accepted, counted, or promoted):

- `docs/`: `compaction.md`, `custom-provider.md`, `development.md`, `extensions.md`, `json.md`, `keybindings.md`, `models.md`, `packages.md`, `prompt-templates.md`, `providers.md`, `rpc.md`, `sdk.md`, `session.md`, `settings.md`, `shell-aliases.md`, `skills.md`, `terminal-setup.md`, `termux.md`, `themes.md`, `tmux.md`, `tree.md`, `tui.md`, `windows.md`, plus `images/` (`doom-extension.png`, `exy.png`, `interactive-mode.png`, `tree-view.png`).
- `examples/`: `README.md`, `rpc-extension-ui.ts`, `sdk/` (`01-minimal.ts` … `12-full-control.ts`, `README.md`), and `extensions/` (~67 entries: `hello.ts`, `custom-footer.ts`, `permission-gate.ts`, `plan-mode/`, `sandbox/`, `subagent/`, `custom-provider-anthropic/`, `custom-provider-gitlab-duo/`, `custom-provider-qwen-cli/`, `doom-overlay/`, `dynamic-resources/`, `with-deps/`, and the remaining ~55 single-file examples).
- `scripts/`: `migrate-sessions.sh`.
- `test/`: 69 files at top level (agent-session/session/compaction/extensions/model/tools/skills/prompt-template/settings/rpc/utilities suites and helper modules such as `test-harness.ts`, `utilities.ts`) plus the `fixtures/` and `session-manager/` subtrees (71 top-level entries, 97 files / 25 dirs in total). No coding-agent test file is a frozen `ZS1-` item; the frozen test item `ZS1-014` belongs to `packages/agent/test`, a different package.
- `src/`: frozen child `ZS1-060`; its own report enumerates the direct files (`cli.ts`, `config.ts`, `index.ts`, `main.ts`, `migrations.ts`), the subdirectories (`bun/`, `cli/`, `core/`, `modes/`, `utils/`) and the frozen `core/` children. That detail is referenced, not re-accepted.

## In-scope vs context-only closure

- **In-scope (acceptance remains with the named child):** the single direct subdirectory `src/` = `ZS1-060`, and transitively `ZS1-056` → `{ZS1-018,019,020,021,028,030,052,053}`. No frozen direct file exists at this level. The work-tree blueprint §5 marks `ZS1-060` `[x]` and `ZS1-056` `[x]`.
- **Context-only, read as integration background and never promoted:** `test/`, `docs/`, `examples/`, `scripts/` and the six non-`src` direct files listed above. Nothing here is accepted or presented as reviewed; no `packages/coding-agent/test` obligation exists in the frozen scope.
- **Root-to-leaf closure:** `packages/coding-agent` → `{ZS1-060}` (no direct files). `G-DIR` is satisfied structurally because the blueprint parser (`tools/validate_stage1_blueprint.py::parse`, lines 298–309) computes the immediate in-scope children of a folder as the direct frozen files plus direct frozen subdirectories and requires that set to equal the folder item's `Depends`. For `ZS1-063` that resolves to exactly `{ZS1-060}`, the row's declared dependency — no missing, extra, skipped or duplicated child.
- **No jump / no duplicate:** the parent `ZS1-064` is `[ ]` and is not accepted here; each frozen child is named once, and the twelve files are reached only through `src/`, never also claimed directly at this level.

## Call relationships inside the frozen subset (package integration semantics)

At this directory level the shipped processes are the files directly under `src/`, and the package is the entry surface for the whole CLI:

- `src/cli.ts` is the `bin` entry (`package.json` `bin.pi = dist/cli.js`). It sets `process.title = "pi"`, suppresses `process.emitWarning`, installs an `undici` `EnvHttpProxyAgent` as the global dispatcher, then imports and calls `main(process.argv.slice(2))` from `src/main.ts`. `src/bun/cli.ts` (context-only) is a Bun-only parallel entry that registers the Bedrock provider before importing `../cli.js`.
- `src/main.ts` is the process-level orchestrator above the `core/` owner. It short-circuits package subcommands (`install/remove/uninstall/update/list`), `config`, runs `runMigrations(process.cwd())` from `src/migrations.ts`, constructs `SettingsManager`/`AuthStorage`/`ModelRegistry`/`DefaultResourceLoader`, loads extensions once to discover CLI flags, applies pending provider registrations, re-parses argv, handles `--version/--help/--list-models/--export` and piped stdin, selects a session (`SessionManager` factory methods, `session_directory` extension hook, `--resume` picker), builds `CreateAgentSessionOptions`, clamps thinking level by model capability, and dispatches to `runRpcMode`, `InteractiveMode.run()` or `runPrintMode`.
- `src/index.ts` is a pure barrel re-exporting the `core/` SDK surface, the `modes/` run modes and interactive components, `main`, `config`, and a few `utils` helpers. It is the declared `main`/`types`/`exports["."]` surface.
- `src/config.ts` owns path resolution and package identity only: it detects Bun binary/runtime and install method, resolves package/theme/export-template/JSON/README/docs/examples/CHANGELOG paths, reads `package.json` at module load to derive `APP_NAME`, `CONFIG_DIR_NAME`, `VERSION`, `ENV_AGENT_DIR`, and computes all `~/.<app>/agent/*` user paths.
- `src/migrations.ts` depends only on `config.ts` plus `fs`/`path`; it owns the legacy auth→`auth.json` merge, the v0.30.0 session-relocation fix, `commands/`→`prompts/` and `tools/`→`bin/` moves, and deprecated-directory warnings.

Dependency direction is acyclic: `config.ts` and `migrations.ts` are leaves over process/config state; the `core/` subtree (via `ZS1-060`) owns session lifecycle, resource loading, model registry, extensions and tools; `modes/` consumes the `AgentSession`; `cli/` and `utils/` are leaf helpers used by `main.ts`; `index.ts` is the programmatic barrel.

### Package build/export integration facts (context-confirmed)

- `package.json` declares `@mariozechner/pi-coding-agent` 0.62.0, ESM, `engines.node >= 20.6.0`, `bin.pi`, `main`/`types` → `dist/index.js`/`.d.ts`, and a second export `"./hooks"` → `dist/core/hooks/index.js`. **`src/core/hooks/` does not exist at this revision** (the extension system lives in `src/core/extensions/`), so the `"./hooks"` export and the `tsconfig.examples.json` `@mariozechner/pi-coding-agent/hooks` path are stale/aspirational and resolve to nothing here. This is a source-fact note, not a target obligation.
- `build` = `tsgo -p tsconfig.build.json` plus `copy-assets`; `build:binary` also builds `../tui`, `../ai`, `../agent` and compiles `./dist/bun/cli.js`. `tsconfig.build.json` sets `rootDir=src`, `outDir=dist`, `include src/**/*.ts`. `tsconfig.examples.json` is `noEmit` and aliases sibling package sources.
- `test` = `vitest --run`; `vitest.config.ts` uses `node` environment and a 30 s timeout; `prepublishOnly` runs `clean`+`build`. No install, build or test execution was performed in this review.

## Data ownership and cross-file invariants

- `config.ts` owns filesystem path facts and `package.json`-derived identity. It reads and parses `package.json` at import time, so a missing/invalid manifest throws during module init; every other owner treats `APP_NAME`/`CONFIG_DIR_NAME`/`VERSION`/`ENV_AGENT_DIR` and the `~/.<app>/agent/*` paths as constants.
- `main.ts` owns the single CLI process lifecycle: two-pass argument parsing, extension-flag publication, resource reload, session selection, backend/session construction, mode dispatch and the process exit code. It writes process env (`PI_OFFLINE`, `PI_SKIP_VERSION_CHECK`), takes over/restores stdout for non-TTY output, and calls `process.exit`/`process.exitCode` directly on its error branches.
- `migrations.ts` owns one-time startup mutations of the user config tree. Its operations are best-effort and non-transactional (`writeFileSync`/`renameSync`/`rmSync` with broad `try/catch`), with warnings rather than errors for deprecated `hooks/`/`tools/` directories.
- The in-scope subtree's cross-cutting invariants (one active turn, one selected model/thinking level, compaction only at dedicated boundaries, extension replacement before persistence, durable JSONL session graph) are owned by `src/core/` and recorded in the `ZS1-056`/`ZS1-060` reports; they are referenced here, not re-accepted.

## Error, cancellation, persistence and reload paths (directory view)

- **Process lifecycle:** `cli.ts` has no error handling of its own; failures surface as rejected promises from `main`. `main.ts` prints user-facing errors and exits for model-resolution, `--api-key`-without-model, RPC-`@file`, `--fork` conflicts, export failures, `PI_STARTUP_BENCHMARK` misuse, and "no models available" in non-interactive mode.
- **Extension/load failures:** extension load errors are printed and startup continues; extension `session_directory` handler throws are caught per-extension; `resourceLoader.reload()` and extension factory loading have no `AbortSignal` on this frozen subset, so there is no cancellation token and no atomic catalogue publication at this layer.
- **Migrations:** all failures are swallowed; there is no fsync, no transactional rename contract, and no rollback of a partially migrated config tree.
- **Persistence:** durable session persistence lives in `core/session-manager.ts` (first physical write deferred until an assistant message exists; incremental `appendFileSync` without fsync; malformed JSONL lines skipped; in-memory version migration). This directory adds no persistence beyond the migration moves.
- **Cancellation:** long-running operations receive abort controllers inside `core/agent-session.ts`; `main.ts` itself has no cancellation signal.

## Target mapping (zenpi owners named by the blueprint)

Mapping references only; this report does not accept any target file or product item. Rows are inherited from the in-scope child `ZS1-060` and its descendants.

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

The blueprint §3 target fingerprints were recomputed from the zenpi worktree by the in-scope child (`ZS1-060`) and all matched; that verification is referenced, not repeated here.

## G-DIR closure check

1. **Direct frozen files:** none (the §3 table assigns zero source files directly under `packages/coding-agent`).
2. **Direct frozen subdirectories:** exactly one, `src/` = `ZS1-060`; work-tree blueprint state `[x]`.
3. **Context-only:** `test/`, `docs/`, `examples/`, `scripts/` and the six non-`src` direct files, listed above and not accepted.
4. **No jump:** parent `packages` (`ZS1-064`) remains `[ ]` and is not accepted here.
5. **No duplicate acceptance:** the twelve frozen files are reached once, through `src/` and its children, never also claimed directly here.
6. **No omission:** `Depends` = `{ZS1-060}` equals the union of immediate in-scope children; the parser enforces this equality.

## Verification performed on this host

- `git rev-parse --verify HEAD` in `/Users/mac/GitHub/pi-mono`: `23282f60782f02b9e22b787e4b22af441454fa16` (the re-frozen revision).
- Whole-blueprint source-hash recomputation by importing the parser (`blueprint.files` + `hashlib.sha256`): 15 of 21 source files present and byte-identical to the §3 table; the 6 absent files are exactly the declaration's historical-binding set (`ZS1-012`, `ZS1-013`, `ZS1-016`, `ZS1-017`, `ZS1-024`, `ZS1-029`). All 11 present coding-agent frozen files match.
- `shasum -a 256` / `wc` inventory over the seven direct files and the five direct subdirectories; identities recorded above.
- Structural blueprint/directory closure by importing `tools/validate_stage1_blueprint.py::parse`: `ZS1-063`'s immediate in-scope children resolve to exactly `{ZS1-060}` and equal the row `Depends`; no missing/duplicate/skipped directory. The parser also confirms `ZS1-060 [x]`, `ZS1-056 [x]`, `ZS1-063 [ ]`.
- `python3 tools/test_validate_stage1_blueprint.py`: 46 tests, all pass, exit 0. This exercises the blueprint parser, G-DIR/G-FILE/null/dup/skip/scaffold/selector rules and the receipt/command-evidence checker, and is the closest available substitute for the declared validator on this host.
- `python3 tools/validate_stage1_blueprint.py --blueprint Docs/stage_1_v3_pi_mono_blueprint.md --evidence-root Docs/learn/stage1_pi_mono --item ZS1-063` was attempted and cannot reach a verdict on this worker checkout for a harness reason, recorded as a substitution limit rather than content failure: the active selector `Docs/execution/active_requirement.json` uses `schema_version: active-requirement/1` and lacks the final newline, while this validator's `read_json`/`selector()` requires a newline-terminated `stage1-selector/v1` with `active: true`. It errors before checking `ZS1-063`. The other declared validator, G-DIR, is implemented inside the same parser and was exercised directly (see the closure check above).

**Non-blocking state note.** The work-tree blueprint marks `ZS1-060` and `ZS1-056` `[x]`, and this report binds those states for the closure check, but the filesystem index `folder_learn_index.tsv` still shows `ZS1-060 [ ]` and `ZS1-056 [ ]`, and the `ZS1-060` candidate report is not materialized in this isolated worktree (it is present in the integrated canonical checkout). The canonical `ZS1-060` report also declares an earlier blueprint snapshot (`5755b56c…`) than the re-frozen authority this claim binds (`cbec7c51…`). These are harness/state inconsistencies, explicitly **not** a blocked condition per the claim instruction; this report neither re-accepts nor overturns `ZS1-060`/`ZS1-056`.

Limits: no TypeScript compile, Bun, Node, Vitest, Cargo, PTY, HTTP, network, provider, migration or extension-factory execution was performed. The `[_]` status and the absent master receipt for this item are retained; structural checks do not confer semantic acceptance. This directory never claims that `ZS1-060`, the target owners, the parent `ZS1-064`, or any product item is complete.

## Rollback

Withdraw only `Docs/learn/stage1_pi_mono/packages/coding-agent/current_folder_learn.md`. Do not recurse into `ZS1-060`, `ZS1-056`, its children, child artifacts or receipts, the frozen files, or any target product code.
