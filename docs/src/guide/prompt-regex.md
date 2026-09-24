# 预设 · Prompt Manager · 正则

## 预设（Presets）

设置 → 预设：

- 存储为 `data/<user>/OpenAI Settings/<名称>.json`，**与 ST 预设文件互通**
- 「应用」= 预设字段合并进当前 oai_settings 草稿（记得点设置保存）
- 「另存当前配置为预设」把整个草稿存为新预设

## Prompt Manager

设置 → Prompt Manager，编辑全局提示词顺序（`prompt_order` #100001）：

- **拖拽排序**、启停开关、marker 徽章（main/chatHistory/worldInfoBefore 等 12 个动态注入位）
- 自定义提示词：角色 / 内容（支持宏）/ 注入位置（相对 or 聊天内绝对深度）/ 深度 /
  注入角色 / injection_order / 禁止角色卡覆盖
- 角色卡 `system_prompt` / `post_history_instructions` 覆盖 main/jailbreak
  （prompt_order 里的开关和 forbid_overrides 均生效）

## 正则脚本

设置 → 正则脚本，全局脚本（`extension_settings.regex`，与 ST 互通）：

| 字段 | 说明 |
| --- | --- |
| findRegex | **裸正则**（无斜杠；大小写不敏感用 `(?i)`） |
| replaceString | 支持 `$1`、`{{match}}`；空 = 删除匹配 |
| 作用位置 | 用户输入 / AI 输出 / 斜杠命令 / 世界书 / 推理 多选 |
| 仅显示 / 仅提示 | markdownOnly / promptOnly 双 pass |
| 编辑时运行 | runOnEdit（编辑消息持久化时也跑） |
| substituteRegex | 0 无 / 1 RAW 宏替换 / 2 ESCAPED 字面转义 |
| min/maxDepth | 距底部深度窗口 |
| trimStrings | 捕获组剔除子串 |

内置**试运行**区：粘贴样本文本即时看替换结果（客户端 JS 引擎，与后端 regex crate
行为一致，look-around 除外）。

作用域链：全局 → 角色内嵌（`data.extensions.regex_scripts`）→ 聊天级
（`chat_metadata.regex_scripts`）。角色内嵌脚本需要 `character_allowed_regex` 放行。
