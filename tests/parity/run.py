"""Run both applications against a local deterministic model and retain evidence."""
from __future__ import annotations

import argparse
import asyncio
import base64
import difflib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request

from playwright.async_api import async_playwright
from model_server import ModelServer, canonical

ROOT = Path(__file__).resolve().parents[2]
ST = ROOT / "refrence" / "SillyTavern"


def port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")


async def wait_http(url, process):
    for _ in range(300):
        if process.poll() is not None:
            raise RuntimeError(f"Application exited ({process.returncode}); inspect server log")
        try:
            await asyncio.to_thread(urllib.request.urlopen, url, timeout=1)
            return
        except (OSError, TimeoutError):
            await asyncio.sleep(0.2)
    raise TimeoutError(url)


async def rpc(page, method, params=None):
    return await page.evaluate("""async ({method,params}) => {
        if (!window.parityRpc) {
            const ws = new WebSocket(location.origin.replace(/^http/, 'ws') + '/ws');
            await new Promise((resolve,reject) => {ws.onopen=resolve;ws.onerror=reject;});
            const pending=new Map(); let serial=0;
            ws.onmessage=event=>{const reply=JSON.parse(event.data);const p=pending.get(reply.id);
                if(!p)return;pending.delete(reply.id);clearTimeout(p.timer);
                reply.error?p.reject(new Error(reply.error.message)):p.resolve(reply.result);};
            window.parityRpc=(method,params)=>new Promise((resolve,reject)=>{
                const id='parity-'+(++serial);
                const timer=setTimeout(()=>{pending.delete(id);reject(new Error('RPC timeout '+method));},90000);
                pending.set(id,{resolve,reject,timer});ws.send(JSON.stringify({id,method,params}));});
        }
        return window.parityRpc(method,params);
    }""", {"method": method, "params": params or {}})


def card(name="ParityActor"):
    return {"spec": "chara_card_v3", "spec_version": "3.0", "data": {
        "name": name, "description": "角色设定：守护城堡。", "personality": "谨慎。",
        "scenario": "在城堡门前。", "first_mes": "欢迎来到城堡。", "mes_example": "",
        "system_prompt": "", "post_history_instructions": "", "alternate_greetings": [],
        "group_only_greetings": [], "extensions": {"talkativeness": 0.7}}}


async def initialize_st(page, base_url, model_url):
    await page.goto(base_url)
    await page.wait_for_function("async () => { try { const m=await import('/scripts/st-context.js'); return !!m.getContext().chatCompletionSettings; } catch { return false; }}", timeout=90000)
    await page.wait_for_function("document.getElementById('preloader') === null", timeout=90000)
    return await page.evaluate("""async modelUrl=>{
        const script=await import('/script.js');
        const {getContext}=await import('/scripts/st-context.js');
        const ctx=getContext();
        script.changeMainAPI('openai');
        Object.assign(ctx.chatCompletionSettings, {chat_completion_source:'custom',custom_url:modelUrl,
            custom_model:'gpt-4o',stream_openai:false,openai_max_context:8192,openai_max_tokens:256,
            temp_openai:0.7,top_p_openai:1,freq_pen_openai:0,pres_pen_openai:0,
            custom_prompt_post_processing:'none',show_thoughts:true});
        script.setOnlineStatus('gpt-4o');
        ctx.saveSettingsDebounced();
        return JSON.parse(JSON.stringify(ctx.chatCompletionSettings));
    }""", model_url)


async def st_import(page, payload, filename):
    token = await page.request.get("/csrf-token")
    headers = {"X-CSRF-Token": (await token.json())["token"]}
    response = await page.request.post("/api/characters/import", headers=headers, multipart={
        "avatar": {"name": filename, "mimeType": "application/octet-stream", "buffer": payload},
        "file_type": filename.rsplit(".", 1)[-1], "user_name": "User"})
    if not response.ok:
        raise RuntimeError(f"ST import {response.status}: {await response.text()}")
    result = await response.json()
    filename = result["file_name"]
    return filename if filename.endswith(".png") else filename + ".png"


async def st_generate(page, avatar, user_message):
    return await page.evaluate("""async ({avatar,message})=>{
        const {getContext}=await import('/scripts/st-context.js');
        await getContext().getCharacters();
        const index=getContext().characters.findIndex(c=>c.avatar===avatar);
        if(index<0)throw new Error('Imported character not found '+avatar);
        await getContext().selectCharacterById(index);
        const textarea=document.getElementById('send_textarea');
        textarea.value=message;textarea.dispatchEvent(new Event('input',{bubbles:true}));
        await getContext().generate('normal');
        await getContext().saveChat();
        return {chat:JSON.parse(JSON.stringify(getContext().chat)),
            metadata:JSON.parse(JSON.stringify(getContext().chatMetadata))};
    }""", {"avatar": avatar, "message": user_message})


