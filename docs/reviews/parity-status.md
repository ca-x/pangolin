# Parity gap status — current tree

Re-checked against the working tree (HEAD `e666bef` plus the uncommitted batches) on
2026-09-21. Every row below is one `gap`/`feature-build` row of the six area reports, decided
from the code, not from `parity-ledger.md`'s "fixed after" section. The area reports are
point-in-time evidence and were not edited.

Statuses: `closed` (the gap is gone) · `open` (still true) · `partial` (partly addressed;
what remains is stated) · `out-of-scope` (one of the four builds the goal excludes) ·
`feature-build` (no capability exists, still in scope).

Row numbers refer to the numbered finding in the matching area report.

| Area | Finding | Status | Evidence in the current tree |
| --- | --- | --- | --- |
| observability | 1 Per-dimension breakdowns absent | closed | `web/src/pages/OverviewPage.tsx:28-43,175-248` renders a `provider\|model\|api_key\|user\|project` breakdown from `/analytics`; row type at `web/src/api.ts:25` |
| observability | 2 No period or date-range selection | partial | Request log has presets plus a custom range (`OperationsPage.tsx:24-44,88-100,132-139`); Overview and its breakdown are still pinned to 24h (`OverviewPage.tsx:58,186`); the generic list still ignores `from`/`until` (`src/api/operations_api.rs:416-433`) |
| observability | 3 Trend chart carries requests and errors only | open | `OverviewPage.tsx:91-97,116-123` two series; `SummaryPoint` has no token/cost bucket (`web/src/api.ts:14`) |
| observability | 4 Rows do not show why a request failed | closed | `OperationsPage.tsx:152-153` renders a localized `failureReason` from `row.error_kind`; the field is in `RequestItem` (`api.ts:18`) |
| observability | 5 No time-window filter on the request log | closed | `OperationsPage.tsx:24-32,98-100` sends `from`/`until` |
| observability | 6 No API-key filter on the request log | open | `src/observability.rs:84-96` — `RequestFilter` still has no `api_key_id`; the console filters status/provider/model only (`OperationsPage.tsx:87`) |
| observability | 7 List columns omit the row's telemetry | open | `OperationsPage.tsx:152` renders no tokens, no `resolved_model`, no TTFT; `RequestItem` has no `ttft_ms`/stream field (`api.ts:18`) |
| observability | 8 Detail has no per-attempt list | partial | The API returns `executions`/`usage`/`cost_items` (`operations_api.rs:496-515`; test `operations.rs:463`) but the console still prints `attemptsUnavailable`/`costComponentUnavailable` and `RequestDetail` omits the fields (`OperationsPage.tsx:439-442`, `api.ts:19`) |
| observability | 9 Upstream status and error text not persisted | partial | The request row now keeps the observed upstream status (`src/operations/lifecycle.rs:582-589`; test `operations.rs:1566`); `request_executions` still has no per-attempt status/error column (`src/db/schema.rs:469-484`) and the detail still says so (`OperationsPage.tsx:424`) |
| observability | 10 Payload view is raw JSON only | open | `OperationsPage.tsx:456-459,472-489` — two JSON blocks plus a download |
| observability | 11 Detail lacks client and network identity | open | `requests.source_ip` is stored (`schema.rs:460`) and returned by the trace bundle (`operations_api.rs:459`), but the observation event has no IP/UA (`observability.rs:570-593`) and the detail shows `api_key_id` only (`OperationsPage.tsx:412`) |
| observability | 12 Trace list omits trace id, request count, first query | open | `OperationsPage.tsx:72` — status/detail/thread/started/finished; `external_id` is projected (`operations_api.rs:331`) but unrendered; no request count |
| observability | 13 Trace detail shows no token or cost totals | closed | `OperationsPage.tsx:304-385` totals per-execution usage and price components from the trace bundle (`operations_api.rs:464-465`) |
| observability | 14 Cost always USD, six decimals | open | `web/src/observability.tsx:19-23` hardcodes `$`; no currency setting anywhere (`rg currency` empty in `web/src` and `src/`) |
| observability | 15 No per-trace archive / pin lifecycle | feature-build | Traces carry a run `status` only (`schema.rs:438-448`); no archive/pin route in `operations_api.rs` |
| prompts | 3 `scopes: null` bypasses validation | closed | `operations_api.rs:1124-1129` normalises null to `{"version":1}` and then validates |
| prompts | 4 Injection 400s non-conversational endpoints | open | `src/orchestration/protection.rs:125-129` still returns `Error::Invalid` |
| prompts | 5 New prompts default to enabled | open | `operations_api.rs:1106` `unwrap_or(true)`; the console checkbox defaults checked (`web/src/pages/shared.tsx:222`) |
| prompts | 6 No `append` action | open | `protection.rs:47-53` builds one `prefix`; no action column in `schema.rs:397-408` |
| prompts | 7 No `order` field | open | `protection.rs:47` `ORDER BY created_at,id` |
| prompts | 8 Conditions cannot match an API key | closed | `policy::context` carries `project_id`/`api_key_id` (`src/orchestration/policy.rs:330-345`) and the orchestrator passes the key (`orchestration/mod.rs:158,231`); test `a_prompt_can_be_scoped_to_the_calling_api_key` |
| prompts | 9 No bulk enable/disable for prompts | partial | The route covers prompts/protection (`operations_api.rs:1157-1158`; test `operations.rs:614`); the console's only bulk panel is channels/credentials with pasted ids (`ChannelsPage.tsx:188,199`) — prompts cannot reach it |
| prompts | 10 Prompt list hides what a prompt does | open | `PromptsPage.tsx:16` — name/role/content/enabled; no activation summary |
| prompts | 11 Protection rules have no preview or metadata | open | No preview route (only routing-preview exists) and no `description`/`archived` in the form (`PromptsPage.tsx:19`) |
| prompts | 12 Duplicate prompt name is a generic 500 | closed | Pre-check plus `ApiError::Conflict` (`operations_api.rs:1090-1105`) and a UNIQUE→409 safety net (`src/api.rs:128-140`) |
| prompts | 13 Playground is a single-turn form | out-of-scope | Still key-paste, one `/v1/responses` (`PlaygroundPage.tsx:82-94`); the goal excludes the playground as a full chat surface |
| prompts | 14 Picker is not endpoint-filtered | open | `PlaygroundPage.tsx:22-24` lists `/v1/models` while every send goes to `/v1/responses` (`:82`) |
| models | 9 Catalog cards never applied to console-created models | open | `operations_api.rs:957-960` defaults capabilities to `["chat"]` and both prices to 0 (`:982`); `db::create_model` with catalog defaults is still dead code (`src/db.rs:322`) |
| models | 10 Rows are channel bindings, not a global record | open | `models.provider_id` still NOT NULL (`schema.rs:67-79`); the list shows no developer/type/icon/limits/archive (`ModelsPage.tsx:54`) |
| models | 11 `/v1/models` returns no metadata | open | `src/orchestration/mod.rs:168` still `{id,object,created,owned_by}` |
| models | 12 No global model settings surface | open | No route or form for fallback/blacklist/hide flags (`rg fallback_to_channels\|model_blacklist` empty) |
| models | 13 Project default routing has no write path or UI | partial | Write path exists and is validated (`operations_api.rs:573,663-704`; test `operations.rs:343`), but the console form submits only affinity/compaction (`SystemPage.tsx:268`), so a console save resets `routing` to `{"version":1}` |
| models | 14 Association conditions: no builder, no domain fields | partial | Context now carries `project_id`/`api_key_id` (`policy.rs:330-345`); still a raw JSON box (`ModelsPage.tsx:62`) with no `daily_time`/media-presence fields |
| models | 15 No `channel_tags + regex`, no exclusions | open | `src/orchestration/repository.rs:174-180` (tag must equal a tag) and `:240-252` (channel-level upstream-name regex only) |
| models | 16 No auto-trim, no hide-original/hide-mapped | open | No `auto_trim`/`hide_original`/`hide_mapped` anywhere; `model_rules` is raw JSON (`ChannelsPage.tsx:184`) |
| models | 17 No volume (non-marginal) tier mode | open | `src/operations/pricing.rs:310-322` marginal only; `:371-378` upper bound uses the maximum rate |
| models | 18 Channel-scoped prices unreachable | open | `operations_api.rs:1281` inserts no `provider_id`; the form has no channel field (`ModelsPage.tsx:68`) |
| models | 19 Service-group ratio has no console surface | open | `groups` is API-only (`operations_api.rs:1286-1310`); no `resource="groups"` page |
| models | 20 Model/provider deletion safety | partial | Both deletes now refuse with a typed 409 naming the blocker (`operations_api.rs:1453-1492`; tests `operations.rs:224,415`); no impact preview and no archive lifecycle (see row 10) |
| models | 21 No per-channel proxy configuration | open | No proxy field in `channel_settings` (`schema.rs:344-352`) or the form (`ChannelsPage.tsx:184`) |
| models | 22 No model batch create, no real bulk lifecycle | open | No batch create; bulk is pasted ids for channels/credentials only (`ChannelsPage.tsx:188-214`) |
| models | 23 No unassociated-model detection | open | No route or UI (`rg unassociated` empty) |
| models | 24 Price list omits the components | open | `operations_api.rs:356` projection has no components; `ModelsPage.tsx:68` columns end at schedule |
| models | 25 Provider model discovery / sync | out-of-scope | `src/catalog/types.rs:270-271` forces `implemented=false`; excluded by the goal |
| models | 26 Full price editor | out-of-scope | `components`/`schedule` are JSON textareas (`ModelsPage.tsx:68`); excluded by the goal |
| access | 2 Member rows identify users by opaque id | closed | `src/access.rs:312-320` joins email/display_name; the console renders name then email (`AccessPage.tsx:445-446`); test `the_member_roster_carries_the_member_identity` |
| access | 3 Role or status cannot be changed in place | open | The members table still offers add/remove only (`AccessPage.tsx:454-467`); the raw-id upsert form is in the Roles tab (`:238,251`) and predates these batches |
| access | 4 Members list has no search, pagination or mobile layout | open | `AccessPage.tsx:420` raw table; every other Access tab uses `ResourcePage` (`shared.tsx:157-163`) |
| access | 5 Members tab unreachable for the principal | closed | `Shell.tsx:34` includes `project:read` in `canAccess` |
| access | 6 No SSO/OIDC entry point on sign-in | partial | `GET /api/v1/auth/oidc/providers` now exists unauthenticated (`access_api.rs:96,584-592`; `oidc.rs:186-204`) but `web/src/Auth.tsx:41-69` still has no SSO button |
| access | 7 OIDC identities never listed or unlinked | open | `OidcIdentitiesPanel` is still a create-only form (`AccessPage.tsx:270-290`) |
| access | 8 No self-service account linking | feature-build | No route or console entry |
| access | 9 Key creation hardcodes type, owner and scopes | open | `AccessPage.tsx:95` still `key_type:'service'`, `scopes:['gateway:use']`; no scopes column (`:107-114`) |
| access | 10 Key status hides expiry, spend and last use | partial | The view now returns `expires_at` and `spent_micros` (`access.rs:452-458`, `src/models.rs:53`; test `the_key_view_carries_expiry_and_spend`); the console still shows an enable toggle and no expiry/spend/last-used column (`AccessPage.tsx:107-134`) |
| access | 11 Per-key usage and cost unreachable from the key | partial | Reachable through the Overview `api_key` breakdown (`OverviewPage.tsx:28,40`), not from the key row |
| access | 12 No rotate, archive or bulk key operations | feature-build | No rotate/revoke route (`access.rs:1390-1530`) |
| access | 13 Invitations are single-use and email-bound | closed | SQLite v18 and `InvitationInput.max_uses` provide email-bound finite reuse (1–100), atomic exhaustion and one transactional audit per acceptance |
| access | 14 OIDC providers have no login-only mode or branding | feature-build | `oidc_providers` columns unchanged (`schema.rs:270-281`) |
| access | 15 Role editor edits raw permission JSON | feature-build | `AccessPage.tsx:59` permissions JSON; no permission-catalog route |
| access | 16 Role bindings never listed or revoked | open | `AccessPage.tsx:255-262` POST only; no consumer for `GET`/`DELETE .../role-bindings` (`access_api.rs:71,75`) |
| access | 17 Create-user form offers ignored controls | closed | `UserInput.enabled` is honoured (`access.rs:267-278,735-741`; test `creating_a_user_honours_the_enabled_switch`) |
| system | 10 Retention form still offers `retain_payloads` | closed | Gone from the console (`SystemPage.tsx:62`), the projection (`operations_api.rs:403-407`) and both locales (`rg retain_payloads` empty) |
| system | 11 Policy parsed with `deny_unknown_fields`, no `version` | partial | Unknown fields are now tolerated on read (`src/operations/logging.rs:35-37`), which closes the admission 500; the document still has no `version` and no startup validation |
| system | 12 A broken schedule fails silently | partial | `last_error` is written and projected (`operations_api.rs:376`; test `operations.rs:722`) but the console table has no column (`SystemPage.tsx:650`) |
| system | 13 No cron, time-of-day or timezone | open | `interval_secs` only (`operations_api.rs:1332-1336`; `SystemPage.tsx:650`) |
| system | 14 Auto-backup retention count only via raw JSON | open | The schedule form is still a raw `payload` box (`SystemPage.tsx:650`) |
| system | 15 No route or surface to list or re-download an artifact | partial | Routes exist (`operations_api.rs:64-71,1509-1540`; test `backup_artifacts_can_be_listed_and_re_downloaded`) but the console still keeps session-only artifacts and its comment saying the route is missing (`SystemPage.tsx:412-414`) is now false |
| system | 16 Delivered archive's location not shown | partial | `object_key` is projected (`operations_api.rs:386`) but the run line renders status/size/error only (`SystemPage.tsx:627`) |
| system | 17 Webhook delivery history has no console surface | open | No `webhook-deliveries` consumer anywhere in `web/src` |
| system | 18 Storage targets are raw JSON, local + S3 only | open | `SystemPage.tsx:48` JSON textareas; `src/operations/storage.rs:13-26` |
| system | 19 One restore conflict strategy | open | `SystemPage.tsx:584` — one `strategy` for the whole artifact |
| system | 20 `catalog:manage` System nav leads nowhere | closed | `/models` is in the nav for `catalog:manage` (`Shell.tsx:36`) |
| system | 21 Admin body rejections bypass the error envelope | closed | `src/api/errors.rs:95-122` normalises `/api/admin/` rejections; test `operations.rs:678` |
| system | 22 No instance-level general settings | open | No currency or timezone setting (`rg currency\|timezone` empty in `web/src` and `src/`) |
| system | 23 No instance retry or upstream-error policy | open | Only per-channel JSON (`schema.rs:344-352`); no instance route |
| system | 24 No quota-collection or routing-mode settings | open | No collection toggle or routing mode; an exhausted quota is always removed (`repository.rs:210-211`) |
| system | 25 Diagnostics tab | feature-build | No cache-diagnostics or clear-cache surface; About's "copy diagnostics" is build identity (`SystemPage.tsx:145-157`) |
| system | 26 Proxy presets, per-webhook timeout/proxy | feature-build | No outbound proxy support; the catalog fetch is still `.no_proxy()` (`src/catalog/refresh.rs:35`) |
| console UX | 1 Bulk selection and bulk actions | open | `shared.tsx:128-132` has no selection column or state; models remain unreachable from the console bulk panel (`ChannelsPage.tsx:188`) |
| console UX | 2 Probing a channel from inside the console | partial | The probe error column is now rendered (`ChannelsPage.tsx:41`); still no per-row test action or inline result (`:216-237`) |
| console UX | 4 Filters on the request log | partial | Window plus status/provider/model facets (`OperationsPage.tsx:128-139`); no API-key facet (observability row 6) and filters are not persisted in the URL |
| console UX | 5 Dismissible onboarding | open | The shell banner still has no dismiss control (`Shell.tsx:148-163`) |
| console UX | 6 Option-list queries fail silently | closed | Every option list reports failure with a retry (`shared.tsx:43-49,207,256`, `ChannelsPage.tsx:170,229`, `ModelsPage.tsx:43-46,95`, `AccessPage.tsx:157,482`) |
| console UX | 7 Invitations list has no empty state | closed | `AccessPage.tsx:313` EmptyState carrying the invite action |
| console UX | 8 Unreadable policy read as "recording on" | closed | `observability.tsx:61-75` adds an `unknown` reason for a failed or unreadable policy; test `observabilityUnknown.test.tsx` |
| console UX | 9 Mono table cells below 14px | closed | `styles.css:265` — `.mono-cell` and `td code` are 14px |
| console UX | 10 Localization leaks | closed | All six strings routed through the locales; guarded by `localizationLeaks.test.tsx` |
| console UX | 11 Shimmer animates a non-composited property | closed | `styles.css:338-352` sweeps a transform on a pseudo-element, disabled under reduced motion |
| console UX | 14 Channel health has no console surface | closed | `ChannelsPage.tsx:34,100-147` renders channel and credential health with an explicit unknown and a failure banner; test `channelHealth.test.tsx` |

