# nast vs SillyTavern 完成度差距（GAPS）

对照基准：refrence/SillyTavern release v1.18.0（HEAD 8172dcd0）。
状态标记：✅ 已对齐 | ⚠️ 有偏差 | ❌ 缺失。修复后请在对应条目标注 commit。

## 一、已完整对齐

**连接（M0）**
- ✅ secrets.json（ST 同构多 key 掩码）+ models.list 代理（临时 url/key 覆盖）
- ✅ custom_url/custom_model 消费（env 降级）；custom_include_headers/body 透传
- ✅ UI：baseURL / API key / 模型 Combobox（拉取列表）/ 连接测试

**聊天链路（M1）**
- ✅ WI 四源书（chat→persona→char(内嵌+charLore extraBooks)→global，strategy 0-2，去重）
- ✅ WI per-entry：scanDepth / caseSensitive / matchWholeWords / useGroupScoring、excludeRecursion、
  delayUntilRecursion 多级、characterFilter（names/tags/isExclude）、matchCreatorNotes
- ✅ WI 预算逐 pass 累计制 `>=budget` + ignoreBudget 计数穿透（D5）
- ✅ WI 内容输出期 WORLD_INFO 正则 + key/keysecondary 宏替换 + /pattern/flags（D7）
- ✅ WI position 0-7 全消费：ANTop/ANBottom 并入 AN、EM 锚点按示例解析、outlet {{outlet::key}}（D8）
- ✅ AN 服务端化：chat_metadata.note_*，interval 取模，位置 0/1/2，角色卡 note 合并，allowWIScan
- ✅ Persona 服务端化：多 persona + default + chat_metadata.persona 绑定；位置 0/2/3/4/9
- ✅ Reasoning：流捕获落盘（extra.reasoning/duration/type + swipe_info 镜像）、auto_parse、
  前端折叠展示（含流式）、time_to_first_token
- ✅ Stopping strings：请求 stop（上限 4）+ 尾部部分前缀剥离
- ✅ cleanUpMessage 管线 + 显示侧 fixMarkdown(true) 引号/星号补齐
- ✅ 消息编辑：按索引只更新当前 swipe + runOnEdit 正则 + tainted
- ✅ names_behavior COMPLETION（name 字段）

**前端（M2）**
- ✅ 全组件库化（shadcn/sonner/dnd-kit）；主题明暗切换；移动端 inspector Sheet
- ✅ 聊天管理：重命名/删除/导出（jsonl/txt）；复制消息；输入草稿按聊天持久化；token 计量条
- ✅ 角色列表：头像图/搜索/收藏/新建/复制
- ✅ WI 激活 UI：global_select 多选 + charLore 编辑 + 聊天书绑定（chats.set_world）

**群聊（M3）**
- ✅ 建群/成员管理（静音/健谈度/增删）、激活策略 0-3、SWAP/APPEND 模式、触发单成员
- ✅ 群生成走插件钩子 + cleanUpMessage + persona 署名；groups.get_chat 初始化语义

**预设/PM/正则（M4）**
- ✅ OpenAI Settings/*.json 读写（ST 互通）+ 应用/另存/删除
- ✅ Prompt Manager：拖拽排序（prompt_order #100001）、启停、增删改（角色/内容/注入位/深度/顺序/forbid_overrides）
- ✅ 正则脚本编辑器：全局脚本全字段 + 试运行 + 启停

**插件（M5，服务端 Lua）**
- ✅ 专用线程 + 限时派发（死循环不阻塞生成）；宿主侧 nast.on/register_command（无样板）
- ✅ KV 持久化（plugin_vars.json，reload 存活）；斜杠命令（空结果吞消息）；toast；
  json_decode/encode；prompt_built 整体重写；message_saved；插件管理面板

**健壮性（M6）**
- ✅ WS 指数退避 + 抖动重连；generate.status 断线恢复流式气泡（进度镜像 + 轮询收尾）
- ✅ tokenizer 启动预热（消除首次生成 ~3s 延迟）
- ✅ 冒烟测试矩阵 m0-m6（连接/链路/群/预设正则/插件/断线），ws 模块路径可环境变量覆盖

## 二、有偏差（待修）

| # | 问题 | ST 行为 | nast 现状 | 位置 |
|---|---|---|---|---|
| D11 ⏸️(范围外) | Claude 次级 system 处理 | prompt_processing_type、空文本 \u200b、示例名字前缀 | 无条件合并 | providers/lib.rs |
| D15 | WI useGroupScoring 评分 | 按 key 命中数评分取最高 | 退化为 order 最高 | world_info.rs |
| D16 | cleanUpMessage 次要项 | 群 cleanGroupMessage、instruct 序列裁剪 | 未实现（无对应路径时无行为差异） | generate.rs |
| D17 | reasoning 再注入 | add_to_prompts（prefix/suffix/max_additions） | 未实现（默认关闭） | generate.rs |
| D18 | bias/logit bias/CFG | assistant bias 注入 | ID_BIAS 恒空 | prompt.rs |
| D19 | 群聊 swipe/重生成 | 群消息可 swipe | 未接（仅单聊） | group_gen.rs |
| D20 | 群 auto-mode | 定时自动续聊 | 未实现 | 前端 |

## 三、明确范围外

文本补全路径（instruct/context 模板）、Claude/Gemini 原生源 UI 暴露（D11 顺延）、
向量/RAG、TTS、翻译、图像生成、多用户账号、浏览器端插件、checkpoints/分支。

## 里程碑完成记录

- M0 连接闭环 ✅ tests/m0_connection_smoke.js 10/10
- M1 聊天链路对齐 ✅ tests/m1_pipeline_smoke.js 20/20
- M2 前端组件化+管理 UI ✅
- M3 群聊前端+加固 ✅ tests/m3_group_smoke.js 12/12
- M4 预设/PM/正则 ✅ tests/m4_preset_regex_smoke.js 6/6
- M5 插件强化 ✅ tests/m5_plugin_smoke.js 8/8（含死循环超时隔离）
- M6 断线恢复+健壮性 ✅ tests/m6_reconnect_smoke.js 7/7
