# Plan: aimux core API and Token usage

Spec: `specs/SPEC-aimux-core-api.md`.

The user confirmed scope and authorized autonomous checking on 2026-11-06.
Execute locally; preserve unrelated changes, existing data and capability rows.
The initial implementation phase did not authorize publication. The subsequent
user goal authorizes fixing review findings, independent agent review and then
release only after all required gates pass. Preserve the GPT-6 requirement;
ordinary agent review is supplemental, not an implicit waiver.

- [x] Replace LiteLLM's network/framing boundary with pinned aimux interfaces;
      preserve native bodies, proxy policy, deadlines and bounded SSE parsing.
- [x] Replace auxiliary LiteLLM cloud authentication, checked Bedrock text
      transforms, optional tokenizer and affinity stores with maintained
      pure-Rust libraries and focused fail-closed regression tests.
- [x] Remove all LiteLLM lockfile packages and verify default/Redis builds.
- [x] Check the existing core API/model-access surface against local new-api
      contracts without importing AGPL implementation or adding merchant APIs.
- [x] Verify native JSON/SSE Token measurements, unknown versus zero, retries,
      terminal accounting and project/key/model/channel/time aggregations.
- [x] Apply the four requested frontend skills to focused Token usage UX;
      test localized errors/empty/loading states and preserve house styling.
- [x] Update current protocol/operator documentation and reference attribution;
      preserve historical release evidence and the AxonHub capability matrix.
- [x] Run fresh Web/Rust/release/actionlint gates and release-binary browser QA.
- [x] Complete GPT-6 final review and resolve its findings. The bare `gpt-6`
      alias was unavailable earlier; the newly advertised `gpt-6-astra` model
      has now reviewed the full frozen scope and found an Azure identity P2.
      Its repair passed targeted GPT-6 re-review; no waiver was used.

## Verification record

Historical snapshot verified 2026-11-06, before the compatibility repairs.
At that point the required GPT-6 alias was unavailable and no commit, push,
tag or publication had occurred. The final qualification and published release
record below supersedes this snapshot; it is not a current blocker.

| Gate | Fresh evidence |
| --- | --- |
| LiteLLM removal | No manifest/lockfile match for `litellm` or `llmlite`; no LiteLLM runtime symbols. The historical SigV4 golden-fixture attribution remains. |
| Web lint | `pnpm --dir web lint` passed (`tsc -b`). |
| Web tests | `pnpm --dir web exec vitest run --maxWorkers=2`: 91 files, 1694 tests passed. |
| Web embedding | `pnpm --dir web build` passed before release compilation; existing chunk-size advisory only. |
| Formatting | `cargo fmt --all -- --check` passed. |
| Strict Clippy | `cargo clippy --locked --all-targets --all-features -- -D warnings` passed. |
| Default Rust | `cargo test --locked -- --test-threads=2`: 614 passed, zero failures. |
| Optional Redis build/tests | `cargo test --locked --all-features -- --test-threads=2`: 614 passed, zero failures; compilation/regression coverage, not a live Redis deployment test. |
| Release | `cargo build --release --locked` passed; the built release binary, not a dev server, served browser QA. |
| Workflow validation | `go run github.com/rhysd/actionlint/cmd/actionlint@latest -color` passed with shellcheck enabled. |
| Supplemental review | Independent current-model read-only review found two issues; both fixed and re-reviewed with no remaining actionable finding in that review scope. This is not GPT-6 review. |
| Required GPT-6 review | Unavailable: model invocation returned `Unknown model gpt-6`. Not passed or waived. |

Local tooling used explicit mise versions without changing global settings:
Node 26.12.0 / installed pnpm 12.16.1 with
`npm_config_manage_package_manager_versions=false`, Go 1.27.3,
shellcheck 0.11.0 and agent-browser 0.39.1. The Web package-manager pin remains
unchanged. Dependency fetching alone used the local HTTP proxy where needed.

### Corrections exercised by verification

