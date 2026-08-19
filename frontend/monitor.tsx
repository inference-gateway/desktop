// Compact always-on-top monitor window for computer-use sessions. Fed by the
// main window's "agent-event" re-broadcast; invokes session-keyed backend
// commands directly. Docks itself to the bottom-right screen edge on launch.
import { useEffect, useReducer, useState } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow, currentMonitor, PhysicalPosition } from "@tauri-apps/api/window";
import { Pause, Play, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/tauri";
import {
  monitorReducer,
  resolveApproval,
  type MonitorEvent,
  type MonitorSession,
  type MonitorState,
} from "@/lib/monitor-state";

const EDGE_MARGIN = 16;

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

  useEffect(() => {
    dockBottomRight().catch(() => {});
    const unlisten = listen<MonitorEvent>("agent-event", (e) =>
      dispatch({ type: "event", msg: e.payload })
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const ids = Object.keys(sessions);
  const id = selectedId && sessions[selectedId] ? selectedId : ids[0] ?? null;
  const session = id ? sessions[id] : null;

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

  if (!session) {
    return (
      <div className="flex h-screen items-center justify-center bg-background p-4 text-center text-sm text-muted-foreground">
        No active computer-use session
      </div>
    );
  }

  const status = STATUS_STYLE[session.status];

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
        <span className="truncate font-medium" title={session.name}>
          {session.name || id}
        </span>
      </div>
      <div className="text-xs text-muted-foreground">{status.label}</div>
      <div className="min-h-0 flex-1 overflow-hidden rounded-md border bg-muted/40 px-2 py-1.5 font-mono text-xs text-muted-foreground">
        {session.lastAction ?? "Waiting for actions..."}
      </div>
      {session.pendingApproval && (
        <div className="flex items-center gap-2 rounded-md border border-orange-500/40 bg-orange-500/10 px-2 py-1.5">
          <span className="min-w-0 flex-1 truncate text-xs font-medium">
            Allow {session.pendingApproval.toolName}?
          </span>
          <Button size="sm" className="h-6 px-2 text-xs" onClick={() => decide(true)}>
            Approve
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="h-6 px-2 text-xs"
            onClick={() => decide(false)}
          >
            Deny
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
    </div>
  );
}
