// Maps computer-use tool calls to overlay actions: where the agent's cursor
// moves or clicks, and what it is typing. Coordinates are passed through in
// the CLI's screenshot API space (computer_use.yaml target_width x target_height);
// the overlay scales them to its own CSS pixels.
import type { ToolCallInfo } from "./tauri";

export type OverlayAction =
  | { kind: "move"; x: number; y: number }
  | { kind: "click"; x: number | null; y: number | null }
  | { kind: "type"; text: string };

function coord(v: unknown): number | null {
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
}

export function overlayAction(tc: ToolCallInfo): OverlayAction | null {
  let args: Record<string, unknown>;
  try {
    args = JSON.parse(tc.args);
  } catch {
    args = {};
  }
  const x = coord(args.x);
  const y = coord(args.y);
  switch (tc.name) {
    case "MouseMove":
    case "MouseScroll":
      return x !== null && y !== null ? { kind: "move", x, y } : null;
    case "MouseClick":
      return { kind: "click", x, y };
    case "KeyboardType": {
      const text = typeof args.text === "string" ? args.text : null;
      const combo = typeof args.key_combo === "string" ? args.key_combo : null;
      const shown = combo ?? text;
      return shown ? { kind: "type", text: shown } : null;
    }
    default:
      return null;
  }
}
