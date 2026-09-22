# AxonHub parity tasks

- [x] Task 1 — Versioned parity schema and migration contracts
- [x] Task 2 — Authorization, projects, users, roles and OIDC
- [x] Task 3 — Request orchestration engine
- [x] Task 4 — Full protocol surface and provider strategies
- [x] Task 5 — Trace, cost, quota and operations
- [x] Task 6 — Complete admin API and console
- [ ] Task 7 — Compatibility ledger, screenshots, documentation and delivery
  - [x] Six area reports re-derived from the working tree
        (`docs/reviews/parity-observability.md`, `-prompts-playground.md`,
        `-models-routing-pricing.md`, `-access-control.md`, `-system-settings.md`,
        `-ux-extras.md`). The original chat reports were never saved.
  - [x] Consolidated ledger, counts derived from those reports
        (`docs/reviews/parity-ledger.md`): 133 rows — 32 `parity`, 77 `gap`,
        13 `feature-build`, 11 `refuted`.
  - [x] Independent current-state audit of every gap row
        (`docs/reviews/parity-status.md`): of 90 gap/feature rows, 20 closed,
        16 partial, 42 open, 9 feature-build still in scope, 3 out of scope.
        It also found seven claims in the ledger that the code did not support;
        four were mine and are now qualified in the ledger, and one was a
        regression this work introduced (saving the orchestration form reset the
        project's routing policy), fixed with red evidence.
  - [ ] Browser QA matrix re-run against the rebuilt binary (in flight). The
        previous run tested a stale embedded bundle, so its findings describe a
        build older than the current tree.
  - [ ] Work the remaining open rows. The backend is close to exhausted for this
        phase: what is left is console wiring, feature builds, or changes that
        need a schema change (`prompts.order`, `http_status` in the record
        system) which this phase excludes.
  - [ ] Documentation updates and final delivery.

## Dispositions recorded by the ledger

- The full price editor is **not** a feature build. `tiers_json` and
  `schedule_json` are modelled, validated and accepted by the write API; what is
  missing is a read projection and the console surface. Recorded as such rather
  than silently treated as out of scope.
- Provider model discovery/sync, trace waterfall/tree views and the playground as
  a full chat surface remain genuine feature builds and are out of scope.
- Eleven claimed differences were refuted with evidence, so they are not "fixed"
  later: prompt activation "version pointers", backup schema-digest coupling, the
  console inventing zeros for unmeasured telemetry, and others.
- Two gaps are recorded as out of reach this phase because they need a schema
  change: `prompts.order`, and persisting the upstream status in the record
  system so a projection rebuild cannot degrade it back to 200/502.
