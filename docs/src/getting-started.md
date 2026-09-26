# 快速开始

## 本机裸跑

```bash
# 1. 构建前端（产物 web/dist，服务端自动托管）
cd web && npm install && npm run build && cd ..

# 2. 启动（单端口：前端 + /ws 同在 8000）
export NAST_USERNAME=admin
export NAST_PASSWORD='替换为你自己的长密码'
cargo run
```

前端开发模式（HMR）：终端 1 `cargo run -p nast-server`，终端 2 `cd web && npm run dev` → http://localhost:3000（devserver 已代理 `/ws`、`/upload`、`/thumbnail`、`/api/tts`）。

## Docker 部署（核心服务）

```bash
cp .env.example .env        # 必须填写 NAST_USERNAME 和 NAST_PASSWORD
docker compose up -d --build
```

- Web UI：http://127.0.0.1:8000
- 数据持久化在 named volume `nast-data`（settings/secrets/角色卡/聊天/世界书/预设/插件 KV）
- 插件在镜像内 `/app/plugins`；要本机管理可挂载 `./plugins:/app/plugins`
- 网络命名为 `nast-net`，供 [IM 桥接栈](./bridges.md) 接入

访问网页后先登录。没有默认密码；升级前补齐账号变量，数据卷保留。会话、HTTPS 反向代理、桥接令牌与密码修改见[登录与部署认证](./guide/authentication.md)。

## 环境变量

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `NAST_PORT` | 8000 | 监听端口 |
| `NAST_USERNAME` / `NAST_PASSWORD` | 必填 | 部署登录账号 |
| `NAST_BRIDGE_TOKEN` | 空 | 桥接专用令牌，与桥接的 BRIDGE_NAST_TOKEN 一致 |
| `NAST_PUBLIC_ORIGIN` | 空 | 反向代理后的实际站点源 |
| `NAST_COOKIE_SECURE` | false | HTTPS 部署使用安全 Cookie |
| `NAST_BIND` | 127.0.0.1 | 绑定地址（容器内需 0.0.0.0，compose 已设置） |
| `NAST_DATA` | ./data | 数据目录（迁移 ST 数据时建议先复制并验证兼容范围） |
| `NAST_WEB` | ./web/dist | 前端静态资源 |
| `NAST_OPENAI_BASE` | https://api.openai.com/v1 | baseURL 降级（优先 UI 配置的 custom_url） |
| `NAST_PLUGIN_TIMEOUT_SECS` | 10 | Lua 插件单次派发超时 |

模型 API 密钥不需要环境变量——在 **设置 → 模型** 面板保存（存服务端 `secrets.json`），与部署登录密码、桥接令牌相互独立。

## 首次配置清单

1. 导入角色卡：侧栏「导入角色卡」或拖入窗口，支持 PNG/JSON/YAML/YML/CHARX/BYAF
2. **设置 → 模型**：点击「添加模型」，选择 API 类型，填写地址、密钥和模型 ID，点击「保存模型」
3. **模型参数**：按需设置最大输入、最大输出及思考级别；模型最大输出覆盖旧采样预设的默认 300
4. 开聊；QQ 机器人接入见 [IM 桥接](./bridges.md)
