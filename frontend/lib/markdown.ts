import MarkdownIt from "markdown-it";

const md = new MarkdownIt({ linkify: true });

// markdown-it already handles ![alt](url); normalize bare image URLs / data
// URIs to markdown image syntax before rendering.
const BARE_IMAGE =
  /(?<!\]\()(data:image\/[a-z]+(?:;[a-z0-9-]+)*;base64,[A-Za-z0-9+/=]+|https?:\/\/[^\s)]+\.(?:png|jpg|jpeg|gif|webp|svg|avif|bmp)(?:\?[^\s)]*)?)/gi;

export function normalizeImageUrls(text: string): string {
  return text.replace(BARE_IMAGE, (url) => `![](${url})`);
}

// Ceiling: a code fence split across two stream events renders
// wrong - accumulate full text before parsing only if that shows up.
export function renderMarkdown(text: string): string {
  return md.render(normalizeImageUrls(text));
}
