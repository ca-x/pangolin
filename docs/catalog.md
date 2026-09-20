# Provider and model catalog

Pangolin embeds 59 provider presets and 453 model cards. The initial catalog works without network access. Its version, upstream revision and per-entry provenance are included in every export. The AxonHub factual catalog source is Apache-2.0; the new-api reference supplied behavioral ideas only. See `src/catalog/data/NOTICE.md`.

Catalog records describe providers and models. They do not grant project access, enable inference channels, install adapters or run code. `adapter_available` is computed from the adapter kinds compiled into Pangolin. The catalog includes providers with no adapter so they remain discoverable without implying support. Discovery/quota fields distinguish upstream endpoint metadata from implemented collectors.

Creating a provider with a catalog preset ID uses its adapter family, default base URL and declarative endpoint mappings. An explicitly supplied base URL wins. Creating a model applies catalog capabilities and USD-per-million token defaults only when the administrator omitted those fields; explicit zero prices are preserved. Model cards and their catalog version are saved in `catalog_metadata_json`. Later subscription refreshes do not overwrite existing channel/model configuration.

## Data format and precedence

Catalog JSON uses `schema_version: 1`, a nonempty `version`, `source`, `providers`, `models`, and an `extensions` object. Provider entries contain name/category/base/auth/adapter, typed endpoint descriptors, discovery/quota metadata, logo keys and provenance. Model entries contain developer/upstream ID/type, input/output modalities, protocols, optional capability booleans, limits, aliases, lifecycle/dates and optional cost defaults. Missing facts are `null`, not invented values. Unknown capability keys and explicit extension objects survive import/export.

Entries merge by stable ID in this order:

1. Built-in catalog.
2. Enabled subscriptions, ascending priority; higher numbers win. Equal priorities use lexicographic source ID as a deterministic tie-breaker, with the later ID winning.
3. Local administrator overrides and custom entries.

Import upserts local entries. It never deletes local entries that are absent from the imported document. A subscription replacing or removing one of its entries cannot delete local custom data. Removing a local override reveals the next applicable source. Export returns a complete versioned merged catalog that can be imported into another instance.

Document-level `extensions` use the same precedence, independently of provider/model capabilities. They merge by top-level key; a higher-priority value replaces the entire JSON value at that key (including explicit `null`). Import upserts these local extension keys without deleting omitted ones. Publisher/application keys such as `applied_sources` or `builtin_version` are preserved; runtime merge provenance is represented by `source.revision` and the effective catalog version, not by overwriting extension keys.

## API

Reads require a signed-in session. Global source/override mutations require the system `catalog:manage` permission (owner/admin by default); project API keys cannot mutate global catalog state.

| Route under `/api/admin/v1/catalog` | Method / behavior |
| --- | --- |
| `/`, `/export` | GET complete merged catalog |
| `/providers`, `/models` | GET filtered/paginated records with effective version and total |
| `/providers/{id}`, `/models/{id}` | GET record; models also resolve upstream IDs and aliases |
| `/import` | POST versioned catalog JSON, upserting local entries |
| `/overrides/{provider\|model}/{id}` | PUT complete entry with matching ID; DELETE local override |
| `/sources` | GET source state; POST source configuration |
| `/sources/{id}` | PUT `{revision, source: {...}}`; DELETE source and its snapshots |
| `/sources/{id}/refresh` | POST manual conditional refresh |
| `/sources/refresh-due` | POST refresh currently due sources |
| `/sources/{id}/snapshots` | GET retained versions, digest, origin URL and signature result |
| `/sources/{id}/rollback` | POST `{revision, snapshot_id}` to activate a retained version |

Filters include `q`, `developer`/`provider`, `type`, `category`, `modality`, `protocol`, `capability`, `adapter_available`, `offset`, and `limit` (1–1000). Provider aliases such as Gemini→Google, Doubao→ByteDance, Zhipu→Z.ai, Kimi→Moonshot and Qwen/Bailian→Alibaba normalize developer searches. Provider/model list APIs apply the filters relevant to each record kind.

Source configuration example:

```json
{
  "name": "Team catalog",
  "url": "https://catalog.example.com/pangolin.json",
  "priority": 100,
  "refresh_interval_secs": 3600,
  "enabled": true,
  "signature_policy": "required",
  "public_key": "BASE64_ENCODED_32_BYTE_ED25519_PUBLIC_KEY"
}
```

There are no automatically configured external subscriptions. Operators opt into a source and choose its trust policy. `none`, `optional` and `required` signature policies are supported. When a signature is supplied, optional policy also requires a matching pinned key and valid signature. Publishers send a base64-encoded 64-byte Ed25519 signature in `X-Pangolin-Catalog-Signature`, signing the **exact JSON response bytes**. No JSON canonicalization is performed before verification.

## Refresh and storage guarantees

Sources must use HTTPS on port 443 with no URL credentials or fragments. Refresh resolves and validates all destination addresses, rejects loopback/private/link-local/reserved destinations, pins the checked addresses, disables environment proxies and refuses redirects. DNS is limited to 5 seconds, HTTP to 15 seconds, and each refresh to 20 seconds. Documents and merged exports are limited to 4 MiB, with strict schema and duplicate-ID validation. At most 32 sources are configured.

ETag and Last-Modified are stored with the active snapshot and sent as conditional headers. A 304 is accepted only when a valid active conditional snapshot exists. Configuration edits clear validators, forcing a full fetch. Invalid JSON/schema, oversize responses, transport errors and signature failures preserve the active snapshot and last-success time.

Validated snapshots activate transactionally with a revision check, so a late refresh cannot overwrite a newer source edit or activation. Aggregate merge validation occurs inside the same transaction. SQLite stores active/previous snapshots, last attempt/success/error, HTTP validators and signature verification metadata. Up to five recent snapshots plus any separately pinned active/previous versions are retained. Rollback is source-scoped and revision-checked. Local overrides use separate rows and remain higher priority than subscriptions.

`catalog::repository::due_sources` and `catalog::refresh::refresh` are the Task 5 scheduler integration points. Automatic scheduler ticks are not installed in Task 4. The manual service/API is complete and refreshes do not require rebuilding the binary.

Logo keys use `lobehub:Name`, `simple-icons:slug`, or `initials:id`, with a nonempty initials fallback, optional brand color and monochrome preference. No upstream logo assets are copied. Task 6 bundles icon libraries offline and resolves the keys during onboarding/provider selection.
