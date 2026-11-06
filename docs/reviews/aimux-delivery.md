# aimux delivery preflight

Status on 2026-11-07: the advertised `gpt-6-astra` model completed the required
whole-scope review and the targeted re-review of its Azure identity P2 repair,
with no remaining blocker. The same model also approved the version-only/
README/status delta across all 48 current files. Complete committed-candidate
qualification remains pending. **No publication may proceed before that gate
and remote main CI pass.**

The earlier bare `gpt-6` invocation was unavailable. That is historical, not
the current blocker: a real advertised GPT-6 model is now being used, without
waiving the review requirement.

## Candidate boundary

- Published baseline: `v0.8.0`; its tag must never be retargeted.
- Worktree baseline: `6ac95bce771216c59c1fd5ad01ece24fda90e834` plus the aimux
  migration and review fixes recorded in `tasks/aimux-core-api-plan.md`.
- Candidate version: `0.9.0`, a minor feature release with the explicit Azure
  migration below. Cargo manifest/lock package and Web now agree on `0.9.0`;
  atomic check-in, final qualification and public delivery remain pending.
- Initial qualified screenshot-capture binary SHA-256, before the Azure repair:
  `b47a50a01a47a08180c7f7f4cc7e70fcec298a2cdd74091343e4a4279b975b22`.
- Runtime source now includes the Azure identity repair. Frontend source is
  unchanged, so the existing captures still represent the operational UI;
  the final versioned binary must be rebuilt and requalified before delivery.

The notes below describe the pending worktree, not the published `v0.8.0`
artifact. Before publication, bind them to the final reviewed main commit and
recheck the remote release/tag baseline.

## Delivery ledger

| Requirement | Current evidence | State |
| --- | --- | --- |
| User scope | aimux replaces LiteLLM; core API and Token usage only, no memberships/recharge/wallet/payments | Done |
| GCP compatibility fixes | Vertex aliases, external-token project inference and explicit account files; red/green regressions and scoped independent review | Done |
| Azure identity repair | Explicit workload/managed SDK flows restored; 16 focused cloud tests and Rust 1.98 strict Clippy pass, including bounded files, single-source failure and an isolated ambient-environment probe; targeted GPT-6 review is clean | Done |
| Existing capability inventory | `docs/axonhub-capability-matrix.md` is unchanged; the scoped new-api matrix does not replace it | Done |
| Prior local checks | Before the Azure repair: Default/Redis Rust 621 each, Web 91 files/1694 tests, fmt, strict Clippy, lint/build, release and actionlint passed; final candidate must be requalified | Historical |
| Prior runtime contracts | 13 release-binary Vertex mock cases; repeat for the final build, with no live-cloud claim | Historical |
| Prior browser QA | Analytics: 12 language/theme/size combinations; Usage: eight, plus keyboard checks; repeat on the final candidate | Historical |
| Current README screenshots | Three desktop and three mobile captures from the qualified release binary, covering Analytics, Usage and Requests | Done |
| Supplemental delivery review | Initial README/screenshot review was clean; GPT-6 repair review also checked the revised Azure migration copy | Done |
| Release copy | Four-outcome draft below, including the Azure migration requirement; version and artifact identity are not final | Draft |
| Required GPT-6 final review | Actual `gpt-6-astra` whole-scope, Azure repair and final seven-file version/README/status delta reviews are clean; all 48 current file hashes checked | Done |
| Final candidate qualification | Full local Web/Rust/release/actionlint and release-binary QA must run on the committed main candidate before pushing | Remaining |
| Atomic commits and version sync | Version fields agree on 0.9.0; keep controller specs/plans separate from implementation at check-in | Remaining |
| Main and remote CI | Push only the reviewed main result; require CI success for that exact SHA before tagging | Remaining |
| Stable tag and GitHub archives | New immutable tag at reviewed main, five native platform archives and `SHA256SUMS`; download and verify contents/hashes | Remaining |
| Native GHCR images | Linux amd64/arm64 native builds, image/index/source identity and public readback; preserve old version tags and advance `latest` only with the verified release | Remaining |
| Final delivery receipt | Record exact commit, tag, workflow conclusions, archive hashes and multiarch digest after external verification | Remaining |

The initial deterministic release-gate snapshot reported a dirty worktree,
local-only remote sync and `v0.8.0` fields. Those checks must be repeated on the
committed 0.9.0 candidate; matching version fields alone never prove readiness.
No remote delivery gate is counted as done.

## Screenshot provenance

Captured 2026-11-07 from the binary identified above, using a fresh temporary
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

## Release notes draft

Title convention: version only, following the previous published release.
Use `v0.9.0` only if it remains the selected new version after the final gates.
The previous release uses English-only, numbered, single-sentence outcomes;
this draft follows that convention rather than adding a second language block.

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

Working-copy omissions: cache/tokenizer/auth dependency internals and local
verification mechanics are not separate user-facing release items. Their
visible outcomes are covered by gateway compatibility and Token reporting.
Add the actual comparison link only after the new immutable tag is published.