## Counts

| Status | Count |
| --- | --- |
| closed | 20 |
| partial | 16 |
| open | 42 |
| feature-build (still in scope) | 9 |
| out-of-scope | 3 |
| **total** | **90** |

Per area: observability 15 (4 closed, 3 partial, 7 open, 1 feature-build) · prompts 12 (3/1/7/1
out-of-scope) · models 18 (0/3/13/2 out-of-scope) · access 17 (3/3/5/6 feature-build) ·
system 17 (3/4/8/2 feature-build) · console UX 11 (7/2/2).

## Fixed by the batches but never listed in the reports

- `From<sea_orm::DbErr>` maps any UNIQUE violation to a typed 409 instead of an opaque 500
  (`src/api.rs:128-140`) — a general safety net beyond the two named pre-checks.
- The request status badge no longer collapses to its icon below ~1150px, and table headers
  went from 10.5px to 12px (`styles.css:259-267`).
- The trend chart draws a single bucket as a level line instead of a triangle to zero, and its
  disclosure table scrolls at 375px (`OverviewPage.tsx:109-119`; test `trendChart.test.tsx`).
- Request-detail prev/next navigation through the page it came from (`OperationsPage.tsx:196-208`).
- A trace now renders as a timeline of real start offsets and durations, not a list of ids
  (`OperationsPage.tsx:261-296`).
