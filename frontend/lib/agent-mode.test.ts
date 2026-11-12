import { expect, test } from "bun:test";
import { modeAfterPlan, nextMode, parseMode } from "./agent-mode";

test("modes cycle in the CLI's Shift+Tab order and wrap", () => {
  expect(nextMode("standard")).toBe("plan");
  expect(nextMode("plan")).toBe("auto");
  expect(nextMode("auto")).toBe("auto-with-judge");
  expect(nextMode("auto-with-judge")).toBe("standard");
});

test("a missing or unknown stored mode falls back to standard", () => {
  expect(parseMode("plan")).toBe("plan");
  expect(parseMode(null)).toBe("standard");
  expect(parseMode("readonly")).toBe("standard");
});

test("an accepted plan runs in auto or standard, a rejected one keeps planning", () => {
  expect(modeAfterPlan("accept")).toBe("auto");
  expect(modeAfterPlan("accept_standard")).toBe("standard");
  expect(modeAfterPlan("reject")).toBeNull();
});
