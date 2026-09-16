# `src/session.rs` per-file learn report (ZS1-074)

- Source: `src/session.rs`
- Complete read range: byte `[0,176828)`; 4,737 lines
- Bytes: `176828`
- SHA-256: `8fb3ff2c387b13d34d8bbe4533bb2b0a81b5e546f5e03216b74b5cf2c61c9a2b`

## Journal and recovery core

`SessionHeader`, `SessionRecord`, `SessionStore`, `RecoveryWarning`, operation markers/outcomes, and `SessionError` define an append-only JSONL journal. `open`, `open_existing`, `open_existing_writable`, `open_in_workspace`, and `open_with_options` validate bounded bytes, ownership, permissions, symlinks, header identity, and complete records. A malformed trailing line is recoverable with a warning; valid records remain appendable without rewriting history. Sequence numbers, turns, handoffs, runtime intents, events, semantic checkpoints, and tree projections are retained as independent durable records.

## Tree, branching, and semantic state

`enable_tree`, `select_tree_leaf`, `annotate_tree_entry`, `tree_ancestry_turns`, `fork_at_tree_leaf`, `selected_tree_turns`, `selected_tree_branch`, and semantic checkpoint accessors provide explicit branch selection. They preserve parent/leaf identity and reject stale or incompatible writers. Handoff and runtime-intent appenders keep control-plane records separate from transcript turns.

## Operation/reconnect ownership

`begin_operation*`, `finish_operation`, `mark_interrupted_operations`, `operation_recovery`, `durable_operation_outcome`, and `decide_operation_recovery` ensure an absent terminal record is never treated as success. `ReconnectJournal` owns a private transport WAL and digest, separate from transcript sequence space; its inode guard prevents two writers from sharing one session transport. Append locks, exclusive creation, and fsync boundaries make concurrent append/fork behavior explicit.

## Session discovery and lifecycle

`list_sessions`, `list_sessions_with_fallback`, `inspect_session`, `import_session`, `export_to`, `fork_to`, `session_catalog`, `resume_last_session`, `execute_session_lifecycle`, `set_session_archived`, `retire_session_mailbox`, and garbage-collection functions provide bounded discovery and explicit archive/import/retire actions. GC is fail-closed for symlinks, unexpected files, active sessions, candidate overflow, dirty journals, and receipts that cannot describe every deletion.

## Mailbox and live owner

`LiveSessionRegistry` registers, heartbeats, unregisters, and claims owners by `(session_id, owner_epoch)`. `SessionMailbox` and its message/action/page records use the same inode lock, reload bounded state before every mutation, append one transition, and never implicitly retry a claimed message. This is an in-process owner boundary, not a daemon or scheduler; filenames do not establish liveness.

## Mapping and limits

The module is Zenpi's durable equivalent of pi-mono session manager/history, with additional branch/tree, semantic checkpoint, operation recovery, reconnect WAL, project lifecycle, mailbox, and owner-epoch semantics. It does not prove TUI rendering, headless routing, provider behavior, or PTY interaction; those require their respective files and host evidence. The report covers the complete source and records these cross-file limits instead of inferring them.
