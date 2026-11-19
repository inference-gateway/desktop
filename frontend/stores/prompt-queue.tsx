import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react";
import { enqueue, takeAll, takeFirst, type PromptQueue } from "@/lib/queue";

/** The prompts typed into a busy chat, waiting per chat for its run to end.
 * `clearQueued` takes the oldest one, `takeAllQueued` all of them as one prompt. */
type PromptQueueStore = {
  queuedPrompts: PromptQueue;
  queuePrompt: (id: string, text: string) => void;
  dropQueued: (id: string) => void;
  clearQueued: (id: string) => string | null;
  takeAllQueued: (id: string) => string | null;
};

const PromptQueueContext = createContext<PromptQueueStore | null>(null);

export function PromptQueueProvider({ children }: { children: ReactNode }) {
  const [queuedPrompts, setQueuedPrompts] = useState<PromptQueue>(() => ({}));
  const queuedRef = useRef<PromptQueue>({});

  const replace = useCallback((next: PromptQueue) => {
    queuedRef.current = next;
    setQueuedPrompts(next);
  }, []);

  const queuePrompt = useCallback(
    (id: string, text: string) => replace(enqueue(queuedRef.current, id, text)),
    [replace],
  );

  const dropQueued = useCallback(
    (id: string) => {
      if (!(id in queuedRef.current)) return;
      const next = { ...queuedRef.current };
      delete next[id];
      replace(next);
    },
    [replace],
  );

  const take = useCallback(
    (taken: { queue: PromptQueue; text: string | null }): string | null => {
      if (taken.text !== null) replace(taken.queue);
      return taken.text;
    },
    [replace],
  );
  const clearQueued = useCallback((id: string) => take(takeFirst(queuedRef.current, id)), [take]);
  const takeAllQueued = useCallback((id: string) => take(takeAll(queuedRef.current, id)), [take]);

  const store = useMemo(
    () => ({ queuedPrompts, queuePrompt, dropQueued, clearQueued, takeAllQueued }),
    [queuedPrompts, queuePrompt, dropQueued, clearQueued, takeAllQueued],
  );

  return <PromptQueueContext.Provider value={store}>{children}</PromptQueueContext.Provider>;
}

export function usePromptQueue(): PromptQueueStore {
  const ctx = useContext(PromptQueueContext);
  if (!ctx) throw new Error("usePromptQueue must be used within PromptQueueProvider");
  return ctx;
}
