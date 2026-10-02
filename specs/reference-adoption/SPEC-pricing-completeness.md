# Spec: Pricing completeness without rewriting historical accounting

Module id: `pricing-completeness`. Shared authority: `specs/SPEC-reference-adoption.md`; read it before this spec.

## Objective and success criteria
- Separate usage settlement (reported/conservative/unreported) from pricing status: legacy, priced, explicit_free, missing_price, incomplete_usage. Add model pricing_configured flag; nonzero configured price or an explicitly saved valid price version is configured; zero is only explicit_free when configuration was explicitly confirmed.
- Migrate historical amounts/immutable snapshots without changing amounts, spend or price versions; mark historical records legacy where completeness cannot be proved. Nonzero legacy model prices remain configured; ambiguous zero defaults remain unconfigured.
- Add an additive version-31 `model_prices.operator_confirmed` Boolean marker, default false. New explicit server saves and confirmed clones set it; migration backfills this new metadata only for rows whose same-project durable `prices.save` audit proves operator confirmation. Existing origin, rates, snapshots, amounts and spend remain unchanged. Default historical `operator` origin alone proves nothing: shared snapshot/setup/clone selection requires the marker or scoped durable audit; unproven automatic/ambiguous versions cannot override current configured model rates. Confirmation must survive audit retention and configuration-only backups.
- New model create/update APIs accept pricing_configured, reject contradictory unconfigured/nonzero input, and preserve compatibility when omitted. Catalog-imported documented prices are configured only when typed usable price facts exist; auto-discovered default zeros are unconfigured.
- Persist pricing_status alongside usage/execution facts, including streams, missing terminal usage and startup interruption. Snapshot status once per attempt. Hard-budget paths reject missing price before network I/O; non-budget calls still execute with unknown cost status. Explicit free works with existing budget validation, and historical settled calls are not re-priced.
- Accounting binds quantities to the known actual source protocol/envelope for generic transformed nonstream, native identity and SSE replies. Foreign protocol containers/aliases and converter compatibility zeros cannot prove measurement or shadow native thinking/total facts. Parse and merge only source-bound counter evidence, while original response fields still control forwarding, terminal classification, identifiers, session/body capture and existing stream/commit/retry policy. Successful conversion still controls delivery; caller fields and historical financial facts remain unchanged. Missing counters keep incomplete/unmeasured treatment and hard-budget conservative settlement.
- Add an additive version-30 `usage_logs.usage_measurement_json` column (valid JSON, default `{}` for historical rows). Persist a bounded closed version-1 Boolean measurement contract for existing exposed quantities, incorporating protocol terminal/trust state independently of financial completeness. Empty historical metadata stays unspecified; no request samples or raw counter values enter this metadata. Existing numeric columns, historical monetary amounts, price snapshots and legacy status remain unchanged. Scoped APIs and live/rebuilt projections expose the flags, and each measured aggregate excludes unavailable placeholders without hiding genuinely measured zero values.
- Authorized read/analytics expose pricing completeness and unknown counts, excluding unknown placeholder zeros from measured cost displays/totals. Console shows a dash and clear reason for unknown, known zero amounts for complete priced or explicit_free records, and can explicitly confirm free prices. No audio price formulas or automatic financial backfill in this change.

## Commands
Backend: `cargo fmt --all -- --check`; `cargo clippy --locked --all-targets -- -D warnings`; `cargo test --locked`. Frontend: `pnpm --dir web lint`; `pnpm --dir web test`; `pnpm --dir web build`. Build embedded assets before Rust checks.

## Structure and style
Follow the existing API/service/repository/adapter separation and named module paths in the implementation plan. Rust uses typed Result and existing SeaORM transaction helpers; React reuses current house components, Geist typography, Radix primitives and bilingual i18n.

## Testing strategy
Focused RED/GREEN mock contracts for the criteria above, plus scope/partial-failure tests. Complete module tests and repository gates before reporting completion.

## Boundaries
All shared architecture, security/privacy, dependency and verification boundaries in the shared spec apply verbatim. No open user decisions; the user authorized the listed behavior and delegated routine choices.
