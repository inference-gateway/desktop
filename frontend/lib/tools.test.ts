import { expect, test } from "bun:test";
import { parseToolResult, safeImageSrc } from "./tools";

test("ImageDecode result with an uploads source does not produce an image item", () => {
  const parsed = parseToolResult(
    '{"tool_name":"ImageDecode","data":{"output":"a cat","source":"/Users/x/.infer/uploads/18cadaad968c6e30.png"},"success":true}'
  );
  expect(parsed?.imagePath).toBeNull();
});

test("ImageGeneration result path under ~/.infer/tmp is previewable", () => {
  const parsed = parseToolResult(
    '{"tool_name":"ImageGeneration","data":{"output":"saved","path":"/Users/x/.infer/tmp/out.png"},"success":true}'
  );
  expect(parsed?.imagePath).toBe("/Users/x/.infer/tmp/out.png");
});

test("ImageGeneration result path under ~/.infer/artifacts/<session-id> is previewable", () => {
  const parsed = parseToolResult(
    '{"tool_name":"ImageGeneration","data":{"output":"saved","path":"/Users/x/.infer/artifacts/sid-1/out.png"},"success":true}'
  );
  expect(parsed?.imagePath).toBe("/Users/x/.infer/artifacts/sid-1/out.png");
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
    expect(
      safeImageSrc("/Users/x/.infer/tmp/screenshots/session-b9fb/frame_001.png")
    ).not.toBeNull();
    expect(safeImageSrc("/Users/x/.infer/artifacts/sid-1/nested/a.png")).not.toBeNull();
  } finally {
    delete (globalThis as Record<string, unknown>).window;
  }
});
