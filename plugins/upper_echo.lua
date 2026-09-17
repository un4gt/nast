-- 示例插件（服务端 Lua）：
-- - user_input 转换钩子：给用户消息加前缀
-- - 斜杠命令 /upper <text>：转大写并作为消息发送；/plugin-stats 演示 KV 与 toast
-- - prompt_built：只读记录（返回 nil 不改写）
nast.on("user_input", function(dataJson)
  local text = string.match(dataJson, '"text":"([^"]*)"') or ""
  if text ~= "" then
    return "[plugin] " .. text
  end
  return nil
end)

nast.on("generation_started", function(dataJson)
  nast.log("generation started: " .. dataJson)
  return nil
end)

nast.on("prompt_built", function(dataJson)
  nast.log("prompt_built (readonly)")
  return nil
end)

nast.register_command("upper", function(args)
  if args == "" then
    return nil
  end
  return string.upper(args)
end)

nast.register_command("plugin-stats", function()
  local n = nast.get_var("runs") or 0
  nast.set_var("runs", n + 1)
  nast.toast("plugin runs: " .. (n + 1), "info")
  return nil
end)
