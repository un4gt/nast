# 服务器部署：GHCR 镜像、Nginx 与 HTTPS

本教程从一台 Ubuntu 24.04 x86_64 服务器开始，只下载部署文件并拉取 GHCR 镜像。服务器不需要 Git、Rust、Node.js 或源码构建。其他发行版可沿用 Compose 和 Nginx 配置，但安装软件的命令需使用相应发行版的版本。

完整流程：安装 Docker → 下载配置 → 设置账号并启动 → 配置域名和 Nginx → 申请 HTTPS 证书 → 可选接入 QQ。

## 1. 准备服务器与 Docker

- 当前镜像仅提供 `linux/amd64`；`uname -m` 应输出 `x86_64`。ARM 服务器暂不能直接使用原生 ARM 镜像。
- 准备域名，例如 `chat.example.com`，把 DNS **A 记录**指向服务器公网 IPv4。只有 IPv6 已正确配置时才添加 AAAA 记录。
- 云服务器安全组及系统防火墙放行 TCP **80、443**，保留你的 SSH 端口。默认配置不需要开放 8000。
- 下文使用有 sudo 权限的普通用户，部署目录为 `~/nast`。已有 Docker Engine 和 Compose v2 时跳过安装。

Ubuntu 24.04 上按 Docker 官方软件源安装：

```bash
sudo apt-get update
sudo apt-get install -y ca-certificates curl openssl
sudo install -m 0755 -d /etc/apt/keyrings
sudo curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc
sudo chmod a+r /etc/apt/keyrings/docker.asc

echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu noble stable" | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
sudo apt-get update
sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
sudo systemctl enable --now docker

sudo docker version
sudo docker compose version
```

