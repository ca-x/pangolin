# Spec: foundation

## Objective

Provide a secure, self-contained runtime that starts with no external services, guides the first administrator through setup, and serves an embedded Web console from the Rust binary.

## Tech stack

- Rust 2024, Axum, Tokio and Tower
- SeaORM 1.1 with SQLite as the default source-of-truth database
- Argon2id password hashing, hashed opaque sessions, encrypted provider secrets
- `rust-embed` for release Web assets

## Commands

- Development: `cargo run`
- Rust checks: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings`
- Rust tests: `cargo test --all-features`
- Web development: `pnpm --dir web dev`
- Web verification: `pnpm --dir web lint && pnpm --dir web test && pnpm --dir web build`
- Release: `pnpm --dir web build && cargo build --release --locked`

## Project structure

- `src/` — Rust runtime, APIs and persistence
- `migration/` — SeaORM migrations
- `web/` — React/Vite console
- `tests/` — Rust integration tests
- `design-system/` — product-wide visual rules
- `docs/` — architecture and operator documentation
- `tasks/` — execution plan and checklist

## Code style

```rust
pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse::from_state(&state))
}
```

Prefer small modules, typed errors at boundaries, explicit state injection, no global mutable state, and structured tracing with secrets redacted.

## Testing strategy

- Unit tests for configuration, password/session handling and crypto envelopes.
- Integration tests create temporary SQLite/DuckDB files and run migrations.
- Browser smoke tests cover setup, sign-in and core navigation against a release binary.

## Boundaries

- Always: validate input, hash credentials, redact secrets, use atomic migrations and fail closed for authentication.
- Ask first: destructive migrations or a change to the source-of-truth database.
- Never: log plaintext credentials, commit secrets, expose setup endpoints after initialization.

## Success criteria

- A new instance can be initialized either entirely from environment variables or from the Web wizard.
- Setup is idempotent and cannot create a second initial administrator.
- The release binary serves the compiled SPA without Node.js installed.
- `/api/health/live` and `/api/health/ready` distinguish process life from storage readiness.

## Open questions

None for v0.1. The default bind is `0.0.0.0:8080` and data directory is `./data`, both configurable.
