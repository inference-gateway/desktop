// Compact always-on-top monitor window for computer-use sessions. Fed by the
// backend's global "agent-event" broadcast; invokes session-keyed backend
// commands directly. Docks itself to the top-center screen edge on launch.
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
import {
  EyeOff,
  GripHorizontal,
  Maximize2,
  Minimize2,
  Pause,
  Play,
  ShieldAlert,
  Square,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/tauri";
import { autoGrow } from "@/lib/textarea";
import { prettyJson, safeImageSrc } from "@/lib/tools";
import { COMPUTER_USE_TOOLS } from "@/lib/transcript";
import {
  monitorReducer,
  resolveApproval,
  type MonitorEvent,
  type MonitorSession,
  type MonitorState,
} from "@/lib/monitor-state";

const TOP_MARGIN = 16;
const COMPACT = { width: 760, height: 300 };
const EXPANDED = { width: 840, height: 540 };

async function dockTopCenter() {
  const mon = await currentMonitor();
  if (!mon) return;
  const win = getCurrentWindow();
  const size = await win.outerSize();
  await win.setPosition(
    new PhysicalPosition(
      mon.workArea.position.x + Math.round((mon.workArea.size.width - size.width) / 2),
      mon.workArea.position.y + TOP_MARGIN
    )
  );
}

async function resizeAndDock(size: { width: number; height: number }) {
  await getCurrentWindow().setSize(new LogicalSize(size.width, size.height));
  await dockTopCenter();
}

async function hideForComputerAction() {
  const win = getCurrentWindow();
  await win.setIgnoreCursorEvents(true);
  await win.hide();
}

async function showInteractive() {
  const win = getCurrentWindow();
  await win.setIgnoreCursorEvents(false);
  await win.show();
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
  const computerCallsRef = useRef<Map<string, string>>(new Map());
  const computerSessionsRef = useRef<Set<string>>(new Set());
  const manuallyHiddenRef = useRef(false);

  useEffect(() => {
    dockTopCenter().catch(() => {});
    const unlisten = listen<MonitorEvent>("agent-event", (e) => {
      const msg = e.payload;
      const event = msg.event;
      dispatch({ type: "event", msg });
      if (event.kind === "AssistantMessage") {
        const calls = event.tool_calls.filter((call) => COMPUTER_USE_TOOLS.has(call.name));
        if (calls.length > 0) {
          if (!computerSessionsRef.current.has(msg.sessionId)) manuallyHiddenRef.current = false;
          computerSessionsRef.current.add(msg.sessionId);
          for (const call of calls) computerCallsRef.current.set(call.id, msg.sessionId);
          hideForComputerAction().catch(() => {});
        }
        return;
      }
      if (event.kind === "ApprovalRequest" && COMPUTER_USE_TOOLS.has(event.tool_name)) {
        computerSessionsRef.current.add(msg.sessionId);
        manuallyHiddenRef.current = false;
        showInteractive().catch(() => {});
        return;
      }
      if (event.kind === "ToolResult" && computerCallsRef.current.delete(event.tool_call_id)) {
        if (!manuallyHiddenRef.current) showInteractive().catch(() => {});
        return;
      }
      if (event.kind === "ComputerUsePaused" || event.kind === "ComputerUseResumed") {
        showInteractive().catch(() => {});
        return;
      }
      if (event.kind === "Done" || event.kind === "Cancelled" || event.kind === "AgentError") {
        if (!computerSessionsRef.current.delete(msg.sessionId)) return;
        for (const [callId, sessionId] of computerCallsRef.current) {
          if (sessionId === msg.sessionId) computerCallsRef.current.delete(callId);
        }
        if (!manuallyHiddenRef.current) showInteractive().catch(() => {});
      }
    });
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

  const decide = async (approved: boolean) => {
    if (!id || !session?.pendingApproval) return;
    const { callId } = session.pendingApproval;
    if (approved) await hideForComputerAction().catch(() => {});
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
      <div className="m-1 flex h-[calc(100vh-0.5rem)] w-[calc(100vw-0.5rem)] items-center justify-center rounded-2xl border-2 border-primary/70 bg-background/85 p-4 text-center text-sm text-muted-foreground shadow-[0_18px_60px_rgba(79,70,229,0.3)] backdrop-blur-xl">
        No active computer-use session
      </div>
    );
  }

  const status = STATUS_STYLE[session.status];
  const frameSrc = session.lastFrame?.startsWith("data:")
    ? session.lastFrame
    : safeImageSrc(session.lastFrame);

  return (
    <div className="m-1 flex h-[calc(100vh-0.5rem)] w-[calc(100vw-0.5rem)] flex-col gap-2.5 overflow-hidden rounded-2xl border-2 border-primary/70 bg-background/85 p-3 text-sm text-foreground shadow-[0_18px_60px_rgba(79,70,229,0.3)] backdrop-blur-xl">
      {ids.length > 1 && (
        <select
          id="monitor-session-select"
          aria-label="Session"
          className="w-full rounded-md border border-primary/30 bg-background/70 px-2 py-1 text-xs"
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
        <Button
          size="sm"
          variant="ghost"
          className="size-6 shrink-0 cursor-grab p-0 active:cursor-grabbing"
          aria-label="Move monitor"
          title="Drag to move"
          onMouseDown={(event) => {
            if (event.button === 0) getCurrentWindow().startDragging().catch(() => {});
          }}
        >
          <GripHorizontal className="size-4" />
        </Button>
        <span className={`size-2 shrink-0 rounded-full ${status.dot}`} />
        <span className="min-w-0 flex-1 truncate font-medium" title={session.name}>
          {session.name || id}
        </span>
        <span className="flex shrink-0 items-center gap-1 rounded-full border border-primary/35 bg-primary/10 px-2 py-1 text-[0.68rem] font-medium text-primary">
          <EyeOff className="size-3" /> Hidden from agent capture
        </span>
        <span className="shrink-0 text-xs text-muted-foreground">{status.label}</span>
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
          onClick={() => {
            manuallyHiddenRef.current = true;
            getCurrentWindow().hide().catch(() => {});
          }}
        >
          <X className="size-3.5" />
        </Button>
      </div>
      {session.pendingApproval && (
        <div className="grid shrink-0 grid-cols-[minmax(0,1fr)_auto] gap-3 rounded-xl border border-primary/60 bg-primary/10 p-3 shadow-[0_0_28px_rgba(99,102,241,0.18)]">
          <div className="min-w-0">
            <div className="mb-1.5 flex items-center gap-2 text-xs font-semibold text-primary">
              <ShieldAlert className="size-4" />
              Approval required
              <span className="rounded bg-primary/15 px-1.5 py-0.5 font-mono text-[0.68rem]">
                {session.pendingApproval.toolName}
              </span>
            </div>
            <pre
              aria-label={`Arguments for ${session.pendingApproval.toolName}`}
              className={`${expanded ? "max-h-48" : "max-h-24"} overflow-auto whitespace-pre-wrap break-all rounded-lg border border-primary/25 bg-background/70 px-2.5 py-2 font-mono text-xs leading-relaxed text-foreground`}
            >
              {prettyJson(session.pendingApproval.toolArgs) || "{}"}
            </pre>
          </div>
          <div className="flex min-w-28 flex-col justify-center gap-2">
            <Button
              size="sm"
              aria-label="Approve"
              className="h-9 justify-between gap-3 px-3 text-xs"
              onClick={() => decide(true)}
            >
              Approve
              <kbd
                aria-hidden="true"
                className="rounded border border-white/35 bg-white/15 px-1.5 py-0.5 font-mono text-[0.68rem]"
              >
                A
              </kbd>
            </Button>
            <Button
              size="sm"
              variant="outline"
              aria-label="Deny"
              className="h-9 justify-between gap-3 border-destructive/40 bg-background/60 px-3 text-xs text-destructive hover:bg-destructive/10 hover:text-destructive"
              onClick={() => decide(false)}
            >
              Deny
              <kbd
                aria-hidden="true"
                className="rounded border border-current/30 bg-destructive/10 px-1.5 py-0.5 font-mono text-[0.68rem]"
              >
                R
              </kbd>
            </Button>
          </div>
        </div>
      )}
      <div className="flex min-h-0 flex-1 gap-2">
        {frameSrc && (
          <img
            src={frameSrc}
            alt="Latest captured frame"
            className={`${expanded ? "w-64" : "w-44"} shrink-0 rounded-lg border border-primary/25 bg-background/50 object-contain`}
          />
        )}
        <div
          ref={logRef}
          className="min-h-0 flex-1 overflow-y-auto whitespace-pre-wrap break-words rounded-lg border border-primary/20 bg-muted/35 px-2.5 py-2 font-mono text-xs text-muted-foreground"
        >
          {session.log.length ? session.log.join("\n") : "Waiting for actions..."}
        </div>
      </div>
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
