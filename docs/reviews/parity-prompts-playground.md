# Parity review — prompts, prompt protection and the playground

Re-derived from code, not from earlier chat summaries. AxonHub (`/home/czyt/code/others/axonhub`,
Apache-2.0/LGPL) is read as a behaviour reference only; no code or assets were copied.

**Snapshot.** Pangolin working tree at `e666bef` plus uncommitted modifications
(`src/api/operations_api.rs`, `web/src/pages/shared.tsx`, `web/src/Shell.tsx`, and others).
All Pangolin line numbers refer to that working tree and may drift while it is being edited.
Nothing was built or executed for this review: every Pangolin claim is a code path, and the
one behavioural chain that matters most is spelled out step by step under "Claims checked".

**Statuses.** `parity` — behaviour matches or Pangolin is a strict superset; `gap` — AxonHub
behaviour an operator can observe is missing or differs in a way that matters; `feature-build`
— a capability that does not exist in Pangolin at all; `refuted` — the suspected finding is not
real.

## Findings

| Finding | AxonHub | Pangolin today | Status | Work needed |
| --- | --- | --- | --- | --- |
| 1. No prompt-version concept | Prompts are mutable rows; no version pointer either (`internal/ent/schema/prompt.go`) | `activation_json` is the condition document, whose `version` is the schema version of that document (`src/orchestration/policy.rs:11-17` requires `version == 1`); no version column (`src/db/schema.rs:397-408`) and no version in the API projection (`src/api/operations_api.rs:298`) | refuted | None. The suspicion that `activation.version` is a version pointer is wrong |
| 2. Write-time validation of `activation` and protection `scopes` | Validates settings and regexes on create/update (`internal/server/biz/prompt.go:88-124`, `:132-134`) | `validate_conditions` on `activation` (`src/api/operations_api.rs:998-1004`), on `scopes` (`:1027-1030`), plus role whitelist (`:1005-1010`), action whitelist and regex compile (`:1017-1024`); same evaluator that runs per request (`src/orchestration/policy.rs:205-223`) | parity | None, except finding 3 |
| 3. Protection `scopes: null` bypasses validation and poisons the project | Not reachable: settings are a typed object | A cleared console JSON box serialises as `null` (`web/src/pages/shared.tsx:80`); the prompt arm normalises null (`:1000`) but the protection arm does not — validation is skipped for null (`:1027`) and the insert stores the string `null` (`:1031`). `protection::load` then fails `policy::document` (`src/orchestration/protection.rs:28`, `policy.rs:11-17`) → `Error::Configuration` → **500 on every gateway request of that project** (`src/orchestration/mod.rs:284`, `src/api.rs:142-144`) | gap | Normalise `scopes: null` to `{"version":1}` exactly as the prompt arm does, and add a red/green test for the cleared-box case |
| 4. Prompt injection returns 400 for non-conversational endpoints | Injection is a pipeline middleware that never fails on request shape (`internal/server/orchestrator/prompt.go:31-74`); prepend/append only touches `Messages` | `inject` handles only `/v1/responses*`, `/v1/messages`, `/v1beta/models:*`, `/v1/chat/completions`, else returns `Error::Invalid` (`src/orchestration/protection.rs:125-129`) → 400 (`src/api.rs:140`). `prepare` runs it for every endpoint before any upstream call (`src/api/gateway.rs:148`, `src/orchestration/mod.rs:291`) | gap | Skip injection (no error) for endpoints that carry no message list; keep the per-endpoint placement logic |
| 5. New prompts default to enabled | `status` defaults to `disabled` (`internal/ent/schema/prompt.go:58-60`; create passes `SetNillableStatus`, `internal/server/biz/prompt.go:158`) | Implemented across DB/API/form: v11 preserves existing rows while new prompts default disabled; omitted update fields preserve their prior state. | parity | None; keep fresh/upgrade schema, HTTP default and form payload tests green |
| 6. No `append` action | `prepend`/`append` (`internal/objects/prompt.go:5-11`), applied in `ApplyPrompts` (`internal/server/biz/prompt_matcher.go:116-159`) | Implemented as a constrained prompt action and applied after the existing conversation in Chat, Messages, Gemini and Responses. | parity | None; keep four-protocol placement tests green |
| 7. No `order` field | `field.Int("order")`, "smaller values are inserted first" (`internal/ent/schema/prompt.go:61-64`), sorted by order then `CreatedAt` (`prompt_matcher.go:121-127`) | Implemented as an integer record/API/UI field; enabled prompts load by `order,created_at,id`. | parity | None; keep ordering/tie-break and projection tests green |
| 8. Conditions cannot match an API key | `api_key` condition type (`internal/objects/prompt.go:29-31`, `internal/server/biz/prompt_matcher.go:97-103`, key id from context `prompt.go:51-56`) | Context is `{body, headers, endpoint}` and every key/token/authorization header is stripped (`src/orchestration/policy.rs:303-330`); no principal field exists | gap | Add the API-key id (or a stable hash) to the condition context; document it as a condition field |
| 9. No bulk enable/disable for prompts | Bulk enable/disable/delete dialogs (`frontend/src/features/prompts/components/prompts-bulk-*.tsx`), inline status switch (`prompts-columns.tsx:20-42`) | `bulk-toggle` accepts only channels, models and credentials (`src/api/operations_api.rs:1045-1055`); the generic table has no row selection at all (`web/src/pages/shared.tsx:118-123`) | gap | Extend `bulk-toggle` to prompts/protection and add row selection |
| 10. Prompt list hides what a prompt does | Columns: name, order, description, role, action type, condition count, status, created (`frontend/src/features/prompts/components/prompts-columns.tsx:58-174`) | Columns: name, role, content, enabled (`web/src/pages/PromptsPage.tsx:16`); `activation` is only visible by opening the JSON editor | gap | Show the activation summary (or at least "conditional") and the role badge in the table |
| 11. Protection rules have no preview and no metadata | Interactive preview of pattern against sample text (`internal/server/biz/prompt_protection_preview.go:22-56`, dialog at `frontend/src/features/prompt-protection-rules/components/rules-action-dialog.tsx:283-320`); `description` and `enabled/disabled/archived` (`internal/ent/schema/prompt_protection_rule.go:47-57`) | Implemented with description + active/archived metadata, archived enforcement exclusion and a bounded project-scoped read-only preview over the production matcher, surfaced in a bilingual dialog. | parity | None; keep preview bounds/read-only/enforcement and dialog state tests green |
| 12. Duplicate prompt name is a generic 500 | Duplicate name is checked and reported (`internal/server/biz/prompt.go:136-149`) | No pre-check; `UNIQUE(project_id,name)` (`src/db/schema.rs:407`) is not the `ON CONFLICT` target (`src/api/operations_api.rs:1011`), so the constraint error becomes `ApiError::Internal` → 500 "An internal error occurred" (`src/api.rs:119-125`) | gap | Pre-check the name inside the transaction and return a conflict with the offending name |
| 13. Playground is a single-turn form, not an admin chat | Multi-turn chat on the console session: streaming, history, reasoning parts, image attachments, regenerate/clear/stop, channel-or-model-gateway source, system prompt, temperature slider (`frontend/src/features/playground/index.tsx:94-651`; server route `/admin/playground/chat`, `internal/server/routes.go:133-140`) | Implemented as a session+CSRF route that delegates an existing project key into the ordinary gateway pipeline without exposing its token. The console carries multi-turn text, bounded images, reasoning, source selection, stop/regenerate/clear and authoritative usage/request links. | parity | None; keep session policy/accounting and five Playground suites green |
| 14. Playground model picker is not endpoint-filtered | Picker is driven by enabled `chat`-type models or a chosen channel (`playground/index.tsx:142-155`, `:319-328`) | Implemented with server-side `/v1/responses` filtering through `visible_models_for`; the selection is scoped to project+key and stale responses cannot restore an invalid model. | parity | None; keep route and picker race/empty/retry tests green |
| 15. Condition language | Typed conditions: `model_id`, `model_pattern`, `api_key` (`internal/objects/prompt.go:20-48`) | JSON-Pointer tree over `{body, headers, endpoint}` with `all`/`any`, `eq/ne/in/contains/regex/gt/...` (`src/orchestration/policy.rs:205-301`) | parity | None; a strict superset except the API-key field in finding 8 |
| 16. Multi-protocol placement | One message list (`llm.Request.Messages`) | Native placement per protocol: responses `input`, Anthropic `system` blocks, Gemini `systemInstruction` + `contents`, chat `messages` (`src/orchestration/protection.rs:57-124`) | parity | None; broader than the reference |
| 17. Protection engine | Regex `pattern`, `mask`/`reject`, role scopes, matched rules reported (`internal/server/biz/prompt_protection_request.go:30-103`) | Regex over content and role pattern, `deny`/`redact`, scopes as a condition document, `test_mode`, nested/tool-output traversal (`src/orchestration/protection.rs:133-286`) | parity | None |
| 18. Scoping, audit and deletion | Rules are global; soft delete + archived state | Everything is project-scoped by both query scope and `project_id` (`src/api/operations_api.rs:295-304`), each mutation is audited (`:1317-1320`), delete is a hard delete with an audit record (`:1330-1353`) | parity | None. Hard delete vs soft delete is a deliberate difference, not a defect |
| 19. List search and pagination | Relay connection with search | Server-side `LIKE` over the projected document plus offset/limit (`src/api/operations_api.rs:408-425`), wired into the shared table (`web/src/pages/shared.tsx:98-104`, `:144-151`) | parity | None |
| 20. Localisation | zh-CN and en locale files | Both pages fully localised, `prompts`/`protection`/`overrides` and all playground copy (`web/src/i18n.ts:27-37`, `:94-101`) | parity | None |

