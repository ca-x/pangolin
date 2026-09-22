# AxonHub parity completion plan

Ordered, independently reviewable implementation batches for the remaining parity
work. Derived from `docs/reviews/parity-ledger.md` (reconciled 2026-09-22) and the
113-item capability-matrix disposition in the same file. Read `AGENTS.md` first; it
is binding.

## How to use this plan

- **One batch = one dim implementation session.** A batch is sized so a single
  session can finish it, run its own gates, and write its own report without
  touching the next batch's files.
- **Do not combine unrelated backend security, data migration and visual work.**
  Where a batch spans layers it is because they are one coherent behaviour (a
  projection change plus the console column that renders it), never because two
  independent problems happened to be nearby.
- **Every batch lands its own red/green evidence.** A test written after the fix is
  not evidence; say so explicitly when a case cannot be made to fail first.
- **Every batch rebuilds what it invalidates.** A console change needs
  `pnpm --dir web build` before the release binary is rebuilt; a projection change
  needs its `SCHEMA_VERSION` bump.
- Batch ids are stable: `B1…B66`. A batch may be split if it turns out larger than
  one session; the split keeps the original id with a letter suffix.

## Verification commands (the standard gate set)

```bash
pnpm --dir web install --frozen-lockfile   # fresh checkout only
pnpm --dir web lint
pnpm --dir web test
pnpm --dir web build
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked             # after web build, before browser QA
```

Per-batch entries below add the focused commands; they never replace the gate set.

---

## Wave 1 — truth and identity corrections (frontend / comments only)

Small, independent, and first: the console's own truthfulness should not still be
moving while new surfaces are added.

### B1 — Reproduce-first measurement pass (C7 contrast, C8 375px clipping)

Scope: ledger C7 and C8. Evidence batch: **no product change unless a measurement
proves a failure.**
Source: `web/src/consoleFixes.test.tsx` (WCAG computation from CSS tokens),
`web/src/components.tsx` (`EnabledPill`), `web/src/pages/ChannelsPage.tsx`
(`HealthPill`), `web/src/styles.css`.
Tests: extend `consoleFixes.test.tsx` only if the ratio is reproducible from source;
otherwise say plainly that it is not and record the browser measurement.
Acceptance: for each of the success/ENABLED badge and the health-failure pill, a
measured ratio in light and dark, from the release binary (rebuild web first) — or
a statement that the ratio cannot be computed from source plus the axe measurement.
Change a colour only if the measured ratio is below 4.5:1 for normal text. For
clipping: 375 px, `en`, populated request list, channels and models tables — either
a screenshot showing no clipping and no page-level horizontal scroll, or the
reproduced defect with its exact element.
Depends on: nothing.
Verify: `pnpm --dir web build && cargo build --release --locked`, then the browser
pass; `pnpm --dir web test` if a source-level guard was added.

### B2 — Access project-switch loading flash (C1)

Scope: ledger C1, `access control` rows 3–5 neighbourhood.
Source: `web/src/project.tsx` (permission query state), `web/src/pages/AccessPage.tsx`
(`active`/`visible` derivation and the no-access card).
Tests: `web/src/pages/accessMembers.test.tsx` or a new two-project case.
Acceptance: while the next project's permissions are in flight the page shows a
loading state, does not render the no-access card, and preserves panel state
(`bindingUser`, accordion); after the load the new project's tabs render.
Depends on: nothing.
Verify: `pnpm --dir web test -- src/pages/accessMembers.test.tsx src/pages/roleBindings.test.tsx`; `pnpm --dir web lint`.

### B3 — One "saved but the list could not be re-read" result (C2)

Scope: ledger C2.
Source: `web/src/pages/AccessPage.tsx` (`KeysPanel` bulk `onSuccess`), `web/src/i18n.ts`.
Tests: `web/src/pages/keyBulkSelection.test.tsx` with `sonner` mocked.
Acceptance: a successful action whose refetch fails produces exactly one localized
message naming both facts; the selection is kept; a plain failure still reports once.
Depends on: nothing.
Verify: `pnpm --dir web test -- src/pages/keyBulkSelection.test.tsx`; `pnpm --dir web lint`.

### B4 — Internal-id links in TraceTimeline and Playground (C3, C4)

Scope: ledger C3, C4.
Source: `web/src/pages/OperationsPage.tsx` (`TraceTimeline` row `href`),
`web/src/pages/PlaygroundPage.tsx` (the `viewRequest` link).
Tests: `web/src/pages/requestIdentity.test.tsx`, `web/src/pages/OperationsPage.test.tsx`
(the case that currently pins `public_id`), `web/src/pages/playgroundUsage.test.tsx`.
Acceptance: both links use the internal request id the bundle/record already
carries; a project that repeats an external id cannot make either link open the
wrong row; the external id stays visible as a column/fact.
Depends on: nothing.
Verify: `pnpm --dir web test -- src/pages/requestIdentity.test.tsx src/pages/OperationsPage.test.tsx src/pages/playgroundUsage.test.tsx`; `pnpm --dir web lint`.

### B5 — Routing form guard and harness root cause (C6)

Scope: ledger C6, `models, routing & pricing` row 13's missing guard.
Source: `web/src/pages/SystemPage.tsx` (the orchestration panel and its tab),
`web/src/pages/SystemPage.test.tsx`.
Tests: new cases in `SystemPage.test.tsx`.
Acceptance: a test activates the `Orchestration settings` tab (the pattern the
Backups cases already use), asserts the routing control renders with the stored
document, edits it and asserts the submitted body carries `routing`; the test fails
if the field is removed from the submit body. Record in the report why the earlier
attempt saw nothing: the panel is in a `keepMounted={false}` tab whose default is
`appearance`.
Depends on: nothing.
Verify: `pnpm --dir web test -- src/pages/SystemPage.test.tsx`; `pnpm --dir web lint`.

### B6 — Correct the D1 test comment (C5)

Scope: ledger C5. Comment-only change in a Rust test file; no behaviour change.
Source: `src/api/gateway/tests/operations.rs` (the comment above
`an_invalid_request_logging_level_or_type_is_rejected`).
Tests: the existing case, unchanged and still green.
Acceptance: the comment states which shape silently defaulted (an unknown *field*
name, which deserialized to the default policy) and which was rejected outside the
envelope (an unknown enum value, answered 422 by the extractor) — matching D1's own
RED evidence, not the current wording.
Depends on: nothing.
Verify: `cargo test --locked an_invalid_request_logging_level_or_type_is_rejected`; `cargo fmt --all -- --check`.

