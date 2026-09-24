// Optional real-model acceptance: needs public Hugging Face access and disk/memory
// for model downloads. No provider mocks or changes to a user's settings.
const { chromium } = require(process.env.NAST_PLAYWRIGHT_MODULE || 'playwright');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const assert = require('node:assert/strict');
const root = path.resolve(__dirname, '../web/dist');
const catalog = require('../resources/tts-providers.json');
const files = fs.readdirSync(root, { recursive: true }).filter((p) => p.endsWith('.js'));
const worker = files.find((p) => fs.readFileSync(path.join(root, p), 'utf8').includes('正在加载语音模型'));
assert.ok(worker, 'Build the frontend first');
const server = http.createServer((req, res) => {
  if (req.url === '/') { res.setHeader('content-type', 'text/html'); return res.end('<title>TTS model acceptance</title>'); }
  const file = path.resolve(root, '.' + decodeURIComponent(req.url.split('?')[0]));
  if (!file.startsWith(root + path.sep) || !fs.existsSync(file)) { res.statusCode = 404; return res.end(); }
  res.setHeader('content-type', ({ '.js': 'text/javascript', '.mjs': 'text/javascript', '.wasm': 'application/wasm' })[path.extname(file)] || 'application/octet-stream');
  fs.createReadStream(file).pipe(res);
});
async function main() {
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const browser = await chromium.launch({ channel: 'msedge', headless: true });
  try {
    const page = await browser.newPage();
    page.on('console', (message) => { if (message.type() === 'error') console.error(message.text().slice(0, 300)); });
    page.on('requestfailed', (request) => console.error('Download failed:', new URL(request.url()).hostname, request.failure()?.errorText));
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    let failed = false;
    for (const provider of ['Kokoro', 'SpeechT5']) {
      const result = await page.evaluate(async ({ provider, config, url, timeout }) => {
        return new Promise((resolve) => {
          // Rspack emits a classic worker (and rewrites the app's Worker options).
          const runtime = new Worker(url);
          let status = 'Worker 启动';
          const finish = (value) => { clearTimeout(timer); runtime.terminate(); resolve(value); };
          const timer = setTimeout(() => finish({ error: `等待模型超时：${status}` }), timeout);
          runtime.onerror = (e) => finish({ error: e.message });
          runtime.onmessage = async ({ data }) => {
            if (data.status) status = data.status;
            if (data.error) finish({ error: data.error, status });
            if (data.blob) {
              const context = new AudioContext();
              try {
                const audio = await context.decodeAudioData(await data.blob.arrayBuffer());
                finish({ bytes: data.blob.size, duration: audio.duration, sampleRate: audio.sampleRate });
              } catch (e) { finish({ error: String(e) }); }
              finally { await context.close(); }
            }
          };
          const embedding = new Float32Array(512).fill(1 / Math.sqrt(512));
          runtime.postMessage({ id: 1, provider, config, text: 'Hello, welcome to this story.', voice: 'af_heart', speaker: btoa(String.fromCharCode(...new Uint8Array(embedding.buffer))) });
        });
      }, { provider, config: catalog.find((p) => p.name === provider).defaults, url: '/' + worker.replaceAll('\\', '/'), timeout: Number(process.env.NAST_TTS_MODEL_TIMEOUT_MS || 180000) });
      console.log(provider, result);
      if (result.error) failed = true;
      else assert.ok(result.bytes > 44 && result.duration > 0);
    }
    if (failed) process.exitCode = 1;
  } finally { await browser.close(); server.closeAllConnections(); await new Promise((r) => server.close(r)); }
}
main().catch((e) => { console.error(e); process.exitCode = 1; server.close(); });
