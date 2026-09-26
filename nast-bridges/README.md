# nast-bridges

本目录是 NAST 仓库内的 IM 桥接项目，统一 Git 管理；保留独立 Cargo 工作区、依赖锁文件、Compose 配置和镜像。核心服务与桥接通过带令牌的 WebSocket RPC 通信。

- QQ 已实现；Discord、飞书目前仍是占位适配器。
- 公共命令、来源会话及生成取消由 `crates/bridge-core` 处理。
- QQ SDK 的源码与许可证位于 `vendor/qqbot-connector`，无需仓库外目录。
- 完整教程：[平台选择、认证、网络、持久化与排错](../docs/src/bridges.md)。

## 只部署 QQ

先从仓库根目录部署 NAST，配置登录账号与 `NAST_BRIDGE_TOKEN`，并在网页导入角色、配置模型。然后：

```bash
cd nast-bridges
cp .env.example .env
# 编辑 .env：BRIDGE_NAST_TOKEN 与服务端 NAST_BRIDGE_TOKEN 相同

docker compose --profile qq up -d --build
docker compose logs -f qq
```

使用 GHCR 镜像时，在本目录 `.env` 设置 `BRIDGE_IMAGE=ghcr.io/un4gt/nast-bridges:latest`，再运行 `docker compose --profile qq pull` 和 `docker compose --profile qq up -d --no-build`。

核心使用根目录 `.env`，桥接使用本目录 `.env`。两套 Compose 通过 `nast-net` 通信，保留独立容器与持久化卷。所有平台都需显式启用 profile；Discord／飞书暂不能用于实际聊天。

## 从仓库根目录构建与测试

```bash
cargo test --manifest-path nast-bridges/Cargo.toml --workspace --locked
docker build -t nast-bridges:local nast-bridges
```

`cargo test --workspace` 在根目录只测试核心服务，桥接通过上述 manifest 单独测试。QQ SDK 的真实端点测试默认忽略，不会触发扫码或创建绑定任务。

`.github/workflows/images.yml` 同时发布核心与桥接镜像；桥接镜像名为 `ghcr.io/<owner>/<repo>-bridges`，与核心共享 main、latest、版本及提交 SHA 标签规则。

## 公共 RPC

依赖 `characters.all`、`characters.chats`、`chats.new/rename/set_world`、`generate.run/status/stop`、`settings.get`、`plugins.list`、`model.command`、`model_catalog.get` 和 `conversation_model.get/set`。接口变更应与本目录的适配放在同一个提交中。

## QQ 长回复与排错

回复现在完整分段发送；超过单次被动回复配额时，用 `/more` 查看持久化的剩余内容。慢生成不再阻塞 QQ 网关心跳。日志默认记录 task_id、结束原因和每段发送结果，具体命令及字段见 [模型与 QQ 排错文档](../docs/src/guide/connection.md#qq-长回复与日志排错)。
