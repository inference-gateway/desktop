import { expect, test } from "bun:test";
import { formatElapsed } from "./elapsed";

test("formatElapsed shows seconds, then minutes, then hours", () => {
  expect(formatElapsed(0)).toBe("0s");
  expect(formatElapsed(8_900)).toBe("8s");
  expect(formatElapsed(72_000)).toBe("1m 12s");
  expect(formatElapsed(3_780_000)).toBe("1h 03m");
  expect(formatElapsed(-5)).toBe("0s");
});
