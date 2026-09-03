# Learn Transform Validation Policy

1. Verify every manifest source path, byte count, and SHA-256 before reading a
   target artifact.
2. Require exactly one target artifact for each manifest row and require each
   artifact to repeat the source path and hash.
3. Preserve headings, code spans, links, and technical names in transform
   notes; do not copy source implementation.
4. Run `cargo fmt --all -- --check`, strict Clippy, Rust tests, the headless
   smoke test, the mode-boundary scan, the Blueprint validator, and the Rust
   Blueprint per-item LOC validator and the informational Rust source inventory.
5. A failed validator leaves the row provisional and records the failure; it
   cannot promote a worker result to the authoritative execution Blueprint.
6. Rollback removes only target artifacts and generated projections. Source
   repositories and accepted zenpi files remain untouched.
