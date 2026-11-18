/** The sidebar's save queue for projects.yaml: one write at a time, and a save
 * made while one is in flight is folded into the next one, so a burst of
 * changes only ever writes the latest document and no two writes can land out
 * of order. Each call's promise settles when the write that carries its
 * content (or the one that superseded it) finishes. */
export function createWriteQueue(write: (data: string) => Promise<unknown>): (data: string) => Promise<void> {
  type Waiter = { resolve: () => void; reject: (reason: unknown) => void };
  let writing = false;
  let pending: { data: string; waiters: Waiter[] } | null = null;

  const flush = (): void => {
    if (writing || !pending) return;
    const { data, waiters } = pending;
    pending = null;
    writing = true;
    write(data).then(
      () => {
        writing = false;
        for (const w of waiters) w.resolve();
        flush();
      },
      (reason: unknown) => {
        writing = false;
        for (const w of waiters) w.reject(reason);
        flush();
      },
    );
  };

  return (data: string) =>
    new Promise<void>((resolve, reject) => {
      if (pending) {
        pending.data = data;
      } else {
        pending = { data, waiters: [] };
      }
      pending.waiters.push({ resolve, reject });
      flush();
    });
}
