# models.dev / Pangolin 0.8.0 candidate delivery

## Publication status

The independent library is published. Pangolin is a local **0.8.0 candidate**,
not a published release. The user explicitly approved substituting an available
model for the literal GPT-6 final-review requirement and continuing publication.
The fresh whole-diff independent final review is clean. All other verification
and artifact gates still precede the corresponding delivery actions.

Post-waiver source verification is fresh: Web89 files/1689 tests in215.69s and
Rust603 tests in215.58s, with fmt, strict Clippy and workflow lint exit0
(`ship-*` logs). Atomic implementation commits are catalog31c080e,
protocol49355a2 and token controlsd4e9d6c. Controller documentation is separate.
The clean committed console/binary is rebuilt and checked before tagging;
remote publication and artifact readback are not asserted by this preflight
snapshot.

| Artifact | Observed evidence |
| --- | --- |
| Library | `lib-x/models-dev-rs`, MIT, version0.1.0 |
| Library source | `3a4a7f807ceddf24931f9324f3860d171c325c0e`, annotated `v0.1.0` |
| Registry publication | crates.io `created_at=2026-10-22T18:06:55.424096Z` (October23 in UTC+8); API readback0.1.0 |
| Registry checksum | `beb8e0c313c43990598987bdac8736cd70f40a702688f7a2927f467bc6c3eb0d` |
| Library CI | GitHub runs39828540041 and39828557333 both success at the published source |
| Gateway dependency | Registry-only0.1.0 in manifest/lock; no sibling path dependency |

Name checks preceded implementation and publication: `models-dev` and
`modelsdev` were occupied; `models-dev-rs` returned404 from crates.io API and
sparse index. Availability was rechecked immediately before publication.

## Implemented contracts

- Official native-schema subscription with explicit network opt-in, daily
  durable refresh, validators, disable/enable/retry, last-known-good and rollback.
- Original-byte signature verification before normalization; original digest
  provenance and immutable normalized snapshots; revision-fenced activation.
- Separate4MiB ordinary-network and16MiB official/normalized/export bounds.
  A real9149-model document normalizes to13130933 bytes and merges to13394914.
- Unknown upstream fields/exact price extensions survive. Unsupported SDKs
  remain catalog-only; trusted adapter/endpoints/icons and configured live
  routing/pricing remain unchanged. Unknown facts are not invented zeros.
- Official-source creation reuses existing configuration and atomically avoids
  duplicate canonical sources. Source editing preserves the configured proxy.
- Nonstreaming Anthropic → OpenAI function-tool round trip, with explicit
  rejection of unrepresentable shapes; native forwarding/usage authority and
  streaming commitment remain unchanged.
- Conflicting flat output-token aliases fail closed. Gemini honors its own
  native ceiling/count; estimates/default reservations are not measurements.
- Accessible bilingual catalog/subscription actions, source/error/freshness
  feedback, correct catalog/project permissions and no new decorative motion.
- Disabling excludes the source from the effective catalog, preserving saved
  snapshots and configured project models. Both languages explain this
  distinction, including disabled sources with a prior error. Operational
  capability values use14px type.

## Verification ledger

Logs are under `/tmp/pangolin-modelsdev-qa` unless otherwise specified.

| Gate | Result / evidence |
| --- | --- |
| Library |32tests+2doctests; parser-only17+2; Rust1.94/1.99.1; fmt/clippy/docs/package and dry-run pass |
| Registry upload | `crate-publish.log`, successful upload/index availability and API readback |
| Web install/lint | Pinned pnpm11 frozen-lock install and TypeScript lint exit0; `focus-web-install.log`, `focus-web-lint.log` |
| Web tests |90files/1693tests pass, serial205.77s; `focus-web-tests.log` |
| Web build | Clean focus-qualified0.8.0 console at99ce61ec543c; `focus-web-build-clean.log` |
| Catalog contracts |25focused tests, including6 new native-source cases, pass |
| Protocol contracts |37provider tests, including12 Anthropic cases, pass after observed RED |
| Token contracts |64orchestration+4hard-budget contracts pass, including5 new tests after observed RED |
| Rust fmt/clippy | Focus-followup fmt and strict all-target Clippy exit0; `focus-rust-fmt.log`, `focus-clippy.log` |
| Rust full tests |603passed,0failed,0ignored in219.09s; `focus-rust-tests.log` |
| Release binary | Focus-qualified optimized locked/offline build exit0 in2m17s; `focus-release-build-clean.log` |
| Release-binary browser |12/12 actual painted desktop focus states,375/768px EN/light and ZH/dark, no remaining new blocker; `desktop-qa/focus-final-report.md`, `focus-final-*` |
| Workflow lint | Official actionlintv1.7.12 source built from cache; lint exit0, `focus-actionlint.log` |
| Independent reviews | Earlier scoped reviews and the fresh whole-diff final independent review are clean; P1 and P2 closed, DQA002 documented/nonblocking |
| Model-specific review gate | Earlier literal `gpt-6` invocation unavailable; user explicitly approved available-model substitution and continuing publication |

