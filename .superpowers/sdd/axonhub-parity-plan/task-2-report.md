# Task 2 report: authorization, projects, users, roles, API keys and OIDC

## Status

Implemented on `feat/axonhub-parity` in the isolated `.worktrees/axonhub-parity` worktree. The parent `main` checkout was not edited. Task 2 is covered by focused database/domain tests and the existing local password/session flow remains compatible.

## Delivered behavior

### Principals and authorization

- Added a single fail-closed permission evaluator for session and API-key principals.
- Session permissions resolve deterministically from global role bindings, active project memberships and project role bindings.
- Project role bindings only contribute scopes while the user has an active membership in that project.
- API-key principals are permanently confined to their owning project and can never exercise system-level permissions.
- `*`, exact permission slugs and the documented `project:manage` management umbrella are resolved explicitly; invalid/malformed scope JSON yields no scopes.
- Scope delegation is bounded: role and API-key creators cannot grant permissions they do not possess.
- Existing provider/model/key/observability admin handlers now require explicit default-project permissions instead of accepting any authenticated session.

### Projects, users, roles and invitations

- Added audited project list/get/create/update/delete behavior; the default project cannot be deleted.
- Added audited user list/create/update/delete and self-service profile/password updates. Self password changes require the current password; the active user cannot deactivate or delete itself.
- Disabled users can no longer authenticate through password or existing sessions. Deactivation revokes personal API keys.
- Added project membership list/upsert/remove behavior with owner protection.
- Added custom project role CRUD with permission validation and immutable seeded system roles.
- Added global/project role-binding list/create/delete behavior, including membership-dependent project bindings.
- Added invitation list/create/inspect/accept/revoke. Tokens are returned once, stored only as hashes, expire, and are atomically single-use. Existing-email acceptance requires the matching authenticated user.

### Scoped API keys

- Added project-scoped API-key list/create/update/delete APIs with type, owner, profile, scopes, expiration, budget and IP policy fields.
- User/personal keys require an active member owner; profile/project consistency remains enforced by Task 1 foreign keys.
- Expired and disabled keys fail authentication. Gateway scope checks parse JSON exactly instead of using substring matching.
- Legacy default-project API-key endpoints remain available and are now restricted to the default project.

### OIDC

- Added audited, secret-redacted OIDC provider CRUD and manual identity link/unlink APIs.
- Client secrets are encrypted with `SecretBox`; provider responses expose only `secret_configured`.
- Authorization starts use only the configured `PANGOLIN_PUBLIC_URL`, never the request Host header; unsafe non-loopback HTTP URLs are rejected.
- Discovery issuer equality and HTTPS/loopback endpoint safety are validated before credentials are sent.
- Added RFC 7636 S256 PKCE with URL-safe verifier generation. State is hashed, verifier encrypted, expiring and atomically consumed exactly once before token exchange.
- Callback exchange uses discovery, authorization-code form exchange and the userinfo endpoint.
- JIT identity creation/linking requires a non-empty subject and does not accept explicitly unverified email. Existing verified-email users are linked, identities remain stable by provider+subject, and configurable claim/group mapping selects a deterministic project role.

### Schema and audit

- Added migration ledger version 3 with user profile/status columns, OIDC state storage, management permission slugs and deterministic system-role grants.
- Initial owner setup now receives a global owner role binding while retaining default-project membership.
- Task 2 control-plane mutations write audit rows inside the same SQLite transaction as the mutation, so either both commit or neither does.

## Admin/public REST surface

- `/api/admin/v1/projects` and project detail
- project members, invitations, roles and scoped API keys
- `/api/admin/v1/users` and user role bindings
- `/api/admin/v1/oidc/providers` and provider identities
- `/api/v1/invitations/inspect` and `/accept`
- `/api/v1/auth/oidc/{provider_id}/start` and `/api/v1/auth/oidc/callback`

## Behavioral tests

- Cross-project session access is denied.
- Project role scopes require active membership.
- API-key principals cannot cross projects or access system permissions.
- Invitation tokens are hashed, expiring and one-time.
- OIDC PKCE challenge/verifier behavior is RFC-compatible; states expire and cannot be replayed.
- OIDC JIT creates one stable linked user and maps sorted group claims to the configured role.
- OIDC callback base URL is configuration-only and rejects unsafe/missing public URLs.
- Local email/password login still issues the existing strict HTTP-only session cookie.
- Existing Task 1 foreign-key, role-scope, API-key-profile and idempotent-migration tests remain green.

## Verification