Counts: **6 gap, 0 feature-build, 1 refuted, 13 parity.**

## Claims checked

- **(a) "No prompt-version concept; `activation_json.version` is the condition-document schema
  version."** Confirmed. `policy::document` requires `version == 1` on every condition document
  (`src/orchestration/policy.rs:11-17`); `prompts` has no version column
  (`src/db/schema.rs:397-408`) and the list projection returns none
  (`src/api/operations_api.rs:298`). Recorded as `refuted` (the suspected version concept does
  not exist).
- **(b) "Prompt `activation` and protection `scopes` are validated at write time."** Confirmed
  for both, in the same upsert path used by create and update
  (`src/api/operations_api.rs:1003-1004`, `:1027-1030`), with the role whitelist and action/regex
  checks alongside. One hole, recorded as finding 3: the protection arm skips validation when
  `scopes` is `null` and then persists the string `null`, which the loader rejects at request
  time. So the write-time guarantee is real but not complete, and the hole is the same
  cleared-JSON-box case the prompt arm's comment names.
- **(c) "AxonHub prompts have an `append` action and an `order` field, and conditions can match
  an API key."** All three confirmed in AxonHub; all three absent in Pangolin (findings 6, 7, 8).
  `append` and `order` are named as parity targets by `docs/axonhub-capability-matrix.md:102`
  ("Prompt records with activation conditions and enable/bulk lifecycle").
