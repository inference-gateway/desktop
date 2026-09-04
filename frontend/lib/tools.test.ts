import { expect, test } from "bun:test";
import { parseToolResult, safeAudioSrc, safeImageSrc } from "./tools";

test("ImageDecode result with an uploads source does not produce an image item", () => {
  const parsed = parseToolResult(
    '{"tool_name":"ImageDecode","data":{"output":"a cat","source":"/Users/x/.infer/uploads/18cadaad968c6e30.png"},"success":true}',
  );
  expect(parsed?.imagePath).toBeNull();
});

test("ImageGeneration result path under ~/.infer/tmp is previewable", () => {
  const parsed = parseToolResult(
    '{"tool_name":"ImageGeneration","data":{"output":"saved","path":"/Users/x/.infer/tmp/out.png"},"success":true}',
  );
  expect(parsed?.imagePath).toBe("/Users/x/.infer/tmp/out.png");
});

test("ImageGeneration result path under ~/.infer/artifacts/<session-id> is previewable", () => {
  const parsed = parseToolResult(
    '{"tool_name":"ImageGeneration","data":{"output":"saved","path":"/Users/x/.infer/artifacts/sid-1/out.png"},"success":true}',
  );
  expect(parsed?.imagePath).toBe("/Users/x/.infer/artifacts/sid-1/out.png");
});

test("failed tool result exposes its top-level error", () => {
  const parsed = parseToolResult(
    '{"tool_name":"Read","arguments":{"file_path":"/outside/file"},"success":false,"error":"path is outside configured sandbox directories"}',
  );
  expect(parsed).toMatchObject({
    name: "Read",
    output: "path is outside configured sandbox directories",
    failed: true,
  });
});

test("safeImageSrc rejects uploads paths, non-image extensions, and traversal", () => {
  expect(safeImageSrc("/Users/x/.infer/uploads/a.png")).toBeNull();
  expect(safeImageSrc("/Users/x/.infer/tmp/a.pdf")).toBeNull();
  expect(safeImageSrc("/Users/x/.infer/artifacts/sid-1/a.pdf")).toBeNull();
  expect(safeImageSrc("/Users/x/.infer/tmp/../uploads/a.png")).toBeNull();
});

test("safeImageSrc allows nested computer-use screenshot paths", () => {
  (globalThis as Record<string, unknown>).window = {
    __TAURI_INTERNALS__: { convertFileSrc: (p: string) => `asset://localhost/${p}` },
  };
  try {
    expect(safeImageSrc("/Users/x/.infer/tmp/screenshots/session-b9fb/frame_001.png")).not.toBeNull();
    expect(safeImageSrc("/Users/x/.infer/artifacts/sid-1/nested/a.png")).not.toBeNull();
  } finally {
    delete (globalThis as Record<string, unknown>).window;
  }
});

test("TextToSpeech result path in the default tts output dir is previewable", () => {
  const parsed = parseToolResult(
    '{"tool_name":"TextToSpeech","data":{"path":"/Users/x/.infer/tts/speech-20260102-150405-123.wav","text":"hi","duration_seconds":1.5},"success":true}',
  );
  expect(parsed?.imagePath).toBe("/Users/x/.infer/tts/speech-20260102-150405-123.wav");
});

test("safeAudioSrc rejects non-tts wav paths, non-wav files, and traversal", () => {
  (globalThis as Record<string, unknown>).window = {
    __TAURI_INTERNALS__: { convertFileSrc: (p: string) => `asset://localhost/${p}` },
  };
  try {
    expect(safeAudioSrc("/Users/x/.infer/tts/speech-1.wav")).not.toBeNull();
    expect(safeAudioSrc("/Users/x/.infer/models/tts/samples/me.wav")).not.toBeNull();
    expect(safeAudioSrc("/Users/x/.infer/uploads/me.wav")).toBeNull();
    expect(safeAudioSrc("/Users/x/.infer/tts/Screen Recording 2026-09-03 at 14.05.28-s1.wav")).not.toBeNull();
    expect(safeAudioSrc("/Users/x/.infer/tts/speech.mp3")).toBeNull();
    expect(safeAudioSrc("/Users/x/.infer/tts/../uploads/me.wav")).toBeNull();
  } finally {
    delete (globalThis as Record<string, unknown>).window;
  }
});
