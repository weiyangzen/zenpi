# ZS1-030 / SRC-0917 — packages/coding-agent/src/core/model-registry.ts

Status: [_] worker candidate. Source `/Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/src/core/model-registry.ts`: 5139 bytes, 173 lines, SHA256 `b94ea3640c3df228eda475665fb10e4812b88a9e98f336349893e73a75ec91e9`. Entire file read consecutively 1–173, hash matches frozen blueprint. No edits or execution of source code.

## Complete behavior by method

Imports/reexports (1–29) expose pi-ai model/stream/auth/provider types, ModelRuntime and ProviderConfigInput; ResolvedRequestAuth is a tagged success (optional key, headers, base URL, environment) or error result. clearApiKeyCache is reexported from provider-composer and is not implemented here.

ModelRegistry is explicitly a synchronous extension compatibility facade, while coding-agent internals use ModelRuntime. Constructor stores that runtime. refresh delegates to runtime.refresh and returns a Promise; callers must await it before synchronous reads. getError returns runtime error state. getAll and getAvailable copy the outer arrays of runtime model snapshots; they do not deep-copy model objects or themselves read/validate models.json. find uses exact provider and model ID through getModel. hasConfiguredAuth delegates at provider granularity, not by checking network validity or model entitlement.

getApiKeyAndHeaders (66–96) awaits runtime.getAuth(model). An absent resolution consults compatibility config: authHeader requires an API key and returns a named missing-key error, otherwise headers-only success is allowed. A present resolution returns key/headers/optional nonempty baseURL/env. Exceptions prefer an Error cause's message, then Error.message, then String(error); the exact authHeader error is normalized to a provider missing-key message. This is a runtime string normalization rule, not proof errors are redacted or safe to persist.

getProviderAuthStatus and getProvider delegate directly. stream and streamSimple pass model/context/options to runtime for request-time auth; complete returns the runtime Promise. getProviderDisplayName falls back to the provider ID. getProviderAuth delegates raw auth, whereas getApiKeyForProvider catches every exception and returns undefined, losing the distinction between no credential and resolution failure. isUsingOAuth uses the provider ID.

registerProvider has two overloads: name+config requires a truthy config or throws before delegation; native Provider objects use registerNativeProvider. unregisterProvider and the registered config/native-provider/ID getters delegate unchanged. There is no new process, internal storage, lock, transaction, cancellation loop, resolver implementation or provider HTTP call defined in this file.

## Context and tested source expectations

Read the model-registry.test.ts setup (1–110) and indexed all test titles; read selected behavior assertions around 865–928 and 1398–1468. The full 2012-line test file was not audited and its tests were not run. Tests cover provider baseURL/header inheritance, custom-model replacement by ID, model compat precedence, refresh/removal, partial cost override, dynamic provider registration/OAuth, auth and caching. A source test explicitly ignores a nonexistent modelOverride without error; zenpi's requested no-silent-remapping rule should reject or diagnose that case instead of copying the silent behavior.

API-key test fixtures can use !-prefixed shell commands to obtain keys and trim output, swallowing failed command lookup. That behavior belongs to provider-composer/runtime dependencies; this facade merely reexports/forwards it. Zenpi should retain its existing explicit secret handle/config owner and must not introduce implicit shell evaluation as part of a model catalogue. This review did not execute any such commands.

Inspected ModelRuntime method locations as dependency context: model getters delegate to models, availability is a cached snapshot, getAuth resolves per request, refresh has options including allowNetwork/signal and provider registration triggers asynchronous refresh. This is not a complete ModelRuntime audit, and the facade alone cannot substantiate atomic refresh/rollback claims.

## Target gaps and implementation boundary

Zenpi baseline model_catalog lists named profiles (or one flat default), including locally configured presence but no model context/output/reasoning/modality descriptors. ProviderCapabilities derives support entirely from the chosen OpenAI wire. ZS1-107 needs a bounded exact (provider, model) registry with source/version provenance and explicit overrides; transport authentication remains in existing EffectiveConfig/SecretHandle/backend setup. /models must present the selected records and actual wire/model negotiation rather than a profile count masquerading as model capabilities. Unknown capability is conservative; absent context is finite; nonexistent exact IDs must not resolve by substring or alias guessing. Profiles remain compatible as named selections of a provider/model.

A local catalogue query should be deterministic and produce no network/auth command requests. Candidate loading/validation and cancellation should publish a complete registry or preserve its previous state. Restart must re-read selected declarative paths/config and preserve identity/provenance; it must not invent online availability from stored credentials. Source refresh can allow network, but the requested zenpi query path need not copy that optional feature.

## Evidence required

Independent Rust tests should validate field-level override provenance, unknown fields and duplicate identities, wire upper bounds, unsupported image/file/structured/tool/reasoning inputs before network dispatch, model-specific context/max-output budgets, exact unknown-model diagnostics, profiles/CLI/env precedence and restart consistency. Actual /models and provider requests must use the owner. Auth expiry/provider failures must remain typed instead of being silently converted to availability. This report is a candidate, not a master acceptance receipt or per-directory integration report.

## Controller review against current integration

Controller independently read every source line 1–173 (bytes 0–5139), this full candidate report, and context-only source test assertions865–928 and1398–1468. All facade methods and error branches above match the source. The test file and ModelRuntime remain outside this per-file acceptance. Source tests were not run.

Current target107 now has exact model descriptors in src/providers/registry.rs, field provenance and config overrides; src/backend.rs negotiates model and wire capabilities before requests and rejects unsolicited tool calls from text-only models. src/core.rs persists selection and effort, validates before state changes, restores by descriptor digest, and clamps context/output budgets. The new registry/effort targets exercise real HTTP requests, failed switch atomicity, restart and unsupported attachments/tools. Source auth forwarding, dynamic native registration and network refresh are not implemented by the local catalogue; existing config/secret handle owner remains responsible for credentials. Native adapters109/110 and full product107 acceptance remain open.

The explicit unknown-model descriptor and user override creation are intentional target behavior, unlike the source test that silently ignores a nonexistent override. This is a complete file-understanding acceptance only; directory056 and product107 require separate evidence and dependency closure.
