# ZS1-084 current master review

`src/headless.rs` was read in full in two contiguous chunks, byte ranges `[0,262144)` and `[262144,370207)`, 9,334 lines. Current SHA-256 is `bf252fec95a859b26f9aea402c918e9efbf6c2c55d3e24b403831f86d80ec06e`. The report covers bounded JSONL framing, durable request/event/terminal replay, async mailbox and steer limits, safe workspace/domain projections, session/recovery/GC control-plane actions, blueprint/goal/learn/evidence dispatch, and the host-only side-effect boundary.
