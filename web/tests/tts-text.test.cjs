const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const Module = require('node:module');
const ts = require('typescript');

function load(file) {
  const filename = path.resolve(__dirname, '../src/tts', file);
  const output = ts.transpileModule(fs.readFileSync(filename, 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2021, esModuleInterop: true } }).outputText;
  const mod = new Module(filename, module); mod.filename = filename; mod.paths = module.paths;
  mod._compile(output, filename); return mod.exports;
}
const { defaults, parseVoiceMap, resolveVoice, providerConfig } = load('config.ts');
const { prepareText, splitLongText, streamingBoundary, quotedBlocks, parseFilter } = load('text.ts');
const prepare = (text, settings = {}) => prepareText(text, { ...defaults, ...settings }, '小雨', '访客');

test('ST quote matching retains nested Chinese / Japanese dialogue and falls back without quotes', () => {
  assert.deepEqual(quotedBlocks('她说：“你好，『朋友』。” 然后说「再见」。'), ['“你好，『朋友』。”', '「再见」']);
  assert.equal(prepare('只有旁白', { narrate_quoted_only: true })[0].text, '只有旁白');
  assert.equal(prepare('*笑* “你好”', { narrate_quoted_only: true, narrate_dialogues_only: true })[0].text, '“你好”');
});
test('macros, fenced code, images, tags and asterisk filters preserve speech content', () => {
  const result = prepare('小雨: {{user}}，你好。\n```js\nalert(1)\n```\n<think>秘密推理</think><hidden>隐藏</hidden>\n*点头* ![图片](a.png)', { skip_codeblocks: true, skip_tags: true, narrate_dialogues_only: true });
  assert.equal(result[0].text, '访客，你好。');
});
test('three segment voices keep dialogue, action and narrative in order', () => {
  assert.deepEqual(prepare('“你好” *点头* 风吹过。', { multi_voice_enabled: true, pass_asterisks: true }), [
    { type: 'dialogue', text: '你好' }, { type: 'action', text: '点头' }, { type: 'other', text: '风吹过。' },
  ]);
  const tts = { ...defaults, multi_voice_enabled: true };
  const cfg = { voiceMap: { '[Default Voice]': 'alloy', '小雨 ("Quotes")': 'nova', '小雨 (*Text inside asterisks*)': 'disabled' } };
  assert.equal(resolveVoice(tts, cfg, '小雨', 'dialogue'), 'nova');
  assert.equal(resolveVoice(tts, cfg, '小雨', 'action'), 'disabled');
  assert.equal(resolveVoice(tts, cfg, '小雨', 'other'), 'alloy');
});
test('paragraph segmentation does not expose lines inside skipped code blocks', () => {
  assert.deepEqual(prepare('第一段。\n```\n秘密\n```\n第二段。', { skip_codeblocks: true, narrate_by_paragraphs: true }).map((s) => s.text), ['第一段。', '第二段。']);
});
test('stream boundaries hold incomplete quotes, tags and reasoning until balanced', () => {
  const cfg = { ...defaults, multi_voice_enabled: true, skip_tags: true };
  for (const text of ['“未完成\n', '```js\nsecret\n', '<hidden>secret\n', '<think>private\n']) assert.equal(streamingBoundary(text, cfg), 0);
  assert.equal(streamingBoundary('完整段落。\n后续', cfg), 6);
  assert.equal(streamingBoundary('“第一行\n第二行”\n', cfg), 10);
});
test('regex literals and raw patterns remove content; invalid patterns are rejected', () => {
  assert.equal(prepare('hello [remove] world', { apply_regex: true, regex_pattern: '/\u005c[.*?\u005c]/g' })[0].text, 'hello world');
  assert.equal('aa'.replace(parseFilter('a'), ''), '');
  assert.throws(() => parseFilter('/[/g'));
});
test('long Unicode text splits without breaking surrogate pairs or losing characters', () => {
  const text = '你好😀'.repeat(180);
  const parts = splitLongText(text, 200);
  assert.equal(parts.join(''), text);
  assert.ok(parts.every((part) => Array.from(part).length <= 200 && !/[\uD800-\uDBFF]$/.test(part)));
});
test('legacy voice maps and per-provider settings remain compatible', () => {
  assert.deepEqual(parseVoiceMap('[Default Voice]:Alloy, 小雨:Nova'), { '[Default Voice]': 'Alloy', 小雨: 'Nova' });
  const cfg = providerConfig({ ...defaults, currentProvider: 'OpenAI', OpenAI: { model: 'gpt-4o-mini-tts', voiceMap: '小雨:Nova', characterInstructions: { 小雨: '温柔' } } });
  assert.equal(cfg.model, 'gpt-4o-mini-tts'); assert.equal(cfg.voiceMap.小雨, 'Nova'); assert.equal(cfg.speed, 1);
});
