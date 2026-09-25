"""Model selection/settings workflows in both themes, desktop and narrow viewports."""
from playwright.async_api import expect
from run import rpc


async def check_models_ui(page, artifact, actor_name):
    page.set_default_timeout(12000)
    directory=artifact/'models-ui';directory.mkdir(exist_ok=True)
    errors=[]
    page.on('pageerror',lambda error:errors.append(str(error)))
    for theme in ['light','dark']:
        for width in [390,1440]:
            await page.set_viewport_size({'width':width,'height':900})
            await page.evaluate('theme=>localStorage.setItem("theme",theme)',theme)
            await page.reload()
            if width<768: await page.get_by_role('button',name='切换侧栏',exact=True).click()
            await page.locator('[data-sidebar="menu-button"]').filter(has_text=actor_name).first.click()
            selector=page.get_by_role('combobox',name='会话模型')
            await expect(selector).to_be_enabled()
            await selector.focus();await selector.press('Space')
            await page.get_by_role('option',name='第二模型',exact=True).click()
            await expect(selector).to_contain_text('第二模型')
            composer=page.get_by_role('textbox',name='输入消息')
            await composer.fill('/model');await composer.press('Enter')
            await expect(page.get_by_role('option',name='主模型',exact=True)).to_be_visible()
            await page.keyboard.press('Escape')
            assert not await page.evaluate('document.documentElement.scrollWidth>innerWidth+1')
            await page.screenshot(path=str(directory/f'{theme}-{width}-selector.png'),full_page=True)
            if width<768: await page.get_by_role('button',name='切换侧栏',exact=True).click()
            await page.get_by_role('button',name='设置',exact=True).click()
            await page.get_by_role('button',name='主模型 · 默认',exact=True).click()
            await page.get_by_role('textbox',name='显示名称',exact=True).fill('保留的模型草稿')
            catalog=await rpc(page,'model_catalog.get')
            await rpc(page,'model_catalog.save',{'catalog':catalog})
            await page.get_by_role('button',name='保存模型目录',exact=True).click()
            await expect(page.get_by_role('alert').filter(has_text='草稿已保留')).to_be_visible()
            await expect(page.get_by_role('textbox',name='显示名称',exact=True)).to_have_value('保留的模型草稿')
            assert not await page.evaluate('document.documentElement.scrollWidth>innerWidth+1')
            await page.screenshot(path=str(directory/f'{theme}-{width}-conflict.png'),full_page=True)
            await page.get_by_role('button',name='放弃草稿并重新加载',exact=True).click()
            await expect(page.get_by_role('textbox',name='显示名称',exact=True)).to_have_value('主模型')
            await page.get_by_role('textbox',name='显示名称',exact=True).press('Escape')
    assert not errors,errors
