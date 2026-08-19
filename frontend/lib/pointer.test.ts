import { expect, test } from "bun:test";
import { overlayAction } from "./pointer";

test("overlay actions parse from computer-use tool calls", () => {
  expect(overlayAction({ id: "1", name: "MouseMove", args: '{"x":100,"y":200}' })).toEqual({
    kind: "move",
    x: 100,
    y: 200,
  });
  expect(overlayAction({ id: "2", name: "MouseScroll", args: '{"x":5,"y":6,"dy":-3}' })).toEqual({
    kind: "move",
    x: 5,
    y: 6,
  });
  expect(overlayAction({ id: "3", name: "MouseClick", args: '{"x":10,"y":20}' })).toEqual({
    kind: "click",
    x: 10,
    y: 20,
  });
  expect(overlayAction({ id: "4", name: "MouseClick", args: '{"button":"left"}' })).toEqual({
    kind: "click",
    x: null,
    y: null,
  });
  expect(overlayAction({ id: "5", name: "KeyboardType", args: '{"text":"hello"}' })).toEqual({
    kind: "type",
    text: "hello",
  });
  expect(overlayAction({ id: "6", name: "KeyboardType", args: '{"key_combo":"cmd+c"}' })).toEqual({
    kind: "type",
    text: "cmd+c",
  });
  expect(overlayAction({ id: "7", name: "GetFocusedApp", args: "{}" })).toBeNull();
  expect(overlayAction({ id: "8", name: "MouseMove", args: "not json" })).toBeNull();
  expect(overlayAction({ id: "9", name: "MouseMove", args: '{"x":"a","y":2}' })).toBeNull();
});
