# aksrtblog-api

Rust 后端 API 服务，博客系统后端。

## 快速开始

### Docker 部署

```bash
# 拉取最新代码
git pull

# 构建并启动
docker build -t aksrtblog-api .
docker run -d -p 4000:4000 \
  -e RUST_API_DATABASE_URL=postgres://user:pass@host:5432/db \
  -v ./uploads:/app/storage/uploads \
  --name aksrtblog-api \
  aksrtblog-api
```

### 使用 Docker Compose

```yaml
version: "3.8"

services:
  db:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: your_password
      POSTGRES_DB: aksrtblog
    volumes:
      - pgdata:/var/lib/postgresql/data
    ports:
      - "5432:5432"

  api:
    build: .
    restart: unless-stopped
    ports:
      - "4000:4000"
    environment:
      RUST_API_DATABASE_URL: postgres://postgres:your_password@db:5432/aksrtblog
      RUST_API_UPLOADS_DIR: /app/storage/uploads
      RUST_API_CORS_ORIGIN: https://your-domain.com
      RUST_API_PUBLIC_SITE_URL: https://your-domain.com
    volumes:
      - uploads:/app/storage/uploads
    depends_on:
      - db

volumes:
  pgdata:
  uploads:
```

## 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `RUST_API_BIND` | `0.0.0.0:4000` | 监听地址 |
| `RUST_API_DATABASE_URL` | - | PostgreSQL 连接字符串 |
| `RUST_API_UPLOADS_DIR` | `/app/storage/uploads` | 上传文件目录 |
| `RUST_API_FRONTEND_DIR` | `""` | 前端静态文件目录（留空则不提供） |
| `RUST_API_CORS_ORIGIN` | `*` | CORS 允许的来源 |
| `RUST_API_PUBLIC_SITE_URL` | `http://localhost:3000` | 公开访问 URL |

## API 端点

- `GET /api/v1/health` - 健康检查
- `GET /api/v1/posts` - 获取文章列表
- `GET /api/v1/posts/:slug` - 获取文章详情
- `POST /api/v1/auth/login` - 登录
- `POST /api/v1/auth/logout` - 登出
- `/api/v1/admin/*` - 管理接口（需认证）

## 数据备份

```bash
# 备份数据库
docker exec your-db-container pg_dump -U postgres aksrtblog > backup.sql
```
