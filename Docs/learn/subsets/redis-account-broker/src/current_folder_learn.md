# Selected src Responsibilities

ae.c dispatches readiness/timers; networking.c handles client buffers and output;
aof.c tracks write/sync progress. Full startup, serverCron, authentication,
shutdown, replication and recovery are outside the selected windows.

The local design uses typed requests, generation-bound client slots, one state
publisher and owned helpers. It does not copy raw-pointer lifecycle, destructive
truncation or relaxed acknowledgement. Tests force disconnect, late completions,
slow readers and storage faults, not only an idle-server success path.
