import { useEffect, useState } from "react";
import { api, type Screen } from "@/lib/tauri";

/** How often a screen someone only watches is refreshed. */
export const WATCHED_POLL_MS = 2000;

/** The latest screen of a session's computer, polled every `pollMs` while `live` and the window is visible. */
export function useScreen(
  sessionId: string,
  project: string | undefined,
  live: boolean,
  pollMs: number,
): Screen | null {
  const [screen, setScreen] = useState<Screen | null>(null);
  useEffect(() => {
    let cancelled = false;
    let pending = false;
    let shownFrame: string | undefined;
    const poll = async () => {
      if (pending || document.hidden) return;
      pending = true;
      const frame = await api.screenFrame(sessionId, project, shownFrame).catch(() => null);
      pending = false;
      if (!frame || cancelled) return;
      shownFrame = frame.id;
      setScreen(frame);
    };
    poll();
    if (!live) return () => void (cancelled = true);
    const timer = setInterval(poll, pollMs);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [sessionId, project, live, pollMs]);
  return screen;
}
