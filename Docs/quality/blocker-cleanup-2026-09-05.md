# Blocker cleanup, 2026-09-05

Scope: Master integration repair for CF-305, CF-307, CF-503, CF-505, and
CF-705. This receipt does not accept any complete Blueprint row.

## Reproduced failures and repairs

| Blocker | Diagnosis | Repair and evidence |
|---|---|---|
| Installed resume smoke returned an empty status summary | It reused the previous process's status request ID, which deliberately replays a durable earlier response. The session had recovered two turns. | Separate request IDs for fresh queries; require successful resume, status, and shutdown; compare both recovered summaries. `resumed_session_replays_old_status_ids_but_fresh_ids_inspect_current_history` independently proves old-ID replay versus fresh-ID inspection with no provider dispatch. |
| TUI completion regression | Adding `/mailbox` made `/m` ambiguous. Forcing the first catalogue entry merely hid an obsolete test expectation. | Retain conservative completion. Tests prove `/m` is unchanged, `/ma` completes mailbox, and `/mo` completes the common model stem; ordinary prompts remain untouched. |
| TUI checkpoint claimed headless reconnect capability | A journal inspection does not own the transport WAL or its durable ACK state. | Journal-only inspection reports no transport capability; the headless checkpoint owner advertises support only with its replay journal. TUI and JSONL tests cover the distinction. |
| Interrupted mailbox integration | A failed worker left export/retirement code and tests awaiting integration. Export failure could delete a preexisting destination sidecar; copying reopened the source path outside its locked descriptor. | Preserve existing destination files and symlinks. Copy bounded bytes from the locked source descriptor. Negative tests assert no destination evidence or link target is removed. |
| Mailbox retirement could resurrect an empty queue | Removing the queue erased its deduplication history without fencing old handles. An active session alias was checked by path only. | Persist a retirement marker before unlink, reject matching active session identity, check retirement before and after acquiring the mailbox lock, and finish an interrupted marker-before-unlink operation on explicit retry. Tests cover late sends, aliases, confirmation, and crash reconciliation. |
| Standalone headless smoke still expected placeholder owners | Its assertions predated the checkpoint WAL and real shell approval owner. | Verify actual checkpoint/ACK capability, missing-recipient errors, empty-bang help, and shutdown cancellation of an unapproved shell. Require no marker file, dispatched operation, or model turn rather than an obsolete `owner_required` response. |

The existing recovery command, provider request-identity, and credential
redaction integration changes remain in the checkout and are included in
regression validation; they are not automatically accepted as complete rows.

## Execution and process cleanup

- Three attempted implementation agents ended with upstream HTTP 502. They
  are terminal failures, not active worker capacity. Their partial files were
  preserved and inspected, not reset.
- Those collaboration agents were not the Spec's required isolated tmux TUI
  workers and cannot count as skill-compliant execution evidence. No further
  fallback worker or repeated model submission was launched in this repair.
- Confirmed both historical CF-305/CF-306 task-local tmux servers absent before
  unlinking their stale socket files. Claim cards and task evidence remain.
- Identified one orphaned `zenpi` TUI by its exact PID/start time, repository
  binary/cwd, temporary fixture session, and completed local-shell journal.
  TERM did not stop it; a second identity check preceded KILL. Exit was
  verified. No unrelated Codex process or cron entry was changed.
- The crontab has no zenpi execution controller. A controller with minimal
  selected-provider bootstrap, atomic request/turn leases, isolated owned
  paths, and authenticated goal liveness remains required before claiming
  maximum-concurrency execution. Local test success does not repair the
  external provider service or create such a controller.

## Validation

| Command | Terminal result |
|---|---|
| `cargo test --all-targets --all-features --locked -- --test-threads=1` | Passed, 422 tests; includes 39 headless protocol, 32 session recovery, and 14 TUI resize cases. |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | Passed. |
| `cargo fmt --all -- --check` | Passed. |
| `python3 tools/user_smoke.py` | Passed after restoring both resume/status assertions with fresh request IDs. Covers isolated production and fixture installs plus actual PTY and local provider-fixture workflows; no live external provider acceptance is implied. |
| `bash tools/headless_smoke.sh --bin target/release/zenpi` | Passed, including real session-control owners and denied-shell absence of effects. Binary is the explicit dev-fixtures build from the installed smoke. |
| `python3 tools/validate_blueprint.py` | Passed, 71 items, 15 unclaimed, 0 self-tested, 56 historical master-accepted. |
| `python3 tools/validate_blueprint_v2.py` | Passed, 46 rows, maximum estimated LOC 2800. |
| `git diff --check` | Passed. |

Remaining product contracts include general worker command/network isolation,
production lease and budget wiring, actual live recipient dispatch, reconnect
sidecar migration/retention, and cross-platform acceptance. All 15 reopened or
new authoritative checklist rows remain `[ ]`.