- **(d) "AxonHub defaults new prompts to disabled, Pangolin to enabled, and Pangolin 400s
  non-conversational endpoints when any prompt matches."** Both confirmed, with three
  corrections to the blast radius:
  1. It is not "any prompt" but any **enabled** prompt whose activation matches; the default
     activation `{"version":1}` matches everything (`policy.rs:230`), and the default for
     `enabled` is true on all three layers (finding 5). So adding one prompt and leaving the
     defaults alone is enough to trigger it.
  2. `/v1/completions` is also broken, and that endpoint is conversational, so "non-conversational"
     understates it. The affected set is every endpoint in `src/providers/mod.rs:38-57` except
     `/v1/chat/completions`, `/v1/responses`, `/v1/responses/compact`, `/v1/messages` and the two
     Gemini `:generate*` actions: legacy completions, embeddings, moderations, alpha search,
     images, videos, audio, rerank and the Doubao task route.
  3. It fires before any candidate or upstream work (`src/api/gateway.rs:148` →
     `src/orchestration/mod.rs:291`), so it is a deterministic 400 with no provider traffic, and
     the message ("prompt injection is unsupported for this endpoint; scope prompts to a
     conversational endpoint") does not name the prompt responsible.
  AxonHub's equivalent middleware has no endpoint-shape failure path at all
  (`internal/server/orchestrator/prompt.go:31-74`), so this is a Pangolin-only defect.

## Residual uncertainty

- Finding 3's 500 is derived from reading the chain `shared.tsx:80` → `operations_api.rs:1031` →
  `protection.rs:28` → `policy.rs:11-17` → `mod.rs:284` → `api.rs:142-144`; no build or request
  was run (out of scope for this task), and no existing test covers it — the prompt/protection
  orchestration test uses `/v1/chat/completions` only (`src/orchestration/tests.rs:200-211`,
  `:351-390`).
- The same un-normalised `null` pattern exists for `model_associations.conditions`
  (`src/api/operations_api.rs:951`), where validation happens to reject it with a 400
  (`:926`); credential `settings` (`:861`, `:863`) shares the pattern and was not traced.
- AxonHub was read, not run: I did not observe its embeddings path end-to-end, only that its
  injection middleware has no error return for request shape.
- Finding 13 is sized by reading the reference UI; the effort to build a session-authenticated
  chat playground is not estimated here.
