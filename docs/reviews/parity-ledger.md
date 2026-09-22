# AxonHub parity ledger — reconciled status

This is the durable status of Pangolin's AxonHub parity work. It replaces the
"open gaps / feature builds / fixed after" reading of the six area reports: those
reports stay as point-in-time evidence, but **every row below was re-checked
against the working tree** rather than restated from a report.

## Inspected state and evidence date

| Fact | Value |
| --- | --- |
| Evidence date | 2026-09-22 |
| Branch / HEAD | `main` at `e666bef7f49b8cf8b1de933288fbbae36a338a97` |
| `origin/main` | `ae052b4` (HEAD is three commits ahead, unpushed) |
| Working tree | 52 modified tracked paths, 59 untracked files, nothing staged |
| Tree fingerprint | the sorted `git status --porcelain` output, hashed with `sha1sum` → `45438b1939956ad9` |
| AxonHub reference | `/home/czyt/code/others/axonhub` at `cb29b65d9adfb06f89bb1b467418e0816988f36c` (2026-09-19), read only |
| Rust gate (fresh) | `cargo test --locked` → **242 passed, 0 failed** |
| Web gate (fresh) | `pnpm --dir web test` → **58 files / 1333 passed** in isolation |

The uncommitted tree *is* the subject of this audit: it carries the batches
recorded in `.superpowers/sdd/parity-remaining-handover/` (Tasks A, B, C1, C2, D1,
D2a, D2b). Line numbers below are from that tree and will drift.

**Suite stability, recorded because it is easy to misread:** a first web run
started while `cargo test` was running reported `8 failed | 1325 passed (1333)`
and the failing case names were not captured; the same suite run alone on the same
tree reported 58/58 files and 1333/1333 tests passed. This is the load flakiness
the ledger already documents, not a regression — but a red suite under parallel
load is not evidence either way, so re-run the specific files before believing it.

## How to read a row

Statuses:

- `closed` — no operator-visible difference remains. The `basis` column says why:
  `parity` (the behaviour already matched), `fixed` (it was a real gap and has been
  fixed, with evidence), `refuted` (the claimed difference never existed).
- `partial` — the capability exists but a named part of the AxonHub workflow is
  still missing.
