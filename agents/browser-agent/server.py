#!/usr/bin/env python3
"""A2A agent server wrapping browser-use for browser automation.

Provides an HTTP API that the infer-agent calls via A2A tool delegation.
Skills: browser_navigate, browser_click, browser_fill, browser_extract_text,
        browser_screenshot, browser_scroll.
"""

import asyncio
import json
import os
import tempfile
import traceback
from pathlib import Path

from aiohttp import web

from browser_use import Agent as BrowserAgent
from browser_use.browser.browser import Browser, BrowserConfig
from browser_use.browser.context import BrowserContext, BrowserContextConfig

# ---------------------------------------------------------------------------
# Browser lifecycle
# ---------------------------------------------------------------------------

_browser: Browser | None = None
_context: BrowserContext | None = None


async def get_browser() -> Browser:
    global _browser
    if _browser is None:
        headless = os.environ.get("HEADLESS", "true").lower() in ("1", "true", "yes")
        _browser = Browser(
            config=BrowserConfig(
                headless=headless,
                disable_security=True,
            )
        )
    return _browser


async def get_context() -> BrowserContext:
    global _context
    if _context is None:
        b = await get_browser()
        _context = await b.new_context(
            config=BrowserContextConfig(
                browser_window_size={"width": 1280, "height": 720},
            )
        )
    return _context


async def close_browser():
    global _browser, _context
    if _context:
        await _context.close()
        _context = None
    if _browser:
        await _browser.close()
        _browser = None


# ---------------------------------------------------------------------------
# Helper: run a single action via browser-use and return structured result
# ---------------------------------------------------------------------------

def _infer_dir() -> Path:
    """Return ~/.infer (or the INFER_DIR override)."""
    return Path(os.environ.get("INFER_DIR", Path.home() / ".infer"))


def _screenshot_path() -> str:
    """Return a path under ~/.infer/tmp/ for a screenshot file."""
    tmp = _infer_dir() / "tmp"
    tmp.mkdir(parents=True, exist_ok=True)
    fd, path = tempfile.mkstemp(suffix=".png", dir=str(tmp))
    os.close(fd)
    return path


async def _run_browser_task(task: str) -> dict:
    """Execute a natural-language browser task via browser-use and return structured output."""
    ctx = await get_context()
    agent = BrowserAgent(
        task=task,
        browser_context=ctx,
        use_vision=False,
        generate_gif=False,
    )
    history = await agent.run(max_steps=30)

    result = {"success": True, "data": {}, "steps": len(history)}

    # Collect output from the final step
    if history:
        last = history[-1]
        if hasattr(last, "result") and last.result:
            result["data"]["output"] = str(last.result)
        if hasattr(last, "url") and last.url:
            result["data"]["url"] = last.url

    return result


async def _do_navigate(url: str) -> dict:
    ctx = await get_context()
    page = await ctx.get_current_page()
    await page.goto(url, wait_until="domcontentloaded")
    current_url = page.url
    title = await page.title()
    return {
        "success": True,
        "data": {"url": current_url, "title": title},
    }


async def _do_click(selector: str) -> dict:
    ctx = await get_context()
    page = await ctx.get_current_page()
    el = await page.query_selector(selector)
    if not el:
        return {"success": False, "error": f"Element not found: {selector}"}
    await el.click()
    return {"success": True, "data": {"selector": selector, "clicked": True}}


async def _do_fill(selector: str, value: str) -> dict:
    ctx = await get_context()
    page = await ctx.get_current_page()
    el = await page.query_selector(selector)
    if not el:
        return {"success": False, "error": f"Input not found: {selector}"}
    await el.fill(value)
    return {"success": True, "data": {"selector": selector, "filled": True}}


async def _do_extract_text(selector: str | None = None) -> dict:
    ctx = await get_context()
    page = await ctx.get_current_page()
    if selector:
        el = await page.query_selector(selector)
        if not el:
            return {"success": False, "error": f"Element not found: {selector}"}
        text = await el.inner_text()
    else:
        text = await page.inner_text("body")
    return {"success": True, "data": {"text": text.strip()[:10000]}}


async def _do_screenshot(selector: str | None = None, full_page: bool = False) -> dict:
    ctx = await get_context()
    page = await ctx.get_current_page()
    path = _screenshot_path()

    if selector:
        el = await page.query_selector(selector)
        if not el:
            return {"success": False, "error": f"Element not found: {selector}"}
        await el.screenshot(path=path)
    else:
        await page.screenshot(path=path, full_page=full_page)

    return {
        "success": True,
        "data": {"path": path, "filename": Path(path).name},
    }


