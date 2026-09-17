# nast vs SillyTavern 完成度差距（GAPS）

对照基准：refrence/SillyTavern release v1.18.0（HEAD 8172dcd0）。
状态标记：✅ 已对齐 | ⚠️ 有偏差 | ❌ 缺失。修复后请在对应条目标注 commit。

## 一、已完整对齐

- ✅ WI 扫描源接线（persona/char desc/personality/depth prompt/scenario/creator notes，含 per-entry match_*）
- ✅ WI 四源书（chat→persona→char(内嵌+charLore extraBooks)→global，strategy 0-2，去重，generate.rs）
- ✅ WI per-entry：scanDepth / caseSensitive / matchWholeWords / useGroupScoring（null=全局）、excludeRecursion、delayUntilRecursion 多级、characterFilter（names/tags/isExclude）、probability、sticky/cooldown/delay
- ✅ WI 预算逐 pass 累计制 `>=budget` + ignoreBudget 计数穿透（D5，world-info.js:4899-4958 精确复刻）
- ✅ WI 内容输出期 WORLD_INFO 正则 + key/keysecondary 宏替换 + /pattern/flags（D7）
- ✅ WI position 0-7 全消费：ANTop/ANBottom 并入 AN、EM 锚点按示例解析前后拼、outlet 走 {{outlet::key}} 宏（D8）
- ✅ AN 服务端化：chat_metadata.note_*，interval 用户消息数取模，位置 0/1/2，角色卡 note replace/prepend/append，默认 note，allowWIScan，ANTop/Bottom 合并
- ✅ Persona 服务端化：power_user.personas/persona_descriptions/default_persona + chat_metadata.persona 绑定；位置 0/2/3/4/9（TOP/BOTTOM_AN 并入 AN、AT_DEPTH 注入、NONE）；persona lorebook 为第 4 WI 源
- ✅ Reasoning：流捕获 → extra.reasoning/reasoning_duration/reasoning_type + swipe_info 镜像；auto_parse 剥离 <think>；前端折叠展示（含流式）；time_to_first_token
- ✅ Stopping strings：custom_stopping_strings（JSON+宏）→ 请求 stop（上限 4）+ 输出尾部前缀剥离
- ✅ cleanUpMessage 管线：user_prompt_bias / 正则默认 pass / collapseNewlines / 行尾空白 / 错误说话人移除（allow_name1/2_display）/ <|endoftext|> 截断 / 名字前缀剥离 / fixMarkdown(false) / trim_sentences / trim_spaces；显示侧 fixMarkdown(true) 引号/星号补齐（前端 st-display.ts）
- ✅ 消息编辑：chats.update_message 按索引只更新当前 swipe（mes+swipes[swipe_id]）+ runOnEdit 正则 + tainted（D12 后续）
- ✅ names_behavior COMPLETION（1）：历史消息带 name 字段
- ✅ 连接闭环：secrets.json（ST 同构掩码）、models.list 代理（临时 url/key 覆盖）、custom_url/custom_model 消费（env 降级）、custom_include_headers/body 透传；UI：baseURL/key/模型 Combobox/连接测试
- ✅ continue nudge 尾部顺序；squash 排除表；预算模型 reserve3/倒序断式填充；Claude 转换主链
- ✅ swipe/用户消息先落盘/regenerate 先删/impersonate/quiet 不落盘；错误不写消息；流式 + 停止保留半截
- ✅ 聊天 jsonl header/integrity/备份节流/原子写；PNG tEXt 解析、ccv3 优先、双写

## 二、有偏差（待修）

| # | 问题 | ST 行为 | nast 现状 | 位置 |
|---|---|---|---|---|
| D11 ⏸️(范围外) | Claude 次级 system 处理 | prompt_processing_type、空文本 \u200b、示例名字前缀 | 无条件合并 | providers/lib.rs |
| D15 | WI insertion group 语义 | 组过滤在预算前、按 pass 候选 | 已按 pass 过滤，但 useGroupScoring 评分退化为 order 最高 | world_info.rs |
| D16 | cleanUpMessage 次要项 | 群 cleanGroupMessage、instruct 序列裁剪 | 未实现（无群/无 instruct 路径时无行为差异） | generate.rs |
| D17 | reasoning 再注入 | reasoning.add_to_prompts（prefix/suffix/max_additions=1） | 未实现（默认关闭） | generate.rs |
| D18 | bias/logit bias/CFG | assistant bias 注入 | ID_BIAS 恒空 | prompt.rs |

## 三、缺失（前端为主）

- 群聊前端（后端 M4 就绪）：建群/成员/talkativeness/策略/触发/静音/auto-mode（M3）
- 预设 + Prompt Manager + 正则脚本编辑器 UI（M4；引擎已消费 prompts/prompt_order/regex_scripts）
- 聊天管理 UI：重命名/删除聊天（RPC 已有）、导出未接线、聊天搜索
- token 上下文计量条；侧栏角色头像图/新建/复制/收藏筛选；主题切换；移动端 inspector
- WS 指数退避重连 + 生成中断线恢复（M6）
- 测试：tests/ 硬编码 D:/temp 路径；continue/quiet/断线覆盖

## 四、明确范围外

文本补全路径（instruct/context 模板）、Claude/Gemini 原生源 UI 暴露（D11 一并顺延）、
向量/RAG、TTS、翻译、图像生成、多用户账号、浏览器端插件。

## 里程碑

- M0 连接闭环 ✅（secrets/models.list/custom_url；tests/m0_connection_smoke.js 10/10）
- M1 聊天链路对齐 ✅（tests/m1_pipeline_smoke.js 20/20；WI/AN/persona/reasoning/停止串/清理/编辑/COMPLETION）
- M2 前端修复与组件化（进行中）
- M3 群聊前端 → M4 预设/PM/正则 → M5 插件强化 → M6 健壮性
