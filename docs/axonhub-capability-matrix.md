# AxonHub → Pangolin capability matrix

This inventory is the parity source of truth. It is cross-checked against AxonHub's Chinese/English documentation, `internal/ent/schema`, HTTP routes, GraphQL schemas, frontend feature folders, scheduler/backup/GC packages and `_test.go` inventory. “Target” means Pangolin must implement and test the observable behavior; it does not mean copying upstream implementation code.

## Public gateway and protocol routes

| Capability | AxonHub evidence | Pangolin parity target |
| --- | --- | --- |
| OpenAI chat completions + SSE | `/v1/chat/completions`, orchestrator streaming tests | Full transform/pass-through, safe terminal/retry semantics |
| Legacy completions | `/v1/completions` | JSON + stream compatibility |
| Responses HTTP | `/v1/responses` | Full pass-through/transform and session continuity |
| Responses compact | `/v1/responses/compact` | Compact request/response path |
| Responses WebSocket | `GET /v1/responses`, websocket tests | Long-lived response.create loop and per-event timeout |
| Model list/retrieve | `GET /v1/models`, wildcard retrieve tests | API-key-aware routability, metadata and hidden-model rules |
| Embeddings | `/v1/embeddings`, trace embedding tests | OpenAI-compatible and cross-provider routing |
| Moderations | `/v1/moderations` | OpenAI-compatible routing and trace shape |
| Alpha search | `/v1/alpha/search` | Candidate/search response contract |
| Image generations | `/v1/images/generations` | JSON generation contract |
| Image edits | `/v1/images/edits` | Multipart forwarding and transform |
| Videos create/get/delete | `/v1/videos`, video storage/tests | Async task lifecycle and storage-aware responses |
| Audio speech | `/v1/audio/speech`, speech tests | Binary response forwarding |
| Audio transcription | `/v1/audio/transcriptions` | Multipart audio request |
| Audio translation | `/v1/audio/translations` | Multipart audio request |
| Anthropic via `/v1/messages` | OpenAI-compatible alias | Native and transformed messages |
| Anthropic namespace | `/anthropic/v1/messages`, models | Native auth, messages and list models |
| Rerank | `/v1/rerank` | OpenAI-style alias |
| Jina embeddings/rerank | `/jina/v1/*` | Native Jina contract |
| Gemini native + alias | `/gemini/:version/models/*action`, `/v1beta` | generateContent, streamGenerateContent, list models |
| Doubao video/content tasks | `/doubao/v3/contents/generations/tasks` | Create/get/delete async tasks |
| AI SDK compatibility | `aisdk.go` | Header/body compatibility contract |
| Realtime | README marks Todo | Upstream Todo; explicitly not a parity blocker |

## Identity, access and external authentication

| Capability | Target behavior |
| --- | --- |
| Users and owner | Local password, profile/avatar/language, password change, owner semantics |
| Projects | CRUD, selection, project ownership and isolation |
| Memberships and invitations | Invite token inspect/register, member role/scopes, removal |
| Roles and permissions | System/project roles, permission slugs/levels, assignments and effective scopes |
| API-key types | User, service account, personal and no-auth semantics |
| API-key status/scopes | Enable/disable/expiry/scopes, allowed IPs and project ownership |
| API-key token mode | Secure generated token or one-time import of an existing high-entropy token; indexed lookup + Argon2id, no plaintext recovery |
| API-key profiles/templates | Model mappings, allowed models, quota/routing policies and reusable templates |
| OIDC providers | Discovery/config, authorize/callback/exchange, PKCE/state, JIT and manual link/unlink |
| Codex OAuth | start/exchange and auth JSON decode |
| xAI OAuth/SSO | start/exchange/SSO decode |
| Claude Code OAuth | start/exchange |
| Antigravity OAuth | Explicitly unavailable until Code Assist project resolution and refresh-backed provider auth are implemented |
| GitHub Copilot device OAuth | Explicitly unavailable until GitHub-to-Copilot token exchange and provider headers are implemented |
| IP security | Global blocklist, per-key allowlist, request-log ban affordance |
| OpenAPI GraphQL auth | Intentional REST-only divergence D6; one authorization/error/audit surface ([ADR 0003](adr/0003-rest-only-control-plane.md)) |

