# Spec: AxonHub feature-range parity

## Objective

Extend Pangolin / 鲮鲤 from a core gateway into an all-in-one AI development platform whose implemented feature range tracks AxonHub: enterprise access control, rich request orchestration, broad protocol/provider surfaces, complete tracing/cost accounting, quota and operational tooling, and corresponding bilingual administration UI.

Success means each supported behavior has a Rust contract test derived from AxonHub's public documentation or test invariant. It does not mean copying AxonHub source or claiming real-cloud verification without credentials.

## Capability requirements

### Identity and access

- Users, projects, invitations and project memberships.
- Roles and permission slugs with system roles plus project-scoped assignments.
- Session and API-key principals share the same authorization decisions.
- OIDC provider configuration, PKCE state validation, JIT identity linking and claim-to-role mapping.
- API keys belong to a project and may carry profiles: model mappings, allowed models, IP allow/deny lists, RPM/TPM quotas, budget and expiration.
- API-key creation supports generated tokens and administrator-supplied existing tokens for low-cost migration. Imported plaintext is never retained or returned; authentication uses an indexed deterministic lookup digest followed by Argon2id verification.
- Request logging policy is site-controlled and API-key-aware. The site owns the master enable/default/override/allow-disable settings; keys choose `inherit`, `off`, `metadata`, `redacted_body` or `full_body` only when permitted. Logging off never disables minimal authoritative usage/quota/cost and security audit facts.

### Channels, models and request orchestration

- Channels support multiple encrypted credentials, enable/disable, tags, endpoint mappings, model prefix/mapping rules, parameter overrides, retry status configuration, probe state and auto-disable policy.
- Model associations support exact/regex matching, tags, priority, weight and conditions.
- Candidate selection order is deterministic: access → profile mapping → model associations → channel eligibility → quota/health → load-balancing strategy.
- Strategies include failover, round-robin, weighted random, least in-flight and latency-aware selection.
- Sticky routing, circuit breaker, bounded queue/admission, per-channel concurrency/rate limits and safe retry boundaries.
- Request overrides use validated JSON merge templates; prompt protection supports role/content patterns, deny/redact actions and test mode.
- Streaming preserves SSE framing and terminal semantics; no retry occurs after downstream bytes are committed.

### Protocol and provider surface

- OpenAI: chat/completions, responses, embeddings, images generations/edits, audio speech/transcriptions, moderations and models/retrieve.
- OpenAI legacy completions, Responses compact/WebSocket, alpha search, videos and audio translations.
- Anthropic Messages and Gemini generateContent/streamGenerateContent.
- Jina-style embeddings/rerank and `/v1/rerank`.
- Doubao asynchronous content/video tasks and AI SDK compatibility behavior.
- Provider families include OpenAI-compatible, Anthropic, Gemini, Azure OpenAI, Bedrock and Vertex/GCP credential strategies, plus named compatible presets (OpenRouter, DeepSeek, Moonshot, Zhipu, Doubao, xAI, Groq, Ollama, NanoGPT, Jina).
- Provider setup helpers include Codex, xAI, Claude Code, Antigravity and GitHub Copilot OAuth/device flows.
- Cross-protocol transformations fail explicitly for unsupported shapes; they never silently drop fields.
- Pin and directly reuse the compatible `litellm-rust` crates at an exact Git commit for provider transformations they implement. Pangolin keeps an adapter boundary and owns orchestration, authorization, observability and missing protocols.

### Trace, cost and operations

