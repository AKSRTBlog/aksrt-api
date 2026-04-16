# aksrtblog-api

Rust 博客系统后端 API 服务

## 一键部署

### 方式一：使用 Docker Hub 镜像（推荐）

```bash
# 克隆项目
git clone https://github.com/Lexo0522/aksrtblog-api.git
cd aksrtblog-api

# 配置环境变量
cp .env.example .env
vim .env

# 启动服务
docker compose up -d
```

### 方式二：使用外部 PostgreSQL 数据库

如果你已经有 PostgreSQL 数据库，只需配置连接字符串：

```bash
# 只启动 API 服务
docker run -d -p 4000:4000 \
  -e RUST_API_DATABASE_URL=postgres://user:password@your-db-host:5432/aksrtblog \
  -e RUST_API_CORS_ORIGIN=https://your-domain.com \
  -e RUST_API_PUBLIC_SITE_URL=https://your-domain.com \
  -v ./uploads:/app/storage/uploads \
  --name aksrtblog-api \
  lexo0522/aksrtblog-api:latest
```

## 环境变量

### docker-compose 方式

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `DB_HOST` | `localhost` | 数据库主机 |
| `DB_PORT` | `5432` | 数据库端口 |
| `DB_USER` | `postgres` | 数据库用户名 |
| `DB_PASSWORD` | `changeme` | 数据库密码 |
| `DB_NAME` | `aksrtblog` | 数据库名 |
| `UPLOADS_DIR` | `./data/uploads` | 上传文件持久化目录 |
| `CORS_ORIGIN` | `*` | CORS 允许的来源 |
| `SITE_URL` | `http://localhost:3000` | 公开访问 URL |

### 独立容器方式

| 变量 | 必须 | 说明 |
|------|------|------|
| `RUST_API_DATABASE_URL` | 是 | PostgreSQL 连接字符串 |
| `RUST_API_UPLOADS_DIR` | 否 | 上传目录，默认 `/app/storage/uploads` |
| `RUST_API_CORS_ORIGIN` | 否 | CORS 配置 |
| `RUST_API_PUBLIC_SITE_URL` | 否 | 公开 URL |

## 部署示例

### 示例 1：本地开发环境

```bash
docker compose up -d
# 访问 http://localhost:4000/api/v1/health
```

### 示例 2：使用外部云数据库

`.env` 文件：
```env
DB_HOST=your-rds.tencentcdb.com
DB_PORT=5432
DB_USER=blog_user
DB_PASSWORD=your_secure_password
DB_NAME=aksrtblog
CORS_ORIGIN=https://yourblog.com
SITE_URL=https://yourblog.com
UPLOADS_DIR=/data/aksrtblog/uploads
```

### 示例 3：服务器 Docker 一键启动

```bash
# SSH 到服务器
ssh user@your-server

# 创建目录
mkdir -p /opt/aksrtblog
cd /opt/aksrtblog

# 下载 docker-compose.yml
curl -O https://raw.githubusercontent.com/Lexo0522/aksrtblog-api/main/docker-compose.yml
curl -O https://raw.githubusercontent.com/Lexo0522/aksrtblog-api/main/.env.example

# 配置环境变量
cp .env.example .env
vim .env

# 启动
docker compose up -d
```

## 数据持久化

上传文件存储在 `${UPLOADS_DIR}` 目录，默认 `./data/uploads`。

**重要**：请确保该目录已创建且有写入权限：

```bash
mkdir -p ./data/uploads
chmod 755 ./data/uploads
```

## 验证部署

```bash
# 检查服务状态
docker compose ps

# 查看日志
docker compose logs -f api

# 健康检查
curl http://localhost:4000/api/v1/health
```

## 数据备份

```bash
# 备份数据库
docker compose exec db pg_dump -U postgres aksrtblog > backup_$(date +%Y%m%d).sql

# 备份上传文件
tar czf uploads_backup.tar.gz ./data/uploads/
```

## 常见问题

### Q: 容器启动失败，数据库连接错误？
A: 检查 `RUST_API_DATABASE_URL` 是否正确，确保数据库已启动且网络可达。

### Q: 上传文件失败？
A: 检查 `${UPLOADS_DIR}` 目录是否存在且有写入权限。

### Q: 如何更新到最新版本？
```bash
docker pull lexo0522/aksrtblog-api:latest
docker compose up -d
```
