# aimux / Pangolin 0.9.0 delivery

Pangolin **v0.9.0** is published at
https://github.com/ca-x/pangolin/releases/tag/v0.9.0, with publication timestamp
`2026-11-06T22:10:52Z` (the operator's 2026-11-07 session). All source workflows,
five downloaded platform archives and anonymous multiarchitecture image
readbacks pass. The downloaded Linux x86_64 binary also passes isolated runtime,
mock-provider and populated-browser checks.

The advertised `gpt-6-astra` model completed the whole 47-file review, identified
the Azure identity P2, and approved its six-file repair. It then approved the
seven-file version/README/status delta, checking all 48 current file hashes.
Its later qualification-evidence review found no blocker. The earlier bare
`gpt-6` alias failure is historical; no review waiver was used.

## Release boundary

- Release source: `14e3e4f42d2d2ffd607864575dbff3a7e7989b0a` (G), version `0.9.0`
  in Cargo manifest, Cargo lock package and Web manifest.
- Atomic commits: `e227a39` controller/spec/plan, `96b65a8` gateway/auth and
  `14e3e4f` Token analytics/UI/docs. Local qualification and source CI preceded
  the new version tag.
- Annotated `v0.9.0` tag object:
  `2265086a02a820d130ab28812b3c6849081cca0c`, dereferencing to G.
- Prior `v0.8.0` tag object remains
  `2fae87c77227e18465ab7d8324f0c24cb3054b69`; neither tag is retargeted.
- GitHub release `437086345` is public, non-draft and non-prerelease, with exactly
  five platform archives plus `SHA256SUMS`. Its version-only title is `v0.9.0`.

This receipt is a documentation-only follow-up to G, not a new product build
or a reason to move the released tag. Historical pre-fix qualification in the
implementation plan is superseded by the final evidence below.

## Delivery ledger

| Requirement | Current evidence | State |
| --- | --- | --- |
| User scope | aimux replaces LiteLLM; core API and Token usage only, no memberships/recharge/wallet/payments | Done |
| GCP compatibility fixes | Vertex aliases, external-token project inference and explicit account files; red/green regressions and scoped independent review | Done |
| Azure identity repair | Explicit workload/managed SDK flows restored; 16 focused cloud tests and Rust 1.98 strict Clippy pass, including bounded files, single-source failure and an isolated ambient-environment probe; targeted GPT-6 review is clean | Done |
| Existing capability inventory | `docs/axonhub-capability-matrix.md` is unchanged; the scoped new-api matrix does not replace it | Done |
| Final local checks | Exact G: Default/all-features Rust 630 each, Web 91 files/1694 tests, frozen install, fmt, strict all-feature Clippy, lint/build, release and actionlint passed | Done |
| Final runtime contracts | Both qualified and downloaded release binaries pass 13 Vertex HTTP mock cases; no live-cloud claim | Done |
| Final browser QA | Both binaries pass Analytics 12 plus Usage eight language/theme/size combinations, 14px, keyboard scrolling and empty browser errors | Done |
| Current README screenshots | Three desktop and three mobile captures from the qualified release binary, covering Analytics, Usage and Requests | Done |
| Supplemental delivery review | Initial README/screenshot review was clean; GPT-6 repair review also checked the revised Azure migration copy | Done |
| Release copy | Four outcomes below, Azure migration, compatibility limits and the existing badge issue; public body readback matches, with one comparison link | Done |
| Required GPT-6 final review | Actual `gpt-6-astra` whole-scope, Azure repair and final seven-file version/README/status delta reviews are clean; all 48 current file hashes checked | Done |
| Final candidate qualification | Full local gates and release-binary QA passed on committed G before pushing | Done |
| Atomic commits and version sync | Three separate commits above; all version fields are 0.9.0 | Done |
| Main and remote CI | Source CI 42289926405 completed successfully at exact G before tagging | Done |
| Stable tag and GitHub archives | Immutable tag, five native archives, checksums and source/format/content validation below | Done |
| Native GHCR images | Docker 42298500609 and anonymous index/config/source readback passed for native amd64/arm64 | Done |
| External delivery receipt | Commit, tag, workflow conclusions, asset hashes and multiarch digest recorded below | Recorded |

## Final qualification

Local gates use Node 26.9.0/pnpm 11.0.0 and Rust 1.98.0 without changing global
settings. Frozen Web install, lint, 91 Vitest files/1694 tests and embedding build
pass. Rust fmt, strict all-target/all-feature Clippy, default and all-feature
suites (630 tests each), optimized locked release build and actionlint pass.
All-feature coverage is local, not an extra remote-CI job or a live Redis test.
The isolated ambient-environment child probe is not counted as another test.

The qualification ledger binds these logs and runtime checks to exact G:
`/tmp/pangolin-v090-qualified-gates.tsv` and `/tmp/pangolin-v090-*.log`.
The qualified local binary SHA-256 is
`f365a5be6c69dacfc278c80ba5736865753c64eb3aa12048096d5ee73253fd53`.

| Workflow at G | Result |
| --- | --- |
| [Source CI 42289926405](https://github.com/ca-x/pangolin/actions/runs/42289926405) | Completed/success: actionlint, Web and Rust jobs; remote Rust 630 tests |
| [Release binaries 42298501078](https://github.com/ca-x/pangolin/actions/runs/42298501078) | Completed/success: preflight, Web assets, all five native platform builds and publication |
| [Docker 42298500609](https://github.com/ca-x/pangolin/actions/runs/42298500609) | Completed/success: preflight, native amd64/arm64 builds and index merge |

## Downloaded artifacts

All six public assets were downloaded and matched their GitHub API sizes and
digests. `SHA256SUMS` contains exactly the five expected archives and has SHA-256
`b05fafdf12cd4e5c984030871e6525efe4c59f1bd49197ac4776cfe03fc361a4`.

| Archive | Bytes | SHA-256 |
| --- | --- | --- |
| `pangolin-x86_64-unknown-linux-gnu.tar.gz` | 30545723 | `c43865ba8b7fbfe20836a031b0442196d81a78a709ccbaee6c8e32992294aaba` |
| `pangolin-aarch64-unknown-linux-gnu.tar.gz` | 28250931 | `a24f61f26029918ab37e97e395efb5e6fc00d9c9ee0bb410002f675a6287a264` |
| `pangolin-x86_64-apple-darwin.tar.gz` | 28143451 | `daf24363315dbc0d4c12816b6d6ada1b848f4a4aa752bc88dc049de9d0f539e5` |
| `pangolin-aarch64-apple-darwin.tar.gz` | 25658636 | `a0e666e47a40ddc7d71b18363e7646bf0e5cd39fbbc293a33bd0fbea91785992` |
| `pangolin-x86_64-pc-windows-msvc.zip` | 25280241 | `9b163f7b323797353a1d35d02f357e290de60265e9d367ff10eafa090eb06edf` |

Archive validation checks bounded sizes, exact expected paths, no links or
special entries, executable ELF/Mach-O/PE architectures and G's 12-hex marker.
Bundled README, LICENSE and `.env.example` match G byte-for-byte on Unix.
Windows checkout converted all three documents from LF to CRLF; their content
is otherwise identical. The verifier permits only source-exact bytes or that
exact Windows-only conversion. Positive LF/CRLF and negative content-change,
mixed-newline and non-Windows-CRLF cases pass. No archive or checksum was edited.

The downloaded Linux x86_64 executable SHA-256 is
`4ca3a372dc92aab23c34e7e6daecec10ecc5562a751369e19c01c27ed47dfa17`.
Its archive also matches the already exercised workflow artifact byte-for-byte.
The public-download executable was independently started against fresh temporary
SQLite/DuckDB stores on loopback. Liveness/readiness return 200; backend and
console report `0.9.0`/`14e3e4f42d2d`, release profile and `console_matches=true`.
Served CSS matches the qualified digest
`2d9acaefe080d352d67a57d4ce1c47f68d29ae8b296a1c8a40bae02890de6ece`.

Thirteen Vertex mock cases pass: ten successful requests and three fail-closed
rejections without upstream I/O. External-token requests perform no exchange;
service-account auth exchanges once and reuses that token across aliases.
Five separate JSON/SSE gateway calls seed the populated browser matrix:
Analytics en/zh-CN x light/dark x 375/768/1440px (12) and Usage en/zh-CN x
light/dark x 375/1440px (eight). Measured total 250, true zero, unknown/partial
values, subset cache/reasoning counts, 14px operational text, no document
overflow, keyboard-local table scrolling and zero browser errors pass.
Desktop light and mobile dark screenshots were visually checked. Evidence is
under `/tmp/pangolin-aimux-browser-KYdzrd`; temporary credentials are removed
and the owned browser, gateway and mock processes are stopped.

## Published images

Anonymous GHCR reads verify matching `0.9.0` and `latest` index digest:
`sha256:8469a9336387837a369d24594f8ee5835c352d7f86b2c1829272d8c0a579487e`.

| Platform | Manifest digest | Config digest |
| --- | --- | --- |
| `linux/amd64` | `sha256:b1e04956ab7a02e7eacd5da248c6402947edc6cd25683fe633c116224518a8fa` | `sha256:a625ef49f45372a70ec2d50fbafaefeeb51fe7faabd514288e04d9ef65c36075` |
| `linux/arm64` | `sha256:0b6b3d86647e444119d05d074e90b7021d6e71f5fc9c40964199d5bd77d047c2` | `sha256:a98b2802e74485cc9a9e875e49a04f0397a7605b941a01d63d1bd21b5ae73494` |

Child manifests and configs match descriptor digests/sizes and full G/version
labels. Configs use `10001:10001`, the Pangolin entrypoint, `/data` volume and
loopback healthcheck; attestation descriptors reference the runtime manifests.
Jobs `128169949639` and `128169949519` and their logs confirm native Ubuntu
x86_64 and aarch64 runners, respectively. No Docker runtime is installed here;
container execution or macOS/Windows runtime testing is not claimed.

Machine-readable evidence is in `/tmp/pangolin-v090-archive-receipt.json`,
`/tmp/pangolin-v090-ghcr-receipt.json`, workflow state JSONs and the
`/tmp/pangolin-v090-public-*.log` runtime/browser logs. Release/tag/API and
registry readbacks provide the durable external artifact identities.

## Screenshot provenance

Captured 2026-11-07 from the pre-Azure-fix screenshot binary (SHA-256
`b47a50a01a47a08180c7f7f4cc7e70fcec298a2cdd74091343e4a4279b975b22`),
using a fresh temporary
SQLite/DuckDB instance and local mock upstream. Five real gateway requests
exercise measured JSON/SSE, measured zero, unknown and partial usage. Analytics
shows a paired total of 250 with cache-read/write/reasoning 60/20/10 counted as
subsets, a true zero and two unmeasured totals. No real provider credentials,
customer prompts or new-api image assets are present.

The desktop captures are 1440×960, English/light; mobile captures are 375×960,
Chinese/dark. Tables scroll within their own viewport rather than widening the
document, and operational text is at least 14px. All six captures have useful
populated data. Browser errors are empty. This is not a full-WCAG or physical
touch-device claim. Evidence is under `/tmp/pangolin-aimux-browser-iMcOqy`;
the temporary login-credential file is removed and owned QA processes stopped.

| Screenshot under `docs/screenshots/` | SHA-256 |
| --- | --- |
| `aimux-analytics-desktop-light-en.png` | `96dbc1d27c82bc52a6728937d956e753b91d0ca191682296d52a7b6bcaad7ed9` |
| `aimux-analytics-mobile-dark-zh.png` | `075c2d871ccbf1573bef6f476d3a670cac0389f25cef6599cdad9a45f3cf8395` |
| `aimux-requests-desktop-light-en.png` | `81adaa9f22962863ffb69e49f294db66b90fdb73f3c81db0cb7a077962997a80` |
| `aimux-requests-mobile-dark-zh.png` | `89182d8fcff2a64918f9333325c1e33d5b02c52c0feed23d334933ac604603a9` |
| `aimux-usage-desktop-light-en.png` | `233fb62575d18a460293669e1cd96f046cb95cf9831f3c73c4d326b41f410029` |
| `aimux-usage-mobile-dark-zh.png` | `b1476002b3a8e79c89737b9c3679c44df612c162a15e389947e3bf4e4566075a` |

## Published release notes

Title convention: version only, following the previous published release.
The previous release uses English-only, numbered, single-sentence outcomes;
`v0.9.0` follows that convention rather than adding a second language block.

1. **API gateway:** Switch the gateway transport from LiteLLM to aimux while retaining native provider fields, endpoint policies and authoritative usage accounting.
2. **Token usage:** Show input/output, paired totals and cache-read/write/reasoning Tokens with measured-attempt coverage, keeping unmeasured values distinct from real zeroes.
3. **Azure identity:** Use explicitly selected workload or managed identity without a client secret, with authentication failures stopping rather than switching to another identity.
4. **Vertex credentials:** Preserve legacy aliases, service-account project inference for external tokens and explicitly configured account files, including credential-file rotation.

**Azure migration:** Entra `DefaultAzureCredential` and
`DeploymentIdentityCredential` chains are now rejected, even when the Entra
profile also contains a direct token or client secret. Remove the chain
selector for direct-token/client-secret profiles, or explicitly name one
workload/managed identity. An explicit workload/managed selector is now
authoritative over an additional client secret; no failure-driven identity
fallback or refresh-only credential discovery is enabled. API-key
authentication remains a separate unchanged path.

**Compatibility:** Bedrock cross-protocol conversion supports nonstreaming,
text-only Converse; unsupported shapes are rejected rather than dropped. Cloud
integrations are contract-tested, not live-tested. This is not full new-api or
aimux-provider parity, and it adds no membership or recharge functionality.

**Known issue:** The existing English/light Analytics confidence badge can
clip at desktop widths.

Release-copy omissions: cache/tokenizer/auth dependency internals and local
verification mechanics are not separate user-facing release items. Their
visible outcomes are covered by gateway compatibility and Token reporting.
The public body contains one comparison link:
https://github.com/ca-x/pangolin/compare/v0.8.0...v0.9.0.