- Thread → Trace → Request → RequestExecution → UsageLog relationships.
- Cross-upstream conversation continuity uses sticky routing plus encrypted exact session replay. Optional versioned compaction activates at configured token thresholds, preferring provider-native Responses compact and falling back to a configured summarizer; failures preserve exact history. Long-term semantic/vector memory is a pluggable provider, never implicit default context.
- Per-execution status, retry reason, channel credential suffix, timing/TTFT, usage dimensions and cost items.
- Price schedules and immutable price versions; input/output, cache read/write, reasoning, flat and per-unit cost components.
- Channel probes, provider quota snapshots and auto-disable/backoff state.
- A versioned embedded provider/model catalog supplies configuration presets, license-safe logo keys, endpoint/auth defaults and model cards (type, modalities, reasoning, tools, vision, limits, release metadata and default costs). Runtime/operator overrides take precedence and catalog source/version remain visible.
- Catalogs support online maintenance: typed local CRUD, versioned JSON import/export, multiple HTTPS subscriptions, conditional refresh, pinned Ed25519 verification, staged validation/atomic activation, last-known-good rollback and explicit adapter-availability status. Remote catalogs are declarative data and can never inject executable code.
- The release binary bundles a nontrivial offline catalog of current mainstream providers and model cards. A fresh offline instance can choose a preset, enter credentials and create a channel; subscriptions enhance rather than bootstrap the product.
- Every built-in provider has an offline `logo_key`, accessible display name and deterministic fallback. The console uses license-compatible `@lobehub/icons`/Simple Icons mappings with light/dark treatment; no third-party project image assets are copied.
- Data-storage policy, payload retention/GC and backup/restore with encrypted secrets preserved.
- Local/S3-compatible data storage, automatic backup scheduling and conflict strategies.
- Webhook notification for channel disable, quota and repeated request failure.
- OpenAPI GraphQL/service playground, request live preview/content download, branding/favicon and onboarding state.
- Audit query API and UI.

### Console

- Bilingual pages for dashboard, projects, users, roles, API keys/profiles, channels/credentials/probes, models/associations/prices, prompts/protection, requests/executions, threads, traces, usage, storage, backup, OIDC and system settings.
- Existing themes, keyboard behavior, responsive layout and restrained motion remain binding.
- README contains real release-binary screenshots for overview, channels, trace detail, access control and mobile/dark mode.

## Data design

SQLite/SeaORM remains the record system. New normalized tables use UUID text identifiers and integer UTC timestamps. Policy/condition/template documents use versioned JSON only where shape evolution is expected. Project ownership and authorization are relational constraints, not embedded JSON.

DuckDB remains a derived analytical store. Transactional request/execution/usage summaries needed for enforcement live in SQLite; high-volume immutable event payloads and aggregates live in DuckDB. Cross-store writes are best-effort derived projection, never a distributed transaction.

Background catalog/probe/quota/webhook/backup/GC work uses durable idempotent jobs or transactional outbox records, with lease/fencing claims and no upstream I/O inside SQLite transactions. Full decisions are documented in `docs/architecture/parity-data-design.md`.

## Commands

- Rust verification: `cargo fmt --all -- --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked`
- Web verification: `pnpm --dir web lint && pnpm --dir web test && pnpm --dir web build`
- AxonHub contract inventory is recorded in `docs/axonhub-capability-matrix.md`; repository URLs and evaluated commits are public documentation, never local filesystem paths.
- Release: `pnpm --dir web build && cargo build --release --locked`

## Testing strategy

- Port public behavior tables and fixtures, not Go implementation details.
- Unit tests cover mappings, conditions, policy decisions, costs and strategy selection.
- Mock-upstream integration tests cover every endpoint and streaming boundary.
- Schema tests verify fresh creation, complete constraints/indexes and idempotent initialization. Pre-release v0.1 data compatibility is not required.
- Browser tests exercise each top-level console capability in Chinese/English, light/dark and 375/1440px.

## Boundaries

- Always: fail closed for authorization, preserve secrets, use deterministic policy order, record mutation audit events, expose unsupported protocol shapes explicitly.
- Ask first: destructive data migration, externally publishing a release/tag, or requiring real paid provider credentials.
- Never: copy AxonHub implementation source, log credentials/bodies by default, claim a provider was live-tested when only mock-tested, retry after streaming bytes were sent.

## Success criteria

- Capability matrix has no unexplained missing row for an AxonHub feature marked implemented upstream.
- Every management entity is creatable, readable, editable, enable/disable-able and deletable where safe.
- Request orchestration tests cover exact/regex/tag candidates, five balancing strategies, rate/concurrency admission, sticky behavior, circuit breaking, overrides, prompt protection and streaming retry rules.
- All listed protocol endpoints pass mock contract tests.
- Authorization tests prove cross-project reads/writes are denied.
- Fresh databases pass complete and idempotent schema initialization tests.
- Release UI screenshots are regenerated and referenced by README.

## Open questions

None requiring a pause. Real-cloud credentials are treated as an external validation phase; mock contract coverage is the binding local criterion.
