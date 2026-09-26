# 模型设置

打开「设置 → 模型 → 添加模型」，填写 API 类型、API 地址、API 密钥和模型 ID，点击一次「保存模型」即可。显示名称可留空，默认使用模型 ID；新聊天使用默认模型，已有聊天从对话顶部切换。

## 添加 API

| API 类型 | 基础地址示例 | 模型选择 |
| --- | --- | --- |
| OpenAI 兼容 | `https://api.openai.com/v1` | 填好地址和密钥后点「获取模型列表」，搜索选择；也可直接输入 |
| Anthropic | `https://api.anthropic.com/v1` | 手动输入服务商提供的模型 ID |
| Gemini | `https://generativelanguage.googleapis.com/v1beta` | 手动输入服务商提供的模型 ID |

OpenRouter、DeepSeek、vLLM、Ollama 等提供 Chat Completions 兼容接口的服务，选择「OpenAI 兼容」并填写其 Base URL。三种类型都支持自定义地址。这里的 OpenAI 兼容指 `/chat/completions`，不包含 Responses API。

获取列表不会先保存模型，也不会使用旧的全局密钥。获取失败时仍可手动填写模型 ID。修改已保存的地址或 API 类型时，需要重新输入密钥或明确清除旧密钥，避免把旧密钥发给另一个服务。读取配置只显示密钥是否已设置。

模型列表提供编辑、删除和设为默认。保存失败保留输入，版本冲突可点「加载最新配置并保留输入」后确认保存。内部 ID 保持不变，旧的多个连接配置保留；常规添加流程只需要配置一个 API。

## 最大输入、最大输出与思考

展开「模型参数」设置，参数随模型保存，不随提示词预设切换。

| 参数 | 实际行为 |
| --- | --- |
| 最大输入 Token | 限制系统提示、世界书、聊天历史等输入总预算；首选连接拼装时使用此预算，备用连接只校验、不重新裁剪 |
| 最大输出 Token | 覆盖采样预设的回复上限；留空才继承预设。历史默认 300 可能使长回复很早停止 |
| 上下文窗口 | 模型的输入与输出总量；实际输入预算取「最大输入」与「上下文窗口 − 最大输出」中较小值 |
| OpenAI 思考强度 | 发送 `reasoning_effort`；支持范围取决于具体模型。推理模型通常选择 `max_completion_tokens`，通用兼容服务可用 `max_tokens` |
| Anthropic 思考 | 自适应模式发送 `thinking.type=adaptive` 和可选 `output_config.effort`；预算模式发送 `thinking.type=enabled` 和 `budget_tokens` |
| Gemini 思考 | 在 `generationConfig.thinkingConfig` 中发送 `thinkingLevel` 或 `thinkingBudget`，两者互斥；预算 -1 表示自动，0 表示关闭，具体能力由模型决定 |

输出预算通常包含思考 Token 和正文 Token，不是可见字数。Anthropic 手动思考预算至少 1024，且必须小于最大输出。选择 OpenAI 或 Anthropic 思考模式时，会启用「使用服务商默认采样参数」，停止发送 Temperature、Top P 和频率／存在惩罚；可以在表单中调整。

示例：最大输入 16000、最大输出 4096、上下文窗口 20000，输入实际最多 15904。不要把服务商公布的上下文窗口直接当成最大输入再额外预留输出。非 OpenAI 模型的 Token 数为近似计数，上游仍可能因更精确的计数拒绝。

长会话会先为固定提示词、最新消息和输出留出空间，再从最旧历史开始裁剪。裁剪与发送前校验采用相同的 tokenizer 映射及消息开销，避免中文或自定义模型名出现两套计数。若最新消息本身已经超过可用空间，会明确报错，不会丢掉问题后继续生成。插件修改后的最终请求及备用连接仍需通过限制校验。

看到旧版 `frozen prompt exceeds route context limit`，请先更新核心镜像。新版超限信息包含模型 ID、估算输入、预留输出和配置上限；例如输入 12000 + 输出 4096 超过上下文 16000。按服务商公布的能力核对「模型参数」，可适当降低输出预留或减少固定提示词；上限不能随意填大。备用模型空间不足时不会重新执行宏、插件或裁剪请求。

达到输出限制会保留正文和思考并标记「未完成」，记录 `length`／`max_tokens`／`MAX_TOKENS`。可以调高最大输出后续写。默认值不能保证适用于所有模型，思考能力不支持时保留默认或按服务商文档设置。

高级请求头、超时、参数覆盖和移除仍可通过 `model_catalog.save` 设置。不能改写模型、消息、系统提示或流式控制字段，凭据使用独立密钥字段。提示词和采样预设中的旧连接字段不再决定运行时连接。

