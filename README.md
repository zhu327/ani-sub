# ani-sub

基于 Cloudflare Workers（Rust/WASM）的动漫订阅自动下载工具。

## 功能

- 定时从 Prowlarr 搜索订阅关键词，自动触发下载
- Web 管理页面：对 `anime` 订阅列表进行增删改查
- 通过 ntfy 发送下载通知
- 不再内置密码登录功能；如需登录验证，请对接 **Cloudflare Access**

## 架构

```
浏览器 → Cloudflare Access（可选）→ Cloudflare Worker (Rust/WASM)
           ├── GET  /                  → Web 管理页面
           ├── GET  /api/anime         → 订阅列表
           ├── POST /api/anime         → 新增订阅
           ├── PUT  /api/anime/:id     → 更新订阅
           └── DELETE /api/anime/:id   → 删除订阅
```

## 配置

在 `wrangler.toml` 的 `[vars]` 中配置：

| 变量 | 说明 |
|------|------|
| `PROWLARR_URL` | Prowlarr 服务地址 |
| `PROWLARR_API_KEY` | Prowlarr API Key |
| `PROWLARR_INDEXER` | 默认 indexer ID |
| `NTFY_TOPIC` | ntfy 通知主题，留空则禁用通知 |

同时需要配置 D1 数据库绑定 `DB`，以及 `schema.sql` 中的 `anime` 表。

## 登录验证（Cloudflare Access）

本项目已经移除了内置的密码登录、`/login`、`/logout` 接口以及 Cookie 认证相关代码。
Worker 本身不再做任何登录校验，所有请求默认直接放行到管理页面 / API。

如果需要登录验证，请将 Worker 部署的域名或路由放到 Cloudflare Access 后面：

1. 打开 Cloudflare Dashboard → **Zero Trust → Access → Applications**。
2. 点击 **Add an application**，选择 **Self-hosted**。
3. 填写 Worker 对应的域名/路由，例如 `anime.example.com/*`。
4. 配置 Access Policy（例如允许指定邮箱、指定域名成员或指定身份提供商）。
5. 保存后，未通过 Cloudflare Access 认证的请求会被 Cloudflare 直接拦截，只有认证通过的请求才会到达 Worker。

采用这种方式后，应用层无需再维护密码或 session，Worker 保持无状态、无鉴权，
登录验证完全由 Cloudflare Access 负责。

## 部署

```bash
wrangler deploy
```

## 本地开发

```bash
cargo check
cargo test
wrangler dev
```