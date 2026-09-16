# ae.c: Selected Event-Loop Windows

Source: redis/redis; exact revision/blob and windows in the manifest.
Symbols: aeProcessEvents, aeMain and adjacent helpers, lines 365-516.

Observed: before-sleep work precedes OS polling; earliest timers bound sleep.
Returned readiness batches drive callbacks. Read callbacks may change event
registration, requiring a refreshed entry before further callbacks. Barrier
ordering can delay replies until preceding work; it does not by itself prove
our budget durability.

Execute locally: process only ready descriptors, bounded pending work and timer
deadlines. Identify clients by slot plus generation, not a bare FD. Revalidate
lifecycle after callbacks and worker completions. Only durable admission can
produce send-ready work. No blocking plugin, DNS or fsync in the reactor.

Tests: close/reuse an FD before a late completion; the new client receives no old
event. Delay journal completion; control remains responsive, paid sends stay zero.
A deep pipeline must not starve another client's cancel.
