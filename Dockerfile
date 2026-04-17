# 鈹€鈹€ Stage 1: Build 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
# 浣跨敤鍥藉唴闀滃儚鍔犻€燂紝绂佺敤浠ｇ悊
ARG RUST_IMAGE=rust:1-bookworm
FROM ${RUST_IMAGE} AS builder

ENV HTTP_PROXY=
ENV HTTPS_PROXY=
ENV http_proxy=
ENV https_proxy=
ENV ALL_PROXY=
ENV all_proxy=
ENV NO_PROXY=
ENV no_proxy=
ENV CARGO_HTTP_PROXY=

WORKDIR /app

# 澶嶅埗婧愮爜
COPY Cargo.toml Cargo.lock* ./
COPY src/ ./src/
COPY migrations/ ./migrations/

# 缂栬瘧
RUN export HTTP_PROXY= HTTPS_PROXY= http_proxy= https_proxy= ALL_PROXY= all_proxy= NO_PROXY= no_proxy= CARGO_HTTP_PROXY= && \
    cargo build --release && rm -rf src

# 鈹€鈹€ Stage 2: Runtime 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
FROM debian:bookworm-slim

# 绂佺敤浠ｇ悊锛屼娇鐢ㄨ吘璁簯闀滃儚
ENV HTTP_PROXY=
ENV HTTPS_PROXY=
ENV http_proxy=
ENV https_proxy=
ENV ALL_PROXY=
ENV all_proxy=
ENV NO_PROXY=
ENV no_proxy=
RUN export HTTP_PROXY= HTTPS_PROXY= http_proxy= https_proxy= ALL_PROXY= all_proxy= NO_PROXY= no_proxy= && \
    apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 澶嶅埗浜岃繘鍒跺拰杩佺Щ鏂囦欢
COPY --from=builder /app/target/release/aksrtblog-rust-api .
COPY --from=builder /app/migrations ./migrations

# 鍒涘缓鏁版嵁鐩綍
RUN mkdir -p /app/storage/uploads

# 榛樿鐜鍙橀噺
ENV RUST_API_BIND=0.0.0.0:4000
ENV RUST_API_DATABASE_URL=postgres://postgres:postgres@db:5432/aksrtblog
ENV RUST_API_UPLOADS_DIR=/app/storage/uploads
ENV RUST_API_FRONTEND_DIR=
ENV RUST_API_CORS_ORIGIN=*
ENV RUST_API_PUBLIC_SITE_URL=http://localhost:3000

EXPOSE 4000

CMD ["./aksrtblog-rust-api"]

