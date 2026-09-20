# Task 4 — protocols, providers and versioned catalogs

Implementation base: `49a4e48a8a1d0f3fee38d2b073950a8511af9c4e`. Work is confined to `.worktrees/axonhub-parity`; unrelated controller specification/ADR/ledger edits are preserved.

## Implemented scope

The public route matrix now includes OpenAI legacy completions, Responses compact and WebSocket, models/retrieve, embeddings, moderations, alpha search, image generations/edits, videos create/get/delete, audio speech/transcriptions/translations, Anthropic aliases/models, Jina aliases/rerank, Gemini namespace/v1beta generation/stream/models, Doubao tasks, and AI SDK legacy/UI stream formats.

`api::protocols::Input` carries JSON, multipart, Gemini URL context and task lookup context into Task 3's shared executor. Candidate protocol identity is separate from the mapped upstream path so custom endpoint mappings cannot bypass token limits. Requests retain project/key/profile/model access, prompt/tool policy, admission, retry/circuit guards, trace identifiers and observations. Model discovery/retrieve use authorized orchestration candidates and now emit their own observation and request/trace headers. Streams never re-enter the retry loop after downstream commitment.

Multipart requests retain file bytes/filenames/MIME types, repeated file fields and transformed text/model fields. Speech retains binary bytes and Content-Type. Durable task rows bind project, API key, original provider, credential and upstream model; GET/DELETE cannot move to a different channel or key. Responses WebSocket has independent FIFO stream IDs, bounded queues, per-event timeout, cancellation and the existing encrypted durable Responses history.

Provider adapters include named compatible presets, Anthropic, Gemini, Azure, Bedrock and Vertex/GCP. Unknown cross-protocol fields and unsupported content forms return explicit errors. Native JSON/provider extensions are preserved. Provider presets can install declared endpoint mappings without overriding a user-supplied base URL.

## Exact LiteLLM reuse

Manifest/lockfile pin `https://github.com/BerriAI/litellm.git` at `8c4c394ecc82c4d6acb5eb371d8781e487894a17`.

- `types/core/llms`: checked provider eligibility and Anthropic text/Bedrock Converse transformations.
- `framing`: SSE parsing through Pangolin's adapter, with existing frame limits and protocol terminals.
- `auth`, `auth-aws`, `auth-azure`, `auth-gcp`: credential placement, SigV4, Azure token strategies and GCP token/provider caching.
- `http`: maintained HTTP defaults through the Pangolin client boundary.
- `token-counter`, `cache`, `cache-memory`: optional model-matched text counting and bounded hash/count cache.

The transitive `litellm-host` is a pure Rust interface crate. Python bridge, host-python, legacy callbacks and PyO3 are absent. No upstream internals were forked. GitHub TLS interruptions during initial dependency fetch were worked around by seeding Cargo's exact-commit cache from the evaluated local checkout; the committed dependency source remains the official Git URL.

## Added catalog requirements

The embedded version `2026-09-20.axonhub-cb29b65.pangolin-1` contains **59 provider presets and 453 model cards**. Factual metadata is projected from AxonHub's Apache-2.0 catalog at `cb29b65d9adfb06f89bb1b467418e0816988f36c`; provider/model source fields and a data NOTICE record provenance. New-api commit `972aed1972820389ea0b603ca58f03f846fbf790` supplied behavioral reference only; no AGPL code or assets were copied.

Provider cards include family/category, base URL, auth, endpoint defaults, explicit adapter availability, discovery/quota metadata, stable `lobehub:`/`simple-icons:`/`initials:` logo keys, initials fallback, monochrome preference and provenance. Model cards include developer/type/modalities/protocols, typed optional capabilities, extensible unknown capabilities, limits, aliases, lifecycle/dates and price defaults. Catalog membership never claims live adapter support or enables a channel automatically.

