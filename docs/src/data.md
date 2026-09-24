# 数据布局与 ST 兼容

## 目录结构

```
data/default-user/
├── settings.json          # 客户端设置 blob（oai_settings/power_user/world_info/extension_settings…）
├── secrets.json           # API 密钥（ST secrets.js 同构：{key: [{id,value,label,active}]}）
├── plugin_vars.json       # Lua 插件 KV（plugin:<名称>: 命名空间）
├── characters/*.png       # 角色卡（chara + ccv3 双写）
├── chats/<角色名>/<聊天>.jsonl
├── group chats/<chat_id>.jsonl
├── groups/<uuid>.json
├── worlds/<名称>.json
├── OpenAI Settings/<名称>.json    # 预设（ST 互通）
├── user/                  # User Avatars
└── backups/               # 聊天（10s 节流）/ settings（600s 节流）备份
```

## 与 SillyTavern 互通

| 内容 | 互通性 |
| --- | --- |
| 角色卡 PNG（chara/ccv3） | ✅ 双向；导入即双写，编辑剔除旧 chunk |
| JSON/YAML/YML/CHARX/BYAF | 支持导入；PNG/JSON 支持导出后交给 ST 再导入，复杂附件变体尚未穷举 |
| 世界书 `worlds/*.json` | ✅ 双向 |
| 预设 `OpenAI Settings/*.json` | ✅ 双向 |
| 聊天 jsonl（含 swipes/reasoning/timedWorldInfo） | 兼容基本布局及已覆盖候选/推理字段；活动世界书计时的 hash 算法有差异，不保证跨应用无缝继承 |
| 群组 / 群聊天 | ✅ 同布局（`group chats/` 平铺） |
| settings.json / secrets.json | ✅ 字段子集消费、其余 blob 透传 |

写保护：聊天保存校验 `chat_metadata.integrity` slug，不匹配即拒绝（force 除外）；
所有写盘 tmp+rename 原子化。

未知字段保留不代表对应扩展行为已执行。建议先复制 ST 数据目录再验证迁移；
具体已验证范围和限制见仓库 `docs/ST_PARITY_IMPLEMENTATION.md`。

## 备份与迁移

- 备份在 `backups/` 自动滚动，无需手动管理
- 整体迁移 = 停栈 → 拷贝 `data/` → 起栈（Docker 场景即迁移 named volume）
- 单聊天导出：消息菜单 → 导出 jsonl / txt
