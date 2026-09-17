# ZS1-086 — `src/view_model.rs` per-file complete re-review (worker report-only candidate)

Status `[_]`: this is a worker self-tested, report-only candidate. The only
owned path is this report. This attempt does not modify any product file, does
not touch the canonical checkout, and does not claim Master G-FILE / G-STAGE
acceptance. The prior accepted report for the earlier source revision was a
2,163-byte summary; this revision re-reads the current bytes and supersedes it.

## Authority, baseline, and this read

- Source: `src/view_model.rs`
- Bytes: `42009`
- Lines: `1255`
- SHA-256: `094833f051ab66d30ce3f6cc74fcc0334ed938e79f5fd8e9269341b62bed08fe`
- Read range: byte `[0,42009)` (single chunk; file is below the 256 KiB chunk limit)
- Claim: `ZS1-086-20260917T011718-041`, item `ZS1-086`, run `20260917T011718-041`,
  layer `L1`, validators `G-FILE；G-STAGE --item ZS1-086`
- Blueprint row (frozen scope table): `src/view_model.rs`, declared `42009` bytes,
  hash `094833f051ab66d30ce3f6cc74fcc0334ed938e79f5fd8e9269341b62bed08fe`.

The declared bytes and hash match the working-tree bytes exactly
(`wc -c` = 42009, `shasum -a 256` = `094833f…`). The file was read sequentially
from line 1 to line 1255 in one pass with no truncation, and the whole file is
within a single allowed range, so `read_ranges` is `[[0, 42009]]`.

This file has **no in-file tests**: zero `#[cfg(test)]` modules and zero
`#[test]` functions. Direct unit/integration coverage lives in
`tests/view_model.rs` (425 lines, 11 tests), which was also read in full and is
recorded below as context only. Other files that reference the module
(`src/headless.rs`, `src/tui.rs`, `src/tool_output.rs`, and several `tests/*`)
were inspected only to establish the integration boundary described in a later
section; they are not part of this item's artifact and are not re-reviewed here.

No product execution, Cargo build, PTY, network, or algorithm run happened in
this attempt. All behavioral statements below are static readings of the source.
Where the reading is uncertain or an invariant is only conditionally provable,
it is marked as a boundary to verify rather than as an established result.

## Purpose and boundary

The module doc comment (L1–8) defines the contract precisely: agent and
provider layers expose different event types, and this module is the small
normalization boundary between those layers and a renderer. Both TUI and
headless transports can carry the same sequence/request/turn/block associations
without sharing a terminal implementation or a network runtime. The existing
wire events remain source-compatible; adapters opt in while the protocol
migrates incrementally.

Consequences that follow from the code, not just the doc:

- This file imports only `std`, `serde`, `thiserror`, and three crate modules:
  `backend::ProviderEvent`, `core::AgentEvent`, `render::MarkdownBlock`
  (L10–15). It does **not** import `ratatui`, `crossterm`, sockets, or process
  APIs. It is therefore renderer-neutral and runtime-neutral.
- It does not own provider I/O, terminal layout, session persistence,
  credentials, or execution. It normalizes events, bounds their size, and
  redacts text before host projection.
- The wire JSONL protocol is not redefined here. `ViewEvent` is a separate,
  versioned serialization surface (`VIEW_MODEL_VERSION`) that adapters may opt
  into; it is not the protocol's envelope.

## Constants and bounds

All limits are compile-time public constants except the terminal-turn set cap.

| Const | Line | Value | Role |
| --- | --- | --- | --- |
| `VIEW_MODEL_VERSION` | 19 | `1u16` | Serialized-shape version; incremented only on incompatible change |
| `MAX_VIEW_ID_BYTES` | 21 | `128` | Correlation-identifier byte cap |
| `MAX_VIEW_TEXT_BYTES` | 23 | `256 * 1024` = 262144 | One text-bearing field cap |
| `MAX_VIEW_BLOCKS` | 25 | `128` | Message block count cap |
| `MAX_VIEW_LIST_ITEMS` | 27 | `128` | One list block's item count cap |
| `MAX_VIEW_EVENT_BYTES` | 29 | `512 * 1024` = 524288 | One serialized event defensive cap |
| `MAX_TERMINAL_TURNS` | 30 | `256` (private) | Bounded set of terminal turn IDs retained |

The version default helper `default_view_model_version` (L1191) returns
`VIEW_MODEL_VERSION`; `ViewEvent::schema_version` uses it as a serde default, so
JSON without a `schema_version` key decodes as version 1 and `validate()`
accepts it.

## Error taxonomy

`ViewModelError` (L108–140) is a `thiserror` enum with these variants and
messages, all `&'static str` field labels except the two turn/timeline ones:

- `Empty { field }` — field empty after trim (from `validate_token`).
- `TooLarge { field, max }` — field exceeds a byte/count max.
- `ControlCharacter { field }` — disallowed control character present.
- `Invalid { field }` — structural/enum bound violation (heading level, drop
  count of 0, completed streaming block reuse, unknown schema version).
