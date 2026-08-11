import { convertFileSrc } from "@tauri-apps/api/core";

export type ParsedToolResult = {
  name: string;
  args: string;
  output: string;
  failed: boolean;
  imagePath: string | null;
};

// infer's ImageGeneration result carries the saved absolute path; WKWebView
// can't load a bare file path, so only paths under ~/.infer/tmp are served
// through Tauri's asset protocol.
const SAFE_IMAGE_PATH = /\.infer\/tmp\/[\w.-]+\.(?:png|gif|webp|avif|jpe?g)$/i;

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
    const output = data?.output ?? (data ? JSON.stringify(data, null, 2) : "");
    return {
      name: result.tool_name || "tool",
      args: JSON.stringify(result.arguments ?? {}),
      output,
      failed: result.success === false,
      imagePath: data?.path || null,
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
