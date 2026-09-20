# Task 6 report: complete admin API and console

## Outcome

Implemented the first complete administration-console pass for Pangolin / 鲮鲤. The console is no longer a six-page demo: it is split into domain routes and exposes project-aware workflows for access control, channels and credentials, models and routing, prompts and protection, operational diagnosis, playground requests, catalogs, storage, backup, webhooks, request logging, branding and system settings.

The visual direction remains the Pangolin design-system direction: a compact operator workspace using warm mineral surfaces, stronger control boundaries, restrained motion and dense, legible tables. The overview now distinguishes measured zero from unavailable telemetry, uses a non-nested 24-hour setup empty state, and keeps four metrics in a compact two-column mobile grid.

## Backend and security work

- API keys now store a unique indexed deterministic BLAKE3 lookup digest plus an Argon2id verification hash. Authentication performs a single indexed lookup and then verifies the Argon2id hash; it no longer parses or depends on a `pg_` prefix.
- `generated` and `import_existing` creation modes are supported by both scoped and legacy creation paths. Imported values must be 32–1024 bytes, contain no whitespace/control characters and pass a Shannon-entropy floor. Duplicate imports return a conflict. Imported plaintext is neither persisted nor returned. Audits retain only a display fingerprint and mode.
- Browser mutations under `/api/admin/v1/` uniformly require the existing `X-Pangolin-CSRF: 1` semantics for session cookies and reject cross-site fetch metadata. API-key principals remain exempt from browser-CSRF handling and use the existing authorization evaluator.
- Added scoped control-plane projections and audited CRUD for channels, multiple encrypted credentials, channel settings, models, associations, key profiles, prompt templates and protection rules. Projections omit encrypted credential material.
- Key-profile CRUD includes RPM, TPM, budget, routing policy, model mappings and exact/regex allowed-model rules. Scoped key updates include profile, budget, expiry and allow/deny IP policy.
- Added bulk channel/model/credential enable-disable, probe/quota job actions, system branding/onboarding settings, and list search/pagination metadata.
- Added project-scoped observability summary/list/detail APIs. Request and detail reads can no longer cross a selected project boundary.
- Added a project-scoped routing-preview endpoint that calls the real orchestration planner and returns safe ordered candidates plus access, mapping, association, eligibility and strategy decisions. The UI does not maintain a divergent regex-only routing implementation.
- Added a permissions endpoint for the selected project. Navigation uses evaluated permission slugs, including wildcard owners, rather than a hardcoded role label.
- Extended role list projections with current permission slugs so edits do not silently discard assignments.
- Preserved transactional audit writes for the new control-plane mutations and secret-free public projections.

## Frontend work

- Removed the monolithic `web/src/Pages.tsx` and added separate route modules for overview, channels, models/routing/catalogs, access, prompts/protection/overrides, operations/details, playground and system/backup.
- Kept login at `/login` and setup at `/setup`, both outside the authenticated shell. Authenticated `/login` redirects to the overview. Direct client routes remain served by the embedded Rust SPA fallback.
- Added a selected-project provider. Every scoped query/mutation derives its project from the accessible project list; no fixed project identifier remains in frontend code.
- Added permission-aware grouped navigation, a project switcher when multiple projects are accessible, mobile modal navigation, skip link, focus restoration and accessible Radix dialogs/selects.
- Added explicit loading, error/retry and useful empty states to all query surfaces. Operational tables use TanStack Table and 14px body text with horizontally contained mobile regions.
- Added project, user, role, invitation, membership/role-binding, OIDC provider and OIDC identity workflows.
- Added generated/imported API-key mode switching, one-time import explanation, profile/budget/expiry/IP editing, enable-disable, delete and per-key request-log policy.
- Added channel CRUD, multiple credential create/edit/rotation, row and bulk enable-disable, manual probe/quota actions, presets, visible resource identifiers, and catalog `logo_key` mapping through `@lobehub/icons`, Simple Icons, then deterministic initials.
- Added model capabilities and prices, association CRUD, backend routing preview/explanation, catalog import/export, local typed override editing and HTTPS subscription create/edit/delete/refresh/snapshot rollback.
- Added prompts, protection rules and channel request-override documents.
- Added request filters with refresh pause, execution/thread/trace/usage/cost/audit tabs, direct request and trace details, payload privacy state and JSON download.
- Added a key-based Responses playground that does not persist the entered key.
- Added request-logging policy, local/S3 storage, selective export/restore/delivery, automatic schedules, jobs, webhooks, retention, branding/favicon and onboarding workflows.
- Completed zh-CN/en copy and initial document-language synchronization. Light/dark/system and bronze/slate/jade themes remain available.
- Motion uses the existing transform/opacity tokens: fine-pointer press feedback at 160ms, trigger-origin popovers at 180ms, centered dialogs at 220ms/160ms exit, symmetric 220ms drawers and reduced-motion removal of transforms. Data/charts and page navigation do not animate.
- Route-level lazy loading keeps the production entry chunk below the prior single-bundle warning threshold.