- Web assets were built before Rust test execution: `pnpm --dir web build`.
- Web lint/test/build: clean; Vitest 1 passed, 0 failed.
- Focused and full Rust tests: `cargo test --locked --no-fail-fast` — 20 passed, 0 failed.
- Rust lint: `cargo clippy --locked --all-targets -- -D warnings` — clean.
- Formatting: `cargo fmt --all -- --check` — clean.
- Release: `cargo build --release --locked` — succeeded.

## Concerns and deliberate boundaries

- OIDC uses the discovery `userinfo_endpoint` as the validated claim source after authorization-code exchange; direct ID-token signature/JWKS validation is not used as a substitute for userinfo.
- Provider/project routing of gateway traffic remains Task 3. Task 2 confines management principals and API-key ownership but does not introduce the orchestration pipeline.
- Existing pre-Task-2 provider/model/default-key DB helpers still perform their historical mutation followed by an audit insert; all newly introduced Task 2 mutations are transactionally audited. Converting those older control-plane helpers to shared transactions belongs with their project-scoped CRUD expansion.
- No real identity provider was contacted. OIDC behavior is local contract-tested, not live-tested.

## Defensive review fix round 1/5

Commit scope: follow-up hardening after `ba317fa`, with composed Axum HTTP tests and an in-process mock OIDC provider.

### Root causes and fixes

- Principal identity was flattened to `user_id` at invitation/self-service boundaries. Invitation acceptance now accepts the complete principal, permits an existing account only for the matching interactive session, forbids API-key/new-account identity switching, and never upgrades an API-key principal to a session. Self-service profile/password authority is likewise session-only.
- Permission checks guarded the mutation type but not the authority being delegated. A shared grant-ceiling check now validates role scope and every permission granted by role binding, membership and invitation mutations. Project owner (`*`) assignment therefore requires an actor who already possesses `*`.
- API-key authentication had no trusted network input or owner lifecycle reconciliation. The server now propagates only the socket peer address through a spoof-resistant private header, parses exact IP/CIDR allow/deny policy fail-closed, and applies it to both admin and gateway authentication. User/personal owners must be enabled active members. Password/status/member mutations transactionally revoke sessions and/or both user/personal key types; authentication also independently checks current owner/member state.
- Project owner updates only changed the project row. Transfers now validate an enabled owner, enforce the owner-role delegation ceiling and transactionally upsert an active owner membership. Every membership mutation preserves the official owner as active with the owner role.
- OIDC state was bearer-only and the general redirect-following HTTP client carried credentials. State now includes a separately hashed browser correlation value in an HttpOnly, SameSite=Lax, Secure-on-HTTPS callback cookie; a mismatch is checked in the atomic delete predicate and does not consume the state. OIDC uses a dedicated redirect-disabled client for discovery, token and userinfo calls.
- Automatic OIDC email linking accepted an absent verification claim. New JIT/email links now require the configured verification claim to be explicitly `true`; manual administrator linking remains the explicit alternative. An already-linked provider subject does not depend on email relinking.
- Established OIDC identities previously updated claims outside an audit/mapping lifecycle. Every login now updates claims, reapplies deterministic group-to-role mapping, writes the login audit and commits those effects in one transaction. Official project owners cannot be demoted by claim mapping.
- Schema ledger version 4 adds the OIDC browser-binding hash without persisting the correlation plaintext.

### Red evidence

- `cargo test access_api::tests::api_key_ -- --nocapture` initially failed both new principal-separation tests: invitation acceptance and self-profile edit returned `200`, expected `403`.
- `cargo test scoped_api_key_ip_policy_uses_trusted_connect_address -- --nocapture` initially returned `200`, expected `401`, proving IP policy/socket context was ignored.
- `cargo test project_manager_cannot_delegate_owner_role_via_membership_or_invitation -- --nocapture` initially returned `200`, expected `403`, proving role authority was not bounded by the grantor.
- The first full run after adding ledger v4 caught two stale schema assertions (`27 passed, 2 failed`); expectations were corrected to version 4 / three ledger rows while preserving the two `backup_runs` foreign keys.
- The mock-IdP regressions exercise baseline-sensitive behavior at the public seam: wrong-browser callback followed by correct-browser reuse, omitted `email_verified`, token/userinfo redirects to a credential sink, and a second login with changed groups. These would respectively bypass correlation, auto-link, follow redirects, and retain stale role/audit state on the reviewed baseline.

### Green evidence and coverage

