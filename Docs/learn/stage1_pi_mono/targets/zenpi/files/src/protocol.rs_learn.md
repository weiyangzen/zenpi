# `src/protocol.rs` per-file learn report (ZS1-072)

- Source: `src/protocol.rs`
- Complete read range: byte `[0,45517)`; 1,309 lines
- Bytes: `45517`
- SHA-256: `90a5644d5a818a8051172b42b302614fb3263ebac737ce9dbf4a842791370d27`

## Contract and framing

The module owns the versioned LF-delimited JSONL contract for headless Zenpi. `StdioRequest` decodes optional correlation/version/project/session/cwd fields and turns into an owned `Command`; `parse_line` accepts one optional CRLF terminator, rejects embedded framing breaks and oversized frames, and never emits diagnostics on stdout. `encode_line` guarantees one JSON value plus one LF.

## Commands and ownership boundaries

`TurnMode`, `CheckpointRequest`, `MailboxRequest`, `ProjectControl`, `UserShellRequest`, `InputQueueRequest`, `TreeRequest`, and `OutputRequest` model turn admission, checkpoint inspection, mailbox operations, project open/select/list/close, shell requests, queue edits, session tree control, and bounded tool-output access. Sender identity and workspace authorization are intentionally absent from client-controlled fields; hosts attach the owner and policy before side effects. `Command` owns payload text and attachments so admission can transfer large values without an extra clone.

## Validation and fail-closed behavior

`validate_cursor`, `validate_page_limit`, `validate_mailbox`, `validate_mailbox_text`, `validate_id`, `validate_identifier`, `validate_optional_field`, `bounded_text`, `bounded_text_allow_bare_bang`, `parse_user_shell_input`, and `validate_attachments` enforce frame/text/id/page/artifact/attachment limits before dispatch. `ProtocolError` carries typed JSON-safe failures; malformed requests do not mutate session state. Shell parsing recognizes a user-shell request but does not grant execution. Queue/tree/output validators reject unknown actions, invalid revisions, cross-project identities, and out-of-budget payloads.

## Responses and events

`StdioResponse` and `StdioEvent` are correlated envelopes with mutually exclusive `data`/`error`, version projection, owner-required errors, and explicit command names. The constructors keep success/error shape stable for reconnecting headless clients. `command_name` and `TurnMode` display helpers avoid ad-hoc string matching across hosts.

## Zenpi mapping and limits

This file is the protocol-side equivalent of pi-mono's host request/event boundary, with Zenpi additions for project owner identity, queue receipts, session tree, output receipts, and explicit fail-closed policy. It does not prove owner linearization, persistence, TUI rendering, or PTY behavior; those remain responsibilities of `headless.rs`, `project_workspace.rs`, `session.rs`, and their host validators. The report therefore records the complete protocol semantics without claiming cross-file acceptance.
