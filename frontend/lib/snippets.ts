export interface Snippet {
  id: string;
  label: string;
  prompt: string;
}

import { api } from "./tauri";

const STORAGE_KEY = "snippets";

export const DEFAULT_SNIPPETS: Snippet[] = [
  {
    id: "init-agents",
    label: "Init AGENTS.md",
    prompt: "/init",
  },
  {
    id: "work-on-issue",
    label: "Work on issue",
    prompt:
      "Work on the GitHub issue linked above. Read the issue body and comments, then implement the requested changes.",
  },
  {
    id: "daily-briefing",
    label: "Daily briefing",
    prompt:
      "Give me a daily briefing of my open issues and pull requests. Summarize what needs attention and suggest where to start.",
  },
  {
    id: "prioritize-issues",
    label: "Prioritize issues",
    prompt: "Look at my open issues and suggest a priority order based on urgency and impact.",
  },
  {
    id: "open-pr",
    label: "Open PR",
    prompt: "Create a pull request for the current branch with a descriptive title and summary of changes.",
  },
];

/** Load snippets from localStorage, merging with DEFAULT_SNIPPETS so new
    defaults appear after a client update. Falls back to defaults on any error. */
export function loadSnippets(): Snippet[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const merged = mergeSnippets(JSON.parse(raw));
      saveSnippets(merged);
      return merged;
    }
  } catch {
    /* ignore corrupt data */
  }
  saveSnippets(DEFAULT_SNIPPETS);
  return DEFAULT_SNIPPETS;
}

/** Stored prompts override defaults, new defaults appear after updates, and
    stored snippets outside the default set are kept. */
export function mergeSnippets(stored: Snippet[]): Snippet[] {
  const storedById = new Map(stored.map((s) => [s.id, s]));
  const defaultIds = new Set(DEFAULT_SNIPPETS.map((d) => d.id));
  return [
    ...DEFAULT_SNIPPETS.map((d) => storedById.get(d.id) ?? d),
    ...stored.filter((s) => !defaultIds.has(s.id)),
  ];
}

export function saveSnippets(snippets: Snippet[]): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(snippets));
  try {
    api.saveDesktopSnippets(snippets).catch(() => {});
  } catch {
    /* outside Tauri (tests): skip */
  }
}

export function defaultForId(id: string): Snippet | undefined {
  return DEFAULT_SNIPPETS.find((s) => s.id === id);
}
