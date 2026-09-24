"""Real ST browser scenarios; the model server never constructs either prompt."""
from __future__ import annotations

import base64
import copy
import difflib
import io
import json
import struct
import zipfile
import zlib

from model_server import canonical
from run import card, rpc, st_import, st_generate, write_json


async def st_post(page, endpoint, data):
    token = await page.request.get('/csrf-token')
    response = await page.request.post(endpoint, data=data, headers={
        'X-CSRF-Token': (await token.json())['token']})
    if not response.ok:
        raise RuntimeError(f'{endpoint}: {response.status}: {await response.text()}')
    return response


def zip_payload(files):
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, 'w', zipfile.ZIP_DEFLATED) as archive:
        for name, value in files.items():
            archive.writestr(name, value if isinstance(value, bytes) else canonical(value))
    return buffer.getvalue()


def png_payload(value):
    def chunk(kind, data):
        return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!2I5B', 1, 1, 8, 6, 0, 0, 0))
            + chunk(b'tEXt', b'chara\0' + base64.b64encode(canonical(value).encode()))
            + chunk(b'IDAT', zlib.compress(b'\0\xff\xff\xff\xff')) + chunk(b'IEND', b''))


def fixtures():
    for format in ['json', 'png', 'yaml', 'yml', 'charx', 'byaf']:
        value = card('Actor_' + format)
        if format in ['yaml', 'yml']:
            # Actual YAML, not JSON accepted by a YAML parser.
            payload = f'name: Actor_{format}\ncontext: 城堡的守卫\ngreeting: 欢迎来到城堡。\n'.encode()
        elif format == 'png':
            payload = png_payload(value)
        elif format == 'charx':
            payload = zip_payload({'card.json': value})
        elif format == 'byaf':
            payload = zip_payload({'manifest.json': {'characters': ['character.json'], 'scenarios': ['scenario.json']},
                'character.json': {'name': 'Actor_byaf', 'displayName': 'Actor_byaf', 'persona': '城堡的守卫'},
                'scenario.json': {'title': 'Castle', 'narrative': '城门前',
                    'firstMessages': [{'text': '欢迎来到城堡。'}], 'messages': []}})
        else:
            payload = canonical(value).encode()
        yield format, payload


def message_state(messages):
    # Intentional projection: compare conversational state, not wall clock/UI telemetry.
    identities = {}
    def batch(message):
        identity = (message.get('extra') or {}).get('gen_id')
        if identity is None:
            return None
        identity = str(identity)
        return identities.setdefault(identity, len(identities))
    return [{
        'name': m.get('name', ''), 'is_user': bool(m.get('is_user')),
        'is_system': bool(m.get('is_system')), 'mes': m.get('mes', ''),
        'swipes': m.get('swipes') or [], 'swipe_id': m.get('swipe_id') or 0,
        'reasoning': (m.get('extra') or {}).get('reasoning') or '',
        'model': (m.get('extra') or {}).get('model') or '',
        'api': (m.get('extra') or {}).get('api') or '',
        'original_avatar': m.get('original_avatar') or '',
        'batch': batch(m),
    } for m in messages]


def metadata_state(metadata):
    timed = copy.deepcopy(metadata.get('timedWorldInfo') or {'sticky': {}, 'cooldown': {}})
    # ST hashes raw, ordered JSON; nast hashes normalized typed data. Compare entry
    # identity and all interval/protection fields, and retain raw hashes in evidence.
    for effects in timed.values():
        for key, effect in effects.items():
            if 'hash' in effect:
                effect['hash'] = 'entry:' + key
    return {'variables': metadata.get('variables') or {},
        'world_info': metadata.get('world_info', metadata.get('world')),
        'timedWorldInfo': timed}


def character_state(value):
    data = value.get('data', value)
    fields = ['name', 'description', 'personality', 'scenario', 'first_mes', 'mes_example',
              'system_prompt', 'post_history_instructions', 'creator_notes']
    return {**{field: data.get(field) or '' for field in fields},
            'alternate_greetings': data.get('alternate_greetings') or [], 'tags': data.get('tags') or []}


