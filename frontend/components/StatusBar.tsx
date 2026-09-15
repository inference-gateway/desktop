import { Fragment, useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { Zap } from "lucide-react";
import { BrowserBridgeDot } from "./BrowserBridgeDot";

const DOT: Record<string, string> = {
  error: "bg-destructive",
  awaiting: "animate-pulse bg-amber-500",
  running: "animate-pulse bg-primary",
  ready: "bg-emerald-500",
  stopped: "bg-red-500",
  idle: "bg-muted-foreground",
};

const formatCost = (cost: number) => `$${cost.toFixed(cost < 0.01 ? 4 : cost < 1 ? 3 : 2)}`;

export function StatusBar() {
  const {
    statusText,
    statusError,
    running,
    ready,
    isAwaitingApproval,
    isRunning,
    runLabel,
    delegations,
    sessionId,
    conversations,
    openConversation,
    runningCount,
    autoMode,
    setAutoMode,
    tokenUsage,
    tools,
    mcpStatus,
    a2aStatus,
    showStatusBar,
  } = useDesktop();
  const [open, setOpen] = useState(false);
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
  const autoModeDescription = autoMode
    ? "Auto approval is on - tool actions run without asking"
    : "Auto approval is off - protected tool actions ask first";
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
    ["in", tokenUsage.input.toLocaleString()],
    ["out", tokenUsage.output.toLocaleString()],
    ["cached", tokenUsage.cached_read.toLocaleString()],
    ["tool calls", tokenUsage.total_tool_calls.toLocaleString()],
  ];
  if (contextPct !== null) stats.unshift(["Context", `${contextPct}%`]);
  if (tokenUsage.cost > 0) stats.push(["cost", formatCost(tokenUsage.cost)]);
  if (!showStatusBar) return null;
  return (
    <div
      id="status-bar"
      className="mx-auto mb-[0.45rem] flex max-w-[52rem] items-center gap-[0.4rem] px-1 text-[0.72rem] font-medium"
    >
      <div ref={dropdownRef} className="relative" role="status" aria-live="polite">
        {label && (
          <button
            aria-expanded={open}
            aria-haspopup="menu"
            aria-label="Agent status"
            onClick={() => setOpen((v) => !v)}
            className="flex items-center gap-[0.4rem] rounded px-1 py-0.5 hover:bg-secondary"
          >
            <span className={cn("h-[0.45rem] w-[0.45rem] shrink-0 rounded-full", DOT[tone])} />
            <span className={isError ? "text-err" : "text-muted-foreground"}>{label}</span>
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
                  <span className="shrink-0 font-mono text-[0.68rem] text-muted-foreground/60">{a.id.slice(0, 5)}</span>
                  <span className={cn("ml-auto shrink-0 pl-3", a.status.error ? "text-err" : "text-muted-foreground")}>
                    {a.status.label}
                  </span>
                </button>
                {a.delegations.map((d) => (
                  <button
                    key={d.id}
                    role="menuitem"
                    aria-label={`${d.kind === "a2a" ? "A2A" : "agent"} ${d.label} under ${a.title}`}
                    onClick={() => {
                      setOpen(false);
                      openConversation(a.id);
                    }}
                    className="flex w-full items-center gap-[0.4rem] rounded py-1 pl-6 pr-2 text-left hover:bg-secondary"
                  >
                    <span className={cn("h-[0.35rem] w-[0.35rem] shrink-0 rounded-full", DOT.running)} />
                    <span className="truncate text-muted-foreground">{d.label}</span>
                    <span className="ml-auto shrink-0 rounded-full bg-secondary px-1.5 text-[0.6rem] font-semibold uppercase tracking-wide text-muted-foreground/80">
                      {d.kind === "a2a" ? "A2A" : "agent"}
                    </span>
                  </button>
                ))}
              </Fragment>
            ))}
          </div>
        )}
      </div>
      <span className="flex items-center gap-1 text-muted-foreground">
        {stats.map(([name, n], i) => (
          <Fragment key={name}>
            {i > 0 && <span className="opacity-40">&middot;</span>}
            <span>
              {name}: {n}
            </span>
          </Fragment>
        ))}
      </span>
      <span className="ml-auto inline-flex items-center gap-1">
        <BrowserBridgeDot />
        <button
          aria-label={autoModeDescription}
          aria-pressed={autoMode}
          title={`${autoModeDescription} (Shift+Tab in composer)`}
          onClick={() => setAutoMode(!autoMode)}
          className={cn(
            "inline-flex size-7 items-center justify-center rounded-full",
            autoMode
              ? "bg-primary text-primary-foreground hover:bg-primary-hover"
              : "text-muted-foreground hover:bg-secondary hover:text-foreground",
          )}
        >
          <Zap size={16} aria-hidden="true" />
        </button>
      </span>
    </div>
  );
}
