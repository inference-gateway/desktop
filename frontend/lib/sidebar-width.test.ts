import { expect, test } from "bun:test";
import { clampSidebarWidth, DEFAULT_SIDEBAR_WIDTH, loadSidebarWidth, MIN_SIDEBAR_WIDTH } from "./sidebar-width";

test("sidebar width clamps to min, 50% window max, and defaults to 320", () => {
  expect(DEFAULT_SIDEBAR_WIDTH).toBe(320);
  expect(clampSidebarWidth(100, 1440)).toBe(MIN_SIDEBAR_WIDTH);
  expect(clampSidebarWidth(5000, 1440)).toBe(720);
  expect(clampSidebarWidth(500, 1440)).toBe(500);
  expect(clampSidebarWidth(401.7, 1440)).toBe(402);
  expect(clampSidebarWidth(200, 300)).toBe(MIN_SIDEBAR_WIDTH);
  expect(clampSidebarWidth(500, 300)).toBe(MIN_SIDEBAR_WIDTH);
});

test("loadSidebarWidth reads its own key and falls back per panel", () => {
  const store = new Map<string, string>();
  globalThis.localStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
  } as unknown as Storage;
  expect(loadSidebarWidth(1440, "chatDockWidth", 400)).toBe(400);
  store.set("chatDockWidth", "500");
  expect(loadSidebarWidth(1440, "chatDockWidth", 400)).toBe(500);
  expect(loadSidebarWidth(1440)).toBe(DEFAULT_SIDEBAR_WIDTH);
});
