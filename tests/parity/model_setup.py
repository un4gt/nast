"""Real insecure-HTTP onboarding, with actual UI writes and captured upstream requests.
Run after cargo build and npm --prefix web run build. No real credentials/data.
"""
import asyncio
import copy
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
from playwright.async_api import async_playwright, expect
from model_server import ModelServer
from run import ROOT, card, port, rpc, wait_http, write_json


async def choose(page, label, option):
    await page.get_by_role('combobox', name=label, exact=True).click()
    await page.get_by_role('option', name=option, exact=True).click()


async def settings(page, width):
    if width < 768:
        await page.get_by_role('button', name='切换侧栏', exact=True).click()
    await page.get_by_role('button', name='设置', exact=True).click()


async def main():
    artifact = Path(tempfile.mkdtemp(prefix='nast-model-setup-'))
    print('Evidence:', artifact, flush=True)
    server = ModelServer(output=artifact/'requests.jsonl')
    threading.Thread(target=server.serve_forever, daemon=True).start()
    endpoint = f'http://127.0.0.1:{server.server_port}'
    http_port = port()
    # A real LAN address gives Chromium an insecure origin; localhost is trustworthy.
    host = os.environ.get('NAST_E2E_HTTP_HOST') or socket.gethostbyname(socket.gethostname())
    assert not host.startswith('127.') and host != 'localhost', 'Set NAST_E2E_HTTP_HOST to a non-loopback IPv4 address'
    url = f'http://{host}:{http_port}'
    env = {**os.environ, 'NAST_BIND':'0.0.0.0', 'NAST_PORT':str(http_port),
           'NAST_DATA':str(artifact/'data'), 'NAST_WEB':str(ROOT/'web/dist'),
           'NAST_USERNAME':'admin', 'NAST_PASSWORD':'isolated-test-password',
           'NAST_ALLOW_ANONYMOUS':'false', 'NAST_PUBLIC_ORIGIN':'',
           'NAST_COOKIE_SECURE':'false', 'NAST_BRIDGE_TOKEN':'', 'OPENAI_API_KEY':'', 'NAST_OPENAI_BASE':''}
    # A legacy key must never be used for newly created configurations.
    write_json(artifact/'data/default-user/secrets.json', {'api_key_custom':[{'active':True,'value':'legacy-secret'}]})
    log = (artifact/'server.log').open('w', encoding='utf-8')
    binary = ROOT/'target/debug'/('nast.exe' if os.name == 'nt' else 'nast')
    process = subprocess.Popen([str(binary)], cwd=artifact, env=env, stdout=log, stderr=log)
    expect.set_options(timeout=20000)
    results, errors = [], []
    page = None
    try:
        await wait_http(f'http://127.0.0.1:{http_port}/healthz', process)
        async with async_playwright() as pw:
            browser = await pw.chromium.launch(args=['--no-proxy-server'])
            for theme, width, protocol, mode in [
                ('light',1440,'openai','high'), ('dark',390,'anthropic','enabled'),
                ('light',390,'gemini','level'), ('dark',1440,'anthropic','adaptive'),
            ]:
                name = f'{protocol}-{mode}-{theme}-{width}'
                context = await browser.new_context(viewport={'width':width,'height':900})
                page = await context.new_page()
                page.set_default_timeout(15000)
                page.on('pageerror', lambda e: errors.append(str(e)))
                try:
                    await page.goto(url)
                    secure = await page.evaluate('({secure:isSecureContext, uuid:typeof crypto.randomUUID})')
                    assert secure == {'secure':False, 'uuid':'undefined'}, secure
                    await page.evaluate('theme=>localStorage.setItem("theme",theme)', theme)
                    await page.reload()
                    await page.get_by_label('用户名',exact=True).fill('admin')
                    await page.get_by_label('密码',exact=True).fill('wrong')
                    await page.get_by_label('密码',exact=True).press('Enter')
                    await expect(page.get_by_role('alert')).to_contain_text('用户名或密码不正确')
                    await page.get_by_label('密码',exact=True).fill(env['NAST_PASSWORD'])
                    await page.get_by_label('密码',exact=True).press('Enter')
                    await expect(page.get_by_role('heading',name='登录 NAST')).not_to_be_visible()
                    await settings(page,width)
                    await page.get_by_role('button',name='添加模型',exact=True).click()
                    dialog = page.get_by_role('dialog',name='添加模型',exact=True)
                    await expect(dialog).to_be_visible()
                    await expect(dialog.get_by_label('API 类型',exact=True)).to_be_focused()
                    await page.screenshot(path=str(artifact/f'{name}-basic.png'),full_page=True)
                    if protocol != 'openai':
                        await choose(page,'API 类型', 'Anthropic' if protocol == 'anthropic' else 'Gemini')
                    await page.get_by_label('API 地址',exact=True).fill(endpoint+'/'+name+'/v1')
                    if protocol != 'gemini':
                        await page.get_by_label('API 密钥',exact=True).fill('key-'+name)
                    if protocol == 'openai':
                        before = await rpc(page,'model_catalog.get')
                        await page.get_by_role('button',name='获取模型列表',exact=True).click()
                        await page.get_by_placeholder('搜索模型…').fill('gpt')
                        await page.get_by_placeholder('搜索模型…').press('ArrowDown')
                        await page.get_by_placeholder('搜索模型…').press('Enter')
                        await expect(page.get_by_label('模型 ID',exact=True)).to_have_value('gpt-4o')
                        assert await rpc(page,'model_catalog.get') == before, 'Discovery must not save the draft'
                    else:
                        await page.get_by_label('模型 ID',exact=True).fill('native-test')
                    await page.get_by_label('显示名称（选填）',exact=True).fill(name)
                    await page.get_by_role('button',name='模型参数',exact=True).click()
                    await page.get_by_label('最大输入 Token',exact=True).fill('8192')
                    await page.get_by_label('最大输出 Token',exact=True).fill('4096')
                    await page.get_by_label('上下文窗口（选填）',exact=True).fill('16384')
                    if protocol == 'openai':
                        await choose(page,'输出参数','max_completion_tokens · OpenAI 推理模型')
                        await choose(page,'思考强度','高（high）')
                    elif protocol == 'anthropic':
                        await choose(page,'思考方式','指定思考预算' if mode == 'enabled' else '自适应思考')
                        if mode == 'enabled':
                            await page.get_by_label('思考预算 Token',exact=True).fill('4096')
                            await page.get_by_role('button',name='保存模型',exact=True).click()
                            await expect(dialog.get_by_role('alert')).to_contain_text('必须小于最大输出')
                            await page.get_by_label('思考预算 Token',exact=True).fill('2048')
                        else:
                            await choose(page,'思考强度','high')
                    else:
                        await choose(page,'思考方式','指定思考级别')
                        await choose(page,'思考级别','high')
                    await page.screenshot(path=str(artifact/f'{name}-parameters.png'),full_page=True)
                    assert not await page.evaluate('document.documentElement.scrollWidth > innerWidth+1')
                    assert not await dialog.evaluate('(el)=>el.scrollWidth > el.clientWidth+1')
                    await page.get_by_role('button',name='保存模型',exact=True).click()
                    await expect(dialog).not_to_be_visible()
                    catalog = await rpc(page,'model_catalog.get')
                    model = next(m for m in catalog['models'] if m['display_name'] == name)
                    config = model['routes'][0]['config']
                    assert config['input_limit']==8192 and config['output_limit']==4096
                    assert catalog['default_model']==model['id']
                    assert 'key-'+name not in json.dumps(catalog)
                    # Editing and stale-save recovery must retain typed input and route credentials.
                    await page.get_by_role('button',name='编辑 '+name,exact=True).click()
                    edit = page.get_by_role('dialog',name='编辑模型',exact=True)
                    await page.get_by_label('显示名称（选填）',exact=True).fill(name+' edited')
                    await rpc(page,'model_catalog.save',{'catalog':catalog})
                    await page.get_by_role('button',name='保存模型',exact=True).click()
                    await expect(edit.get_by_role('alert')).to_contain_text('输入已保留')
                    await expect(page.get_by_label('显示名称（选填）',exact=True)).to_have_value(name+' edited')
                    await page.get_by_role('button',name='加载最新配置并保留输入',exact=True).click()
                    await page.get_by_role('button',name='保存模型',exact=True).click()
                    await expect(edit).not_to_be_visible()
                    await page.keyboard.press('Escape')
                    if width < 768:
                        await page.keyboard.press('Escape')
                    # Import through the file chooser, then start the first chat through the UI.
                    async with page.expect_file_chooser() as chosen:
                        await page.get_by_role('button',name='导入角色卡',exact=True).last.click()
                    await (await chosen.value).set_files({'name':name+'.json','mimeType':'application/json','buffer':json.dumps(card(name),ensure_ascii=False).encode()})
                    if width < 768 and await page.get_by_role('button',name='切换侧栏',exact=True).is_visible():
                        await page.get_by_role('button',name='切换侧栏',exact=True).click()
                    await page.locator('[data-sidebar="menu-button"]').filter(has_text=name).first.click()
                    selector = page.get_by_role('combobox',name='会话模型')
                    await expect(selector).to_contain_text(name+' edited')
                    start = len(server.captures)
                    composer = page.get_by_role('textbox',name='输入消息')
                    await composer.fill('hello model setup')
                    await composer.press('Enter')
                    await expect(page.get_by_text('已收到上下文：',exact=False).last).to_be_visible()
                    await expect(selector).to_be_enabled()
                    assert len(server.captures)==start+1
                    captured = server.captures[-1]
                    body = captured['body']
                    assert captured['route']==name
                    if protocol == 'gemini':
                        assert body['generationConfig']['maxOutputTokens']==4096
                        assert body['generationConfig']['thinkingConfig']=={'thinkingLevel':'HIGH'}
                        assert captured['credential_hash'] == hashlib.sha256(b'').hexdigest()
                    else:
                        assert captured['credential_hash'] == hashlib.sha256((('Bearer ' if protocol=='openai' else '')+'key-'+name).encode()).hexdigest()
                        assert 'temperature' not in body and 'top_p' not in body
                        if protocol=='openai':
                            assert body['max_completion_tokens']==4096 and 'max_tokens' not in body
                            assert body['reasoning_effort']=='high'
                        else:
                            assert body['max_tokens']==4096
                            assert body['thinking']==({'type':'enabled','budget_tokens':2048} if mode=='enabled' else {'type':'adaptive'})
                            if mode=='adaptive': assert body['output_config']['effort']=='high'
                    await page.screenshot(path=str(artifact/f'{name}-chat.png'),full_page=True)
                    await page.reload()
                    await expect(page.get_by_role('heading',name='登录 NAST')).not_to_be_visible()
                    persisted=await rpc(page,'model_catalog.get')
                    assert next(m for m in persisted['models'] if m['id']==model['id'])['display_name']==name+' edited'
                    results.append(name)
                    print('PASS',name,flush=True)
                except Exception:
                    await page.screenshot(path=str(artifact/'failure.png'),full_page=True)
                    (artifact/'failure.txt').write_text(await page.locator('body').inner_text(),encoding='utf-8')
                    raise
                await context.close()
            assert not errors, errors
            await browser.close()
    except Exception as error:
        print("FAIL", repr(error), flush=True)
        if page and not page.is_closed():
            try:
                await page.screenshot(path=str(artifact/'failure.png'),full_page=True)
                (artifact/'failure.txt').write_text(await page.locator('body').inner_text(),encoding='utf-8')
            except Exception:
                pass
        raise
    finally:
        process.terminate();process.wait(timeout=10);log.close()
        server.shutdown();server.server_close()
        write_json(artifact/'report.json',{'passed':results,'browser_errors':errors,'url':url})
    print('PASS real insecure-HTTP onboarding, three protocols, four theme/viewports',flush=True)


if __name__=='__main__': asyncio.run(main())
