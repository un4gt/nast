# 连接与模型

## 仅 OpenAI 兼容源

当前阶段只支持 `/chat/completions` 协议的自定义端点，一套配置覆盖所有兼容服务：
OpenRouter、DeepSeek、SiliconFlow、new-api 中转、本地 vLLM 等。

```
设置 → 连接
├─ 自定义端点（含 /v1）    https://openrouter.ai/api/v1
├─ API 密钥               sk-…（粘贴后点密钥框旁的「保存」）
├─ 模型 Combobox          「获取模型列表」拉取或手动填写
└─ 高级                   附加请求头（每行 Header: value）/ 附加请求体（JSON）
```

> **常见坑**：API 密钥有**独立的保存按钮**，只点设置总保存不会保存密钥；
> 「获取模型列表」优先使用输入框中的密钥（未保存则仅本次生效）。

## 密钥存储

密钥保存在服务端 `data/<user>/secrets.json`（ST 同构格式，多 key + 激活位），
`secrets.get` RPC 只返回掩码（`••••abcd`），原文永不下发到浏览器。

优先级：UI 保存的 secrets → 环境变量 `OPENAI_API_KEY` / `NAST_OPENAI_BASE` 降级。

## 采样参数

设置 → 采样参数：

- `max_tokens`：**务必调大**。服务端默认 300（ST Default.json 历史默认），
  中文约 200 字就截断；建议 1024–2048（按模型单次输出上限）
- `openai_max_context`：上下文窗口（影响世界书预算与历史填充）
- temperature / top_p / frequency_penalty / presence_penalty

底部聊天输入区有 token 计量条（当前会话总 token / 预算）。

## 连接问题速查

| 现象 | 原因 |
| --- | --- |
| `401 Missing Authentication header` | 密钥未保存（见上面的坑） |
| `401 Invalid API key` | 密钥错误/余额问题 |
| 生成到一半断句 | max_tokens 太小（无省略号截断） |
| 回复带 `…` 结尾 | 桥接侧 `BRIDGE_MAX_CHARS` 截断保护（QQ 场景） |
