# Accounting and operations

SQLite is authoritative. Requests freeze their log policy and price snapshot before an upstream attempt. A terminal transaction writes execution status, usage, itemized integer micro-USD costs and API-key spend together. Legacy model input/output rates are automatically captured as immutable price versions. Operator price versions take precedence and support marginal tiers, cache read/write (including 5m/1h variants), reasoning, flat and protocol-unit components. Each item rounds upward to the nearest micro-USD. Service-group ratios are integer millionths and are frozen per request.

Hard budgets reserve a conservative upper bound before contacting a provider. Streams settle provider-reported usage before delivering a terminal event. Cancellation, lost usage or process interruption retain a conservative charge instead of releasing potentially spent budget; `settlement_kind` makes that distinction visible. A definitive HTTP rejection is uncharged. Media unit quantities are image count, explicit video seconds or speech characters. Uploaded media with an unknown duration requires flat pricing for hard-budget keys. Task polling/deletion is uncharged. Repeated provider response IDs are deduplicated within project, API key, provider and endpoint; distinct clients never share a deduplication namespace.

Key/profile budgets enforce the reserved output ceiling even without TPM limits, using the native Chat/Responses/Gemini limit field and Gemini candidate count. Partial stream reports (including Anthropic `message_start`) do not release reservations; only a valid protocol-final usage report permits exact settlement.

On startup, interrupted attempts settle before the HTTP listener starts. DuckDB failure never changes enforcement or SQLite settlement. The existing global DuckDB endpoints require system authority; project analytics and operational browsing use project-scoped SQLite queries and continue in degraded mode.

## Request logging

`GET/PUT /api/admin/v1/settings/request-logging` is system-only. The document contains:

```json
{"enabled":true,"default_level":"metadata","key_override_enabled":false,"key_disable_allowed":false}
```

Keys select `inherit`, `off`, `metadata`, `redacted_body` or `full_body` through the project `key-logging` mutation. Site disable always wins. Key choices require site override permission; key disable additionally requires `key_disable_allowed`. Defaults store metadata only. `off` creates no browsable thread/trace/request/execution/content rows and sends no DuckDB event, while retaining minimal accounting and security audit records. Existing encrypted Responses history remains an operational session store.

Both body levels remove authorization, cookie, password, credential and token fields recursively. Full-body logging also omits structured base64/data-URI media, including media inside JSON-encoded tool output, and removes URL credentials/query strings. Logging sanitization does not mutate the upstream request. Stored stream content is bounded to 1 MiB. gzip, deflate, Brotli and zstd upstream responses use reqwest decoding before the existing decoded-body/SSE limits and settlement; unsupported residual/stacked encodings fail explicitly. Incoming compressed requests and historical header-casing quirks are not enabled by this change.

## Project API

Base path: `/api/admin/v1/projects/{project}/operations`.

Interactive requests use the existing session cookie. Mutations also require `X-Pangolin-CSRF: 1`; cross-site browser mutations are rejected. Scoped bearer/API-key principals use the same authorization evaluator and do not require browser CSRF headers. Reads require `project:read`; mutations require `project:manage`. Audit records and control-plane changes commit in the same transaction, preserving service-key subject IDs separately from user IDs.

`GET /{resource}` supports `offset` and `limit` (maximum 500). `GET /{resource}/{id}` reads one scoped item. Read resources include `threads`, `traces`, `requests`, `executions`, `usage`, `cost-items`, `prices`, `probes`, `quotas`, `health`, `credential-health`, `storage`, `schedules`, `jobs`, `backup-target-results`, `webhooks`, `webhook-deliveries`, `groups`, `retention` and `audit`. `GET /content/{request_id}` returns permitted stored request/response content, including in-flight inbound content.

`POST /{resource}` mutations:

| Resource | Input |
| --- | --- |
| `key-logging` | `api_key_id`, `level` |
| `prices` | `model_id`, `components`, optional `valid_from`, `valid_until`, `schedule` |
| `groups` | `name`, `tier`, `ratio_millionths` (or decimal `ratio`), optional `channels`, `api_keys`, `enabled` |
| `health-policy` | `provider_id`, versioned `policy`: `enabled`, `action` (`channel`/`credential`), threshold/status/pattern, duration or recovery cron/timezone |
| `storage` | `name`, typed `config`, optional private `secret`; updates include `id`, `revision` |
| `schedules` | `kind`, `payload`, `interval_secs`, optional `enabled`; updates include `id`, `revision` |
| `probe` / `quota` | `provider_id`; quota also accepts provider-relative `path`, `remaining_pointer`, `scale_micros`, period boundaries |
| `webhooks` | `name`, `url`, `events`, optional public `headers`, private `secret_headers`, `body`, `enabled` |
| `webhook-echo` | Emits a `test` event to subscribers |
| `retention` | `resource_type` (`requests`, `payloads`, `probes`, `quota`), `retention_days` |

