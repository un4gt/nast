# 服务端群聊与 Lua

服务器从进程工作目录的 `plugins/*.lua` 加载插件。每个文件有独立 Lua 实例，插件按文件名排序，钩子按注册顺序串行执行。`plugins.reload` 重载，`plugins.list` 仅用于帮助和补全。网页、桥接端和直接 RPC 客户端无需查询清单即可执行命令。

## RPC

WebSocket `/ws` 请求格式为 `{"id":"request-1","method":"commands.execute","params":{...}}`，响应有 `result` 或 `error`。

```json
{
  "conversation": {"kind":"group","group_id":"群 ID","chat_id":"会话 ID"},
  "input":"/group-order reverse",
  "task_id":"客户端生成的唯一任务 ID"
}
```

`commands.execute` 和 `generate.group` 的 `user_message` 共用服务端解析。插件命令返回字符串时，该字符串进入一次输入处理和生成；返回 nil 时命令结束。命令异常返回 RPC 错误，未知命令返回 `status: "unknown"`，均不请求模型。内置命令优先于 Lua，Lua 优先于设置中的自定义文本命令。不支持 STScript 管道、闭包或 JavaScript 扩展运行时。

同一 `task_id`、方法和参数会等待正在运行的任务或重放结果；不同参数重用 ID 返回 conflict。缓存限 256 项，仅在当前服务器进程内有效；跨进程重启不要自动重试未确认任务，应先检查会话落盘状态。生成提交后不依赖 WebSocket 存续，`generate.status` 返回任务、会话、成员 avatar、队列和阶段；`generate.stop` 停止后不再启动下一成员。

`groups.auto_start({id,chat_id,max_turns?})` 在服务端定时触发群聊；`groups.auto_status` 查询，`groups.auto_stop` 停止后续轮次。`generate.stop` 同时停止自动对话及当前生成。

## Lua API

| API | 行为 |
| --- | --- |
| `nast.on(event, function(raw) ... end)` | raw 是 JSON 上下文字符串，兼容旧回调 |
| `nast.register_command(name, function(args, context) ... end)` | context 为 Lua table，旧单参数回调仍可用 |
| `nast.get_var(key)` / `nast.set_var(key,value)` | 插件文件名命名空间；JSON 兼容值写穿 `plugin_vars.json`，重启恢复 |
| `nast.json_encode(value)` / `nast.json_decode(text)` | JSON 转换，最大嵌套深度 64 |
| `nast.notify(message,type)` | 写日志并广播通用 notification；无客户端也立即完成 |
| `nast.toast(message,type)` | notify 的兼容入口，同时广播旧 toast 事件 |
| `nast.log(...)` | 服务端日志 |

每次 Lua 回调受 5,000,000 指令预算和 `NAST_PLUGIN_TIMEOUT_SECS`（默认 10 秒）期限限制；超时会实际打断 Lua，包括嵌套 pcall 死循环。每个实例内存上限 32MB。原生阻塞 I/O、os、package/require、dofile/loadfile、coroutine 不开放；这些不是浏览器扩展 API。异常记录在服务端日志，转换钩子继续使用最后有效结果并执行后续插件。

## 群钩子

共同上下文包含 `is_group`、`group_id`、`chat_id`、`conversation`、`task_id`、`batch_id`、`generation_type`、`trigger`；按阶段增加 `group`、`member`（avatar）、`queue`、`text`、`message`、`success`、`cancelled`。命令回调运行于生成批次创建之前，所以不含 batch_id。

| 事件 | 返回值 |
| --- | --- |
| generation_started / group_batch_started | 忽略；开始通知 |
| user_input | 字符串替换输入，nil 保持 |
| group_queue | 有序 avatar 数组；nil 保持，空数组无人生成 |
| group_member_started / group_member_ended | 忽略；成员生命周期 |
| prompt_built | `{messages={{role="system",content="..."},...}}` 改写提示 |
| ai_output | 字符串替换输出，nil 保持 |
| message_saved | 忽略；落盘消息及索引 |
| group_batch_ended / generation_ended | 忽略；成功、取消或失败结果 |

队列只接受当前群内、存在且未禁用的成员，不允许重复；无效返回保留前一有效队列。指定成员、swipe、continue 不进入队列改派。所有事件均由后端业务流程触发，无前端确认或回调。

示例见 `examples/plugins/group_context.lua`。复制后可用 Python websockets 直接提交上述 RPC；完整无网页运行脚本为 `tests/parity/group_headless.py`，覆盖加载、命令、队列、生成、落盘、重启 KV、死循环恢复和长测。

## 群命令与持久化接口

支持 `member-list/count/get/peek/add/remove/enable/disable/up/down`、ST 成员命令别名与零起始成员序号；同名角色使用 avatar 消歧。`member-get field=name|avatar|index|id` 返回单字段。`member-peek` 返回角色数据，由客户端决定如何展示。

生成命令：`trigger`、`gen`、`generate`、`member-trigger`、`regenerate`、`continue`、`impersonate/imp`、`quiet`。会话命令：`newchat`、`name/renamechat`、`go`、`world`、`persona`、`sys/narrator`、`branch`、`bookmark`、`parent`、`swipe`、`swipe-delete/delswipe/swipedel`、`del`、`model`。这些是 NAST 支持的命令子集，不代表完整 STScript 语法兼容。

`groups.history/new_chat/open_chat/rename_chat/delete_chat/import_chat/export_chat` 管理会话；`groups.set_metadata/set_world/set_persona/set_note` 管理绑定；`groups.add_message/update_message/delete_message/hide_message/swipe/delete_swipe` 管理消息；`groups.branch/parent/from_private` 管理分支与单聊复制转换。消息序号从零开始，不计 JSONL header。所有群会话写操作校验归属并与生成互斥。删除和整批重生成创建可恢复备份；多文件操作尚不是数据库级事务。

JSON/JSONL 的未知字段和扩展数据保留，但 ST JavaScript 扩展不会自动执行。重命名会同步更新群内分支和书签引用，并在修改前保存恢复快照；失败会尝试还原。
