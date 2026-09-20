# Pangolin / 鲮鲤 agent guide

This file is the repository-level operating contract for coding agents.

## Product identity

- English product and binary name: `Pangolin` / `pangolin`.
- Formal Chinese product name: `鲮鲤`. Do not translate the brand as `穿山甲` in product copy.
- Pangolin is a self-hosted AI API gateway, control plane and observability console written in Rust with an embedded React frontend.

## Sources of truth

- Full parity spec: `specs/SPEC-axonhub-parity.md`.
- Capability inventory: `docs/axonhub-capability-matrix.md`. No row may disappear silently.
- Data architecture: `docs/architecture/parity-data-design.md`.
- Reference synthesis: `docs/adr/0002-axonhub-new-api-design-synthesis.md`.
- Visual rules: `design-system/pangolin/MASTER.md`.
- Active implementation plan: `tasks/axonhub-parity-plan.md`.

## Architecture invariants

- SQLite through SeaORM is the authoritative record system for identity, policy, configuration, budgets, durable sessions, execution, usage/cost and job state.
- DuckDB is a derived analytical projection. DuckDB failure may degrade observability but must not grant access, change routing enforcement or stop the gateway.
- Do not perform provider/network/object-store I/O while holding a SQLite business transaction.
- Background work uses durable idempotent jobs/outbox records, bounded leases and fencing tokens.
- Money is integer micro-USD. Historical cost references immutable price versions.
- The supported release is single-node. Do not claim distributed limits/consensus unless implemented and tested.

## Security and privacy

- Authorization fails closed. Session, API-key and system principals are distinct and never silently upgraded.
- Every control-plane mutation is transactionally audited.
- Cross-project reads/writes are denied by both service checks and relational constraints where possible.
- Passwords/API keys are Argon2id hashed. Imported API keys are never retained in plaintext.
- Provider/storage/OIDC secrets use authenticated encryption and are never returned, logged or committed in fixtures.
- Request bodies are not logged by default. Effective logging policy is resolved once at admission; even full-body mode always strips credentials/cookies/authorization.
- Never retry after downstream streaming bytes are committed.
- Catalog subscriptions are declarative data only: bounded HTTPS fetch, DNS pinning, no redirects, schema validation, optional/required pinned Ed25519 signature, staged atomic activation and last-known-good fallback.

## Dependency and reference policy

- Prefer mature maintained license-compatible crates over hand-rolled protocol/infrastructure code. Wrap them behind Pangolin interfaces.
- LiteLLM Rust dependencies are pinned to exact commit `8c4c394ecc82c4d6acb5eb371d8781e487894a17`; use pure-Rust crates and exclude Python bridge/legacy host paths.
- [looplj/axonhub](https://github.com/looplj/axonhub) is an Apache/LGPL behavior/test reference. Port observable contracts, not Go implementation.
- [QuantumNous/new-api](https://github.com/QuantumNous/new-api) is AGPL-3.0. Study product behavior/design only; never copy code or image assets.
- [ca-x/raindrop](https://github.com/ca-x/raindrop) is a Rust engineering reference for embedded assets, releases and backup job/target safety.
- [farion1231/cc-switch](https://github.com/farion1231/cc-switch) is an MIT Rust proxy reference for staged forwarding, failover locks, usage/cache token parsing, media sanitization and client compatibility; do not import its Tauri lifecycle wholesale.
- Provider logos use catalog `logo_key` mapped to bundled license-compatible icon libraries; do not copy project-owned trademark assets.

## Backend conventions

- Keep HTTP extraction/serialization, authorization, domain service, repository, orchestrator and provider adapter boundaries separate.
- Use typed errors at module boundaries. Public protocol errors must never include internal database/decryption/network causes.
- Bound request bodies, multipart fields/files, SSE events, WebSocket lanes/queues, catalog documents, caches and stored sessions.
- Preserve native provider fields on pass-through. Cross-protocol transforms must reject unsupported shapes explicitly rather than silently dropping fields.
- Any route added to the public gateway must pass through project/API-key policy, model access, admission, retry/circuit, trace and terminal accounting.

## Frontend conventions

- React UI is bilingual (`zh-CN`, `en`), supports system/light/dark and bronze/slate/jade accents, and must remain usable at 375/768/1440px.
- Use Radix primitives for dialogs/selects/tooltips and Lucide for structural icons. Provider brands use the catalog icon map.
- Normal operational table text is at least 14px. Show `—`, not invented zero values, when telemetry is unmeasured.
- Every async query has loading, explicit error/retry and useful empty states.
- Motion serves feedback/spatial/state purposes only, stays under 300ms, animates transform/opacity, gates hover by pointer and respects reduced motion.
- All visible copy is localized. Icon-only controls require accessible names; forms keep labels and inline errors.

## Build and verification

Release embedding requires `web/dist` to exist. Build Web assets before Rust verification in a fresh checkout.

```bash
pnpm --dir web install --frozen-lockfile
pnpm --dir web lint
pnpm --dir web test
pnpm --dir web build
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
go run github.com/rhysd/actionlint/cmd/actionlint@latest -color
```

- Add focused red/green tests for every bug, policy boundary and protocol transform.
- Use mock upstreams/IdPs/object stores for contracts. Mark integrations `contract-tested`; only real credentials justify `live-tested`.
- Browser QA must use the release binary and cover populated operational screens in both themes/languages plus 375px mobile.
- Do not claim completion until fresh verification output and GPT-6 final review are clean.

## Git and release

- Preserve user changes and unrelated work. Never use destructive broad reset/clean commands.
- Keep commits atomic and conventional. Do not mix controller specs/plans with task implementation commits.
- Do not push, tag, publish a release or publish Docker images until the plan's final delivery task and all gates are green.
- Final release publishes checksummed Linux/macOS/Windows binaries and native `linux/amd64` + `linux/arm64` GHCR images.
