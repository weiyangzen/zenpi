# ZS1-088 — src/lib.rs

Target-file understanding accepted by controller under 3.1.17. learn_mode: understand. The following worker capture used the earlier authority; current acceptance and limitations are recorded below. Authority3.1.16, requirement digest `c0262492bc6e3b4d5f56b35bedc7c1d1658854df11c4a975eb612a514cceef8f`.

Original blueprint baseline: source_path `src/lib.rs`,592bytes/31lines, SHA256 `2b429ff43bfba672fea949a9fb51ee6eda41a2975589dbc852a50bef2e13d575`. Retrieved read-only via git show6f252a20c628e9b1ede14e2887acc04657c71d7c:src/lib.rs and exact hash checked. Current main snapshot:844bytes/42lines, SHA256 `3d6a3b6902330ad2bdc4f4f6342730b1590cd9dac90d9a1ddf3e743df1bb5ff8`. Both files were read completely, baseline1–31 and current1–42; no section was skipped because the file is small. Exact byte ranges/capture time are in worker-owner-review/input-manifest.json. No prior local or main report exists, so this is a new report preserving both source baselines explicitly.

Temporal boundary: the original worker report repeated a then-available controller status describing122 as scratch-only. That status is stale and is not relied on for current product state. The controller has since integrated122/123 and subsequent identity fixes. Their integration and production evidence remain separate from088, and module declarations do not establish any product acceptance.

## Every line and module declaration

Line1 is the crate-level documentation comment describing a shared library and TUI/headless binary modes; line2 is blank. Lines3–42 are exactly40 unconditional public module declarations. The table below covers every declaration, its resolved source and whether it was in the original29-module baseline. Each declaration resolved to exactly one existing module entry file in the read-only main tree; full resolved-source hashes/bytes are recorded only as lookup context, not a full review of those sibling files.

| Current line | Public module | Resolved entry | Original baseline |
| --- | --- | --- | --- |
| 3 | `approval` | `src/approval.rs` | present |
| 4 | `b3` | `src/b3.rs` | present |
| 5 | `backend` | `src/backend.rs` | present |
| 6 | `config` | `src/config.rs` | present |
| 7 | `context` | `src/context.rs` | present |
| 8 | `core` | `src/core.rs` | present |
| 9 | `diagnostics` | `src/diagnostics.rs` | present |
| 10 | `directory_picker` | `src/directory_picker.rs` | new |
| 11 | `domain_execution` | `src/domain_execution.rs` | present |
| 12 | `domain_store` | `src/domain_store.rs` | present |
| 13 | `domains` | `src/domains.rs` | present |
| 14 | `error` | `src/error.rs` | present |
| 15 | `extension_runtime` | `src/extension_runtime.rs` | new |
| 16 | `extensions` | `src/extensions.rs` | present |
| 17 | `governance` | `src/governance.rs` | present |
| 18 | `headless` | `src/headless.rs` | present |
| 19 | `input_queue` | `src/input_queue.rs` | new |
| 20 | `layout` | `src/layout.rs` | present |
| 21 | `persona` | `src/persona.rs` | present |
| 22 | `project_workspace` | `src/project_workspace.rs` | new |
| 23 | `prompt_templates` | `src/prompt_templates.rs` | new |
| 24 | `protocol` | `src/protocol.rs` | present |
| 25 | `providers` | `src/providers/mod.rs` | new |
| 26 | `render` | `src/render.rs` | present |
| 27 | `resource_loader` | `src/resource_loader.rs` | new |
| 28 | `resources` | `src/resources.rs` | present |
| 29 | `runtime` | `src/runtime.rs` | present |
| 30 | `runtime_intent` | `src/runtime_intent.rs` | present |
| 31 | `search` | `src/search.rs` | new |
| 32 | `security` | `src/security.rs` | present |
| 33 | `session` | `src/session.rs` | present |
| 34 | `session_tree` | `src/session_tree.rs` | new |
| 35 | `skills` | `src/skills.rs` | present |
| 36 | `slash` | `src/slash.rs` | present |
| 37 | `slash_actions` | `src/slash_actions.rs` | present |
| 38 | `tool_output` | `src/tool_output.rs` | new |
| 39 | `tool_runtime` | `src/tool_runtime.rs` | new |
| 40 | `tools` | `src/tools.rs` | present |
| 41 | `tui` | `src/tui.rs` | present |
| 42 | `view_model` | `src/view_model.rs` | present |

The original29 declarations are all retained in their relative order. The11 additions are directory_picker, extension_runtime, input_queue, project_workspace, prompt_templates, providers, resource_loader, search, session_tree, tool_output and tool_runtime. providers resolves to src/providers/mod.rs; the other39 entries resolve to src/NAME.rs. This is an actual path-resolution inventory, not an assertion that all transitive files or conditional targets compile or have passed owner review. Original/current source snapshots remain separate immutable evidence.

## Public boundary and call mapping

Each `pub mod` exposes that module namespace through the zenpi crate. It does not make private or pub(crate) members externally public, re-export every item at the crate root, invoke an initializer or determine runtime callback order. Every declaration is unconditional at this file level: there are no cfg/feature gates, path overrides, inline module bodies, macros, use/re-export statements, root functions/types/constants, mutable state, unsafe blocks, tests or runtime error branches. Platform/feature behavior inside modules must be reviewed at their own boundaries, not inferred from this flat index.

