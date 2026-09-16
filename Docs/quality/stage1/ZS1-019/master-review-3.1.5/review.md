# ZS1-019 source review

Source: packages/coding-agent/src/core/skills.ts
SHA256: 055dbfde974fd1951267dd6f9204b5d713ff0004e0991eb870fce9158c6e359a
Read bytes [0,14687); all 509 lines.

## Independent controller review, 3.1.5

The controller read all 509 source lines in order (1-240 and 241-509), all bytes [0,14687), and the complete worker report. This accepts source understanding only. The original target-baseline gap section is historical; it does not describe the current implemented worktree.

Source details independently checked: the root SKILL.md short-circuit occurs after loadSkillFromFile even if its metadata yields no skill; ordinary Markdown is considered only at a scan root, never at recursive levels. Filesystem readdir order is not sorted in this file, so same-scope first-winner order must not be claimed platform-deterministic. The ignore matcher is shared during traversal and rules are prefixed; escaped exclamation handling strips the escape and passes the resulting pattern to the ignore package. The file follows valid symlinks and has no visited-directory or depth guard. Cancellation and atomic publication are not provided here. Prompt generation escapes metadata and gives instructions for loading a file; it neither invokes the skill nor confines relative resource reads.

Target mapping was checked in src/skills.rs load_with_paths, metadata_index, load_body, read_metadata, discover_markdown, resolve_resource and read_skill_text/open_skill_file; plus the newly integrated ModelSkillTools and Agent registration/turn/provenance patch. Zenpi preserves TOML compatibility, uses explicit > project > user precedence, rejects malformed selected skills atomically, sorts directory entries, bounds traversal, and rejects symlinks within the selected canonical root. Standard SKILL.md is supported; arbitrary root .md files in the source are an explicit discovery difference, not implemented parity. Names/description/body validation is stricter than source warnings. Metadata discovery still reads files transiently; progressive disclosure means bodies are excluded from the model prompt until selected, not that discovery performs no body I/O.

The target's model load_skill/read_skill_resource tools are sequential because a resource read can depend on a preceding load in the same batch. A scoped turn guard clears loaded metadata after all terminal paths; the read path rechecks source hash, disable-model-invocation and relative containment. Existing immutable Blueprint grants do not authorize these cross-root resource tools. Tool results and source provenance are journaled, unlike the source metadata-only formatter. These are reviewed implementation mappings; product ZS1-113 still requires its own full host/UX/fault evidence and dependency closure.

The controller inspected source-test context lines 29-141 and the collision-test locator (393-429), not the full source test file and did not execute a source suite. Worker test observations remain attributed to the worker. No source-test pass count is inferred. Directory ZS1-056, sibling files, target whole-file ZS1-078 and product ZS1-113 remain independently unaccepted.
