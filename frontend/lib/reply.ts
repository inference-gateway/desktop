const EXCERPT_LIMIT = 280;

/** The marker a prompt starts with when the user replies to an assistant message or a generated file. */
export function replyMarker(kind: "message" | "audio", text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  const excerpt = flat.length > EXCERPT_LIMIT ? `${flat.slice(0, EXCERPT_LIMIT)}…` : flat;
  return `[Replying to ${kind}: ${excerpt}]`;
}

/** Puts the reply marker above the prompt, so the agent reads what the user is replying to first. */
export function withReply(marker: string | null, text: string): string {
  if (!marker) return text;
  return text ? `${marker}\n\n${text}` : marker;
}
