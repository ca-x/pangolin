# Task 1 report: versioned parity schema and initialization contracts

## Status

Implemented the fresh-install AxonHub parity schema at migration version 2, its Rust query models, deterministic seed data, and focused schema contracts.

## Scope ruling

During implementation, the user clarified that Pangolin has not been delivered and experimental v0.1 database rows do not need compatibility or preservation. That ruling overrides the original task brief's v0.1-upgrade requirement.

Accordingly:

- there is no data-preserving v0.1 upgrade path or upgrade-preservation test;
- a detected experimental migration version 1 is reset before the fresh version 2 schema is initialized;
- the binding migration tests cover fresh initialization and idempotent re-initialization only.

## Implementation

- Split schema initialization from runtime queries into `src/db/schema.rs`.
- Added normalized relational tables and indexes for:
  - projects, memberships, invitations, roles, permissions, and role bindings;
  - OIDC providers and linked identities;
  - API-key profiles, model mappings, allowed models, ownership, type, expiry, and IP policies;
  - channel credentials and versioned channel settings;
  - model associations, immutable price-version rows, and price components;
  - prompts and prompt-protection rules;
  - threads, traces, requests, executions, usage logs, and cost items;
  - provider quota snapshots, probes, and health/backoff state;
  - webhooks and delivery attempts;
  - storage configurations, retention policies, backup configurations, and backup runs.
- Added fixed identifiers for the default project and system roles, permission seeds, and deterministic first-admin ownership/membership binding.
- Made project ownership mandatory for newly created providers/channels and API keys.
- Normalized provider secrets into `channel_credentials`; gateway routing selects the highest-priority enabled credential.
- Added Rust `FromQueryResult` models covering the new management, routing, lifecycle, quota, webhook, storage, and backup entities.

## Focused contracts

- Fresh schema reaches version 2 and contains the required entity tables.
- SQLite reports no foreign-key violations.
- Representative foreign-key counts and indexes are asserted across access, routing, lifecycle, quota, webhook, and backup domains.
- System roles and exactly one fixed default project are seeded.
- Re-running initialization does not add tables, indexes, migrations, projects, or roles.
- Initial admin creation assigns the fixed project owner role deterministically.
- Provider creation atomically creates its normalized credential and channel-settings rows.

## Verification

Executed in `/home/czyt/code/rust/pangolin/.worktrees/axonhub-parity` after rebuilding embedded web assets:

```text
pnpm --dir web build
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked db::
```

Results:

- web build: passed;
- rustfmt check: passed;
- clippy all targets with warnings denied: passed;
- focused DB tests: 3 passed, 0 failed.

## Self-review

- Confirmed no credential secret is duplicated on the channel/provider row.
- Confirmed all Task 1 writes remain inside the isolated `feat/axonhub-parity` worktree.
- Confirmed unrelated pre-existing capability/spec/task changes are not included in the Task 1 commit.

## Concerns

None within the revised fresh-install scope. Later tasks still need CRUD/authorization/orchestration behavior over these schema contracts.

## Fix round 1/5: authorization and scope invariants

### Reviewer findings addressed

- Changed API-key owner deletion from `SET NULL` to cascading revocation. User and personal keys now require an owner, and attempts to clear that owner are rejected.
- Replaced the profile `SET NULL` relationship with a restrictive composite foreign key on `(profile_id, project_id)`. A referenced profile cannot be deleted, and a key cannot reference a profile from another project.
- Added role scope shape checks plus symmetric insert/update triggers for memberships, invitations, and user-role bindings. Added a role-update trigger so moving the role cannot invalidate existing assignments.
- Added NULL-aware partial unique indexes for global model price versions, global role bindings, and global retention policies.
- Added focused enforcement tests for every reported invalid insert, update, and delete path, plus the equivalent invitation and role-update paths.

### Red/green evidence

The new tests reproduced the original defects before the DDL changes:

- owner deletion left `personal-key` present (`left: 1`, `right: 0`);
- a cross-project membership insert was accepted;
- a duplicate global model-price version was accepted.

After the schema fixes, each focused test passed individually.

### Final verification

Executed:

```text
cargo fmt --all
pnpm --dir web build
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked db::
```

Results:

- web build: passed;
- rustfmt check: passed;
- clippy all targets with warnings denied: passed;
- focused DB tests: 8 passed, 0 failed.

### Fix-round self-review

- Owner deletion either fails because another restrictive ownership relationship applies or revokes dependent API keys; it never leaves an enabled key with a cleared owner.
- Profile deletion is restricted while referenced and cannot erase routing/quota restrictions.
- Membership, invitation, binding, API-key, role, and profile changes preserve project scope on both inserts and updates.
- Partial indexes cover exactly the NULL scopes that ordinary SQLite composite uniqueness does not.
- No unrelated pre-existing worktree changes are included in this fix commit.
