// nast WS RPC 冒烟测试
const WebSocket = require('D:/temp/nast-smoke/node_modules/ws');
const fs = require('fs');

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

const events = [];
ws.on('message', (raw) => {
  const msg = JSON.parse(raw);
  if (msg.event) { events.push(msg.event); return; }
  const p = pending.get(msg.id);
  if (!p) return;
  pending.delete(msg.id);
  if (msg.error) p.reject(new Error(`${msg.error.code}: ${msg.error.message}`));
  else p.resolve(msg.result);
});

ws.on('open', async () => {
  try {
    // 1. 导入真实 Seraphina PNG 卡
    const png = fs.readFileSync('E:/vibe_coding_workspace/nast/refrence/SillyTavern/default/content/default_Seraphina.png');
    const imported = await rpc('characters.import', {
      data_base64: png.toString('base64'),
    });
    console.log('IMPORT:', JSON.stringify(imported));

    // 2. 角色列表
    const all = await rpc('characters.all', {});
    console.log('CHARACTERS:', all.map(c => c.name).join(', '));

    // 3. 聊天列表 + 读聊天
    const chats = await rpc('characters.chats', { avatar: imported.avatar });
    console.log('CHATS:', JSON.stringify(chats));
    const chat = await rpc('chats.get', { avatar: imported.avatar, file_name: chats[0] });
    console.log('CHAT_LINES:', chat.length);

    // 4. 保存聊天（模拟追加一条消息）
    chat.push({ name: 'User', is_user: true, is_system: false, send_date: new Date().toISOString(), mes: 'hello Seraphina' });
    await rpc('chats.save', { avatar: imported.avatar, file_name: chats[0], chat });
    const chat2 = await rpc('chats.get', { avatar: imported.avatar, file_name: chats[0] });
    console.log('AFTER_SAVE_LINES:', chat2.length);

    // 5. 导入 Eldoria 世界书
    const book = fs.readFileSync('E:/vibe_coding_workspace/nast/crates/nast-model/tests/fixtures/world_eldoria.json');
    await rpc('worlds.save', { name: 'Eldoria', book: JSON.parse(book) });
    console.log('WORLDS:', JSON.stringify(await rpc('worlds.list', {})));

    // 6. settings save/get
    await rpc('settings.save', { settings: { oai_settings: { openai_max_context: 8192 } } });
    const s = await rpc('settings.get', {});
    console.log('SETTINGS_CTX:', s.oai_settings.openai_max_context);

    // 7. 错误路径：未知方法
    try { await rpc('nope.nothing', {}); } catch (e) { console.log('ERR_OK:', e.message); }

    console.log('EVENTS_SEEN:', JSON.stringify(events));
    console.log('SMOKE PASS');
    process.exit(0);
  } catch (e) {
    console.error('SMOKE FAIL:', e.message);
    process.exit(1);
  }
});
ws.on('error', (e) => { console.error('WS ERROR', e.message); process.exit(1); });
