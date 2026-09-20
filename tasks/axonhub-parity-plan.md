# Plan: AxonHub feature-range parity

Spec: `specs/SPEC-axonhub-parity.md`

Inventory: `docs/axonhub-capability-matrix.md`

## Global constraints

- The software is pre-release: redesign/rebuild the SQLite schema freely. Fresh initialization must remain idempotent; v0.1 experimental data compatibility is not required.
- SQLite is authoritative; DuckDB is derived analytics and must not block gateway traffic.
- Authorization fails closed and every control-plane mutation emits an audit event.
- Secrets remain encrypted; plaintext credentials never appear in API responses, logs, fixtures or screenshots.
- Do not copy AxonHub implementation source. Port public contracts and test invariants with attribution.
- No retry after downstream streaming bytes are committed.
- UI stays bilingual, theme-complete, keyboard accessible, responsive and restrained in motion.
- No completion claim without fresh Rust/Web/release/browser verification and GPT-6 final review.
- Every row in `docs/axonhub-capability-matrix.md` must finish with a verification label or an explicit upstream-todo/ADR disposition.
- Prefer mature, maintained, license-compatible crates/components over custom implementations. Wrap dependencies behind Pangolin interfaces. Hand-roll only missing product semantics or when a dependency conflicts with security, licensing or single-binary deployment; record the reason.

## Task 1: Versioned parity schema and migration contracts

Implement normalized SQLite schema and Rust query models for projects, memberships, invitations, roles, permissions, user-role bindings, OIDC providers/identities, API-key profiles, channel credentials/settings, model associations/prices, prompts/protection rules, threads/traces/requests/executions/usage, quota/probe state, webhooks, data storage and backup metadata. Seed system roles/default project. Add fresh/idempotent schema tests; pre-release v0.1 upgrade compatibility is explicitly not required.

Acceptance: all foreign keys/indexes exist, fresh setup seeds deterministic owner/default project/system roles, and repeated initialization is a no-op.

Verify: focused migration tests and `cargo test --locked db::`.

## Task 2: Authorization, projects, users, roles and OIDC

Implement principal/scopes, permission evaluator, project CRUD, memberships/invitations, user administration, role CRUD/assignments, scoped API-key ownership and OIDC PKCE/JIT/claim mapping. Add admin REST APIs and audit writes. Port relevant AxonHub authz/scopes/biz tests as Rust behavior tests.

Acceptance: cross-project access is denied, system/project roles resolve predictably, OIDC state is one-time and expiring, local login remains compatible.

Verify: authorization/OIDC integration tests.

## Task 3: Request orchestration engine

Implement API-key profile mapping, exact/regex/tag/condition candidate selection, channel eligibility, endpoint capability filters, sticky routing, failover/round-robin/weighted/least-inflight/latency strategies, concurrency/RPM/TPM admission, bounded queue, circuit breaker, health/quota state, retry policies, JSON request overrides and prompt protection. Port AxonHub orchestrator invariants.

Acceptance: deterministic selection order, concurrency-safe counters, safe streaming boundary and explicit diagnostic decisions.

Verify: strategy simulations, race tests and mock-upstream retry/stream tests.

## Task 4: Full protocol surface and provider strategies

Add OpenAI chat/responses/embeddings/images/audio/moderations/models, Anthropic Messages, Gemini generate/stream, rerank and provider auth/URL strategies for OpenAI-compatible, Anthropic, Gemini, Azure, Bedrock and Vertex plus compatible presets. Add request/response transformations and mock contract fixtures.

Pin the reusable pure-Rust crates from `https://github.com/BerriAI/litellm.git` at exact evaluated commit `8c4c394ecc82c4d6acb5eb371d8781e487894a17`: `litellm-types`, `litellm-core`, `litellm-llms`, `litellm-framing`, `litellm-auth`, `litellm-auth-aws`, `litellm-auth-azure`, `litellm-auth-gcp`, `litellm-http`, `litellm-token-counter`, `litellm-cache`, `litellm-cache-memory`, and optional `litellm-cache-redis`. Route them through Pangolin adapters; do not fork internals. Exclude Python bridge/host-python/legacy callbacks from the pure-Rust binary. Pangolin implements missing Gemini/image/rerank/video shapes and remains responsible for orchestration and tracing.

Add a versioned embedded provider/model catalog derived from public metadata and behavior in AxonHub plus design study of `QuantumNous/new-api`. Provider presets include default URL/auth/endpoints/discovery/quota capability and a license-safe logo key. Model cards include developer/type/modalities/reasoning/tool/temperature/vision/context/output/knowledge/release/cost defaults. Preserve source/version and operator overrides; do not copy AGPL code or trademark assets.

Catalogs are online-maintainable: local CRUD; versioned JSON import/export; multiple prioritized HTTPS subscription sources; bounded timeout/size; ETag/Last-Modified; optional/required Ed25519 signature with pinned keys; staged strict validation then atomic activation; last-known-good rollback; manual refresh/diff/status API. Merge order is local overrides > high-priority subscription > low-priority subscription > built-in, and remote updates never delete local custom entries or deliver executable code. Typed model/provider capability schemas preserve unknown extension fields and expose whether a local adapter exists.