OIDC branding intentionally diverges from AxonHub's remote `icon_url` field.
Pangolin stores `logo_key`, resolves it only through the bundled `ProviderIcon`
catalog, and uses deterministic initials for unknown keys. This prevents an
administrator-supplied value from causing a remote image fetch on the public
sign-in page; no provider trademark asset is copied into Pangolin.

## Channels, credentials and models

| Capability | Target behavior |
| --- | --- |
| Channel CRUD/bulk | Create, clone/merge, update, delete, enable/disable and bulk operations |
| Channel families | OpenAI, Anthropic, Gemini, Azure, Bedrock, Vertex/GCP and compatible presets |
| Provider preset catalog | Versioned built-ins for display name/category/logo key/default Base URL/auth/default endpoints/model discovery/quota capabilities/source, with admin override and safe fallback |
| Catalog online maintenance | Local CRUD, import/export, prioritized signed HTTPS subscriptions, ETag refresh, diff/status, atomic activation and last-known-good fallback |
| Multiple credentials | Encrypted key list, OAuth/GCP credentials, suffix identification and per-key disable state |
| Credential recovery | Invalid/encryption recovery path without leaking plaintext |
| Proxy settings | HTTP/SOCKS URL, username/password, connection reuse policy |
| Endpoint mappings | Per-format path/base URL/transport and provider-family routing |
| Model discovery/sync | Fetch/query channel models, manual models, scheduled auto-sync |
| Model transformations | Prefix, lowercase, explicit mappings, auto-trim, hide original/mapped |
| Model protocol policy | Per-model API formats and stream policy |
| Model cards | Reasoning/tools/temperature/vision/modalities/cost/limits/knowledge/release metadata |
| Model catalog defaults | Versioned developer/model metadata snapshot, fallback + refresh/cache, aliases/provider mappings and operator override |
| Extensible model capabilities | Typed common capabilities plus preserved unknown extension values, adapter availability and no-code-update discovery for existing protocols |
| Model CRUD/bulk | Create/update/archive/enable/disable/delete, routability-aware listing |
| Associations | channel+model, channel regex, global regex/model, channel tags+model/regex, exclusions |
| Conditions | Nested AND/OR field/operator/value filters |
| Developer settings | Per-developer associations, inheritance control and reasoning-effort mapping |
| Pricing | Channel price entries, immutable versions, schedule/timezone/overrides |
| Pricing modes | Flat, per-unit and tiered pricing; cache-write variants |
| Channel probing | Scheduled probes, success/TTFT/TPS history and test prompts |
| Provider quota collection | Provider-specific normalization, periods, URLs, snapshots and backoff |
| Auto-disable | Status/error pattern counters, duration/cron recovery and API-key/channel actions |

## Request orchestration