## Files

Backend:

- `src/access.rs`, `src/access_api.rs`
- `src/api.rs`, `src/api/operations_api.rs`
- `src/api/gateway/tests/operations.rs` and API-key fixture callers
- `src/crypto.rs`
- `src/db.rs`, `src/db/schema.rs`
- `src/models.rs`
- `src/observability.rs`
- `src/orchestration/tests.rs`
- `src/web.rs`

Frontend:

- `web/package.json`, `web/pnpm-lock.yaml`
- `web/src/App.tsx`, `web/src/App.test.tsx`
- `web/src/Shell.tsx`, `web/src/project.tsx`
- `web/src/api.ts`, `web/src/components.tsx`, `web/src/i18n.ts`, `web/src/styles.css`
- `web/src/ProviderIcon.tsx`
- `web/src/pages/AccessPage.tsx`, `AccessPage.test.tsx`
- `web/src/pages/ChannelsPage.tsx`
- `web/src/pages/ModelsPage.tsx`
- `web/src/pages/OperationsPage.tsx`
- `web/src/pages/OverviewPage.tsx`
- `web/src/pages/PlaygroundPage.tsx`
- `web/src/pages/PromptsPage.tsx`
- `web/src/pages/SystemPage.tsx`
- `web/src/pages/shared.tsx`
- `web/src/test-setup.ts`
- removed `web/src/Pages.tsx`

## Targeted verification

Per the instruction not to duplicate Tasks 1–5 verification, no prior full Rust suite, release build, actionlint run or unrelated test suite was repeated.

- `cargo test --locked task6_ --no-fail-fast`
  - passed: 1 control-plane CRUD/redaction/audit/routing-preview/system-settings test.
- `cargo test --locked access_api::tests:: --no-fail-fast`
  - passed: 12 access/OIDC/API-key tests.
- `cargo test --locked access_api::tests::imported_api_keys_authenticate_by_digest_without_returning_plaintext --no-fail-fast`
  - passed: imported non-`pg_` token authenticates, response omits plaintext, duplicate import conflicts.
- `cargo test --locked crypto::tests::imported_tokens_require_real_symbol_entropy --no-fail-fast`
  - passed: high-entropy token accepted; low-entropy and whitespace values rejected.
- `cargo test --locked db::schema::tests::fresh_database_has_v2_schema_seeds_foreign_keys_and_indexes --no-fail-fast`
  - passed: fresh schema includes the unique lookup-digest index and existing integrity contracts.
- `cargo test --locked observability::tests:: --no-fail-fast`
  - passed: 2 projection/degraded-store tests.
- `cargo test --locked web::tests::embedded_spa_serves_direct_admin_and_auth_routes --no-fail-fast`
  - passed: `/login`, `/setup`, `/channels` and a nested trace route all return embedded `index.html` with no-cache semantics.
- `pnpm --dir web lint && pnpm --dir web test && pnpm --dir web build`
  - passed: TypeScript clean, 6 Vitest behavior/accessibility/route tests passed, production build completed with route-level chunks and no size warning.
- `cargo fmt --all -- --check`
  - passed.
- `cargo clippy --locked --bin pangolin -- -D warnings`
  - passed with strict warnings for the changed binary target.

## Browser verification

Browser workflows ran against the embedded production assets served by the Rust binary, not the Vite development server. A local mock upstream populated a real channel, credential, model, route, API key, gateway request, trace, execution and usage path. This is protocol-contract evidence, not a live-cloud claim.

