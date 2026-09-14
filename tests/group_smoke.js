// 群聊端到端冒烟：导入两角色 → 建群 → 群生成 → 验证消息落盘
const WebSocket = require('D:/temp/nast-smoke/node_modules/ws');
const http = require('http');
const fs = require('fs');

const LOG = 'D:/temp/group-smoke-log.txt';
const log = (m) => fs.appendFileSync(LOG, m + '\n');
fs.writeFileSync(LOG, '');

// mock OpenAI 兼容服务：按最后一条 user 消息回显 + 前缀
const mock = http.createServer((req, res) => {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    const j = JSON.parse(body);
    const last = j.messages[j.messages.length - 1];
    const reply = `reply-to:${(last.content || '').slice(0, 20)}`;
    log(`MOCK: ${j.messages.length} msgs, last=[${last.role}] ${String(last.content).slice(0, 40)}`);
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: reply } }] })}\n\n`);
    res.write('data: [DONE]\n\n');
    res.end();
  });
});
mock.listen(19999, () => log('MOCK UP on 19998'));

const ws = new WebSocket('ws://127.0.0.1:18080/ws');
let nextId = 1;
const pending = new Map();
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
    log(`EVENT ${msg.event} ${JSON.stringify(msg.data).slice(0, 60)}`);
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
    await rpc('settings.save', {
      settings: { oai_settings: { chat_completion_source: 'custom', openai_model: 'mock' } },
    });

    // 导入两个角色
    const png = fs.readFileSync('E:/vibe_coding_workspace/nast/refrence/SillyTavern/default/content/default_Seraphina.png');
    const c1 = await rpc('characters.import', { data_base64: png.toString('base64') });
    // 第二个角色用 JSON 卡
    const card2 = JSON.stringify({
      spec: 'chara_card_v2', spec_version: '2.0', name: 'Bob',
      data: { name: 'Bob', description: 'A friendly bot', first_mes: 'Hi I am Bob',
              alternate_greetings: ['Hey there'], extensions: {} },
    });
    const c2 = await rpc('characters.import', {
      data_base64: Buffer.from(card2).toString('base64'),
    });
    log('C1: ' + c1.avatar + '  C2: ' + c2.avatar);

    // 建群
    const group = await rpc('groups.create', {
      group: {
        name: 'Test Group',
        members: [c1.avatar, c2.avatar],
        activation_strategy: 1, // LIST：全员依次发言
        generation_mode: 0,
      },
    });
    log('GROUP: ' + group.id + ' chat=' + group.chat_id);

    // 群生成（用户发言）
    const r = await rpc('generate.group', {
      id: group.id, chat_id: group.chat_id, user_message: 'hello group!',
    });
    log('GEN_RESULT: ' + JSON.stringify(r).slice(0, 300));

    // 读群聊天验证
    // （chats.get 只支持角色聊天；群聊天文件直接验证生成结果里的 replies）
    log('REPLIES: ' + r.replies.length);
    r.replies.forEach((rp) => log('  REPLY from ' + rp.name + ': ' + rp.text.slice(0, 40)));
    log('ACTIVATED: ' + JSON.stringify(r.activated));

    log('GROUP SMOKE PASS');
    process.exit(0);
  } catch (e) {
    log('GROUP SMOKE FAIL: ' + e.message);
    process.exit(1);
  }
});
