// Bounded snapshot history for whole-document undo/redo: push the previous
// value before an edit, undo/redo swap it with the current one. Snapshots beat
// command inversion when every edit is a pure function to a new value, as the
// timeline helpers are. push drops the redo branch and trims entries past the
// limit. ponytail: snapshots only - per-command deltas if timelines get huge.

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
