# Spec: Attempt-specific conversion and affinity explanations

Module id: `execution-diagnostics`. Shared authority: `specs/SPEC-reference-adoption.md`; read it before this spec.

## Objective and success criteria
- Persist bounded conversion_diagnostics and affinity_diagnostics JSON arrays per request execution: maximum 64 conversion entries and 16 affinity entries; fixed phase/reason/code, known schema field path, bounded IDs/scope digest and expiry; no payload/header/URL/secret values or raw protocol cursor.
- Record conversion rejection/allowed normalization on the responsible execution/attempt and return typed safe public errors. Rejections still stop unsupported conversion before upstream I/O; successful native pass-through preserves unknown fields. Do not adopt AstrLink's lossy conversion semantics.
- Explain rule-driven affinity first/hit/expired/ineligible/established/released/candidate-failed transitions without changing strict, TTL, disabled eligibility, late success/failure or existing Responses/WS channel pinning semantics. Cache remains derived; explanation facts are authoritative and project-scoped.
- Existing execution/request/trace reads return bounded safe diagnostics; control UI renders localized reasons and field paths without enabling body capture. Cover attempt isolation, project isolation, persisted/reopened records and malformed/oversized metadata.

## Commands
Backend: `cargo fmt --all -- --check`; `cargo clippy --locked --all-targets -- -D warnings`; `cargo test --locked`. Frontend: `pnpm --dir web lint`; `pnpm --dir web test`; `pnpm --dir web build`. Build embedded assets before Rust checks.

## Structure and style
Follow the existing API/service/repository/adapter separation and named module paths in the implementation plan. Rust uses typed Result and existing SeaORM transaction helpers; React reuses current house components, Geist typography, Radix primitives and bilingual i18n.

## Testing strategy
Focused RED/GREEN mock contracts for the criteria above, plus scope/partial-failure tests. Complete module tests and repository gates before reporting completion.

## Boundaries
All shared architecture, security/privacy, dependency and verification boundaries in the shared spec apply verbatim. No open user decisions; the user authorized the listed behavior and delegated routine choices.