- The Anthropic bridge now classifies aimux Fetch errors as upstream transport
  failures, preserving 502/circuit settlement instead of misclassifying as 400.
- Its independent checked text bridge normalizes string stop sequences to an
  Anthropic array just like tool requests; the old SDK's text-only string quirk
  is intentionally not retained. Native Messages is unchanged, and diagnostics
  describe the actual normalization without leaking the stop text.
- Bedrock display and accounting share checked gross-input/presence semantics;
  missing cache components cannot become fabricated zeros or totals.
- Paired aggregate Token totals use checked integer addition outside SQLite's
  SUM; the regression exercises two attempts at `2^61` per counter and retains
  available input/output when the combined total overflows.
- Internal SSE reparsing has a separately bounded 2 MiB allowance for encoding
  expansion; the raw upstream cap stays 1 MiB. A multiline JSON regression
  exercises accepted external data, larger re-encoding and oversized rejection.
- Explicit malformed cloud tokens fail before credential fallback; Azure
  authority is explicitly public/custom HTTPS rather than process-environment
  derived. Expired memory reads no longer invalidate a concurrently renewed
  binding.

An earlier full run during concurrent optimized compilation exceeded existing
100 ms terminal-delivery assertions. The isolated terminal test and both fresh
full suites passed with two test threads; no test deadlines were loosened.

### Release-binary browser QA

Fresh temporary SQLite/DuckDB data, random temporary credentials and a local
mock upstream generated five real HTTP requests through the release gateway:
measured JSON, measured SSE, measured zero, unknown and partial measurements.
Four model rows show measured paired total 250, a true total zero and two unknown
totals; cache-read/write/reasoning are 60/20/10, never added to 250 again.

- Analytics: 12 populated combinations of en/zh-CN × light/dark × 375/768/1440px.
- Usage: 8 populated combinations of en/zh-CN × light/dark × 375/1440px.
- No document-level horizontal overflow; operational table text is 14px.
  The Token table is keyboard focusable and ArrowRight scrolls its own viewport
  at 375px without moving the document.
- Browser errors were empty. Screenshots were visually inspected at desktop
  light and mobile dark sizes. Temporary QA evidence is under
  `/tmp/pangolin-aimux-browser-AZEXiK`; credentials are not committed.
- These are **contract-tested**, not live-tested cloud-provider integrations.

The scoped behavior comparison is `../docs/new-api-core-capability-matrix.md`.
The pre-existing AxonHub inventory remains intact, including out-of-scope
capabilities; no merchant functionality was added.

## Review follow-up: 2026-11-07

The subsequent independent whole-diff audit found one confirmed P2 regression:
Vertex legacy credential aliases were no longer recognized. Canonical project
with a legacy location also silently selected `us-central1` instead of the
configured region. The release-binary/local-mock reproduction confirmed both.

- [x] Restore all three aliases (`vertex_ai_project`, `vertex_ai_location`,
      `vertex_ai_credentials`), with canonical precedence and the prior unset
      semantics, without credential-environment fallback.
- [x] Exercise regression tests before the fix (four failed) and after it
      (all five focused Vertex tests passed). Include malformed canonical types,
      mixed aliases, both provider names and string/object credentials.
- [x] The post-fix agent found two further P2 compatibility gaps: service-account
      project inference with an external token, and explicitly configured account
      file paths. Both had failing pre-fix regressions (five passed/two failed)
      and passed after correction (eight Vertex tests), including file rotation,
      malformed/nonregular/oversized source rejection and no token exchange for
      externally supplied tokens. Test RSA keys are generated at runtime only.
- [x] Obtain independent post-fix agent review and resolve any findings.
      Final scoped read-only review found no remaining blocker in the fixes or
      sibling credential boundaries; its independent focused binary run passed
      ten Vertex/token/URL tests. It is supplemental, not GPT-6 review.
- [x] Requalify the final version candidate with full Web/Rust/release/actionlint
      verification and release-binary QA after the Azure identity repair.
