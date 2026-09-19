# Spec: delivery

## Objective

Ship reproducible source checks, native release binaries and OCI images with the same embedded console.

## Requirements

- CI verifies Rust format, Clippy, tests, Web lint/typecheck/tests/build and release asset embedding.
- Tagged releases build Linux amd64/arm64, macOS amd64/arm64 and Windows amd64 archives on native runners.
- Docker publishes Linux amd64/arm64 to GHCR; Docker Hub is optional when credentials exist.
- The runtime image is non-root, owns `/data`, exposes health checks and contains no Node toolchain.
- GitHub Actions are pinned to immutable commit SHAs following the raindrop reference.

## Success criteria

- `docker build .` creates an image that initializes and serves the UI.
- A `v*` tag produces checksummed binary archives and a GitHub Release.
- README documents setup, environment variables, Docker, source builds, architecture, security defaults and all reference projects with links and licenses.
