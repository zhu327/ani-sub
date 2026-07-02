# 动漫订阅 CRUD + 认证功能设计

## 背景

ani-sub 的 Cloudflare Workers 版本（`workers` 分支）当前仅有 `#[event(scheduled)]` 定时触发。由于 Cloudflare 下线了 D1 的数据管理界面，需要添加 Web 管理能力。

## 需求

1. 为 `anime` 表提供 CRUD 操作（增删改查订阅列表）
2. 单密码认证（环境变量存储）
3. 仅提供 HTML 管理页面，无需独立 API 文档
4. 不影响现有的定时触发逻辑

## 架构

```
浏览器 → Cloudflare Worker (Rust/WASM)
           ├── #[event(fetch)]     ← 新增: HTTP 入口
           │   ├── POST /login     → 验证密码，设置 HttpOnly Cookie
           │   ├── POST /logout    → 清除 Cookie
           │   ├── GET  /          → HTML 管理页面（需认证）或登录页
           │   └── /api/anime      → CRUD 操作（需认证）
           └── #[event(scheduled)] ← 已有: 定时触发不变
```

## API 设计

| 方法 | 路径 | 说明 | 请求体 | 响应 |
|------|------|------|--------|------|
| GET | / | 管理页面/登录页 | - | HTML |
| POST | /login | 登录 | `{"password":"..."}` | `{"ok":true}` + Cookie |
| POST | /logout | 登出 | - | `{"ok":true}` + 清除Cookie |
| GET | /api/anime | 列表 | - | `{"ok":true,"data":[...]}` |
| POST | /api/anime | 创建 | `{"keywords":"...","exclude_keywords":"...","indexer":5}` | `{"ok":true,"data":{...}}` |
| PUT | /api/anime/:id | 更新 | 同上 | `{"ok":true,"data":{...}}` |
| DELETE | /api/anime/:id | 删除 | - | `{"ok":true}` |

## 认证

- 密码通过 `AUTH_PASSWORD` 环境变量配置（建议使用 wrangler secret）
- 登录成功后设置 `HttpOnly; Secure; SameSite=Strict; Path=/` 的 `session` Cookie
- Cookie 值为密码明文（单用户场景可接受）
- 未认证访问 `/` 返回登录页 HTML
- 未认证访问 `/api/*` 返回 401

## D1 表结构

不变。使用现有 `anime` 表：

```sql
CREATE TABLE IF NOT EXISTS anime (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    keywords VARCHAR(128) NOT NULL,
    exclude_keywords VARCHAR(128) NOT NULL DEFAULT '',
    indexer INTEGER
);
```

## HTML 页面

单个 HTML 文件内嵌为 Rust 常量字符串，包含：
- 登录表单（密码输入 + 登录按钮）
- 管理界面（认证后显示）：订阅列表表格 + 新增/编辑/删除按钮
- 极简 CSS，移动端友好
- 纯 vanilla JS，无第三方依赖

## 新增环境变量

```toml
[vars]
AUTH_PASSWORD = "your_secret_password"
```

## 错误处理

- 统一 JSON 响应格式：`{"ok":true/false, "data":..., "error":"..."}`
- D1 查询失败 → 500
- 无效请求体 → 400
- 记录未找到 → 404
- 未认证 HTML 请求 → 返回登录页
- 未认证 API 请求 → 401

## 不在范围内

- 密码哈希
- 多用户支持
- 分页
- CSRF token（SameSite Cookie 已足够）
- 前端框架
- 独立 API 文档

## 验证方式

- `cargo check` 编译检查
- `cargo build` 完整编译
- `wrangler dev` 本地模拟（用户自行测试）