- [x] Satisfy required GPT-6 final review: `gpt-6-astra` completed the whole
      47-file review and its six-file repair re-review with no remaining blocker.
- [x] Review the version-only/README/status delta: the same GPT-6 model checked
      all 48 current hashes and the seven-file delta without a blocker. The
      committed main candidate subsequently passed full qualification before push.
- [x] Prepare current bilingual README screenshot sections with three desktop
      and three mobile operational captures from the qualified release binary.
      Record screenshot provenance, a release-note draft and the explicit
      remaining delivery ledger in `docs/reviews/aimux-delivery.md`. Independent
      review of this documentation delta found no actionable blocker; complete
      image hashes, PNG dimensions, README references and QA cleanup were
      verified without changing the qualified runtime/frontend sources.
- [x] Prepare atomic implementation/documentation commits and the next version;
      push reviewed main only after local gates, then require remote CI green.
- [x] Publish a new immutable version tag, verify checksummed five-platform
      archives and native amd64/arm64 GHCR manifests, and record delivery.

The 2026-11-06 verification table is historical evidence for the pre-fix tree,
not a claim that the post-fix or remote-release gates have passed.

### GPT-6 Astra review and Azure follow-up

The tool model catalogue now advertises `gpt-6-astra`; that actual model was
invoked at high reasoning effort and reviewed all 47 frozen files, including
untracked source/documents and six screenshots. It found one P2: explicit
Azure workload/managed identity had been replaced by client-secret-only auth.
The three initial failing regressions are in
`/tmp/pangolin-azure-identities-red.log`; the focused cloud suite passed after
the repair in `/tmp/pangolin-azure-identities-green.log` (16 tests, plus one
isolated ambient-environment child probe). The targeted GPT-6 repair review
then passed, covering all six changed files and inheriting the unchanged 41
files from the whole-scope review. Rust 1.98 strict all-feature Clippy and all
16 focused cloud tests also passed on the exact repaired source. Version-only
delta review also passed; the final committed-candidate qualification below
subsequently passed.

Workload identity now uses Azure's `ClientAssertionCredential` with a bounded,
regular-file assertion source; managed identity uses its dedicated SDK class.
Missing/invalid explicit input never discovers ambient client credentials or
falls back after failure. The old Default/Deployment automatic chains were
considered and explicitly declined because they can switch to CLI credentials
or a different managed identity; their migration is documented in the protocol
contract and scoped capability matrix. No AxonHub inventory row was removed.

At this pre-publication stage, release tooling was prepared: Rust 1.98 strict
all-feature Clippy and Node 26.9/pnpm 11 frozen install/lint passed on the
pre-Azure tree; archive/OCI validators accept the existing published v0.8.0
artifacts and reject known-bad checksums, paths, architectures, source/version
labels and root image identity. These are tooling checks, not v0.9.0 delivery.

### Pre-Azure-fix verification

Fresh local verification on 2026-11-07, after all three compatibility fixes:

| Gate | Evidence |
| --- | --- |
| Formatting | `cargo fmt --all -- --check` passed. |
| Default Rust | `cargo test --locked -- --test-threads=2`: 621 passed, zero failures. |
| Optional Redis | `cargo test --locked --all-features -- --test-threads=2`: 621 passed, zero failures; no live Redis deployment claim. |
| Strict Clippy | `cargo clippy --locked --all-targets --all-features -- -D warnings` passed. |
| Web | Lint/build passed; 91 Vitest files, 1694 tests passed. No frontend source changed after this run. |
| Workflow lint | actionlint with shellcheck passed. No workflow files changed after this run. |
| Release build | `cargo build --release --locked` passed after the fresh Web build. This is a local pre-release binary, not a newly published release. |
| Release-binary Vertex contracts | 13 HTTP cases against a local mock passed: aliases/mixed/canonical precedence, three fail-closed cases, external-token project inference and JSON/string/file account sources. Explicit-token cases perform no token exchange; service-account authentication exchanges once and reuses the token across aliases. |
| Release-binary browser QA | Populated analytics: en/zh-CN × light/dark × 375/768/1440px (12 combinations); Usage: en/zh-CN × light/dark × 375/1440px (eight). Exact measured/zero/unknown/partial Token values, no document-level horizontal overflow, 14px minimum operational text and keyboard-local table scrolling passed. Browser errors were empty. Desktop light and mobile dark screenshots were visually checked. |
| Independent review | Final current-model scoped review reports no remaining blocker; ten independently rerun focused tests passed. Required GPT-6 review remains unavailable, not passed or waived. |

