# zenpi root directory integration learn report (ZS1-091)

- Folder: repository root (`.`)
- Scope: accepted target integrations ZS1-090, ZS1-093, ZS1-097, ZS1-098, ZS1-405, ZS1-406
- Integration layer: L2; child directory/file receipts remain authoritative

## Root mapping

The root assembles the Rust crate (`src`, `tests`, `vendor`), Cargo manifests/lockfile, CI/release workflows, and documentation/learn evidence. `Cargo.toml` and `Cargo.lock` define the build/dependency identity consumed by src owners; `src` is the product owner graph; tests exercise the public owner boundaries; vendor supplies the pinned terminal dependency; workflows describe CI and packaging rather than runtime behavior.

The accepted child directories establish the intended two-tree boundary: production source and its tests/vendor support are mapped to the current worktree, while blueprint/learn files and receipts live under `Docs`. Root integration does not treat generated evidence as product code and does not infer runtime success from a manifest or workflow declaration.

## Cross-root invariants

Project directory selection enters through TUI/headless owner code and remains canonical across session journals, tool roots, and project tabs. Cargo lock identity must match the source graph; test and smoke artifacts must use the same current source revision; CI/release permissions and artifact lifecycles remain separate from Zenpi session ownership. The root contains no worker supervisor beyond the explicit host owners already covered by src and headless reports.

## Limits

This report integrates accepted child mappings and root-level data flow. It does not reaccept child files, claim external GitHub Actions execution, or modify the protected blueprint gantt. No worker was started.
