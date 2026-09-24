# nast 与 SillyTavern 功能对齐审计

> 实施更新（2026-09-23）：下文是实施前的缺口基线，不代表所有问题仍然存在。角色导入/内嵌书关联、设置归一化、生成宏与正则、群生成复用及 JSON/SSE 等已实施修复；最新已验证范围、证据及剩余限制见 [ST_PARITY_IMPLEMENTATION.md](ST_PARITY_IMPLEMENTATION.md)。未在实施记录中销项的高级场景仍需单独验证。

审计日期：2026-09-22。参考：本仓库 `refrence/SillyTavern`，v1.18.0，提交 `8172dcd`。nast 基线提交：`51d5df0`，包含当前工作区未提交的前端和 TTS 改动。

## 目标与结论

重写保留两个明确方向：简化前端，把扩展业务执行放到服务端。其余部分以 ST 的数据格式、生成语义和使用能力为对齐目标，尤其角色卡、角色内嵌书、独立世界书及其关联关系。

当前项目已经具备角色对话、世界书扫描、提示词拼装、群聊、正则、服务端 Lua 插件和 TTS 的基础框架，但还不能视作“除界面和插件架构之外基本对齐 ST”。最需要先解决的是导入和实际生成之间的兼容断点；部分已有界面、字段和引擎代码尚未形成可用闭环。

旧 `GAPS.md` 中把文本补全、Claude/Gemini 原生界面、RAG、翻译、图像生成、多用户、分支列为“范围外”的结论，不再直接适用于此次审计。这些应进入待对齐清单，按使用价值排序。ST 本身也有服务端插件加载器；本项目的变化主要针对其浏览器扩展生态及业务执行方式。

本报告区分以下状态：

| 状态 | 含义 |
| --- | --- |
| 已有基础 | 找到了实际入口和调用链；只说明列出的基础行为存在 |
| 行为偏差 | 功能存在，但格式、执行结果或状态变化与参考不一致 |
| 接线缺口 | 模型、接口或引擎存在，生成链路没有消费，或前端缺少入口 |
| 缺失 | 在当前应用接口、前端和业务代码中未找到对应实现 |
| 架构差异 | 为简洁界面和服务端扩展而保留的设计选择 |

“字段能保存”“菜单能显示”“某个冒烟测试通过”均不单独作为完整对齐的证据。审计覆盖本地 ST 核心和随仓库提供的主要扩展，不代表已逐个审查所有第三方插件。

## 优先修复的兼容与生成缺陷

以下 A01–A11 建议作为第一阶段的正确性工作。它们会影响现有卡片、预设或聊天的实际行为，优先于增加新的扩展功能。

### A01：独立世界书字段名、空值类型不兼容

ST 和 nast 前端使用 `selectiveLogic`、`preventRecursion`、`ignoreBudget`、`scanDepth`、`useProbability`、`matchCharacterDescription` 等字段。Rust `WIEntry` 对应字段使用 snake_case，除少数字段外没有 serde 别名或重命名。存储和 RPC 直接反序列化，没有归一化步骤。

结果是这些 ST 字段被放进 `extra`，引擎读取的 typed 字段仍是默认值。UI 修改同名 camelCase 字段也不能修复引擎读取值。常驻、正文、关键词等同名字段可以工作，不能据此认定高级规则有效。

另一个独立问题是 ST 的 `sticky/cooldown/delay` 可以为 `null`，当前使用非可空整数，会让整本书解析失败。生成器以 `if let Ok(book)` 读取书，失败时会跳过该书。

隔离验证：输入 `selectiveLogic: 3 / preventRecursion: true / ignoreBudget: true / scanDepth: 9 / useProbability: false`，实际引擎字段为 `0 / false / false / null / true`；输入 `sticky: null` 得到 `invalid type: null, expected i64`。

证据：[WIEntry][n-world-model]、[worlds_get/worlds_save][n-rpc]、[read_world/save_world][n-storage]、[前端 worldStore][n-world-store]；参考 [newWorldInfoEntryDefinition][s-world]。

### A02：ST 世界书设置层级和关联键没有迁移

ST 保存的是：

```text
settings.world_info_settings.world_info_depth
settings.world_info_settings.world_info.globalSelect
settings.world_info_settings.world_info.charLore
```

nast 读取的是：

```text
settings.world_info.world_info_depth
settings.world_info.global_select（同层兼容 globalSelect）
settings.world_info.char_lore
```

因此，直接使用 ST 的设置文件不会恢复同样的扫描参数、全局激活书和角色辅助书。隔离生成中，将一本可用书放进 ST 的 `globalSelect` 后，该书没有出现在模型请求里；通过 nast 的聊天绑定则可以注入。

