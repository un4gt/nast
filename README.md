# nast-bridges

nast 的 IM 桥接插件栈：独立 workspace、独立镜像，nast 只通过公开 WS RPC 被访问。

```
crates/
├── bridge-core/        # 公共层：NastClient（断线重试）、会话（来源→角色+聊天）、
│                       #   命令层（/help /chars /char /newchat /worlds /world）、
│                       #   自定义命令展开、插件命令透传、截断
├── bridge-qq/          # QQ：qqbot-connector 扫码凭据 + 官方网关 + 被动回复
├── bridge-discord/     # 占位（Token + Discord Gateway，见 crate 注释）
└── bridge-feishu/      # 占位（应用密钥 + 事件订阅，见 crate 注释）
```

## 部署

前置：nast 侧 compose 网络已命名为 `nast-net`。

```bash
cp .env.example .env
docker compose up -d                                  # QQ（默认）
docker compose --profile discord --profile feishu up -d  # 追加平台
docker compose logs -f qq                             # 首次扫码绑定
```

## 新增平台适配器

1. `crates/bridge-<platform>/`：二进制 crate，依赖 `bridge-core` + 平台 SDK；
2. 实现三件事——凭据、平台网关长连接、（收消息→清洗→`BridgeContext::handle_inbound`→发回复）；
3. Dockerfile 的 cargo build 追加 `-p bridge-<platform>` 并 COPY 二进制；
4. compose 加一个 service（锚点 `<<: *bridge` + `command` + `profiles` + 凭据）。

## Bridge API（对 nast 的全部依赖）

characters.all / characters.chats / chats.new / chats.rename / chats.set_world /
generate.run / settings.get / plugins.list ——均为 nast 公开 WS RPC。
nast 升级若改动这些方法需同步适配本仓库。
