/// Turns an async refresh into one that never overlaps itself: a call made
/// while a run is in flight is remembered and run once, after it, so a burst
/// of calls costs at most one more request and the newest state wins.
export function coalesce(run: () => Promise<unknown>): () => void {
  let running = false;
  let pending = false;
  const start = (): void => {
    running = true;
    run()
      .catch(() => {})
      .finally(() => {
        running = false;
        if (!pending) return;
        pending = false;
        start();
      });
  };
  return () => {
    if (running) {
      pending = true;
      return;
    }
    start();
  };
}
