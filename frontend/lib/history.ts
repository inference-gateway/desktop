export interface History<T> {
  push: (prev: T) => void;
  begin: (current: T) => void;
  commit: (current: T) => boolean;
  undo: (current: T) => T | undefined;
  redo: (current: T) => T | undefined;
  canUndo: () => boolean;
  canRedo: () => boolean;
  reset: () => void;
}

export function createHistory<T>(limit = 50): History<T> {
  let past: T[] = [];
  let future: T[] = [];
  let start: T | undefined;
  const push = (prev: T) => {
    past.push(prev);
    if (past.length > limit) past.shift();
    future = [];
  };
  return {
    push,
    begin(current) {
      start = current;
    },
    commit(current) {
      const from = start;
      start = undefined;
      if (from === undefined || from === current) return false;
      push(from);
      return true;
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
    canUndo: () => past.length > 0,
    canRedo: () => future.length > 0,
    reset() {
      past = [];
      future = [];
      start = undefined;
    },
  };
}
