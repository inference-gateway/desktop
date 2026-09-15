// App-wide keyboard shortcuts, mirroring the `infer chat` CLI conventions:
// Cmd/Ctrl+N = new chat, Esc = cancel the active run, Shift+Tab = toggle auto mode,
// a/d = approve/deny the pending tool approval. Pure matchers so the decision
// logic is unit-testable without a DOM.

export type Shortcut = "newChat" | "cancel" | "autoModeToggle";
export type ApprovalShortcut = "approve" | "deny";

export interface KeyInput {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  repeat: boolean;
  defaultPrevented: boolean;
  inComposer: boolean;
  editable?: boolean;
}

export function matchShortcut(e: KeyInput): Shortcut | null {
  if (e.repeat) return null;
  if (e.key === "Escape" && !e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey) {
    return "cancel";
  }
  if (e.defaultPrevented) return null;
  if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "n") {
    return "newChat";
  }
  if (e.key === "Tab" && e.shiftKey && !e.metaKey && !e.ctrlKey && !e.altKey && e.inComposer) {
    return "autoModeToggle";
  }
  return null;
}

// The composer counts in: the prompt that triggered the approval was just sent
// from there, so focus is still in it when the approval appears. Any other
// editable target keeps its keys.
export function matchApprovalShortcut(e: KeyInput): ApprovalShortcut | null {
  if (e.repeat || e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey) return null;
  if (e.editable && !e.inComposer) return null;
  const key = e.key.toLowerCase();
  if (key !== "a" && key !== "d") return null;
  return key === "a" ? "approve" : "deny";
}
