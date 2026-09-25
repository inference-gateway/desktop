// Agent recording helpers: where the overlay draws the frame around the
// recorded area, and how the recording session lists its captured input.
import type { InputEvent, RecordedArea } from "./tauri";

export type OverlayRect = { left: number; top: number; width: number; height: number };

/** The overlay window's size in CSS pixels (it spans the primary screen's full
 * width) and how far below the screen top it starts (the macOS menu bar). */
export type OverlayScreen = { width: number; height: number; dy: number };

const EDGE_INSET = 4;

/** Maps a recorded area from the CLI's frame space to overlay CSS pixels and
 * grows it by `border` on every side, so the frame sits just outside what is
 * recorded and never lands in the video. An edge that reaches the screen edge
 * (a full-screen recording) is clamped inside, like the computer-use frame.
 * ponytail: that clamped edge is only kept out of the video by the overlay's
 * contentProtected, which Tauri does not support on Linux - hide the frame for
 * full-screen recordings there if it shows up in X11 captures. */
export function recordingFrame(area: RecordedArea, screen: OverlayScreen, border: number): OverlayRect {
  const s = screen.width / area.frame_width;
  const x = area.x * s;
  const y = area.y * s - screen.dy;
  const left = Math.max(EDGE_INSET, x - border);
  const top = Math.max(EDGE_INSET, y - border);
  const right = Math.min(screen.width - EDGE_INSET, x + area.width * s + border);
  const bottom = Math.min(screen.height - EDGE_INSET, y + area.height * s + border);
  return { left, top, width: Math.max(0, right - left), height: Math.max(0, bottom - top) };
}

/** `00:12.4 click left (640, 380)` / `00:15.1 key cmd+s`; null for a bare
 * modifier release, which carries no keys. */
export function formatInput(e: InputEvent): string | null {
  const tenths = Math.round(e.t * 10);
  const time = `${String(Math.floor(tenths / 600)).padStart(2, "0")}:${((tenths % 600) / 10).toFixed(1).padStart(4, "0")}`;
  if (e.kind === "click") return `${time} click ${e.button} (${e.x}, ${e.y})`;
  return e.keys ? `${time} key ${e.keys}` : null;
}
