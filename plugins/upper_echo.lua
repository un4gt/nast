-- 示例插件：把用户输入转为大写前缀演示（user_input 转换钩子）
nast.hooks = {}
function nast.on(event, fn)
  nast.hooks[event] = nast.hooks[event] or {}
  table.insert(nast.hooks[event], fn)
end

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