---

## Wave 2 — observability completion

### B7 — Request-list telemetry columns (observability 7)

Scope: tokens, resolved model, TTFT and the stream flag in the list row and table.
Source: `src/observability.rs` (list payload, DuckDB shape, `SCHEMA_VERSION`),
`web/src/api.ts` (`RequestItem`), `web/src/pages/OperationsPage.tsx`.
Tests: a `src/observability.rs` list case; `web/src/pages/observabilityWiring.test.tsx`.
Acceptance: the row carries input/output/cache/reasoning tokens, `resolved_model`,
`ttft_ms` and the stream flag; the table renders tokens and the requested→resolved
pair; an unmeasured value prints `—`, never `0`; the projection shape change rebuilds
from the record system once.
Depends on: nothing.
Verify: `cargo test --locked observability::`; `pnpm --dir web test -- src/pages/observabilityWiring.test.tsx`.

### B8 — Overview window selector and token/cost trend series (observability 2, 3)

Scope: the dashboard's own time window and the missing series.
Source: `src/observability.rs` (`summary_for`, `SummaryPoint`), `web/src/api.ts`,
`web/src/pages/OverviewPage.tsx`.
Tests: `src/observability.rs` summary cases; `web/src/pages/trendChart.test.tsx`,
`web/src/pages/observabilityWiring.test.tsx`.
Acceptance: the window selector changes the summary *and* the breakdown query; the
trend carries token and cost series with their own scale; a single bucket still
renders as a level line; unmeasured buckets print `—` in the disclosure table.
Depends on: nothing.
Verify: `cargo test --locked observability::`; `pnpm --dir web test -- src/pages/trendChart.test.tsx src/pages/observabilityWiring.test.tsx`.

### B9 — Trace list columns and trace-attempt provider identity (observability 12, small)

Scope: `external_id`, request count and first user query in the traces list; the
trace bundle's executions should name the channel the way the request detail does.
Source: `src/api/operations_api.rs` (traces projection, trace-detail executions),
`web/src/pages/OperationsPage.tsx`.
Tests: a `src/api/gateway/tests/operations.rs` trace case; `web/src/pages/observabilityWiring.test.tsx`.
Acceptance: the list renders the client trace id, a per-trace request count and the
first user query (or `—` when no payload was captured); the trace's attempt rows
show the frozen `provider_name`, not a provider id.
Depends on: nothing.
Verify: `cargo test --locked trace`; `pnpm --dir web test -- src/pages/observabilityWiring.test.tsx`.

### B10 — Per-trace archive / pin lifecycle (observability 15)

Scope: trace-level retention state, the control-plane mutation with audit, and the
list actions.
Source: `src/db/schema.rs` (additive migration), `src/api/operations_api.rs`,
`web/src/pages/OperationsPage.tsx`, `web/src/i18n.ts`.
Tests: a Rust HTTP case per action plus an audit assertion; a web list-action case.
Acceptance: archive/unarchive/retain/unretain are project-scoped, audited in the
same transaction, refused for a foreign trace with 404, and the list shows the state
with the actions; retention GC respects the pinned state.
Depends on: nothing (additive migration only).
Verify: `cargo test --locked operations::`; `pnpm --dir web test -- src/pages/observabilityWiring.test.tsx`; `cargo fmt`/`clippy`.

### B11 — Client and network identity in the request detail (observability 11)

Scope: project the stored client IP (and user agent if the policy keeps one) into
the request detail.
Source: `src/observability.rs` (event/projection), `src/api/operations_api.rs`
(requests projection), `web/src/api.ts`, `web/src/pages/OperationsPage.tsx`,
`docs/operations.md` (privacy statement).
Tests: a Rust projection case; a web detail case.
Acceptance: the detail shows the recorded client IP with `—` when unrecorded; the
logging policy decision is stated in `docs/operations.md` (metadata is not body
content); no header or credential is projected.
Depends on: the privacy decision recorded in `docs/operations.md` (part of this batch).
Verify: `cargo test --locked observability::`; `pnpm --dir web test -- src/pages/requestAttempts.test.tsx`.

### B12 — Payload viewer over the stored body (observability 10)

Scope: conversation/chunk rendering, JSON view and a curl-style preview.
Source: `web/src/pages/OperationsPage.tsx`, `web/src/i18n.ts`; a chunk read route in
`src/api/operations_api.rs` only if the stored body cannot be parsed client-side.
Tests: a new `web/src/pages/payloadViewer.test.tsx`.
Acceptance: an SSE body renders as ordered chunks with the terminal event marked; a
non-stream body renders as a conversation when it has a message shape and as JSON
otherwise; the raw JSON view and download remain; a capture-off project still shows
the policy alert.
Depends on: nothing.
Verify: `pnpm --dir web test -- src/pages/payloadViewer.test.tsx`; `pnpm --dir web lint`.

### B13 — The generic resource list honours `from`/`until` (observability 2 remainder)

Scope: the shared list endpoint's time window.
Source: `src/api/operations_api.rs` (`list`/`Filter`), `web/src/pages/shared.tsx` if a
control is exposed.
Tests: a Rust case asserting a windowed list and a matching total.
Acceptance: `from`/`until` filter the list and the count describes the same window;
a resource without a time column ignores the parameters rather than erroring; the
console passes them where it already has a window control.
Depends on: nothing.
Verify: `cargo test --locked operations::`; `cargo fmt`/`clippy`.

---

## Wave 3 — prompts and playground

### B14 — Prompt record parity: `order`, `action`, and a disabled default (prompts 5, 6, 7)

Scope: one coherent change to the prompt record and its injection placement.
Source: `src/db/schema.rs` (one additive migration: `order`, `action`),
`src/orchestration/protection.rs` (ordering and append placement in all four
protocols), `src/api/operations_api.rs` (write path, validation, default),
`web/src/pages/PromptsPage.tsx` (form + columns), `web/src/i18n.ts`.
Tests: `src/orchestration/tests.rs` injection cases (order, append per protocol);
a Rust HTTP case asserting a created prompt is disabled; `web/src/pages/promptSummary.test.tsx`.
Acceptance: prompts inject by `order` then `created_at`; `append` places content
after the conversation for chat/messages/Gemini/responses; a new prompt is created
disabled on all three layers (DB default, API default, console default); a malformed
activation is still refused with 400.
Depends on: nothing.
Verify: `cargo test --locked protection`; `pnpm --dir web test -- src/pages/promptSummary.test.tsx`.

