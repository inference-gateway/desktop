import MarkdownIt from "markdown-it";

const md = new MarkdownIt({ linkify: true });

// A remote image is fetched the moment it renders, which lets agent output
// send data out in the URL with no tool call to approve. It renders as a link
// instead, so nothing leaves until the user clicks it.
const PROTOCOL_RELATIVE = /^\/\//;

// The http(s) address a webview would fetch `src` from, however it is
// spelled (`//host`, `https:host`), or null when it stays on this machine.
function remoteUrl(src: string): URL | null {
  try {
    const url = new URL(src.replace(PROTOCOL_RELATIVE, "https://"));
    return url.protocol === "http:" || url.protocol === "https:" ? url : null;
  } catch {
    return null;
  }
}

const renderImage = md.renderer.rules.image!;
md.renderer.rules.image = (tokens, idx, options, env, self) => {
  const url = remoteUrl(String(tokens[idx].attrGet("src") ?? ""));
  if (!url) return renderImage(tokens, idx, options, env, self);
  const alt = self.renderInlineAsText(tokens[idx].children ?? [], options, env);
  const label = alt ? `${alt} (image from ${url.host})` : `Image from ${url.host}`;
  return `<a href="${md.utils.escapeHtml(url.href)}">${md.utils.escapeHtml(label)}</a>`;
};

// markdown-it already handles ![alt](url); normalize bare data URIs to
// markdown image syntax before rendering.
const BARE_IMAGE = /(?<!\]\()data:image\/[a-z]+(?:;[a-z0-9-]+)*;base64,[A-Za-z0-9+/=]+/gi;

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
