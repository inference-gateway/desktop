import type { Issue, MarkdownAgent } from "./tauri";

/** A menu token the cursor is in: where its trigger sits and the text typed after it. */
export type Mention = { trigger: string; start: number; query: string };

/** One menu row: the text it inserts, the text it shows and the agent an
 * agents row names, so the menu can show its avatar. */
export type MentionRow = { value: string; label: string; agent?: AgentFace };

/** The bit of an agent a menu row shows: its name, picture and emoji. */
export type AgentFace = Pick<MarkdownAgent, "name" | "emoji" | "avatar">;

/** What a trigger's menu lists per source, in the order the context's table gives. */
export type MenuSources = { files: string[]; issues: Issue[]; agents: AgentFace[] };

const MAX_ROWS = 50;

/** The token ending at the cursor that starts with one of the composer
 * table's triggers, or null. The trigger starts the text or follows
 * whitespace, so an email address, `C#` or a URL fragment opens nothing, and
 * it does not recur in the token, so `work on #5#7` refines nothing. */
export function mentionAt(text: string, cursor: number, triggers: string[]): Mention | null {
  const before = text.slice(0, cursor);
  const token = /[^ \t\n]*$/.exec(before)?.[0] ?? "";
  for (const trigger of triggers) {
    if (!token.startsWith(trigger) || token.slice(trigger.length).includes(trigger)) continue;
    return { trigger, start: before.length - token.length, query: token.slice(trigger.length) };
  }
  return null;
}

/** The files whose path contains the query, ignoring case.
    ponytail: the first 50 matches, rank them if big projects bury the right one. */
export function filterFiles(files: string[], query: string): string[] {
  const needle = query.toLowerCase();
  return files.filter((file) => file.toLowerCase().includes(needle)).slice(0, MAX_ROWS);
}

/** The issues whose number or title contains the query, ignoring case. */
export function filterIssues(issues: Issue[], query: string): Issue[] {
  const needle = query.toLowerCase();
  return issues.filter((issue) => `${issue.number} ${issue.title.toLowerCase()}`.includes(needle));
}

/** The agents whose name contains the query, ignoring case. */
export function filterAgents(agents: AgentFace[], query: string): AgentFace[] {
  const needle = query.toLowerCase();
  return agents.filter((agent) => agent.name.toLowerCase().includes(needle));
}

/** The rows the open token's menu offers: one section per source the context's
 * table lists for its trigger. The trigger only picks which menu opens and in
 * which order its sections come - every row inserts its own sigil, `@<path>`
 * files, `@<name>` agents and `#<number>` issues - and the whole menu caps at
 * the section order's first rows. */
export function menuRows(mention: Mention | null, table: Record<string, string[]>, sources: MenuSources): MentionRow[] {
  if (!mention) return [];
  const rows: MentionRow[] = [];
  for (const source of table[mention.trigger] ?? []) {
    if (source === "issues") {
      rows.push(
        ...filterIssues(sources.issues, mention.query).map((issue) => ({
          value: `#${issue.number}`,
          label: `#${issue.number} ${issue.title}`,
        })),
      );
    } else if (source === "agents") {
      rows.push(
        ...filterAgents(sources.agents, mention.query).map((agent) => ({
          value: `@${agent.name}`,
          label: agent.name,
          agent,
        })),
      );
    } else if (source === "files") {
      rows.push(...filterFiles(sources.files, mention.query).map((file) => ({ value: `@${file}`, label: file })));
    }
  }
  return rows.slice(0, MAX_ROWS);
}

/** The text with the token replaced by `value` and a space, and the cursor right
    after them. Text that already goes on with a space gets no second one. */
export function insertMention(
  text: string,
  cursor: number,
  mention: Mention,
  value: string,
): { text: string; cursor: number } {
  const tail = text.slice(cursor);
  const head = `${text.slice(0, mention.start)}${value}${/^\s/.test(tail) ? "" : " "}`;
  return { text: head + tail, cursor: head.length };
}

/** What a key does in an open menu of `count` rows with row `index` active - the
    slash menu's interaction: the arrows move to a row, Enter picks it, Escape closes. */
export function menuKey(key: string, index: number, count: number): number | "pick" | "close" | null {
  if (key === "ArrowDown") return Math.min(index + 1, count - 1);
  if (key === "ArrowUp") return Math.max(index - 1, 0);
  if (key === "Enter") return "pick";
  if (key === "Escape") return "close";
  return null;
}
