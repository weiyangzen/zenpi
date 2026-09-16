# `src/config.rs` per-file learn report (ZS1-076)

- Source: `src/config.rs`
- Complete read range: byte `[0,77782)`; 2,113 lines
- Bytes: `77782`
- SHA-256: `f5691bd3818dcaf1481a88b63cc4f3874bc6b007234059e26124b74046325d12`

## Configuration and credentials

`ConfigPaths`, `ConfigFile`, `ProviderProfile`, `ConfigOverrides`, `EffectiveConfig`, `AuthFile`, and `ConfigError` define user settings without putting secrets in TOML. Resolution is deterministic CLI > environment > workspace/user config > defaults. Auth is a separate JSON map with redacted Debug/status views, profile-specific key operations, and explicit revoke/pair semantics.

## Workspace/profile resolution

`resolve`, `resolve_default`, `load_workspace_config`, and `resolve_workspace` validate merged settings before an owner switch. Workspace overlays cannot escape their root, malformed project config fails preparation atomically, and legacy flat fields remain readable as the implicit default profile. Model/profile catalogues and `/doctor` summaries expose metadata and credential presence/source only.

## Codex pairing and diagnostics

`import_codex_from_home`, `pair_from_codex`, `import_codex`, `import_codex_profile`, `doctor`, `doctor_for_profile`, `list_profiles`, `use_profile`, `revoke`, `status`, `status_for_profile`, and `doctor_value` import compatible provider/model data while preserving unrelated keys and avoiding secret output. Pairing is byte-idempotent after the first successful write.

## BentoBox preference persistence

`load_layout_preferences`, `migrate_layout_preferences`, `save_layout_preferences`, `load_layout`, `save_layout`, `reset_layout`, and `reset_layout_profile` own validated, user-owned layout state separately from provider config. Reads reject malformed/oversized/stale/symlinked files without rewriting; writes serialize and validate before atomic replacement and preserve the previous valid snapshot on failure.

## Path and file safety

`scoped_path`, bounded config/auth readers, atomic writers, symlink checks, profile-name/optional-value validation, endpoint redaction, and exact-key API-key extraction keep untrusted paths and credentials fail-closed. The module does not start providers or alter TUI layout decisions; it supplies validated data to those owners.

## Mapping and limits

This file maps to pi-mono's config/auth/model registry and adds Zenpi's workspace overlay, profile routing, secret handles, BentoBox persistence, redacted diagnostics, and explicit atomic migration. TUI rendering, headless ownership, provider transport, and PTY behavior remain cross-file obligations and are not inferred here.
