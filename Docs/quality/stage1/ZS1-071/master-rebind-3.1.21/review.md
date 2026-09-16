# ZS1-071 — independent requirement-delta rebind 3.1.21

The controller read the complete 3.1.20→3.1.21 delta. This item was checked individually: its full Item record, state, validators, dependencies and owned paths are unchanged. Both prior receipt artifact chains were verified byte-for-byte; their original receipts are archived under `.ops/stage1_execution/registration-3.1.21/activation/before/`. Existing semantic reviews are reused with those exact hashes, not represented as a new full-source reading or current runtime rerun.

Item identity: `5c40ba4284705b08ec8f5c2c933c1103b43a62d4c5e7db9513627a0d1ba7956d`.
Frozen file/directory identity: `{"artifact": "Docs/learn/stage1_pi_mono/targets/zenpi/files/src/runtime.rs_learn.md", "declared_bytes": 28121, "item_id": "ZS1-071", "path": "src/runtime.rs", "scope": "target", "sha256": "2183b622d96a8fdc084784af8f537fa09a4e1703b5ff70de3bc459d29fb05315", "source_id": "ZS1-071"}`.
Original worker receipt: `2ff7edef136c442a877d70768a20778ce1fa46725d1f08affd64e922a28b4b85`.
Original master receipt: `3e8b0e80bed0764521771f4fe72a3058ff470aaff3902132bf28f2539c8c2aa6`.

The extension registers vendor mio.rs as099, headless_project_workspace.rs as133 and their seven directory containers400–406. The original56 frozen file records remain identical. Only open091/123/129/131/132 gain scope or dependencies. Every prior behavior, real-entrypoint requirement, release budget, failed gate and BentoBox obligation remains in force. The table-spacing correction merely keeps099/133 inside its Markdown table.

Old requirement: `8d525b351d066ce9b0487a485337647d47233275782e4f4d154423a623fe1645`; new: `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`.
Old baseline collection: `9b0c63151bf3a04582fd35912f9c14394879c42176337e2dbf9ca49df9cb56d5`; extended collection: `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`. The baseline pointer changes solely because two unchanged pre-fix files are added; it does not claim this historical execution used the extended manifest or tested their fixes.

For001 only, the historical worker integrated-tree hash predates vendor integration and is preserved, not re-certified as a current worker build. Its original artifacts and command receipts were checked; the current master receipt independently passes current integrated-input verification. All other retained worker and master receipts passed their existing structural/hash checks individually.

Decision: retain ZS1-071 acceptance for its unchanged obligation only. Do not accept099,133, any new directory, any open product item, a new platform or the whole stage. Prior semantic reviews, read ranges, commands, build identities and their limitations remain in the appended receipt chain.
