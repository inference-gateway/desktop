import { expect, test } from "bun:test";
import { isClickAction, overlayAction } from "./pointer";

test("key-cast text parses from Computer type and key calls only", () => {
  expect(overlayAction({ id: "5", name: "Computer", args: '{"action":"type","text":"hello"}' })).toEqual({
    kind: "type",
    text: "hello",
  });
  expect(overlayAction({ id: "6", name: "Computer", args: '{"action":"key","combo":"cmd+c"}' })).toEqual({
    kind: "type",
    text: "cmd+c",
  });
  expect(overlayAction({ id: "1", name: "Computer", args: '{"action":"move","x":100,"y":200}' })).toBeNull();
  expect(overlayAction({ id: "3", name: "Computer", args: '{"action":"click","x":10,"y":20}' })).toBeNull();
  expect(overlayAction({ id: "7", name: "Computer", args: '{"action":"screenshot"}' })).toBeNull();
  expect(overlayAction({ id: "9", name: "GetLatestFrame", args: "{}" })).toBeNull();
  expect(overlayAction({ id: "10", name: "Computer", args: "not json" })).toBeNull();
});

test("click actions ripple, moves and keys do not", () => {
  expect(isClickAction("click")).toBe(true);
  expect(isClickAction("double_click")).toBe(true);
  expect(isClickAction("triple_click")).toBe(true);
  expect(isClickAction("move")).toBe(false);
  expect(isClickAction("type")).toBe(false);
});