The inspected Cargo.toml names the package zenpi and does not override library/binary target paths with a custom lib/bin section. The fully read11-line src/main.rs imports ExitCode and calls zenpi::core::run, converting Ok to success and Err to a printed error/failure exit. Thus core is an actual public integration entry used by the binary, while integration tests use paths such as zenpi::extensions and zenpi::extension_runtime. The line1 description is a high-level mode summary; it does not enumerate auxiliary CLI commands handled by core (extension management was separately observed in080), and must not be interpreted as enforcing a two-command CLI restriction.

Added module declarations make the corresponding implementations available to compile and reference: resource_loader/prompt_templates support resource admission; extension_runtime is the115 runtime entry; input_queue/session_tree/tool_output provide typed state/output owners; providers/tool_runtime support provider/tool surfaces; directory_picker/project_workspace/search are additional host/workspace surfaces. These names are navigation context only. Their actual admission, cancellation, ownership, persistence and policy behavior remains the responsibility of each file's independent report. No sibling or parent directory gets accepted because its module appears here. Changes inside116 domain/core modules can integrate without another lib.rs declaration, so an unchanged index is not evidence of an unchanged product.

## Error, cancellation, recovery and validation scope

There is no executable branch or side effect in this file: it does not open resources, spawn processes, route requests, create sessions, register tools, cancel work or recover journals. Missing/ambiguous declared entry paths or private/public API mismatches are compile-time integration issues; the path inventory checks only existence/uniqueness, not full type checking. Runtime failure/cancel/restart behavior lives in exported modules and must not be claimed as handled by the index. Declaration order is not a lifecycle ordering guarantee.

The40-entry inventory and original-to-current11-addition comparison are newly executed structural/integrity checks, not behavior tests. Existing C115 native-fixture package records a real17-case production/library run and strict clippy on that task's local tree; main separately reports integrating it and passing17 in the current116 tree. Neither report is relabeled an088 full-feature/cross-platform compile. The exact current lib.rs is also identical to the earlier079 captured-main compile-context lib.rs; that actual snapshot probe compiled a real main library and ran two slash parser counterexamples, with its support-file identities retained. This supplies historical current-index compilation context, not a rerun or semantic coverage of40 modules. No new Cargo build/test is needed for this read-only declaration review and no runtime case is newly claimed.

Source pi-mono's package exports/types/runner/AgentSession use different packaging and runtime ownership surfaces. There is no one-to-one claim that Rust's library index implements those source behaviors. Source022/023/028 reports remain separate; module connection is only the prerequisite for their mapped target owners. ZS1-090 parent integration depends on independent file acceptance and is not completed by088.

G-FILE is main's semantic gate. G-STAGE runs against read-only main with Python bytecode writes disabled; structural validity or missing master receipt is not semantic acceptance. The verifier checks complete current/baseline byte ranges,40 unique declarations, original retention/11 additions, actual module entry resolution and immutable evidence hashes. Current dependency paths are recorded at capture; later changes to sibling contents do not change which lib.rs bytes were reviewed. Candidate changes only this new report and088 evidence. Rollback removes them, with no source/index/Cargo/CI modification or acceptance promotion.


## Controller complete-file acceptance, authority 3.1.17

The controller independently read the full original31-line/592-byte library index, full current42-line/844-byte index, complete74-line worker report, full11-line main.rs and46-line Cargo.toml context. It verified all15 candidate/base/per-file-patch sets, applied the entire scoped patch in an isolated repository, checked all resulting bytes, reversed it and checked that all15 newly created paths disappeared. All10 historical references were independently hashed and retained. Those historical references establish only their stated capture context, not fresh behavioral execution.

Current line1 is a crate doc comment, line2 is blank, and every remaining line is one unconditional pub mod declaration. A separate current inventory resolves all40 declarations uniquely: providers uses src/providers/mod.rs and the other39 use src/NAME.rs. All29 original declarations remain in their relative order; the11 additions exactly match the worker table. The current index hash is unchanged from the reviewed worker snapshot. These declarations expose module namespaces, preserve item visibility and contain no executable bodies, initialization order, feature branches or cancellation/recovery paths. Full semantic review of sibling files and the parent src directory remains independent.

The controller's actual default-feature cargo build --locked succeeded on the current42-line index; after the queue identity integration the all-target/all-feature strict Clippy check also succeeded. The complete library test command compiled successfully but a separate extension pipe cancellation fixture failed to become ready twice; those failures are retained and under investigation. No whole-library runtime success, platform-independent success or product115 acceptance is claimed here. For this declaration-only file, full-byte review, unique module resolution and actual compilation provide the applicable evidence; inventing runtime cases for pub mod statements would not improve it.

Authority3.1.17 adds129 kill/yank and the related122/128 contracts, while088's baseline, owner scope, validators and dependencies remain unchanged. The standard receipt's source_hash and read_ranges bind the frozen592-byte baseline; current_source_hash/current_read_ranges separately bind the844-byte current implementation. Neither identity replaces the other. This accepts only088 whole-file understanding and leaves090, all sibling owners and all product gates open.
