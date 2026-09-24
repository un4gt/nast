// Browser acceptance with a real isolated nast server and deterministic speech services.
const { chromium } = require(process.env.NAST_PLAYWRIGHT_MODULE || 'playwright');
const WebSocket = require(process.env.NAST_WS_MODULE || 'ws');
const assert = require('node:assert/strict');
const http = require('node:http');
const net = require('node:net');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const root = path.resolve(__dirname, '..');
const output = fs.mkdtempSync(path.join(os.tmpdir(), 'nast-tts-browser-'));
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const audio = Buffer.alloc(44 + 48000); audio.write('RIFF'); audio.writeUInt32LE(audio.length - 8, 4); audio.write('WAVEfmt ', 8);
audio.writeUInt32LE(16, 16); audio.writeUInt16LE(1, 20); audio.writeUInt16LE(1, 22); audio.writeUInt32LE(24000, 24);
audio.writeUInt32LE(48000, 28); audio.writeUInt16LE(2, 32); audio.writeUInt16LE(16, 34); audio.write('data', 36); audio.writeUInt32LE(audio.length - 44, 40);
let failNext = false, delayNext = false, gen = 0;
const speech = [];
const management = [];
const mock = http.createServer(async (req, res) => {
  const chunks = []; for await (const c of req) chunks.push(c);
  let body = {}; try { body = JSON.parse(Buffer.concat(chunks)); } catch {}
  const route = new URL(req.url, 'http://localhost');
  const json = (value) => { res.setHeader('content-type', 'application/json'); res.end(JSON.stringify(value)); };
  if (route.pathname === '/api/currentsettings') return json({ current_model_loaded: 'xtts', current_engine_loaded: 'xtts', models_available: [{ name: 'xtts' }], deepspeed_capable: true, deepspeed_available: true, deepspeed_enabled: management.includes('deepspeed'), lowvram_capable: true, lowvram_enabled: false });
  if (['/api/reload', '/api/deepspeed', '/api/lowvramsetting'].includes(route.pathname)) { management.push(route.pathname.split('/').pop()); return json({ status: 'success' }); }
  if (route.pathname === '/api/text-to-speech/coqui/coqui-api/check-model-state') return json({ model_state: 'absent' });
  if (route.pathname === '/api/text-to-speech/coqui/coqui-api/install-model') { management.push(body.action); return json({ status: 'done' }); }
  if (req.url === '/v1/models') { res.setHeader('content-type', 'application/json'); return res.end(JSON.stringify({ data: [{ id: 'mock-tts' }] })); }
  if (req.url === '/v1/audio/speech') {
    speech.push(body);
    if (failNext) { failNext = false; res.statusCode = 401; return res.end('{}'); }
    if (delayNext) { delayNext = false; await pause(900); }
    res.setHeader('content-type', 'audio/wav'); return res.end(audio);
  }
  if (req.url === '/v1/chat/completions') {
    const id = ++gen;
    const pieces = [`第${id}次的第一段。\n\n`, `第${id}次的第二段。`];
    res.setHeader('content-type', 'text/event-stream');
    for (const piece of pieces) { res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: piece } }] })}\n\n`); await pause(500); }
    return res.end('data: [DONE]\n\n');
  }
  res.statusCode = 404; res.end('{}');
});
async function freePort() { const s = net.createServer(); await new Promise((r) => s.listen(0, '127.0.0.1', r)); const p = s.address().port; await new Promise((r) => s.close(r)); return p; }
async function rpcClient(origin) {
  const ws = new WebSocket(origin.replace('http', 'ws') + '/ws'); await new Promise((r, j) => { ws.once('open', r); ws.once('error', j); });
  const pending = new Map(); let id = 0;
  ws.on('message', (raw) => { const v = JSON.parse(raw); const p = pending.get(v.id); if (p) { pending.delete(v.id); v.error ? p.reject(Error(v.error.message)) : p.resolve(v.result); } });
  return { ws, call: (method, params = {}) => new Promise((resolve, reject) => { const key = String(++id); pending.set(key, { resolve, reject }); ws.send(JSON.stringify({ id: key, method, params })); }) };
}
async function main() {
  await new Promise((r) => mock.listen(0, '127.0.0.1', r));
  const port = await freePort(), origin = `http://127.0.0.1:${port}`, mockBase = `http://127.0.0.1:${mock.address().port}`;
  const binary = process.env.NAST_TEST_BIN || path.join(root, 'target/debug', process.platform === 'win32' ? 'nast.exe' : 'nast');
  const child = spawn(binary, [], { cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, NAST_DATA: path.join(output, 'data'), NAST_PORT: String(port), NAST_WEB: path.join(root, 'web/dist') } });
  let logs = ''; child.stdout.on('data', (b) => { logs += b; }); child.stderr.on('data', (b) => { logs += b; });
  let browser, rpc, page;
  try {
    for (let i = 0; i < 100; i++) { try { await fetch(origin); break; } catch { await pause(100); } }
    rpc = await rpcClient(origin);
    const chars = [];
    for (const name of ['小雨', '林舟']) {
      const card = { spec: 'chara_card_v2', spec_version: '2.0', data: { name, description: '故事中的旅人', personality: '温柔', scenario: '书店', first_mes: `${name}：你好，欢迎来到书店。`, mes_example: '', tags: [] } };
      const character = await rpc.call('characters.import', { data_base64: Buffer.from(JSON.stringify(card)).toString('base64') });
      await rpc.call('chats.new', { avatar: character.avatar, greeting_index: 0 });
      chars.push(character);
    }
    const group = await rpc.call('groups.create', { group: { name: '朗读验收群', members: chars.map((c) => c.avatar), activation_strategy: 1 } });
    await rpc.call('settings.save', { settings: { name1: 'User', oai_settings: { custom_url: mockBase + '/v1', custom_model: 'mock-chat', chat_completion_source: 'custom', stream_openai: true, openai_max_context: 4096, openai_max_tokens: 128 } } });
    browser = await chromium.launch({ channel: 'msedge', headless: true });
    const context = await browser.newContext({ viewport: { width: 1440, height: 960 } });
    await context.addInitScript(() => {
      let timer; window.__systemSpeech = [];
      window.SpeechSynthesisUtterance = class { constructor(text) { this.text = text; } };
      const synth = { speaking: false, getVoices: () => [{ name: '测试系统音色', voiceURI: 'test-system', lang: 'zh-CN' }],
        speak(utt) { window.__systemSpeech.push({ text: utt.text, rate: utt.rate, pitch: utt.pitch }); this.speaking = true; utt.onstart?.(); timer = setTimeout(() => { this.speaking = false; utt.onend?.(); }, 200); },
        cancel() { clearTimeout(timer); this.speaking = false; }, pause() {}, resume() {}, addEventListener() {}, removeEventListener() {} };
      Object.defineProperty(window, 'speechSynthesis', { value: synth });
    });
    page = await context.newPage(); const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    const screenshot = (name) => page.screenshot({ path: path.join(output, name + '.png') });
    const choose = async (name) => { await page.locator('[data-sidebar="menu-button"]').filter({ hasText: name }).first().click(); await page.getByRole('heading', { name, exact: true }).waitFor(); };
    const openTts = async () => {
      await page.getByRole('button', { name: '设置', exact: true }).click();
      await page.getByRole('button', { name: '语音朗读', exact: true }).click();
    };
    const save = async () => { await page.getByRole('button', { name: '保存更改', exact: true }).click(); await page.waitForFunction(() => [...document.querySelectorAll('button')].some((b) => b.textContent === '保存更改' && b.disabled)); };
    const close = () => page.getByRole('button', { name: '关闭', exact: true }).click();
    const waitSpeech = async (count) => { for (let i = 0; i < 100 && speech.length < count; i++) await pause(100); assert.ok(speech.length >= count, `expected ${count} speech jobs, got ${speech.length}`); };
    await page.goto(origin); await page.getByText('已连接', { exact: true }).first().waitFor();
    await choose('小雨'); await openTts();
    await page.getByRole('switch', { name: '启用语音朗读', exact: true }).click();
    await page.getByRole('combobox', { name: '语音服务商' }).click(); await page.getByRole('option', { name: 'OpenAI 兼容', exact: true }).click();
    await page.getByLabel('服务端点', { exact: true }).fill(mockBase + '/v1/audio/speech');
    await page.getByLabel('API Key', { exact: true }).fill('tts-browser-key');
    await page.getByRole('button', { name: '保存密钥', exact: true }).click();
    await page.getByLabel('语音模型', { exact: true }).fill('mock-tts');
    await page.getByLabel('默认音色', { exact: true }).fill('alloy');
    await page.getByRole('button', { name: '刷新音色', exact: true }).click();
    await page.getByText('列表已更新', { exact: true }).waitFor();
    await page.getByRole('switch', { name: '生成时按段朗读', exact: true }).click();
    await page.getByLabel('服务端点', { exact: true }).scrollIntoViewIfNeeded();
    await screenshot('settings-desktop-dark'); await save();
    assert.equal((await rpc.call('settings.get')).extension_settings.tts.currentProvider, 'OpenAI Compatible');
    await page.getByRole('button', { name: '试听当前配置', exact: true }).click(); await waitSpeech(1);
    assert.equal(speech[0].model, 'mock-tts'); assert.equal(speech[0].voice, 'alloy');
    await page.getByRole('button', { name: '停止试听', exact: true }).click(); await close();
    console.log('PASS TTS configuration, secret storage, voices and real WAV preview');

    await page.getByRole('button', { name: '朗读此消息', exact: true }).first().click(); await waitSpeech(2);
    await page.getByRole('button', { name: '暂停', exact: true }).click(); await page.getByRole('button', { name: '继续播放', exact: true }).waitFor();
    await page.getByRole('button', { name: '继续播放', exact: true }).click(); await page.getByRole('button', { name: '停止朗读', exact: true }).click();
    assert.equal(await page.getByRole('button', { name: '停止朗读', exact: true }).count(), 0);
    delayNext = true;
    await page.getByRole('button', { name: '朗读此消息', exact: true }).first().click(); await waitSpeech(3);
    await choose('林舟'); await waitSpeech(4);
    assert.ok(speech.at(-1).input.includes('林舟'));
    await page.getByRole('button', { name: '停止朗读', exact: true }).waitFor({ state: 'hidden' });
    await pause(1000);
    assert.equal(await page.getByRole('button', { name: '停止朗读', exact: true }).count(), 0);
    await choose('小雨'); await page.getByRole('button', { name: '停止朗读', exact: true }).waitFor({ state: 'hidden' });
    console.log('PASS manual narration, pause/resume and no late playback after switching chats');

    const before = speech.length;
    await page.getByRole('textbox', { name: '输入消息', exact: true }).fill('分两段回复');
    await page.getByRole('button', { name: '发送消息', exact: true }).click();
    await waitSpeech(before + 2); await pause(1300);
    assert.deepEqual(speech.slice(before).map((v) => v.input), ['第1次的第一段。', '第1次的第二段。']);
    console.log('PASS automatic streaming paragraphs speak once, without repeating final message');
    await page.getByRole('textbox', { name: '输入消息', exact: true }).fill('/speak voice="小雨" 命令朗读');
    await page.getByRole('button', { name: '发送消息', exact: true }).click(); await waitSpeech(before + 3);
    assert.equal(gen, 1); assert.equal(speech.at(-1).input, '命令朗读');
    await page.getByRole('button', { name: '停止朗读', exact: true }).click();
    console.log('PASS /speak uses role voice and does not trigger a chat completion');

    failNext = true;
    await page.getByRole('button', { name: '朗读此消息', exact: true }).first().click();
    await page.getByText(/语音服务返回 HTTP 401/).first().waitFor();
    await screenshot('player-error');
    await page.getByRole('button', { name: '朗读此消息', exact: true }).first().click();
    await page.getByRole('button', { name: '停止朗读', exact: true }).waitFor(); await page.getByRole('button', { name: '停止朗读', exact: true }).click();
    console.log('PASS provider errors surface in the player and retry works');

    await openTts();
    await page.getByRole('button', { name: '角色音色映射', exact: true }).click();
    await page.getByRole('combobox', { name: '林舟', exact: true }).scrollIntoViewIfNeeded();
    await pause(250); // Let the enclosing accordion finish changing the scroll range.
    await page.getByRole('combobox', { name: '林舟', exact: true }).click();
    await page.getByRole('option', { name: 'nova', exact: true }).click();
    await page.getByRole('switch', { name: '也朗读用户消息', exact: true }).click(); await save(); await close();
    await choose('朗读验收群');
    const groupBefore = speech.length;
    await page.getByRole('textbox', { name: '输入消息', exact: true }).fill('一起打个招呼');
    await page.getByRole('button', { name: '发送消息', exact: true }).click();
    await waitSpeech(groupBefore + 5); await pause(1200);
    const groupChat = await rpc.call('groups.get_chat', { chat_id: group.chat_id });
    assert.equal(speech[groupBefore].input, groupChat.find((m) => m.is_user).mes);
    assert.ok(speech.slice(groupBefore).some((v) => v.voice === 'nova'));
    assert.equal(speech.slice(groupBefore).length, 5);
    console.log('PASS group user + two members narrate with separate voices');

    await openTts();
    await page.getByRole('combobox', { name: '语音服务商' }).click(); await page.getByRole('option', { name: 'AllTalk', exact: true }).click();
    await page.getByLabel('服务端点', { exact: true }).fill(mockBase);
    await page.getByRole('button', { name: '模型服务管理', exact: true }).click();
    await page.getByRole('button', { name: '读取服务状态', exact: true }).click();
    await page.getByText('服务状态已更新', { exact: true }).waitFor();
    await page.getByRole('button', { name: '切换服务模型', exact: true }).click();
    await page.getByText('服务状态已更新', { exact: true }).waitFor();
    await page.getByRole('switch', { name: 'DeepSpeed', exact: true }).click();
    await page.getByText('服务状态已更新', { exact: true }).waitFor();
    assert.ok(management.includes('reload') && management.includes('deepspeed'));
    await page.getByRole('combobox', { name: '语音服务商' }).click(); await page.getByRole('option', { name: 'Coqui', exact: true }).click();
    await page.getByLabel('服务端点', { exact: true }).fill(mockBase);
    await page.getByLabel('服务模型 ID', { exact: true }).fill('tts_models/en/ljspeech/vits');
    await page.getByRole('button', { name: '检查安装状态', exact: true }).click(); await page.getByText('模型尚未安装', { exact: true }).waitFor();
    await page.getByRole('button', { name: '下载模型', exact: true }).click(); await page.getByText('模型安装完成', { exact: true }).waitFor();
    await page.getByLabel('音色名称', { exact: true }).fill('本地旁白');
    await page.getByRole('button', { name: '保存音色到草稿', exact: true }).click();
    await save();
    assert.equal((await rpc.call('settings.get')).extension_settings.tts.Coqui.customVoices['本地旁白'], 'tts_models/en/ljspeech/vits');
    assert.ok(management.includes('download'));
    console.log('PASS AllTalk settings and Coqui installation/voice configuration');

    await page.getByRole('combobox', { name: '语音服务商' }).click(); await page.getByRole('option', { name: '系统语音', exact: true }).click();
    await page.getByRole('button', { name: '刷新音色', exact: true }).click(); await page.getByText('测试系统音色', { exact: true }).first().waitFor();
    await page.getByRole('button', { name: '试听当前配置', exact: true }).click();
    await page.waitForFunction(() => window.__systemSpeech.length > 0);
    assert.equal((await page.evaluate(() => window.__systemSpeech))[0].rate, 1);
    console.log('PASS browser speech synthesis API and default voice fallback');
    await page.getByRole('button', { name: '外观', exact: true }).click(); await page.getByRole('radio', { name: '浅色主题', exact: true }).click();
    await page.getByRole('button', { name: '语音朗读', exact: true }).click(); await screenshot('settings-desktop-light');
    await page.setViewportSize({ width: 390, height: 844 });
    await page.getByRole('combobox', { name: '设置分类' }).click(); await page.getByRole('option', { name: '语音朗读', exact: true }).click();
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    await screenshot('settings-mobile-light'); await close();
    await screenshot('chat-mobile-light');
    assert.deepEqual(errors, []);
    const touch = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, reducedMotion: 'reduce' });
    const mobile = await touch.newPage();
    await mobile.goto(origin); await mobile.getByText('已连接', { exact: true }).first().waitFor({ state: 'attached' });
    await mobile.getByTitle('切换侧栏（Ctrl / ⌘ + B）').tap();
    await mobile.getByRole('button', { name: '设置', exact: true }).tap();
    await mobile.getByRole('combobox', { name: '设置分类' }).tap(); await mobile.getByRole('option', { name: '语音朗读', exact: true }).tap();
    await mobile.getByRole('switch', { name: '启用语音朗读', exact: true }).tap();
    await mobile.getByRole('button', { name: '保存更改', exact: true }).tap();
    await mobile.screenshot({ path: path.join(output, 'settings-mobile-dark-touch.png') });
    await touch.close();
    console.log('PASS desktop/mobile layout, both themes and no uncaught browser exceptions');
    console.log('Browser evidence:', output);
  } catch (error) {
    await page?.screenshot({ path: path.join(output, 'failure.png') });
    console.error('Browser evidence:', output);
    console.error(await page?.locator('[role="listbox"]').evaluateAll((els) => els.map((e) => ({ box: e.getBoundingClientRect().toJSON(), maxHeight: getComputedStyle(e).maxHeight, transform: getComputedStyle(e).transform }))));
    console.error(logs.slice(-800)); throw error;
  }
  finally { await browser?.close(); rpc?.ws.close(); child.kill(); mock.closeAllConnections(); await new Promise((r) => mock.close(r)); }
}
main().catch((e) => { console.error(e); process.exitCode = 1; });