- `GET .../backup/artifacts` and a single-artifact download route (`operations_api.rs:64-71,1509`),
  `GET /api/v1/auth/oidc/providers` (`access_api.rs:96`), the About build-provenance panel with
  "copy diagnostics", a mobile catalog list, a provider radio-card picker, and `SecretInput` with a
  localized visibility toggle — none of these were gap rows.
- `_internal.observation_reset_required` marks a projection whose shape changed so startup
  re-derives it (`src/main.rs:53-62`, `src/observability.rs:330`).
- `db::list_api_keys` now selects `expires_at` (`src/db.rs:425`).
- Nineteen new gateway regression tests (`src/api/gateway/tests/operations.rs`) and the
  `channelHealth`/`observabilityWiring`/`observabilityUnknown`/`localizationLeaks`/`consoleEnums`/
  `playgroundStream`/`unsettledUsage`/`confirmCallSites` suites.

## Claims not backed by the current code

- Ledger "A broken schedule says why": true for the API and the projection, **not** for the
  operator — `SystemPage.tsx:650` still has no `last_error` column, so the console still shows a
  schedule that silently never runs.
- Ledger "The request detail now explains the request": true at the API
  (`operations_api.rs:496-515`), but the console still renders `attemptsUnavailable` and
  `costComponentUnavailable` and `RequestDetail` does not carry the new fields
  (`OperationsPage.tsx:439-442`, `web/src/api.ts:19`). The operator-visible gap is unchanged.
