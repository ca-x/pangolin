# Reference functional adoption implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development to implement this plan task-by-task. Steps use checkbox syntax for tracking. The user authorized all seven prioritized improvements and delegated routine decisions; continue without new approval questions.

**Goal:** Deliver the seven reviewed improvements as tested, usable Pangolin gateway/control-plane/console behavior.

**Architecture:** Extend existing services with one additive SQLite migration and bounded typed metadata. Keep DuckDB derived, provider network I/O outside transactions, and project/key policy before routing. Existing native forwarding, stream commitment, budgets and immutable price references remain authoritative.

**Tech Stack:** Rust 1.98/edition 2024, Axum 0.8, SeaORM/SQLite, bundled DuckDB, existing LiteLLM Rust pin; React 19, Mantine/Radix, TanStack Query/Table, pnpm 11. No new dependencies required.

**Spec:** `specs/SPEC-reference-adoption.md` and its seven `specs/reference-adoption/SPEC-*.md` module specifications.

## Global Constraints

- Explicit user requirement: apply `ddia-principles` and `docs/architecture/parity-data-design.md` to every database change; use semantic identity/discovery fingerprints, consistent scoped reads, atomic fenced activation and idempotent financial facts.

- SQLite through SeaORM is authoritative; DuckDB failure degrades only observability.
- No provider/network/object-store I/O inside a SQLite business transaction.
- Control-plane mutations and their audit records commit transactionally.
- Project/API-key/model access fails closed; no cross-project references or silent principal upgrade.
- Requests/bodies/arrays/events/caches/diagnostics are bounded; secrets, raw samples and restore mappings never enter logs/audits.
- Preserve native provider fields. Cross-protocol unsupported shapes are explicitly rejected.
- Never retry after downstream streaming bytes are committed.
- Money is integer micro-USD; historical amounts and immutable price versions are not rewritten.
- React is bilingual zh-CN/en, light/dark/system, existing accents; usable at 375/768/1440px; operational text >=14px; unknown telemetry is a dash.
- Reuse existing house components, Radix dialogs/selects/tooltips and Lucide structural icons; motion <=300ms transform/opacity and reduced-motion aware.
- LiteLLM Rust stays at commit 8c4c394ecc82c4d6acb5eb371d8781e487894a17; do not import Go RelayKit/AGPL code or project-owned assets.
- Release embedding requires fresh web/dist before Rust verification. No push/tag/publish/release action.

## Review Focus

- A secret rotation, disable or config edit while discovery is in flight must fence stale visibility, while partial fetch failure retains matching last-known-good models.
- Headers, created/role/heartbeat/reasoning events and nonstream results must never be labeled measured first visible text.
- Conditional/heterogeneous routes and unknown metadata must not overpromise capabilities or reveal foreign models.
- User/tool JSON keys named signature must not bypass inspection; valid signed continuation must remain byte-for-byte intact; samples/tokens must be forgotten on dialog close.
- Missing price must not be presented as free or allow an unpriced hard-budget request to reach upstream; completed historical accounting remains unchanged.

## Task 1: Additive authoritative schema and upgrade contracts

**Files:** Modify `src/db/schema.rs`; test in its existing test module. If full-instance backups enumerate tables, modify `src/operations/instance_backup.rs` and its focused tests solely to include new visibility tables in dependency order.

**Interfaces produced (migration version 29):**
- `credential_model_snapshots`: credential_id PRIMARY KEY; provider_id/project_id; credential_fingerprint/provider_config_fingerprint; status CHECK known/stale; last_success_at nullable; last_attempt_at non-null; last_error_code nullable. Add unique parent indexes providers(id,project_id), channel_credentials(id,provider_id); enforce composite child FKs and deletion cascade. These are inventory facts, not decrypted credential copies.
- `credential_model_availability`: credential_id, upstream_name bounded to catalog identifier contract, metadata_json valid JSON default {}; PRIMARY KEY(credential_id,upstream_name); cascade from the snapshot.
- `channel_probes`: endpoint nullable; stream nullable Boolean; response_headers_ms/first_event_ms/first_text_ms nullable nonnegative. Historical header-based ttft_ms moves to response_headers_ms and ttft_ms becomes NULL, retaining the measured fact under its correct name.
- `request_executions`: response_headers_ms/first_event_ms/first_text_ms nullable nonnegative; conversion_diagnostics_json and affinity_diagnostics_json NOT NULL DEFAULT '[]' with json_valid; pricing_status NOT NULL DEFAULT 'legacy' constrained to legacy/priced/explicit_free/missing_price/incomplete_usage.
- `execution_facts` and `usage_logs`: same pricing_status default/check. `models`: pricing_configured INTEGER NOT NULL DEFAULT 0 CHECK Boolean; migrate nonzero legacy model prices to configured, retain zero defaults as unconfirmed. Do not alter settled amounts or immutable price records.
- `prompt_protection_rules`: allowlist_json NOT NULL DEFAULT '[]' with json_valid.

**Acceptance/tests:** `reference_schema_upgrade_preserves_facts`, `reference_schema_rejects_cross_project_inventory`, `reference_schema_is_idempotent`. Test real SQL constraints with two projects/providers/credentials; unavailable columns make the new tests fail before migration. Reopen/migrate twice and preserve existing model/manual/enabled flags, monetary rows, probe historical header time, and child deletion behavior.

