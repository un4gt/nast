# 世界书

## 四个来源（按合并顺序）

| 来源 | 存放 | 绑定入口 |
| --- | --- | --- |
| 聊天书 | `chat_metadata.world` | Inspector → World Info → 本聊天绑定 |
| Persona 书 | persona 绑定的 lorebook | 设置 → Persona |
| 角色书 | `data.extensions.world` 主书 + `world_info_settings.world_info.charLore[].extraBooks` 辅助书 | Inspector → World Info → 主书 / 角色辅助书 |
| 全局书 | `world_info_settings.world_info.globalSelect` | 设置 → 世界书全局 → 全局激活勾选 |

角色内嵌书在卡片的 `data.character_book` 中；导入时转换为独立世界书，
通过 `data.extensions.world` 关联主书。编辑或导出角色时同步当前书内容。
旧版 nast 的 `world_info.global_select` / `char_lore` 设置仍可读取；同名书只生效一次。
插入策略（角色优先/全局优先/均匀混排）在全局设置里选。

## 编辑器

Inspector → World Info → 打开世界书管理：

- 条目字段全覆盖：主/次级 key、selectiveLogic 0–3、position（0–7 含 @D 深度语法）、
  order/probability/sticky/cooldown/delay、constant、ignoreBudget、preventRecursion、
  scanDepth/caseSensitive/wholeWords（留空=跟全局）、characterFilter、match 扩展源
- 保存为 `worlds/<名称>.json`，**与 ST 世界书文件直接互换**

## 全局设置（设置 → 世界书全局）

扫描深度、预算（上下文 % + 硬上限）、最少激活条数、递归开关与步数、
大小写/全词/插入组评分、插入策略、全局激活列表。

预算按 ST v1.18 的累计规则实现：每 pass 累计 `content\n`，`(基线 + 累计) >= 预算` 溢出；
被预算丢弃的条目内容仍参与递归扫描；`ignoreBudget` 条目穿透预算。
精确预算边界、概率与互斥组等高级组合尚未全部纳入双端差分，详见仓库 `docs/ST_PARITY_IMPLEMENTATION.md`。

## position 0–7 速查

| 值 | 含义 |
| --- | --- |
| 0 / 1 | 角色描述前 / 后（WI 块） |
| 2 / 3 | AN 顶部 / 底部（并入 AN 文本） |
| 4 | 聊天内 @ 深度（支持 `@D3` / `@D2[a]` 字符串语法） |
| 5 / 6 | 示例对话区前 / 后锚点（按示例对话解析拼接） |
| 7 | outlet：内容替换示例对话中的 `{{outlet::名称}}` 宏 |

key 支持 `/pattern/flags` 正则形式（覆盖全局大小写/全词设置）；
条目内容在输出期会过 WORLD_INFO 位正则脚本。
