/** What the sidebar drags: the chats picked (or the one row grabbed), or a project. */
export type SidebarDrag = { kind: "chats"; ids: string[] } | { kind: "project"; name: string };

export type SidebarDrop =
  | { kind: "assign"; ids: string[]; project: string }
  | { kind: "unassign"; ids: string[] }
  | { kind: "moveProject"; name: string; group: string };

const PROJECT_PREFIX = "project:";
const GROUP_PREFIX = "group:";
export const UNGROUPED_TARGET = "ungrouped";

export const projectTarget = (name: string) => `${PROJECT_PREFIX}${name}`;
export const groupTarget = (label: string) => `${GROUP_PREFIX}${label}`;

/** What dropping `drag` on the sidebar target `target` does, or null when that
 * target does not take it: chats go into a project or out of every project, and
 * a project goes into a group. */
export function sidebarDrop(drag: SidebarDrag, target: string): SidebarDrop | null {
  if (drag.kind === "chats") {
    if (target === UNGROUPED_TARGET) return { kind: "unassign", ids: drag.ids };
    if (target.startsWith(PROJECT_PREFIX)) {
      return { kind: "assign", ids: drag.ids, project: target.slice(PROJECT_PREFIX.length) };
    }
    return null;
  }
  if (target.startsWith(GROUP_PREFIX)) {
    return { kind: "moveProject", name: drag.name, group: target.slice(GROUP_PREFIX.length) };
  }
  return null;
}
