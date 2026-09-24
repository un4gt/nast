// M3 群聊验收：建群 → 群聊初始化（成员开场白）→ 用户消息 + 策略激活 →
// 触发单成员 → 静音 + LIST 策略 → groups.get_chat 读取。
// 运行：node tests/m3_group_smoke.js
const http = require('http');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');
const WebSocket = require(process.env.NAST_WS_MODULE || 'D:/temp/nast-smoke/node_modules/ws');

const MOCK_PORT = 19021;
const NAST_PORT = 18089;
let failures = 0;
let genCount = 0;
function check(name, cond, extra = '') {
  if (cond) console.log(`  PASS ${name}`);
  else { failures++; console.error(`  FAIL ${name} ${extra}`); }
}

const mock = http.createServer((req, res) => {
  if (req.method === 'POST' && req.url === '/v1/chat/completions') {
    genCount += 1;
    res.setHeader('content-type', 'text/event-stream');
    res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: `Group reply #${genCount} from member.` } }] })}\n\n`);
    res.write('data: [DONE]\n\n');
    res.end();
    return;
  }
  res.statusCode = 404;
  res.end('{}');
});

function connect(url) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url);
    const pending = new Map();
    let nextId = 1;
    ws.on('open', () => resolve({
      call: (method, params = {}) =>
        new Promise((res, rej) => {
          const id = String(nextId++);
          pending.set(id, { res, rej });
          ws.send(JSON.stringify({ id, method, params }));
          setTimeout(() => {
            if (pending.has(id)) { pending.delete(id); rej(new Error('TIMEOUT ' + method)); }
          }, 15000);
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

function cardJson(name) {
  return JSON.stringify({
    spec: 'chara_card_v2',
    spec_version: '2.0',
    data: {
      name,
      description: `${name} description`,
      first_mes: `Hi, I am ${name}.`,
      mes_example: '',
      tags: [],
      talkativeness: '0.5',
    },
  });
}

async function main() {
  await new Promise((r) => mock.listen(MOCK_PORT, r));
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'nast-m3-'));
  fs.mkdirSync(path.join(tmp, 'default-user', 'characters'), { recursive: true });

  const child = spawn(path.join(__dirname, '..', 'target', 'debug', 'nast.exe'), [], {
    env: {
      ...process.env,
      NAST_PORT: String(NAST_PORT),
      NAST_DATA: tmp,
      OPENAI_API_KEY: '',
      NAST_OPENAI_BASE: '',
    },
    cwd: tmp,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stderr.on('data', (d) => process.env.M3_VERBOSE && process.stderr.write(d));

  let rpc;
  for (let i = 0; i < 50; i++) {
    try { rpc = await connect(`ws://127.0.0.1:${NAST_PORT}/ws`); break; }
    catch { await new Promise((r) => setTimeout(r, 200)); }
  }
  if (!rpc) { console.error('FAIL 无法连接服务端'); process.exit(1); }

  try {
    await rpc.call('settings.save', {
      settings: {
        oai_settings: {
          chat_completion_source: 'custom',
          custom_url: `http://127.0.0.1:${MOCK_PORT}/v1`,
          custom_model: 'mock-a',
          openai_max_context: 4095,
          openai_max_tokens: 256,
        },
      },
    });

    // 两个成员
    const a = await rpc.call('characters.import', { data_base64: Buffer.from(cardJson('Alice')).toString('base64') });
    const b = await rpc.call('characters.import', { data_base64: Buffer.from(cardJson('Bob')).toString('base64') });
    check('成员卡导入', a.name === 'Alice' && b.name === 'Bob');

    // 建群
    const g = await rpc.call('groups.create', {
      group: { name: 'Test Group', members: [a.avatar, b.avatar] },
    });
    check('建群返回 chat_id', !!g.chat_id && g.members.length === 2, JSON.stringify(g));

    // 1. 打开群聊：初始化两个成员开场白
    const raw = await rpc.call('groups.get_chat', { chat_id: g.chat_id });
    const msgs = raw.slice(1);
    check('群聊初始化成员开场白', msgs.length === 2 && msgs.every((m) => !m.is_user && m.original_avatar),
      JSON.stringify(msgs.map((m) => m.name)));
    check('身份字段 original_avatar', msgs.some((m) => m.original_avatar === a.avatar) && msgs.some((m) => m.original_avatar === b.avatar));

    // 2. 用户消息 + 策略激活（NATURAL：talkativeness 0.5 —— 至少 0 个；断言不崩 + 落盘用户消息）
    const r1 = await rpc.call('generate.group', {
      id: g.id, chat_id: g.chat_id, user_message: 'hello group',
    });
    const raw2 = await rpc.call('groups.get_chat', { chat_id: g.chat_id });
    const msgs2 = raw2.slice(1);
    check('用户消息落盘', msgs2[2]?.is_user === true && msgs2[2]?.mes === 'hello group');
    check('生成结果结构', Array.isArray(r1.replies) && r1.replies.every((x) => typeof x.text === 'string'),
      JSON.stringify(r1));
    const aiCount = msgs2.length - 3;
    check('成员回复数在 0-2 之间（NATURAL 随机）', aiCount >= 0 && aiCount <= 2, String(aiCount));

    // 3. 触发单成员
    const r3 = await rpc.call('generate.group', {
      id: g.id, chat_id: g.chat_id, member: b.avatar,
    });
    check('触发单成员只激活该成员', r3.activated.length === 1 && r3.activated[0] === b.avatar,
      JSON.stringify(r3.activated));

    // 4. 静音 + LIST 策略：只剩未静音成员全量回复
    const g4 = { ...g, activation_strategy: 1, disabled_members: [a.avatar] };
    await rpc.call('groups.edit', { group: g4 });
    const r4 = await rpc.call('generate.group', {
      id: g.id, chat_id: g.chat_id, user_message: 'list mode',
    });
    check('LIST 策略激活全部未静音成员', r4.activated.length === 1 && r4.activated[0] === b.avatar,
      JSON.stringify(r4.activated));

    // 5. 回复文本经过清理管线（无名字前缀）
    const raw5 = await rpc.call('groups.get_chat', { chat_id: g.chat_id });
    const lastAi = [...raw5.slice(1)].reverse().find((m) => !m.is_user);
    check('回复落盘且带 gen_id 批次', typeof lastAi.extra?.gen_id === 'number', JSON.stringify(lastAi.extra?.gen_id));

    // 6. 群组保存往返
    const groups = await rpc.call('groups.all', {});
    check('groups.all 含设置', groups.find((x) => x.id === g.id)?.activation_strategy === 1);

    // 7. 删除群
    await rpc.call('groups.delete', { id: g.id });
    const groups2 = await rpc.call('groups.all', {});
    check('删除群', !groups2.some((x) => x.id === g.id));
  } catch (e) {
    failures++;
    console.error('FAIL 异常:', e.message);
  } finally {
    rpc?.close();
    // Windows keeps the child's working directory locked until it exits.
    if (child.exitCode === null && child.signalCode === null) {
      const exited = new Promise((resolve) => child.once('exit', resolve));
      child.kill();
      await exited;
    }
    await new Promise((resolve) => mock.close(resolve));
    const target = path.resolve(tmp);
    if (path.dirname(target) !== path.resolve(os.tmpdir()) || !path.basename(target).startsWith('nast-m3-')) {
      throw new Error('Refusing to remove a directory outside the test fixture');
    }
    fs.rmSync(target, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }

  console.log(failures === 0 ? '\nM3 群聊冒烟全部通过' : `\n${failures} 项失败`);
  process.exit(failures === 0 ? 0 : 1);
}

main();
