# Pangolin tasks

- [x] Foundation scaffold and persistence
  - Acceptance: configuration, migrations, setup/auth and embedded assets compile and are tested.
  - Verify: `cargo test --all-features` and release embedding test.
- [x] Gateway control plane and compatible endpoints
  - Acceptance: providers/models/routes/keys work; mock provider contracts pass.
  - Verify: Rust integration tests against a local mock upstream.
- [x] DuckDB observation store
  - Acceptance: events persist independently, aggregates filter correctly, failures do not fail requests.
  - Verify: observation integration tests with temporary files.
- [x] React administration console
  - Acceptance: setup and daily operations work in English/Chinese across themes and breakpoints.
  - Verify: Vitest plus browser smoke/a11y checks.
- [x] Delivery and documentation
  - Acceptance: Docker and release workflows mirror the proven raindrop structure and README credits references.
  - Verify: local builds, workflow syntax review and container build where Docker is available.
- [x] Independent review and final verification
  - Acceptance: `gpt-6-astra` findings resolved or explicitly documented; all available checks pass.
  - Verify: fresh full command suite and requirement checklist.
