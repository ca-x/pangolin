# models.dev and gateway usability delivery

Spec: `specs/SPEC-modelsdev-and-gateway-usability.md`.
Extends, rather than replaces, `tasks/axonhub-parity-plan.md` and the capability
inventory. Work begins from clean Pangolin `ee89e4b` / v0.7.0.

## Implementation

- [x] Verify crate name and organization permissions before crate creation.
- [x] Build and verify independent `models-dev-rs` crate (parallel worker).
- [x] Normalize models.dev through the existing secure catalog subscription path.
- [x] Add focused protocol conversion contracts (parallel worker).
- [x] Add focused explicit token-control contracts (parallel worker).
- [x] Add localized, accessible subscription onboarding (parallel worker).
- [x] Record new-api behavior evaluation, boundaries and reference attribution.

## Final delivery gate

- [x] Fresh crate fmt/clippy/tests/docs/package verification and independent review.
- [x] Publish crate/repository and resolve Pangolin's locked registry dependency.
- [x] Web install/lint/test/build; Rust fmt/clippy/test/release; actionlint.
- [x] Release-binary browser QA with populated operational data and mock upstream.
- [x] Clean final independent review with an available model under the user's
      explicit GPT-6 naming waiver; resolve blocking findings and rerun gates.
- [x] Prepare 0.8.0 candidate versions and document verified evidence.
- [x] Create atomic implementation commits after the final-review gate is satisfied.
- [ ] Push/tag only after preceding gates; verify binary checksums and native
      linux/amd64 + linux/arm64 GHCR publication.

Do not mark an unchecked gate complete without fresh observable evidence.

## Observed implementation evidence

- Name availability checked before creation and again before publication.
- `lib-x/models-dev-rs` main `3a4a7f8`, annotated tag `v0.1.0`, crates.io0.1.0
  publication and API readback confirmed. MIT; no parent-directory path dependency
  remains in the Pangolin manifest.
- Crate:32 tests +2 doctests on Rust1.94 and1.99.1; fmt/clippy/docs/package,
  actionlint and parent `cargo publish --dry-run --locked` pass. Independent
  review found no scoped blocker (available model, not GPT-6).
- Catalog:25 focused tests pass, including6 new models.dev tests. Independent
  actual-snapshot normalization:232 providers/9149 models,6058415 original bytes,
  13130933 normalized bytes,13394914 merged bytes, below16MiB bounds.
- Protocol:37 provider tests, including12 Anthropic contracts, pass. Actual RED
  fixtures also exposed standard empty OpenAI metadata, now accepted without
  weakening rejection of meaningful or unknown response fields.
- Tokens:5 new contracts,64 orchestration and4 hard-budget regressions green;
  conflicting-flat-alias RED captured before the production fix.
- Web frozen followup:89 files/1689 tests pass serially in244.26s. The
  disable-semantics and14px followup passes69 focused tests across8 files after7
  observed RED failures;0.8.0 assets rebuilt at2026-10-22T18:44:57Z.
- Final proxy-followup source: Rust fmt, strict all-target Clippy,603/603 tests
  (234.48s), optimized release build (2m31s), workflow lint and diff whitespace
  checks pass. Gate chain completed2026-10-22T19:01:03Z.
- Available-model integrated review found and closed the disabled-proxy P2:
  API RED400→GREEN200, retained configuration, unknown/re-enable refusal,
  recovery, revision fencing and auditing are covered. It is not GPT-6 review.
- Final binary SHA256 `7589d6188c36433c84d46f4dd9cf611d04b35bebbed057549e63676976460d5e`;
  backend/console0.8.0 match, profile release. All12 desktop captures and375/768px
  subscription flows pass with populated data,14px capabilities,44px new card
  controls and no page overflow/JavaScript exceptions. Automated axe has zero
  violations with documented incomplete offscreen contrast checks.
- Real metadata9361→disabled453→recovered9361; saved snapshots and configured
  model prices remain unchanged. Final-binary proxy rejection/retry and required
  signature last-known-good/retry are verified. Physical touch/provider-live
  inference is not claimed. Original unrelated analytics badge clipping is
  documented, not silently fixed.
- The legacy gateway output-ceiling fixture now rejects conflicting aliases and
  accepts equal aliases; its focused test passes after the full-suite failure.
- Initial concurrent Web edits invalidated some Rust MBX compilation inputs;
  those failed build attempts are not counted as verification passes.
- The earlier literal `gpt-6` invocation returned `Unknown model gpt-6`.
  The user explicitly approved the requested available-model substitution and
  continuing publication. This exception applies to this delivery only; all
  other verification, review and publication gates remain mandatory.
- A fresh whole-diff independent review under that waiver inspected all27
  tracked changes and12 new files. No remaining blocker or evidence of copied
  AGPL code/assets was found. Exact inherited model identity is not exposed and
  is not represented as a literal GPT-6 review. Known DQA002 remains documented.
- Fresh post-waiver verification: Web89 files/1689 tests in215.69s; Rust603
  tests in215.58s; fmt, strict Clippy, locked registry resolution and workflow
  lint pass (`ship-*` logs). No source changes follow these checks.
- Atomic implementation commits: catalog31c080e, protocol49355a2, token
  controlsd4e9d6c. Controller specs/plans/evaluation are committed separately.
