# Transform Evidence Records

The transform route is represented by bounded b3 records rather than a hidden
controller. These examples are illustrative wire values accepted by the Rust
validators; they do not start workers or execute commands.

```json
{"estimate_id":"est-zp","task_ref":"ZP-101","estimated_parameters":{"tokens":2000},"hard_caps":{"tokens":4000,"wall_clock_ms":30000,"attempts":1,"disk_bytes":1048576},"rationale":"small foundation"}
{"route_id":"route-zp","parent_ref":"ZP-101","route_class":"local","runner":"cargo","validator_strength":"strict"}
{"log_id":"log-zp","grain":"task","target_ref":"ZP-101","instrument_ref":"validator","observed_effect":"helped","target_movement":"accepted","evidence_refs":["e-zp"],"master_state":"self_tested"}
```

The corresponding `EvidenceRecord` names changed paths and validation-command
labels. A host may serialize these records into its own handoff; zenpi keeps
them inert and leaves scheduling, reward, and Master authority to the host.