| Capability | Required invariant |
| --- | --- |
| Request source/thread/trace middleware | API/playground source, supplied/generated IDs, context propagation |
| API-key profile mapping | Rewrite requested model before candidate selection |
| Candidate generation | Exact/regex/model-id/tag/condition candidates with deterministic priority |
| Access filtering | Project/key/model/protocol permissions before routing |
| Quota filtering | Provider quota and per-key/channel budget/rate availability |
| Sticky routing | Trace/session stickiness modes and deterministic fallback |
| Load balancing | Failover, round-robin, weighted random, least in-flight, latency/adaptive composite |
| Admission control | RPM, TPM, max concurrent, queue size and queue timeout |
| Circuit breaker | Failure-window state, half-open recovery and model/channel isolation |
| Retry policy | Global/per-channel limits, delay, retryable status/error patterns and transport errors |
| Streaming policy | First-event timeout, EOF/terminal event handling, no post-commit retry |
| Empty response detection | Configurable empty-success treatment |
| Upstream error policy | Pass-through, normalized or custom messages |
| Header/body overrides | Ordered conditional JSON operations and header mutation |
| Pass-through modes | Body and User-Agent pass-through controls |
| Transform options | Developer/system role normalization, array forcing and reasoning effort |
| Prompt injection/actions | Prompt records with activation conditions and enable/bulk lifecycle |
| Prompt protection | Regex matcher, deny/redact replacement, scopes and preview/test mode |
| Allowed tools | Tool allow/filter behavior without corrupting message order |
| Auto reasoning effort | Developer/model-aware effort inference |
| Responses sessions | Session continuity and compact behavior |
| Cross-upstream context economy | Sticky route, durable exact replay, optional threshold compaction, prompt-cache hints; bounded project/key-scoped semantic memory remains opt-in, with optional scoped-candidate reranking |
| Channel/API-key request tracking | In-flight and terminal counters are concurrency safe |

## Request lifecycle, observability and analytics

| Capability | Target behavior |
| --- | --- |
| Thread | Group related traces and preserve client threading headers |
| Trace | End-to-end logical call with API-key/project/user attribution |
| Request | Inbound normalized request, content policy and source/IP metadata |
| Site/key logging levels | Site master/default/override policy plus per-key inherit/off/metadata/redacted/full body; secrets always excluded; accounting/audit facts remain |
| Request execution | One row per channel/credential attempt, timing, error/retry and chosen model |
| Usage log | Input/output/cache/reasoning tokens, request units and cost references |
| Detailed cost | Price item, quantity, tiers, cache TTL variants and subtotal |
| Live preview | Authorized in-flight request snapshot |
| Content download | Authorized stored request/response body retrieval |
| Dashboard | Requests/errors/latency, top channels/models/keys/projects/users |
| Analytics | Date and dimension filters; daily and overview stats |
| Performance analytics | Throughput, TTFT, latency and confidence per channel/model |
| Cost analytics | Channel/model/key/user/project breakdown |
| Retention and GC | Per-resource cleanup policy, body cleanup and database vacuum |
| Metrics/logging | Prometheus-compatible gateway/admission/limiter/queue metrics and redacted logs |

## Operations and system management

| Capability | Target behavior |
| --- | --- |
| System initialization/onboarding | Owner, brand name/logo/title and onboarding progress |
| System retry/model/quota settings | Global defaults with per-resource overrides |
| CORS and request timeouts | Operator configuration with safe defaults |
| Data storage | Local and S3-compatible storage configuration |
| Backup/restore | Selective resources, conflict strategies, secret preservation and validation |
| Automatic backup | Schedule/frequency/storage/retention/status and manual trigger |
| Webhooks | Targets, headers/body template, subscriptions, echo and delivery retry |
| Scheduler | Probe, model sync, quota collection, backup and GC jobs |
| Favicon/static SPA | Embedded branded assets and deep links |
| Playground/chat | Admin-selected channel/model request testing |
| Request content policy | Store chunks/body/live preview toggles |
| Provider OAuth credential helpers | Codex/xAI/Claude Code setup flows; Antigravity/Copilot fail closed pending complete provider adapters |

## Console feature pages

Dashboard; analytics; API keys/profiles; channels/credentials/probes/prices; models/associations; playground/chats; projects/members; roles; users; prompts; prompt-protection rules; requests/live/content; threads; traces/executions; usage statistics; data storage; backup/restore; OIDC/system/security/onboarding/settings.

## Verification labels

- `implemented+tested`: local behavior and contract tests pass.
- `contract-tested`: provider protocol verified against mock fixtures, no real credential claim.
- `live-tested`: verified with an explicitly configured real external provider.
- `upstream-todo`: not implemented by AxonHub itself and not a parity blocker.

No row may disappear silently. Any intentional divergence requires an ADR, test and README disclosure.
