export interface History<T> {
  push: (prev: T) => void;
  undo: (current: T) => T | undefined;
  redo: (current: T) => T | undefined;
  reset: () => void;
}

export function createHistory<T>(limit = 50): History<T> {
  let past: T[] = [];
  let future: T[] = [];
  return {
    push(prev) {
      past.push(prev);
      if (past.length > limit) past.shift();
      future = [];
    },
    undo(current) {
      const prev = past.pop();
      if (prev === undefined) return undefined;
      future.unshift(current);
      return prev;
    },
    redo(current) {
      const next = future.shift();
      if (next === undefined) return undefined;
      past.push(current);
      return next;
    },
    reset() {
      past = [];
      future = [];
    },
  };
}