`noble` 对应 Ubuntu 24.04。其他 Ubuntu 版本及旧 Docker 软件包冲突处理见 [Docker 官方安装文档](https://docs.docker.com/engine/install/ubuntu/)。不要在已有业务服务器上直接卸载其容器运行时。

如果 UFW 已启用，执行 `sudo ufw allow 80/tcp` 和 `sudo ufw allow 443/tcp`。云厂商安全组也要设置；不要在未允许 SSH 端口时启用或重置防火墙。

## 2. 下载部署文件并设置账号

以下下载命令用于**首次部署的新目录**；已有配置不要直接覆盖。

```bash
mkdir -p ~/nast
cd ~/nast
umask 077
curl -fL https://raw.githubusercontent.com/un4gt/nast/main/deploy/compose.yaml -o compose.yaml
curl -fL https://raw.githubusercontent.com/un4gt/nast/main/deploy/.env.example -o .env
curl -fL https://raw.githubusercontent.com/un4gt/nast/main/deploy/nginx.conf -o nginx.conf
chmod 600 .env
nano .env
```

至少修改用户名和密码：

```dotenv
NAST_USERNAME=admin
NAST_PASSWORD='替换为你自己的长密码'
```

密码包含 `$` 或 `#` 时用单引号包裹；示例使用 Compose 的 `.env` 语法。网页登录账号是部署级单账号，没有默认密码。暂时保留 `NAST_PUBLIC_ORIGIN=` 和 `NAST_COOKIE_SECURE=false`，证书配置完成后再启用 HTTPS 设置。

配置文件没有 `build` 字段。`NAST_IMAGE` 和 `BRIDGE_IMAGE` 默认分别使用 `ghcr.io/un4gt/nast:latest` 和 `ghcr.io/un4gt/nast-bridges:latest`；两个镜像均可公开拉取，无需 GitHub 登录。

## 3. 启动并检查核心服务

后续 Compose 命令均在 `~/nast` 执行：

```bash
sudo docker compose config --quiet
sudo docker compose pull
sudo docker compose up -d --no-build
sudo docker compose ps
sudo docker compose logs --tail=100 nast
curl -fsS http://127.0.0.1:8000/healthz
```

健康检查应返回 `ok`，容器稍后显示 `healthy`。第一次启动只运行网页核心，QQ 不会自动启动。配置固定项目名 `nast`，服务仅发布到宿主机 `127.0.0.1:8000`；公网通过后面的 Nginx 访问。

**暂时没有域名**时，在你自己的电脑上建立 SSH 隧道：

```bash
ssh -N -L 18000:127.0.0.1:8000 用户名@服务器IP
```

保持 SSH 连接，浏览器打开 `http://127.0.0.1:18000` 并登录。这种用法保持 HTTPS 两项为默认值，可跳过第 4、5 步；QQ 接入不要求网页有公网域名。

如果健康检查失败，先查看 `docker compose logs`。不要把 `NAST_ALLOW_ANONYMOUS=true` 当成认证故障的修复方法。

## 4. 配置宿主机 Nginx

本节使用**宿主机安装的 Nginx**，上游地址是 `127.0.0.1:8000`。如果已经有 Nginx，复用现有服务，不要再安装一个争用 80/443 的代理。

```bash
sudo apt-get install -y nginx certbot python3-certbot-nginx
sudo systemctl enable --now nginx
cd ~/nast
nano nginx.conf
```

将 `server_name chat.example.com;` 改为你的实际域名；若修改过 `.env` 的 `NAST_HTTP_PORT`，也要修改 `proxy_pass` 的端口。下载的完整配置如下：

```nginx
# 放入 Ubuntu 的 /etc/nginx/sites-available/nast，再链接到 sites-enabled。
# 替换 chat.example.com。先以 HTTP 启动，再由 certbot --nginx 添加 HTTPS。
# 本文件须由 nginx.conf 的 http {} 包含；不要放进另一个 server {}。
map $http_upgrade $nast_connection_upgrade {
    default upgrade;
    '' close;
}

server {
    listen 80;
    listen [::]:80;
    server_name chat.example.com;

    client_max_body_size 100m;

    location / {
        proxy_pass http://127.0.0.1:8000;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $nast_connection_upgrade;

        proxy_buffering off;
        proxy_read_timeout 660s;
        proxy_send_timeout 660s;
    }
}
```

`location /` 将整个站点交给 NAST，包含登录、上传、头像、TTS、API 和 `/ws`。HTTP/1.1 与 Upgrade 头负责 WebSocket；关闭响应缓冲并设置 660 秒空闲超时，以支持生成连接。示例上传上限为 100 MiB，可按需调整。

安装站点并检查语法：

```bash
sudo install -m 644 nginx.conf /etc/nginx/sites-available/nast
sudo ln -s /etc/nginx/sites-available/nast /etc/nginx/sites-enabled/nast
sudo nginx -t
sudo systemctl reload nginx
curl -fsS http://chat.example.com/healthz
```

将命令中的域名也替换为你的域名。只在 `nginx -t` 成功后 reload。链接已存在时跳过 `ln -s`；不需要删除其他站点或默认站点。此时只验证 `/healthz`，下一步配置好 HTTPS 再从公网登录。

`map` 必须位于 Nginx 的 `http` 上下文。Ubuntu 的 `sites-enabled` 默认满足这一点；使用面板时不要把整份配置粘贴进已有 `server` 块。

## 5. 申请证书、启用 HTTPS 与续期

确认域名解析正确、80/443 已放行后执行，替换域名和通知邮箱：

```bash
sudo certbot --nginx -d chat.example.com --redirect --agree-tos -m you@example.com
sudo nginx -t
sudo systemctl reload nginx
```

Certbot 会修改已安装的 Nginx 站点，添加证书配置并将 HTTP 跳转到 HTTPS。后续编辑 `/etc/nginx/sites-available/nast`；不要再用最初的 HTTP 模板覆盖它。

修改 `~/nast/.env`：

```dotenv
NAST_PUBLIC_ORIGIN=https://chat.example.com
NAST_COOKIE_SECURE=true
```

源只包含协议、域名和可选端口，不含 `/nast` 等路径。本教程使用独立域名的根路径部署。应用配置并验证：

```bash
cd ~/nast
sudo docker compose up -d --no-build
curl -fsS https://chat.example.com/healthz
sudo systemctl enable --now certbot.timer
sudo systemctl list-timers certbot.timer
sudo certbot renew --dry-run
```

浏览器访问 `https://chat.example.com`，使用部署账号登录，导入角色卡，在「设置 → 模型」填写 API 地址和密钥，添加模型或从列表批量同步，再发送一条消息。开发者工具的 Network → WS 中，`/ws` 应返回 **101 Switching Protocols**。重启服务后需要重新登录。

续期测试成功后保留 80/443 可达，Certbot 定时续期。不要额外再给 Nginx 添加一层 Basic Auth，否则 QQ 的 Bearer 认证和网页认证可能与其冲突。

### 使用 1Panel、宝塔或容器化 Nginx

在面板中创建域名的反向代理站点，申请并启用证书、HTTP 跳转 HTTPS，转发**整个站点**并启用 WebSocket。设置上游、上传上限、关闭代理缓冲和超时的语义与上面的配置一致，然后填写同样的 NAST HTTPS 环境变量。

面板的 Nginx 如果也运行在容器里，`127.0.0.1` 指向代理容器自身。应将它连接到本部署的 Docker 网络，使用 `http://nast:8000` 作为上游。例如本教程默认网络为 `nast_default`：

```bash
sudo docker network connect nast_default 你的Nginx容器名
```

这条命令对现有容器生效；还需在面板或代理自身的 Compose 配置中持久化该外部网络，避免重建后丢失。容器代理方式无需将 NAST 的 8000 端口开放到公网。

## 6. 可选：只接 QQ

先完成网页中的角色和模型配置，再生成独立令牌：

```bash
openssl rand -hex 32
```

把结果填入 `~/nast/.env`，同时启用 QQ profile：

```dotenv
COMPOSE_PROFILES=qq
NAST_BRIDGE_TOKEN=填入刚生成的64位十六进制字符串
```

这一套 Compose 自动把同一个令牌传给核心和桥接，**只需维护一份 `.env`**。不要使用网页登录密码或模型 API 密钥作为桥接令牌。

```bash
cd ~/nast
sudo docker compose pull
sudo docker compose up -d --no-build
sudo docker compose logs -f qq
```

在手机 QQ 中扫描日志二维码绑定。`Ctrl+C` 只退出日志查看，不停止容器。发送 `/help` 查看命令；角色默认使用第一个角色，也可在 `.env` 设置 `BRIDGE_AVATAR=角色文件名.png`。凭据与来源会话映射保存在 QQ 数据卷，重启后复用。

仅用网页时保持 `COMPOSE_PROFILES=`；如果 QQ 已运行，先执行 `sudo docker compose stop qq`，再清空 profile。改变 profile 本身不会停止已存在的容器。

**Discord、飞书目前仍是占位实现，不能用于实际聊天。** 当前部署文件只提供可用的 QQ profile。公共命令、会话行为和平台开发约定见 [IM 桥接](./bridges.md)。

## 7. 数据、更新、备份与恢复

默认核心数据卷为 `nast_nast-data`，包含模型密钥、角色、聊天、世界书及设置；QQ 数据卷为 `nast_qq-creds`，包含扫码凭据和会话映射。它们由 `.env` 的 `NAST_DATA_VOLUME` / `QQ_DATA_VOLUME` 指定。镜像内自带默认插件；自定义插件可额外挂载自己准备好的目录到 `/app/plugins`，该宿主机目录需单独备份。

### 更新镜像

先备份数据，并记录当前镜像摘要，方便退回同一版本。默认使用 latest 时可这样记录：

```bash
sudo docker image inspect ghcr.io/un4gt/nast:latest --format '{{index .RepoDigests 0}}'
# 只有已启用 QQ 才执行
sudo docker image inspect ghcr.io/un4gt/nast-bridges:latest --format '{{index .RepoDigests 0}}'
```

更新命令始终在部署目录运行，`.env` 中的 `COMPOSE_PROFILES` 会决定是否同时更新 QQ：

```bash
sudo docker compose pull
sudo docker compose up -d --no-build
sudo docker compose ps
```

固定版本时，将 `.env` 的 `NAST_IMAGE` / `BRIDGE_IMAGE` 改为 `ghcr.io/un4gt/nast@sha256:实际摘要` / `ghcr.io/un4gt/nast-bridges@sha256:实际摘要`，或 `sha-完整提交SHA` 标签，再执行上述命令。不要在更新镜像时重新下载 `.env` 覆盖账号或数据卷设置。修改密码或令牌后同样执行 `up`；单独 `restart` 不会加载新的环境变量。

### 停机备份

以下命令逐条执行并确认成功。停止写入后备份两份数据卷；`run` 只运行镜像内的 tar，不启动服务或 QQ 登录：

```bash
cd ~/nast
backup_dir="backups/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$backup_dir"
chmod 700 backups "$backup_dir"
cp .env compose.yaml "$backup_dir/"
sudo docker compose stop
sudo docker compose run --rm --no-deps -T --entrypoint tar nast -C /app/data -czf - . > "$backup_dir/nast-data.tar.gz"
# 只有已启用 QQ 才执行
sudo docker compose --profile qq run --rm --no-deps -T --entrypoint tar qq -C /data -czf - . > "$backup_dir/qq-data.tar.gz"
tar -tzf "$backup_dir/nast-data.tar.gz" > /dev/null
# 有 QQ 备份时验证它
tar -tzf "$backup_dir/qq-data.tar.gz" > /dev/null
sudo docker compose up -d --no-build
```

同时保存已安装的 `/etc/nginx/sites-available/nast` 和 `/etc/letsencrypt`（证书及私钥），或记录在新服务器重新申请证书的步骤。备份包含密码与 API 密钥，应放在私有备份位置并复制到另一台机器。

### 恢复或回滚

保留备份中的 `compose.yaml` 与 `.env`。在同一服务器恢复时先 `sudo docker compose stop`；在新服务器先安装 Docker 并准备部署目录。把 `.env` 的数据卷名称改成**新的空卷名**，例如 `nast_restore_data_20260926` 和 `nast_restore_qq_20260926`，恢复时不要覆盖正在使用的数据卷。

确认待恢复的卷名尚不存在（`sudo docker volume inspect 卷名` 应提示不存在），如需回滚，再把镜像变量改为备份时记录的摘要。然后执行，替换实际备份目录：

```bash
backup_dir=backups/20260926-120000
sudo docker compose pull
sudo docker compose run --rm --no-deps -T --entrypoint tar nast -C /app/data -xzf - < "$backup_dir/nast-data.tar.gz"
# 只有恢复 QQ 时执行
sudo docker compose --profile qq run --rm --no-deps -T --entrypoint tar qq -C /data -xzf - < "$backup_dir/qq-data.tar.gz"
sudo docker compose up -d --no-build
sudo docker compose ps
```

确认角色、聊天和 QQ 映射恢复后再处理旧卷。镜像回滚若涉及数据格式升级，应同时恢复对应版本的备份。不要用 `docker compose down -v` 更新服务，该命令会删除数据卷。

### 已有部署迁移到本配置

先备份，并检查旧容器实际挂载的卷，例如 `sudo docker inspect nast --format '{{json .Mounts}}'`（将 `nast` 替换为旧容器名）。原 `docker run` 可能使用 `nast-data`，原独立桥接 Compose 可能使用 `nast-bridges_qq-creds`；将**实际名称**写入新 `.env`，不要假设都是本教程的默认值。若原来使用宿主机目录挂载，应继续使用原绑定路径，或离线复制到新卷。

迁移前停止旧核心和桥接，保证同一数据只由一套服务写入；再用新配置启动。根目录及 `nast-bridges/` 中的源码 Compose 使用两份 `.env` 和 `nast-net` 网络，而本教程使用一份 `.env` 和 `nast_default` 网络，不要把两套教程混用。

## 8. 常见问题

| 现象 | 检查及处理 |
| --- | --- |
| 镜像提示 no matching manifest | `uname -m` 是否为 x86_64；当前未发布 ARM 镜像 |
| GHCR 拉取超时 | 检查服务器到 ghcr.io 的 DNS、出站 HTTPS 和网络连通性；无需在服务器改成源码编译 |
| 公网域名不通或证书申请失败 | 核对 A/AAAA、80/443、安全组、UFW，以及是否有其他进程占用端口 |
| Nginx 502 | 先从服务器访问 `http://127.0.0.1:8000/healthz`；检查核心日志、宿主机端口和容器代理的网络 |
| 登录后返回登录页 | 确认 HTTPS 访问、实际域名与 `NAST_PUBLIC_ORIGIN` 一致；HTTP 隧道测试须清空 HTTPS 源并关闭 Secure Cookie |
| 403 / 跨站访问被拒绝 | 源必须是地址栏中的协议、域名和端口；修改 `.env` 后执行 `up -d --no-build` |
| 登录 429 | 等待五分钟；当前反向代理后的登录尝试共享代理来源的限流额度 |
| 页面能打开但聊天断开 | 检查 `/ws` 是否返回 101、Upgrade 头与代理超时；不要只代理 `/api/` |
| 上传返回 413 | 调整 Nginx 的 `client_max_body_size`，验证语法并 reload |
| QQ 返回 401 | 检查令牌至少 32 字符，核心和桥接都已按最新 `.env` 重建 |
| 更新后看起来没有历史数据 | 检查实际数据卷名，停止服务后改回原卷；不要初始化或覆盖原数据 |

进一步说明见 [登录与部署认证](./guide/authentication.md)、[模型设置](./guide/connection.md) 和 [IM 桥接](./bridges.md)。
