# 语音朗读（TTS）

本次之前，nast 没有 TTS 服务、播放器或设置界面，README / GAPS 将其列为范围外。
现按本地 `refrence/SillyTavern` v1.18.0、提交 `8172dcd` 的 TTS 模块补齐接入与交互。
下文区分已实现行为、服务运行前提和仍存在的差异。

## 开始使用

1. 重新构建并启动后端，构建前端后刷新页面。
2. 打开 **设置 → 语音朗读**，启用朗读，选择服务商。
3. 云服务填写密钥并点击“保存密钥”；本地服务填写实际运行的地址。
4. 选择模型，刷新音色，设置默认音色。模型和音色 ID 也可以手动填写。
5. 点击“试听当前配置”，确认后“保存更改”。试听使用草稿，密钥单独保存。

无需模型部署时可以先选 **系统语音**，使用浏览器 / 操作系统提供的声音。
OpenAI 兼容服务需要填写完整的 `/v1/audio/speech` 地址；聊天模型配置与语音模型配置分别保存。

## 朗读与交互

- 每条消息有朗读按钮；聊天顶部可朗读整段会话、暂停、继续或停止。
- 自动朗读新回复，可选择包含用户消息；单聊与群聊都支持角色音色映射。
- 生成时按完整段落入队，最终消息扣除已经朗读的前缀；续写只朗读新增部分。
- 打开只有一条开场白的聊天时，可自动朗读开场白。
- 停止会清空队列并抑制本次生成余下的自动朗读；切换聊天、关闭 TTS、切换服务商会取消旧播放。
- `/speak voice="小雨" 你好` 使用“小雨”的映射音色；`/tts-stop` 停止朗读。
- 浏览器阻止自动播放时显示“继续播放”；请求失败保留配置并显示可重试的错误。

规则包括普通分段、只读引号、忽略星号动作、保留星号、跳过代码块 / 成对标签、正则移除、
朗读 `extra.display_text`、对话 / 动作 / 其他文本三种音色，以及 0.25–3 倍播放速度。
与参考一致，“只读引号”找不到完整引号时读原文；动作音色需要同时开启“将星号传给语音引擎”。
流式朗读不会把未闭合的代码、引用或推理标签提前送入语音服务。

## 服务商

共 28 个接入项：25 个 HTTP 服务、系统语音、两个浏览器模型。
服务商被列出表示适配器已实现，不表示免费、免密钥，或对应服务已经在本机启动。

| 服务商 | 模型 / 配置与运行前提 |
| --- | --- |
| System | 浏览器 SpeechSynthesis，音色取决于浏览器和操作系统 |
| OpenAI | `tts-1`、`tts-1-hd`、`gpt-4o-mini-tts`；mini 模型支持按角色朗读风格；API Key |
| OpenAI Compatible | 兼容语音端点、自定义模型 / 音色，密钥可选 |
| ElevenLabs | 动态模型 / 音色、历史复用、上传音频创建音色；API Key |
| Azure | 区域、音色列表、SSML 合成；Azure Speech Key |
| Google Gemini TTS | AI Studio 密钥、3 个参考模型、预置音色；参考版本的 Vertex AI 选项本身也不可用 |
| Google Translate | 在线朗读，251 项语言名称 / ID 与参考依赖兼容；部分翻译语言可能没有语音 |
| MiniMax | 4 个参考模型、自定义模型 / 音色、语速 / 音量 / 音高；API Key + Group ID |
| Volcengine | 火山引擎预置 / 自定义音色；App ID + Access Key + Resource ID |
| Novel | NovelAI 语音及自定义 voice seed；NovelAI Key |
| Electron Hub | 动态模型 / 音色和扩展生成参数；API Key |
| Chutes | Kokoro API 音色；API Key |
| Pollinations | 模型音色列表与音频生成；API Key |
| AllTalk | 单独运行 AllTalk v1 / v2；旁白与服务端 RVC、模型切换、DeepSpeed、低显存模式 |
| Chatterbox | 单独运行兼容 Chatterbox 服务；预置 / 参考音色及生成参数 |
| Coqui | 启用 Coqui 的 Extras 服务；67 个参考模型元数据及本地模型、安装检查 / 下载 / 修复、音色映射 |
| CosyVoice (Unofficial) | 对应 CosyVoice 服务；说话人列表和合成 |
| Edge | 已运行的 SillyTavern Extras 或 Edge 插件服务；nast 不内置这些独立服务 |
| GSVI | 对应 GSVI 服务；角色列表与生成参数 |
| GPT-SoVITS-Adapter | 对应适配服务；目标音色和角色信息 |
| GPT-SoVITS-V2 (Unofficial) | 对应 V2 服务；服务端参考音频目录和语言设置 |
| SBVits2 | Style-Bert-VITS2 服务；模型 / speaker / style 组合音色 |
| Silero | Silero / Extras 服务；speaker 列表 |
| TTS WebUI | OpenAI 格式语音端点；音色、模型和 Chatterbox 参数 |
| VITS | VITS / W2V2-VITS / BERT-VITS2 服务；模型与 speaker 组合音色 |
| XTTSv2 | XTTS 服务；语言、采样参数、文本分段和流式开关 |
| Kokoro | 浏览器按需下载 ONNX 模型并在 Worker 中合成；默认 q8 / WASM，可配置 WebGPU |
| SpeechT5 | 浏览器下载 SpeechT5；上传 512 维 Float32（2048 字节）的 `.bin` 音色文件 |