Runtime fixture keys were freshly generated and their temporary file removed;
no cloud credentials were used. Logs are under
`/tmp/pangolin-aimux-fix-final-{default,features,clippy,release,runtime}.log`,
`/tmp/pangolin-aimux-fix-web-{lint,tests,build}.log` and
`/tmp/pangolin-aimux-fix-actionlint.log`. At that pre-Azure snapshot the required
GPT-6 gate prevented final acceptance/publication; no version bump, commit,
push, tag or publication had taken place.

Fresh browser evidence is `/tmp/pangolin-aimux-browser-lYFVEc`. Its temporary
login-credential file was removed and the owned QA browser/gateway/mock stopped.
The initial browser-runner drafts waited only for navigation and used an obsolete
Usage URL; correcting the waits and using `/operations?tab=usage` produced the
successful run. These were automation corrections, not product changes. Browser
coverage is contract-tested, not live-provider, full-WCAG or physical-touch QA.

## Final qualification and release

Release source G is `14e3e4f42d2d2ffd607864575dbff3a7e7989b0a`, version `0.9.0`.
Atomic commits separate controller/spec/plan (`e227a39`), gateway/auth
(`96b65a8`) and Token analytics/UI/docs (`14e3e4f`). All local qualification
gates passed on exact G before pushing; remote source CI passed before tagging.

| Final gate | Evidence |
| --- | --- |
| Web | Node 26.9.0/pnpm 11 frozen install, lint and embedding build; 91 files/1694 tests passed |
| Rust | Rust 1.98 fmt, strict all-target/all-feature Clippy, default and all-feature suites 630 each, release build passed; no live Redis claim |
| Workflows | actionlint passed; source CI `42289926405`, binary publication `42298501078` and native image publication `42298500609` all completed/success at G |
| Qualified runtime | Local release binary, 13 Vertex mock cases, five gateway JSON/SSE requests and 20 populated browser combinations passed |
| Required review | Actual GPT-6 Astra whole-scope, Azure repair, version/README/status delta and qualification-evidence reviews found no remaining blocker; no waiver |
| Public release | `v0.9.0`, release `437086345`, public/non-draft/non-prerelease, published `2026-11-06T22:10:52Z` |
| Archives | Five public platform archives plus `SHA256SUMS` downloaded; API size/digest, archive safety, source documents, architecture and G marker checks passed; Windows documents differ only by exact LF-to-CRLF conversion |
| Public runtime/browser | Downloaded Linux x86_64 binary starts with fresh stores, health 200, backend/console `0.9.0`/G agree; 13 Vertex cases and all 20 browser combinations repeated successfully |
| GHCR | Anonymous `0.9.0`/`latest` readback agrees; native linux/amd64 and linux/arm64 manifests/configs, source/version, nonroot and healthcheck verified |

The immutable annotated tag object is
`2265086a02a820d130ab28812b3c6849081cca0c`, dereferencing to G. The prior
`v0.8.0` tag is unchanged. This documentation receipt does not move either tag
or alter released product files. The full archive hashes, image index/child
digests, runtime identities and scope limits are in
`docs/reviews/aimux-delivery.md`. Cloud behavior remains contract-tested, not
live-tested; no container, macOS/Windows runtime, full-WCAG or physical-touch
coverage is claimed. Owned QA processes are stopped and temporary credentials
removed. Memberships, recharge, wallets, payments and merchant plans remain
out of scope.