设置读取是原样 JSON，没有旧格式迁移。类似地，ST 的顶层 `username` 与当前读取的 `power_user.username` 也应统一。独立世界书模型只有 `entries`，读写后还会丢掉 ST 转换内嵌书时保留的书级 `originalData`，影响往返导出。

证据：[build_assemble_input/wi_settings/resolve_persona][n-generate]、[存储][n-storage]、[ST 默认设置][s-settings]、[ST 设置保存][s-script]。

### A03：角色卡解析与 V2/V3 往返不完整

已复现以下问题：

| 合成输入 | 当前结果 |
| --- | --- |
| `data.extensions.talkativeness: 0.7` | 拒绝解析，要求字符串；ST 使用数值的路径存在 |
| 内嵌书 `extensions.delay_until_recursion: 2` | 拒绝解析，要求布尔值 |
| 内嵌书 `extensions.sticky: null` | 拒绝解析，要求整数 |
| V3 的 `spec/spec_version` | 解析再序列化后缺失 |

`Character` 没有保存顶层 `spec/spec_version`，导入和编辑却根据序列化后的 `spec` 决定是否写 `ccv3`。因此底层 PNG chunk 工具具有写 V3 的能力，也不能保证当前角色导入/编辑路径保留 V3 身份。V3 部分数据字段仍然保留，不能把这一问题描述为“所有 V3 内容全丢失”。

角色复制另外只修改顶层 `name`，没有同步 `data.name`，存在卡片内外名称不一致。完整的未知字段保留也应覆盖根对象、书对象和条目，而非仅部分 `extra`。

证据：[Character/from_card_json/BookEntryExtensions][n-card-model]、[characters_import/edit/duplicate][n-rpc]、[PNG chunk 写入][n-cards]；参考 [ST 角色导入与导出][s-characters]。

### A04：角色卡内嵌世界书缺少导入、绑定、回写闭环

`data.character_book` 目前主要是保存字段。角色导入没有将它转换为独立世界书，也没有 ST 的“导入角色书并关联”入口。生成器只按 `data.extensions.world` 读取磁盘上的独立书。

ST 的设计是识别内嵌书、提示或允许用户导入、转换条目并关联角色；并非仅发现 `character_book` 就无条件扫描。本项目缺少的是这个明确的导入工作流及后续管理能力。

当前侧栏把 `extensions.world` 对应的外部书标为“内嵌书（卡内）”，容易混淆“卡内携带的数据”和“已经绑定的独立世界书”。隔离生成也确认：只携带一条常驻内嵌书条目的角色卡，不会把条目带入请求。

证据：[characters_import][n-rpc]、[build_assemble_input][n-generate]、[WorldInfoPanel][n-world-panel]；参考 [convertCharacterBook/checkEmbeddedWorld/importEmbeddedWorldInfo][s-world]、[ST 角色书回写][s-characters]。

### A05：正则脚本格式、执行语义和作用域未对齐

ST 脚本的 `scriptName/findRegex/replaceString/markdownOnly/promptOnly/runOnEdit/minDepth/maxDepth` 等是 camelCase；当前模型无相应别名，且没有未知字段透传。合成 ST 脚本解析后，名称、查找表达式和替换文本均为空。

即使手工改成当前字段名，仍有以下偏差：

- ST 的 `/pattern/flags` 字面量没有转换；实测 `find_regex: "/foo/gi"` 无法将 `foo FOO` 替换为预期结果。
- 当前 Rust `regex` 不支持常见 JS look-around、反向引用等语法，解析失败时静默跳过。
- 默认脚本在当前实现中会参与多个 pass；ST 对普通脚本限制为原文处理 pass。双勾选 `markdownOnly/promptOnly` 时，当前 `if/else` 也不等价于 ST 的 OR 条件。
- ST 1.18 的集合包括全局、预设、角色脚本及角色/预设允许启用状态；当前聚合的是全局、角色、聊天，没有预设脚本和对应允许启用门控。
- UI 试运行直接使用 `new RegExp`，与服务端执行不一致，不能作为兼容验收。

这些会影响角色卡中常见的文本整理、状态栏和显示替换能力。应保留服务端执行，并补足数据格式和语义。

证据：[RegexScript][n-regex-model]、[regex_engine][n-regex-engine]、[collect_regex_scripts][n-generate]、[RegexPanel][n-regex-ui]；参考 [ST regex engine][s-regex]。

### A06：普通生成丢弃角色系统提示和 PHI 覆盖

生成器读到了 `system_prompt`、`post_history_instructions`，并放入 `BridgeInput`，但普通生成调用的 `assemble_with_macros` 把两项重设为 `None`。单聊普通回复、swipe、重生成和当前群聊路径都会经过这里。