Example structural assertion (inside existing schema tests):
```rust
let row = db.query_one(statement("SELECT COUNT(*) AS count FROM pragma_table_info('request_executions') WHERE name IN ('first_text_ms','conversion_diagnostics_json','pricing_status')")).await?.unwrap();
assert_eq!(row.try_get::<i64>("", "count")?, 3);
```
Focused command: `cargo test --locked reference_schema`.
**Commit:** `feat(db): add reference adoption metadata and migration contracts`.

- [ ] **Step 1: Write the named behavioral regressions below and run the focused command before implementing.** Record expected RED output in the task report. Use existing composed mock fixtures; do not weaken tests to bless the implementation.
- [ ] **Step 2: Implement the contract in the listed files, preserving existing defaults and public compatibility where specified.** Apply only this task's required changes.
- [ ] **Step 3: Run focused GREEN verification, then the relevant full suite once.** Backend tasks: `cargo fmt --all -- --check`, `cargo test --locked`. Frontend tasks: `pnpm --dir web lint`, `pnpm --dir web test`, `pnpm --dir web build`. New source may be formatted with `cargo fmt --all` before checks. Capture exact commands, exit codes and relevant counts.
- [ ] **Step 4: Read your own diff for scope, typed boundaries, leaked content and regressions; fix findings and repeat only covering checks.** Commit the implementation/tests atomically with the conventional subject specified below. Never include controller-owned specs/plans/docs in a source implementation commit.
- [ ] **Step 5: Write the task report, including RED/GREEN evidence, changed files, self-review and concerns.** Return DONE/DONE_WITH_CONCERNS/BLOCKED/NEEDS_CONTEXT plus commit and one-line test summary. No worker-spawned agents or reviewers.

## Task 2: Protocol/model/credential probes and honest timing

**Spec:** `specs/reference-adoption/SPEC-probe-measurement.md` plus shared spec.
**Files:** `src/operations/runtime.rs`, `src/operations/lifecycle.rs`, `src/api/gateway.rs`, `src/api/operations_api.rs`, `src/providers/framing.rs` or new small `src/providers/timing.rs`; related operations/protocol tests and observability projection code only where needed to expose correct times.
**Consumes:** Task 1 timing columns. **Produces:** authorized probes with optional `endpoint`, `credential_id`, `stream`; execution/probe JSON `response_headers_ms`, `first_event_ms`, `first_text_ms`; `ttft_ms` is first visible text for new measured records.

Implement a protocol-aware visible-text classifier, excluding empty/role/reasoning/tool-call-only events. Recognize Chat content delta, Responses output_text delta, Anthropic text_delta/text content starts, and Gemini non-thought text parts. Keep timestamp capture once per phase using Instant. Gateway first_event timeout/commit logic remains exactly its existing contract; ordinary tool-only replies remain successful with first_text_ms absent. Ensure live/authoritative/derived reads and analytics do not relabel first_event as text.

Probe input supports only configured conversational endpoints (/v1/chat/completions, /v1/responses, /v1/messages, canonical Gemini generateContent/streamGenerateContent). Validate selected credential belongs to the same enabled provider/project and selected model supports endpoint/stream, both enqueue and job execution. Omitted fields keep family defaults. Read stored endpoint mapping/proxy configuration. A helper may return the selected target rather than widening existing callers. Request preparation and I/O are outside the fenced result transaction. Read at most 1 MiB total, <=2048 events and 30s total. Validate response output and stream terminal, persist safe error taxonomy: authentication, rate_limited, model_unavailable, invalid_response, empty_response, missing_terminal, timeout, network, upstream_http. Unknown/public causes remain sanitized; no raw probe body storage.

**RED/GREEN cases:** headers->created/heartbeat/role/reasoning->text->terminal produces distinct timings; nonstream and terminal-only no first text; tool-only gateway remains valid; HTTP200 unrelated object fails probe; wrong credential/project/disabled model/unsupported endpoint rejected without network; custom path applied; interruption/oversize fenced. Use channels/controlled events rather than brittle sleep thresholds. Extend existing gateway tests `operations.rs` and `protocols.rs`.
Example classifier expectations:
```rust
assert!(!visible_text(&json!({"type":"response.created"})));
assert!(!visible_text(&json!({"choices":[{"delta":{"role":"assistant"}}]})));
assert!(visible_text(&json!({"type":"response.output_text.delta","delta":"OK"})));
```
Focused: `cargo test --locked reference_probe`; `cargo test --locked reference_timing`.
**Commit:** `feat(observability): measure protocol probes and first text accurately`.

- [ ] **Step 1: Write the named behavioral regressions below and run the focused command before implementing.** Record expected RED output in the task report. Use existing composed mock fixtures; do not weaken tests to bless the implementation.
- [ ] **Step 2: Implement the contract in the listed files, preserving existing defaults and public compatibility where specified.** Apply only this task's required changes.
- [ ] **Step 3: Run focused GREEN verification, then the relevant full suite once.** Backend tasks: `cargo fmt --all -- --check`, `cargo test --locked`. Frontend tasks: `pnpm --dir web lint`, `pnpm --dir web test`, `pnpm --dir web build`. New source may be formatted with `cargo fmt --all` before checks. Capture exact commands, exit codes and relevant counts.
- [ ] **Step 4: Read your own diff for scope, typed boundaries, leaked content and regressions; fix findings and repeat only covering checks.** Commit the implementation/tests atomically with the conventional subject specified below. Never include controller-owned specs/plans/docs in a source implementation commit.
- [ ] **Step 5: Write the task report, including RED/GREEN evidence, changed files, self-review and concerns.** Return DONE/DONE_WITH_CONCERNS/BLOCKED/NEEDS_CONTEXT plus commit and one-line test summary. No worker-spawned agents or reviewers.

