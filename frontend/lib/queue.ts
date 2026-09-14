// Pending follow-up prompts keyed by session: typed in the composer while the
// session's turn is running (#231). The first item is sent when the turn ends
// (Done or Cancelled); ArrowUp pops the whole list back into the composer for
// editing, the queued banner discards it.
export type PromptQueue = Record<string, string[]>;

export const enqueue = (queue: PromptQueue, id: string, text: string): PromptQueue => ({
  ...queue,
  [id]: [...(queue[id] ?? []), text],
});

export const takeFirst = (queue: PromptQueue, id: string): { queue: PromptQueue; text: string | null } => {
  const items = queue[id] ?? [];
  if (items.length === 0) return { queue, text: null };
  const text = items[0] ?? null;
  const rest = items.slice(1);
  const next = { ...queue };
  if (rest.length > 0) next[id] = rest;
  else delete next[id];
  return { queue: next, text };
};

export const takeAll = (queue: PromptQueue, id: string): { queue: PromptQueue; text: string | null } => {
  const items = queue[id] ?? [];
  if (items.length === 0) return { queue, text: null };
  const next = { ...queue };
  delete next[id];
  return { queue: next, text: items.join("\n") };
};
