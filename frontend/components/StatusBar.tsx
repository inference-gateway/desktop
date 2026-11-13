import { Fragment, useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { useNow } from "@/hooks/useNow";
import { formatElapsed } from "@/lib/elapsed";
import { delegationSessionId, type Delegation } from "@/lib/transcript";
import { useChatStream, useDesktop } from "@/store";
import { useSidebar } from "@/stores/sidebar";
import { useBackend } from "@/stores/backend";
import { useStatus, useStatusActions } from "@/stores/status";
import { ClipboardList, Scale, ShieldCheck, Zap, type LucideIcon } from "lucide-react";
import { MODE_DESCRIPTIONS, MODE_LABELS, nextMode, type AgentMode } from "@/lib/agent-mode";
import { ArtifactsButton } from "./ArtifactsButton";
import { BrowserBridgeDot } from "./BrowserBridgeDot";
import { ShellOutputDialog, type ViewedShell } from "./ShellOutputDialog";

const DOT: Record<string, string> = {
  error: "bg-destructive",
  awaiting: "animate-pulse bg-amber-500",
  running: "animate-pulse bg-primary",
  ready: "bg-emerald-500",
  stopped: "bg-red-500",
  idle: "bg-muted-foreground",
};

const BADGE: Record<Delegation["kind"], string> = { a2a: "A2A", subagent: "agent", shell: "shell", recording: "rec" };

const formatCost = (cost: number) => `$${cost.toFixed(cost < 0.01 ? 4 : cost < 1 ? 3 : 2)}`;

const MODE_ICONS: Record<AgentMode, LucideIcon> = {
  standard: ShieldCheck,
  plan: ClipboardList,
  auto: Zap,
  "auto-with-judge": Scale,
};

export function StatusBar() {
  const { ready, tools, mcpStatus, a2aStatus } = useBackend();
  const { projects } = useSidebar();
  const { runLabel, delegations, runningTasks, tokenUsage } = useChatStream();
  const {
    running,
    isAwaitingApproval,
    isRunning,
    sessionId,
    conversations,
    openConversation,
    runningCount,
    agentMode,
    setAgentMode,
    turnStartedAt,
    elapsedMs,
    showStatusBar,
  } = useDesktop();
  const { statusText, statusError } = useStatus();
  const { clearError } = useStatusActions();
  const now = useNow(turnStartedAt !== null);
  const sessionMs = turnStartedAt !== null ? now - turnStartedAt : elapsedMs;
  const [open, setOpen] = useState(false);
  const [viewedShell, setViewedShell] = useState<ViewedShell | null>(null);
  const dropdownRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: MouseEvent) => {
      if (!dropdownRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  const agents = conversations
    .map((c) => ({ ...c, status: runLabel(c.id) }))
    .filter((c): c is typeof c & { status: { label: string; error: boolean } } => c.status != null)
    .map((c) => ({
      id: c.id,
      title: c.title || "Orchestrator",
      status: c.status,
      delegations: delegations(c.id),
      tone: c.status.error
        ? "error"
        : isAwaitingApproval(c.id)
          ? "awaiting"
          : isRunning(c.id)
            ? "running"
            : c.status.label === "Stopped"
              ? "stopped"
              : "ready",
    }));

  const session = !statusError && sessionId ? runLabel(sessionId) : null;
  const sessionDelegations = sessionId ? delegations(sessionId) : [];
  const sessionTasks = sessionId ? runningTasks(sessionId) : 0;
  const label =
    session?.label === "Running Agent..." && sessionDelegations.length > 0
      ? `Running Agent (${sessionDelegations.length})...`
      : (session?.label ?? statusText);
  const isError = session ? session.error : statusError;
  const tone = isError
    ? "error"
    : sessionId && isAwaitingApproval(sessionId)
      ? "awaiting"
      : running
        ? "running"
        : session?.label === "Stopped"
          ? "stopped"
          : ready
            ? "ready"
            : "idle";
  const ModeIcon = MODE_ICONS[agentMode];
  const contextPct =
    tokenUsage.context_window > 0 ? Math.round((tokenUsage.last_input / tokenUsage.context_window) * 100) : null;
  const stats: [string, string][] = [
    ["Tools", tools.length.toLocaleString()],
    ...(mcpStatus?.enabled && mcpStatus.total_servers > 0
      ? [
          [
            "🔌",
            `${mcpStatus.connected_servers}/${mcpStatus.total_servers}${mcpStatus.total_tools > 0 ? ` (${mcpStatus.total_tools})` : ""}`,
          ] as [string, string],
        ]
      : []),
    ...(a2aStatus && a2aStatus.total_agents > 0
      ? [["A2A", `${a2aStatus.ready_agents}/${a2aStatus.total_agents}`] as [string, string]]
      : []),
    ...(sessionTasks > 0 ? [["Tasks", String(sessionTasks)] as [string, string]] : []),
    ["in", tokenUsage.input.toLocaleString()],
    ["out", tokenUsage.output.toLocaleString()],
    ["cached", tokenUsage.cached_read.toLocaleString()],
    ["tool calls", tokenUsage.total_tool_calls.toLocaleString()],
  ];
  if (contextPct !== null) stats.unshift(["Context", `${contextPct}%`]);
  if (tokenUsage.cost > 0) stats.push(["cost", formatCost(tokenUsage.cost)]);
  if (sessionMs !== null) stats.push(["time", formatElapsed(sessionMs)]);
  if (!showStatusBar) return null;
  return (
    <div
      id="status-bar"
      className="mx-auto mb-[0.45rem] flex max-w-[52rem] flex-col gap-0.5 px-1 text-[0.72rem] font-medium"
    >
      <div className="flex items-center gap-[0.4rem]">
        <div ref={dropdownRef} className="relative min-w-0" role="status" aria-live="polite">
          {label && (
            <button
              aria-expanded={open}
              aria-haspopup="menu"
              aria-label="Agent status"
              onClick={() => setOpen((v) => !v)}
              className="flex max-w-full items-center gap-[0.4rem] whitespace-nowrap rounded px-1 py-0.5 hover:bg-secondary"
            >
              <span className={cn("h-[0.45rem] w-[0.45rem] shrink-0 rounded-full", DOT[tone])} />
              <span className={cn("truncate", isError ? "text-err" : "text-muted-foreground")}>{label}</span>
              {statusError && (
                <span
                  role="button"
                  aria-label="Dismiss error"
                  title="Dismiss"
                  onClick={(e) => {
                    e.stopPropagation();
                    clearError();
                  }}
                  className="ml-1 rounded px-1 text-muted-foreground hover:bg-secondary hover:text-foreground"
                >
                  ×
                </span>
              )}
              {runningCount > 0 && (
                <span
                  title={`${runningCount} orchestrator${runningCount === 1 ? "" : "s"} running`}
                  className="min-w-[1.1rem] rounded-full bg-primary/15 px-1 text-center text-[0.65rem] font-semibold text-primary"
                >
                  {runningCount}
                </span>
              )}
            </button>
          )}
          {open && (
            <div
              role="menu"
              aria-label="Agents"
              className="absolute bottom-full left-0 z-50 mb-1 min-w-[18rem] max-w-[26rem] rounded-md border border-border bg-popover p-1 shadow-md"
            >
              {agents.length === 0 && <div className="px-2 py-1.5 text-muted-foreground">No orchestrators running</div>}
              {agents.map((a) => (
                <Fragment key={a.id}>
                  <button
                    role="menuitem"
                    onClick={() => {
                      setOpen(false);
                      openConversation(a.id);
                    }}
                    className={cn(
                      "flex w-full items-center gap-[0.4rem] rounded px-2 py-1.5 text-left hover:bg-secondary",
                      a.id === sessionId && "bg-secondary/60",
                    )}
                  >
                    <span className={cn("h-[0.45rem] w-[0.45rem] shrink-0 rounded-full", DOT[a.tone])} />
                    <span className="truncate text-foreground">{a.title}</span>
                    <span className="shrink-0 font-mono text-[0.68rem] text-muted-foreground/60">
                      {a.id.slice(0, 5)}
                    </span>
                    <span
                      className={cn("ml-auto shrink-0 pl-3", a.status.error ? "text-err" : "text-muted-foreground")}
                    >
                      {a.status.label}
                    </span>
                  </button>
                  {a.delegations.map((d) => (
                    <button
                      key={d.id}
                      role="menuitem"
                      aria-label={
                        d.kind === "shell" ? `View shell ${d.label}` : `${BADGE[d.kind]} ${d.label} under ${a.title}`
                      }
                      onClick={() => {
                        setOpen(false);
                        if (d.kind === "shell") {
                          setViewedShell({
                            sessionId: a.id,
                            project: projects[a.id],
                            jobId: d.id.slice("job:".length),
                          });
                          return;
                        }
                        openConversation(delegationSessionId(a.id, d, conversations) ?? a.id);
                      }}
                      className="flex w-full items-center gap-[0.4rem] rounded py-1 pl-6 pr-2 text-left hover:bg-secondary"
                    >
                      <span className={cn("h-[0.35rem] w-[0.35rem] shrink-0 rounded-full", DOT.running)} />
                      <span className="truncate text-muted-foreground">{d.label}</span>
                      <span className="ml-auto shrink-0 rounded-full bg-secondary px-1.5 text-[0.6rem] font-semibold uppercase tracking-wide text-muted-foreground/80">
                        {BADGE[d.kind]}
                      </span>
                    </button>
                  ))}
                </Fragment>
              ))}
            </div>
          )}
        </div>
        <span className="ml-auto inline-flex shrink-0 items-center gap-1">
          <BrowserBridgeDot />
          <ArtifactsButton />
          <button
            aria-label={MODE_DESCRIPTIONS[agentMode]}
            title={`${MODE_DESCRIPTIONS[agentMode]} - click or press Shift+Tab in the composer for ${MODE_LABELS[nextMode(agentMode)]}`}
            onClick={() => setAgentMode(nextMode(agentMode))}
            className={cn(
              "inline-flex h-7 min-w-7 items-center justify-center gap-1 rounded-full px-1.5 text-[0.75rem] font-medium",
              agentMode === "standard"
                ? "text-muted-foreground hover:bg-secondary hover:text-foreground"
                : "bg-primary text-primary-foreground hover:bg-primary-hover",
            )}
          >
            <ModeIcon size={16} aria-hidden="true" />
            {agentMode !== "standard" && <span aria-hidden="true">{MODE_LABELS[agentMode]}</span>}
          </button>
        </span>
      </div>
      <span className="flex flex-wrap items-center gap-x-1 px-1 tabular-nums text-muted-foreground">
        {stats.map(([name, n], i) => (
          <Fragment key={name}>
            {i > 0 && <span className="opacity-40">&middot;</span>}
            <span className="whitespace-nowrap">
              {name}: {n}
            </span>
          </Fragment>
        ))}
      </span>
      <ShellOutputDialog shell={viewedShell} onClose={() => setViewedShell(null)} />
    </div>
  );
}
