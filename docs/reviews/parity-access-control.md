# Parity review — access control (Access page, members, invitations, OIDC, API-key lifecycle)

Scope: the Access page (keys, key profiles, projects, users, members, roles, invitations, OIDC
providers, OIDC identities), project members and invitations, OIDC sign-in, and API-key
lifecycle/status presentation.

Method: re-derived from Pangolin's own code (paths are relative to the repository root) and from
AxonHub's console/Go routes (paths are relative to `/home/czyt/code/others/axonhub`), read as a
behaviour reference only. No code or assets were copied. Pangolin evidence is the working tree as
of this review; several files are modified but uncommitted (see the note under the table). No
build, test or lint command was executed for this review.

Status legend:

- `parity` — the operator-visible behaviour matches.
- `gap` — the experience is missing in Pangolin's console although the data or route already exists.
- `feature-build` — the capability does not exist in Pangolin at all (new route, column or flow).
- `refuted` — the difference was claimed but does not hold.

| # | Finding | AxonHub | Pangolin today (evidence) | Status | Work needed |
|---|---|---|---|---|---|
| 1 | Per-project members surface exists | Dedicated "Project Users" page with toolbar, pagination, bulk selection and row actions (`frontend/src/features/proejct-users/components/users-table.tsx:133`, `:64`) | A `members` tab on the Access page with list/add/remove, loading, error+retry and empty states (`web/src/pages/AccessPage.tsx:12`, `:43`, panel `:339-453`) | parity | None for existence; see rows 2–4 for the surface's depth |
| 2 | Member rows identify users only by opaque id | Columns show first/last name, email, owner badge, role badges and scope badges (`proejct-users/components/users-columns.tsx:38-91`, `:94-113`) | Renders `row.user_id` as a `Code` cell (`web/src/pages/AccessPage.tsx:409`); `access::MembershipView` does not join the user (noted at `AccessPage.tsx:344-346`) | gap | Add display fields (email/name) to `MembershipView`, or resolve them in the console |
| 3 | A member's role or status cannot be changed in place | Row action opens a dialog that toggles roles and scopes for an existing member (`proejct-users/components/project-user-action-dialog.tsx:172-175`) | Implemented as an in-place role/status dialog over the existing POST upsert, with project-switch-safe refresh and protected owner actions. Pangolin deliberately keeps one role per membership. | parity | None; keep edit/owner/race tests green |
| 4 | Members list has no search, pagination or mobile layout | Toolbar (search, view options) plus pagination (`proejct-users/components/users-table.tsx:133`, `:187`) | Implemented through the shared resource shell: search, pagination, desktop table, mobile cards and explicit query states. | parity | None; keep shared-shell/member responsive tests green |
| 5 | Members tab is unreachable for the principal it was granted to | n/a (AxonHub has no equivalent permission gate) | `ACCESS_TABS` grants `members` to `project:read` (`AccessPage.tsx:12`) and the system `member` role holds exactly `project:read`+`gateway:use` (`src/db/schema.rs:643-644`), but the Access nav link requires one of `project:manage, api_key:manage, role:manage, user:manage, oidc:manage` (`web/src/Shell.tsx:34`) | gap | Include `project:read` in `canAccess`, or gate the tab on a stronger permission |
| 6 | No SSO/OIDC entry point on the sign-in page | Sign-in lists one button per active provider and can enforce provider-only login (`auth/sign-in/components/user-auth-form.tsx:29`, `:45`, `:134-183`) | Implemented: `Login` reads the minimal unauthenticated enabled-provider list and renders localized native links to the encoded server `start` route. Empty/failure states preserve password login without false claims. Login-only enforcement remains row 14. | parity | None for the entry point; keep `Auth.test.tsx` green |
| 7 | OIDC identities can be created but never listed or unlinked | Provider list and account linking are first-class surfaces (`auth/sign-in/components/user-auth-form.tsx:148-183`) | Implemented: provider picker, subject/user binding list, explicit query states, confirmed audited unlink and captured-context refusal. The existing admin create form remains available. | parity | None; keep identity panel and audit tests green |
| 8 | No self-service account linking | Signed-in user can start a link flow (`internal/server/routes.go:131`) | Implemented under Account: authenticated CSRF-protected POST start, safe HTTP(S) navigation, durable one-time link intent with captured user, conflict refusal and transactional audit. | parity | None; real-IdP browser contract remains in final QA |
| 9 | Key creation hardcodes type, owner and scopes, and never shows scopes | Create dialog exposes type, profiles and scopes; the table shows the active profile (`apikeys/components/apikeys-columns.tsx:220-226`) | Implemented: all four backend types, active same-project owner selection for user/personal, all five seeded project scopes, inline validation/server errors and a scopes column. The server still authorizes every delegated scope and the view exposes a real array. | parity | None; keep `accessKeyState.test.tsx` and the Rust delegation/serialization tests green |
| 10 | Key status hides expiry, spend and last use | Table has a status cell (enabled/disabled/archived) plus createdAt/updatedAt and a per-key token chart (`apikeys/components/apikeys-columns.tsx:194-213`, `:227-241`) | Admission rejects a key that is disabled, expired or out of budget (`src/db.rs:519-533`), yet the only status control is an enable toggle (`AccessPage.tsx:109-119`) and there is no expiry, spend or last-used column, so an expired or exhausted key reads "Enabled". `expires_at` is returned (`src/access.rs:1368`); `spent_micros`/`last_used_at` are stored (`src/db/schema.rs:89,91`) but not in `ScopedApiKeyView` (`src/access.rs:436-451`) | gap | Derive a status (enabled/disabled/expired/budget-exhausted/owner-inactive) and show expiry, last use and spend |
| 11 | Per-key usage and cost are unreachable from the key | Per-key token usage dialog with Today/7-day/all windows (`apikeys/components/data-table-row-actions.tsx`, `api-key-token-chart-dialog.tsx`) | Implemented from each key row over the project-scoped analytics endpoint: overall input/output/cache/total tokens, cost and top models, with loading/error/retry and truthful unmeasured marks. Foreign and unknown key filters are opaque 404s. | parity | None; keep `keyUsage.test.tsx` and the analytics isolation test green |
| 12 | No rotate, archive or bulk key operations | Rotate, archive/restore and bulk lifecycle (`apikeys/components/data-table-row-actions.tsx`, `apikeys-*dialog.tsx`) | Implemented as terminal audited archive plus atomic one-time rotate and bulk enable/disable/archive. Archived keys cannot authenticate, edit, rotate or re-enable; archive remains available for emergency revocation even when an owner is suspended. | parity | None for the approved terminal-archive contract; keep lifecycle/rollback/UI tests green |
| 13 | Invitations are single-use and email-bound | Invite dialog issues a reusable link with expiry presets (1/6/24/168 hours, 0 = never) and `maxUses` 1 or unlimited, no email (`proejct-users/components/users-invite-dialog.tsx:20-23`) | B27 adds finite 1–100 reuse while preserving email binding and the 60s–30d lifetime. Acceptance conditionally claims one use in the same transaction as membership and audit; exhausted tokens remain opaque. | fixed | Reuse, exhaustion, rollback and migration tests plus the invitation console test |
| 14 | OIDC providers have no login-only mode or branding | Providers carry login-only and display branding | Implemented with server-enforced all-provider login-only mode, display name, validated button color and bundled logo key. Remote icon URLs are intentionally replaced by license-compatible local icon lookup/fallback. | parity | None; real-IdP/browser theme matrix remains final QA |
| 15 | Role editor edits raw permission JSON; no permission catalog in the console | Scope picker in the role dialog and scope chips in the table | Implemented: separate typed permission-catalog route, actor-filtered delegation boundary, accessible create/edit picker and unknown stored-slug preservation; writes independently enforce anti-escalation. | parity | None; keep catalog/service/UI tests green |
| 16 | Role bindings can be created but never listed or revoked | Role assignment is a picker over the role list (`project-user-action-dialog.tsx:172-175`) | `AssignmentsPanel` takes raw `user_id`/`role_id` text inputs (`AccessPage.tsx:217-218`); `GET`/`DELETE /users/{user_id}/role-bindings` (`src/access_api.rs:71`, `:75`) have no console consumer | gap | Replace the id inputs with pickers and add the binding list with revoke |
| 17 | Create-user form offers controls the backend ignores | Add-user dialog covers name, email, status and roles | The form offers an `enabled` switch and the table shows `role` (`AccessPage.tsx:41`), but `UserInput` has neither (`src/access.rs:265-270`) and `create_user` always inserts `role='member', enabled=1` (`src/access.rs:722`) | gap | Drop the switch or implement it; show how the role is assigned |
| 18 | Key-profile templates | Save/load/transfer profile templates (`apikeys-save-template-dialog.tsx`, `apikeys-load-template-popover.tsx`, `apikeys-template-transfer-dialog.tsx`) | Profiles are a plain `ResourcePage` with name, RPM, TPM, budget, routing policy, mappings and allowed models (`AccessPage.tsx:35`) | feature-build | Add profile templates if operators need reuse |
| 19 | Invitation inspect/accept flow | `/sign-up?invite=` with token inspection and registration | `/invite?token=` page inspects then accepts, with password and display name for new accounts (`web/src/Auth.tsx:73-142`), over `POST /api/v1/invitations/inspect|accept` (`src/access_api.rs:94-95`) | parity | None |
| 20 | Member routes are unconsumed by the console | — | Refuted: the members tab reads `GET /projects/{id}/members` (`AccessPage.tsx:360`), writes `POST` (`:367`) and deletes `DELETE /members/{user_id}` (`:374`), matching `src/access_api.rs:34-41` | refuted | None |
| 21 | Invitation routes are unconsumed by the console | — | Refuted: the invitations tab reads `GET`, creates `POST` and deletes `DELETE /invitations/{id}` (`AccessPage.tsx:280`, `:283`, `:284`), matching `src/access_api.rs:42-48` | refuted | None |

