// M5 插件验收：宿主侧 API（无样板）、斜杠命令、KV 落盘、prompt_built 重写、
// toast 事件、plugins.list 信息、死循环插件超时隔离。
// 运行：node tests/m5_plugin_smoke.js
const http = require('http');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');
const WebSocket = require(process.env.NAST_WS_MODULE || 'D:/temp/nast-smoke/node_modules/ws');

const MOCK_PORT = 19041;
const NAST_PORT = 18109;
let lastGen = null;
let failures = 0;
function check(name, cond, extra = '') {
  if (cond) console.log(`  PASS ${name}`);
  else { failures++; console.error(`  FAIL ${name} ${extra}`); }
}

const SMOKE_PLUGIN = `
nast.register_command("ping", function(args) return "pong " .. args end)
nast.register_command("quiet", function() return nil end)
nast.register_command("bump", function()
  local n = nast.get_var("runs") or 0
  nast.set_var("runs", n + 1)
  nast.toast("runs=" .. (n + 1), "info")
  return "RUNS=" .. (n + 1)
end)
nast.on("prompt_built", function(dataJson)
  local d = nast.json_decode(dataJson)
  table.insert(d.messages, {role = "system", content = "PLUGIN-PROMPT-MARK"})
  return nast.json_encode({messages = d.messages})
end)
`;

const LOOP_PLUGIN = `
nast.on("user_input", function() while true do end end)
`;

