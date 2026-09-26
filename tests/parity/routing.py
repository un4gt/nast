"""Isolated end-to-end routing acceptance: real RPC, storage and three upstream protocols."""
from __future__ import annotations
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
import time
from playwright.async_api import async_playwright
from model_server import ModelServer
from run import ROOT, card, port, rpc, wait_http, write_json


async def main():
    artifact = Path(tempfile.mkdtemp(prefix='nast-routing-'))
    print(f'Evidence: {artifact}', flush=True)
    server = ModelServer(output=artifact/'requests.jsonl')
    threading.Thread(target=server.serve_forever,daemon=True).start()
    endpoint = f'http://127.0.0.1:{server.server_port}'
    user = artifact/'data'/'default-user'
    user.mkdir(parents=True)
    settings = {'oai_settings':{'custom_url':endpoint+'/legacy/v1','custom_model':'gpt-4o','chat_completion_source':'custom','stream_openai':True,'openai_max_tokens':128,'openai_max_context':16384}, 'power_user':{'username':'User'}}
    write_json(user/'settings.json',settings)
    write_json(user/'secrets.json',{'api_key_custom':[{'active':True,'value':'legacy-secret'}]})
    plugins = artifact/'plugins';plugins.mkdir()
    (plugins/'counter.lua').write_text('for _, event in ipairs({"user_input", "prompt_built"}) do nast.on(event, function() nast.set_var(event, tostring(tonumber(nast.get_var(event) or "0") + 1)) end) end',encoding='utf-8')
    http_port = port()
    env = {**os.environ,'NAST_PORT':str(http_port),'NAST_DATA':str(artifact/'data'),'NAST_WEB':str(ROOT/'web'/'dist'),'NAST_BIND':'127.0.0.1', 'NAST_USERNAME':'', 'NAST_PASSWORD':'', 'NAST_BRIDGE_TOKEN':'', 'NAST_PUBLIC_ORIGIN':'', 'NAST_ALLOW_ANONYMOUS':'true'}
    binary = ROOT/'target'/'debug'/('nast.exe' if os.name=='nt' else 'nast')
    log = (artifact/'server.log').open('w',encoding='utf-8')
    process = subprocess.Popen([str(binary)],cwd=artifact,env=env,stdout=log,stderr=log)
    results=[]
    def passed(name):
        results.append({'name':name,'passed':True}); print('PASS '+name,flush=True)
    try:
        await wait_http(f'http://127.0.0.1:{http_port}',process)
        async with async_playwright() as pw:
            browser=await pw.chromium.launch()
            page=await browser.new_page()
            await page.goto(f'http://127.0.0.1:{http_port}')
            async def restart():
                nonlocal process, page
                # Linux SIGTERM drains active WebSockets; Windows terminate is immediate.
                # Close clients first and allow Actix's 30-second graceful shutdown.
                await page.close()
                process.terminate()
                await asyncio.to_thread(process.wait, timeout=35)
                process=subprocess.Popen([str(binary)],cwd=artifact,env=env,stdout=log,stderr=log)
                await wait_http(f'http://127.0.0.1:{http_port}',process)
                page=await browser.new_page()
                await page.goto(f'http://127.0.0.1:{http_port}')
            async def call(method,**params): return await rpc(page,method,params)
            async def fails(method,**params):
                try: await call(method,**params)
                except Exception as error:
                    assert "RPC timeout" not in str(error), error
                    return str(error)
                raise AssertionError('expected failure: '+method)
            catalog=await call('model_catalog.get')
            assert catalog['models'][0]['routes'][0]['credential_configured']
            assert 'legacy-secret' not in json.dumps(catalog)
            assert (user/'backups'/'pre-models-secrets.json').exists()
            original_catalog=(user/'models.json').read_bytes()
            await restart()
            assert (user/'models.json').read_bytes()==original_catalog
            passed('migration/backup-credential-idempotent-restart')
            avatar=(await call('characters.import',filename='actor.json',data_base64=base64.b64encode(json.dumps(card('RouterActor')).encode()).decode()))['avatar']
            def route(id,protocol='openai',priority=0):
                return {'id':id,'provider':id,'protocol':protocol,'upstream_model':'gpt-4o' if protocol=='openai' else 'native-test','priority':priority,'enabled':True,'config':{'endpoint':endpoint+'/'+id+'/v1','context_limit':16384,'output_limit':1024,'connect_timeout_secs':2,'first_token_timeout_secs':2,'idle_timeout_secs':2,'headers':{},'parameters':{},'remove_parameters':[]}}
            async def configure(routes, credentials=None):
                c=await call('model_catalog.get')
                c['models']=[{'id':'default','display_name':'主模型','routes':routes},{'id':'second','display_name':'第二模型','routes':[route('second')]}]
                c['default_model']='default'
                await call('model_catalog.save',catalog=c,credentials=credentials or {})
            async def new(): return (await call('chats.new',avatar=avatar,greeting_index=0))['file_name']
            def ref(chat): return {'kind':'private','avatar':avatar,'chat_file':chat}
            async def selection(chat): return (await call('conversation_model.get',conversation=ref(chat)))['state']
            async def generate(chat,**extra): return await call('generate.run',avatar=avatar,chat_file=chat,user_message='hello',**extra)
            async def lines(chat): return await call('chats.get',avatar=avatar,file_name=chat)
            async def switch(chat,id): return await call('conversation_model.set',conversation=ref(chat),model_id=id)
            await configure([route('a',priority=10),route('b',priority=0)],{'a':'key-a','b':'key-b'})
            c=await call('model_catalog.get')
            await call('model_catalog.save',catalog=c)
            assert '模型目录已更新' in await fails('model_catalog.save',catalog=c)
            invalid=await call('model_catalog.get');invalid['models'][0]['routes'][0]['config']['parameters']={'model':'wrong'}
            assert '不能覆盖' in await fails('model_catalog.save',catalog=invalid)
            published=(user/'models.json').read_bytes()
            (user/'models.json').rename(user/'models.saved')
            (user/'models.json').mkdir()
            try:
                broken=await call('model_catalog.get');broken['models'][0]['display_name']='must not publish'
                await fails('model_catalog.save',catalog=broken,credentials={'a':'must-not-publish'})
                assert (await call('model_catalog.get'))['models'][0]['display_name']=='主模型'
            finally:
                (user/'models.json').rmdir();(user/'models.saved').rename(user/'models.json')
            assert (user/'models.json').read_bytes()==published
            passed('catalog/conflict-validation-atomic-failure')

            chat=await new();other=await new()
            server.plans['a']=[{'status':503}]*2
            start=len(server.captures)
            result=await generate(chat)
            captures=server.captures[start:]
            assert [c['route'] for c in captures]==['a','a','b']
            assert captures[0]['body']==captures[1]['body']
            assert captures[0]['credential_hash']==hashlib.sha256(b'Bearer key-a').hexdigest()
            assert captures[2]['credential_hash']==hashlib.sha256(b'Bearer key-b').hexdigest()
            assert result['routing']['status']=='complete'
            assert (await selection(chat))['active_route']=='b'
            assert len([m for m in await lines(chat) if m.get('is_user')])==1
            kv=json.loads((user/'plugin_vars.json').read_text())
            assert all(str(value)=='1' for value in kv.values()), kv
            await generate(chat);assert server.captures[-1]['route']=='b'
            await generate(other);assert server.captures[-1]['route']=='a'
            await switch(chat,'default');assert (await selection(chat))['active_route']=='b'
            await switch(chat,'second');assert 'active_route' not in await selection(chat)
            await switch(chat,'default')
            passed('routing/exact-retries-fallback-stickiness-conversation-isolation-secrets')

            for status in [400,401,403,413,422,500]:
                current=await new();server.plans['a']=[{'status':status}];start=len(server.captures)
                await fails('generate.run',avatar=avatar,chat_file=current,user_message='fail')
                assert len(server.captures)-start==1
                assert 'active_route' not in await selection(current)
            for status in [429,502,503,504]:
                current=await new();server.plans['a']=[{'status':status}];start=len(server.captures)
                await generate(current);assert [c['route'] for c in server.captures[start:]]==['a','a']
            current=await new();server.plans['a']=[{'status':401,'message':'invalid credential key-a; check account'}]
            error=await fails('generate.run',avatar=avatar,chat_file=current)
            assert 'check account' in error and 'key-a' not in error and '[REDACTED]' in error
            passed('routing/transient-and-terminal-status-boundaries')
            current=await new();server.plans['a']=[{'status':503}]*2;server.plans['b']=[{'status':503}]*2
            start=len(server.captures);await fails('generate.run',avatar=avatar,chat_file=current)
            assert [c['route'] for c in server.captures[start:]]==['a','a','b','b']
            passed('routing/all-routes-exhausted-without-wraparound')
            current=await new();server.plans['a']=[{'status':429,'retry_after':31}];start=len(server.captures)
            await generate(current);assert [c['route'] for c in server.captures[start:]]==['a','b']
            current=await new();server.plans['a']=[{'status':429,'retry_after':2}];t=time.monotonic();await generate(current)
            assert time.monotonic()-t>=2
            passed('routing/retry-after-long-skip-and-short-wait')
            for cutoff in ['before','reasoning','body','no_done']:
                current=await new();server.plans['a']=[{'cutoff':cutoff}]*2;start=len(server.captures)
                result=await generate(current)
                if cutoff=='before':
                    assert [c['route'] for c in server.captures[start:]]==['a','a','b']
                    assert result['routing']['status']=='complete'
                else:
                    assert len(server.captures)-start==1
                    assert result['routing']['status']=='incomplete'
                    saved=(await lines(current))[-1]
                    assert saved['extra']['nast_model']['status']=='incomplete'
                    assert saved['extra']['reasoning']
                    assert 'active_route' not in await selection(current)
                server.plans['a']=[]
            passed('stream/eof-before-content-and-partial-body-or-reasoning')
            current=await new();server.plans['a']=[{'invalid_json':True}];start=len(server.captures)
            await fails('generate.run',avatar=avatar,chat_file=current)
            assert len(server.captures)-start==1
            passed('stream/invalid-response-is-terminal')
            current=await new();server.plans['a']=[{'status':503}];start=len(server.captures)
            task=asyncio.create_task(generate(current,task_id='cancel-backoff'))
            while len(server.captures)==start: await asyncio.sleep(.02)
            assert not (await call('generate.stop',task_id='other-task'))['ok']
            await call('generate.stop',task_id='cancel-backoff')
            try: await task
            except Exception: pass
            assert len(server.captures)-start==1
            current=await new();server.plans['a']=[{'delay':2}];start=len(server.captures)
            await fails('generate.run',avatar=avatar,chat_file=current,time_budget_secs=1)
            assert len(server.captures)-start<=1
            passed('cancellation/task-identity-backoff-total-budget')

            for protocol in ['anthropic','gemini']:
                await configure([route(protocol,protocol)])
                for streaming in [False,True]:
                    settings['oai_settings']['stream_openai']=streaming
                    await call('settings.save',settings=settings)
                    current=await new();result=await generate(current)
                    assert result['routing']['status']=='complete'
                    assert (await lines(current))[-1]['extra']['reasoning']=='核对上下文'
                current=await new();server.plans[protocol]=[{'stream_error':'overloaded_error' if protocol=='anthropic' else 'UNAVAILABLE'}]
                start=len(server.captures);await generate(current);assert len(server.captures)-start==2
            passed('protocols/native-endpoints-json-stream-reasoning-native-temporary-errors')
            for protocol, reason in [('openai','length'),('anthropic','max_tokens'),('gemini','MAX_TOKENS')]:
                await configure([route('limit',protocol),route('unused')])
                for streaming in [False,True]:
                    settings['oai_settings']['stream_openai']=streaming
                    await call('settings.save',settings=settings)
                    current=await new();server.plans['limit']=[{'finish_reason':reason}];start=len(server.captures)
                    result=await generate(current,task_id='output-limit-'+protocol+'-'+str(streaming))
                    assert result['routing']['status']=='incomplete'
                    assert result['routing']['finish_reason']==reason
                    assert result['routing']['error']['detail']['type']=='output_limit'
                    assert len(server.captures)==start+1 and result['text']
                    assert 'active_route' not in await selection(current)
                    assert (await lines(current))[-1]['extra']['nast_model']['status']=='incomplete'
            passed('protocols/output-limit-json-and-stream-retains-text-without-failover')


            await configure([route('a'),route('b')])
            current=await new()
            for kind in ['normal','swipe','continue','regenerate','quiet','impersonate']:
                result=await generate(current,type=kind)
                assert result['routing']['status']=='complete'
            saved=await lines(current)
            assert saved[-1]['extra']['nast_model']['upstream_model']=='gpt-4o'
            assert saved[-1]['swipe_info'][0]['extra']['nast_model']['route']=='a'
            passed('entrypoints/all-generation-modes-and-history-provenance')
            start=len(await lines(current));before=len(server.captures)
            command=await call('generate.run',avatar=avatar,chat_file=current,user_message='/model second')
            assert command['command'] and len(await lines(current))==start and len(server.captures)==before
            assert (await selection(current))['selected_model']=='second'
            info=await call('model.command',conversation=ref(current),argument='info')
            assert '下次候选路由' in info['text']
            c=await call('model_catalog.get');c['default_model']='second';await call('model_catalog.save',catalog=c)
            assert (await selection(other))['selected_model']=='default'
            assert (await selection(await new()))['selected_model']=='second'
            renamed=(await call('chats.rename',avatar=avatar,original_file=current,renamed_file='renamed'))['name']+'.jsonl'
            assert (await selection(renamed))['selected_model']=='second'
            imported=await lines(other);await call('chats.save',avatar=avatar,file_name='imported.jsonl',chat=imported)
            assert (await selection('imported.jsonl'))['selected_model']=='default' and 'active_route' not in await selection('imported.jsonl')
            passed('conversation/commands-default-rename-import')

            avatar2=(await call('characters.import',filename='actor2.json',data_base64=base64.b64encode(json.dumps(card('RouterActor2')).encode()).decode()))['avatar']
            group=await call('groups.create',group={'name':'RoutingGroup','members':[avatar,avatar2],'activation_strategy':1,'generation_mode':0})
            await call('groups.new_chat',id=group['id'])
            group=next(g for g in await call('groups.all') if g['id']==group['id'])
            group_ref={'kind':'group','group_id':group['id'],'chat_id':group['chat_id']}
            await call('conversation_model.set',conversation=group_ref,model_id='default')
            server.plans['a']=[{'status':503}]*2;start=len(server.captures)
            result=await call('generate.group',id=group['id'],chat_id=group['chat_id'],user_message='group')
            assert [c['route'] for c in server.captures[start:]]==['a','a','b','b']
            assert len(result['replies'])==2
            await call('conversation_model.set',conversation=group_ref,model_id='second')
            await call('conversation_model.set',conversation=group_ref,model_id='default')
            server.plans['a']=[{'cutoff':'reasoning'}];start=len(server.captures)
            result=await call('generate.group',id=group['id'],chat_id=group['chat_id'],user_message='partial')
            assert len(result['replies'])==1 and len(server.captures)-start==1
            assert result['replies'][0]['routing']['status']=='incomplete'
            passed('groups/shared-stickiness-and-stop-after-partial-member')
            a,b,c=route('a',priority=10),route('b',priority=5),route('c',priority=0)
            a['enabled']=b['enabled']=False
            await configure([a,b,c])
            await call('generate.group',id=group['id'],chat_id=group['chat_id'],member=avatar,user_message='seed sticky c')
            a['enabled']=b['enabled']=True;await configure([a,b,c])
            server.plans['c']=[{'status':503}]*2
            server.plans['a']=[{'status':503},{'status':503},{}]
            server.plans['b']=[{},{'status':503},{'status':503}]
            start=len(server.captures)
            await call('generate.group',id=group['id'],chat_id=group['chat_id'],user_message='reorder sticky')
            assert [x['route'] for x in server.captures[start:]]==['c','c','a','a','b','b','b','a']
            passed('groups/priority-after-sticky-route-changes-between-members')

            # Long Chinese conversations must be cropped with the same estimate used at send time.
            for protocol in ['openai','anthropic','gemini']:
                bounded=route('budget',protocol)
                bounded['upstream_model']='family-chat-v1'  # unknown/custom model alias
                bounded['config'].update(context_limit=1536,input_limit=1250,output_limit=128)
                await configure([bounded])
                current=await new();seed=await lines(current)
                for i in range(80):
                    seed.append({'name':'User' if i%2==0 else 'RouterActor','is_user':i%2==0,
                                 'mes':f'历史记录{i}：'+('妈妈，今天上课好无聊，终于下课了。😊'*8),
                                 'send_date':'2026-09-26T10:00:00Z','is_system':False})
                await call('chats.save',avatar=avatar,file_name=current,chat=seed,force=True)
                start=len(server.captures)
                result=await call('generate.run',avatar=avatar,chat_file=current,user_message='最新问题保留标记：今天晚餐吃什么？')
                assert result['routing']['status']=='complete' and len(server.captures)==start+1
                body=json.dumps(server.captures[-1]['body'],ensure_ascii=False)
                assert '最新问题保留标记' in body and '历史记录0：' not in body
                assert '历史记录79：' in body
                if protocol=='anthropic':
                    settings['oai_settings']['continue_prefill']=True
                    await call('settings.save',settings=settings)
                    prior=result['text']
                    result=await generate(current,type='continue')
                    assert result['routing']['status']=='complete'
                    messages=server.captures[-1]['body']['messages']
                    assert sum(json.dumps(m,ensure_ascii=False).count(prior) for m in messages)==1
                    settings['oai_settings']['continue_prefill']=False
                    await call('settings.save',settings=settings)
            passed('budget/long-chinese-custom-alias-three-protocols-and-single-prefill')

            # Preserve request semantics when fallback has a smaller context or unsupported capabilities.
            smaller=route('b');smaller['config']['context_limit']=129;smaller['config']['output_limit']=128
            await configure([route('a'),smaller]);current=await new();server.plans['a']=[{'status':503}]*2;start=len(server.captures)
            error=await fails('generate.run',avatar=avatar,chat_file=current,user_message='context')
            assert '上下文上限' in error and '估算输入' in error and '预留输出' in error and len(server.captures)-start==2
            passed('routing/fallback-context-validation-no-retrim')
            smaller=route('b');smaller['config']['input_limit']=1
            await configure([route('a'),smaller]);current=await new();server.plans['a']=[{'status':503}]*2;start=len(server.captures)
            error=await fails('generate.run',avatar=avatar,chat_file=current,user_message='input budget')
            assert '最大输入' in error and len(server.captures)-start==2
            passed('routing/fallback-input-limit-no-retrim')

            await configure([route('a'),route('b')],{'a':'frozen-a','b':'frozen-b'})
            current=await new();server.plans['a']=[{'status':503,'retry_after':3},{'status':503}];start=len(server.captures)
            task=asyncio.create_task(generate(current))
            while len(server.captures)==start: await asyncio.sleep(.02)
            changed=await call('model_catalog.get')
            changed['models'][0]['routes'][1]['upstream_model']='changed-after-start'
            await call('model_catalog.save',catalog=changed,credentials={'b':'changed-key'})
            await task
            attempts=server.captures[start:]
            assert [c['route'] for c in attempts]==['a','a','b']
            assert attempts[-1]['body']['model']=='gpt-4o'
            assert attempts[-1]['credential_hash']==hashlib.sha256(b'Bearer frozen-b').hexdigest()
            assert (await selection(current))['active_route_info']['upstream_model']=='gpt-4o'
            persisted=(await selection(current))
            await restart()
            assert await selection(current)==persisted
            await generate(current);assert server.captures[-1]['body']['model']=='changed-after-start'
            assert server.captures[-1]['route']=='b'
            passed('routing/frozen-config-credentials-and-successful-stickiness-after-restart')
            timed=route('a');timed['config']['first_token_timeout_secs']=1
            await configure([timed,route('b')]);current=await new();server.plans['a']=[{'delay':2}]*2;start=len(server.captures)
            await generate(current)
            assert [c['route'] for c in server.captures[start:]]==['a','a','b']
            timed['config']['idle_timeout_secs']=1
            await configure([timed,route('b')]);current=await new();server.plans['a']=[{'chunk_delay':2}];start=len(server.captures)
            result=await generate(current);assert result['routing']['status']=='incomplete' and len(server.captures)-start==1
            passed('timeouts/first-content-failover-and-reasoning-idle-partial')
            current=await new();await switch(current,'second')
            deleted=await call('model_catalog.get');deleted['models']=[deleted['models'][0]]
            await call('model_catalog.save',catalog=deleted)
            assert (await call('conversation_model.get',conversation=ref(current)))['error']
            start=len(server.captures);await fails('generate.run',avatar=avatar,chat_file=current)
            assert len(server.captures)==start
            passed('catalog/deleted-selection-is-actionable-without-auto-switch')
            await configure([route('a'),route('b')])
            from model_ui import check_models_ui
            await check_models_ui(page,artifact,'RouterActor')
            passed('ui/model-selector-keyboard-themes-narrow-draft-conflict')
            logs=(artifact/'server.log').read_text(encoding='utf-8')
            assert 'generation_attempt' in logs and 'generation_finished' in logs and 'generation_backoff' in logs
            assert 'output-limit-openai-True' in logs and 'output_limit' in logs
            assert 'key-a' not in logs and 'frozen-b' not in logs and 'legacy-secret' not in logs
            passed('observability/task-retry-finish-reason-and-secret-free-logs')
            await browser.close()
            corrupt_root=artifact/'corrupt-data';corrupt_user=corrupt_root/'default-user';corrupt_user.mkdir(parents=True)
            (corrupt_user/'secrets.json').write_bytes(b'{broken-secret-file')
            broken_process=subprocess.Popen([str(binary)],cwd=artifact,env={**env,'NAST_DATA':str(corrupt_root)},stdout=log,stderr=log)
            assert broken_process.wait(timeout=10)!=0
            assert (corrupt_user/'secrets.json').read_bytes()==b'{broken-secret-file'
            assert not (corrupt_user/'models.json').exists()
            passed('migration/corrupt-secrets-fail-closed-without-overwrite')
    finally:
        if process.poll() is None:
            process.terminate()
            try: process.wait(timeout=35)
            except subprocess.TimeoutExpired:
                process.kill();process.wait(timeout=10)
        log.close();server.shutdown();server.server_close()
        write_json(artifact/'report.json',results)
    print(f'{len(results)} routing acceptance groups passed',flush=True)

if __name__=='__main__': asyncio.run(main())
