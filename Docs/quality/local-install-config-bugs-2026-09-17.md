# Local install and config bugs, 2026-09-17

Scope: bugs found while installing zenpi locally (`cargo install --path .
--locked`) and wiring it to a local OpenAI-compatible gateway backed by a
ChatGPT Codex OAuth upstream account. The gateway chain during diagnosis was
`zenpi -> local dev proxy -> local gateway backend -> Codex OAuth upstream`,
serving a model that is not in zenpi's builtin model registry.

The binary build itself is clean: the release install compiled on the first
attempt with no errors. All findings below are in the "configure, then
actually use it" segment of the README quick start, which does not currently
converge in this environment without manual edits.

Reproduction environment notes are genericized; any OpenAI-compatible gateway
fronting a Codex OAuth (chatgpt.com backend) account with an uncatalogued
model should behave the same.

## Bug list

| # | Area | Severity | Summary |
|---|---|---|---|
| 1 | `config import-codex` | High | Imported `model_reasoning_effort` is rejected at runtime for any uncatalogued model; the imported profile can never serve a request |
| 2 | Responses wire | High | `max_output_tokens` is sent unconditionally, and Codex OAuth upstreams reject it with 400; the Responses wire is unusable against them |
| 3 | `config doctor` | Medium | `ready=true` is a purely static verdict and stays green in both failure modes above |
| 4 | Gateway error sanitization (external) | Medium | The gateway swallows the upstream 400 body, returning only "Upstream request failed", which hides which field was rejected |

## Bug 1: import-codex produces a profile that always fails

Symptom: immediately after a successful `zenpi config import-codex`, any
request exits with:

```
zenpi: backend configuration: selected model or wire does not support reasoning effort
```

Trigger:

1. The source Codex config sets `model_reasoning_effort` (a common Codex
   default).
2. Run `zenpi config import-codex --profile NAME`.
3. Run any prompt, e.g. a headless `{"type":"prompt", ...}` request.

Root cause:

- `src/config.rs:821` copies Codex's `model_reasoning_effort` verbatim, with
  no validation.
- At runtime, `validate_reasoning` (`src/providers/registry.rs:84-98`)
  requires the model descriptor to advertise the requested level. A model
  absent from the registry falls back to `unknown()`
  (`src/providers/registry.rs:384`), which sets `reasoning: false` and an
  empty `reasoning_levels` set. Any imported effort value is therefore
  rejected for every uncatalogued model.

Workaround: delete the `model_reasoning_effort` line from the imported
profile in `~/.zenpi/config.toml`.

Suggested fix: validate the effort against the target model descriptor during
import (drop the field with a warning when unsupported), or let the
`unknown()` fallback pass effort through for uncatalogued models.

## Bug 2: Responses wire always sends `max_output_tokens`; Codex OAuth upstreams reject it

Symptom: with `wire_api = "responses"`, every request fails with
`backend: backend HTTP status: 400` (surfaced through the gateway as a
generic upstream error).

Trigger:

1. Point a profile at a gateway that transparently forwards Responses
   payloads to a ChatGPT Codex OAuth upstream, with an uncatalogued model.
2. Send any prompt.

Root cause, confirmed by replaying the captured request body field by field:

- zenpi's `instructions`, `store: false`, and `stream: true` all match the
  upstream's requirements.
- The single field that triggers the upstream 400 is `max_output_tokens:
  4096`. Removing it makes the identical request succeed; no other field
  matters.
- `src/backend.rs:1170` injects the token limit whenever a model descriptor
  exists or the request carries one. The `unknown()` fallback guarantees a
  descriptor always exists (`UNKNOWN_MAX_OUTPUT = 4096`,
  `src/providers/registry.rs:16`), and the normal turn path additionally sets
  it explicitly (`src/core.rs:3759`). There is no configuration that omits
  the field on the Responses wire.
- The Codex OAuth backend rejects this field; the official Codex client does
  not send it.

Impact: zenpi's primary Responses protocol cannot talk to Codex OAuth class
upstreams at all. Chat Completions is unaffected because the gateway
translates the protocol itself and drops the field.

Suggested fix: on the Responses wire, omit `max_output_tokens` unless the
user configured an explicit output limit, or add a `model_overrides` switch
that suppresses the field.

## Bug 3: `config doctor` reports ready in both failure modes

Symptom: `zenpi config doctor --profile NAME` prints `ready=true` while Bugs
1 and 2 make every real request fail.

Root cause: `status_for_profile` (`src/config.rs:1420`) is static resolution
only: the config parses and an API key exists, so the profile is "ready". It
sends no probe request and runs neither the reasoning-effort validation nor
any wire-compatibility check.

Suggested fix: add an optional minimal live probe to doctor, and include
locally decidable checks such as `validate_reasoning`.

## Bug 4 (external): gateway sanitizes upstream error bodies

The local gateway answers a rejected upstream call with
`{"error":{"message":"Upstream request failed","type":"upstream_error"}}`
and logs only `upstream error: 400 (client response sanitized)`. The upstream
body that names the rejected field is discarded, so this class of failure can
only be diagnosed by replaying captured traffic.

This is not a zenpi bug, but zenpi users behind such gateways see an
opaque 400 for what is actually a request-shape incompatibility. Surfacing
the upstream error message in the gateway log would have made Bug 2 a
one-step diagnosis.

## Verified working configuration

After removing the imported `model_reasoning_effort` and switching the
profile to `wire_api = "chat_completions"`, a headless prompt completes
successfully end-to-end (assistant reply returned, correct model and usage
reported). The Responses wire remains unusable in this environment until
Bug 2 is fixed.
