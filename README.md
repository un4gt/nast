# nast — Not A SillyTavern

把 SillyTavern 浏览器端的核心生成逻辑全部移入 Rust 服务端的重新实现（非移植，按行为语义重写）。

## 架构

```
nast/
├── crates/
│   ├── nast-model/       # ST 1.18.0 同构数据类型（卡片/世界书/聊天/预设/正则/群组）
│   ├── nast-cards/       # PNG tEXt chunk 读写（chara/ccv3），V1/V2/V3 卡规范化
│   ├── nast-storage/     # data/<user>/ 布局、jsonl 聊天、integrity、节流备份
│   ├── nast-engine/      # 宏引擎、ChatCompletion 拼装、世界书引擎、正则引擎、群聊调度、token 计数
│   ├── nast-providers/   # OpenAI 兼容 / Anthropic / Gemini + 统一 StreamEvent
│   ├── nast-plugin/      # mlua 插件：事件钩子 + KV 存储
│   └── nast-server/      # actix-web + actix-ws 单端口：WS RPC、生成状态机、静态托管
└── web/                  # rsbuild + react + tailwind + zustand
```

## 运行

```bash
cargo run -p nast-server            # http://127.0.0.1:8000
cd web && npm run build             # 前端构建到 web/dist（服务端托管）
```

环境变量：

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `NAST_PORT` | 8000 | 监听端口 |
| `NAST_DATA` | ./data | 数据目录（可直接指向现有 ST data/） |
| `NAST_WEB` | ./web/dist | 前端静态资源 |
| `NAST_OPENAI_BASE` | https://api.openai.com/v1 | OpenAI 兼容 baseURL（本地 vLLM 等） |
| `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` / `GOOGLE_API_KEY` | — | 各源 API key |

## 行为兼容性（对照 SillyTavern release 1.18.0）

- **数据同构**：角色卡 PNG `chara`/`ccv3` chunk、`worlds/*.json` 的 `{entries:{uid:…}}`、
  `chats/<char>/*.jsonl` 首行 header + integrity、`groups/*.json` 均与 ST 文件直接互换。
- **宏引擎**：legacy `evaluateMacros` 三段有序替换；`{{pick}}` 经 getStringHash/alea-ARC4
  逐位复刻，与 ST 同 chat/同内容/同位置结果一致；变量即时副作用持久化。
- **提示拼装**：prompt_order 全局 100001、预算 = context − max_tokens、reserve 3、
  `[Start a new Chat]` 无条件插历史首、倒序填充首条塞不下即断、绝对深度注入
  （order 组降序处理升序呈现、角色 system→user→assistant）、squash 排除表。
- **世界书**：scan 状态机、/regex/ key 覆盖全局匹配、无 `*` 通配、次级逻辑四枚举、
  sticky/cooldown/delay（hash 失效、chat_metadata.timedWorldInfo）、组 override/加权、
  预算 sticky 优先 + ignoreBudget、position 0-7 + `@D` 语法、before 块 unshift 翻转。
- **正则**：placement 0-4、markdownOnly/promptOnly/双 pass、深度窗口、trimStrings 过滤
  捕获组值、`{{match}}`/`$N`、substituteRegex RAW/ESCAPED、全局→角色→聊天链式。
- **生成语义**：字符串类型 normal/swipe/impersonate/continue/regenerate/quiet；
  用户消息先落盘；swipe 追加新槽位 + 每 swipe 独立 send_date；regenerate 先删后生成；
  impersonate/quiet 不落消息；错误不写入消息文本。
- **群聊**：NATURAL（提及+talkativeness+禁连发+兜底）/LIST/MANUAL/POOLED；
  SWAP/APPEND/APPEND_DISABLED 卡片合并（join prefix/suffix + `<FIELDNAME>`）；
  成员 depth_prompt；随机开场白；`group chats/` 平铺 + gen_id 批次。

## 已知范围外（对照 ST）

文本补全路径（instruct/context 模板）、向量/RAG、图像生成、TTS、翻译、多用户账号、
世界书 outlet/EM 锚点注入（引擎已产出数据，拼装端未消费）。

## 测试

```bash
cargo test --workspace   # 17 套件 / 100+ 测试
node crates/nast-server/tests/smoke.js       # RPC 冒烟（需先启动服务 + npm i ws）
node crates/nast-server/tests/gen_smoke.js   # 生成链路冒烟（内置 mock provider）
node crates/nast-server/tests/group_smoke.js # 群聊冒烟
```
