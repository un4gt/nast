# 架构总览

## 分层

```
浏览器 ──────────── Web UI（React + zustand + shadcn/ui）
        │  WS RPC（/ws，JSON 行协议：{id,method,params} / {event,data}）
        ▼
nast-server（单进程，actix-web）
 ├─ rpc.rs            方法分发（characters/chats/worlds/settings/plugins/generate/groups/presets/secrets…）
 ├─ generate.rs       生成状态机（normal/swipe/regenerate/continue/impersonate/quiet）
 ├─ connection.rs     模型连接（settings+secrets → provider；custom_url/custom_model）
 ├─ plugin 线程       Lua 插件专用线程 + 限时派发
 └─ nast-bridges(独立仓库) 经同一 WS RPC 接入（QQ/Discord/飞书）
```

## Cargo workspace

| crate | 职责 |
| --- | --- |
| `nast-server`（根包） | WS RPC、生成状态机、静态托管、HTTP（/upload /thumbnail） |
| `nast-model` | ST 同构数据类型：角色卡/聊天 jsonl/世界书/预设/正则/群组/settings |
| `nast-cards` | PNG tEXt chunk 读写（chara/ccv3 双写）、V1/V2/V3 规范化 |
| `nast-storage` | `data/<user>/` 布局、jsonl 聊天、integrity 校验、节流备份、原子写 |
| `nast-engine` | 宏引擎、ChatCompletion 拼装、世界书引擎、正则引擎、群聊调度、token 计数 |
| `nast-providers` | OpenAI 兼容流式接入，统一 StreamEvent（Token/Reasoning/Usage） |
| `nast-plugin` | mlua 插件宿主（专用线程、KV 落盘、斜杠命令、toast） |

IM 桥接已拆分至独立仓库 `nast-bridges`（bridge-core + 平台适配器），见[专门章节](./bridges.md)。

## 生成管线（单条消息的生命周期）

1. **入口** `generate.run`：同一时刻仅允许一个生成（ST `is_send_press` 语义）
2. **用户消息落盘**（先于生成；插件 `user_input` 钩子可改写；斜杠命令在此前拦截）
3. **persona 解析**：聊天绑定（`chat_metadata.persona`）> 默认 persona > username
4. **世界书扫描**：四源（聊天书 → persona 书 → 角色内嵌+charLore 辅助 → 全局激活书），
   逐 pass 累计预算、递归、timed effects、position 0–7 全消费
5. **AN 合成**：`chat_metadata.note_*` + interval 取模门控 + WI ANTop/ANBottom 并入 + persona TOP/BOTTOM_AN 并入
6. **提示拼装**：prompt_order（#100001）驱动、预算倒序填充、深度注入、squash
7. **provider 流式调用**：逐 token 广播（`stream_token_received` / `stream_reasoning_received`）
8. **cleanUpMessage**：停止串剥离、正则默认 pass、名字清理、endoftext 截断、fixMarkdown
9. **落盘**：swipe 基础设施、reasoning/ttft 进 extra、`message_saved` 事件

## 与 ST 对齐的工作方式

- 行为基准：仓库 `refrence/SillyTavern`（v1.18.0 检出）
- 差异登记：`GAPS.md`（✅ 对齐 / ⚠️ 偏差 / ❌ 缺失），修复即销号
- 每个语义点落地时在代码注释标注 ST 源码锚点（如 `world-info.js:4899`）
