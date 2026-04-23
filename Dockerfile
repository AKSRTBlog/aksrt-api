# Stage 1: Build
ARG RUST_IMAGE=rust:1-bookworm
FROM ${RUST_IMAGE} AS builder

# Optional: pass a mirror like sparse+https://rsproxy.cn/index/
ARG CARGO_REGISTRY_MIRROR=
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
ENV CARGO_NET_RETRY=10
ENV CARGO_HTTP_TIMEOUT=600
ENV CARGO_HTTP_MULTIPLEXING=false

WORKDIR /app

COPY Cargo.toml Cargo.lock* ./
COPY src/ ./src/
COPY migrations/ ./migrations/

RUN if [ -n "$CARGO_REGISTRY_MIRROR" ]; then \
      printf '[source.crates-io]\nreplace-with = "mirror"\n[source.mirror]\nregistry = "%s"\n' "$CARGO_REGISTRY_MIRROR" > /usr/local/cargo/config.toml; \
    fi && \
    HTTP_PROXY= HTTPS_PROXY= http_proxy= https_proxy= ALL_PROXY= all_proxy= NO_PROXY= no_proxy= \
    cargo build --release --locked && rm -rf src

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN HTTP_PROXY= HTTPS_PROXY= http_proxy= https_proxy= ALL_PROXY= all_proxy= NO_PROXY= no_proxy= \
    apt-get update -o Acquire::Retries=3 -o Acquire::http::Proxy=false -o Acquire::https::Proxy=false && \
    HTTP_PROXY= HTTPS_PROXY= http_proxy= https_proxy= ALL_PROXY= all_proxy= NO_PROXY= no_proxy= \
    apt-get install -y --no-install-recommends \
    ca-certificates libssl3 && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/aksrtblog-rust-api .
COPY --from=builder /app/migrations ./migrations

RUN mkdir -p /app/storage/uploads

ENV RUST_API_BIND=0.0.0.0:4000
ENV RUST_API_DATABASE_URL=postgres://postgres:postgres@db:5432/aksrtblog
ENV RUST_API_UPLOADS_DIR=/app/storage/uploads
ENV RUST_API_CORS_ORIGIN=*
ENV RUST_API_PUBLIC_SITE_URL=http://localhost:3000

EXPOSE 4000

CMD ["./aksrtblog-rust-api"]
