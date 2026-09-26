# IM 桥接（nast-bridges）

`nast-bridges/` 位于当前 NAST 仓库内，统一提交与发布；保留独立的 Cargo 工作区和镜像。每个平台运行一个容器，通过 NAST 的 `/ws` RPC 连接同一个服务；角色、模型、世界书和聊天仍由 NAST 保存。

## 先确认平台支持状态

| 平台 | 当前状态 | 配置入口 |
| --- | --- | --- |
| QQ | 已实现扫码、官方网关及收发消息 | `qq` profile |
| Discord | 只有占位二进制，尚未实现 Gateway 与消息收发 | `discord` profile 是预留配置，不能用于实际聊天 |
| 飞书 | 只有占位二进制，尚未实现事件订阅与消息收发 | `feishu` profile 是预留配置 |

因此当前能实际部署的是 QQ。设置 Discord Token 或启动 Discord 容器不会让占位实现变成可用适配器。下方同时给出独立平台与组合平台的 Compose 用法，供相应适配器实现后使用。

## 准备目录与 NAST

一次克隆即可得到核心服务、桥接和 QQ SDK：

```text
nast/
├── Cargo.toml
├── docker-compose.yml
└── nast-bridges/
    ├── Cargo.toml
    ├── docker-compose.yml
    └── vendor/qqbot-connector/
```

`ghcr.io/un4gt/nast` 运行核心服务，`ghcr.io/un4gt/nast-bridges` 运行桥接；两者由同一仓库的 `images.yml` 发布。QQ SDK 的源码、测试和许可证保存在 `nast-bridges/vendor/qqbot-connector/`，构建不再需要仓库外目录或额外 Docker 上下文。使用 Docker Compose v2。

1. 按 [登录与认证](./guide/authentication.md) 部署 NAST，填写账号密码和独立的 `NAST_BRIDGE_TOKEN`。
2. 在网页登录，导入至少一个角色，在「设置 → 模型」配置可用模型并测试聊天。
3. 确认 NAST 容器名为 `nast`，且加入 `nast-net` 网络。核心 Compose 会创建此网络；使用 `docker run` 时先创建网络并添加 `--network nast-net`。

## 只接 QQ

在仓库根目录进入 `nast-bridges/`，复制环境文件：

```bash
cd nast-bridges
cp .env.example .env
```

填写公共配置：

```dotenv
BRIDGE_NAST_SERVER=ws://nast:8000/ws
BRIDGE_NAST_TOKEN=填写与NAST_BRIDGE_TOKEN相同的值
BRIDGE_AVATAR=
BRIDGE_MAX_CHARS=1500
BRIDGE_GEN_TIMEOUT_SECS=240
```

`BRIDGE_AVATAR` 留空时使用第一个角色；也可以填写已导入的角色文件名（含 `.png`）。这里只填写 NAST 桥接令牌，QQ 平台凭据在下一步扫码取得。

```bash
# 只启动 QQ
docker compose --profile qq up -d --build
docker compose logs -f qq
```

也可使用 GHCR 预构建镜像：在桥接 `.env` 中设置 `BRIDGE_IMAGE=ghcr.io/un4gt/nast-bridges:latest`，然后执行：

```bash
docker compose --profile qq pull
docker compose --profile qq up -d --no-build
```

核心与桥接分别使用根目录和 `nast-bridges/` 的 `.env`，各自的用户名／令牌配置见上文。两份 Compose 保留独立服务和数据卷。从原同级目录迁移时，将原桥接 `.env` 一并移到本目录，并沿用原项目名与数据卷；默认项目名仍为 `nast-bridges`，已有扫码凭据可继续使用。如果以前使用 `-p` 或 `COMPOSE_PROJECT_NAME` 指定过项目名，迁移后继续使用同一名称。

使用手机 QQ 扫描日志显示的二维码，按平台要求完成绑定。QQ群中 @机器人，或向机器人发送私聊；发送 `/help` 查看命令。

QQ 凭据及来源会话映射放在独立卷 `qq-creds` 的 `/data` 中，重启后复用。不同群／用户使用独立线程；`/char` 切换角色并恢复原线程，`/newchat` 明确新开聊天，`/model` 选择当前线程模型。网页可以查看这些聊天。