- `MissingTurn { field }` — event requires a turn association.
- `MissingBlock { field }` — event requires a block association.
- `SequenceDiscontinuity { expected, actual }` — append out of order.
- `SequenceExhausted` — `u64` sequence counter overflow.
- `EventTooLarge { max }` — encoded event exceeds `MAX_VIEW_EVENT_BYTES` or the
  buffer's own byte budget.
- `ReplayGap { requested, first_available }` — requested sequence was evicted.
- `ReplayFuture { requested, next_sequence }` — requested beyond what was written.
- `Serialization(String)` — `serde_json` encode failure.
- `TerminalTurn { turn_id }` — an event for a turn already terminal.
- `StreamClosed` — buffer permanently closed by a `Closed` event.

`ViewModelError` derives `Debug, Clone, PartialEq, Eq, Error`. It is conversion
target of `headless` (`View(#[from] ViewModelError)` at `src/headless.rs:527`).

## Vocabulary enums

These are `Copy`, `Serialize`, `Deserialize`, `rename_all = "snake_case"`:

- `ViewRole` (L145): `User, Assistant, Tool, System, Error`.
- `ToolStatus` (L156): `Queued, Running, Succeeded, Failed, Cancelled`.
- `ApprovalState` (L167): `Pending, Allowed, Denied`.
- `ViewTurnMode` (L502): `StartOrSteer, StartIfIdle, Steer`.
- `ViewRejection` (L511): `EmptyInput, NotIdle, NoActiveTurn,
  ExpectedTurnMismatch, ActiveTurnNotSteerable, Closed, Invalid`.
- `ViewStream` (L524): `Provider, Agent, Terminal, Control`.
- `ViewBlockKind` (L430): the 11 stable block discriminators (see below).

## Streaming text lifecycle

`StreamingBlock` (L35–104) is mutable streaming state, not a serialized type:
`block_id: String`, `text: String`, `completed: bool`, with `Debug, Clone,
PartialEq, Eq` only.

- `new(block_id)` (L42) validates the id (`validate_id`, 128-byte token rule)
  and starts with empty text and `completed = false`.
- `append(delta)` (L64) fails closed with `Invalid { "completed streaming
  block" }` if already completed; otherwise validates the delta as text and
  rejects if `text.len() + delta.len()` would exceed `MAX_VIEW_TEXT_BYTES`
  (saturating add, so no overflow), then appends.
- `complete(final_text)` (L82) fails closed if already completed. If
  `final_text` is `Some`, it validates size/controls before replacing the
  accumulated deltas outright (`text.clear()` then push); it returns a
  `ViewBlock::Paragraph { text }` clone and sets `completed = true`.
- `block_id()`, `is_completed()`, `text()` are read-only accessors.

Semantics to keep: completion is one-way; a later `final_text` replaces, it does
not concatenate; the accumulated text can never exceed `MAX_VIEW_TEXT_BYTES`
because every `append` is bounded, and a replacement is bounded before it is
applied. `StreamingBlock` deliberately has no `Serialize`/`Deserialize`.

## `ViewBlock`: renderer-neutral bounded content

`ViewBlock` (L175–228) is `#[serde(tag = "kind", rename_all = "snake_case",
deny_unknown_fields)]`. Variants:

| Variant | Fields | Notes |
| --- | --- | --- |
| `PlainText` | `text` | literal/unknown syntax |
| `Paragraph` | `text` | may pass through simple Markdown renderer |
| `Heading` | `level: u8`, `text` | level bound enforced in validate |
| `List` | `ordered: bool`, `items: Vec<String>` | one block so folding/replay is wrapping-independent |
| `Quote` | `text` | |
| `Code` | `language: Option<String>`, `text` | |
| `Diff` | `path: Option<String>`, `patch` | bounded unified patch; renderer may split hunks |
| `ToolStatus` | `call_id`, `name`, `status: ToolStatus`, `output: Option<String>` (default/skip) | |
| `Approval` | `approval_id`, `tool`, `arguments`, `state: ApprovalState` | |
| `Error` | `code: Option<String>` (default/skip), `message`, `retryable: bool` | |
| `Rule` | none | |

### `redacted()` (L234–295)

Uses `crate::security::redact_text(text, &[])` (an empty known-secret list, so
detection is pattern-based). It redacts every free-text field and preserves
structural fields verbatim:

- Redacted: `PlainText.text`, `Paragraph.text`, `Heading.text`, `Quote.text`,
  each `List.items` entry, `Code.text`, `Diff.patch`, `ToolStatus.output`,
  `Approval.arguments`, `Error.message`.
- Not redacted (cloned unchanged): `Heading.level`, `List.ordered`,
  `Code.language`, `Diff.path`, `ToolStatus.call_id`/`name`/`status`,
  `Approval.approval_id`/`tool`/`state`, `Error.code`/`retryable`.
- `Rule` is unchanged.