The binary must bundle a useful offline catalog, not sample placeholders: major mainstream provider presets and a meaningful current model-card set with validated defaults. Tests assert nontrivial counts, representative providers/models, schema validity and fresh offline availability. Catalog subscriptions update the built-in baseline but are not required for first use.

Every provider preset carries a stable `logo_key`, optional brand color/monochrome preference and accessible name. Task 6 maps keys to bundled `@lobehub/icons` then Simple Icons with deterministic initials fallback; do not copy AxonHub/new-api image assets.

Acceptance: every documented endpoint routes through the orchestrator and preserves protocol-specific errors/streaming semantics.

Verify: endpoint contract test matrix derived from AxonHub public fixtures.

## Task 5: Trace, cost, quota and operations

Implement thread/trace/request/execution/usage lifecycle, immutable price versions and detailed cost items, channel probes, quota snapshots/backoff/auto-disable, retention GC, backup/restore and webhook delivery with retry. Project necessary analytics into DuckDB.

Add optional session compaction: exact encrypted replay remains default; at configured token/window thresholds use provider-native Responses compact when supported, otherwise a configured summarizer model; persist immutable summary version, covered history range/hash, model and usage; failure falls back to exact history. Preserve tool-call ordering and project/key isolation. Define a pluggable semantic-memory interface but do not inject long-term vector memory by default.

Add declarative channel-affinity rules inspired by new-api: match model/path/User-Agent; key sources limited to trusted header, JSON Pointer, trace/thread/session; `off/prefer/strict`; bounded TTL/capacity; switch-on-success and release-on-failure; hashed fingerprints only; Codex `prompt_cache_key` and Claude `metadata.user_id` safe defaults; cache-hit-token savings metrics. Use the LiteLLM cache interface with memory default and optional Redis. Explicitly disabled channels always invalidate affinity and WebSocket channel pin changes require reconnect.

Implement site-level request logging settings (`enabled`, default level, key override enabled, key disable allowed) and per-key `inherit/off/metadata/redacted_body/full_body`. Resolve one immutable effective policy at admission. `off` suppresses browsable request/trace/execution/content logs while preserving minimal authoritative usage/quota/cost and security-audit facts. Even `full_body` always strips credentials/cookies/authorization. Add policy matrix, mid-request setting-change and no-derived-event tests.

Backup implementation must also study [ca-x/raindrop](https://github.com/ca-x/raindrop) contracts: immutable target/config snapshots, revision fencing, encrypted credential envelopes outside public artifacts, atomic job claims, per-target outcomes, bounded retention restricted to owned prefixes, CSRF-protected mutations, and current-snapshot manual retry. Reuse the engineering pattern, not RSS-specific code.

Acceptance: every attempt is explainable and costed; backup round-trip preserves encrypted configuration; derived-store failure does not lose enforcement state.

Verify: lifecycle, cost, quota, backup and degraded-store tests.

## Task 6: Complete admin API and console

Expose CRUD/list/filter/pagination/enable-disable/test operations for every management entity. Add bilingual React pages matching the spec, with explicit loading/error/empty states, permission-aware navigation and accessible dialogs/tables/forms.

API-key creation supports `generated` and `import_existing` token modes. Imported keys accept existing provider/gateway formats subject to high-entropy length/no-whitespace/uniqueness validation, store an indexed deterministic lookup digest plus Argon2id hash, never retain/return plaintext, and authenticate without scanning all keys. UI presents an explicit mode switch and one-time security explanation.

Acceptance: an administrator can configure and diagnose all backend capabilities without editing database files or environment variables.

Visual acceptance from GPT-6 baseline review: improve Pangolin from 7.0/10 to at least AxonHub's supplied 8.1/10 evidence. Empty overview must name the no-data window and link to setup without nested dashed cards; unmeasured rates/latency display `—`, not invented zeroes; operational table body text is at least 14px; control-boundary contrast is stronger than panel separators; populated desktop/mobile overview, channels, models, request list and trace detail are verified in both themes; mobile metrics avoid four unnecessarily tall single-column cards.

Verify: TypeScript/Vitest plus browser workflows at 375/768/1440px in both themes/languages.

## Task 7: Compatibility ledger, screenshots, documentation and delivery

Create an AxonHub capability/test mapping document with implemented/tested status and external-validation notes. Generate at least six sanitized release-binary screenshots and add them to README: at least three desktop and at least three mobile, covering representative operational workflows. Update operator/API docs, migrations, environment variables and CI. Run GPT-6 final review, fix findings, merge and push only after green verification.

README references must include AxonHub, LiteLLM Rust, Traceloop Hub, Raindrop, new-api and cc-switch with URLs, licenses, evaluated commits, exact borrowed design ideas and an independent-implementation/no-affiliation statement.

After green local/remote verification, bump the pre-release version, create and push the release tag, publish GitHub binary archives/checksums, build and publish native `linux/amd64` + `linux/arm64` Docker images to GHCR, and verify the multi-architecture manifest. The user explicitly authorized these external publishing actions.

Merge the reviewed feature branch into local `main` first, rerun the full verification suite on the merge result, push `main`, and require its remote CI to pass before creating the release tag. Release assets must point at the merged main commit.

Acceptance: every upstream implemented feature is mapped; README screenshots are current; local CI plus remote CI/Release/Docker workflows are green; release assets and GHCR manifests are externally visible and verified.

Verify: full Rust/Web/release/browser/actionlint suite and GitHub Actions.