隔离拼装输入设置了两段唯一标记，输出中两段均不存在，且没有错误。续写等变体使用另一套 bridge 函数，不能用其已传递覆盖字段来证明普通生成正常。修复时还需保留 ST 的角色提示偏好、`forbid_overrides`、聊天级覆盖等条件。

证据：[普通拼装桥][n-bridge]（`assemble_with_macros`）、[生成输入][n-generate]；参考 [ST 角色提示读取][s-script]、[PromptManager][s-prompt-manager]。

### A07：宏引擎有实现，运行上下文没有接齐

`MacroContext` 有局部/全局变量、聊天长度、历史消息、输入、token 上限、swipe 编号等字段，实际替换却反复创建默认上下文，没有载入和回写聊天变量，也没有共享一次生成的变量状态。

实测：一段文本执行 `setvar`，下一段文本的 `getvar` 读不到值；`lastMessage` 为空，`maxContext` 为 0。同一段文本内的变量替换能够工作，跨段和跨回合的持久语义缺失。

`group` 多处被填为当前角色名；模型名、其他成员、角色版本等宏环境也不完整。WI outlet 在部分桥接阶段有数据，但提示词引擎重新构造环境时没有传入 outlet，不能把所有位置的 outlet 宏视为已对齐。

证据：[MacroEnv/MacroContext][n-macros]、[substitute_text/env_for][n-bridge]、[prompt::substitute_dyn][n-prompt]、[WI substitute_entry][n-wi-engine]；参考 [ST macros][s-macros]、[variables][s-variables]。

### A08：群聊没有复用完整的角色生成上下文

群聊使用单独的 `build_group_input`，显式传入空世界书、空 Persona 描述、空作者注记和空 outlet，`name1` 固定为 `User`。它还使用空 `chat_file` 去读取单聊元数据，拿不到群聊天元数据。

因此，即使群用户消息署名使用了 Persona，也不代表该 Persona 的描述、世界书、姓名宏已进入生成；全局/角色/聊天世界书和作者注记同样不生效。ST 群聊会选出成员后调用通用 `Generate`，这些上下文仍参与生成。

群聊还有独立的正则、推理保存、停止串和插件钩子接线差异。当前群流式推理可以显示，但生成落盘没有保存对应 reasoning。应统一生成上下文，再保留成员选择和 SWAP/APPEND 逻辑。

证据：[generate_group/build_group_input][n-group-gen]；参考 [ST generateGroupWrapper][s-group]、[通用 Generate][s-script]。

### A09：世界书驻留/冷却状态会在生成后丢失

扫描阶段把定时状态单独写回磁盘，但生成结束又保存了扫描前载入的 `chat`，覆盖刚写入的元数据。模型中的键还是 `timed_world_info`，ST 使用 `timedWorldInfo`，也需要往返兼容。

隔离真实生成调用链中，一条带 `sticky: 5` 的常驻世界书条目成功进入请求，回复也保存成功，但保存后的聊天元数据没有定时状态。引擎层存在 sticky/cooldown 单测不能覆盖这一问题。

证据：[build_assemble_input/run_message_gen/save_chat_metadata][n-generate]、[ChatMetadata][n-chat-model]、[全量 save_chat][n-storage]；参考 [ST WorldInfoTimedEffects][s-world]。

### A10：非流式回复没有正确解析，流式开关不生效

普通生成和群生成把 `GenRequest.stream` 固定为 `true`，没有消费 UI 的 `stream_openai`。隔离调用确认开关设为 false 时，请求仍带 `stream: true`。

更影响功能的是：非流式 `Provider::generate` 发送 `stream: false`，响应仍进入 SSE 解析器。隔离上游返回标准 OpenAI JSON、正文为 `JSON_REPLY_SENTINEL`，当前返回空字符串。使用这一调用的 quiet、impersonate 和 continue 不能算完整可用，先前只返回 SSE 的 mock 不能证明这些路径支持真实非流式协议。

证据：[provider generate/send_stream][n-providers]、[call_provider/run_continue/run_quiet/run_impersonate][n-generate]、[AI 回复开关][n-ai-response]。

### A11：Persona 编辑路径把 `.png` ID 拆开

Persona ID 形如 `audit.png`，编辑器拼出 `power_user.personas.audit.png`，设置面板的通用 `patch` 再按 `.` 拆分路径。实测执行从实际源码提取的函数，结果是 `{audit: {png: "Audit User"}}`，而非 `{"audit.png": "Audit User"}`；描述字段同样错误。

这直接影响新建 Persona，以及带点号标识的名称和描述修改。后端有 Persona 解析代码并不意味着 UI 管理闭环已完成。

证据：[PersonaPanel][n-persona-ui]、[SettingsSheet.patch][n-settings-ui]、[resolve_persona][n-generate]。

