# Spec: gateway

## Objective

Offer a drop-in AI API endpoint with manageable providers, model aliases and virtual keys. Operators should be able to switch upstreams without changing clients.

## Interfaces

- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/messages`
- `GET /v1/models`
- Admin APIs under `/api/admin/v1/*`

## Functional requirements

- Authenticate gateway calls using hashed virtual Bearer keys.
- Resolve a requested model alias to enabled targets ordered by health, priority and weight.
- Retry only safe pre-response failures; never replay a streaming response after bytes have been sent.
- Preserve upstream status codes and OpenAI-shaped error envelopes where applicable.
- Track token usage and integer micro-USD cost without floating-point accounting.
- Keep protocol transformation behind a replaceable adapter modeled after LiteLLM Rust. v0.1 implements the supported non-streaming Anthropic bridge locally because the upstream crates are not independently published and force full-repository Git resolution; use protocol-compatible passthrough for streaming and unsupported endpoints.
- Support OpenAI-compatible, Anthropic and custom OpenAI-compatible upstream definitions in v0.1.

## Data model

- `providers`: kind, base URL, encrypted secret, enabled state and health state.
- `models`: public alias, upstream model, capabilities, integer prices and priority. Multiple rows sharing a public alias are its ordered targets.
- `api_keys`: prefix, Argon2id hash, scopes, optional budget/rate policy and usage total.
- `audit_events`: administrator mutations with actor and structured details.

## Testing strategy

- Mock upstreams prove header redaction, route choice, fallback and error mapping.
- Contract fixtures cover OpenAI chat requests, SSE passthrough and Anthropic Messages.
- Concurrency tests prove budget updates cannot lose increments within one process.

## Success criteria

- The official OpenAI SDK can call the configured Pangolin base URL without request-shape changes.
- A disabled or unhealthy target is skipped; a second target receives an eligible retry.
- API secrets never appear in JSON responses or logs.
- Model CRUD and key creation are usable from both REST and the console.
