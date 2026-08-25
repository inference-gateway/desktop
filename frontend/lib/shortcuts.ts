// App-wide keyboard shortcuts, mirroring the `infer chat` CLI conventions:
// Cmd/Ctrl+N = new chat, Esc = cancel the active run, Shift+Tab = toggle auto mode.
// Pure matcher so the decision logic is unit-testable without a DOM.

export type Shortcut = "newChat" | "cancel" | "autoModeToggle";

export interface KeyInput {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  repeat: boolean;
  defaultPrevented: boolean;
  inComposer: boolean;
}

export function matchShortcut(e: KeyInput): Shortcut | null {
  if (e.repeat || e.defaultPrevented) return null;
  if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "n") {
    return "newChat";
  }
  if (e.key === "Escape" && !e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey) {
    return "cancel";
  }
  if (e.key === "Tab" && e.shiftKey && !e.metaKey && !e.ctrlKey && !e.altKey && e.inComposer) {
    return "autoModeToggle";
  }
  return null;
}
