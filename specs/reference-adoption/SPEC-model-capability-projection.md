# Spec: Conservative metadata and capability eligibility

Module id: `model-capability-projection`. Shared authority: `specs/SPEC-reference-adoption.md`; read it before this spec.

## Objective and success criteria
- Extend the bounded public model-card projection with common nullable capabilities, modalities and reasoning levels. Parse only typed bounded fields from stored card/discovery data; do not expose raw extension JSON publicly.
- Aggregate the candidates eligible for the same discovery endpoint/context: an explicit false prevents claiming a shared capability; all-known true becomes true; other unknowns stay unknown. Context/output minimum is exposed only when all participating cards provide a positive known bound. Inconsistent developer/type/logo/cost facts remain unknown rather than borrowing the first candidate.
- Intersect known reasoning levels/modalities; retain unknown vs explicit empty. Metadata remains scoped to the project's and key's eligible candidates and requested endpoint. Request conditions may narrow candidates independently of the discovery projection.
- Before upstream I/O filter explicit capability mismatches for image inputs and tools, and known context/output limits where a trustworthy estimator exists. Unknown facts do not grant access or become false telemetry. Keep unsupported cross-protocol shapes explicitly rejected.
- Cover heterogeneous alias members, unknown cards, disabled/foreign members, endpoint-specific routes, image/tools requests and metadata string/array bounds.

## Commands
Backend: `cargo fmt --all -- --check`; `cargo clippy --locked --all-targets -- -D warnings`; `cargo test --locked`. Frontend: `pnpm --dir web lint`; `pnpm --dir web test`; `pnpm --dir web build`. Build embedded assets before Rust checks.

## Structure and style
Follow the existing API/service/repository/adapter separation and named module paths in the implementation plan. Rust uses typed Result and existing SeaORM transaction helpers; React reuses current house components, Geist typography, Radix primitives and bilingual i18n.

## Testing strategy
Focused RED/GREEN mock contracts for the criteria above, plus scope/partial-failure tests. Complete module tests and repository gates before reporting completion.

## Boundaries
All shared architecture, security/privacy, dependency and verification boundaries in the shared spec apply verbatim. No open user decisions; the user authorized the listed behavior and delegated routine choices.
