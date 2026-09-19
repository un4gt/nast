# 按 frontend-evaluation.md 修复（4 项 Action Items）

范围：群组功能补齐（消息微操 + 流式）、store 异常处理与输入回填、侧边栏入口清理、连接面板密钥提示。部署仅重建/重启 **nast** 容器（qqbot 不动，凭据 volume 不碰，无需扫码）。

## A. 后端（nast-server，支撑群聊 UI）

1. **`groups.update_message` / `groups.delete_message` RPC**（rpc.rs）：
   - update：按索引改 `mes`（若消息带 swipes 同步当前槽），用该消息 `original_avatar` 对应角色卡跑 runOnEdit 正则，`chat_metadata.tainted=true`；
   - delete：删除指定消息。两者作用于 `group chats/<chat_id>.jsonl`。
2. **群生成改流式**（group_gen.rs）：把成员生成的 `call_provider` 换成流式接收——emit `stream_token_received` / `stream_reasoning_received`，逐成员累积文本后再走既有 cleanUpMessage 落盘。前端凭已有的 `group_member_drafted`（成员开始）+ token 事件（累积）即可渲染逐字气泡。

## B. 前端

3. **store.ts 吞错修复 + 输入回填**：`send / swipe / regenerate / continueGen / sendGroup` 全部加 catch——失败时 best-effort `reloadChat()`（防状态卡死）并返回 `false`；ChatArea / GroupChatArea 在 send 失败时把用户输入回填输入框（不丢字）。
4. **GroupChatArea 补齐**：
   - **统一消息组件**：GroupMessage 改用 MessageBubble（悬停复制/编辑/删除、双击编辑、reasoning 折叠）；MessageBubble 增加可选 `group` 模式 props（编辑/删除走 `groups.*` RPC，头像已按 original_avatar 解析）；
   - **流式渲染**：generating 期间订阅 `group_member_drafted`（切换成员名、清空缓冲）+ token/reasoning 事件，用 StreamingBubble 逐字显示；
   - **底部操作**：补“重生成”（删除最后一条群消息 + 触发一次无用户消息的群生成）。swipe/续写/代入需要服务端群语义支持，本轮不做（完成后在总结中说明）。
5. **LeftSidebar**：删除底部禁用的“Groups（即将支持）”按钮，群组入口统一为群列表 + “新建群组”。
6. **ConnectionPanel**（对应报告第 3 点痛点，轻量）：密钥框下加一行说明——“获取模型列表/连接测试优先使用输入框中的密钥（未保存则仅本次生效）”，消除新旧密钥歧义。

## C. 验证与部署

- `npm run build` + `cargo test --workspace` + m0/m1/m3 冒烟（本地 target 二进制）；
- `docker compose build nast && docker compose up -d nast`——**只重建并重启 nast 服务**（镜像共享但 qqbot 容器不 recreate，扫码凭据在独立 volume，不受影响）；
- 验证 web 200 与 RPC 可用；群聊流式/编辑在网页端实际发消息确认；
- git commit。