## 只接 Discord，或同时接多个平台

**当前 Discord 尚未实现，以下是适配器完成后的启动方式，不是当前可用能力。** 届时需先按适配器要求配置 Discord 应用、Bot Token、Gateway intents 和邀请权限，并在 `.env` 填写 `DISCORD_TOKEN`。公共 NAST 连接与认证配置同 QQ。

```bash
# 仅 Discord：不会启动 QQ
docker compose --profile discord up -d --build

# QQ + Discord
docker compose --profile qq --profile discord up -d --build

# 也可按服务名显式选择，不启动其他服务
docker compose up -d --build qq discord
```

所有平台都使用显式 profile，不带 profile 或服务名的 `docker compose up` 不会默认启动 QQ。也可以在 `.env` 中写 `COMPOSE_PROFILES=qq` 或 `COMPOSE_PROFILES=qq,discord` 保存选择；不要启用尚未实现的适配器。

已有容器不会因为下一次只选择另一个 profile 而自动停止。切换平台时显式停止不再需要的服务：

```bash
docker compose stop qq
docker compose restart qq
docker compose logs -f qq
docker compose ps -a
```

多个平台共享 NAST 的模型目录，使用各自的会话和持久化卷；目前 NAST 同时只允许一个生成任务。另一个平台在忙时可能收到「generation already in progress」，不会自动排队或重复提交请求。

## 网络、配置与排错

| 变量／现象 | 说明 |
| --- | --- |
| `BRIDGE_NAST_SERVER` | 同 Docker 网络使用 `ws://nast:8000/ws`；远程部署使用实际域名的 `wss://…/ws` |
| `BRIDGE_NAST_TOKEN` | 与 NAST 的 `NAST_BRIDGE_TOKEN` 一致；在握手请求头传递，不放 URL |
| `BRIDGE_AVATAR` | 默认角色文件名；空值使用第一个角色 |
| `BRIDGE_MAX_CHARS` | 单条分段长度，默认 1500（128–1500）；完整回复保留，超出 QQ 回复配额用 `/more` 查看 |
| `BRIDGE_GEN_TIMEOUT_SECS` | 默认 240 秒；扣除收尾时间传给 NAST，超时只取消匹配任务 ID |
| `BRIDGE_SESSIONS_PATH` | 裸跑默认 `data/bridge-sessions.json`；Compose 固定 `/data/bridge-sessions.json` |
| `network nast-net declared as external, but could not be found` | 先启动 NAST Compose，或创建并把 NAST 加入同名网络 |
| 401／认证失败 | 检查双方令牌和版本；更改环境变量后重新创建两个容器 |
| 连接拒绝 | 确认 NAST 已启动；桥接容器中的 `127.0.0.1` 指向桥接自身 |
| 没有角色／模型不可用 | 在网页导入角色并配置可用模型，可用 `/chars`、`/char`、`/model info` 检查 |
| Discord／飞书容器反复退出 | 当前是占位实现，请停止该服务；配置 Token 无法解决 |
| 重启后要重新扫码 | 确认 `qq-creds` 卷仍在，不要用 `docker compose down -v` 删除凭据和会话 |

源码部署升级后使用 `docker compose --profile qq up -d --build`；镜像部署先 `pull`，再 `up -d --no-build`。修改 `.env` 后用 `up -d --force-recreate qq`，不要只执行 `restart`。已提交生成在断线后不会自动重发。映射文件损坏时会提示错误，不会悄悄新建线程覆盖已有会话。

## 新增平台适配器

适配器负责平台认证、网关长连接及消息收发：收到消息后调用 `BridgeContext::handle_inbound(InboundMessage)`，再把结果回复给平台。公共层统一负责 NAST 认证、来源会话、`/model` 等命令和生成取消。新增平台应使用唯一的来源键前缀及独立会话卷。

## QQ 长回复与排错

回复现在完整分段发送；超过单次被动回复配额时，用 `/more` 查看持久化的剩余内容。慢生成不再阻塞 QQ 网关心跳。日志默认记录 task_id、结束原因和每段发送结果，具体命令及字段见 [模型与 QQ 排错文档](guide/connection.md#qq-长回复与日志排错)。
