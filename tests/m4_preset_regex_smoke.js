// M4 验收：预设 round-trip、全局正则脚本作用于生成、Prompt Manager 顺序生效。
// 运行：node tests/m4_preset_regex_smoke.js
const http = require('http');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');
const WebSocket = require(process.env.NAST_WS_MODULE || 'D:/temp/nast-smoke/node_modules/ws');

const MOCK_PORT = 19031;
const NAST_PORT = 18099;
let lastGen = null;
let failures = 0;
function check(name, cond, extra = '') {
  if (cond) console.log(`  PASS ${name}`);
  else { failures++; console.error(`  FAIL ${name} ${extra}`); }
}

const mock = http.createServer((req, res) => {
  if (req.method === 'POST' && req.url === '/v1/chat/completions') {
    let b = ''; req.on('data', (c) => (b += c)); req.on('end', () => {
      lastGen = JSON.parse(b);
      res.setHeader('content-type', 'text/event-stream');
      res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: 'CLEAN secret-token text.' } }] })}\n\n`);
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
    ws.on('open', () => resolve({
      call: (method, params = {}) => new Promise((res, rej) => {
        const id = String(nextId++);
        pending.set(id, { res, rej });
        ws.send(JSON.stringify({ id, method, params }));
        setTimeout(() => { if (pending.has(id)) { pending.delete(id); rej(new Error('TIMEOUT ' + method)); } }, 15000);
      }),
      close: () => ws.close(),
    }));
    ws.on('message', (raw) => {
      const msg = JSON.parse(raw.toString());
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
    data: { name: 'RxChar', description: 'DESC', first_mes: 'Hi', mes_example: '', tags: [] },
  });
}

async function main() {
  await new Promise((r) => mock.listen(MOCK_PORT, r));
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'nast-m4-'));
  const child = spawn(path.join(__dirname, '..', 'target', 'debug', 'nast.exe'), [], {
    env: { ...process.env, NAST_PORT: String(NAST_PORT), NAST_DATA: tmp, OPENAI_API_KEY: '', NAST_OPENAI_BASE: '' },
    cwd: tmp, stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stderr.on('data', (d) => process.env.M4_VERBOSE && process.stderr.write(d));
  let rpc;
  for (let i = 0; i < 50; i++) {
    try { rpc = await connect(`ws://127.0.0.1:${NAST_PORT}/ws`); break; }
    catch { await new Promise((r) => setTimeout(r, 200)); }
  }
  if (!rpc) { console.error('FAIL 无法连接服务端'); process.exit(1); }

  try {
    // 1. 预设 round-trip
    await rpc.call('presets.save', { name: 'My Preset', preset: { temperature: 0.7, openai_max_tokens: 123 } });
    const list = await rpc.call('presets.list', {});
    check('presets.list', list.includes('My Preset'), JSON.stringify(list));
    const got = await rpc.call('presets.get', { name: 'My Preset' });
    check('presets.get', got.temperature === 0.7 && got.openai_max_tokens === 123);
    await rpc.call('presets.delete', { name: 'My Preset' });
    check('presets.delete', !(await rpc.call('presets.list', {})).includes('My Preset'));

    // 2. 角色卡
    const imp = await rpc.call('characters.import', { data_base64: Buffer.from(cardJson()).toString('base64') });
    const created = await rpc.call('chats.new', { avatar: imp.avatar, greeting_index: 0 });

    // 3. 全局正则脚本（AI_OUTPUT 默认 pass 删除 secret-token）+ 自定义提示词顺序
    await require('./model_catalog_helper')(rpc.call.bind(rpc), `http://127.0.0.1:${MOCK_PORT}/v1`, 'mock-a');
    await rpc.call('settings.save', {
      settings: {
        oai_settings: {
          chat_completion_source: 'custom',
          custom_url: `http://127.0.0.1:${MOCK_PORT}/v1`,
          custom_model: 'mock-a',
          openai_max_context: 4095,
          openai_max_tokens: 256,
          prompts: [
            { identifier: 'main', name: 'Main Prompt', marker: true, role: 'system', content: '' },
            { identifier: 'chatHistory', name: 'Chat History', marker: true, role: 'system', content: '' },
            { identifier: 'custom-top', name: 'Custom Top', marker: false, role: 'system', content: 'CUSTOM-TOP-MARKER' },
          ],
          prompt_order: [{ character_id: 100001, order: [
            { identifier: 'main', enabled: true },
            { identifier: 'custom-top', enabled: true },
            { identifier: 'chatHistory', enabled: true },
          ] }],
        },
        extension_settings: {
          regex: [
            { id: 'r1', script_name: 'strip secret', find_regex: 'secret-token\\s*', replace_string: '', trim_strings: [], placement: [2], disabled: false, markdown_only: false, prompt_only: false, run_on_edit: true, substitute_regex: 0, min_depth: null, max_depth: null },
          ],
        },
      },
    });

    const gen = await rpc.call('generate.run', {
      avatar: imp.avatar, chat_file: created.file_name, type: 'normal', user_message: 'hello',
    });
    // 正则默认 pass（保存文本）：secret-token 剥离
    check('全局正则作用于保存文本', gen.text === 'CLEAN text.', JSON.stringify(gen.text));

    const chat = await rpc.call('chats.get', { avatar: imp.avatar, file_name: created.file_name });
    const saved = chat[chat.length - 1];
    check('落盘文本过正则', saved.mes === 'CLEAN text.', JSON.stringify(saved.mes));

    // 4. prompt_order 顺序：custom-top 在 main 之后（相对顺序）
    const texts = lastGen.messages.map((m) => m.content);
    const mainIdx = texts.findIndex((t) => typeof t === 'string' && t.includes('Write'));
    const customIdx = texts.findIndex((t) => typeof t === 'string' && t.includes('CUSTOM-TOP-MARKER'));
    check('自定义提示词注入', customIdx >= 0, JSON.stringify(texts));
    if (customIdx >= 0 && mainIdx >= 0) {
      check('顺序：main → custom-top', mainIdx < customIdx, `main=${mainIdx} custom=${customIdx}`);
    }
  } catch (e) {
    failures++;
    console.error('FAIL 异常:', e.message);
  } finally {
    rpc.close(); child.kill(); mock.close();
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  console.log(failures === 0 ? '\nM4 预设/正则/PM 冒烟全部通过' : `\n${failures} 项失败`);
  process.exit(failures === 0 ? 0 : 1);
}

main();
