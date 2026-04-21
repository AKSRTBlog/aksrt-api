# aksrtblog-api

AKSRT 博客系统后端 API 服务（Rust + PostgreSQL）

## 仓库信息

- **GitHub**: https://github.com/AKSRTBlog/aksrt-api
- **Docker Hub**: `kate522/aksrtblog-api:latest`
- **架构**: Rust (Actix-web) + PostgreSQL 16

## 一键部署

### 方式一：使用 Docker Compose（推荐，含内置数据库）

```bash
# 克隆项目
git clone https://github.com/AKSRTBlog/aksrt-api.git
cd aksrt-api

# 配置环境变量
cp .env.example .env
vim .env

# 启动服务（PostgreSQL + API）
docker compose up -d
```

### 方式二：使用外部 PostgreSQL 数据库

如果你已有云数据库（如腾讯云 RDS），只需启动 API 容器：

```bash
docker run -d -p 4000:4000 \
  -e RUST_API_DATABASE_URL=postgres://user:password@your-db-host:5432/aksrtblog \
  -e RUST_API_CORS_ORIGIN=https://your-domain.com \
  -e RUST_API_PUBLIC_SITE_URL=https://your-domain.com \
  -v ./uploads:/app/storage/uploads \
  --name aksrtblog-api \
  --restart unless-stopped \
  kate522/aksrtblog-api:latest
```

## 环境变量

| 变量 | 必须 | 默认值 | 说明 |
|------|------|--------|------|
| `DB_HOST` | 否 | `localhost` | 数据库主机（Docker Compose 模式） |
| `DB_PORT` | 否 | `5432` | 数据库端口（Docker Compose 模式） |
| `DB_USER` | 否 | `postgres` | 数据库用户名 |
| `DB_PASSWORD` | 否 | `changeme` | 数据库密码 |
| `DB_NAME` | 否 | `aksrtblog` | 数据库名 |
| `EXTERNAL_DB_URL` | 否 | — | 外部数据库连接字符串（优先于上述 DB_* 变量） |
| `CORS_ORIGIN` | 否 | `*` | CORS 允许的来源 |
| `SITE_URL` | 否 | `http://localhost:3000` | 公开站点 URL（用于 SEO canonical_url 等） |
| `UPLOADS_DIR` | 否 | `./data/uploads` | 上传文件持久化目录 |

### 独立容器方式的环境变量

| 变量 | 说明 |
|------|------|
| `RUST_API_DATABASE_URL` | PostgreSQL 连接字符串（必须） |
| `RUST_API_CORS_ORIGIN` | CORS 配置 |
| `RUST_API_PUBLIC_SITE_URL` | 公开站点 URL（初始化时自动写入 canonical_url） |
| `RUST_API_UPLOADS_DIR` | 上传目录路径 |
| `RUST_API_BIND` | 绑定地址，默认 `0.0.0.0:4000` |

## 首次启动行为

首次启动且数据库为空时，后端会自动执行：

1. **执行数据库迁移** (`migrations/*.sql`) —— 建表、建索引
2. **创建默认管理员账户**：
   - 用户名: `admin`
   - 密码: 由环境变量 `ADMIN_PASSWORD` 或默认 `admin123456`
3. **初始化站点设置** —— 标题、描述、SEO 信息等
4. **初始化存储配置** —— 本地存储模式
5. **初始化 SMTP / 验证码配置** —— 默认关闭

> **注意**：`seo_canonical_url` 在初始化时会自动填入 `RUST_API_PUBLIC_SITE_URL` 的值，
> 无需手动在后台配置站点地址。

## 部署示例

### 示例 1：本地开发环境

```bash
docker compose up -d
# 访问 http://localhost:4000/api/v1/health
```

### 示例 2：使用外部云数据库

`.env` 文件：

```env
EXTERNAL_DB_URL=postgres://blog_user:password@your-rds.tencentcdb.com:5432/aksrtblog
CORS_ORIGIN=https://yourblog.com
SITE_URL=https://yourblog.com
UPLOADS_DIR=/data/aksrtblog/uploads
```

### 示例 3：服务器一键部署

```bash
# SSH 到服务器
ssh user@your-server

mkdir -p /opt/aksrtblog && cd /opt/aksrtblog

# 下载配置文件
curl -O https://raw.githubusercontent.com/AKSRTBlog/aksrt-api/main/docker-compose.yml
curl -O https://raw.githubusercontent.com/AKSRTBlog/aksrt-api/main/.env.example

# 配置并启动
cp .env.example .env && vim .env
docker compose up -d
```

## 数据持久化

上传文件存储在 `${UPLOADS_DIR}` 目录（默认 `./data/uploads`）。

**重要**：确保该目录有写入权限：

```bash
mkdir -p ./data/uploads
chmod 755 ./data/uploads
```

PostgreSQL 数据通过 Docker Volume `pgdata` 持久化。

## 验证部署

```bash
# 检查服务状态
docker compose ps

# 查看日志
docker compose logs -f api

# 健康检查
curl http://localhost:4000/api/v1/health

# 测试公开 API
curl http://localhost:4000/api/v1/public/site-settings
curl http://localhost:4000/api/v1/public/articles?page=1&pageSize=6
```

## 构建与推送镜像

```bash
# 本地构建（需交叉编译到 linux/amd64）
docker build --platform linux/amd64 -t kate522/aksrtblog-api:latest .

# 推送到 Docker Hub
docker push kate522/aksrtblog-api:latest
```

> **国内网络提示**：如遇 `TLS handshake timeout`，请配置 Docker 镜像加速：
> 编辑 `~/.docker/daemon.json` 添加 `registry-mirrors`，或开启代理后重试。

## 更新版本

```bash
# 方式一：重新构建
docker compose up -d --build

# 方式二：拉取新镜像
docker pull kate522/aksrtblog-api:latest
docker compose up -d
```

## 备份与恢复

```bash
# 备份数据库
docker compose exec db pg_dump -U postgres aksrtblog > backup_$(date +%Y%m%d).sql

# 备份上传文件
tar czf uploads_backup.tar.gz ./data/uploads/

# 恢复数据库
cat backup.sql | docker compose exec -T db psql -U postgres aksrtblog
```

## API 接口概览

| 分类 | 路径前缀 | 说明 |
|------|----------|------|
| 公开 API | `/api/v1/public/*` | 站点设置、文章、分类、标签、归档、搜索、友链等 |
| 评论提交 | `/api/v1/comments` | 游客发表评论 |
| 友链申请 | `/api/v1/friend-links/applications` | 提交友链申请 |
| 管理 API | `/api/v1/admin/*` | 需要 admin token，覆盖所有管理功能 |
| 健康检查 | `/api/v1/health` | 服务状态 |

## 常见问题

**Q: 容器启动失败，数据库连接错误？**
A: 检查 `RUST_API_DATABASE_URL` 是否正确，确保数据库已启动且网络可达。使用外部数据库时确认防火墙已开放 5432 端口。

**Q: 上传文件失败？**
A: 检查 `${UPLOADS_DIR}` 目录是否存在且有写入权限。

**Q: 首次访问前端报错 canonical_url 指向 localhost？**
A: 已修复——初始化时自动使用 `RUST_API_PUBLIC_SITE_URL` 的值。确保 `.env` 中 `SITE_URL` 设置为真实域名。

**Q: 如何重置管理员密码？**
A: 删除数据库中的 admin 用户记录，重启容器会重新创建：
```sql
DELETE FROM users WHERE username = 'admin';
```
然后重启 API 容器即可。