## Task 3: Credential visibility snapshots and conservative route metadata

**Specs:** credential-model-discovery and model-capability-projection module specs.
**Files:** `src/providers/discovery.rs`, new small `src/operations/model_inventory.rs` if runtime needs a focused helper, `src/operations/runtime.rs`, `src/orchestration/repository.rs`, `src/orchestration/mod.rs`, `src/catalog/types.rs`; existing discovery/orchestration/gateway contract tests. Keep API extraction in `src/api/operations_api.rs` only for safe credential discovery metadata projection.
**Consumes:** inventory tables from Task 1. **Produces:** per-enabled-credential known/stale visibility facts; credentials list adds discovery_status/discovery_model_count/discovery_last_success_at/discovery_error_code; `/v1/models?include_metadata=true` adds bounded nullable capabilities/modalities/reasoning levels and aggregated limits.

Fetch all enabled keys with bounded concurrency <=4. Capture secret envelope and provider configuration fingerprints; recheck actual row/config and jobs::fence at atomic activation. Successful empty key replaces its visibility; failure preserves its matching previous snapshot. Union only active matching credential snapshots for discovery-managed models; manual rows/prices/names stay unchanged. If no valid results/all failures, report safe failure and preserve prior channel models. Never store credential plaintext or provider raw JSON.

Candidate filtering consults matched inventory: if a model is discovered for another credential, a known credential whose list omits it cannot route it. If no credential has ever confirmed a manually configured upstream ID, that visibility stays unknown and existing admin model intent remains possible, still after project/key/model authorization. Disabled/rotated keys never regain old eligibility.

Expand `ModelCardProjection` with `capabilities` nullable booleans, `modalities` optional bounded input/output lists, `reasoning_levels` optional bounded list (<=16 entries, each <=32 bytes). Retain existing fields for compatibility. Parse capabilities from typed card/discovery facts. Aggregate cards from eligible candidates in the same endpoint/context: explicit false dominates shared capability, all-known true yields true, otherwise None; limits require all positive known and use min; unknown limits stay None; common developer/type/logo/cost facts only when consistent. Intersect known modalities/efforts without claiming unknown sets. Do not emit arbitrary extension JSON publicly.

Filter explicit image/tools/output-limit capability mismatches before provider calls, using actual request structure and existing authorization. Unknown capability remains unknown, not a guessed promise or access grant. Keep unsupported transforms rejected. Preserve all existing route rules and priority tiers.