- Populated overview, channels, models, backend routing explanation, request list, request detail and trace detail were exercised.
- Light/en and dark/zh-CN screenshots were visually inspected at desktop, tablet and phone sizes.
- A 12-case matrix covered 375, 768 and 1440 widths × light/dark × en/zh-CN. Every case reported the expected `lang`, expected resolved theme and `scrollWidth <= innerWidth`.
- Operational table body text computed to 14px.
- At 375px the four overview metrics computed as a compact two-column grid.
- The 768px check caught an over-narrow desktop rail; the shell breakpoint was moved to 900px and rechecked with modal navigation.

Screenshots were kept as ephemeral verification artifacts. Task 7 owns sanitized release screenshot selection and README publication.

## External-validation boundaries

- No paid/live provider, real enterprise OIDC issuer, S3-compatible target, webhook receiver, remote signed catalog host, provider quota endpoint or OAuth/device-flow helper was contacted.
- Catalog refresh/signature, OIDC, object storage, webhook and provider behavior remains contract-tested with local logic/mocks where covered, not live-tested.
- Browser population used a local OpenAI-compatible mock; it validates the complete console/gateway/trace path but not vendor-specific behavior.
- Whole-instance release packaging, consolidated full suites, actionlint, release-binary publication and public screenshots remain Task 7 work by explicit instruction.

## Self-review

- Rechecked all generic resource endpoints: plain-array and `{data}` list shapes are both handled; overridden resources use their actual POST/PATCH/PUT/DELETE item routes; append-only prices remain creatable; mutable resources retain edit/enable-disable/delete where safe.
- Rechecked selected-project propagation across operations, observability, direct detail links, keys, roles, invitations and routing preview. Frontend source contains no fixed default-project identifier.
- Rechecked secret surfaces: credential envelopes, imported plaintext, OIDC secrets, storage secrets and webhook secret headers are absent from public projections and UI responses.
- Rechecked unsafe session mutations for CSRF and audit coverage.
- Rechecked identifiers required by dependent workflows are visible in tables, including channels, credentials, models, keys, projects, users, roles and storage targets.
- Rechecked the GPT-6 baseline items: non-nested useful empty overview, explicit 24-hour window, no invented error-rate/latency zeroes, 14px operational rows, stronger control borders and compact mobile metrics.
- Rechecked motion for transform/opacity-only transitions, fine-pointer press gating, symmetric drawer timing and reduced-motion behavior.
- `git diff --check` is clean. Controller-owned specification, ADR, capability and earlier task-report changes were not staged or modified by this task.

## Fix round 1

Commit base: `93a7f5b`.

The confirmed GPT-6 review findings were addressed as one focused correction pass. The public test seams were the project operations API, scoped API-key PATCH API, OIDC public projection, embedded bootstrap payload, React route/forms, and the real embedded browser render.

