# Spec: Credential-specific model visibility

Module id: `credential-model-discovery`. Shared authority: `specs/SPEC-reference-adoption.md`; read it before this spec.

## Objective and success criteria
- Synchronize every enabled credential of a channel through bounded provider requests (maximum 4 simultaneous); decrypt/refresh and fetch outside business transactions. Record a fingerprint of encrypted credential revision, provider config fingerprint and last success/error time.
- Persist snapshot and per-model availability with relational project/provider/credential constraints. Revalidate captured credential/config revisions and fencing before activation. A raced disable/rotation/config edit must not activate stale results.
- Successful empty discovery replaces that credential's snapshot. Failed fetch retains matching last-known-good availability and records a safe stale/failure marker. Disabled/rotated credentials do not contribute stale availability.
- Union currently enabled credential model lists for discovery-managed channel models, preserving manual models and names/prices. If all credentials fail and no matching old snapshot exists, preserve configured models and report failure.
- Candidate selection skips a known credential that cannot serve a model discovered for another credential. Models never known to any credential and manual models remain explicitly unknown, not an invented denial or capability; all existing project/key/model authorization still applies. Expose safe discovery status/count/time on credential metadata, never raw secrets.

## Commands
Backend: `cargo fmt --all -- --check`; `cargo clippy --locked --all-targets -- -D warnings`; `cargo test --locked`. Frontend: `pnpm --dir web lint`; `pnpm --dir web test`; `pnpm --dir web build`. Build embedded assets before Rust checks.

## Structure and style
Follow the existing API/service/repository/adapter separation and named module paths in the implementation plan. Rust uses typed Result and existing SeaORM transaction helpers; React reuses current house components, Geist typography, Radix primitives and bilingual i18n.

## Testing strategy
Focused RED/GREEN mock contracts for the criteria above, plus scope/partial-failure tests. Complete module tests and repository gates before reporting completion.

## Boundaries
All shared architecture, security/privacy, dependency and verification boundaries in the shared spec apply verbatim. No open user decisions; the user authorized the listed behavior and delegated routine choices.
