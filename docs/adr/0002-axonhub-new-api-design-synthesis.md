# ADR 0002: AxonHub and new-api design synthesis

Status: accepted

## Context

Pangolin targets the operational depth of AxonHub while studying new-api as a mature AI-gateway product. AxonHub emphasizes project-scoped authorization, request orchestration and traceability. new-api emphasizes provider breadth, channel presets, groups/tokens/ratios, task adapters and operational gateway ergonomics. Pangolin is Apache-2.0; new-api commit `972aed1972820389ea0b603ca58f03f846fbf790` is AGPL-3.0, so only public behavior and design semantics may be studied—no code or assets are copied.

cc-switch commit `06082e189d65e6d6dbadc35dacdac1ce6c79d89a` is an MIT Rust proxy reference for pipeline boundaries, failover switching, usage/cache parsing, media sanitization and client-specific compatibility. Tauri lifecycle/UI code is not part of Pangolin.

## Decisions

| Domain | Chosen design | Why | Rejected alternative |
| --- | --- | --- | --- |
| Tenant/access model | AxonHub-style Project + membership + RBAC; API keys belong to projects | Strong isolation and enterprise delegation | new-api global groups as the only tenant boundary |
| Service tiers/groups | Add new-api-style service groups inside a project for model/channel availability and pricing policy | Useful operational abstraction without weakening project isolation | Encoding tiers as ad-hoc API-key JSON |
| Routing | AxonHub candidate associations/conditions plus new-api channel priority, weight, affinity and multi-key ergonomics | Combines explainability with practical provider operations | One opaque weighted channel table |
| Session continuity | new-api rule-driven affinity (`off/prefer/strict`, header/body/trace keys, TTL/capacity, cache-hit metrics) + AxonHub durable Responses history | Maximizes provider cache reuse, then preserves correctness on failover | Default vector memory or provider-local IDs only |
| Provider presets | Versioned built-in provider catalog, informed by both projects; online signed subscriptions and local overrides | New providers/models arrive without binary changes; fresh installs work offline | Hardcoded constants only or mandatory first-run network fetch |
| Model metadata | AxonHub-style model cards plus new-api-style type/price/ratio compatibility metadata | Strong capability discovery and operational migration | Model name + two prices only |
| Accounting | Immutable price versions and integer micro-USD cost are authoritative; ratios/groups are derived operational policy | Auditability and deterministic arithmetic | Floating-point ratio as the financial record |
| Provider icons | Catalog `logo_key` mapped to bundled `@lobehub/icons`, then Simple Icons, then initials | Consistent offline light/dark rendering; new-api independently validates the same ecosystem choice | Copying project-owned logos or hand-drawn SVGs |
| Protocol conversion | LiteLLM Rust pure crates + Pangolin adapters + native pass-through | Reuses maintained protocol/auth/framing/token/cache work without surrendering gateway semantics | Reimplement every provider or use an Agent framework as gateway core |
| Async media/tasks | Durable task lifecycle with built-in declarative adapters; no remote executable plugins from catalog subscriptions | Preserves extensibility without supply-chain code injection | new-api-style arbitrary remotely sourced executable plugin behavior |
| Traceability | AxonHub Thread → Trace → Request → Execution → Usage/Cost | Explains retries, routing and cost per attempt | One flat request log |
| Backup | AxonHub selective/conflict UX + Raindrop immutable target snapshots, revision fencing, encrypted secret separation and owned-prefix retention | Strong recoverability in Rust/SQLite | Database file copy presented as complete backup |
| Proxy internals | cc-switch-style body filter → transform → upstream → stream/response → usage stages, plus switch singleflight and media sanitization | Proven Rust separation and fewer protocol/accounting leaks | One monolithic proxy handler or Tauri-specific lifecycle |
| Commercial billing | Core supports balances, budgets, service tiers and subscriptions; payment/top-up providers remain modular | Keeps gateway useful for teams and operators without coupling core to one reseller business model | Baking payment processors into request execution |

## Catalog precedence and lifecycle

`local override > high-priority signed subscription > lower-priority subscription > bundled catalog`.

- Bundled data is nonempty and versioned.
- Subscription documents are declarative JSON only, bounded and schema validated.
- HTTPS conditional requests, Ed25519 key pinning, staging and atomic activation preserve a last-known-good snapshot.
- Unknown typed capability extensions are retained for future UI/adapter versions.
- A catalog entry explicitly distinguishes `metadata_available` from `adapter_available` and `live_tested`.

## Consequences

- Pangolin gains more schema and catalog maintenance than either a minimal proxy or a direct fork.
- Provider/model additions using existing protocols can ship through signed catalog updates.
- New executable protocol/auth behavior still requires a reviewed binary release.
- new-api is credited in README, but no AGPL implementation or image asset enters Pangolin.
- Explicitly disabled channels always invalidate affinity; Pangolin does not adopt a keep-disabled binding default.