- Ledger "The project's default routing policy is now writable": true at the API
  (`operations_api.rs:683`, test `operations.rs:343`), but the console form submits no `routing`
  field (`SystemPage.tsx:268`) and the field is `#[serde(default)]` (`:573`) — a save from the
  console silently resets an existing routing document to `{"version":1}`. This is a new defect.
- Ledger "Bulk enable/disable covers prompts and protection rules": true at the route
  (`operations_api.rs:1157-1158`, test `operations.rs:614`), but the only bulk panel in the console
  is on the channels page and its resource union is `channels | credentials`
  (`ChannelsPage.tsx:188`), so a prompts operator cannot invoke it.
- The ledger's closing "Open defect with a confirmed root cause" (the projection freezing at the
  first console read) is stale: reads now run on the writer's own connection through
  `Command::Read` (`src/observability.rs:100-121`), and the only `Connection::open` calls are the
  writer thread's (`:215`) and the store constructor's (`:554`).
- `docs/catalog.md:7` claims creating a model applies catalog capabilities and USD-per-million
  defaults when the administrator omitted them; the reachable route defaults capabilities to
  `["chat"]` and both prices to 0 (`operations_api.rs:957-960,982`) and the catalog-aware
  `db::create_model` is still `#[allow(dead_code)]` (`src/db.rs:322`).
- `SystemPage.tsx:412` still states "The API has no artifact list route"; the route exists
  (`operations_api.rs:64-71`).