## 会话选择与黏性

新聊天绑定当时的默认模型；旧聊天首次打开模型选择器或生成时补齐选择。之后修改默认模型不影响这些聊天。切换到其他模型清除线路黏性，重复选择同一模型保留黏性。生成期间不能切换。

首次调用按优先级选路，完整成功后记录该会话的成功线路。以后优先使用仍存在且启用的成功线路。私聊彼此独立；群聊同一线程所有角色共享选择与成功线路。重命名保留状态，新聊天重新绑定默认模型并清除黏性。导入保留模型选择但清除线路；模型已删除或没有启用线路时会明确提示修复，服务端不会静默切换逻辑模型。

- `/model`：网页打开模型选择器，桥接端显示当前与可选模型。
- `/model <id>`：只切换当前线程的逻辑模型。
- `/model info`：显示逻辑模型、已成功使用的线路和上游模型；尚未成功时显示下次候选线路。

这些内置命令在生成插件之前处理，不进入历史或提示词。没有 `/provider` 命令。

## 故障与未完成结果

超时、连接中断、429、502、503、504，以及协议明确的临时错误在内容输出前重试一次，再切换下一条线路。默认等待一秒并加少量抖动；尊重最多 30 秒的 `Retry-After`，更长等待直接尝试备用线路。400、401、403、参数／上下文／能力／配置错误及未知错误直接返回。分类使用状态码和协议错误类型，不匹配报错文案。

默认连接超时 10 秒、首段正文或思考等待 60 秒、流式内容空闲超时 60 秒，服务端总预算默认 600 秒。停止操作会取消请求和退避等待。首段之前的普通断流可重试；已有正文或思考时断流保留部分结果、标记「未完成」，不会切换线路，也不会提交新的成功黏性。SSE 必须收到对应协议的结束标记。群聊成员生成未完成后停止本轮其余成员。

提示词、宏、插件和世界书状态只构建一次。备用线路不能容纳冻结请求或无法表达其能力时会明确拒绝，不重新裁剪。上下文检查与提示词裁剪统一使用同一套 token 估算；非 OpenAI 模型使用近似计数，仍可能被上游更精确的计数拒绝。原生协议不支持的名字字段、惩罚参数或 Gemini prefill 不会被静默丢弃。

消息及 swipe 的 `extra.model` 保存实际上游模型；`extra.nast_model` 保存逻辑模型、连接、任务 ID、`finish_reason`、Token 用量（上游提供时）、`complete`／`incomplete` 和结构化诊断。历史消息的「模型调用信息」可查看这些信息。

## 升级、备份与回滚

旧 settings 或 secrets 文件损坏时拒绝启动，修复原文件后重试，避免用空配置覆盖。首次启动且没有 `data/default-user/models.json` 时，从旧 settings、secrets 和环境变量迁移一个默认模型及线路。先保存 `backups/pre-models-settings.json` 与 `backups/pre-models-secrets.json`，再创建线路专属凭据与目录；重复启动不重复迁移，不覆盖原始备份。旧字段保留，新增线路不会使用旧全局密钥。TTS 继续读取原配置。

目录 `version` 是保存冲突检测版本。密钥写入 secrets.json 的新不可变引用后，原子发布 models.json 才会使配置生效。目录发布失败不会更换内存中的有效配置，也不会改变旧引用；可能留下未引用的秘密版本，勿将 secrets.json 加入版本控制。

升级前复制数据到隔离目录或独立容器验证。回滚到旧二进制时可继续使用保留的旧字段；如需恢复升级前状态，从上述备份恢复 settings 和 secrets。不要把生产卷挂到迁移测试容器。

## 公开 RPC

| 方法 | 参数／结果 |
| --- | --- |
| `model_catalog.get` | 返回 `{version, default_model, models}`，线路附 `credential_configured`；不返回明文密钥 |
| `model_catalog.save` | `{catalog, credentials?: {route_id: "新密钥"}}`；空字符串清除，省略保留；返回新 version |
| `conversation_model.get` | `{conversation}`，返回 state、model、candidate_route、可操作的 error |
| `conversation_model.set` | `{conversation, model_id}` |
| `model.command` | `{conversation, argument: "" 或 "info" 或模型 ID}`；返回 text 和 view |
| `models.list` | `{connection:{endpoint,key?,route_id?}}` 测试未保存的 OpenAI 兼容配置；也支持 `{route_id}` 使用已保存连接。省略 key 时仅在端点和协议未变化时使用指定连接的密钥 |
| `generate.run` / `generate.group` | 兼容原参数；可加 `task_id`、`time_budget_secs`（1–600 秒） |
| `generate.status` | 原字段加 `info.task_id`、`route_state`、`reasoning` 和 `cancelling`，显示路由阶段与恢复进度 |
| `generate.stop` | 可加 `{task_id}`；不匹配时返回 `{ok:false,reason:"task_mismatch"}` |