**RED/GREEN cases:** two keys with disjoint model lists (A cannot route B's model); partial failure retains A; disabled/rotated/raced credential cannot activate; empty successful list removes only discovered models; manual unknown preserved; heterogeneous alias true/false/unknown capabilities; limits [100,80]->80, [100,None]->None; hidden/foreign/disabled models absent; actual image/tool conditions narrow candidates; malformed metadata bounded.
```rust
assert_eq!(aggregate_limits(&[Some(100), Some(80)]), Some(80));
assert_eq!(aggregate_limits(&[Some(100), None]), None);
```
Adapt helper name to the focused projection module, keeping behavior. Focused commands: `cargo test --locked reference_inventory`; `cargo test --locked reference_metadata`; `cargo test --locked reference_capability`.
**Commit:** `feat(routing): discover credential models and aggregate route capabilities`.

- [ ] **Step 1: Write the named behavioral regressions below and run the focused command before implementing.** Record expected RED output in the task report. Use existing composed mock fixtures; do not weaken tests to bless the implementation.
- [ ] **Step 2: Implement the contract in the listed files, preserving existing defaults and public compatibility where specified.** Apply only this task's required changes.
- [ ] **Step 3: Run focused GREEN verification, then the relevant full suite once.** Backend tasks: `cargo fmt --all -- --check`, `cargo test --locked`. Frontend tasks: `pnpm --dir web lint`, `pnpm --dir web test`, `pnpm --dir web build`. New source may be formatted with `cargo fmt --all` before checks. Capture exact commands, exit codes and relevant counts.
- [ ] **Step 4: Read your own diff for scope, typed boundaries, leaked content and regressions; fix findings and repeat only covering checks.** Commit the implementation/tests atomically with the conventional subject specified below. Never include controller-owned specs/plans/docs in a source implementation commit.
- [ ] **Step 5: Write the task report, including RED/GREEN evidence, changed files, self-review and concerns.** Return DONE/DONE_WITH_CONCERNS/BLOCKED/NEEDS_CONTEXT plus commit and one-line test summary. No worker-spawned agents or reviewers.

## Task 4: Client setup read API and quick-connect dialog

**Spec:** client-onboarding module.
**Files:** `src/api/operations_api.rs` (or small `src/api/client_setup.rs` registered there), existing gateway isolation tests; new `web/src/pages/ClientSetupDialog.tsx`, `web/src/clientSetup.ts`, corresponding tests; `web/src/pages/AccessPage.tsx`, `web/src/i18n.ts`.
**Consumes:** bounded metadata/model eligibility from Task 3. **Produces:** `GET /api/admin/v1/projects/{project}/api-keys/{key_id}/client-models?endpoint=<known conversational endpoint>` -> {models:[existing public-model shape]}; a labeled Connect action and code tabs for Codex, Claude Code, Gemini CLI, curl, Python and Node.

Authorize scoped reads with existing actor/principal handling. Load the key by project/id, validate enabled/lifecycle/expiry, reuse the existing routable-model projection and profile/key model filters with a sanitized context. Never recover/decrypt a gateway token or list foreign models.

Keep code generation pure TS functions accepting client, operator-confirmed gatewayBase, public model and optional one-time token/effort. Base defaults to current deployment origin/base and is editable with HTTP(S) URL validation, preserving a reverse-proxy subpath and exactly one protocol-specific /v1 suffix. Codex uses a named Pangolin provider with wire_api="responses" and a secure environment key/one-time token configuration; Claude env/JSON and Gemini base use their correct root paths. Python/Node use JSON-safe string literals and SDK requests; shell/PowerShell snippets safely quote data and never interpolate untrusted strings as shell code.

Show model choices using endpoint-scoped read above. No-model/error/loading states explicit with retry. For old keys snippets use an env variable placeholder. New create/rotate can temporarily inject raw token; clear token/snippet/model/sample on close and never localStorage. Keep current one-time key dialog semantics and copy feedback; use existing Radix-backed dialog/select components, i18n for all labels and accessible icon controls. Existing tokens need no recovery and client config files are not read/written by the server.

**RED/GREEN tests:** foreign project/expired/disabled keys; restricted model/profile alias; no token field in API; root and subpath URL variants; escaping quotes/newlines/$/backticks; serialization valid JSON/TOML shape; correct Responses protocol; clear sensitive state on close; query loading/error/empty/retry; both languages.
```ts
expect(generateClientSetup({ client: 'codex', baseUrl: 'https://gateway.example/team/', model: 'public' })).toContain('wire_api = "responses"')
expect(generateClientSetup({ client: 'claude', baseUrl: 'https://gateway.example/team/v1', model: 'public' })).not.toContain('/v1/v1')
```
Match exported function names to this contract or report exact produced signatures. Focused: `cargo test --locked reference_client_models`; `pnpm --dir web test -- src/clientSetup.test.ts src/pages/ClientSetupDialog.test.tsx`.
**Commit:** `feat(console): add safe client setup for project API keys`.

- [ ] **Step 1: Write the named behavioral regressions below and run the focused command before implementing.** Record expected RED output in the task report. Use existing composed mock fixtures; do not weaken tests to bless the implementation.
- [ ] **Step 2: Implement the contract in the listed files, preserving existing defaults and public compatibility where specified.** Apply only this task's required changes.
- [ ] **Step 3: Run focused GREEN verification, then the relevant full suite once.** Backend tasks: `cargo fmt --all -- --check`, `cargo test --locked`. Frontend tasks: `pnpm --dir web lint`, `pnpm --dir web test`, `pnpm --dir web build`. New source may be formatted with `cargo fmt --all` before checks. Capture exact commands, exit codes and relevant counts.
- [ ] **Step 4: Read your own diff for scope, typed boundaries, leaked content and regressions; fix findings and repeat only covering checks.** Commit the implementation/tests atomically with the conventional subject specified below. Never include controller-owned specs/plans/docs in a source implementation commit.
- [ ] **Step 5: Write the task report, including RED/GREEN evidence, changed files, self-review and concerns.** Return DONE/DONE_WITH_CONCERNS/BLOCKED/NEEDS_CONTEXT plus commit and one-line test summary. No worker-spawned agents or reviewers.

## Task 5: Structured privacy traversal, allowlists and request dry run

**Spec:** privacy-inspection module.
**Files:** `src/orchestration/protection.rs`, new small `src/orchestration/inspection.rs` if needed, `src/api/operations_api.rs`, `src/orchestration/mod.rs` only for module export/preview glue; existing policy and gateway contract tests.
**Consumes:** allowlist_json from Task 1. **Produces:** protection CRUD optional `allowlist` string array; `GET /api/admin/v1/projects/{project}/operations/protection-templates` static safe preset documents; `POST /api/admin/v1/projects/{project}/protection-request-preview` {endpoint,body} -> {decision,redacted_body,findings,suppressed_findings,truncated:false}. Legacy text protection-preview remains compatible.

Implement one protocol/role/path aware inspection traversal reused online and dry run. Cover Chat tool_calls.function.arguments (parse valid JSON strings while preserving valid shape and literal typed fields); Responses function/custom tool outputs and call arguments; Anthropic tool_use.input/tool_result; Gemini functionCall.args/functionResponse.response. Existing ordinary text/system/instructions coverage remains. Detect only appropriate content leaves, never schema keys/IDs/media carriers. Valid signed thinking and opaque continuation exempt only at authentic protocol locations/roles/types; spoofed names or objects do not bypass. Bounds: full preview 64KiB, depth32, segments4096, findings512; overflow rejects with safe explicit errors. Online traversal remains bounded by existing gateway body and these structural limits whenever policy is applied.

Per-rule exact-match allowlist max64 entries, <=256 UTF-8 bytes each, suppress matching spans only. Pattern order, role/scopes, enabled/state/test mode use same shared engine; denied preview returns decision deny with safe findings without sending anything upstream. Add built-in templates email/phone/API-key-like with fixed Rust-regex-compatible patterns, offered but not enabled/saved automatically. Template save/edit is the existing audited protection mutation. Public finding: rule_id, path, start/end UTF-8 byte offsets, action and fixed reason; no match/original-value field. Samples, findings and redacted_body live only in this response and never audit/request logs/durable sessions.

**RED/GREEN tests:** hidden secrets in arbitrary tool JSON leaves; numbers/keys/tool IDs preserved; exact allowlist doesn't suppress a second sensitive span; signature-named user/tool fields still checked; actual signed reasoning/encrypted replay untouched; role/scope-disabled/test behavior consistent; project auth rejects foreign; oversized/deep arrays fail closed; legacy text preview unchanged; samples absent from audit/DB/projection.
Example behavior case:
```rust
let payload = json!({"messages":[{"role":"assistant","tool_calls":[{"id":"call-1","type":"function","function":{"name":"lookup","arguments":"{\"email\":\"person@example.test\",\"count\":2}"}}]}]});
// The composed request/preview must redact only the email value, preserving call-1, lookup and count=2.
```
Focused: `cargo test --locked reference_privacy`; `cargo test --locked reference_privacy_preview`.
**Commit:** `feat(privacy): inspect structured tool content and explain request dry runs`.

- [ ] **Step 1: Write the named behavioral regressions below and run the focused command before implementing.** Record expected RED output in the task report. Use existing composed mock fixtures; do not weaken tests to bless the implementation.
- [ ] **Step 2: Implement the contract in the listed files, preserving existing defaults and public compatibility where specified.** Apply only this task's required changes.
- [ ] **Step 3: Run focused GREEN verification, then the relevant full suite once.** Backend tasks: `cargo fmt --all -- --check`, `cargo test --locked`. Frontend tasks: `pnpm --dir web lint`, `pnpm --dir web test`, `pnpm --dir web build`. New source may be formatted with `cargo fmt --all` before checks. Capture exact commands, exit codes and relevant counts.
- [ ] **Step 4: Read your own diff for scope, typed boundaries, leaked content and regressions; fix findings and repeat only covering checks.** Commit the implementation/tests atomically with the conventional subject specified below. Never include controller-owned specs/plans/docs in a source implementation commit.
- [ ] **Step 5: Write the task report, including RED/GREEN evidence, changed files, self-review and concerns.** Return DONE/DONE_WITH_CONCERNS/BLOCKED/NEEDS_CONTEXT plus commit and one-line test summary. No worker-spawned agents or reviewers.

## Task 6: Persist safe per-attempt conversion and affinity diagnostics

**Spec:** execution-diagnostics module.
**Files:** new small `src/providers/diagnostics.rs`, `src/providers/mod.rs`/`transforms.rs`, `src/api/errors.rs`/`gateway.rs`, `src/operations/lifecycle.rs`, `src/orchestration/affinity.rs`/`mod.rs`, `src/api/operations_api.rs`; corresponding focused tests.
**Consumes:** Task1 diagnostic arrays. **Produces:** execution/request/trace reads expose `conversion_diagnostics` and `affinity_diagnostics`; safe diagnostic DTOs with fixed codes/phase/path or rule/scope/channel/expiry identifiers.

Conversion schema {phase:'request'|'response',code,path?,reason}; <=64 items, code<=128 bytes, known-schema path<=256 bytes, reason a fixed template/code rather than formatted provider cause. Record failed unsupported request/response shapes and supported explicit normalizations on the owning attempt. Keep current safe public errors and reject before upstream. Unknown field names/payload values never become stored messages; known tools/messages fields may use fixed structural paths. Native pass-through does not invent transformations. Ensure execution exists and can store a rejected attempt if failure happens before send, without counting network contact or charging an uncontacted attempt.

Affinity schema {rule_id,scope_digest,reason,provider_id?,expires_at?}; <=16, fixed reason first/hit/expired/ineligible/established/released/candidate_failed. Reuse existing bounded cache and project/key/model/path hashing. Record explanation in durable execution metadata and existing decision trace. Preserve strict binding, eligibility override, disabled-key/project handling and old-completion protection; no new global release action or raw user session/cursor storage. Diagnostics updated/finalized with existing lifecycle transaction; malformed old arrays exposed as empty or safe explicit invalid metadata, never raw strings.

**RED/GREEN tests:** unsupported tools/nontext request has field/code, returns safe 4xx and sends no upstream call; native unknown fields round-trip; retry attempts get separate diagnostics; response failure diagnostic belongs to the actual attempt; alias/strict hit/expiry/ineligible/new establishment reasons; old failed completion can't release newer success; logs/audit/details no content or secrets; reopened DB preserves metadata; cross-project detail cannot read it; bound violations explicit.
```rust
assert_eq!(execution["conversion_diagnostics"][0]["phase"], "request");
assert_eq!(execution["conversion_diagnostics"][0]["code"], "unsupported_request_shape");
```
Use specific fixed codes for supported known rejection cases; keep a generic safe code for unknown shapes. Focused: `cargo test --locked reference_conversion_diagnostics`; `cargo test --locked reference_affinity_diagnostics`.
**Commit:** `feat(trace): explain protocol conversion and affinity per attempt`.

- [ ] **Step 1: Write the named behavioral regressions below and run the focused command before implementing.** Record expected RED output in the task report. Use existing composed mock fixtures; do not weaken tests to bless the implementation.
- [ ] **Step 2: Implement the contract in the listed files, preserving existing defaults and public compatibility where specified.** Apply only this task's required changes.
- [ ] **Step 3: Run focused GREEN verification, then the relevant full suite once.** Backend tasks: `cargo fmt --all -- --check`, `cargo test --locked`. Frontend tasks: `pnpm --dir web lint`, `pnpm --dir web test`, `pnpm --dir web build`. New source may be formatted with `cargo fmt --all` before checks. Capture exact commands, exit codes and relevant counts.
- [ ] **Step 4: Read your own diff for scope, typed boundaries, leaked content and regressions; fix findings and repeat only covering checks.** Commit the implementation/tests atomically with the conventional subject specified below. Never include controller-owned specs/plans/docs in a source implementation commit.
- [ ] **Step 5: Write the task report, including RED/GREEN evidence, changed files, self-review and concerns.** Return DONE/DONE_WITH_CONCERNS/BLOCKED/NEEDS_CONTEXT plus commit and one-line test summary. No worker-spawned agents or reviewers.

## Task 7: Pricing completeness and missing-price budget enforcement

**Spec:** pricing-completeness module.
**Files:** `src/db/schema.rs` for additive migration30 and focused upgrade tests; `src/operations/pricing.rs`, `src/operations/lifecycle.rs`, `src/operations/runtime.rs` startup interruption where needed, model create/update/catalog import/clone paths in `src/db.rs`/`src/api.rs`/`src/api/operations_api.rs`; request/usage/analytics response projection; focused accounting/isolation tests. Keep price versions immutable and existing calculations/ratios intact.
**Consumes:** Task1 pricing_configured/pricing_status. **Produces:** model CRUD/read `pricing_configured`; snapshots/execution/usage `pricing_status` legacy/priced/explicit_free/missing_price/incomplete_usage; analytic missing_pricing_count/measured-cost availability metadata.

Add migration30: `usage_logs.usage_measurement_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(usage_measurement_json))`. Store a bounded closed version-1 Boolean contract for existing exposed quantity measurements, independently of cost status and with protocol terminal/trust semantics. Historical `{}` is unspecified, never proof of zero; keep existing numeric columns and all financial history/snapshots unchanged. Expose `usage_measurement` through scoped usage/request/trace and live/rebuilt derived reads; per-quantity aggregates exclude unmeasured placeholders and retain measured zero. No request samples/raw values in metadata. Add migration/reopen/backup/history and priced-but-unreported-zero-rate-dimension regressions. Task9 consumes these authoritative flags for telemetry dashes.

A typed PricingStatus is separate from settlement_kind. Snapshot configuration status and immutable price version once per attempt; classify priced vs explicit_free only with complete trusted quantity and configured components. Missing/partial usage is incomplete_usage unless existing conservative reservation semantics apply (preserve conservative totals and label estimates separately). No price is missing_price; actual totals/spend remain integers. For hard-budget keys/profiles reject unknown price before contacting provider; explicit free is allowed subject to existing budget/admission rules, not a bypass. Snapshot/calculation failures remain fail-closed; no price refresh/lookup during a transaction.

Nonzero old model prices are configured. Zero without explicit pricing_configured=true or an explicitly saved price version is missing, including auto-discovered default models. Create/update omitted flag remains compatible (nonzero infers configured; preserve existing configured state on unrelated edits); explicit false with nonzero prices rejects. Clone/import/backup roundtrips preserve the flag or valid configured version. Catalog usable typed price facts can establish configuration; metadata absence cannot. Admin price CRUD creates an explicit version, including intentional zero price; legacy auto-created snapshots do not accidentally turn unconfigured model into explicit free.

Historical amounts/status legacy and prices remain unchanged. Expose status in authoritative observation rebuild/live feeds and project list/detail/usage/analytics so unknown placeholder zero isn't counted as measured price. UI task9 will show dash plus reason and unknown-count warnings. No automatic backfill or expression evaluator.

**RED/GREEN tests:** configured nonzero -> priced, configured zero -> explicit_free, auto-discovered zero -> missing_price; hard-budget missing rejects before mock contacted; nonbudget missing succeeds but status unknown; incomplete/stream cut usage remains conservative/unreported correctly; unrelated model edit preserves price flag; all create/import/clone paths distinguish; historical settled snapshots/costs/spend unchanged; repeated finalization idempotent and restart preserved; isolation and fail-closed overflow unchanged.
```rust
assert_eq!(result["pricing_status"], "missing_price");
assert_eq!(upstream_calls.load(std::sync::atomic::Ordering::SeqCst), 0); // for a hard-budget request
```
Focused: `cargo test --locked reference_pricing`; `cargo test --locked reference_missing_price_budget`.
**Commit:** `feat(accounting): distinguish missing prices from confirmed free usage`.

- [ ] **Step 1: Write the named behavioral regressions below and run the focused command before implementing.** Record expected RED output in the task report. Use existing composed mock fixtures; do not weaken tests to bless the implementation.
- [ ] **Step 2: Implement the contract in the listed files, preserving existing defaults and public compatibility where specified.** Apply only this task's required changes.
- [ ] **Step 3: Run focused GREEN verification, then the relevant full suite once.** Backend tasks: `cargo fmt --all -- --check`, `cargo test --locked`. Frontend tasks: `pnpm --dir web lint`, `pnpm --dir web test`, `pnpm --dir web build`. New source may be formatted with `cargo fmt --all` before checks. Capture exact commands, exit codes and relevant counts.
- [ ] **Step 4: Read your own diff for scope, typed boundaries, leaked content and regressions; fix findings and repeat only covering checks.** Commit the implementation/tests atomically with the conventional subject specified below. Never include controller-owned specs/plans/docs in a source implementation commit.
- [ ] **Step 5: Write the task report, including RED/GREEN evidence, changed files, self-review and concerns.** Return DONE/DONE_WITH_CONCERNS/BLOCKED/NEEDS_CONTEXT plus commit and one-line test summary. No worker-spawned agents or reviewers.

## Task 8: Operational probe and model-inventory console

**Specs:** probe-measurement, credential-model-discovery, model-capability-projection.
**Files:** `web/src/pages/ChannelsPage.tsx`, `ModelsPage.tsx`, shared document types/fields if needed, `web/src/i18n.ts`, focused page tests. Reuse task4 client setup components where related model metadata is shown; do not redesign shell/theme/navigation.
**Consumes:** task2 probe endpoint/credential/stream fields and distinct nullable timings; task3 credential status/count/time and bounded model metadata.

Probe form presents enabled selected-channel credentials, supported conversational protocol, streaming switch and model. Submission uses existing durable-job endpoint with the selected options. Query filters and error/retry/empty/loading are explicit; switching project/channel clears obsolete selection. Probe history shows protocol and separate headers/first event/first text; unknown is a dash, historical migrated header is not TTFT. Token generation rate requires measured output and first_text, never a fallback zero or a fabricated nonstream rate.

Credentials show safe known/stale/unmeasured model-inventory status and last success/count, without plaintext fingerprints/secrets. Model details expose nullable shared capability/limits/effort/modality facts, distinguish unknown/unsupported and label that routes may depend on request conditions. Keep row selectors/bulk/filters usable, localized and >=14px; avoid wider page overflow at375px.

**RED/GREEN tests:** choose credential/protocol/stream posts correct job payload; stale selection cleared on project change; unknown times and nonstream TPS dash; old header timing labeled separately; fresh/stale/unknown discovery labels; known false vs unknown metadata; query failures/retry/empty states; both translations. Focused: `pnpm --dir web test -- src/pages/referenceProbe.test.tsx src/pages/referenceInventory.test.tsx` (create these meaningful integration tests).
**Commit:** `feat(console): expose protocol probes and credential model inventory`.

- [ ] **Step 1: Write the named behavioral regressions below and run the focused command before implementing.** Record expected RED output in the task report. Use existing composed mock fixtures; do not weaken tests to bless the implementation.
- [ ] **Step 2: Implement the contract in the listed files, preserving existing defaults and public compatibility where specified.** Apply only this task's required changes.
- [ ] **Step 3: Run focused GREEN verification, then the relevant full suite once.** Backend tasks: `cargo fmt --all -- --check`, `cargo test --locked`. Frontend tasks: `pnpm --dir web lint`, `pnpm --dir web test`, `pnpm --dir web build`. New source may be formatted with `cargo fmt --all` before checks. Capture exact commands, exit codes and relevant counts.
- [ ] **Step 4: Read your own diff for scope, typed boundaries, leaked content and regressions; fix findings and repeat only covering checks.** Commit the implementation/tests atomically with the conventional subject specified below. Never include controller-owned specs/plans/docs in a source implementation commit.
- [ ] **Step 5: Write the task report, including RED/GREEN evidence, changed files, self-review and concerns.** Return DONE/DONE_WITH_CONCERNS/BLOCKED/NEEDS_CONTEXT plus commit and one-line test summary. No worker-spawned agents or reviewers.

## Task 9: Privacy, diagnostics and pricing console surfaces

**Specs:** privacy-inspection, execution-diagnostics, pricing-completeness.
**Files:** `web/src/pages/PromptsPage.tsx`, new focused `ProtectionRequestPreview.tsx` if necessary, `web/src/pages/ModelsPage.tsx` only explicit pricing control, `web/src/pages/shared.tsx`/`OperationsPage.tsx`/`observability.tsx` and existing cost presentation helpers as needed; i18n and corresponding focused tests. Use purpose-specific components to avoid expanding large pages with unrelated state.
**Consumes:** task5 templates/allowlists/full-request preview DTO; task6 safe execution diagnostics; task7 pricing statuses/unknown counts. Existing text preview remains accessible.

Full-request preview allows known protocol and bounded JSON request, clear inline JSON/size errors; renders active decision, matched/suppressed reason/path/offset entries and redacted resulting JSON. Explicit template selection saves through normal project protection mutation; forms label and validate exact-match allowlists. Clear sample/results on close/project change, no localStorage or backend sample persistence.

Execution/request/trace views render conversion phase/code/known path and affinity rule/reason/channel/expiry with localized labels, even when body capture is off. Do not render arbitrary HTML or raw diagnostic JSON as product copy. Keep unknown/malformed data graceful and identities scoped.

Model editing can confirm intentional free pricing (pricing_configured) with explicit label/help. All cost displays consume pricing_status separately from usage measurement: missing_price/incomplete_usage show dash and safe reason, explicit_free shows actual zero; legacy amounts remain historical with a localized legacy marker; reported/conservative/unreported remain distinguishable. Aggregates show measured totals and unknown-price request count so partial totals aren't shown as complete. No re-pricing action.

**RED/GREEN tests:** structured sample posts correct endpoint/body; invalid JSON/oversize blocks; suppression/path/decision localized; close clears sample; templates opt-in save audit API path; allowlist inline errors; diagnostics shown without payload; cross-attempt entries do not merge; missing price vs explicit free vs legacy/conservative; nonzero model edits and unrelated edits preserve pricing_configured; query error/retry/empty and zh/en. Focused tests named `referencePrivacy.test.tsx`, `referenceDiagnostics.test.tsx`, `referencePricing.test.tsx`.
**Commit:** `feat(console): explain privacy decisions diagnostics and pricing completeness`.

- [ ] **Step 1: Write the named behavioral regressions below and run the focused command before implementing.** Record expected RED output in the task report. Use existing composed mock fixtures; do not weaken tests to bless the implementation.
- [ ] **Step 2: Implement the contract in the listed files, preserving existing defaults and public compatibility where specified.** Apply only this task's required changes.
- [ ] **Step 3: Run focused GREEN verification, then the relevant full suite once.** Backend tasks: `cargo fmt --all -- --check`, `cargo test --locked`. Frontend tasks: `pnpm --dir web lint`, `pnpm --dir web test`, `pnpm --dir web build`. New source may be formatted with `cargo fmt --all` before checks. Capture exact commands, exit codes and relevant counts.
- [ ] **Step 4: Read your own diff for scope, typed boundaries, leaked content and regressions; fix findings and repeat only covering checks.** Commit the implementation/tests atomically with the conventional subject specified below. Never include controller-owned specs/plans/docs in a source implementation commit.
- [ ] **Step 5: Write the task report, including RED/GREEN evidence, changed files, self-review and concerns.** Return DONE/DONE_WITH_CONCERNS/BLOCKED/NEEDS_CONTEXT plus commit and one-line test summary. No worker-spawned agents or reviewers.

## Task 10: Documentation, fresh gates and release-binary browser QA

**Files:** controller-owned `docs/operations.md`, `docs/request-orchestration.md`, `docs/protocol-contracts.md`, `docs/axonhub-capability-matrix.md` and relevant spec/plan progress; README only update accurate feature/reference attribution if needed. Source fixes only through implementer/review loop, not an unreviewed controller patch.
**Consumes:** complete tasks1-9 APIs and UI. **Produces:** documented actual contracts, preserved matrix rows, fresh verification evidence and browser screenshots/artifacts.

Document new safe inputs, timing meanings and historical migration, visibility unknown/stale semantics, route metadata limits, no-token-recovery onboarding, typed privacy coverage/allowlists/sample handling, per-attempt diagnostic safety, pricing status and hard-budget behavior. Preserve Apache2 product licensing; reference Magpie MIT and AstrLink independent Apache2 behavior while excluding RelayKit/AGPL. Don't claim local end-user/on-device processing, OCR, semantic classifier, restoration, live-tested providers or distributed guarantees.

Run the exact repository gates from shared spec fresh, in order (web install/lint/test/build before Rust fmt/clippy/tests/release build; actionlint). Start built release binary with fresh isolated data dir and local mock upstream; initialize owner/project/key via API, populate operational data and probe requests. Browser automation MUST use agent-browser skill/core and release binary. Cover client setup, probe selection/history, credential inventory/model metadata, privacy preview including error state, request/execution diagnostics and missing/free price surfaces in zh/en light/dark at1440 and375, plus768 one smoke. Inspect accessibility tree, screenshots and console errors; record paths and actual results. Use ephemeral fixture credentials only; never print/commit live keys or sampled real data.

Final whole-branch GPT-6 review receives the full diff/specs, task reports/ledger and fresh verification evidence. Fix all blocking findings and obtain a clean scoped re-review. Ensure main checkout has no unrelated changes before integrating the reviewed branch; no remote push/publish. Keep source commits separate from this controller documentation commit.
**Verification:** all commands exit0, complete release QA cases pass, no open important/critical review findings, `git diff --check` clean; final report lists exact features and material remaining limitations.
**Commit:** `docs: document reference adoption contracts and verified behavior`.

- [ ] **Step 1: Write the named behavioral regressions below and run the focused command before implementing.** Record expected RED output in the task report. Use existing composed mock fixtures; do not weaken tests to bless the implementation.
- [ ] **Step 2: Implement the contract in the listed files, preserving existing defaults and public compatibility where specified.** Apply only this task's required changes.
- [ ] **Step 3: Run focused GREEN verification, then the relevant full suite once.** Backend tasks: `cargo fmt --all -- --check`, `cargo test --locked`. Frontend tasks: `pnpm --dir web lint`, `pnpm --dir web test`, `pnpm --dir web build`. New source may be formatted with `cargo fmt --all` before checks. Capture exact commands, exit codes and relevant counts.
- [ ] **Step 4: Read your own diff for scope, typed boundaries, leaked content and regressions; fix findings and repeat only covering checks.** Commit the implementation/tests atomically with the conventional subject specified below. Never include controller-owned specs/plans/docs in a source implementation commit.
- [ ] **Step 5: Write the task report, including RED/GREEN evidence, changed files, self-review and concerns.** Return DONE/DONE_WITH_CONCERNS/BLOCKED/NEEDS_CONTEXT plus commit and one-line test summary. No worker-spawned agents or reviewers.
