-- Copy to <server working directory>/plugins/group_context.lua, then plugins.reload.
-- No browser, plugin discovery, DOM or frontend callback is needed.
nast.register_command('group-order', function(args, context)
    assert(context.is_group, 'This command requires a group conversation')
    assert(args == 'normal' or args == 'reverse' or args == 'empty', 'normal|reverse|empty')
    nast.set_var('order:' .. context.conversation.chat_id, args)
    nast.notify('Group order saved: ' .. args, 'info')
end)

nast.register_command('group-say', function(args, context)
    assert(context.is_group, 'This command requires a group conversation')
    return args -- Backend applies user_input and generates exactly once.
end)

nast.on('group_queue', function(raw)
    local context = nast.json_decode(raw)
    local mode = nast.get_var('order:' .. context.chat_id)
    if mode == 'empty' then return {} end
    if mode ~= 'reverse' then return nil end
    local queue = {}
    for i = #context.queue, 1, -1 do queue[#queue + 1] = context.queue[i] end
    return queue -- Ordered avatar identifiers, never display names.
end)

nast.on('user_input', function(raw)
    local context = nast.json_decode(raw)
    if context.is_group then return context.text .. '\n[让各成员保持自己的身份。]' end
end)

nast.on('ai_output', function(raw)
    local context = nast.json_decode(raw)
    if context.is_group then return context.text:gsub('%s+$', '') end
end)

nast.on('group_batch_ended', function(raw)
    local context = nast.json_decode(raw)
    nast.set_var('last_batch:' .. context.chat_id, {
        task_id = context.task_id,
        batch_id = context.batch_id,
        success = context.success,
        cancelled = context.cancelled,
    })
end)
