# Spec: Safe client and SDK onboarding

Module id: `client-onboarding`. Shared authority: `specs/SPEC-reference-adoption.md`; read it before this spec.

## Objective and success criteria
- Add an authorized per-key model/options read that reuses model access/profile/endpoint candidate rules, without needing/recovering the plaintext token. Foreign key IDs are rejected and disabled/expired keys do not appear usable.
- Provide Codex TOML (Responses), Claude Code JSON/env, Gemini CLI env, curl, Python and Node snippets. Respect configured deployment base/subpath and client-specific /v1 conventions, safely quote arbitrary model/base/token strings.
- Offer snippets both after create/rotate and from an existing-key Connect action. One-time token exists only in the open dialog's memory; after closing use an environment-variable/placeholder, no localStorage or token recovery. No server-side scanning or editing of client files.
- Use protocol-scoped authorized model choices and compatible reasoning choices; all async loading/error/retry/empty states, labels and copy feedback are localized. Dialog is keyboard accessible and usable at 375px.

## Commands
Backend: `cargo fmt --all -- --check`; `cargo clippy --locked --all-targets -- -D warnings`; `cargo test --locked`. Frontend: `pnpm --dir web lint`; `pnpm --dir web test`; `pnpm --dir web build`. Build embedded assets before Rust checks.

## Structure and style
Follow the existing API/service/repository/adapter separation and named module paths in the implementation plan. Rust uses typed Result and existing SeaORM transaction helpers; React reuses current house components, Geist typography, Radix primitives and bilingual i18n.

## Testing strategy
Focused RED/GREEN mock contracts for the criteria above, plus scope/partial-failure tests. Complete module tests and repository gates before reporting completion.

## Boundaries
All shared architecture, security/privacy, dependency and verification boundaries in the shared spec apply verbatim. No open user decisions; the user authorized the listed behavior and delegated routine choices.
