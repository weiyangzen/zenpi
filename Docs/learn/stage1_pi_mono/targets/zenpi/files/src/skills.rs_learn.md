# `src/skills.rs` per-file learn report (ZS1-078)

- Source: `src/skills.rs`
- Complete read range: byte `[0,40533)`; 1,173 lines
- Bytes: `40533`
- SHA-256: `a6d0a2f671febdef2e631ca53d2788ab869ab7b86db6c0cb1feaf24d128f83e8`

## Skill data and validation

`SkillScope`, `SkillMetadata`, `SkillManifest`, `SkillHooks`, `LoadedSkill`, `SkillBody`, `SkillCollision`, `SkillInvocation`, and `SkillError` define provenance, discovery metadata, explicit/model invocation, bounded TOML manifests, hooks, and loaded Markdown bodies. Manifest validation rejects invalid IDs/versions/instructions/tools/hooks, unknown fields, controls, NULs, and oversized values.

## Discovery and precedence

`SkillSet::load` and `load_with_paths` build a complete replacement before publication. Priority is explicit paths (later entries win), then project, then user; TOML wins over Markdown at the same scope. User/project roots and explicit `SKILL.md` paths are canonicalized, symlink roots are rejected, missing optional roots are skipped, explicit missing paths fail, and cancellation is checked between traversal units. Collisions are retained as visible metadata rather than silently hidden.

## Bounded recursive loading

The remainder of the file recursively loads manifests/Markdown, validates relative roots and depth/file/index limits, hashes source bytes, reads bodies only on explicit invocation, and exposes catalogue metadata without retaining Markdown bodies. Helpers enforce canonical ancestry, UTF-8/byte limits, cancellation, deterministic ordering, and source provenance. Skill reload returns a replacement set only after all checks pass, so a failed or cancelled scan cannot publish a partial catalogue.

## Mapping and limits

This maps to pi-mono's skills/resource loader while adding explicit user/project/explicit provenance, TOML hook manifests, collision visibility, cancellation, bounded recursive discovery, and source hashes used by the TUI palette. It does not prove command palette rendering, prompt routing, or provider execution; those are cross-file obligations.
