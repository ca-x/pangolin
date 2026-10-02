# Spec: Correct probe and request timing

Module id: `probe-measurement`. Shared authority: `specs/SPEC-reference-adoption.md`; read it before this spec.

## Objective and success criteria
- Separate response_headers_ms, first_event_ms and first_text_ms; ttft_ms is the first nonempty visible text latency for text-generating probes, never response headers, heartbeat, role-only, reasoning-only or an empty delta. A tool-only normal gateway reply remains valid with unmeasured first text.
- Existing first-event timeout, terminal detection and irreversible stream commitment remain intact. Historical probe header timing is retained under response_headers_ms; historical first-event times are not relabeled first text.
- Manual probe accepts a configured conversational endpoint, optional credential_id and stream boolean; omitted options preserve provider-family defaults. Validate project/provider/model/credential/endpoint eligibility before enqueue and before I/O; use saved endpoint/proxy settings.
- Bound probe response to 1 MiB and total execution to 30 seconds; use known stable error codes, no saved raw response. Verify output shape and protocol terminal rather than accepting arbitrary HTTP-200 JSON. Persist results through existing durable job fencing and health behavior.
- Console exposes protocol/credential/stream selection and the three distinct measurements with useful unknown/error/loading states.

## Commands
Backend: `cargo fmt --all -- --check`; `cargo clippy --locked --all-targets -- -D warnings`; `cargo test --locked`. Frontend: `pnpm --dir web lint`; `pnpm --dir web test`; `pnpm --dir web build`. Build embedded assets before Rust checks.

## Structure and style
Follow the existing API/service/repository/adapter separation and named module paths in the implementation plan. Rust uses typed Result and existing SeaORM transaction helpers; React reuses current house components, Geist typography, Radix primitives and bilingual i18n.

## Testing strategy
Focused RED/GREEN mock contracts for the criteria above, plus scope/partial-failure tests. Complete module tests and repository gates before reporting completion.

## Boundaries
All shared architecture, security/privacy, dependency and verification boundaries in the shared spec apply verbatim. No open user decisions; the user authorized the listed behavior and delegated routine choices.
