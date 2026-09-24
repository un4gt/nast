"""Visible browser workflows and responsive evidence, against isolated nast data."""
import re
from playwright.async_api import expect

from run import rpc, card


async def run_ui(suite):
    page = suite.nast
    page.set_default_timeout(12000)
    actor = await rpc(page, 'characters.create', {'card': card('UI 验收角色')})
    await rpc(page, 'chats.new', {'avatar': actor['avatar']})
    group = await rpc(page, 'groups.create', {'group': {'name': 'UI 城堡议事厅', 'members': [actor['avatar']], 'activation_strategy': 1}})
    await rpc(page, 'settings.save', {'settings': suite.settings})
    await rpc(page, 'worlds.save', {'name': 'UI 城堡世界书', 'book': {'entries': {'0': {
        'uid': 0, 'key': ['城堡'], 'comment': '城堡与守卫 Castle lore',
        'content': '山顶的城堡有一扇蓝色大门。守卫认识每一位远道而来的旅人。', 'order': 100}}}})
    directory = suite.artifact / 'ui'
    directory.mkdir(exist_ok=True)

    async def check(name, expected, actual):
        suite.compare('ui-checks/' + name, expected, actual)
    async def sidebar(width):
        if width < 768:
            await page.get_by_role('button', name='切换侧栏', exact=True).click()
    async def inspector(width):
        if width < 1280:
            await page.get_by_role('button', name='打开对话详情').click()
    async def capture(name):
        await page.screenshot(path=str(directory / (name + '.png')), full_page=True)
        overflow = await page.evaluate('document.documentElement.scrollWidth > innerWidth + 1')
        await check(name + '-horizontal-overflow', False, overflow)

    for theme in ['light', 'dark']:
        for width in [390, 768, 1440]:
            label = f'{theme}-{width}'
            await page.set_viewport_size({'width': width, 'height': 900})
            await page.evaluate('theme => localStorage.setItem("theme", theme)', theme)
            await page.reload()
            await sidebar(width)
            await page.locator('[data-sidebar="menu-button"]').filter(has_text='UI 验收角色').click()
            await expect(page.locator('h1')).to_have_text('UI 验收角色')
            composer = page.get_by_role('textbox', name='输入消息')
            await composer.fill('未发送的中文草稿')
            before = len(suite.server.captures)
            await composer.dispatch_event('keydown', {'key': 'Enter', 'code': 'Enter', 'keyCode': 229, 'isComposing': True, 'bubbles': True})
            await check(label + '-ime-keeps-draft', '未发送的中文草稿', await composer.input_value())
            await check(label + '-ime-no-request', before, len(suite.server.captures))
            await capture(label + '-private')
            await inspector(width)
            await page.get_by_role('tab', name='世界书', exact=True).click()
            await page.get_by_role('button', name='打开世界书管理').click()
            dialog = page.get_by_role('dialog', name='世界书管理')
            await dialog.get_by_role('combobox', name='选择世界书').click()
            await page.get_by_role('option', name='UI 城堡世界书', exact=True).click()
            keywords = dialog.get_by_role('textbox', name='主关键词', exact=True)
            await keywords.fill('城堡, 蓝色钥匙')
            # Escape while still focused must flush the latest keyword text.
            await keywords.press('Escape')
            await dialog.wait_for(state='hidden')
            book = await rpc(page, 'worlds.get', {'name': 'UI 城堡世界书'})
            await check(label + '-keyword-close-save', ['城堡', '蓝色钥匙'], book['entries']['0']['key'])
            await inspector(width)
            await page.get_by_role('tab', name='世界书', exact=True).click()
            await page.get_by_role('button', name='打开世界书管理').click()
            await dialog.locator('summary', has_text='高级激活规则').click()
            await dialog.get_by_role('spinbutton', name='扫描深度', exact=True).scroll_into_view_if_needed()
            await capture(label + '-world')
            await dialog.get_by_role('textbox', name='搜索世界书条目').press('Escape')
            await dialog.wait_for(state='hidden')
            await sidebar(width)
            await page.locator('[data-sidebar="menu-button"]').filter(has_text='UI 城堡议事厅').click()
            await page.locator('summary', has_text='群会话与世界书').click()
            await capture(label + '-group')
            await sidebar(width)
            await page.locator('[data-sidebar="menu-button"]').filter(has_text='UI 验收角色').click()
            await expect(page.locator('h1')).to_have_text('UI 验收角色')
            await check(label + '-draft-return', '未发送的中文草稿', await composer.input_value())
    await page.get_by_role('tab', name='角色', exact=True).click()
    await page.get_by_role('button', name=re.compile('备选开场白')).click()
    await page.get_by_role('button', name='添加开场白', exact=True).click()
    await page.get_by_role('textbox', name='备选开场白 1', exact=True).fill('另一种问候：欢迎回到城堡。')
    await page.get_by_role('button', name='角色名称', exact=True).click()
    await page.get_by_role('textbox', name='角色名称', exact=True).fill('UI 重新命名的角色')
    await page.get_by_role('button', name='保存卡片', exact=True).click()
    await expect(page.locator('h1')).to_have_text('UI 重新命名的角色')
    await check('rename-retains-message-draft', '未发送的中文草稿', await page.get_by_role('textbox', name='输入消息').input_value())
    characters = await rpc(page, 'characters.all')
    renamed = next(c for c in characters if c['name'] == 'UI 重新命名的角色')
    saved = await rpc(page, 'characters.get', {'avatar': renamed['avatar']})
    await check('card-rename-and-alternate-save', ['另一种问候：欢迎回到城堡。'], saved['data']['alternate_greetings'])
    await page.screenshot(path=str(directory / 'card-edited.png'), full_page=True)
    suite.report['ui_screenshots'] = str(directory)
