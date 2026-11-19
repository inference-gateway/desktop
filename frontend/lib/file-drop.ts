export type Point = { x: number; y: number };

/** Where a file dropped from the OS lands in the page, in CSS pixels. Tauri
 * measures the drop against the webview's frame, in device pixels on Windows.
 * On macOS that frame runs under the title bar while the page starts below it,
 * so the part of the frame above the page comes off. */
export function dropPoint(
  position: Point,
  view: { devicePixels: boolean; scale: number; frameHeight: number; pageHeight: number },
): Point {
  const scale = view.devicePixels ? view.scale : 1;
  return { x: position.x / scale, y: position.y / scale - (view.frameHeight - view.pageHeight) };
}
