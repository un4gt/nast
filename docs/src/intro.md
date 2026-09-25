# 简介

**nast**（Not A SillyTavern）把 SillyTavern 浏览器端的核心生成逻辑全部移入 Rust 服务端的重新实现——非移植，按行为语义重写。目标是：单端口、单二进制、开箱即用，聊天链路与 SillyTavern v1.18.0 行为一致。

## 它是什么

- **角色扮演聊天平台**：角色卡（PNG/JSON/YAML/YML/CHARX/BYAF 导入）、多聊天、swipe、世界书、预设、正则脚本、群聊
- **服务端生成**：宏引擎、世界书扫描、提示拼装、token 计数全部在服务端完成；前端只是瘦客户端
- **数据互通**：兼容 ST 的 `data/` 目录布局及已实现字段；迁移前先复制数据，兼容范围和计时状态差异见[数据布局](./data.md)
- **可扩展**：服务端 Lua 插件（生成管线钩子 + 斜杠命令）+ 独立 IM 桥接栈（QQ/Discord/飞书）

## 它不是什么

- 不是 SillyTavern 的 fork：没有复用 ST 前端，提示拼装语义以 ST v1.18.0 为基准逐条对齐（对照差异见仓库 `docs/ST_PARITY_AUDIT.md`）
- 当前阶段模型接入仅 OpenAI 兼容 `/chat/completions`（自定义 baseURL，覆盖 OpenRouter / DeepSeek / 中转 / 本地 vLLM）

## 一分钟预览

```bash
cd web && npm install && npm run build && cd ..
cargo run            # http://127.0.0.1:8000
```

导入角色卡 → 设置 → 连接（填 baseURL / API 密钥 / 模型）→ 开聊。
Docker 部署、QQ 机器人接入见[快速开始](./getting-started.md)。
