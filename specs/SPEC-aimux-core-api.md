# Spec: aimux core API and Token usage

## Scope and authority

The user confirmed on 2026-11-06: replace the API access layer with
`arcships/aimux`; LiteLLM is no longer required; align core API behavior and
Token usage with `/home/czyt/code/go/new-api`. Memberships, recharge, payments,
subscriptions and merchant features are excluded. The user authorized the agent
to check the design and proceed without another approval round.

This continuation extends the existing `gateway`, `protocol-surface`,
`trace-cost` and `console` modules, in that order. Existing AxonHub capability
rows, protocol contracts, project isolation, budgets and audit requirements
remain binding. Do not delete existing capabilities to narrow this iteration.

## Evidence

- Pangolin baseline: `6ac95bce771216c59c1fd5ad01ece24fda90e834` (v0.8.0).
- aimux: MIT, evaluated commit `4c0b40c54d689439937155d2c36720a4b3c7454d`.
- Local new-api: AGPL-3.0, commit `e03439033238722c0b8d4437a052445f3aeeed83`.
  Its README, public routes and behavioral tests are references only; no Go,
  RelayKit, JavaScript implementation or image assets may be copied.

## Adapter contract

- Native requests use aimux's `FetchRequest` / `FetchResponse` boundary without
  normalizing arbitrary provider JSON into an SDK generation DTO. Preserve
  multipart bytes, binary responses, unknown native fields and upstream status.
- Pangolin controls the HTTP client so configured proxy credentials, pinned
  proxy DNS, connection reuse, deadlines and redirect refusal are preserved.
  SDK-global transports, automatic retries and recording must not bypass policy.
- Use aimux SSE framing with a Pangolin-owned hard event-size bound. Cover
  fragmented UTF-8, CRLF, comments, multi-line data, malformed input and EOF.
- Remove every LiteLLM Cargo dependency, including optional Redis affinity.
  Use maintained, pure-Rust auth/tokenizer/cache libraries behind local adapters.
- Retain OpenAI Chat/Responses, Anthropic Messages and Gemini native routes,
  models, existing embeddings/rerank/media routes and explicit rejection of
  unsupported cross-protocol shapes. Do not advertise all aimux provider names
  as contract-tested gateway integrations.
- Keep cloud credentials explicit. Never silently fall back to unrelated
  process environment credentials. Preserve Azure key/token/client-secret and
  explicitly selected workload/managed identity,
  Bedrock signing and Vertex service-account/external-token contracts.
- Reject Azure's former automatic `DefaultAzureCredential` and
  `DeploymentIdentityCredential` chains explicitly, rather than restoring
  failure-driven switches to developer logins or a different managed identity.
  Document this intentional security tightening and the single-identity
  migration; a refresh flag alone must not discover credentials.
- No provider/network I/O inside a SQLite business transaction and no retries
  after downstream bytes are committed.

## Token usage contract

- SQLite execution and usage facts remain authoritative; DuckDB is rebuildable
  analytics only. Requests, attempts and terminal settlements stay idempotent.
- Preserve native input/output, cache-read/cache-write and reasoning counters,
  including valid zero values. Unknown is not zero; show `—` when unmeasured.
- Never add cache or reasoning subsets to a total that already includes them.
  Never treat a local tokenizer/admission reservation or SDK conversion default
  as reported provider usage.
- Streaming progress is not final usage. Missing final usage, cancellation,
  errors and retries keep their existing conservative settlement behavior.
- Verify project/key/model/channel/time filters, totals and request details
  agree; refuse cross-project reads and preserve explicit error/retry states.
- Retain immutable micro-USD prices and existing cost accounting; no new wallet,
  paid subscription or billing formulas are part of this work.

## Frontend and interactions

Use `ui-ux-pro-max` for accessibility/forms/data states, `emil-design-eng` for
feedback and polish, `frontend-design` for intentional hierarchy, and
`apple-design` for immediate, interruptible, spatially consistent interactions.
The existing MASTER tokens win over generic skill examples. Keep zh-CN/en,
light/dark/system, bronze/slate/jade, 375/768/1440px, Radix focus management,
14px operational text and reduced motion. No new commercial navigation.

## Verification

Build Web assets before Rust verification. Add red/green contract tests for
changed adapters and retain existing protocol/usage/affinity regressions.
Run the commands in AGENTS.md, including `redis-affinity` compilation/testing.
Browser QA uses a fresh release binary with populated, sanitized mock data;
provider integrations are `contract-tested`, not `live-tested`. GPT-6 review
remains a separate final gate; an unavailable model is not a passed review.
