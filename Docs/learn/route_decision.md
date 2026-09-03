# Learn Route Decision

```yaml
route_id: ROUTE-ZP-001
learn_mode: transform
source_subset: pi-mode-migration, pi-headless, pi-session, pi-tui, b3-contract, rust-quality
route_class: high_reasoning_contract_transform
runner: local_read_only_analysis
validator_strength: strong
context_strategy: locked_manifest_plus_source_hash
why_not_cheaper: mode reduction, persistence compatibility, and ownership contracts require semantic review
why_not_more_expensive: the source scope is explicit and all outputs have deterministic validators
fallback_route: reopen only the affected manifest row and preserve accepted artifacts
```

Automatic subset expansion, nested workers, network retrieval, and translation
routes are not selected. The target language for code/process artifacts is
English; the user-facing README separately carries Chinese and Japanese.
