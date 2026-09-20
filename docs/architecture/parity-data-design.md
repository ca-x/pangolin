# Full-parity data architecture (DDIA)

This document applies the reliability, scalability and evolvability principles from *Designing Data-Intensive Applications* to Pangolin's AxonHub/new-api parity work.

## Systems of record and derived data

| Class | Storage | Authoritative contents | Failure behavior |
| --- | --- | --- | --- |
| Record system | SQLite through SeaORM | identities, projects/RBAC, API keys/profiles, provider credentials/config, routing policies, budgets/quotas, active catalog pointer, durable sessions, request/execution/usage/cost facts, job/outbox state | Failure makes affected control-plane or enforcement operation unavailable; fail closed |
| Analytical projection | DuckDB | request-event scans, dashboard buckets, percentiles, searchable redacted payload projection | Degrade observability only; gateway/control facts continue |
| Ephemeral derived state | bounded memory, optional Redis adapter | affinity, limiter windows, latency EWMA, circuit hot state, catalog/cache entries | Rebuild or fall back to authoritative configuration; never grant access because cache is missing |
| External artifacts | local/S3-compatible target | encrypted/versioned backup artifacts, optional large request/media bodies | Metadata and ownership stay in SQLite; target failure is explicit and retryable |

## Consistency decisions

Strong transactional consistency is required for:

- user/project/role membership and permission delegation;
- API-key status, imported-token uniqueness and budget reservation/settlement;
- provider credential enable/disable and routing eligibility;
- catalog snapshot validation and active-version switch;
- Responses session ownership/version update;
- request execution terminal status, usage/cost fact and enforcement counters;
- job claim/lease/fencing state and mutation audit/outbox append.

Eventual consistency is allowed for:

- DuckDB event projection and dashboard refresh;
- provider/model catalog subscription fetch cache before activation;
- Webhook delivery, metrics export and operator notifications;
- probe/quota display after authoritative snapshot persistence;
- affinity and latency caches.

## Catalog data flow

```text
built-in snapshot ─┐
subscription A ────┼─> bounded fetch -> schema/signature validation
subscription B ────┤                         |
local overrides ───┘                         v
                                    immutable staged snapshot
                                               |
                                      one SQLite transaction
                                    entries + active pointer + audit
                                               |
                            cache/UI/routing rebuild from active version
```

Subscription retries use URL + validator/ETag + revision as the idempotency identity. Invalid, oversized, unsigned-when-required or semantically inconsistent snapshots never move the active pointer. Unknown typed extension fields survive import/export. Local overrides are separate authoritative rows and cannot be deleted by a subscription.

## Request and execution lifecycle

One logical Request may have several immutable RequestExecution attempts. Each attempt records the selected channel credential/config snapshot, actual model, timing, retry/circuit decision and terminal outcome. UsageLog/CostItem facts reference the execution and immutable price version. An operation ID deduplicates finalization. No database transaction stays open during upstream network I/O.

Streaming has one irreversible boundary: before the first downstream event, eligible attempts may fail over; after commitment, failure finalizes the same execution and cannot start another.

## Background jobs and outbox

Catalog refresh, probes, quota collection, webhook delivery, backup and GC use durable jobs:

- enqueue in the same transaction as the business fact/outbox entry;
- claim with a lease and monotonic fencing token;
- heartbeat outside business transactions;
- execute external I/O without holding SQLite locks;
- finalize conditionally on claim token;
- reuse operation ID for automatic retries; manual retry creates a new job from current configuration snapshot;
- bound attempts, payload size and error text.

## Budget and rate semantics

Money is integer micro-USD. Immutable price versions avoid historical reinterpretation. Hard budget paths reserve an upper bound before I/O and settle actual cost atomically; when an upper bound cannot be calculated (unsupported media/stream), the request is rejected for hard-budget keys rather than silently undercounted. Rate/cache state may be process-local in single-node mode, but durable spend and quotas remain authoritative.

## Backup and recovery

Backups select authoritative SQLite resources and encrypted secret envelopes. DuckDB is optional derived data. A backup artifact is versioned, checksummed and immutable; restore validates completely into staging before one atomic replacement/import transaction. Target credentials are never copied into public artifacts. Restore is idempotent by artifact ID and conflict strategy.

## Scale and future storage

The supported release is single-node. SQLite serialized writes and bundled DuckDB are intentional simplicity. Interfaces keep these future substitutions possible without claiming them now:

- PostgreSQL for a multi-node record system;
- Redis for distributed admission/affinity;
- ClickHouse/Parquet for large analytical history;
- object storage for payload/media bodies.

No future store weakens authorization, budget or ownership consistency merely to improve availability.
