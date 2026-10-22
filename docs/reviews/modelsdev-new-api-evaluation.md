# models.dev and new-api behavior evaluation

## Evidence and licensing

Pangolin baseline: `ee89e4b` / v0.7.0. Evaluated local new-api checkout:
`7fb6e57a5ff671582131c74e00bdfaa867d4e985` (AGPL-3.0). Its README and observable
contract/test scenarios inform independently implemented Rust/UI behavior only;
no implementation code or assets are copied. The official models.dev MIT
`api.json` and `@opencode-ai/models` SDK are the schema/behavior references for
the separately maintained MIT `lib-x/models-dev-rs` crate.

## Selection

| Area | Adopted for this iteration | Boundary / not adopted |
| --- | --- | --- |
| Protocol conversion | Nonstreaming Anthropic/OpenAI function tools with IDs, arguments and text results; explicit rejection of lossy shapes | No claim of universal tools, streaming transforms or multimodal conversion |
| Token control | One explicit output-budget interpretation; conflicting aliases fail closed across preflight, admission and model limits | No byte estimate presented as measured usage; no new financial formula or distributed quota |
| Model information | Safe optional models.dev subscription, daily refresh, validators, provenance, last-known-good and local override precedence | No metadata-controlled adapter install, silent provider enablement or live-price rewrite |
| Ease of use | One-click official source, direct catalog-to-subscriptions navigation, visible sync/disable/retry and editable existing source | No mandatory first-run network access or extra setup pages; disabling excludes source metadata but preserves snapshots and configured project models |
| Operational trust | Preserve custom proxy/signature/interval; reuse existing official source across clients; distinguish unmeasured facts | No inferred zero counts or claim of live-tested provider inference |
| Existing capabilities retained | Project-scoped keys, budgets, model access, multi-key channels, probes, connection snippets and immutable costing | No global-group-only tenancy, payments/subscriptions or merchant features unrelated to a self-hosted gateway |

## UI review

| Before | After | Why |
| --- | --- | --- |
| Native models.dev JSON cannot be connected through the generic feed form | Named, explicit official-source action with existing settings preserved | Reduce format/setup mistakes without hidden network access |
| Catalog browsing gives no direct route to automatic updates | Localized shortcut to subscriptions | Keep the next action near the data it updates |
| A failed update can look like missing or fresh data | Error/retry plus last successful snapshot status | Make recovery and data freshness explicit |
| Subscription editing omits the selected outbound proxy | Preserve the existing proxy value | An unrelated edit must not change network policy |
| An unavailable assigned proxy prevents stopping a subscription | Allow disabling while retaining the reference; refuse re-enabling until a usable proxy is restored or the assignment is removed | An operational stop must not depend on outbound availability or erase network policy |
| Disabling is described as merely pausing background updates | Explicit disable/enable actions explain exclusion from the effective catalog and preservation of saved snapshots | Match the existing backend semantics rather than promise stale metadata remains active |
| Operational capability values render at 12.5px | Shared capability values render at 14px | Keep reference data readable without restyling unrelated microcopy |
| Frequent operational actions invite decorative transitions | New data/status/actions stay static; existing feedback tokens remain | Responsiveness and reduced-motion safety take precedence |

## Verification and release status

Fresh command results, release-binary browser evidence and independent review
findings are recorded in `tasks/modelsdev-and-gateway-usability-plan.md` and the
delivery report. A requested GPT-6 review was attempted; the tool returned
`Unknown model gpt-6`. Independent available-model review is not labeled GPT-6
and does not, without explicit user waiver, satisfy that release gate.
