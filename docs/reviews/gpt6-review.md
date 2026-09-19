# GPT-6 independent review disposition

Review model: `gpt-6-astra` (read-only, high reasoning). Review scope covered gateway correctness/security, storage, React UX/accessibility, Docker and GitHub Actions.

## Resolved findings

- Budget concurrency: budget-limited keys now serialize balance re-check, upstream call and settlement per key. Streaming is rejected for budget-limited keys because provider-neutral usage settlement is not reliable.
- Authentication: gateway endpoints accept both `Authorization: Bearer` and Anthropic SDK's `x-api-key`.
- Retry/observation: pre-response 429/5xx can fail over even for streaming requests; exhausted targets record a 502 event.
- Timeouts: the upstream whole-request timeout is bounded and configurable.
- Streaming observations: final latency is recorded when the stream completes; read failure and client cancellation receive explicit terminal events; unavailable stream usage is explicitly marked.
- Anthropic conversion: system messages, stop sequences, assistant tool calls, tool results and tool choice are mapped and covered by a multi-round contract test.
- Observability degradation: DuckDB startup failure no longer prevents gateway startup; health, admin APIs and Prometheus expose degraded/lost-event state.
- Observation operations: queued writes batch up to 64 events per transaction and startup applies the configured retention window.
- Routing: endpoint and stored capability now filter same-alias targets.
- Auditability: provider/model/key creates and deletes append actor/resource audit events.
- Frontend: desktop focus trapping is disabled, list/query failures render explicit retry states, clipboard errors have a fallback, zero budget is not displayed as unlimited, and remaining fixed UI copy is localized.
- Actions: setup-node no longer requests pnpm caching before pnpm exists on a clean runner.
- Repository hygiene: `/target` ignores the local build-cache symlink.

## Accepted v0.1 boundaries

- A single non-streaming request may exceed a very small remaining budget; budgets are operational guardrails, not a prepaid hard ledger.
- Unbudgeted streams may not expose provider usage. They are marked `usage_unavailable` instead of inventing token/cost values.
- Cross-protocol streaming conversion is not attempted. Protocol-native passthrough remains available.
- v0.1 does not yet include distributed rate limits, SSO/RBAC projects, weighted health scoring, model discovery or a multi-node control database.

## Verification after fixes

- Rust unit/integration tests include SQLite setup/key auth, encrypted secrets, DuckDB normal/degraded behavior, OpenAI proxying, `x-api-key`, and Anthropic tool round trips.
- Browser QA covered setup, provider/model/key creation, English/Chinese, light/dark, desktop and 375px mobile navigation.
- Release binary startup and deep-link SPA content type were verified locally.
