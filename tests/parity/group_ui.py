"""Group UI at six theme/viewport combinations; compare Lua results with a browser-free client."""
import asyncio
import base64
import json
import os
from pathlib import Path
import subprocess
import shutil
import tempfile
import threading
from playwright.async_api import async_playwright, expect
from group_headless import Client, PLUGIN
from model_server import ModelServer, canonical
from run import ROOT, card, port, wait_http, write_json
from cases import message_state


async def main():
    artifact=Path(tempfile.mkdtemp(prefix='nast-group-ui-'))
    print('Evidence:',artifact,flush=True)
    server=ModelServer(output=artifact/'requests.jsonl')
    threading.Thread(target=server.serve_forever,daemon=True).start()
    user=artifact/'data/default-user'; user.mkdir(parents=True)
    settings={'oai_settings':{'custom_url':f'http://127.0.0.1:{server.server_port}/test/v1','custom_model':'gpt-4o','chat_completion_source':'custom','stream_openai':True,'openai_max_tokens':128,'openai_max_context':8192},'power_user':{'username':'User','personas':{'tester':'验收 Persona'}}}
    write_json(user/'settings.json',settings)
    plugins=artifact/'plugins';plugins.mkdir();(plugins/'10_group.lua').write_text(PLUGIN)
    http_port=port();url=f'http://127.0.0.1:{http_port}'
    env={**os.environ,'NAST_DATA':str(artifact/'data'),'NAST_PORT':str(http_port),'NAST_BIND':'127.0.0.1','NAST_ALLOW_ANONYMOUS':'true','NAST_USERNAME':'','NAST_PASSWORD':'','NAST_BRIDGE_TOKEN':'','NAST_PUBLIC_ORIGIN':'','NAST_WEB':str(ROOT/'web/dist'),'OPENAI_API_KEY':'','NAST_OPENAI_BASE':''}
    binary=Path(os.environ.get('NAST_TEST_BINARY',ROOT/'target/debug'/('nast.exe' if os.name=='nt' else 'nast')))
    isolated_binary=artifact/binary.name
    shutil.copy2(binary,isolated_binary)
    binary=isolated_binary
    log=(artifact/'server.log').open('w',encoding='utf-8')
    process=subprocess.Popen([str(binary)],cwd=artifact,env=env,stdout=log,stderr=log)
    results=[];errors=[];client=None
    try:
        await wait_http(url+'/healthz',process)
        client=await Client().connect(url.replace('http:','ws:')+'/ws')
        avatars=[]
        for name in ['成员甲','成员乙']:
            avatars.append((await client.call('characters.create',card=card(name)))['avatar'])
        await client.call('worlds.save',name='群验收书',book={'entries':{'0':{'uid':0,'key':['城堡'],'content':'群会话知识','order':100}}})
        async with async_playwright() as pw:
            browser=await pw.chromium.launch()
            expect.set_options(timeout=30000)
            for theme in ['light','dark']:
                for width in [390,768,1440]:
                    label=f'{theme}-{width}'
                    context=await browser.new_context(viewport={'width':width,'height':900})
                    await context.add_init_script(f'localStorage.setItem("theme", {json.dumps(theme)})')
                    page=await context.new_page();page.on('pageerror',lambda e:errors.append(str(e)))
                    async def dismiss_toasts():
                        if width<1280: return
                        close=page.get_by_role('button',name='Close toast',exact=True)
                        while await close.count():
                            count=await close.count()
                            await close.first.click()
                            await expect(close).to_have_count(count-1)
                    group=await client.call('groups.create',group={'name':'群验收 '+label,'members':avatars,'activation_strategy':1,'auto_mode_delay':1})
                    await page.goto(url)
                    if width<768: await page.get_by_role('button',name='切换侧栏',exact=True).click()
                    await page.locator('[data-sidebar="menu-button"]').filter(has_text=group['name']).click()
                    await expect(page.locator('h1')).to_have_text(group['name'])
                    composer=page.get_by_role('textbox',name='输入消息')
                    await composer.fill('原会话草稿')
                    await page.locator('summary',has_text='群会话与世界书').click()
                    await page.get_by_role('button',name='新会话',exact=True).click()
                    await expect(composer).to_have_value('')
                    await composer.fill('第二会话草稿')
                    await page.locator('summary',has_text='群会话与世界书').click()
                    await page.get_by_role('combobox',name='当前会话',exact=True).click()
                    await page.get_by_role('option',name=group['chat_id'],exact=True).click()
                    await expect(composer).to_have_value('原会话草稿')
                    await page.get_by_role('button',name='群设置',exact=True).click()
                    dialog=page.get_by_role('dialog',name='群设置',exact=True)
                    await dialog.get_by_role('button',name='上移 成员乙',exact=True).click()
                    await dialog.get_by_role('textbox',name='拼接前缀').fill('[<FIELDNAME>] ')
                    await page.screenshot(path=str(artifact/(label+'-settings.png')),full_page=True)
                    await dialog.get_by_role('button',name='保存',exact=True).click()
                    await expect(dialog).not_to_be_visible()
                    await dismiss_toasts()
                    if width<1280: await page.get_by_role('button',name='打开对话详情',exact=True).click()
                    await page.get_by_role('tab',name='群聊',exact=True).click()
                    await page.screenshot(path=str(artifact/(label+'-inspector.png')),full_page=True)
                    (artifact/(label+'-inspector.yml')).write_text(await page.locator('body').aria_snapshot(),encoding='utf-8')
                    await page.get_by_role('textbox',name='群标签（逗号分隔）',exact=True).fill('验收, 城堡')
                    await page.get_by_role('textbox',name='本会话场景覆盖',exact=True).fill('城堡里的群聊')
                    await page.get_by_role('button',name='保存群详情',exact=True).click()
                    await expect(page.get_by_role('button',name='保存群详情',exact=True)).to_be_enabled()
                    await dismiss_toasts()
                    await page.get_by_role('combobox',name='本会话 Persona',exact=True).click()
                    await page.get_by_role('option',name='验收 Persona',exact=True).click()
                    await expect(page.get_by_role('listbox')).not_to_be_visible()
                    await dismiss_toasts()
                    await page.screenshot(path=str(artifact/(label+'-details.png')),full_page=True)
                    await page.get_by_role('tab',name='作者注记',exact=True).click()
                    await page.get_by_placeholder('注入到提示的指导文本…').fill('关注各成员的身份')
                    await page.get_by_role('button',name='保存到本聊天',exact=True).click()
                    await expect(page.get_by_role('button',name='保存到本聊天',exact=True)).to_be_enabled()
                    await dismiss_toasts()
                    await page.get_by_role('tab',name='世界书',exact=True).click()
                    await page.get_by_role('combobox',name='本聊天绑定',exact=True).click()
                    await page.get_by_role('option',name='群验收书',exact=True).click()
                    if width<1280:
                        detail=page.get_by_role('dialog',name='对话详情',exact=True)
                        await detail.get_by_role('button',name='关闭',exact=True).click()
                        await expect(detail).not_to_be_visible()
                    await composer.fill('/queue reverse')
                    await page.get_by_role('button',name='发送消息',exact=True).click()
                    await expect(composer).to_be_enabled()
                    g={'id':group['id'],'chat_id':group['chat_id']}
                    snapshot=await client.call('groups.export_chat',**g)
                    before=await client.call('groups.get_chat',**g)
                    assert before[0]['chat_metadata']['persona']=='tester'
                    assert before[0]['chat_metadata']['note_prompt']=='关注各成员的身份'
                    assert before[0]['chat_metadata']['world_info']=='群验收书'
                    start=len(server.captures)
                    await composer.fill('/say browser')
                    await page.get_by_role('button',name='发送消息',exact=True).click()
                    await expect(composer).to_be_enabled()
                    visible=await client.call('groups.get_chat',**g)
                    assert visible[-3]['mes']=='command:browser:input'
                    web_requests=[r['body'] for r in server.captures[start:]]
                    assert len(web_requests)==2
                    clone=await client.call('groups.create',group={'name':group['name'],'members':avatars[::-1],'activation_strategy':1})
                    clone=await client.call('groups.import_chat',id=clone['id'],text=base64.b64decode(snapshot['data_base64']).decode())
                    start=len(server.captures)
                    await client.call('commands.execute',conversation={'kind':'group','group_id':clone['id'],'chat_id':clone['chat_id']},input='/say browser')
                    headless=await client.call('groups.get_chat',id=clone['id'],chat_id=clone['chat_id'])
                    assert canonical(web_requests)==canonical([r['body'] for r in server.captures[start:]])
                    assert message_state(visible[1:])==message_state(headless[1:])
                    await client.call('groups.delete',id=clone['id'])
                    await client.call('groups.auto_start',**g,max_turns=1)
                    await expect(composer).to_be_disabled()
                    await expect(composer).to_be_enabled()
                    automatic=await client.call('groups.get_chat',**g)
                    assert len(automatic)==len(visible)+2
                    await expect(page.get_by_text(automatic[-1]['mes'],exact=False).first).to_be_visible()
                    await page.reload()
                    if width<768: await page.get_by_role('button',name='切换侧栏',exact=True).click()
                    await page.locator('[data-sidebar="menu-button"]').filter(has_text=group['name']).click()
                    await expect(page.get_by_text(automatic[-1]['mes'],exact=False).first).to_be_visible()
                    assert not await page.evaluate('document.documentElement.scrollWidth>innerWidth+1')
                    if width<768: await expect(page.get_by_role('button',name='关闭侧栏',exact=True)).not_to_be_visible()
                    await page.screenshot(path=str(artifact/(label+'-conversation.png')),full_page=True)
                    if label=='light-1440':
                        await page.locator('[data-sidebar="menu-button"]').filter(has_text='成员甲').click()
                        await expect(page.locator('h1')).to_have_text('成员甲')
                        await composer.fill('/sys 单聊后端命令')
                        await page.get_by_role('button',name='发送消息',exact=True).click()
                        await expect(composer).to_be_enabled()
                        await expect(page.get_by_text('单聊后端命令',exact=True)).to_be_visible()
                        await composer.fill('/newchat')
                        await page.get_by_role('button',name='发送消息',exact=True).click()
                        await expect(composer).to_have_value('')
                        await composer.fill('/name PrivateCommandAcceptance')
                        await page.get_by_role('button',name='发送消息',exact=True).click()
                        await expect(composer).to_have_value('')
                        assert any('PrivateCommandAcceptance' in file for file in await client.call('characters.chats',avatar=avatars[0]))
                    results.append({'name':label+'/draft-members-bindings-lua-client-equivalence','passed':True})
                    print('PASS',label,flush=True)
                    await context.close()
            await browser.close()
        assert not errors,errors
    except Exception as error:
        results.append({'name':'failure','passed':False,'error':repr(error)})
        if 'page' in locals():
            try: await page.screenshot(path=str(artifact/'failure.png'),full_page=True)
            except Exception: pass
        raise
    finally:
        if client: await client.close()
        process.terminate();process.wait(timeout=10);server.shutdown();log.close()
        write_json(artifact/'report.json',{'results':results,'browser_errors':errors})
        write_json(ROOT/'.cache/groupchat/ui-latest.json',{'artifact':str(artifact),'results':results})

if __name__=='__main__':asyncio.run(main())
