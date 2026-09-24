// Isolated HTTP contract checks. No real provider credentials or paid calls.
// NAST_TEST_BIN=<new binary> NAST_WS_MODULE=<ws module> node tests/tts_smoke.cjs
const assert = require('node:assert/strict');
const http = require('node:http');
const net = require('node:net');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const WebSocket = require(process.env.NAST_WS_MODULE || 'ws');
const catalog = require('../resources/tts-providers.json');
const root = path.resolve(__dirname, '..');
const samples = Buffer.alloc(4800);
function wav() {
  const h = Buffer.alloc(44); h.write('RIFF'); h.writeUInt32LE(samples.length + 36, 4); h.write('WAVEfmt ', 8);
  h.writeUInt32LE(16, 16); h.writeUInt16LE(1, 20); h.writeUInt16LE(1, 22); h.writeUInt32LE(24000, 24);
  h.writeUInt32LE(48000, 28); h.writeUInt16LE(2, 32); h.writeUInt16LE(16, 34); h.write('data', 36); h.writeUInt32LE(samples.length, 40);
  return Buffer.concat([h, samples]);
}
const audio = wav();
const calls = [];
let failureMode = '', miniMode = 'hex', history = false;
const mock = http.createServer(async (req, res) => {
  const chunks = []; for await (const chunk of req) chunks.push(chunk);
  const raw = Buffer.concat(chunks).toString();
  let body = {}; try { body = JSON.parse(raw); } catch { if (req.headers['content-type']?.startsWith('application/x-www-form-urlencoded')) body = Object.fromEntries(new URLSearchParams(raw)); }
  const url = new URL(req.url, 'http://localhost');
  const index = Number(url.pathname.split('/')[1]);
  const provider = catalog[index]?.name;
  const route = url.pathname.replace(/^\/\d+/, '');
  calls.push({ provider, method: req.method, route, body, raw, headers: req.headers, query: Object.fromEntries(url.searchParams) });
  const json = (value) => { res.setHeader('content-type', 'application/json'); res.end(JSON.stringify(value)); };
  const sound = () => { res.setHeader('content-type', 'audio/wav'); res.end(audio); };
  if (failureMode === 'auth') { res.statusCode = 401; return json({ error: 'test-api_key_custom_openai_tts' }); }
  if (failureMode === 'html') { res.setHeader('content-type', 'text/html'); return res.end('<html>wrong endpoint</html>'); }
  if (failureMode === 'empty') { res.setHeader('content-type', 'audio/wav'); return res.end(); }
  if (route === '/api/currentsettings') return json({ models_available: [{ name: 'xtts' }], current_model_loaded: 'xtts', current_engine_loaded: 'xtts', deepspeed_capable: true, deepspeed_available: true, deepspeed_enabled: false, lowvram_capable: true, lowvram_enabled: false });
  if (route === '/api/rvcvoices') return json({ rvcvoices: ['example.pth'] });
  if (['/api/reload', '/api/deepspeed', '/api/lowvramsetting'].includes(route)) return json({ status: 'success' });
  if (route === '/api/text-to-speech/coqui/local/get-models') return json({ models_list: ['my-model'] });
  if (route === '/api/text-to-speech/coqui/coqui-api/check-model-state') return json({ model_state: 'absent' });
  if (route === '/api/text-to-speech/coqui/coqui-api/install-model') return json({ status: 'done' });
  if (provider === 'TTS WebUI' && body.stream) {
    const streamed = Buffer.from(audio); streamed.writeUInt32LE(0xffffffff, 4); streamed.writeUInt32LE(0, 40);
    res.setHeader('content-type', 'audio/wav'); res.write(streamed.subarray(0, 21)); return res.end(streamed.subarray(21));
  }
  if (route === '/get_predefined_voices') return json([{ display_name: '测试音色', voice_id: 'test-voice' }]);
  if (route === '/get_reference_files') return json(['sample.wav']);
  if (route === '/voice/speakers') return json({ VITS: [{ id: 0, name: '测试音色', lang: 'zh' }] });
  if (route === '/models/info') return json({ 0: { spk2id: { 测试音色: 0 }, style2id: { Neutral: 0 } } });
  if (route === '/character_list') return json({ 测试音色: ['default'] });
  if (route === '/history') return json({ history: history ? [{ text: '你好 <&> 世界', voice_id: 'test-voice', model_id: 'eleven_turbo_v2_5', history_item_id: 'saved-123' }] : [] });
  if (route === '/history/saved-123/audio') return sound();
  if (route === '/voices/add') return json({ voice_id: 'new-voice' });
  if (route.endsWith('/models')) {
    if (provider === 'Pollinations') return json([{ name: 'openai-audio', voices: ['alloy'] }]);
    if (provider === 'ElevenLabs') return json([{ model_id: 'test-tts', can_do_text_to_speech: true }, { model_id: 'stt-only', can_do_text_to_speech: false }]);
    return json({ data: [{ id: 'tts-1', voices: ['alloy'] }] });
  }
  if (route === '/cognitiveservices/voices/list' || route.endsWith('/edge-tts/list')) return json([{ ShortName: 'zh-CN-Test', Locale: 'zh-CN' }]);
  if (route.includes('/voices/chatterbox')) return json({ voices: [{ value: 'test-voice', label: '测试音色' }] });
  if (route === '/speakers' || route === '/voices') return json({ voices: [{ voice_id: 'test-voice', name: '测试音色' }] });
  if (route === '/api/voices') return json({ voices: ['test-voice'] });
  if (route === '/set_tts_settings') return json({ ok: true });
  if (route === '/api/tts-generate') return json({ output_file_url: `/${index}/audio.wav` });
  if (provider === 'Google Gemini TTS') return json({ candidates: [{ content: { parts: [{ inlineData: { mimeType: 'audio/L16;rate=24000', data: samples.toString('base64') } }] } }] });
  if (provider === 'MiniMax') {
    if (route === '/audio.wav') return sound();
    return json({ base_resp: { status_code: 0 }, data: miniMode === 'url' ? { url: `http://127.0.0.1:${mock.address().port}/${index}/audio.wav` } : { audio: audio.toString('hex') } });
  }
  if (provider === 'Volcengine') { res.setHeader('content-type', 'application/json'); const line = JSON.stringify({ code: 0, data: audio.toString('base64') }); res.write(line.slice(0, 17)); res.end(line.slice(17) + '\n' + JSON.stringify({ code: 20000000 })); return; }
  if (provider === 'Pollinations') return json({ choices: [{ message: { audio: { data: audio.toString('base64') } } }] });
  sound();
});
async function port() { const server = net.createServer(); await new Promise((r) => server.listen(0, '127.0.0.1', r)); const port = server.address().port; await new Promise((r) => server.close(r)); return port; }
async function connect(url) {
  const ws = new WebSocket(url); await new Promise((r, j) => { ws.once('open', r); ws.once('error', j); });
  let id = 0; const pending = new Map();
  ws.on('message', (raw) => { const data = JSON.parse(raw); const entry = pending.get(data.id); if (entry) { pending.delete(data.id); data.error ? entry.reject(Error(data.error.message)) : entry.resolve(data.result); } });
  return { ws, call: (method, params = {}) => new Promise((resolve, reject) => { const key = String(++id); pending.set(key, { resolve, reject }); ws.send(JSON.stringify({ id: key, method, params })); }) };
}
async function main() {
  await new Promise((r) => mock.listen(0, '127.0.0.1', r));
  const serverPort = await port();
  const data = fs.mkdtempSync(path.join(os.tmpdir(), 'nast-tts-test-'));
  const binary = process.env.NAST_TEST_BIN || path.join(root, 'target', 'debug', process.platform === 'win32' ? 'nast.exe' : 'nast');
  const server = spawn(binary, [], { cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, NAST_PORT: String(serverPort), NAST_BIND: '127.0.0.1', NAST_DATA: data, NAST_WEB: path.join(root, 'web/dist') } });
  let log = ''; server.stdout.on('data', (v) => { log += v; }); server.stderr.on('data', (v) => { log += v; });
  let rpc;
  try {
    const base = `http://127.0.0.1:${serverPort}`;
    for (let i = 0; i < 100; i++) { try { await fetch(base); break; } catch { await new Promise((r) => setTimeout(r, 100)); } }
    rpc = await connect(`ws://127.0.0.1:${serverPort}/ws`);
    for (const key of new Set(catalog.flatMap((p) => p.secrets))) await rpc.call('secrets.set', { key, value: `test-${key}` });
    const post = (action, input) => fetch(`${base}/api/tts/${action}`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(input) });
    const setups = new Map();
    for (const [index, p] of catalog.entries()) {
      if (p.runtime !== 'http') continue;
      const endpoint = `http://127.0.0.1:${mock.address().port}/${index}`;
      const config = { ...p.defaults, provider_endpoint: endpoint, reuse_history: false };
      if (['OpenAI', 'OpenAI Compatible', 'Electron Hub', 'TTS WebUI'].includes(p.name)) config.provider_endpoint += '/v1/audio/speech';
      if (p.name === 'MiniMax') { delete config.provider_endpoint; config.apiHost = endpoint; config.format = 'wav'; }
      if (p.name === 'Azure') config.region = 'test-region';
      if (p.name === 'Volcengine') config.resource_id = 'seed-tts-test';
      if (p.name === 'Coqui') config.customVoices = { 'test-voice': 'tts_models/multilingual/test[0][1]' };
      const voiceId = p.name === 'VITS' ? 'VITS&0' : p.name === 'SBVits2' ? '0-0-Neutral' : 'test-voice';
      const input = { provider: p.name, settings: config, text: '你好 <&> 世界', voice: voiceId, character: '小雨', characters: ['小雨'] };
      setups.set(p.name, input);
      const list = await post('voices', input); assert.equal(list.status, 200, `${p.name} voices: ${await list.clone().text()}`);
      assert.ok(Array.isArray(await list.json()), `${p.name} voice shape`);
      const response = await post('synthesize', input); assert.equal(response.status, 200, `${p.name} synthesis: ${await response.clone().text()}`);
      const bytes = Buffer.from(await response.arrayBuffer()); assert.ok(bytes.length > 44, `${p.name} nonempty audio`);
      assert.equal(bytes.subarray(0, 4).toString(), 'RIFF', `${p.name} audio signature`);
      assert.match(response.headers.get('content-type'), /^audio\//);
      console.log(`PASS ${p.name}: voices + synthesis`);
    }
    const last = (name, route) => calls.findLast((c) => c.provider === name && (!route || c.route === route));
    assert.equal(last('OpenAI').body.input, '你好 <&> 世界');
    assert.equal(last('OpenAI').headers.authorization, 'Bearer test-api_key_openai');
    assert.equal(last('OpenAI Compatible').headers.authorization, 'Bearer test-api_key_custom_openai_tts');
    assert.equal(last('ElevenLabs').headers['xi-api-key'], 'test-api_key_elevenlabs');
    assert.equal(last('ElevenLabs').body.voice_settings.similarity_boost, 0.75);
    assert.match(last('Azure').raw, /你好 &lt;&amp;&gt; 世界/);
    assert.equal(last('Google Gemini TTS').headers['x-goog-api-key'], 'test-api_key_makersuite');
    assert.equal(last('MiniMax').query.GroupId, 'test-minimax_group_id');
    assert.equal(last('MiniMax').body.voice_setting.voice_id, 'test-voice');
    assert.equal(last('Volcengine').headers['x-api-access-key'], 'test-volcengine_access_key');
    assert.equal(last('Coqui').body.language_id, 0); assert.equal(last('Coqui').body.speaker_id, 1);
    assert.equal(last('GPT-SoVITS-Adapter').body.target_voice, 'test-voice');
    assert.equal(last('GPT-SoVITS-V2 (Unofficial)').body.ref_audio_path, './参考音频/test-voice.wav');
    assert.equal(last('SBVits2').query.style, 'Neutral');
    assert.equal(last('VITS').body.id, '0');
    assert.ok(calls.some((c) => c.provider === 'XTTSv2' && c.route === '/set_tts_settings' && c.body.top_k === 50));
    for (const provider of ['OpenAI Compatible', 'ElevenLabs', 'Electron Hub', 'TTS WebUI']) {
      const response = await post('models', setups.get(provider)); assert.equal(response.status, 200); assert.ok((await response.json()).length);
    }
    console.log('PASS auth headers, model lists, SSML, local-model parameters');
    const eleven = structuredClone(setups.get('ElevenLabs')); eleven.settings.reuse_history = true; history = true;
    assert.equal((await post('synthesize', eleven)).status, 200); assert.equal(last('ElevenLabs').route, '/history/saved-123/audio');
    const clone = await post('voices/add', { ...eleven, name: '新音色', files: [`data:audio/wav;base64,${audio.toString('base64')}`] });
    assert.equal(clone.status, 200); assert.match(last('ElevenLabs').headers['content-type'], /multipart\/form-data/);
    miniMode = 'url'; assert.equal((await post('synthesize', setups.get('MiniMax'))).status, 200);
    console.log('PASS ElevenLabs history/upload and MiniMax URL audio');
    const streaming = structuredClone(setups.get('TTS WebUI')); streaming.settings.streaming = true;
    const stream = await post('synthesize', streaming);
    assert.equal(stream.status, 200); const wav = Buffer.from(await stream.arrayBuffer());
    assert.equal(last('TTS WebUI').body.stream, true);
    assert.equal(wav.readUInt32LE(4), wav.length - 8); assert.equal(wav.readUInt32LE(40), wav.length - 44);
    for (const action of ['status', 'rvc-voices', 'reload', 'deepspeed', 'low-vram']) {
      assert.equal((await post('manage', { ...setups.get('AllTalk'), action, model_id: 'xtts', value: true })).status, 200);
    }
    assert.equal(last('AllTalk', '/api/reload').query.tts_method, 'xtts');
    assert.equal(last('AllTalk', '/api/deepspeed').query.new_deepspeed_value, 'True');
    for (const action of ['local-models', 'check-model', 'install-model', 'repair-model']) {
      assert.equal((await post('manage', { ...setups.get('Coqui'), action, model_id: 'tts_models/en/test/model' })).status, 200);
    }
    assert.equal(last('Coqui').body.action, 'repare');
    assert.equal((await post('manage', { ...setups.get('OpenAI'), action: 'reload', model_id: 'test' })).status, 400);
    assert.equal((await post('manage', { ...setups.get('AllTalk'), action: 'reload' })).status, 400);
    console.log('PASS streaming WAV, AllTalk controls and Coqui model management');
    const compat = setups.get('OpenAI Compatible');
    const saved = { unrelated: { keep: true }, extension_settings: { tts: { enabled: true, currentProvider: 'OpenAI Compatible', 'OpenAI Compatible': { ...compat.settings, voiceMap: { '[Default Voice]': 'test-voice' } } } } };
    await rpc.call('settings.save', { settings: saved });
    assert.deepEqual(await rpc.call('settings.get'), saved);
    assert.equal((await post('synthesize', { text: 'saved settings', voice: 'test-voice' })).status, 200);
    const masked = await rpc.call('secrets.get'); assert.ok(!JSON.stringify(masked).includes('test-api_key'));
    assert.equal((await post('synthesize', { ...compat, text: '' })).status, 400);
    assert.equal((await post('synthesize', { ...compat, provider: 'unknown' })).status, 400);
    assert.equal((await post('synthesize', { ...compat, settings: { provider_endpoint: 'file:///sensitive' } })).status, 400);
    failureMode = 'auth'; const rejected = await post('synthesize', compat); assert.equal(rejected.status, 502); assert.ok(!(await rejected.text()).includes('test-api_key'));
    failureMode = 'html'; assert.equal((await post('synthesize', compat)).status, 502);
    failureMode = 'empty'; assert.equal((await post('synthesize', compat)).status, 502);
    console.log('PASS settings compatibility, secret masking, invalid input and upstream failures');
    console.log('TTS HTTP contract checks passed. Test data:', data);
  } catch (error) { console.error(log.slice(-1500)); throw error; }
  finally { rpc?.ws.close(); server.kill(); mock.closeAllConnections(); await new Promise((r) => mock.close(r)); }
}
main().catch((error) => { console.error(error); process.exitCode = 1; });