This is an important and honest boundary: redaction is applied to text payloads,
not to identifiers, names, languages, paths, or error codes. If a caller puts a
secret into `ToolStatus.name`, `Diff.path`, or `Error.code`, `redacted()` will
not mask it. The tests only assert that `Error.message` is masked. Do not
describe the function as "redacts all fields".

### `validate()` (L298–371)

- `PlainText | Paragraph | Quote`: `validate_text`.
- `Heading`: `level` must be in `1..=6` else `Invalid { "heading level" }`, then
  `validate_text`.
- `List`: rejects empty (`Empty { "list items" }`), rejects
  `> MAX_VIEW_LIST_ITEMS` (`TooLarge`), then validates every item as text.
- `Code`: optional `language` validated as a token capped at 64 bytes; text via
  `validate_text`.
- `Diff`: optional `path` validated as a token capped at `4 * 1024` bytes; patch
  via `validate_text`.
- `ToolStatus`: `call_id` as id; `name` as token capped at `MAX_VIEW_ID_BYTES`;
  optional `output` as text.
- `Approval`: `approval_id` as id; `tool` as id-token; `arguments` as text.
- `Error`: optional `code` as token capped at `MAX_VIEW_ID_BYTES`; `message` as
  text.
- `Rule`: `Ok`.

Note there is no aggregate size check here. With 128 list items each up to
`MAX_VIEW_TEXT_BYTES`, a single `ViewBlock::List` can pass `validate()` while its
serialized form is many megabytes; the event-level `MAX_VIEW_EVENT_BYTES` check
in `ViewEvent::validate` is what actually bounds an emitted event, and a bare
`ViewMessage` (which has no serialized-size check) is not bounded in aggregate.

### `kind()` (L374–388)

Maps each variant to `ViewBlockKind`; `const fn`, so it is usable in const
contexts. This is the stable discriminator contract for consumers/tests.

### `from_markdown` / `from_markdown_text` (L392–424)

- `from_markdown(&MarkdownBlock)`: `Paragraph→Paragraph`,
  `Heading→Heading`, `Code→Code`, `Quote→Quote`,
  `ListItem→List{ordered, items: vec![text]}`, `Rule→Rule`; then `validate()`.
  Each parser `ListItem` becomes its own single-item `List` block — the parser's
  per-item model is not coalesced here.
- `from_markdown_text(text)`: calls `crate::render::parse_markdown(text)`;
  rejects `blocks.len() > MAX_VIEW_BLOCKS` with `TooLarge { "message blocks" }`;
  then maps each block through `from_markdown`.
- `crate::render::parse_markdown` itself bounds input to `MAX_MARKDOWN_BYTES`
  (256 KiB) and sanitizes text; it does not bound the number of blocks, which is
  why the explicit block-count guard exists. The parser is loss-tolerant: an
  unclosed fence becomes a code block at EOF and unknown syntax stays paragraph
  text (`src/render.rs:51–61`). If the parser produced a heading level outside
  1..=6, `from_markdown` validation would fail and `from_markdown_text` would
  propagate `Invalid`.

## `ViewMessage`

`ViewMessage` (L445–452) is `deny_unknown_fields` with `role: ViewRole`,
`turn_id: Option<String>` (default/skip), `blocks: Vec<ViewBlock>`.

- `new` (L455) builds then validates.
- `from_markdown` (L469) builds blocks via `ViewBlock::from_markdown_text` then `new`.
- `validate` (L477): optional `turn_id` must be a valid id; blocks must be
  non-empty (`Empty { "message blocks" }`); at most `MAX_VIEW_BLOCKS`; every
  block validated.
- Deserialization bypasses `new`, so a decoded `ViewMessage` must be explicitly
  `validate()`d before use. Like `ViewBlock`, it has no aggregate serialized
  size check.

## `ViewEventKind`: typed bounded lifecycle payload

`ViewEventKind` (L535–622) is `#[serde(tag = "type", rename_all =
"snake_case", deny_unknown_fields)]` with 20 variants:

`RequestAccepted { mode }`, `RequestRejected { reason }`,
`TurnStarted { response_id?, model? }`, `TextDelta { delta }`,
`ReasoningDelta { delta }`, `Block { block }`,
`ToolStarted { call_id, name }`,
`ToolCallDelta { call_id?, name?, arguments_delta }`,
`ToolCallReady { call_id, name, arguments }`,
`ToolFinished { call_id, status, output? }`,
`ApprovalRequired { approval_id, tool, arguments }`,
`ApprovalResolved { approval_id, state }`,
`Usage { input_tokens, output_tokens, total_tokens }`,
`Handoff { handoff_id, to? }`, `Warning { message }`,
`TurnCompleted { response_id?, model? }`,
`TurnFailed { code?, message, retryable }`,
`TurnCancelled { reason? }`, `Dropped { stream, count }`, `Closed`.

### `validate()` (L624–704)

- `RequestAccepted | RequestRejected | Usage | Closed`: `Ok` (payloads are typed
  enums/integers).
- `TurnStarted | TurnCompleted`: optional `response_id` as id; optional `model`
  as token capped at `MAX_VIEW_ID_BYTES`.
