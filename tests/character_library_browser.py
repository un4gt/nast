"""Exercise character-load failures in the production UI without changing data.

Install the Playwright dependency from tests/parity/requirements.txt first.
Run against a built nast instance; --live also checks its real library read-only.
"""
import argparse
import asyncio
import json
from pathlib import Path

from playwright.async_api import async_playwright, expect


async def check_mock(browser, base_url, output, scenario, characters):
    page = await browser.new_page(viewport={"width": 1440, "height": 900})
    errors = []
    page.on("pageerror", lambda error: errors.append(str(error)))

    def mock_socket(socket):
        def reply(raw):
            request = json.loads(raw)
            results = {
                "characters.all": characters,
                "settings.get": {},
                "groups.all": [],
                "generate.status": {"running": False},
                "characters.chats": [],
                "characters.get": {"name": "可用角色", "data": {"name": "可用角色"}},
                "worlds.list": [],
            }
            socket.send(json.dumps({"id": request["id"], "result": results.get(request["method"], {})}))
        socket.on_message(reply)

    await page.route_web_socket("**/ws", mock_socket)
    await page.route("**/thumbnail?*", lambda route: route.fulfill(status=404, body=""))
    try:
        await page.goto(base_url)
        await expect(page.get_by_role("button", name="连接与设置", exact=True)).to_be_visible()
        if characters:
            failed = page.locator('[data-sidebar="menu-button"]').filter(has_text="受损角色")
            await expect(failed).to_be_disabled()
            await expect(failed).to_contain_text("读取失败")
            await page.get_by_role("textbox", name="搜索角色或标签").fill("受损")
            await expect(failed).to_be_visible()
            await page.get_by_role("button", name="清空搜索", exact=True).click()
        await page.screenshot(path=str(output / f"{scenario}.png"), full_page=True)
        if scenario == "mixed":
            healthy = page.locator('[data-sidebar="menu-button"]').filter(has_text="可用角色")
            await expect(healthy).to_be_enabled()
            await healthy.click()
            await expect(page.locator("h1")).to_have_text("可用角色")
            await expect(page.get_by_role("textbox", name="输入消息")).to_be_visible()
        assert not errors, errors
        return {"scenario": scenario, "passed": True, "page_errors": errors}
    except Exception:
        print(json.dumps({"scenario": scenario, "page_errors": errors}, ensure_ascii=False))
        await page.screenshot(path=str(output / f"{scenario}-failed.png"), full_page=True)
        raise
    finally:
        await page.close()


async def check_live(browser, base_url):
    page = await browser.new_page(viewport={"width": 1440, "height": 900})
    errors = []
    page.on("pageerror", lambda error: errors.append(str(error)))
    try:
        await page.goto(base_url)
        characters = await page.evaluate("""() => new Promise((resolve, reject) => {
            const ws = new WebSocket(location.origin.replace(/^http/, 'ws') + '/ws');
            const timer = setTimeout(() => { ws.close(); reject(new Error('RPC timeout')); }, 10000);
            ws.onopen = () => ws.send(JSON.stringify({id:'read-only-check',method:'characters.all',params:{}}));
            ws.onmessage = ({data}) => {
                const reply = JSON.parse(data);
                if (reply.id !== 'read-only-check') return;
                clearTimeout(timer); ws.close();
                reply.error ? reject(new Error(reply.error.message)) : resolve(reply.result);
            };
        })""")
        failures = [entry.get("error") for entry in characters if entry.get("error")]
        assert not failures, failures
        assert all(isinstance(entry.get("name"), str) and isinstance(entry.get("tags"), list) for entry in characters)
        for entry in characters:
            await expect(page.locator('[data-sidebar="menu-button"]').filter(has_text=entry["name"]).first).to_be_enabled()
        await expect(page.get_by_role("button", name="连接与设置", exact=True)).to_be_visible()
        assert not errors, errors
        return {"scenario": "live-read-only", "passed": True, "readable_characters": len(characters), "page_errors": errors}
    finally:
        await page.close()


async def main(args):
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    failed = {"avatar": "受损角色.png", "error": "json error: duplicate field `selectiveLogic`"}
    healthy = {"avatar": "healthy.png", "name": "可用角色", "tags": ["城堡"]}
    async with async_playwright() as playwright:
        browser = await playwright.chromium.launch()
        try:
            report = []
            for scenario, characters in [("mixed", [healthy, failed]), ("only-error", [failed]), ("empty", [])]:
                report.append(await check_mock(browser, args.base_url, output, scenario, characters))
            if args.live:
                report.append(await check_live(browser, args.base_url))
            print(json.dumps(report, ensure_ascii=False, indent=2))
        finally:
            await browser.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", default="http://127.0.0.1:8000")
    parser.add_argument("--output", default=".cache/character-library-browser")
    parser.add_argument("--live", action="store_true")
    asyncio.run(main(parser.parse_args()))
