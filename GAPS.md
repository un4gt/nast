# nast vs SillyTavern 完成度差距（GAPS）

对照基准：refrence/SillyTavern release v1.18.0（HEAD 8172dcd0）。
状态标记：✅ 已对齐 | ⚠️ 有偏差 | ❌ 缺失。修复后请在对应条目标注 commit。

## 一、已完整对齐

- ✅ WI 扫描源接线（persona/char desc/personality/depth prompt/scenario，generate.rs:436-450）
- ✅ WI probability 掷骰语义（world_info.rs:437-442 = world-info.js:4922）
- ✅ WI 预算公式 round(%×context) + cap + sticky 优先
- ✅ WI 插入组 groupOverride/groupWeight；selectiveLogic 0-3
- ✅ WI @D 位置字符串（@D3 / @D2[a] / [r] / [s]）
- ✅ WI key 匹配主体（/regex/ 覆盖、整词、多词子串）
- ✅ continue nudge 尾部顺序（在 controlPrompts 之前、prompt 最末）
- ✅ squash 排除表 + 空消息丢弃
- ✅ 预算模型 reserve3/newMainChat 无条件/倒序断式填充/main 超限报错
- ✅ Claude 转换主链（前导 system 提取、mid system→user、连续合并、prefill trimEnd、空历史占位）
- ✅ swipe 弹出被重 roll 消息；用户消息先落盘；regenerate 先删；impersonate/quiet 不落盘
- ✅ swipe 保存（追加槽位/独立 send_date/swipe_info/setFirstSwipe 镜像）
- ✅ 停止保留半截文本；错误不写消息
- ✅ 后端逐 token 流式事件
- ✅ 聊天 jsonl header/integrity/备份节流/原子写
- ✅ PNG tEXt-only 解析（与 ST 一致）、ccv3 优先读取

## 二、有偏差（待修）

| # | 问题 | ST 行为 | nast 现状 | 位置 |
|---|---|---|---|---|
| D1 | WI after 块顺序 | before/after 都 unshift → 双升序 | after 用 push → 降序镜像 | world_info.rs:272 |
| D2 | min_activations 失效 | 每 pass 扫描深度+1，重扫新 buffer | skew 未用、buffer 不加深；深度上限误用 chat_length | world_info.rs:205-222 |
| D3 | sticky→cooldown 武装 | sticky 到期立即写同 horizon cooldown；protected 回滚（聊天未推进删非 protected 记录） | else-if 使 sticky+cooldown 共存条目永不进冷却；protected 存而不用 | world_info.rs:466-486 |
| D4 | timed hash 兼容 | getStringHash(JSON.stringify(entry)) | 自身 serde hash，ST 记录全部失配 | world_info.rs:663 |
| D5 | 预算累计制 | 累计 newContent+换行，`>=budget` 溢出 | 逐条 tok+1，`>budget` 溢出（多塞一条） | world_info.rs:249 |
| D6 | 装饰器解析过宽 | 仅内容以 @@ 开头才解析；@@@ 为字面 | 任意位置 @@ 行剥离、未知行丢弃 | world_info.rs:508 |
| D7 | WI 内容/key 预处理 | 内容过 WORLD_INFO 正则；key 过宏替换、/flags 解析 | 均不做 | world_info.rs:545 |
| D8 | EM 锚点丢弃 | position 5/6 注入示例区 | 收集后无下游 | generate.rs:475 |
| D9 | 示例对话解析 | 实际角色名前缀、续行合并、<START> 大小写不敏感 | 仅字面 {{user}}:/{{char}}:、丢续行 | generate.rs:661 |
| D10 | tokenizer 按 source | claude/llama 等近似器 | 拼装/WI 硬编码 o200k | prompt.rs:40 |
| D11 | Claude 次级 | 尊重 prompt_processing_type；空文本 \u200b；示例名字前缀 | 无条件合并；无处理；name 恒 None | providers/lib.rs:199 |
| D12 | 消息字段形状 | gen_started/gen_finished 顶层；token_count 可选 | 写在 extra；顶层字段透传缺失（原位改写丢字段） | generate.rs:208 |
| D13 | 卡导入 | 总写 chara+ccv3；写前剔除旧 tEXt | 永不写 ccv3；不剔旧 chunk（V3 编辑被旧 ccv3 遮蔽） | rpc.rs:144 |
| D14 | AN 注入链路 | AN 扩展槽/injectToMain | 前端发 in_chat_injections，后端从不读取 | rpc.rs:357 |

## 三、缺失

**WI**：per-entry scanDepth、matchCreatorNotes、characterFilter、delayUntilRecursion 多级、include_names、persona/global lorebooks（WiBooks 恒空）。

**Prompt**：injectToMain 相对注入（authorsNote/summary start/end）；bias/logit bias/CFG；COMPLETION names_behavior；工具调用；媒体内联。

**管线**：stopping strings 剥离；cleanUpMessage 引号/代码块平衡；reasoning 落盘；time_to_first_token；power_user 杂项。

**前端**：角色编辑 UI；头像 PNG 显示（全为首字母）；新聊天开场白（当前空白页！）；消息删除；checkpoints/分支；Prompt Manager UI；正则编辑器；聊天导出/重命名 UI（chats.rename 后端有但无 UI）；历史聊天点击 bug（恒选最新）；群聊前端；背景；persona 管理（名字固定 User）；instruct/context 模板；API key 配置 UI；markdown 渲染；**流式订阅（P1）**。

## 修复顺序

P1 流式订阅 → P2 AN 链路 → P3 问候消息 → P4 头像 → P5 消息删除+聊天管理 → P6 显示正则+markdown → P7 插件机制（mlua 运行时接入生成管线）→ P8 WI 四连（D1-D4）→ P9 示例解析（D9）→ P10 卡导入（D13）+ 字段形状（D12）+ tokenizer（D10）。其余偏差随里程碑消化。
