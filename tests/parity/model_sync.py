"""Batch model discovery/addition through real UI and credential-isolation RPC checks."""
import asyncio
import base64
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
from playwright.async_api import async_playwright, expect
from model_server import ModelServer
from run import ROOT, card, port, rpc, wait_http, write_json

async def main():
    artifact=Path(tempfile.mkdtemp(prefix='nast-model-sync-'))
    print('Evidence:',artifact,flush=True)
    server=ModelServer(output=artifact/'requests.jsonl')
    server.models=['gpt-4o','google/gemini-flash','anthropic/claude-sonnet','google/'+('long-model-name-'*8)]
    threading.Thread(target=server.serve_forever,daemon=True).start()
    endpoint=f'http://127.0.0.1:{server.server_port}'
    url=f'http://127.0.0.1:{port()}'
    env={**os.environ,'NAST_BIND':'127.0.0.1','NAST_PORT':url.rsplit(':',1)[1],
         'NAST_DATA':str(artifact/'data'),'NAST_WEB':str(ROOT/'web/dist'),'NAST_USERNAME':'','NAST_PASSWORD':'',
         'NAST_BRIDGE_TOKEN':'','NAST_PUBLIC_ORIGIN':'','NAST_ALLOW_ANONYMOUS':'true','NAST_COOKIE_SECURE':'false',
         'OPENAI_API_KEY':'','NAST_OPENAI_BASE':''}
    user=artifact/'data/default-user'
    write_json(user/'secrets.json',{'api_key_custom':[{'active':True,'value':'legacy-secret'}]})
    log=(artifact/'server.log').open('w',encoding='utf-8')
    binary=ROOT/'target/debug'/('nast.exe' if os.name=='nt' else 'nast')
    process=subprocess.Popen([str(binary)],cwd=artifact,env=env,stdout=log,stderr=log)
    results=[];errors=[];page=None
    try:
        await wait_http(url+'/healthz',process)
        async with async_playwright() as pw:
            browser=await pw.chromium.launch()
            expect.set_options(timeout=20000)
            for theme,width in [('light',1440),('dark',390),('light',390),('dark',1440)]:
                context=await browser.new_context(viewport={'width':width,'height':900})
                page=await context.new_page()
                page.on('pageerror',lambda error:errors.append(str(error)))
                await page.goto(url)
                await page.evaluate('theme=>localStorage.setItem("theme",theme)',theme)
                name=f'{theme}-{width}'
                catalog=await rpc(page,'model_catalog.get')
                source={'id':'model-'+name,'display_name':name,'routes':[{'id':'source-'+name,'provider':'','protocol':'openai',
                    'upstream_model':'gpt-4o','enabled':True,'priority':0,'config':{'endpoint':endpoint+'/'+name+'/v1',
                    'input_limit':8192,'output_limit':512,'context_limit':16384,'parameters':{'max_tokens':512},'headers':{},'remove_parameters':[]}}]}
                catalog['models'].append(source)
                await rpc(page,'model_catalog.save',{'catalog':catalog,'credentials':{'source-'+name:'batch-key'}})
                await page.reload()
                if width<768: await page.get_by_role('button',name='切换侧栏',exact=True).click()
                await page.get_by_role('button',name='设置',exact=True).click()
                before=await rpc(page,'model_catalog.get')
                original=next(m for m in before['models'] if m['id']==source['id'])
                if theme=='light' and width==1440: server.list_failures=1
                await page.get_by_role('button',name='同步 '+name+' 的模型',exact=True).click()
                dialog=page.get_by_role('dialog',name='同步模型',exact=True)
                if theme=='light' and width==1440:
                    await expect(dialog.get_by_role('alert')).to_contain_text('获取失败')
                    await dialog.get_by_role('button',name='重试',exact=True).click()
                await expect(dialog.get_by_role('checkbox',name='gpt-4o',exact=True)).to_be_disabled()
                assert await rpc(page,'model_catalog.get')==before,'discovery may not save'
                assert server.model_lists[-1]['credential_hash']==hashlib.sha256(b'Bearer batch-key').hexdigest()
                search=dialog.get_by_role('textbox',name='搜索模型',exact=True)
                await search.fill('google')
                await dialog.get_by_role('checkbox',name='全选搜索结果',exact=True).check()
                await search.fill('anthropic')
                choice=dialog.get_by_role('checkbox',name='anthropic/claude-sonnet',exact=True)
                await choice.focus();await choice.press('Space')
                await expect(dialog.get_by_role('status').filter(has_text='已选')).to_contain_text('已选 3 个')
                await search.fill('google')
                await expect(dialog.get_by_role('checkbox',name='全选搜索结果',exact=True)).to_be_checked()
                await dialog.get_by_role('checkbox',name=server.models[-1],exact=True).uncheck()
                await search.fill('')
                await expect(dialog.get_by_role('status').filter(has_text='已选')).to_contain_text('已选 2 个')
                await page.screenshot(path=str(artifact/(name+'-selected.png')),full_page=True)
                assert not await dialog.evaluate('(el)=>el.scrollWidth>el.clientWidth+1')
                assert not await page.evaluate('document.documentElement.scrollWidth>innerWidth+1')
                if theme=='light' and width==1440:
                    await rpc(page,'model_catalog.save',{'catalog':before})
                    await dialog.get_by_role('button',name='添加 2 个模型',exact=True).click()
                    await expect(dialog.get_by_role('alert')).to_contain_text('选择已保留')
                    assert len((await rpc(page,'model_catalog.get'))['models'])==len(before['models'])
                    await dialog.get_by_role('button',name='加载最新配置并保留选择',exact=True).click()
                    await expect(dialog.get_by_role('checkbox',name='google/gemini-flash',exact=True)).to_be_checked()
                version=(await rpc(page,'model_catalog.get'))['version']
                await dialog.get_by_role('button',name='添加 2 个模型',exact=True).click()
                await expect(dialog).not_to_be_visible()
                after=await rpc(page,'model_catalog.get')
                assert after['version']==version+1
                assert after['default_model']==before['default_model']
                assert next(m for m in after['models'] if m['id']==source['id'])==original
                added=[m for m in after['models'] if m['id'] not in {x['id'] for x in before['models']}]
                assert len(added)==2 and {m['display_name'] for m in added}=={'google/gemini-flash','anthropic/claude-sonnet'}
                refs=[original['routes'][0]['config']['credential_ref']]+[m['routes'][0]['config']['credential_ref'] for m in added]
                assert len(set(refs))==3 and all(m['routes'][0]['credential_configured'] for m in added)
                assert 'batch-key' not in json.dumps(after)
                avatar=(await rpc(page,'characters.import',{'filename':'actor.json','data_base64':base64.b64encode(json.dumps(card('BatchActor')).encode()).decode()}))['avatar']
                # Rotating the original key must not silently rotate independent model copies.
                await rpc(page,'model_catalog.save',{'catalog':after,'credentials':{'source-'+name:'rotated-key'}})
                for model in added:
                    chat=(await rpc(page,'chats.new',{'avatar':avatar,'greeting_index':0}))['file_name']
                    await rpc(page,'conversation_model.set',{'conversation':{'kind':'private','avatar':avatar,'chat_file':chat},'model_id':model['id']})
                    result=await rpc(page,'generate.run',{'avatar':avatar,'chat_file':chat,'user_message':'hello batch'})
                    assert result['routing']['status']=='complete'
                    capture=server.captures[-1]
                    assert capture['body']['model']==model['routes'][0]['upstream_model']
                    assert capture['body']['max_tokens']==512
                    assert capture['credential_hash']==hashlib.sha256(b'Bearer batch-key').hexdigest()
                # Sync again: additions are disabled, nothing gets duplicated.
                await page.get_by_role('button',name='同步 '+name+' 的模型',exact=True).click()
                await expect(dialog.get_by_role('checkbox',name='google/gemini-flash',exact=True)).to_be_disabled()
                await expect(dialog.get_by_role('button',name='添加 0 个模型',exact=True)).to_be_disabled()
                await dialog.get_by_role('button',name='返回',exact=True).click()
                await context.close()
                results.append(name);print('PASS batch UI/credentials '+name,flush=True)
            context=await browser.new_context(viewport={'width':390,'height':900})
            page=await context.new_page();await page.goto(url)
            page.on('pageerror',lambda error:errors.append(str(error)))
            await page.get_by_role('button',name='切换侧栏',exact=True).click()
            await page.get_by_role('button',name='设置',exact=True).click()
            before=await rpc(page,'model_catalog.get')
            await page.get_by_role('button',name='添加模型',exact=True).click()
            editor=page.get_by_role('dialog',name='添加模型',exact=True)
            await editor.get_by_label('API 地址',exact=True).fill(endpoint+'/fresh/v1')
            await editor.get_by_label('API 密钥',exact=True).fill('fresh-key')
            await editor.get_by_label('显示名称（选填）',exact=True).fill('Draft name')
            # Back from an empty discovery response keeps the hand-entered draft.
            listed=server.models;server.models=[]
            await editor.get_by_role('button',name='获取模型列表',exact=True).click()
            dialog=page.get_by_role('dialog',name='同步模型',exact=True)
            await expect(dialog.get_by_text('服务没有返回模型，可以返回手动填写模型 ID。',exact=True)).to_be_visible()
            await expect(dialog.get_by_role('button',name='添加 0 个模型',exact=True)).to_be_disabled()
            await dialog.get_by_role('button',name='返回',exact=True).click()
            await expect(editor.get_by_label('API 地址',exact=True)).to_have_value(endpoint+'/fresh/v1')
            await expect(editor.get_by_label('API 密钥',exact=True)).to_have_value('fresh-key')
            await expect(editor.get_by_label('显示名称（选填）',exact=True)).to_have_value('Draft name')
            server.models=listed
            await editor.get_by_role('button',name='获取模型列表',exact=True).click()
            search=dialog.get_by_role('textbox',name='搜索模型',exact=True)
            await expect(search).to_be_focused()
            await search.fill('does-not-exist')
            await expect(dialog.get_by_text('没有匹配的模型，试试其他关键词。',exact=True)).to_be_visible()
            await search.fill('')
            await dialog.get_by_role('checkbox',name='全选搜索结果',exact=True).check()
            await expect(dialog.get_by_role('button',name='添加 4 个模型',exact=True)).to_be_enabled()
            await dialog.get_by_role('button',name='清空选择',exact=True).click()
            await expect(dialog.get_by_role('button',name='添加 0 个模型',exact=True)).to_be_disabled()
            for model_id in ['gpt-4o','google/gemini-flash']:
                await dialog.get_by_role('checkbox',name=model_id,exact=True).check()
            await dialog.get_by_role('button',name='模型参数',exact=True).click()
            await dialog.get_by_label('最大输入 Token',exact=True).fill('8192')
            await dialog.get_by_label('最大输出 Token',exact=True).fill('512')
            await dialog.get_by_role('button',name='模型参数',exact=True).click()
            await dialog.get_by_role('combobox',name='默认模型',exact=True).click()
            await page.get_by_role('option',name='google/gemini-flash',exact=True).click()
            for control in [dialog.get_by_role('heading',name='同步模型',exact=True),
                            dialog.get_by_role('button',name='添加 2 个模型',exact=True),
                            dialog.get_by_role('button',name='返回',exact=True)]:
                await expect(control).to_be_in_viewport(ratio=1)
            await page.screenshot(path=str(artifact/'fresh-390-default.png'),full_page=True)
            await dialog.get_by_role('button',name='添加 2 个模型',exact=True).click()
            await expect(dialog).not_to_be_visible()
            after=await rpc(page,'model_catalog.get')
            assert after['version']==before['version']+1
            added=[m for m in after['models'] if m['id'] not in {x['id'] for x in before['models']}]
            assert len(added)==2
            assert next(m for m in added if m['id']==after['default_model'])['display_name']=='google/gemini-flash'
            assert len({m['routes'][0]['config']['credential_ref'] for m in added})==2
            for model in added:
                chat=(await rpc(page,'chats.new',{'avatar':avatar,'greeting_index':0}))['file_name']
                await rpc(page,'conversation_model.set',{'conversation':{'kind':'private','avatar':avatar,'chat_file':chat},'model_id':model['id']})
                result=await rpc(page,'generate.run',{'avatar':avatar,'chat_file':chat,'user_message':'hello fresh batch'})
                assert result['routing']['status']=='complete'
                capture=server.captures[-1]
                assert capture['body']['model']==model['routes'][0]['upstream_model']
                assert capture['body']['max_tokens']==512
                assert capture['credential_hash']==hashlib.sha256(b'Bearer fresh-key').hexdigest()
            results.append('fresh-api-batch-default-and-draft');print('PASS fresh API batch/default/draft',flush=True)
            # Invalid copy sources/targets must fail without publishing any partial catalog or secrets.
            catalog=await rpc(page,'model_catalog.get')
            saved=(user/'models.json').read_bytes();secrets=(user/'secrets.json').read_bytes()
            for variant in ['endpoint','protocol','missing','existing','ambiguous']:
                proposed=copy.deepcopy(catalog)
                model=copy.deepcopy(next(m for m in proposed['models'] if m['id']=='model-dark-1440'))
                model['id']='invalid-copy';model['routes'][0]['id']='invalid-target'
                target=model['routes'][0]
                if variant=='endpoint':target['config']['endpoint']='https://other.example/v1'
                if variant=='protocol':target['protocol']='anthropic'
                proposed['models'].append(model)
                params={'catalog':proposed,'credential_copies':{'invalid-target':'source-dark-1440'}}
                if variant=='missing':params['credential_copies']['invalid-target']='missing'
                if variant=='existing':params['credential_copies']={'source-dark-1440':'source-dark-1440'}
                if variant=='ambiguous':params['credentials']={'invalid-target':'other-key'}
                try:await rpc(page,'model_catalog.save',params)
                except Exception as e:assert 'RPC timeout' not in str(e)
                else:raise AssertionError('copy should fail: '+variant)
                assert (user/'models.json').read_bytes()==saved
                assert (user/'secrets.json').read_bytes()==secrets
            results.append('credential-copy-validation-and-atomic-failure')
            assert not errors,errors
            await browser.close()
    except Exception:
        if page and not page.is_closed():
            try:await page.screenshot(path=str(artifact/'failure.png'),full_page=True)
            except Exception:pass
        raise
    finally:
        if process.poll() is None:
            process.terminate()
            try:process.wait(timeout=35)
            except subprocess.TimeoutExpired:process.kill();process.wait(timeout=10)
        log.close();server.shutdown();server.server_close()
        write_json(artifact/'report.json',{'passed':results,'browser_errors':errors})
    print('PASS batch model synchronization acceptance',flush=True)

if __name__=='__main__':asyncio.run(main())
