// Maps Computer tool calls to overlay actions: where the agent's cursor
// moves or clicks, and what it is typing. Coordinates arrive in the CLI's
// frame coordinate space (computer_use.yaml screenshot.target_width x
// target_height, aspect preserved); the overlay scales them to its own CSS
// pixels.
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
  if (tc.name !== "Computer") return null;
  let args: Record<string, unknown>;
  try {
    args = JSON.parse(tc.args);
  } catch {
    return null;
  }
  const x = coord(args.x);
  const y = coord(args.y);
  switch (args.action) {
    case "move":
    case "scroll":
      return x !== null && y !== null ? { kind: "move", x, y } : null;
    case "click":
    case "double_click":
    case "triple_click":
      return { kind: "click", x, y };
    case "type":
    case "key": {
      const shown =
        typeof args.combo === "string"
          ? args.combo
          : typeof args.text === "string"
            ? args.text
            : null;
      return shown ? { kind: "type", text: shown } : null;
    }
    default:
      return null;
  }
}
