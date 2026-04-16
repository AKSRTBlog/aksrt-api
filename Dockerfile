# ── Stage 1: Build ──────────────────────────────────────────
FROM rust:1.82-bookworm AS builder

WORKDIR /app

# 复制源码
COPY Cargo.toml Cargo.lock* ./
COPY src/ ./src/
COPY migrations/ ./migrations/

# 编译
RUN cargo build --release && rm -rf src

# ── Stage 2: Runtime ────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 复制二进制和迁移文件
COPY --from=builder /app/target/release/aksrtblog-rust-api .
COPY --from=builder /app/migrations ./migrations

# 创建数据目录
RUN mkdir -p /app/storage/uploads

# 默认环境变量
ENV RUST_API_BIND=0.0.0.0:4000
ENV RUST_API_DATABASE_URL=postgres://postgres:postgres@db:5432/aksrtblog
ENV RUST_API_UPLOADS_DIR=/app/storage/uploads
ENV RUST_API_FRONTEND_DIR=
ENV RUST_API_CORS_ORIGIN=*
ENV RUST_API_PUBLIC_SITE_URL=http://localhost:3000

EXPOSE 4000

CMD ["./aksrtblog-rust-api"]
