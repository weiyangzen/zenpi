# `src/slash.rs` per-file learn report (ZS1-079)

- Source: `src/slash.rs`
- Complete read range: byte `[0,81726)`; 2,287 lines
- Bytes: `81726`
- SHA-256: `d0702d86e3cf8087dbc09a3fe68c9067dd6bed9f37f499abcdb7e0131927a1c9`

## Typed control grammar

`SlashRoute`, `SlashCommand`, `ProjectAction`, `RecoveryAction`, `SessionAction`, `MailboxAction`, `LayoutAction`, `PaneAction`, `ApproveDecision`, `BlueprintAction`, and `SlashError` form a typed control-plane grammar. Commands are parsed but never executed in this module; hosts decide authorization and owner routing. Explicit project/session/layout/pane/approval/blueprint actions preserve bounded payloads and confirmation requirements.

## Routing and parsing

`route_input` distinguishes ordinary prompts, slash control messages, resource candidates, and explicit user-shell input. `parse` tokenizes quoted/escaped arguments without shell expansion, rejects malformed/unknown built-ins instead of sending them to the model, and enforces command/text/retention/path bounds. `resource_input_candidate`, `resource_control`, `output_control`, `tree_control`, `input_queue_control`, and `external_evidence_control` keep newer host protocols on the same fail-closed boundary.

## Completion and help

The static `SlashCommandSpec` catalogue, `spec`, `complete`, `help`, and `parity_note` provide deterministic TUI/headless discovery, aliases, summaries, and Codex-compatible guidance without a UI dependency. Canonical names remain separate from runtime/b3ehive routes so completion cannot imply local execution.

## Specialized parsers

Layout/pane parsers validate tab/pane IDs and visibility intent; session parsers require explicit paths and confirmation for destructive operations; mailbox, queue, tree, output, goal, learn, blueprint, approval, and domain parsers convert text into typed protocol actions. Session GC requires both bounded retention dimensions and an explicit confirmation token.

## Mapping and limits

This maps to pi-mono/Codex slash command dispatch and completion, adding Zenpi's project owner, BentoBox layout, mailbox/queue/tree/output control, bounded GC, external evidence, and b3ehive routes. It does not perform provider calls, filesystem mutations, TUI rendering, or shell execution; those remain host responsibilities.
