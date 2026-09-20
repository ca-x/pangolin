# Task 3 report — request orchestration

## Status and scope

Implemented the request orchestration engine in the isolated `feat/axonhub-parity` worktree. Existing Chat Completions, Responses, Anthropic Messages and model-list endpoints remain available. The root `main` checkout was checked and remains clean. Controller-owned capability/spec/plan files were not staged or edited by this task.

The implementation replaces the old unscoped `db::resolve_targets` loop with focused policy, repository, runtime, protection, SSE and Responses-session modules, plus a gateway execution module. Existing provider transformation and observation helpers remain behind that boundary for Tasks 4 and 5 to consume. No AxonHub Go implementation was copied.

Commit and final release result are recorded in the final handoff section below.

## Delivered behavior

- Shared Task 2 authentication, trusted-peer IP, lifecycle and scope checks remain binding. Disabled projects, disallowed public models and disallowed endpoints fail closed. Model listing now respects project, profile alias/allowlist and routable candidate boundaries.
- Public-model profile mapping precedes associations. Exact and regex mappings, exact/regex/tag associations, model/provider narrowing, nested conditions, channel tag modes, stable priority and final upstream-model deduplication are covered.
- Candidate filtering covers enable state, credential selection, endpoint and stream capabilities, model exclusions, persisted channel backoff, channel-wide quota and credential quota. A newer credential snapshot cannot hide exhausted channel-wide quota; exhausted credentials can fall back to another enabled credential.
- Model rules support prefix stripping, lowercasing, explicit mappings, prefix addition, developer/system normalization, content-array forcing and reasoning-effort mappings.
- Failover, round robin, weighted sampling without replacement, least-inflight, terminal latency EWMA and adaptive selection retain stable priority tiers and stable score tie-breaking. Trace/session affinity is scoped to project/key/model/endpoint and has bounded cache capacity/expiry.
- Governor quotas and Tokio owned semaphore permits enforce API-key/channel RPM, conservative TPM, channel/key concurrency and a bounded queue with timeout and cancellation cleanup. Same-channel retries consume fresh channel admission and additional key token reservations. Atomic counters track inflight, completed and failed requests, including cancellation.
- Model/channel circuit state uses a failure window and one exclusive half-open probe. Success closes a recovery probe, upstream failure reopens it, and local cancellation releases it without recording an upstream fault.
- Global and channel attempt limits, delay, configurable statuses/error patterns, transport failures, empty successes and normalized/pass-through/custom upstream errors are handled centrally. Configured overrides also apply to the existing Anthropic bridge's headers and endpoint.
- Streaming reads a complete first SSE event before downstream commitment. Empty/failed/timed-out first events may retry. After a downstream event is yielded there is no control-flow path back to the retry loop. EOF before protocol completion is a failure; terminal Chat/Anthropic/Responses semantics determine success. Dropping an unpolled or partially consumed downstream body releases key/channel/circuit guards.
- Ordered conditional JSON Merge Patch/RFC 6902 operations and typed request-value references operate on a fresh per-attempt body. Selected model and stream mode cannot be changed after routing. Sensitive/hop-by-hop headers cannot be overridden; inbound sensitive headers cannot enter template context. Native pass-through retains body fields; User-Agent forwarding is opt-in.
- Prompt injection, role/content regex protection, deny/literal-redact/test mode, conditional scopes and tool allow/filter rules preserve message/tool history order. Protection and allowed-tool policy are reapplied after channel overrides.
- Supplied/generated request and trace IDs are propagated; thread/session headers are bounded and propagated. Redacted `Plan.decisions` expose explicit stage/candidate/reason order through debug tracing. Public metrics add aggregate resource-kind counters without exposing project/channel/key identifiers.

## Durable Responses ruling and schema

The controller explicitly ruled that process-local-only session continuity was insufficient. Schema ledger version 5 adds `response_sessions`, a composite API-key/project foreign key, uniqueness on `(api_key_id,response_id)`, an expiry index and a scope/update index. The API-key parent receives the required composite unique index. Fresh-schema and idempotence tests include the new schema version.

