/** A menu token the cursor is in: where its trigger sits and the text typed after it. */
export type Mention = { trigger: string; start: number; query: string };

const MENTION = /(^|\s)(@)(\S*)$/;
const MAX_ROWS = 50;

/** The `@` token that ends at the cursor, or null. The trigger starts the text
    or follows whitespace, so an email address opens nothing. */
export function mentionAt(text: string, cursor: number): Mention | null {
  const match = MENTION.exec(text.slice(0, cursor));
  if (!match) return null;
  return { trigger: match[2], start: match.index + match[1].length, query: match[3] };
}

/** The files whose path contains the query, ignoring case.
    ponytail: the first 50 matches, rank them if big projects bury the right one. */
export function filterFiles(files: string[], query: string): string[] {
  const needle = query.toLowerCase();
  return files.filter((file) => file.toLowerCase().includes(needle)).slice(0, MAX_ROWS);
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
