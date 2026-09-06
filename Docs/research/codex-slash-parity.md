# Slash-command parity audit

Captured locally on 2026-09-06 from `codex-cli 0.153.4` (`codex --help`).
Codex exposes an interactive TUI by default and keeps lifecycle operations
such as `resume`, `fork`, `queue`, and `review` as explicit commands. Zenpi
keeps one workspace and exposes its projections through slash commands.

| Surface | Zenpi contract | Compatibility decision |
|---|---|---|
| `/goal` | bounded b3ehive goal owner | Deliberately not claimed identical to Codex internal goal state |
| `/yolo` | explicit approval posture toggle | Local, host-gated, no provider turn |
| `/approval` | `ask`, `always`, `never` | Maps to `ReadOnly`, `Always`, `Never` |
| `!echo` | local shell request | Parsed before provider submission; gates still apply |
| `resume/session` | durable Zenpi session owner | Explicit slash projection; no background daemon |

The minimal parity rule is behavioral rather than textual: commands are local
control-plane messages, invalid arguments are rejected, request IDs remain
idempotent, and no slash command is silently converted into a model prompt.

## Command matrix

| Command family | Interaction contract |
|---|---|
| `/help`, `/persona`, `/model`, `/models`, `/doctor` | local query/configuration; no provider turn |
| `/goal`, `/plan`, `/blueprint` | bounded planning/domain projections; explicit persistence/run actions |
| `/learn`, `/review`, `/diff`, `/resources` | read-only projections unless an explicit write action is named |
| `/session`, `/resume`, `/recovery` | durable lifecycle/replay; request IDs and confirmation gates apply |
| `/approval`, `/yolo`, `/approve` | approval control-plane; policy changes are host-owned |
| `/attach`, `/compact`, `/clear`, `/cancel`, `/exit` | local state/input lifecycle |
| `/mailbox`, `/compete`, `/loop` | addressed handoff or runtime route; parser never launches work |
| `!echo` | explicit local shell route; never ordinary provider text and never a gate bypass |
