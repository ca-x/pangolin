# syntax=docker/dockerfile:1.7

FROM node:26.9.0-bookworm-slim AS web-builder
WORKDIR /src/web
RUN npm install --global pnpm@11.0.0 --ignore-scripts
COPY web/package.json web/pnpm-lock.yaml ./
RUN --mount=type=cache,target=/root/.local/share/pnpm/store,sharing=locked \
    pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

FROM rust:1.98.0-bookworm AS rust-builder
WORKDIR /src
RUN apt-get update && apt-get install --yes --no-install-recommends g++ && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock rust-toolchain.toml build.rs ./
COPY src ./src
COPY web ./web
COPY --from=web-builder /src/web/dist ./web/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked && cp target/release/pangolin /tmp/pangolin

FROM debian:bookworm-slim AS runtime
ARG VERSION=dev
ARG BUILD_TIME=unknown
ARG GIT_COMMIT=unknown
LABEL org.opencontainers.image.title="Pangolin" \
      org.opencontainers.image.description="Pangolin / 鲮鲤 self-hosted AI API gateway" \
      org.opencontainers.image.source="https://github.com/ca-x/pangolin" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.created="${BUILD_TIME}" \
      org.opencontainers.image.revision="${GIT_COMMIT}"
RUN apt-get update && apt-get install --yes --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 pangolin && \
    useradd --uid 10001 --gid 10001 --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin pangolin && \
    install -d -o 10001 -g 10001 -m 0700 /data
COPY --from=rust-builder --chown=10001:10001 /tmp/pangolin /usr/local/bin/pangolin
ENV PANGOLIN_DATA_DIR=/data PANGOLIN_BIND=0.0.0.0:8080
VOLUME ["/data"]
EXPOSE 8080
STOPSIGNAL SIGTERM
USER 10001:10001
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 CMD ["curl", "--fail", "--silent", "http://127.0.0.1:8080/api/health/live"]
ENTRYPOINT ["/usr/local/bin/pangolin"]