## 按功能梳理其余未完成部分

表中的“已有基础”均受上面相关缺陷约束。优先级：P0 为上面的兼容和生成正确性；P1 为核心日常使用；P2 为长上下文、自动化及主要扩展能力；P3 为后续可选能力。P2/P3 表示实施顺序，不表示永久排除。

### 角色卡、Persona 和世界书

| 功能 | 当前基础 | 仍需补齐 | 建议 |
| --- | --- | --- | --- |
| 角色创建与基础导入 | 有新建、PNG/JSON 导入、收藏、搜索、复制、删除 | A03 的数据兼容；导入诊断与迁移提示 | P0 |
| 角色详细编辑 | UI 可改描述、性格、场景、开场白、示例、系统提示和 PHI | 角色改名、换头像；标签编辑；作者/版本/作者注释；备用和群专用开场白；深度提示；扩展字段管理。部分文本字段已有 RPC，UI 没入口 | P1 |
| 角色书生命周期 | 有外部书名字段和角色辅助书选择 | A04；选择/替换主关联书、内嵌书导入、独立书回写卡片、导出携带角色书 | P0 → P1 |
| 卡片格式和资源 | 有 PNG chunk 工具，模型保留部分 V3 字段 | V2/V3 正确导出入口；CHARX/BYAF/YAML 等参考支持的导入；V3 assets 实际导入、资源寻址和资源包导出 | P1 / P2 |
| Persona | 有名称、描述、默认和聊天绑定的后端路径、注入位置 | A11；头像上传与实际展示；角色/群绑定及多 Persona 关联；从角色转换；Persona 世界书选择和导入导出 | P0 → P1 |
| 世界书基础扫描 | 有关键词、次关键词、常驻、概率、递归、预算、深度注入代码 | A01/A02/A08/A09，确保真实导入书与手工创建书具有相同行为 | P0 |
| 世界书高级激活 | 部分字段和算法存在 | `useGroupScoring` 当前按 order 选胜者，ST 按命中评分；生成类型 `triggers`、`automationId` 调用、vectorized 检索未执行；扫描时包含发言者姓名没有接入；组内定时效果优先规则还需对齐 | P1 / P2 |
| 世界书编辑器 | 可建书、删书、加删条目、编辑基础关键词/位置/概率/定时字段 | 搜索筛选、折叠、排序、复制/移动/批量操作、导入导出、重命名及关联更新；position 7/outlet、分组权重、逐条扫描覆盖、角色过滤、触发类型、自动化等高级入口缺失 | P1 |
| 激活检查与预算解释 | 引擎有结果及预算状态 | UI 缺本轮激活条目、未激活原因、触发来源和预算溢出信息，无法确认复杂角色书是否生效 | P1 |

主要证据：[角色侧栏][n-character-ui]、[角色列表][n-sidebar]、[角色 RPC][n-rpc]、[世界书编辑器][n-world-editor]、[世界书全局设置][n-world-global]；参考 [ST 角色管理][s-characters]、[personas.js][s-personas]、[world-info.js][s-world]。

### 聊天与群聊

| 功能 | 当前基础 | 仍需补齐 | 建议 |
| --- | --- | --- | --- |
| 单聊与消息操作 | 新聊天、开场白选择、流式、单聊 swipe/重生成、编辑、复制、删除、历史切换 | 非流式变体按 A10 修复；消息移动/插入/隐藏及选择性排除上下文等 ST 编辑工作流未完整提供 | P0 / P1 |
| 聊天导入与检索 | 可读既有 JSONL 文件，单聊可导出 JSONL/TXT；有角色名称搜索 | 从 UI 导入聊天、其他格式转换、跨聊天全文搜索、搜索结果定位、最近聊天管理 | P1 |
| 分支与检查点 | 有普通聊天文件读写 | 从指定消息分支、检查点、返回父聊天和关联管理缺失；普通历史切换不能代替这些能力 | P1 |
| 作者注记 | 单聊有文本、位置、深度、间隔编辑，后端有默认注记/角色注记处理 | 群聊接线；全局默认/角色专用注记管理和扫描设置入口；不同注入位置与间隔边界需行为验收 | P1 |
| 群聊基础 | 建群、成员增删/静音/健谈度、激活策略、SWAP/APPEND、指定成员回复 | A08；APPEND 示例/群聊天覆盖字段等进一步对齐 | P0 / P1 |
| 群聊重生成与 swipe | 已有“删除末条后再生成”的按钮 | ST 重生成按 `gen_id` 删除上一整批回复；当前只删一条。缺针对原发言成员的 swipe/continue 等分支，不能复用普通续聊冒充 | P1 |
| 群聊会话管理 | 有群聊天存储与部分列表 RPC | UI 基本围绕单个 `group.chat_id`；新会话、历史会话切换、导入导出、检查点等不完整 | P1 |
| 自动对话 | 手动发送和点名 | 群 auto-mode 定时任务、自动续写/自动重试等调度缺失 | P2 |
| 生成失败、停止与重连 | 有取消 token、进度查询、WS 重连及单聊进度恢复 | 群生成的错误分支可在清理生成状态之前返回；需要一致的生命周期和部分结果处理。当前只证明框架存在 | P1 |