Session state is versioned JSON encrypted using the existing `SecretBox`. Its encrypted metadata binds it to project, API key and response ID. SQLite is authoritative even on a hot-cache hit: restore checks scope, expiry, version and envelope revision. Missing or foreign response identifiers fail explicitly rather than being forwarded to a shared provider credential.

Completed input/output history survives reopening SQLite with the same master key, including custom tool calls. Current instructions are preserved, transport status fields are removed, and history ordering is retained. A streamed completed response is persisted before its terminal event is yielded, so client disconnect immediately after that event does not lose continuity.

Storage is bounded: 30-minute expiry; 1 MiB plaintext per record; 128 records per key; 10,000 records / 256 MiB encrypted state globally; weighted hot cache of 32 MiB. Writes prune expired/old records transactionally. Key/project deletion cascades. Oversized session records are deliberately not retained.

## AxonHub invariant sources and Rust coverage

Read-only source inventory used `/home/czyt/code/others/axonhub/internal/server/orchestrator/*_test.go`. Tests port observable contracts into Rust fixtures, not Go implementation structure.

| AxonHub source tests | Pangolin coverage |
| --- | --- |
| `model_mapper_test.go`, `model_access_test.go` | Public alias access before rewriting; exact/regex mapping; profile allowlist and limits/budget; project-aware model listing |
| `candidates_basic_test.go`, `candidates_condition_test.go`, `candidates_tags_test.go`, `candidates_dedup_test.go` | Exact/regex/tag/model/provider candidates, nested field conditions, any/all/none tags, deterministic best-priority dedup and project isolation |
| `candidates_quota_test.go`, `candidates_stream_policy_test.go`, `select_endpoints_test.go` | Quota precedence/expiry, credential fallback, health backoff, disabled credentials and endpoint/stream eligibility |
| `lb_strategy_rr_test.go`, `lb_strategy_weight_test.go`, `lb_strategy_latency_test.go`, `lb_strategy_composite_test.go`, strategy simulations | 300-selection exact round-robin distribution, seeded 10,000-selection weight simulation, priority across all strategies, least-inflight and measured latency/adaptive decisions |
| `candidates_sticky_test.go` | Scoped cached selection and deterministic missing-candidate fallback |
| `rate_limit_admission_test.go`, `channel_limiter_test.go`, `channel_request_tracker_test.go` | 100-request parallel RPM race, peak concurrency exactly five under 100-way contention, exact terminal counters, queue-full/cancel/timeout cleanup and retry TPM charges |
| `model_circuit_breaker_test.go`, circuit simulations | Model isolation, failure-window opening, exclusive half-open probe, cancellation release and successful recovery |
| `retry_test.go`, `upstream_transport_error_test.go`, `orchestrator_streaming_test.go`, `terminal_stream_test.go` | Mock HTTP status fallback, global/channel bounds, nonretryable stop, fresh retry bodies, first-event empty/timeout fallback, no retry after first event, incomplete EOF failure and dropped-body cleanup |
| `override_test.go`, `pass_through_test.go`, `transform_options_test.go` | JSON merge/patch, typed references, header/body protection, channel endpoint/header override through Anthropic, model/role/content/reasoning transformations |
| `prompt_test.go`, `prompt_protection_test.go`, `allowed_tools_test.go` | Activated prompt prefix, text-array redaction, scoped roles, non-mutating test mode, deny behavior, post-override enforcement, unchanged tool history and allowed-tool choice propagation |
| `responses_session_test.go` | Scope isolation, normalization, custom tool history, streamed record-before-close, SQLite reopen, encrypted-at-rest state, expiry on cache hits, cleanup and FK cascade/scope constraints |

There are 28 new focused/domain/composed tests; all 29 pre-existing tests remain green, for 57 Rust tests total.

## Maintained library boundaries

The lockfile pins the resolved crates. Manifest license checks were performed against the downloaded crate manifests:

