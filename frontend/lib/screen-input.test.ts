import { expect, test } from "bun:test";
import { framePoint, isUnsupportedInput, keyInput, latestOnly, mouseButton, wheelInputs } from "./screen-input";

const key = (key: string, modifiers: Partial<Record<"ctrlKey" | "metaKey" | "altKey" | "shiftKey", boolean>> = {}) => ({
  key,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  ...modifiers,
});

test("a pointer maps from the displayed image onto the frame and stays inside it", () => {
  const image = { left: 100, top: 50, width: 640, height: 400 };
  const frame = { width: 1280, height: 800 };
  expect(framePoint({ x: 420, y: 250 }, image, frame)).toEqual({ x: 640, y: 400 });
  expect(framePoint({ x: 0, y: 9999 }, image, frame)).toEqual({ x: 0, y: 800 });
  expect(framePoint({ x: 420, y: 250 }, image, { width: 0, height: 0 })).toBeNull();
});

test("mouse buttons keep their side", () => {
  expect([0, 1, 2].map(mouseButton)).toEqual(["left", "middle", "right"]);
});

test("plain text types and special keys tap", () => {
  expect(keyInput(key("a"))).toEqual({ kind: "type", text: "a" });
  expect(keyInput(key("A", { shiftKey: true }))).toEqual({ kind: "type", text: "A" });
  expect(keyInput(key("Enter"))).toEqual({ kind: "key", combo: "enter" });
  expect(keyInput(key("ArrowLeft"))).toEqual({ kind: "key", combo: "left" });
  expect(keyInput(key("F5"))).toEqual({ kind: "key", combo: "f5" });
  expect(keyInput(key("Shift", { shiftKey: true }))).toBeNull();
});

test("a modifier chord taps as one combo", () => {
  expect(keyInput(key("l", { ctrlKey: true }))).toEqual({ kind: "key", combo: "ctrl+l" });
  expect(keyInput(key("T", { ctrlKey: true, shiftKey: true }))).toEqual({ kind: "key", combo: "ctrl+shift+t" });
  expect(keyInput(key("Tab", { altKey: true }))).toEqual({ kind: "key", combo: "alt+tab" });
  expect(keyInput(key("Control", { ctrlKey: true }))).toBeNull();
});

test("a wheel event scrolls at least one click and at most ten per axis", () => {
  expect(wheelInputs(0, 30)).toEqual([{ kind: "scroll", direction: "vertical", amount: 1 }]);
  expect(wheelInputs(-250, 0)).toEqual([{ kind: "scroll", direction: "horizontal", amount: -3 }]);
  expect(wheelInputs(0, 99999)).toEqual([{ kind: "scroll", direction: "vertical", amount: 10 }]);
  expect(wheelInputs(0, 0)).toEqual([]);
});

test("latestOnly keeps the newest value and tells when the slot was empty", () => {
  const moves = latestOnly<number>();
  expect(moves.take()).toBeNull();
  expect(moves.put(1)).toBe(true);
  expect(moves.put(2)).toBe(false);
  expect(moves.put(3)).toBe(false);
  expect(moves.take()).toBe(3);
  expect(moves.take()).toBeNull();
  expect(moves.put(4)).toBe(true);
});

test("isUnsupportedInput recognises an older CLI refusing an input kind", () => {
  expect(isUnsupportedInput('unsupported screen input "mouse_down"')).toBe(true);
  expect(isUnsupportedInput(new Error('unsupported screen input "mouse_up"'))).toBe(true);
  expect(isUnsupportedInput("the screen is not held by the person")).toBe(false);
});
