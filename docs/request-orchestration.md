# Request orchestration contract

The existing Chat Completions, Responses and Anthropic Messages endpoints use the same orchestration boundary. Provider transformations remain adapters; selection, access, admission and retry decisions belong to Pangolin. All examples below are version 1 configuration documents stored in the Task 1 normalized tables. Management UI and expanded protocol adapters are separate parity tasks.

## Decision order

1. Authenticate the API key, trusted peer IP, lifecycle, project, scopes and budget using the shared access code.
2. Check the profile's public-model allowlist, then map the public model. Exact mapping entries and `regex:` entries follow `(priority, id)` order. Regex targets support capture replacement such as `$1`.
3. Match project-local model associations: exact requested name, requested-name regex, or channel tag. Optional `model_id` and `provider_id` narrow the target. Tag associations without a model ID retain the requested public model. Conditions are evaluated before an association contributes a candidate. Direct model names remain fallback associations.
4. Filter disabled models/channels/credentials, allowed tags, endpoint capabilities, stream policy, model exclusions, active health backoff and both channel-wide and credential quota snapshots. An expired quota period does not suppress a channel. Exhausted credentials are skipped before choosing the lowest `(priority, id)` enabled credential.
5. Resolve upstream model transformations; deduplicate by channel and final upstream model, keeping the best `(association priority, channel ID, model ID)` match.
6. Filter open model/channel circuits; order priority tiers using the selected strategy. Apply a valid scoped sticky binding if present. Sticky bindings may keep a previously successful lower-priority channel; failed or filtered bindings fall back to normal ordering.
7. Inject active prompts, enforce prompt protection and allowed tools, and reserve API-key admission. Each actual attempt obtains fresh channel admission and an exclusive circuit probe where needed. Overrides are applied to a fresh payload per attempt; protection and tool restrictions run again afterwards.

`Plan.decisions` contains ordered stage/candidate/reason records. Debug tracing emits these identifiers and fixed reasons, never credentials or request bodies. Authentication errors, model denial and malformed configuration fail closed. `/v1/models` uses the same project/profile/candidate filters and includes explicit profile aliases.

## Routing, limits and circuit settings

Project `settings_json.routing` supplies routing defaults for keys without a profile. A profile's `routing_policy_json` supplies its routing policy; its relational `rpm_limit` and `tpm_limit` are authoritative. Example:

```json
{
  "version": 1,
  "strategy": "round_robin",
  "sticky": "trace",
  "max_attempts": 3,
  "allowed_endpoints": ["/v1/chat/completions", "/v1/responses"],
  "allowed_tags": ["eu", "production"],
  "tag_mode": "all",
  "allowed_tools": ["search", "lookup"],
  "limits": {"concurrent": 16, "queue": 32, "queue_timeout_ms": 1000}
}
```

Strategies: `failover`, `round_robin`, `weighted`, `least_inflight`, `latency`, and `adaptive`. Priority tiers are deterministic. Round robin uses an atomic scoped sequence; weighted selection samples without replacement using association weights. Least-inflight uses live held permits, latency uses successful terminal EWMA measurements, and adaptive multiplies latency by `(inflight + 1)`. Score ties use stable candidate IDs. Stickiness uses `x-trace-id` or `x-session-id`, scoped to project/key/model/endpoint, with a 30-minute idle expiry and 10,000-entry cache limit. A successful terminal outcome establishes the binding.

Channel `providers.settings_json`:

```json
{
  "version": 1,
  "tags": ["eu", "production"],
  "pass_user_agent": true,
  "limits": {"rpm": 60, "tpm": 200000, "concurrent": 8, "queue": 16, "queue_timeout_ms": 2000},
  "circuit": {"enabled": true, "failures": 5, "window_ms": 60000, "recovery_ms": 30000}
}
```

Limits are process-local, intended for Pangolin's single-process deployment. RPM/TPM use Governor's replenishing minute quotas; zero disables admission, omission removes that limit. Tokio semaphores provide bounded FIFO concurrency and waiting. Cancellation or timeout releases slots without detached waiters. Pending policy updates replace an idle resource pool; existing permits keep their original pool. Atomic completed/failed/inflight counters and queued gauges are aggregated by resource kind on the existing public `/metrics` endpoint; project/channel/key identifiers are never exposed there.

API-key RPM counts logical requests; channel RPM counts every upstream attempt. Each retry reserves API-key TPM again. Token reservations use UTF-8 request bytes plus an output ceiling; this is intentionally conservative for text. Missing output ceilings are set to 4096 for TPM-limited requests. Media requests under TPM policy fail explicitly until a provider-aware estimator is installed. Reservations are not refunded for failures. Quotas may conservatively consume token capacity when a later RPM/circuit check rejects admission. No global/distributed rate guarantee is claimed.

Circuit failures are isolated by channel and resolved upstream model, recorded in a moving failure window. Recovery admits one probe; success closes the circuit, failure reopens it, cancellation releases the probe without classifying client cancellation as a provider failure. Local admission rejection does not create a circuit failure.

