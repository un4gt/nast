// 生成链路端到端冒烟：mock OpenAI 兼容服务 + nast 生成状态机
const WebSocket = require('D:/temp/nast-smoke/node_modules/ws');
const http = require('http');
const fs = require('fs');

const LOG = 'D:/temp/gen-smoke-log.txt';
const log = (m) => fs.appendFileSync(LOG, m + '\n');
fs.writeFileSync(LOG, '');

// 1. mock OpenAI 兼容服务：流式回显 "echo: <最后一条 user 消息>" 分 5 个 token
const mock = http.createServer((req, res) => {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    const j = JSON.parse(body);
    const userMsg = [...j.messages].reverse().find((m) => m.role === 'user');
    const reply = `echo:${userMsg ? userMsg.content : 'none'}`;
    log(`MOCK got ${j.messages.length} msgs, stream=${j.stream}, model=${j.model}`);
    if (!j.stream) {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ choices: [{ message: { role: 'assistant', content: reply } }] }));
      return;
    }
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    const tokens = [];
    for (let i = 0; i < reply.length; i += 4) tokens.push(reply.slice(i, i + 4));
    tokens.forEach((t) => res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: t } }] })}\n\n`));
    res.write('data: [DONE]\n\n');
    res.end();
  });
});
mock.listen(19999, () => log('MOCK UP on 19999'));

// 2. WS 客户端驱动生成
const ws = new WebSocket('ws://127.0.0.1:18080/ws');
let nextId = 1;
const pending = new Map();
const streamTokens = [];
function rpc(method, params) {
  return new Promise((resolve, reject) => {
    const id = String(nextId++);
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });
}
ws.on('message', (raw) => {
  const msg = JSON.parse(raw);
  if (msg.event) {
    if (msg.event === 'stream_token_received') streamTokens.push(msg.data.text);
    log(`EVENT ${msg.event} ${msg.event === 'stream_token_received' ? '' : JSON.stringify(msg.data).slice(0, 80)}`);
    return;
  }
  const p = pending.get(msg.id);
  if (!p) return;
  pending.delete(msg.id);
  if (msg.error) p.reject(new Error(`${msg.error.code}: ${msg.error.message}`));
  else p.resolve(msg.result);
});
ws.on('error', (e) => { log('WS ERR ' + e.message); process.exit(1); });

ws.on('open', async () => {
  try {
    // 配置：指向 mock
    await rpc('settings.save', {
      settings: { oai_settings: { chat_completion_source: 'custom', openai_model: 'mock-model', openai_max_tokens: 100 } },
    });
    process.env.NAST_OPENAI_BASE = 'http://127.0.0.1:19999/v1';

    // 导入角色
    const png = fs.readFileSync('E:/vibe_coding_workspace/nast/refrence/SillyTavern/default/content/default_Seraphina.png');
    const imported = await rpc('characters.import', { data_base64: png.toString('base64') });
    log('IMPORT: ' + JSON.stringify(imported));

    // 找最新聊天文件
    const chats = await rpc('characters.chats', { avatar: imported.avatar });
    const chatFile = chats[chats.length - 1];
    log('CHAT: ' + chatFile);

    // normal 生成
    const r1 = await rpc('generate.run', {
      avatar: imported.avatar, chat_file: chatFile, type: 'normal', user_message: 'hello world',
    });
    log('GEN1: ' + JSON.stringify(r1).slice(0, 200));
    log('STREAM_TOKENS: ' + streamTokens.length + ' joined=' + streamTokens.join(''));

    // 读聊天验证落盘
    const chat = await rpc('chats.get', { avatar: imported.avatar, file_name: chatFile });
    const userMsg = chat.find((m) => m.is_user);
    const aiMsg = chat.filter((m) => !m.is_user && m.mes).pop();
    log('USER_SAVED: ' + (userMsg ? userMsg.mes : 'NONE'));
    log('AI_SAVED: ' + (aiMsg ? aiMsg.mes : 'NONE'));
    log('AI_HAS_SWIPES: ' + (aiMsg && Array.isArray(aiMsg.swipes) ? aiMsg.swipes.length : 'NO'));

    // swipe 生成
    const r2 = await rpc('generate.run', {
      avatar: imported.avatar, chat_file: chatFile, type: 'swipe',
    });
    log('GEN2_SWIPE: ' + JSON.stringify(r2).slice(0, 120));
    const chat2 = await rpc('chats.get', { avatar: imported.avatar, file_name: chatFile });
    const aiMsg2 = chat2.filter((m) => !m.is_user && m.mes).pop();
    log('SWIPE_SWIPES: ' + JSON.stringify(aiMsg2.swipes));
    log('SWIPE_ID: ' + aiMsg2.swipe_id);

    // impersonate（不落消息）
    const msgCountBefore = chat2.length;
    const r3 = await rpc('generate.run', {
      avatar: imported.avatar, chat_file: chatFile, type: 'impersonate',
    });
    const chat3 = await rpc('chats.get', { avatar: imported.avatar, file_name: chatFile });
    log('IMPERSONATE_TEXT: ' + r3.text.slice(0, 40));
    log('IMPERSONATE_SAVED: ' + r3.saved + ' CHAT_UNCHANGED: ' + (chat3.length === msgCountBefore));

    // regenerate
    const r4 = await rpc('generate.run', {
      avatar: imported.avatar, chat_file: chatFile, type: 'regenerate',
    });
    const chat4 = await rpc('chats.get', { avatar: imported.avatar, file_name: chatFile });
    log('REGEN_SAVED: ' + r4.saved + ' LINES: ' + chat4.length);

    log('GEN SMOKE PASS');
    process.exit(0);
  } catch (e) {
    log('GEN SMOKE FAIL: ' + e.message);
    process.exit(1);
  }
});
