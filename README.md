# nast — Not A SillyTavern

把 SillyTavern 浏览器端的核心生成逻辑全部移入 Rust 服务端的重新实现（非移植，按行为语义重写）。

## 架构

```
nast/
├── src/                  # 服务端根包（nast-server）：WS RPC、生成状态机、静态托管
│   ├── main.rs           # 入口（cargo run 即启动）
│   ├── generate.rs       # 生成状态机
│   ├── rpc.rs / ws.rs    # WS RPC 分发与会话
│   └── ...
├── crates/
│   ├── nast-model/       # ST 1.18.0 同构数据类型（卡片/世界书/聊天/预设/正则/群组）
│   ├── nast-cards/       # PNG tEXt chunk 读写（chara/ccv3），V1/V2/V3 卡规范化
│   ├── nast-storage/     # data/<user>/ 布局、jsonl 聊天、integrity、节流备份
│   ├── nast-engine/      # 宏引擎、ChatCompletion 拼装、世界书引擎、正则引擎、群聊调度、token 计数
│   ├── nast-providers/   # OpenAI 兼容 / Anthropic / Gemini + 统一 StreamEvent
│   └── nast-plugin/      # mlua 插件：事件钩子 + KV 存储
├── tests/                # 端到端冒烟脚本（Node）
└── web/                  # rsbuild + react + tailwind + zustand
```

## 运行

入口在 `crates/nast-server`（根目录无包，这是 Cargo workspace）。

```bash
# 1. 首次：构建前端（产物 web/dist，服务端自动托管）
cd web && npm install && npm run build && cd ..

# 2. 启动（单端口：http://127.0.0.1:8000 同时服务前端与 /ws）
cargo run            # 服务端在根包 nast-server，无需 -p

# 或 release 模式
cargo build --release && ./target/release/nast
```

也可以直接用一键脚本（自动补建前端 + 启动）：

```bash
./run.sh        # Git Bash / Linux / macOS
./run.ps1       # PowerShell
```

### 前端开发模式（HMR）

两个终端：API 服务（8000）+ rsbuild devserver（3000，热更新）。devserver 把
`/ws`（WebSocket）与 `/upload` 代理到 8000，前端代码改动即时生效，无需构建：

```bash
# 终端 1
cargo run -p nast-server
# 终端 2
cd web && npm run dev   # → http://localhost:3000
```

代理目标在 `web/rsbuild.config.ts` 的 `server.proxy`，后端端口改动时同步修改。

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
node tests/smoke.js       # RPC 冒烟（需先启动服务 + npm i ws）
node tests/gen_smoke.js   # 生成链路冒烟（内置 mock provider）
node tests/group_smoke.js # 群聊冒烟
```
