export type ProjectType = "code" | "content";

/** What `projects.yaml` holds: the project catalog, and for every chat the
 * project, agent, workspace and title the sidebar files it under. */
export type SidebarDocument = {
  assignments: Record<string, string>;
  names: string[];
  contexts: Record<string, string>;
  paths: Record<string, string>;
  groups: Record<string, string>;
  types: Record<string, ProjectType>;
  agents: Record<string, string>;
  workspaces: Record<string, string>;
  titles: Record<string, string>;
  selected: Set<string>;
  active: string | null;
};

function strings(value: unknown, keep: (text: string) => boolean): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, text] of Object.entries(value ?? {})) {
    if (typeof text === "string" && keep(text)) out[key] = text;
  }
  return out;
}

const filled = (text: string) => text.trim() !== "";
const any = () => true;

/** Reads the stored document, dropping anything malformed. A project that only
 * appears as a chat's assignment still becomes a project, and a selection or
 * active project that no longer exists is dropped. */
export function parseSidebarDocument(raw: string): SidebarDocument {
  const parsed = JSON.parse(raw);
  const assignments = strings(parsed?.assignments, filled);
  const names = new Set<string>();
  if (Array.isArray(parsed?.names)) {
    for (const n of parsed.names) {
      if (typeof n === "string" && n.trim()) names.add(n);
    }
  }
  for (const name of Object.values(assignments)) names.add(name);
  const types: Record<string, ProjectType> = {};
  for (const [name, t] of Object.entries(parsed?.types ?? {})) {
    if (t === "content") types[name] = t;
  }
  const selected = new Set<string>();
  if (Array.isArray(parsed?.selected)) {
    for (const n of parsed.selected) {
      if (typeof n === "string" && names.has(n)) selected.add(n);
    }
  }
  return {
    assignments,
    names: Array.from(names),
    contexts: strings(parsed?.contexts, filled),
    paths: strings(parsed?.paths, filled),
    groups: strings(parsed?.groups, filled),
    types,
    agents: strings(parsed?.agents, filled),
    workspaces: strings(parsed?.workspaces, any),
    titles: strings(parsed?.titles, any),
    selected,
    active: typeof parsed?.active === "string" && names.has(parsed.active) ? parsed.active : null,
  };
}

export function serializeSidebarDocument(doc: SidebarDocument): string {
  return JSON.stringify({ ...doc, selected: Array.from(doc.selected), active: doc.active ?? undefined });
}