class Suite:
    def __init__(self, st, nast, server, artifact, report, settings):
        self.st, self.nast, self.server = st, nast, server
        self.artifact, self.report, self.settings = artifact, report, settings

    def compare(self, name, left, right):
        directory = self.artifact / name
        write_json(directory / 'st.json', left)
        write_json(directory / 'nast.json', right)
        diff = '\n'.join(difflib.unified_diff(canonical(left).splitlines(), canonical(right).splitlines(), fromfile='ST', tofile='nast'))
        (directory / 'difference.diff').write_text(diff, encoding='utf-8')
        passed = canonical(left) == canonical(right)
        self.report['cases'].append({'name': name, 'passed': passed})
        print(('PASS ' if passed else 'FAIL ') + name, flush=True)
        return passed

    async def import_pair(self, payload, filename):
        st_avatar = await st_import(self.st, payload, filename)
        imported = await rpc(self.nast, 'characters.import', {'filename': filename,
            'data_base64': base64.b64encode(payload).decode()})
        await self.st.evaluate('''async avatar => {
            const {getContext}=await import('/scripts/st-context.js');
            await getContext().getCharacters();
            await getContext().selectCharacterById(getContext().characters.findIndex(c=>c.avatar===avatar));
        }''', st_avatar)
        return st_avatar, imported['avatar']

    async def private(self, name, pair, messages, setup=None, stream=False, after_round=None):
        st_avatar, nast_avatar = pair
        settings = copy.deepcopy(self.settings)
        settings['oai_settings']['stream_openai'] = stream
        await rpc(self.nast, 'settings.save', {'settings': settings})
        await self.st.evaluate('''async stream => {
            const {getContext}=await import('/scripts/st-context.js');
            getContext().chatCompletionSettings.stream_openai=stream;
        }''', stream)
        chat = await rpc(self.nast, 'chats.new', {'avatar': nast_avatar, 'greeting_index': 0})
        if setup:
            await setup(chat)
        for index, message in enumerate(messages):
            start = len(self.server.captures)
            st_state = await st_generate(self.st, st_avatar, message)
            result = await rpc(self.nast, 'generate.run', {'avatar': nast_avatar,
                'chat_file': chat['file_name'], 'type': 'normal', 'user_message': message})
            nast_state = await rpc(self.nast, 'chats.get', {'avatar': nast_avatar, 'file_name': chat['file_name']})
            self.generation(name + '-' + str(index + 1), start, st_state, nast_state, [result['text']])
            if after_round:
                await after_round(index)

    def generation(self, name, start, st_state, nast_state, replies, continued_from=''):
        captures = self.server.captures[start:]
        left = [c['body'] for c in captures if c['path'].startswith('/st/')]
        right = [c['body'] for c in captures if c['path'].startswith('/nast/')]
        write_json(self.artifact / name / 'st-raw-state.json', st_state)
        write_json(self.artifact / name / 'nast-raw-state.json', nast_state)
        self.compare(name + '/requests', left, right)
        self.compare(name + '/state', {'messages': message_state(st_state['chat']), 'metadata': metadata_state(st_state['metadata'])},
            {'messages': message_state(nast_state[1:]), 'metadata': metadata_state(nast_state[0]['chat_metadata'])})
        expected = [c['reply'] for c in captures if c['path'].startswith('/st/')]
        actual_st = [m['mes'] for m in st_state['chat'][-len(expected):]] if expected else []
        self.compare(name + '/replies', expected, replies)
        self.compare(name + '/st-saved-replies', [continued_from + (' ' if continued_from else '') + text for text in expected], actual_st)
        self.report['cases'].append({'name': name + '/request-count', 'passed': len(left) == len(right) == len(replies) and bool(left)})

    async def cards(self):
        for format, payload in fixtures():
            pair = await self.import_pair(payload, 'actor.' + format)
            st_card = await self.st.evaluate('''async () => {
                const {getContext}=await import('/scripts/st-context.js');
                return getContext().characters[getContext().characterId];
            }''')
            nast_card = await rpc(self.nast, 'characters.get', {'avatar': pair[1]})
            self.compare('cards-' + format + '/import', character_state(st_card), character_state(nast_card))
            await self.private('cards-' + format, pair, ['请描述城堡。'], stream=format == 'png')
            for exported_format in ['json', 'png']:
                st_export = await st_post(self.st, '/api/characters/export', {'avatar_url': pair[0], 'format': exported_format})
                nast_export = await rpc(self.nast, 'characters.export', {'avatar': pair[1], 'format': exported_format})
                # Both outputs must be accepted by the real ST importer.
                left_avatar = await st_import(self.st, await st_export.body(), 'left.' + exported_format)
                right_avatar = await st_import(self.st, base64.b64decode(nast_export['data_base64']), 'right.' + exported_format)
                exports = await self.st.evaluate('''async avatars => {
                    const {getContext}=await import('/scripts/st-context.js'); await getContext().getCharacters();
                    return avatars.map(a=>getContext().characters.find(c=>c.avatar===a));
                }''', [left_avatar, right_avatar])
                self.compare('cards-' + format + '/export-' + exported_format,
                    character_state(exports[0]), character_state(exports[1]))

    async def embedded(self):
        value = card('EmbeddedActor')
        value['data']['character_book'] = {'name': 'Embedded Castle', 'entries': [{
            'id': 7, 'keys': ['城堡'], 'content': '内嵌书：城堡的钥匙是蓝色的。',
            'enabled': True, 'insertion_order': 100, 'extensions': {}}]}
        pair = await self.import_pair(canonical(value).encode(), 'embedded.json')
        async def setup(chat):
            await self.st.evaluate('''async () => {
                const wi=await import('/scripts/world-info.js'); await wi.importEmbeddedWorldInfo(true);
                await wi.charUpdatePrimaryWorld('Embedded Castle');
            }''')
            await st_post(self.st, '/api/characters/merge-attributes', {
                'avatar': pair[0], 'data': {'extensions': {'world': 'Embedded Castle'}}})
            await rpc(self.nast, 'characters.import_book', {'avatar': pair[1]})
        await self.private('embedded-linked', pair, ['城堡的钥匙是什么颜色？', '再说一次城堡的钥匙。'], setup)

    async def worlds(self):
        pair = await self.import_pair(canonical(card('WorldActor')).encode(), 'world.json')
        books = {
            'Global Castle': {'entries': {'0': {'uid': 0, 'key': ['城堡'], 'content': '全局书：城堡建于山顶。', 'order': 100, 'position': 0}}},
            'Chat Castle': {'entries': {'0': {'uid': 0, 'key': ['/(?<=蓝色)钥匙/u'], 'content': '聊天书：蓝色钥匙打开北门。', 'order': 90, 'position': 1}}},
        }
        async def setup(chat):
            for name, book in books.items():
                await self.st.evaluate('''async ({name,book}) => {
                    const wi=await import('/scripts/world-info.js'); await wi.saveWorldInfo(name,book,true);
                }''', {'name': name, 'book': book})
                await rpc(self.nast, 'worlds.save', {'name': name, 'book': book})
            wi_settings = {'world_info': {'globalSelect': ['Global Castle']}}
            await self.st.evaluate('''async settings => {
                const wi=await import('/scripts/world-info.js');
                await wi.updateWorldInfoList(); wi.selected_world_info.splice(0);
                wi.setWorldInfoSettings(settings,{world_names:['Global Castle','Chat Castle']});
                const {getContext}=await import('/scripts/st-context.js');
                getContext().chatMetadata.world_info='Chat Castle'; await getContext().saveChat();
            }''', wi_settings)
            await rpc(self.nast, 'settings.save', {'settings': {**self.settings, 'world_info_settings': wi_settings}})
            await rpc(self.nast, 'chats.set_world', {'avatar': pair[1], 'file_name': chat['file_name'], 'world': 'Chat Castle'})
        await self.private('worlds-global-chat-regex', pair, ['城堡的蓝色钥匙在哪里？', '再说一次蓝色钥匙。'], setup)

        pair = await self.import_pair(canonical(card('TimedActor')).encode(), 'timed.json')
        async def setup_timed(chat):
            name = 'Timed World'
            book = {'entries': {
                '0': {'uid': 0, 'key': ['启动'], 'content': '限时设定：灯塔正在照亮海面。', 'sticky': 3, 'cooldown': 3, 'order': 100, 'position': 0},
                '1': {'uid': 1, 'key': ['开门'], 'content': '递归种子：银色钥匙。', 'order': 90, 'position': 0},
                '2': {'uid': 2, 'key': ['银色钥匙'], 'content': '递归结果：门后是花园。', 'order': 80, 'position': 1},
            }}
            wi_settings = {'world_info': {'globalSelect': [name]}, 'world_info_depth': 1,
                'world_info_recursive': True, 'world_info_character_strategy': 1}
            await self.st.evaluate('''async ({name,book,settings}) => {
                const wi=await import('/scripts/world-info.js'); await wi.saveWorldInfo(name,book,true);
                await wi.updateWorldInfoList(); wi.selected_world_info.splice(0);
                wi.setWorldInfoSettings(settings,{world_names:[name]});
            }''', {'name': name, 'book': book, 'settings': wi_settings})
            await rpc(self.nast, 'worlds.save', {'name': name, 'book': book})
            await rpc(self.nast, 'settings.save', {'settings': {**self.settings, 'world_info_settings': wi_settings}})
        await self.private('worlds-timed-recursion', pair, ['启动并开门', '继续描述海面', '继续描述海面', '继续描述海面', '再次启动'], setup_timed)

    async def macros(self):
        value = card('VariableActor')
        read_variables = '城门：{{getvar::gate}}；塔：{{getglobalvar::tower}}。'
        value['data']['description'] = '{{setvar::gate::blue}}{{setglobalvar::tower::north}}' + read_variables
        pair = await self.import_pair(canonical(value).encode(), 'variables.json')
        async def setup(chat):
            await self.st.evaluate('''async () => {
                const wi=await import('/scripts/world-info.js'); wi.selected_world_info.splice(0);
                const {getContext}=await import('/scripts/st-context.js'); getContext().extensionSettings.variables.global={};
            }''')
        async def after_round(index):
            st_globals = await self.st.evaluate('''async () => {
                const {getContext}=await import('/scripts/st-context.js'); return getContext().extensionSettings.variables.global;
            }''')
            settings = await rpc(self.nast, 'settings.get')
            self.compare(f'macros-{index+1}/globals', st_globals, settings['extension_settings']['variables']['global'])
            persisted = json.loads((self.artifact / 'nast-data' / 'default-user' / 'settings.json').read_text(encoding='utf-8'))
            self.compare(f'macros-{index+1}/persisted-globals', st_globals, persisted['extension_settings']['variables']['global'])
            if index == 0:
                await st_post(self.st, '/api/characters/merge-attributes', {'avatar': pair[0], 'description': read_variables, 'data': {'description': read_variables}})
                await rpc(self.nast, 'characters.edit', {'avatar': pair[1], 'data': {'description': read_variables}})
        await self.private('macros-persistent', pair, ['城门与塔在哪里？', '再确认一次。'], setup, after_round=after_round)

    async def groups(self):
        # Explicit list activation eliminates random speaker selection from the oracle.
        for mode in [0, 1, 2]:
            pairs = []
            for i in [0, 1]:
                value = card(f'Group{mode}Actor{i}')
                book_name = f'Group{mode}Book{i}'
                value['data']['character_book'] = {'name': book_name, 'entries': [{
                    'id': 0, 'keys': ['城堡'], 'content': f'角色书：成员{i}掌管第{i}座塔。',
                    'enabled': True, 'insertion_order': 100, 'extensions': {}}]}
                pair = await self.import_pair(canonical(value).encode(), 'group.json')
                await self.st.evaluate('''async () => { const wi=await import('/scripts/world-info.js'); await wi.importEmbeddedWorldInfo(true); }''')
                await st_post(self.st, '/api/characters/merge-attributes', {'avatar': pair[0], 'data': {'extensions': {'world': book_name}}})
                await rpc(self.nast, 'characters.import_book', {'avatar': pair[1]})
                pairs.append(pair)
            group = {'name': f'Parity Group {mode}', 'members': [p[0] for p in pairs],
                'activation_strategy': 1, 'generation_mode': mode, 'allow_self_responses': True,
                'generation_mode_join_prefix': '[<FIELDNAME>] ', 'generation_mode_join_suffix': ''}
            st_group = await (await st_post(self.st, '/api/groups/create', group)).json()
            nast_group = await rpc(self.nast, 'groups.create', {'group': {**group, 'members': [p[1] for p in pairs]}})
            await rpc(self.nast, 'settings.save', {'settings': self.settings})
            await self.st.evaluate('''async id => {
                const {getContext}=await import('/scripts/st-context.js'); await getContext().getCharacters();
                const wi=await import('/scripts/world-info.js'); wi.selected_world_info.splice(0);
                const groups=await import('/scripts/group-chats.js'); await groups.getGroups(); await groups.openGroupById(id);
            }''', st_group['id'])
            await rpc(self.nast, 'groups.get_chat', {'chat_id': nast_group['chat_id']})
            shared_name = f'Group{mode}Shared'
            shared_book = {'entries': {'0': {'uid': 0, 'key': ['城堡'], 'content': '群会话书：城堡在雪山上。', 'position': 0, 'order': 100}}}
            await self.st.evaluate('''async ({name,book}) => {
                const wi=await import('/scripts/world-info.js'); await wi.saveWorldInfo(name,book,true); await wi.updateWorldInfoList();
                const {getContext}=await import('/scripts/st-context.js'); getContext().chatMetadata.world_info=name;
            }''', {'name': shared_name, 'book': shared_book})
            await rpc(self.nast, 'worlds.save', {'name': shared_name, 'book': shared_book})
            await rpc(self.nast, 'groups.set_world', {'id': nast_group['id'], 'chat_id': nast_group['chat_id'], 'world': shared_name})
            global_name, auxiliary_name = f'Group{mode}Global', f'Group{mode}Auxiliary'
            for name, content, order in [(global_name, '全局设定：城堡的钟每天响三次。', 70),
                                          (auxiliary_name, '辅助角色书：成员零持有钟楼地图。', 60)]:
                book = {'entries': {'0': {'uid': 0, 'key': ['城堡'], 'content': content, 'position': 0, 'order': order}}}
                await self.st.evaluate('''async ({name,book}) => {
                    const wi=await import('/scripts/world-info.js'); await wi.saveWorldInfo(name,book,true);
                }''', {'name': name, 'book': book})
                await rpc(self.nast, 'worlds.save', {'name': name, 'book': book})
            wi_settings = {'world_info_depth': 2, 'world_info_recursive': False, 'world_info_character_strategy': 1,
                'world_info': {'globalSelect': [global_name], 'charLore': [{'name': pairs[0][0].removesuffix('.png'), 'extraBooks': [auxiliary_name]}]}}
            await self.st.evaluate('''async settings => {
                const wi=await import('/scripts/world-info.js'); await wi.updateWorldInfoList(); wi.selected_world_info.splice(0);
                wi.setWorldInfoSettings(settings,{world_names:wi.world_names});
            }''', wi_settings)
            nast_wi = copy.deepcopy(wi_settings)
            nast_wi['world_info']['charLore'][0]['name'] = pairs[0][1].removesuffix('.png')
            await rpc(self.nast, 'settings.save', {'settings': {**self.settings, 'world_info_settings': nast_wi}})
            start = len(self.server.captures)
            st_state = await self.st.evaluate('''async () => {
                const {getContext}=await import('/scripts/st-context.js');
                const input=document.getElementById('send_textarea'); input.value='请介绍这座城堡。';
                input.dispatchEvent(new Event('input',{bubbles:true}));
                await getContext().generate('normal'); await getContext().saveChat();
                return {chat:JSON.parse(JSON.stringify(getContext().chat)),metadata:JSON.parse(JSON.stringify(getContext().chatMetadata))};
            }''')
            result = await rpc(self.nast, 'generate.group', {'id': nast_group['id'], 'chat_id': nast_group['chat_id'], 'user_message': '请介绍这座城堡。'})
            nast_state = await rpc(self.nast, 'groups.get_chat', {'chat_id': nast_group['chat_id']})
            self.generation('groups-mode-' + str(mode), start, st_state, nast_state, [r['text'] for r in result['replies']])
            for action in ['regenerate', 'swipe', 'continue']:
                start = len(self.server.captures)
                prefix = st_state['chat'][-1]['mes'] if action == 'continue' else ''
                st_state = await self.st.evaluate('''async action => {
                    const {getContext}=await import('/scripts/st-context.js');
                    if(action==='regenerate') { const groups=await import('/scripts/group-chats.js'); await groups.regenerateGroup(); }
                    else if(action==='swipe') { const script=await import('/script.js'); await script.swipe_right(); }
                    else await getContext().generate(action);
                    await getContext().saveChat();
                    return {chat:JSON.parse(JSON.stringify(getContext().chat)),metadata:JSON.parse(JSON.stringify(getContext().chatMetadata))};
                }''', action)
                result = await rpc(self.nast, 'generate.group', {'id': nast_group['id'], 'chat_id': nast_group['chat_id'], 'type': action})
                nast_state = await rpc(self.nast, 'groups.get_chat', {'chat_id': nast_group['chat_id']})
                self.generation(f'groups-mode-{mode}-{action}', start, st_state, nast_state, [r['text'] for r in result['replies']], prefix)


async def run_cases(st, nast, server, artifact, report, settings, stage):
    report['comparison_scope'] = {'requests': 'complete parsed JSON',
        'state': 'message roles/names/text/swipes/reasoning/provider/model/source-avatar/batch relationships; local variables, chat world and timed effects',
        'excluded': 'wall-clock timestamps, UI telemetry, generated IDs; timed hashes normalized to world/uid because serialization differs; raw states retained; exhaustive advanced scenarios pending'}
    suite = Suite(st, nast, server, artifact, report, settings)
    async def management():
        from management import run_management
        await run_management(suite)
    suite.management = management
    async def ui():
        from ui_checks import run_ui
        await run_ui(suite)
    suite.ui = ui
    for name in ['cards', 'embedded', 'worlds', 'macros', 'groups', 'management', 'ui']:
        if stage in ['all', name]:
            try:
                await getattr(suite, name)()
            except Exception as error:
                report['cases'].append({'name': name + '/execution', 'passed': False, 'error': str(error)})
                print(f'FAIL {name}: {error}', flush=True)
