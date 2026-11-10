/** Events the backend emits to the windows. `backend/src/events.rs` holds the
 * same names, and a Rust test fails when the two lists differ. */
export const BACKEND_EVENTS = {
  agentEvent: "agent-event",
  browserBridge: "browser-bridge",
  gitChanged: "git-changed",
  projectChanged: "project-changed",
} as const;

/** Events the windows send each other, which the backend never sees. */
export const WINDOW_EVENTS = {
  approvalResolved: "approval-resolved",
  monitorSend: "monitor-send",
  screenRecording: "screen-recording",
} as const;
