# 模型与会话路由

聊天顶部选择逻辑模型；在「设置 → 模型」管理模型和线路。模型名称可以修改，模型 ID 创建后保持不变。每条线路包含说明性名称、协议、上游模型 ID、端点、独立 API 密钥、启用状态和优先级。优先级越大越优先，同优先级遵循配置顺序。

支持 OpenAI 兼容、Anthropic 和 Gemini；三者均可使用自定义端点。端点填写到 API 版本层，例如 `https://api.openai.com/v1`、`https://api.anthropic.com/v1`、`https://generativelanguage.googleapis.com/v1beta`。OpenAI 兼容线路可在保存后获取上游模型列表；原生协议手动填写模型 ID。线路名称（如 OpenRouter）只是标签，不决定协议。

高级配置可设置上下文／输出限制、连接／首段内容／空闲超时、附加请求头、请求参数覆盖和移除。覆盖与移除使用 JSON，对模型、消息、系统消息和流式控制字段的改写会被拒绝。密钥应填入独立密钥字段，不允许通过鉴权头覆盖。Gemini 参数放入 `generationConfig` 对象。

模型目录单独保存；修改提示词、采样参数或应用旧预设不会改变会话模型或运行时连接。保存失败保留草稿；出现版本冲突时可复制草稿后重新加载最新目录。

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

提示词、宏、插件和世界书状态只构建一次。备用线路不能容纳冻结请求或无法表达其能力时会明确拒绝，不重新裁剪。上下文检查沿用现有 token 计数；非 OpenAI 模型使用近似计数，仍可能被上游更精确的计数拒绝。原生协议不支持的名字字段、惩罚参数或 Gemini prefill 不会被静默丢弃。

消息及 swipe 的 `extra.model` 保存实际上游模型；`extra.nast_model` 保存逻辑模型、线路、任务 ID、`complete`／`incomplete` 和结构化诊断。历史消息的「模型调用信息」可查看这些信息。

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
| `models.list` | `{route_id}` 按已保存线路发现上游模型；保留旧 url/key 连接测试语义 |
| `generate.run` / `generate.group` | 兼容原参数；可加 `task_id`、`time_budget_secs`（1–600 秒） |
| `generate.status` | 原字段加 `info.task_id`、`route_state`、`reasoning` 和 `cancelling`，显示路由阶段与恢复进度 |
| `generate.stop` | 可加 `{task_id}`；不匹配时返回 `{ok:false,reason:"task_mismatch"}` |

私聊引用为 `{kind:"private",avatar:"角色.png",chat_file:"聊天.jsonl"}`；群聊引用为 `{kind:"group",group_id:"…",chat_id:"…"}`，服务端校验群聊归属。私聊通过 `chats.save` 导入新文件自动清除原线路，覆盖导入已有文件需传 `imported:true`。

新增事件 `model_catalog_changed`、`conversation_model_changed`、`generation_route`、`generation_diagnostic`。会话事件携带明确引用，生成事件携带任务 ID。错误响应保留 code/message 并增加 diagnostic；上游错误使用 `upstream`，目录冲突使用 `conflict`。诊断和广播不含密钥。

桥接端默认等待 240 秒，扣除 5 秒收尾余量作为服务端预算；显式 `BRIDGE_GEN_TIMEOUT_SECS` 优先。已提交请求断线后不会整次重发，超时取消必须匹配任务 ID。来源与聊天映射保存在 `BRIDGE_SESSIONS_PATH`，重启恢复原线程；只有 `/newchat` 主动另开线程。
