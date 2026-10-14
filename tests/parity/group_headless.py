"""No browser is started. Real RPC, isolated data, local deterministic model, restart and faults."""
import asyncio
import base64
import json
import os
from pathlib import Path
import subprocess
import shutil
import tempfile
import threading
import time
import websockets
from model_server import ModelServer
from run import ROOT, card, port, wait_http, write_json


class Client:
    async def connect(self, url):
        self.ws = await websockets.connect(url, max_size=16*1024*1024)
        self.pending, self.events, self.serial = {}, [], 0
        self.reader = asyncio.create_task(self.read())
        return self

    async def read(self):
        try:
            async for raw in self.ws:
                value = json.loads(raw)
                if 'event' in value:
                    self.events.append(value)
                elif value.get('id') in self.pending:
                    future = self.pending.pop(value['id'])
                    if not future.done():
                        if 'error' in value: future.set_exception(RuntimeError(json.dumps(value['error'])))
                        else: future.set_result(value.get('result'))
        except websockets.exceptions.ConnectionClosed:
            pass
        finally:
            for future in self.pending.values():
                if not future.done(): future.set_exception(ConnectionError('disconnected'))

    async def call(self, method, **params):
        self.serial += 1
        ident = str(self.serial)
        future = asyncio.get_running_loop().create_future()
        self.pending[ident] = future
        await self.ws.send(json.dumps({'id':ident, 'method':method, 'params':params}))
        return await asyncio.wait_for(future, 60)

    async def close(self):
        await self.ws.close()
        await self.reader


PLUGIN = r'''
nast.register_command('say', function(args, ctx)
  assert(ctx.is_group and ctx.group_id and ctx.conversation.chat_id)
  nast.set_var('commands', (nast.get_var('commands') or 0) + 1)
  nast.notify('server notification', 'info')
  return 'command:' .. args
end)
nast.register_command('queue', function(args) nast.set_var('queue', args) end)
nast.register_command('touch', function() nast.set_var('touch', (nast.get_var('touch') or 0) + 1) end)
nast.register_command('broken', function() error('intentional command failure') end)
nast.on('user_input', function(raw)
  local d = nast.json_decode(raw)
  assert(d.group_id and d.chat_id and d.task_id and d.batch_id)
  if d.text:sub(1,8) == 'command:' then assert(d.trigger == 'command') end
  nast.set_var('inputs', (nast.get_var('inputs') or 0) + 1)
  return d.text .. ':input'
end)
nast.on('group_queue', function(raw)
  local d = nast.json_decode(raw)
  local mode = nast.get_var('queue')
  if mode == 'empty' then return {} end
  if mode == 'bad' then return {'outside.png'} end
  if mode == 'reverse' then local q={}; for i=#d.queue,1,-1 do q[#q+1]=d.queue[i] end; return q end
end)
nast.on('prompt_built', function(raw)
  local d = nast.json_decode(raw)
  assert(d.member and d.batch_id and d.group_id and d.task_id)
  table.insert(d.messages, {role='system',content='HEADLESS-PLUGIN'})
  return {messages=d.messages}
end)
nast.on('ai_output', function(raw) return nast.json_decode(raw).text .. ':output' end)
for _, event in ipairs({'group_batch_started','group_batch_ended','group_member_started','group_member_ended','message_saved'}) do
  nast.on(event,function(raw)
    nast.set_var(event, (nast.get_var(event) or 0) + 1)
  end)
end
'''