- `TextDelta` / `ReasoningDelta`: text validation on `delta`.
- `Block`: delegates to `ViewBlock::validate`.
- `ToolStarted`: `call_id` id, `name` token.
- `ToolCallDelta`: optional `call_id` id, optional `name` token,
  `arguments_delta` text.
- `ToolCallReady`: `call_id` id, `name` token, `arguments` text.
- `ToolFinished`: `call_id` id, optional `output` text.
- `ApprovalRequired`: `approval_id` id, `tool` token, `arguments` text.
- `ApprovalResolved`: `approval_id` id only.
- `Handoff`: `handoff_id` id, optional `to` id.
- `Warning`: `message` text.
- `TurnFailed`: optional `code` **as id** (≤128, no controls), `message` text.
- `TurnCancelled`: optional `reason` **as text** (≤256 KiB, allows `\n\r\t`).
  Note the asymmetry between `code` (id) and `reason` (text).
- `Dropped`: `count` must be non-zero else `Invalid { "drop count" }`.

### Association requirements (L706–729)

`requires_turn_id()` is true for exactly: `RequestAccepted`, `TurnStarted`,
`TextDelta`, `ReasoningDelta`, `Block`, `ToolStarted`, `ToolCallDelta`,
`ToolCallReady`, `ToolFinished`, `ApprovalRequired`, `ApprovalResolved`,
`TurnCompleted`. It is **false** for `RequestRejected`, `Usage`, `Handoff`,
`Warning`, `TurnFailed`, `TurnCancelled`, `Dropped`, `Closed`.

`requires_block_id()` is true for exactly `TextDelta`, `ReasoningDelta`, `Block`.

So a failed/cancelled/warning/handoff/dropped event may legally have no turn
association, and `AgentEvent::Error` maps to a no-turn `TurnFailed` (see below).
Streaming deltas and block events must carry both a turn id and a block id.

## `ViewEvent` envelope

`ViewEvent` (L733–746) has `schema_version: u16` (serde default), `sequence:
u64`, optional `request_id`/`turn_id`/`block_id` (skip when none), and
`#[serde(flatten)] event: ViewEventKind`. It derives
`Debug, Clone, PartialEq, Eq, Serialize, Deserialize`.

- `new(sequence, event)` (L749) delegates to `with_context(..,None,None,None,..)`.
- `with_context` (L753) constructs then `validate()`s, so constructors are
  fail-closed.
- `validate()` (L772):
  1. `schema_version` must equal `VIEW_MODEL_VERSION` (1);
  2. each optional id validated as an id;
  3. `event.validate()`;
  4. if `event.requires_turn_id()` and `turn_id.is_none()` →
     `MissingTurn { "event" }`;
  5. if `event.requires_block_id()` and `block_id.is_none()` →
     `MissingBlock { "event" }`;
  6. `encoded_len() <= MAX_VIEW_EVENT_BYTES` else `EventTooLarge`.
- `encoded_len()` (L797) is `serde_json::to_vec(self).len()`, mapping JSON
  errors to `Serialization`.

Serialization note: the envelope has no `deny_unknown_fields`, and the payload
uses `#[serde(flatten)]`. `ViewEventKind` does declare `deny_unknown_fields`, but
serde's `deny_unknown_fields` interacts incompletely with `flatten`; whether an
unknown top-level key is silently ignored or rejected is a static uncertainty to
verify, not a proven guarantee. Also, `serde_json::from_*` does **not** call
`validate()`, so a decoded `ViewEvent` may carry a wrong `schema_version` or
violate the other invariants until someone calls `validate()`; the buffer's
`append` does call it, which is the normal ingestion path.

## Adapter mappings

### `from_agent_event(sequence, request_id, &AgentEvent)` (L803–914)

| `AgentEvent` | `ViewEventKind` | turn_id | block_id |
| --- | --- | --- | --- |
| `TurnAccepted { turn_id, mode }` | `RequestAccepted { mode: TurnMode→ViewTurnMode }` | `Some(turn_id)` | none |
| `TurnRejected { reason }` | `RequestRejected { reason: NotSubmittedReason→ViewRejection }` | none | none |
| `AssistantMessage { turn_id, content }` | `Block { Paragraph { content } }` | `Some(turn_id)` | `default_block_id(turn, "assistant")` |
| `Handoff { handoff_id, to }` | `Handoff { .. }` | none | none |
| `ToolCall { turn_id, call_id, tool }` | `ToolStarted { call_id, name: tool }` | `Some(turn_id)` | none |
| `ToolProgress { turn_id, progress }` | `progress.canonical_kind()` (a `Block`/`ToolStatus::Running`) | `Some(turn_id)` | `output-{sha256(turn_id,0x00,call_id,stream_byte)}` |
| `ToolResult { turn_id, call_id, success }` | `ToolFinished { status: Succeeded/Failed, output: None }` | `Some(turn_id)` | none |
| `Provider { turn_id, event }` | `from_provider_event(.., Some(turn_id), None, event)` | per provider mapping | per provider mapping |
| `Error { message }` | `TurnFailed { code: None, message, retryable: false }` | none | none |

