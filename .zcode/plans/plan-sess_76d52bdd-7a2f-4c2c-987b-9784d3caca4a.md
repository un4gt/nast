# nast 对齐 SillyTavern 稳定日用 — 实施计划

**原则**（贯穿所有里程碑）：
- 聊天链路以 `refrence/SillyTavern` v1.18.0 为唯一行为基准，每个语义点实现时对照 ST 源码（调研已拿到全部 file:line 依据），完成后同步更新 GAPS.md。
- 前端只用现成组件：shadcn/ui 生态（现有 26 组件 + 补齐缺失组件）+ 少量成熟库（dnd-kit 拖拽、cmdk 已随 command 组件），禁止手写 UI 部件；存量手写件一并替换。
- 模型接入仅 OpenAI 兼容 `/chat/completions`（custom 源：baseURL + key + 模型拉取，覆盖 OpenRouter/DeepSeek/siliconflow/new-api/本地 vLLM）。Claude/Gemini 原生 provider 代码保留，UI 不暴露，D11 等 Claude 专属偏差顺延不做。
- 插件坚持服务端运行（mlua），不做浏览器端插件。
- API key 存 `data/<user>/secrets.json`（ST 同构格式 `{api_key_custom:[{id,value,label,active}]}`），环境变量降级为 fallback。

---

## M0 连接闭环（先行，其他一切的前提）

**后端**：
1. `nast-storage`：secrets.json 读写（原子写，同 settings 备份策略）。
2. RPC 新增 `secrets.get`（值掩码返回）/ `secrets.set`；`models.list`：服务端代 理 `GET {base}/models`（带 key）返回 `{data:[...]}`。
3. `src/rpc.rs:575-586` provider 构造改为：读 settings 的 `custom_url`（含 /v1，服务端补 `/chat/completions`，对齐 ST cc-srv.js:2305）+ secrets 的 key，env 仅 fallback；模型字段用 `custom_model`（`generate.rs` 中 `oai.openai_model` 的取值改为按源选字段）。
4. 顺带修 GenRequest：`extra_headers`/`extra_body` 从 settings 透传（ST `custom_include_headers`/`custom_include_body`）。

**前端**（ConnectionPanel 重构，全 shadcn）：
- 源选择（现阶段仅 custom）、baseURL Input、API key（password 型，存 secrets）、模型 Combobox（shadcn command+popover：拉取列表或手输）、连接测试按钮 + 成功显示模型数。

**验收**：UI 填 URL/key/model 后不开环境变量直接可生成；换模型不重启。

---

## M1 聊天链路完全对齐 ST（最核心）

按 ST 依据逐条落地（括号内为 ST 参考位置）：

1. **全局/辅助世界书接线**：消费 `settings.world_info.global_select` 与 `char_lore[].extraBooks`（WI.js:6039-6053）；合并顺序 = chatLore → personaLore → (charLore+globalLore 按 `world_info_insertion_strategy` 0/1/2) 组内 order 降序（WI.js:4478-4540）。改 `generate.rs:469-475` 的空 vec。
2. **WI 条目字段补全**：per-entry `scanDepth`/`caseSensitive`/`matchWholeWords`/`useGroupScoring`（null=跟随全局）、`excludeRecursion`、`delayUntilRecursion` 多级（bool/数值，WI.js:4641-4649）、`matchCreatorNotes`、`characterFilter`（names/tags/exclude，WI.js:4701-4726）；**D5** 预算边界改累计制 `>=budget`；**D7** 内容过 WORLD_INFO 正则、key 宏替换 + /flags 解析；position 2/3（并入 AN 上下文）、**D8** position 5/6 EM 锚点注入示例对话区（script.js:4576-4596）、position 7 outlet `@@outlet>`。
3. **AN 服务端化**：`chat_metadata` 存 `note_prompt/note_interval/note_depth/note_position/note_role`（AN.js:30-36），interval 按用户消息数取模插入（AN.js:324-392），位置 0=主提示后/1=聊天内深度/2=主提示前，角色卡 AN 合并（replace/prepend/append），默认 note，AN 参与扫描；**删除前端 localStorage 草稿链路**（store.ts:83-96 中 AN 部分）。
4. **Persona 服务端化**：`power_user.personas` + `persona_descriptions`（多 persona 管理）、`default_persona`、`chat_metadata.persona` 每聊天绑定 + 自动锁定（personas.js:934-940）、位置 0/2/3/4/9（IN_PROMPT/TOP_AN/BOTTOM_AN/AT_DEPTH/NONE，TOP_AN/BOTTOM_AN 并入 AN 文本）；**统一现有两套冲突机制**（PersonaPanel vs CharacterPanel 草稿）为一个；persona lorebook 作为第 4 个 WI 源。
5. **Reasoning 落盘**：流循环捕获 `StreamEvent::Reasoning`（generate.rs:186 现在丢弃）→ `extra.reasoning`/`reasoning_duration`/`reasoning_type` + 每 swipe 独立存 `swipe_info[i].extra`；`auto_parse` 剥离 `<think>...</think>`；提示词再注入（`reasoning.add_to_prompts`、prefix/suffix、max_additions=1）；前端折叠展示（Collapsible）+ 独立编辑。
6. **Stopping strings**：`power_user.custom_stopping_strings`（JSON 串）+ 宏开关；请求 `stop` 参数（上限 4）+ 输出侧尾部部分前缀剥离（script.js:6418-6428）。
7. **cleanUpMessage 管线**（仅生成输出侧，script.js:6383-6533）：`user_prompt_bias` 前置、错误说话人名字移除（allow_name1/2_display）、`<|endoftext|>` 截断、群消息清理、名字前缀剥离、`fixMarkdown(false)`、`trimToEndSentence`、trim_spaces，全部走 power_user 开关（默认值与 ST 一致）；显示侧 `messageFormatting` 补引号/星号闭合（fixMarkdown(true)，仅渲染不落盘）。
8. **消息编辑语义修复**：新增 `chats.update_message`（按索引+swipe 定位，不按内容匹配），只更新当前 swipe（`mes` + `swipes[swipe_id]`，script.js:8080-8136），编辑后重跑 regex(runOnEdit)、`chat_metadata.tainted`。
9. **names_behavior COMPLETION**（generate.rs:445 硬编码 None 处）。

