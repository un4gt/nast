# 插件系统（Lua）

服务端运行（非浏览器端），`plugins/*.lua`，重载即生效（设置 → 插件 → 重载）。

## 隔离与安全

- 每插件独立 Lua VM，全部执行固定在**专用 OS 线程**
- 每次派发限时（`NAST_PLUGIN_TIMEOUT_SECS`，默认 10s）——**死循环插件不阻塞生成**
- 插件互相隔离；KV 以 `plugin:<名称>:` 命名空间落盘

## API

```lua
nast.on(event, fn)                 -- 注册钩子（可多次）
nast.register_command(name, fn)    -- 斜杠命令：fn(args) -> string|nil
nast.get_var(key) / nast.set_var(key, value)  -- KV（plugin_vars.json 持久化）
nast.toast(message, type)          -- 前端 toast（"info"|"success"|"error"）
nast.log(...)                      -- 服务端日志
nast.json_decode(s) / nast.json_encode(v)     -- 结构化钩子用
```

## 钩子事件

| 事件 | 时机 | 返回字符串 = |
| --- | --- | --- |
| `generation_started` | 生成开始（含群） | — |
| `user_input` | 用户消息落盘前 | 改写用户消息 |
| `prompt_built` | 拼装完成后 | `{messages={{role=,content=},…}}` 整体重写提示 |
| `ai_output` | 流式结束后、清理前 | 改写 AI 回复 |
| `message_saved` | 消息落盘后 | — |
| `generation_ended` | 生成结束（含群） | — |

参数以 **JSON 文本**传入钩子，用 `nast.json_decode` 解析。

## 示例

```lua
-- plugins/my.lua
nast.on("user_input", function(dataJson)
  local d = nast.json_decode(dataJson)
  if string.find(d.text, "密码") then
    return string.gsub(d.text, "密码", "***")
  end
  return nil  -- nil = 不改写
end)

nast.on("prompt_built", function(dataJson)
  local d = nast.json_decode(dataJson)
  table.insert(d.messages, {role = "system", content = "保持回复简短。"})
  return nast.json_encode({messages = d.messages})
end)

nast.register_command("ping", function(args) return "pong " .. args end)
```

完整示例见仓库 `plugins/upper_echo.lua`。

## 调试

`docker compose logs -f nast`（或本机 `cargo run` 终端）看 `[plugin:名称]` 日志；
语法错误在加载时打印并跳过该插件；超时会在服务端日志出现
`dispatch '事件' timed out`。
