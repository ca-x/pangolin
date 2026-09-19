# Spec: observability

## Objective

Make gateway behavior explainable without mixing high-volume event scans into the transactional control database.

## Storage decision

DuckDB is embedded with its bundled library and writes to `observability.duckdb`, separate from `pangolin.db`. SQLite remains authoritative for control-plane state. This follows the DDIA record-system/derived-system split: OLTP indexes and constraints stay predictable while columnar scans serve time-series and request analysis.

The deployment remains one binary and one data directory. The accepted costs are a larger binary and slower native builds. Native GitHub runners for each release architecture avoid cross-compiling DuckDB's C++ core.

## Event model

`request_events` stores request id, trace id, timestamps, endpoint, key id, route, provider, requested/resolved model, HTTP status, error class, latency, time-to-first-token, token counters, integer cost, payload capture state and optional redacted JSON payloads.

## Reliability rules

- A dedicated blocking writer owns the DuckDB write connection.
- Gateway tasks send events through a bounded channel; writes are batched.
- Queue pressure increments a dropped-events metric instead of delaying model traffic; the writer batches up to 64 queued events per transaction.
- Payload capture is disabled by default; secrets and authorization headers are never captured.
- Retention is configurable and old rows are removed at startup.
- DuckDB failure degrades observability, not gateway availability.

## Query interfaces

- `GET /api/admin/v1/observability/summary`
- `GET /api/admin/v1/observability/requests`
- `GET /api/admin/v1/observability/requests/:id`
- `GET /metrics`

## Success criteria

- Request list filtering by time, status, provider and model does not query SQLite.
- Dashboard aggregates report request count, error rate, p95 latency, token use and cost.
- Request detail clearly states when payload capture was disabled or an event was dropped.
- Gateway traffic continues when the observation writer is unavailable.
