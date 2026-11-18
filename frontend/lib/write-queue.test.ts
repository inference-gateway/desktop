import { expect, test } from "bun:test";
import { createWriteQueue } from "./write-queue";

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

test("a burst of changes while a save is in flight writes only the latest document", async () => {
  const written: string[] = [];
  const writes: ReturnType<typeof deferred>[] = [];
  const save = createWriteQueue((data: string) => {
    const d = deferred();
    writes.push(d);
    written.push(data);
    return d.promise;
  });

  const first = save("first");
  save("second");
  save("third");
  const last = save("fourth");
  expect(written).toEqual(["first"]); // one write at a time

  writes[0].resolve();
  await first;
  // the changes made meanwhile coalesce: the next write carries only the newest document
  expect(written).toEqual(["first", "fourth"]);
  expect(writes.length).toBe(2);

  writes[1].resolve();
  await last;
  expect(written).toEqual(["first", "fourth"]); // idle, the burst cost one extra write
});

test("a failed write rejects its callers and does not block the next one", async () => {
  let calls = 0;
  const save = createWriteQueue(() => {
    calls++;
    return calls === 1 ? Promise.reject(new Error("disk gone")) : Promise.resolve();
  });
  const first = save("one");
  const second = save("two");
  await expect(first).rejects.toThrow("disk gone");
  await expect(second).resolves.toBeUndefined();
  expect(calls).toBe(2);
});
