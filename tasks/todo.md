# Pangolin tasks

- [x] Foundation scaffold and persistence
  - Acceptance: configuration, migrations, setup/auth and embedded assets compile and are tested.
  - Verify: `cargo test --all-features` and release embedding test.
- [x] Gateway control plane and compatible endpoints
  - Acceptance: providers/models/routes/keys work; mock provider contracts pass.
  - Verify: Rust integration tests against a local mock upstream.
- [x] DuckDB observation store
  - Acceptance: events persist independently, aggregates filter correctly, failures do not fail requests.
  - Verify: observation integration tests with temporary files.
- [x] React administration console
  - Acceptance: setup and daily operations work in English/Chinese across themes and breakpoints.
  - Verify: Vitest plus browser smoke/a11y checks.
- [x] Delivery and documentation
  - Acceptance: Docker and release workflows mirror the proven raindrop structure and README credits references.
  - Verify: local builds, workflow syntax review and container build where Docker is available.
- [x] Independent review and final verification
  - Acceptance: `gpt-6-astra` findings resolved or explicitly documented; all available checks pass.
  - Verify: fresh full command suite and requirement checklist.

## Follow-ups from the v0.3.0 review

- [ ] Restore reader concurrency without weakening budget admission
  - Context: `db::connect` pins `max_connections(1)` so the budget reservation's writer-lock
    invariant holds. That also serialises reads, and long admin transactions
    (`operations::backup::export_as`/restore, `runtime::gc`) hold the connection while they scan
    or delete in bulk, so gateway API-key and policy reads can queue behind them.
  - Direction: allow multiple reader connections and serialise only the reservation writer, e.g.
    a dedicated writer connection or `BEGIN IMMEDIATE` on the budget path; batch the backup/gc work
    so no transaction spans a full scan.
  - Verify: a test that a concurrent gateway read completes while a large admin transaction is open.
- [ ] Durable projection outbox and a rebuild path
  - Context: the DuckDB projection is fed in-process, so a crash between the SQLite commit and the
    enqueue loses the event permanently, and there is no way to rebuild from the authoritative
    ledger (`request_facts`/`usage_logs`).
  - Direction: persist an outbox row in the same SQLite transaction and drain it idempotently; add a
    rebuild command that replays the ledger into the projection.
- [ ] Audit retention and rate limiting for authentication events
  - Context: rejected logins are recorded for unauthenticated callers (bounded per row, but the
    table itself has no retention policy) and there is no login rate limit.
  - Direction: apply the audit retention policy to `audit_events` and add per-IP/identifier
    throttling on the login endpoint.
- [ ] Per-action gating in the access console
  - Context: tab-level permission gating now mirrors the backend's implication closure, but tabs mix
    actions with different requirements (key profiles and key logging need `project:manage`).
  - Direction: gate those controls individually so a principal never sees an action that will 403.