| Finding | Correction | Focused evidence |
| --- | --- | --- |
| 1. Blank secrets replaced envelopes | Optional secret fields are omitted by form serialization; storage and webhook handlers also treat explicit JSON null as omission. | `task6_patch_preserves_omitted_secrets_and_channel_settings` compares encrypted envelopes before/after null edits without printing secrets; React resource test asserts blank secret is absent from request JSON. |
| 2. Channel edit erased settings | Channel upsert reads and preserves existing `settings_json` when settings are omitted. The channel form exposes tags/limits/circuit JSON, and a named Channel policies tab manages endpoint maps, model rules, overrides, retries, proxy and auto-disable documents. | Focused Rust test seeds non-default tags/limits/catalog metadata, performs routine edit, and asserts the full document is unchanged. |
| 3. OIDC field mismatch | Public OIDC projection now serializes `scopes` and `claim_mapping`, matching create/update input names; blank client secret is omitted. | Focused OIDC test asserts old `*_json` names are absent and custom JIT mapping/scopes round-trip under normalized names. |
| 4. Invitation token lost | Invitation administration is a dedicated workflow that reveals one delivery URL once and offers copy. `/invite?token=…` is an unauthenticated inspect/accept route with password onboarding for a new user and session-aware acceptance for an existing user. | Vitest exercises direct unauthenticated inspection and verifies the acceptance password form. Existing Rust invitation one-time token contracts remain targeted by the access module. |
| 5. Fixed limit/no pagination | Generic resources now send `offset`, `limit` and `q`, honor server totals, expose previous/next/page-size controls, and client-page legacy array endpoints. Scoped request browsing returns `{data,total,offset,limit}` and supports SQL offset. | React test advances to offset 25 and asserts the server query. Observability test and Task 6 operations tests cover the amended contract. |
| 6. Restrictions could not clear | Added explicit Missing/Null/Value PATCH semantics for profile, budget and expiry. Blank console values intentionally serialize to null; omitted fields retain current values. | `scoped_api_key_patch_distinguishes_omitted_and_explicit_null` proves both retention and clearing through HTTP responses. |
| 7. Hidden project orchestration settings | Added versioned, validated GET/PUT for `affinity_rules` and `session_compaction`, preserving unrelated project settings and resetting affinity cache after update. Console exposes both documents. | Focused Task 6 Rust test accepts a valid rule/compaction document, reads it back, and rejects TTL zero. |
| 8. Placebo branding controls | Bootstrap loads persisted system branding with legacy instance-name fallback. Runtime applies document title/favicon and Shell brand; onboarding state controls a visible setup banner. Saving invalidates bootstrap data and synchronizes the legacy instance-name key. | Vitest asserts Shell label, title and favicon from bootstrap. Task 6 Rust test confirms persisted branding/onboarding appears in `/api/v1/bootstrap`. |
| 9. 375px document overflow | Content descendants are min-width constrained; tables own their horizontal overflow; mobile generic resources switch to compact cards; identifiers/headings wrap; hidden labels cannot affect layout width. | Embedded 375px checks: channels `documentWidth=375`, routing `documentWidth=375`, trace detail `documentWidth=375`; all report `overflow=false`. |
| 10. UUID-first rows/forms | Human names, credential suffixes and states lead desktop/mobile rows. IDs move to copy actions and raw-detail disclosure. Channel/model/profile/role relationships use named Radix selectors; generic mobile cards retain edit/delete actions. | 375px populated channel/model/routing inspection plus 1440px model action visibility. |
| 11. 12px operational code | Table/detail monospace descendants, JSON blocks, URLs, IDs and preset URLs are at least 14px. | Computed browser values: channels monospace `14px`; routing code `14px`; trace IDs `14px`; desktop table body/monospace `14px`. |
| 12. Raw routing/trace UX | Routing stages/reasons are localized and humanized. Trace detail prioritizes outcome, measured duration and start time; links related requests and lists executions; raw IDs are secondary disclosure; navigation state returns to the originating trace tab. | Populated routing explanation and trace-detail browser workflows at 375px and 1440px. |
| 13. Dialog focus restoration | Resource launch buttons are recorded and focus is restored after save, Escape or close after the dialog unmounts. | React test covers save and Escape restoration to the actual Edit trigger. |

### Fix-round verification

- `cargo test --locked task6_ --no-fail-fast`
  - passed: 2 Task 6 control-plane tests.
- `cargo test --locked scoped_api_key_patch_distinguishes_omitted_and_explicit_null --no-fail-fast`
  - passed: 1 tri-state PATCH test.
- `cargo test --locked oidc::tests::pkce_state_is_encrypted_expiring_one_time_and_jit_maps_roles --no-fail-fast`
  - passed: normalized projection plus existing OIDC/PKCE/JIT contract.
- `cargo test --locked observability::tests::records_and_queries_events --no-fail-fast`
  - passed: amended request filter/count projection contract.
- `pnpm --dir web lint && pnpm --dir web test && pnpm --dir web build`
  - passed: TypeScript clean, 9 Vitest tests passed, production assets built.
- Representative embedded browser checks:
  - 375px populated channels: `documentWidth=375`, no document overflow, mobile cards `grid`, desktop table `none`, monospace `14px`.
  - 375px populated routing: `documentWidth=375`, no document overflow, mobile cards `grid`, code `14px`.
  - 375px trace detail: `documentWidth=375`, no document overflow, all inspected IDs `14px`.
  - 1440px populated models: `documentWidth=1440`, no document overflow, table client/scroll width both `1075`, status/actions visible, body and monospace `14px`.

No full Tasks 1–5 suite, full 12-case browser matrix, release build or actionlint run was repeated in this fix round.
