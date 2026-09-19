# Pangolin implementation plan

## Assumptions

1. `Pangolin` is the English product and binary name; the formal Chinese product name is `鲮鲤`.
2. v0.1 is a self-hosted single-node release, not a hosted multi-tenant billing service.
3. The supplied image is the authoritative logo and may be committed as a Web asset.
4. OpenAI-compatible chat is the primary compatibility target; Anthropic Messages and Responses are included with narrower provider coverage.
5. Self-review replaces interactive approval; final architecture/code review uses `gpt-6-astra` as requested.

## Technical sequence

1. Foundation: initialize Rust/Web workspaces, config, migrations, auth/setup and embedded assets.
2. Gateway: CRUD control plane, virtual keys, route resolution and provider calls.
3. Observability: DuckDB worker, request middleware, aggregate/detail APIs and metrics.
4. Console: design tokens, shell, setup, resources, requests, themes and i18n.
5. Delivery: tests, Docker, CI/release workflows, README and repository publishing.
6. Review: animation opportunity sweep, browser QA, `gpt-6-astra` review, fixes and full verification.

## Risks and mitigations

- LiteLLM Rust is pre-1.0 and lives in a larger unpublished workspace: document the evaluated commit and keep Pangolin's adapter boundary replaceable without making every build fetch the entire repository.
- DuckDB bundled builds are heavy: use native release runners, cache Cargo artifacts and keep extensions disabled.
- Cross-protocol streaming translation is easy to get subtly wrong: prefer transparent streaming passthrough in v0.1 and test SSE framing.
- Payload observation can leak sensitive data: default off, redact known secret fields and make capture state visible.
