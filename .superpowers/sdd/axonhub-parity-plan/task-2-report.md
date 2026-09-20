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
