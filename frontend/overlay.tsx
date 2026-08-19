// Fullscreen, transparent, click-through overlay that visualizes computer-use
// actions: a cursor dot glides to each MouseMove/MouseScroll target, a ring
// ripples on MouseClick, and a key-cast pill at the bottom shows what the
// agent is typing. Fed by the main window's "agent-event" re-broadcast; all
// animation is CSS inside this webview, so no per-frame IPC.
import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import {
  getCurrentWindow,
  currentMonitor,
  PhysicalPosition,
  PhysicalSize,
} from "@tauri-apps/api/window";
import type { AgentEvent } from "@/lib/tauri";
import { overlayAction } from "@/lib/pointer";

const IDLE_HIDE_MS = 1600;
const ACCENT = "99, 102, 241";

const STYLE = `
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

function Overlay() {
  const [cursor, setCursor] = useState<{ x: number; y: number } | null>(null);
  const [ripple, setRipple] = useState<Ripple | null>(null);
  const [keycast, setKeycast] = useState<Keycast | null>(null);
  const cursorRef = useRef<{ x: number; y: number } | null>(null);
  const seqRef = useRef(0);
  const hideTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    const win = getCurrentWindow();
    win.setIgnoreCursorEvents(true).catch(() => {});
    currentMonitor()
      .then(async (mon) => {
        if (!mon) return;
        await win.setPosition(new PhysicalPosition(mon.position.x, mon.position.y));
        await win.setSize(new PhysicalSize(mon.size.width, mon.size.height));
      })
      .catch(() => {});

    const wake = () => {
      win.show().catch(() => {});
      window.clearTimeout(hideTimer.current);
      hideTimer.current = window.setTimeout(() => {
        win.hide().catch(() => {});
        setKeycast(null);
        setRipple(null);
      }, IDLE_HIDE_MS);
    };

    const unlisten = listen<{ sessionId: string; event: AgentEvent }>("agent-event", (e) => {
      const ev = e.payload.event;
      if (ev.kind !== "AssistantMessage") return;
      for (const tc of ev.tool_calls) {
        const action = overlayAction(tc);
        if (!action) continue;
        wake();
        if (action.kind === "type") {
          setKeycast({ text: action.text, seq: seqRef.current++ });
          continue;
        }
        const target =
          action.x !== null && action.y !== null
            ? { x: action.x, y: action.y }
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

const el = document.getElementById("overlay");
if (el) createRoot(el).render(<Overlay />);
