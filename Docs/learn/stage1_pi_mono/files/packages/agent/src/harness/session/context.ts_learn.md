# ZS1-013 / SRC-0156 — session/context.ts

Source: `packages/agent/src/harness/session/context.ts` at pi-mono `bbb61e34aaf231639fdaaad1adbd757947034eac`.
SHA256: `409078e31155a77bbfa1fff20ab81d55370fa379ad95c12933eecbf832efeaf1`.
Controller read the complete 64-line file. This report covers this file only.

`SessionContextBuildOptions` optionally maps custom entry types to projectors. `buildContextEntries` finds the latest compaction by reverse scan and returns that entry plus all subsequent path entries, or a shallow copy of the complete path when there is none. It assumes the caller already selected the proper ancestry; it does not validate a parent graph or cross-session identities.

`isContextMessage` keeps non-assistant messages and filters assistant error, aborted and deferred stop reasons. `sessionEntryToContextMessages` maps ordinary messages through that filter; compaction becomes its summary message followed by the filtered retained tail; a nonempty branch summary becomes a branch-summary message; empty branch summaries and custom entries yield no built-in messages. The summary message constructors are imported context-only helpers.

`buildSessionContext` defaults options, takes the projected entries, preserves order and dispatches custom projectors sequentially with the same harness context. Missing projectors and null/undefined results contribute nothing. A rejecting projector propagates its exception because this function contains no catch. Cancellation, time/byte budgets and capability revocation therefore require the supplied context and caller; the file does not itself check an abort signal. It performs no journal writes, tool execution or provider requests.

Target: `src/context.rs::restore_semantic_checkpoint` validates session/branch/source prefix and appends later records to the retained tail. Failed assistant metadata is filtered; legacy deterministic checkpoints remain a different format. `src/session.rs` loads the last recorded semantic entry through the same durable journal. ZS1-104 still must choose it in actual provider requests; ZS1-105/106 must supply selected branch ancestry, and ZS1-115 owns bounded custom context hooks. The target refuses invalid source/branch identity instead of trusting arbitrary path arrays.

Executable criterion: the new `stage1_context_checkpoint` target tests aborted assistant exclusion, exact retained IDs, mismatched branch/session/source rejection and new-process recovery with later turns. Error and deferred variants use the same explicit filter but need additional variant coverage before the product is accepted. No pi tests were executed for this report. Directory-level relationships remain ZS1-051's separate report.