async def execute(args):
    artifact = Path(args.output).resolve() if args.output else Path(tempfile.mkdtemp(prefix="nast-st-parity-"))
    artifact.mkdir(parents=True, exist_ok=True)
    print(f"Evidence: {artifact}", flush=True)
    binary = ROOT / "target" / "debug" / ("nast.exe" if os.name == "nt" else "nast")
    if not args.skip_build:
        subprocess.run(["cargo", "build", "--bin", "nast"], cwd=ROOT, check=True)
        subprocess.run(["npm.cmd" if os.name == "nt" else "npm", "run", "build"], cwd=ROOT / "web", check=True)
    if not (ST / "node_modules").exists():
        raise RuntimeError("Install reference dependencies with npm ci in refrence/SillyTavern")
    server = ModelServer(output=artifact / "requests.jsonl")
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    model_url = f"http://127.0.0.1:{server.server_port}"
    nast_port, st_port = port(), port()
    nast_data, st_data = artifact / "nast-data", artifact / "st-data"
    st_config = artifact / "st-config.yaml"
    shutil.copyfile(ST / "default" / "config.yaml", st_config)
    processes, logs = [], []
    report = {"stage": args.stage, "reference": subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ST, text=True).strip(), "cases": []}
    try:
        for app, cmd, cwd, env in [
            ("st", ["node", "server.js", "--configPath", str(st_config), "--dataRoot", str(st_data),
                "--port", str(st_port), "--browserLaunchEnabled", "false"], ST, os.environ.copy()),
            ("nast", [str(binary)], artifact, {**os.environ, "NAST_PORT": str(nast_port),
                "NAST_DATA": str(nast_data), "NAST_WEB": str(ROOT / "web" / "dist"),
                "NAST_BIND": "127.0.0.1", 'NAST_USERNAME':'', 'NAST_PASSWORD':'', 'NAST_BRIDGE_TOKEN':'', 'NAST_PUBLIC_ORIGIN':'', 'NAST_ALLOW_ANONYMOUS':'true', "OPENAI_API_KEY": "", "NAST_OPENAI_BASE": ""}),
        ]:
            log = (artifact / f"{app}.log").open("w", encoding="utf-8")
            logs.append(log)
            processes.append(subprocess.Popen(cmd, cwd=cwd, env=env, stdout=log, stderr=log))
        await asyncio.gather(wait_http(f"http://127.0.0.1:{st_port}", processes[0]),
                             wait_http(f"http://127.0.0.1:{nast_port}", processes[1]))
        async with async_playwright() as playwright:
            browser = await playwright.chromium.launch()
            try:
                st_context = await browser.new_context(base_url=f"http://127.0.0.1:{st_port}")
                nast_context = await browser.new_context(base_url=f"http://127.0.0.1:{nast_port}")
                st_page, nast_page = await st_context.new_page(), await nast_context.new_page()
                errors = []
                st_page.on("pageerror", lambda error: errors.append({"app": "st", "error": str(error)}))
                nast_page.on("pageerror", lambda error: errors.append({"app": "nast", "error": str(error)}))
                oai = await initialize_st(st_page, f"http://127.0.0.1:{st_port}", model_url + "/st/v1")
                await nast_page.goto(f"http://127.0.0.1:{nast_port}")
                settings = {"oai_settings": {**oai, "custom_url": model_url + "/nast/v1"},
                            "power_user": {"username": "User"}}
                catalog = await rpc(nast_page, 'model_catalog.get')
                route = catalog['models'][0]['routes'][0]
                route['config']['endpoint'] = model_url + '/nast/v1'
                route['config']['context_limit'] = None
                route['config']['output_limit'] = None
                route['upstream_model'] = oai.get('custom_model') or oai.get('openai_model', 'gpt-4o')
                await rpc(nast_page, 'model_catalog.save', {'catalog':catalog})
                from cases import run_cases
                await run_cases(st_page, nast_page, server, artifact, report, settings, args.stage)
                report["browser_errors"] = errors
                report["cases"].append({"name": "browser/no-uncaught-errors", "passed": not errors})
                await st_page.screenshot(path=str(artifact / "st.png"), full_page=True)
                await nast_page.screenshot(path=str(artifact / "nast.png"), full_page=True)
            finally:
                await browser.close()
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        for process in processes:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
        for log in logs:
            log.close()
        server.shutdown()
        server.server_close()
        write_json(artifact / "report.json", report)
    return 0 if report["cases"] and all(case["passed"] for case in report["cases"]) else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage", choices=["cards", "embedded", "worlds", "macros", "groups", "management", "ui", "all"], default="all")
    parser.add_argument("--output")
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()
    raise SystemExit(asyncio.run(execute(args)))
