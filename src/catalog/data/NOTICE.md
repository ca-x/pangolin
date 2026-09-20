# Catalog data attribution

`builtin.json` contains factual provider/model metadata projected and normalized from AxonHub's Apache-2.0 catalog files:

- Repository: https://github.com/looplj/axonhub
- Revision: `cb29b65d9adfb06f89bb1b467418e0816988f36c`
- Files: `internal/server/biz/catalogdata/providers.json` and `models.json`
- License: Apache License, Version 2.0; the repository's root Apache license applies to these `internal/` data files. Pangolin's root `LICENSE` includes the same license text.

Pangolin changed the schema, projected factual capability/limit/price fields, qualified model identifiers by developer, added curated provider setup metadata, recorded provenance and introduced explicit unknown values and adapter-availability markers. Original prose descriptions and upstream image assets are not included.

The new-api repository at `972aed1972820389ea0b603ca58f03f846fbf790` was consulted only for observable provider categorization and metadata-management behavior. No AGPL implementation code or image assets were copied.

`logo_key` values are identifiers intended for an independently bundled icon library, not included image files. Every provider also has locally generated initials. Provider names are identification metadata and do not imply endorsement. Task 6 supplies offline icons and their library notices; unsupported keys fall back to initials.