Versioned import/export, filtered APIs, local overrides and online subscription APIs are implemented. Merge precedence is local override > higher subscription priority > lower priority > built-in; subscriptions cannot delete local custom entries. Provider/model creation consumes defaults only when administrator fields are absent, preserving explicit prices/capabilities/base URLs and snapshotting model metadata.

SQLite sources/snapshots/overrides persist ETag, Last-Modified, last attempt/success/error, priorities, intervals, active/previous versions and verification metadata. Refresh requires public HTTPS destinations, pins validated DNS addresses, disables redirects/environment proxies, limits bytes/time, validates schema, optionally/requires pinned Ed25519 signatures, then atomically activates with a revision fence. Invalid, oversized, unsigned-required, tampered and stale refreshes preserve the prior active snapshot. Merge-size/schema validation occurs in the activation/import/configuration transaction. Source-scoped rollback and bounded history are available.

No subscription is configured automatically. The complete initial catalog remains available with the network disconnected.

## Schema and downstream seams

- Version 6: `protocol_tasks`, key/project ownership and original provider/credential/model bindings.
- Version 7: `catalog_sources`, `catalog_snapshots`, `catalog_overrides`, model catalog metadata, system `catalog:manage` permission.
- Task 5 scheduler calls `catalog::repository::due_sources` and `catalog::refresh::refresh`; service/manual APIs are complete, scheduler ticks remain Task 5.
- Task 5 still owns reliable streaming/media settlement, asset retention and full execution/cost accounting.
- Task 6 consumes `/api/admin/v1/catalog/{providers,models}`, preset IDs and logo keys; offline icon libraries/rendering and onboarding UI remain Task 6.
- Task 7 README attribution/release screenshots remain Task 7; catalog data licensing notice is already included.

## Evidence and verification

There are **96 Rust tests**, including 23 new Task 4 tests beyond the 73-test Task 3 baseline. Independent JSON fixtures cite AxonHub route/test sources and compare complete native request/response shapes. Integration tests exercise multipart bytes, speech binary, native Anthropic/Gemini terminals, async ownership, models, WebSocket continuation, AI SDK formats, catalog authorization/filtering/import/export and preset/default application. Provider contracts verify compatible auth, Azure deployment/version/token behavior, Bedrock signed body and Vertex project/auth URL construction.

