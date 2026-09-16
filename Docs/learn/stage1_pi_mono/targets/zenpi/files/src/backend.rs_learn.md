# `src/backend.rs` per-file learn report (ZS1-075)

- Source: `src/backend.rs`
- Complete read range: byte `[0,98741)`; 2,706 lines
- Bytes: `98741`
- SHA-256: `d11105597a7e4260c67c9ce7796394687b4219617a6ebfdfeca637ae469c3357`

## Normalized backend boundary

`InputAttachment`, `RequestAttachment`, `ProviderCapabilities`, `CompletionRequest`, `ProviderEvent`, `Usage`, `Completion`, `BackendError`, and `Backend` define the provider-independent contract. The core submits immutable session/history/context views and receives one normalized completion or bounded stream events; HTTP and SDK details remain outside the state machine. `EchoBackend` provides deterministic no-credential wiring for tests.

## OpenAI-compatible transport

`OpenAiWireApi`, `OpenAiCompatibleBackend`, constructors/from-env/settings, endpoint normalization, model registry/capability checks, reasoning validation, retry/circuit configuration, idempotency keys, and `complete_openai` implement Chat Completions and Responses APIs. Both APIs accept bounded SSE/JSON variants and fold deltas into the same `Completion`; Responses is explicit or selected by environment while Chat remains compatibility default. Retry-after parsing, timeout slices, circuit state, and typed error mapping preserve cancellation and failure visibility.

## Attachments and response parsing

Chat/Responses attachment builders materialize workspace references only for the bounded request; journal text does not contain raw bytes. `chat_message`, `responses_input_items`, attachment source/data URL helpers, and internal turn-id stripping keep provider metadata separate. Content/refusal/annotation/tool-call/usage extraction validates response shape and terminal events. Duplicate tool calls are de-duplicated by identity and malformed function arguments return typed errors.

## Safety and limits

Attachment validation, response size caps, strict terminal validation, provider capability intersection, credential checks, endpoint normalization, and `map_ureq_error` make the boundary fail closed. The synchronous trait is intentional: independent processes provide concurrency without adding an async runtime to the default binary. The nested `chat_stream` and `transport` modules own wire details and cancellation slices.

## Mapping and limits

This file maps to pi-mono's provider request/stream normalization and model selection, while adding Zenpi's owner-bound attachments, circuit/retry state, explicit wire API choice, secret-handle support, idempotency, and strict Responses validation. It does not prove TUI/headless owner routing, journal persistence, or real PTY behavior; those are separate validators. The report covers all source bytes and does not infer those cross-file claims.
