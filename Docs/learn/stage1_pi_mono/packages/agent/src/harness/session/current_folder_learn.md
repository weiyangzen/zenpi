# ZS1-051 — packages/agent/src/harness/session

Scope: frozen Stage1 source subset, pi-mono bbb61e34aaf231639fdaaad1adbd757947034eac.
Accepted direct file: ZS1-013, context.ts, SHA256409078e31155a77bbfa1fff20ab81d55370fa379ad95c12933eecbf832efeaf1.
Its independent per-file report is ../../../../../files/packages/agent/src/harness/session/context.ts_learn.md within the learn tree; canonical path is Docs/learn/stage1_pi_mono/files/packages/agent/src/harness/session/context.ts_learn.md.
No direct subdirectory belongs to this frozen subset. Actual jsonl/ and testing/
subdirectories plus sibling implementation files are outside coverage; listing them
for navigation did not accept their storage, fork or conformance behavior.

The accepted file consumes an already selected ordered ancestry of Entry values.
Within this subset, buildSessionContext owns the output message array and calls
buildContextEntries once to select the last compaction plus following entries.
Each non-custom entry passes through sessionEntryToContextMessages; compaction
retainedTail and ordinary assistant messages use the same failure filter.
Custom projectors are looked up by customType and awaited in source order using
the caller's shared harness Context. Imported types and summary constructors are
context-only boundaries, not additional accepted files.

Cross-boundary invariants: caller must select the correct branch ancestry;
ordinary output order and compaction tail order remain stable; assistant error,
aborted and deferred records do not become successful context; an absent/nullish
projector adds nothing; projector rejection propagates. These functions neither
mutate the source path nor append a journal, run tools or request a model.
Cancellation, capability and budget enforcement of a custom projector belong to
its caller and implementation. This subset does not establish persistence or
restart semantics for the unreviewed storage files.

Target ownership is split: src/context.rs projects validated checkpoint/tail;
src/session.rs owns checkpoint journal reads/writes. Source assumes ancestry and
allows arbitrary custom projectors, whereas target103 validates source identities
and retains existing journal ownership. Production summary use104, branch ancestry
105/106 and bounded extension hooks115 remain separate unfinished obligations.
This directory synthesis derives only from accepted013; it does not replace a
file report or close parent harness054, which also needs compaction050.