- `governor` 0.10.4 — MIT; replenishing minute quotas.
- `dashmap` 6.2.1 — MIT; concurrent resource/circuit/rotation maps.
- `moka` 0.12.16 — `(MIT OR Apache-2.0) AND Apache-2.0`; bounded affinity and weighted session caches.
- `regex` 1.13.1 — MIT OR Apache-2.0; model, condition and protection regexes.
- `json-patch` 4.2.0 — MIT/Apache-2.0; JSON Merge Patch and RFC 6902.
- `eventsource-stream` 0.2.3 — MIT OR Apache-2.0; incremental SSE parsing.
- Existing `ipnet` continues to own trusted-peer CIDR policy parsing; Tokio owns semaphore fairness/cancellation; existing encryption remains unchanged.

Pangolin-specific wrappers own policy ordering, SQLite joins, scoping, RAII terminal outcomes and explicit retry boundaries.

## Self-review

- Rechecked request lifecycle cancellation, including the unpolled downstream body case; owned guards are captured before response return and are released on drop.
- Corrected channel-wide/credential quota precedence and added a regression test; the selected credential cannot mask channel exhaustion.
- Added SQLite validation even on hot session cache hits, encrypted ownership binding and envelope-revision invalidation, preventing stale cache entries from overriding expiry or deletion.
- Kept original body clones per candidate so a failed channel's override cannot contaminate fallback requests.
- Reapplied prompt/tool policy after channel overrides and reserved key tokens against the largest effective candidate payload, not just the original request.
- Disabled upstream HTTP redirects in the production client, preserving credentials across routing boundaries.
- Checked the historical `/metrics` route and found it is public. New metrics therefore aggregate by resource kind and never include tenant/channel/key identifiers; this task does not change that existing route's access contract.
- Checked the root checkout: `main`, clean. Controller-owned `CAPABILITY_MAP.md`, capability matrix, spec and plan/todo files remain outside this commit.

## Verification

- `pnpm --dir web lint` — passed.
- `pnpm --dir web test` — 1 passed, 0 failed.
- `pnpm --dir web build` — passed before final Rust verification.
- `cargo fmt --all -- --check` — passed.
- `cargo clippy --locked --all-targets -- -D warnings` — passed.
- `cargo test --locked` — 57 passed, 0 failed.
- `git diff --check` — passed.

## Fix round 2 — Anthropic outcome and capability closure

Two remaining Important findings were closed.

- Anthropic bridge `reqwest::Error` failures now always settle the active
  `AttemptGuard` as `UpstreamFailure` before the retry policy is consulted.
  With transport retry disabled the request ends as a 502, but the channel/key
  counters and a half-open circuit are still settled as a provider failure.
- Candidate preflight now separates candidate request compatibility from TPM
  accounting. An Anthropic Chat candidate whose final transformed/overridden
  request has `n != 1` is removed with an
  `unsupported_completion_count` capability decision; compatible OpenAI
  candidates remain routable. If that leaves no candidate, preparation returns
  the explicit Anthropic completion-count request error. Full TPM/media
  validation stays in attempt admission, preserving the prior policy boundary.

Red/green evidence:

- Red: the bridge only called `attempt.finish(UpstreamFailure)` when
  `retry.transport` was true; setting it false returned a local bad-request
  path and left a half-open probe/circuit outcome unresolved. Also, preflight
  invoked full `attempt_payload` for each candidate, turning a lower-priority
  Anthropic `n=2` incompatibility into a global error.
- Green: `review_anthropic_transport_failure_settles_half_open_when_retry_is_disabled`
  uses a closed local socket, confirms a 502, failed channel/key counters and
  a reopened circuit. `review_anthropic_multi_choice_candidates_are_skipped_without_blocking_openai`
  confirms the capability decision, one OpenAI `n=2` upstream call and the
  explicit all-Anthropic 400. The existing TPM/media policy test remains green.

Fresh verification after the fix:

- `pnpm --dir web lint` — passed.
- `pnpm --dir web test` — 1 passed, 0 failed.
- `pnpm --dir web build` — passed.
- `cargo fmt --all -- --check` — passed.
- `cargo clippy --locked --all-targets -- -D warnings` — passed.
- `cargo test --locked` — 73 passed, 0 failed.
- `cargo build --release --locked` — passed.
- `git diff --check` — passed.
- `cargo build --release --locked` — passed; release build completed in 1m 49s.

