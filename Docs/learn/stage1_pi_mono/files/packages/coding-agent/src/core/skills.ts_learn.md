# ZS1-019 — packages/coding-agent/src/core/skills.ts

Worker candidate: [_]. learn_mode: understand. Master semantic review remains required.

source_id: `SRC-0939`
source_path: `packages/coding-agent/src/core/skills.ts`
source_hash: `055dbfde974fd1951267dd6f9204b5d713ff0004e0991eb870fce9158c6e359a`
source_bytes: 14687
source_lines: 509
coverage: complete byte range [0,14687); lines 1–509, read in order, including comments and declarations; this source file contains no inline tests. No input exceeds 256 KiB. Hash matches the frozen blueprint input.
review_baseline: inherited dirty snapshot at HEAD `6f252a20c628e9b1ede14e2887acc04657c71d7c`; this report describes that input, not subsequent implementation.

## Complete behavior review

- Imports/constants/interfaces (1–101): synchronous filesystem discovery, ignore package, path normalization, YAML parser and SourceInfo owner. Skill holds name/description/filePath/baseDir/sourceInfo/disableModelInvocation, never body; LoadSkillsResult separates diagnostics from successful records. Name maximum 64 and description maximum 1024 are warning limits, not read bounds.
- `toPosixPath`, `prefixIgnorePattern`, `addIgnoreRules` (20–65) normalize separators and rebase directory rules; blank/comment lines disappear, negation/escaped exclamation/root slash are handled. All three .gitignore/.ignore/.fdignore files contribute; read errors are swallowed. One mutable matcher is passed through recursion, so rules accumulate with prefixed paths rather than a copied scope stack.
- `validateName`, `validateDescription` (103–142) diagnose length, allowed lower-case ASCII/digits/hyphens, edge/consecutive hyphens, missing/blank description. `createSkillSourceInfo` distinguishes user/project/local explicit scope and preserves arbitrary source identifiers.
- `loadSkillsFromDir` and `loadSkillsFromDirInternal` (162–279) return empty for absent roots. A valid file entry named SKILL.md is examined first and stops traversal even if metadata validation rejects that file. Otherwise root-level .md files are candidates and only subdirectory SKILL.md files recurse. Hidden entries and node_modules are skipped, nested ignore files apply, broken symlinks skip; valid file/directory symlinks are followed. No visited-directory set/depth cap exists here. Directory exceptions silently leave partial results.
- `loadSkillFromFile` (281–355) reads all UTF-8 text then parses frontmatter. Read error produces warning; parse errors on declared SKILL.md warn, unrelated markdown quietly skips. Missing description always skips; non-string name falls back to parent dirname. Invalid name or overlong description warns but still loads. Unknown frontmatter keys remain inert. Only boolean true disables model invocation.
- `formatSkillsForPrompt`, `escapeXml` (365–404) omit disabled skills, return empty when none visible, select read/bash instruction text and emit name/description/location XML with all five XML delimiters escaped. Relative resources are described as relative to baseDir; this function does not open or constrain referenced resources.
- `loadSkills`, nested `addSkills`/`isUnderPath`/`getSource` (406–509) resolve cwd/agent/explicit paths and tilde via resolvePath. Defaults load user then project: first name wins, later collision includes both winner and loser paths. Canonical duplicate files skip; only accepted winners enter realPathSet. Explicit files must end .md; missing/unsupported paths diagnose. With defaults disabled explicit paths inside known roots retain user/project scope. This standalone order differs from resource-loader's project-first package path order.

## Errors, cancellation, recovery and side effects

Reads only; no skill code, subprocess or shell evaluation. Discovery reads bodies transiently but retains only metadata. There is no cancellation token, size cap, atomic catalogue publication, persisted index, freshness check or body-loading method in this file. A new call re-reads disk; filesystem failures can be silent/partial. These are facts to improve under zenpi's bounded owner contract, not source guarantees.

## Source tests and behavior criteria

