import { convertFileSrc } from "@tauri-apps/api/core";

// Bash mode mirrors the CLI: a single leading `!` runs the rest as a shell
// command; `!!` (two) is a direct tool call and takes precedence.
export function isBashCommand(text: string): boolean {
  return text.startsWith("!") && !text.startsWith("!!");
}

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
const SAFE_IMAGE_PATH = /\.infer\/(?:tmp|artifacts)\/(?:[\w-][\w.-]*\/)*[\w-][\w.-]*\.(?:png|gif|webp|avif|jpe?g)$/i;

// infer's TextToSpeech writes WAVs to its default output dir ~/.infer/tts and
// voice samples live in ~/.infer/models/tts/samples; both are in the asset
// protocol scope (tauri.conf.json). ponytail: a custom text_to_speech.output_dir
// outside these dirs falls back to the plain tool card.
const SAFE_AUDIO_PATH = /\.infer\/(?:tts|models\/tts\/samples)\/(?:(?!\.\.\/)[^/\0]+\/)*[^/\0]+\.wav$/i;

export function prettyJson(str: string): string {
  try {
    return JSON.stringify(JSON.parse(str), null, 2);
  } catch {
    return str;
  }
}

// The CLI's ToolExecutionResult: streamed inline as AG-UI TOOL_CALL_RESULT
// content, and projected as an entry's tool_execution by `conversations show
// --format json`. Both carry success, so failure is never guessed.
export function toolResultFrom(result: any): ParsedToolResult {
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
}

// AG-UI TOOL_CALL_RESULT.content is raw JSON; anything unparseable returns null
// so the caller can show it raw.
export function parseToolResult(content: string): ParsedToolResult | null {
  const brace = content.indexOf("{");
  if (brace === -1) return null;
  try {
    return toolResultFrom(JSON.parse(content.slice(brace)));
  } catch {
    return null;
  }
}

export function safeImageSrc(path: string | null | undefined): string | null {
  if (!path || !SAFE_IMAGE_PATH.test(path)) return null;
  return convertFileSrc(path);
}

// The backend widens the asset scope to each project directory when its
// timeline opens (list_timelines), so this only rejects traversal and
// non-media extensions; the asset protocol itself refuses anything else.
const SAFE_PROJECT_MEDIA_PATH = /^\/(?:(?!\.\.\/)[^/\0]+\/)*[^/\0]+\.(?:mp4|mov|m4v|webm|wav|mp3|m4a|aac|ogg|flac)$/i;

export function safeProjectMediaSrc(path: string | null | undefined): string | null {
  if (!path || !SAFE_PROJECT_MEDIA_PATH.test(path)) return null;
  return convertFileSrc(path);
}

export function safeAudioSrc(path: string | null | undefined): string | null {
  if (!path || !SAFE_AUDIO_PATH.test(path)) return null;
  return convertFileSrc(path);
}

export function imageFilename(path: string): string {
  return path.split("/").pop() ?? path;
}