**验收**：新增 parity 测试脚本 —— 用同一份 ST 导出的 settings+chat，在 mock provider 下断言两边最终发出的 messages 逐条一致；GAPS.md D5-D8 关闭。

---

## M2 前端修复与组件化

- **替换手写件**：toasts→sonner（已装未挂载）、设置导航→Tabs、inspector 标签栏→Tabs/ToggleGroup、裸 textarea→`<Textarea>`（CharacterPanel.tsx:122、AiResponsePanel.tsx:162）、坏掉的 LocalSlider→Slider（AppearancePanel.tsx:44-62）。
- **补 shadcn 组件**（CLI 按 tailwind3 兼容路径添加）：command、popover、checkbox、radio-group、collapsible、context-menu、hover-card；新依赖 `@dnd-kit/core`+`@dnd-kit/sortable`（WI 条目/提示词排序用）。
- **聊天管理补全**：重命名/删除聊天 UI（后端 RPC 已有）、导出接线（jsonl/txt）、聊天搜索（command palette）。
- **消息操作**：复制按钮、删除加 alert-dialog 确认。
- **输入草稿**：按聊天持久化（localStorage keyed by chat），切聊天/生成不丢。
- **token 计量条**：`chats.get` 附带每消息 token_count + 总预算占比，Progress 条显示。
- **角色列表**：显示头像图（修 dev proxy `/thumbnail` 未代理问题）、新建空白角色、复制角色、收藏/标签筛选（cmdk）、排序。
- **主题**：挂载 next-themes（已装）+ 补 `.dark` 变量 + AppearancePanel 切换开关。
- **流式期间渲染 markdown**（StreamingBubble 目前纯文本）；移动端 inspector 改 Sheet。

---

## M3 群聊前端（后端 M4 已就绪，纯 UI + 少量接线）

- 建群对话框（成员多选、头像网格）、侧栏群列表、群聊天视图（发言人头像/名字、隐藏/静音成员）。
- 每成员：触发单聊按钮（force_chid 语义）、talkativeness 滑条、静音开关。
- 群设置：激活策略 NATURAL/LIST/MANUAL/POOLED、allow_self_responses、auto-mode 延时循环（可后置）。
- 群消息 swipe/重生成/继续接线 `generate.group`。
- **修 group_gen.rs 绕过插件钩子的问题**（目前不派发任何 plugin 事件）。

---

## M4 预设 + Prompt Manager + 正则编辑器

- **预设**：`OpenAI Settings/*.json` 读写 RPC（ST preset 字段子集，含 prompts/prompt_order）+ 选择/另存/删除 UI。
- **Prompt Manager**：拖拽排序（dnd-kit）、启停、增删改自定义提示词、marker 特殊标识（chatHistory/worldInfoBefore 等 12 个）、注入位置/深度/顺序/角色/forbid_overrides 编辑；全局 100001 + 按角色 prompt_order。
- **正则编辑器**：编辑 `extension_settings.regex` 全局脚本 + 角色内嵌脚本，字段全覆盖（placement 多选 1/2/3/5/6、markdownOnly/promptOnly/runOnEdit、substituteRegex 0/1/2、min/maxDepth、trimStrings）、启停、对样例文本试运行、`character_allowed_regex` 角色脚本总开关。

---

## M5 插件强化（服务端 mlua）

- KV 持久化：`plugin:<name>:` 命名空间落盘（当前内存态、reload 即失）。
- 宿主侧 API：`nast.on` 注册进宿主（插件去样板）、dispatch 超时隔离（死循环插件不再卡死生成锁）、`nast.toast`、`nast.register_command`（斜杠命令，在 user_input 前解析）。
- 新钩子：`prompt_built`（拼装后快照/参数改写）、`wi_entries`（世界书条目过滤）、`message_saved`、群聊生成各阶段（依赖 M3 的 group_gen 修复）。
- 插件管理 UI：列表/启停/重载/最近日志。

---

## M6 健壮性

- WS 指数退避 + 抖动重连；重连后 `generate.status` RPC 恢复：流事件改为携带累积全文（或定期快照），断线重连能续上流式气泡。
- 测试修复与扩展：去掉 D:/temp、端口硬编码（改 env）；补 continue/quiet/stop 中断/reasoning 落盘/全局 WI 激活/预设 round-trip/断线恢复覆盖；parity 对比脚本（M1 验收工具沉淀为常驻测试）。

---

## 执行顺序与理由

M0（不修无法日用）→ M1（用户定义的最高优先级：链路一致）→ M2（日用体验痛点）→ M3 → M4 → M5 → M6。M1/M2 内部条目可交叉推进，但每个里程碑以"对照 ST 行为可验证"为完成标准，GAPS.md 随做随更。

**明确不做（本轮范围外）**：文本补全路径与 instruct/context 模板、Claude/Gemini 原生源的 UI 暴露及其专属行为（D11、assistant prefill）、向量/RAG、TTS、翻译、图像生成、多用户账号、浏览器端插件。