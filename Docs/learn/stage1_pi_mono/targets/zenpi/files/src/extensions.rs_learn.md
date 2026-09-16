# `src/extensions.rs` per-file learn report (ZS1-080)

- Source: `src/extensions.rs`
- Complete read range: byte `[0,20742)`; 626 lines
- Bytes: `20742`
- SHA-256: `38a242eff3ec911f4560e3dea3868b742d511205ff0ccab1b622962b41e420ed`

## Extension manifests and catalog

`ExtensionManifest`, `ExtensionPermissions`, `ExtensionToolManifest`, `ExtensionSummary`, `ExtensionCatalog`, and `ExtensionError` define bounded local-process extensions. Manifest validation checks IDs, versions, hook timeout, relative executable paths, tool definitions, permissions, and unknown/oversized data. `ExtensionCatalog::load` reads a complete catalog before publication, keeps summaries separate from executable details, rejects symlink/path escapes, and exposes only validated metadata.

## Tool registration and lifecycle

`register_tools` converts enabled extension tools into the existing `Tool` contract and routes invocation through the host-owned workspace/policy context. `LocalProcessTool` uses bounded cancellable process execution and preserves typed failures. `install`, `remove`, `upgrade`, and `set_disabled` copy/validate trees atomically enough to avoid publishing partial manifests, preserve disabled state, and enforce root ownership. `copy_tree` is bounded and rejects invalid relative components.

## Capability broker

`CapabilityScope`, `CapabilityHandle`, and `CapabilityBroker` issue opaque, time-bounded subject-bound handles. `authorize` checks ID, subject, scope, and expiry; `revoke` and `revoke_subject` immediately invalidate capabilities. Handles do not grant permissions by themselves; the extension/tool host must still apply the compiled side-effect policy.

## Mapping and limits

This maps to pi-mono's extension runner/tool registration while adding Zenpi local-process lifecycle, explicit permissions, bounded installation, disabled-state persistence, and revocable capability handles. TUI rendering, provider calls, headless routing, and BentoBox layout remain outside this file's proof.
