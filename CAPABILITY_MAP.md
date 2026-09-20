# Capability Map: Pangolin

Pangolin（中文正式名称：鲮鲤）is a self-hosted AI API gateway and operations console. The modules below are independently testable and intentionally ordered by dependency.

| Module id | Responsibility | Depends on |
| --- | --- | --- |
| `foundation` | Runtime configuration, setup lifecycle, authentication, SQLite/SeaORM source of truth, secret protection, embedded Web assets | — |
| `gateway` | OpenAI-compatible and Anthropic-compatible endpoints, provider/model catalog, virtual API keys, routing, retries, health and cost accounting | `foundation` |
| `observability` | Separate DuckDB event store, request timeline, aggregates, payload redaction, retention and Prometheus metrics | `foundation`, `gateway` |
| `console` | React administration UI, setup wizard, dashboard, providers/models/keys/request detail, themes, Chinese/English and accessibility | `foundation`, `gateway`, `observability` |
| `delivery` | Docker image, embedded single binary, GitHub Actions CI/releases, multi-architecture publishing and operator documentation | all modules |
| `identity-access` | Projects, users, invitations, roles, permissions, memberships, OIDC identities and scoped API keys | `foundation` |
| `request-orchestrator` | Candidate generation, mappings, tags/conditions, sticky routing, load balancing, admission limits, retry/circuit breaker, overrides and prompt protection | `foundation`, `identity-access`, `gateway` |
| `protocol-surface` | OpenAI, Anthropic, Gemini, embeddings, rerank, images, audio, moderation and provider-family compatibility | `request-orchestrator` |
| `trace-cost` | Threads, traces, request executions, usage logs, price versions, quota snapshots, storage policies and webhooks | `request-orchestrator`, `observability` |
| `operations` | Backup/restore, garbage collection, probes, model discovery/sync, provider quota checks and system policies | `identity-access`, `request-orchestrator`, `trace-cost` |

Build order: `foundation` → `gateway` → `observability` → `console` → `delivery`.

Full-parity continuation order: `identity-access` → `request-orchestrator` → `protocol-surface` → `trace-cost` → `operations` → expanded `console` → `delivery`.

## Scope decisions

- Single-node is the supported v0.1 deployment. The database model and APIs avoid assumptions that prevent a future PostgreSQL/control-plane mode.
- SQLite is the authoritative OLTP store. DuckDB is a separate, disposable/rebuildable analytical store for request observations.
- Pangolin pins compatible LiteLLM Rust crates at one evaluated Git commit behind a local adapter boundary. Pangolin owns authorization, orchestration, observability and protocol shapes the upstream crates do not provide; unsupported conversions fail explicitly rather than dropping fields.
- OIDC SSO and single-node rate/admission controls are included. Distributed multi-node coordination, billing invoices and multi-region consensus remain outside this release.

## Full-parity continuation

- The target is feature-range parity with AxonHub's currently implemented public behavior, including its request orchestration strengths. AxonHub's own Realtime API remains outside scope while it is marked Todo upstream.
- AxonHub tests are semantic references. Pangolin ports observable contracts and invariants into Rust tests; it does not copy Go implementation code.
- Single-node SQLite remains the authoritative deployment for this release. Redis/distributed coordination is represented by replaceable interfaces and local equivalents, not falsely advertised as multi-node support.