- Principal separation: API-key invitation acceptance and API-key self-service profile mutation both return `403`; no session cookie is emitted.
- Browser binding: wrong correlation returns `400`, and the same state still succeeds with the original cookie, proving validation precedes one-time consumption.
- Verification: a mock userinfo response without `email_verified:true` cannot auto-link to the existing owner (`403`).
- Redirect safety: mock token and userinfo `307` responses are rejected (`401`) and the sink receives zero requests.
- Repeat login: changed groups replace owner mapping with member mapping; two same-transaction login audits are present.
- Delegation ceilings: project manager attempts to assign/invite/bind the owner role all return `403`.
- Trusted IP: an allowed peer succeeds, a mismatched peer fails, and a spoofed private header without peer metadata fails.
- Lifecycle: password reset and user disable revoke sessions; disable, suspension and membership removal revoke both user/personal keys; disabled owners cannot receive new user keys.
- Ownership: transfer creates owner authority for a previously unjoined enabled user, while demotion/suspension returns `400`.
- Full post-fix verification is recorded in the final handoff below.

### Mature security primitives

- Added `oauth2` 5.0.0 with default features disabled (MIT OR Apache-2.0; Rust 1.65 minimum) for random CSRF state/browser-correlation values and RFC 7636 S256 verifier/challenge generation. This replaces the local SHA-256/base64 PKCE implementation without pulling in a second HTTP stack.
- Added `ipnet` 2.x (MIT OR Apache-2.0) for correct IPv4/IPv6 address and CIDR parsing/matching instead of hand-rolled prefix logic.
- Retained Pangolin-specific code for SQLite state envelopes, correlation-cookie policy, discovery issuer/HTTPS validation, redirect-disabled `reqwest` transport, userinfo claim mapping and transactional audit writes. These are application policy/integration seams rather than generic protocol primitives; adopting the full `openidconnect` client here would duplicate the existing HTTP/runtime stack and introduce ID-token/JWKS behavior outside this userinfo-based task contract.
- URL parsing/form encoding use `reqwest::Url` and `reqwest` form serialization rather than local encoders. Cryptographic state/verifiers remain hashed/encrypted at rest through the existing `blake3` token hash and XChaCha20-Poly1305 `SecretBox`.

### Fix-round final verification

- `pnpm --dir web build` — succeeded before Rust verification.
- `pnpm --dir web lint` — clean.
- `pnpm --dir web test` — 1 file / 1 test passed.
- `cargo fmt --all -- --check` — clean.
- `cargo clippy --locked --all-targets -- -D warnings` — clean.
- `cargo test --locked` — 29 passed, 0 failed.
- `cargo build --release --locked` — succeeded.

## Independent handoff verification (2026-09-20)

- Re-read the complete unstaged diff against `ba317fa`, the Task 2 brief, this report and the review ledger. The changes remain limited to Task 2 authorization/OIDC hardening, its required `ipnet`/`oauth2` dependency updates, and this report. Controller-owned `CAPABILITY_MAP.md`, `docs/`, `specs/` and `tasks/` work remains explicitly unstaged.
- A suspected custom-role delegation gap was re-checked and rejected: both custom-role creation and update already authorize every requested permission before any role mutation. No extra code correction was needed in this handoff.
- `git diff --check` — clean.
- `pnpm --dir web build` — succeeded.
- `pnpm --dir web lint` — clean.
- `pnpm --dir web test` — 1 file / 1 test passed.
- `cargo fmt --all -- --check` — clean.
- `cargo clippy --locked --all-targets -- -D warnings` — clean.
- `cargo test --locked --no-fail-fast` — 29 passed, 0 failed.
- `cargo build --release --locked` — succeeded.

## Defensive review fix round 2/5

- Root cause: the repeat-OIDC-login membership upsert unconditionally wrote `status='active'` on conflict. An explicit administrator suspension was therefore silently undone as part of successful SSO authentication.
- Fix: repeat login still updates the mapped role and timestamp, but preserves an existing non-owner membership status. The conflict clause keeps the official project owner active, retaining the owner-role invariant even if a legacy/corrupt row is encountered. New JIT memberships remain active.
- Regression: the composed mock-IdP test now creates an active linked identity, suspends it through the administrator membership operation, then completes a repeat OIDC login with changed claims. It proves the mapped role updates, membership remains `suspended`, `project:read` remains denied, and exactly two OIDC login audits exist.
- Red evidence: before the SQL change, `cargo test --locked access_api::tests::repeat_oidc_login_preserves_admin_suspension_while_reapplying_role_mapping -- --nocapture` failed with `left: "active"`, `right: "suspended"`.
- Green evidence: the same focused test passed after the fix.
- `cargo fmt --all -- --check` — clean.
- `cargo clippy --locked --all-targets -- -D warnings` — clean.
- `cargo test --locked --no-fail-fast` — 29 passed, 0 failed.
- `cargo build --release --locked` — succeeded.
