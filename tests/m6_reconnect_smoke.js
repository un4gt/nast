// M6 验收：生成中断线重连——generate.status 恢复进度、最终消息落盘。
// 运行：node tests/m6_reconnect_smoke.js
const http = require('http');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');
const WebSocket = require(process.env.NAST_WS_MODULE || 'D:/temp/nast-smoke/node_modules/ws');

const MOCK_PORT = 19051;
const NAST_PORT = 18119;
let failures = 0;
function check(name, cond, extra = '') {
  if (cond) console.log(`  PASS ${name}`);
  else { failures++; console.error(`  FAIL ${name} ${extra}`); }
}

// mock：先发一半 token，等 3s 再发剩余（制造断线窗口）
let pendingRes = null;
const mock = http.createServer((req, res) => {
  if (req.method === 'POST' && req.url === '/v1/chat/completions') {
    let b = ''; req.on('data', (c) => (b += c)); req.on('end', () => {
      res.setHeader('content-type', 'text/event-stream');
      res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: 'PART1-' } }] })}\n\n`);
      pendingRes = res;
      setTimeout(() => {
        res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: 'PART2.' } }] })}\n\n`);
        res.write('data: [DONE]\n\n');
        res.end();
      }, 3000);
    });
    return;
  }
  res.statusCode = 404; res.end('{}');
});

function makeClient(url) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url);
    const pending = new Map(); let nextId = 1;
    const client = {
      ws,
      call: (method, params = {}) => new Promise((res, rej) => {
        const id = String(nextId++);
        pending.set(id, { res, rej });
        ws.send(JSON.stringify({ id, method, params }));
        setTimeout(() => { if (pending.has(id)) { pending.delete(id); rej(new Error('TIMEOUT ' + method)); } }, 30000);
      }),
      close: () => ws.close(),
    };
    ws.on('open', () => resolve(client));
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
    data: { name: 'RcChar', description: 'DESC', first_mes: 'Hi', mes_example: '', tags: [] },
  });
}

async function main() {
  await new Promise((r) => mock.listen(MOCK_PORT, r));
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'nast-m6-'));
  const child = spawn(path.join(__dirname, '..', 'target', 'debug', 'nast.exe'), [], {
    env: { ...process.env, NAST_PORT: String(NAST_PORT), NAST_DATA: tmp, OPENAI_API_KEY: '', NAST_OPENAI_BASE: '' },
    cwd: tmp, stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stderr.on('data', (d) => process.env.M6_VERBOSE && process.stderr.write(d));

  let rpc;
  for (let i = 0; i < 50; i++) {
    try { rpc = await makeClient(`ws://127.0.0.1:${NAST_PORT}/ws`); break; }
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
          openai_max_tokens: 64,
        },
      },
    });
    const imp = await rpc.call('characters.import', { data_base64: Buffer.from(cardJson()).toString('base64') });
    const created = await rpc.call('chats.new', { avatar: imp.avatar, greeting_index: 0 });

    // 发起生成（不等待），在 PART1 已流出后断线
    const genPromise = rpc.call('generate.run', {
      avatar: imp.avatar, chat_file: created.file_name, type: 'normal', user_message: 'hi',
    }).catch(() => null); // 响应帧随连接丢失

    // 等首 token 流出后再断线（生成侧可能因 tokenizer 预热/网络稍慢）
    let rpcW;
    {
      let w;
      for (let i = 0; i < 50; i++) {
        try { w = await makeClient(`ws://127.0.0.1:${NAST_PORT}/ws`); break; }
        catch { await new Promise((r) => setTimeout(r, 300)); }
      }
      rpcW = w;
      let text = '';
      for (let i = 0; i < 60; i++) {
        await new Promise((r) => setTimeout(r, 250));
        const st = await rpcW.call('generate.status', {});
        text = st.text ?? '';
        if (text.includes('PART1-')) break;
      }
      check('断线前 PART1 已流出', text.includes('PART1-'), JSON.stringify(text));
      rpcW.close();
    }
    rpc.close();

    // 重连（模拟前端退避后恢复）
    let rpc2;
    for (let i = 0; i < 50; i++) {
      try { rpc2 = await makeClient(`ws://127.0.0.1:${NAST_PORT}/ws`); break; }
      catch { await new Promise((r) => setTimeout(r, 300)); }
    }
    check('断线后重连成功', !!rpc2);
    const st = await rpc2.call('generate.status', {});
    check('重连时生成仍在进行', st.running === true, JSON.stringify(st));
    check('进度已恢复（PART1 可见）', String(st.text).includes('PART1-'), JSON.stringify(st.text));
    check('状态含目标信息', st.info?.avatar === imp.avatar && st.info?.chat_file === created.file_name);

    // 等生成结束（旧连接的响应已丢，以 status 轮询为准）
    let final = null;
    for (let i = 0; i < 40; i++) {
      await new Promise((r) => setTimeout(r, 250));
      final = await rpc2.call('generate.status', {});
      if (!final.running) break;
    }
    check('生成结束（status.running=false）', final.running === false);
    const chat = await rpc2.call('chats.get', { avatar: imp.avatar, file_name: created.file_name });
    const last = chat[chat.length - 1];
    check('断线期间的消息照常落盘', last.mes === 'PART1-PART2.', JSON.stringify(last.mes));
    void genPromise;
    rpc2.close();
  } catch (e) {
    failures++;
    console.error('FAIL 异常:', e.message);
  } finally {
    child.kill(); mock.close();
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  console.log(failures === 0 ? '\nM6 断线恢复冒烟全部通过' : `\n${failures} 项失败`);
  process.exit(failures === 0 ? 0 : 1);
}

main();
