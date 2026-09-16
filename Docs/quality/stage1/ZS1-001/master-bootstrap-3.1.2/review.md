# ZS1-001 master review

Decision: accepted for the bootstrap/validator item only. Reviewer: controller.
The controller read the candidate validator and its tests, reviewed the three
repair diffs, and reran the four prescribed commands in the authoritative checkout.
Original candidate author: independent session 01a08be3-4678-75a0-9369-fbf8dbddb1ed.
Candidate commits: ed20b2d11529ae9f2c38f2dc3a5bec6bd2cf90cf and
34dd6bc0bddf7456e0a6cfd6f36455f0dce75212. No worker-authored command is executed
from a receipt; the command runner uses a fixed controller list.

The real master bootstrap froze 21 pi files/16 directories, 25 target files/4
directories and 10 Codex reference files/5 directories. The existing user dirty
baseline was hash-verified before any target implementation was replaced.
Independent manifests, file/folder indexes, continuous oversized byte chunks,
route decision, three-session claim ledger and current todo exist and pass G-STAGE.
The selector binds requirement 3.1.2 and the original baseline, and was created
last by the explicit master bootstrap. No old accepted state was inherited.

46 executed tests reject missing files, skipped directory ancestors, scope/tree
substitution, wrong source hashes or actual source HEAD, malformed states, cycles,
dual authority, forged acceptance, missing/zero per-target test execution, stale
claims and mutated receipt artifacts. Cursor-only changes preserve requirement
digest; behavioral changes do not. Three repaired checks now include root build
inputs such as Cargo, enforce actual frozen source HEAD and accept historical
integration commits only if ancestors with unchanged relevant current inputs.

Controller also repaired temporary test fixtures to normalize acceptance marks
to [ ] before bootstrap, without changing the live checklist. The prior 001
receipt was archived because test code and the requirement digest changed.
This replay accepts the current bytes and current 3.1.2 scope.

Semantic/manual truth remains explicitly outside machine proof: the checker
verifies receipt structure and actual artifact bytes, not the truthfulness of
arbitrary prose. This review closes only ZS1-001. Product features and each source
file/directory require their own review and evidence. No Rust feature is accepted
by these structural gates. Rollback restores the previous selector snapshot and
removes only this run's new scaffold, preserving all existing product files.
