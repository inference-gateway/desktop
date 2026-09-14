import { expect, test } from "bun:test";
import { enqueue, takeAll, takeFirst } from "./queue";

test("enqueue while running accumulates prompts for the session", () => {
  let queue = enqueue({}, "s1", "first");
  queue = enqueue(queue, "s1", "second");
  expect(queue).toEqual({ s1: ["first", "second"] });
  expect(takeFirst(queue, "s1")).toEqual({ queue: { s1: ["second"] }, text: "first" });
  expect(takeAll(queue, "s1").text).toBe("first\nsecond");
});

test("flush on Done sends the first queued prompt and keeps the rest", () => {
  const first = takeFirst(enqueue(enqueue({}, "s1", "a"), "s1", "b"), "s1");
  expect(first.text).toBe("a");
  const second = takeFirst(first.queue, "s1");
  expect(second.text).toBe("b");
  expect(second.queue).toEqual({});
});

test("flush on Cancelled sends the queued prompt the same way", () => {
  const { queue, text } = takeFirst(enqueue({}, "s1", "after cancel"), "s1");
  expect(text).toBe("after cancel");
  expect(queue).toEqual({});
  expect(takeFirst(queue, "s1").text).toBeNull();
});

test("discard removes everything so nothing is sent when the turn ends", () => {
  const { queue, text } = takeAll(enqueue(enqueue({}, "s1", "a"), "s1", "b"), "s1");
  expect(text).toBe("a\nb");
  expect(queue).toEqual({});
  expect(takeFirst(queue, "s1").text).toBeNull();
});

test("queues are per session: one session's flush never touches another", () => {
  const queue = enqueue(enqueue({}, "a", "for a"), "b", "for b");
  const { queue: next, text } = takeFirst(queue, "a");
  expect(text).toBe("for a");
  expect(next).toEqual({ b: ["for b"] });
  expect(takeFirst(queue, "b").text).toBe("for b");
  expect(takeFirst(queue, "c").text).toBeNull();
});