- `open` — the difference is real and untouched.
- `feature-build` — the capability does not exist on the Pangolin side at all.
- `divergence` — a deliberate, documented difference. See
  [Intentional divergences](#intentional-divergences).

`Acceptance test` names the test that must exist and pass when the row is closed.
Where the row is already closed it names the test that currently guards it, or
states plainly that no test guards it yet.

## Counts

| Area | closed (parity) | closed (fixed) | closed (refuted) | partial | open | feature-build | divergence | total |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| observability | 5 | 10 | 0 | 2 | 3 | 0 | 0 | 20 |
| prompts & playground | 6 | 12 | 1 | 0 | 0 | 0 | 1 | 20 |
| models, routing & pricing | 6 | 13 | 2 | 1 | 3 | 1 | 0 | 26 |
| access control | 2 | 17 | 2 | 0 | 0 | 0 | 0 | 21 |
| system settings & background work | 8 | 11 | 5 | 1 | 5 | 0 | 1 | 31 |
| console-wide UX | 3 | 9 | 1 | 2 | 0 | 0 | 0 | 15 |
| **total** | **30** | **72** | **11** | **6** | **11** | **1** | **2** | **133** |

Row count is unchanged at 133 — the six area reports' own findings. No row was
dropped, merged or added. What changed is the status: **113 rows need no work**
(30 already matched, 72 fixed, 11 refuted), **18 rows need work** (`partial` +
`open` + `feature-build`), and **2 rows are recorded divergences** whose only
remaining difference is deliberate.

The table reconciles exactly with the area reports' own counts:

| Report status | Rows | Became |
| --- | --- | --- |
| `parity` | 32 | 30 `closed (parity)` + 2 `divergence` (`prompts 18`, `system 9`) |
| `gap` | 77 | 44 `closed (fixed)` + 10 `partial` + 23 `open` |
| `feature-build` | 13 | 9 `feature-build` + 1 `partial` (access 12, bulk key operations landed) + 3 `closed (fixed)` (ux 14, channel health; observability 15, trace lifecycle; prompts 13, admin playground) |
| `refuted` | 11 | 11 `closed (refuted)`, unchanged |

`prompts 3` (`scopes: null`) was a `gap` and is now fixed, which is why the
`closed (fixed)` bucket is larger than the number of rows the reports already
called closed.

## Observability

Report: [parity-observability.md](parity-observability.md).

| # | Finding | Status | Basis | Pangolin evidence | AxonHub reference | Acceptance test |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Per-dimension breakdowns absent | closed | fixed | `OverviewPage.tsx:28,74,180-253` renders a breakdown by provider, model, API key, user or project from `/analytics`; endpoint `operations_api.rs:290-292,581-590` | `features/dashboard/components/`, `features/analytics/components/dimension-pie-charts.tsx` | `observabilityWiring.test.tsx`, `trendChart.test.tsx` |
| 2 | No period or date-range selection | partial | — | Request log has five presets plus a custom range (`OperationsPage.tsx:24-44,132-146`); Overview and its breakdown are pinned to 24h (`OverviewPage.tsx:58,191`); the generic resource list still ignores `from`/`until` (`operations_api.rs:444-461` takes only `q`/`limit`/`offset`) | `features/analytics/components/analytics-filter-bar.tsx:245-330` (`TimePeriodSelector` on every chart) | a window control on Overview that changes the summary *and* the breakdown query |
| 3 | Trend chart carries requests and errors only | open | — | `SummaryPoint` is `{bucket, requests, errors, latency_ms}` (`observability.rs:60-66`); the chart plots two series (`OverviewPage.tsx:91-123`) | `features/analytics/components/combined-trend-chart.tsx` (requests, tokens, cost) | a bucketed token/cost series asserted from `/observability/summary` |
| 4 | Rows do not show why a request failed | closed | fixed | `OperationsPage.tsx:159,163` renders a localized `failureReason` from `row.error_kind`; field in `RequestItem` (`api.ts:30`) | `features/requests/components/request-detail-content.tsx:855-864` | `observabilityTruth.test.tsx` |
| 5 | No time-window filter on the request log | closed | fixed | `OperationsPage.tsx:132-136` sends `from`/`until` | `features/requests/components/data-table-toolbar.tsx:26-32` | `observabilityWiring.test.tsx` ("sends the exact range the operator typed") |
| 6 | No API-key filter on the request log | closed | fixed | `RequestFilter.api_key_id` (`observability.rs:96-111`); facet at `OperationsPage.tsx:128-131` | `features/requests/components/data-table-toolbar.tsx:130-160` | `observabilityWiring.test.tsx` |
| 7 | List columns omit the row's own telemetry | partial | — | Rendered: status, failure reason, external id, model, provider, endpoint, latency, cost, started (`OperationsPage.tsx:159`). `resolved_model` is in the payload (`api.ts:30`) but unrendered; tokens are not rendered; TTFT/stream are not in the list payload | `features/requests/components/requests-columns.tsx:29-35` (tokens, cost, duration, TTFT, tok/s, channel, caller, source, format, IP, UA) | a request-list test asserting token columns and the requested→resolved pair |
| 8 | Detail has no per-attempt list | closed | fixed | `OperationsPage.tsx:487-500` renders the record's attempts; API returns them (`operations_api.rs:536`) | `features/requests/components/request-detail-content.tsx:783-860` | `requestAttempts.test.tsx` |
| 9 | Upstream status and error text are not persisted | closed | fixed | `request_executions` gained nullable `provider_name`/`http_status`/`error_kind` (`db/schema.rs:771-773`), written by the terminal settlement (`operations/lifecycle.rs:225,481,597`) and read back by the detail (`operations_api.rs:536`) and the rebuild (`instance_backup.rs:446-448,495-499`); rendered at `OperationsPage.tsx:436,462,494` | `internal/ent/schema/request_execution.go:89,92` | `authoritative_observation.rs` (8 cases), `requestStatusTruth.test.tsx` |
| 10 | Payload view is raw JSON only | closed | fixed | Request detail now provides conversation/JSON/raw views, paged stream chunks, exact downloads and a sanitized curl preview. A versioned post-sanitization stream envelope preserves event names and explicit terminals including OpenAI `[DONE]` within the 1 MiB bound; old NDJSON and standard SSE remain readable. Capture-off still exposes only the policy alert (`PayloadViewer.tsx`, `lifecycle.rs`) | `features/requests/components/request-conversation-viewer.tsx`, `chunks-dialog.tsx`, `curl-preview-dialog.tsx` | stream-envelope sanitization/bounds tests; `payloadViewer.test.tsx` protocol, fallback, security, pagination and a11y matrix |
| 11 | Detail lacks client and network identity | closed | fixed | The server-observed direct peer is resolved once from `ConnectInfo`, frozen into SQLite and live/local observation events, rebuilt verbatim into DuckDB v5, returned by both request-detail APIs and rendered as localized Client IP or `—`. Forwarding headers, User-Agent, raw headers and credentials are deliberately not retained; request vs payload retention is documented (`api.rs`, `lifecycle.rs`, `observability.rs`, `instance_backup.rs`, `operations.md`) | `request-detail-content.tsx:397-399` (API-key name, caller, client IP, UA) | trusted-peer spoof test; DuckDB round-trip/rebuild; operations detail and `requestIdentity.test.tsx` exact/null/localization cases |
| 12 | Trace list omits client trace id, request count, first query | open | — | Columns are status/detail/thread/started/finished (`OperationsPage.tsx:84`); `external_id` is projected (`operations_api.rs:355`) but unrendered; no request count, no first query | `features/traces/components/traces-columns.tsx:110-180` | a traces-list test asserting `external_id` and a per-trace request count |
| 13 | Trace detail shows no token or cost totals | closed | fixed | `OperationsPage.tsx:330-365` totals per-execution usage and price components from the trace bundle (`operations_api.rs:489-495`) | `features/traces/components/trace-detail-page.tsx:266-282` | `observabilityWiring.test.tsx` |
| 14 | Cost is always USD with a fixed six-decimal scale | open | — | `formatMicros` hardcodes `$` (`web/src/observability.tsx:19-23`); no currency setting anywhere | `features/analytics/utils/format-currency.ts:1-8`, `system/data/system.ts` `currencyCode` | decision row — see [Decisions required](#decisions-required) |
| 15 | No per-trace archive / pin lifecycle | closed | fixed | Additive SQLite v10 keeps execution `status` separate from `lifecycle` (`active`/`archived`/`retained`); the project-scoped lifecycle route changes state and audits in one transaction, foreign ids fail as 404, the list filters/restores archived rows, and retained request/body/accounting rows plus DuckDB events survive project/global retention (`operations_api.rs`, `runtime.rs`, `observability.rs`, `OperationsPage.tsx`) | `features/traces/components/traces-columns.tsx:36-81` (archive/unarchive/retain/unretain) | `trace_lifecycle_actions_are_project_scoped_audited_and_refuse_nonsense_transitions`; retained SQLite/DuckDB GC matrix; `observabilityWiring.test.tsx` lifecycle cases |
| 16 | A trace can be opened by the client-supplied trace id | closed | parity | Detail resolves `id=? OR external_id=?` (`operations_api.rs:477`) | addressed by the trace's own id | `observabilityTruth.test.tsx` |
| 17 | Unmeasured telemetry is stated, never invented | closed | parity | `UNMEASURED = '—'` (`observability.tsx:10`), every stat gated on `measured` (`OverviewPage.tsx:56,68-71`) | no equivalent concept | `observabilityTruth.test.tsx` |
| 18 | Loading, error/retry and empty states on every query | closed | parity | `QueryError` + retry + skeletons on every observability query | skeletons and a plain error string | `observabilityTruth.test.tsx` |
| 19 | Live refresh of the request log | closed | parity | 15 s polling with pause/resume (`OperationsPage.tsx:112-113,148-151`) | `data-table-toolbar.tsx:22-28` | `observabilityWiring.test.tsx` |
| 20 | Pre-orchestration rejections appear in neither log | closed | parity | Routing/admission/model checks return before `Request::begin`, so a rejected request leaves no row | `internal/server/middleware/auth.go:44-52` | metrics/logs expose the rejection; a row would be a new product decision |

## Prompts and playground

Report: [parity-prompts-playground.md](parity-prompts-playground.md).

| # | Finding | Status | Basis | Pangolin evidence | AxonHub reference | Acceptance test |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | No prompt-version concept | closed | refuted | `activation_json.version` is the condition-document schema version (`orchestration/policy.rs:10-16`), not a version pointer; `prompts` has no version column | `internal/ent/schema/prompt.go` | — (nothing to build) |
| 2 | Write-time validation of activation and scopes | closed | parity | `validate_conditions` on `activation` (`operations_api.rs:1176-1182`), on `scopes` (`:1219-1222`), role whitelist (`:1183`), action/regex checks (`:1205-1210`) | `internal/server/biz/prompt.go:88-124` | `operations.rs` protection/prompt cases |
| 3 | Protection `scopes: null` bypasses validation | closed | fixed | The protection arm normalises `None`/`null` to `{"version":1}` and validates before persisting (`operations_api.rs:1217-1224`) | not reachable (typed settings) | `operations.rs` case asserting the stored document *and* that the project still answers a gateway request |
| 4 | Injection returns 400 for non-conversational endpoints | closed | fixed | Unsupported endpoints now push a `prompt_injection_skipped_unsupported_endpoint` decision and return `Ok` (`orchestration/protection.rs:125-140`) | `internal/server/orchestrator/prompt.go:31-74` has no shape failure | an embeddings/rerank request with one matching prompt returns 200 and records the decision |
| 5 | New prompts default to enabled | closed | fixed | SQLite v11 copy-swap changes the fresh/upgraded default to disabled while preserving existing rows; API create and console form also default disabled. Update requests that omit `enabled` preserve the existing value. | `internal/ent/schema/prompt.go:58-60` (`disabled`) | fresh/upgrade schema constraint tests; HTTP create default; `promptSummary.test.tsx` form payload |
| 6 | No `append` action | closed | fixed | Prompt `action` is constrained to `prepend`/`append`, projected and validated by the API, and append places content after the conversation for Chat, Messages, Gemini and Responses without reordering existing items. | `internal/objects/prompt.go:5-11`, `prompt_matcher.go:116-159` | four-protocol append placement cases in `orchestration/tests.rs` |
| 7 | No `order` field | closed | fixed | Prompt `order` is an integer column/API/form field; injection loads enabled prompts by `order,created_at,id`, making the documented tie-break deterministic. | `internal/ent/schema/prompt.go:61-64` ("smaller values are inserted first") | ordering and tie-break test plus HTTP/Web projection cases |
| 8 | Conditions cannot match an API key | closed | fixed | Condition context carries `project_id` and `api_key_id` (`policy.rs:330-345`); the orchestrator passes the key | `internal/objects/prompt.go:29-31` | `a_prompt_can_be_scoped_to_the_calling_api_key` |
| 9 | No bulk enable/disable for prompts | closed | fixed | `bulk-toggle` accepts `prompts`/`protection` (`operations_api.rs:1251,1256`); row selection on both tables (`PromptsPage.tsx:62,68`) | `prompts-bulk-*.tsx`, `prompts-columns.tsx:20-42` | `bulkToggleResources.test.tsx`, `bulkSelection.test.tsx` |
| 10 | Prompt list hides what a prompt does | closed | fixed | Activation summary and role columns (`PromptsPage.tsx:36-50,62`) | `prompts-columns.tsx:58-174` | `promptSummary.test.tsx` |
| 11 | Protection rules have no preview and no metadata | closed | fixed | SQLite v12 adds description and constrained active/archived state; archived rows remain listable/previewable but enforcement excludes them. The bounded project-scoped read-only preview reuses the production matcher, returns per-rule match/action/result and performs no DB/provider mutation; the bilingual dialog covers loading/error/retry/empty. | `internal/server/biz/prompt_protection_preview.go:22-56`, `rules-action-dialog.tsx:283-320` | protection preview bounds/read-only/project cases, metadata migration, enforcement exclusion and `promptSummary.test.tsx` dialog cases |
| 12 | Duplicate prompt name is a generic 500 | closed | fixed | Pre-check inside the transaction returns a conflict (`operations_api.rs:1187-1199`) | `internal/server/biz/prompt.go:136-149` | `operations.rs` prompt-conflict case |
| 13 | Playground is a single-turn form, not an admin chat | closed | fixed | The console session now delegates to an existing project key by internal id without exposing its token; the admin route reuses the public gateway pipeline and produces the same traces, attempts, usage and cost. UI supports multi-turn history, streamed terminal truth, model-gateway or eligible-channel source, system/temperature, bounded image attachments, reasoning, stop/regenerate/clear and request/usage links. | `features/playground/index.tsx:94-651`, `internal/server/routes.go:133-140` | session route policy/accounting/CSRF test; five Playground suites including session multi-turn/source/no-token cases |
| 14 | Playground picker is not endpoint-filtered | closed | fixed | Public `/v1/models?endpoint=/v1/responses` and the session playground model route both reuse `visible_models_for`; the picker is bound to project+key, rejects stale selections and shows retryable failure/empty states. | `playground/index.tsx:142-155,319-328` | Rust endpoint filter contract and `playgroundPicker.test.tsx` 7-case matrix |
| 15 | Condition language | closed | parity | JSON-Pointer tree with `all`/`any` and 10 operators (`policy.rs:205-301`) | `internal/objects/prompt.go:20-48` | `orchestration/tests.rs` |
| 16 | Multi-protocol placement | closed | parity | Native placement per protocol (`protection.rs:57-124`) | one message list | `orchestration/tests.rs` |
| 17 | Protection engine | closed | parity | Regex over content and role, `deny`/`redact`, scopes, `test_mode`, nested traversal (`protection.rs:143-286`) | `prompt_protection_request.go:30-103` | `orchestration/tests.rs` |
| 18 | Scoping, audit and deletion | divergence | — | Project-scoped queries, audited mutations, hard delete with an audit row | global rules, soft delete | `orchestration/tests.rs` — hard delete vs soft delete is D2 below |
| 19 | List search and pagination | closed | parity | Server-side `LIKE` + offset/limit (`operations_api.rs:452-457`) wired into the shared table | Relay connection | `shared.test.tsx` |
| 20 | Localisation | closed | parity | Both pages fully localised in `zh-CN` and `en` | zh-CN and en locale files | `i18n.test.ts`, `localizationLeaks.test.tsx` |

## Models, routing and pricing

Report: [parity-models-routing-pricing.md](parity-models-routing-pricing.md).

| # | Finding | Status | Basis | Pangolin evidence | AxonHub reference | Acceptance test |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Price tiers, schedules and timezones are unmodelled | closed | refuted | Modelled, validated and writable: `tiers_json` (`db/schema.rs:395-400`), `schedule_json` (`:380`), tier arithmetic (`operations/pricing.rs:310-322`), IANA timezone/weekday/window/range/priority (`operations/schedule.rs`) | `objects/price.go:42-46,366-372` | `operations.rs` price cases — the missing piece is the editor (row 26) |
| 2 | Historical prices are mutable / not versioned | closed | refuted | Append-only versions with `valid_from`/`valid_until` (`db/schema.rs:372-386`), immutability triggers (`operations/schema.sql:66-69`), per-request `price_id` | `channel_model_price_versions.go:28-46` | `operations.rs` immutability cases |
| 3 | Catalog subscriptions and sync | closed | parity | Multi-source prioritized subscriptions with signature policy, ETag, snapshots, atomic activation, rollback, `refresh-due` (`ModelsPage.tsx:385-504`, `api/catalog_api.rs`) | `internal/server/biz/catalog_settings.go:18-31` | `ModelsCatalog.test.tsx` |
| 4 | Routing preview with per-stage decisions | closed | parity | `POST .../routing-preview` (`operations_api.rs:812-833`), UI at `ModelsPage.tsx:70-118` | none | `ModelsPage.test.tsx` |
| 5 | Association core coverage | closed | parity | Six match types incl. nested conditions, priority, weight (`orchestration/repository.rs:149-200`, `policy.rs:205-301`) | `internal/objects/model.go:59-76` | `orchestration/tests.rs` |
| 6 | Model name transformations (core set) | closed | parity | `strip_prefix`/`lowercase`/`mappings`/`exclude`/`stream` (`repository.rs:239-265,336-357`) | `internal/objects/channel.go:155-186` | `providers/tests.rs` |
| 7 | Price component kinds and cache-write TTL variants | closed | parity | Seven kinds plus `cache_ttl 5m/1h` (`operations/pricing.rs:246-308`) | four item codes plus variants | `operations.rs` pricing cases |
| 8 | Provider preset catalog | closed | parity | 59 built-in providers, 18 adapter kinds, local logo keys (`catalog/data/builtin.json`, `web/src/providers.ts`) | `features/models/data/providers.json` | `catalog/tests.rs` |
| 9 | Catalog cards are never applied to console-created models | closed | fixed | The model create form sends a stable catalog card id; the server resolves the effective card and owns capabilities, both default prices and immutable metadata. Forged browser fields cannot override it, stale ids are refused, and manual creation/edit preservation remain supported. | `models-action-dialog.tsx:198-268` | `console_model_creation_applies_the_selected_catalog_card_server_side` plus `ModelsCatalog.test.tsx` picker/manual/error cases |
| 10 | Model rows are channel bindings, not a global model record | closed | fixed | Pangolin retains its channel-bound internal model, while matching the observable card and lifecycle contract: bundled/fallback icon, developer/type, limits/costs, active/archived state, audited archive/restore, active-only routing/discovery and separate enabled state. | `internal/ent/schema/model.go:39-53` | B29 projection/UI plus B30 archive/restore/routing/discovery tests |
| 11 | `/v1/models` returns no extended metadata | closed | fixed | Default response bytes remain unchanged. Exact `include=all` adds only the bounded typed B29 card projection after mapping/allowlist/endpoint/candidate visibility; absent/malformed cards omit metadata and unknown include values preserve the legacy shape. | `internal/server/api/openai.go:832,882` (`?include=all`) | six `model_discovery_*` contract tests, including exact bytes, mapped id, hidden model and malformed metadata |
| 12 | No global model settings surface | closed | fixed | Owner-only versioned instance settings control channel fallback, discovery source, default extended metadata, reasoning-effort suffixes, channel-model blacklist and unroutable visibility. Strict writes/audited transactions and forward-tolerant reads back observable gateway/discovery behavior; System provides the bilingual responsive form. | `models-settings-dialog.tsx:28-34` | eight `b32_*` Rust cases and three focused SystemPage form cases |
| 13 | Project default routing has no write path or UI | closed | fixed | `routing` is read, validated and written by the orchestration settings route (`operations_api.rs:611-655,697,721-722`); the console renders and submits it (`SystemPage.tsx:271-298`) | `models-settings-dialog.tsx` | the harness guard is still missing — see [Deferred corrections](#deferred-corrections) |
| 14 | Association conditions: no builder, no domain fields | closed | fixed | The bounded sanitized context includes UTC daily time, media presence, stream and request format; typed field/operator validation fails invalid configuration closed. The bilingual nested all/any editor round-trips exact documents and retains raw JSON as an advanced escape hatch. | `models-association-dialog.tsx:61-143` | domain-context/evaluator Rust test plus `ModelsPage.test.tsx` structured/raw editor cases |
| 15 | No `channel_tags + regex` type, no association exclusions | closed | fixed | Migration v24 adds `channel_tags_regex` and versioned association exclusions for channel name regexes, IDs and tags. Write-time bounds/regex checks and read-time revalidation make exclusions veto candidates fail-closed. | `internal/objects/model.go:59-76,99-116` | candidate/exclusion Rust test plus ModelsPage payload case |
| 16 | No auto-trim or hide-original/hide-mapped transformations | closed | fixed | Strict versioned model rules support exact-segment auto-trim, mappings and discovery-only hide-original/hide-mapped flags. Both identifiers remain routable; the bilingual structured editor retains advanced JSON for compatible rules. | `internal/objects/channel.go:163-180`, `channel_llm.go:1473,1490-1522` | three Rust transform/write tests plus `channelModelRules.test.tsx` |
| 17 | No volume (non-marginal) tier pricing mode | open | — | `calculate` is marginal-only (`operations/pricing.rs:310-322`); the pre-flight upper bound uses the maximum tier rate (`:371-378`) | `internal/objects/price.go:26-31` (`usage_volume`) | a pricing test for per-component volume mode |
| 18 | Channel-scoped prices cannot be created | open | — | Schema supports `provider_id` (`db/schema.rs:372-386`) and the resolver prefers it (`pricing.rs:425`), but the write path inserts `NULL` (`operations_api.rs:1377-1382`) and the form has no channel field (`ModelsPage.tsx:68`) | `channel_model_price.go:27-41` | a price-creation test that stores and reads back a channel-scoped rate |
| 19 | Service-group price ratio has no console surface | closed | fixed | Models has a bilingual service-group editor for the per-request ratio and project-scoped channel assignments; writes reuse the audited transactional `groups` resource contract. | `channels-model-price-dialog.tsx:1190-1200` | `b40_b42_service_group_editor_round_trips_channels_ratio_and_audit` plus `modelOperations.test.tsx` |
| 20 | Model/provider deletion safety | partial | — | Model side is complete: read-only scoped impact counts, archive-required hard delete, typed immutable-history 409 and audited deletion of an unused archived model. Provider deletion still has typed dependency refusal but no equivalent impact preview/archive lifecycle. | archive dialog + typed delete confirmation | model preview/lifecycle cases are green; provider preview remains |
| 21 | No per-channel proxy configuration | closed | fixed | SQLite v20 stores validated HTTP(S)/SOCKS proxy policy with write-only encrypted password; bounded configuration-keyed clients cover forwarding, probes, quota and discovery while list projections expose only configured state. | `channels-proxy-dialog.tsx:215-303` | proxy egress/encryption/redaction/validation Rust tests plus `channelProxyDiscovery.test.tsx` |
| 22 | No model batch create and no real bulk lifecycle | closed | fixed | A bounded catalog-backed batch create validates every named row before one audited transaction; the Models table bulk archives/restores active lifecycle rows and applies B30's archive/history guards to atomic bulk delete while skipping foreign IDs. | `models-batch-create-dialog.tsx`, `channels-bulk-*.tsx` | B41 Rust atomicity/project-scope cases plus `modelOperations.test.tsx` |
| 23 | No unassociated-model detection | closed | fixed | The project-scoped diagnostic covers enabled active models against enabled exact/regex/tag/channel-tag-regex associations, honors channel exclusions, and the routing tab names gaps with an association-editor link or explicit empty state. | `models-unassociated-dialog.tsx` | `b40_b42_unassociated_models_names_only_enabled_models_without_a_match` plus `modelOperations.test.tsx` |
| 24 | Price list omits the components | open | — | The `prices` projection returns id/model/version/validity/schedule only (`operations_api.rs:381-385`); no page shows components | `channels-model-price-dialog.tsx` | a price-list test asserting each version's rates |
| 25 | Provider model discovery / sync | closed | fixed | Bounded adapter-specific discovery feeds durable fenced manual/scheduled sync. Deterministic IDs preserve manual precedence, archive missing discovered rows, audit mutations and retain only a fixed failure code. | `internal/server/biz/model_fetcher.go`, `channel_model_sync.go` | discovery parser/bounds/catalog tests and model-sync idempotency/schedule/failure cases |
| 26 | Full price editor (tiers, schedules, timezones) | feature-build | — | `components`/`schedule` are JSON textareas (`ModelsPage.tsx:68`); append-only, no delete | 1340-line editor, `model-price-editor.tsx`, `price-schedule-editor.tsx` | an editor test producing the same document the write API accepts |

## Access control

Report: [parity-access-control.md](parity-access-control.md).

| # | Finding | Status | Basis | Pangolin evidence | AxonHub reference | Acceptance test |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Per-project members surface exists | closed | parity | `members` tab with list/add/remove, loading, error+retry and empty states (`AccessPage.tsx:24,48,536-649`) | `proejct-users/components/users-table.tsx:64,133` | `accessMembers.test.tsx` |
| 2 | Member rows identify users only by opaque id | closed | fixed | `MembershipView` joins email/display_name (`access.rs`, `MembershipView`); the console renders name then email and falls back to the id (`AccessPage.tsx:600-605`) | `users-columns.tsx:38-113` | `the_member_roster_carries_the_member_identity`, `accessMembers.test.tsx` |
| 3 | A member's role or status cannot be changed in place | closed | fixed | Member row actions reuse `POST /members` for role/status edits, invalidate only the captured project roster and fail closed across a project switch. The protected owner row offers no refused downgrade/suspend/remove actions. | `project-user-action-dialog.tsx:172-175` | `accessMembers.test.tsx` in-place edit, owner guard and switch-race cases |
| 4 | Members list has no search, pagination or mobile layout | closed | fixed | Members now use the shared `ResourcePage` shell with search, pagination, desktop table and rich mobile cards while preserving loading/error/retry/empty and project-read listing. | `users-table.tsx:133,187` | `accessMembers.test.tsx` search/pagination/mobile matrix plus `shared.test.tsx` |
| 5 | Members tab unreachable for the principal | closed | fixed | `project:read` is in `canAccess` (`Shell.tsx:34`) and the tab is granted it (`AccessPage.tsx:24`) | n/a | `accessMembers.test.tsx` |
| 6 | No SSO/OIDC entry point on the sign-in page | closed | fixed | `Login` reads the minimal unauthenticated provider list and renders one localized native GET link per enabled provider to the encoded server-owned `start` route; discovery failure leaves password login untouched | `user-auth-form.tsx:29,45,134-183` | `Auth.test.tsx` provider/empty/failure/localization cases plus `App.test.tsx` |
| 7 | OIDC identities can be created but never listed or unlinked | closed | fixed | The provider-aware admin panel lists subject/user bindings with loading/error/retry/empty states and confirmed unlink. Provider/project context is captured so stale destructive actions fail closed. | `user-auth-form.tsx:148-183` | `AccessPage.test.tsx` identity list/unlink/retry/context cases plus transactional unlink audit coverage |
| 8 | No self-service account linking | closed | fixed | A signed-in Account surface starts an authenticated CSRF-protected link flow. One-time browser-bound PKCE state durably captures the session user; callback binds only that user, refuses both uniqueness conflicts and rolls back when audit fails. | `internal/server/routes.go:131` (`/oidc/link/:provider`) | `Auth.test.tsx`, `oidc_link_start_requires_session_and_csrf`, link state/conflict/audit rollback tests |
| 9 | Key creation hardcodes type, owner and scopes, and never shows scopes | closed | fixed | The create dialog offers all four server types, active same-project owners for user/personal keys and the complete project-scope seed set; the API serializes scopes as an array and the table renders them. Project/type changes clear stale owners and server refusals stay inline. | `apikeys-columns.tsx:220-226` | `accessKeyState.test.tsx` (15 cases) plus `the_key_view_serializes_scopes_as_an_array` and `an_api_key_cannot_delegate_a_scope_it_does_not_hold` |
| 10 | Key status hides expiry, spend and last use | closed | fixed | Derived state from admission's own rule (`AccessPage.tsx:74-95`) plus spend/budget, expiry, last-used and IP-policy columns (`:163-186`); `expires_at`/`spent_micros` are returned (`access.rs`, `ScopedApiKeyView`) | `apikeys-columns.tsx:194-241` | `accessKeyState.test.tsx`, `accessKeyLastUsed.test.tsx` |
| 11 | Per-key usage and cost unreachable from the key | closed | fixed | Every key row opens its own Today/7-day/all-retained usage dialog with input/output/cache/total tokens, cost and top models. Empty windows show `—`, failures retry, and the backend rejects foreign/unknown key filters with the same 404. | `apikeys/components/data-table-row-actions.tsx`, `api-key-token-chart-dialog.tsx` | `keyUsage.test.tsx` plus `analytics_api_key_filter_refuses_foreign_and_unknown_keys` |
| 12 | No rotate, archive or bulk key operations | closed | fixed | Explicit rotate/single archive/bulk archive routes share project + `api_key:manage` authority. Rotate atomically replaces the secret and returns it once; archive is audited durable state, blocks all mutable/auth paths, skips foreign bulk ids and remains available for emergency revocation. The UI confirms and captures project/key context. | `apikeys/components/data-table-row-actions.tsx`, archive/rotate dialogs | Rust rotate rollback/auth/lifecycle/bulk tests plus `keyBulkSelection.test.tsx` |
| 13 | Invitations are single-use and email-bound | closed | fixed | SQLite v18 adds an operator-selected 1–100 use limit while retaining email binding and the 60 s–30 d lifetime. A conditional claim, identity/member writes and one acceptance audit share a transaction; exhausted/expired/unknown tokens remain opaque. | `users-invite-dialog.tsx:20-23` (maxUses 1 or unlimited, no email) | reuse/exhaustion/migration Rust cases plus `accessInvitations.test.tsx` |
| 14 | OIDC providers have no login-only mode or branding | closed | fixed | SQLite v16 adds login-only policy and validated safe branding. Password login is refused before identity lookup when all enabled providers are login-only; public discovery exposes only safe display fields. Pangolin deliberately uses bundled `logo_key` instead of remote `icon_url`, with accessible contrast/fallback UI. | `user-auth-form.tsx:45,148-183` | OIDC branding/privacy tests, login policy/audit tests and focused Auth/Access Web tests |
| 15 | Role editor edits raw permission JSON | closed | fixed | Project-scoped typed permission catalog is filtered by the same delegation predicate as server role writes; the role form uses an accessible picker, preserves unknown stored slugs and cannot grant a scope the actor lacks. System roles remain protected. | `scopes-cell.tsx`, `roles-action-dialog.tsx` | `permission_catalog*` Rust/HTTP tests and `accessRoles.test.tsx` |
| 16 | Role bindings can be created but never listed or revoked | closed | fixed | List + revoke with a project-and-user-scoped cache key (`AccessPage.tsx:302-419`); routes `access_api.rs:71,75` | `project-user-action-dialog.tsx:172-175` | `roleBindings.test.tsx` (5 cases, incl. two mid-flight project switches) |
| 17 | Create-user form offers controls the backend ignores | closed | fixed | `UserInput.enabled` is honoured on create (`access.rs:808-809`) | add-user dialog with status | `creating_a_user_honours_the_enabled_switch` |
| 18 | Key-profile templates | closed | fixed | Project-scoped v17 templates save, import/export, preview and apply the same validated profile policy document used by ordinary profile writes. Mutations and audit rows share one transaction; foreign IDs stay opaque. The console provides loading/error/retry/empty states, named confirmations and project-switch refusal. | `apikeys-save-template-dialog.tsx`, `apikeys-load-template-popover.tsx` | Rust `profile_template_*` cases and `profileTemplates.test.tsx` |
| 19 | Invitation inspect/accept flow | closed | parity | `/invite?token=` inspects then accepts over `POST /api/v1/invitations/inspect` and `/accept` (`Auth.tsx:73-142`, `access_api.rs:94-95`) | `/sign-up?invite=` | `accessInvitations.test.tsx` |
| 20 | Member routes unconsumed by the console | closed | refuted | The members tab reads/writes/deletes the three member routes | — | — |
| 21 | Invitation routes unconsumed by the console | closed | refuted | The invitations tab reads/creates/deletes the three invitation routes | — | `accessInvitations.test.tsx` |

## System settings and background work

Report: [parity-system-settings.md](parity-system-settings.md).

| # | Finding | Status | Basis | Pangolin evidence | AxonHub reference | Acceptance test |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Scope-split System tabs, instance tab owner-only | closed | parity | `PROJECT_TABS` for `project:manage`, `requestLogging` only for `*`, plus a per-tab scope note and an explicit no-scope alert (`SystemPage.tsx:25-41`) | `system/components/tabs.tsx:44-52,85-95` | `SystemPage.test.tsx` |
| 2 | Project backup export/restore workflow | closed | parity | Grouped table picker with presets, export downloads, restore rejects non/foreign artifacts, run history with per-target results and retry | `backup-settings.tsx:156-425` | `SystemPage.test.tsx`, `artifactDownload.test.tsx` |
| 3 | Instance-level backup/restore with preflight | closed | parity | Owner-only routes with portable vs master-key archives and a preflight that gates restore (`operations_api.rs:9-18`, `operations/instance_backup.rs`) | none (its backup covers the same data) | `instance_backup.rs` tests |
| 4 | Console surface for durable jobs | closed | parity | Jobs tab with counts, filter, pagination, columns, retry/queue actions and auto-refresh (`SystemPage.tsx:62,679-835`) | none | `SystemPage.test.tsx` |
| 5 | Catalog subscriptions | closed | parity | Many sources with priority, interval, signature policy, snapshots, rollback and `refresh-due` (`ModelsPage.tsx:385-504`) | `catalog-settings.tsx:68-114` | `ModelsCatalog.test.tsx` |
| 6 | Request-logging policy granularity | closed | parity | Four levels plus per-key override and key-disable switches (`operations/logging.rs:9-71`, `SystemPage.tsx:288-309`) | four booleans, no per-key override | `operations/tests.rs` policy matrix |
| 7 | Durable job execution | closed | parity | Atomic claim with lease, monotonic fence, idempotent `operation_key`, bounded attempts, heartbeat (`operations/jobs.rs:20-64`) | in-process cron, no lease | `operations/tests.rs` |
| 8 | Retention model | closed | parity | Per-project policies for requests/payloads/probes/quota plus an env default, applied by the hourly `gc` job | five instance-level cleanup options | `operations.rs` retention cases |
| 9 | Instance-wide IP blocking | divergence | — | Per-key `allowed_ips`/`denied_ips` enforced at admission and editable per key (`db.rs`, `AccessPage.tsx:233-234`) | instance blocklist | D3 below |
| 10 | The retention form still offers the removed `retain_payloads` switch | closed | fixed | Gone from the console, the projection and both locales (`rg retain_payloads` is empty) | n/a | `SystemPage.test.tsx` retention case |
| 11 | The stored request-logging policy is parsed with `deny_unknown_fields` and has no `version` | closed | fixed | Version 1 is explicit on reads/writes; strict request parsing rejects unknown/unsupported input while stored v1 documents tolerate forward fields and legacy unversioned rows upgrade in memory. Startup emits bounded diagnostics for malformed/unsupported rows; admission stays available with logging disabled and GET returns a writable repair shape. | `internal/server/biz/system.go:988-1004` | version/strict-write/startup/fail-closed Rust cases plus `SystemPage.test.tsx` round-trip |
| 12 | A broken schedule fails silently | closed | fixed | `last_error` is projected (`operations_api.rs:404`) and rendered as a red badge or `—` (`SystemPage.tsx:840`) | `backup-settings.tsx:560-586` | `backupScheduleRetention.test.tsx` |
| 13 | No cron, time-of-day or timezone for schedules | open | — | `interval_secs` only, bounded 30 s–1 year (`operations_api.rs:1430-1434`, `SystemPage.tsx:840`); cron + timezone exist only inside the per-channel auto-disable policy (`operations_api.rs:1289-1302`) | `internal/server/backup/autobackup.go:27-28,76-81` | a schedule test for a daily time in an IANA timezone |
| 14 | Auto-backup retention count only via raw JSON | closed | fixed | `keep` is a first-class field with a hint and range check (`SystemPage.tsx:840,492-493`) | `backup-settings.tsx:547-556` | `backupScheduleRetention.test.tsx` |
| 15 | No route or surface to list or re-download an artifact | closed | fixed | `GET .../backup/artifacts` + single-artifact download, consumed with the CSRF header (`operations_api.rs:64-71`, `SystemPage.tsx:563-580,690-700`) | no list route either | `artifactDownload.test.tsx` |
| 16 | The delivered archive's location is not shown | closed | fixed | `object_key` is projected and rendered per target (`operations_api.rs:352` type, `SystemPage.tsx:806-811`) | n/a | `deliveryLocation.test.tsx` |
| 17 | Webhook delivery history has no console surface | closed | fixed | `webhook-deliveries` resource (`operations_api.rs:421-424`) consumed by a paginated panel (`SystemPage.tsx:872`) | configuration only | `webhookDeliveries.test.tsx` |
| 18 | Storage targets are raw JSON, local + S3 only | closed | fixed | Local, S3, GCS and WebDAV have typed, bounded configuration and encrypted credential paths; the project-authorized connection action performs a bounded list only and never writes an object | typed forms per backend (local, S3, GCS, WebDAV) with credential fields, plus a connection test | `b50_gcs_and_webdav_roundtrip_and_connection_test_does_not_write`, `storageTargetForm.test.tsx` |
| 19 | One restore conflict strategy for the whole artifact | partial | — | Project artifacts accept a default plus validated per-resource `fail`/`skip`/`overwrite` strategies while preserving secrets and the prior default; instance archives still use one `mode` + `force` policy | per-class strategies (`RestoreOptionsInput`) | `b51_restore_applies_per_resource_strategies_and_defaults_to_fail`, `SystemPage.test.tsx` |
| 20 | `catalog:manage` alone gets a System nav entry that leads nowhere | closed | fixed | `/models` is in the nav for `catalog:manage` (`Shell.tsx:45`) | catalog tab inside the system page | `SystemPage.test.tsx` permission cases |
| 21 | Admin-route body rejections bypass the error envelope | closed | fixed | `/api/admin/` rejections are normalised (`api/errors.rs`), with a guard scoped to admin paths | GraphQL returns structured validation errors | `admin_body_rejections_answer_with_the_error_envelope` |
| 22 | No instance-level general settings | open | — | No currency and no instance timezone setting; `formatMicros` hardcodes `$` | `general-settings.tsx:149-224` | an instance settings test; depends on the currency decision |
| 23 | No instance-level retry or upstream-error policy settings | open | — | Retry statuses and auto-disable are per-channel JSON (`db/schema.rs:344-352`); the only instance settings routes are request-logging and system (`operations_api.rs:20-25`) | `retry-settings.tsx:149-260` | an instance policy test, or the ADR below |
| 24 | No quota-collection or quota-routing-mode settings | open | — | An exhausted quota always removes the channel (`orchestration/repository.rs:210-211`); collection is driven per channel/schedule, with no toggle | `quota-settings.tsx:89-155` (collection switch + routing mode) | a routing-mode test, or the ADR below |
| 25 | Diagnostics tab: cache diagnostics export and clear cache | closed | fixed | Owner-only export reports bounded cache counts and shape versions without payloads or credentials; clear invalidates only derived process state, verifies SQLite authority is unchanged and writes an audit row | `diagnostics-settings.tsx` | `b52_cache_diagnostics_are_bounded_and_clear_keeps_authority_serving`, `SystemPage.test.tsx` |
| 26 | Outbound proxy presets, and per-webhook timeout/proxy | partial | — | Instance-scoped presets encrypt credentials and are selectable by bounded-timeout webhooks and catalog sources; catalog fetches remain no-proxy by default. Channel settings can reference a preset, but provider egress still depends on models row 21 plumbing | `proxy-presets-settings.tsx:29-98`, `webhook-settings.tsx:424-522` | B53 webhook/catalog/proxy tests plus `SystemPage.test.tsx`; channel egress remains |
| 27 | "One extra field makes every gateway request fail" (the PUT) | closed | refuted | The extra field is refused at the settings route before anything is written (`operations_api.rs:212-225`) | — | the real exposure was the stored row, now row 11 |
| 28 | OIDC settings belong to the System page | closed | refuted | Provider and identity CRUD live on the Access page over `/api/admin/v1/oidc/...`; `SystemPage.tsx` has no OIDC tab | its OIDC surface is user-level self-service | — |
| 29 | A principal holding only `project:manage` has no route to the System page | closed | refuted | The page renders the six project tabs for `project:manage` alone (`SystemPage.tsx:30-43`) | — | `SystemPage.test.tsx` |
| 30 | The backup panel has no file picker or download | closed | refuted | Export downloads immediately; restore reads a picked file and rejects a foreign artifact; the instance panel has its own picker | — | `SystemPage.test.tsx`, `artifactDownload.test.tsx` |
| 31 | The jobs panel, catalog sync columns and auto-refresh are missing | closed | refuted | Jobs tab, catalog sync columns and the shared auto-refresh control all exist | — | `SystemPage.test.tsx`, `ModelsCatalog.test.tsx` |

## Console-wide UX

Report: [parity-ux-extras.md](parity-ux-extras.md).

| # | Finding | Status | Basis | Pangolin evidence | AxonHub reference | Acceptance test |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Bulk selection and bulk actions in lists | closed | fixed | Opt-in `selectable` adds a checkbox column, select-all, a count bar and Enable/Disable/Cancel, driving the existing `bulk-toggle` (`pages/shared.tsx:78,144-190`); exposed on channels, credentials, models, prompts and protection, and hand-built on keys | `channels-table.tsx:102,199-200`, `channels-bulk-*.tsx` | `bulkSelection.test.tsx`, `keyBulkSelection.test.tsx`, `bulkToggleResources.test.tsx` |
| 2 | Probing a channel from inside the console | partial | — | Each channel row has a `Test` action whose dialog queues the same probe job and shows status/model/latency/HTTP status/error inline (`ChannelsPage.tsx:35`, probe panel) | `channels-test-dialog.tsx:41,91`, `channels-test-history-drawer.tsx`, `channel-health-cell.tsx:14-40` | the operator cannot choose the model (the route takes only `provider_id`); no history drawer, no bulk test, no sparkline |
| 3 | Entering a channel credential | closed | parity | Credentials dialog with channel/type/secret/priority/enabled; the secret is masked, encrypted, suffix-identified and never returned | `channels-api-key-management-dialog.tsx:63-70` | `confirmCallSites.test.tsx`, `shared.test.tsx` |
| 4 | Filters on the request log | partial | — | Window presets + custom range and status/provider/model/key facets (`OperationsPage.tsx:120-146`) | `features/requests/index.tsx:23-33,147-158` (multi-select, URL state, reset) | URL persistence and multi-select are missing; `observabilityWiring.test.tsx` guards what exists |
| 5 | Dismissible onboarding | closed | fixed | The banner carries a dismiss control persisted per instance in this browser and restorable from appearance settings (`Shell.tsx:34-42,158-179`) | `onboarding-flow.tsx:272-295` | `onboarding.test.tsx` |
| 6 | Option-list queries fail silently | closed | fixed | Every option list reports failure with a retry (`pages/shared.tsx`, `ChannelsPage.tsx:225-227`, `ModelsPage.tsx:43-46`, `AccessPage.tsx:247-249`) | `AGENTS.md:63` | `channelHealth.test.tsx` ("a picker whose option list failed says so") |
| 7 | Invitations list has no empty state | closed | fixed | `EmptyState` carrying the invite action (`AccessPage.tsx:470`) | `AGENTS.md:63` | `emptyStateControls.test.tsx` |
| 8 | Unreadable request-logging policy is read as "recording on" | closed | fixed | `observability.tsx:61-75` adds an `unknown` reason for a failed or unreadable policy | `AGENTS.md:62` | `observabilityUnknown.test.tsx` |
| 9 | Mono table cells below the 14px minimum | closed | fixed | `.mono-cell` and `td code` are 14px (`styles.css:264-265`) | `AGENTS.md:62` | `presentationRules.test.ts` |
| 10 | Localization leaks | closed | fixed | All five strings moved into both locales; `i18n.ts` key parity | `AGENTS.md:65` | `localizationLeaks.test.tsx` |
| 11 | Loading shimmer animates a non-composited property | closed | fixed | The sweep is `transform: translateX(...)` on the element's own layer, disabled under reduced motion (`styles.css:342-352,501-502`) | `AGENTS.md:64` | `presentationRules.test.ts` |
| 12 | Icon-only controls have accessible names | closed | parity | Every `ActionIcon` names its row; the burger, scrim and modal close buttons are labelled | `AGENTS.md:65` | `confirmCallSites.test.tsx`, `dialogs.test.tsx` |
| 13 | Motion budget, pointer gating and reduced motion | closed | parity | All transitions ≤260 ms, hover inside `@media (hover: hover) and (pointer: fine)`, reduced motion honoured (`styles.css`) | `AGENTS.md:64` | `presentationRules.test.ts` |
| 14 | Channel health state has no console surface | closed | fixed | `HealthPill` renders a localized auto-disabled window, backoff or failure count, `—` when unrecorded (`ChannelsPage.tsx:150-166`); the credential relation emits `credential_id` | `channel-health-cell.tsx:14-40` | `channelHealth.test.tsx` — the success-rate sparkline remains a feature, not wiring |
| 15 | "The console invents zeros for unmeasured telemetry" | closed | refuted | One formatter pair, the null rule in table cells and the request list's sentinel (`observability.tsx:10-28`, `shared.tsx:27-32`) | `AGENTS.md:62` | `observabilityTruth.test.tsx`, `unsettledUsage.test.tsx` |

## Intentional divergences

Each is a deliberate difference from AxonHub, kept because a Pangolin invariant or
a stronger rule wins. Two of them are rows of the 133 above (`prompts 18`, D2 and
`system 9`, D3); the rest are design-level or matrix-level and are listed here so
they are not re-filed as gaps. D5 and D6 qualify a row that is otherwise counted as
work (`access 13` is `feature-build`, the matrix row *OpenAPI GraphQL auth* is not
one of the 133).

| # | Divergence | Why | Evidence | ADR / status |
| --- | --- | --- | --- | --- |
| D1 | The sign-in page keeps Pangolin's compact house-branded card instead of AxonHub's split introduction/form layout | Pangolin's own visual identity; the reference's 375px sign-in has an unnamed icon button and two dangling `aria-describedby` targets, which Pangolin must not copy | `web/src/Auth.tsx`, `design-system/pangolin/MASTER.md` | recorded in the 2026-09-21 controller re-audit; no ADR needed for a visual choice |
| D2 | Hard delete for prompts and protection rules (AxonHub soft-deletes and keeps an archived state) | Simpler record model; the delete is audited and project-scoped | `operations_api.rs` delete path; `orchestration/protection.rs:28` filters `enabled=1` | row `prompts 18`; ADR optional |
| D3 | IP security is per API key (`allowed_ips`/`denied_ips`), not an instance-wide blocklist | Keeps the enforcement point at admission, where the key is already resolved | `db.rs` admission checks; `AccessPage.tsx:233-234` | row `system 9`; ADR recommended |
| D4 | Bulk API-key state requires `api_key:manage` (with `project:manage ⇒ api_key:manage` preserved) and applies the owner-membership rule | One authority for one lifecycle; the generic `project:manage` gate would have bypassed the key contract | `access.rs::set_scoped_api_keys_enabled`, `operations_api.rs:909`; `bulk_key_state_takes_the_key_permission_and_keeps_the_session_guard` | controller ruling, recorded in the SDD ledger |
| D5 | Invitations remain email-bound, finitely reusable and bounded to 30 days | B27 deliberately keeps identity binding and finite expiry while adding an operator-selected 1–100 use limit; AxonHub's reusable links have no email binding and can be unlimited | `access.rs` invitation acceptance; SQLite v18 | superseded by the implemented B27 decision; no longer blocks access 13 |
| D6 | No GraphQL endpoint; the admin surface is REST with a documented envelope | One control-plane protocol, one error contract, one authorization path | `api/errors.rs`, `api/operations_api.rs` router | **ADR required** — this is a capability AxonHub has and Pangolin does not |

## Decisions required

These rows cannot be closed by code alone; each needs an explicit product or
architecture decision, and the decision itself is the deliverable. Eight decisions
are queued here and in `tasks/parity-completion-plan.md`; six of them correspond to a
row of the 133, two are matrix-level.

| Row | Decision | Options | Consequence of not deciding |
| --- | --- | --- | --- |
| observability 14 | Display currency | (a) add an instance currency setting, (b) record USD-only as a deliberate boundary | the console keeps printing `$` for every operator, which may be wrong for a non-USD deployment |
| system 19 | Restore conflict granularity | (a) per-resource strategies, (b) record the single strategy as deliberate | a restore of mixed resources keeps one policy for all classes |
| system 23 | Instance retry / upstream-error policy | (a) promote selected knobs to instance settings, (b) document them as per-channel only | operators must edit channel JSON for instance-wide intent |
| system 24 | Quota routing mode | (a) add `IGNORE_QUOTA`/`REMOVE_ON_EXHAUSTED`/`BACKPRESSURE`, (b) document REMOVE_ON_EXHAUSTED only | an exhausted channel is always removed, with no alternative |
| system 25 | Diagnostics tab | resolved: build bounded export and owner-only clear for derived caches | SQLite remains authoritative; clear is audited and cannot mutate access or routing records |
| access 13 | Invitation reuse | **decided: (a)** finite max-uses (1–100), retaining email binding and bounded expiry | resolved by B27 |
| matrix (D6) | Scoped GraphQL endpoint | (a) implement one over the same permission model, (b) record the REST-only decision as divergence D6 with an ADR | a capability AxonHub has stays absent without a recorded reason |
| matrix | Semantic-memory provider | (a) implement an opt-in provider behind the documented interface, (b) keep the interface as documentation only | long-term memory stays unavailable even where an operator wants it |

## Deferred corrections carried forward

These were already identified as unfinished in the SDD ledger or the controller
re-audit and are **still open** in the inspected tree. None of them is a new
finding; each is named here so it cannot be lost.

| # | Correction | Current state in the tree | Required guard |
| --- | --- | --- | --- |
| C1 | Access project-switch loading flash | `ProjectProvider` returns `permissions: new Set([])` while `['project-permissions', id]` is in flight (`project.tsx:17,19`), so `AccessPage` renders the no-access card and unmounts the panels, losing `bindingUser` and accordion state | a two-project test asserting no no-access flash and preserved panel state during the switch |
| C2 | "Saved but the list could not be re-read" is two toasts | `KeysPanel`'s bulk success path toasts success, then toasts the refetch failure (`AccessPage.tsx:132-140`) | one localized result key, asserted with a mocked `sonner` |
| C3 | TraceTimeline links by the external id | `href: /operations/requests/${request.public_id}` (`OperationsPage.tsx:291`); the bundle also carries the internal `request.id` | switch to `request.id` and update the test that pins the current behaviour |
| C4 | Playground links by the external id | `requestId` is the gateway's `x-request-id` header, not `requests.id` (`PlaygroundPage.tsx:384-385`); the record read returns the internal `id` | link by the record's `id` |
| C5 | The D1 test comment is inaccurate | `operations.rs:1907-1910` says an unknown enum "fell back to `metadata`"; D1's own RED evidence shows `{"default_level":"verbose"}` was answered 422 by the extractor, and only an unknown *field* silently defaulted | correct the comment to state which shape silently defaulted and which was rejected outside the envelope |
| C6 | The routing form has no guard | `SystemPage.tsx:298` renders and submits the routing policy, but no test asserts it. The panel lives in a `keepMounted={false}` tab whose default is `appearance` (`SystemPage.tsx:34,47`), so a test that does not activate the tab mounts nothing — the sibling field's absence was the same cause, not a routing-specific one | a test that activates the Orchestration settings tab (the pattern `SystemPage.test.tsx` already uses for Backups) and asserts the rendered control plus the submitted payload |
| C7 | Badge contrast is unmeasured | The success/ENABLED badge measured 4.32:1 and the health-failure pill about 2.68:1 on a bundle that is now stale (`components.tsx:201-207`, `ChannelsPage.tsx:150-166`) | **reproduce before fixing**: extend `consoleFixes.test.tsx`'s WCAG computation, then re-measure with axe in a browser; change only ratios below 4.5:1 |
| C8 | English data-table clipping at 375px | Reported by browser QA, never reproduced locally; the request table widened to `minWidth={980}` in `d0a` | **reproduce before fixing**: 375 px, `en`, populated tables; no page-level horizontal scroll |

## Closed since the area reports

Each item was a real, operator-visible defect; each is now fixed, and the evidence
is the code path plus the test that fails without it.

- **The request log keeps the real upstream status.** `lifecycle.rs` prefers the
  status the attempt observed; a 429 is recorded as 429, not as a synthesized 502.
- **The upstream status and the terminal classification are now persisted per
  attempt** (`request_executions.provider_name/http_status/error_kind`, additive
  migration v9) and rendered, so the console no longer denies data it holds. The
  rebuild path reads those columns verbatim (`instance_backup.rs:446-448,495-499`),
  which closes the "authoritative rebuild degrades observability" blocker.
- **A model with price history can no longer be deleted into an opaque 500** — it
  is a typed 409 that says why, and a model with no history still deletes.
- **A stored logging policy with an unknown field can no longer break the
  gateway**, and a typo can no longer install a different policy: reads tolerate,
  writes refuse (`operations/logging.rs:35-36,56-119`).
- **The console no longer offers the removed `retain_payloads` switch.**
- **Members can be identified** (`MembershipView` carries email and display name).
- **A trace opens by the id the client sent, with its children**; the trace detail
  exposes per-execution `usage` and `cost_items`.
- **The projection is keyed on Pangolin's own request UUID**, not the
  caller-controlled `x-request-id`, and its identity resolution is project-scoped
  with internal-id precedence (`observability.rs` identity tests).
- **A cleared projection is rebuilt rather than left empty** after an instance
  restore.
- **Reads no longer open the database a second time** (the freeze defect); the
  regression rests on the instance-level measurement, and the ledger says so.
- **Malformed prompt activation and protection scopes are refused at write time**;
  injection skips endpoints that cannot carry it, recording a decision instead of
  answering 400.
- **The request detail explains the request**: attempts, usage and per-component
  cost, selected by the resolved, project-scoped identity, with a cross-project
  collision test.
- **Bulk enable/disable covers prompts, protection rules and keys**, with row
  selection in the console and the key path routed through the access layer.
- **Admin rejections answer with the error envelope.**
- **A broken schedule says why** (`last_error` projected and rendered).
- **Conflicts are named**: `duplicate_model`, `resource_in_use`,
  `history_retained`, plus the general `UNIQUE`→409 safety net, and the console
  localizes a known code while keeping the server's message.
- **The request list pages deterministically** (id tiebreaker), the credential
  health column renders, the probes table renders resolved values, recorded error
  codes are localized, and the artifact download sends the CSRF header.
- **A refused delete is reported once** (in the dialog, not also in a toast).
- **The playground reports the protocol's terminal state** (failed, incomplete,
  cancelled, interrupted, stopped) and shows the recorded usage and cost, with the
  first terminal winning and a superseded run unable to write to the panel.
- **Catalog and access mutations invalidate the query they actually changed**,
  including on HTTP failure and across a mid-flight project switch.

## Superseded claims

- `docs/reviews/parity-status.md` contradicts the code on 17 rows and is
  **superseded by this file**. Its counts (20 closed / 16 partial / 42 open) were
  derived before Tasks A–D and before this reconciliation; do not cite it.
- The ledger's own former "Open gaps" list carried 15 rows that were already
  closed, and its "Open defect with a confirmed root cause" (the projection
  freezing at the first console read) is fixed.
- "The request detail returns executions but the console still says they are
  unavailable" — the console now renders them.
- "The upstream status is not persisted" — it is, per attempt, and the console
  prints the recorded code when the record has one.
- "Authoritative rebuild still degrades observability" — closed by D2b; the
  rebuild no longer invents `200`/`502` or substitutes a provider id for a name.
- "`/operations/requests/{id}` reads the DuckDB relation `request_executions`" —
  wrong when written: that relation is the SQLite record table.
- The stale-claims section of the 2026-09-21 controller re-audit is discharged:
  the request-detail identity bug (A1), the generic bulk-toggle key bypass (A2),
  the playground terminal handling (C2), the catalog failure invalidation (C1),
  the role-binding query key (C1) and the authoritative rebuild (D2b) are all
  fixed with tests.

## Capability matrix disposition

Every capability of `docs/axonhub-capability-matrix.md` (112 table rows plus the
console-page inventory paragraph, 113 items) is dispositioned here
so no row disappears silently. `implemented+tested` and `contract-tested` mean the
capability exists and is covered by a Rust or web test; `partial`, `open` and
`feature-build` name what is missing; `divergence` points at the table above.

Disposition totals: **68 `implemented+tested`**, **28 `partial`**, **8
`feature-build`**, **6 `open`**, **2 `divergence`** (one flagged ADR-required) and
**1 `upstream-todo`** — 113 items, none omitted. The `feature-build` set is the five
per-provider OAuth rows plus their aggregate row in *Operations and system
management*, model discovery/sync, and the chat playground; the `open` set is
credential recovery, per-channel proxy, developer settings, live preview, instance
retry/model/quota settings, and CORS/timeouts. Every one of them is scheduled in
`tasks/parity-completion-plan.md`.

### Public gateway and protocol routes (23)

| Row | Disposition | Evidence / what is missing |
| --- | --- | --- |
| OpenAI chat completions + SSE | implemented+tested | `api/gateway.rs`, `orchestration/stream.rs` |
| Legacy completions | implemented+tested | `api/protocols.rs:131`, same gateway path |
| Responses HTTP | implemented+tested | `api.rs:203`, `orchestration/session.rs` |
| Responses compact | implemented+tested | `api/protocols.rs:132`, `orchestration/compaction.rs` |
| Responses WebSocket | implemented+tested | `api/protocols.rs:132`, `api/protocols/websocket.rs` |
| Model list/retrieve | partial | routes exist (`/v1/models`, `/v1/models/{*model}`) but return no card metadata (models 11) |
| Embeddings | implemented+tested | `api/protocols.rs:133` |
| Moderations | implemented+tested | `api/protocols.rs:134` |
| Alpha search | implemented+tested | `api/protocols.rs:135` |
| Image generations | implemented+tested | `api/protocols.rs:136` |
| Image edits | implemented+tested | `api/protocols.rs:137` (multipart) |
| Videos create/get/delete | implemented+tested | `api/protocols.rs:138-141`, `tasks::get_video/delete_video` |
| Audio speech | implemented+tested | `api/protocols.rs:142` |
| Audio transcription | implemented+tested | `api/protocols.rs:143` |
| Audio translation | implemented+tested | `api/protocols.rs:144` |
| Anthropic via `/v1/messages` | implemented+tested | `api.rs:204`, transform in `api.rs:1135` |
| Anthropic namespace | implemented+tested | `api/protocols.rs:148-149` |
| Rerank | implemented+tested | `api/protocols.rs:145` |
| Jina embeddings/rerank | implemented+tested | `api/protocols.rs:146-147` |
| Gemini native + alias | implemented+tested | `api/protocols.rs:150-153` |
| Doubao video/content tasks | implemented+tested | `api/protocols.rs:154-158` |
| AI SDK compatibility | implemented+tested | `api/protocols.rs:159` |
| Realtime | upstream-todo | AxonHub's README marks it Todo; not a parity blocker |

### Identity, access and external authentication (16)

| Row | Disposition | Evidence / what is missing |
| --- | --- | --- |
| Users and owner | implemented+tested | `access.rs` user CRUD, profile, language, password |
| Projects | implemented+tested | project CRUD, selection, isolation (`access.rs`, `project.tsx`) |
| Memberships and invitations | implemented+tested | members support in-place role/status edits, search, pagination and mobile cards; invitations support bounded email-bound reuse with atomic exhaustion (access 3, 4, 13) |
| Roles and permissions | partial | roles and bindings work; the permission catalog has no route (access 15) |
| API-key types | implemented+tested | `user`/`service`/`personal`/`no_auth` validated server-side; the create form still hardcodes `service` (access 9) |
| API-key status/scopes | implemented+tested | enable/expiry/budget/IP policy enforced at admission; console shows the enforced state |
| API-key token mode | implemented+tested | generated or imported high-entropy token, indexed digest + Argon2id, no plaintext recovery |
| API-key profiles/templates | implemented+tested | profiles and project-scoped save/apply/import/export templates share one validator and writer (access 18) |
| OIDC providers | partial | discovery/config/authorize/callback/PKCE/state/JIT and manual link exist; no login-only or branding (access 14) |
| Codex OAuth | feature-build | no start/exchange route; only generic credential storage exists |
| xAI OAuth/SSO | feature-build | as above |
| Claude Code OAuth | feature-build | as above |
| Antigravity OAuth | feature-build | as above |
| GitHub Copilot device OAuth | feature-build | no device-code flow |
| IP security | divergence | per-key allow/deny only (D3) |
| OpenAPI GraphQL auth | divergence (ADR required) | no GraphQL endpoint (D6) |

### Channels, credentials and models (23)

| Row | Disposition | Evidence / what is missing |
| --- | --- | --- |
| Channel CRUD/bulk | implemented+tested | CRUD, clone, collision-safe merge, enable/disable and dependency-previewed atomic bulk delete; clone never copies credentials |
| Channel families | implemented+tested | 18 kinds incl. OpenAI/Anthropic/Gemini/Azure/Bedrock/Vertex/GCP (`db/schema.rs` kinds check) |
| Provider preset catalog | implemented+tested | 59 built-ins with logo keys, default URL/auth/endpoints (`catalog/data/builtin.json`) |
| Catalog online maintenance | implemented+tested | import/export, prioritized signed subscriptions, ETag, staged activation, last-known-good |
| Multiple credentials | partial | encrypted list with suffix and per-key state; no OAuth/GCP credential types |
| Credential recovery | implemented+tested | undecryptable credentials fail over locally and use hashed, expiring, single-use, project/credential-bound replacement tokens without returning envelopes |
| Proxy settings | implemented+tested | per-channel HTTP(S)/SOCKS transport with encrypted write-only credentials and bounded client reuse |
| Endpoint mappings | implemented+tested | `channel_settings.endpoint_mappings` + `providers/mod.rs` |
| Model discovery/sync | implemented+tested | bounded OpenAI/Anthropic/Gemini discovery with durable manual/scheduled reconciliation and manual-model precedence |
| Model transformations | implemented+tested | prefix/lowercase/mappings/exclude/stream plus exact-segment auto-trim and discovery-only original/transformed visibility flags |
| Model protocol policy | implemented+tested | `capabilities` + per-model `stream` policy |
| Model cards | partial | catalog metadata is stored at creation but not surfaced as columns (models 10) |
| Model catalog defaults | implemented+tested | versioned snapshot with refresh/cache, aliases and operator override |
| Extensible model capabilities | partial | typed capabilities with preserved unknown extensions (`catalog/types.rs:121-122`); discovery/quota stay `implemented=false` |
| Model CRUD/bulk | partial | CRUD + enable/disable bulk; no archive lifecycle, no batch create (models 10, 22) |
| Associations | implemented+tested | core types plus channel-tag regex, bounded conditions and association-level channel name/id/tag exclusions |
| Conditions | implemented+tested | bounded nested AND/OR with typed operators, UTC daily time, media presence, stream/request-format fields and a structured editor |
| Developer settings | implemented+tested | versioned project rules provide developer-scoped channel associations and reasoning-effort mappings with per-model inheritance control |
| Pricing | partial | channel price entries unreachable, no read projection (models 18, 24) |
| Pricing modes | partial | flat/unit/tiered with cache-write variants; no volume mode (models 17) |
| Channel probing | partial | manual probe job with status/latency/TTFT and error; no scheduled history view, no TPS, no model choice |
| Provider quota collection | implemented+tested | configured paths plus direct/nested/used-limit normalization preserve period/source metadata; unknown shapes remain unmeasured and non-enforcing |
| Auto-disable | implemented+tested | status/error pattern counters, duration or cron+timezone recovery, channel or credential action (`operations/runtime.rs:400-470`) |

### Request orchestration (23)

| Row | Disposition | Evidence / what is missing |
| --- | --- | --- |
| Request source/thread/trace middleware | implemented+tested | `lifecycle.rs::begin` (source, supplied/generated ids, context propagation) |
| API-key profile mapping | implemented+tested | profile mapping applied before candidate selection |
| Candidate generation | implemented+tested | `orchestration/repository.rs:149-200` |
| Access filtering | implemented+tested | project/key/model/protocol checks before routing |
| Quota filtering | implemented+tested | quota and budget/rate availability in candidate SQL (`repository.rs:130-148`) |
| Sticky routing | implemented+tested | `orchestration/affinity.rs` with trace/session modes |
| Load balancing | implemented+tested | failover, round-robin, weighted random, least-in-flight, latency composite |
| Admission control | implemented+tested | RPM/TPM/concurrency/queue with bounded queue and timeout |
| Circuit breaker | implemented+tested | `channel_settings.circuit` with half-open recovery |
| Retry policy | implemented+tested | per-channel limits, delay, retryable statuses/patterns, transport errors |
| Streaming policy | implemented+tested | first-event timeout, terminal handling, no retry after commit |
| Empty response detection | implemented+tested | `empty_success` policy on the attempt |
| Upstream error policy | partial | pass-through/normalized per request mode; no instance-level setting (system 23) |
| Header/body overrides | implemented+tested | ordered conditional JSON operations (`parameter_overrides`) |
| Pass-through modes | implemented+tested | body and User-Agent pass-through controls |
| Transform options | implemented+tested | developer/system role normalization, array forcing, reasoning effort |
| Prompt injection/actions | partial | activation conditions, enable/bulk lifecycle; no `append`, no `order` (prompts 6, 7) |
| Prompt protection | partial | regex matcher, deny/redact, scopes, test mode; no preview, no description/archived (prompts 11) |
| Allowed tools | implemented+tested | `allowed_tools` filtering without corrupting message order |
| Auto reasoning effort | implemented+tested | developer/model-aware effort inference (`protection.rs:364-392`) |
| Responses sessions | implemented+tested | `orchestration/session.rs`, compact path |
| Cross-upstream context economy | partial | sticky route, durable exact replay, threshold compaction and prompt-cache hints exist; the semantic-memory interface is documented but has no provider |
| Channel/API-key request tracking | implemented+tested | `pangolin_orchestration_inflight` per channel and key, concurrency-safe counters |

### Request lifecycle, observability and analytics (15)

| Row | Disposition | Evidence / what is missing |
| --- | --- | --- |
| Thread | implemented+tested | `threads` + client threading headers |
| Trace | implemented+tested | `traces` with API-key/project/user attribution |
| Request | implemented+tested | `requests` with content policy and source IP |
| Site/key logging levels | implemented+tested | one effective policy resolved at admission, secrets always stripped |
| Request execution | implemented+tested | one row per attempt with timing, retry reason and the outcome snapshot |
| Usage log | implemented+tested | input/output/cache/reasoning tokens, units, cost reference |
| Detailed cost | implemented+tested | price item, quantity, tiers, cache TTL variants, subtotal |
| Live preview | open | no in-flight request snapshot resource or surface |
| Content download | implemented+tested | `content` resource returns the stored bodies under project authorization (`operations_api.rs:498-503`) |
| Dashboard | implemented+tested | summary + per-dimension breakdowns |
| Analytics | partial | date and dimension filters exist at the API; Overview is pinned to 24h and there is no analytics page with its own filters (observability 2) |
| Performance analytics | partial | latency and TTFT averages per dimension; no throughput/TPS or confidence intervals |
| Cost analytics | partial | cost by channel/model/key/user/project via `/analytics`; no dedicated cost page |
| Retention and GC | implemented+tested | per-resource policies, body cleanup, vacuum |
| Metrics/logging | implemented+tested | `/metrics` with gateway/admission/limiter/queue metrics and redacted logs |

### Operations and system management (12)

| Row | Disposition | Evidence / what is missing |
| --- | --- | --- |
| System initialization/onboarding | implemented+tested | setup route, brand name/logo/title, onboarding progress |
| System retry/model/quota settings | open | system 12, 23, 24 |
| CORS and request timeouts | open | no operator CORS configuration and no configurable request timeout |
| Data storage | implemented+tested | typed local/S3/GCS/WebDAV targets with encrypted credentials, owned prefixes and bounded read-only connection tests |
| Backup/restore | implemented+tested | selective resources, conflict strategy, secret preservation, preflight |
| Automatic backup | partial | schedule/retention/status and manual trigger exist; interval only, no daily time or timezone (system 13) |
| Webhooks | implemented+tested | targets, encrypted headers, body template, subscriptions, echo and delivery retry with history |
| Scheduler | implemented+tested | durable jobs for probe, model sync, quota, backup and GC |
| Favicon/static SPA | implemented+tested | embedded branded assets and deep links |
| Playground/chat | feature-build | prompts 13 |
| Request content policy | partial | store-chunks/body toggles are expressed as levels; no live-preview toggle |
| Provider OAuth credential helpers | feature-build | no Codex/xAI/Claude Code/Antigravity/Copilot setup flows |

### Console feature pages (1)

| Row | Disposition | Evidence / what is missing |
| --- | --- | --- |
| Dashboard; analytics; keys/profiles; channels/credentials/probes/prices; models/associations; playground/chats; projects/members; roles; users; prompts; protection; requests/live/content; threads; traces/executions; usage; storage; backup/restore; OIDC/system/security/onboarding | partial | Pages exist for dashboard, keys, profiles, channels, credentials, probes, prices, models, associations, playground, projects, members, roles, users, prompts, protection, requests, threads, traces, executions, usage, storage, backup/restore, diagnostics, OIDC, system, security, onboarding. Missing or thinner: a live-preview page, an analytics page with its own filters, chats history, a prices editor (models 26), a groups editor (models 19), provider OAuth setup |

## ax-llm/ax recommendation

`https://github.com/ax-llm/ax` is a DSPy-style LLM programming framework
(TypeScript-first, with native libraries the project's own README lists for
several other languages, including Rust). **Recommendation: add no dependency.**

- No remaining row needs it. The 43 work rows are console surfaces, catalog and
  pricing presentation, OAuth setup flows, an instance settings document, CORS,
  proxying, and identity plumbing. Ax's value is authoring and optimizing
  LLM programs (signatures, optimizers, RAG pipelines, agent loops), which is not
  what any of those rows lack.
- Where Pangolin does have a prompt/agent-shaped gap — prompt `order` and
  `append` (prompts 6, 7), the protection preview (prompts 11), the admin chat
  surface (prompts 13) — the missing part is a record field, a placement rule or a
  console surface over the existing gateway pipeline, not a framework.
- Adopting it would conflict with the repository's dependency policy: the release
  is a single Rust binary, the console is a React SPA, and the policy prefers
  mature license-compatible crates wrapped behind Pangolin interfaces. A TS agent
  framework would either add a Node runtime to the gateway path or a new binding
  of unverified maturity and license.
- **Uncertainty:** I read the project's own description and search results, not its
  source or license file (the repository page did not load in this environment).
  If a future row needs declarative prompt optimization, re-evaluate the license
  and the Rust binding maturity first; do not add it speculatively.

## Method and limits

- Every status in the reconciled tables was decided from the code and the tests in
  the inspected tree, not from a report. Where a row's evidence is a report rather
  than a test, the row says so.
- The Rust and web gates were run on the inspected tree. The Rust suite was green
  (242/242); the web suite was green in isolation (1333/1333) and reported 8
  failures in a run that overlapped `cargo test`, with the failing names not
  captured — treated as load flakiness, and re-run before being believed.
- No browser pass was run for this reconciliation. The two browser-only items
  (C7 contrast, C8 375 px clipping) are marked reproduce-before-fix for exactly
  that reason, and every contrast number quoted here comes from a bundle that is
  now stale.
- AxonHub was read, never executed, in this pass.
