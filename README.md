# nast-bridges

nast 的 IM 桥接插件栈：独立 workspace、独立镜像，nast 只通过公开 WS RPC 被访问。

```
crates/
├── bridge-core/        # 公共层：NastClient（只读查询可重试，已提交生成不重发）、会话（来源→角色+聊天）、
│                       #   命令层（/help /chars /char /newchat /worlds /world /model）、
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

## 会话、模型与超时

- `/model` 显示当前与可选模型；`/model <id>` 只切换此来源的会话；`/model info` 显示成功线路或下次候选。
- 来源 → 角色／聊天映射默认保存在 `data/bridge-sessions.json`。设置 `BRIDGE_SESSIONS_PATH` 可指定持久化文件；compose 使用 `/data/bridge-sessions.json` 并挂载平台独立卷。
- 重启恢复原聊天，升级时可从来源命名的现有聊天恢复。`/char` 复用目标角色的来源线程；只有 `/newchat` 明确另开。映射损坏或属于其他服务会报错，不静默新建。
- 总等待默认 240 秒，环境变量 `BRIDGE_GEN_TIMEOUT_SECS` 优先。扣除 5 秒收尾预算传给服务端，超时只取消相同 `task_id` 的任务。
- 已提交的生成、写入及命令不因断线自动重发；只读查询可重新连接重试。部分结果保留「未完成」提示，即使长回复被截断也保留提示。

新增公开 API 依赖：`model.command`、`model_catalog.get`、`conversation_model.get/set`、`generate.status/stop`。运行 `cargo test --workspace` 验证持久化会话和断线行为。
