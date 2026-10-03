# Reference adoption verification

This delivery implements seven selected Magpie/AstrLink-inspired capabilities in Pangolin / 鲮鲤:

| Capability | Delivered contract |
| --- | --- |
| Protocol probes and timing | Project/channel/model/credential/protocol/stream selection; separate headers/event/visible-text timing and final-output throughput. |
| Credential inventory | Per-credential known/stale/unknown model visibility; fenced activation and last-known-good fallback. |
| Public model facts | Conservative nullable capabilities, modalities, reasoning levels and shared limits, with request eligibility checks. |
| Client setup | Authorized key-specific model choices and safe Codex/Claude Code/Gemini CLI/curl/Python/Node examples, including proxy subpaths. |
| Privacy inspection | Structured tool-content traversal, exact allowlists, opt-in templates and transient request preview. |
| Execution explanations | Bounded safe conversion and affinity diagnostics on each attempt. |
| Pricing completeness | Independent price status and quantity measurement evidence; immutable microUSD history and honest unknown values. |

## Verification scope

All local upstream/IdP/storage contracts are contract-tested; no production provider credentials were used. This is the supported single-node system. SQLite/SeaORM remains authoritative and DuckDB remains a rebuildable derived projection, following ddia-principles. No request-preview sample is retained in query caches, audit or request logs. OCR, semantic classification, reversible restoration and distributed guarantees are outside this delivery.

## Fresh gates

Exact complete-source revision: `93b69f349ba06ff380cfc008f3840785395944a7`. All nine required commands exited0, sequentially with no concurrent heavy compiler/full-JS jobs. Frontend:1,660 tests in87 files. Rust:579 tests,0failed/ignored,223.80s test time. Earlier diagnostic failures remain in the local evidence; assertions/timeouts were not widened.

| Command | Exit | Wall time |
| --- | --- | --- |
| `pnpm --dir web install --frozen-lockfile` | 0 | 0.76s |
| `pnpm --dir web lint` | 0 | 1.46s |
| `pnpm --dir web test` | 0 | 48.75s |
| `pnpm --dir web build` | 0 | 2.43s |
| `cargo fmt --all -- --check` | 0 | 0.97s |
| `cargo clippy --locked --all-targets -- -D warnings` | 0 | 16.35s |
| `cargo test --locked` | 0 | 265.19s |
| `cargo build --release --locked` | 0 | 121.93s |
| `go run github.com/rhysd/actionlint/cmd/actionlint@latest -color` | 0 | 1.36s |

Raw logs/results are retained in the local evidence archive under `final-verification-font/`. The later evidence/progress commit changes documentation only; production tree identities are checked before final release embedding. Backend/embedded frontend provenance must match the final documentation commit, verified through `/api/v1/version`.

## Release browser QA

The broad repaired release at `be9e49d8bcd0` passed100 paired browser captures, with39 populated baseline screens,32 money variants,12 privacy decision/theme/language cases and12 real diagnostic detail captures. It covers375/768/1440px, both main languages/themes and complementary combinations. Exact native clipboard paste, one-time key disposal, delayed-preview close/project roundtrip, template error/empty/retry, typed tool JSON, focus return and long-result keyboard scrolling passed. No preview sample entered captured bodies or audit. Contrast is17.49:1 in light and15.18:1 in dark; document overflow is0. Axe incomplete checks and one settled toast-transition recheck remain documented rather than represented as blanket WCAG certification.

Actual mock runtime proofs pass missing-native-usage conservative holds, constant25/free0 money with unreported quantities, source-bound foreign-text rejection and configuredcl100k known-context enforcement before I/O. Mock contracts remain contract-tested.

One original font acceptance remained in Models catalog limits/rates and was corrected by exactly four font props; known/zero/unknown values and captions are preserved. Final matching release `93b69f349ba0` passed eight model table/mobile combinations across four language/theme pairs at375/1440px. All primary values compute14px, including known limits/rates, actual API-created zero tariffs and unknown dashes.16 instrumented captures have0 axe violations,0 overflow and empty browser errors/console; incomplete checks remain preserved. The catalog/source projection is unchanged.

Independent GPT-6 Astra whole-branch and scoped reviews accepted the source fixes and documentation correction, with no remaining source Critical/Important findings. All original I1–I11 and M1/M2 are addressed. Final full source gates and matching-release runtime checks have no remaining Critical/Important acceptance finding. The final documentation-only commit is followed by matching-provenance release smoke; the local archive records that final revision and integration result.

## Upgrade compatibility

The compatibility fixture was generated via APIs using pinned original commit f3bba9e1686fed940b25530469a0b25d28446eed at schema28, then opened by the new release. Read-only comparisons preserved old numeric quantities, monetary amounts, immutable price references and byte-identical execution price_json. Historical header timing moves to response_headers_ms while first-text/TTFT stays unknown; measurement availability remains unspecified. No SQL financial facts or customer data were created or modified.

## References and boundaries

Magpie (MIT) and AstrLink independent Apache2 behavior informed these contracts. AstrLink RelayKit/AGPL code and assets were excluded. LiteLLM Rust remains pinned to8c4c394ecc82c4d6acb5eb371d8781e487894a17. All existing capability inventory rows remain present. No push, tag or publishing occurs in this delivery.

## Local evidence

Raw review packages/reports, exact gate outputs, failed diagnostic runs, screenshot/accessible-state/computed-style evidence and the exhaustive decision ledger are retained at `/home/czyt/code/rust/pangolin/.superpowers/evidence/reference-adoption-20261003-0e12i0zr`. Synthetic private fixture credentials, helpers and data are excluded from the public evidence bundle. Local integration compares the main data-file metadata without opening or changing that database. No remote push, tag or release is performed.
