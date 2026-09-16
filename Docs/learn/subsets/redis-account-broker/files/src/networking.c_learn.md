# networking.c: Selected Client Windows

Source: redis/redis; exact revision/blob and sparse windows in the manifest.
Inspected: initialization, pre-allocation connection limits, incremental input,
main-thread execution handoff, pending writes and hard/soft output limits.

Observed: incomplete input stays buffered; unauthenticated lookahead is reduced.
I/O-thread parsing returns command execution to its owner. Pending replies get an
immediate write attempt, then write interest only when bytes remain. Soft-limit
timers reset below threshold. Client destruction can be deferred until safe.

Execute locally: check capacity before large allocations; authenticate one frame
before pipelines. Preserve partial cursors, bound per-client work, deduplicate
pending-write membership, remove drained write interest, and enter Closing before
retiring the slot. Never discard deltas and claim complete model output.

Tests: split/coalesced frames, full capacity, repeated threshold crossings, slow
readers, duplicate pending-write insertion, and close during callback. Broker
numeric limits are local policy, not copied database defaults.