Commands use the installed Node24.22.1 path and run pinned `corepack pnpm` from
`web/`. A concurrent-load full Web attempt hit an existing probe test's5s
timeout; the unchanged full suite passes serially. No timeout/test source was
weakened. Vite's existing large-chunk warning remains advisory.

One local invocation accidentally forwarded an extra `--`, leaving Vitest at
default concurrency and reproducing load-sensitive pre-existing timeouts. That
run was stopped and is not counted as passing. The frozen suite above uses
`corepack pnpm test --maxWorkers=1`; no assertions or timeouts were weakened.

The prescribed actionlint `go run ...@latest` attempt cannot reach
`proxy.golang.org`. Official GitHub latest-release readback isv1.7.12. Building
the already cached exact module with `go build -mod=readonly` and running that
binary verifies the same workflow files without changing repository tooling.

Initial Rust compilation attempts during concurrent Web edits were discarded
by MBX input-change detection; those are not counted as passing gates. Final
Rust validation uses frozen source and the published registry dependency.

The earlier pre-focus gate chain completed at2026-10-22T19:01:03Z. Its local binary
SHA-256 is `7589d6188c36433c84d46f4dd9cf611d04b35bebbed057549e63676976460d5e`.
The restarted listener and the file hash match. `/api/v1/version` reports
backend/console0.8.0, matching `ee89e4bfade0-dirty`, `profile=release` and
`console_matches=true` (`final2-version.json`). This is a local candidate built
from uncommitted changes, not a published0.8.0 artifact. Last public Pangolin
release readback remainsv0.7.0; no0.8.0 tag or remote write has been made.

## Browser evidence

### Final focus-finding correction

The clean committed-source browser pass at `d51c7f8` overrides earlier
unqualified focus-visibility statements. Computed `2px solid` outlines did not
prove visible paint: the focused viewport and its clipping scroll parent share
the same bounds. Natural Tab and a paced recording reproduce SHIP-DQA-001.
The two-pixel inset-outline prototype remains partially covered by the sticky
header, so the minimal fix instead paints a noninteractive inset frame on the
scroll container, above that header, only when the direct viewport has visible
keyboard focus. It keeps tab order, scrolling, child-control focus and motion
unchanged. Four new CSS/selector tests reproduce RED and33 focused tests pass
GREEN. Final rebuilt-paint verification still precedes publication.

A fresh independent review of this two-file product delta is clean. The direct
viewport selector excludes descendant-control focus; the frame remains outside
scrolling content, above sticky headers and pointer-transparent. Supported
selector engines include Chrome/Edge105+, Safari15.4+ and Firefox121+. That
review does not replace the final real-browser paint check.

The followup is committed separately as822eab9. Fresh full verification on its
product bytes passes Web90 files/1693 tests in205.77s, Rust603 tests in219.09s,
fmt, strict all-target Clippy and workflow lint. The chain completed at
2026-10-22T23:50:28Z (`focus-*` logs). These results supersede the earlier count
of1689 Web tests; the clean committed artifact still receives a final paint
recheck before any push/tag.

That actual-paint recheck now passes on clean99ce61e, SHA-256
`cfc9a6cb0b9215fa00bee66c851fba3013a89e7ca26fbbc24f4f55f802f698c0`.
All12 desktop states visibly show all48 frame edges, above sticky headers and
during internal scrolling;72 bounded hit-target checks and child/next Tab
behavior pass.234 capability samples stay14px; no page overflow, JavaScript
errors or axe violations appear.20 contrast nodes remain incomplete, and the
previously disclosed DQA002 remains nonblocking.375/768px EN/light and ZH/dark
frames also pass, with real keyboard focus and no pointer interception. An
initial main-script ArrowRight assertion ran before native scrolling settled;
waiting for positive scrollLeft resolves the probe without any product edit.

This signoff commit changes controller documentation only, not product/build
paths. The qualified CSS SHA-256 is
`2d9acaefe080d352d67a57d4ce1c47f68d29ae8b296a1c8a40bae02890de6ece`.
The subsequent clean provenance rebuild must retain that digest and matching
backend/console identity. Remote CI/artifact checks still precede a published
release claim.

| Before | After | Why |
| --- | --- | --- |
| The table's outer focus outline is clipped, despite correct computed style | A two-pixel inset container frame paints above sticky headers while its viewport has keyboard focus | Make keyboard position visible without intercepting pointer or scroll input |

### Earlier contract and layout evidence

The isolated local release candidate runs against fake QA identities and a mock
inference upstream; real credentials are not used. A live models.dev metadata
fetch produced9361 effective catalog entries, retained the one configured
`qa-tools` model and its100/200 micro-USD input/output settings, and created no
duplicate official source. Required-signature failure returned400 while retaining
the active snapshot and effective catalog; restoring the original policy and
using UI retry recovered the source. These observations do not imply that
inference providers are live-tested.

