# Reference adoption: shared implementation contract

## Objective and scope
Implement the seven prioritized Magpie/AstrLink-inspired improvements authorized by the user: accurate protocol probes, credential-specific model visibility, conservative route metadata, client setup snippets, structured privacy inspection/dry run, attempt/binding diagnostics, and pricing completeness. Follow the existing gateway/control-plane separation. These are independently testable enhancements to existing modules, not a replacement gateway.

## Capability map and order
| Module id | Responsibility | Depends on |
| --- | --- | --- |
| probe-measurement | Protocol/model/credential probes and separate headers/event/text times | foundation, operations |
| credential-model-discovery | Per-enabled-credential discovery and durable visibility snapshots | foundation, operations |
| model-capability-projection | Conservative public metadata and request capability eligibility | credential-model-discovery, request-orchestrator |
| client-onboarding | Authorized model choices and safe client/SDK snippets | model-capability-projection, identity-access |
| privacy-inspection | Typed request traversal, templates, allowlists and full-request dry run | request-orchestrator, identity-access |
| execution-diagnostics | Bounded conversion and affinity reason metadata per execution | request-orchestrator, trace-cost |
| pricing-completeness | Distinguish measured price/free/missing price without rewriting history | trace-cost, gateway |

Build order: additive schema -> probes -> discovery/metadata -> onboarding -> privacy -> execution diagnostics -> pricing -> operational console -> final delivery verification. Existing CAPABILITY_MAP modules and capability matrix rows remain present.

## Shared data architecture
SQLite/SeaORM is authoritative for new visibility/diagnostic/pricing facts. DuckDB is only a projection. All provider I/O occurs outside SQLite transactions. Persist discovery results only after lease fencing and rechecking provider/project, credential enable state and the captured encrypted credential revision. Preserve last-known-good visibility after a failed fetch; never reactivate a disabled credential. Imported secrets and plaintext samples never become logs or audit details.

## Explicit DDIA database design
The user explicitly requested `ddia-principles`. Apply the existing [full-parity data architecture](../docs/architecture/parity-data-design.md), particularly DDIA transactions, schema evolution, partial failures/fencing, and record systems versus derived data.

- SQLite/SeaORM stores authoritative scoped inventory, configuration, execution, price/usage, audit and durable job facts; DuckDB is reconstructible analytics only.
- Composite ownership constraints and short consistent reads prevent cross-project references and mixing old inventory with newly rotated credentials.
- Perform external I/O first, then revalidate meaningful identity/configuration and lease fence inside atomic activation/audit; retries have durable operation identities.
- Validity uses actual credential/discovery inputs, not generic wall-clock `updated_at`; label/priority/routing-only edits do not invalidate supplier observations. Instant measures durations; wall time labels dates and expiry.
- Additive versioned fields/defaults preserve old data and immutable financial history. Unknown quantity/price facts remain distinct from measured zero. Version-30 usage measurement metadata stores closed Boolean trust flags independently of pricing status; historical empty metadata is unspecified, numeric compatibility columns remain intact, and scoped/live/rebuilt measured projections exclude unknown placeholders.
- Provider observations and analytical projections may be stale/partial, while authorization and budgets fail closed. The supported system remains single-node, without a distributed consistency or consensus claim.

## Commands
Install/build web first: `pnpm --dir web install --frozen-lockfile`; `pnpm --dir web lint`; `pnpm --dir web test`; `pnpm --dir web build`.
Rust: `cargo fmt --all -- --check`; `cargo clippy --locked --all-targets -- -D warnings`; `cargo test --locked`; `cargo build --release --locked`.
Workflows: `go run github.com/rhysd/actionlint/cmd/actionlint@latest -color`.
Focused tests may use a test-name filter while iterating; full Rust suite once per backend task, web lint/test/build once per frontend task. Final checks run fresh on the complete tree.

## Project structure and code style
`src/db/schema.rs` owns monotonic migrations; services live in `src/operations` and `src/orchestration`; protocol adapters in `src/providers`; extraction/serialization in `src/api`; React in `web/src/pages`. Add small purpose-specific modules only where needed, not a generic plugin or policy framework. Follow existing typed Result and SQL helper conventions:
```rust
let tx = state.db.begin().await?;
jobs::fence(&tx, claim).await?;
tx.execute(sql("UPDATE channel_settings SET model_sync_error=NULL WHERE provider_id=?", vec![provider.into()])).await?;
tx.commit().await?;
```

## Testing strategy
Write focused RED/GREEN regressions for changed measurements, policy boundaries and transforms using existing mock upstreams and composed Router fixtures. Cover scope/isolation and partial failures, not only helpers. Reuse existing fixtures in `src/api/gateway/tests/operations.rs`, `src/orchestration/tests.rs`, and React Testing Library tests. Browser QA uses the release binary, populated screens, both languages/themes and 375/768/1440px. Mock integrations remain contract-tested.

## Boundaries
Always: preserve native fields, explicitly reject unsupported transforms, authorize before route selection, keep control mutations transactionally audited, preserve integer micro-USD and immutable prices, gate all new visible copy through i18n.
Never: introduce RelayKit/AGPL code or assets, new dependencies without a concrete need, Tauri/Wails lifecycle, server-side access to remote users' home directories, plaintext token recovery/storage, request-body logging by default, retries after downstream commitment, fabricated unknown metrics, network I/O inside business transactions, publishing/pushing/tagging.
The user delegated implementation choices; routine reversible schema/interface decisions are made and recorded without a new approval gate. No standalone ONNX workers, semantic classifier, reversible restoration, new quota strategy, OCR or multimedia privacy claims in this implementation.

## Success criteria
Every module below has its observable API/UI behavior and regression tests. Fresh repository verification and independent GPT-6 review pass. Existing access, accounting, streaming, backup and release contracts continue to work. Historical financial amounts/price snapshots are not rewritten.