async def main():
    artifact = Path(tempfile.mkdtemp(prefix='nast-group-headless-'))
    print('Evidence:', artifact, flush=True)
    server = ModelServer(output=artifact/'requests.jsonl')
    threading.Thread(target=server.serve_forever, daemon=True).start()
    user = artifact/'data/default-user'; user.mkdir(parents=True)
    write_json(user/'settings.json', {'oai_settings':{'custom_url':f'http://127.0.0.1:{server.server_port}/test/v1', 'custom_model':'gpt-4o','chat_completion_source':'custom','stream_openai':True,'openai_max_tokens':128,'openai_max_context':8192},'power_user':{'username':'User','personas':{'tester':'Tester'}}})
    plugins = artifact/'plugins'; plugins.mkdir()
    (plugins/'00_empty.lua').write_text("nast.on('unrelated', function() end)")
    (plugins/'10_group.lua').write_text(PLUGIN)
    http_port = port()
    env = {**os.environ, 'NAST_DATA':str(artifact/'data'),'NAST_PORT':str(http_port),'NAST_BIND':'127.0.0.1',
        'NAST_ALLOW_ANONYMOUS':'true','NAST_USERNAME':'','NAST_PASSWORD':'','NAST_BRIDGE_TOKEN':'','NAST_PUBLIC_ORIGIN':'',
        'NAST_WEB':str(ROOT/'web/dist'),'OPENAI_API_KEY':'','NAST_OPENAI_BASE':'','NAST_PLUGIN_TIMEOUT_SECS':'1'}
    log = (artifact/'server.log').open('w',encoding='utf-8')
    binary = Path(os.environ.get('NAST_TEST_BINARY', ROOT/'target/debug'/('nast.exe' if os.name == 'nt' else 'nast')))
    isolated_binary = artifact/binary.name
    shutil.copy2(binary, isolated_binary)
    binary = isolated_binary
    process = None; clients = []; results = []
    async def start():
        nonlocal process
        process = subprocess.Popen([str(binary)], cwd=artifact, env=env, stdout=log, stderr=log)
        await wait_http(f'http://127.0.0.1:{http_port}/healthz',process)
        client = await Client().connect(f'ws://127.0.0.1:{http_port}/ws'); clients.append(client)
        return client
    def passed(name):
        results.append({'name':name,'passed':True}); print('PASS',name,flush=True)
    def kv(): return json.loads((user/'plugin_vars.json').read_text())
    async def failure(call):
        try: await call
        except RuntimeError: return
        raise AssertionError('expected rejection')
    try:
        c = await start()
        avatars=[]
        for name in ['HeadlessA','HeadlessB']:
            avatars.append((await c.call('characters.import',filename=name+'.json',data_base64=base64.b64encode(json.dumps(card(name)).encode()).decode()))['avatar'])
        group = await c.call('groups.create',group={'name':'Headless','members':avatars,'activation_strategy':1,'auto_mode_delay':1})
        g = {'id':group['id'],'chat_id':group['chat_id']}
        conversation = {'kind':'group','group_id':g['id'],'chat_id':g['chat_id']}
        await c.call('groups.get_chat',**g)
        async def command(text, **extra): return await c.call('commands.execute',conversation=conversation,input=text,**extra)
        await command('/queue reverse')
        result = await command('/say hello',task_id='command-once')
        assert result['activated'] == avatars[::-1]
        assert kv()['plugin:10_group:commands']==1 and kv()['plugin:10_group:inputs']==1
        chat=await c.call('groups.get_chat',**g)
        assert chat[-3]['mes']=='command:hello:input'
        assert all(m['mes'].endswith(':output') for m in chat[-2:])
        assert chat[-1]['extra']['gen_id']==chat[-2]['extra']['gen_id']
        assert all(any(m['content']=='HEADLESS-PLUGIN' for m in request['body']['messages']) for request in server.captures)
        count=len(server.captures)
        assert await command('/say hello',task_id='command-once') == result
        assert len(server.captures)==count and kv()['plugin:10_group:commands']==1
        await failure(command('/say different',task_id='command-once'))
        passed('commands/no-discovery/context/hooks-once/replay')
        assert any(e['event']=='notification' for e in c.events)
        for name in ['unknown-command']:
            assert (await command('/'+name))['status']=='unknown'
        assert len(server.captures)==count
        await failure(command('/broken'))
        assert len(server.captures)==count
        assert (await command('/membercount'))['result']==2
        assert (await command('/getmember field=avatar 0'))['result']==avatars[0]
        await command('/disablemember 0')
        await command('/enablemember 0')
        await command('/memberdown 0')
        await command('/memberup 1')
        await command('/persona Tester')
        passed('commands/aliases-index-persona-error-no-generation')
        await command('/queue empty')
        assert (await c.call('generate.group',**g,user_message='empty'))['activated']==[]
        assert len(server.captures)==count
        forced=await c.call('generate.group',**g,member=avatars[0])
        assert forced['activated']==[avatars[0]]
        await command('/queue bad')
        assert (await c.call('generate.group',**g,user_message='bad queue'))['activated']==avatars
        passed('queue/empty-invalid-and-forced-member')
        await command('/queue reverse')
        await c.call('generate.group',**g,type='swipe')
        await c.call('groups.swipe',**g,direction='left')
        await c.call('groups.delete_swipe',**g)
        before=await c.call('groups.get_chat',**g)
        assert len(before[-1]['swipes'])==1
        await command('/swipe right')
        assert len((await c.call('groups.get_chat',**g))[-1]['swipes'])==2
        for kind in ['quiet','impersonate']:
            result=await c.call('generate.group',**g,type=kind,user_message='quiet instruction')
            assert len(result['replies'])==1
            assert len(await c.call('groups.get_chat',**g))==len(before)
        passed('generation/swipe-delete-quiet-impersonate')
        await c.call('groups.set_metadata',**g,metadata={'foreign':{'keep':[1,2]},'scenario':'scene'})
        await c.call('groups.set_persona',**g,persona='tester')
        await c.call('groups.set_note',**g,note={'prompt':'note','depth':1,'interval':1})
        branch=await c.call('groups.branch',**g,index=1,bookmark=True)
        assert (await c.call('groups.get_chat',id=g['id'],chat_id=branch['chat_id']))[0]['chat_metadata']['foreign']=={'keep':[1,2]}
        renamed=g['chat_id']+'-renamed'
        await c.call('groups.rename_chat',**g,name=renamed)
        assert (await c.call('groups.parent',id=g['id'],chat_id=branch['chat_id']))['chat_id']==renamed
        await c.call('groups.rename_chat',id=g['id'],chat_id=renamed,name=g['chat_id'])
        restored=await c.call('groups.parent',id=g['id'],chat_id=branch['chat_id'])
        assert restored['chat_id']==g['chat_id']
        other=await c.call('groups.create',group={'name':'Other','members':avatars})
        await failure(c.call('groups.delete_message',id=other['id'],chat_id=g['chat_id'],index=0))
        passed('sessions/metadata-persona-note-bookmark-ownership')
        imported_rows=await c.call('groups.get_chat',**g)
        imported_rows[-1]['foreign_message']={'nested':[1,2,3]}
        imported_rows[-1]['swipe_info'][0]['foreign_candidate']='kept'
        imported=await c.call('groups.import_chat',id=g['id'],text='\n'.join(json.dumps(row) for row in imported_rows))
        foreign={'id':g['id'],'chat_id':imported['chat_id']}
        await c.call('groups.update_message',**foreign,index=len(imported_rows)-2,text='edited foreign')
        await c.call('generate.group',**foreign,type='swipe')
        await c.call('generate.group',**foreign,type='continue')
        roundtrip=await c.call('groups.get_chat',**foreign)
        assert roundtrip[-1]['foreign_message']=={'nested':[1,2,3]}
        assert roundtrip[-1]['swipe_info'][0]['foreign_candidate']=='kept'
        await c.call('groups.open_chat',**g)
        passed('storage/unknown-message-and-candidate-fields-roundtrip')
        private=await c.call('chats.new',avatar=avatars[0],greeting_index=0)
        private_target={'avatar':avatars[0],'file_name':private['file_name']}
        original_private=await c.call('chats.get',**private_target)
        converted=await c.call('groups.from_private',avatar=avatars[0],chat_file=private['file_name'],members=[avatars[1]],name='Converted')
        assert converted['members']==avatars
        assert await c.call('chats.get',**private_target)==original_private
        converted_rows=await c.call('groups.get_chat',id=converted['id'],chat_id=converted['chat_id'])
        assert converted_rows[-1]['original_avatar']==avatars[0]
        private_conversation={'kind':'private','avatar':avatars[0],'chat_file':private['file_name']}
        await c.call('commands.execute',conversation=private_conversation,input='/sys private narrator')
        assert (await c.call('chats.get',**private_target))[-1]['mes']=='private narrator'
        await c.call('commands.execute',conversation=private_conversation,input='/persona Tester')
        assert (await c.call('chats.get',**private_target))[0]['chat_metadata']['persona']=='tester'
        passed('sessions/private-conversion-copy-and-shared-commands')
        # A slow upstream provides a deterministic stop/duplicate/disconnect window.
        catalog=await c.call('model_catalog.get'); route=catalog['models'][0]['routes'][0]
        route['config']['endpoint']=f'http://127.0.0.1:{server.server_port}/slow/v1'
        await c.call('model_catalog.save',catalog=catalog)
        server.plans['slow']=[{'delay':1.5}]
        slow=asyncio.create_task(c.call('generate.group',**g,user_message='disconnect',task_id='detached'))
        await asyncio.sleep(.25)
        await c.close(); clients.remove(c)
        try: await slow
        except ConnectionError: pass
        c=await Client().connect(f'ws://127.0.0.1:{http_port}/ws');clients.append(c)
        result=await c.call('generate.group',**g,user_message='disconnect',task_id='detached')
        assert len(result['replies'])==2
        passed('lifecycle/disconnect-reconnect-same-task')
        server.plans['slow']=[{'delay':1.5}]
        slow=asyncio.create_task(c.call('generate.group',**g,user_message='stop',task_id='stoppable'))
        await asyncio.sleep(.25)
        status=await c.call('generate.status')
        assert status['running'] and status['group']['queue'] and status['group']['member'] in avatars
        await failure(c.call('groups.set_metadata',**g,metadata={'race':True}))
        await c.call('generate.stop',task_id='stoppable')
        try: result=await slow
        except RuntimeError: pass
        assert not (await c.call('generate.status'))['running']
        passed('lifecycle/stop-status-mutation-exclusion')
        server.plans['slow']=[{'status':429,'retry_after':0},{}]
        assert len((await c.call('generate.group',**g,member=avatars[0]))['replies'])==1
        before=await c.call('groups.get_chat',**g)
        server.plans['slow']=[{'status':503},{'status':503}]
        await failure(c.call('generate.group',**g,type='regenerate'))
        after=await c.call('groups.get_chat',**g)
        assert before[1:]==after[1:], 'failed regeneration must restore the previous batch'
        server.plans['slow']=[{'cutoff':'body'}]
        partial=await c.call('generate.group',**g,user_message='partial')
        assert len(partial['replies'])==1 and partial['replies'][0]['routing']['status']=='incomplete'
        assert (await c.call('groups.get_chat',**g))[-1]['mes']
        passed('faults/429-retry-503-rollback-partial-stop-next-member')

        (plugins/'05_loop.lua').write_text("nast.on('user_input', function() while true do end end)")
        await c.call('plugins.reload')
        started=time.monotonic()
        await c.call('generate.group',**g,user_message='loop recovery',member=avatars[0])
        assert time.monotonic()-started<30
        assert kv()['plugin:10_group:inputs']>=1
        await c.call('plugins.reload')
        await command('/touch')
        passed('lua/instruction-interrupt-following-plugin-reload')
        # Recreate the process and verify the namespaced KV, with no plugin list call.
        await c.close();clients.remove(c);process.terminate();process.wait(timeout=10)
        c=await start()
        await command('/touch')
        assert kv()['plugin:10_group:touch']==2
        passed('lua/kv-process-restart-without-browser')
        count=len(server.captures)
        await c.call('groups.auto_start',**g,max_turns=1)
        await c.close();clients.remove(c)
        await asyncio.sleep(.1)
        c=await Client().connect(f'ws://127.0.0.1:{http_port}/ws');clients.append(c)
        for _ in range(100):
            if await c.call('groups.auto_status') is None: break
            await asyncio.sleep(.1)
        assert await c.call('groups.auto_status') is None
        assert len(server.captures)==count+2
        passed('automatic/server-owned-after-client-disconnect')

        # 2,000 stateful session operations, including periodic generation and imports.
        operations=0
        for i in range(200):
            current=await c.call('groups.new_chat',id=g['id']);operations+=1
            target={'id':g['id'],'chat_id':current['chat_id']}
            await c.call('groups.set_metadata',**target,metadata={'round':i});operations+=1
            await c.call('groups.add_message',**target,text=f'narrator {i}');operations+=1
            await c.call('groups.add_message',**target,text=f'actor {i}',member=avatars[0]);operations+=1
            await c.call('groups.hide_message',**target,index=3,hidden=True);operations+=1
            await c.call('groups.update_message',**target,index=3,text=f'edited {i}');operations+=1
            exported=await c.call('groups.export_chat',**target);operations+=1
            imported=await c.call('groups.import_chat',id=g['id'],text=base64.b64decode(exported['data_base64']).decode());operations+=1
            imported_chat=await c.call('groups.get_chat',id=g['id'],chat_id=imported['chat_id'])
            assert imported_chat[0]['chat_metadata']['round']==i and imported_chat[-1]['mes']==f'edited {i}'
            assert imported_chat[-1]['is_system'] is True
            await c.call('groups.delete_chat',**target);operations+=1
            await c.call('groups.delete_chat',id=g['id'],chat_id=imported['chat_id']);operations+=1
            if i%20==0:
                print(f'operations {operations}/2000',flush=True)
                await c.call('generate.group',**g,member=avatars[0],type='quiet',user_message='probe')
        passed(f'soak/{operations}-session-operations')
    except Exception as error:
        results.append({'name':'unhandled-failure','passed':False,'error':repr(error)})
        raise
    finally:
        for client in clients: await client.close()
        if process and process.poll() is None: process.terminate();process.wait(timeout=10)
        server.shutdown();server.server_close();log.close()
        write_json(artifact/'report.json',{'browser_started':False,'plugin_discovery_required':False,'results':results,'model_requests':len(server.captures)})
        write_json(ROOT/'.cache/groupchat/headless-latest.json',{'artifact':str(artifact),'results':results})

if __name__=='__main__': asyncio.run(main())
