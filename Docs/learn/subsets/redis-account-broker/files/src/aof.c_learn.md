# aof.c: Selected Flush Windows

Source: redis/redis; revision/blob in the manifest; flushAppendOnlyFile, 1399-1608.

Observed: buffered bytes, written offsets and synchronized offsets are distinct.
Sync policies differ in acknowledgement safety. Writes/fsync can stall; short
writes and sync failures require explicit handling. Truncation/error-exit paths
are not suitable local deletion policy. Rewrite/replay was not audited.

Execute locally: batch ordered records; return durable_seq to the state owner.
Publish send permission only through that sequence. Pending reservations already
consume capacity. Write uncertainty freezes affected dispatch and yields
CommitUncertain, not success or clean rejection. Verify group markers/checksums;
preserve torn segments and resume in a new segment without truncation. Batch
metadata, not token-by-token fsync; paid admission never uses relaxed durability.

Tests: short write, fsync error, slow writer, committed request without client ACK,
and crash after send authorization. Do not infer remote exactly-once.
