"""Real Rust + rebuilt UI round trips, with isolated data and a local model service.

Run after npm --prefix web run build:
  .cache/st-parity-venv/Scripts/python.exe tests/parity/frontend_integration.py
"""
import asyncio
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading

from playwright.async_api import async_playwright, expect
from model_server import ModelServer
from run import ROOT, card, port, rpc, wait_http


async def main():
    artifact = Path(tempfile.mkdtemp(prefix='nast-frontend-integration-'))
    print(f'Evidence: {artifact}', flush=True)
    model = ModelServer()
    threading.Thread(target=model.serve_forever, daemon=True).start()
    endpoint = f'http://127.0.0.1:{model.server_port}/v1'
    url = f'http://127.0.0.1:{port()}'
    env = {**os.environ, 'NAST_PORT': url.rsplit(':', 1)[1], 'NAST_BIND': '127.0.0.1',
           'NAST_DATA': str(artifact/'data'), 'NAST_WEB': str(ROOT/'web/dist'),
           'NAST_USERNAME': 'integration', 'NAST_PASSWORD': 'isolated-integration-password',
           'NAST_PUBLIC_ORIGIN': url, 'NAST_COOKIE_SECURE': 'false', 'NAST_ALLOW_ANONYMOUS': 'false'}
    log = (artifact/'server.log').open('w', encoding='utf-8')
    process = subprocess.Popen([str(ROOT/'target/debug/nast.exe')], cwd=artifact, env=env, stdout=log, stderr=log)
    results, errors, rpc_errors = [], [], []
    def passed(name):
        results.append(name)
        print('PASS', name, flush=True)
    try:
        await wait_http(url+'/healthz', process)
        async with async_playwright() as pw:
            browser = await pw.chromium.launch()
            context = await browser.new_context(viewport={'width':1440, 'height':1000}, reduced_motion='reduce')
            page = await context.new_page()
            page.set_default_timeout(10000)
            page.on('pageerror', lambda e: errors.append(str(e)))
            def watch_socket(ws):
                def received(raw):
                    reply = json.loads(raw)
                    if reply.get('error'): rpc_errors.append(reply['error']['code'])
                ws.on('framereceived', received)
            page.on('websocket', watch_socket)
            await page.goto(url)
            await page.get_by_label('用户名', exact=True).fill('integration')
            await page.get_by_label('密码', exact=True).fill('wrong')
            await page.get_by_label('密码', exact=True).press('Enter')
            await expect(page.get_by_role('alert')).to_be_visible()
            await page.get_by_label('密码', exact=True).fill(env['NAST_PASSWORD'])
            await page.get_by_label('密码', exact=True).press('Enter')
            await expect(page.get_by_role('status', name='已连接到服务端')).to_be_visible()
            assert (await (await page.request.get(url+'/api/auth/session')).json())['authenticated']
            passed('Login errors, cookie session and real WebSocket')

            actor = await rpc(page, 'characters.create', {'card':card('IntegrationActor')})
            avatar = actor['avatar']
            await page.reload()
            await page.locator('[data-sidebar="menu-button"]').filter(has_text='IntegrationActor').first.click()
            await page.get_by_role('button', name='编辑角色卡', exact=True).click()
            await page.get_by_label('描述（Description）', exact=True).fill('UI saved character description')
            await page.get_by_label('角色标签', exact=True).fill('integration, roundtrip')
            await page.get_by_role('button', name='保存卡片', exact=True).click()
            await expect(page.get_by_role('button', name='保存卡片', exact=True)).to_be_disabled()
            saved = await rpc(page, 'characters.get', {'avatar':avatar})
            assert saved['data']['description'] == 'UI saved character description'
            assert saved['data']['tags'] == ['integration','roundtrip']
            await page.get_by_label('角色名称', exact=True).fill('IntegrationRenamed')
            await page.get_by_role('button', name='保存卡片', exact=True).click()
            await expect(page.locator('h1')).to_have_text('IntegrationRenamed')
            avatar = next(c['avatar'] for c in await rpc(page,'characters.all') if c['name']=='IntegrationRenamed')
            await page.get_by_role('dialog').get_by_role('button', name='关闭', exact=True).first.click()
            await expect(page.get_by_role('dialog')).to_have_count(0)
            assert not rpc_errors, rpc_errors
            passed('Character description/tags save and rename with chat migration')

            await page.get_by_role('button', name='设置', exact=True).click()
            await page.get_by_role('button', name='采样参数', exact=True).click()
            await page.get_by_role('switch', name='解除上下文上限校验', exact=True).click()
            await page.get_by_role('button', name='保存更改', exact=True).click()
            await expect(page.get_by_role('button', name='保存更改', exact=True)).to_be_disabled()
            assert (await rpc(page,'settings.get'))['oai_settings']['max_context_unlocked'] is True
            await page.get_by_role('button', name='模型', exact=True).click()
            await page.get_by_role('button', name='编辑 默认模型', exact=True).click()
            await page.get_by_label('API 地址', exact=True).fill(endpoint)
            await page.get_by_label('模型 ID', exact=True).fill('gpt-4o')
            await page.get_by_role('button', name='保存模型', exact=True).click()
            await expect(page.get_by_role('dialog', name='编辑模型', exact=True)).to_have_count(0)
            catalog = await rpc(page,'model_catalog.get')
            assert catalog['models'][0]['routes'][0]['config']['endpoint'] == endpoint
            await page.get_by_role('dialog', name='设置', exact=True).get_by_role('button',name='关闭',exact=True).click()
            passed('Settings and model editor save to real backend')

            composer = page.get_by_role('textbox',name='输入消息')
            await composer.fill('前后端集成验证')
            await composer.press('Enter')
            await expect(page.get_by_text('收到上下文：',exact=False).first).to_be_visible(timeout=30000)
            await expect(composer).to_be_enabled(timeout=30000)
            chats = await rpc(page,'characters.chats',{'avatar':avatar})
            messages = await rpc(page,'chats.get',{'avatar':avatar,'file_name':chats[-1]})
            assert messages[-2]['mes'] == '前后端集成验证'
            assert '收到上下文：' in messages[-1]['mes']
            assert model.captures
            article = page.get_by_role('article').last
            await article.hover()
            await article.get_by_role('button',name='编辑消息').click()
            await article.get_by_role('textbox').fill('UI edited response')
            await article.get_by_role('button',name='保存',exact=True).click()
            await expect(article.get_by_role('textbox')).to_have_count(0)
            assert (await rpc(page,'chats.get',{'avatar':avatar,'file_name':chats[-1]}))[-1]['mes'] == 'UI edited response'
            passed('UI send -> real Rust -> local model -> persisted reply, then message edit')

            await page.get_by_role('tab',name='世界书',exact=True).click()
            await page.get_by_role('button',name='打开世界书管理',exact=True).click()
            await page.get_by_role('textbox',name='新书名称',exact=True).fill('IntegrationWorld')
            await page.get_by_role('button',name='新建',exact=True).click()
            await page.get_by_role('button',name='添加条目',exact=True).click()
            await page.get_by_role('textbox',name='条目标题',exact=True).fill('Roundtrip entry')
            await page.get_by_role('textbox',name='主关键词',exact=True).fill('castle, 城堡')
            # Closing while keyword input is focused must flush its onBlur update.
            await page.get_by_role('dialog').get_by_role('button',name='关闭',exact=True).click()
            await expect(page.get_by_role('dialog')).to_have_count(0)
            book = await rpc(page,'worlds.get',{'name':'IntegrationWorld'})
            assert book['entries']['0']['key'] == ['castle','城堡']
            assert book['entries']['0']['comment'] == 'Roundtrip entry'
            passed('World creation and close-to-save keyword round trip')

            # Delay only character reads, while forwarding all requests to the real server.
            delayed = {'enabled':True}
            async def route(socket):
                server = socket.connect_to_server()
                ids = set()
                def sent(raw):
                    request = json.loads(raw)
                    if request.get('method') == 'characters.get' and delayed['enabled']: ids.add(request['id'])
                    server.send(raw)
                def received(raw):
                    reply = json.loads(raw)
                    if reply.get('id') in ids:
                        ids.remove(reply['id'])
                        asyncio.get_running_loop().call_later(2, socket.send, raw)
                    else:
                        socket.send(raw)
                socket.on_message(sent)
                server.on_message(received)
            await page.route_web_socket('**/ws', route)
            await page.reload()
            await page.locator('[data-sidebar="menu-button"]').filter(has_text='IntegrationRenamed').first.click()
            edit = page.get_by_role('button',name='编辑角色卡',exact=True)
            await expect(edit).to_be_disabled()
            await expect(edit).to_be_enabled()
            delayed['enabled'] = False
            passed('Delayed character read cannot overwrite unloaded fields')
            slow = await context.new_page()
            slow.set_default_timeout(10000)
            async def slow_connect(socket):
                await asyncio.sleep(2)
                socket.connect_to_server()
            await slow.route_web_socket('**/ws', slow_connect)
            await slow.goto(url)
            await slow.get_by_role('button',name='设置',exact=True).click()
            await expect(slow.get_by_role('button',name='编辑 默认模型',exact=True)).to_be_visible()
            assert await slow.get_by_role('alert').count() == 0
            await slow.close()
            passed('Model settings recover when opened before WebSocket connects')
            await page.screenshot(path=str(artifact/'verified.png'))
            await page.get_by_role('button',name='退出登录',exact=True).click()
            await expect(page.get_by_role('heading',name='欢迎回到 NAST')).to_be_visible()
            assert not (await (await page.request.get(url+'/api/auth/session')).json())['authenticated']
            passed('Logout revokes cookie session')
            assert not errors, errors
            assert not rpc_errors, rpc_errors
            await browser.close()
        (artifact/'report.json').write_text(json.dumps({'passed':results,'browser_errors':errors},ensure_ascii=False,indent=2),encoding='utf-8')
    finally:
        process.terminate()
        process.wait(timeout=10)
        model.shutdown()
        model.server_close()
        log.close()


if __name__ == '__main__':
    asyncio.run(main())
