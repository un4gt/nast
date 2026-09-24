# 聊天操作

## 基础操作

| 操作 | 入口 |
| --- | --- |
| 发送 | Enter（Shift+Enter 换行）；未发出的草稿按聊天持久化，切聊天不丢 |
| 停止 | 生成中红色停止按钮；**已流出的半截文本会保留落盘**（ST 语义） |
| Swipe | 底部 ←/→；右箭头在末位时生成新 swipe；每 swipe 独立 send_date |
| 重生成 | 删除最后一条 AI 消息后重新生成 |
| 续写 | nudge 模式（默认）在被续消息后追加 |
| 代入 | 以用户身份生成发言，结果填入输入框（不落盘） |
| 编辑 | 双击消息或悬停铅笔；**只更新当前 swipe**，编辑后重跑 runOnEdit 正则 |
| 删除 | 悬停垃圾桶；复制按钮一并悬停显示 |

输入框草稿、inspector 折叠状态、主题、字号/宽度均为本地持久化。

## Author's Note（作者注释）

Inspector → Author Note，**按聊天**保存在 `chat_metadata.note_*`：

- 内容 + 三个位置：主提示之前 / 主提示之后 / 聊天内 @ 深度
- 深度模式下可调注入深度（0–16）与角色（System/User/Assistant）
- **插入频率**：每 N 条用户消息插入一次（N=1 恒插）
- 世界书 ANTop/ANBottom 条目会自动并入 AN 文本上下方
- 角色卡自带 note（`extension_settings.note.chara`）按 替换/前置/后缀 合并

## Persona（用户人设）

设置 → 用户 / Persona：多 persona 管理（名称 + 描述 + 注入位置 + 默认）。

- 注入位置五种：提示词中（personaDescription 标记位）/ AN 顶部 / AN 底部 / 按深度注入 / 不注入
- **每聊天绑定**：面板顶部「当前聊天绑定」写 `chat_metadata.persona`，
  不绑则用默认 persona；用户消息署名随之切换
- persona 可绑定专属世界书（第 4 个 WI 来源）

## Reasoning（思考链）

推理模型（R1 / o 系 / OpenRouter reasoning）的思考过程：

- 流式期间折叠显示「思考中…」；落盘后折叠条带秒数
- 存于 `extra.reasoning`（+ duration/type），每 swipe 独立
- `auto_parse` 自动剥离正文中的 `<think>…</think>` 到 reasoning

## 消息时间与 token

每条消息显示时间戳；开启 token 计数后 `extra.token_count` 记录
（推理 + 正文）；底部计量条 = 会话总 token / (max_context − max_tokens)。
