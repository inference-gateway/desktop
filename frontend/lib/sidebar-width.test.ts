import { expect, test } from "bun:test";
import { clampSidebarWidth, DEFAULT_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH } from "./sidebar-width";

test("sidebar width clamps to min, 50% window max, and defaults to 320", () => {
  expect(DEFAULT_SIDEBAR_WIDTH).toBe(320); // double-click reset target
  expect(clampSidebarWidth(100, 1440)).toBe(MIN_SIDEBAR_WIDTH); // never below min
  expect(clampSidebarWidth(5000, 1440)).toBe(720); // never above half the window
  expect(clampSidebarWidth(500, 1440)).toBe(500); // in-range values pass through
  expect(clampSidebarWidth(401.7, 1440)).toBe(402); // fractional drags round
  // Tiny window: max would be < min, so min still wins (clamp stays valid).
  expect(clampSidebarWidth(200, 300)).toBe(MIN_SIDEBAR_WIDTH);
  expect(clampSidebarWidth(500, 300)).toBe(MIN_SIDEBAR_WIDTH);
});
