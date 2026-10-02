# Spec: Protocol-aware privacy and explainable dry run

Module id: `privacy-inspection`. Shared authority: `specs/SPEC-reference-adoption.md`; read it before this spec.

## Objective and success criteria
- Preserve existing regex deny/redact/test-mode and text preview. Add protocol/role/path-aware traversal of conversational text and structured tool arguments/results across Chat, Responses, Anthropic and Gemini.
- Do not mutate tool schema keys, numeric/boolean values, protocol identifiers or opaque replay fields. Continuation exemptions require valid array/location/role/type structure; a user/tool field named signature or encrypted_content is still inspectable. Preserve signed thinking together with its signature.
- Add exact-match per-rule allowlists (64 entries maximum, each <=256 UTF-8 bytes) with suppression per matching span, not suppression of all text that merely contains an allowlisted value. Provide explicit opt-in templates for email, phone and API-key-like strings; saving a template uses normal audited project mutation.
- Add full-request dry run (body plus endpoint, 64 KiB maximum, depth<=32, segments<=4096, findings<=512) using the same traversal, rule order, role/scopes, allowlists and transformations as online enforcement. Return decision, redacted_body, matched/suppressed entries with rule ID, safe JSON path and UTF-8 byte offsets; no plaintext match field. Sample and findings are not persisted/audited/logged.
- Console can inspect a protocol request, see ignored/matched reasons and the resulting body, add/edit templates/allowlists, and clear sample on close. No image/file/audio/OCR coverage or reversible response restoration claim.

## Commands
Backend: `cargo fmt --all -- --check`; `cargo clippy --locked --all-targets -- -D warnings`; `cargo test --locked`. Frontend: `pnpm --dir web lint`; `pnpm --dir web test`; `pnpm --dir web build`. Build embedded assets before Rust checks.

## Structure and style
Follow the existing API/service/repository/adapter separation and named module paths in the implementation plan. Rust uses typed Result and existing SeaORM transaction helpers; React reuses current house components, Geist typography, Radix primitives and bilingual i18n.

## Testing strategy
Focused RED/GREEN mock contracts for the criteria above, plus scope/partial-failure tests. Complete module tests and repository gates before reporting completion.

## Boundaries
All shared architecture, security/privacy, dependency and verification boundaries in the shared spec apply verbatim. No open user decisions; the user authorized the listed behavior and delegated routine choices.
