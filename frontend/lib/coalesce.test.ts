import { expect, test } from "bun:test";
import { coalesce } from "./coalesce";

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

test("calls during a run collapse into one more run after it", async () => {
  const runs: ReturnType<typeof deferred>[] = [];
  const refresh = coalesce(() => {
    const d = deferred();
    runs.push(d);
    return d.promise;
  });

  refresh();
  refresh();
  refresh();
  expect(runs.length).toBe(1);

  runs[0].resolve();
  await runs[0].promise;
  await Promise.resolve();
  expect(runs.length).toBe(2);

  runs[1].resolve();
  await runs[1].promise;
  await Promise.resolve();
  expect(runs.length).toBe(2);
});

test("a failed run does not block the next call", async () => {
  let calls = 0;
  const refresh = coalesce(() => {
    calls++;
    return Promise.reject(new Error("daemon away"));
  });
  refresh();
  await Promise.resolve();
  await Promise.resolve();
  refresh();
  expect(calls).toBe(2);
});