`ToolProgress` is the only mapping that hashes its block id: it feeds
`turn_id`, a `0x00` separator, `call_id`, and a stream discriminator byte
(stdout `0`, stderr `1`) into SHA-256 and formats `output-{:x}` (71 chars). This
deterministic id keeps repeated progress snapshots in the same block.

`AgentEvent::Provider` forwards the turn id and a `None` block id, letting
`from_provider_event` derive block ids.

### `from_provider_event(sequence, request_id, turn_id, block_id, &ProviderEvent)` (L916–1000)

Block-id derivation first:

- `content_event = TextDelta | TextDone | Refusal`.
- If `ReasoningDelta`: `block_id = turn_id.map(default_block_id(_, "reasoning"))`,
  ignoring any supplied block id.
- Else: use the supplied `block_id`, or, if `content_event` and a turn id are
  present, `default_block_id(turn, "assistant")`.

Then the event mapping (all 11 `ProviderEvent` variants are covered):

| `ProviderEvent` | `ViewEventKind` |
| --- | --- |
| `ResponseCreated { response_id, model }` | `TurnStarted { response_id, model }` |
| `TextDelta { delta }` | `TextDelta { delta }` |
| `ReasoningDelta { delta }` | `ReasoningDelta { delta }` |
| `TextDone { text }` | `Block { Paragraph { text } }` |
| `Refusal { text }` | `Block { Error { code: Some("refusal"), message: text, retryable: false } }` |
| `ToolCallDelta { call_id, name, arguments_delta }` | `ToolCallDelta { .. }` |
| `ToolCallDone { call }` | `ToolCallReady { call_id, name, arguments: json!(call.arguments) }` (serialization may fail → `Serialization`) |
| `Usage { usage }` | `Usage { input_tokens, output_tokens, total_tokens }` |
| `Warning { message }` | `Warning { message }` |
| `Completed { response_id, model }` | `TurnCompleted { response_id, model }` |
| `Failed { message }` | `TurnFailed { code: Some("provider_failed"), message, retryable: true }` |

Because `TextDelta`/`ReasoningDelta`/`Block` require block ids, calling this
with `turn_id = None` for a text/reasoning/text-done/refusal event yields a
fail-closed error (`MissingBlock`), not a silently unassociated event.
`ToolCallDone` serializes `ToolCall.arguments` (a `serde_json::Value`) into a
string; that value is then re-validated as text.

## `ViewEventBuffer`: bounded ordered replay

`ViewEventBuffer` (L1007–1167) derives `Debug, Clone`. Fields: `capacity`,
`max_bytes`, `next_sequence`, `bytes`, `dropped`, `events: VecDeque<ViewEvent>`,
`terminal_turns: HashSet<String>`, `terminal_order: VecDeque<String>`,
`closed: bool`.

`new(capacity, max_bytes)` (L1020) clamps both inputs to at least 1
(`capacity.max(1)`, `max_bytes.max(1)`), so a zero-sized request silently
becomes a one-event buffer. Accessors: `next_sequence`, `len`, `is_empty`,
`retained_bytes`, `dropped`, `take_dropped` (`mem::take`), `iter`.

### `push` (L1062) and `append` (L1075)

`push` labels the event with the current `next_sequence` and calls `append`.

`append(event)` in order:

1. `event.validate()`.
2. `closed` → `StreamClosed`.
3. `event.sequence != next_sequence` → `SequenceDiscontinuity { expected, actual }`
   (no mutation yet).
4. If the event has a `turn_id` already in `terminal_turns` → `TerminalTurn`.
5. `encoded_len() > max_bytes` → `EventTooLarge` (rejects rather than drops).
6. If the event kind is `TurnCompleted | TurnFailed | TurnCancelled` with a turn
   id, insert into `terminal_turns` (only on first insert) and push the id onto
   `terminal_order`; evict the oldest ids beyond `MAX_TERMINAL_TURNS` (256).
7. If the kind is `Closed`, set `self.closed = true`.
8. `next_sequence.checked_add(1)` → `SequenceExhausted`.
9. Add encoded bytes (saturating), push the event, then evict from the front
   while `len > capacity || bytes > max_bytes`, subtracting each evicted event's
   encoded length and incrementing `dropped` (saturating).

Invariants and honest boundaries:

- Sequence monotonicity survives eviction: `next_sequence` never rewinds, so a
  replay caller can distinguish an old gap from an empty stream (doc comment
  L1003–1005). Eviction increments `dropped` per event.
- Terminal turns are remembered across eviction and across `drain`; a stale
  retry for a completed turn is rejected until its id ages out of the bounded
  256-entry order. `drain` does not clear this set.
- The order of steps means the terminal-turn bookkeeping (step 6) and the
  `closed` flag (step 7) happen before the `checked_add` (step 8). On the
  `SequenceExhausted` path the function is therefore not perfectly atomic:
  terminal state or `closed` may already be mutated. This is only reachable
  after `u64::MAX` events, so it is a theoretical boundary, not a practical bug.
