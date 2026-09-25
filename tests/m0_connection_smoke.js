// M0 连接闭环验收：UI 等价的 model_catalog.save 配置路由后即可生成（无环境变量）。
// 前置：target/debug/nast.exe 已构建；ws 模块来自 D:/temp/nast-smoke。
// 运行：node tests/m0_connection_smoke.js
const http = require('http');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');
const WebSocket = require(process.env.NAST_WS_MODULE || 'D:/temp/nast-smoke/node_modules/ws');

const MOCK_PORT = 19001;
const NAST_PORT = 18077;
const AVATAR = '陆八魔阿露.png';

let failures = 0;
function check(name, cond, extra = '') {
  if (cond) console.log(`  PASS ${name}`);
  else { failures++; console.error(`  FAIL ${name} ${extra}`); }
}

// ---------- mock OpenAI 兼容服务 ----------
let lastGen = null; // {path, auth, body}
const mock = http.createServer((req, res) => {
  if (req.method === 'GET' && req.url === '/v1/models') {
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify({ data: [{ id: 'mock-b' }, { id: 'mock-a' }] }));
    return;
  }
  if (req.method === 'POST' && req.url === '/v1/chat/completions') {
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      lastGen = { path: req.url, auth: req.headers['authorization'] || '', body: JSON.parse(body) };
      res.setHeader('content-type', 'text/event-stream');
      res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: '你好' } }] })}\n\n`);
      res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: '，世界' } }] })}\n\n`);
      res.write('data: [DONE]\n\n');
      res.end();
    });
    return;
  }
  res.statusCode = 404;
  res.end('{}');
});

// ---------- WS RPC 客户端 ----------
function connect(url) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url);
    const pending = new Map();
    let nextId = 1;
    ws.on('open', () => resolve({
      call: (method, params = {}) =>
        new Promise((res, rej) => {
          const id = String(nextId++);
          pending.set(id, { res, rej, method });
          ws.send(JSON.stringify({ id, method, params }));
        }),
      close: () => ws.close(),
    }));
    ws.on('message', (raw) => {
      const msg = JSON.parse(raw.toString());
      const p = pending.get(msg.id);
      if (!p) return;
      pending.delete(msg.id);
      if (msg.error) p.rej(new Error(`${msg.error.code}: ${msg.error.message}`));
      else p.res(msg.result);
    });
    ws.on('error', reject);
  });
}

async function main() {
  await new Promise((r) => mock.listen(MOCK_PORT, r));

  // 隔离数据目录：拷贝一张现成角色卡
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'nast-m0-'));
  fs.mkdirSync(path.join(tmp, 'default-user', 'characters'), { recursive: true });
  fs.copyFileSync(
    path.join(__dirname, '..', 'data', 'default-user', 'characters', AVATAR),
    path.join(tmp, 'default-user', 'characters', AVATAR),
  );

  // 确保没有 OPENAI_API_KEY / NAST_OPENAI_BASE 干扰（模拟"纯 UI 配置"场景）
  const child = spawn(path.join(__dirname, '..', 'target', 'debug', 'nast.exe'), [], {
    env: {
      ...process.env,
      NAST_PORT: String(NAST_PORT),
      NAST_DATA: tmp,
      NAST_WEB: path.join(__dirname, '..', 'web', 'dist'),
      OPENAI_API_KEY: '',
      NAST_OPENAI_BASE: '',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stderr.on('data', (d) => process.env.M0_VERBOSE && process.stderr.write(d));

  let rpc;
  for (let i = 0; i < 50; i++) {
    try { rpc = await connect(`ws://127.0.0.1:${NAST_PORT}/ws`); break; }
    catch { await new Promise((r) => setTimeout(r, 200)); }
  }
  if (!rpc) { console.error('FAIL 无法连接服务端'); process.exit(1); }

  try {
    // 1. UI 语义的配置：独立模型目录 + settings.save + 旧密钥接口兼容（无任何环境变量）
    await require('./model_catalog_helper')(rpc.call.bind(rpc), `http://127.0.0.1:${MOCK_PORT}/v1`, 'mock-a', 'sk-m0-test');
    await rpc.call('settings.save', {
      settings: {
        oai_settings: {
          chat_completion_source: 'custom',
          custom_url: `http://127.0.0.1:${MOCK_PORT}/v1`,
          custom_model: 'mock-a',
          openai_max_context: 4095,
          openai_max_tokens: 64,
        },
      },
    });
    await rpc.call('secrets.set', { key: 'api_key_custom', value: 'sk-m0-test' });

    // 2. 密钥掩码：值不下发
    const secrets = await rpc.call('secrets.get');
    const entry = (secrets.api_key_custom ?? [])[0];
    check('secrets.get 返回掩码且不含原值', entry?.active && entry?.masked === '••••test' && JSON.stringify(secrets).indexOf('sk-m0-test') === -1, JSON.stringify(secrets));

    // 3. 模型列表（走已保存配置）
    const models = await rpc.call('models.list', {});
    check('models.list 使用已保存 URL/key', JSON.stringify(models.data) === JSON.stringify(['mock-a', 'mock-b']), JSON.stringify(models));
    const models2 = await rpc.call('models.list', { url: `http://127.0.0.1:${MOCK_PORT}/v1`, key: 'sk-override' });
    check('models.list 支持临时 url/key 覆盖', Array.isArray(models2.data) && models2.data.length === 2);

    // 4. 生成：custom_model + Bearer key + 正确路径
    const chars = await rpc.call('characters.all');
    check('角色可见', chars.some((c) => c.avatar === AVATAR));
    const created = await rpc.call('chats.new', { avatar: AVATAR, greeting_index: 0 });
    const gen = await rpc.call('generate.run', {
      avatar: AVATAR,
      chat_file: created.file_name,
      type: 'normal',
      user_message: '打个招呼',
    });
    check('生成文本正确', gen.text === '你好，世界', JSON.stringify(gen));
    check('生成已落盘', gen.saved === true);
    check('请求打在 /v1/chat/completions', lastGen?.path === '/v1/chat/completions');
    check('使用 custom_model', lastGen?.body?.model === 'mock-a', JSON.stringify(lastGen?.body?.model));
    check('Bearer 使用 secrets 密钥', lastGen?.auth === 'Bearer sk-m0-test', lastGen?.auth);

    // 5. 落盘验证：jsonl 里有用户消息和 AI 消息
    const chat = await rpc.call('chats.get', { avatar: AVATAR, file_name: created.file_name });
    const msgs = chat.slice(1);
    check('聊天包含开场白+用户+AI 三条', msgs.length === 3 && msgs[1].is_user === true && msgs[2].mes === '你好，世界', String(msgs.length));
  } catch (e) {
    failures++;
    console.error('FAIL 异常:', e.message);
  } finally {
    rpc.close();
    child.kill();
    mock.close();
    fs.rmSync(tmp, { recursive: true, force: true });
  }

  console.log(failures === 0 ? '\nM0 冒烟全部通过' : `\n${failures} 项失败`);
  process.exit(failures === 0 ? 0 : 1);
}

main();
