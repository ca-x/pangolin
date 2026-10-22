# models.dev and gateway usability

## Objective and scope

Deliver an independently authored Rust client for models.dev, reuse it in
Pangolin, and adopt focused, contract-tested improvements informed by new-api's
observable behavior. Preserve Apache-2.0 licensing; do not copy AGPL source or
assets. The reference checkout is `7fb6e57a5`.

| Capability | Responsibility | Dependency |
| --- | --- | --- |
| modelsdev-client | Typed, bounded models.dev parsing and HTTPS retrieval | — |
| modelsdev-subscription | Normalize metadata into immutable SQLite catalog snapshots | modelsdev-client |
| protocol-controls | Explicit, loss-aware cross-protocol request conversion | Existing provider adapters |
| token-controls | Honor explicit token budgets without inventing measurements | Existing admission/tokenizers |
| catalog-experience | One-click subscription and actionable sync feedback | modelsdev-subscription |
| delivery | Fresh verification, release-binary QA, independent review and publication | All above |

## Contracts

- Crate: `models-dev-rs`, repository `lib-x/models-dev-rs`. Before implementation,
  crates.io API and sparse-index checks returned 404 for this name; `models-dev`
  and `modelsdev` were occupied. Recheck before publication; availability is not
  a reservation. Publish only after crate tests, lint, docs and package checks.
- Use the official models.dev API and MIT SDK as schema/behavior references,
  not a dependency on JavaScript. No build-time network requests.
- Subscription format is recognized only for the canonical
  `https://models.dev/api.json`. Other sources retain the versioned Pangolin
  schema and existing signature rules. Verify signatures against original bytes
  before normalization; required signatures must never silently downgrade.
- Explicit user activation opts into network access. Thereafter the existing
  durable scheduler refreshes daily, honoring validators. A failed download,
  malformed document, unsupported schema or conflicting revision retains the
  last-known-good snapshot. Disabling stops automatic fetches.
- Fetching stays outside SQLite business transactions. Keep HTTPS/DNS pinning,
  no redirects, public-address checks, configured proxy safety and timeouts.
  models.dev downloads and normalized catalogs are bounded to 16 MiB; ordinary
  subscription downloads retain their 4 MiB bound. Entry limits remain bounded.
- Imported metadata is declarative only. Unknown provider SDK/package names
  cannot enable adapters. Trusted built-in adapter/endpoints/icon mappings stay
  authoritative. Preserve source, document digest, nullable capabilities,
  modalities, dates, context/output limits, prices and tier metadata.
- Advertised prices remain reference metadata, never live billing updates.
  Existing project model configuration, local overrides, immutable price
  versions, budgets, routes and historical cost/usage facts are not rewritten.
- Preserve native provider requests. Cross-protocol conversion must reject
  unsupported or conflicting token/reasoning shapes rather than drop fields.
  No retries after streaming bytes commit. Exact context checks require a
  reliably configured matching tokenizer; estimates are not measured usage.
- Add a bounded nonstreaming Anthropic Messages / OpenAI function-tool round
  trip: preserve tool IDs, JSON arguments and text-only results; reject media,
  server tools, error results or block order that cannot be represented. Source
  usage remains authoritative before response conversion.
- Conflicting positive output-token aliases fail closed across preflight,
  context/output checks and admission. Equal aliases remain compatible; native
  unknown fields and absent explicit caps are preserved unless an existing
  budget/TPM policy already requires a bound.
- Extend the existing React models screen rather than redesign navigation.
  Both locales, all theme/accent modes, 375/768/1440px, accessible controls,
  loading/error/retry/empty states and existing restrained motion tokens apply.
  No animation of data rows or keyboard actions and no new motion dependency.

## Verification

Offline fixtures and mock transports cover normalization, nullable vs false
facts, vendor-qualified IDs, unknown providers, conditional fetch, download
bounds, signature verification, failed-refresh fallback, local override
precedence, atomic/revision-fenced activation and real API response envelopes.
Protocol/token changes require focused red/green regressions. Live catalog
retrieval is metadata live testing, not live-tested provider inference.

Run the repository's Web gates before Rust gates, release build and actionlint.
Use the release binary for populated browser QA in both locales/themes and at
375/768/1440px. Final GPT-6 review and all final-delivery gates must be clean
before pushing/tagging/publishing Pangolin binaries or GHCR images.