### B15 — Protection rule preview and metadata (prompts 11)

Scope: a preview endpoint over sample text plus `description` and an archived state.
Source: `src/orchestration/protection.rs` (reusable matcher), `src/api/operations_api.rs`
(preview route), `src/db/schema.rs` (additive columns), `web/src/pages/PromptsPage.tsx`
(preview dialog, form fields), `web/src/i18n.ts`.
Tests: a Rust preview case over fixtures; a web dialog case.
Acceptance: the preview returns, per rule, whether it matched and what the redacted
result would be, without contacting a provider and without persisting anything; the
dialog shows it in the active language; `archived` rules are excluded from
enforcement but remain listed.
Depends on: nothing.
Verify: `cargo test --locked protection`; `pnpm --dir web test -- src/pages/promptSummary.test.tsx`.

### B16 — Endpoint-filtered playground picker (prompts 14)

Scope: the picker must list only models the playground's endpoint can reach.
Source: `web/src/pages/PlaygroundPage.tsx`, `web/src/i18n.ts`; server-side filtering
in `src/orchestration/mod.rs`/`src/api.rs` only if `/v1/models` cannot express it.
Tests: `web/src/pages/playgroundPicker.test.tsx`.
Acceptance: a channel whose capabilities refuse `/v1/responses` does not appear; an
empty list says why and offers a retry; the chosen model is the one sent.
Depends on: nothing.
Verify: `pnpm --dir web test -- src/pages/playgroundPicker.test.tsx`; `pnpm --dir web lint`.

### B17 — Session-authenticated admin chat playground (prompts 13)

Scope: multi-turn chat on the console session: history, streaming, reasoning parts,
attachments, regenerate/clear/stop, channel-or-model source, system prompt,
temperature — reusing the gateway pipeline instead of a pasted key.
Source: `src/api.rs`/`src/api/gateway.rs` (a session-authorized chat entry that runs
the same orchestration), `web/src/pages/PlaygroundPage.tsx`, `web/src/i18n.ts`.
Tests: Rust cases proving the console session is authorized through the same
policy/admission/accounting path as an API key; web cases for each control.
Acceptance: a console session can chat without pasting a key; every request still
passes project/key policy, model access, admission, retry/circuit, trace and
terminal accounting; no new bypass of `AGENTS.md`'s gateway invariants.
Depends on: B16 (picker), and the C2 terminal/usage work already landed.
Verify: `cargo test --locked gateway`; `pnpm --dir web test`; full browser pass.

---

## Wave 4 — access control

### B18 — Members: in-place edit and the shared table shell (access 3, 4)