- A single event larger than the buffer's `max_bytes` is rejected, not evicted.
- `capacity` counts events; `max_bytes` counts the sum of their serialized
  lengths, so the retained footprint is bounded on both axes.

### `drain` (L1133)

Clears `bytes` and drains all events (returning them) but leaves
`next_sequence`, `dropped`, `terminal_turns`, `terminal_order`, and `closed`
untouched. A drained-then-pushed stream continues numbering; a drained closed
stream stays closed.

### `replay_from(sequence)` (L1138)

- `sequence > next_sequence` → `ReplayFuture { requested, next_sequence }`.
- Empty buffer: if `sequence < next_sequence` →
  `ReplayGap { requested, first_available: next_sequence }`; else returns an
  empty `Vec`. Note `first_available` is reported as `next_sequence` here, not a
  real retained sequence.
- Non-empty and `sequence < front.sequence` →
  `ReplayGap { requested, first_available: front.sequence }`.
- Otherwise returns clones of all retained events with `sequence >= requested`.

Replay never invents history and never rewinds; it distinguishes gap from future
from empty. It does not check `closed`, so retained events remain replayable
after a `Closed` event.

## Conversion impls

- `impl From<crate::protocol::TurnMode> for ViewTurnMode` (L1169) maps all three
  modes one-to-one.
- `impl From<crate::core::NotSubmittedReason> for ViewRejection` (L1179) maps
  all five core reasons (`EmptyInput`, `NotIdle`, `NoActiveTurn`,
  `ExpectedTurnMismatch`, `ActiveTurnNotSteerable`). `ViewRejection::Closed` and
  `ViewRejection::Invalid` deliberately have no producer here; they are
  available for adapters that need to express those host-side conditions.

## Validation helpers

- `default_block_id(turn_id, suffix)` (L1195): builds `"{truncated_turn}:{suffix}"`
  so the result is at most `MAX_VIEW_ID_BYTES` (128) bytes; truncation walks back
  to a UTF-8 char boundary. Used with `"assistant"` and `"reasoning"`.
- `validate_id` (L1207): `validate_token(value, field, MAX_VIEW_ID_BYTES)`.
- `validate_optional_id` (L1211): validates when `Some`.
- `validate_optional_text` (L1218): validates when `Some`.
- `validate_token` (L1228): rejects whitespace-only as `Empty`; rejects
  `len > max` as `TooLarge`; rejects any `char::is_control` as
  `ControlCharacter`. This is the identifier/name rule (no newlines allowed).
- `validate_text` (L1241): rejects `len > MAX_VIEW_TEXT_BYTES` as `TooLarge`;
  rejects control characters **except** `\n`, `\r`, `\t` as `ControlCharacter`.
  It does **not** reject empty text, so empty paragraphs/messages-as-text are
  structurally legal.

`validate_token` and `validate_text` differ deliberately: identifiers are small,
trimmed, and control-free; text is large and may contain newlines/tabs.

## Integration boundary (context only)

The module is consumed on both transports; the following is a bounded reading to
place the module in context, not a re-review of those files.

- `src/headless.rs`: builds view events from provider events
  (`ViewEvent::from_provider_event`, L3992, L4157) and from agent events
  (`from_agent_event`, L8945); turns blocks into JSON with `block.redacted()`
  (L4230, L9001) and maps `ViewModelError` into the headless error enum (L527);
  uses approval blocks (L4075–4095); marks output `"redacted": true` (L2283).
- `src/tui.rs`: stores `Vec<ViewBlock>` per job (L111) and
  `Option<StreamingBlock>` (L1735, L2406); maps `ToolRunStatus` to `ToolStatus`
  (L3903), appends blocks (L4051), and consumes `ViewEvent` envelopes in
  `apply_view_event_for_job` (L4272) including `Dropped` and terminal handling;
  builds view events from agent/provider events (L14177, L14201) and renders
  block text via `view_block_text` (L14628).
- `src/tool_output.rs`: `OutputProgress::canonical_kind` (L334) returns a
  `ViewEventKind::Block` wrapping a `ViewBlock::ToolStatus` with `Running` and a
  bounded `[stream: N bytes]` prefixed output; the host supplies the envelope
  sequence/turn and drains updates before emitting `ToolFinished`. The string is
  truncated on a char boundary to `MAX_VIEW_TEXT_BYTES`.

Takeaway: `view_model` is a compatibility and safety boundary shared by both
hosts. It owns normalization, bounds, and (text) redaction, but it owns neither
provider I/O, layout, sessions, credentials, nor execution.

## Static boundaries, caveats, and things not proven

Recorded as boundaries for later verification, not as confirmed defects:

1. `ViewBlock::redacted()` redacts text fields only. Identifiers, tool names,
   languages, diff paths, and error codes are cloned verbatim; a secret placed
   there would not be masked. Tests only cover `Error.message`.
2. `redact_text` is called with an empty known-secret list, so redaction relies
   entirely on `security::redact_text`'s pattern detection.
