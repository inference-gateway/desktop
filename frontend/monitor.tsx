// Compact always-on-top monitor window for computer-use sessions. Fed by the
// backend's global "agent-event" broadcast; invokes session-keyed backend
// commands directly. Docks itself to the bottom-right screen edge on launch.
// Expanding grows the window and adds a composer that round-trips follow-up
// prompts through the main window via the "monitor-send" event, so the main
// transcript stays the single source of truth.
import { useEffect, useReducer, useRef, useState } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import {
  getCurrentWindow,
  currentMonitor,
  LogicalSize,
  PhysicalPosition,
} from "@tauri-apps/api/window";
import { register, unregister } from "@tauri-apps/plugin-global-shortcut";
import { Maximize2, Minimize2, Pause, Play, Square, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/tauri";
import { autoGrow } from "@/lib/textarea";
import { safeImageSrc } from "@/lib/tools";
import {
  monitorReducer,
  resolveApproval,
  type MonitorEvent,
  type MonitorSession,
  type MonitorState,
} from "@/lib/monitor-state";

const EDGE_MARGIN = 16;
const COMPACT = { width: 320, height: 220 };
const EXPANDED = { width: 460, height: 480 };

async function dockBottomRight() {
  const mon = await currentMonitor();
  if (!mon) return;
  const win = getCurrentWindow();
  const size = await win.outerSize();
  await win.setPosition(
    new PhysicalPosition(
      mon.position.x + mon.size.width - size.width - EDGE_MARGIN,
      mon.position.y + mon.size.height - size.height - EDGE_MARGIN
    )
  );
}

async function resizeAndDock(size: { width: number; height: number }) {
  await getCurrentWindow().setSize(new LogicalSize(size.width, size.height));
  await dockBottomRight();
}

const STATUS_STYLE: Record<MonitorSession["status"], { label: string; dot: string }> = {
  running: { label: "Running", dot: "bg-green-500 animate-pulse" },
  paused: { label: "Paused", dot: "bg-yellow-500" },
  awaiting: { label: "Awaiting approval", dot: "bg-orange-500 animate-pulse" },
  done: { label: "Finished", dot: "bg-muted-foreground" },
};

type Action = { type: "event"; msg: MonitorEvent } | { type: "resolved"; sessionId: string };

function reducer(state: MonitorState, action: Action): MonitorState {
  return action.type === "event"
    ? monitorReducer(state, action.msg)
    : resolveApproval(state, action.sessionId);
}

export default function Monitor() {
  const [sessions, dispatch] = useReducer(reducer, {});
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const logRef = useRef<HTMLDivElement | null>(null);
  const promptRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    dockBottomRight().catch(() => {});
    const unlisten = listen<MonitorEvent>("agent-event", (e) =>
      dispatch({ type: "event", msg: e.payload })
    );
    const unlistenResolved = listen<{ sessionId: string }>("approval-resolved", (e) =>
      dispatch({ type: "resolved", sessionId: e.payload.sessionId })
    );
    return () => {
      unlisten.then((f) => f());
      unlistenResolved.then((f) => f());
    };
  }, []);

  const ids = Object.keys(sessions);
  const id = selectedId && sessions[selectedId] ? selectedId : ids[0] ?? null;
  const session = id ? sessions[id] : null;

  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [session?.log, session?.lastFrame]);

  const toggleExpanded = () => {
    const next = !expanded;
    setExpanded(next);
    resizeAndDock(next ? EXPANDED : COMPACT).catch(() => {});
  };

  const sendInstruction = () => {
    const el = promptRef.current;
    const text = el?.value.trim() ?? "";
    if (!text || !id) return;
    emit("monitor-send", { sessionId: id, text }).catch(() => {});
    if (el) {
      el.value = "";
      autoGrow(el);
    }
  };

  const decide = (approved: boolean) => {
    if (!id || !session?.pendingApproval) return;
    const { callId } = session.pendingApproval;
    api.sendApproval(id, callId, approved).catch(() => {});
    emit("approval-resolved", {
      sessionId: id,
      callId,
      status: approved ? "approved" : "denied",
    }).catch(() => {});
    dispatch({ type: "resolved", sessionId: id });
  };
  const decideRef = useRef(decide);
  decideRef.current = decide;

  const hasPending = Boolean(session?.pendingApproval);
  useEffect(() => {
    if (!hasPending) return;
    register("A", (e) => {
      if (e.state === "Pressed") decideRef.current(true);
    }).catch(() => {});
    register("R", (e) => {
      if (e.state === "Pressed") decideRef.current(false);
    }).catch(() => {});
    return () => {
      unregister(["A", "R"]).catch(() => {});
    };
  }, [hasPending]);

  if (!session) {
    return (
      <div className="flex h-screen items-center justify-center bg-background p-4 text-center text-sm text-muted-foreground">
        No active computer-use session
      </div>
    );
  }

  const status = STATUS_STYLE[session.status];
  const frameSrc = session.lastFrame?.startsWith("data:")
    ? session.lastFrame
    : safeImageSrc(session.lastFrame);

  return (
    <div className="flex h-screen flex-col gap-2.5 bg-background p-3 text-sm text-foreground">
      {ids.length > 1 && (
        <select
          id="monitor-session-select"
          aria-label="Session"
          className="w-full rounded-md border border-input bg-background px-2 py-1 text-xs"
          value={id ?? ""}
          onChange={(e) => setSelectedId(e.target.value)}
        >
          {ids.map((sid) => (
            <option key={sid} value={sid}>
              {sessions[sid].name.slice(0, 40) || sid.slice(0, 8)}
            </option>
          ))}
        </select>
      )}
      <div className="flex items-center gap-2">
        <span className={`size-2 shrink-0 rounded-full ${status.dot}`} />
        <span className="min-w-0 flex-1 truncate font-medium" title={session.name}>
          {session.name || id}
        </span>
        <Button
          size="sm"
          variant="ghost"
          className="size-6 shrink-0 p-0"
          aria-label={expanded ? "Collapse monitor" : "Expand monitor"}
          onClick={toggleExpanded}
        >
          {expanded ? <Minimize2 className="size-3.5" /> : <Maximize2 className="size-3.5" />}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          className="size-6 shrink-0 p-0"
          aria-label="Close monitor"
          onClick={() => getCurrentWindow().hide().catch(() => {})}
        >
          <X className="size-3.5" />
        </Button>
      </div>
      <div className="text-xs text-muted-foreground">{status.label}</div>
      {frameSrc && (
        <img
          src={frameSrc}
          alt="Latest captured frame"
          className={`w-full shrink-0 rounded-md border object-contain ${expanded ? "max-h-36" : "max-h-20"}`}
        />
      )}
      <div
        ref={logRef}
        className="min-h-0 flex-1 overflow-y-auto whitespace-pre-wrap break-words rounded-md border bg-muted/40 px-2 py-1.5 font-mono text-xs text-muted-foreground"
      >
        {session.log.length ? session.log.join("\n") : "Waiting for actions..."}
      </div>
      {session.pendingApproval && (
        <div className="flex items-center gap-2 rounded-md border border-orange-500/40 bg-orange-500/10 px-2 py-1.5">
          <span className="min-w-0 flex-1 truncate text-xs font-medium">
            Allow {session.pendingApproval.toolName}?
          </span>
          <Button size="sm" className="h-6 px-2 text-xs" onClick={() => decide(true)}>
            Approve (A)
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="h-6 px-2 text-xs"
            onClick={() => decide(false)}
          >
            Deny (R)
          </Button>
        </div>
      )}
      <div className="flex gap-2">
        {session.status === "paused" ? (
          <Button
            size="sm"
            variant="outline"
            className="h-7 flex-1 text-xs"
            aria-label="Resume"
            onClick={() => id && api.sendComputerUseControl(id, "resume").catch(() => {})}
          >
            <Play className="size-3.5" /> Resume
          </Button>
        ) : (
          <Button
            size="sm"
            variant="outline"
            className="h-7 flex-1 text-xs"
            aria-label="Pause"
            disabled={session.status === "done"}
            onClick={() => id && api.sendComputerUseControl(id, "pause").catch(() => {})}
          >
            <Pause className="size-3.5" /> Pause
          </Button>
        )}
        <Button
          size="sm"
          variant="outline"
          className="h-7 flex-1 text-xs text-destructive hover:text-destructive"
          aria-label="Stop session"
          disabled={session.status === "done"}
          onClick={() => id && api.cancelAgent(id).catch(() => {})}
        >
          <Square className="size-3.5" /> Stop
        </Button>
      </div>
      {expanded && (
        <textarea
          ref={promptRef}
          id="monitor-prompt-input"
          aria-label="Instruct the agent"
          rows={2}
          disabled={session.status !== "done"}
          placeholder={
            session.status === "done" ? "Instruct the agent..." : "Agent is running..."
          }
          className="max-h-24 w-full resize-none rounded-md border border-input bg-background px-2 py-1.5 text-xs outline-none placeholder:text-muted-foreground focus:ring-1 focus:ring-ring disabled:opacity-50"
          onInput={(e) => autoGrow(e.currentTarget)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              sendInstruction();
            }
          }}
        />
      )}
    </div>
  );
}
