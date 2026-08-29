import { expect, test } from "bun:test";
import { overlayAction } from "./pointer";

test("overlay actions parse from Computer tool calls", () => {
  expect(overlayAction({ id: "1", name: "Computer", args: '{"action":"move","x":100,"y":200}' })).toEqual({
    kind: "move",
    x: 100,
    y: 200,
  });
  expect(overlayAction({ id: "2", name: "Computer", args: '{"action":"scroll","x":5,"y":6,"amount":-3}' })).toEqual({
    kind: "move",
    x: 5,
    y: 6,
  });
  expect(overlayAction({ id: "3", name: "Computer", args: '{"action":"click","x":10,"y":20}' })).toEqual({
    kind: "click",
    x: 10,
    y: 20,
  });
  expect(overlayAction({ id: "4", name: "Computer", args: '{"action":"double_click","button":"left"}' })).toEqual({
    kind: "click",
    x: null,
    y: null,
  });
  expect(overlayAction({ id: "5", name: "Computer", args: '{"action":"type","text":"hello"}' })).toEqual({
    kind: "type",
    text: "hello",
  });
  expect(overlayAction({ id: "6", name: "Computer", args: '{"action":"key","combo":"cmd+c"}' })).toEqual({
    kind: "type",
    text: "cmd+c",
  });
  expect(overlayAction({ id: "7", name: "Computer", args: '{"action":"screenshot"}' })).toBeNull();
  expect(overlayAction({ id: "8", name: "Computer", args: '{"action":"cursor"}' })).toBeNull();
  expect(overlayAction({ id: "9", name: "GetLatestFrame", args: "{}" })).toBeNull();
  expect(overlayAction({ id: "10", name: "Computer", args: "not json" })).toBeNull();
  expect(overlayAction({ id: "11", name: "Computer", args: '{"action":"move","x":"a","y":2}' })).toBeNull();
  expect(overlayAction({ id: "12", name: "Computer", args: '{"action":"scroll","direction":"vertical"}' })).toBeNull();
});