3. `ViewBlock::validate` performs no aggregate size check, and `ViewMessage`
   has none either; only `ViewEvent::validate` enforces
   `MAX_VIEW_EVENT_BYTES`. A standalone `ViewMessage` can be validated while
   serializing to many megabytes.
4. `ViewEvent`/`ViewEventKind`/`ViewMessage`/`ViewBlock` deserialization bypasses
   `validate`; a decoded value can hold out-of-range heading levels, oversized
   collections, or a wrong `schema_version` until explicitly validated.
5. `deny_unknown_fields` combined with `#[serde(flatten)]` is a known serde gap;
   unknown-key rejection for `ViewEvent` is not asserted as guaranteed.
6. `ViewEventBuffer::append` is not perfectly atomic on the `SequenceExhausted`
   path (terminal bookkeeping/`closed` may be set before the overflow error);
   practically unreachable.
7. `new` silently clamps `capacity`/`max_bytes` of 0 to 1.
8. `replay_from` on an emptied buffer reports `first_available = next_sequence`,
   which is a sentinel rather than a retained sequence.
9. `drain` retains terminal-turn memory and `closed`; intended, but callers must
   not assume a drained buffer forgets terminal state.
10. `ViewRejection::Closed`/`Invalid` are unproducible via the core conversion.
11. `ToolProgress` block ids are SHA-256-derived and are not the
    `default_block_id` shape (`turn:suffix`); consumers keying on the
    `turn:suffix` convention must not assume it for output-progress blocks.
12. `from_markdown` maps each parser list item to its own single-item block; it
    does not coalesce adjacent list items.

## Tests (`tests/view_model.rs`, full read; executed 0)

11 integration tests, all in `tests/view_model.rs`:

1. `blocks_and_messages_round_trip_with_bounded_shapes` (L17) — message with
   Paragraph/Code/Diff/ToolStatus round-trips; `kind()` and `validate()`.
2. `block_redaction_preserves_shape_and_hides_credentials` (L54) — `Error`
   redaction drops `sk-block-secret`, keeps `retryable`, emits `<redacted>`.
3. `markdown_parser_maps_to_shared_blocks` (L74) — parser output maps heading,
   list items, quote, code; `MarkdownBlock::Rule → ViewBlockKind::Rule`.
4. `validation_rejects_control_bytes_and_unbounded_blocks` (L91) — ESC control
   char, over-limit list, over-limit paragraph.
5. `event_envelope_requires_associations_and_flattens_payload` (L129) — missing
   turn rejected; flattened JSON fields (`type`, `sequence`, ids) and round trip.
6. `streaming_block_merges_deltas_and_freezes_after_completion` (L164) — append
   merge and fail-closed after completion.
7. `agent_and_provider_adapters_preserve_turn_and_block_correlation` (L189) —
   agent accept/assistant/reject/error and provider text/tool-call mappings.
8. `bounded_buffer_keeps_order_and_reports_replay_gaps` (L266) — capacity 2,
   3 pushes, sequence/dropped, `ReplayGap`, `ReplayFuture`, `take_dropped`.
9. `buffer_rejects_sequence_discontinuity_without_mutating_state` (L332) —
   non-contiguous append leaves buffer empty.
10. `buffer_rejects_events_after_terminal_turn_but_allows_next_turn` (L357) —
    `TerminalTurn` for the completed turn, accepts a new turn.
11. `buffer_rejects_events_after_stream_closed` (L409) — `StreamClosed`.

These tests are existing supporting code. They were read, not executed in this
attempt, and passing them cannot be inferred from their assertions. Additional
referencing tests (`tests/stage1_chat_stream.rs`, `tests/stage1_tool_output.rs`,
`tests/tools.rs`, and the `tests/tui_*` files) exercise the module through the
hosts; they are context only here.

## Minimal verification matrix (all steps NOT executed in this attempt)

- V01 — identity: re-hash `src/view_model.rs`; require 42009 bytes and
  `094833f051ab66d30ce3f6cc74fcc0334ed938e79f5fd8e9269341b62bed08fe`, and that
  this report repeats both.
- V02 — text bounds: construct each text-bearing `ViewBlock` and
  `ViewEventKind` at exactly `MAX_VIEW_TEXT_BYTES` and one over; expect Ok/TooLarge.
- V03 — control chars: verify `\n\r\t` pass `validate_text`, other C0/C1 reject;
  verify `validate_token` rejects newlines.
- V04 — association: each `requires_turn_id`/`requires_block_id` kind must fail
  `ViewEvent::new` without its association and pass with it.
- V05 — redaction scope: place a secret in `ToolStatus.name`, `Diff.path`, and
  `Error.code`; confirm `redacted()` behaviour matches the scope table above
  (i.e. records the limitation rather than assuming full redaction).
- V06 — buffer order/limits: capacity and byte eviction, `dropped` accounting,
  `drain` preserving sequence/terminal state, `replay_from` gap/future/empty.
