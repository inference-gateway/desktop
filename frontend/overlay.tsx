// Fullscreen, transparent, click-through overlay that visualizes computer-use
// actions: a rounded border glows at the screen edges while any computer-use
// session is active, a cursor dot glides to each Computer move/scroll target,
// a ring ripples on a Computer click, and a key-cast pill at the bottom shows what
// the agent is typing or which non-pointer tool (screenshot, WebSearch, ...) it
// is waiting on, so an unfocused user still sees activity. Fed by the backend's global "agent-event" broadcast;
// all animation is CSS inside this webview, so no per-frame IPC. One Frame
// draws three variants: blue at the screen edges for computer use, red at the
// screen edges during workflow capture ("screen-recording" event from the top
// bar), and red just outside the recorded area of an agent recording
// (RecordStart; RecordingArea until RecordingStopped or the process ends).
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  getCurrentWindow,
  currentMonitor,
  primaryMonitor,
  PhysicalPosition,
  PhysicalSize,
} from "@tauri-apps/api/window";
import type { AgentEvent, ToolCallInfo } from "@/lib/tauri";
import { overlayAction } from "@/lib/pointer";
import { type OverlayRect, type OverlayScreen, recordingFrame } from "@/lib/recording";
import { COMPUTER_USE_TOOLS } from "@/lib/transcript";

const IDLE_HIDE_MS = 1600;
const ACCENT = "99, 102, 241";
const RECORD = "239, 68, 68";
const API_WIDTH = 1024;
const API_HEIGHT = 768;
const FRAME_BORDER = 3;

