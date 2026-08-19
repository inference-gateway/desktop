// Fullscreen, transparent, click-through overlay that visualizes computer-use
// actions: a rounded border glows at the screen edges while any computer-use
// session is active, a cursor dot glides to each MouseMove/MouseScroll target,
// a ring ripples on MouseClick, and a key-cast pill at the bottom shows what
// the agent is typing. Fed by the backend's global "agent-event" broadcast;
// all animation is CSS inside this webview, so no per-frame IPC.
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  getCurrentWindow,
  currentMonitor,
  primaryMonitor,
  PhysicalPosition,
  PhysicalSize,
} from "@tauri-apps/api/window";
import type { AgentEvent } from "@/lib/tauri";
import { overlayAction } from "@/lib/pointer";
import { COMPUTER_USE_TOOLS } from "@/lib/transcript";

const IDLE_HIDE_MS = 1600;
const ACCENT = "99, 102, 241";
// ponytail: 1024x768 mirrors the CLI's computer_use.yaml screenshot.target_*;
// read it from config if it ever becomes configurable. The CLI fits the frame
// inside that box with one uniform scale factor (aspect preserved), so the
// inverse here is a single factor too.
const API_WIDTH = 1024;
const API_HEIGHT = 768;

const STYLE = `
#frame {
  position: fixed;
  inset: 4px;
  border: 3px solid rgba(${ACCENT}, 0.75);
  border-radius: 14px;
  box-shadow: 0 0 18px rgba(${ACCENT}, 0.5), inset 0 0 24px rgba(${ACCENT}, 0.25);
  animation: breathe 3s ease-in-out infinite;
  pointer-events: none;
}
@keyframes breathe {
  50% { opacity: 0.45; }
}
#cursor {
  position: fixed;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: rgba(${ACCENT}, 0.9);
  border: 2px solid rgba(255, 255, 255, 0.9);
  box-shadow: 0 0 12px rgba(${ACCENT}, 0.8);
  transform: translate(-50%, -50%);
  transition: left 0.35s cubic-bezier(0.25, 1, 0.4, 1), top 0.35s cubic-bezier(0.25, 1, 0.4, 1);
  pointer-events: none;
}
.ripple {
  position: fixed;
  width: 64px;
  height: 64px;
  border-radius: 50%;
  border: 3px solid rgba(${ACCENT}, 0.95);
  transform: translate(-50%, -50%) scale(0.2);
  animation: ripple 0.55s ease-out forwards;
  pointer-events: none;
}
@keyframes ripple {
  to { transform: translate(-50%, -50%) scale(1.8); opacity: 0; }
}
#keycast {
  position: fixed;
  bottom: 48px;
  left: 50%;
  transform: translateX(-50%);
  max-width: 70vw;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  padding: 8px 16px;
  border-radius: 10px;
  background: rgba(20, 20, 24, 0.85);
  border: 1px solid rgba(255, 255, 255, 0.15);
  color: #fff;
  font: 500 14px ui-monospace, monospace;
  animation: keycast 0.15s ease-out;
  pointer-events: none;
}
@keyframes keycast {
  from { transform: translateX(-50%) translateY(8px); opacity: 0; }
}
`;

type Ripple = { x: number; y: number; seq: number };
type Keycast = { text: string; seq: number };

export default function Overlay() {
  const [cursor, setCursor] = useState<{ x: number; y: number } | null>(null);
  const [ripple, setRipple] = useState<Ripple | null>(null);
  const [keycast, setKeycast] = useState<Keycast | null>(null);
  const [active, setActive] = useState(false);
  const cursorRef = useRef<{ x: number; y: number } | null>(null);
  const activeSessions = useRef<Set<string>>(new Set());
  const seqRef = useRef(0);
  const hideTimer = useRef<number | undefined>(undefined);
  const sizedRef = useRef(false);
  const mapRef = useRef({ sx: 1, sy: 1, dx: 0, dy: 0 });

  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    const win = getCurrentWindow();
    win.setIgnoreCursorEvents(true).catch(() => {});

    const fitToScreen = async () => {
      if (sizedRef.current) return;
      const mon =
        (await primaryMonitor().catch(() => null)) ?? (await currentMonitor().catch(() => null));
      if (!mon) return;
      // Top stays below the menu bar: macOS shifts any window overlapping it.
      const top = mon.workArea.position.y;
      await win.setPosition(new PhysicalPosition(mon.position.x, top));
      await win.setSize(
        new PhysicalSize(mon.size.width, mon.position.y + mon.size.height - top)
      );
      const f = mon.scaleFactor || 1;
      const s = Math.max(mon.size.width / f / API_WIDTH, mon.size.height / f / API_HEIGHT, 1);
      mapRef.current = { sx: s, sy: s, dx: 0, dy: (top - mon.position.y) / f };
      sizedRef.current = true;
    };
    fitToScreen().catch(() => {});

    const wake = () => {
      fitToScreen()
        .then(() => win.show())
        .catch(() => {});
      window.clearTimeout(hideTimer.current);
      hideTimer.current = window.setTimeout(() => {
        setKeycast(null);
        setRipple(null);
      }, IDLE_HIDE_MS);
    };

    const endSession = (sessionId: string) => {
      if (!activeSessions.current.delete(sessionId)) return;
      if (activeSessions.current.size > 0) return;
      setActive(false);
      setCursor(null);
      setKeycast(null);
      setRipple(null);
      cursorRef.current = null;
      win.hide().catch(() => {});
    };

    const unlisten = listen<{ sessionId: string; event: AgentEvent }>("agent-event", (e) => {
      const ev = e.payload.event;
      if (ev.kind === "Done" || ev.kind === "Cancelled" || ev.kind === "AgentError") {
        endSession(e.payload.sessionId);
        return;
      }
      if (ev.kind !== "AssistantMessage") return;
      for (const tc of ev.tool_calls) {
        if (COMPUTER_USE_TOOLS.has(tc.name) && !activeSessions.current.has(e.payload.sessionId)) {
          activeSessions.current.add(e.payload.sessionId);
          setActive(true);
        }
        const action = overlayAction(tc);
        if (!action) continue;
        wake();
        if (action.kind === "type") {
          setKeycast({ text: action.text, seq: seqRef.current++ });
          continue;
        }
        const m = mapRef.current;
        const target =
          action.x !== null && action.y !== null
            ? { x: action.x * m.sx - m.dx, y: action.y * m.sy - m.dy }
            : cursorRef.current;
        if (!target) continue;
        cursorRef.current = target;
        setCursor(target);
        if (action.kind === "click") {
          setRipple({ ...target, seq: seqRef.current++ });
        }
      }
    });
    return () => {
      unlisten.then((f) => f());
      window.clearTimeout(hideTimer.current);
    };
  }, []);

  return (
    <>
      <style>{STYLE}</style>
      {active && <div id="frame" />}
      {cursor && <div id="cursor" style={{ left: cursor.x, top: cursor.y }} />}
      {ripple && <div key={ripple.seq} className="ripple" style={{ left: ripple.x, top: ripple.y }} />}
      {keycast && (
        <div key={keycast.seq} id="keycast">
          {keycast.text}
        </div>
      )}
    </>
  );
}