- V07 — terminal turns: complete turn, reject stale delta, accept new turn;
  exceed 256 terminal ids and confirm bounded eviction.
- V08 — closed semantics: `Closed` retains the event, sets `closed`, rejects
  further appends, and replay still returns retained events.
- V09 — adapter coverage: every `AgentEvent` and `ProviderEvent` variant maps
  without panicking and satisfies the envelope association requirements.
- V10 — deserialization: decoded values are not auto-validated; call `validate()`
  and confirm out-of-range/unknown-field behaviour on this toolchain.
- V11 — serialized-size: single event just over `MAX_VIEW_EVENT_BYTES` is
  rejected; an aggregate message of many legal blocks is unbounded (documented).
- V12 — gate: run the item's prescribed G-FILE/G-STAGE evidence path and the
  repo local gate from `spec.json` (`cargo test --locked`) in an owner-approved
  run; not run here.

## Definition index

Locations, not a substitute for the semantics above. Overloads are not merged.

| Line | Owner | Definition |
| --- | --- | --- |
| 19 | module | `VIEW_MODEL_VERSION` |
| 21 | module | `MAX_VIEW_ID_BYTES` |
| 23 | module | `MAX_VIEW_TEXT_BYTES` |
| 25 | module | `MAX_VIEW_BLOCKS` |
| 27 | module | `MAX_VIEW_LIST_ITEMS` |
| 29 | module | `MAX_VIEW_EVENT_BYTES` |
| 30 | module | `MAX_TERMINAL_TURNS` |
| 35 | `StreamingBlock` | struct |
| 42 | `StreamingBlock` | `new` |
| 52 | `StreamingBlock` | `block_id` |
| 56 | `StreamingBlock` | `is_completed` |
| 60 | `StreamingBlock` | `text` |
| 64 | `StreamingBlock` | `append` |
| 82 | `StreamingBlock` | `complete` |
| 108 | `ViewModelError` | enum |
| 145 | `ViewRole` | enum |
| 156 | `ToolStatus` | enum |
| 167 | `ApprovalState` | enum |
| 177 | `ViewBlock` | enum |
| 234 | `ViewBlock` | `redacted` |
| 298 | `ViewBlock` | `validate` |
| 374 | `ViewBlock` | `kind` |
| 392 | `ViewBlock` | `from_markdown` |
| 415 | `ViewBlock` | `from_markdown_text` |
| 430 | `ViewBlockKind` | enum |
| 447 | `ViewMessage` | struct |
| 455 | `ViewMessage` | `new` |
| 469 | `ViewMessage` | `from_markdown` |
| 477 | `ViewMessage` | `validate` |
| 502 | `ViewTurnMode` | enum |
| 511 | `ViewRejection` | enum |
| 524 | `ViewStream` | enum |
| 535 | `ViewEventKind` | enum |
| 625 | `ViewEventKind` | `validate` |
| 706 | `ViewEventKind` | `requires_turn_id` |
| 724 | `ViewEventKind` | `requires_block_id` |
| 734 | `ViewEvent` | struct |
| 749 | `ViewEvent` | `new` |
| 753 | `ViewEvent` | `with_context` |
| 772 | `ViewEvent` | `validate` |
| 797 | `ViewEvent` | `encoded_len` |
| 803 | `ViewEvent` | `from_agent_event` |
| 916 | `ViewEvent` | `from_provider_event` |
| 1007 | `ViewEventBuffer` | struct |
| 1020 | `ViewEventBuffer` | `new` |
| 1034 | `ViewEventBuffer` | `next_sequence` |
| 1038 | `ViewEventBuffer` | `len` |
| 1042 | `ViewEventBuffer` | `is_empty` |
| 1046 | `ViewEventBuffer` | `retained_bytes` |
| 1050 | `ViewEventBuffer` | `dropped` |
| 1054 | `ViewEventBuffer` | `take_dropped` |
| 1058 | `ViewEventBuffer` | `iter` |
| 1062 | `ViewEventBuffer` | `push` |
| 1075 | `ViewEventBuffer` | `append` |
| 1133 | `ViewEventBuffer` | `drain` |
| 1138 | `ViewEventBuffer` | `replay_from` |
| 1169 | `From<TurnMode>` for `ViewTurnMode` | `from` |
| 1179 | `From<NotSubmittedReason>` for `ViewRejection` | `from` |
| 1191 | module | `default_view_model_version` |
| 1195 | module | `default_block_id` |
| 1207 | module | `validate_id` |
| 1211 | module | `validate_optional_id` |
| 1218 | module | `validate_optional_text` |
| 1228 | module | `validate_token` |
| 1241 | module | `validate_text` |

## Ownership, rollback, and delivery

- Sole owned path: `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/view_model.rs_learn.md`.
- No product or source file is modified. Rollback removes/restores only this
  report; source files and the canonical checkout are never touched.
- This candidate is worker self-tested only. It records the read coverage,
  identity, semantics, limits, and boundaries above; Master G-FILE/G-STAGE
  acceptance of the item remains a separate, independent review.