`packages/coding-agent/test/skills.test.ts` was inspected: valid/name mismatch/invalid name/long name/missing description/unknown fields/nesting/root preference/no frontmatter/bad YAML/multiline/disable flag tests cover the branches above; XML tests cover escaping and visibility; explicit path tests cover missing and tilde paths. Collision test simulates a map rather than invoking loadSkills, so it alone cannot verify actual precedence. `test/suite/regressions/2781-skill-collision-precedence.test.ts` and `test/resource-loader.test.ts` are additional integration evidence. Reproduction criteria: root SKILL.md excludes nested children; a disabled skill remains explicitly addressable while absent from prompt; two paths with same name produce deterministic winner and collision diagnostic. Source suites are context evidence, not claimed executed by this worker.

## Zenpi mapping and gaps

Existing `src/skills.rs` loads one-level skill.toml, validates manifests and uses project overrides plus ordered static hooks. `src/core.rs` calls SkillSet::load and injects effective_instructions. There is no standard Markdown metadata/body split in the baseline; `src/slash.rs` and `src/slash_actions.rs` have no skill invocation variant/owner. ZS1-113 must preserve TOML/hook/public API behavior while adding bounded recursive Markdown discovery, explicit source precedence/conflict diagnostics, disabled-model filtering, fresh body reads and resource containment. Pi's warning-only invalid names, silent read failures and unrestricted symlinks are intentional non-parity candidates requiring documented decisions. Directory acceptance remains with ZS1-056; this file report cannot accept siblings.

## Independent controller review, 3.1.5

The controller read all 509 source lines in order (1-240 and 241-509), all bytes [0,14687), and the complete worker report. This accepts source understanding only. The original target-baseline gap section is historical; it does not describe the current implemented worktree.

Source details independently checked: the root SKILL.md short-circuit occurs after loadSkillFromFile even if its metadata yields no skill; ordinary Markdown is considered only at a scan root, never at recursive levels. Filesystem readdir order is not sorted in this file, so same-scope first-winner order must not be claimed platform-deterministic. The ignore matcher is shared during traversal and rules are prefixed; escaped exclamation handling strips the escape and passes the resulting pattern to the ignore package. The file follows valid symlinks and has no visited-directory or depth guard. Cancellation and atomic publication are not provided here. Prompt generation escapes metadata and gives instructions for loading a file; it neither invokes the skill nor confines relative resource reads.

Target mapping was checked in src/skills.rs load_with_paths, metadata_index, load_body, read_metadata, discover_markdown, resolve_resource and read_skill_text/open_skill_file; plus the newly integrated ModelSkillTools and Agent registration/turn/provenance patch. Zenpi preserves TOML compatibility, uses explicit > project > user precedence, rejects malformed selected skills atomically, sorts directory entries, bounds traversal, and rejects symlinks within the selected canonical root. Standard SKILL.md is supported; arbitrary root .md files in the source are an explicit discovery difference, not implemented parity. Names/description/body validation is stricter than source warnings. Metadata discovery still reads files transiently; progressive disclosure means bodies are excluded from the model prompt until selected, not that discovery performs no body I/O.

The target's model load_skill/read_skill_resource tools are sequential because a resource read can depend on a preceding load in the same batch. A scoped turn guard clears loaded metadata after all terminal paths; the read path rechecks source hash, disable-model-invocation and relative containment. Existing immutable Blueprint grants do not authorize these cross-root resource tools. Tool results and source provenance are journaled, unlike the source metadata-only formatter. These are reviewed implementation mappings; product ZS1-113 still requires its own full host/UX/fault evidence and dependency closure.

The controller inspected source-test context lines 29-141 and the collision-test locator (393-429), not the full source test file and did not execute a source suite. Worker test observations remain attributed to the worker. No source-test pass count is inferred. Directory ZS1-056, sibling files, target whole-file ZS1-078 and product ZS1-113 remain independently unaccepted.