主要证据：[ChatActionsMenu][n-chat-menu]、[store][n-store]、[GroupChatArea][n-group-ui]、[群生成][n-group-gen]；参考 [ST chats 路由][s-chats]、[bookmarks.js][s-bookmarks]、[group-chats.js][s-group]。

### 模型连接、提示词与正则

| 功能 | 当前基础 | 仍需补齐 | 建议 |
| --- | --- | --- | --- |
| OpenAI 兼容连接 | 自定义 URL/模型、密钥、模型列表、附加请求头和 JSON 请求体 | ST 各原生源配置迁移；当前除 Claude/Gemini 外均落到 custom，不能直接沿用 OpenRouter 等来源的模型/密钥字段 | P1 |
| Claude / Gemini 原生协议 | 后端有请求和流解析路径 | 连接 UI 仅 custom，进入面板会将非 custom 草稿改成 custom；原生密钥/模型列表/专用参数、提示后处理和缓存/推理块行为不完整 | P1 |
| 文本补全模型 | 当前主要是 Chat Completions | instruct/context/system prompt 模板及文本补全请求；Kobold/llama.cpp/Ooba/NovelAI/Horde 等对应工作流缺失。服务商能提供兼容 chat 接口仅覆盖其一部分 | P1 / P2 |
| 采样和请求控制 | Temperature、Top P、频率/存在惩罚、上下文/回复长度、stop | Top K、Min P、重复惩罚、seed、多候选、logit bias、CFG 等没有完整原生映射；附加 JSON 可手工传部分字段，不等于 ST 预设兼容；`custom_exclude_body` 等缺失 | P1 / P2 |
| Prompt Manager / 预设 | 有顺序、开关、自定义项、深度注入及预设保存/应用/删除 | UI 把 main/nsfw/jailbreak/enhanceDefinitions 也当 marker，隐藏其正文编辑；缺生成类型触发编辑、预设导入导出与完整调试；pin examples 未执行；A06/A07 | P0 / P1 |
| 提示词与 token 检查 | 有预算裁剪和历史 token 统计 | 缺实际请求预览、组成明细和裁剪原因；非 OpenAI tokenizer 主要近似使用 cl100k，WI 固定按 gpt-4o 计数；拼装预算错误未在请求前可靠中止 | P1 |
| reasoning | 单聊有部分推理增量捕获、think 标签提取、保存和折叠显示 | 历史推理再次注入、格式/次数控制、签名和原生 thinking 配置；Gemini thought 当前跳过；群 reasoning 保存；`include_reasoning` 未形成完整消费 | P1 / P2 |
| 工具调用与多模态 | 请求消息正文主要是 String | tools/tool_calls 的请求、执行、结果回填和继续生成；图片/音频/视频内容块；模型生成媒体回传和展示；JSON schema 等原生工作流 | P2 |
| 正则管理 | 全局编辑器与服务端处理基础 | A05；角色/预设作用域管理、导入导出、排序、允许启用状态；编辑和显示重算，保证预览与服务端一致 | P0 / P1 |

主要证据：[connection.rs][n-connection]、[providers][n-providers]、[OaiSettings][n-preset-model]、[PromptManagerPanel][n-prompt-ui]、[PresetPanel][n-preset-ui]、[tokenizers][n-tokens]；参考 [ST openai.js][s-openai]、[PromptManager][s-prompt-manager]、[tool-calling.js][s-tools]、[ST tokenizers][s-tokenizers]。

### 自动化、扩展与语音

