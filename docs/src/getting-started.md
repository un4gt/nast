# 快速开始

## 本机裸跑

```bash
# 1. 构建前端（产物 web/dist，服务端自动托管）
cd web && npm install && npm run build && cd ..

# 2. 启动（单端口：前端 + /ws 同在 8000）
cargo run
```

前端开发模式（HMR）：终端 1 `cargo run -p nast-server`，终端 2 `cd web && npm run dev` → http://localhost:3000（devserver 已代理 `/ws`、`/upload`、`/thumbnail`、`/api/tts`）。

## Docker 部署（核心服务）

```bash
cp .env.example .env        # 可选：改端口
docker compose up -d --build
```

- Web UI：http://127.0.0.1:8000
- 数据持久化在 named volume `nast-data`（settings/secrets/角色卡/聊天/世界书/预设/插件 KV）
- 插件在镜像内 `/app/plugins`；要本机管理可挂载 `./plugins:/app/plugins`
- 网络命名为 `nast-net`，供 [IM 桥接栈](./bridges.md) 接入

## 环境变量

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `NAST_PORT` | 8000 | 监听端口 |
| `NAST_BIND` | 127.0.0.1 | 绑定地址（容器内需 0.0.0.0，compose 已设置） |
| `NAST_DATA` | ./data | 数据目录（迁移 ST 数据时建议先复制并验证兼容范围） |
| `NAST_WEB` | ./web/dist | 前端静态资源 |
| `NAST_OPENAI_BASE` | https://api.openai.com/v1 | baseURL 降级（优先 UI 配置的 custom_url） |
| `NAST_PLUGIN_TIMEOUT_SECS` | 10 | Lua 插件单次派发超时 |

API 密钥不需要环境变量——在 **设置 → 连接** 面板保存（存服务端 `secrets.json`）。

## 首次配置清单

1. 导入角色卡：侧栏「导入角色卡」或拖入窗口，支持 PNG/JSON/YAML/YML/CHARX/BYAF
2. **设置 → 连接**：自定义端点（含 `/v1`）→ 粘贴 API 密钥 → **点密钥框旁的「保存」**（独立按钮，别只点设置保存）→ 获取模型列表 → 选模型 → 保存设置
3. **设置 → 采样参数**：max_tokens 按模型上限调大（默认 300 是 ST 历史默认，中文约 200 字就截断）
4. 开聊；QQ 机器人接入见 [IM 桥接](./bridges.md)
