"""Authentication acceptance against an isolated real server and Chromium."""
import asyncio
import json
import http.client
import os
from pathlib import Path
import subprocess
import tempfile
from playwright.async_api import async_playwright, expect
from run import ROOT, port, wait_http

TOKEN = 'isolated-bridge-token-0123456789abcdef'

async def socket_status(page, url):
    return await page.evaluate('''url => new Promise(resolve => {
        const ws = new WebSocket(url.replace('http', 'ws') + '/ws');
        ws.onopen = () => { ws.send(JSON.stringify({id:'auth-test',method:'characters.all',params:{}})); };
        ws.onmessage = e => { const v=JSON.parse(e.data); if(v.id==='auth-test'){ ws.close(); resolve(!!v.result); } };
        ws.onerror = () => resolve(false);
        setTimeout(() => { ws.close(); resolve(false); }, 4000);
    })''', url)

async def main():
    artifact = Path(tempfile.mkdtemp(prefix='nast-auth-'))
    print('Evidence:', artifact, flush=True)
    url = f'http://127.0.0.1:{port()}'
    env = {**os.environ, 'NAST_BIND':'127.0.0.1', 'NAST_PORT':url.rsplit(':',1)[1],
           'NAST_DATA':str(artifact/'data'), 'NAST_WEB':str(ROOT/'web/dist'),
           'NAST_USERNAME':'admin', 'NAST_PASSWORD':'isolated-test-password',
           'NAST_BRIDGE_TOKEN':TOKEN, 'NAST_ALLOW_ANONYMOUS':'false',
           'NAST_PUBLIC_ORIGIN':'', 'NAST_COOKIE_SECURE':'false'}
    binary = ROOT/'target/debug'/('nast.exe' if os.name == 'nt' else 'nast')
    log = (artifact/'server.log').open('w', encoding='utf-8')
    process = subprocess.Popen([str(binary)], cwd=artifact, env=env, stdout=log, stderr=log)
    results=[]
    try:
        await wait_http(url+'/healthz',process)
        async with async_playwright() as pw:
            browser=await pw.chromium.launch()
            api=await pw.request.new_context(base_url=url)
            for path in ['/ws','/thumbnail?file=a.png','/assets/characters/a/a.png']:
                assert (await api.get(path)).status == 401,path
            for path in ['/upload','/api/tts/synthesize']:
                assert (await api.post(path,data='test')).status == 401,path
            assert (await api.get('/healthz')).status == 200
            assert (await api.get('/ws',headers={'Authorization':'Bearer invalid'})).status == 401
            assert (await api.post('/api/auth/login',headers={'X-NAST-Request':'1','Origin':'https://evil.example'},data={'username':'admin','password':env['NAST_PASSWORD']})).status == 403
            assert (await api.post('/api/auth/login',data={'username':'admin','password':env['NAST_PASSWORD']})).status == 403
            results.append('HTTP paths, bridge rejection and cross-site login protection')
            errors=[]
            for theme in ['light','dark']:
                for width in [390,1440]:
                    context=await browser.new_context(viewport={'width':width,'height':900})
                    page=await context.new_page()
                    page.on('pageerror',lambda e:errors.append(str(e)))
                    await page.goto(url)
                    await page.evaluate('theme=>localStorage.setItem("theme",theme)',theme)
                    await page.reload()
                    await expect(page.get_by_role('heading',name='登录 NAST')).to_be_visible()
                    assert not await socket_status(page,url)
                    await page.get_by_label('用户名',exact=True).fill('admin')
                    await page.get_by_label('密码',exact=True).fill('wrong')
                    await page.get_by_label('密码',exact=True).press('Enter')
                    await expect(page.get_by_role('alert')).to_contain_text('用户名或密码不正确')
                    await expect(page.get_by_label('用户名',exact=True)).to_have_value('admin')
                    assert not await page.evaluate('document.documentElement.scrollWidth > innerWidth+1')
                    await page.screenshot(path=str(artifact/f'login-{theme}-{width}.png'),full_page=True)
                    await page.get_by_label('密码',exact=True).fill(env['NAST_PASSWORD'])
                    await page.get_by_label('密码',exact=True).press('Enter')
                    await expect(page.get_by_role('heading',name='登录 NAST')).not_to_be_visible()
                    assert await socket_status(page,url)
                    cookies=await context.cookies()
                    cookie=next(c for c in cookies if c['name']=='nast_session')
                    assert cookie['httpOnly'] and cookie['sameSite']=='Strict'
                    assert 'nast_session' not in await page.evaluate('document.cookie')
                    await page.reload()
                    await expect(page.get_by_role('heading',name='登录 NAST')).not_to_be_visible()
                    # Open another live session in the same cookie jar; logout must revoke both.
                    sibling=await context.new_page();await sibling.goto(url)
                    await sibling.evaluate('''url => { window.closedByLogout=false; window.probe=new WebSocket(url.replace('http','ws')+'/ws'); window.probe.onclose=()=>window.closedByLogout=true; }''',url)
                    await sibling.wait_for_function('window.probe.readyState===1')
                    if width < 768: await page.get_by_role('button',name='切换侧栏',exact=True).click()
                    await page.get_by_role('button',name='退出登录',exact=True).click()
                    await expect(page.get_by_role('heading',name='登录 NAST')).to_be_visible()
                    await expect(sibling.get_by_role('heading',name='登录 NAST')).to_be_visible()
                    assert (await api.get('/thumbnail',headers={'Cookie':f"nast_session={cookie['value']}"})).status==401
                    await context.close()
            results.append('Login, errors, keyboard submit, refresh and multi-tab logout in four theme/viewports')
            # Native bridges can supply handshake headers; browser WebSocket cannot.
            connection=http.client.HTTPConnection('127.0.0.1',int(env['NAST_PORT']),timeout=5)
            connection.request('GET','/ws',headers={'Authorization':f'Bearer {TOKEN}',
                'Upgrade':'websocket','Connection':'Upgrade','Sec-WebSocket-Version':'13',
                'Sec-WebSocket-Key':'dGhlIHNhbXBsZSBub25jZQ=='})
            assert connection.getresponse().status==101
            connection.close()
            results.append('Bridge bearer token authenticates the real WebSocket upgrade')
            # Cross-site WebSocket uses no session and cannot reach RPC.
            page=await browser.new_page();await page.goto('about:blank')
            assert not await socket_status(page,url)
            assert not errors,errors
            await api.dispose();await browser.close()
        (artifact/'report.json').write_text(json.dumps({'passed':results,'browser_errors':errors},ensure_ascii=False,indent=2),encoding='utf-8')
        print('PASS',len(results),'authentication acceptance groups',flush=True)
    finally:
        process.terminate();process.wait(timeout=10);log.close()

if __name__=='__main__': asyncio.run(main())