Catalog tests cover nontrivial offline counts/key presets/logos, precedence, custom-entry preservation, unknown-capability roundtrip, ETag/Last-Modified 304, invalid/oversized payloads, required Ed25519 signatures, tampering, source revision fencing, rollback and network/URL rejection. Existing authorization, admission, retry, circuit and session suites remain green.

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo test --locked`: 96 passed, 0 failed.
- `pnpm --dir web lint`: passed.
- `pnpm --dir web test`: 1 passed.
- `pnpm --dir web build`: passed.
- `cargo build --release --locked`: passed for the final implementation.
- Release-binary smoke: fresh isolated SQLite/DuckDB, readiness 200, embedded Web 200, login 200, offline catalog 200 with 59 providers/453 models and zero configured subscriptions; server stopped cleanly afterwards.
- `git diff --check`: passed.

## Explicit limits and review notes

Providers are contract-tested, never live-tested. Cross-protocol Gemini/Anthropic/Bedrock support is deliberately narrower than native forwarding: unsupported tools/multimodal/stream shapes are rejected, not silently dropped. AI SDK currently accepts text messages/parts; nontext/tool/reasoning parts fail explicitly. Responses WebSocket `generate` is explicitly unsupported. Upstream WebSocket channel transports are rejected as unsupported presets; WebSocket ingress is implemented over the shared HTTP/SSE provider boundary.

Token counting uses a supplied `PANGOLIN_TOKENIZER_CL100K` vocabulary for recognized GPT-4/GPT-3.5 text shapes and conservatively falls back elsewhere. Media, cached context and multimodal TPM requests require a provider estimator. Budget-limited streaming/media remains disabled pending Task 5 settlement. Media streaming variants are explicitly rejected. Multipart input is bounded to 64 MiB/128 fields; upstream buffered response bound remains 16 MiB. Subscription documents/merged exports are capped at 4 MiB, at most 32 HTTPS sources are configured, and refresh history is bounded.

Self-review fixed multipart duplicate Content-Type, encoded Gemini key handling, custom-endpoint TPM identity, strict cross-protocol tool fields, native Gemini tool/protection coverage, original task credential binding, normalized protocol errors, source activation fencing/aggregate validation and discovery tracing. The DDIA skill informed snapshot transactions/revision fencing; verification-before-completion gates back the completion evidence. No external publishing or paid provider requests were made.

## Fix round 1/5 — review findings 2–5

Finding 1 (preserving old V2–V5 provider CHECK schemas) was explicitly ruled out of scope by the controller/user: this unreleased software may rebuild its fresh schema. No provider preservation migration was added.

The four in-scope findings are addressed:

1. **Internal-error disclosure:** one `ApiError` public projection strips internal anyhow/database/decryption/session causes. Anthropic/Gemini wrappers and WebSocket per-event errors use it, as do catalog batch/last-error reporting. Internal API logging no longer serializes the cause. A SQLite-trigger sentinel test proves HTTP native aliases/model lists and an already-upgraded WebSocket all return a generic message without the cause; direct wrapper tests cover arbitrary anyhow sentinels.
2. **Gemini query-key logging:** the production TraceLayer explicitly records method and URI path only. Captured debug/trace logging verifies both the Gemini `key` sentinel and unrelated query sentinels are absent while method/path remain observable.
3. **Native error contracts:** a shared protocol mapper covers handler failures, early JSON validation, extractor/body-limit failures and normalized upstream HTTP errors. Gemini uses canonical `UNAUTHENTICATED`, `PERMISSION_DENIED`, `RESOURCE_EXHAUSTED`, `INTERNAL`, `UNAVAILABLE` and related statuses. Anthropic uses authentication/permission/rate-limit/request/API types. Native SSE errors retain rate-limit classification while removing private details. Explicit upstream pass-through remains an intentional exception for provider response bodies, never for local internal causes.
4. **Document-level extensions:** fresh schema v7 now includes `catalog_document_extensions`. Local imports persist extension keys transactionally. Effective catalogs merge whole JSON values by top-level key in the same built-in → ascending subscription priority → local order as entries. Omitted keys do not delete local values; explicit null is preserved. Runtime provenance no longer overwrites publisher keys such as `applied_sources` or `builtin_version`. Dedicated tests verify subscription precedence, local precedence and export/import equality independently of model capability extensions.

Additional reviewed edges now have executable coverage:

- WebSocket lanes execute concurrently while each lane preserves FIFO. Reads remain independent of bounded writes, so disconnect cancels backpressured attempts promptly instead of waiting for the per-event timeout.
- Gemini terminal state accumulates completed candidate indices across frames and waits for every requested candidate.
- Multipart repeated file names retain each file's name, MIME and binary bytes; aggregate requests above 64 MiB are rejected before upstream contact.
- AWS signing matches the exact public botocore golden signature vector through Pangolin's cloud-auth boundary.
- A real TCP HTTP fixture exercises the production reqwest builder's DNS override and no-redirect behavior; HTTPS/public-address validation is separately tested. No real-cloud or external TLS-provider verification is claimed.

Validation for this round: `cargo fmt --all -- --check`, strict all-target clippy and the full **107-test** Rust suite pass. Web lint, the web test, production build and final stable-assets release rebuild pass. Fresh-binary smoke verified readiness/Web 200, Gemini 401 with `UNAUTHENTICATED`, catalog import/export 200 and document-extension preservation with 59 providers/453 models. Captured release debug logs contain method/path but no Gemini query-key sentinel; the isolated server stopped cleanly.
