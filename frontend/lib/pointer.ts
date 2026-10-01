// Maps Computer tool calls to the overlay's key-cast: what the agent is
// typing or which key combo it presses. Pointer moves and clicks come from the
// CLI's computer_use activity events instead, already in screen coordinates.
import type { ToolCallInfo } from "./tauri";

export type OverlayAction = { kind: "type"; text: string };

export function overlayAction(tc: ToolCallInfo): OverlayAction | null {
  if (tc.name !== "Computer") return null;
  let args: Record<string, unknown>;
  try {
    args = JSON.parse(tc.args);
  } catch {
    return null;
  }
  if (args.action !== "type" && args.action !== "key") return null;
  const shown = typeof args.combo === "string" ? args.combo : typeof args.text === "string" ? args.text : null;
  return shown ? { kind: "type", text: shown } : null;
}

/** Whether a computer-use action positions the pointer with a click, so the
 * overlay ripples at it. */
export function isClickAction(action: string): boolean {
  return action === "click" || action === "double_click" || action === "triple_click";
}
