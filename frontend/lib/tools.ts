import { convertFileSrc } from "@tauri-apps/api/core";

export type ParsedToolResult = {
  name: string;
  args: string;
  output: string;
  failed: boolean;
  imagePath: string | null;
  imageData: string | null;
};

// infer's ImageGeneration result carries the saved absolute path; WKWebView
// can't load a bare file path, so only paths under ~/.infer/artifacts/<session-id>
// or ~/.infer/tmp (including nested dirs like tmp/screenshots/session-<id>/)
// are served through Tauri's asset protocol.
const SAFE_IMAGE_PATH =
  /\.infer\/(?:tmp|artifacts)\/(?:[\w-][\w.-]*\/)*[\w-][\w.-]*\.(?:png|gif|webp|avif|jpe?g)$/i;

export function prettyJson(str: string): string {
  try {
    return JSON.stringify(JSON.parse(str), null, 2);
  } catch {
    return str;
  }
}

// AG-UI TOOL_CALL_RESULT.content is raw JSON; anything unparseable returns null
// so the caller can show it raw.
export function parseToolResult(content: string): ParsedToolResult | null {
  const brace = content.indexOf("{");
  if (brace === -1) return null;
  try {
    const result = JSON.parse(content.slice(brace));
    const data = result.data;
    const output = result.error ?? data?.output ?? data?.Message ?? (data ? JSON.stringify(data, null, 2) : "");
    return {
      name: result.tool_name || "tool",
      args: JSON.stringify(result.arguments ?? {}),
      output,
      failed: result.success === false,
      imagePath: data?.path || null,
      imageData:
        typeof result.images?.[0]?.data === "string"
          ? `data:${result.images[0].mime_type || "image/jpeg"};base64,${result.images[0].data}`
          : null,
    };
  } catch {
    return null;
  }
}

export function safeImageSrc(path: string | null | undefined): string | null {
  if (!path || !SAFE_IMAGE_PATH.test(path)) return null;
  return convertFileSrc(path);
}

export function imageFilename(path: string): string {
  return path.split("/").pop() ?? path;
}
