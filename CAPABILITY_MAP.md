# Capability Map: Pangolin

Pangolin（中文正式名称：鲮鲤）is a self-hosted AI API gateway and operations console. The modules below are independently testable and intentionally ordered by dependency.

| Module id | Responsibility | Depends on |
| --- | --- | --- |
| `foundation` | Runtime configuration, setup lifecycle, authentication, SQLite/SeaORM source of truth, secret protection, embedded Web assets | — |
| `gateway` | OpenAI-compatible and Anthropic-compatible endpoints, provider/model catalog, virtual API keys, routing, retries, health and cost accounting | `foundation` |
| `observability` | Separate DuckDB event store, request timeline, aggregates, payload redaction, retention and Prometheus metrics | `foundation`, `gateway` |
| `console` | React administration UI, setup wizard, dashboard, providers/models/keys/request detail, themes, Chinese/English and accessibility | `foundation`, `gateway`, `observability` |
| `delivery` | Docker image, embedded single binary, GitHub Actions CI/releases, multi-architecture publishing and operator documentation | all modules |

Build order: `foundation` → `gateway` → `observability` → `console` → `delivery`.

## Scope decisions

- Single-node is the supported v0.1 deployment. The database model and APIs avoid assumptions that prevent a future PostgreSQL/control-plane mode.
- SQLite is the authoritative OLTP store. DuckDB is a separate, disposable/rebuildable analytical store for request observations.
- Pangolin ships a useful OpenAI-compatible gateway first. LiteLLM Rust is an architectural reference; v0.1 keeps a replaceable local adapter because the upstream crates are not independently published and resolving the full Git workspace materially harms normal builds. Unsupported streaming shapes use transparent provider passthrough rather than lossy conversion.
- Enterprise SSO, distributed rate limiting, billing invoices and multi-region consensus are explicitly outside v0.1.
