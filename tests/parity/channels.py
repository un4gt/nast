"""Channel catalog, sticky failover and actual settings UI, with isolated local upstreams."""
import asyncio
import base64
import copy
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import uuid
from playwright.async_api import async_playwright, expect
from model_server import ModelServer
from run import ROOT, card, port, rpc, wait_http, write_json


async def main():
    artifact = ROOT/'.cache'/('channels-'+uuid.uuid4().hex[:8]); artifact.mkdir(parents=True)
    print(f'Evidence: {artifact}', flush=True)
    upstream = ModelServer(output=artifact/'requests.jsonl')
    threading.Thread(target=upstream.serve_forever, daemon=True).start()
    endpoint = f'http://127.0.0.1:{upstream.server_port}'
    user = artifact/'data/default-user'; user.mkdir(parents=True)
    write_json(user/'settings.json', {'oai_settings': {'custom_url':endpoint+'/a/v1', 'custom_model':'gpt-4o', 'stream_openai':True, 'openai_max_context':8192, 'openai_max_tokens':128}})
    write_json(user/'secrets.json', {'api_key_custom':[{'active':True,'value':'original-secret'}]})
    http_port = port(); origin = f'http://127.0.0.1:{http_port}'
    env = {**os.environ, 'NAST_PORT':str(http_port), 'NAST_DATA':str(artifact/'data'), 'NAST_WEB':str(ROOT/'web/dist'), 'NAST_BIND':'127.0.0.1', 'NAST_ALLOW_ANONYMOUS':'true', 'NAST_USERNAME':'', 'NAST_PASSWORD':'', 'NAST_PUBLIC_ORIGIN':'', 'NAST_BRIDGE_TOKEN':''}
    binary = ROOT/'target/debug'/('nast.exe' if os.name == 'nt' else 'nast')
    log = (artifact/'server.log').open('w', encoding='utf-8')
    process = subprocess.Popen([str(binary)], cwd=artifact, env=env, stdout=log, stderr=log)
    results = []; errors = []
    def passed(name): results.append(name); print('PASS '+name, flush=True)
    try:
        await wait_http(origin, process)
        async with async_playwright() as pw:
            browser = await pw.chromium.launch()
            page = await browser.new_page(viewport={'width':1440,'height':1000})
            page.on('pageerror', lambda e: errors.append(str(e)))
            await page.goto(origin)
            async def call(method, **params): return await rpc(page, method, params)
            async def fails(method, **params):
                try: await call(method, **params)
                except Exception as e: assert 'RPC timeout' not in str(e); return str(e)
                raise AssertionError('expected failure: '+method)
            initial = await call('model_catalog.get')
            assert initial['schema_version'] == 2 and len(initial['channels']) == 1
            disk = json.loads((user/'models.json').read_text(encoding='utf-8'))
            assert 'endpoint' not in disk['models'][0]['routes'][0]['config']
            assert 'original-secret' not in json.dumps(initial)
            passed('migration/shared-channel-no-duplicated-connection-or-key')
            def channel(id, name=None): return {'id':id,'name':name or id,'protocol':'openai','endpoint':endpoint+'/'+id+'/v1','enabled':True,'headers':{},'discovered_models':[]}
            def route(id, priority=0, images=None, context=8192): return {'id':'route-'+id,'channel_id':id,'provider':id,'protocol':'openai','upstream_model':'gpt-4o','priority':priority,'enabled':True,'config':{'endpoint':endpoint+'/'+id+'/v1','context_limit':context,'supports_images':images,'output_limit':128,'connect_timeout_secs':2,'first_token_timeout_secs':2,'idle_timeout_secs':2,'headers':{},'parameters':{},'remove_parameters':[]}}
            async def configure(routes=None):
                c = await call('model_catalog.get')
                c['channels'] = [channel('a','主渠道'),channel('b','备用渠道'),channel('c','长上下文')]
                c['models'] = [{'id':'model-a','display_name':'模型 A','routes':routes or [route('a',10,False),route('b',5,True),route('c',0,True)]}, {'id':'model-b','display_name':'模型 B','routes':[{**route('a'), 'id':'route-secondary'}]}]
                c['default_model']='model-a'
                await call('model_catalog.save', catalog=c, channel_credentials={'a':'secret-a','b':'secret-b','c':'secret-c'})
            await configure()
            avatar = (await call('characters.import', filename='actor.json', data_base64=base64.b64encode(json.dumps(card('渠道验收')).encode()).decode()))['avatar']
            async def new(): return (await call('chats.new',avatar=avatar,greeting_index=0))['file_name']
            def ref(chat): return {'kind':'private','avatar':avatar,'chat_file':chat}
            async def selection(chat): return (await call('conversation_model.get',conversation=ref(chat)))['state']
            async def generate(chat, **params): return await call('generate.run',avatar=avatar,chat_file=chat,user_message='继续这段对话',**params)
            chat = await new(); await generate(chat)
            assert (await selection(chat))['active_route']=='route-a'
            upstream.plans['a']=[{'status':503}]*2; before=len(upstream.captures)
            await generate(chat)
            assert [r['route'] for r in upstream.captures[before:]]==['a','a','b']
            await generate(chat); assert upstream.captures[-1]['route']=='b'
            other=await new(); await generate(other); assert upstream.captures[-1]['route']=='a'
            await call('conversation_model.set',conversation=ref(chat),model_id='model-b')
            await generate(chat)
            await call('conversation_model.set',conversation=ref(chat),model_id='model-a')
            await generate(chat); assert upstream.captures[-1]['route']=='b'
            passed('routing/failover-sticks-no-bounce-conversation-and-model-isolation')
            await page.close(); process.terminate(); await asyncio.to_thread(process.wait,timeout=35)
            process=subprocess.Popen([str(binary)],cwd=artifact,env=env,stdout=log,stderr=log)
            await wait_http(origin,process); page=await browser.new_page(viewport={'width':1440,'height':1000}); page.on('pageerror',lambda e:errors.append(str(e))); await page.goto(origin)
            await generate(chat); assert upstream.captures[-1]['route']=='b'
            passed('routing/binding-survives-restart')
            image_chat=await new(); await generate(image_chat,required_capabilities={'images':True}); assert upstream.captures[-1]['route']=='b'
            await configure([route('a',10,False,160),route('b',5,True,160),route('c',0,True,8192)])
            before=len(upstream.captures); await generate(await new()); assert [r['route'] for r in upstream.captures[before:]]==['c']
            passed('routing/capability-and-context-filter-skips-ineligible-candidates')
            await configure(); upstream.plans['a']=[{'status':401}]; before=len(upstream.captures)
            await generate(await new()); assert [r['route'] for r in upstream.captures[before:]]==['a','b']
            upstream.plans['a']=[{'cutoff':'body'}]; before=len(upstream.captures)
            result=await generate(await new())
            assert result['routing']['status']=='complete' and result['routing']['route']=='route-b'
            assert [r['route'] for r in upstream.captures[before:]]==['a','b']
            assert result['routing']['failed_attempts'][0]['route']=='route-a'
            assert result['text']==upstream.captures[-1]['reply']
            passed('routing/auth-failover-and-stream-replacement-not-concatenation')
            c=await call('model_catalog.get'); c['channels'][0]['endpoint']=endpoint+'/changed/v1'
            assert '重新输入' in await fails('model_catalog.save',catalog=c)
            c=await call('model_catalog.get'); c['channels'][0]['credential_ref']=c['channels'][1]['credential_ref']
            await call('model_catalog.save',catalog=c)
            restored=await call('model_catalog.get'); assert restored['channels'][0]['credential_ref']!=restored['channels'][1]['credential_ref']
            await call('model_catalog.save',catalog=restored,channel_credentials={'a':'updated-shared-key'})
            before=len(upstream.captures); await generate(await new())
            second=await new(); await call('conversation_model.set',conversation=ref(second),model_id='model-b'); await generate(second)
            assert upstream.captures[before]['credential_hash']==upstream.captures[-1]['credential_hash']
            passed('credentials/shared-update-reference-forgery-and-endpoint-change-guard')
            upstream.models=['gpt-4o','new-model-one','new-model-two']
            c=await call('model_catalog.get')
            c['models'][0]['display_name']='Claude Sonnet'; c['models'][1]['display_name']='Gemini Flash'
            for i,name in enumerate(['DeepSeek V3','Claude Haiku','Gemini Pro','Qwen','Grok','Llama']):
                c['models'].append({'id':'ui-'+str(i),'display_name':name,'routes':[{**route('b'), 'id':'ui-route-'+str(i)}]})
            await call('model_catalog.save',catalog=c)
            await page.reload()
            await page.get_by_role('button',name='设置',exact=True).wait_for()
            await page.evaluate("window.dispatchEvent(new Event('nast:open-model-settings'))")
            await expect(page.get_by_role('heading',name='为对话，选一个好搭档。')).to_be_visible()
            assert await page.get_by_role('button',name='同步模型',exact=True).count()==0
            await page.screenshot(path=str(artifact/'models-desktop.png'),animations='disabled')
            await page.get_by_role('tab',name='渠道').click()
            await page.screenshot(path=str(artifact/'channels-desktop.png'),animations='disabled')
            await page.get_by_role('button',name='获取模型列表',exact=True).first.click()
            dialog=page.get_by_role('dialog').last
            await expect(dialog.get_by_text('new-model-one',exact=True)).to_be_visible()
            await dialog.get_by_label('new-model-one',exact=True).check()
            await dialog.get_by_role('combobox',name='new-model-one 关联到').click()
            await page.get_by_role('option',name='关联到 Claude Sonnet',exact=True).click()
            await dialog.get_by_label('new-model-two',exact=True).check()
            await dialog.get_by_role('button',name='关联 2 个模型',exact=True).click()
            await expect(page.get_by_role('dialog')).to_have_count(1)
            synced=await call('model_catalog.get')
            assert len(synced['models'])==len(c['models'])+1
            assert any(r['upstream_model']=='new-model-one' for r in synced['models'][0]['routes'])
            await page.get_by_role('button',name='获取模型列表',exact=True).first.click()
            await expect(page.get_by_role('dialog').last.get_by_text('已关联',exact=True)).to_have_count(3)
            await page.get_by_role('dialog').last.get_by_role('button',name='取消',exact=True).click()
            passed('ui/channel-discovery-link-existing-create-new-no-duplicates')
            await page.get_by_role('button',name='添加渠道',exact=True).click()
            dialog=page.get_by_role('dialog').last
            await dialog.get_by_label('渠道名称',exact=True).fill('新渠道')
            await dialog.get_by_label('API 地址',exact=True).fill(endpoint+'/new/v1')
            await dialog.get_by_label('API 密钥',exact=True).fill('new-channel-secret')
            await dialog.get_by_role('button',name='测试连接',exact=True).click()
            await expect(dialog.get_by_text('连接成功',exact=False)).to_be_visible()
            await dialog.get_by_role('button',name='保存渠道',exact=True).click()
            await expect(page.get_by_role('dialog')).to_have_count(1)
            await page.get_by_role('tab',name='模型').click()
            await page.get_by_role('button',name='配置 Claude Sonnet',exact=True).click()
            dialog=page.get_by_role('dialog').last
            await dialog.get_by_role('combobox',name='关联一个渠道').click()
            await page.get_by_role('option',name='新渠道',exact=True).click()
            await dialog.get_by_label('这个渠道的模型 ID',exact=True).fill('alternate-sonnet')
            await dialog.get_by_role('button',name='保存模型',exact=True).click()
            await expect(page.get_by_role('dialog')).to_have_count(1)
            assert (await call('model_catalog.get'))['models'][0]['routes'][-1]['upstream_model']=='alternate-sonnet'
            passed('ui/create-test-channel-and-configure-model-association')
            for theme in ['light','dark']:
                for width in [390,1440]:
                    await page.set_viewport_size({'width':width,'height':1000})
                    await page.evaluate('theme=>localStorage.setItem("theme",theme)',theme)
                    await page.reload()
                    await page.get_by_role('button',name='切换侧栏',exact=True).wait_for()
                    await page.evaluate("window.dispatchEvent(new Event('nast:open-model-settings'))")
                    await expect(page.get_by_role('heading',name='为对话，选一个好搭档。')).to_be_visible()
                    await page.screenshot(path=str(artifact/f'models-{theme}-{width}.png'),animations='disabled')
                    assert not await page.evaluate('document.documentElement.scrollWidth > innerWidth+1')
                    await page.get_by_role('button',name='配置 Claude Sonnet',exact=True).click()
                    await page.screenshot(path=str(artifact/f'editor-{theme}-{width}.png'),animations='disabled')
                    assert not await page.evaluate('document.documentElement.scrollWidth > innerWidth+1')
                    await page.get_by_role('dialog').last.get_by_role('button',name='取消',exact=True).click()
            passed('ui/light-dark-desktop-mobile-no-overflow')
            assert not errors, errors
            await browser.close()
        write_json(artifact/'report.json',{'passed':results,'page_errors':errors})
        print('ALL PASS',flush=True)
    finally:
        process.terminate()
        try: await asyncio.to_thread(process.wait,timeout=35)
        except subprocess.TimeoutExpired: process.kill()
        upstream.shutdown();log.close()


if __name__=='__main__': asyncio.run(main())
