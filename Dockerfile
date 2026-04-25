ARG RUST_IMAGE=rust:1-bookworm
ARG RUNTIME_IMAGE=debian:bookworm-slim

# Stage 1: Build
FROM ${RUST_IMAGE} AS builder

ARG CARGO_REGISTRY_MIRROR=sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/
ARG TARGETPLATFORM

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
ENV CARGO_NET_RETRY=10
ENV CARGO_HTTP_TIMEOUT=300
ENV CARGO_HTTP_MULTIPLEXING=false
ENV CARGO_TERM_VERBOSE=true

WORKDIR /app

COPY Cargo.toml Cargo.lock* ./
RUN --mount=type=cache,id=aksrtblog-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=aksrtblog-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    unset HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY http_proxy https_proxy all_proxy no_proxy; \
    set -eux; \
    mkdir -p /usr/local/cargo; \
    printf '[source.crates-io]\nreplace-with = "tuna"\n\n[source.tuna]\nregistry = "%s"\n' "$CARGO_REGISTRY_MIRROR" > /usr/local/cargo/config.toml; \
    cargo --version; \
    echo "Target platform: ${TARGETPLATFORM:-local}"; \
    echo "Using Cargo registry mirror:"; \
    cat /usr/local/cargo/config.toml; \
    cargo fetch --locked --verbose

# Pre-build dependencies with a tiny placeholder binary so source edits
# don't invalidate the whole dependency graph on every rebuild.
RUN --mount=type=cache,id=aksrtblog-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=aksrtblog-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=aksrtblog-target-linux-amd64,target=/app/target,sharing=locked \
    unset HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY http_proxy https_proxy all_proxy no_proxy; \
    set -eux; \
    mkdir -p src && \
    printf 'fn main() {}\n' > src/main.rs && \
    cargo build --release --locked --offline --verbose && \
    rm -f target/release/aksrtblog-rust-api target/release/aksrtblog-rust-api.d && \
    rm -rf target/release/deps/aksrtblog_rust_api* target/release/.fingerprint/aksrtblog-rust-api-* && \
    rm -rf src

COPY src ./src/
COPY migrations ./migrations/

RUN --mount=type=cache,id=aksrtblog-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=aksrtblog-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=aksrtblog-target-linux-amd64,target=/app/target,sharing=locked \
    unset HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY http_proxy https_proxy all_proxy no_proxy; \
    set -eux; \
    find src -type f -exec touch {} +; \
    rm -f target/release/aksrtblog-rust-api target/release/aksrtblog-rust-api.d; \
    rm -rf target/release/deps/aksrtblog_rust_api* target/release/.fingerprint/aksrtblog-rust-api-*; \
    cargo build --release --locked --offline --verbose && \
    mkdir -p /app/bin && \
    cp /app/target/release/aksrtblog-rust-api /app/bin/aksrtblog-rust-api && \
    test -s /app/bin/aksrtblog-rust-api && \
    grep -a "Rust API listening" /app/bin/aksrtblog-rust-api >/dev/null

# Stage 2: Runtime
FROM ${RUNTIME_IMAGE}

RUN unset HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY http_proxy https_proxy all_proxy no_proxy; \
    apt-get update \
      -o Acquire::Retries=3 \
      -o Acquire::http::Proxy=false \
      -o Acquire::https::Proxy=false && \
    unset HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY http_proxy https_proxy all_proxy no_proxy; \
    apt-get install -y --no-install-recommends \
    ca-certificates libssl3 && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/bin/aksrtblog-rust-api .
COPY --from=builder /app/migrations ./migrations

RUN mkdir -p /app/storage/uploads

ENV RUST_API_BIND=0.0.0.0:4000
ENV RUST_API_DATABASE_URL=postgres://postgres:postgres@db:5432/aksrtblog
ENV RUST_API_UPLOADS_DIR=/app/storage/uploads
ENV RUST_API_CORS_ORIGIN=*
ENV RUST_API_PUBLIC_SITE_URL=http://localhost:3000

EXPOSE 4000

CMD ["./aksrtblog-rust-api"]