Profile budgets aggregate existing SQLite API-key spend and serialize requests sharing the budget. Budget-limited streaming remains explicitly unavailable until provider-neutral streaming settlement is implemented. Persisted `channel_health_state` and `provider_quota_snapshots` are consumed here; scheduled probes, quota collection and durable execution/cost accounting belong to the operations task.

## Conditions, model rules and overrides

Conditions use JSON pointers into `{"body": ..., "headers": ..., "endpoint": ...}`. Sensitive headers are absent. Supported operators are `eq`, `ne`, `in`, `contains`, `regex`, `exists`, `gt`, `gte`, `lt`, `lte`; nested `all`/`any` groups have a depth limit of 16. Missing fields never satisfy `ne`. Unknown operators and malformed regexes are configuration errors. An empty version-1 document matches unconditionally.

```json
{"version":1,"all":[
  {"field":"/endpoint","op":"eq","value":"/v1/chat/completions"},
  {"field":"/body/temperature","op":"lte","value":0.5}
]}
```

Channel `model_rules_json` supports `strip_prefix`, `lowercase`, exact `mappings`, then `prefix`; `exclude` is a list of regexes against the original upstream name. `stream:false` excludes streaming. Request transforms support `developer_to_system`, `force_content_array`, and `reasoning_effort` mappings (including `auto` for missing effort).

`endpoint_mappings_json.paths` maps an inbound endpoint to a provider-relative path. URLs, protocol-relative paths and query/fragment injection are rejected. The existing OpenAI-to-Anthropic bridge defaults to `/v1/messages`.

`parameter_overrides_json.operations` is an ordered array of optional `when`, JSON Merge Patch `merge`, RFC 6902 `patch`, and `headers` objects. A singleton `{"$request":"/body/model"}` references a typed value from sanitized request context. Ordinary strings are literal. This is a Rust JSON contract, not an interpreter for Go templates.

```json
{"version":1,"operations":[
  {"when":{"field":"/body/model","op":"eq","value":"public"},
   "merge":{"temperature":0.2,"metadata":{"routed_model":{"$request":"/body/model"}}},
   "headers":{"x-gateway":"pangolin"}},
  {"patch":[{"op":"add","path":"/seed","value":42}]}
]}
```

The selected model and stream flag are immutable after routing. Credentials, cookies, sensitive headers and hop-by-hop headers cannot be overridden or referenced from inbound headers. A null header value removes an earlier override. Native requests preserve unrecognized body fields; User-Agent pass-through is opt-in. Request/trace IDs and bounded thread/session IDs are propagated independently of authentication headers.

Prompt records prepend activated messages in stable `(created_at,id)` order; native Anthropic system prompts use the top-level system field. Protection rules match role/content regexes and conditional scopes, with deny, literal redaction replacement or non-mutating test mode. They cover text and text arrays in Messages, Responses input, system and instructions. Errors never echo matched content. Tool allowlists filter definitions and reject disallowed explicit choices; historical tool calls/results retain their order. Responses `allowed_tools` choices survive unchanged when permitted.

## Retries and streaming

Channel `retry_statuses_json` accepts `statuses`, `error_patterns`, `attempts`, `delay_ms`, `transport`, `empty_success`, `first_event_timeout_ms`, `event_timeout_ms`, `error_mode` (`normalized`, `pass_through`, `custom`) and `error_message`. Per-channel attempts and the routing-wide maximum both apply. Defaults are one attempt per candidate, three overall, and retry statuses 408/409/429/500/502/503/504. Error-pattern matching never logs the upstream error body. Nonretryable errors stop immediately. Error pass-through is an explicit operator policy; normalized errors are the default.

An SSE parser buffers a complete first event before returning a downstream response. Empty streams, first-event failures/timeouts and transport errors can retry before commitment. Once the downstream body emits any event, it permanently owns the attempt and cannot return to the retry loop. Truncated EOF or event timeout then fails the stream. Chat `[DONE]`, Anthropic `message_stop`, and a completed Responses terminal event define success; provider failure events remain failures. SSE event/data/id/retry semantics are preserved; framing is normalized and comments are not forwarded. An event has a conservative 1 MiB buffering bound; nonstream responses are limited to 16 MiB.

## Durable Responses continuity

Completed Responses outputs and normalized inputs are encrypted with Pangolin's existing `SecretBox` and stored in `response_sessions`. Composite API-key/project ownership is enforced by SQLite foreign keys. The row contains a UUID, external response ID, version, encrypted state, update time and expiry; `(api_key_id,response_id)` is unique. Key or project deletion cascades to session state.

The cache is derived acceleration. Every restore checks the authoritative row's scope and expiry and validates encrypted ownership, version and envelope revision. Another key, expired response or missing response ID fails explicitly instead of forwarding an unscoped upstream identifier. Restoring history removes transport status fields, preserves tool/message ordering and keeps current instructions. Persistence completes before a streamed terminal event is delivered.

Limits: 30-minute expiry, 1 MiB plaintext per record, 128 records per key, 10,000 records and 256 MiB encrypted state globally; hot cache is weighted to 32 MiB. Writes prune expired/old records transactionally. Oversized records are not persisted. Reopening SQLite and reloading the same master key restores continuity. Responses compact/WebSocket transports consume this boundary in the protocol task.