const STYLE = `
.frame {
  position: fixed;
  inset: 4px;
  box-sizing: border-box;
  border: ${FRAME_BORDER}px solid rgba(${ACCENT}, 0.75);
  border-radius: 14px;
  box-shadow: 0 0 18px rgba(${ACCENT}, 0.5), inset 0 0 24px rgba(${ACCENT}, 0.25);
  animation: breathe 3s ease-in-out infinite;
  pointer-events: none;
}
.frame.workflow-capture {
  border-color: rgba(${RECORD}, 0.75);
  box-shadow: 0 0 18px rgba(${RECORD}, 0.5), inset 0 0 24px rgba(${RECORD}, 0.25);
}
.frame.agent-recording {
  inset: auto;
  border-color: rgba(${RECORD}, 0.85);
  border-radius: 4px;
  box-shadow: 0 0 18px rgba(${RECORD}, 0.5);
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
type FrameVariant = "computer-use" | "workflow-capture" | "agent-recording";

/** A glowing border: at the screen edges without `rect`, around `rect` with it. */
function Frame({ variant, rect }: { variant: FrameVariant; rect?: OverlayRect }) {
  return <div className={`frame ${variant}`} style={rect} />;
}

function busyLabel(tc: ToolCallInfo): string {
  try {
    const action = JSON.parse(tc.args).action;
    return typeof action === "string" ? `${tc.name} · ${action}` : tc.name;
  } catch {
    return tc.name;
  }
}

export default function Overlay() {
  const [cursor, setCursor] = useState<{ x: number; y: number } | null>(null);
  const [ripple, setRipple] = useState<Ripple | null>(null);
  const [keycast, setKeycast] = useState<Keycast | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [active, setActive] = useState(false);
  const [recording, setRecording] = useState(false);
  const [agentFrame, setAgentFrame] = useState<OverlayRect | null>(null);
  const cursorRef = useRef<{ x: number; y: number } | null>(null);
  const activeSessions = useRef<Set<string>>(new Set());
  const seqRef = useRef(0);
  const hideTimer = useRef<number | undefined>(undefined);
  const sizedRef = useRef(false);
  const mapRef = useRef({ sx: 1, sy: 1, dx: 0, dy: 0 });
  const screenRef = useRef<OverlayScreen | null>(null);

  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    const win = getCurrentWindow();
    win.setIgnoreCursorEvents(true).catch(() => {});

    const fitToScreen = async () => {
      if (sizedRef.current) return;
      const mon = (await primaryMonitor().catch(() => null)) ?? (await currentMonitor().catch(() => null));
      if (!mon) return;
      const top = mon.workArea.position.y;
      await win.setSize(new PhysicalSize(mon.size.width, mon.position.y + mon.size.height - top));
      await win.setPosition(new PhysicalPosition(mon.position.x, top));
      const f = mon.scaleFactor || 1;
      const s = Math.max(mon.size.width / f / API_WIDTH, mon.size.height / f / API_HEIGHT, 1);
      const dy = (top - mon.position.y) / f;
      mapRef.current = { sx: s, sy: s, dx: 0, dy };
      screenRef.current = {
        width: mon.size.width / f,
        height: (mon.position.y + mon.size.height - top) / f,
        dy,
      };
      sizedRef.current = true;
    };
    fitToScreen().catch(() => {});

    const wake = () => {
      fitToScreen()
        .then(() => win.isVisible())
        .then((visible) => (visible ? undefined : win.show()))
        .catch(() => {});
      window.clearTimeout(hideTimer.current);
      hideTimer.current = window.setTimeout(() => {
        setKeycast(null);
        setRipple(null);
      }, IDLE_HIDE_MS);
    };

    let recordingNow = false;
    let agentRecordingOwner: string | null = null;
    const hideIfIdle = () => {
      if (activeSessions.current.size === 0 && !recordingNow && !agentRecordingOwner) win.hide().catch(() => {});
    };
    const endAgentRecording = (sessionId: string) => {
      if (agentRecordingOwner !== sessionId) return;
      agentRecordingOwner = null;
      setAgentFrame(null);
      hideIfIdle();
    };
    const endSession = (sessionId: string) => {
      endAgentRecording(sessionId);
      if (!activeSessions.current.delete(sessionId)) return;
      if (activeSessions.current.size > 0) return;
      setActive(false);
      setCursor(null);
      setKeycast(null);
      setBusy(null);
      setRipple(null);
      cursorRef.current = null;
      hideIfIdle();
    };

    const unlistenRecording = listen<{ recording: boolean }>("screen-recording", (e) => {
      recordingNow = e.payload.recording;
      setRecording(recordingNow);
      if (recordingNow) wake();
      else hideIfIdle();
    });

    const unlisten = listen<{ sessionId: string; event: AgentEvent }>("agent-event", (e) => {
      const ev = e.payload.event;
      if (ev.kind === "Done" || ev.kind === "Cancelled" || ev.kind === "AgentError") {
        endSession(e.payload.sessionId);
        return;
      }
      if (ev.kind === "RecordingStopped") {
        endAgentRecording(e.payload.sessionId);
        return;
      }
      if (ev.kind === "RecordingArea") {
        const sessionId = e.payload.sessionId;
        agentRecordingOwner = sessionId;
        fitToScreen()
          .then(() => {
            if (agentRecordingOwner !== sessionId || !screenRef.current) return;
            setAgentFrame(recordingFrame(ev.area, screenRef.current, FRAME_BORDER));
            wake();
          })
          .catch(() => {});
        return;
      }
      if (ev.kind === "ToolResult" && activeSessions.current.has(e.payload.sessionId)) {
        setBusy(null);
        return;
      }
      if (ev.kind !== "AssistantMessage") return;
      for (const tc of ev.tool_calls) {
        if (COMPUTER_USE_TOOLS.has(tc.name) && !activeSessions.current.has(e.payload.sessionId)) {
          activeSessions.current.add(e.payload.sessionId);
          setActive(true);
        }
        if (!activeSessions.current.has(e.payload.sessionId)) continue;
        const action = overlayAction(tc);
        if (!action) {
          wake();
          setBusy(busyLabel(tc));
          continue;
        }
        setBusy(null);
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
      unlistenRecording.then((f) => f());
      window.clearTimeout(hideTimer.current);
    };
  }, []);

  return (
    <>
      <style>{STYLE}</style>
      {(active || recording) && <Frame variant={recording ? "workflow-capture" : "computer-use"} />}
      {agentFrame && <Frame variant="agent-recording" rect={agentFrame} />}
      {cursor && <div id="cursor" style={{ left: cursor.x, top: cursor.y }} />}
      {ripple && <div key={ripple.seq} className="ripple" style={{ left: ripple.x, top: ripple.y }} />}
      {keycast ? (
        <div key={keycast.seq} id="keycast">
          {keycast.text}
        </div>
      ) : (
        busy && <div id="keycast">⟳ {busy}</div>
      )}
    </>
  );
}
