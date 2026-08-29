import { expect, test } from "bun:test";
import { DEFAULT_SNIPPETS, mergeSnippets, type Snippet } from "./snippets";

// mergeSnippets is exported for #166 import hydration (store.tsx); keep the
// merge contract pinned: stored prompts win, new defaults appear, extras kept.
test("mergeSnippets: stored overrides defaults, custom snippets kept", () => {
  const stored: Snippet[] = [
    { id: DEFAULT_SNIPPETS[0].id, label: DEFAULT_SNIPPETS[0].label, prompt: "custom" },
    { id: "mine", label: "Mine", prompt: "keep me" },
  ];
  const merged = mergeSnippets(stored);
  expect(merged.find((s) => s.id === DEFAULT_SNIPPETS[0].id)?.prompt).toBe("custom");
  expect(merged.some((s) => s.id === "mine" && s.prompt === "keep me")).toBe(true);
  for (const d of DEFAULT_SNIPPETS) expect(merged.some((s) => s.id === d.id)).toBe(true);
});