| 功能 | 当前基础 | 仍需补齐 | 建议 |
| --- | --- | --- | --- |
| STScript / Slash Commands | 少量内置命令，自定义命令展开成文本，Lua 注册命令 | 管道、参数、闭包、变量作用域、条件、循环、返回值及广泛命令集；当前群聊只拦截 TTS 命令，其他命令没有同等通路 | P2 |
| Quick Replies | 有自定义文本命令 | ST 的 QR 集、按钮、上下文菜单、聊天/角色绑定、启动/聊天/用户/AI 等事件自动执行缺失 | P2 |
| 长期记忆与摘要 | “记忆”面板为占位，作者注记可手工填写 | 自动摘要、摘要维护、历史压缩、摘要注入及记忆更新策略 | P2 |
| Data Bank / 附件 / 向量 | 消息 extra 能保留部分未知字段 | 上传和解析文档、全局/角色/聊天附件库、embedding、向量索引/查询、聊天向量化、检索内容注入；WI vectorized 也依赖此能力 | P2 |
| 翻译 | TTS 可选显示文本作为朗读来源 | 输入/输出翻译服务、语言设置、译文状态和缓存缺失；已有 `display_text` 不能证明翻译已实现 | P2 |
| 图片描述和生成 | 普通 Markdown 可展示可访问的图片 URL | Caption、生成图片、提示词和风格预设、来源配置、图片保存/切换以及模型视觉输入 | P2 |
| 表情、画廊、背景和资源 | 基本头像、明暗主题、消息 HTML/Markdown 展示 | 表情分类/立绘、角色画廊、资源包和背景管理、资源 URL 解析等；原生展示层可按简洁 UI 设计实现 | P2 / P3 |
| 语音识别 | 新 TTS 处理输出语音 | ST speech 后端已有转写能力；当前无对应录音/转写业务链路。语音输入应单独立项 | P2 |
| TTS | 当前工作区已有 28 provider 入口、配置、映射、自动/手动朗读、播放器及部分服务管理 | HTTP 按文本段缓冲，缺逐 PCM 块即时播放；SpeechT5 在浏览器运行而参考在服务端；无全局 RVC 扩展接线。跨 provider 的所有线上账户/服务版本未逐家实测 | 已有基础，差异见 TTS 文档 |
| 联网搜索和页面读取 | 无完整业务入口 | 参考仓库 search 后端的搜索源、网页读取等能力，以及与模型工具和检索链路的连接 | P2 |

TTS 这次按已存在的源码能力计入，不再列为“没有 TTS”。Kokoro/SpeechT5 目前在浏览器 Worker 做本地推理，属于内置模型运行位置；它与“第三方插件业务必须在服务端执行”的要求需分别管理。若进一步统一模型推理到服务端，当前这两个路径仍需迁移设计。浏览器负责播放与交互符合现有架构目标。

主要证据：[commands.ts][n-commands]、[记忆占位面板][n-inspector]、[TTS 实现与验证记录][n-tts-doc]、[服务入口][n-main]；参考 [ST slash parser][s-slash]、[Quick Reply][s-qr]、[memory][s-memory]、[attachments][s-attachments]、[vectors][s-vectors]、[translate][s-translate]、[caption][s-caption]、[图像生成][s-imagegen]、[expressions][s-expressions]、[speech][s-speech]、[search][s-search]。

### 服务端插件与运行管理

| 功能 | 当前基础 | 仍需补齐 | 建议 |
| --- | --- | --- | --- |
| 服务端扩展宿主 | Lua 专用线程、钩子、命令、KV、日志、toast、JSON、重载；有 Rust hook 接口 | 支持插件读取受控聊天/角色/世界书上下文，调用模型/HTTP/存储，注册任务和工具；插件设置 schema、版本/依赖和启停/更新管理 | P1 基础 API，P2 生态 |
| 插件组合与事件 | 若干生成钩子已调用，声明了一批 ST 同名 WS 事件 | 声明的事件不等于已派发到 Lua；普通、quiet、continue、群聊钩子不统一；transform 只取首个返回值，结构化派发可因前一个插件没有该 hook 提前返回。缺明确的优先级、串联和生命周期契约 | P1 |
| 插件超时恢复 | 调用方等待有超时 | 同一个 worker 执行所有 Lua；调用方超时不会终止卡住的 Lua，后续派发/列表/重载仍可能排队。需要可恢复的执行限额/隔离机制；本轮没有运行死循环探测 | P1 |
| 连接配置管理 | 有一份连接配置和密钥存取 | ST Connection Manager 的命名配置切换、模型/预设关联、单服务多密钥标签及激活切换等 | P1 / P2 |
| 数据互通 | 有相似目录布局，聊天文件原样 JSONL 保存、部分额外字段透传 | A01–A05/A09 的格式迁移；角色/书/预设/Persona/聊天导入导出闭环；未知字段、关联和资源往返保障 | P0 → P1 |
| 备份与恢复 | 有聊天和设置的节流备份、原子写和 integrity 校验 | 备份列表、下载/恢复和清理入口，完整用户数据迁移与恢复流程 | P1 |
| 多用户与账户 | 启动固定 `default-user`，事件广播到所有连接 | 用户选择/登录、用户数据隔离、账户管理和会话范围。需作为单独能力阶段实现；本轮不做配置或部署变更 | P2 |

插件后端化是保留的架构方向。旧浏览器插件不能直接装载这一点本身不作为缺陷；它们提供的业务能力应通过服务端 API、任务/工具机制和少量声明式 UI 重新提供。也不应以“允许执行 Lua”推断已具备 ST 扩展生态的能力覆盖。

