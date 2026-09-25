# 登录与部署认证

NAST 使用一个部署级账号保护同一份数据，暂不提供多用户、注册或权限分组。网页显示登录页；服务端同时验证 HTTP 数据接口和 WebSocket，不能通过绕过页面直接读取聊天、上传文件或调用模型。

## Docker 配置

复制 `.env.example` 为 `.env`，填写：

```dotenv
NAST_USERNAME=admin
NAST_PASSWORD='替换为你自己的长密码'
NAST_BRIDGE_TOKEN=
NAST_PUBLIC_ORIGIN=
NAST_COOKIE_SECURE=false
```

没有默认密码。Compose 缺少用户名或密码时会直接提示配置错误；服务端只配置其中一项也会拒绝启动。`.env` 已被 Git 和 Docker 构建上下文忽略。包含 `$` 或 `#` 的密码在 Compose 的 `.env` 中使用单引号，避免插值或注释。

```bash
docker compose up -d --build
```

打开 `http://127.0.0.1:8000`，使用上面的账号登录。只想运行 GHCR 镜像时：

```bash
docker network create nast-net    # 已存在时跳过
docker volume create nast-data
docker run -d --name nast --restart unless-stopped \
  --network nast-net --env-file .env \
  -p 8000:8000 -v nast-data:/app/data \
  ghcr.io/un4gt/nast:latest
```

`docker run --env-file` 不按 Compose 规则移除引号或展开变量，使用这个命令时 `.env` 的值不要包裹引号。环境文件包含账号信息，不要提交或放进静态资源目录。

## HTTPS 与反向代理

公网部署通过 HTTPS 反向代理访问，并填写实际站点源：

```dotenv
NAST_PUBLIC_ORIGIN=https://chat.example.com
NAST_COOKIE_SECURE=true
```

源只含协议、域名和可选端口，不含路径。代理须转发 WebSocket 升级请求；所有 `/api/`、`/ws`、上传、头像和角色资源继续交给同一 NAST 服务。配置源后，其他站点发来的请求会被拒绝。尚未配置 HTTPS 的本机 HTTP 部署保持 `NAST_COOKIE_SECURE=false`，否则浏览器不会发送安全 Cookie。

网页使用 `HttpOnly`、`SameSite=Strict` Cookie，密码和会话令牌不写入浏览器 localStorage。会话有效期 12 小时；退出登录立即撤销当前会话，并关闭同一会话的 WebSocket（包括其他标签页）。服务器重启后需要重新登录。登录接口对实际连接来源限速：五分钟最多 20 次尝试；反向代理后的用户共享代理来源的额度。

## 桥接认证

生成一个独立随机令牌：

```bash
python -c "import secrets; print(secrets.token_hex(32))"
```

在 NAST 的 `.env` 中设置 `NAST_BRIDGE_TOKEN`，在 `nast-bridges` 的 `.env` 中设置相同值的 `BRIDGE_NAST_TOKEN`，随后重建相应容器。令牌至少 32 字符，仅包含字母、数字或 `-_.~`。无需将网页登录密码交给桥接。

桥接在 WebSocket 握手的 `Authorization: Bearer …` 请求头发送令牌，正常连接、重连和取消生成的控制连接均携带。令牌拥有这个单用户服务的 API 权限，不能放到 URL、客户端页面或聊天消息里。远程桥接使用 `wss://`；同一 Docker 网络使用 `ws://nast:8000/ws`。未设置 `NAST_BRIDGE_TOKEN` 时，服务端不接受桥接令牌认证。

只接 QQ、Discord 的当前状态及多平台配置见 [IM 桥接](../bridges.md)。

## 本机运行与测试

裸跑时直接设置进程环境变量；`cargo run` 和启动脚本不会自动读取 `.env`。

```powershell
$env:NAST_USERNAME = 'admin'
$env:NAST_PASSWORD = '替换为你自己的长密码'
cargo run
```

只在本机开发或隔离测试中需要免登录时，清空账号变量并显式设置 `NAST_ALLOW_ANONYMOUS=true`。此开关会开放整个 API；不要用于可从公网或不可信网络访问的部署。现有自动化测试在自己的子进程中设置该开关，不修改你的部署环境。

## 排错

| 现象 | 检查项 |
| --- | --- |
| 启动提示缺少账号 | 同时填写 `NAST_USERNAME`、`NAST_PASSWORD`，重新创建容器 |
| 登录后又回到登录页 | HTTP 上不要启用 Secure Cookie；检查代理域名及浏览器 Cookie 设置 |
| 403 不允许跨站访问 | `NAST_PUBLIC_ORIGIN` 与地址栏是否一致，是否带正确端口 |
| 登录返回 429 | 等待五分钟后重试，检查是否有错误密码的重复请求 |
| 桥接握手返回 401 | 两侧令牌是否一致、NAST 是否重建、桥接是否升级到支持认证的版本 |
| 忘记密码 | 修改部署环境变量并重建 NAST 容器；角色、聊天和模型数据保留 |
| 健康检查失败 | 使用无需登录的 `/healthz`，响应只包含 `ok` |

修改账号或令牌后执行 `docker compose up -d --force-recreate`；仅 `restart` 不会重新加载 Compose 环境变量。
