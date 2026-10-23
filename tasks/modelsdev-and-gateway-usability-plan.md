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
- [x] Release-binary browser QA with populated operational data and mock upstream;
      final table-focus visibility finding closed by actual paint verification.
- [x] Clean final independent review with an available model under the user's
      explicit GPT-6 naming waiver; resolve blocking findings and rerun gates.
- [x] Prepare 0.8.0 candidate versions and document verified evidence.
- [x] Create atomic implementation commits after the final-review gate is satisfied.
- [x] Push/tag only after preceding gates; verify binary checksums and native
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
- Clean committed-artifact QA found SHIP-DQA-001: the table viewport's computed
  2px outer outline was clipped by its same-sized scroll container. Earlier
  computed-style focus checks were insufficient and do not close this finding.
  Natural Tab and screenshots reproduce it. A negative-inset outline prototype
  still loses its top edge beneath sticky headers. The scoped fix paints an
  inset, noninteractive focus frame on the container above those headers, only
  while its own viewport has keyboard focus; no new tab stop or motion is added.
  Four regression tests fail before the fix;33 focused tests then pass. Final
  independent delta review is clean, including pointer/scroll/child-focus and
  mature-browser selector support. Fresh full gates pass: Web90 files/1693 tests
  (205.77s), Rust603 tests (219.09s), fmt, strict Clippy and workflow lint.
  Focus fix822eab9 is a separate implementation commit. The clean99ce61e
  release-paint recheck closes SHIP-DQA-001:12 desktop states show all48 frame
  edges, retained during scrolling;72 hit targets and child/next Tab behavior
  pass.375/768px EN/light and ZH/dark frames are also visually verified. All
  source/dist/business data remained unchanged during QA. See the final report
  under `/tmp/pangolin-modelsdev-qa/desktop-qa/focus-final-report.md`.
- This prepublication signoff changes controller documentation only. Product
  paths (`src`, `web`, Cargo manifests, build scripts and workflows) are unchanged
  from the qualified99ce61e artifact. Rebuilding only provenance metadata must
  preserve the qualified CSS digest before push/tag.

## Published delivery receipt

- Stable `v0.8.0` was published at2026-10-23T00:48:03Z:
  https://github.com/ca-x/pangolin/releases/tag/v0.8.0.
- Release source is `dbb1ac15ca6b02383550706a5e71788fe2c98e14`; annotated tag
  object `2fae87c77227e18465ab7d8324f0c24cb3054b69` dereferences to that source.
  Subsequent documentation receipts do not move the release tag.
- Source CI39876886640, Release39876936998 and Docker39876936926 all complete
  successfully. CI verifies the pinned Rust1.98.0 and Node26.9.0 environment,
  including1693 Web tests,603 Rust tests and the optimized release build.
- All five Linux/macOS/Windows archives and `SHA256SUMS` were downloaded. Every
  published digest, expected path, license, executable architecture and embedded
  source revision passes verification. Release remains stable/non-draft with
  the same six asset identities after curated release-note readback.
- The downloaded Linux x86_64 executable starts on a fresh isolated database;
  liveness/readiness return200, backend/console report0.8.0 at `dbb1ac15ca6b`,
  and the compiled console CSS retains the qualified digest above. Browser
  setup renders without JavaScript errors, console messages or page overflow.
- Native amd64/arm64 jobs and GHCR manifests/configurations are verified.
  `ghcr.io/ca-x/pangolin:0.8.0` and `:latest` share index digest
  `sha256:c0de57d238971d16140b5e0f1aa37d565c3401c2632e17d189f0d851e2dcd680`;
  both image configurations identify the release source. Container runtime,
  macOS/Windows runtime and live-provider inference are not claimed.
- Evidence: `docs/reviews/modelsdev-delivery.md`, local
  `/tmp/pangolin-modelsdev-qa/archives-qa/v0.8.0-1j6f5b/receipt.json`,
  `official-runtime-smoke.json`, `released-ghcr-0.8.0.json` and
  `source-final-run-*.json`. Dedicated QA browsers, gateway and mock processes
  are stopped; no production deployment was changed. DQA002 remains disclosed.