私聊引用为 `{kind:"private",avatar:"角色.png",chat_file:"聊天.jsonl"}`；群聊引用为 `{kind:"group",group_id:"…",chat_id:"…"}`，服务端校验群聊归属。私聊通过 `chats.save` 导入新文件自动清除原线路，覆盖导入已有文件需传 `imported:true`。

新增事件 `model_catalog_changed`、`conversation_model_changed`、`generation_route`、`generation_diagnostic`。会话事件携带明确引用，生成事件携带任务 ID。错误响应保留 code/message 并增加 diagnostic；上游错误使用 `upstream`，目录冲突使用 `conflict`。诊断和广播不含密钥。

桥接端默认等待 240 秒，扣除 5 秒收尾余量作为服务端预算；显式 `BRIDGE_GEN_TIMEOUT_SECS` 优先。已提交请求断线后不会整次重发，超时取消必须匹配任务 ID。来源与聊天映射保存在 `BRIDGE_SESSIONS_PATH`，重启恢复原线程；只有 `/newchat` 主动另开线程。

## QQ 长回复与日志排错

QQ 桥接等待 NAST 完成生成后分段发送文字。`BRIDGE_MAX_CHARS` 现在控制单条消息长度，默认 1500，实际限制在 128–1500；不再丢弃总回复超出的尾部。按 QQ 官方被动回复配额，一次单聊最多发送 4 条，群聊最多 5 条。超过时提示发送 `/more` 继续查看，命令不调用模型。

未发送内容写入 `qq-outbox.json`，只有 QQ 返回成功后才移除对应片段。默认与 `BRIDGE_SESSIONS_PATH` 或 `BRIDGE_QQ_CRED_FILE` 同目录，也可用 `BRIDGE_QQ_OUTBOX_PATH` 指定；请把目录放在持久化卷。连接异常时不自动重发可能已送达的片段，手动 `/more` 可能再次收到这一个片段，剩余内容不会因此丢掉。

在运行这两个服务的 Compose 目录查看：

```bash
docker compose logs --since 20m --timestamps nast qq
docker compose logs -f --tail 100 nast qq
```

如果 QQ 使用独立 Compose，则分别在核心目录查看 `nast`，在桥接目录查看 `qq`。默认 `info` 级别已包含下面的日志，不需要打开全量 debug。

| 日志事件 | 用途 |
| --- | --- |
| `bridge_generation_started` | QQ 请求对应的 `task_id`，同一行的 `qq_message` 包含平台消息 ID |
| `generation_requested` / `generation_attempt` | 服务端收到任务、实际模型、输出预算、尝试次数及是否切换备用连接 |
| `generation_prompt_rejected` | 提示词构建时预算不足，包含任务 ID、用量和上限 |
| `generation_attempt_failed` / `generation_backoff` | 错误类型、HTTP 状态、超时阶段、预算用量／上限及重试等待；不打印原始上游正文 |
| `generation_finished` | 完成状态、`finish_reason`、耗时、正文／思考字数和可用 Token 用量 |
| `bridge_rpc_failed` | 桥接收到的错误代码、类型、HTTP 状态及超限数值；凭任务 ID 对照核心日志 |
| `bridge_generation_result` | 桥接实际拿到的完成状态和字数，可与核心日志对照 |
| `qq_reply_ready` / `qq_reply_segment_sent` | 总回复字数和每一段的实际发送字数、序号 |
| `qq_reply_segment_failed` | QQ HTTP 状态／错误码或网络错误；剩余片段保留，可用 `/more` 取回 |
| `qq_reply_finished` | 剩余待发送段数，0 表示该来源队列已发送完 |
| `rpc_failed` / `rpc_delivery_failed` | 请求处理失败，或结果无法写回客户端连接 |

`finish_reason=length/max_tokens/MAX_TOKENS` 表示模型输出额度用尽；`error_kind=disconnected` 表示缺少完整流结束标记；`timeout_stage=first content/stream content/total budget` 区分首段等待、流式空闲和总时限。核心已完整生成、QQ 仍缺尾部时，重点看分段发送日志和剩余段数。

网关收到 `server reconnect requested` 会重连，这一行本身不能证明生成截断。生成工作现在在独立异步任务中执行，不再阻塞网关心跳。日志记录关联 ID、状态及长度，不记录用户消息正文、模型回复正文或 API 密钥。