服务端适配器会转换 Gemini 的 PCM、MiniMax 的十六进制 / URL 音频、火山引擎的分块 Base64、
Pollinations 的 JSON 音频；流式 WAV 在完成后修正长度，便于浏览器正确播放结束。

“模型服务管理”里的切换 / 下载会立即作用于配置的服务；普通音色配置仍需“保存更改”。
服务商高级参数支持补充 JSON，例如自定义音色、模型和 OpenAI 兼容服务的 `extra_body`。
密钥必须通过密钥栏保存，不能放进普通配置。

## 与 SillyTavern 的数据兼容

沿用 `settings.json` 中的 `extension_settings.tts`，服务商配置放在同名字段下。
`voiceMap` 支持对象和旧的 `角色:音色,角色:音色` 字符串格式，保留其他扩展配置。

保留 `[Default Voice]`、`disabled`，以及多音色的 `角色 ("Quotes")`、
`角色 (*Text inside asterisks*)`、`角色 (Other text)` 映射名称。
密钥沿用服务端 `secrets.json` 的 ST 字段；前端只读取掩码，合成时由 Rust 注入请求。

## 实现差异与边界

- HTTP 音频按文本段缓冲后播放。服务商的 `streaming` 开关会影响真实请求，但当前没有复刻
  SillyTavern 的 AudioWorklet 逐 PCM 块即时播放，首段延迟可能更长。
- SpeechT5 在浏览器 Worker 运行；参考在 Node 服务端运行。Kokoro 同样按需加载。
  首次下载较大，需要访问 Hugging Face；后续是否复用缓存取决于浏览器。
- nast 没有 SillyTavern 浏览器扩展宿主，因此没有其全局 RVC 扩展注入和 `TTS_*` 扩展事件。
  AllTalk 自身的 RVC 请求参数和音色查询已实现。
- 运行界面沿用 nast 的设置与播放器设计；没有复制参考的全部服务商专用管理页面。
  已覆盖配置和主要服务操作，JSON 可保存额外参数，但只有对应适配器使用的参数才会参与请求。
- 停止后浏览器不会播放迟到的音频；已经到达远端的合成 / 下载是否停止，取决于服务商。
- 默认仍关闭 TTS。没有改动聊天模型的生成参数或将商业服务当成免密钥服务。

## 验证

验收使用隔离的临时数据目录，不修改实际角色、聊天、设置或密钥。

| 检查 | 证据范围 |
| --- | --- |
| Rust 全工作区测试 | 全部通过，包含 TTS 音频格式 / URL 校验 / WAV 流长度测试 |
| `npx tsc --noEmit`、`npm run build` | 前端类型和生产构建通过 |
| Docker 前端构建阶段 | Node 20、`npm ci` 和跨目录 TTS 资源导入通过 |
| `npm run test:tts` | 8 项文本、流式边界、多音色、Unicode 与旧配置测试通过 |
| `tests/tts_smoke.cjs` | 25 个 HTTP provider 的模拟服务契约、密钥传递、特殊音频、管理接口与错误处理 |
| `tests/tts_browser.cjs` | Edge 中真实 WAV 播放、暂停继续、切聊天取消、单聊 / 群聊分段去重、设置与主题 / 窄屏 |
| `tests/m3_group_smoke.js` | 12 项原有群聊行为通过；修复 Windows 临时目录清理时序 |
| `tests/tts_local_browser.cjs` | Edge 中真实下载并运行 Kokoro、SpeechT5，输出音频可解码；SpeechT5 使用测试嵌入向量 |
| 依赖审计 | `npm audit` 无已知漏洞 |

没有使用真实商业服务密钥逐家调用，也没有替用户启动第三方模型服务；模拟服务测试证明请求契约，
不能证明商业账户权限、服务额度或用户安装的模型服务版本。
本地模型验证覆盖 WASM；WebGPU 与所有语言 / 音色的质量没有逐项验收。
构建保留了 transformers / kokoro-js 的 `import.meta` 警告，其 Worker 已做实际运行验证。

复现方式：

```bash
cd web
npm ci
npx tsc --noEmit
npm run test:tts
npm run build
cd ..
cargo build -p nast-server
cargo test --workspace
node tests/tts_smoke.cjs
node tests/tts_browser.cjs
node tests/tts_local_browser.cjs
```

HTTP 测试需要 `ws`，浏览器测试还需要 `playwright` 和 Microsoft Edge。
可以在临时目录安装测试依赖，再用 `NAST_WS_MODULE` / `NAST_PLAYWRIGHT_MODULE` 指向模块目录，
不用把测试依赖加入应用。默认测试二进制为 `target/debug/nast(.exe)`，可用 `NAST_TEST_BIN` 覆盖。
真实模型测试默认每个模型最多等待 180 秒，可用 `NAST_TTS_MODEL_TIMEOUT_MS` 调整。
