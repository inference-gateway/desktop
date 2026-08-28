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

// Clicks on generated links must never navigate the webview away from the
// app (there is no back affordance); route http(s) links to the system
// browser instead. Backend open_url refuses non-http schemes, so anything
// else keeps default behavior.
export function handleLinkClick(
  event: { target: unknown; preventDefault: () => void },
  open: (url: string) => void,
): void {
  const href = (event.target as Element | null)?.closest?.("a[href]")?.getAttribute("href") ?? "";
  if (!/^https?:\/\//.test(href)) return;
  event.preventDefault();
  open(href);
}