Scope: a row action that changes role/status in place, plus search, pagination and
mobile cards through the shared shell.
Source: `web/src/pages/AccessPage.tsx` (`MembersPanel`), `web/src/i18n.ts`.
Tests: `web/src/pages/accessMembers.test.tsx`.
Acceptance: the row action reuses `POST /members` and refreshes the roster; the
owner's row still offers no action that would be refused; search/pagination/mobile
behaviour matches the other Access tabs; listing still needs only `project:read`.
Depends on: B2 (the same panel's loading state).
Verify: `pnpm --dir web test -- src/pages/accessMembers.test.tsx`.

### B19 — Key creation: type, owner and scopes (access 9)

Scope: stop hardcoding `key_type: 'service'` and `scopes: ['gateway:use']`, and show
scopes in the table.
Source: `web/src/pages/AccessPage.tsx`, `src/access.rs` (validation already exists),
`web/src/i18n.ts`.
Tests: `web/src/pages/accessKeyState.test.tsx` plus a new creation case.
Acceptance: the form offers the four types the backend accepts and requires an owner
for `user`/`personal`; an invalid scope is refused inline with the server's reason;
the table shows the key's scopes; an API-key principal cannot escalate by creating a
key it could not have created through the API.
Depends on: nothing.
Verify: `pnpm --dir web test -- src/pages/accessKeyState.test.tsx`; `cargo test --locked access::`.

### B20 — Per-key usage and cost from the key row (access 11)

Scope: reach the key's own usage from the key, not only from the Overview breakdown.
Source: `web/src/pages/AccessPage.tsx`, `/analytics?dimension=api_key` (exists).
Tests: a new key-usage case in `web/src/pages/accessKeyState.test.tsx` or a sibling file.
Acceptance: the row opens usage for that key's id only, with tokens/cost and `—`
when unmeasured; a foreign key id is refused.
Depends on: nothing.
Verify: `pnpm --dir web test -- src/pages/accessKeyState.test.tsx`.

### B21 — Key rotate and archive (access 12 remainder)

Scope: rotate and archive routes plus bulk archive, on top of the existing bulk
enable/disable.
Source: `src/access.rs` (rotate/archive with the owner-membership rule), `src/access_api.rs`,
`web/src/pages/AccessPage.tsx`, `web/src/i18n.ts`.
Tests: Rust cases proving the old token stops authenticating after a rotate and that
an archived key cannot authenticate or be re-enabled; web cases for the actions.
Acceptance: rotate issues a new token once and never returns the old one; archive is
audited; bulk archive skips foreign ids and applies the same owner rule as the
single-key path.
Depends on: nothing.
Verify: `cargo test --locked access::`; `pnpm --dir web test -- src/pages/keyBulkSelection.test.tsx`.

### B22 — OIDC sign-in button (access 6)

Scope: the console entry point for the unauthenticated provider list that already
exists.
Source: `web/src/Auth.tsx`, `web/src/i18n.ts`.
Tests: a new case in `web/src/Auth` coverage (e.g. `web/src/App.test.tsx`).
Acceptance: one button per enabled provider, each following
`GET /api/v1/auth/oidc/{provider_id}/start`; a provider list that cannot be read
leaves password login untouched and says nothing untrue; 375 px layout holds.
Depends on: nothing.
Verify: `pnpm --dir web test`; `pnpm --dir web lint`.

### B23 — OIDC identities: list, unlink, self-service link (access 7, 8)

Scope: replace the create-only panel and add a signed-in link flow.
Source: `src/access_api.rs` (existing `GET`/`DELETE`), `web/src/pages/AccessPage.tsx`,
`web/src/Auth.tsx` or a settings entry for the self-service flow, `web/src/i18n.ts`.
Tests: `web/src/pages/AccessPage.test.tsx` plus a link-flow case.
Acceptance: identities are listed with their subject and user, unlink is confirmed
and audited; the self-service flow starts from the signed-in user and refuses to
link a subject already bound to another user.
Depends on: B22.
Verify: `pnpm --dir web test -- src/pages/AccessPage.test.tsx`; `cargo test --locked oidc`.

### B24 — OIDC login-only mode and branding (access 14)

Scope: provider `login_only`, `display_name`, `button_color`, `icon_url` plus the
sign-in enforcement.
Source: `src/db/schema.rs` (additive migration), `src/oidc.rs`, `src/access_api.rs`,
`web/src/Auth.tsx`, `web/src/pages/AccessPage.tsx`, `web/src/i18n.ts`.
Tests: a Rust case proving local password login is refused when every enabled
provider is login-only; a web case for the branding fields.
Acceptance: login-only is enforced server-side (not only hidden in the UI); branding
fields are validated and rendered; the icon is a bundled/allow-listed asset, never a
copied trademark.
Depends on: B22.
Verify: `cargo test --locked oidc`; `pnpm --dir web test`.

### B25 — Permission catalog and role picker (access 15)

Scope: a read-only catalog route and a picker in the role editor.
Source: `src/access_api.rs` (catalog route over the seeded table), `web/src/pages/AccessPage.tsx`.
Tests: a Rust route case (authorization and shape); a web picker case.
Acceptance: the route returns slug/level/description for the actor's scope; the role
form picks from it and still submits a plain permission list; a role edit cannot
grant a scope the actor does not hold.
Depends on: nothing.
Verify: `cargo test --locked access::`; `pnpm --dir web test`.

### B26 — Key-profile templates (access 18)

Scope: save/load/transfer a profile's model mappings, allowed models, quotas and
routing policy.
Source: `src/access.rs`/`src/api/operations_api.rs` (template resource),
`web/src/pages/AccessPage.tsx`, `web/src/i18n.ts`.
Tests: a Rust case proving a template round-trips and cannot cross projects; a web
case for save/load.
Acceptance: a template is project-scoped, audited, validated like the profile it
came from, and loading one is a normal profile write that cannot exceed the actor's
own authority.
Depends on: nothing.
Verify: `cargo test --locked access::`; `pnpm --dir web test`.

### B27 — Invitation reuse (access 13)

Scope: **decision first** (ledger *Decisions required*): implement `max_uses` or
record the single-use/email-bound bound as a deliberate divergence (D5).
Source: `src/access.rs` (`InvitationInput`, acceptance), `web/src/pages/AccessPage.tsx`,
`docs/reviews/parity-ledger.md` (divergence row if that is the decision).
Tests: if implemented, a Rust case for a two-use invite and one for an exhausted
invite; if diverged, no test.
Acceptance: either the multi-use path is enforced server-side with an audit row per
acceptance, or the ADR records the decision and the ledger row moves to `divergence`.
Depends on: the user's decision.
Verify: `cargo test --locked access::`; `pnpm --dir web test`.

---

## Wave 5 — models, routing and pricing

### B28 — Catalog cards applied to console-created models (models 9)

Scope: a catalog picker in the model form and server-side application of
capabilities, cost defaults and card metadata.
Source: `src/db.rs` (the catalog-aware creation that is currently dead code),
`src/api/operations_api.rs` (the reachable write path), `web/src/pages/ModelsPage.tsx`.
Tests: a Rust case proving a catalog-created model carries capabilities and both
default prices; a web case driving the picker.
Acceptance: a model created from a catalog card is priced and capable as the catalog
says, and a model created without one still works; the dead code path is either used
or deleted, never left as an unused twin.
Depends on: nothing.
Verify: `cargo test --locked catalog`; `pnpm --dir web test -- src/pages/ModelsCatalog.test.tsx`.

### B29 — Model card fields as columns (models 10, read half)

Scope: surface developer, type, icon, limits and cost defaults from
`catalog_metadata_json`.
Source: `src/api/operations_api.rs` (models projection), `web/src/pages/ModelsPage.tsx`.
Tests: `web/src/pages/ModelsPage.test.tsx`.
Acceptance: a model with a card shows its developer/type/limits/cost; a model without
one shows `—` rather than invented values; the list stays readable at 375 px.
Depends on: nothing.
Verify: `pnpm --dir web test -- src/pages/ModelsPage.test.tsx`.

### B30 — Model archive lifecycle (models 10, lifecycle half; models 20 preview)

Scope: an archived state for models with an impact preview before a destructive
delete.
Source: `src/db/schema.rs` (additive migration), `src/api/operations_api.rs`,
`src/orchestration/repository.rs` (archived models must not be routable),
`web/src/pages/ModelsPage.tsx`.
Tests: a Rust case proving an archived model is not routable and that the preview
lists what a delete would cascade; a web case for the action.
Acceptance: archive is audited and reversible; a delete of an archived model with
price history still answers a typed 409; the preview names the dependent rows.
Depends on: nothing.
Verify: `cargo test --locked operations::`; `pnpm --dir web test`.

### B31 — `/v1/models` extended metadata (models 11)

Scope: `include=all` (or an always-on subset) built from catalog card metadata.
Source: `src/orchestration/mod.rs`, `src/api.rs`.
Tests: a Rust contract case for the extended document and for the default shape.
Acceptance: the default response is unchanged for existing clients; `include=all`
adds card metadata only for models whose card exists; a hidden or unroutable model is
still absent.
Depends on: B29 (the projection shape it reads).
Verify: `cargo test --locked models`.

### B32 — Global model settings document and form (models 12)

Scope: `fallback_to_channels_on_model_not_found`, `query_all_channel_models`,
`default_model_api_include_all`, `auto_reasoning_effort`, `model_blacklist_regex`,
`hide_unroutable_models_in_list`.
Source: `src/api/operations_api.rs` (instance settings document with a strict write
contract like `PolicyInput`), `src/orchestration/mod.rs` (consumption),
`web/src/pages/SystemPage.tsx`, `web/src/i18n.ts`.
Tests: a Rust case per knob's observable effect plus a strict-write case; a web form case.
Acceptance: every knob changes the observable behaviour it names; a typo in the
document is refused with the envelope and nothing is written; a stored document with
an unknown field cannot break routing.
Depends on: B31 for the include-all knob.
Verify: `cargo test --locked operations::`; `pnpm --dir web test -- src/pages/SystemPage.test.tsx`.

### B33 — Association condition builder and domain context fields (models 14)

Scope: `daily_time`, media presence, `stream` and `request_format` in the condition
context, plus a structured editor.
Source: `src/orchestration/policy.rs` (context + evaluator), `src/api/operations_api.rs`
(validation), `web/src/pages/ModelsPage.tsx` (builder), `web/src/i18n.ts`.
Tests: `src/orchestration/tests.rs` per new field; a web builder case.
Acceptance: each new field is expressible, validated at write time and evaluated per
request; the raw JSON box remains as an escape hatch; an unsupported field is refused
rather than silently ignored.
Depends on: nothing.
Verify: `cargo test --locked orchestration::`; `pnpm --dir web test -- src/pages/ModelsPage.test.tsx`.

### B34 — `channel_tags + regex` and association exclusions (models 15)

Scope: a tag+regex match type and channel-name/id/tag exclusion lists.
Source: `src/orchestration/repository.rs`, `src/api/operations_api.rs`, `web/src/pages/ModelsPage.tsx`.
Tests: candidate-generation cases for the new type and for each exclusion form.
Acceptance: a regex over channel tags matches without requiring an exact tag; an
excluded channel is never a candidate even when another association matches it; the
console can express both.
Depends on: B33 (the same editor).
Verify: `cargo test --locked orchestration::`.

### B35 — Auto-trim, hide-original/hide-mapped and a mapping editor (models 16)

Scope: the three transformation flags plus a structured channel mapping editor.
Source: `src/orchestration/repository.rs` (`resolve_model`, `model_rules`),
`src/api/operations_api.rs` (validation), `web/src/pages/ChannelsPage.tsx`, `web/src/i18n.ts`.
Tests: resolution cases per flag; a web editor case.
Acceptance: auto-trim strips only the configured prefixes; hidden originals/mapped
names do not appear in `/v1/models` while remaining routable; the editor produces the
same document the write path accepts.
Depends on: nothing.
Verify: `cargo test --locked orchestration::`; `pnpm --dir web test`.

### B36 — Volume (non-marginal) tier pricing mode (models 17)

Scope: a per-component tier mode where every unit bills at the matched tier rate.
Source: `src/operations/pricing.rs` (calculation and the pre-flight upper bound),
`src/api/operations_api.rs` (component validation), `web/src/pages/ModelsPage.tsx`.
Tests: pricing cases for volume vs marginal at a tier boundary and a bound case.
Acceptance: a volume component bills every unit at one rate; the budget bound no
longer over-estimates for that component; existing marginal prices are unchanged.
Depends on: nothing.
Verify: `cargo test --locked pricing`.

### B37 — Channel-scoped prices (models 18)

Scope: accept and edit `provider_id` on a price.
Source: `src/api/operations_api.rs` (price write path), `src/operations/pricing.rs`
(resolution already prefers a provider row), `web/src/pages/ModelsPage.tsx`.
Tests: a Rust case proving a channel-scoped rate wins for that channel only; a web case.
Acceptance: a channel-scoped price is creatable, listed with its channel and used
only for that channel; the global version index is unaffected.
Depends on: B38 (the read projection makes it visible).
Verify: `cargo test --locked pricing`; `pnpm --dir web test`.

### B38 — Price components read projection (models 24)

Scope: include a version's components (the actual rates) in the list projection.
Source: `src/api/operations_api.rs` (`prices` projection), `web/src/pages/ModelsPage.tsx`.
Tests: a Rust projection case; a web case rendering the rates.
Acceptance: every version's components, tiers and cache TTL variants are visible and
match the stored rows; an unmeasurable field prints `—`.
Depends on: nothing.
Verify: `cargo test --locked operations::`; `pnpm --dir web test`.

### B39 — Full price editor (models 26)

Scope: tier, schedule and timezone editing.
Source: `web/src/pages/ModelsPage.tsx` (editor components), `web/src/i18n.ts`.
Tests: `web/src/pages/ModelsPage.test.tsx` per editor.
Acceptance: the editor produces exactly the document the write API accepts,
including IANA timezone, weekday mask, daily window, date range and priorities; an
invalid document is refused inline before the request; append-only versioning is
preserved.
Depends on: B38.
Verify: `pnpm --dir web test -- src/pages/ModelsPage.test.tsx`.

### B40 — Service-group editor (models 19)

Scope: a console surface for `resource="groups"`.
Source: `web/src/pages/ModelsPage.tsx` or `AccessPage.tsx`, `web/src/i18n.ts`.
Tests: a web case for the ratio and channel assignment.
Acceptance: the ratio is settable and applied per request (the multiplier already
exists); the group's channel list is editable; a group write is audited.
Depends on: nothing.
Verify: `pnpm --dir web test`.

### B41 — Batch model create and bulk delete/archive (models 22)

Scope: batch create (import from catalog) plus bulk delete/archive on the models table.
Source: `src/api/operations_api.rs` (batch create, bulk archive), `web/src/pages/ModelsPage.tsx`.
Tests: a Rust batch case asserting per-row validation and no partial write; web cases.
Acceptance: a batch with one invalid row is refused as a whole with the row named; a
bulk archive skips foreign ids; bulk delete still answers 409 for a model with
history.
Depends on: B30 (archive lifecycle).
Verify: `cargo test --locked operations::`; `pnpm --dir web test -- src/pages/bulkSelection.test.tsx`.

### B42 — Unassociated-model detection (models 23)

Scope: a query for channel models with no matching enabled association, surfaced on
the routing tab.
Source: `src/api/operations_api.rs`, `web/src/pages/ModelsPage.tsx`, `web/src/i18n.ts`.
Tests: a Rust case with one associated and one unassociated model; a web case.
Acceptance: the list names the unassociated models and links to the association
editor; an empty result is an explicit empty state.
Depends on: nothing.
Verify: `cargo test --locked orchestration::`; `pnpm --dir web test`.

### B43 — Per-channel proxy configuration (models 21)

Scope: HTTP/SOCKS URL, credentials, connection-reuse policy per channel.
Source: `src/db/schema.rs` (additive column in `channel_settings`), `src/providers/upstream.rs`
(per-channel client), `src/api/operations_api.rs` (validation + encrypted secret),
`web/src/pages/ChannelsPage.tsx`, `web/src/i18n.ts`.
Tests: a Rust case proving the channel's proxy is used and the credential is not
returned; a web form case.
Acceptance: the proxy is per channel, its password is encrypted and never returned,
the global client stays the default, and a malformed URL is refused with 400.
Depends on: nothing.
Verify: `cargo test --locked providers::`; `pnpm --dir web test`.

### B44 — Provider model discovery and sync (models 25)

Scope: fetch a provider's models, apply them, and schedule the sync.
Source: `src/catalog/types.rs` (stop forcing `implemented=false` for discovery),
`src/providers/*` (a discovery call per adapter), `src/operations/runtime.rs`
(a `model_sync` job), `src/api/operations_api.rs`, `web/src/pages/ChannelsPage.tsx`.
Tests: mock-upstream discovery cases; a scheduled-sync case; a web case for the result.
Acceptance: discovery works against a mock provider, manual models are never deleted
by a sync, a failed sync records its error on the channel, and the job is bounded and
idempotent.
Depends on: nothing.
Verify: `cargo test --locked catalog`; `cargo test --locked operations::`; `pnpm --dir web test`.

---

## Wave 6 — system and platform

### B45 — Request-logging policy `version` and startup validation (system 11)

Scope: version the stored document and validate the row at startup.
Source: `src/operations/logging.rs`, `src/main.rs` (startup check), `src/db/schema.rs`
if a migration is needed, `docs/operations.md`.
Tests: a case proving a malformed stored row is reported at startup and cannot take
the gateway down; a case proving a versioned document round-trips.
Acceptance: the document carries `version`; an unknown version is handled explicitly
(refused or upgraded, never silently parsed); the strict-write/tolerant-read asymmetry
is preserved.
Depends on: nothing.
Verify: `cargo test --locked logging`; `cargo test --locked operations::`.

### B46 — Schedule cron, time-of-day and timezone (system 13)

Scope: express "back up every day at 02:00 in this zone".
Source: `src/operations/schema.sql`/`src/db/schema.rs`, `src/operations/jobs.rs`
(next-run computation), `src/api/operations_api.rs` (validation with the existing
`cron`/`chrono-tz` dependencies), `web/src/pages/SystemPage.tsx`, `web/src/i18n.ts`.
Tests: next-run cases for a daily time across a DST boundary and for an invalid zone;
a web form case.
Acceptance: the interval form still works; an anchored daily time in an IANA zone
computes the next run correctly; an invalid expression is refused with 400; a broken
schedule still reports `last_error`.
Depends on: nothing.
Verify: `cargo test --locked operations::`; `pnpm --dir web test -- src/pages/backupScheduleRetention.test.tsx`.

### B47 — Instance general settings: currency and timezone (system 22, observability 14)

Scope: **decision first** (ledger *Decisions required*), then the setting.
Source: `src/api/operations_api.rs` (instance settings document), `web/src/observability.tsx`
(`formatMicros`), `web/src/pages/SystemPage.tsx`, `web/src/i18n.ts`.
Tests: a formatter case per currency; a settings round-trip case.
Acceptance: cost formatting follows the instance currency (or the USD-only boundary
is recorded in the ledger and `docs/operations.md` and the row moves to `divergence`);
an unset value keeps today's behaviour; no stored amount changes.
Depends on: the user's decision.
Verify: `pnpm --dir web test -- src/observability*`; `cargo test --locked operations::`.

### B48 — Instance retry and upstream-error policy (system 23)

Scope: **decision first**, then promote the chosen knobs from per-channel JSON to an
instance default with per-channel override.
Source: `src/db/schema.rs`/`src/api/operations_api.rs`, `src/orchestration/mod.rs`,
`web/src/pages/SystemPage.tsx`.
Tests: a case per knob proving the instance default applies when a channel does not
override it; a case proving an override still wins.
Acceptance: the instance default never weakens the no-retry-after-commit rule; a
channel override is preserved; the settings document has a strict write contract.
Depends on: the user's decision.
Verify: `cargo test --locked orchestration::`; `pnpm --dir web test -- src/pages/SystemPage.test.tsx`.

### B49 — Quota collection toggle and routing mode (system 24)

Scope: **decision first**, then a collection toggle and a routing mode.
Source: `src/orchestration/repository.rs` (quota filtering), `src/operations/runtime.rs`
(collection jobs), `src/api/operations_api.rs`, `web/src/pages/SystemPage.tsx`.
Tests: a candidate case per routing mode; a case proving collection off stops
snapshot writes without stopping traffic.
Acceptance: `REMOVE_ON_EXHAUSTED` remains the default and today's behaviour; another
mode is selectable and observable in the routing preview; collection off degrades
observability only.
Depends on: the user's decision.
Verify: `cargo test --locked orchestration::`; `pnpm --dir web test`.

### B50 — Storage: GCS/WebDAV and a connection test (system 18 remainder)

Scope: two more backends and a test-connection action.
Source: `src/operations/storage.rs` (`Config`), `src/operations/backup.rs`,
`src/api/operations_api.rs` (test route), `web/src/pages/SystemPage.tsx`, `web/src/i18n.ts`.
Tests: a Rust case per backend using a mock endpoint; a case proving the test action
never writes an object and never returns the credential.
Acceptance: each backend round-trips a backup to a mock; the connection test reports
success/failure with a bounded, non-leaking message; an unknown kind is refused.
Depends on: nothing.
Verify: `cargo test --locked backup`; `pnpm --dir web test -- src/pages/storageTargetForm.test.tsx`.

### B51 — Per-resource restore strategy (system 19)

Scope: **decision first**, then per-resource-class strategies.
Source: `src/operations/backup.rs`, `src/api/operations_api.rs`, `web/src/pages/SystemPage.tsx`.
Tests: a restore case with two classes and different strategies; a case proving the
default is unchanged when none is given.
Acceptance: each class applies its own strategy; a foreign artifact is still refused;
secrets are preserved under every strategy.
Depends on: the user's decision.
Verify: `cargo test --locked backup`; `pnpm --dir web test -- src/pages/SystemPage.test.tsx`.

### B52 — Diagnostics: cache diagnostics export and clear cache (system 25)

Scope: **decision first** (low value — Pangolin's caches rebuild from SQLite).
Source: `src/observability.rs`/`src/orchestration/runtime.rs`, `src/api/operations_api.rs`,
`web/src/pages/SystemPage.tsx`.
Tests: a case proving a clear-cache request re-derives from the record system and
cannot grant access or change routing.
Acceptance: the export contains counts and shape versions, never payloads or
credentials; clearing the cache leaves the record system untouched and the gateway
serving; the action is audited and owner-only.
Depends on: the user's decision.
Verify: `cargo test --locked observability::`; `pnpm --dir web test`.

### B53 — Outbound proxy presets and per-webhook timeout/proxy (system 26)

Scope: reusable proxy presets referenced by channels and webhooks, plus a per-webhook
timeout and proxy.
Source: `src/db/schema.rs` (preset table), `src/operations/runtime.rs` (webhook
delivery), `src/catalog/refresh.rs` (the current `.no_proxy()`), `src/api/operations_api.rs`,
`web/src/pages/SystemPage.tsx`, `web/src/i18n.ts`.
Tests: a Rust case proving a preset is used by a webhook delivery and that the catalog
fetch keeps its no-proxy default unless configured; a web case.
Acceptance: presets are instance-scoped, their credentials are encrypted and never
returned; a webhook timeout is bounded; the catalog subscription path keeps DNS
pinning and no redirects.
Depends on: B43 (per-channel proxy plumbing).
Verify: `cargo test --locked operations::`; `pnpm --dir web test`.

### B54 — Provider OAuth credential helpers (matrix)

Scope: Codex, xAI, Claude Code, Antigravity and GitHub Copilot device-code setup
flows.
Source: new `src/oauth/` module behind a Pangolin interface, `src/api/operations_api.rs`
routes, `src/db/schema.rs` (credential type + token envelope), `web/src/pages/ChannelsPage.tsx`.
Tests: mock-IdP cases per flow (device-code poll lifecycle, PKCE where applicable);
a case proving no token is ever returned or logged.
Acceptance: each flow ends with an encrypted credential and no plaintext token in any
response, log or fixture; the device flow polls with a bounded interval and expires;
a failed exchange leaves no credential row.
Depends on: nothing.
Verify: `cargo test --locked oauth`; `pnpm --dir web test`.

### B55 — CORS and request timeouts (matrix)

Scope: operator-configurable CORS and request timeouts with safe defaults.
Source: `src/api.rs` (layers), `src/api/operations_api.rs` (settings document),
`web/src/pages/SystemPage.tsx`.
Tests: a Rust case per setting's observable effect; a case proving the default is
unrestricted-for-same-origin and bounded.
Acceptance: an allowed origin is echoed, a disallowed one is not; a timeout produces
the documented error envelope rather than a truncated body; no setting weakens the
CSRF guard.
Depends on: nothing.
Verify: `cargo test --locked gateway`; `pnpm --dir web test`.

### B56 — Live preview of in-flight requests (matrix)

Scope: an authorized snapshot of in-flight requests.
Source: `src/orchestration/runtime.rs` (in-flight registry), `src/api/operations_api.rs`
(read route), `web/src/pages/OperationsPage.tsx` (a tab or panel), `web/src/i18n.ts`.
Tests: a Rust case proving the snapshot is project-scoped and contains no payload;
a web case.
Acceptance: the snapshot names the model, channel, key and start time, never a
request or response body; it is empty when nothing is in flight (an explicit empty
state, not zeros); a foreign project's in-flight requests are invisible; the request
content policy gains the matching live-preview toggle and turning it off hides the
snapshot while leaving accounting facts intact.
Depends on: nothing.
Verify: `cargo test --locked gateway`; `pnpm --dir web test`.

### B57 — Analytics page with filters (matrix; observability 2 remainder)

Scope: a dedicated analytics surface with date and dimension filters, plus the
performance and cost breakdowns the dashboard does not carry.
Source: `web/src/pages/` (a new page), `web/src/Shell.tsx`, `src/api/operations_api.rs`
(`/analytics` already takes `from`/`until`/`dimension`), `web/src/i18n.ts`.
Tests: a new page test asserting the query parameters and the rendered dimensions.
Acceptance: date range and dimension filters change the query; TTFT/latency, throughput
(tokens/s) and cost views render with `—` when the projection cannot compute an average
or a rate, and no confidence figure is invented; filters are shareable in the URL.
Depends on: B8 (shared window control).
Verify: `pnpm --dir web test`; `cargo test --locked operations::`.

### B58 — Credential recovery path (matrix)

Scope: recover from an invalid or undecryptable credential envelope without leaking
plaintext.
Source: `src/access.rs`/`src/api/operations_api.rs` (credential state), `src/crypto`/
secret handling, `web/src/pages/ChannelsPage.tsx`, `web/src/i18n.ts`.
Tests: a Rust case proving an undecryptable credential is reported as such, cannot
authenticate, and can be replaced without exposing the stored bytes.
Acceptance: the console names the credential as unrecoverable and offers replacement;
the stored envelope is never returned; the failure never leaks the cause to a public
protocol response.
Depends on: nothing.
Verify: `cargo test --locked access::`; `pnpm --dir web test`.

### B59 — Developer settings document (matrix)

Scope: per-developer associations, inheritance control and reasoning-effort mapping.
Source: `src/api/operations_api.rs` (settings document), `src/orchestration/repository.rs`
(consumption), `web/src/pages/ModelsPage.tsx`, `web/src/i18n.ts`.
Tests: a candidate case per developer rule and per inheritance toggle.
Acceptance: a developer rule applies only to that developer's models; inheritance can
be turned off without losing the parent document; an unknown developer is preserved
rather than dropped.
Depends on: B33.
Verify: `cargo test --locked orchestration::`; `pnpm --dir web test`.

### B60 — Channel clone/merge and bulk delete (matrix)

Scope: the remaining channel lifecycle actions.
Source: `src/api/operations_api.rs`, `web/src/pages/ChannelsPage.tsx`, `web/src/i18n.ts`.
Tests: a Rust case proving a clone copies settings and models but no credential
secret, and that a merge is refused when names collide; web cases.
Acceptance: a clone never copies a credential secret; bulk delete answers 409 for a
channel with models or credentials and skips foreign ids; every action is audited.
Depends on: nothing.
Verify: `cargo test --locked operations::`; `pnpm --dir web test`.

### B61 — Provider quota normalization surface (matrix)

Scope: per-provider quota periods, URLs, snapshots and backoff, normalized the way
AxonHub does it.
Source: `src/orchestration/repository.rs` (quota filtering), `src/operations/runtime.rs`
(collection), `src/api/operations_api.rs`, `web/src/pages/ChannelsPage.tsx`.
Tests: a mock-provider case per normalization shape; a case proving an unknown shape
degrades to unmeasured instead of zero.
Acceptance: a quota snapshot names its period and remaining amount; an unparseable
provider response leaves the channel usable and the quota unmeasured.
Depends on: B49.
Verify: `cargo test --locked operations::`; `pnpm --dir web test`.

### B62 — Semantic-memory provider interface (matrix)

Scope: **decision first**. The compaction module documents an opt-in semantic
retrieval hook; nothing implements it.
Source: `src/orchestration/compaction.rs` (the interface), `src/api/operations_api.rs`
(opt-in setting), `web/src/pages/SystemPage.tsx`.
Tests: a case proving the default path never calls a memory provider and that an
opted-in failure falls back to exact replay.
Acceptance: exact encrypted replay remains the default; enabling memory is explicit
and per project; a provider failure never loses history or corrupts tool-call order.
Depends on: the user's decision.
Verify: `cargo test --locked compaction`.

### B63 — Scoped GraphQL endpoint (matrix, divergence D6)

Scope: **ADR first.** Either implement a scoped service GraphQL endpoint or record
the REST-only decision as divergence D6.
Source: `docs/adr/` (the ADR), then `src/api/` and the console if implemented.
Tests: if implemented, an authorization case per scope plus a playground case.
Acceptance: the ADR is written and linked from the ledger and the matrix row; if
implemented, the endpoint authorizes with the same permission model and never exposes
a mutation the REST surface refuses.
Depends on: the user's decision.
Verify: ADR review; if implemented, `cargo test --locked gateway`.

---

## Wave 7 — console-wide UX remainder

### B64 — Request-log URL persistence and multi-select facets (ux 4)

Scope: filters shareable in the URL; multi-select for the observed facets.
Source: `web/src/pages/OperationsPage.tsx`, `web/src/i18n.ts`.
Tests: `web/src/pages/observabilityWiring.test.tsx`.
Acceptance: reloading a filtered URL reproduces the same list; back/forward restores
the previous filter state; a multi-select sends the values the API accepts (or the
API is extended with a bounded list, in the same batch).
Depends on: nothing.
Verify: `pnpm --dir web test -- src/pages/observabilityWiring.test.tsx`.

### B65 — Probe depth: model choice, history, bulk test, sparkline (ux 2, ux 14 remainder)

Scope: the parts of AxonHub's channel testing the console still lacks.
Source: `src/api/operations_api.rs` (probe route taking a model), `src/operations/runtime.rs`
(the runner no longer picks the model itself), `web/src/pages/ChannelsPage.tsx`,
`web/src/i18n.ts`.
Tests: a Rust case proving the requested model is probed and a refused model is
reported; web cases for the history drawer, the bulk test and the sparkline.
Acceptance: the operator chooses the model; a probe history is readable per channel
with success rate, latency, TTFT and tokens/s; a bulk test runs with bounded
concurrency and reports per channel; the sparkline prints `—` when no probe history
exists and never a fabricated success rate.
Depends on: nothing.
Verify: `cargo test --locked operations::`; `pnpm --dir web test -- src/pages/channelProbeRow.test.tsx src/pages/channelHealth.test.tsx`.

---

## Wave 8 — delivery

### B66 — Final delivery

Scope: rebuild, verify, close the ledger, and publish.
Source: `web/dist` (rebuild), the release binary, `docs/reviews/parity-ledger.md`,
`README.md` (screenshots), `Cargo.toml`/version files, CI workflows.
Tests: the full gate set plus the browser matrix.
Acceptance: web assets rebuilt before the binary; the six gate commands green on the
merge result; the browser matrix (375/768/1440 × light/dark × `zh-CN`/`en`) covering
the goal's workflows — backup/restore, jobs, catalog sync, access grants, playground
runs, requests/traces — with screenshots; every closed row moved in the ledger with
its red/green evidence and the counts re-derived; `actionlint` clean; then the
version bump, `main` push, tag, release assets and GHCR images per
`tasks/axonhub-parity-plan.md` Task 7.
Depends on: every batch above, or an explicit deferral recorded in the ledger.
Verify: the standard gate set, then `go run github.com/rhysd/actionlint/cmd/actionlint@latest -color`, then the browser matrix.

---

## Decision queue (do these before the batches that depend on them)

| Decision | Blocks | Ledger row |
| --- | --- | --- |
| Display currency: instance setting or documented USD-only boundary | B47 | observability 14, system 22 |
| Restore conflict granularity: per-resource or deliberate single strategy | B51 | system 19 |
| Instance retry/upstream-error knobs: promote or document as per-channel | B48 | system 23 |
| Quota routing mode: implement alternatives or document REMOVE_ON_EXHAUSTED | B49, B61 | system 24 |
| Diagnostics tab: build or record as not planned | B52 | system 25 |
| Invitation reuse: max-uses or keep single-use as deliberate | B27 | access 13 |
| GraphQL: implement a scoped endpoint or record divergence D6 | B63 | matrix |
| Semantic memory: opt-in provider or interface only | B62 | matrix |

## Explicitly not planned

Nothing in the AxonHub surface is excluded by this plan. The scope expansion means
the previous "out of scope" list (model discovery, the full price editor, the chat
playground) is now scheduled: B44, B39 and B17. The only rows that can end without
code are the eight decisions above and the two recorded divergences.