主要证据：[PluginHost/PluginManager][n-plugin]、[事件常量][n-events]、[插件面板][n-plugin-ui]、[secrets RPC][n-rpc]、[用户和事件状态][n-state]、[服务入口][n-main]；参考 [ST 服务端插件加载器][s-plugin-loader]、[Connection Manager][s-connections]、[备份接口][s-backups]、[用户接口][s-users]。

## 建议的实施顺序与验收方式

1. **数据兼容和生成正确性。** 先完成 A01–A11，建立 ST 原始格式到 nast 实际请求的验证。重点包括内嵌书、camelCase 正则、可空字段、V2/V3 标记、变量状态、角色提示、群上下文、非流式回复和 Persona 编辑。
2. **角色/世界书日常管理。** 完成角色和书的导入、编辑、关联、导出；补齐预设核心正文编辑、Persona、聊天分支/检索/备份恢复以及群聊操作。高级设置可以折叠，保持常用路径简洁。
3. **服务端扩展基础与主要模型能力。** 统一单群聊生成上下文和钩子，再完善原生模型源、文本补全、推理、工具、多模态，以及插件受控 API 和恢复机制。
4. **长对话与自动化。** 接入摘要、Data Bank/向量、STScript/Quick Reply，随后翻译、图片、语音输入和其余展示扩展。顺序可按实际使用频率调整。

每项完成至少区分三种验收：

- 格式：ST 导出的数据直接导入，nast 再导出，关键字段、未知字段、资源和关联保留。
- 行为：使用同一组合成卡/书/预设/聊天，比较发给模型的消息、激活条目和状态变更；随机行为控制随机源。无需靠模型最终回复文本判断是否对齐。
- 使用：用户能在简洁 UI 中完成日常操作，保存后能影响下一次生成，并能看到失败原因。

对于群聊，应额外覆盖多成员一批回复、指定成员、整批重生成、swipe、continue、停止和失败；对于世界书，应覆盖跨回合的驻留/冷却/递归，而非只测一次扫描。

## 本轮验证范围

本轮没有修改业务源码、运行中的用户数据、实际角色卡、聊天、设置或密钥，也没有更新 Docker 部署。新增本报告；诊断程序和合成数据位于系统临时目录。

实际执行的隔离诊断使用当前源码/path dependencies，未替代被检查的解析器或生成器：

| 检查 | 结果 |
| --- | --- |
| ST 世界书 camelCase 非默认值 | 复现 typed 字段回落默认值 |
| 世界书和内嵌书 `null` 定时字段、数字递归层级 | 复现解析失败 |
| 角色数值 talkativeness、V3 序列化 | 复现类型拒绝、顶层格式标记丢失 |
| ST 正则 JSON、`/foo/gi` | 复现空脚本、字面量未按 ST 执行 |
| 普通拼装的 system/PHI、跨文本变量和历史宏 | 复现覆盖缺失、上下文缺失 |
| 实际 SettingsSheet.patch 函数处理 Persona ID | 复现 `.png` 被拆为嵌套对象 |
| `GenerateSession.run` + 本机 HTTP mock + 合成聊天/世界书 | 确认聊天书进入请求；复现定时状态被覆盖、ST 全局书未激活、内嵌书无导入闭环、关闭流式仍发 stream=true |
| 标准非流式 OpenAI JSON 响应 | 复现 provider 返回空文本 |

其余结论来自接口、UI、模型、实际调用链及参考源码的交叉核对。没有逐家真实调用商业模型/TTS 服务，没有重跑完整浏览器功能矩阵，也没有给出缺乏分母的“对齐完成百分比”。TTS 的既有专项验证范围见 [TTS 文档][n-tts-doc]；本报告不把旧运行容器未更新计作源码能力缺失。

审计发现说明旧 `GAPS.md` 的部分“完整对齐”结论需要重新验收。后续修复可以按本报告编号登记，并在完成上述格式、行为、使用三层验证后再销项。

