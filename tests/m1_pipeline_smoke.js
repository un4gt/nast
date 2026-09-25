// M1 聊天链路一致性验收：对照 ST v1.18.0 语义。
// 覆盖：全局/辅助/persona 世界书接线、AN 服务端化（interval/ANTop 合并/深度注入）、
// EM 锚点、outlet 宏、reasoning 落盘、停止串（请求 stop + 输出剥离）、
// 消息编辑（当前 swipe 语义）、names_behavior COMPLETION。
// 运行：node tests/m1_pipeline_smoke.js
const http = require('http');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');
const WebSocket = require(process.env.NAST_WS_MODULE || 'D:/temp/nast-smoke/node_modules/ws');

const MOCK_PORT = 19011;
const NAST_PORT = 18079;
let lastGen = null;
let failures = 0;
function check(name, cond, extra = '') {
  if (cond) console.log(`  PASS ${name}`);
  else { failures++; console.error(`  FAIL ${name} ${extra}`); }
}

// ---------- mock：SSE 回复（先 reasoning_content 再 content，正文尾带停止串前缀） ----------
const mock = http.createServer((req, res) => {
  if (req.method === 'POST' && req.url === '/v1/chat/completions') {
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      lastGen = { path: req.url, auth: req.headers['authorization'] || '', body: JSON.parse(body) };
      res.setHeader('content-type', 'text/event-stream');
      res.write(`data: ${JSON.stringify({ choices: [{ delta: { reasoning_content: 'thinking hard' } }] })}\n\n`);
      res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: 'Reply text.STO' } }] })}\n\n`);
      res.write('data: [DONE]\n\n');
      res.end();
    });
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
          pending.set(id, { res, rej, method });
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

function cardJson() {
  return JSON.stringify({
    spec: 'chara_card_v2',
    spec_version: '2.0',
    data: {
      name: 'TestChar',
      description: 'DESC-FIELD',
      personality: '',
      scenario: '',
      first_mes: 'Hello there',
      mes_example: 'How TestChar talks:\n<START>\nUser: yo\nTestChar: hey\n{{outlet::slot}}',
      system_prompt: '',
      post_history_instructions: '',
      tags: ['t-tag'],
    },
  });
}

function book(entries) {
  const out = { entries: {} };
  for (const e of entries) out.entries[String(e.uid)] = e;
  return out;
}
const entry = (uid, key, content, extra = {}) => ({
  uid, key, keysecondary: [], comment: '', content, constant: false,
  selective: true, selectiveLogic: 0, order: 100, position: 0, disable: false,
  probability: 100, useProbability: true, depth: 4, role: 0,
  sticky: 0, cooldown: 0, delay: 0, excludeRecursion: false, preventRecursion: false,
  delayUntilRecursion: false, ...extra,
});

async function main() {
  await new Promise((r) => mock.listen(MOCK_PORT, r));
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'nast-m1-'));
  fs.mkdirSync(path.join(tmp, 'default-user', 'characters'), { recursive: true });

  const child = spawn(path.join(__dirname, '..', 'target', 'debug', 'nast.exe'), [], {
    env: {
      ...process.env,
      NAST_PORT: String(NAST_PORT),
      NAST_DATA: tmp,
      OPENAI_API_KEY: '',
      NAST_OPENAI_BASE: '',
    },
    cwd: tmp, // 隔离 plugins/（不加载示例插件）
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stderr.on('data', (d) => process.env.M1_VERBOSE && process.stderr.write(d));

  let rpc;
  for (let i = 0; i < 50; i++) {
    try { rpc = await connect(`ws://127.0.0.1:${NAST_PORT}/ws`); break; }
    catch { await new Promise((r) => setTimeout(r, 200)); }
  }
  if (!rpc) { console.error('FAIL 无法连接服务端'); process.exit(1); }

  let avatar = null;
  let chatFile = null;
  try {
    // 1. 世界书：全局（含 ANTop/EM/outlet 条目）+ 角色辅助 + persona 书
    await rpc.call('worlds.save', { name: 'GB', book: book([
      entry(0, ['dragon'], 'GB-CONTENT'),
      entry(1, ['dragon'], 'AN-TOP-CONTENT', { position: 2 }),
      entry(2, ['dragon'], 'EM-WI:\n<START>\nUser: em-q\nTestChar: em-a', { position: 5 }),
      entry(3, ['dragon'], 'OUTLET-CONTENT', { position: 7, outletName: 'slot' }),
    ]) });
    await rpc.call('worlds.save', { name: 'AUX', book: book([entry(0, ['dragon'], 'AUX-CONTENT')]) });
    await rpc.call('worlds.save', { name: 'PB', book: book([entry(0, ['dragon'], 'PB-CONTENT')]) });

    // 2. settings：连接 + WI 激活 + persona + 停止串 + names_behavior
    await require('./model_catalog_helper')(rpc.call.bind(rpc), `http://127.0.0.1:${MOCK_PORT}/v1`, 'mock-a');
    await rpc.call('settings.save', {
      settings: {
        oai_settings: {
          chat_completion_source: 'custom',
          custom_url: `http://127.0.0.1:${MOCK_PORT}/v1`,
          custom_model: 'mock-a',
          openai_max_context: 4095,
          openai_max_tokens: 256,
          character_names_behavior: 1, // COMPLETION：历史带 name 字段
        },
        world_info: {
          world_info_depth: 2,
          global_select: ['GB'],
          char_lore: [{ name: 'TestChar', extraBooks: ['AUX'] }],
        },
        power_user: {
          username: 'Tester',
          personas: { 'user-default.png': 'Tester' },
          default_persona: 'user-default.png',
          persona_descriptions: {
            'user-default.png': {
              description: 'A brave tester.',
              position: 0, // IN_PROMPT marker
              depth: 2,
              role: 0,
              lorebook: 'PB',
            },
          },
          custom_stopping_strings: '["STO"]',
        },
      },
    });

    // 3. 导入 JSON 卡（带 {{outlet::slot}} 示例）
    const imp = await rpc.call('characters.import', {
      data_base64: Buffer.from(cardJson()).toString('base64'),
    });
    avatar = imp.avatar;
    check('JSON 卡导入', imp.name === 'TestChar');

    // 4. 新聊天 + AN（in-chat depth 4）
    const created = await rpc.call('chats.new', { avatar, greeting_index: 0 });
    chatFile = created.file_name;
    await rpc.call('chats.set_note', {
      avatar, file_name: chatFile,
      note: { prompt: 'AN-CONTENT', interval: 1, position: 1, depth: 4, role: 0 },
    });

    // 5. 生成
    const gen = await rpc.call('generate.run', {
      avatar, chat_file: chatFile, type: 'normal', user_message: 'a dragon appears',
    });
    check('停止串剥离尾部前缀', gen.text === 'Reply text.', JSON.stringify(gen.text));

    const msgs = lastGen?.body?.messages ?? [];
    const allText = msgs.map((m) => (typeof m.content === 'string' ? m.content : '')).join('\n');

    // 6. 世界书三源接线
    check('全局书激活', allText.includes('GB-CONTENT'));
    check('角色辅助书激活（charLore extraBooks）', allText.includes('AUX-CONTENT'));
    check('persona 世界书激活', allText.includes('PB-CONTENT'));
    // 7. AN：ANTop 并入 + in-chat 深度注入
    const anMsg = msgs.find((m) => typeof m.content === 'string' && m.content.includes('AN-CONTENT'));
    check('AN 注入（ANTop 合并 + 深度）', !!anMsg && anMsg.content.includes('AN-TOP-CONTENT') && anMsg.role === 'system',
      JSON.stringify(anMsg));
    // 8. persona marker
    check('persona IN_PROMPT marker', allText.includes('A brave tester.'));
    // 9. names_behavior COMPLETION：历史消息带 name 字段
    const namedUser = msgs.find((m) => m.name === 'Tester');
    check('COMPLETION names：用户消息带 name', !!namedUser, JSON.stringify(msgs.map((m) => m.name)));
    // 10. EM 锚点 + outlet
    check('EM 锚点进示例区', allText.includes('em-q') && allText.includes('em-a'));
    check('outlet 宏替换', allText.includes('OUTLET-CONTENT') && !allText.includes('{{outlet::slot}}'));
    // 11. 停止串进请求
    check('停止串进请求 stop', JSON.stringify(lastGen.body.stop) === '["STO"]', JSON.stringify(lastGen.body.stop));
    // 12. 角色卡字段
    check('角色描述进 prompt', allText.includes('DESC-FIELD'));

    // 13. reasoning 落盘
    const chat = await rpc.call('chats.get', { avatar, file_name: chatFile });
    const lastMsg = chat[chat.length - 1];
    check('reasoning 落盘 extra.reasoning', lastMsg.extra?.reasoning === 'thinking hard', JSON.stringify(lastMsg.extra?.reasoning));
    check('reasoning_duration 记录', typeof lastMsg.extra?.reasoning_duration === 'number');
    check('swipe_info 镜像 reasoning', lastMsg.swipe_info?.[lastMsg.swipe_id]?.extra?.reasoning === 'thinking hard');
    check('time_to_first_token 记录', typeof lastMsg.extra?.time_to_first_token === 'number');

    // 14. swipe 生成 + 编辑语义（只改当前 swipe）
    await rpc.call('generate.run', { avatar, chat_file: chatFile, type: 'swipe' });
    const chat2 = await rpc.call('chats.get', { avatar, file_name: chatFile });
    // 数组含 header：最后一条（AI）= chat2.length-1；update_message 的消息索引 = chat2.length-2
    const aiArrIdx = chat2.length - 1;
    const aiMsgIdx = chat2.length - 2;
    const before = chat2[aiArrIdx];
    check('swipe 追加槽位', before.swipes?.length === 2 && before.swipe_id === 1,
      JSON.stringify({ len: before.swipes?.length, id: before.swipe_id }));
    await rpc.call('chats.update_message', {
      avatar, file_name: chatFile, index: aiMsgIdx, text: 'EDITED-TEXT',
    });
    const chat3 = await rpc.call('chats.get', { avatar, file_name: chatFile });
    const edited = chat3[aiArrIdx];
    check('编辑更新 mes', edited.mes === 'EDITED-TEXT', JSON.stringify(edited.mes));
    check('编辑同步当前 swipe', edited.swipes[edited.swipe_id] === 'EDITED-TEXT');
    check('编辑置 tainted', chat3[0].chat_metadata?.tainted === true);
  } catch (e) {
    failures++;
    console.error('FAIL 异常:', e.message);
  } finally {
    rpc.close();
    child.kill();
    mock.close();
    fs.rmSync(tmp, { recursive: true, force: true });
  }

  console.log(failures === 0 ? '\nM1 链路冒烟全部通过' : `\n${failures} 项失败`);
  process.exit(failures === 0 ? 0 : 1);
}

main();
