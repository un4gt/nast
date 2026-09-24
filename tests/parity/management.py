"""Application invariants for destructive/reference-updating library operations.

These checks are deliberately labelled nast-only, not SillyTavern differential tests.
All files belong to the runner's disposable data directory.
"""
import base64
import json

from run import rpc, card


async def run_management(suite):
    page = suite.nast
    async def call(method, **params):
        return await rpc(page, method, params)
    async def rejected(method, **params):
        try:
            await call(method, **params)
        except Exception:
            return True
        return False
    def check(name, expected, actual):
        suite.compare('nast-management/' + name, expected, actual)

    actor = await call('characters.create', card=card('ManagementActor'))
    avatar = actor['avatar']
    private = await call('chats.new', avatar=avatar)
    group = await call('groups.create', group={'name': 'Management Group', 'members': [avatar], 'disabled_members': [avatar]})
    first = group['chat_id']
    initial = await call('groups.get_chat', chat_id=first)
    second = await call('groups.new_chat', id=group['id'])
    check('new-preserves-original', True, first in second['chats'] and len(second['chats']) == 2)
    renamed = await call('groups.rename_chat', id=group['id'], name='Managed session')
    check('rename-current', 'Managed session', renamed['chat_id'])
    opened = await call('groups.open_chat', id=group['id'], chat_id=first)
    check('open-original', initial, await call('groups.get_chat', chat_id=opened['chat_id']))
    exported = await call('groups.export_chat', id=group['id'])
    text = base64.b64decode(exported['data_base64']).decode()
    imported = await call('groups.import_chat', id=group['id'], text=text)
    copy = await call('groups.get_chat', chat_id=imported['chat_id'])
    check('import-messages', initial[1:], copy[1:])
    check('import-fresh-integrity', True, copy[0]['chat_metadata']['integrity'] != initial[0]['chat_metadata']['integrity'])
    foreign = await call('groups.create', group={'name': 'Other Group', 'members': [avatar]})
    check('ownership-rejected', True, await rejected('groups.open_chat', id=group['id'], chat_id=foreign['chat_id']))
    check('path-traversal-rejected', True, await rejected('groups.rename_chat', id=group['id'], name='../outside'))
    check('invalid-import-rejected', True, await rejected('groups.import_chat', id=group['id'], text='{"mes":42}'))
    check('invalid-header-rejected', True, await rejected('groups.import_chat', id=group['id'], text='{"chat_metadata":42}'))
    removed_id = imported['chat_id']
    remaining = await call('groups.delete_chat', id=group['id'], chat_id=removed_id)
    check('delete-removes-reference', False, removed_id in remaining['chats'])
    backups = list((suite.artifact / 'nast-data').rglob('group-*-' + removed_id + '.jsonl'))
    check('delete-archived', 1, len(backups))

    stem = avatar.removesuffix('.png')
    settings = {**suite.settings, 'world_info_settings': {'world_info': {'charLore': [{'name': stem, 'extraBooks': ['Rename World']}]}},
        'extension_settings': {'character_allowed_regex': [avatar]}, 'tag_map': {avatar: ['test-tag']}}
    await call('settings.save', settings=settings)
    await call('worlds.save', name='Rename World', book={'entries': {'0': {'uid': 0, 'content': 'reference', 'characterFilter': {'names': [stem], 'tags': [], 'isExclude': False}}}})
    renamed = await call('characters.rename', avatar=avatar, name='Renamed Actor')
    new_avatar = renamed['avatar']
    check('rename-card', 'Renamed Actor', (await call('characters.get', avatar=new_avatar))['data']['name'])
    check('rename-removes-old-card', True, await rejected('characters.get', avatar=avatar))
    check('rename-private-path', [private['file_name']], await call('characters.chats', avatar=new_avatar))
    chat = await call('chats.get', avatar=new_avatar, file_name=private['file_name'])
    check('rename-private-speaker', 'Renamed Actor', chat[-1]['name'])
    groups = await call('groups.all')
    updated = next(g for g in groups if g['id'] == group['id'])
    check('rename-group-references', [[new_avatar], [new_avatar]], [updated['members'], updated['disabled_members']])
    chat = await call('groups.get_chat', chat_id=first)
    check('rename-group-message', ['Renamed Actor', new_avatar], [chat[-1]['name'], chat[-1]['original_avatar']])
    settings = await call('settings.get')
    check('rename-settings', [[new_avatar], ['test-tag'], new_avatar.removesuffix('.png')], [settings['extension_settings']['character_allowed_regex'], settings['tag_map'][new_avatar], settings['world_info_settings']['world_info']['charLore'][0]['name']])
    book = await call('worlds.get', name='Rename World')
    check('rename-world-filter', [new_avatar.removesuffix('.png')], book['entries']['0']['character_filter']['names'])
    check('rename-backup', True, bool(list((suite.artifact / 'nast-data').rglob('character-rename-*'))))

    # Selecting an existing candidate must restore its own reasoning, not the previous candidate's.
    imported = await call('groups.import_chat', id=group['id'], text=json.dumps({'name': 'Renamed Actor', 'is_user': False,
        'mes': 'second', 'swipe_id': 1, 'swipes': ['first', 'second'], 'extra': {'reasoning': 'second thought'},
        'swipe_info': [{'extra': {'reasoning': 'first thought'}}, {'extra': {'reasoning': 'second thought'}}]}))
    await call('groups.swipe', id=group['id'], direction='left')
    chat = await call('groups.get_chat', chat_id=imported['chat_id'])
    check('candidate-restores-metadata', ['first', 0, 'first thought'], [chat[-1]['mes'], chat[-1]['swipe_id'], chat[-1]['extra']['reasoning']])
    await call('groups.swipe', id=group['id'], direction='right')
    chat = await call('groups.get_chat', chat_id=imported['chat_id'])
    check('candidate-roundtrip', ['second', 1, 'second thought'], [chat[-1]['mes'], chat[-1]['swipe_id'], chat[-1]['extra']['reasoning']])

    for path in ['/assets/settings.json', '/assets/user/secrets.json', '/assets/characters/../settings.json']:
        response = await page.request.get(path)
        check('asset-boundary-' + path.replace('/', '_'), True, response.status in [400, 403, 404])

    from cases import png_payload, zip_payload
    asset_card = card('AssetActor')
    asset_card['data']['assets'] = [{'type': 'emotion', 'name': 'happy', 'ext': 'png', 'uri': 'embeded://assets/happy.png'}]
    asset_bytes = png_payload(card('Image'))
    imported = await call('characters.import', filename='assets.charx', data_base64=base64.b64encode(
        zip_payload({'card.json': asset_card, 'assets/happy.png': asset_bytes})).decode())
    check('asset-import-count', 1, len(imported['resources']))
    response = await page.request.get('/assets/' + imported['resources'][0]['path'])
    check('asset-http-status', 200, response.status)
    check('asset-content', True, await response.body() == asset_bytes)