Storage, schedules, webhooks, retention policies and groups support DELETE. Price versions and accounting facts are immutable through these APIs. Referenced historical prices also prevent destructive model/channel deletion; disable the resource instead.

Price schedule rules have `priority`, IANA `timezone`, `start_minute`, `end_minute`, optional ISO weekday numbers (Monday=1), epoch `from`/`until`, and a `prices` map keyed by component kind. Lower priority wins; midnight-crossing windows belong to their starting weekday. Components have `kind`, `unit_size`, `unit_price_micros`, optional marginal `tiers` and cache-write `cache_ttl`.

`GET /api/admin/v1/projects/{project}/analytics` accepts `from`, `until`, `dimension` (`day`, `provider`, `model`, `api_key`, `user`, `project`), `model`, `provider`, and `api_key`. It returns request/attempt/error counts, usage, cache-hit tokens/savings, cost and available timing averages. Missing measurements remain null.

## Affinity and compaction

Project `settings_json.affinity_rules` is an ordered array of rules. Each has `id`, `mode` (`off`, `prefer`, `strict`), tagged `source`, `ttl_secs` (1–86400), optional regex `model`/`path`/`user_agent`, and `release_on_failure`. Example source: `{"kind":"pointer","value":"/prompt_cache_key"}`. Other sources are `trace`, `thread`, `session`, or a middleware-trusted header; currently only `x-pangolin-trusted-client-ip` is accepted as a trusted header. Arbitrary client headers cannot declare themselves trusted.

The default rules recognize Codex `prompt_cache_key` and Claude `metadata.user_id`. Fingerprints are hashed and key/project/model/path scoped. Memory affinity uses LiteLLM's bounded 10,000-entry cache. Optional Redis builds use `--features redis-affinity` and `PANGOLIN_AFFINITY_REDIS_URL`; configure Redis memory/eviction limits for the deployment. Affinity switch commits are serialized per fingerprint; a late failure cannot erase a newer successful binding. Independent inference requests are never response-coalesced. Disabled/ineligible channels invalidate bindings; pinned WebSocket channels require reconnect when unavailable.

Exact encrypted session replay is the default. Optional `settings_json.session_compaction` contains `enabled`, `threshold_tokens`, `retain_items`, `native`, and `summarizer_model`. Native Responses compact is tried first when selected; a configured summarizer can provide fallback. Failure retains exact current history. Immutable encrypted summaries record covered item count, scoped history hash, model, usage and creation identity. No tool-call/result pair crosses the covered boundary. A semantic-memory trait exists as an extension contract; no vector retrieval is installed or injected.

## Durable jobs and backups

Jobs claim atomically, carry monotonically increasing fencing tokens, renew bounded leases, retry with bounded exponential delays and stop after their attempt limit. External I/O occurs outside SQLite transactions. Each invalid schedule is isolated and records an error without blocking valid schedules. Catalog subscriptions automatically enqueue refresh jobs from their configured intervals.

Schedule kinds include `probe`, `quota`, `automatic_backup`, and `backup_retention`. An automatic backup payload contains `targets`, `resources`, and optional `retention_count` (1–1000). Target revision/configuration/credential envelopes are frozen when the due slot is claimed. Each target has its own status/error/size; successful deliveries are not repeated when another target fails. `channel.disabled`, `quota.exhausted`, `request.failed` and `test` webhook events are durable jobs; receivers should deduplicate the `Idempotency-Key` header. Webhook bodies may embed the whole event using `"$event"`.

Backup routes under `/api/admin/v1/projects/{project}/backup`:

- `POST /export`: `{"resources":[...]}` produces a checksummed, encrypted artifact.
- `POST /restore`: `{"artifact":{...},"strategy":"fail|skip|overwrite"}` validates and atomically imports it.
- `POST /run`: `{"targets":[...],"resources":[...]}` queues delivery.
- `POST /{job}/retry`: queues a new job using current target snapshots.

Selectable resources are providers/credentials/settings, models/associations/prices/components, key profiles/mappings/allowed models, API keys, prompts/protection, service groups/bindings, webhooks, retention, threads/traces/requests/contents, authoritative request/execution/usage/cost/dedup facts, Responses sessions/summaries, probes and quota snapshots. Use database table names as resource names and include missing dependencies when restoring into an empty project. Named channel/model/profile conflicts remap dependent references. Existing spend never decreases during restore. Immutable financial versions/facts cannot be overwritten with different values. Restore IDs deduplicate repeated imports.

