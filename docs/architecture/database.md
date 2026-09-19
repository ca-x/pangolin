# Database architecture

## Why two databases

Pangolin has two different access patterns:

1. Configuration, identity and routing are OLTP data: small random reads/writes, uniqueness constraints and atomic updates.
2. Request observations are append-heavy and queried through scans, time buckets and percentiles.

Using one SQLite schema for both makes analytical scans compete with routing and setup writes. Using DuckDB for control state weakens concurrent transactional ergonomics. Pangolin therefore uses SQLite through SeaORM as the record system and a separate DuckDB file as the analytical observation system.

## SQLite schema

| Table | Purpose | Important constraints |
| --- | --- | --- |
| `users` | Local administrators | normalized email unique; Argon2id hash |
| `sessions` | Web sessions | token hash unique; expiry indexed |
| `settings` | Instance and capture settings | key primary key |
| `providers` | Upstream endpoints and encrypted credentials | name unique; secret envelope never returned |
| `models` | Provider model definitions and public aliases | provider/public/upstream tuple unique; priority orders same-alias targets |
| `api_keys` | Virtual gateway credentials | prefix indexed; full Argon2id hash only |
| `audit_events` | Control-plane mutations | append-only by application policy |

Money is stored as integer micro-USD and timestamps as UTC RFC3339/Unix microseconds. JSON is limited to evolving capability/metadata fields; relationships and constraints remain normalized.

## DuckDB schema

`request_events` is append-oriented and partitionable by `started_at`. Payload columns are nullable and disabled by default. Common filter columns are first-class typed columns rather than fields buried in JSON.

DuckDB has one writer owned by a background worker. The query path opens short-lived connections in blocking tasks. Event persistence is best effort: request serving has priority over telemetry completeness, and loss is surfaced through metrics. The writer batches up to 64 queued events per transaction; startup removes events older than the configured retention window.

## Evolution

- Idempotent startup migrations execute through SeaORM's connection/transaction interfaces and record their schema version.
- DuckDB migrations are versioned in a metadata table and are additive in v0.x.
- Deleted columns are not reused with different meaning.
- Future PostgreSQL support belongs to the control plane only; the observation-store trait can later target ClickHouse or object-storage Parquet without changing gateway contracts.
