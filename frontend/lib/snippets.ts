export interface Snippet {
  id: string;
  label: string;
  prompt: string;
}

const STORAGE_KEY = "snippets";

export const DEFAULT_SNIPPETS: Snippet[] = [
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
    prompt:
      "Look at my open issues and suggest a priority order based on urgency and impact.",
  },
  {
    id: "open-pr",
    label: "Open PR",
    prompt:
      "Create a pull request for the current branch with a descriptive title and summary of changes.",
  },
];

/** Load snippets from localStorage, merging with DEFAULT_SNIPPETS so new
    defaults appear after a client update. Falls back to defaults on any error. */
export function loadSnippets(): Snippet[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const stored: Snippet[] = JSON.parse(raw);
      const storedById = new Map(stored.map((s) => [s.id, s]));
      // Merge: keep stored prompts, add any new defaults
      const merged = DEFAULT_SNIPPETS.map((d) => storedById.get(d.id) ?? d);
      saveSnippets(merged);
      return merged;
    }
  } catch {
    /* ignore corrupt data */
  }
  saveSnippets(DEFAULT_SNIPPETS);
  return DEFAULT_SNIPPETS;
}

export function saveSnippets(snippets: Snippet[]): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(snippets));
}

export function defaultForId(id: string): Snippet | undefined {
  return DEFAULT_SNIPPETS.find((s) => s.id === id);
}