The entire desktop matrix was repeated against the final restarted binary at
19:02:53–19:03:49Z: populated models, catalog, channels, request logs and
analytics in English/light and Chinese/dark, plus representative crossed
language/theme combinations. All12 captures have no page overflow, unnamed
visible icon controls, uncaught JavaScript errors or automated axe violations.
Operational capability text computes to14px across all6 applicable samples.
Keyboard focus rings are2px, wide tables scroll internally, and focus is not
trapped. Baseline artifacts remain separate.

Final375/768px subscription captures cover English/light and Chinese/dark,
with reduced motion enabled. Page width equals viewport width, operational
table cells compute to14px, and the new card controls are44px high. Automated
mobile/tablet axe reports have0 violations; clipped/offscreen tab/table columns
have incomplete contrast results, as do desktop request columns. This is not a
full manual WCAG certification or a physical-touch/live-provider claim.

The final binary also passes the assigned-disabled-proxy browser flow: disable
succeeds while preserving the snapshot/proxy and changes the effective catalog
9361→453; enabling with that disabled proxy fails closed and shows a retryable
error. Explicitly restoring the original proxy assignment and clicking Retry
recovers9361 entries. The fake proxy is removed, no configured model prices
change, and the source retains its original daily interval/signature policy.
Required-signature failure and last-known-good UI were repeated on this binary:
HTTP400 preserves the active snapshot and9361 entries; explicit policy restore
and UI retry recover cleanly. Final state is1 enabled healthy source,0 QA
proxies, configured model prices100/200 unchanged, and no page overflow.
Expected fault-injection400s are recorded separately; healthy final reloads
have empty console/uncaught-error files.

Dedicated QA browsers and the isolated gateway/mock processes were closed after
verification. Local evidence and the candidate binary remain outside the
checkout under `/tmp/pangolin-modelsdev-qa` and `/tmp/pangolin-release-build`.
No production deployment was changed.

Browser QA identified and the followup fixes misleading pause wording and
12.5px operational capability text. One unrelated pre-existing cosmetic issue
remains recorded, not silently fixed: DQA-002, the English/light analytics
confidence badge can collapse its visible label at1440px
(`web/src/pages/AnalyticsPage.tsx:117`). Its accessible text is present. It is
outside the changed analytics scope; no publication decision is implied.

## Independent review

Available-model review found one concrete protocol blocker: normal OpenAI tool
replies include `refusal:null` and `annotations:[]`. The old response allowlist
rejected those after inference. The wire fixture reproduced HTTP400 before the
fix. The correction accepts only semantically empty named metadata and retains
rejection of meaningful/malformed values and unknown fields. Both upstream
turns and source-bound SQLite usage assertions remain in the real-router test.

A subsequent integrated review found a P2: an assigned proxy that was later
disabled prevented the subscription itself from being disabled. The API
regression reproduced400 instead of200. Update validation now permits an
existing disabled proxy reference only while saving a disabled source; unknown
references and enabling with a disabled proxy still fail closed. CREATE,
refresh/runtime guards, revision fencing and transactional auditing are
unchanged. The same regression verifies retained settings, both refusal paths,
recovery after proxy reactivation, revisions and audit counts; focused GREEN
is1/1 (`disabled-proxy-red-contract.log`, `disabled-proxy-green.log`). An initial
test-harness visibility error is not counted as RED. The602-test pre-followup
suite passed, but its queued release build was deliberately canceled to apply
this fix; final gates rerun against603 tests.

The earlier scoped reviews are not represented as literal GPT-6 reviews. The
user subsequently gave the explicit waiver requested for available-model final
review and publication. The waiver does not waive independent review itself,
fresh verification, artifact provenance or release-asset checks.

The fresh final reviewer inspected the full27-tracked/12-new-file inventory
against `ee89e4b`, the repository contract and this feature's spec/plan. It found
no remaining blocker in normalization/LKG, trusted adapters, billing isolation,
source concurrency/proxy preservation, tool conversion/usage, token aliases/TPM,
or UI lifecycle. The reviewer found no evidence of copied AGPL code/assets and
did not modify source or run concurrent builds. Exact inherited model identity
is not exposed; no literal GPT-6 claim is made.

## Memory provenance

Targeted Nowledge CLI retrieval: query `Pangolin models catalog pricing`, mode
`fast_bm25_vector`, scope `default`, no semantic filters, limit1. Rank1 memory
`dde8d068-e781-4ff8-a7eb-38e2feac49ce`, “Pangolin：计价来源与独立用量测量的边界”,
score0.9846565432702229, informs preservation of historical cost and independent
quantity evidence. Graph rendering is skipped because this CLI host cannot
verify a matching authenticated browser owner/Space. Durable library-publication
and safe-integration facts are saved as
`6d5730d7-a30a-412d-8e37-ed17e09787a9`; this is not a saved conversation.