Newly imported running executions are atomically marked interrupted and settled during selective restore; contacted attempts retain their reserved charge and uncontacted attempts cost zero. Existing live destination executions are not recovered or interrupted.

Project artifacts are project-bound and require the original master key; referenced projects/users must already exist. Whole-instance disaster recovery uses the separate owner-only mode below and does not require source identities to exist in advance. Keep the master key securely and separately from artifacts. Public manifests never contain target credentials. Local targets use a validated subdirectory of `PANGOLIN_DATA_DIR/storage`; S3-compatible targets use object_store with HTTPS and encrypted credentials bound to project/target identity. Retention deletes at most 100 objects per pass, restricted to successful objects recorded under the target's owned prefix and current revision. It never enumerates/deletes arbitrary bucket contents.

## Whole-instance disaster recovery

System owners can use three additional endpoints. Authorization and browser CSRF checks run before the potentially large request body is parsed.

- `POST /api/admin/v1/instance/backup`: `{"include_history":false}` creates an encrypted, consistent SQLite snapshot. Add `"passphrase":"..."` for portable export.
- `POST /api/admin/v1/instance/restore/preflight`: accepts `artifact`, optional `passphrase`, and `mode`; returns schema compatibility, source table counts, destination occupancy and encryption mode after validation in an isolated temporary database.
- `POST /api/admin/v1/instance/restore`: accepts the same document. `mode:"fail"` permits an empty bootstrapped destination; `mode:"replace"` explicitly replaces an established instance. Repeated artifact restoration is idempotent unless `force:true` is supplied.

This mode includes users, projects, memberships, roles/permissions/bindings, invitations, OIDC configuration/identities, settings, catalogs and overrides, provider credentials/configuration, models/prices/policies, API keys, prompts, groups, webhooks, storage metadata, retention and schedules. `include_history:true` also retains request, execution, usage, cost, conversation and probe histories. Spend counters, quota state and response-settlement deduplication fingerprints remain when history is omitted. Interrupted attempts are conservatively settled before becoming visible in the restored instance.

A fresh destination only needs its temporary bootstrap owner to authorize the operation; no original user/project IDs need to be pre-created. After restoration, authenticate using identities from the archive. Browser sessions, pending OIDC handshakes, transient delivery/job claims and recursive prior backup artifacts are not replayed. Existing external object-store/media files are not bundled. Storage metadata/credentials and schedule definitions are preserved.

Default disaster-recovery archives require the original `master.key` or `PANGOLIN_MASTER_KEY` in the destination. Portable archives use Argon2id (64 MiB, 3 iterations, a random 32-byte salt) and XChaCha20-Poly1305. A separately encrypted source-key envelope remains inside the authenticated archive; all restored secret envelopes are re-encrypted to the destination's existing master key. Neither keys nor plaintext credentials appear in public manifests. Wrong keys/passphrases, changed checksums, schema differences, invalid foreign keys, invalid role scopes or absence of an active owner fail preflight without changing the live database.

The on-disk SQLite snapshot is limited to 64 MiB (128 MiB HTTP artifact limit) and must match the running schema. SQLite's online-backup API performs the final replacement atomically, including the restore audit and receipt. Live requests/worker I/O must be idle for this step; the maintenance guard remains held even if the restoring client disconnects. Derived routing/session caches reset, old WebSocket connections require reconnect, and old DuckDB events are cleared with generation fencing. A durable reset marker prevents stale derived data resurfacing after a crash; DuckDB reset failure leaves analytics degraded while authoritative service remains available.

GC expires session history and configured request/payload/probe/quota records. The newest provider/credential quota snapshot is retained for enforcement. SQLite uses incremental auto-vacuum on fresh databases. Pre-release schema compatibility is not promised; rebuild experimental databases when moving to this schema.

Hourly GC applies project request/payload retention to DuckDB too. Payload-only expiry clears both bodies and the capture flag while retaining metadata; request expiry removes the derived row. Queued/late events obey the active rules, and startup/instance restore refreshes them. Derived retention failure disables analytics reads without blocking authoritative settlement. Legacy derived rows without a project identifier expire conservatively under applicable retention cutoffs.

The implementation is contract-tested locally. S3 service behavior, Redis deployment limits, quota endpoints and provider-native compaction need deployment-specific validation with real services. Quota collectors support API-key families and reject structured cloud credentials until a dedicated quota adapter exists; they never send a cloud credential document as a bearer token. Thinking rectifiers, automatic Anthropic cache-marker injection, client desktop integration and header-casing preservation remain provider/client adaptation work rather than implicit operational behavior.
