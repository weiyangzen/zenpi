---
schemaVersion: research-memo/v1
generator: Codex
generatedAt: 2026-09-13
slug: provider-account-routing
mode: extension
researchMode: targeted
status: discussion-evidence
localRevision: be15ebd
question: Which provider/auth boundaries and quota/cache constraints should govern multi-account routing?
sources:
  - https://github.com/earendil-works/pi/blob/main/packages/ai/README.md
  - https://opencode.ai/docs/go/
  - https://opencode.ai/docs/zen/
  - https://help.aliyun.com/zh/model-studio/coding-plan
  - https://help.aliyun.com/zh/model-studio/coding-plan-faq
  - https://help.aliyun.com/zh/model-studio/qwen-api-via-openai-responses
  - https://www.volcengine.com/docs/82379/1795150
  - https://developer.volcengine.com/articles/7615528054736945158
  - https://developers.openai.com/api/docs/guides/prompt-caching
  - https://learn.chatgpt.com/docs/auth
  - https://openrouter.ai/docs/guides/overview/auth/oauth
  - https://code.claude.com/docs/en/legal-and-compliance
---

# Provider and Account Routing Evidence

## Summary

Use a Rust-native canonical provider boundary and a deterministic account router
over the existing runtime and resource owners. A provider is not an account, an
API key is not an independent quota pool, and a subscription is not unrestricted
API capacity. Sticky placement improves cache opportunities but cannot guarantee
remote KV reuse. This is evidence for a discussion draft, not product acceptance.

## Local Findings

| Component | Observed state | Direction |
|---|---|---|
| `Cargo.toml` | Rust dependencies include ureq/serde; no pi-ai or Node runtime | Keep native runtime |
| `src/backend.rs` | One OpenAI-compatible implementation; Responses and Chat enum; canonical request/event foundations; Chat explicitly non-streaming | Adapt and separate protocol adapters behind the existing boundary |
| `src/core.rs::make_backend` | Production construction resolves one profile; absent wire_api defaults to responses | Move immutable route resolution to each request boundary |
| `src/backend.rs::OpenAiWireApi::default` | Library-level default remains Chat Completions | Reconcile default call sites explicitly, not a blanket claim about CLI behavior |
| `src/config.rs` | Named profiles, static model catalog, API-key storage and selected Codex import | Migrate profile decoding to provider/account/model references |
| `src/config.rs::find_api_key` | Deliberately does not import arbitrary OAuth token fields | Preserve secret boundary; add explicit auth providers, not recursive token discovery |
| `src/governance.rs` | Session/worker reservation and settlement foundations | Extend accounting semantics; no parallel competing resource ledger |
| `src/runtime.rs`, `src/session.rs` | Owned jobs, cancellation and durable operation foundations | Reuse settlement/recovery, add account-scoped admission identity |

No live login, paid provider request, quota query, token inspection or runtime
test was performed for this research. Built-in support and end-to-end vendor
compatibility are separate acceptance claims.

## External Findings

### Library Scope Is Not zenpi Scope