## Counts

- `parity`: 12 (rows 1, 3, 4, 6, 7, 8, 9, 11, 12, 14, 15, 19)
- `gap`: 5 (rows 2, 5, 10, 16, 17)
- `feature-build`: 2 (rows 13, 18)
- `refuted`: 2 (rows 20, 21)

## Note on the in-flight members workstream

The members tab is **already in the working tree**, not merely planned: `git status` lists
`web/src/pages/AccessPage.tsx` as modified (+123/−6), the diff adds `ACCESS_TABS`'s `members` entry,
`<Tabs.Panel value="members">` and `MembersPanel`, and the untracked
`web/src/pages/accessMembers.test.tsx` covers the panel. Both languages carry the new keys
(`web/src/i18n.ts:67`, `:131`), which the locale guard test checks. The work is uncommitted, and I
did not execute the tests. Two consequences of that work are worth fixing before it lands: rows 5
(the tab is granted to `project:read` while the nav link is not) and 4 (it bypasses the shared
table shell used by every other Access tab).

Pangolin evidence above reflects the working tree, so any row that cites a modified file
(`AccessPage.tsx`, `Shell.tsx`, `shared.tsx`) may differ from the last commit. Files with no
uncommitted change — `src/access.rs`, `src/access_api.rs`, `src/oidc.rs`, `src/db.rs`,
`src/db/schema.rs`, `src/api.rs`, `src/api/operations_api.rs`, `web/src/Auth.tsx` — match `HEAD`.
