import { expect, test } from "bun:test";
import { clampSidebarWidth, DEFAULT_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH } from "./sidebar-width";

test("sidebar width clamps to min, 50% window max, and defaults to 320", () => {
  expect(DEFAULT_SIDEBAR_WIDTH).toBe(320);
  expect(clampSidebarWidth(100, 1440)).toBe(MIN_SIDEBAR_WIDTH);
  expect(clampSidebarWidth(5000, 1440)).toBe(720);
  expect(clampSidebarWidth(500, 1440)).toBe(500);
  expect(clampSidebarWidth(401.7, 1440)).toBe(402);
  expect(clampSidebarWidth(200, 300)).toBe(MIN_SIDEBAR_WIDTH);
  expect(clampSidebarWidth(500, 300)).toBe(MIN_SIDEBAR_WIDTH);
});
