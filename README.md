# aksrtblog-api

AKSRT Blog 的 Rust 后端，当前技术栈为 `axum + PostgreSQL`。

## 目录说明

- `src/main.rs`: HTTP 路由、鉴权、公开接口、后台接口
- `src/db.rs`: 数据库访问与业务写入逻辑
- `migrations/`: PostgreSQL 初始化 SQL
- `docker-compose.yml`: 后端 + PostgreSQL 的本地/部署编排

## 快速开始

### 1. 使用 Docker Compose

```bash
cp .env.example .env
docker compose up -d --build
```

启动后可检查：

```bash
curl http://localhost:4000/api/v1/health
curl http://localhost:4000/api/v1/public/site-settings
```

### 2. 本地直接运行

先准备 PostgreSQL，然后设置环境变量：

```env
RUST_API_BIND=127.0.0.1:4000
RUST_API_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/aksrtblog
RUST_API_UPLOADS_DIR=storage/uploads
RUST_API_CORS_ORIGIN=http://127.0.0.1:3000
RUST_API_PUBLIC_SITE_URL=http://127.0.0.1:3000
```

运行：

```bash
cargo run
```

## 关键环境变量

### Docker Compose 场景

- `DB_PASSWORD`: PostgreSQL 密码
- `CORS_ORIGIN`: 允许访问 API 的前端来源
- `SITE_URL`: 公开站点地址，用于初始化 SEO canonical URL

### 直接运行场景

- `RUST_API_BIND`: 服务监听地址，默认 `0.0.0.0:4000`
- `RUST_API_DATABASE_URL`: PostgreSQL 连接串
- `RUST_API_UPLOADS_DIR`: 上传文件目录，默认 `storage/uploads`
- `RUST_API_CORS_ORIGIN`: CORS 来源
- `RUST_API_PUBLIC_SITE_URL`: 公开站点地址，默认 `http://127.0.0.1:3000`
- `RUST_API_FRONTEND_DIR`: 可选。设置后后端会直接托管前端静态产物

## 默认初始化行为

数据库为空时，服务启动会自动：

1. 执行 `migrations/*.sql`
2. 初始化默认管理员
3. 初始化站点设置、存储配置、SMTP 配置等基础记录

## 常用命令

```bash
cargo test
cargo run
docker compose up -d --build
docker compose logs -f api
```
