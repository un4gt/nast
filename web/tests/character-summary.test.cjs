const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const Module = require('node:module');
const ts = require('typescript');

const filename = path.resolve(__dirname, '../src/lib/character-summary.ts');
const output = ts.transpileModule(fs.readFileSync(filename, 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2021 },
}).outputText;
const loaded = new Module(filename, module);
loaded.filename = filename;
loaded.paths = module.paths;
loaded._compile(output, filename);
const { normalizeCharacterSummary } = loaded.exports;

test('unreadable cards keep their identity and error without missing render fields', () => {
  const input = { avatar: '旧角色 #1.png', error: 'duplicate field `selectiveLogic`' };
  const result = normalizeCharacterSummary(input);
  assert.deepEqual(result, {
    avatar: input.avatar,
    avatarUrl: '/thumbnail?file=' + encodeURIComponent(input.avatar),
    name: '旧角色 #1', description: '', tags: [], fav: false, chat: null,
    error: input.error,
  });
  assert.deepEqual(Object.keys(input), ['avatar', 'error']);
});

test('valid character summaries preserve display and chat metadata', () => {
  const input = { avatar: 'actor.png', name: 'Actor', description: 'Description', tags: ['castle'], fav: true, chat: 'current' };
  const result = normalizeCharacterSummary(input);
  assert.deepEqual(result, { ...input, avatarUrl: '/thumbnail?file=actor.png', error: undefined });
});

test('optional or malformed display metadata cannot break search and rendering', () => {
  const result = normalizeCharacterSummary({ avatar: 'actor.png', name: '', tags: null });
  assert.equal(result.name, 'actor');
  assert.deepEqual(result.tags, []);
  assert.ok(result.error);
  assert.deepEqual(normalizeCharacterSummary({ avatar: 'actor.png', name: 'Actor', tags: ['castle', null, 42] }).tags, ['castle']);
});