async def _do_scroll(direction: str, amount: int = 500) -> dict:
    ctx = await get_context()
    page = await ctx.get_current_page()
    dx, dy = 0, 0
    match direction:
        case "down":
            dy = amount
        case "up":
            dy = -amount
        case "right":
            dx = amount
        case "left":
            dx = -amount
        case _:
            return {"success": False, "error": f"Unknown direction: {direction}"}

    await page.evaluate(f"window.scrollBy({dx}, {dy})")
    return {"success": True, "data": {"direction": direction, "amount": amount}}


# ---------------------------------------------------------------------------
# A2A HTTP handlers
# ---------------------------------------------------------------------------

async def handle_health(request: web.Request) -> web.Response:
    return web.json_response({"status": "ok"})


async def handle_agent_card(request: web.Request) -> web.Response:
    """Return the agent card so the infer agent discovers our tools."""
    card = {
        "name": "browser-agent",
        "description": "Browser automation via browser-use. Navigate, click, fill, extract text, screenshot.",
        "version": "1.0.0",
        "skills": [
            {
                "name": "browser_navigate",
                "description": "Navigate to a URL",
                "args": {"url": {"type": "string", "description": "The URL", "required": True}},
            },
            {
                "name": "browser_click",
                "description": "Click an element identified by CSS selector",
                "args": {"selector": {"type": "string", "description": "CSS selector", "required": True}},
            },
            {
                "name": "browser_fill",
                "description": "Fill an input field with text",
                "args": {
                    "selector": {"type": "string", "description": "CSS selector", "required": True},
                    "value": {"type": "string", "description": "Text to type", "required": True},
                },
            },
            {
                "name": "browser_extract_text",
                "description": "Extract visible text from the page or an element",
                "args": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector (optional, defaults to body)",
                        "required": False,
                    }
                },
            },
            {
                "name": "browser_screenshot",
                "description": "Take a screenshot of the page or an element",
                "args": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector (optional, defaults to viewport)",
                        "required": False,
                    },
                    "full_page": {
                        "type": "boolean",
                        "description": "Capture full scrollable page",
                        "required": False,
                    },
                },
            },
            {
                "name": "browser_scroll",
                "description": "Scroll the page in a direction",
                "args": {
                    "direction": {
                        "type": "string",
                        "description": "down, up, left, or right",
                        "required": True,
                    },
                    "amount": {
                        "type": "integer",
                        "description": "Pixels to scroll (default 500)",
                        "required": False,
                    },
                },
            },
        ],
    }
    return web.json_response(card)


async def handle_tool_call(request: web.Request) -> web.Response:
    """Execute a browser tool and return the structured result for A2A."""
    try:
        body = await request.json()
    except Exception:
        return web.json_response({"success": False, "error": "invalid JSON"}, status=400)

    tool_name = body.get("tool_name", "")
    args = body.get("arguments", {})
    tool_call_id = body.get("tool_call_id", "")

    try:
        match tool_name:
            case "browser_navigate":
                result = await _do_navigate(args["url"])
            case "browser_click":
                result = await _do_click(args["selector"])
            case "browser_fill":
                result = await _do_fill(args["selector"], args["value"])
            case "browser_extract_text":
                result = await _do_extract_text(args.get("selector"))
            case "browser_screenshot":
                result = await _do_screenshot(args.get("selector"), args.get("full_page", False))
            case "browser_scroll":
                result = await _do_scroll(args.get("direction", "down"), args.get("amount", 500))
            case _:
                return web.json_response(
                    {"success": False, "error": f"Unknown tool: {tool_name}"}, status=400
                )
    except Exception as e:
        traceback.print_exc()
        result = {"success": False, "error": str(e)}

    response = {
        "tool_call_id": tool_call_id,
        "tool_name": tool_name,
        "success": result.get("success", False),
        "data": result.get("data", {}),
    }
    if "error" in result:
        response["error"] = result["error"]

    return web.json_response(response)


# ---------------------------------------------------------------------------
# App
# ---------------------------------------------------------------------------

async def create_app() -> web.Application:
    app = web.Application()
    app.router.add_get("/health", handle_health)
    app.router.add_get("/agent.json", handle_agent_card)
    app.router.add_post("/tool_call", handle_tool_call)

    # Clean up on shutdown
    app.on_shutdown.append(lambda _: close_browser())
    return app


def main():
    port = int(os.environ.get("BROWSER_AGENT_PORT", "8083"))
    web.run_app(create_app(), host="0.0.0.0", port=port)


if __name__ == "__main__":
    main()