[n-rpc]: ../src/rpc.rs
[n-world-model]: ../crates/nast-model/src/world.rs
[n-world-store]: ../web/src/worldStore.ts
[n-storage]: ../crates/nast-storage/src/lib.rs
[n-generate]: ../src/generate.rs
[n-card-model]: ../crates/nast-model/src/card.rs
[n-cards]: ../crates/nast-cards/src/lib.rs
[n-world-panel]: ../web/src/components/inspector/WorldInfoPanel.tsx
[n-regex-model]: ../crates/nast-model/src/regex_script.rs
[n-regex-engine]: ../crates/nast-engine/src/regex_engine.rs
[n-regex-ui]: ../web/src/components/settings/sections/RegexPanel.tsx
[n-bridge]: ../src/prompt_bridge.rs
[n-macros]: ../crates/nast-engine/src/macros.rs
[n-prompt]: ../crates/nast-engine/src/prompt.rs
[n-wi-engine]: ../crates/nast-engine/src/world_info.rs
[n-group-gen]: ../src/group_gen.rs
[n-chat-model]: ../crates/nast-model/src/chat.rs
[n-providers]: ../crates/nast-providers/src/lib.rs
[n-ai-response]: ../web/src/components/settings/sections/AiResponsePanel.tsx
[n-persona-ui]: ../web/src/components/settings/sections/PersonaPanel.tsx
[n-settings-ui]: ../web/src/components/settings/SettingsSheet.tsx
[n-character-ui]: ../web/src/components/inspector/CharacterPanel.tsx
[n-sidebar]: ../web/src/components/layout/LeftSidebar.tsx
[n-world-editor]: ../web/src/WorldEditor.tsx
[n-world-global]: ../web/src/components/settings/sections/WorldInfoGlobalPanel.tsx
[n-chat-menu]: ../web/src/components/chat/ChatActionsMenu.tsx
[n-store]: ../web/src/store.ts
[n-group-ui]: ../web/src/components/chat/GroupChatArea.tsx
[n-connection]: ../src/connection.rs
[n-preset-model]: ../crates/nast-model/src/preset.rs
[n-prompt-ui]: ../web/src/components/settings/sections/PromptManagerPanel.tsx
[n-preset-ui]: ../web/src/components/settings/sections/PresetPanel.tsx
[n-tokens]: ../crates/nast-engine/src/tokens.rs
[n-commands]: ../web/src/commands.ts
[n-inspector]: ../web/src/components/inspector/RightInspector.tsx
[n-tts-doc]: ./src/guide/tts.md
[n-main]: ../src/main.rs
[n-plugin]: ../crates/nast-plugin/src/lib.rs
[n-events]: ../src/events.rs
[n-plugin-ui]: ../web/src/components/settings/sections/PluginsPanel.tsx
[n-state]: ../src/state.rs
[s-world]: ../refrence/SillyTavern/public/scripts/world-info.js
[s-settings]: ../refrence/SillyTavern/default/content/settings.json
[s-script]: ../refrence/SillyTavern/public/script.js
[s-characters]: ../refrence/SillyTavern/src/endpoints/characters.js
[s-regex]: ../refrence/SillyTavern/public/scripts/extensions/regex/engine.js
[s-prompt-manager]: ../refrence/SillyTavern/public/scripts/PromptManager.js
[s-macros]: ../refrence/SillyTavern/public/scripts/macros.js
[s-variables]: ../refrence/SillyTavern/public/scripts/variables.js
[s-group]: ../refrence/SillyTavern/public/scripts/group-chats.js
[s-personas]: ../refrence/SillyTavern/public/scripts/personas.js
[s-chats]: ../refrence/SillyTavern/src/endpoints/chats.js
[s-bookmarks]: ../refrence/SillyTavern/public/scripts/bookmarks.js
[s-openai]: ../refrence/SillyTavern/public/scripts/openai.js
[s-tools]: ../refrence/SillyTavern/public/scripts/tool-calling.js
[s-tokenizers]: ../refrence/SillyTavern/src/endpoints/tokenizers.js
[s-slash]: ../refrence/SillyTavern/public/scripts/slash-commands/SlashCommandParser.js
[s-qr]: ../refrence/SillyTavern/public/scripts/extensions/quick-reply/index.js
[s-memory]: ../refrence/SillyTavern/public/scripts/extensions/memory/index.js
[s-attachments]: ../refrence/SillyTavern/public/scripts/extensions/attachments/index.js
[s-vectors]: ../refrence/SillyTavern/public/scripts/extensions/vectors/index.js
[s-translate]: ../refrence/SillyTavern/public/scripts/extensions/translate/index.js
[s-caption]: ../refrence/SillyTavern/public/scripts/extensions/caption/index.js
[s-imagegen]: ../refrence/SillyTavern/public/scripts/extensions/stable-diffusion/index.js
[s-expressions]: ../refrence/SillyTavern/public/scripts/extensions/expressions/index.js
[s-speech]: ../refrence/SillyTavern/src/endpoints/speech.js
[s-search]: ../refrence/SillyTavern/src/endpoints/search.js
[s-plugin-loader]: ../refrence/SillyTavern/src/plugin-loader.js
[s-connections]: ../refrence/SillyTavern/public/scripts/extensions/connection-manager/index.js
[s-backups]: ../refrence/SillyTavern/src/endpoints/backups.js
[s-users]: ../refrence/SillyTavern/src/endpoints/users-private.js