const mock = http.createServer((req, res) => {
  if (req.method === 'POST' && req.url === '/v1/chat/completions') {
    let b = ''; req.on('data', (c) => (b += c)); req.on('end', () => {
      lastGen = JSON.parse(b);
      res.setHeader('content-type', 'text/event-stream');
      res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: 'ok.' } }] })}\n\n`);
      res.write('data: [DONE]\n\n'); res.end();
    });
    return;
  }
  res.statusCode = 404; res.end('{}');
});

function connect(url) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url);
    const pending = new Map(); let nextId = 1;
    const events = [];
    ws.on('open', () => resolve({
      call: (method, params = {}) => new Promise((res, rej) => {
        const id = String(nextId++);
        pending.set(id, { res, rej });
        ws.send(JSON.stringify({ id, method, params }));
        setTimeout(() => { if (pending.has(id)) { pending.delete(id); rej(new Error('TIMEOUT ' + method)); } }, 30000);
      }),
      events,
      close: () => ws.close(),
    }));
    ws.on('message', (raw) => {
      const msg = JSON.parse(raw.toString());
      if (msg.event) { events.push(msg); return; }
      const p = pending.get(msg.id); if (!p) return;
      pending.delete(msg.id);
      if (msg.error) p.rej(new Error(`${msg.error.code}: ${msg.error.message}`)); else p.res(msg.result);
    });
    ws.on('error', reject);
  });
}

function cardJson() {
  return JSON.stringify({
    spec: 'chara_card_v2', spec_version: '2.0',
    data: { name: 'PlugChar', description: 'DESC', first_mes: 'Hi', mes_example: '', tags: [] },
  });
}

async function main() {
  await new Promise((r) => mock.listen(MOCK_PORT, r));
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'nast-m5-'));
  fs.mkdirSync(path.join(tmp, 'default-user', 'characters'), { recursive: true });
  fs.mkdirSync(path.join(tmp, 'plugins'), { recursive: true });
  fs.writeFileSync(path.join(tmp, 'plugins', 'smoke.lua'), SMOKE_PLUGIN);

  const child = spawn(path.join(__dirname, '..', 'target', 'debug', 'nast.exe'), [], {
    env: {
      ...process.env,
      NAST_USERNAME: '', NAST_PASSWORD: '', NAST_BRIDGE_TOKEN: '', NAST_PUBLIC_ORIGIN: '', NAST_ALLOW_ANONYMOUS: 'true', NAST_PORT: String(NAST_PORT),
      NAST_DATA: tmp,
      OPENAI_API_KEY: '',
      NAST_OPENAI_BASE: '',
      NAST_PLUGIN_TIMEOUT_SECS: '2',
    },
    cwd: tmp, stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stderr.on('data', (d) => process.env.M5_VERBOSE && process.stderr.write(d));
  let rpc;
  for (let i = 0; i < 50; i++) {
    try { rpc = await connect(`ws://127.0.0.1:${NAST_PORT}/ws`); break; }
    catch { await new Promise((r) => setTimeout(r, 200)); }
  }
  if (!rpc) { console.error('FAIL 无法连接服务端'); process.exit(1); }

  try {
    // 0. 插件清单
    const list = await rpc.call('plugins.list', {});
    const smoke = (list.plugins ?? []).find((p) => p.name === 'smoke');
    check('plugins.list 含钩子/命令信息', !!smoke && smoke.commands.includes('ping') && smoke.hooks.includes('prompt_built'),
      JSON.stringify(list));

    await require('./model_catalog_helper')(rpc.call.bind(rpc), `http://127.0.0.1:${MOCK_PORT}/v1`, 'mock-a');
    await rpc.call('settings.save', {
      settings: {
        oai_settings: {
          chat_completion_source: 'custom',
          custom_url: `http://127.0.0.1:${MOCK_PORT}/v1`,
          custom_model: 'mock-a',
          openai_max_context: 4095,
          openai_max_tokens: 128,
        },
      },
    });
    const imp = await rpc.call('characters.import', { data_base64: Buffer.from(cardJson()).toString('base64') });
    const created = await rpc.call('chats.new', { avatar: imp.avatar, greeting_index: 0 });

    // 1. prompt_built：请求含插件注入的 system 消息
    await rpc.call('generate.run', { avatar: imp.avatar, chat_file: created.file_name, type: 'normal', user_message: 'hi' });
    const texts = lastGen.messages.map((m) => m.content);
    check('prompt_built 重写生效', texts.includes('PLUGIN-PROMPT-MARK'), JSON.stringify(texts));

    // 2. 斜杠命令 /ping：替换用户消息
    await rpc.call('generate.run', { avatar: imp.avatar, chat_file: created.file_name, type: 'normal', user_message: '/ping hello' });
    let chat = await rpc.call('chats.get', { avatar: imp.avatar, file_name: created.file_name });
    let users = chat.filter((m) => m.is_user).map((m) => m.mes);
    check('/ping 命令替换消息', users.includes('pong hello'), JSON.stringify(users));

    // 3. /quiet：吞掉消息（不保存不生成）
    const before = (await rpc.call('chats.get', { avatar: imp.avatar, file_name: created.file_name })).length;
    const q = await rpc.call('generate.run', { avatar: imp.avatar, chat_file: created.file_name, type: 'normal', user_message: '/quiet' });
    const after = (await rpc.call('chats.get', { avatar: imp.avatar, file_name: created.file_name })).length;
    check('/quiet 吞掉消息', q.saved === false && before === after, `before=${before} after=${after}`);

    // 4. KV 落盘 + toast 事件
    await rpc.call('generate.run', { avatar: imp.avatar, chat_file: created.file_name, type: 'normal', user_message: '/bump' });
    chat = await rpc.call('chats.get', { avatar: imp.avatar, file_name: created.file_name });
    users = chat.filter((m) => m.is_user).map((m) => m.mes);
    check('/bump KV 读写', users.includes('RUNS=1'), JSON.stringify(users));
    const kvFile = path.join(tmp, 'default-user', 'plugin_vars.json');
    check('KV 文件落盘', fs.existsSync(kvFile) && fs.readFileSync(kvFile, 'utf8').includes('plugin:smoke:runs'));
    const toastEvents = rpc.events.filter((e) => e.event === 'toast');
    check('toast 事件广播', toastEvents.some((e) => String(e.data?.message).includes('runs=1')),
      JSON.stringify(toastEvents.map((e) => e.data?.message)));

    // 5. 死循环插件超时隔离：加入 loop 插件 → reload → 生成仍能完成（2s 超时放行）
    fs.writeFileSync(path.join(tmp, 'plugins', 'loop.lua'), LOOP_PLUGIN);
    await rpc.call('plugins.reload', {});
    const t0 = Date.now();
    const gen = await rpc.call('generate.run', { avatar: imp.avatar, chat_file: created.file_name, type: 'normal', user_message: 'still alive' });
    const elapsed = Date.now() - t0;
    check('死循环插件超时放行', gen.saved === true && elapsed < 20000, `elapsed=${elapsed}ms saved=${gen.saved}`);
  } catch (e) {
    failures++;
    console.error('FAIL 异常:', e.message);
  } finally {
    rpc.close(); child.kill(); mock.close();
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  console.log(failures === 0 ? '\nM5 插件冒烟全部通过' : `\n${failures} 项失败`);
  process.exit(failures === 0 ? 0 : 1);
}

main();
