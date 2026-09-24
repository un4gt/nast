# IM 桥接（nast-bridges）

独立仓库 `nast-bridges`（与 nast 同级目录），独立 workspace、独立镜像。
nast 对 IM **完全无感知**——桥接和浏览器用的是同一套公开 WS RPC。

## 结构

```
nast-bridges/
├── crates/
│   ├── bridge-core/        # 公共层：NastClient（断线重试）、会话、命令层、
│   │                       #   自定义命令、生成超时守卫、截断
│   ├── bridge-qq/          # QQ：扫码凭据 + 官方网关 + 被动回复
│   ├── bridge-discord/     # 占位（Token + Discord Gateway）
│   └── bridge-feishu/      # 占位（应用密钥 + 事件订阅）
├── Dockerfile              # 一个镜像，三个二进制
└── docker-compose.yml      # qq 默认；discord/feishu 走 profiles
```

适配器契约只有一条：收消息 → 清洗 → `handle_inbound(InboundMessage) → Option<String>` → 平台回复。

## 部署 QQ

前置：nast 已起（网络 `nast-net`）。

```bash
cd nast-bridges
cp .env.example .env
docker compose up -d --build
docker compose logs -f qq          # 首次：手机 QQ 扫日志里的二维码
```

- 凭据缓存在 volume `qq-creds`（`/data/qqbot-credentials.json`），**重启免扫码**
- QQ 群 @机器人 或私聊即走 nast 角色生成；命令（/char /newchat /world…）与网页通用
- 每个 QQ 来源（群/用户）独立聊天文件 `qq-g-<id>` / `qq-u-<id>`，网页可见可接管

## 配置

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `BRIDGE_NAST_SERVER` | ws://nast:8000/ws | nast WS 地址 |
| `BRIDGE_AVATAR` | 空=第一个角色 | 默认角色卡 |
| `BRIDGE_MAX_CHARS` | 1500 | 回复截断保护（带 `…`） |
| `BRIDGE_GEN_TIMEOUT_SECS` | 120 | 生成超时：超时即中止（generate.stop）并回复错误，防单次卡死锁住后续消息 |
| `BRIDGE_QQ_CRED_FILE` | /data/qqbot-credentials.json | 凭据缓存 |

## 启用多平台

```bash
docker compose --profile discord --profile feishu up -d
docker compose stop qq                       # 单独停某平台
```

## 新增平台适配器

1. `crates/bridge-<platform>/`：二进制 crate，依赖 `bridge-core` + 平台 SDK；
2. 实现三件事——凭据、平台网关长连接、收发消息（委托 core 处理）；
3. Dockerfile 的 build 追加 `-p bridge-<platform>` 并 COPY 二进制；
4. compose 加一个 service（`<<: *bridge` 锚点 + `command` + `profiles`）。

## Bridge API（对 nast 的全部依赖）

`characters.all` / `characters.chats` / `chats.new` / `chats.rename` /
`chats.set_world` / `generate.run` / `generate.stop` / `settings.get` / `plugins.list`

均为公开 RPC；nast 升级若改动这些方法需同步适配本仓库。
