import { expect, test } from "bun:test";
import { formatInput, recordingFrame } from "./recording";

// Retina MacBook: 2880x1800 physical at scale 2 = 1440x900 CSS; the overlay
// starts below a 25pt menu bar. The CLI frame for that screen is 1024x640.
const screen = { width: 1440, height: 875, dy: 25 };

test("a region maps from frame space to CSS pixels with the frame just outside it", () => {
  const area = { x: 100, y: 100, width: 400, height: 300, frame_width: 1024, frame_height: 640 };
  const s = 1440 / 1024;
  expect(recordingFrame(area, screen, 3)).toEqual({
    left: 100 * s - 3,
    top: 100 * s - 25 - 3,
    width: 400 * s + 6,
    height: 300 * s + 6,
  });
});

test("a full-screen recording clamps the frame inside the screen edges", () => {
  const area = { x: 0, y: 0, width: 1024, height: 640, frame_width: 1024, frame_height: 640 };
  expect(recordingFrame(area, screen, 3)).toEqual({ left: 4, top: 4, width: 1432, height: 867 });
});

test("input events format with the time since the recording started", () => {
  expect(formatInput({ kind: "click", t: 12.43, button: "left", x: 640, y: 380 })).toBe(
    "00:12.4 click left (640, 380)",
  );
  expect(formatInput({ kind: "key", t: 75.1, keys: "cmd+s", text: "s" })).toBe("01:15.1 key cmd+s");
  expect(formatInput({ kind: "key", t: 59.96, keys: "a", text: "a" })).toBe("01:00.0 key a");
  expect(formatInput({ kind: "key", t: 3, keys: "", text: "" })).toBeNull();
});
