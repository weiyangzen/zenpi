# `src/slash_actions.rs` per-file learn report (ZS1-087)

- Source: `src/slash_actions.rs`
- Complete read range: byte `[0,76441)`; 1,998 lines
- Bytes: `76441`
- SHA-256: `413a1ddba252ef2d435459af257cefa0cf45ef4c0b6ff9007b0b1454d69a8e9d`

## Shared slash action boundary

This module is the host-facing owner for slash actions used by TUI and headless dispatch. `models_value`, resource controls, compact, tree/output controls, and dynamic resource routing share the active Agent owner and preserve cancellation and project-aware workspace semantics. Selected skills/templates are reread and hash-checked at turn admission rather than exposing bodies through classification.

## Bounded diff inspection

`diff_value_at` and its cancellation-aware variant canonicalize a workspace, reject unsafe paths and symlinks, run bounded Git status/diff commands in controlled process groups, support empty-tree repositories, binary-file classification, explicit untracked-file diffs, redaction, truncation, and a 30-second deadline. Status and patch output remain within terminal/JSONL limits and non-difference Git failures are surfaced as typed errors.

## Attachment staging

`attach_value`, `attachment_for_path`, size/digest helpers, and MIME classification validate regular workspace files, enforce `MAX_ATTACHMENT_BYTES`, reject traversal and symlink aliases, preserve native relative path spelling, and stage only a bounded reference. The Agent reopens and hashes the file at admission; this module never writes file contents to the session journal.

## Integration boundary

The action layer provides deterministic, testable projections for the interactive hosts. It does not call providers, bypass Agent authorization, or own the TUI layout; those remain core, backend, and renderer responsibilities.
