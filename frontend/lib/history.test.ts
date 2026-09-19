import { describe, expect, test } from "bun:test";
import { createHistory } from "./history";

describe("createHistory", () => {
  test("undo walks back through pushes, redo walks forward again", () => {
    const h = createHistory<string>();
    h.push("a");
    h.push("b");
    expect(h.undo("c")).toBe("b");
    expect(h.undo("b")).toBe("a");
    expect(h.undo("a")).toBeUndefined();
    expect(h.redo("a")).toBe("b");
    expect(h.redo("b")).toBe("c");
    expect(h.redo("c")).toBeUndefined();
  });

  test("a push after an undo drops the redo branch", () => {
    const h = createHistory<string>();
    h.push("a");
    expect(h.undo("b")).toBe("a");
    h.push("a");
    expect(h.redo("a")).toBeUndefined();
    expect(h.undo("x")).toBe("a");
  });

  test("the past is bounded to the limit, oldest dropped first", () => {
    const h = createHistory<number>(3);
    for (let i = 1; i <= 5; i++) h.push(i);
    expect(h.undo(6)).toBe(5);
    expect(h.undo(5)).toBe(4);
    expect(h.undo(4)).toBe(3);
    expect(h.undo(3)).toBeUndefined();
  });

  test("reset clears both stacks", () => {
    const h = createHistory<number>();
    h.push(1);
    expect(h.undo(2)).toBe(1);
    h.reset();
    expect(h.undo(1)).toBeUndefined();
    expect(h.redo(1)).toBeUndefined();
  });
});