## Explicit boundaries for later tasks

- Rate, concurrency, latency and circuit state are process-local; this single-process gateway does not claim distributed admission guarantees. Session continuity is durable in SQLite.
- TPM uses conservative text reservations. Provider-aware token estimates for media and exact usage reconciliation belong at the Task 4/5 adapter/accounting seams. Media plus a TPM policy returns an explicit error today; it is not silently undercounted.
- Existing streaming budget restriction is preserved for both key and profile budgets until reliable stream usage settlement exists. Profile spend is currently derived from SQLite API-key spend.
- Scheduled probes, quota collection, auto-disable persistence/recovery jobs, detailed request/execution/usage storage and analytics are Task 5. This task consumes their schema state and exposes the orchestration boundary they need.
- Compact/WebSocket and additional protocol/provider routes remain Task 4; the durable Responses store is transport-independent and ready for those adapters. No real provider credentials were used or claimed verified.
- JSON policy syntax is deliberately Rust-native and documented in `docs/request-orchestration.md`. It does not execute Go templates. Native body fields are preserved; this task does not introduce a lossy body-field whitelist mode.
- SSE event/data/id/retry semantics are preserved with normalized framing; comments are not forwarded. Conservative buffering caps are 1 MiB per SSE event and 16 MiB per nonstream body.

## Final handoff

Completed in atomic commit `5ed7a93d0e7a804962a818f2fc2fa373d05ffc6a` (`feat: add scoped request orchestration and durable response sessions`).

All required local gates passed: web lint/test/build, Rust format check, clippy with warnings denied, 57 Rust tests, release build and staged whitespace check. No source edits followed those verification commands. The final worktree status contains only controller-owned capability/spec/plan changes; the parent `main` checkout is clean. No external publication occurred.

## Fix round 1 — parity review closure

The eight Important review findings were re-audited against the unstaged Task 3
diff: TPM reserves now multiply a validated `n` by the selected output ceiling
and the Anthropic bridge uses that same ceiling; allowed-tool policy rejects
legacy `functions`/`function_call`; nested Anthropic and Responses tool output
is protected; durable Responses history excludes injected prompts; Responses
terminal status variants are classified before persistence; rotation state is
bounded and only allocated for nontrivial round-robin selection; and upstream
failure classification is independent of retry eligibility, sticky binding and
circuit health.

One additional stream-boundary gap was found during this audit. Before the
change, EOF or an event-read error after a downstream SSE event reached the
client yielded a raw I/O error and skipped the configured normalized/custom/
pass-through SSE error policy. The new `interrupted_event` path emits a safe,
protocol-valid terminal event, releases the upstream guards before that final
yield and never retries after commitment. The historical EOF test was updated
from expecting a raw I/O error to asserting that terminal policy event, while
retaining the no-retry and failed-observation checks.

Red/green evidence:

- Red (inspection and prior assertion): the post-commit `Ok(None)`/`Err` stream
  arms yielded `Err` directly; `no_retry_after_first_event_and_eof_is_not_success`
  encoded that raw-I/O behavior.
- Green: `review_post_commit_stream_interruptions_obey_each_error_policy` proves
  Responses EOF delivers `response.failed` for normalized, custom and
  pass-through modes, uses the configured custom text, records one failed
  channel attempt and makes one upstream call. The updated historical EOF test
  proves an OpenAI stream emits its terminal normalized error without retrying.

Fresh verification after the fix:

- `pnpm --dir web lint` — passed.
- `pnpm --dir web test` — 1 passed, 0 failed.
- `pnpm --dir web build` — passed.
- `cargo fmt --all -- --check` — passed.
- `cargo clippy --locked --all-targets -- -D warnings` — passed.
- `cargo test --locked` — 71 passed, 0 failed.
- `cargo build --release --locked` — passed.
- `git diff --check` — passed.