The current upstream README lists OpenAI, Azure, Codex, Anthropic, Google,
Vertex, Bedrock, DeepSeek, Mistral, Groq, Cerebras, xAI, OpenRouter, OpenCode
Go/Zen, MiniMax, Moonshot, Kimi Coding, Qwen Token Plan, ZAI, Cloudflare,
Vercel, Together, Baseten, Hugging Face, NVIDIA, Fireworks, Ant Ling and MiMo,
plus compatible custom endpoints. Its OAuth section lists Anthropic, Codex,
Copilot and OpenRouter; OpenRouter exchanges authorization for an API key.
This is a mutable upstream snapshot, not dependencies or support inherited by
zenpi. Old fork README lists are not used as the current OAuth inventory.
[Upstream README](https://github.com/earendil-works/pi/blob/main/packages/ai/README.md).

### OpenCode Go and Zen

Both expose API-key access and model catalogs. Go uses `/zen/go/v1`; Zen uses
`/zen/v1`. Model routes vary across Responses, Chat Completions and Messages;
Zen also lists Google-protocol models. Go requests should identify the real
client and send a stable `x-opencode-session` per conversation. Go has layered
usage windows and an optional remote fallback to Zen balance: local
subscription-only policy cannot ensure zero paid overage without controlling
that setting. Model membership is mutable; do not bake this day's list into a
scheduler. [Go](https://opencode.ai/docs/go/), [Zen](https://opencode.ai/docs/zen/).

### Alibaba Billing Products Must Stay Separate

Coding Plan uses a dedicated key and endpoint and is distinct from ordinary
pay-as-you-go. The FAQ explicitly supports Chat Completions and Messages, not
Responses; concurrency is dynamically limited. Non-interactive bulk usage is
excluded, so those credentials cannot default into unattended batch pools.
[Plan](https://help.aliyun.com/zh/model-studio/coding-plan),
[FAQ](https://help.aliyun.com/zh/model-studio/coding-plan-faq).

Ordinary Model Studio separately documents Responses, workspace/region endpoints
and optional session-cache behavior. The page also says some unsupported fields
are ignored, making vendor capability profiles necessary. Its detailed cache
matching wording is internally inconsistent, so exact behavior needs fixtures
and provider confirmation rather than a copied universal rule.
[Responses reference](https://help.aliyun.com/zh/model-studio/qwen-api-via-openai-responses).

### Volcengine Evidence and Gap

The official Ark product page shows API-key requests to `/api/v3/responses`.
The service-assistant integration article distinguishes Coding Plan's
`/api/coding/v3` endpoint and plan model names from paid inference. Ordinary
inference support does not prove that a specific Coding/Agent Plan supports
Responses or permits arbitrary batch use. Attempts to fetch the current plan
and Codex integration pages failed; those exact plan routes, limits and terms
remain a release gate, not guessed configuration.
[Ark](https://www.volcengine.com/docs/82379/1795150),
[Service integration note](https://developer.volcengine.com/articles/7615528054736945158).

### Cache and OAuth Boundaries

Remote prompt caching depends on compatible rendered prefixes, model settings,
scope and server routing/lifetime. A stable session does not guarantee a hit;
OpenAI documents organization and processing-region isolation. Cache behavior
must be model/version-aware, not one universal TTL or cache-key rule.
[Prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching).

Codex documents ChatGPT subscription sign-in separately from API-key billing.
That alone does not establish a public third-party OAuth client registration
contract; zenpi's integration requires its own supported auth/endpoint evidence.
[Authentication](https://learn.chatgpt.com/docs/auth).
OpenRouter explicitly documents client PKCE authorization, including local and
headless interaction, that yields a user-controlled API key.
[OpenRouter auth](https://openrouter.ai/docs/guides/overview/auth/oauth).
Anthropic's current policy does not permit third-party developers to offer
Claude.ai login in their own apps or intermediate users' subscription tokens.
Do not promote an upstream library implementation to permitted product support.
[Credential-use policy](https://code.claude.com/docs/en/legal-and-compliance).

## Options Compared

| Option | Benefit | Cost |
|---|---|---|
| Native Rust adapters plus deterministic router | Reuses current ownership; no added process/protocol; transparent quotas | Must complete adapters and auth integrations with tests |
| JavaScript provider library bridge | Broader upstream catalog and auth implementations | Adds runtime/IPC/cancel/secret owners; incompatible with the current lightweight boundary without a separate decision |
| Remote gateway as sole router | Minimal local account policy and remote fleet management | Upstream identities, cache residency and quotas may be opaque |

## Recommendation

Choose native ownership with an explicit remote-routing policy for one gateway.
Keep provider definitions data-driven and protocol adapters shared. Separate
sticky session binding from per-request concurrency reservation. Route only
already-ready tasks; the router never becomes a second Goal/DAG engine.

## Risks and Acceptance Implications

- Same-owner keys may share organization/subscription limits; aliases cannot multiply capacity.
- Offline quota estimates must be labeled; unknown remote balance is not infinity.
- OAuth refresh rotation needs credential-scoped, cross-process exclusion.
- Observed remote usage may already include local calls; reconciliation cannot double-debit.
- Late requests after stop cannot be retried on another account just because a lease expired.
- Local strict spend policy cannot override hidden gateway fallback or external clients.
- Protocol, auth and plan availability require independent evidence before built-in support.
